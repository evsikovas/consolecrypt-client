#!/usr/bin/env python3
"""Exercise the production Swift exporter using generated, nonsecret bytes.

Does not launch ConsoleCrypt, show dialogs, alter quarantine or use Keychain.
The desktop process policy is checked separately in an isolated test app;
these tests cover integrity and exclusive writes on real files.
"""
from pathlib import Path
import plistlib
import shutil
import subprocess
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[2]
RUNNER = ROOT / "client/flutter/macos/Runner"


class MacOSUpdateTests(unittest.TestCase):
    @unittest.skipUnless(shutil.which("swiftc"), "Swift compiler unavailable")
    def test_verified_export_preserves_existing_files_rejects_links_and_cleans_corrupt_output(self):
        source = (RUNNER / "UpdateBridge.swift").read_text()
        core = source.split("final class UpdateBridge", 1)[0].replace("import FlutterMacOS\n", "")
        program = core + r'''
let manager = FileManager.default
let root = URL(fileURLWithPath: CommandLine.arguments[1], isDirectory: true)
let source = root.appendingPathComponent("cache.dmg")
let destination = root.appendingPathComponent("approved.dmg")
// Multiple chunks: exercise streaming rather than only a tiny fixed fixture.
let data = Data((0..<180000).map { UInt8($0 % 251) })
let hash = SHA256.hash(data: data).map { String(format: "%02x", $0) }.joined()
func checkDestination() throws {
  let result = try Data(contentsOf: destination)
  precondition(result == data)
}
try data.write(to: source)
try VerifiedUpdateExport.write(source: source, destination: destination, bytes: Int64(data.count), sha256: hash)
try checkDestination()
do {
  try VerifiedUpdateExport.write(source: source, destination: destination, bytes: Int64(data.count), sha256: hash)
  fatalError("overwrote an existing file")
} catch UpdateExportError.destinationExists { }
try checkDestination()
let linked = root.appendingPathComponent("linked.dmg")
try manager.createSymbolicLink(at: linked, withDestinationURL: destination)
do {
  try VerifiedUpdateExport.write(source: source, destination: linked, bytes: Int64(data.count), sha256: hash)
  fatalError("followed a destination link")
} catch UpdateExportError.destinationExists { }
try checkDestination()
let corrupt = root.appendingPathComponent("corrupt.dmg")
do {
  try VerifiedUpdateExport.write(source: source, destination: corrupt, bytes: Int64(data.count), sha256: String(repeating: "0", count: 64))
  fatalError("accepted corrupt bytes")
} catch UpdateExportError.checksum { }
precondition(!manager.fileExists(atPath: corrupt.path))
do {
  try VerifiedUpdateExport.write(source: source, destination: corrupt, bytes: 3, sha256: hash)
  fatalError("accepted the wrong size")
} catch UpdateExportError.checksum { }
precondition(!manager.fileExists(atPath: corrupt.path))
do {
  try VerifiedUpdateExport.write(source: linked, destination: corrupt, bytes: Int64(data.count), sha256: hash)
  fatalError("followed a source link")
} catch UpdateExportError.storage { }
precondition(!manager.fileExists(atPath: corrupt.path))
print("Verified macOS export: streaming, checksum, existing file, source/destination links, cleanup PASS")
'''
        with tempfile.TemporaryDirectory(prefix="cc-macos-update-") as temp:
            root = Path(temp)
            (root / "main.swift").write_text(program)
            for command in (["swiftc", "-module-cache-path", str(root / "cache"),
                             "main.swift", "-o", "test"], [str(root / "test"), str(root)]):
                result = subprocess.run(command, cwd=root, capture_output=True, text=True)
                self.assertEqual(result.returncode, 0, result.stderr)

    def test_full_shell_policy_keeps_hardened_runtime_and_verified_update_export(self):
        for name in ["Release.entitlements", "DebugProfile.entitlements"]:
            with (RUNNER / name).open("rb") as file:
                settings = plistlib.load(file)
            self.assertNotIn("com.apple.security.app-sandbox", settings)
            self.assertNotIn("com.apple.security.inherit", settings)
            self.assertNotIn("com.apple.security.cs.disable-library-validation", settings)
            self.assertEqual(settings.get("com.apple.security.cs.allow-jit", False), name == "DebugProfile.entitlements")
        bridge = (RUNNER / "UpdateBridge.swift").read_text()
        self.assertNotIn("removexattr", bridge)
        self.assertNotIn("com.apple.quarantine", bridge)
        self.assertNotIn("copyItem", bridge)


if __name__ == "__main__":
    unittest.main()
