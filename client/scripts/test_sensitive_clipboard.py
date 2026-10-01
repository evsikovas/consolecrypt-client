#!/usr/bin/env python3
"""Compile the production native clipboard handlers against recording OS fakes.

No application, real pasteboard, Android device or Keychain is accessed. These
tests verify handler options/validation; physical OS capture/Handoff behavior
still needs an isolated device check. Kotlin requires kotlinc on PATH, or a
preinstalled compiler classpath supplied via CC_KOTLIN_COMPILER_CLASSPATH.
"""
from pathlib import Path
import os
import shutil
import subprocess
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[2]
ANDROID = ROOT / "client/flutter/android/app/src/main/kotlin/io/consolecrypt/consolecrypt/MainActivity.kt"
IOS = ROOT / "client/flutter/rust_builder/ios/Classes/CcBridgePlugin.swift"


class NativeClipboardTests(unittest.TestCase):
    def run_compiler(self, command, cwd):
        result = subprocess.run(command, cwd=cwd, text=True, capture_output=True)
        self.assertEqual(result.returncode, 0, result.stderr)

    @unittest.skipUnless(shutil.which("swiftc"), "Swift compiler unavailable")
    def test_ios_native_copy_is_local_expiring_and_validates_arguments(self):
        source = IOS.read_text()
        self.assertIn('"consolecrypt/clipboard"', source)
        self.assertIn('case "copySecret":', source)
        handler = source.split("  private func copySecret(", 1)[1].split("  private func availability()", 1)[0]
        handler = "  func copySecret(" + handler
        program = r'''
import Foundation
import CoreFoundation
typealias FlutterResult = (Any?) -> Void
struct FlutterError { let code: String; let message: String; let details: Any? }
final class UIPasteboard {
  enum OptionsKey: Hashable { case localOnly, expirationDate }
  static let general = UIPasteboard()
  var calls = 0
  var items: [[String: Any]] = []
  var options: [OptionsKey: Any] = [:]
  func setItems(_ items: [[String: Any]], options: [OptionsKey: Any]) {
    calls += 1; self.items = items; self.options = options
  }
}
final class Handler {
''' + handler + r'''
}
let handler = Handler()
let secret = UUID().uuidString
let start = Date()
var completions = 0
handler.copySecret(arguments: ["text": secret, "clearAfterMilliseconds": 30000]) { result in
  precondition(result == nil); completions += 1
}
precondition(completions == 1 && UIPasteboard.general.calls == 1)
precondition(UIPasteboard.general.items.first?["public.utf8-plain-text"] as? String == secret)
precondition(UIPasteboard.general.options[.localOnly] as? Bool == true)
let expires = UIPasteboard.general.options[.expirationDate] as! Date
precondition(expires.timeIntervalSince(start) >= 30 && expires.timeIntervalSince(start) < 31)
let invalid: [[String: Any]?] = [nil, [:], ["text": 1, "clearAfterMilliseconds": 1000],
  ["text": secret, "clearAfterMilliseconds": true], ["text": secret, "clearAfterMilliseconds": 0],
  ["text": secret, "clearAfterMilliseconds": -1], ["text": secret, "clearAfterMilliseconds": 86400001],
  ["text": secret, "clearAfterMilliseconds": "30000"], ["text": secret, "clearAfterMilliseconds": 0.5],
  ["text": secret, "clearAfterMilliseconds": Double.infinity]]
for args in invalid {
  handler.copySecret(arguments: args) { result in
    let error = result as! FlutterError
    precondition(error.code == "bad_args" && error.details == nil && !error.message.contains(secret))
  }
}
precondition(UIPasteboard.general.calls == 1)
for ttl in [1, 86400000] {
  handler.copySecret(arguments: ["text": secret, "clearAfterMilliseconds": ttl]) { precondition($0 == nil) }
}
precondition(UIPasteboard.general.calls == 3)
'''
        with tempfile.TemporaryDirectory(prefix="cc-clipboard-swift-") as temp:
            root = Path(temp)
            (root / "main.swift").write_text(program)
            self.run_compiler(["swiftc", "-module-cache-path", str(root / "cache"), "main.swift", "-o", "test"], root)
            self.run_compiler([str(root / "test")], root)

    def test_android_native_copy_is_sensitive_and_rejects_invalid_requests(self):
        compiler = shutil.which("kotlinc")
        classpath = os.environ.get("CC_KOTLIN_COMPILER_CLASSPATH")
        java = shutil.which("java")
        if not compiler and not (classpath and java):
            self.skipTest("Kotlin compiler unavailable; supply CC_KOTLIN_COMPILER_CLASSPATH")
        source = ANDROID.read_text()
        handler = source.split('"consolecrypt/clipboard").setMethodCallHandler { call, result ->', 1)[1]
        handler = handler.split('        MethodChannel(flutterEngine.dartExecutor.binaryMessenger, "consolecrypt/updates")', 1)[0]
        # Retain the exact production lambda body (including its early returns).
        program = r'''
import java.util.UUID
class PersistableBundle {
  val flags = mutableMapOf<String, Boolean>()
  fun putBoolean(key: String, value: Boolean) { flags[key] = value }
}
class Description { var extras: PersistableBundle? = null }
class ClipData(val label: String, val text: String) {
  val description = Description()
  companion object { fun newPlainText(label: String, text: String) = ClipData(label, text) }
}
class ClipboardManager {
  var calls = 0; var clip: ClipData? = null; var fail = false
  fun setPrimaryClip(value: ClipData) { if (fail) throw IllegalStateException(); calls += 1; clip = value }
}
class Call(val method: String, val args: Map<String, Any?>) {
  @Suppress("UNCHECKED_CAST") fun <T> argument(key: String): T? = args[key] as T?
}
class Result {
  var completions = 0; var code: String? = null; var message: String? = null; var details: Any? = null
  fun success(value: Any?) { check(value == null); completions += 1 }
  fun error(code: String, message: String, details: Any?) {
    this.code = code; this.message = message; this.details = details; completions += 1
  }
  fun notImplemented() { code = "notImplemented"; completions += 1 }
}
class Handler {
  val clipboard = ClipboardManager()
  @Suppress("UNCHECKED_CAST") fun <T> getSystemService(type: Class<T>): T = clipboard as T
  fun setMethodCallHandler(block: (Call, Result) -> Unit, call: Call, result: Result) { block(call, result) }
  fun handle(call: Call, result: Result) = setMethodCallHandler({ call, result ->
''' + handler.rstrip()[:-1] + r'''
  }, call, result)
}
fun main() {
  val handler = Handler(); val secret = UUID.randomUUID().toString()
  fun request(ttl: Any?, text: Any? = secret, method: String = "copySecret"): Result {
    val result = Result(); handler.handle(Call(method, mapOf("text" to text, "clearAfterMilliseconds" to ttl)), result)
    check(result.completions == 1); return result
  }
  check(request(30000).code == null && handler.clipboard.calls == 1)
  check(handler.clipboard.clip!!.text == secret && handler.clipboard.clip!!.label.isEmpty())
  check(handler.clipboard.clip!!.description.extras!!.flags["android.content.extra.IS_SENSITIVE"] == true)
  for (ttl in listOf(null, 0, -1, 86400001L, true, "30000", 1.5)) {
    val result = request(ttl); check(result.code == "bad_args" && result.details == null)
    check(!result.message!!.contains(secret))
  }
  check(request(1000, null).code == "bad_args" && request(1000, 1).code == "bad_args")
  check(request(1000, method = "unknown").code == "notImplemented" && handler.clipboard.calls == 1)
  check(request(1).code == null && request(86400000L).code == null && handler.clipboard.calls == 3)
  handler.clipboard.fail = true
  val failure = request(30000)
  check(failure.code == "clipboard" && failure.details == null && !failure.message!!.contains(secret))
}
'''
        with tempfile.TemporaryDirectory(prefix="cc-clipboard-kotlin-") as temp:
            root = Path(temp)
            (root / "test.kt").write_text(program)
            if compiler:
                command = [compiler, "test.kt", "-include-runtime", "-d", "test.jar"]
                runtime = [java or "java", "-jar", "test.jar"]
            else:
                command = [java, "-cp", classpath, "org.jetbrains.kotlin.cli.jvm.K2JVMCompiler", "-no-stdlib",
                           "-no-reflect", "-classpath", classpath, "test.kt", "-d", "test.jar"]
                runtime = [java, "-cp", os.pathsep.join(["test.jar", classpath]), "TestKt"]
            self.run_compiler(command, root)
            self.run_compiler(runtime, root)


if __name__ == "__main__":
    unittest.main()
