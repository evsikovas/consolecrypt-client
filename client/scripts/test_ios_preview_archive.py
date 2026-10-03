"""Simulator archive retention, metadata privacy and aggregate upload policy."""
import hashlib
import importlib.util
import json
from pathlib import Path
import plistlib
import tempfile
from types import SimpleNamespace
import unittest
from unittest import mock
import zipfile

ROOT = Path(__file__).resolve().parents[2]
spec = importlib.util.spec_from_file_location('ios_preview', ROOT / 'client/scripts/retain-ios-preview.py')
preview = importlib.util.module_from_spec(spec)
spec.loader.exec_module(preview)
SOURCE = 'a' * 40
JOB = 1367


class IosPreviewArchiveTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory(prefix='cc-ios-preview-fixture-')
        self.folder = Path(self.temporary.name)
        self.archive = self.folder / f'ConsoleCrypt-0.3.0+{JOB}-ios-simulator-universal.zip'
        self.cache = self.folder / 'private-cache'
        self.public = self.folder / 'public-metadata'
        self.make_archive()

    def tearDown(self):
        self.temporary.cleanup()

    def make_archive(self, *, version='0.3.0', build=JOB, name='Runner.app/synthetic-data'):
        with zipfile.ZipFile(self.archive, 'w', compression=zipfile.ZIP_STORED) as z:
            z.writestr('Runner.app/Info.plist', plistlib.dumps({
                'CFBundleShortVersionString': version, 'CFBundleVersion': str(build),
                'CFBundleIdentifier': 'io.consolecrypt.consolecrypt',
                'DTPlatformName': 'iphonesimulator',
            }))
            z.writestr(name, b'synthetic public build fixture\n' * 65536)

    def retain(self):
        return preview.retain_preview(self.archive, self.cache, self.public, SOURCE, JOB)

    def test_atomic_retention_and_small_public_metadata_without_absolute_paths(self):
        manifest = self.retain()
        retained = self.cache / SOURCE / str(JOB) / self.archive.name
        self.assertEqual(retained.read_bytes(), self.archive.read_bytes())
        self.assertEqual(manifest['sha256'], hashlib.sha256(self.archive.read_bytes()).hexdigest())
        self.assertEqual(retained.stat().st_mode & 0o777, 0o600)
        for folder in (self.cache, self.cache / SOURCE, retained.parent):
            self.assertEqual(folder.stat().st_mode & 0o777, 0o700)
        self.assertEqual({p.name for p in self.public.iterdir()}, set(preview.PUBLIC_FILES))
        self.assertLess(sum(p.stat().st_size for p in self.public.iterdir()), 8 * 1024)
        for path in self.public.iterdir():
            self.assertNotIn(str(self.folder), path.read_text())
        self.assertFalse(manifest['ci_artifacts_contain_native_bundle'])
        self.assertFalse(manifest['installable_iphone_ipa'])
        self.assertFalse(manifest['release_publication_performed'])
        # A later checkout clean removes the input, not the retained archive.
        self.archive.unlink()
        self.assertEqual(preview.digest(retained), manifest['sha256'])

    def test_repeat_is_idempotent_and_different_existing_bundle_never_overwritten(self):
        first = self.retain()
        self.assertEqual(self.retain(), first)
        target = self.cache / SOURCE / str(JOB) / self.archive.name
        target.write_bytes(b'unrelated retained fixture')
        with self.assertRaisesRegex(ValueError, 'replace an existing retained'):
            self.retain()
        self.assertEqual(target.read_bytes(), b'unrelated retained fixture')
        self.assertFalse(list(target.parent.glob('.incoming-*')))

    def test_existing_identical_bundle_requires_private_mode_and_same_owner(self):
        self.retain()
        target = self.cache / SOURCE / str(JOB) / self.archive.name
        original = target.read_bytes()
        target.chmod(0o644)
        with self.assertRaisesRegex(ValueError, 'existing retained'):
            self.retain()
        self.assertEqual(target.stat().st_mode & 0o777, 0o644)
        self.assertEqual(target.read_bytes(), original)
        target.chmod(0o600)
        original_stat = Path.stat

        def foreign_owner(path, *args, **kwargs):
            info = original_stat(path, *args, **kwargs)
            if path == target:
                return SimpleNamespace(st_mode=info.st_mode, st_uid=info.st_uid + 1)
            return info

        # Simulate an unrelated owner; never chown a real or fixture file.
        with mock.patch.object(Path, 'stat', foreign_owner):
            with self.assertRaisesRegex(ValueError, 'existing retained'):
                self.retain()
        self.assertEqual(target.read_bytes(), original)

    def test_native_build_or_identity_mismatch_rejects_before_retention(self):
        self.make_archive(build=1366)
        with self.assertRaisesRegex(ValueError, 'native identity'):
            self.retain()
        self.assertFalse(self.cache.exists())
        self.make_archive(version='0.3.1')
        with self.assertRaisesRegex(ValueError, 'filename differs'):
            self.retain()

    def test_source_requires_full_sha_and_counter_requires_positive_value(self):
        for source, job in [('a' * 7, JOB), (SOURCE, 0), ('../' + SOURCE, JOB)]:
            with self.subTest(source=source, job=job), self.assertRaises(ValueError):
                preview.retain_preview(self.archive, self.cache, self.public, source, job)
        self.assertFalse(self.cache.exists())

    def test_unsafe_zip_or_bad_crc_rejects_before_retention(self):
        self.make_archive(name='../outside-fixture')
        with self.assertRaisesRegex(ValueError, 'Unsafe'):
            self.retain()
        self.assertFalse(self.cache.exists())
        self.make_archive()
        data = bytearray(self.archive.read_bytes())
        at = data.index(b'synthetic public build fixture')
        data[at] ^= 1
        self.archive.write_bytes(data)
        with self.assertRaisesRegex(ValueError, 'CRC failed'):
            self.retain()

    def test_private_cache_or_public_output_symlink_is_rejected(self):
        actual = self.folder / 'actual-private'
        actual.mkdir(mode=0o700)
        self.cache.symlink_to(actual, target_is_directory=True)
        with self.assertRaisesRegex(ValueError, 'symlink'):
            self.retain()
        self.cache.unlink()
        self.public.symlink_to(actual, target_is_directory=True)
        with self.assertRaisesRegex(ValueError, 'metadata directory'):
            self.retain()
        self.assertFalse(list(actual.iterdir()))

    def test_weak_existing_cache_permissions_are_rejected(self):
        self.cache.mkdir(mode=0o755)
        self.cache.chmod(0o755)
        with self.assertRaisesRegex(ValueError, 'mode 0700'):
            self.retain()
        self.assertEqual(self.cache.stat().st_mode & 0o777, 0o755)

    def test_ci_uploads_metadata_only_not_all_split_or_full_zip_bytes(self):
        text = (ROOT / 'client/ci/ios.yml').read_text()
        body = text.split('build-ios-preview:\n', 1)[1]
        artifact = body.split('  artifacts:\n', 1)[1]
        paths = [line.strip()[2:] for line in artifact.splitlines() if line.strip().startswith('- ')]
        expected = ['dist/ios/ci-preview/' + name for name in preview.PUBLIC_FILES]
        self.assertEqual(paths, expected)
        self.assertIn('    when: always', artifact)
        self.assertFalse(any('.zip' in path or '*' in path for path in paths))


if __name__ == '__main__':
    unittest.main()
