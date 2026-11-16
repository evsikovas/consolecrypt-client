"""Fixed metadata/argv paths only; no actual unit, delegation, or WAL proof."""
from pathlib import Path
import hashlib
import sys
import unittest
import uuid

COLLECTOR = Path(__file__).resolve().parents[1] / "collector"


class ServiceMetadata(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        sys.path.insert(0, str(COLLECTOR))
        import service_binding
        import service_unit_spec
        import disk_unit_spec
        cls.binding = service_binding
        cls.spec = service_unit_spec
        cls.interactive = disk_unit_spec
        sys.path.pop(0)

    def original(self):
        m = hashlib.sha256(b"synthetic exact full manifest").digest()
        address = bytearray(m[:16])
        address[6] = address[6] & 15 | 0x80
        address[8] = address[8] & 63 | 0x80
        base = self.binding.DiskBinding(
            "localProfile", "11111111-1111-4111-8111-111111111111",
            "22222222-2222-4222-8222-222222222222", "a"*64,
            "33333333-3333-4333-8333-333333333333", "b"*64, "c"*64,
            str(uuid.UUID(bytes=bytes(address))), m.hex())
        return self.binding.ServiceDiskBinding(
            base, "44444444-4444-4444-8444-444444444444", "d"*64, 1, 1, 87400)

    def test_service_reuses_same_collector_wal_address_but_not_interactive_binding(self):
        original = self.original()
        self.assertEqual(original.collector_id, original.original.collector_id)
        with self.assertRaises(ValueError):
            original.check_bytes(original.original.bytes())
        with self.assertRaises(ValueError):
            original.original.check_bytes(original.bytes())
        original.check_bytes(original.bytes())

    def test_changed_service_policy_floor_or_expiry_cannot_adopt_original_binding(self):
        original = self.original()
        for field, value in (
                ("delegation_sha256", "e"*64), ("policy_revision", 2),
                ("revocation_epoch", 2), ("expires_unix_seconds", 87401)):
            args = dict(vars(original), **{field: value})
            changed = self.binding.ServiceDiskBinding(**args)
            with self.assertRaises(ValueError):
                changed.check_bytes(original.bytes())

    def test_service_has_fixed_entry_same_kernel_caps_and_original24h_max(self):
        original = self.original()
        session = "55555555555545558555555555555555"
        spec = self.spec.ServiceUnitSpec.from_private_handoff(
            session, original.collector_id, "f"*64, 86400*10**9)
        args = spec.start_argv()
        self.assertIn(self.spec.ENTRY, args)
        self.assertNotIn(self.interactive.ENTRY, args)
        self.assertIn("--property=RuntimeMaxSec=86390s", args)
        self.assertIn("--property=MemoryMax=536870912", args)
        self.assertIn("--property=MemorySwapMax=0", args)
        self.assertIn("--property=Restart=no", args)
        with self.assertRaises(ValueError):
            self.spec.ServiceUnitSpec.from_private_handoff(
                session, original.collector_id, "f"*64, 86400*10**9+1)

    def test_service_time_window_never_widens_existing_interactive_spec(self):
        original = self.original()
        session = "55555555555545558555555555555555"
        with self.assertRaises(ValueError):
            self.interactive.DiskUnitSpec.from_private_handoff(
                session, original.collector_id, "f"*64, 86400*10**9)
        interactive = self.interactive.DiskUnitSpec.from_private_handoff(
            session, original.collector_id, "f"*64, 300*10**9)
        self.assertIn(self.interactive.ENTRY, interactive.start_argv())
        self.assertNotIn(self.spec.ENTRY, interactive.start_argv())


class ServiceWorkerDiagnostics(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        import importlib.util
        spec=importlib.util.spec_from_file_location("owned_service_daemon_diagnostics",COLLECTOR/"service_daemon.py")
        cls.module=importlib.util.module_from_spec(spec);spec.loader.exec_module(cls.module)

    def test_actual_closed_identity_denial_codes_are_classified_without_echo(self):
        import json
        from types import SimpleNamespace
        from contextlib import redirect_stderr
        from io import StringIO
        for code in ("Storage","CapabilityDenied","UncertainCommit","InvalidTime"):
            observed=self.module._WorkerStderr()
            observed.accept(json.dumps({"event":"identityDenied","code":code}).encode()+b"\n")
            self.assertEqual(observed.code(),code)
            sink=StringIO()
            with redirect_stderr(sink):observed.report(SimpleNamespace(returncode=1,pid=4321))
            value=json.loads(sink.getvalue().split(" ",1)[1])
            self.assertEqual(value["errorCode"],code);self.assertEqual(value["workerExit"],1)
            self.assertNotIn("identityDenied",sink.getvalue())

    def test_malformed_duplicate_extra_unknown_and_secret_inputs_never_echo(self):
        from types import SimpleNamespace
        from contextlib import redirect_stderr
        from io import StringIO
        for raw in (b"not JSON secret-sentinel",b'{"event":"identityDenied","code":"Storage","code":"Expired"}',
                    b'{"event":"identityDenied","code":"Storage","password":"secret-sentinel"}',
                    b'{"event":"identityDenied","code":"secret-sentinel"}',b'{"event":"readerDenied","code":"Storage"}',
                    b'{"event":"identityDenied","code":7}',b'\xff'):
            observed=self.module._WorkerStderr();observed.accept(raw)
            self.assertEqual(observed.code(),"unknown");sink=StringIO()
            with redirect_stderr(sink):observed.report(SimpleNamespace(returncode=1,pid=4321))
            self.assertNotIn("secret-sentinel",sink.getvalue());self.assertNotIn("password",sink.getvalue())
            self.assertEqual(len(sink.getvalue().splitlines()),1)

    def test_overflow_discards_raw_and_cannot_borrow_valid_suffix(self):
        observed=self.module._WorkerStderr();observed.accept(b"secret-sentinel"*400)
        observed.accept(b'{"event":"identityDenied","code":"Storage"}\n')
        self.assertTrue(observed.overflow);self.assertEqual(observed.raw,bytearray())
        self.assertEqual(observed.code(),"unknown")

    def test_nonblocking_poll_drain_is_bounded_even_after_overflow(self):
        from types import SimpleNamespace
        from unittest.mock import patch
        observed=self.module._WorkerStderr();stream=SimpleNamespace(fileno=lambda:42)
        with patch.object(self.module.os,"read",return_value=b"s"*4096)as read:
            observed.drain(stream);self.assertEqual(read.call_count,16)
        self.assertTrue(observed.overflow);self.assertLessEqual(len(observed.raw),4096)
        with patch.object(self.module.os,"read",side_effect=BlockingIOError):observed.drain(stream)
        self.assertEqual(observed.code(),"unknown")


if __name__ == "__main__":
    unittest.main()
