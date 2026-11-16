"""Explicit opt-in download of an official pinned vmagent binary, never install.

The lock and this module are trusted application code. Digests supplied by an
untrusted manifest are never accepted. Upstream checksum publication via HTTPS
does not supply an independently pinned release signing authority.
"""
from __future__ import annotations

import argparse
import hashlib
import io
import json
import os
from pathlib import Path
import stat
import tarfile
import urllib.request
import zipfile

ROOT = Path(__file__).resolve().parent
MAX_ARCHIVE_BYTES = 192 * 1024 * 1024
MAX_BINARY_BYTES = 80 * 1024 * 1024


def check_path(path: Path) -> Path:
    """Reject pre-existing links/reparse points in every path component.

    These helpers require an owner-controlled directory. They do not claim to
    defeat a privileged attacker concurrently replacing directory components.
    Checking only the leaf misses Windows junctions and linked parents.
    """
    path = Path(path)
    if ".." in path.parts:
        raise ValueError("parent traversal denied")
    absolute = Path(os.path.abspath(path))
    for current in (*reversed(absolute.parents), absolute):
        try:
            info = current.lstat()
        except FileNotFoundError:
            continue
        if stat.S_ISLNK(info.st_mode) or getattr(info, "st_file_attributes", 0) & getattr(stat, "FILE_ATTRIBUTE_REPARSE_POINT", 0x400):
            raise ValueError("linked or reparse path denied")
    return absolute


def _existing_regular(path: Path, limit: int) -> Path:
    path = check_path(path)
    info = path.stat()
    if not stat.S_ISREG(info.st_mode) or info.st_size > limit:
        raise ValueError("invalid supply file")
    return path


def lock() -> dict:
    return json.loads((ROOT / "supply-lock.json").read_text(encoding="utf-8"))


def verify_binary(binary: Path, platform: str) -> Path:
    entry = lock()["collector"]["artifacts"].get(platform)
    if entry is None:
        raise ValueError("unsupported collector platform")
    binary = _existing_regular(binary, MAX_BINARY_BYTES)
    with binary.open("rb") as source:
        actual = hashlib.file_digest(source, "sha256").hexdigest()
    if actual != entry["binary_sha256"]:
        raise ValueError("collector binary digest mismatch")
    return binary.resolve()


def _get(url: str, limit: int) -> bytes:
    # Explicit command only. No service startup performs this download.
    request = urllib.request.Request(url, headers={"User-Agent": "ConsoleCrypt-Agent-Prototype/0.1"})
    with urllib.request.urlopen(request, timeout=30) as response:
        if not response.geturl().startswith("https://"):
            raise ValueError("insecure supply redirect")
        result = response.read(limit + 1)
    if len(result) > limit:
        raise ValueError("supply size limit exceeded")
    return result


def fetch_pinned(platform: str, destination: Path) -> Path:
    metadata = lock()["collector"]
    entry = metadata["artifacts"].get(platform)
    if entry is None:
        raise ValueError("unsupported collector platform")
    destination = check_path(destination)
    destination.mkdir(mode=0o700, parents=True, exist_ok=True)
    check_path(destination)
    binary = check_path(destination / entry["binary"])
    check_path(destination / "LICENSE.vmagent")
    if binary.exists():
        return verify_binary(binary, platform)
    url = "https://github.com/VictoriaMetrics/VictoriaMetrics/releases/download/" + metadata["version"] + "/" + entry["archive"]
    raw = _get(url, MAX_ARCHIVE_BYTES)
    if hashlib.sha256(raw).hexdigest() != entry["archive_sha256"]:
        raise ValueError("collector archive digest mismatch")
    # Read only the named regular binary. Never extract arbitrary archive paths.
    if entry["archive"].endswith(".zip"):
        with zipfile.ZipFile(io.BytesIO(raw)) as archive:
            member = archive.getinfo(entry["binary"])
            if member.file_size > MAX_BINARY_BYTES:
                raise ValueError("collector binary too large")
            payload = archive.read(member)
    else:
        with tarfile.open(fileobj=io.BytesIO(raw), mode="r:gz") as archive:
            member = archive.getmember(entry["binary"])
            if not member.isfile() or member.size > MAX_BINARY_BYTES:
                raise ValueError("invalid collector archive member")
            payload = archive.extractfile(member).read(MAX_BINARY_BYTES + 1)
    if hashlib.sha256(payload).hexdigest() != entry["binary_sha256"]:
        raise ValueError("collector binary digest mismatch")
    # Obtain the full license before publishing the new local binary. Existing
    # paths are never overwritten or followed, including a license symlink.
    license_bytes = _get(metadata["license_url"], 32 * 1024)
    license_path = check_path(destination / "LICENSE.vmagent")
    if license_path.exists():
        _existing_regular(license_path, 32 * 1024)
        if license_path.read_bytes() != license_bytes:
            raise ValueError("existing collector license differs")
    else:
        with license_path.open("xb") as output:
            output.write(license_bytes)
            output.flush()
            os.fsync(output.fileno())
    check_path(binary)
    with binary.open("xb") as output:
        output.write(payload)
        output.flush()
        os.fsync(output.fileno())
    binary.chmod(0o700)
    return verify_binary(binary, platform)


def main() -> None:
    parser = argparse.ArgumentParser(description="Fetch a digest-pinned vmagent; does not install or start it")
    parser.add_argument("--platform", required=True, choices=tuple(lock()["collector"]["artifacts"]))
    parser.add_argument("--destination", type=Path, required=True)
    args = parser.parse_args()
    print(fetch_pinned(args.platform, args.destination))


if __name__ == "__main__":
    main()
