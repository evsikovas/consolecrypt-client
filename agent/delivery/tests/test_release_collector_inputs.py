"""Pinned collector input failures must precede package/output construction."""
import importlib.util
import os
from pathlib import Path
import tempfile
import unittest
from unittest import mock

AGENT = Path(__file__).resolve().parents[2]
spec = importlib.util.spec_from_file_location("collector_release_pipeline", AGENT / "delivery/release_pipeline.py")
pipeline = importlib.util.module_from_spec(spec)
spec.loader.exec_module(pipeline)


class ReleaseCollectorInputs(unittest.TestCase):
    def test_arm_is_compile_only_and_never_relabels_installed_metrics(self):
        with tempfile.TemporaryDirectory(prefix="cc-collector-release-") as directory:
            root = Path(directory)
            # The full package API denies ARM even before reading any supplied
            # input. An ARM compile artifact cannot acquire an AMD64 package.
            with self.assertRaisesRegex(ValueError, "amd64 only"):
                pipeline.capture_collector(root, "aarch64-unknown-linux-gnu", root / "missing")
            self.assertEqual(list(root.iterdir()), [])

    def test_missing_explicit_collector_never_discovers_ambient_binary(self):
        with tempfile.TemporaryDirectory(prefix="cc-collector-release-") as directory:
            root = Path(directory)
            with self.assertRaisesRegex(ValueError, "explicit pinned vmagent"):
                pipeline.capture_collector(root, "x86_64-unknown-linux-gnu", None)
            self.assertEqual(list(root.iterdir()), [])

    def test_foreign_elf_is_rejected_by_actual_maintained_supply_before_copy(self):
        source_root = Path(os.environ.get("CC_TEST_PACKAGE_SOURCE_ROOT", str(AGENT)))
        with tempfile.TemporaryDirectory(prefix="cc-collector-release-") as directory:
            root = Path(directory)
            supplied = root / "foreign-vmagent"
            supplied.write_bytes(b"\x7fELF\x02\x01" + b"\0" * 12 + bytes([62, 0]))
            original = supplied.read_bytes()
            # This calls the real current collector.supply.verify_binary, not a
            # mock verifier and not an accepted synthetic digest.
            with mock.patch.object(pipeline, "AGENT", source_root), self.assertRaisesRegex(ValueError, "digest mismatch"):
                pipeline.capture_collector(root, "x86_64-unknown-linux-gnu", supplied)
            self.assertEqual(supplied.read_bytes(), original)
            self.assertEqual([p.name for p in root.iterdir()], ["foreign-vmagent"])


if __name__ == "__main__":
    unittest.main()
