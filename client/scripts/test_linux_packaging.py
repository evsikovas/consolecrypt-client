"""Linux release payload/metadata checks; real package checks run in the builder."""
import importlib.util
import json
import os
from pathlib import Path
import shutil
import struct
import subprocess
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[2]
spec = importlib.util.spec_from_file_location('linux_packaging', ROOT / 'client/scripts/package-linux.py')
packaging = importlib.util.module_from_spec(spec)
spec.loader.exec_module(packaging)
export_spec = importlib.util.spec_from_file_location('source_export', ROOT / 'client/scripts/export-source.py')
source_export = importlib.util.module_from_spec(export_spec)
export_spec.loader.exec_module(source_export)
stage_spec = importlib.util.spec_from_file_location('linux_stage', ROOT / 'client/scripts/stage-linux-source.py')
linux_stage = importlib.util.module_from_spec(stage_spec)
stage_spec.loader.exec_module(linux_stage)


class LinuxPackagingTest(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory(prefix='cc-linux-package-test-')
        self.folder = Path(self.temporary.name)
        self.bundle = self.folder / 'bundle'
        header = bytearray(64)
        header[:6] = b'\x7fELF\x02\x01'
        struct.pack_into('<H', header, 18, 62)
        for name in packaging.REQUIRED_ELFS:
            path = self.bundle / name
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_bytes(header)
        (self.bundle / 'data/flutter_assets').mkdir(parents=True)
        (self.bundle / 'data/icudtl.dat').write_bytes(b'fixture data')

    def tearDown(self):
        self.temporary.cleanup()

    def test_complete_bundle_licence_launcher_and_build_metadata(self):
        stage = self.folder / 'stage'
        receipt = packaging.stage_bundle(ROOT, self.bundle, stage, '0.2.5+123')
        app = stage / 'opt/consolecrypt'
        self.assertEqual((app / 'LICENSE').read_bytes(), (ROOT / 'LICENSE').read_bytes())
        self.assertEqual((app / 'ConsoleCrypt.version').read_text(), '0.2.5+123\n')
        self.assertEqual(json.loads((app / 'package-info.json').read_text()), receipt)
        self.assertEqual(receipt['deb_version'], '0.2.5-123')
        self.assertEqual(receipt['rpm_release'], '123')
        for name in packaging.REQUIRED_ELFS:
            self.assertEqual((app / name).read_bytes(), (self.bundle / name).read_bytes())
        self.assertEqual((stage / 'usr/bin/consolecrypt').read_text(),
                         '#!/bin/sh\nexec /opt/consolecrypt/consolecrypt "$@"\n')
        self.assertEqual((stage / 'usr/bin/consolecrypt').stat().st_mode & 0o777, 0o755)
        self.assertEqual((app / 'consolecrypt').stat().st_mode & 0o777, 0o755)
        self.assertEqual((app / 'lib/libcc_bridge.so').stat().st_mode & 0o777, 0o644)
        self.assertFalse((stage / 'DEBIAN').exists())
        self.assertFalse((app / 'private-source').exists())

    def test_reject_missing_native_core_wrong_architecture_and_symlinks(self):
        native = self.bundle / 'lib/libcc_bridge.so'
        header = native.read_bytes()
        native.unlink()
        with self.assertRaisesRegex(ValueError, 'Missing release binary'):
            packaging.validate_bundle(self.bundle, '0.2.5+123')
        arm_header = bytearray(header)
        struct.pack_into('<H', arm_header, 18, 183)
        native.write_bytes(arm_header)
        with self.assertRaisesRegex(ValueError, 'x86-64 ELF'):
            packaging.validate_bundle(self.bundle, '0.2.5+123')
        native.unlink()
        native.symlink_to(self.bundle / 'lib/libapp.so')
        with self.assertRaisesRegex(ValueError, 'regular files'):
            packaging.validate_bundle(self.bundle, '0.2.5+123')

    def test_versions_and_binary_export_are_strict(self):
        for bad in ('0.2.5', '0.2.5+0', '0.2.5+1\n', '0.2.5+1;echo secret', '0.2.5+01'):
            with self.subTest(version=bad), self.assertRaises(ValueError):
                packaging.validate_bundle(self.bundle, bad)
        for name in ('client/package.deb', 'client/ConsoleCrypt.RPM', 'client/build/private.rpm'):
            self.assertFalse(source_export.publishable(name))
        self.assertTrue(source_export.publishable('client/ci/linux/Dockerfile'))
        self.assertTrue(source_export.publishable('client/packaging/linux/consolecrypt.desktop'))

    def test_reject_debug_assets_left_by_native_integration_tests(self):
        assets = self.bundle / 'data/flutter_assets'
        for name in ('kernel_blob.bin', 'vm_snapshot_data', 'isolate_snapshot_data'):
            path = assets / name
            path.write_bytes(b'generated debug artifact')
            with self.subTest(name=name), self.assertRaisesRegex(ValueError, 'Debug artifacts'):
                packaging.validate_bundle(self.bundle, '0.2.5+123')
            path.unlink()
        self.assertEqual(packaging.validate_bundle(self.bundle, '0.2.5+123'), ('0.2.5', '123'))

    def test_container_staging_excludes_history_private_files_and_untracked_inputs(self):
        repo = self.folder / 'repo'
        files = {'client/scripts/export-source.py': (ROOT / 'client/scripts/export-source.py').read_bytes(),
                 'client/ci/linux/Dockerfile': b'FROM ubuntu\n',
                 'client/flutter/lib/main.dart': b'void main() {}\n',
                 'LICENSE': (ROOT / 'LICENSE').read_bytes(),
                 'rust-toolchain.toml': b'[toolchain]\nchannel="1.98"\n',
                 'client/AGENTS.md': b'internal notes', 'client/.env': os.urandom(32),
                 'client/private.key': os.urandom(32), 'client/old.deb': b'old binary',
                 'server/ignored.rs': b'server not needed', '.local/note.txt': b'private note'}
        for name, contents in files.items():
            destination = repo / name
            destination.parent.mkdir(parents=True, exist_ok=True)
            destination.write_bytes(contents)
        subprocess.run(['git', 'init', '-q', str(repo)], check=True)
        subprocess.run(['git', '-C', str(repo), 'add', '--', *files], check=True)
        (repo / 'client/untracked.txt').write_text('untracked input')
        output = self.folder / 'tracked-snapshot'
        self.assertEqual(linux_stage.stage(repo, output), 5)
        self.assertEqual({p.relative_to(output).as_posix() for p in output.rglob('*') if p.is_file()},
                         {'client/scripts/export-source.py', 'client/ci/linux/Dockerfile',
                          'client/flutter/lib/main.dart', 'LICENSE', 'rust-toolchain.toml'})
        with self.assertRaisesRegex(ValueError, 'new staging'):
            linux_stage.stage(repo, output)

    @unittest.skipUnless(os.uname().sysname == 'Linux' and
                         all(shutil.which(tool) for tool in ('gcc', 'dpkg-deb', 'rpm', 'rpmbuild')),
                         'real Linux packaging toolchain required')
    def test_real_deb_rpm_contents_dependencies_and_rebuild_guard(self):
        source = self.folder / 'fixture.c'
        source.write_text('int main(void) { return 0; }\n')
        for name in packaging.REQUIRED_ELFS:
            command = ['gcc', str(source), '-o', str(self.bundle / name)]
            if name != 'consolecrypt':
                command[1:1] = ['-shared', '-fPIC']
            subprocess.run(command, check=True)
        output = self.folder / 'output'
        receipt = packaging.build_packages(ROOT, self.bundle, output, '0.2.5+123')
        deb = output / 'ConsoleCrypt-0.2.5+123-linux-x64.deb'
        rpm = output / 'ConsoleCrypt-0.2.5+123-linux-x64.rpm'
        self.assertEqual(subprocess.check_output(['dpkg-deb', '-f', str(deb), 'Version'], text=True).strip(), '0.2.5-123')
        deb_dependencies = subprocess.check_output(['dpkg-deb', '-f', str(deb), 'Depends'], text=True)
        self.assertIn('libgles2', deb_dependencies)
        self.assertEqual(subprocess.check_output(['rpm', '-qp', '--qf', '%{VERSION}-%{RELEASE} %{ARCH}', str(rpm)], text=True),
                         '0.2.5-123 x86_64')
        extracted = self.folder / 'extracted'
        subprocess.run(['dpkg-deb', '-x', str(deb), str(extracted)], check=True)
        for name, expected in receipt['native_sha256'].items():
            self.assertEqual(packaging.digest(extracted / 'opt/consolecrypt' / name), expected)
        self.assertEqual((extracted / 'opt/consolecrypt/LICENSE').read_bytes(), (ROOT / 'LICENSE').read_bytes())
        scripts = subprocess.check_output(['rpm', '-qp', '--scripts', str(rpm)], text=True)
        self.assertEqual(scripts, '')
        rpm_dependencies = subprocess.check_output(['rpm', '-qp', '--requires', str(rpm)], text=True)
        self.assertIn('libc.so.6', rpm_dependencies)
        self.assertIn('libGLESv2.so.2()(64bit)', rpm_dependencies)
        with self.assertRaisesRegex(ValueError, 'fresh build number'):
            packaging.build_packages(ROOT, self.bundle, output, '0.2.5+123')


if __name__ == '__main__':
    unittest.main()
