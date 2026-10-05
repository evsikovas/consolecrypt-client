#!/usr/bin/env python3
"""Export exact Linux package files using directory handles, never links."""

from contextlib import ExitStack
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import re
import secrets
import stat
import subprocess
import sys


SPEC = importlib.util.spec_from_file_location("safe_linux_files", Path(__file__).with_name("retain-linux-receipt.py"))
SAFE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(SAFE)


def read_file(parent, name, limit):
    descriptor = os.open(name, os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK, dir_fd=parent)
    with os.fdopen(descriptor, "rb") as reader:
        info = os.fstat(reader.fileno())
        if not stat.S_ISREG(info.st_mode) or info.st_nlink != 1 or not 0 < info.st_size <= limit:
            raise ValueError("invalid package file")
        contents = reader.read(limit + 1)
    if len(contents) > limit:
        raise ValueError("oversized package file")
    return contents


def export(source, destination):
    job = int(os.environ.get("CC_BUILD_NUMBER") or os.environ["CI_JOB_ID"])
    if job <= 0:
        raise ValueError("invalid build number")
    match = re.search(r"^version: (\d+\.\d+\.\d+)\+\d+\s*$", (destination / "client/flutter/pubspec.yaml").read_text(), re.M)
    if match is None:
        raise ValueError("invalid source version")
    release = match.group(1)
    version = release + "+" + str(job)
    prefix = "ConsoleCrypt-" + version + "-linux-x64"
    with ExitStack() as stack:
        parent = SAFE.directory(stack, source, ("dist", "linux"))
        recorded = read_file(parent, "ConsoleCrypt.version", 128).decode("ascii").strip()
        if recorded != version:
            raise ValueError("unexpected package version")
        manifest = json.loads(read_file(parent, prefix + ".json", 65536))
        if not isinstance(manifest, dict) or set(manifest) != {
            "version", "platform", "deb_version", "rpm_version", "rpm_release", "native_sha256", "packages",
        }:
            raise ValueError("unexpected package manifest")
        expected = {
            "version": version, "platform": "linux-x64", "deb_version": release + "-" + str(job),
            "rpm_version": release, "rpm_release": str(job),
        }
        if any(manifest[key] != value for key, value in expected.items()):
            raise ValueError("inconsistent package manifest")
        native = manifest["native_sha256"]
        if not isinstance(native, dict) or set(native) != {
            "consolecrypt", "lib/libapp.so", "lib/libflutter_linux_gtk.so", "lib/libcc_bridge.so",
        } or any(not isinstance(value, str) or re.fullmatch(r"[a-f0-9]{64}", value) is None for value in native.values()):
            raise ValueError("unexpected native hash manifest")
        names = (prefix + ".deb", prefix + ".rpm")
        if not isinstance(manifest["packages"], dict) or set(manifest["packages"]) != set(names):
            raise ValueError("unexpected package list")
        contents = {}
        for name in names:
            data = read_file(parent, name, 200 * 1024 * 1024)
            details = manifest["packages"][name]
            digest = hashlib.sha256(data).hexdigest()
            if not isinstance(details, dict) or set(details) != {"bytes", "sha256"} or details != {"bytes": len(data), "sha256": digest}:
                raise ValueError("package checksum mismatch")
            contents[name] = data
        sums = "".join(manifest["packages"][name]["sha256"] + "  " + name + "\n" for name in names).encode("ascii")
        if read_file(parent, prefix + ".SHA256SUMS", 4096) != sums:
            raise ValueError("checksum sidecar mismatch")
        commit = subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=destination, text=True).strip()
        if re.fullmatch(r"[a-f0-9]{40}", commit) is None:
            raise ValueError("invalid source identity")
        manifest.update(source_commit=commit, ci_job_id=job)
        contents.update({
            prefix + ".json": (json.dumps(manifest, indent=2) + "\n").encode("utf-8"),
            prefix + ".SHA256SUMS": sums, "ConsoleCrypt.version": (version + "\n").encode("ascii"),
        })
        # All source files are validated before creating any public output.
        target = SAFE.directory(stack, destination, ("dist", "linux"), create=True)
        for name, data in contents.items():
            temporary = ".linux-package-" + secrets.token_hex(12)
            output = os.open(temporary, os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW, 0o600, dir_fd=target)
            try:
                with os.fdopen(output, "wb") as writer:
                    writer.write(data)
                    writer.flush()
                    os.fsync(writer.fileno())
                os.replace(temporary, name, src_dir_fd=target, dst_dir_fd=target)
            finally:
                try:
                    os.unlink(temporary, dir_fd=target)
                except FileNotFoundError:
                    pass


def main():
    try:
        if len(sys.argv) != 3:
            raise ValueError("invalid arguments")
        export(Path(sys.argv[1]), Path(sys.argv[2]))
    except (OSError, ValueError, TypeError, KeyError, subprocess.SubprocessError):
        print("Safe Linux package export failed", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
