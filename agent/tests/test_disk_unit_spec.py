"""Pure fixed-argv/expiry tests; not a kernel cap or Actor admission proof."""
from pathlib import Path
import importlib.util
import sys
import unittest
import hashlib
import json
import os
import platform
import tempfile
import types
from unittest.mock import patch

PATH = Path(__file__).resolve().parents[1] / "collector/disk_unit_spec.py"
SPEC = importlib.util.spec_from_file_location("cc_staged_disk_unit_spec", PATH)
disk = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = disk
SPEC.loader.exec_module(disk)
SESSION = "10000000000040008000000000000001"
COLLECTOR = "20000000-0000-8000-8000-000000000002"


class FixedDiskSpec(unittest.TestCase):
    def entry(self):
        path = PATH.parent / 'disk_unit_entry.py'
        spec = importlib.util.spec_from_file_location('cc_disk_runtime_data', path)
        entry = importlib.util.module_from_spec(spec);spec.loader.exec_module(entry)
        return entry

    def test_disk_session_v1_backcompat_and_closed_v2_runtime(self):
        entry = self.entry()
        path = PATH.parent / 'disk_binding.py'
        spec = importlib.util.spec_from_file_location('cc_disk_runtime_binding', path)
        binding = importlib.util.module_from_spec(spec);sys.modules[spec.name] = binding;spec.loader.exec_module(binding)
        digest = 'a' * 64;address = bytearray.fromhex(digest)[:16]
        address[6] = address[6] & 15 | 0x80;address[8] = address[8] & 63 | 0x80
        import uuid
        original = binding.DiskBinding('localProfile', '11111111-1111-4111-8111-111111111111',
            '22222222-2222-4222-8222-222222222222', 'b' * 64,
            '33333333-3333-4333-8333-333333333333', 'c' * 64, 'd' * 64, str(uuid.UUID(bytes=bytes(address))), digest)
        entry.binding_types = lambda: (binding.DiskBinding, None)
        value = dict(format='cc-disk-prometheus-session/v1', session=SESSION, unit='cc-agent-disk-' + SESSION + '.service',
            bootId='boot', createdBoottimeNs=1, deadlineBoottimeNs=2, expiresUnixNs=3,
            scrapeSha256='e' * 64, receiver='http://127.0.0.1:9090/api/v1/write', statusPort=19090,
            ownerPid=100, ownerStartTicks=1, manifestSha256=digest, binarySha256='f' * 64, binding=vars(original))
        encode = lambda v: json.dumps(v, sort_keys=True, separators=(',', ':')).encode('ascii')
        self.assertEqual(entry.private_config(encode(value), SESSION), value)
        runtime = dict(kind='prometheusHardmode', upstreamTag='3.16.0', platform='linux-aarch64', binarySha256='f' * 64, binaryBytes=64000)
        with self.assertRaises(ValueError):entry.private_config(encode(dict(value, runtime=runtime)), SESSION)
        v2 = dict(value, format='cc-disk-prometheus-session/v2', runtime=runtime)
        self.assertEqual(entry.private_config(encode(v2), SESSION), v2)
        for changed in (dict(runtime, kind='vmagent'), dict(runtime, platform='linux-arm64'),
                        dict(runtime, binaryBytes=True), dict(runtime, binarySha256='e' * 64), dict(runtime, ready=True)):
            with self.assertRaises(ValueError):entry.private_config(encode(dict(v2, runtime=changed)), SESSION)

    def test_selected_disk_runtime_requires_same_record_platform_and_real_elf_machine(self):
        # Real owned file bytes/FD identities; signature/root custody is an
        # explicit fixture substitute here, not ARM execution or OS acceptance.
        entry = self.entry()
        with tempfile.TemporaryDirectory() as temporary:
            code = Path(temporary);release = code / 'releases/0.1.0'
            (release / 'delivery').mkdir(parents=True);(release / 'collector').mkdir()
            (code / 'collector').mkdir()
            helper = (PATH.parent.parent / 'delivery/release_platforms.py').read_bytes()
            (release / 'delivery/release_platforms.py').write_bytes(helper)
            raw = bytearray(64);raw[:7] = b'\x7fELF\x02\x01\x01';raw[18:20] = (183).to_bytes(2, 'little')
            binary = release / 'collector/prometheus-hardmode';binary.write_bytes(raw)
            runtime = dict(kind='prometheusHardmode', upstreamTag='3.16.0', platform='linux-aarch64',
                           binarySha256=hashlib.sha256(raw).hexdigest(), binaryBytes=len(raw))
            current = dict(version='0.1.0', artifact_sha256='e' * 64)
            record = dict(format='cc-agent-installed-release/v2', version='0.1.0', artifact_sha256='e' * 64,
                verified_release=dict(version=1, displayVersion='0.1.0', protocol=1, platform='linux-aarch64', artifactSha256='e' * 64, artifactBytes=256),
                files={'delivery/release_platforms.py': hashlib.sha256(helper).hexdigest(), 'collector/prometheus-hardmode': runtime['binarySha256']},
                file_modes={'delivery/release_platforms.py': '0644', 'collector/prometheus-hardmode': '0755'})
            (code / 'current.json').write_text(json.dumps(current), encoding='utf-8')
            record_path = release / 'installed.json';record_path.write_text(json.dumps(record), encoding='utf-8')
            entry.BINARY = code / 'collector/prometheus-hardmode'
            def captured(path, *, uid, mode, cap):
                fd = os.open(path, os.O_RDONLY | os.O_BINARY) if os.name == 'nt' else os.open(path, os.O_RDONLY)
                raw = os.read(fd, cap + 1);self.assertLessEqual(len(raw), cap);return fd, raw
            with patch.object(entry, 'regular', captured), patch.object(entry.sys, 'platform', 'linux'), \
                 patch.object(entry.os, 'uname', return_value=types.SimpleNamespace(machine='aarch64'), create=True), \
                 patch.object(platform, 'machine', return_value='aarch64'):
                self.assertEqual(entry.selected_runtime(runtime), 183)
                raw[18:20] = (62).to_bytes(2, 'little');binary.write_bytes(raw)
                runtime['binarySha256'] = hashlib.sha256(raw).hexdigest();record['files']['collector/prometheus-hardmode'] = runtime['binarySha256']
                record_path.write_text(json.dumps(record), encoding='utf-8')
                with self.assertRaises(ValueError):entry.selected_runtime(runtime)
                binary.unlink()
                with self.assertRaises(FileNotFoundError):entry.selected_runtime(runtime)

    def test_fixed_normal_prometheus_boolean_argv_and_foreign_path_refusal(self):
        path = Path(__file__).resolve().parents[1] / 'collector/disk_unit_entry.py'
        spec = importlib.util.spec_from_file_location('cc_permanent_disk_entry', path)
        entry = importlib.util.module_from_spec(spec); spec.loader.exec_module(entry)
        argv = entry.fixed_argv(entry.ROOT / SESSION,
            Path('/var/lib/consolecrypt-agent/metrics-disk') / COLLECTOR / 'wal', 19090)
        self.assertIn('--no-storage.agent.wal-compression', argv)
        self.assertNotIn('--storage.agent.wal-compression=false', argv)
        self.assertFalse(any('disableOnDiskQueue' in arg or arg.endswith('=none') for arg in argv))
        with self.assertRaises(ValueError):
            entry.fixed_argv(Path('/tmp/foreign'), Path('/tmp/wal'), 19090)
    def test_wal_residence_never_extends_original_authority(self):
        spec = disk.DiskUnitSpec.from_private_handoff(SESSION, COLLECTOR, "a" * 64, 300_000_000_000)
        self.assertEqual(spec.runtime_seconds, 290)
        self.assertIn("--property=RuntimeMaxSec=290s", spec.start_argv())
        self.assertEqual(disk.RESIDENCE_SECONDS, 7200)
        with self.assertRaises(ValueError):
            disk.DiskUnitSpec.from_private_handoff(SESSION, COLLECTOR, "a" * 64, 7200_000_000_000)
        for invalid in (True, 11_999_999_999, 300_000_000_001):
            with self.assertRaises(ValueError):
                disk.DiskUnitSpec.from_private_handoff(SESSION, COLLECTOR, "a" * 64, invalid)

    def test_fixed_disk_unit_can_write_only_original_session_and_manifest_root(self):
        spec = disk.DiskUnitSpec.from_private_handoff(SESSION, COLLECTOR, "b" * 64, 30_000_000_000)
        argv = spec.start_argv()
        self.assertIn("--property=Restart=no", argv)
        self.assertIn("--property=MemorySwapMax=0", argv)
        self.assertIn("--property=ReadWritePaths=" + spec.directory + " " + spec.wal_root, argv)
        self.assertEqual(argv[-3], disk.ENTRY)
        self.assertTrue(disk.ENTRY.startswith("/opt/consolecrypt-agent-install/opt/consolecrypt-agent/collector/"))
        self.assertEqual(spec.stop_argv()[-1], spec.name)
        with self.assertRaises(ValueError):
            disk.DiskUnitSpec.from_private_handoff(SESSION, "../../foreign", "b" * 64, 30_000_000_000)
        with self.assertRaises(ValueError):
            disk.DiskUnitSpec(SESSION, COLLECTOR, "b" * 64, 7200).start_argv()


if __name__ == "__main__":
    unittest.main()
