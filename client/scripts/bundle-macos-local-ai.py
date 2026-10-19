#!/usr/bin/env python3
"""Bundle pinned llama.cpp helpers before signing the macOS app.

App Sandbox cannot execute newly downloaded/quarantined code in its container.
Both architectures therefore ship as nested code; only GGUF models download at
runtime. Archives are size/SHA-256 checked before inspecting any member.
"""

import argparse
import hashlib
import json
from pathlib import Path
import re
import shutil
import sys
import tarfile
import tempfile
import urllib.parse
import urllib.request

VERSION = "b11739"
COMMIT = "0ce8cd11416c8840498e6701bcbe39ad4b858c2b"
RUNTIMES = {
    "macArm64": {
        "filename": "llama-b11739-bin-macos-arm64.tar.gz",
        "sha256": "a2073ed1aa00904d408a6694429aa9b6dcf4949c2683206c403930849ce222ac",
        "bytes": 12324618,
    },
    "macX64": {
        "filename": "llama-b11739-bin-macos-x64.tar.gz",
        "sha256": "f439aa26fa4d60ff60053b06ed901efe75865cd381063d1e65a0407a0d2761e6",
        "bytes": 11799331,
    },
}
DOWNLOAD_ROOT = f"https://github.com/ggml-org/llama.cpp/releases/download/{VERSION}/"
NOTICES = Path(__file__).with_name("local-ai-third-party-notices.txt")


