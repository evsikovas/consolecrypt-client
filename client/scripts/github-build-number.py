#!/usr/bin/env python3
"""Reserve a GitHub native build identity without relying on reset GitLab IDs."""
import argparse
import os
from pathlib import Path

PLATFORMS = {'macos': 1, 'windows': 2, 'linux': 3, 'android': 4, 'ios': 5}
BASE = 10000  # Above every published GitLab artifact (last release: build 1393).


def build_number(run, attempt, platform):
    if not run.isascii() or not run.isdecimal() or int(run) < 1:
        raise ValueError('GITHUB_RUN_NUMBER must be a positive ASCII integer')
    if not attempt.isascii() or not attempt.isdecimal() or not 1 <= int(attempt) <= 99:
        raise ValueError('GITHUB_RUN_ATTEMPT must be between 1 and 99')
    if platform not in PLATFORMS:
        raise ValueError('Unknown native platform')
    number = BASE + int(run) * 1000 + int(attempt) * 10 + PLATFORMS[platform]
    if number > 2100000000:
        raise ValueError('Android versionCode limit reached')
    return number


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('platform', choices=PLATFORMS)
    parser.add_argument('--print-only', action='store_true')
    args = parser.parse_args()
    number = build_number(os.environ['GITHUB_RUN_NUMBER'], os.environ['GITHUB_RUN_ATTEMPT'], args.platform)
    if args.print_only:
        print(number)
        return
    with Path(os.environ['GITHUB_ENV']).open('a', encoding='utf-8') as output:
        output.write(f'CC_BUILD_NUMBER={number}\n')
    print(f'Reserved {args.platform} build {number}')


if __name__ == '__main__':
    main()
