#!/usr/bin/env python3
"""Export buildable public sources without private repository history or caches.

Only git-visible source files are considered. Never copy .git or ignored files.
The destination must be new; this tool cannot overwrite an existing checkout.
"""
from __future__ import annotations

import argparse
from pathlib import Path, PurePosixPath
import shutil
import subprocess

ROOT_FILES = {
    'README.md', 'SECURITY.md', 'LICENSE-MIT', 'LICENSE-APACHE', 'AUTHORS',
    '.gitignore', '.gitattributes', '.dockerignore', '.gitlab-ci.yml',
    'rust-toolchain.toml',
}
PRIVATE_DIRS = {
    '.git', '.github', '.codex', '.claude', '.agents', '.idea', '.vscode',
    'target', 'build', 'dist', '.dart_tool', '.gradle', '.cxx', 'Pods',
    'ephemeral', '__pycache__', '.dev-mail', 'secrets', '.local',
}
PRIVATE_NAMES = {
    'AGENTS.md', 'CLAUDE.md', '.DS_Store', 'local.properties', 'key.properties',
    '.flutter-plugins', '.flutter-plugins-dependencies', '.env',
}
PRIVATE_SUFFIXES = {
    '.key', '.pem', '.p12', '.pfx', '.jks', '.keystore', '.mobileprovision',
    '.db', '.sqlite', '.sqlite3', '.log', '.pyc', '.apk', '.dmg', '.exe',
    '.zip', '.dylib', '.dll', '.so', '.a', '.o',
}


def publishable(name: str) -> bool:
    path = PurePosixPath(name)
    if path.is_absolute() or '..' in path.parts:
        return False
    if name in ROOT_FILES:
        return True
    if name.startswith('server/web/') or name.startswith('server/deploy/evsikov.'):
        return False
    if not path.parts or path.parts[0] not in {'client', 'crates', 'server', 'docs'}:
        return False
    if any(part in PRIVATE_DIRS for part in path.parts):
        return False
    if path.name in PRIVATE_NAMES or path.suffix.lower() in PRIVATE_SUFFIXES:
        return False
    if path.name.startswith('.env') and path.name != '.env.example':
        return False
    if path.parts[0] == 'docs':
        return name.startswith('docs/public/') or name == 'docs/brand/consolecrypt.svg'
    # Keep only curated user-facing docs; vendored license notices are retained.
    if path.suffix.lower() == '.md':
        return path.name.upper() in {'LICENSE.MD', 'NOTICE.MD', 'COPYING.MD'}
    return True


def export(root: Path, destination: Path) -> list[str]:
    if destination.exists():
        raise ValueError('Destination already exists; choose a new directory')
    files = subprocess.check_output(
        ['git', '-C', str(root), 'ls-files', '-co', '--exclude-standard', '-z'],
    ).decode().split('\0')
    selected = sorted({name for name in files if name and publishable(name)})
    for name in selected:
        source = root / name
        if source.is_symlink():
            raise ValueError(f'Refusing source symlink: {name}')
        if not source.is_file():
            raise ValueError(f'Missing source file: {name}')
    destination.mkdir(parents=True)
    for name in selected:
        target = destination / name
        target.parent.mkdir(parents=True, exist_ok=True)
        shutil.copy2(root / name, target)
    # Guard future public commits too; local internal docs are never removed.
    with (destination / '.gitignore').open('a') as ignore:
        ignore.write('\n# Private development notes do not belong in this public tree.\n'
                     'AGENTS.md\nCLAUDE.md\nCLIENT_*.md\nSERVER_*.md\n'
                     'PARALLEL_*.md\nPROTOCOL_CHANGELOG.md\n'
                     '.codex/\n.claude/\n.agents/\ndocs/adr/\ndocs/design/\n')
    return selected


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--root', type=Path, default=Path(__file__).resolve().parents[2])
    args = parser.parse_args()
    files = export(args.root.resolve(), args.output.resolve())
    size = sum((args.output / name).stat().st_size for name in files)
    print(f'Exported {len(files)} files ({size / 1024 / 1024:.1f} MiB) to {args.output}')


if __name__ == '__main__':
    main()
