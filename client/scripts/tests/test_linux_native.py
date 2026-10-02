"""Regression tests for privacy and success gates in the native test harness."""

from contextlib import redirect_stdout
import importlib.util
import io
import json
import os
from pathlib import Path
import tempfile
import unittest


SOURCE = Path(__file__).resolve().parents[1] / "test-linux-native.py"
SPEC = importlib.util.spec_from_file_location("linux_native_acceptance", SOURCE)
HARNESS = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(HARNESS)
SUITE = "integration_test/rust_core_test.dart"


class NativeReporterTest(unittest.TestCase):
    def run_reporter(self, *, completed=True, missing=False, exit_code=0, large=False, error=False):
        marker = os.urandom(32).hex()  # generated diagnostic payload, never printed
        events = [{"type": "print", "message": marker}]
        if error:
            events.append({"type": "error", "message": marker, "stackTrace": marker})
        if large:
            events.append({"type": "print", "message": marker * 10000})
        for index, name in enumerate(sorted(HARNESS.SUITES[SUITE])):
            events.append({"type": "testStart", "test": {"id": index, "name": name}})
            if not missing or index != 0:
                events.append({"type": "testDone", "testID": index, "result": "success", "skipped": False})
        if completed:
            events.append({"type": "done", "success": True})
        with tempfile.TemporaryDirectory(prefix="consolecrypt-native-reporter-") as temp:
            fake = Path(temp) / "fake-flutter"
            fake.write_text(
                "#!/usr/bin/env python3\nimport json\n"
                f"events = {events!r}\n"
                "for event in events: print(json.dumps(event), flush=True)\n"
                f"raise SystemExit({exit_code})\n",
                encoding="utf-8",
            )
            fake.chmod(0o700)
            log, stdout = io.StringIO(), io.StringIO()
            with redirect_stdout(stdout):
                result = HARNESS.run_flutter(str(fake), SUITE, dict(os.environ), 10, log)
            recorded = log.getvalue() + stdout.getvalue() + json.dumps(result)
            self.assertFalse(marker in recorded, "untrusted diagnostic payload must never be logged")
            return result

    def test_reports_real_expected_status_without_diagnostics(self):
        result = self.run_reporter()
        self.assertTrue(result["success"])
        self.assertEqual(len(result["passed_tests"]), 2)

    def test_large_untrusted_line_does_not_hide_following_status(self):
        result = self.run_reporter(large=True)
        self.assertTrue(result["success"])

    def test_each_success_gate_is_required(self):
        for option in ({"completed": False}, {"missing": True}, {"exit_code": 1}, {"error": True}):
            with self.subTest(option=option):
                self.assertFalse(self.run_reporter(**option)["success"])

    def test_compiler_filter_rejects_runtime_auth_and_credentials(self):
        for line in (
            b"Error: Unhandled Exception: payload",
            b"Error: VM service listening http://127.0.0.1:12345/session/",
            b"Error: recovery words diagnostic",
            b"Error: arbitrary short runtime payload",
            ("Error: " + os.urandom(32).hex()).encode(),
        ):
            self.assertIsNone(HARNESS.safe_compiler_line(line, "/tmp/owned"))
        safe = HARNESS.safe_compiler_line(b"CMake Error at /tmp/owned/generated.cmake:12 (include)", "/tmp/owned")
        self.assertEqual(safe, "CMake Error at <owned-temp>/generated.cmake:12 (include)")

    def test_file_trace_cannot_emit_private_or_traversing_paths(self):
        for path in ("/tmp/owned/home/keyring", "/home/user/vault.db", "/opt/consolecrypt/../../tmp/owned"):
            line = f'openat(AT_FDCWD, "{path}", O_RDONLY) = -1 ENOENT (No such file or directory)'.encode()
            self.assertIsNone(HARNESS.known_file_failure(line))
        result = HARNESS.known_file_failure(b'openat(AT_FDCWD, "/usr/lib/libGLESv2.so.2", O_RDONLY) = -1 ENOENT (No such file or directory)')
        self.assertEqual(result, {"operation": "openat", "path": "/usr/lib/libGLESv2.so.2", "errno": "ENOENT"})

    def test_ui_diagnostics_reject_unknown_values_keys_and_non_boolean_fields(self):
        event = {
            "event": "input", "step": "newPassphrase", "app_stage": "needsVault",
            "route": "onboarding", "finder_hit": True, "hit_testable": True,
            "field_matches": False, "button_enabled": None, "failure": None,
        }
        self.assertEqual(HARNESS.safe_ui_diagnostic("CC_UI_DIAG " + json.dumps(event)), event)
        for change in (
            {"step": os.urandom(32).hex()}, {"route": "/hosts/private-id"},
            {"field_matches": "secret"}, {"field_matches": 1},
            {"unexpected": "private"}, {"failure": "private"},
        ):
            with self.subTest(keys=tuple(change)):
                self.assertIsNone(HARNESS.safe_ui_diagnostic("CC_UI_DIAG " + json.dumps(event | change)))
        self.assertIsNone(HARNESS.safe_ui_diagnostic("untrusted CC_UI_DIAG " + json.dumps(event)))


if __name__ == "__main__":
    unittest.main()
