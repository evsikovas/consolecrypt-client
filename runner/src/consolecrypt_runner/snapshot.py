"""Immutable bounded project trees. No tar extraction, symlinks or hardlinks."""
from __future__ import annotations

from dataclasses import dataclass
import hashlib
import os
from pathlib import Path, PurePosixPath
import stat

from .errors import RunnerError, require
from .protocol import encode

MAX_FILES = 10_000
MAX_BYTES = 100 * 1024 * 1024
MAX_PATH_BYTES = 1024
MAX_DEPTH = 32


def relative_path(value: str) -> PurePosixPath:
    require(isinstance(value, str) and 0 < len(value.encode("utf-8")) <= MAX_PATH_BYTES,
            "project_path", "Invalid project path length")
    require(not any(char in value for char in ("\\", ":", "\x00", "\r", "\n")),
            "project_path", "Non-portable project path")
    parts = value.split("/")
    require(len(parts) <= MAX_DEPTH and all(part not in ("", ".", "..") for part in parts),
            "project_path", "Project path escapes its root")
    require(all(len(part.encode("utf-8")) <= 255 for part in parts), "project_path", "Project filename exceeds limit")
    require(parts[0] != ".git", "project_path", "Git metadata is not part of a project snapshot")
    return PurePosixPath(value)


@dataclass(frozen=True)
class ProjectSnapshot:
    digest: str
    entries: tuple[dict, ...]
    bytes: int

    @property
    def files(self):
        return tuple(entry for entry in self.entries if entry["type"] == "file")

    def to_json(self):
        return {"version": 1, "digest": self.digest, "entries": list(self.entries), "bytes": self.bytes}


def create_snapshot(source: Path, destination: Path, *, max_files=MAX_FILES, max_bytes=MAX_BYTES) -> ProjectSnapshot:
    """Capture source without following ancestors supplied by project entries.

    On Unix walk directory FDs, not joined paths: swapping a checked directory
    with a symlink cannot redirect a later file open outside the selected root.
    Destination is a new private directory owned by the helper, never the source.
    """
    require(os.name == "posix", "unsupported_backend", "Snapshots execute in a Unix helper")
    source = Path(source).absolute()
    destination = Path(destination).absolute()
    require(not source.is_symlink(), "project_link", "Project root must not be a symlink")
    require(not destination.exists(), "snapshot_exists", "Snapshot destination already exists")
    require(not destination.resolve().is_relative_to(source.resolve()), "snapshot_destination", "Snapshot must be outside project")
    destination.mkdir(mode=0o700)
    records = []
    byte_count = 0
    entry_count = 0
    file_count = 0
    flags = os.O_RDONLY | os.O_CLOEXEC | os.O_NOFOLLOW | os.O_NONBLOCK
    root_fd = os.open(source, flags | os.O_DIRECTORY)

    def visit(directory_fd: int, prefix: str):
        nonlocal byte_count, entry_count, file_count
        before = os.fstat(directory_fd)
        with os.scandir(directory_fd) as scan:
            entries = []
            for entry in scan:
                require(entry_count < max_files * 2, "project_entries", "Project directory count exceeds limit")
                entry_count += 1
                entries.append(entry)
            entries.sort(key=lambda item: item.name)
        for entry in entries:
            if entry.name == ".git":
                continue
            name = f"{prefix}/{entry.name}" if prefix else entry.name
            path = relative_path(name)
            info = entry.stat(follow_symlinks=False)
            require(not stat.S_ISLNK(info.st_mode), "project_link", "Project contains a symlink")
            target = destination.joinpath(*path.parts)
            if stat.S_ISDIR(info.st_mode):
                target.mkdir(mode=0o700)
                records.append({"path": name, "type": "directory"})
                child_fd = os.open(entry.name, flags | os.O_DIRECTORY, dir_fd=directory_fd)
                try:
                    require(os.fstat(child_fd).st_ino == info.st_ino, "project_changed", "Project changed while capturing")
                    visit(child_fd, name)
                finally:
                    os.close(child_fd)
                continue
            require(stat.S_ISREG(info.st_mode) and info.st_nlink == 1,
                    "project_file_type", "Project contains a special file or hardlink")
            require(file_count < max_files, "project_files", "Project file count exceeds limit")
            file_count += 1
            require(info.st_size <= max_bytes - byte_count, "project_bytes", "Project exceeds snapshot byte limit")
            fd = os.open(entry.name, flags, dir_fd=directory_fd)
            try:
                opened = os.fstat(fd)
                require((opened.st_dev, opened.st_ino) == (info.st_dev, info.st_ino) and opened.st_nlink == 1,
                        "project_changed", "Project changed while capturing")
                digest = hashlib.sha256()
                size = 0
                mode = 0o700 if opened.st_mode & 0o111 else 0o600
                output_fd = os.open(target, os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW, mode)
                with os.fdopen(output_fd, "wb") as output:
                    while chunk := os.read(fd, min(1024 * 1024, max_bytes - byte_count + 1)):
                        size += len(chunk)
                        byte_count += len(chunk)
                        require(byte_count <= max_bytes, "project_bytes", "Project exceeds snapshot byte limit")
                        digest.update(chunk)
                        output.write(chunk)
                    output.flush()
                    os.fsync(output.fileno())
                after = os.fstat(fd)
                require((opened.st_size, opened.st_mtime_ns, opened.st_ctime_ns, opened.st_nlink) ==
                        (after.st_size, after.st_mtime_ns, after.st_ctime_ns, after.st_nlink) and size == opened.st_size,
                        "project_changed", "Project changed while capturing")
                records.append({"path": name, "type": "file", "mode": mode, "bytes": size, "sha256": digest.hexdigest()})
            finally:
                os.close(fd)
        after = os.fstat(directory_fd)
        require((before.st_mtime_ns, before.st_ctime_ns) == (after.st_mtime_ns, after.st_ctime_ns),
                "project_changed", "Project changed while capturing")

    try:
        visit(root_fd, "")
    except OSError as error:
        raise RunnerError("project_io", "Cannot capture project files") from error
    finally:
        os.close(root_fd)
    records.sort(key=lambda record: record["path"])
    digest = hashlib.sha256(encode({"version": 1, "entries": records})).hexdigest()
    return ProjectSnapshot(digest=digest, entries=tuple(records), bytes=byte_count)


def verify_snapshot(root: Path, snapshot: ProjectSnapshot) -> None:
    """Re-capture into a private sibling before use, detecting edits since approval."""
    import tempfile
    import shutil
    temporary = Path(tempfile.mkdtemp(prefix="cc-verify-", dir=root.parent))
    try:
        captured = create_snapshot(root, temporary / "tree")
        require(captured.digest == snapshot.digest, "revision_changed", "Snapshot changed since approval")
    finally:
        shutil.rmtree(temporary)
