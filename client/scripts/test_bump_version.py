import importlib.util
from pathlib import Path
from tempfile import TemporaryDirectory
import unittest

spec = importlib.util.spec_from_file_location('bump_version', Path(__file__).with_name('bump-version.py'))
versioning = importlib.util.module_from_spec(spec)
spec.loader.exec_module(versioning)


def fixture(root, version='0.1.0+1'):
    flutter = root / 'client/flutter'
    (flutter / 'lib/app').mkdir(parents=True)
    (flutter / 'pubspec.yaml').write_text(f'name: sample\nversion: {version}\n\ndependencies: {{}}\n')
    (flutter / 'lib/app/app_info.dart').write_text("const kAppVersion = '0.1.0';\nconst kAppBuildNumber = 1;\n")


class VersionTests(unittest.TestCase):
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
