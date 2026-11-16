"""Real production binding codec tests, not Actor/lock/kernel proofs."""
from pathlib import Path
import dataclasses
import importlib.util
import sys
import unittest
import uuid

PATH = Path(__file__).resolve().parents[1] / "collector/disk_binding.py"
SPEC = importlib.util.spec_from_file_location("cc_staged_disk_binding", PATH)
disk = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = disk
SPEC.loader.exec_module(disk)


def original():
    full = "a" * 64
    address = bytearray.fromhex(full)[:16]
    address[6] = (address[6] & 15) | 0x80
    address[8] = (address[8] & 63) | 0x80
    return disk.DiskBinding("localProfile", "10000000-0000-4000-8000-000000000001",
        "20000000-0000-4000-8000-000000000002", "b" * 64,
        "30000000-0000-4000-8000-000000000003", "c" * 64, "d" * 64,
        str(uuid.UUID(bytes=bytes(address))), full)


class BindingData(unittest.TestCase):
    def test_same_resource_address_never_adopts_different_full_manifest(self):
        old = original()
        replacement = dataclasses.replace(old, manifest_sha256="a" * 32 + "f" * 32)
        self.assertEqual(old.collector_id, replacement.checked().collector_id)
        with self.assertRaisesRegex(ValueError, "disk_binding_changed"):
            replacement.check_bytes(old.bytes())
        old.check_bytes(old.bytes())

    def test_existing_certificate_owner_or_node_change_is_refusal_not_rebinding(self):
        old = original()
        for field, value in [("certificate_sha256", "f" * 64), ("node_key_fingerprint", "f" * 64),
                             ("owner_signing_key_sha256", "f" * 64),
                             ("owner_deployment_id", "40000000-0000-4000-8000-000000000004"),
                             ("owner_kind", "personalAccount")]:
            changed = dataclasses.replace(old, **{field: value})
            with self.assertRaisesRegex(ValueError, "disk_binding_changed"):
                changed.check_bytes(old.bytes())

    def test_binding_is_closed_canonical_public_metadata_only(self):
        binding = original()
        for raw in (binding.bytes() + b"\n", b'{"ready":true}', binding.bytes().replace(b'"localProfile"', b'"team"')):
            with self.assertRaises(ValueError):
                binding.check_bytes(raw)
        self.assertNotIn(b"lease", binding.bytes())
        self.assertNotIn(b"private", binding.bytes())
        with self.assertRaises(ValueError):
            dataclasses.replace(binding, collector_id="../../foreign").checked()


if __name__ == "__main__":
    unittest.main()
