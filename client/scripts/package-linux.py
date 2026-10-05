#!/usr/bin/env python3
"""Package a verified Flutter Linux x64 release bundle as DEB and RPM."""
from __future__ import annotations

import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import struct
import subprocess
import tempfile

VERSION = re.compile(r'(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\+([1-9][0-9]*)')
REQUIRED_ELFS = ('consolecrypt', 'lib/libapp.so', 'lib/libflutter_linux_gtk.so', 'lib/libcc_bridge.so')
DEB_DEPENDS = ('libc6 (>= 2.35), libstdc++6 (>= 11), libgcc-s1, '
               'libgtk-3-0 (>= 3.22), libglib2.0-0 (>= 2.56), libblkid1, libepoxy0, liblzma5, libgl1, libegl1, libgles2, '
               'xdg-desktop-portal')


def digest(path: Path) -> str:
    hasher = hashlib.sha256()
    with path.open('rb') as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b''):
            hasher.update(chunk)
    return hasher.hexdigest()


def validate_bundle(bundle: Path, version: str) -> tuple[str, str]:
    match = VERSION.fullmatch(version)
    if not match:
        raise ValueError('Version must be major.minor.patch+positive_build')
    if not bundle.is_dir() or bundle.is_symlink():
        raise ValueError('Expected an actual release bundle directory')
    for item in bundle.rglob('*'):
        if item.is_symlink() or not (item.is_dir() or item.is_file()):
            raise ValueError('Release bundle must contain only regular files and directories')
    for name in REQUIRED_ELFS:
        binary = bundle / name
        if not binary.is_file():
            raise ValueError(f'Missing release binary: {name}')
        with binary.open('rb') as stream:
            header = stream.read(20)
        if len(header) < 20 or header[:6] != b'\x7fELF\x02\x01' or struct.unpack('<H', header[18:20])[0] != 62:
            raise ValueError(f'Expected a Linux x86-64 ELF: {name}')
    if not (bundle / 'data/icudtl.dat').is_file() or not (bundle / 'data/flutter_assets').is_dir():
        raise ValueError('Missing Flutter release assets')
    assets = bundle / 'data/flutter_assets'
    if any((assets / name).exists() for name in ('kernel_blob.bin', 'vm_snapshot_data', 'isolate_snapshot_data')):
        raise ValueError('Debug artifacts must not enter a release package')
    return '.'.join(match.groups()[:3]), match.group(4)


