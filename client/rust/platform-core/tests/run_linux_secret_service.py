#!/usr/bin/env python3
"""Run real keyring checks on a disposable Linux builder, never a login bus.

Requires cargo, dbus-daemon, gdbus and gnome-keyring-daemon. All daemons get an
owned temporary HOME/runtime/data tree and a bus without activation service
directories. Keyring password and test keys are generated in memory; values
are not command arguments, files or output. The encrypted keyring itself is
deleted along with its temporary directory after every case.
"""

from __future__ import annotations

import base64
import os
from pathlib import Path
import re
import selectors
import shutil
import subprocess
import sys
import tempfile
import time
from typing import Callable


def run_case(
    mode: str,
    cargo: str,
    *,
    desktop_check: Callable[[dict[str, str]], None] | None = None,
) -> None:
    with tempfile.TemporaryDirectory(prefix="consolecrypt-linux-keyring-", dir="/tmp") as temp:
        root = Path(temp)
        for name in ("home", "runtime", "data", "config", "cache", "tmp", "runtime/keyring"):
            (root / name).mkdir(mode=0o700, parents=True, exist_ok=True)
        env = dict(os.environ)
        # Override only the child process environment, not the invoking user's
        # home or bus. No daemon in this harness can reach a login keyring.
        env.update({
            "HOME": str(root / "home"),
            "XDG_RUNTIME_DIR": str(root / "runtime"),
            "XDG_DATA_HOME": str(root / "data"),
            "XDG_CONFIG_HOME": str(root / "config"),
            "XDG_CACHE_HOME": str(root / "cache"),
            "TMPDIR": str(root / "tmp"),
            "CC_LINUX_KEYRING_IT": "owned-disposable",
            "CC_LINUX_KEYRING_CASE": mode,
        })
        for name in ("DBUS_SESSION_BUS_ADDRESS", "DBUS_STARTER_ADDRESS", "GNOME_KEYRING_CONTROL", "GNOME_KEYRING_PID", "SSH_AUTH_SOCK", "DISPLAY", "WAYLAND_DISPLAY"):
            env.pop(name, None)
        # No <servicedir>: absent-service checks cannot autostart a provider.
        config = root / "bus.conf"
        config.write_text(f'''<!DOCTYPE busconfig PUBLIC "-//freedesktop//DTD D-Bus Bus Configuration 1.0//EN" "http://www.freedesktop.org/standards/dbus/1.0/busconfig.dtd">
<busconfig><type>session</type><listen>unix:tmpdir={root / "runtime"}</listen><auth>EXTERNAL</auth><policy context="default"><allow send_destination="*"/><allow receive_sender="*"/><allow own="*"/></policy></busconfig>
''')
        bus = subprocess.Popen(["dbus-daemon", "--nofork", f"--config-file={config}", "--print-address=1"], env=env, stdout=subprocess.PIPE, stderr=subprocess.DEVNULL, text=True)
        daemon = None
        try:
            with selectors.DefaultSelector() as selector:
                selector.register(bus.stdout, selectors.EVENT_READ)
                if not selector.select(10):
                    raise RuntimeError("private session bus did not start")
                address = bus.stdout.readline().strip()
            if not address.startswith("unix:") or bus.poll() is not None:
                raise RuntimeError("private session bus unavailable")
            env["DBUS_SESSION_BUS_ADDRESS"] = address
            if mode != "no-service":
                daemon_args = [
                    "gnome-keyring-daemon", "--foreground", "--components=secrets",
                    f"--control-directory={root / 'runtime/keyring'}",
                ]
                if mode == "roundtrip":
                    daemon_args.append("--unlock")
                daemon = subprocess.Popen(daemon_args, env=env, stdin=subprocess.PIPE if mode == "roundtrip" else subprocess.DEVNULL, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
                if mode == "roundtrip":
                    password = bytearray(base64.b64encode(os.urandom(32)) + b"\n")
                    try:
                        daemon.stdin.write(password)
                        daemon.stdin.close()
                    finally:
                        password[:] = b"\0" * len(password)
                deadline = time.monotonic() + 15
                while True:
                    ready = subprocess.run(["gdbus", "call", "--session", "--dest", "org.freedesktop.secrets", "--object-path", "/org/freedesktop/secrets", "--method", "org.freedesktop.DBus.Peer.Ping"], env=env, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, timeout=3).returncode == 0
                    if ready:
                        break
                    if time.monotonic() > deadline or daemon.poll() is not None:
                        raise RuntimeError("private Secret Service did not start")
                    time.sleep(0.1)
                if mode == "roundtrip":
                    # A PAM-less GNOME fixture can create the encrypted login
                    # collection without assigning the desktop default alias.
                    # Configure that alias explicitly in the TEST HARNESS;
                    # production deliberately refuses a missing default.
                    output = subprocess.check_output(["gdbus", "call", "--session", "--dest", "org.freedesktop.secrets", "--object-path", "/org/freedesktop/secrets", "--method", "org.freedesktop.Secret.Service.ReadAlias", "login"], env=env, text=True, timeout=5)
                    match = re.search(r"'(/[^']*)'", output)
                    if not match or match.group(1) == "/":
                        raise RuntimeError("private encrypted login collection unavailable")
                    subprocess.run(["gdbus", "call", "--session", "--dest", "org.freedesktop.secrets", "--object-path", "/org/freedesktop/secrets", "--method", "org.freedesktop.Secret.Service.SetAlias", "default", match.group(1)], env=env, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, check=True, timeout=5)
                elif mode == "transient-default":
                    output = subprocess.check_output(["gdbus", "call", "--session", "--dest", "org.freedesktop.secrets", "--object-path", "/org/freedesktop/secrets", "--method", "org.freedesktop.Secret.Service.ReadAlias", "session"], env=env, text=True, timeout=5)
                    match = re.search(r"'(/[^']*)'", output)
                    if not match or match.group(1) == "/":
                        raise RuntimeError("private transient collection unavailable")
                    subprocess.run(["gdbus", "call", "--session", "--dest", "org.freedesktop.secrets", "--object-path", "/org/freedesktop/secrets", "--method", "org.freedesktop.Secret.Service.SetAlias", "default", match.group(1)], env=env, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, check=True, timeout=5)
            # Don't inherit RUST_LOG: tests must not enable provider tracing.
            env.pop("RUST_LOG", None)
            if desktop_check is None:
                subprocess.run([cargo, "test", "--locked", "-p", "cc-platform-core", "--test", "linux_secret_service_it", "--", "--ignored", "--test-threads=1"], env=env, check=True, timeout=120)
            else:
                # Native Flutter acceptance reuses this exact private bus and
                # unlocked persistent keyring, without copying its password.
                desktop_check(env)
            print(f"Secret Service isolated case {mode}: PASS", flush=True)
        finally:
            for process in (daemon, bus):
                if process is not None and process.poll() is None:
                    process.terminate()
                    try:
                        process.wait(timeout=5)
                    except subprocess.TimeoutExpired:
                        process.kill()
                        process.wait(timeout=5)


def main() -> None:
    if sys.platform != "linux" or os.environ.get("CC_LINUX_DISPOSABLE_BUILDER") != "1":
        raise SystemExit("Refusing: set CC_LINUX_DISPOSABLE_BUILDER=1 only inside an owned disposable Linux builder")
    # A crashed provider/test must not serialize generated keys into a dump.
    import resource
    resource.setrlimit(resource.RLIMIT_CORE, (0, 0))
    cargo = shutil.which("cargo")
    if not cargo:
        raise SystemExit("cargo is required")
    for tool in ("dbus-daemon", "gnome-keyring-daemon", "gdbus"):
        if not shutil.which(tool):
            raise SystemExit(f"{tool} is required")
    os.chdir(Path(__file__).resolve().parents[2])
    subprocess.run([cargo, "test", "--locked", "-p", "cc-platform-core", "--test", "linux_secret_service_it", "--no-run"], check=True)
    for mode in ("no-service", "missing-default", "transient-default", "roundtrip"):
        run_case(mode, cargo)


if __name__ == "__main__":
    main()
