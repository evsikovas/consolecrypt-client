#!/usr/bin/env python3
"""Exercise fullscreen toolbar restoration with real AppKit window objects."""
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[2]
WINDOW = ROOT / "client/flutter/macos/Runner/MainFlutterWindow.swift"


class MacOSFullscreenTests(unittest.TestCase):
    @unittest.skipUnless(sys.platform == "darwin" and shutil.which("swiftc"), "Requires macOS AppKit and Swift")
    def test_toolbar_restoration(self):
        source = WINDOW.read_text()
        core = source.split("final class FullscreenToolbar {", 1)[1].split(
            "/// Reconciles asynchronous AppKit transitions", 1
        )[0]
        program = "import Cocoa\nfinal class FullscreenToolbar {" + core + r'''
let app = NSApplication.shared
let window = NSWindow(contentRect: NSRect(x: 0, y: 0, width: 960, height: 600),
                      styleMask: [.titled, .closable, .resizable, .fullSizeContentView],
                      backing: .buffered, defer: false)
let toolbar = NSToolbar(identifier: "fullscreen-test")
window.toolbar = toolbar
toolbar.isVisible = true
let chrome = FullscreenToolbar()

// Ordinary fullscreen: repeated did-enter / watchdog notifications are idempotent.
chrome.hide(in: window)
precondition(!toolbar.isVisible)
chrome.hide(in: window)
chrome.restore(in: window)
precondition(toolbar.isVisible)
precondition(window.toolbar === toolbar)

// Cancelled entry takes the same restoration path as a successful exit.
chrome.hide(in: window)
chrome.restore(in: window)
precondition(toolbar.isVisible)

// RDP can hide the toolbar before native entry; its lease restores it afterwards.
toolbar.isVisible = false
chrome.hide(in: window)
chrome.hide(in: window)
chrome.restore(in: window)
precondition(!toolbar.isVisible)
toolbar.isVisible = true

// A settings change must not resurrect a removed toolbar or alter a replacement.
chrome.hide(in: window)
window.toolbar = nil
chrome.restore(in: window)
precondition(window.toolbar == nil)
window.toolbar = toolbar
toolbar.isVisible = true
chrome.hide(in: window)
let replacement = NSToolbar(identifier: "replacement")
window.toolbar = replacement
replacement.isVisible = true
chrome.hide(in: window)
chrome.restore(in: window)
precondition(replacement.isVisible)
print("Fullscreen toolbar: normal exit, repeated notifications, cancelled entry, RDP, replacement PASS")
'''
        with tempfile.TemporaryDirectory(prefix="cc-macos-fullscreen-") as temp:
            root = Path(temp)
            (root / "main.swift").write_text(program)
            for command in (["swiftc", "-module-cache-path", str(root / "cache"),
                             "main.swift", "-o", "test"], [str(root / "test")]):
                result = subprocess.run(command, cwd=root, capture_output=True, text=True, timeout=120)
                self.assertEqual(result.returncode, 0, result.stdout + result.stderr)


if __name__ == "__main__":
    unittest.main()