def stage_bundle(root: Path, bundle: Path, stage: Path, version: str) -> dict:
    release, build = validate_bundle(bundle, version)
    license_file = root / 'LICENSE'
    if 'GNU AFFERO GENERAL PUBLIC LICENSE' not in license_file.read_text():
        raise ValueError('The distribution must include the full AGPL licence')
    rdp_notice = root / 'client/rust/rdp-core/THIRD_PARTY_NOTICES.txt'
    if (root / 'client/rust/rdp-core').is_dir():
        packaged_notice = bundle / 'data/flutter_assets/assets/licenses/RDP-THIRD-PARTY-NOTICES.txt'
        if not rdp_notice.is_file() or not packaged_notice.is_file() or packaged_notice.read_bytes() != rdp_notice.read_bytes():
            raise ValueError('The RDP release bundle must include the current third-party notice')
    installation = stage / 'opt/consolecrypt'
    shutil.copytree(bundle, installation)
    for path in installation.rglob('*'):
        path.chmod(0o755 if path.is_dir() else 0o644)
    (installation / 'consolecrypt').chmod(0o755)
    shutil.copy2(license_file, installation / 'LICENSE')
    if rdp_notice.is_file():
        shutil.copy2(rdp_notice, installation / 'RDP-THIRD-PARTY-NOTICES.txt')
    receipt = {
        'version': version, 'platform': 'linux-x64', 'deb_version': f'{release}-{build}',
        'rpm_version': release, 'rpm_release': build,
        'native_sha256': {name: digest(bundle / name) for name in REQUIRED_ELFS},
    }
    (installation / 'ConsoleCrypt.version').write_text(version + '\n')
    (installation / 'package-info.json').write_text(json.dumps(receipt, indent=2) + '\n')
    launcher = stage / 'usr/bin/consolecrypt'
    launcher.parent.mkdir(parents=True)
    launcher.write_text('#!/bin/sh\nexec /opt/consolecrypt/consolecrypt "$@"\n')
    launcher.chmod(0o755)
    copies = {
        'consolecrypt.desktop': 'usr/share/applications/io.consolecrypt.consolecrypt.desktop',
        'consolecrypt.metainfo.xml': 'usr/share/metainfo/io.consolecrypt.consolecrypt.metainfo.xml',
        'consolecrypt.svg': 'usr/share/icons/hicolor/scalable/apps/io.consolecrypt.consolecrypt.svg',
    }
    for source, target in copies.items():
        destination = stage / target
        destination.parent.mkdir(parents=True, exist_ok=True)
        shutil.copy2(root / 'client/packaging/linux' / source, destination)
    documentation = stage / 'usr/share/doc/consolecrypt'
    documentation.mkdir(parents=True)
    shutil.copy2(license_file, documentation / 'LICENSE')
    if rdp_notice.is_file():
        shutil.copy2(rdp_notice, documentation / 'RDP-THIRD-PARTY-NOTICES.txt')
    (documentation / 'copyright').write_text(
        'ConsoleCrypt\nCopyright 2026 Alexander Evsikov <i@evsikov.net>\n'
        'First-party software: AGPL-3.0-only. Full text: LICENSE.\n'
        'Third-party notices: /opt/consolecrypt/data/flutter_assets/NOTICES.Z\n'
        'Sources: https://github.com/evsikovas/consolecrypt-client\n')
    return receipt


