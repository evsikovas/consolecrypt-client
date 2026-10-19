import importlib.util
from pathlib import Path
import plistlib
from types import SimpleNamespace
import unittest
from unittest.mock import patch


SPEC = importlib.util.spec_from_file_location("macos_process_smoke", Path(__file__).with_name("smoke-macos-processes.py"))
smoke = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(smoke)
RUNNER = Path(__file__).parents[1] / "flutter/macos/Runner"


class MacProcessPolicyTests(unittest.TestCase):
    def test_current_release_debug_and_helper_policies_allow_full_local_processes(self):
        helper = plistlib.loads(Path(__file__).with_name("local-ai-helper.entitlements").read_bytes())
        for filename in ("Release.entitlements", "DebugProfile.entitlements"):
            host = plistlib.loads((RUNNER / filename).read_bytes())
            smoke.check_process_policy(host, helper)

    def test_stale_sandbox_helper_or_weakened_library_validation_blocks_packaging(self):
        for host, helper in [
            ({"com.apple.security.app-sandbox": True}, {}),
            ({"com.apple.security.inherit": True}, {}),
            ({"com.apple.security.cs.disable-library-validation": True}, {}),
            ({}, {"com.apple.security.app-sandbox": True, "com.apple.security.inherit": True}),
            ({}, {"com.apple.security.cs.disable-library-validation": True}),
        ]:
            with self.subTest(host=host, helper=helper), self.assertRaises(ValueError):
                smoke.check_process_policy(host, helper)

    def test_codesign_empty_entitlements_and_explicit_dictionary_are_supported(self):
        for output in (b"", plistlib.dumps({})):
            with patch.object(smoke, "checked", return_value=SimpleNamespace(stdout=output)):
                self.assertEqual(smoke.read_entitlements(Path("Fixture.app")), {})
        stale = {"com.apple.security.app-sandbox": True}
        with patch.object(smoke, "checked", return_value=SimpleNamespace(stdout=plistlib.dumps(stale))):
            self.assertEqual(smoke.read_entitlements(Path("Fixture.app")), stale)


if __name__ == "__main__":
    unittest.main()
