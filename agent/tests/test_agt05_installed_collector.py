"""DRAFT/UNRUN actual module tests; no vmagent/service/namespace execution.

The positive copy test needs an explicit reviewed vmagent binary input. It does
not sign a release or establish installed/systemd/crypto acceptance.
"""
import importlib.util
import os
from pathlib import Path
import sys
import tempfile
import time
import unittest

AGENT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(AGENT / "delivery"))
import stage
import prototype_package


def module(name, path):
    spec = importlib.util.spec_from_file_location(name, path)
    result = importlib.util.module_from_spec(spec)
    sys.modules[name] = result
    spec.loader.exec_module(result)
    return result


entry = module("cc_installed_unit_entry_test", AGENT / "collector/volatile_unit_entry.py")
launcher = module("cc_installed_launch_test", AGENT / "launch.py")


class InstalledCollectorTests(unittest.TestCase):
    def binary(self):
        path = os.environ.get("CC_AGT05_TEST_VMAGENT")
        self.assertIsNotNone(path, "explicit reviewed public binary input required; no cache discovery/skip")
        return prototype_package.collector_binary_bytes(Path(path))

    def test_fixed_collector_input_rejects_fifo_before_read_and_requires_exact_binary(self):
        with tempfile.TemporaryDirectory(prefix="cc-collector-input-") as root:
            path = Path(root) / "wrong";path.write_bytes(b"not a pinned ELF")
            with self.assertRaises(ValueError): prototype_package.collector_binary_bytes(path)
            if hasattr(os, "mkfifo"):
                fifo = Path(root) / "fifo";os.mkfifo(fifo)
                began = time.monotonic()
                with self.assertRaises(ValueError): prototype_package.collector_binary_bytes(fifo)
                self.assertLess(time.monotonic() - began, 1)

    def test_signed_runtime_closure_is_complete_fixed_mode_and_pinned(self):
        files = {n: (b"# fixture source\n" if not n.endswith("vmagent-prod") else self.binary()) for n in stage.COLLECTOR_FILES}
        modes = {n: ("0755" if n.endswith("vmagent-prod") else "0644") for n in files}
        self.assertTrue(stage._collector_files(files, modes))
        partial = dict(files);partial.pop("collector/unit_spec.py")
        with self.assertRaises(stage.StageError): stage._collector_files(partial, modes)
        wrong = dict(modes);wrong["collector/vmagent-prod"] = "0644"
        with self.assertRaises(stage.StageError): stage._collector_files(files, wrong)
        wrong = dict(files);wrong["collector/vmagent-prod"] = b"unreviewed"
        with self.assertRaises(stage.StageError): stage._collector_files(wrong, modes)

    def test_verified_managed_stable_copies_track_selection_and_refuse_tamper(self):
        # This is the unprivileged materializer boundary AFTER verification,
        # not a substitute signature verifier or installed root authority test.
        with tempfile.TemporaryDirectory(prefix="cc-collector-managed-") as root:
            root = Path(root);code = root / "opt/consolecrypt-agent"
            files = {n: (b"# first fixture source\n" if not n.endswith("vmagent-prod") else self.binary()) for n in stage.COLLECTOR_FILES}
            files["launch.py"] = b"# inert fixture launch\n"
            stage._managed_files(root, code, files)
            for n in stage.COLLECTOR_FILES:
                self.assertEqual((code/n).read_bytes(), files[n])
            updated = dict(files);updated["collector/unit_spec.py"] = b"# second fixture source\n"
            stage._managed_files(root, code, updated)
            self.assertEqual((code/"collector/unit_spec.py").read_bytes(), updated["collector/unit_spec.py"])
            stage._managed_files(root, code, files)  # Same managed restore path used by rollback.
            self.assertEqual((code/"collector/unit_spec.py").read_bytes(), files["collector/unit_spec.py"])
            foreign = b"foreign modification\n";(code/"collector/unit_spec.py").write_bytes(foreign)
            with self.assertRaises(stage.StageError): stage._managed_files(root, code, updated)
            self.assertEqual((code/"collector/unit_spec.py").read_bytes(), foreign)

    def test_private_user_namespace_requires_exact_nonroot_service_self_maps(self):
        self.assertTrue(entry.self_mapping("      1200       1200          1\n", 1200))
        for raw, uid in [("0 0 4294967295", 1200), ("0 0 1", 0), ("65534 65534 1", 65534),
                         ("1200 1300 1", 1200), ("1200 1200 2", 1200), ("1200 1200 1\n0 0 1", 1200),
                         ("1200 1200 1\n1200 1200 1", 1200), ("+1200 1200 1", 1200)]:
            self.assertFalse(entry.self_mapping(raw, uid))

    def test_launcher_requires_full_signed_collector_before_any_payload_import(self):
        with tempfile.TemporaryDirectory(prefix="cc-collector-launch-") as root:
            with self.assertRaises(ValueError): launcher._stable_collector(Path(root), Path(root), {"files": {}, "file_modes": {}})


if __name__ == "__main__":
    unittest.main()
