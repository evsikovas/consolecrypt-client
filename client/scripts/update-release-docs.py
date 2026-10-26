#!/usr/bin/env python3
"""Point the docs README and the website at a verified client release.

Takes the JSON printed by verify-client-release.py and rewrites:

- consolecrypt-docs/README.md: the guide version line, the release notes link,
  the "Готовые сборки" heading, tag, checksum link and table (iOS row kept);
- consolecrypt-site download.html, index.html and assets/en.js: the release
  heading, the tag link, every installer URL, version+build labels and the
  hero version.

The previous version is read from the docs README. After rewriting, no file
may still mention it; otherwise nothing is written. --check only reports
what would change (exit 1). Run the site tests (npm test) afterwards; the
site package version and releases/0.1.N.md stay manual.

    update-release-docs.py verified-downloads.json --docs ../consolecrypt-docs --site ../consolecrypt-site
"""

from __future__ import annotations

import argparse
import json
from pathlib import Path
import re
import sys

ROWS = {
    "macos-universal.dmg": ("macOS, Apple Silicon и Intel", "DMG"),
    "windows-x64-setup.exe": ("Windows x64", "EXE"),
    "windows-x64.zip": ("Windows x64", "Portable ZIP"),
    "linux-x64.deb": ("Linux x64", "DEB"),
    "linux-x64.rpm": ("Linux x64", "RPM"),
    "android-arm64.apk": ("Android ARM64", "APK"),
}
SITE_FILES = ["download.html", "index.html", "assets/en.js"]


def spaced(size: int) -> str:
    return f"{size:,}".replace(",", " ")


def old_release(readme: str) -> str:
    match = re.search(r"^## Готовые сборки (\d+\.\d+\.\d+)$", readme, re.M)
    if not match:
        raise SystemExit("docs README has no '## Готовые сборки X.Y.Z' section")
    return match[1]


def notes_name(version: str) -> str:
    return f"RELEASE_{version.replace('.', '_')}.md"


def rewrite_readme(readme: str, verified: dict) -> str:
    old, new, tag = old_release(readme), verified["version"], verified["release"]
    readme = readme.replace(f"описывает клиент {old};", f"описывает клиент {new};")
    link = f"[версия {new}](docs/public/{notes_name(new)})"
    if link not in readme:
        readme = readme.replace("[Обновления](docs/public/UPDATES.md), ", f"[Обновления](docs/public/UPDATES.md), {link}, ", 1)
    rows = "\n".join(
        f"| {ROWS[item['suffix']][0]} | {new}+{item['build']} | [{ROWS[item['suffix']][1]}]({item['url']}) | {spaced(item['size'])} |"
        for item in verified["installers"]
    )
    # The heading, the tag and checksum lines, the table head and every row
    # before the iOS one (an older preview build that stays as it is).
    section = re.compile(r"## Готовые сборки \d+\.\d+\.\d+\n\n[\s\S]*?\n\| --- \| --- \| --- \| ---: \|\n(?:\| (?!iOS)[^\n]*\n)+")
    count = len(verified["installers"])
    words = {6: "Шесть", 7: "Семь", 5: "Пять"}
    header = (
        f"## Готовые сборки {new}\n\n"
        f"{words.get(count, str(count))} файлов выпущены из одного тега [{tag}]({verified['release_url']}).\n"
        f"Перед установкой сверьте файл с [SHA-256 релиза]({verified['checksums_url']}).\n\n"
        "| Платформа | Сборка | Файл | Размер, байт |\n| --- | --- | --- | ---: |\n"
    )
    readme, replaced = section.subn(lambda _: header + rows + "\n", readme, count=1)
    if not replaced:
        raise SystemExit("docs README: the build table has an unexpected layout")
    return readme.replace(f"Выпуск {old} для iOS", f"Выпуск {new} для iOS")


def old_builds(html: str, old: str) -> dict[str, int]:
    builds = {}
    for suffix in ROWS:
        found = set(re.findall(rf"ConsoleCrypt-{re.escape(old)}%2B(\d+)-{re.escape(suffix)}", html))
        if len(found) != 1:
            raise SystemExit(f"download.html: expected one {suffix} link of {old}, found {sorted(found)}")
        builds[suffix] = int(found.pop())
    return builds


def rewrite_site(text: str, verified: dict, old: str, builds: dict[str, int]) -> str:
    new, tag = verified["version"], verified["release"]
    for item in verified["installers"]:
        before = builds[item["suffix"]]
        text = text.replace(f"v{old}/ConsoleCrypt-{old}%2B{before}-{item['suffix']}", f"{tag}/ConsoleCrypt-{new}%2B{item['build']}-{item['suffix']}")
    # Labels and analytics carry version+build per platform (same build within one).
    for item in verified["installers"]:
        text = text.replace(f"{old}+{builds[item['suffix']]}", f"{new}+{item['build']}")
    text = text.replace(f"releases/tag/v{old}", f"releases/tag/{tag}")
    text = re.sub(rf"(?<![\d.]){re.escape('v' + old)}(?![\d.])", tag, text)
    return re.sub(rf"(· ){re.escape(old)}(?![\d.+])", rf"\g<1>{new}", text)


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("verified", type=Path, help="output of verify-client-release.py")
    parser.add_argument("--docs", type=Path, required=True, help="consolecrypt-docs checkout")
    parser.add_argument("--site", type=Path, required=True, help="consolecrypt-site checkout")
    parser.add_argument("--check", action="store_true", help="report the files that would change, write nothing")
    args = parser.parse_args()
    verified = json.loads(args.verified.read_text())
    readme_path = args.docs / "README.md"
    readme = readme_path.read_text()
    old = old_release(readme)
    changed: dict[Path, str] = {}
    if old != verified["version"] or rewrite_readme(readme, verified) != readme:
        changed[readme_path] = rewrite_readme(readme, verified)
    builds = old_builds((args.site / "download.html").read_text(), old)
    for name in SITE_FILES:
        path = args.site / name
        text = path.read_text()
        updated = rewrite_site(text, verified, old, builds) if old != verified["version"] else text
        if updated != text:
            changed[path] = updated
    if old != verified["version"]:
        stale = [
            str(path)
            for path, text in changed.items()
            if re.search(rf"(?<![\d.]){re.escape(old)}(?![\d])", text.replace(f"[версия {old}]", "").replace(notes_name(old), ""))
        ]
        if stale:
            raise SystemExit(f"Still mentions {old} after rewriting (nothing written): {', '.join(stale)}")
    if args.check:
        for path in changed:
            print(f"would update {path}")
        sys.exit(1 if changed else 0)
    for path, text in changed.items():
        path.write_text(text)
        print(f"updated {path}")
    if not changed:
        print(f"docs and site already point at {verified['release']}")


if __name__ == "__main__":
    main()
