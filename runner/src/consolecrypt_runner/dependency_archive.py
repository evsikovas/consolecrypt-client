"""Offline staging of pinned tar dependencies, without publishing or execution.

This internal building block is intentionally not a helper RPC/capability. The
future installer must separately approve fetching and commit a reviewed result.
Never call tar.extract/extractall on untrusted archives.
"""
from contextlib import contextmanager
import gzip
import hashlib
import io
import os
from pathlib import Path
import re
import tarfile
import tempfile
import unicodedata

from .dependencies import dependency_target
from .errors import RunnerError, require
from .protocol import encode
from .snapshot import MAX_BYTES, MAX_FILES, ProjectSnapshot, relative_path

MAX_ARCHIVE_BYTES = 32 * 1024 * 1024
MAX_TAR_BYTES = MAX_BYTES + MAX_FILES * 2 * 1024 + 1024 * 1024
_DEVICE = re.compile(r'(?:con|prn|aux|nul|com[1-9]|lpt[1-9])(?:\..*)?', re.I)


class _TarReader:
    """Bound parser reads *before* tarfile allocates a metadata payload.

    The total tar is already bounded on disk, but a forged PAX/GNU header can
    request an enormous read independently of the actual remaining file size.
    Regular file bodies are consumed in at most 1 MiB pieces as well.
    """
    def __init__(self, source):
        self.source = source

    def read(self, size=-1):
        require(type(size) is int and 0 <= size <= 1024 * 1024,
                'dependency_archive_metadata', 'Dependency archive requests oversized metadata')
        return self.source.read(size)

    def seek(self, offset, whence=0):
        require(type(offset) is int and whence == 0 and 0 <= offset <= MAX_TAR_BYTES,
                'dependency_archive_size', 'Dependency archive seeks outside its byte budget')
        return self.source.seek(offset)

    def tell(self):
        return self.source.tell()


def _portable_path(name):
    relative = relative_path(name)
    require(all(not any(ord(c) < 32 or ord(c) == 127 for c in part)
                and not part.endswith((' ', '.')) and not _DEVICE.fullmatch(part)
                and not any(c in '<>"|?*' for c in part)
                and unicodedata.normalize('NFC', part) == part
                for part in relative.parts),
            'dependency_archive_path', 'Dependency archive has a non-portable path')
    return relative


@contextmanager
def staged_dependency(raw, record, private_parent, *, strip_components=0):
    with _stage(raw, record, private_parent, strip_components=strip_components, pinned=True) as result:
        yield result


@contextmanager
def preview_dependency(raw, selection, private_parent, *, strip_components=0):
    """Compute the first tree pin from an explicitly selected archive SHA.

    Selection deliberately has no tree_digest; the result is review evidence,
    not an approved installation. No supplier identity is inferred from a URL.
    """
    require(isinstance(selection, dict) and set(selection) == {'kind', 'name', 'version', 'source'},
            'dependency_selection', 'Select an exact artifact, name and version')
    record = dict(selection, source=dict(selection['source']), tree_digest='0' * 64)
    with _stage(raw, record, private_parent, strip_components=strip_components, pinned=False) as result:
        yield result


