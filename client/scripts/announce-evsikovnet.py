#!/usr/bin/env python3
"""Publish a ConsoleCrypt client release on evsikov.net (project ConsoleCrypt).

Runs in the client CI after `publish`, for a stable vX.Y.Z tag. The notes are
the Russian section of the GitHub release (docs/public/RELEASE_X_Y_Z.md);
downloads point at consolecrypt.dev, no files go to evsikov.net. The token is
the evsikov.net CI token in the EVSIKOVNET_TOKEN secret; it can only create
and edit releases. An existing version is updated in place.

    announce-evsikovnet.py --tag v0.9.3 --dry-run   # print the request body
    announce-evsikovnet.py --tag v0.9.3             # CI only: create or update
"""

from __future__ import annotations

import argparse
import datetime as dt
import json
import os
import re
import sys
import urllib.error
import urllib.parse
import urllib.request

REPO = "evsikovas/consolecrypt-client"
API = "https://evsikov.net/api"
PROJECT_SLUG = "consolecrypt"
PROJECT_ID = "3cc5d700-6719-4099-bd0d-1c8d206a9755"
DOCS_BASE = f"https://github.com/{REPO}/blob/main/docs/public/"
DOWNLOADS = [
    ("macos", "Universal, .dmg — на consolecrypt.dev", "macos"),
    ("windows", "Установщик x64, .exe — на consolecrypt.dev", "windows"),
    ("windows", "Portable x64, .zip — на consolecrypt.dev", "windows"),
    ("linux", "x64, .deb — на consolecrypt.dev", "linux"),
    ("linux", "x64, .rpm — на consolecrypt.dev", "linux"),
    ("android", "arm64, .apk — на consolecrypt.dev", "android"),
]


def github_release(tag: str) -> dict:
    request = urllib.request.Request(f"https://api.github.com/repos/{REPO}/releases/tags/{urllib.parse.quote(tag)}")
    request.add_header("Accept", "application/vnd.github+json")
    if token := os.environ.get("GH_TOKEN") or os.environ.get("GITHUB_TOKEN"):
        request.add_header("Authorization", f"Bearer {token}")
    with urllib.request.urlopen(request, timeout=30) as response:
        return json.load(response)


def russian_notes(body: str) -> str:
    """The `## Русский` section with relative links made absolute (they point
    at docs/public of the client repository)."""
    lines = body.splitlines()
    start = next((i for i, line in enumerate(lines) if re.match(r"^##\s+Русский\s*$", line)), None)
    if start is None:
        sys.exit("The release notes have no '## Русский' section")
    end = next((i for i in range(start + 1, len(lines)) if re.match(r"^##\s", lines[i])), len(lines))
    text = "\n".join(lines[start + 1 : end]).strip()

    def absolute(match: re.Match[str]) -> str:
        label, url = match.group(1), match.group(2)
        if re.match(r"^(https?:|mailto:|#)", url):
            return match.group(0)
        return f"[{label}]({urllib.parse.urljoin(DOCS_BASE, url)})"

    return re.sub(r"\[([^\]]+)\]\(([^)\s]+)\)", absolute, text)


def payload(release: dict) -> dict:
    tag = release.get("tag_name", "")
    if release.get("draft") or release.get("prerelease") or not re.fullmatch(r"v\d+\.\d+\.\d+", tag):
        sys.exit(f"{tag or 'release'} is not a published stable release")
    published = dt.datetime.fromisoformat(release["published_at"].replace("Z", "+00:00"))
    return {
        "project_id": PROJECT_ID,
        "version": tag[1:],
        "title": (release.get("name") or f"ConsoleCrypt {tag[1:]}")[:200],
        "notes_md": russian_notes(release.get("body") or "")[:100_000],
        "released_at": published.astimezone(dt.timezone.utc).strftime("%Y-%m-%d"),
        "published": True,
        "downloads": [
            {"platform": platform, "label": label, "url": f"https://consolecrypt.dev/download#{anchor}"}
            for platform, label, anchor in DOWNLOADS
        ],
    }


def call(method: str, path: str, token: str | None = None, body: dict | None = None, opener=None) -> tuple[int, object]:
    request = urllib.request.Request(
        API + path,
        method=method,
        data=None if body is None else json.dumps(body).encode(),
        headers={"Content-Type": "application/json", "Accept": "application/json", "User-Agent": "ConsoleCrypt-CI"},
    )
    if token:
        request.add_header("Authorization", f"Bearer {token}")
    try:
        with (opener or urllib.request.build_opener()).open(request, timeout=30) as response:
            return response.status, json.load(response)
    except urllib.error.HTTPError as error:
        try:
            detail = json.loads(error.read() or b"{}")
        except ValueError:
            detail = {}
        return error.code, detail
    except urllib.error.URLError as error:
        sys.exit(f"evsikov.net is unreachable: {error.reason}")


def publish(body: dict, token: str, opener=None) -> str:
    """Creates the release, or updates the published one of the same version."""
    status, result = call("POST", "/releases", token, body, opener)
    if status == 201:
        return f"created {body['version']} ({result.get('id')})"
    if status != 409:
        sys.exit(f"evsikov.net refused the release ({status}): {result.get('error', 'no details')}")
    status, releases = call("GET", f"/public/releases?project={PROJECT_SLUG}", opener=opener)
    existing = next((r for r in releases if r.get("version") == body["version"]), None) if status == 200 else None
    if existing is None:
        sys.exit(f"{body['version']} exists on evsikov.net but is not published; fix it in the admin panel")
    status, result = call("PUT", f"/releases/{urllib.parse.quote(existing['id'])}", token, body, opener)
    if status != 200:
        sys.exit(f"evsikov.net refused the update ({status}): {result.get('error', 'no details')}")
    return f"updated {body['version']} ({existing['id']})"


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--tag", required=True, help="stable client tag, e.g. v0.9.3")
    parser.add_argument("--dry-run", action="store_true", help="print the request body instead of publishing")
    args = parser.parse_args()
    body = payload(github_release(args.tag))
    if args.dry_run:
        print(json.dumps(body, ensure_ascii=False, indent=2))
        return
    # Owner decision (DOC-04): publication happens only in the release CI.
    if os.environ.get("GITHUB_ACTIONS") != "true" or os.environ.get("GITHUB_REF") != f"refs/tags/{args.tag}":
        sys.exit("Publishing runs only in the client release CI for this tag; use --dry-run locally")
    token = os.environ.pop("EVSIKOVNET_TOKEN", "")
    if not token:
        sys.exit("EVSIKOVNET_TOKEN is not set")
    print(f"evsikov.net: {publish(body, token)}")


if __name__ == "__main__":
    main()
