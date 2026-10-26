#!/usr/bin/env python3
"""Check a published client release before the docs and the website point at it.

Reads the GitHub release and its SHA256SUMS file and checks: a published
stable vX.Y.Z, exactly one of each installer, uploaded assets on this
repository's release path, one build number per platform, sizes within the
update mirror limit, and GitHub digests equal to the publisher checksums.
Known signed feeds (stable.json, legacy-stable.json, linux-stable.json) may be
present; anything else is refused. --download also streams every installer
and compares its SHA-256. Prints the verified downloads as JSON for
update-release-docs.py.

    verify-client-release.py --tag v0.9.3 > verified-downloads.json
    verify-client-release.py --tag v0.9.3 --download > verified-downloads.json
    verify-client-release.py --release-json release.json --checksums SHA256SUMS-0.9.3.txt
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import sys
import urllib.request
from urllib.parse import quote, unquote, urlsplit

REPOSITORY = "evsikovas/consolecrypt-client"
LIMIT = 500 * 1024 * 1024  # The update mirror refuses larger installers.
# Installer suffix → (platform, package). iOS is not built for releases.
PACKAGES = {
    "macos-universal.dmg": ("macos", "dmg"),
    "windows-x64-setup.exe": ("windows", "exe"),
    "windows-x64.zip": ("windows", "zip"),
    "linux-x64.deb": ("linux", "deb"),
    "linux-x64.rpm": ("linux", "rpm"),
    "android-arm64.apk": ("android", "apk"),
}
FEEDS = {"stable.json", "legacy-stable.json", "linux-stable.json"}


class ReleaseError(ValueError):
    pass


def parse_checksums(text: str, version: str) -> dict[str, str]:
    checksums: dict[str, str] = {}
    for line in text.strip().splitlines():
        match = re.fullmatch(r"([a-f0-9]{64}) [ *](ConsoleCrypt-[A-Za-z0-9.+_-]+)", line)
        if not match or match[2] in checksums or not match[2].startswith(f"ConsoleCrypt-{version}+"):
            raise ReleaseError(f"Malformed, foreign or duplicate checksum line: {line[:100]}")
        checksums[match[2]] = match[1]
    return checksums


def validate(release: dict, checksum_text: str) -> dict:
    tag = release.get("tag_name", "")
    if not re.fullmatch(r"v(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)", tag):
        raise ReleaseError(f"Not a stable release tag: {tag!r}")
    version = tag[1:]
    if release.get("draft") is not False or release.get("prerelease") is not False or not release.get("published_at"):
        raise ReleaseError(f"{tag} is not a published stable release")
    release_url = f"https://github.com/{REPOSITORY}/releases/tag/{tag}"
    if release.get("html_url") != release_url:
        raise ReleaseError("Unexpected release repository or URL")
    checksums = parse_checksums(checksum_text, version)
    sums_name = f"SHA256SUMS-{version}.txt"
    assets = release.get("assets", [])
    names = [asset.get("name") for asset in assets]
    if len(set(names)) != len(names) or sums_name not in names:
        raise ReleaseError("Duplicate assets or no checksum file")
    installers, builds, seen = [], {}, set()
    for asset in assets:
        name = asset["name"]
        if asset.get("state") != "uploaded":
            raise ReleaseError(f"{name} is not fully uploaded")
        url = urlsplit(asset.get("browser_download_url", ""))
        if (url.scheme != "https" or url.netloc != "github.com" or url.query or url.fragment
                or unquote(url.path) != f"/{REPOSITORY}/releases/download/{tag}/{name}"):
            raise ReleaseError(f"Unexpected download URL for {name}")
        if name in FEEDS or name == sums_name:
            if not isinstance(asset.get("size"), int) or not 0 < asset["size"] <= 65536:
                raise ReleaseError(f"Unexpected size of {name}")
            continue
        match = re.fullmatch(rf"ConsoleCrypt-{re.escape(version)}\+([1-9][0-9]*)-(.+)", name)
        if not match or match[2] not in PACKAGES or match[2] in seen:
            raise ReleaseError(f"Unexpected or repeated asset: {name}")
        seen.add(match[2])
        platform, package = PACKAGES[match[2]]
        build = int(match[1])
        if builds.setdefault(platform, build) != build:
            raise ReleaseError(f"{platform} packages have different build numbers")
        if not isinstance(asset.get("size"), int) or not 0 < asset["size"] <= LIMIT:
            raise ReleaseError(f"{name} is empty or exceeds the update mirror limit")
        digest = checksums.get(name)
        if not digest or asset.get("digest") != "sha256:" + digest:
            raise ReleaseError(f"GitHub digest and publisher checksum of {name} differ or are missing")
        installers.append({
            "platform": platform, "package": package, "suffix": match[2], "build": build,
            "name": name, "size": asset["size"], "sha256": digest,
            "url": f"https://github.com/{REPOSITORY}/releases/download/{tag}/" + quote(name, safe="-._"),
        })
    if seen != set(PACKAGES) or set(checksums) != {item["name"] for item in installers}:
        raise ReleaseError("The installer or checksum set is incomplete or has extra files")
    installers.sort(key=lambda item: list(PACKAGES).index(item["suffix"]))
    return {
        "release": tag, "version": version, "release_url": release_url,
        "published_at": release["published_at"], "checksums_url": f"https://github.com/{REPOSITORY}/releases/download/{tag}/{sums_name}",
        "builds": builds, "installers": installers,
        "feeds": sorted(set(names) & FEEDS), "binaries_verified": False,
    }


def fetch(url: str, accept: str = "application/octet-stream"):
    request = urllib.request.Request(url, headers={"Accept": accept, "User-Agent": "ConsoleCrypt-release-check"})
    if url.startswith("https://api.github.com/") and (token := os.environ.get("GH_TOKEN") or os.environ.get("GITHUB_TOKEN")):
        request.add_header("Authorization", f"Bearer {token}")
    return urllib.request.urlopen(request, timeout=120)


def download_and_hash(item: dict) -> None:
    digest, size = hashlib.sha256(), 0
    with fetch(item["url"]) as response:
        while chunk := response.read(1 << 20):
            size += len(chunk)
            if size > LIMIT:
                raise ReleaseError(f"{item['name']} exceeds the update mirror limit")
            digest.update(chunk)
    if size != item["size"] or digest.hexdigest() != item["sha256"]:
        raise ReleaseError(f"Downloaded {item['name']} differs from the published checksum")


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--tag", help="release tag, read from GitHub")
    parser.add_argument("--release-json", type=Path, help="saved GitHub release JSON instead of --tag")
    parser.add_argument("--checksums", type=Path, help="saved SHA256SUMS file (with --release-json)")
    parser.add_argument("--download", action="store_true", help="also download every installer and hash it")
    args = parser.parse_args()
    if args.release_json:
        if not args.checksums:
            parser.error("--release-json needs --checksums")
        release, sums = json.loads(args.release_json.read_text()), args.checksums.read_text()
    elif args.tag:
        with fetch(f"https://api.github.com/repos/{REPOSITORY}/releases/tags/{quote(args.tag)}", "application/vnd.github+json") as r:
            release = json.load(r)
        asset = next((a for a in release.get("assets", []) if a.get("name") == f"SHA256SUMS-{args.tag[1:]}.txt"), None)
        if asset is None:
            sys.exit(f"{args.tag} has no SHA256SUMS file")
        with fetch(asset["browser_download_url"]) as r:
            sums = r.read(65537).decode()
    else:
        parser.error("give --tag or --release-json")
    try:
        verified = validate(release, sums)
        if args.download:
            for item in verified["installers"]:
                download_and_hash(item)
                print(f"verified {item['name']}", file=sys.stderr)
            verified["binaries_verified"] = True
    except ReleaseError as error:
        sys.exit(f"Release check failed: {error}")
    print(json.dumps(verified, indent=2))


if __name__ == "__main__":
    main()
