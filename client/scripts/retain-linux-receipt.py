#!/usr/bin/env python3
"""Retain only the native harness JSON without following any filesystem links."""

from contextlib import ExitStack
import importlib.util
import json
import os
from pathlib import Path
import secrets
import re
import stat
import sys


RECEIPT = "linux-ffi-integration.json"
MAX_BYTES = 512 * 1024


def validate(receipt):
    specification = importlib.util.spec_from_file_location(
        "linux_native_receipt_schema", Path(__file__).with_name("test-linux-native.py"),
    )
    harness = importlib.util.module_from_spec(specification)
    specification.loader.exec_module(harness)
    top = {
        "schema", "success", "suites", "isolation", "in_memory_secure_store",
        "mock_services", "mode", "raw_diagnostics_saved", "limitations",
        "native_library_sha256", "metadata_unchanged", "metadata_sha256",
        "fixture_cleanup_complete", "failure_category", "failure_stage",
    }
    suite_fields = {
        "suite", "command", "exit_code", "duration_seconds", "passed_tests",
        "expected_tests", "reporter_done_success", "timed_out", "reported_errors",
        "failure_signals", "started_tests", "other_test_start_kinds", "ui_diagnostics", "success",
    }

    def ensure(condition):
        if not condition:
            raise ValueError("unexpected receipt format")

    def checksum(value):
        return isinstance(value, str) and re.fullmatch(r"[a-f0-9]{64}", value) is not None

    ensure(isinstance(receipt, dict) and set(receipt) <= top and type(receipt.get("schema")) is int and receipt["schema"] == 1)
    ensure(type(receipt.get("success")) is bool)
    for field in ("raw_diagnostics_saved", "mock_services", "in_memory_secure_store"):
        ensure(receipt.get(field) is False)
    for field in ("metadata_unchanged", "fixture_cleanup_complete"):
        if field in receipt:
            ensure(type(receipt[field]) is bool)
    if "isolation" in receipt:
        ensure(receipt["isolation"] == "owned disposable Linux builder; temporary HOME/XDG/TMPDIR, private session bus, encrypted GNOME login keyring, own software Xvfb")
    if "mode" in receipt:
        ensure(receipt["mode"] == "debug (Flutter integration test requires debug)")
    if "limitations" in receipt:
        ensure(receipt["limitations"] == [
            "No physical GPU/Wayland or real SSH connection acceptance.",
            "Existing app_persistence_test records shell layout overflows without failing; this checks persistence, not visual layout.",
        ])
    if "native_library_sha256" in receipt:
        ensure(checksum(receipt["native_library_sha256"]))
    if "metadata_sha256" in receipt:
        metadata = receipt["metadata_sha256"]
        ensure(isinstance(metadata, dict) and set(metadata) == {
            "client/flutter/pubspec.yaml", "client/flutter/lib/app/app_info.dart",
        } and all(checksum(value) for value in metadata.values()))
    if "failure_category" in receipt:
        ensure(receipt["failure_category"] in {
            "RuntimeError", "OSError", "ValueError", "TypeError", "FileNotFoundError",
            "PermissionError", "TimeoutExpired", "CalledProcessError", "ProcessLookupError",
        })
    if "failure_stage" in receipt:
        ensure(receipt["failure_stage"] in set(harness.FAILURE_STAGES.values()))
    suites = receipt.get("suites")
    ensure(isinstance(suites, list) and len(suites) <= len(harness.SUITES))
    codes = set(harness.INSTALLED_DIAGNOSTICS.values()) | {
        "dart_compiler_error", "cmake_error", "ninja_error", "clang_error",
        "rust_compiler_error", "linux_build_failure", "ui_finder_timeout",
    }
    codes |= {"ui_" + item for item in (
        "timeout", "finderCount", "notHitTestable", "buttonDisabled", "fieldInput",
        "kitMissing", "scenario", "flutterError",
    )}
    codes |= {"ui_step_" + item for item in harness.UI_DIAGNOSTIC_VALUES["step"]}
    codes |= {"ui_step_" + item for item in harness.UI_STEPS}
    for suite in suites:
        ensure(isinstance(suite, dict) and set(suite) == suite_fields)
        name = suite["suite"]
        ensure(isinstance(name, str) and name in harness.SUITES)
        ensure(suite["command"] == [
            "/opt/flutter/bin/flutter", "test", name, "-d", "linux",
            "--dart-define=CC_MOCK=false", "--dart-define=CC_IT_MEMORY_STORE=false",
            "--reporter", "json",
        ])
        for field in ("success", "reporter_done_success", "timed_out"):
            ensure(type(suite[field]) is bool)
        ensure(type(suite["exit_code"]) is int and -256 <= suite["exit_code"] <= 256)
        ensure(type(suite["reported_errors"]) is int and 0 <= suite["reported_errors"] <= 1000000)
        ensure(type(suite["duration_seconds"]) in (int, float) and 0 <= suite["duration_seconds"] <= 86400)
        for field in ("expected_tests", "passed_tests", "started_tests"):
            values = suite[field]
            ensure(isinstance(values, list) and all(isinstance(value, str) and value in harness.SUITES[name] for value in values))
        ensure(set(suite["expected_tests"]) == harness.SUITES[name])
        signals = suite["failure_signals"]
        ensure(isinstance(signals, list) and all(isinstance(code, str) and code in codes for code in signals))
        starts = suite["other_test_start_kinds"]
        ensure(isinstance(starts, dict) and set(starts) == {"loading", "setUpAll", "tearDownAll", "other", "known_with_suffix"})
        ensure(all(type(value) is int and 0 <= value <= 1000000 for value in starts.values()))
        diagnostics = suite["ui_diagnostics"]
        ensure(isinstance(diagnostics, list) and len(diagnostics) <= 512)
        ensure(all(isinstance(value, dict) and harness.safe_ui_diagnostic("CC_UI_DIAG " + json.dumps(value)) == value for value in diagnostics))


