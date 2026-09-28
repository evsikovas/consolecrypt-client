"""Regression cases for whole-segment vs partial-segment RELRO protection."""
import importlib.util
from pathlib import Path
import unittest

spec = importlib.util.spec_from_file_location('verify_apk', Path(__file__).with_name('verify-android-apk.py'))
verifier = importlib.util.module_from_spec(spec)
spec.loader.exec_module(verifier)


class RelroTests(unittest.TestCase):
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
