#!/usr/bin/env python3
"""Supply a private disposable fixture and exercise Android's real Stop action.

Start integration_test/kubernetes_android_test.dart with its documented defines,
then run this helper. Nothing reads ~/.kube/config or embeds credentials in an
APK. Only an explicitly named emulator is supported.
"""
from __future__ import annotations

import argparse
import json
import os
from pathlib import Path
import re
import subprocess
import time
import urllib.request
import xml.etree.ElementTree as ET

PACKAGE = "io.consolecrypt.consolecrypt"


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--adb", required=True, type=Path)
    parser.add_argument("--device", required=True)
    parser.add_argument("--kubeconfig", required=True, type=Path)
    parser.add_argument("--output", required=True, type=Path)
    parser.add_argument("--timeout", type=int, default=1200)
    options = parser.parse_args()
    if not re.fullmatch(r"emulator-\d+", options.device):
        parser.error("an explicit emulator device is required")
    options.output.mkdir(parents=True, exist_ok=True)
    os.chmod(options.output, 0o700)
    deadline = time.monotonic() + options.timeout

    def adb(*args: str, data: bytes | None = None, check: bool = True) -> subprocess.CompletedProcess:
        return subprocess.run(
            [str(options.adb), "-s", options.device, *args],
            input=data, capture_output=True, check=check, timeout=20,
        )

    def wait_until(predicate, message: str):
        while time.monotonic() < deadline:
            result = predicate()
            if result:
                return result
            time.sleep(0.5)
        raise RuntimeError(message)

    # Reject an unexpected file shape without displaying its private contents.
    source = options.kubeconfig.read_text()
    source, changed = re.subn(
        r"(?m)^(\s*)server:\s*[^\n]+$",
        r"\1server: https://10.0.2.2:16443",
        source,
    )
    if changed != 1:
        raise RuntimeError("fixture must contain exactly one API server")
    if re.search(r"(?m)^\s*tls-server-name:", source):
        source = re.sub(
            r"(?m)^(\s*)tls-server-name:\s*[^\n]+$",
            r"\1tls-server-name: cc-kube-acceptance.local", source,
        )
    else:
        source = re.sub(
            r"(?m)^(\s*)server: ([^\n]+)$",
            r"\1server: \2\n\1tls-server-name: cc-kube-acceptance.local", source,
        )
    wait_until(
        lambda: adb("shell", "run-as", PACKAGE, "pwd", check=False).returncode == 0,
        "debug app was not installed",
    )
    adb("shell", "pm", "grant", PACKAGE, "android.permission.POST_NOTIFICATIONS")
    # Input flows over stdin directly to a private file; no credential appears
    # in argv, environment, logs, external storage, or shell interpolation.
    adb(
        "shell", "run-as", PACKAGE, "sh", "-c",
        "'umask 077; mkdir -p files; cat > files/kube-acceptance.yaml'",
        data=source.encode(),
    )
    source = ""
    print("Private fixture supplied; waiting for the native forward.", flush=True)
    ready_path = "no_backup/consolecrypt/kube-smoke-ready.json"

    def ready():
        result = adb("shell", "run-as", PACKAGE, "cat", ready_path, check=False)
        if result.returncode:
            return None
        try:
            value = json.loads(result.stdout)
        except ValueError:
            return None
        return value if value.get("stage") == "notification" else None

    state = wait_until(ready, "native smoke did not reach the notification stage")
    address = state.get("address", "")
    if not re.fullmatch(r"127\.0\.0\.1:\d{1,5}", address):
        raise RuntimeError("unexpected forward address")
    port = int(address.rsplit(":", 1)[1])
    host_port = adb("forward", "tcp:0", f"tcp:{port}").stdout.decode().strip()
    if not host_port.isdecimal():
        raise RuntimeError("ADB did not allocate a test port")
    try:
        # Home backgrounds the Activity while the native service keeps the
        # socket alive; prove traffic still reaches the isolated fixture.
        adb("shell", "input", "keyevent", "KEYCODE_HOME")
        time.sleep(2)
        with urllib.request.urlopen(f"http://127.0.0.1:{host_port}/", timeout=10) as response:
            if response.status != 200:
                raise RuntimeError("forward stopped while app was backgrounded")
        services = adb("shell", "dumpsys", "activity", "services", PACKAGE).stdout.decode()
        if "KubeForwardService" not in services or "isForeground=true" not in services:
            raise RuntimeError("forward service is not foreground")
        adb("shell", "cmd", "statusbar", "expand-notifications")
        time.sleep(1)
        adb("shell", "uiautomator", "dump", "/sdcard/cc-notification.xml")
        snapshot = adb("shell", "cat", "/sdcard/cc-notification.xml").stdout
        root = ET.fromstring(snapshot)
        buttons = [node for node in root.iter("node") if node.get("text", "").lower() == "stop tunnels"]
        if len(buttons) != 1:
            raise RuntimeError("ConsoleCrypt notification Stop action was not visible")
        match = re.fullmatch(r"\[(\d+),(\d+)\]\[(\d+),(\d+)\]", buttons[0].get("bounds", ""))
        if not match:
            raise RuntimeError("notification action bounds unavailable")
        x1, y1, x2, y2 = map(int, match.groups())
        screenshot = adb("exec-out", "screencap", "-p").stdout
        (options.output / "foreground-notification.png").write_bytes(screenshot)
        adb("shell", "input", "tap", str((x1 + x2) // 2), str((y1 + y2) // 2))
        time.sleep(1)
        adb("shell", "cmd", "statusbar", "collapse")
        adb("shell", "am", "start", "-n", f"{PACKAGE}/.MainActivity")
        print("Background HTTP + foreground notification verified; Stop pressed.", flush=True)
        (options.output / "notification-result.json").write_text(json.dumps({
            "background_http": True, "foreground_service": True,
            "notification_stop_pressed": True,
        }) + "\n")
    finally:
        adb("forward", "--remove", f"tcp:{host_port}", check=False)
        adb("shell", "rm", "-f", "/sdcard/cc-notification.xml", check=False)


if __name__ == "__main__":
    main()