def directory(stack, root, parts, *, create=False):
    flags = os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW
    descriptor = os.open(root, flags)
    stack.callback(os.close, descriptor)
    for part in parts:
        if create:
            try:
                os.mkdir(part, 0o700, dir_fd=descriptor)
            except FileExistsError:
                pass
        descriptor = os.open(part, flags, dir_fd=descriptor)
        stack.callback(os.close, descriptor)
    return descriptor


def retain(source, destination):
    with ExitStack() as stack:
        try:
            parent = directory(stack, source, ("dist", "linux", "acceptance"))
            descriptor = os.open(RECEIPT, os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK, dir_fd=parent)
        except FileNotFoundError:
            # A build can fail before the harness starts; there is no receipt.
            return
        stack.callback(os.close, descriptor)
        info = os.fstat(descriptor)
        if not stat.S_ISREG(info.st_mode) or info.st_nlink != 1 or info.st_size > MAX_BYTES:
            raise ValueError("untrusted receipt file")
        with os.fdopen(os.dup(descriptor), "rb") as reader:
            contents = reader.read(MAX_BYTES + 1)
        if len(contents) > MAX_BYTES:
            raise ValueError("oversized receipt")
        receipt = json.loads(contents)
        validate(receipt)
        normalized = (json.dumps(receipt, indent=2) + "\n").encode("utf-8")
        target = directory(stack, destination, ("dist", "linux", "acceptance"), create=True)
        temporary = ".linux-receipt-" + secrets.token_hex(12)
        output = os.open(temporary, os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW, 0o600, dir_fd=target)
        try:
            with os.fdopen(output, "wb") as writer:
                writer.write(normalized)
                writer.flush()
                os.fsync(writer.fileno())
            # replace acts on the directory entry, never on a symlink target.
            os.replace(temporary, RECEIPT, src_dir_fd=target, dst_dir_fd=target)
        finally:
            try:
                os.unlink(temporary, dir_fd=target)
            except FileNotFoundError:
                pass


def main():
    if len(sys.argv) != 3:
        return 1
    try:
        retain(Path(sys.argv[1]), Path(sys.argv[2]))
    except (OSError, ValueError, TypeError):
        # Do not expose JSON content, paths or exception diagnostics in CI.
        print("Safe Linux receipt retention failed", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
