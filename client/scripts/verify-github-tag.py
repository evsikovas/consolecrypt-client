#!/usr/bin/env python3
"""Gate native release jobs before a persistent runner receives the checkout."""
import argparse
import os
from pathlib import Path
import re
import subprocess

SEMVER = r'(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)'


def release_version(ref, pubspec):
    match = re.fullmatch(r'refs/tags/v(' + SEMVER + r')', ref)
    if not match:
        raise ValueError('Native packages require an exact stable vX.Y.Z tag')
    version = match[1]
    source = re.search(r'^version: (\S+)\s*$', pubspec, re.M)
    if not source or not re.fullmatch(re.escape(version) + r'\+[1-9]\d*', source[1]):
        raise ValueError('Tag and pubspec release version differ')
    return version


def build_version(ref, pubspec, event, allow_main_candidate=False):
    candidate = allow_main_candidate and event == 'workflow_dispatch' and ref == 'refs/heads/main'
    if event == 'workflow_dispatch' and not candidate:
        raise ValueError('Manual candidates require the current main branch')
    if event not in ('push', 'workflow_dispatch'):
        raise ValueError('Unsupported native build event')
    if candidate:
        source = re.search(r'^version: (' + SEMVER + r')\+[1-9]\d*\s*$', pubspec, re.M)
        if source is None:
            raise ValueError('Invalid candidate version')
        version = source[1]
    else:
        version = release_version(ref, pubspec)
    return version, candidate


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--root', type=Path, default=Path(__file__).resolve().parents[2])
    parser.add_argument('--allow-main-candidate', action='store_true')
    args = parser.parse_args()
    ref = os.environ.get('GITHUB_REF', '')
    version, candidate = build_version(ref, (args.root / 'client/flutter/pubspec.yaml').read_text(),
                                       os.environ.get('GITHUB_EVENT_NAME', ''), args.allow_main_candidate)
    # Checkout fetches origin/main explicitly. Never publish an unrelated tag
    # merely because its version string happens to match the source manifest.
    subprocess.run(['git', 'merge-base', '--is-ancestor', 'HEAD', 'origin/main'], cwd=args.root, check=True)
    tag_commit = subprocess.check_output(['git', 'rev-parse', ('origin/main' if candidate else ref) + '^{commit}'], cwd=args.root, text=True).strip()
    head = subprocess.check_output(['git', 'rev-parse', 'HEAD'], cwd=args.root, text=True).strip()
    if tag_commit != head:
        raise ValueError('Checkout is not the tagged commit')
    if output := os.environ.get('GITHUB_OUTPUT'):
        with open(output, 'a', encoding='utf-8') as stream:
            stream.write(f'version={version}\n')
    print(f'Validated v{version} at {head}')


if __name__ == '__main__':
    main()
