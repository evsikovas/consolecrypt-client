import importlib.util
from pathlib import Path
from tempfile import TemporaryDirectory
from types import SimpleNamespace
import subprocess
import unittest
from unittest.mock import patch

spec = importlib.util.spec_from_file_location("sign_macos", Path(__file__).with_name("sign-macos.py"))
signing = importlib.util.module_from_spec(spec)
spec.loader.exec_module(signing)
CERT_A = "A" * 40
CERT_B = "B" * 40
IDENTITIES = f'  1) {CERT_A} "Developer ID Application: Test (TEAM)"\n  2) {CERT_B} "Local test"\n'


class SigningTests(unittest.TestCase):
    def test_exact_identity_or_fingerprint(self):
        with patch.object(signing.subprocess, "run", return_value=SimpleNamespace(stdout=IDENTITIES)):
            self.assertEqual(signing.resolve_identity("Developer ID Application: Test (TEAM)"), CERT_A)
            self.assertEqual(signing.resolve_identity(CERT_A.lower()), CERT_A)
            for invalid in ["-", "", "Developer ID", "missing"]:
                with self.assertRaises(ValueError):
                    signing.resolve_identity(invalid)

    def test_missing_and_ambiguous_identity(self):
        for output in ["0 valid identities found", IDENTITIES.replace("Local test", "Developer ID Application: Test (TEAM)")]:
            with patch.object(signing.subprocess, "run", return_value=SimpleNamespace(stdout=output)):
                with self.assertRaises(ValueError):
                    signing.resolve_identity("Developer ID Application: Test (TEAM)")

    def test_inside_out_signing_preserves_entitlements_but_replaces_adhoc_requirement(self):
        with TemporaryDirectory() as temp:
            app = Path(temp) / "ConsoleCrypt.app"
            framework = app / "Contents/Frameworks/Engine.framework"
            library = framework / "Versions/A/Engine"
            library.parent.mkdir(parents=True)
            library.write_bytes(bytes.fromhex("feedfacf") + b"test")
            try:
                (framework / "Engine").symlink_to("Versions/A/Engine")
            except OSError as error:
                if getattr(error, 'winerror', None) == 1314:
                    self.skipTest('Windows requires Developer Mode or symlink privilege')
                raise
            (app / "Contents/data.txt").write_text("resource")
            with patch.object(signing.subprocess, "run") as run:
                signing.sign(app, CERT_A)
            commands = [call.args[0] for call in run.call_args_list]
            self.assertEqual([command[-1] for command in commands[:-1]], [str(library), str(framework), str(app)])
            for command in commands[:-1]:
                self.assertIn("--preserve-metadata=identifier,entitlements", command)
                self.assertNotIn("--deep", command)
                self.assertNotIn("requirements", " ".join(command))
            self.assertIn("--verify", commands[-1])
            self.assertIn("--strict", commands[-1])

    def test_failed_signing_stops_before_verification(self):
        with TemporaryDirectory() as temp:
            app = Path(temp) / "Test.app"
            app.mkdir()
            with patch.object(signing.subprocess, "run", side_effect=subprocess.CalledProcessError(1, "codesign")) as run:
                with self.assertRaises(subprocess.CalledProcessError):
                    signing.sign(app, CERT_A)
                self.assertEqual(run.call_count, 1)


if __name__ == "__main__":
    unittest.main()
