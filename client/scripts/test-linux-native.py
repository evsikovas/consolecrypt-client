#!/usr/bin/env python3
"""Real Linux Flutter/FRB acceptance inside an owned disposable builder.

Uses an ephemeral encrypted GNOME login keyring, private D-Bus and its own
software-rendered Xvfb display. Never run against a user's desktop session.
Only allowlisted JSON test status is logged: failure diagnostics can contain
generated credentials/recovery words and are deliberately discarded.
"""

from __future__ import annotations

import argparse
from contextlib import contextmanager
import ctypes
import ctypes.util
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import re
import selectors
import shutil
import signal
import struct
import subprocess
import sys
import time
import zlib


SCRIPT = Path(__file__).resolve()
ROOT = SCRIPT.parents[2] if len(SCRIPT.parents) > 2 else SCRIPT.parent
SUITES = {
    "integration_test/rust_core_test.dart": {
        "local profile → host with inline password → lock/unlock → restart → backup",
        "core-gaps: reveal, planner preview, snippets, errors, SFTP browser, prompts, backups",
    },
    "integration_test/app_persistence_test.dart": {
        "create a local vault in the app, restart → Vault Unlock, data intact",
    },
}
MAX_LINE_BYTES = 256 * 1024
FAILURE_STAGES = {
    "private session bus did not start": "private_bus_start_timeout",
    "private session bus unavailable": "private_bus_unavailable",
    "private Secret Service did not start": "private_keyring_start_timeout",
    "private encrypted login collection unavailable": "private_persistent_collection_missing",
    "private transient collection unavailable": "private_transient_collection_missing",
    "isolated Xvfb startup timeout": "xvfb_start_timeout",
    "isolated Xvfb unavailable": "xvfb_unavailable",
    "isolated Xlib unavailable": "xlib_unavailable",
    "isolated Xvfb connection failed": "xvfb_connection_failed",
    "installed application exited before first frame": "app_exit_before_first_frame",
    "installed application exited after first frame": "app_exit_after_first_frame",
    "installed Flutter/Rust libraries not mapped": "native_library_not_mapped",
    "installed first-frame timeout": "first_frame_timeout",
    "isolated first-frame image unavailable": "first_frame_capture_failed",
    "unexpected isolated Xvfb image format": "first_frame_capture_format",
    "isolated window dimensions out of bounds": "first_frame_capture_dimensions",
    "native Flutter suite failed; raw diagnostics suppressed": "native_flutter_suite_failed",
    "real Rust FFI library missing": "native_test_library_missing",
}
INSTALLED_DIAGNOSTICS = {
    b"cannot open display": "display_unavailable",
    b"Failed to initialize GTK": "gtk_initialization",
    b"error while loading shared libraries": "dynamic_loader_error",
    b"GLIBC_": "glibc_requirement",
    b"GLIBCXX_": "libstdcxx_requirement",
    b"Failed to create GL context": "gl_context_failure",
    b"Unable to create a GL context": "gl_context_failure",
    b"EGL": "egl_diagnostic",
    b"libGL error": "libgl_error",
    b"libGLESv2.so.2": "gles_runtime_library",
    b"Failed to start Flutter engine": "flutter_engine_start",
    b"Failed to load dynamic library": "ffi_load_error",
    b"No such file or directory": "missing_runtime_file",
    b"Permission denied": "runtime_permission",
    b"filesystem error": "filesystem_path_error",
    b"/proc/self/exe": "proc_self_executable_path",
    b"rosetta": "translated_executable_path",
    b"libapp.so": "aot_library_path",
    b"flutter_assets": "flutter_assets_path",
    b"icudtl.dat": "icu_data_path",
    b"Failed to load": "runtime_load_failure",
    b"Could not prepare isolate": "dart_isolate_preparation",
    b"Could not set up": "native_setup_failure",
    b"Check failed": "native_check_failure",
    b"Fatal": "native_fatal",
    b"FATAL": "native_fatal",
    b"Failed to initialize": "native_initialize_failure",
    b"Failed to build Linux application": "native_build_failure",
    b"Failed to load": "runtime_load_failure",
    b"Error waiting for a debug connection": "debug_connection_failure",
    b"Failed to connect to the VM service": "debug_connection_failure",
    b"GLib-GIO-ERROR": "glib_gio_fatal",
    b"GLib-GIO-CRITICAL": "glib_gio_critical",
    b"Gtk-ERROR": "gtk_fatal",
    b"Gdk-ERROR": "gdk_fatal",
    b"dbus-launch": "dbus_launch_helper",
    b"Failed to execute child process": "native_child_exec_failure",
    b"Unable to execute": "native_exec_failure",
    b"GSettings": "gsettings_diagnostic",
    b"schemas": "settings_schema_diagnostic",
    b"std::system_error": "native_system_error",
    b"std::filesystem": "native_filesystem_error",
    b"The VM Service is not available": "vm_service_unavailable",
    b"Connecting to the VM Service timed out": "vm_service_connect_timeout",
    b"Unable to start the app on the device": "integration_app_start_failed",
    b"Dart Development Service": "debug_dds",
    b"SocketException": "debug_socket_exception",
    b"ProcessException": "native_process_exception",
    b"TimeoutException": "debug_timeout_exception",
    b"Service protocol connection closed": "debug_protocol_closed",
    b"Compilation failed": "dart_compilation_failed",
    b"Failed assertion": "flutter_assertion",
    b"Assertion failed": "flutter_assertion",
    b"Bad state: No element": "dart_no_element",
    b"Bad state: Too many elements": "dart_multiple_elements",
    b"Null check operator used on a null value": "dart_null_check",
    b"Multiple exceptions": "flutter_multiple_exceptions",
    b"A RenderFlex overflowed": "flutter_layout_overflow",
    b"Unable to load asset": "flutter_asset_missing",
    b"Cannot use \"ref\" after the widget was disposed": "provider_disposed",
    b"unmounted": "widget_unmounted",
    b"StateError": "dart_state_error",
    b"TestFailure": "test_assertion_failure",
}
STATIC_FRAMEWORK_WORDS = frozenset((
    "GLib-GIO-ERROR GLib-GIO-CRITICAL GLib-ERROR Gtk-ERROR Gtk-WARNING Gdk-ERROR "
    "ERROR Error error Failed failed Unable unable to execute child process "
    "dbus-launch dbus-daemon No such file or directory Check Fatal FATAL "
    "GSettings schema schemas are not installed GTK GDK initialize settings "
    "Unable prepare isolate AOT ICU snapshot Flutter engine g_application_register "
    "g_spawn_async g_spawn_sync g_dbus_connection_new_for_address_sync "
    "std::system_error std::filesystem libapp.so libcc_bridge.so "
    "libflutter_linux_gtk.so icudtl.dat kernel_blob.bin"
).split())
UI_STEPS = (
    "welcome-local", "new-passphrase", "confirm-passphrase", "create-vault",
    "reveal-kit", "kit-saved", "kit-continue", "verify-submit",
    "local-notice-continue", "nav-hosts", "unlock-passphrase", "unlock-submit",
)
UI_DIAGNOSTIC_VALUES = {
    "event": frozenset("begin found tap input timeout failed flutterError layoutOverflow complete".split()),
    "step": frozenset("setup openCore renderApp welcomeLocal newPassphrase confirmPassphrase createVault revealKit kitSaved kitContinue verifyReady verifyWord verifySubmit localNotice hostsReady saveHost hostVisible reopenPreference closeUi closeCore reopenCore rerenderApp unlockReady unlockPassphrase unlockSubmit restoredHost complete".split()),
    "app_stage": frozenset("unavailable loading welcome signedOut needsVault recoveryKitPending locked awaitingApproval unlocked".split()),
    "route": frozenset("unavailable loading welcome onboarding recoveryKit verifyKit localNotice unlock hosts other".split()),
    "failure": frozenset("timeout finderCount notHitTestable buttonDisabled fieldInput kitMissing scenario".split()),
}
UI_DIAGNOSTIC_BOOLS = frozenset(("finder_hit", "hit_testable", "field_matches", "button_enabled"))