def verify(path, artifact):
    if path.is_symlink() or not path.is_file() or path.stat().st_size != artifact["bytes"]:
        return False
    digest = hashlib.sha256()
    with path.open("rb") as source:
        for chunk in iter(lambda: source.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest() == artifact["sha256"]


def trusted_url(url):
    parsed = urllib.parse.urlsplit(url)
    if parsed.scheme != "https" or parsed.username or parsed.password or parsed.hostname not in {
        "github.com", "release-assets.githubusercontent.com", "objects.githubusercontent.com",
    }:
        raise ValueError("Untrusted runtime download/redirect URL")
    return url


class TrustedRedirect(urllib.request.HTTPRedirectHandler):
    def redirect_request(self, request, response, code, message, headers, new_url):
        return super().redirect_request(request, response, code, message, headers, trusted_url(new_url))


def download(cache, artifact):
    cache.mkdir(parents=True, exist_ok=True)
    destination = cache / artifact["filename"]
    if verify(destination, artifact):
        return destination
    opener = urllib.request.build_opener(TrustedRedirect())
    url = trusted_url(DOWNLOAD_ROOT + artifact["filename"])
    with tempfile.NamedTemporaryFile(dir=cache, prefix="runtime-", delete=False) as temporary:
        partial = Path(temporary.name)
        try:
            with opener.open(url, timeout=60) as response:
                trusted_url(response.url)
                total = 0
                while chunk := response.read(1024 * 1024):
                    total += len(chunk)
                    if total > artifact["bytes"]:
                        raise ValueError("Runtime download exceeds pinned size")
                    temporary.write(chunk)
            temporary.flush()
            if not verify(partial, artifact):
                raise ValueError("Runtime archive size/SHA-256 mismatch")
            partial.replace(destination)
        finally:
            partial.unlink(missing_ok=True)
    return destination


def extract(archive_path, destination, artifact, platform):
    """Materialize only server, dylibs and license; never create archive links."""
    if not verify(archive_path, artifact):
        raise ValueError("Runtime archive size/SHA-256 mismatch")
    if destination.exists() or destination.is_symlink():
        raise ValueError("Runtime staging destination already exists")
    prefix = f"llama-{VERSION}/"
    with tarfile.open(archive_path, "r:gz") as archive:
        entries = {}
        total = 0
        for member in archive.getmembers():
            if member.isdir() and member.name.rstrip("/") == prefix.rstrip("/"):
                continue
            if not member.name.startswith(prefix):
                raise ValueError("Unexpected archive root")
            name = member.name[len(prefix):]
            if not re.fullmatch(r"[A-Za-z0-9_-][A-Za-z0-9_.-]*", name) or name in {".", ".."}:
                raise ValueError("Unsafe archive path")
            if name.lower() in {key.lower() for key in entries}:
                raise ValueError("Duplicate archive path")
            if not (member.isfile() or member.issym()):
                raise ValueError("Unexpected archive member type")
            total += member.size
            if member.size < 0 or total > 512 * 1024 * 1024 or len(entries) > 2048:
                raise ValueError("Archive expansion exceeds bound")
            entries[name] = member
        selected = {name for name in entries if name == "llama-server" or name == "LICENSE" or name.endswith(".dylib")}
        if not {"llama-server", "LICENSE", "libllama-server-impl.dylib"} <= selected:
            raise ValueError("Required runtime files missing")
        destination.mkdir(parents=True)
        try:
            def contents(name, seen=None):
                seen = set() if seen is None else seen
                if name not in entries or name in seen or len(seen) > 16:
                    raise ValueError("Invalid archive link")
                seen.add(name)
                member = entries[name]
                if member.issym():
                    if member.linkname not in selected:
                        raise ValueError("Archive link escapes selected runtime")
                    return contents(member.linkname, seen)
                source = archive.extractfile(member)
                if source is None:
                    raise ValueError("Unreadable runtime member")
                with source:
                    data = source.read(member.size + 1)
                if len(data) != member.size:
                    raise ValueError("Runtime member size mismatch")
                return data

            for name in sorted(selected):
                target = destination / name
                target.write_bytes(contents(name))
                target.chmod(0o755 if name == "llama-server" else 0o644)
            (destination / "runtime.json").write_text(json.dumps({
                "version": VERSION, "platform": platform, "archiveSha256": artifact["sha256"],
                "sourceCommit": COMMIT,
            }, indent=2) + "\n")
        except Exception:
            shutil.rmtree(destination)
            raise


def bundle(app, cache):
    if app.suffix != ".app" or app.is_symlink() or not (app / "Contents/Info.plist").is_file():
        raise ValueError("--app must be a built application bundle")
    if not NOTICES.is_file():
        raise ValueError("Local AI third-party notices are missing")
    helpers = app / "Contents/Helpers"
    helpers.mkdir(exist_ok=True)
    target = helpers / "LocalAI"
    if target.is_symlink():
        raise ValueError("Refusing symlinked LocalAI directory")
    resources = app / "Contents/Resources"
    resources.mkdir(exist_ok=True)
    resource_target = resources / "LocalAI"
    if resource_target.is_symlink():
        raise ValueError("Refusing symlinked LocalAI resources")
    # Keep any previous valid helper set until both new archives are verified.
    with tempfile.TemporaryDirectory(prefix=".local-ai-", dir=helpers) as temporary:
        staging = Path(temporary)
        code = staging / "code"
        data = staging / "resources"
        code.mkdir()
        data.mkdir()
        for platform, artifact in RUNTIMES.items():
            runtime = code / platform
            extract(download(cache, artifact), runtime, artifact, platform)
            metadata = data / platform
            metadata.mkdir()
            # Helpers is a nested-code location: unsigned data files there
            # break codesign --deep. Resource seals cover these separately.
            for name in ["runtime.json", "LICENSE"]:
                (runtime / name).rename(metadata / name)
        shutil.copyfile(NOTICES, data / "THIRD_PARTY_NOTICES.txt")
        if target.exists():
            shutil.rmtree(target)
        if resource_target.exists():
            shutil.rmtree(resource_target)
        code.rename(target)
        data.rename(resource_target)
    shutil.copyfile(NOTICES, resources / "LOCAL-AI-THIRD-PARTY-NOTICES.txt")
    print(f"Bundled llama.cpp {VERSION}: macArm64 + macX64 (verified archives)")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--app", type=Path, required=True)
    parser.add_argument("--cache", type=Path, default=Path.home() / ".cache/consolecrypt-local-ai-build")
    args = parser.parse_args()
    try:
        bundle(args.app.absolute(), args.cache.absolute())
    except (ValueError, OSError, tarfile.TarError) as error:
        print(f"Local AI bundle failed: {error}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
