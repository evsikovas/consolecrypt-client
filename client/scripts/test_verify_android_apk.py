"""Regression cases for whole-segment vs partial-segment RELRO protection."""
import importlib.util
from pathlib import Path
import unittest
import tempfile
import zipfile

spec = importlib.util.spec_from_file_location('verify_apk', Path(__file__).with_name('verify-android-apk.py'))
verifier = importlib.util.module_from_spec(spec)
spec.loader.exec_module(verifier)


class RelroTests(unittest.TestCase):
    def test_distribution_notices_match_source_and_reject_stale_or_missing_assets(self):
        with tempfile.TemporaryDirectory(prefix='cc-apk-notice-') as temporary:
            root = Path(temporary)
            notice = root / 'notice.txt'
            notice.write_bytes(b'synthetic licence notice')
            archive = root / 'fixture.apk'
            for case in ('valid', 'stale', 'missing', 'abbreviated-agpl'):
                with self.subTest(case=case):
                    with zipfile.ZipFile(archive, 'w') as apk:
                        apk.writestr('assets/flutter_assets/assets/licenses/AGPL-3.0-only.txt',
                                     b'GNU AFFERO GENERAL PUBLIC LICENSE' if case != 'abbreviated-agpl' else b'AGPL')
                        if case != 'missing':
                            apk.writestr('assets/flutter_assets/assets/licenses/RDP-THIRD-PARTY-NOTICES.txt',
                                         notice.read_bytes() if case != 'stale' else b'outdated')
                    with zipfile.ZipFile(archive) as apk:
                        if case == 'valid':
                            verifier.verify_distribution_notices(apk, notice)
                        else:
                            with self.assertRaises((AssertionError, KeyError)):
                                verifier.verify_distribution_notices(apk, notice)

    def test_whole_segment_with_padding_is_valid(self):
        verifier.verify_relro([(0x17ef0, 0x23000), (0x321e8, 0x429e0)], [(0x17ef0, 0x23000)])

    def test_aligned_prefix_is_valid(self):
        verifier.verify_relro([(0x17000, 0x28000)], [(0x17000, 0x20000)])

    def test_unaligned_prefix_is_rejected(self):
        with self.assertRaises(AssertionError):
            verifier.verify_relro([(0x17000, 0x28000)], [(0x17000, 0x21000)])

    def test_protection_must_not_cover_neighboring_writable_bytes(self):
        for segments in [[(0x17ef0, 0x23000), (0x23100, 0x28000)], [(0x17000, 0x17900), (0x17ef0, 0x23000)]]:
            with self.assertRaises(AssertionError):
                verifier.verify_relro(segments, [(0x17ef0, 0x23000)])


if __name__ == '__main__':
    unittest.main()
