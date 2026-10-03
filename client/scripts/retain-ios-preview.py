#!/usr/bin/env python3
"""Retain a Simulator ZIP privately; export only small public CI metadata.

GitLab uploads one archive for all artifacts, so splitting a ZIP inside that
archive cannot avoid its total upload limit. Native previews stay outside the
checkout on the runner; release publication is a separate reviewed operation.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path, PurePosixPath, PureWindowsPath
import plistlib
import re
import stat
import tempfile
import zipfile

PUBLIC_FILES = ('ConsoleCrypt-ios-preview.json', 'SHA256SUMS-ios-preview.txt',
                'README-ios-preview.txt')
MAX_ENTRIES = 25000
MAX_UNCOMPRESSED_BYTES = 4 * 1024 ** 3


def digest(path):
    checksum = hashlib.sha256()
    with path.open('rb') as handle:
        while block := handle.read(1024 * 1024):
            checksum.update(block)
    return checksum.hexdigest()


def inspect_archive(archive, job):
    if archive.is_symlink() or not archive.is_file():
        raise ValueError('Preview archive must be a regular file')
    with zipfile.ZipFile(archive) as bundle:
        entries = bundle.infolist()
        if (len(entries) > MAX_ENTRIES
                or sum(item.file_size for item in entries) > MAX_UNCOMPRESSED_BYTES):
            raise ValueError('Preview archive exceeds the validation bound')
        names = set()
        for item in entries:
            name = item.filename
            if ('\x00' in item.orig_filename or '\\' in name
                    or PurePosixPath(name).is_absolute() or PureWindowsPath(name).drive
                    or '..' in name.split('/') or name in names
                    or stat.S_ISLNK(item.external_attr >> 16)):
                raise ValueError('Unsafe or duplicate preview archive entry')
            names.add(name)
        metadata = bundle.getinfo('Runner.app/Info.plist')
        if metadata.file_size > 64 * 1024:
            raise ValueError('Preview bundle metadata exceeds the validation bound')
        info = plistlib.loads(bundle.read(metadata))
        version = info.get('CFBundleShortVersionString', '')
        if (not re.fullmatch(r'(?:0|[1-9][0-9]*)\.(?:0|[1-9][0-9]*)\.(?:0|[1-9][0-9]*)', version)
                or info.get('CFBundleVersion') != str(job)
                or info.get('CFBundleIdentifier') != 'io.consolecrypt.consolecrypt'
                or info.get('DTPlatformName') != 'iphonesimulator'):
            raise ValueError('Preview native identity or reserved job number differs')
        expected = f'ConsoleCrypt-{version}+{job}-ios-simulator-universal.zip'
        if archive.name != expected:
            raise ValueError('Preview archive filename differs from native metadata')
        if bundle.testzip() is not None:
            raise ValueError('Preview archive CRC failed')
    return version + '+' + str(job)


def private_directory(path):
    if path.is_symlink():
        raise ValueError('Private preview directory must not be a symlink')
    path.mkdir(parents=True, mode=0o700, exist_ok=True)
    info = path.stat()
    if not stat.S_ISDIR(info.st_mode) or info.st_uid != os.getuid() or info.st_mode & 0o077:
        raise ValueError('Private preview directory must be owned with mode 0700')


def retain_preview(archive, cache_root, public_dir, source, job):
    if not re.fullmatch(r'[0-9a-f]{40}', source) or not isinstance(job, int) or job < 1:
        raise ValueError('A full source SHA and positive CI job number are required')
    version = inspect_archive(archive, job)
    private_directory(cache_root)
    private_directory(cache_root / source)
    private_directory(cache_root / source / str(job))
    target = cache_root / source / str(job) / archive.name
    before = archive.stat()
    checksum = hashlib.sha256()
    descriptor, temporary_name = tempfile.mkstemp(prefix='.incoming-', dir=target.parent)
    temporary = Path(temporary_name)
    try:
        with os.fdopen(descriptor, 'wb') as destination, archive.open('rb') as origin:
            while block := origin.read(1024 * 1024):
                destination.write(block)
                checksum.update(block)
            destination.flush()
            os.fsync(destination.fileno())
        after = archive.stat()
        if (before.st_size, before.st_mtime_ns) != (after.st_size, after.st_mtime_ns):
            raise ValueError('Preview archive changed during retention')
        sha = checksum.hexdigest()
        if target.exists() or target.is_symlink():
            if (target.is_symlink() or not target.is_file()
                    or target.stat().st_uid != os.getuid()
                    or target.stat().st_mode & 0o777 != 0o600 or digest(target) != sha):
                raise ValueError('Refusing to replace an existing retained preview')
        else:
            # Exclusive link publishes a complete, fsynced file atomically.
            os.link(temporary, target)
        if digest(target) != sha:
            raise ValueError('Retained preview checksum differs')
    finally:
        temporary.unlink(missing_ok=True)
    manifest = {
        'schema_version': 1, 'source': source, 'job': job, 'version': version,
        'file': archive.name, 'bytes': before.st_size, 'sha256': sha,
        'archive_crc_verified': True, 'retained_on_runner': True,
        'runner_cache_key': f'{source}/{job}/{archive.name}',
        'ci_artifacts_contain_native_bundle': False,
        'installable_iphone_ipa': False,
        'release_publication_performed': False,
    }
    readme = (
        'ConsoleCrypt iOS Simulator preview\n\n'
        'This CI artifact contains metadata and checksums only, not the native app.\n'
        'The verified full ZIP is retained on the macOS runner outside its checkout.\n'
        'Its default private cache location is:\n'
        f'$HOME/.cache/consolecrypt/ios-preview/{source}/{job}/{archive.name}\n'
        'The cache is private to the runner account (0700); the ZIP is 0600.\n'
        'If the owner chose --cache-root, use that private root with the same source/job key.\n'
        'Ask the runner owner for the ZIP and verify SHA-256 before extracting it.\n'
        'It is an iOS Simulator preview, not an installable iPhone IPA.\n'
        'Reviewed public release publication and optional ZIP parts happen separately.\n'
    )
    output = [json.dumps(manifest, indent=2) + '\n', f'{sha}  {archive.name}\n', readme]
    if public_dir.is_symlink():
        raise ValueError('Public metadata directory must not be a symlink')
    public_dir.mkdir(parents=True, exist_ok=True)
    for name, contents in zip(PUBLIC_FILES, output):
        path = public_dir / name
        if path.exists() or path.is_symlink():
            if path.is_symlink() or not path.is_file() or path.read_text() != contents:
                raise ValueError('Refusing to replace different preview metadata')
        else:
            with path.open('x') as handle:
                handle.write(contents)
    return manifest


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--archive', type=Path, required=True)
    parser.add_argument('--source', required=True)
    parser.add_argument('--job', type=int, required=True)
    parser.add_argument('--cache-root', type=Path,
                        default=Path.home() / '.cache/consolecrypt/ios-preview')
    parser.add_argument('--public-dir', type=Path, required=True)
    args = parser.parse_args()
    manifest = retain_preview(args.archive, args.cache_root, args.public_dir,
                              args.source, args.job)
    # No absolute local paths or credentials enter the public log/manifest.
    print(json.dumps(manifest, sort_keys=True))


if __name__ == '__main__':
    main()
