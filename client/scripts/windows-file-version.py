#!/usr/bin/env python3
"""Map a full build identity to the four WORDs in a Windows PE version."""
import argparse
import re


def file_version(value):
    match = re.fullmatch(r'(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\+([1-9][0-9]*)', value)
    if match is None:
        raise ValueError('Expected major.minor.patch+positive_build')
    major, minor, patch, build = map(int, match.groups())
    if max(major, minor, patch) > 65535:
        raise ValueError('Windows release components must fit in 16 bits')
    if build > 2100000000:
        raise ValueError('Android versionCode limit reached')
    # CI identities keep increasing beyond 65535. Only this numeric PE field
    # wraps; AppVersion, textual PE versions and updater identities stay full.
    # Runner.rc uses the same mask. Never use this field to compare releases.
    return f'{major}.{minor}.{patch}.{build & 0xffff}'


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('version')
    args = parser.parse_args()
    try:
        print(file_version(args.version))
    except ValueError as error:
        parser.error(str(error))


if __name__ == '__main__':
    main()
