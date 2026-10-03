import importlib.util
from pathlib import Path
from tempfile import TemporaryDirectory
import unittest
from unittest.mock import patch

spec = importlib.util.spec_from_file_location('bump_version', Path(__file__).with_name('bump-version.py'))
versioning = importlib.util.module_from_spec(spec)
spec.loader.exec_module(versioning)


def fixture(root, version='0.1.0+1'):
    flutter = root / 'client/flutter'
    (flutter / 'lib/app').mkdir(parents=True)
    (flutter / 'pubspec.yaml').write_text(f'name: sample\nversion: {version}\n\ndependencies: {{}}\n')
    (flutter / 'lib/app/app_info.dart').write_text("const kAppVersion = '0.1.0';\nconst kAppBuildNumber = 1;\n")


class VersionTests(unittest.TestCase):
    def test_ci_ids_below_or_equal_to_source_floor_do_not_reuse_a_counter(self):
        with TemporaryDirectory() as tmp:
            root = Path(tmp)
            fixture(root, '0.3.0+1362')
            for job in ('1361', '1362'):
                with patch.dict('os.environ', {'CC_BUILD_NUMBER': job}), self.assertRaisesRegex(ValueError, 'source build floor'):
                    versioning.bump(root)
                self.assertEqual(versioning.read_version(root)[2][-1], 1362)
            with patch.dict('os.environ', {'CC_BUILD_NUMBER': '1363'}):
                self.assertEqual(versioning.bump(root), '0.3.0+1363')
            self.assertEqual(versioning.bump(root), '0.3.0+1364')

    def test_dev_revision_and_native_build_preserve_channel_and_continuous_counter(self):
        with TemporaryDirectory() as tmp:
            root = Path(tmp)
            fixture(root, '0.3.0-dev.1+82')
            self.assertEqual(versioning.bump(root), '0.3.0-dev.1+83')
            self.assertEqual(versioning.bump(root, 'dev'), '0.3.0-dev.2+84')
            self.assertEqual(versioning.bump(root), '0.3.0-dev.2+85')
            self.assertEqual(versioning.native_version('0.3.0-dev.2+85'), '0.3.0+85')
            self.assertIn("kAppVersion = '0.3.0-dev.2'", (root / 'client/flutter/lib/app/app_info.dart').read_text())

    def test_dev_cannot_modify_stable_checkout_or_parse_untrusted_prerelease(self):
        with TemporaryDirectory() as tmp:
            root, stable = Path(tmp) / 'dev', Path(tmp) / 'stable'
            fixture(root, '0.3.0-dev.1+82')
            fixture(stable, '0.2.5+81')
            with self.assertRaisesRegex(ValueError, 'stable and dev'):
                versioning.bump(root, source_root=stable)
            self.assertEqual(versioning.read_version(root)[2][-1], 82)
            self.assertEqual(versioning.read_version(stable)[2][-1], 81)
        for version in ['0.3.0-dev.0+82', '0.3.0-dev.01+82', '0.3.0-rc.1+82', '0.3.0-dev.1+082',
                        '0.3.0-dev.1+82\n', '0.3.0-dev.1+82;touch', '0.3.0-dev.1١+82']:
            with self.subTest(version=version), self.assertRaises(ValueError):
                versioning.native_version(version)

    def test_native_ci_builds_use_unique_job_numbers_and_validate_them(self):
        with TemporaryDirectory() as tmp:
            root = Path(tmp)
            fixture(root)
            with patch.dict('os.environ', {'CC_BUILD_NUMBER': '400'}):
                self.assertEqual(versioning.bump(root), '0.1.0+400')
            with patch.dict('os.environ', {'CC_BUILD_NUMBER': '401'}):
                self.assertEqual(versioning.bump(root), '0.1.0+401')
            for value in ['-1', 'oops', '2100000001']:
                with patch.dict('os.environ', {'CC_BUILD_NUMBER': value}), self.assertRaises(ValueError):
                    versioning.bump(root)
            self.assertEqual(versioning.read_version(root)[2], (0, 1, 0, 401))

    def test_feature_and_repeated_builds_increase_numbers_and_keep_metadata_in_sync(self):
        with TemporaryDirectory() as tmp:
            root = Path(tmp)
            fixture(root)
            self.assertEqual(versioning.bump(root, 'patch'), '0.1.1+2')
            self.assertEqual(versioning.bump(root), '0.1.1+3')
            self.assertEqual(versioning.bump(root), '0.1.1+4')
            self.assertIn('version: 0.1.1+4\n\ndependencies:', (root / 'client/flutter/pubspec.yaml').read_text())
            dart = (root / 'client/flutter/lib/app/app_info.dart').read_text()
            self.assertIn("kAppVersion = '0.1.1'", dart)
            self.assertIn('kAppBuildNumber = 4', dart)

    def test_major_and_minor_reset_only_release_components(self):
        with TemporaryDirectory() as tmp:
            root = Path(tmp)
            fixture(root, '1.2.9+41')
            self.assertEqual(versioning.bump(root, 'minor'), '1.3.0+42')
            self.assertEqual(versioning.bump(root, 'major'), '2.0.0+43')

    def test_old_commit_reserves_a_new_number_from_main_checkout(self):
        with TemporaryDirectory() as tmp:
            root, source = Path(tmp) / 'main', Path(tmp) / 'old'
            fixture(root, '0.2.0+20')
            fixture(source)
            self.assertEqual(versioning.bump(root, source_root=source), '0.1.0+21')
            self.assertEqual(versioning.read_version(root)[2], (0, 2, 0, 21))
            self.assertEqual(versioning.read_version(source)[2], (0, 1, 0, 21))
            self.assertEqual(versioning.bump(root), '0.2.0+22')

    def test_invalid_metadata_does_not_partially_bump_pubspec(self):
        with TemporaryDirectory() as tmp:
            root = Path(tmp)
            fixture(root)
            (root / 'client/flutter/lib/app/app_info.dart').write_text('broken metadata')
            with self.assertRaises(ValueError):
                versioning.bump(root)
            self.assertEqual(versioning.read_version(root)[2], (0, 1, 0, 1))

    def test_invalid_version_and_android_overflow_are_rejected(self):
        for initial in ['latest', '0.1.0+2100000000']:
            with self.subTest(initial=initial), TemporaryDirectory() as tmp:
                root = Path(tmp)
                fixture(root, initial)
                with self.assertRaises(ValueError):
                    versioning.bump(root)


if __name__ == '__main__':
    unittest.main()
