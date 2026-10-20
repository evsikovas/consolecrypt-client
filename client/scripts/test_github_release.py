"""Exercise tag and artifact trust boundaries without credentials/network."""
import hashlib
import importlib.util
from pathlib import Path
import tempfile
import unittest


def load(name):
    spec = importlib.util.spec_from_file_location(name, Path(__file__).with_name(name + '.py'))
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


tag = load('verify-github-tag')
release = load('prepare-github-release')


class TagTests(unittest.TestCase):
    def test_only_matching_stable_tag(self):
        source = 'name: consolecrypt\nversion: 0.4.0+13034\n'
        self.assertEqual(tag.release_version('refs/tags/v0.4.0', source), '0.4.0')
        for ref in ('refs/heads/main', 'refs/heads/v0.4.0', 'refs/tags/v0.4.1',
                    'refs/tags/v0.4.0-rc.1', 'refs/tags/v00.4.0', 'refs/tags/v0.4.0/extra'):
            with self.subTest(ref=ref), self.assertRaises(ValueError):
                tag.release_version(ref, source)


class ArtifactTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.artifacts = self.root / 'artifacts'
        self.output = self.root / 'release'
        self.names = {
            'macos': ['ConsoleCrypt-0.4.0+14011-macos-universal.dmg'],
            'windows': ['ConsoleCrypt-0.4.0+14012-windows-x64-setup.exe', 'ConsoleCrypt-windows.zip'],
            'linux': ['ConsoleCrypt-0.4.0+14013-linux-x64.deb', 'ConsoleCrypt-0.4.0+14013-linux-x64.rpm'],
            'android': ['ConsoleCrypt-android-arm64.apk'],
        }
        for i, (platform, names) in enumerate(self.names.items(), 1):
            folder = self.artifacts / platform
            folder.mkdir(parents=True)
            (folder / 'ConsoleCrypt.version').write_text(f'0.4.0+1401{i}\n')
            for name in names:
                (folder / name).write_bytes(name.encode())
            (folder / 'private-test-home').write_text('must not ship')

    def test_closed_asset_set_and_actual_checksums(self):
        release.prepare(self.artifacts, self.output, '0.4.0')
        self.assertEqual(len(list(self.output.iterdir())), 7)
        self.assertTrue((self.output / 'ConsoleCrypt-0.4.0+14014-android-arm64.apk').is_file())
        self.assertTrue((self.output / 'ConsoleCrypt-0.4.0+14012-windows-x64.zip').is_file())
        for line in (self.output / 'SHA256SUMS-0.4.0.txt').read_text().splitlines():
            digest, name = line.split('  ')
            self.assertEqual(digest, hashlib.sha256((self.output / name).read_bytes()).hexdigest())

    def test_unrequested_ios_preview_is_not_published(self):
        folder = self.artifacts / 'ios'
        folder.mkdir()
        (folder / 'ConsoleCrypt.version').write_text('0.3.0+13015')
        (folder / 'ConsoleCrypt-0.3.0+13015-ios-simulator-universal.zip').write_bytes(b'old preview')
        names = release.prepare(self.artifacts, self.output, '0.4.0')
        self.assertEqual(len(names), 6)
        self.assertFalse(any('ios' in name for name in names))

    def test_mixed_release_is_rejected_before_output(self):
        (self.artifacts / 'android/ConsoleCrypt.version').write_text('0.3.2+13034')
        with self.assertRaises(ValueError):
            release.prepare(self.artifacts, self.output, '0.4.0')
        self.assertFalse(self.output.exists())

    def test_missing_platform_is_rejected(self):
        (self.artifacts / 'linux/ConsoleCrypt.version').unlink()
        with self.assertRaises(ValueError):
            release.prepare(self.artifacts, self.output, '0.4.0')

    def test_symlink_cannot_publish_external_file(self):
        apk = self.artifacts / 'android/ConsoleCrypt-android-arm64.apk'
        apk.unlink()
        outside = self.root / 'outside'
        outside.write_text('private')
        apk.symlink_to(outside)
        with self.assertRaises(ValueError):
            release.prepare(self.artifacts, self.output, '0.4.0')

    def test_existing_release_destination_is_not_replaced(self):
        self.output.mkdir()
        (self.output / 'keep').write_text('retained')
        with self.assertRaises(ValueError):
            release.prepare(self.artifacts, self.output, '0.4.0')
        self.assertEqual((self.output / 'keep').read_text(), 'retained')

    def test_every_package_obeys_mirror_size_limit_before_any_output(self):
        # Sparse files exercise the actual filesystem boundary without allocating
        # 500 MiB. Check optional archives/packages too, not just signed updaters.
        for platform, names in self.names.items():
            for name in names:
                source = self.artifacts / platform / name
                original = source.read_bytes()
                with self.subTest(platform=platform, name=name):
                    try:
                        with source.open('wb') as stream:
                            stream.truncate(500 * 1024 * 1024 + 1)
                        with self.assertRaisesRegex(ValueError, 'mirror limit of 500 MiB'):
                            release.prepare(self.artifacts, self.output, '0.4.0')
                        self.assertFalse(self.output.exists(), 'No partial release output may be copied')
                    finally:
                        source.write_bytes(original)


if __name__ == '__main__':
    unittest.main()