def safe_ui_diagnostic(message: str) -> dict | None:
    """Only reconstruct a closed-vocabulary diagnostic; never return raw text."""
    prefix = "CC_UI_DIAG "
    if not message.startswith(prefix) or len(message) > 2048:
        return None
    try:
        event = json.loads(message[len(prefix):])
    except ValueError:
        return None
    if not isinstance(event, dict) or event.keys() != UI_DIAGNOSTIC_VALUES.keys() | UI_DIAGNOSTIC_BOOLS:
        return None
    for key, allowed in UI_DIAGNOSTIC_VALUES.items():
        value = event[key]
        if value is None and key == "failure":
            continue
        if not isinstance(value, str) or value not in allowed:
            return None
    if any(event[key] is not None and type(event[key]) is not bool for key in UI_DIAGNOSTIC_BOOLS):
        return None
    return {key: event[key] for key in sorted(event)}


class InstalledSmokeFailure(RuntimeError):
    def __init__(self, stage: str, exit_code: int | None, diagnostics: bytearray, context: dict | None = None):
        super().__init__(stage)
        self.safe_details = {
            "stage": stage, "app_exit_code": exit_code,
            "diagnostic_signals": sorted({code for token, code in INSTALLED_DIAGNOSTICS.items() if token in diagnostics}),
            "static_framework_tokens": [word for word in re.findall(r"[A-Za-z_][-A-Za-z0-9_:.]*", diagnostics.decode("ascii", errors="ignore")) if word in STATIC_FRAMEWORK_WORDS][:64],
        }
        if context is not None:
            self.safe_details.update(context)


