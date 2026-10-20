#!/usr/bin/env python3
"""Assemble only public native installers; fail on absent/mixed artifacts."""
import argparse
import hashlib
from pathlib import Path
import re
import shutil

MAX_INSTALLER_BYTES = 500 * 1024 * 1024  # The serving update mirror applies this cap to every package.


def prepare(artifacts, output, version):
    if not re.fullmatch(r'(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)', version):
        raise ValueError('Invalid stable version')
    if output.exists() and any(output.iterdir()):
        raise ValueError('Release destination must be empty')
    files = []
    for platform in ('macos', 'windows', 'linux', 'android'):
        folder = artifacts / platform
        receipts = list(folder.glob('*.version'))
        if len(receipts) != 1 or receipts[0].is_symlink():
            raise ValueError(f'Missing or ambiguous {platform} version receipt')
        full = receipts[0].read_text().strip()
        if not re.fullmatch(re.escape(version) + r'\+[1-9]\d*', full):
            raise ValueError(f'{platform} artifact belongs to another release')
        prefix = f'ConsoleCrypt-{full}'
        names = {
            'macos': [(f'{prefix}-macos-universal.dmg', f'{prefix}-macos-universal.dmg')],
            'windows': [(f'{prefix}-windows-x64-setup.exe', f'{prefix}-windows-x64-setup.exe'),
                        ('ConsoleCrypt-windows.zip', f'{prefix}-windows-x64.zip')],
            'android': [('ConsoleCrypt-android-arm64.apk', f'{prefix}-android-arm64.apk')],
            'linux': [(f'{prefix}-linux-x64.deb', f'{prefix}-linux-x64.deb'),
                      (f'{prefix}-linux-x64.rpm', f'{prefix}-linux-x64.rpm')],
        }[platform]
        for source_name, destination_name in names:
            source = folder / source_name
            if source.is_symlink() or not source.is_file() or source.stat().st_size <= 0:
                raise ValueError(f'Missing regular installer: {source_name}')
            if source.stat().st_size > MAX_INSTALLER_BYTES:
                raise ValueError(f'Installer exceeds the update mirror limit of 500 MiB: {source_name}')
            files.append((source, destination_name))
    output.mkdir(parents=True, exist_ok=True)
    checksums = []
    for source, name in sorted(files, key=lambda item: item[1]):
        destination = output / name
        shutil.copyfile(source, destination)
        with destination.open('rb') as stream:
            digest = hashlib.file_digest(stream, 'sha256').hexdigest()
        checksums.append(f'{digest}  {name}\n')
    (output / f'SHA256SUMS-{version}.txt').write_text(''.join(checksums), encoding='utf-8')
    return [name for _, name in files]


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--artifacts', required=True, type=Path)
    parser.add_argument('--output', required=True, type=Path)
    parser.add_argument('--version', required=True)
    args = parser.parse_args()
    print('\n'.join(prepare(args.artifacts, args.output, args.version)))