def build_packages(root: Path, bundle: Path, output: Path, version: str) -> dict:
    release, build = validate_bundle(bundle, version)
    for tool in ('dpkg-deb', 'rpmbuild'):
        if shutil.which(tool) is None:
            raise ValueError(f'Install the {tool} packaging tool first')
    output.mkdir(parents=True, exist_ok=True)
    names = [f'ConsoleCrypt-{version}-linux-x64.{suffix}' for suffix in ('deb', 'rpm')]
    if any((output / name).exists() for name in names):
        raise ValueError('This build already has packages; reserve a fresh build number')
    # Work inside the output filesystem. No install scripts, network operations,
    # login keyring access or package-manager changes happen while packaging.
    with tempfile.TemporaryDirectory(prefix='.linux-package-', dir=output) as folder:
        temporary = Path(folder)
        stage = temporary / 'payload'
        receipt = stage_bundle(root, bundle, stage, version)
        deb_stage = temporary / 'deb'
        shutil.copytree(stage, deb_stage)
        control = deb_stage / 'DEBIAN'
        control.mkdir()
        installed_size = (sum(p.stat().st_size for p in stage.rglob('*') if p.is_file()) + 1023) // 1024
        (control / 'control').write_text(
            f'Package: consolecrypt\nVersion: {release}-{build}\nArchitecture: amd64\n'
            f'Maintainer: Alexander Evsikov <i@evsikov.net>\nSection: net\nPriority: optional\n'
            f'Installed-Size: {installed_size}\nDepends: {DEB_DEPENDS}\n'
            # The rfd XDG picker requires the frontend; let the desktop choose
            # its FileChooser provider instead of forcing GTK onto KDE/GNOME.
            'Recommends: gnome-keyring, libgl1-mesa-dri, fonts-dejavu-core\n'
            'Suggests: openssh-client, xdg-desktop-portal-gtk | xdg-desktop-portal-gnome | xdg-desktop-portal-kde\n'
            'Homepage: https://consolecrypt.dev\n'
            'Description: SSH, SFTP and encrypted workspaces for Linux\n'
            ' Manage hosts, snippets and team sharing. Requires a graphical desktop\n'
            ' and an unlocked Secret Service keyring on its session D-Bus.\n')
        subprocess.run(['dpkg-deb', '--build', '--root-owner-group', str(deb_stage), str(temporary / names[0])], check=True)
        rpm_top = temporary / 'rpm'
        for directory in ('BUILD', 'BUILDROOT', 'RPMS', 'SOURCES', 'SPECS', 'SRPMS'):
            (rpm_top / directory).mkdir(parents=True)
        spec = rpm_top / 'SPECS/consolecrypt.spec'
        spec.write_text(
            '%global debug_package %{nil}\n%global __os_install_post %{nil}\n'
            '%global _build_id_links none\n'
            f'Name: consolecrypt\nVersion: {release}\nRelease: {build}\n'
            'Summary: SSH, SFTP and encrypted workspaces for Linux\nLicense: AGPL-3.0-only\n'
            'URL: https://consolecrypt.dev\nBuildArch: x86_64\n'
            'Requires: gtk3 >= 3.22\nRequires: glib2 >= 2.56\nRequires: dbus\n'
            'Requires: xdg-desktop-portal\n'
            # Impeller dlopens GLES, so ELF dependency discovery cannot find it.
            'Requires: mesa-libGL\nRequires: mesa-libEGL\nRequires: libGLESv2.so.2()(64bit)\n'
            'Recommends: gnome-keyring\nRecommends: mesa-dri-drivers\n'
            'Recommends: dejavu-sans-mono-fonts\nSuggests: openssh-clients\n'
            'Suggests: xdg-desktop-portal-gtk\nSuggests: xdg-desktop-portal-gnome\n'
            'Suggests: xdg-desktop-portal-kde\n'
            '%description\nManage hosts, snippets and team sharing. Requires a graphical desktop\n'
            'and an unlocked Secret Service keyring on its session D-Bus.\n'
            '%prep\n%build\n%install\n'
            f'mkdir -p "%{{buildroot}}"\ncp -a "{stage}/." "%{{buildroot}}/"\n'
            '%files\n%defattr(-,root,root,-)\n/opt/consolecrypt\n/usr/bin/consolecrypt\n'
            '/usr/share/applications/io.consolecrypt.consolecrypt.desktop\n'
            '/usr/share/metainfo/io.consolecrypt.consolecrypt.metainfo.xml\n'
            '/usr/share/icons/hicolor/scalable/apps/io.consolecrypt.consolecrypt.svg\n'
            '/usr/share/doc/consolecrypt\n')
        subprocess.run(['rpmbuild', '-bb', '--define', f'_topdir {rpm_top}',
                        '--define', '_buildhost reproducible', str(spec)], check=True)
        rpms = list((rpm_top / 'RPMS/x86_64').glob('*.rpm'))
        if len(rpms) != 1:
            raise ValueError('Expected exactly one x86-64 RPM')
        shutil.copy2(rpms[0], temporary / names[1])
        receipt['packages'] = {name: {'bytes': (temporary / name).stat().st_size,
                                     'sha256': digest(temporary / name)} for name in names}
        for name in names:
            os.replace(temporary / name, output / name)
        # Replace sidecars without truncating a retained artifact's inode.
        sidecars = {'ConsoleCrypt.version': version + '\n',
                    f'ConsoleCrypt-{version}-linux-x64.json': json.dumps(receipt, indent=2) + '\n',
                    f'ConsoleCrypt-{version}-linux-x64.SHA256SUMS': ''.join(
                        f'{receipt["packages"][name]["sha256"]}  {name}\n' for name in names)}
        for name, contents in sidecars.items():
            source = temporary / name
            source.write_text(contents)
            os.replace(source, output / name)
    return receipt


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--root', type=Path, default=Path(__file__).resolve().parents[2])
    parser.add_argument('--bundle', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--version', required=True)
    args = parser.parse_args()
    build_packages(args.root.resolve(), args.bundle.resolve(), args.output.resolve(), args.version)
    print(f'Linux DEB and RPM packaged: {args.version}')


if __name__ == '__main__':
    main()