def sha256(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def stop_group(process: subprocess.Popen) -> None:
    """Stop only the process group created by this harness, including children."""
    try:
        os.killpg(process.pid, signal.SIGTERM)
    except ProcessLookupError:
        return
    try:
        process.wait(timeout=5)
    except subprocess.TimeoutExpired:
        pass
    # The group leader can exit before its compiler/native-app children.
    try:
        os.killpg(process.pid, signal.SIGKILL)
    except ProcessLookupError:
        pass
    process.wait(timeout=5)


@contextmanager
def private_display(xvfb_tool: str, env: dict[str, str]):
    read_fd, write_fd = os.pipe()
    try:
        xvfb = subprocess.Popen([
            xvfb_tool, "-displayfd", str(write_fd), "-screen", "0", "1600x1000x24",
            "-nolisten", "tcp", "-noreset", "-ac",
        ], env=env, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
            pass_fds=(write_fd,), start_new_session=True)
    finally:
        os.close(write_fd)
    try:
        with selectors.DefaultSelector() as selector:
            selector.register(read_fd, selectors.EVENT_READ)
            if not selector.select(10):
                raise RuntimeError("isolated Xvfb startup timeout")
            display = os.read(read_fd, 64).strip()
        if not display.isdigit() or xvfb.poll() is not None:
            raise RuntimeError("isolated Xvfb unavailable")
        env["DISPLAY"] = ":" + display.decode("ascii")
        yield
    finally:
        os.close(read_fd)
        stop_group(xvfb)


class XWindowAttributes(ctypes.Structure):
    _fields_ = [
        (name, ctypes.c_int) for name in ("x", "y", "width", "height", "border_width", "depth")
    ] + [
        ("visual", ctypes.c_void_p), ("root", ctypes.c_ulong),
    ] + [
        (name, ctypes.c_int) for name in ("class_", "bit_gravity", "win_gravity", "backing_store")
    ] + [
        ("backing_planes", ctypes.c_ulong), ("backing_pixel", ctypes.c_ulong),
        ("save_under", ctypes.c_int), ("colormap", ctypes.c_ulong),
        ("map_installed", ctypes.c_int), ("map_state", ctypes.c_int),
        ("all_event_masks", ctypes.c_long), ("your_event_mask", ctypes.c_long),
        ("do_not_propagate_mask", ctypes.c_long), ("override_redirect", ctypes.c_int),
        ("screen", ctypes.c_void_p),
    ]


class XImagePrefix(ctypes.Structure):
    _fields_ = [
        (name, ctypes.c_int) for name in ("width", "height", "xoffset", "format")
    ] + [("data", ctypes.c_void_p)] + [
        (name, ctypes.c_int) for name in (
            "byte_order", "bitmap_unit", "bitmap_bit_order", "bitmap_pad", "depth", "bytes_per_line", "bits_per_pixel",
        )
    ] + [(name, ctypes.c_ulong) for name in ("red_mask", "green_mask", "blue_mask")]


def capture_window(xlib, display, window: int, attrs, destination: Path) -> None:
    # Only the synthetic application's window on our Xvfb is read. The fixed
    # 24-bit Xvfb TrueColor layout deliberately avoids host display formats.
    image = xlib.XGetImage(display, window, 0, 0, attrs.width, attrs.height, ctypes.c_ulong(-1), 2)
    if not image:
        raise RuntimeError("isolated first-frame image unavailable")
    try:
        info = image.contents
        if (info.bits_per_pixel, info.byte_order, info.red_mask, info.green_mask, info.blue_mask) != (32, 0, 0xFF0000, 0xFF00, 0xFF):
            raise RuntimeError("unexpected isolated Xvfb image format")
        if not 1 <= attrs.width <= 1600 or not 1 <= attrs.height <= 1000:
            raise RuntimeError("isolated window dimensions out of bounds")
        pixels = ctypes.string_at(info.data, info.bytes_per_line * info.height)
        rows = bytearray()
        for y in range(info.height):
            row = pixels[y * info.bytes_per_line:y * info.bytes_per_line + info.width * 4]
            rgb = bytearray(info.width * 3)
            rgb[0::3], rgb[1::3], rgb[2::3] = row[2::4], row[1::4], row[0::4]
            rows.append(0)
            rows.extend(rgb)

        def block(kind: bytes, data: bytes) -> bytes:
            return struct.pack(">I", len(data)) + kind + data + struct.pack(">I", zlib.crc32(kind + data))

        png = b"\x89PNG\r\n\x1a\n" + block(b"IHDR", struct.pack(">IIBBBBB", info.width, info.height, 8, 2, 0, 0, 0))
        png += block(b"IDAT", zlib.compress(rows)) + block(b"IEND", b"")
        destination.write_bytes(png)
        os.chmod(destination, 0o600)
    finally:
        xlib.XDestroyImage(image)


def known_file_failure(line: bytes) -> dict | None:
    """Filter strace file operations to immutable app and known system paths."""
    if b"= -1 " not in line:
        return None
    decoded = line.decode("ascii", errors="ignore")
    operation = re.search(r"\b(openat|open|stat|statx|lstat|newfstatat|readlink|readlinkat|access)\(", decoded)
    failure = re.search(r"= -1 (ENOENT|EACCES|ENOTDIR|ELOOP)\b", decoded)
    path = re.search(r'"(/[A-Za-z0-9_./-]+)"', decoded)
    if operation is None or failure is None or path is None:
        return None
    candidate = path.group(1)
    if ".." in Path(candidate).parts:
        return None
    immutable_app = candidate.startswith("/opt/consolecrypt/")
    known_system = (
        candidate == "/proc/self/exe"
        or candidate.startswith(("/usr/lib/locale/", "/usr/share/locale/"))
        or (candidate.startswith(("/usr/lib/", "/usr/lib64/", "/lib/", "/lib64/"))
            and re.fullmatch(r"lib[A-Za-z0-9_-]+\.so(?:\.[0-9]+)*", Path(candidate).name) is not None)
    )
    if not immutable_app and not known_system:
        return None
    # Never emit private HOME/XDG/keyring/dataDir paths or trace arguments.
    return {"operation": operation.group(1), "path": candidate, "errno": failure.group(1)}


def safe_compiler_line(line: bytes, private_root: str) -> str | None:
    """Return only a fixed diagnostic category, never source text or paths."""
    text = line.decode("utf-8", errors="replace").strip()
    if re.search(r"VM.?service|vm-service|https?://|wss?://|auth|password|passphrase|recovery|secret|token|-----BEGIN|Unhandled|Exception|TestFailure", text, re.I):
        return None
    if re.search(r"[A-Za-z0-9+_=-]{48,}", text):
        return None
    categories = (
        (r"[^\n]*\.dart:\d+:\d+: Error:", "dart_compiler_error"),
        (r"CMake Error", "cmake_error"),
        (r"ninja: (?:error|build stopped)", "ninja_error"),
        (r"clang(?:\+\+)?: error:", "clang_error"),
        (r"error(?:\[[A-Z]\d+\])?:", "rust_compiler_error"),
        (r"Failed to build Linux application", "linux_build_failure"),
    )
    return next((category for pattern, category in categories if re.match(pattern, text)), None)


def installed_smoke(app: Path, env: dict[str, str], receipts: Path, trace: str | None = None) -> dict:
    lib = ctypes.util.find_library("X11")
    if lib is None:
        raise RuntimeError("isolated Xlib unavailable")
    xlib = ctypes.CDLL(lib)
    xlib.XOpenDisplay.argtypes, xlib.XOpenDisplay.restype = [ctypes.c_char_p], ctypes.c_void_p
    xlib.XDefaultRootWindow.argtypes, xlib.XDefaultRootWindow.restype = [ctypes.c_void_p], ctypes.c_ulong
    xlib.XQueryTree.argtypes = [ctypes.c_void_p, ctypes.c_ulong, ctypes.POINTER(ctypes.c_ulong), ctypes.POINTER(ctypes.c_ulong), ctypes.POINTER(ctypes.POINTER(ctypes.c_ulong)), ctypes.POINTER(ctypes.c_uint)]
    xlib.XFetchName.argtypes = [ctypes.c_void_p, ctypes.c_ulong, ctypes.POINTER(ctypes.c_void_p)]
    xlib.XGetWindowAttributes.argtypes = [ctypes.c_void_p, ctypes.c_ulong, ctypes.POINTER(XWindowAttributes)]
    xlib.XGetImage.argtypes = [ctypes.c_void_p, ctypes.c_ulong, ctypes.c_int, ctypes.c_int, ctypes.c_uint, ctypes.c_uint, ctypes.c_ulong, ctypes.c_int]
    xlib.XGetImage.restype = ctypes.POINTER(XImagePrefix)
    xlib.XDestroyImage.argtypes = [ctypes.POINTER(XImagePrefix)]
    xlib.XFree.argtypes = [ctypes.c_void_p]
    xlib.XCloseDisplay.argtypes = [ctypes.c_void_p]
    display = xlib.XOpenDisplay(env["DISPLAY"].encode("ascii"))
    if not display:
        raise RuntimeError("isolated Xvfb connection failed")
    command = [str(app)] if trace is None else [trace, "-f", "-qq", "-s", "256", "-e", "trace=openat,open,stat,statx,lstat,newfstatat,readlink,readlinkat,access", str(app)]
    process = subprocess.Popen(command, env=env, stdout=subprocess.PIPE, stderr=subprocess.STDOUT, start_new_session=True)
    os.set_blocking(process.stdout.fileno(), False)
    diagnostics = bytearray()
    context = {
        "proc_executable_class": "unavailable", "cwd_exists": Path.cwd().exists(),
        "bundle_files_present": {
            "runner": app.is_file(), "icu_data": (app.parent / "data/icudtl.dat").is_file(),
            "aot_library": (app.parent / "lib/libapp.so").is_file(),
            "rust_library": (app.parent / "lib/libcc_bridge.so").is_file(),
            "flutter_engine": (app.parent / "lib/libflutter_linux_gtk.so").is_file(),
            "asset_manifest": (app.parent / "data/flutter_assets/AssetManifest.bin").is_file(),
        },
        "known_file_failures": [], "file_trace_enabled": trace is not None,
    }
    trace_pending = bytearray()
    target_pid = process.pid

    def executable_class() -> None:
        nonlocal target_pid
        if trace is not None:
            try:
                children = Path(f"/proc/{process.pid}/task/{process.pid}/children").read_text().split()
                for child in children:
                    if os.readlink(f"/proc/{int(child)}/exe") == str(app):
                        target_pid = int(child)
                        break
            except OSError:
                pass
        try:
            link = os.readlink(f"/proc/{target_pid}/exe")
        except OSError:
            return
        context["proc_executable_class"] = (
            "installed_runner" if link == str(app) else "translated_rosetta" if "rosetta" in link.lower() else "other_executable"
        )

    executable_class()

    def drain() -> None:
        while True:
            try:
                data = os.read(process.stdout.fileno(), 65536)
            except BlockingIOError:
                return
            if not data:
                return
            diagnostics.extend(data)
            del diagnostics[:-65536]
            if trace is not None:
                trace_pending.extend(data)
                while b"\n" in trace_pending:
                    line, _, rest = trace_pending.partition(b"\n")
                    trace_pending[:] = rest
                    failure = known_file_failure(line)
                    if failure is not None and failure not in context["known_file_failures"] and len(context["known_file_failures"]) < 256:
                        context["known_file_failures"].append(failure)
                if len(trace_pending) > 4096:
                    trace_pending.clear()

    started = time.monotonic()
    try:
        root = xlib.XDefaultRootWindow(display)
        while time.monotonic() - started < 60:
            drain()
            executable_class()
            if process.poll() is not None:
                drain()
                raise InstalledSmokeFailure("app_exit_before_first_frame", process.returncode, diagnostics, context)
            root_return, parent, children, count = ctypes.c_ulong(), ctypes.c_ulong(), ctypes.POINTER(ctypes.c_ulong)(), ctypes.c_uint()
            xlib.XQueryTree(display, root, ctypes.byref(root_return), ctypes.byref(parent), ctypes.byref(children), ctypes.byref(count))
            window = None
            try:
                for i in range(count.value):
                    name, attrs = ctypes.c_void_p(), XWindowAttributes()
                    if xlib.XFetchName(display, children[i], ctypes.byref(name)) and name.value:
                        try:
                            is_app = ctypes.string_at(name) == b"ConsoleCrypt"
                        finally:
                            xlib.XFree(name)
                        if is_app and xlib.XGetWindowAttributes(display, children[i], ctypes.byref(attrs)) and attrs.map_state == 2:
                            window = int(children[i])
                            break
            finally:
                if children:
                    xlib.XFree(children)
            if window is not None:
                # Runner shows its GTK toplevel only in first_frame_cb.
                time.sleep(3)
                drain()
                if process.poll() is not None:
                    raise InstalledSmokeFailure("app_exit_after_first_frame", process.returncode, diagnostics, context)
                maps = Path(f"/proc/{target_pid}/maps").read_text()
                if "/libcc_bridge.so" not in maps or "/libflutter_linux_gtk.so" not in maps:
                    raise RuntimeError("installed Flutter/Rust libraries not mapped")
                image = receipts / "linux-installed-first-frame.png"
                capture_window(xlib, display, window, attrs, image)
                return {
                    "app": str(app), "app_sha256": sha256(app), "visible_first_frame": True,
                    "flutter_engine_mapped": True, "native_bridge_mapped": True,
                    "duration_seconds": round(time.monotonic() - started, 2),
                    "first_frame_png": str(image), "first_frame_png_sha256": sha256(image),
                    "requires_visual_review_to_exclude_startup_error": True,
                    **context,
                }
            time.sleep(0.2)
        raise InstalledSmokeFailure("first_frame_timeout", process.poll(), diagnostics, context)
    finally:
        stop_group(process)
        process.stdout.close()
        diagnostics[:] = b"\0" * len(diagnostics)
        trace_pending[:] = b"\0" * len(trace_pending)
        xlib.XCloseDisplay(display)


def run_flutter(
    flutter: str, suite: str, env: dict[str, str], timeout: int, log,
    *, compiler_diagnostics: bool = False,
) -> dict:
    command = [
        flutter, "test", suite, "-d", "linux",
        "--dart-define=CC_MOCK=false",
        "--dart-define=CC_IT_MEMORY_STORE=false", "--reporter", "json",
    ]
    process = subprocess.Popen(
        command, cwd=ROOT / "client/flutter", env=env,
        stdout=subprocess.PIPE, stderr=subprocess.STDOUT, start_new_session=True,
    )
    started = time.monotonic()
    last_progress = started
    names: dict[int, str] = {}
    passed: set[str] = set()
    done_success = False
    reported_errors = 0
    failure_signals: set[str] = set()
    ui_diagnostics: list[dict] = []
    start_kinds = {"loading": 0, "setUpAll": 0, "tearDownAll": 0, "other": 0, "known_with_suffix": 0}
    pending = bytearray()
    dropping = False
    timed_out = False

    def emit(message: str) -> None:
        print(message, flush=True)
        print(message, file=log, flush=True)

    def accept(line: bytes) -> None:
        nonlocal done_success, reported_errors
        try:
            event = json.loads(line)
        except (ValueError, UnicodeDecodeError):
            if compiler_diagnostics and not names:
                safe = safe_compiler_line(line, str(Path(env["HOME"]).parent))
                if safe is not None:
                    emit("PRETEST_COMPILER " + safe)
            return
        if not isinstance(event, dict):
            return
        kind = event.get("type")
        if kind == "testStart":
            test = event.get("test", {})
            if isinstance(test, dict) and test.get("name") in SUITES[suite]:
                names[test["id"]] = test["name"]
                emit(f"START {test['name']}")
            elif isinstance(test, dict) and isinstance(test.get("name"), str):
                name = test["name"]
                category = (
                    "loading" if name.startswith("loading ") else "setUpAll" if name == "(setUpAll)"
                    else "tearDownAll" if name == "(tearDownAll)"
                    else "known_with_suffix" if any(expected in name for expected in SUITES[suite]) else "other"
                )
                start_kinds[category] += 1
        elif kind == "testDone" and event.get("testID") in names:
            name = names[event["testID"]]
            success = event.get("result") == "success" and not event.get("skipped", False)
            if success:
                passed.add(name)
            emit(f"{'PASS' if success else 'FAIL'} {name}")
        elif kind == "done":
            done_success = event.get("success") is True
        elif kind == "print" and suite == "integration_test/app_persistence_test.dart":
            message = event.get("message")
            if isinstance(message, str):
                for diagnostic_line in message.splitlines():
                    diagnostic = safe_ui_diagnostic(diagnostic_line)
                    if diagnostic is not None and len(ui_diagnostics) < 512:
                        ui_diagnostics.append(diagnostic)
                        emit("UI_STATUS " + json.dumps(diagnostic, sort_keys=True))
        elif kind == "error":
            # Do not print event.message/stackTrace or print-event text.
            reported_errors += 1
            message = event.get("error", event.get("message", ""))
            if isinstance(message, str):
                if compiler_diagnostics and not names:
                    for compiler_line in message.splitlines():
                        safe = safe_compiler_line(compiler_line.encode("utf-8"), str(Path(env["HOME"]).parent))
                        if safe is not None:
                            emit("PRETEST_COMPILER " + safe)
                if "timed out waiting for" in message:
                    failure_signals.add("ui_finder_timeout")
                    for step in UI_STEPS:
                        if step in message:
                            failure_signals.add(f"ui_step_{step}")
                fixed_failure = re.search(r"CC_UI_FAILURE (timeout|finderCount|notHitTestable|buttonDisabled|fieldInput|kitMissing|scenario|flutterError) step=([A-Za-z]+)\b", message)
                if fixed_failure is not None and fixed_failure.group(2) in UI_DIAGNOSTIC_VALUES["step"]:
                    failure_signals.add("ui_" + fixed_failure.group(1))
                    failure_signals.add("ui_step_" + fixed_failure.group(2))
                for token, code in INSTALLED_DIAGNOSTICS.items():
                    if token.decode("ascii") in message:
                        failure_signals.add(code)
            emit("Flutter reported a test error; payload suppressed")

    try:
        with selectors.DefaultSelector() as selector:
            selector.register(process.stdout, selectors.EVENT_READ)
            while selector.get_map():
                now = time.monotonic()
                if now - started > timeout:
                    timed_out = True
                    emit("Flutter integration timeout")
                    stop_group(process)
                    break
                if now - last_progress >= 30:
                    emit(f"RUNNING {suite}: {int(now - started)}s, {len(passed)} verified tests")
                    last_progress = now
                for key, _ in selector.select(1):
                    chunk = os.read(key.fd, 65536)
                    if not chunk:
                        selector.unregister(key.fileobj)
                        continue
                    for piece in chunk.splitlines(keepends=True):
                        ends = piece.endswith(b"\n")
                        if not dropping:
                            pending.extend(piece)
                            if len(pending) > MAX_LINE_BYTES:
                                pending.clear()
                                dropping = True
                        if ends:
                            if not dropping:
                                accept(bytes(pending))
                            pending.clear()
                            dropping = False
            if pending and not dropping:
                accept(bytes(pending))
        code = process.wait(timeout=15)
    finally:
        stop_group(process)
        process.stdout.close()
    result = {
        "suite": suite, "command": command, "exit_code": code,
        "duration_seconds": round(time.monotonic() - started, 2),
        "passed_tests": sorted(passed), "expected_tests": sorted(SUITES[suite]),
        "reporter_done_success": done_success, "timed_out": timed_out,
        "reported_errors": reported_errors,
        "failure_signals": sorted(failure_signals),
        "started_tests": sorted(names.values()), "other_test_start_kinds": start_kinds,
        "ui_diagnostics": ui_diagnostics,
    }
    result["success"] = code == 0 and done_success and passed == SUITES[suite] and not timed_out and reported_errors == 0
    emit(f"SUITE {'PASS' if result['success'] else 'FAIL'} {suite}: {len(passed)}/{len(SUITES[suite])}")
    return result


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--receipts", type=Path, default=Path("/receipts"))
    parser.add_argument("--timeout", type=int, default=2400, help="seconds per native suite")
    parser.add_argument("--installed-app", type=Path, help="installed release first-frame smoke instead of SDK integration tests")
    parser.add_argument("--trace-known-files", action="store_true", help="installed smoke only: allowlisted failed file syscalls; never save raw trace")
    parser.add_argument("--compiler-diagnostics", action="store_true", help="fixed compiler/build categories only, before testStart; no raw text or paths")
    parser.add_argument("--keyring-fixture", type=Path, default=ROOT / "client/rust/platform-core/tests/run_linux_secret_service.py")
    args = parser.parse_args()
    if sys.platform != "linux" or os.environ.get("CC_LINUX_DISPOSABLE_BUILDER") != "1":
        parser.error("requires an owned disposable Linux builder and CC_LINUX_DISPOSABLE_BUILDER=1")
    # Native crash dumps would serialize test process memory, including keys.
    import resource
    resource.setrlimit(resource.RLIMIT_CORE, (0, 0))
    if args.timeout < 30 or args.timeout > 7200:
        parser.error("timeout must be 30..7200 seconds")
    required = ["Xvfb", "dbus-daemon", "gnome-keyring-daemon", "gdbus"]
    if args.installed_app is None:
        required += ["flutter", "cargo"]
    elif not args.installed_app.is_file() or not os.access(args.installed_app, os.X_OK):
        parser.error("installed application must be an executable file")
    if args.trace_known_files:
        if args.installed_app is None:
            parser.error("file tracing is allowed only for the installed empty-profile smoke")
        required.append("strace")
    tools = {name: shutil.which(name) for name in required}
    if any(path is None for path in tools.values()):
        parser.error("missing required Flutter/Rust/Xvfb/D-Bus/GNOME Keyring tools")
    # These caches belong to the disposable builder, not to the test home.
    pub_cache = os.environ.get("PUB_CACHE", str(Path.home() / ".pub-cache"))
    metadata = [] if args.installed_app is not None else [ROOT / "client/flutter/pubspec.yaml", ROOT / "client/flutter/lib/app/app_info.dart"]
    before = {str(path.relative_to(ROOT)): sha256(path) for path in metadata}
    args.receipts.mkdir(mode=0o700, parents=True, exist_ok=True)
    receipt_path = args.receipts / "linux-ffi-integration.json"
    log_path = args.receipts / "linux-ffi-integration.log"
    result = {
        "schema": 1, "success": False, "suites": [],
        "isolation": "owned disposable Linux builder; temporary HOME/XDG/TMPDIR, private session bus, encrypted GNOME login keyring, own software Xvfb",
        "in_memory_secure_store": False, "mock_services": False,
        "mode": "installed release first-frame smoke" if args.installed_app is not None else "debug (Flutter integration test requires debug)",
        "raw_diagnostics_saved": False,
        "limitations": [
            "No physical GPU/Wayland or real SSH connection acceptance.",
            "Existing app_persistence_test records shell layout overflows without failing; this checks persistence, not visual layout.",
        ],
    }
    fixture_root: Path | None = None
    spec = importlib.util.spec_from_file_location(
        "linux_keyring_fixture", args.keyring_fixture,
    )
    fixture = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(fixture)

    with log_path.open("w", encoding="utf-8") as log:
        os.chmod(log_path, 0o600)

        def desktop_check(env: dict[str, str]) -> None:
            nonlocal fixture_root
            fixture_root = Path(env["HOME"]).parent
            env.update({
                "PUB_CACHE": pub_cache, "GDK_BACKEND": "x11", "LIBGL_ALWAYS_SOFTWARE": "1",
                "NO_AT_BRIDGE": "1", "FLUTTER_SUPPRESS_ANALYTICS": "true",
            })
            with private_display(tools["Xvfb"], env):
                if args.installed_app is not None:
                    result["installed_smoke"] = installed_smoke(args.installed_app.resolve(), env, args.receipts, tools.get("strace"))
                    return
                for suite in SUITES:
                    outcome = run_flutter(tools["flutter"], suite, env, args.timeout, log, compiler_diagnostics=args.compiler_diagnostics)
                    result["suites"].append(outcome)
                    if not outcome["success"]:
                        raise RuntimeError("native Flutter suite failed; raw diagnostics suppressed")
                library = ROOT / "client/flutter/build/linux/x64/debug/bundle/lib/libcc_bridge.so"
                if not library.is_file():
                    raise RuntimeError("real Rust FFI library missing")
                result["native_library_sha256"] = sha256(library)

        try:
            fixture.run_case("roundtrip", tools.get("cargo", "unused"), desktop_check=desktop_check)
            result["success"] = True
        except Exception as error:
            # Arbitrary exception values can include subprocess diagnostics.
            result["failure_category"] = type(error).__name__
            if isinstance(error, InstalledSmokeFailure):
                result["failure_stage"] = error.safe_details["stage"]
                result["installed_smoke_failure"] = error.safe_details
                print(f"FAIL stage={result['failure_stage']} exit={error.safe_details['app_exit_code']}", file=log, flush=True)
            if error.args and isinstance(error.args[0], str) and error.args[0] in FAILURE_STAGES:
                result["failure_stage"] = FAILURE_STAGES[error.args[0]]
                print(f"FAIL stage={result['failure_stage']}", file=log, flush=True)
            print(f"Linux native acceptance failed ({type(error).__name__}); raw payload suppressed", file=log, flush=True)
        finally:
            result["metadata_unchanged"] = all(sha256(path) == before[str(path.relative_to(ROOT))] for path in metadata)
            result["metadata_sha256"] = before
            result["fixture_cleanup_complete"] = fixture_root is not None and not fixture_root.exists()
            result["success"] = result["success"] and result["metadata_unchanged"] and result["fixture_cleanup_complete"]
            receipt_path.write_text(json.dumps(result, indent=2) + "\n", encoding="utf-8")
            os.chmod(receipt_path, 0o600)
    print(f"Linux native FFI acceptance: {'PASS' if result['success'] else 'FAIL'}; {receipt_path}", flush=True)
    return 0 if result["success"] else 1


if __name__ == "__main__":
    raise SystemExit(main())
