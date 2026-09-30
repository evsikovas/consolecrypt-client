#!/usr/bin/env python3
"""Bump the client release/build and keep Dart/native version metadata in sync."""
import argparse
import os
from pathlib import Path
import re

VERSION_LINE = re.compile(r"^version: (\d+)\.(\d+)\.(\d+)\+(\d+)[ \t]*$", re.MULTILINE)


def read_version(root):
    pubspec = root / "client/flutter/pubspec.yaml"
    text = pubspec.read_text()
    matches = list(VERSION_LINE.finditer(text))
    if len(matches) != 1:
        raise ValueError(f"Expected one major.minor.patch+build version in {pubspec}")
    return pubspec, text, tuple(map(int, matches[0].groups()))


def prepared_files(root, release, build):
    pubspec, contents, _ = read_version(root)
    metadata = root / "client/flutter/lib/app/app_info.dart"
    dart = metadata.read_text()
    dart, count = re.subn(r"const kAppVersion = '[^']+';", f"const kAppVersion = '{release}';", dart)
    if count != 1:
        raise ValueError(f"Expected one kAppVersion in {metadata}")
    if 'const kAppBuildNumber =' in dart:
        dart, count = re.subn(r"const kAppBuildNumber = \d+;", f"const kAppBuildNumber = {build};", dart)
        if count != 1:
            raise ValueError(f"Invalid kAppBuildNumber in {metadata}")
    else:
        dart = dart.replace(f"const kAppVersion = '{release}';", f"const kAppVersion = '{release}';\nconst kAppBuildNumber = {build};")
    return [(pubspec, VERSION_LINE.sub(f'version: {release}+{build}', contents)), (metadata, dart)]


def bump(root, part='build', source_root=None):
    _, _, current = read_version(root)
    major, minor, patch, build = current
    if part == 'major':
        major, minor, patch = major + 1, 0, 0
    elif part == 'minor':
        minor, patch = minor + 1, 0
    elif part == 'patch':
        patch += 1
    elif part != 'build':
        raise ValueError('Unknown version part')
    build += 1
    # Native jobs use a project-wide GitLab job ID, so fresh checkouts and
    # retries cannot reuse a build number on different operating systems.
    ci_build = os.environ.get('CC_BUILD_NUMBER')
    if ci_build is not None:
        if part != 'build' or not ci_build.isdecimal() or int(ci_build) <= 0:
            raise ValueError('CC_BUILD_NUMBER must be a positive integer for a native build')
        build = max(build, int(ci_build))
    if build > 2100000000:
        raise ValueError('Android versionCode limit reached')
    release = f'{major}.{minor}.{patch}'
    changes = prepared_files(root, release, build)
    if source_root is not None and source_root.resolve() != root.resolve():
        if part != 'build':
            raise ValueError('A historical source checkout can only reserve a build number')
        _, _, source = read_version(source_root)
        release = '.'.join(map(str, source[:3]))
        changes.extend(prepared_files(source_root, release, build))
    # Validate every target before changing any file. Builds in one Flutter
    # checkout must run sequentially (plugin registrant and outputs are shared).
    for path, contents in changes:
        path.write_text(contents)
    return f'{release}+{build}'


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--root', type=Path, default=Path(__file__).resolve().parents[2])
    parser.add_argument('--source-root', type=Path)
    parser.add_argument('--part', choices=['build', 'patch', 'minor', 'major'], default='build')
    args = parser.parse_args()
    print(bump(args.root, args.part, args.source_root))


if __name__ == '__main__':
    main()