@contextmanager
def _stage(raw, record, private_parent, *, strip_components, pinned):
    """Yield a new private tree + verified receipt; remove it on every exit.

    The caller supplies an already reviewed lock record and an exact archive.
    No network call, target/project write, package installation or code import
    is performed. Stripping one archive wrapper is explicit, never guessed.
    Parent is helper-owned state, never a project directory from wire input.
    """
    require(os.name == 'posix' and os.getuid() != 0, 'unsupported_backend',
            'Dependency staging requires the non-root Unix helper')
    target = dependency_target(record)
    # Pin inputs before filesystem/parser work; a caller-owned mutable dict
    # must not replace the expected tree after the archive was approved.
    record = dict(record, source=dict(record['source']))
    require(type(strip_components) is int and strip_components in (0, 1),
            'dependency_archive_layout', 'Unsupported archive wrapper policy')
    require(type(raw) is bytes and 0 < len(raw) <= MAX_ARCHIVE_BYTES,
            'dependency_archive_size', 'Dependency archive exceeds its compressed byte budget')
    require(hashlib.sha256(raw).hexdigest() == record['source']['sha256'],
            'dependency_archive_digest', 'Dependency archive differs from its pinned SHA-256')
    parent = Path(private_parent)
    require(parent.is_dir() and not parent.is_symlink() and parent.stat().st_uid == os.getuid()
            and parent.stat().st_mode & 0o077 == 0, 'dependency_stage_parent',
            'A private helper-owned staging parent is required')
    with tempfile.TemporaryDirectory(prefix='cc-dependency-', dir=parent) as temporary:
        stage = Path(temporary)
        tree = stage/'tree'
        tree.mkdir(mode=0o700)
        records, entries, names = {}, set(), set()
        file_count = byte_count = member_count = 0
        wrappers = set()

        def claim(name, kind, *, explicit=False):
            path = _portable_path(name)
            folded = name.casefold()
            require(folded not in names or name in records, 'dependency_archive_collision',
                    'Dependency archive has colliding portable paths')
            previous = records.get(name)
            require(previous is None or (kind == 'directory' and previous['type'] == kind and not explicit),
                    'dependency_archive_duplicate', 'Dependency archive repeats or replaces a path')
            if previous is not None:
                return path
            require(len(records) < MAX_FILES * 2, 'dependency_archive_entries',
                    'Dependency archive exceeds its entry budget')
            names.add(folded)
            records[name] = {'path': name, 'type': kind}
            return path

        def directory(name):
            path = claim(name, 'directory')
            output = tree.joinpath(*path.parts)
            output.mkdir(mode=0o700, exist_ok=True)

        try:
            # Bound *all* decompressed tar bytes, including PAX/long-name headers
            # skipped internally by tarfile. A compressed metadata bomb must not
            # bypass the eventual extracted-file quotas.
            with (stage/'archive.tar').open('xb') as expanded:
                source = gzip.GzipFile(fileobj=io.BytesIO(raw)) if raw[:2] == b'\x1f\x8b' else io.BytesIO(raw)
                with source:
                    total = 0
                    while chunk := source.read(min(1024 * 1024, MAX_TAR_BYTES - total + 1)):
                        total += len(chunk)
                        require(total <= MAX_TAR_BYTES, 'dependency_archive_size',
                                'Dependency archive exceeds its expanded tar budget')
                        expanded.write(chunk)
            with (stage/'archive.tar').open('rb') as tar_source, \
                    tarfile.open(fileobj=_TarReader(tar_source), mode='r:') as archive:
                for member in archive:
                    member_count += 1
                    require(member_count <= MAX_FILES * 2, 'dependency_archive_entries',
                            'Dependency archive exceeds its member budget')
                    require(member.type in (tarfile.REGTYPE, tarfile.AREGTYPE, tarfile.DIRTYPE)
                            and not member.issparse(), 'dependency_archive_type',
                            'Dependency archive contains a link, sparse or special file')
                    original = member.name[:-1] if member.name.endswith('/') and member.isdir() else member.name
                    parts = _portable_path(original).parts
                    if strip_components:
                        wrappers.add(parts[0])
                        require(len(wrappers) == 1, 'dependency_archive_layout',
                                'Archive must have exactly one explicit wrapper directory')
                        if len(parts) == 1:
                            require(member.isdir(), 'dependency_archive_layout',
                                    'Cannot strip an archive root file')
                            require(original not in entries, 'dependency_archive_duplicate',
                                    'Dependency archive repeats a path')
                            entries.add(original)
                            continue
                        parts = parts[1:]
                    name = '/'.join(parts)
                    require(name not in entries, 'dependency_archive_duplicate',
                            'Dependency archive repeats a path')
                    entries.add(name)
                    for index in range(1, len(parts)):
                        directory('/'.join(parts[:index]))
                    if member.isdir():
                        require(member.size == 0, 'dependency_archive_type', 'Directory has a file payload')
                        # A parent may already have been inferred from a child.
                        previous = records.get(name)
                        require(previous is None or previous['type'] == 'directory',
                                'dependency_archive_duplicate', 'Directory replaces a file')
                        directory(name)
                        continue
                    path = claim(name, 'file', explicit=True)
                    file_count += 1
                    require(file_count <= MAX_FILES and 0 <= member.size <= MAX_BYTES - byte_count,
                            'dependency_archive_size', 'Dependency archive exceeds its file or byte budget')
                    mode = 0o700 if member.mode & 0o111 else 0o600
                    digest, size = hashlib.sha256(), 0
                    stream = archive.extractfile(member)
                    require(stream is not None, 'dependency_archive_type', 'Dependency file payload is missing')
                    fd = os.open(tree.joinpath(*path.parts), os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW, mode)
                    with stream, os.fdopen(fd, 'wb') as output:
                        while chunk := stream.read(min(1024 * 1024, member.size - size + 1)):
                            size += len(chunk)
                            require(size <= member.size, 'dependency_archive_size', 'Invalid dependency file size')
                            digest.update(chunk)
                            output.write(chunk)
                    require(size == member.size, 'dependency_archive_size', 'Truncated dependency file')
                    byte_count += size
                    records[name].update(mode=mode, bytes=size, sha256=digest.hexdigest())
                # Do not silently accept a second archive or a non-tar payload
                # hidden after the first end marker. Normal zero padding is OK.
                archive.fileobj.seek(archive.offset)
                while trailing := archive.fileobj.read(1024 * 1024):
                    require(not any(trailing), 'dependency_archive_trailing',
                            'Dependency archive has data after its end marker')
        except (OSError, EOFError, tarfile.TarError, ValueError) as error:
            raise RunnerError('dependency_archive_invalid', 'Cannot read the pinned dependency archive') from error
        ordered = tuple(records[name] for name in sorted(records))
        snapshot = ProjectSnapshot(hashlib.sha256(encode({'version': 1, 'entries': ordered})).hexdigest(), ordered, byte_count)
        require(not pinned or snapshot.digest == record['tree_digest'], 'dependency_archive_tree',
                'Extracted dependency differs from its pinned tree digest')
        files = {entry['path'] for entry in snapshot.files}
        required = ('tasks/main.yml', 'tasks/main.yaml') if record['kind'] == 'role' else ('galaxy.yml', 'MANIFEST.json')
        require(any(name in files for name in required), 'dependency_layout',
                'Dependency archive has no supported role or collection entry point')
        receipt = {'version': 1, 'target': target, 'source': dict(record['source']),
                   'tree_digest': snapshot.digest, 'strip_components': strip_components,
                   'files': file_count, 'bytes': byte_count}
        yield tree, snapshot, receipt
