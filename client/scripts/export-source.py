#!/usr/bin/env python3
"""Export buildable public sources without private repository history or caches.

Only git-visible source files are considered. Never copy .git or ignored files.
The destination must be new; this tool cannot overwrite an existing checkout.
"""
from __future__ import annotations

import argparse
import hashlib
from pathlib import Path, PurePosixPath
import shutil
import stat
import subprocess

ROOT_FILES = {
    'README.md', 'SECURITY.md', 'LICENSE', 'AUTHORS', 'PROTOCOL_CHANGELOG.md',
    '.gitignore', '.gitattributes', '.dockerignore', '.gitlab-ci.yml',
    'rust-toolchain.toml',
}
PUBLIC_DOCUMENTS = {
    'server/deploy/native/README.md',
    'runner/deployment/README.md',
    'client/rust/rdp-core/vendor/PROVENANCE.md',
    'client/rust/terminal-core/vendor/PROVENANCE.md',
    # These public upstream files are compile-time include_str! inputs.
    'client/flutter/vendor/security/glib/README.md',
    'client/rust/platform-core/vendor/zbus/README.md',
    'client/rust/rdp-core/vendor/ashpd/README.md',
    'client/rust/ssh-core/vendor/ssh-key/README.md',
}
PRIVATE_DIRS = {
    '.git', '.github', '.codex', '.claude', '.agents', '.idea', '.vscode',
    'target', 'build', 'dist', '.dart_tool', '.gradle', '.cxx', 'Pods',
    'ephemeral', '__pycache__', '.dev-mail', 'secrets', '.local',
}
PRIVATE_NAMES = {
    'AGENTS.md', 'CLAUDE.md', '.DS_Store', 'local.properties', 'key.properties',
    '.flutter-plugins', '.flutter-plugins-dependencies', '.env', 'server.json',
    'windows_password.txt',
}
PRIVATE_SUFFIXES = {
    '.key', '.pem', '.p12', '.pfx', '.jks', '.keystore', '.mobileprovision',
    '.db', '.sqlite', '.sqlite3', '.log', '.pyc', '.apk', '.dmg', '.exe',
    '.zip', '.deb', '.rpm', '.dylib', '.dll', '.so', '.a', '.o', '.env',
}

AGENT_PRIVATE_DIRS = {'cache', '.cache', 'evidence', 'state', 'private', 'artifacts', 'examples'}
AGENT_DOCUMENTS = {
    'agent/README.md', 'agent/BUILDING.md', 'agent/THIRD_PARTY.md',
    'agent/OPERATOR_TUN.md',
    'agent/delivery/MONITORING_CONFIG_BACKUP.md',
    'agent/delivery/roles/consolecrypt_monitoring_prefix/README.md',
    'agent/delivery/roles/consolecrypt_agent_profile/README.md',
    'agent/delivery/roles/consolecrypt_agent_identity/README.md',
    'agent/delivery/roles/consolecrypt_config_candidate/README.md',
    'agent/identity/vendor/libsqlite3-sys/README.md',
    'agent/identity/vendor/libsqlite3-sys/Upgrade.md',
    'agent/identity/vendor/libsqlite3-sys/CONSOLECRYPT_BACKPORT.md',
}
AGENT_WHEEL = 'agent/third_party/websockets/websockets-17.4-py3-none-any.whl'
AGENT_WHEEL_SHA256 = 'c6bd2c9781915592b209160d0f017d4c7c82ad53075e6b0c24d2a89299e7f9e9'


def publishable(name: str) -> bool:
    path = PurePosixPath(name)
    if path.is_absolute() or '..' in path.parts:
        return False
    if name in ROOT_FILES:
        return True
    if name.startswith('server/web/') or name.startswith('server/deploy/evsikov.'):
        return False
    if not path.parts or path.parts[0] not in {'client', 'crates', 'server', 'docs', 'agent', 'runner'}:
        return False
    if path.parts[0] == 'runner' and not (
        name.startswith(('runner/src/', 'runner/setup/', 'runner/deployment/', 'runner/tests/', 'runner/test-fixture/'))
        or name in {'runner/pyproject.toml', 'runner/environment.json', 'runner/requirements-linux-x86_64-py312.lock', 'runner/requirements-linux-x86_64-py314.lock'}
    ):
        return False
    if any(part in PRIVATE_DIRS for part in path.parts):
        return False
    if path.name in PRIVATE_NAMES or path.suffix.lower() in PRIVATE_SUFFIXES:
        return False
    lowered_name = path.name.lower()
    if (lowered_name.startswith('.env') or '.env.' in lowered_name) and not lowered_name.endswith('.env.example'):
        return False
    if path.parts[0] == 'agent':
        if ('\\' in name or ':' in name or str(path) != name
                or any(part in AGENT_PRIVATE_DIRS for part in path.parts)
                or path.suffix.lower() in {'.elf', '.test'}):
            return False
        if path.suffix.lower() == '.whl':
            return name == AGENT_WHEEL
        if path.suffix.lower() == '.md':
            return name in AGENT_DOCUMENTS or path.name.upper() in {'LICENSE.MD', 'NOTICE.MD', 'COPYING.MD'}
        return True
    if path.parts[0] == 'docs':
        return name.startswith('docs/public/') or name == 'docs/brand/consolecrypt.svg'
    # Keep only curated user-facing docs; vendored license notices are retained.
    if path.suffix.lower() == '.md':
        return name in PUBLIC_DOCUMENTS or path.name.upper() in {'LICENSE.MD', 'NOTICE.MD', 'COPYING.MD'}
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
        if name.startswith('agent/'):
            info = source.lstat()
            if (not stat.S_ISREG(info.st_mode) or getattr(info, 'st_file_attributes', 0) & 0x400
                    or source.resolve(strict=True) != source.absolute() or info.st_size > 32 * 1024**2):
                raise ValueError(f'Regular bounded Agent source required: {name}')
            with source.open('rb') as stream:
                if stream.read(4).startswith((b'\x7fELF', b'MZ')):
                    raise ValueError(f'Agent executable artifact refused: {name}')
            if name == AGENT_WHEEL:
                with source.open('rb') as stream:
                    wheel_sha256 = hashlib.file_digest(stream, 'sha256').hexdigest()
                if info.st_size != 212900 or wheel_sha256 != AGENT_WHEEL_SHA256:
                    raise ValueError('Agent wheel differs from the pinned pure-Python source')
    destination.mkdir(parents=True)
    for name in selected:
        target = destination / name
        target.parent.mkdir(parents=True, exist_ok=True)
        shutil.copy2(root / name, target)
    # Guard future public commits too; local internal docs are never removed.
    with (destination / '.gitignore').open('a') as ignore:
        ignore.write('\n# Private development notes do not belong in this public tree.\n'
                     'AGENTS.md\nCLAUDE.md\nCLIENT_*.md\nSERVER_*.md\n'
                     'PARALLEL_*.md\n'
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
