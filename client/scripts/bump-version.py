#!/usr/bin/env python3
"""Bump the client release/build and keep Dart/native version metadata in sync."""
import argparse
import os
from pathlib import Path
import re

VERSION = r"(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)(?:-(dev\.[1-9][0-9]*))?\+([1-9][0-9]*)"
VERSION_LINE = re.compile(r"^version: " + VERSION + r"[ \t]*$", re.MULTILINE)


def parse_version(value):
    match = re.fullmatch(VERSION, value)
    if match is None:
        raise ValueError('Expected major.minor.patch[-dev.revision]+positive_build')
    major, minor, patch, prerelease, build = match.groups()
    return int(major), int(minor), int(patch), prerelease, int(build)


def native_version(value):
    """Apple/Windows numeric metadata; Dart About retains the dev prerelease."""
    major, minor, patch, _, build = parse_version(value)
    return f'{major}.{minor}.{patch}+{build}'


def read_version(root):
    pubspec = root / "client/flutter/pubspec.yaml"
    text = pubspec.read_text()
    matches = list(VERSION_LINE.finditer(text))
    if len(matches) != 1:
        raise ValueError(f"Expected one major.minor.patch[-dev.revision]+build version in {pubspec}")
    major, minor, patch, _, build = matches[0].groups()
    # Keep the existing numeric API for callers; prerelease is read separately.
    return pubspec, text, tuple(map(int, (major, minor, patch, build)))


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
    prerelease = VERSION_LINE.search(read_version(root)[1]).group(4)
    if part == 'major':
        major, minor, patch = major + 1, 0, 0
    elif part == 'minor':
        minor, patch = minor + 1, 0
    elif part == 'patch':
        patch += 1
    elif part == 'dev':
        if prerelease is None:
            raise ValueError('A dev revision requires an existing dev prerelease')
        prerelease = f'dev.{int(prerelease.split(".")[1]) + 1}'
    elif part != 'build':
        raise ValueError('Unknown version part')
    build += 1
    # Native jobs use a project-wide GitLab job ID, so fresh checkouts and
    # retries cannot reuse a build number on different operating systems.
    ci_build = os.environ.get('CC_BUILD_NUMBER')
    if ci_build is not None:
        if part != 'build' or not ci_build.isdecimal() or int(ci_build) <= 0:
            raise ValueError('CC_BUILD_NUMBER must be a positive integer for a native build')
        # Different fresh CI checkouts must never collapse older job IDs to
        # the same source-floor+1 counter on different platforms.
        if int(ci_build) < build:
            raise ValueError('CC_BUILD_NUMBER must exceed the source build floor')
        build = int(ci_build)
    if build > 2100000000:
        raise ValueError('Android versionCode limit reached')
    release = f'{major}.{minor}.{patch}'
    if prerelease is not None:
        release += f'-{prerelease}'
    changes = prepared_files(root, release, build)
    if source_root is not None and source_root.resolve() != root.resolve():
        if part != 'build':
            raise ValueError('A historical source checkout can only reserve a build number')
        _, _, source = read_version(source_root)
        release = '.'.join(map(str, source[:3]))
        source_prerelease = VERSION_LINE.search(read_version(source_root)[1]).group(4)
        if (prerelease is None) != (source_prerelease is None):
            raise ValueError('Do not reserve versions across stable and dev checkouts')
        if source_prerelease is not None:
            release += f'-{source_prerelease}'
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
    parser.add_argument('--part', choices=['build', 'dev', 'patch', 'minor', 'major'], default='build')
    parser.add_argument('--native-version', help='Print numeric native metadata without reserving a build')
    args = parser.parse_args()
    print(native_version(args.native_version) if args.native_version else bump(args.root, args.part, args.source_root))


if __name__ == '__main__':
    main()
