import importlib.util
from pathlib import Path
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[2]
spec = importlib.util.spec_from_file_location('package_wheel', ROOT / 'delivery/package_wheel.py')
wheel = importlib.util.module_from_spec(spec)
spec.loader.exec_module(wheel)


class PinnedWheelTests(unittest.TestCase):
    def test_exact_pure_wheel_selected_without_import_or_extraction(self):
        selected = wheel.wheel_file(ROOT)
        self.assertEqual(set(selected), {wheel.PACKAGE})
        self.assertEqual(len(selected[wheel.PACKAGE]), 212900)

    def test_modified_wheel_rejected(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            target = root / wheel.SOURCE
            target.parent.mkdir(parents=True)
            content = bytearray((ROOT / wheel.SOURCE).read_bytes())
            content[-1] ^= 1
            target.write_bytes(content)
            with self.assertRaisesRegex(ValueError, 'changed'):
                wheel.wheel_file(root)


if __name__ == '__main__':
    unittest.main()
