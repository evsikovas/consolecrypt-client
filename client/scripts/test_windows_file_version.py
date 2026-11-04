"""Windows metadata must accept full CI identities across 16-bit boundaries."""
import importlib.util
from pathlib import Path
import unittest

spec = importlib.util.spec_from_file_location('windows_file_version', Path(__file__).with_name('windows-file-version.py'))
module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)


class WindowsFileVersionTests(unittest.TestCase):
    def test_existing_versions_and_word_boundaries(self):
        for build, expected in [(1, 1), (65012, 65012), (65535, 65535),
                                (65536, 0), (66012, 476), (67012, 1476),
                                (131072, 0), (2100000000, 29952)]:
            with self.subTest(build=build):
                self.assertEqual(module.file_version(f'1.0.6+{build}'), f'1.0.6.{expected}')

    def test_every_numeric_component_fits_a_word(self):
        self.assertEqual(module.file_version('65535.65535.65535+65536'), '65535.65535.65535.0')
        for release in ('65536.0.0', '0.65536.0', '0.0.65536'):
            with self.subTest(release=release), self.assertRaises(ValueError):
                module.file_version(f'{release}+1')

    def test_invalid_versions_fail_before_packaging(self):
        for value in ('1.0.6', '1.0.6+0', '1.0.6+-1', '1.0.6+01', '01.0.6+1',
                      '1.0.6+2100000001', '1.0.6+١', '1.0.6+1\n', '1.0.6+1.2'):
            with self.subTest(value=value), self.assertRaises(ValueError):
                module.file_version(value)


if __name__ == '__main__':
    unittest.main()
