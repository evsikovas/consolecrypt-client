"""Actual Linux startup filesystem/side-effect boundaries, without authority.

The placeholder verifier and TLS/public bytes are never executed or admitted.
Real TUN/TLS acceptance requires the separate whole composition check.
"""
import contextlib
import hashlib
import io
import json
import os
from pathlib import Path
import sys
import tempfile
import time
import unittest
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[2]
sys.path[:0] = [str(ROOT), str(ROOT / 'runtime')]
from integration import __main__ as entry
from integration.tunnel_config import TunnelConfig, TunnelRunner
from integration.sqlite_worker_gate import WorkerChannel


@unittest.skipUnless(sys.platform == 'linux', 'requires actual Linux filesystem')
class TunnelStartupTests(unittest.TestCase):
    def setUp(self):
        temp = tempfile.TemporaryDirectory(prefix='cc-tun-startup-')
        self.addCleanup(temp.cleanup)
        self.root = Path(temp.name)
        data = {}
        for name in ('store', 'anchor', 'node', 'audit'):
            path = self.root / name
            path.mkdir(mode=0o700)
            data[name] = str(path)
        for name in ('executable', 'owner_pin', 'policy', 'certificate', 'private_key', 'client_ca', 'relay_ca', 'relay_offer'):
            path = self.root / name
            path.write_bytes(b'Test placeholder, not authority or an executable')
            path.chmod(0o755 if name == 'executable' else 0o600)
            data[name] = str(path)
        # /tmp is deliberately NOEXEC in the actual fixture. Use an existing
        # immutable system executable as DATA; WorkerChannel._start is denied.
        # No placeholder is made executable through a fixture policy change.
        data['executable'] = '/usr/bin/true'
        data['executable_sha256'] = hashlib.sha256(Path(data['executable']).read_bytes()).hexdigest()
        vector = json.loads((ROOT / 'identity/src/tunnel_policy_vector.json').read_bytes())
        Path(data['policy']).write_text(json.dumps(vector))
        self.agent_id = vector['agentId']
        self.offer = dict(origin='wss://relay.test', slotId='11111111-1111-4111-8111-111111111111',
            dialIp='127.0.0.1', capability='ccrl_' + 'A' * 43,
            expiresAt=int(time.time()) + 120, idleSeconds=15, maxCiphertextBytes=1048576)
        self.data = data
        self.write_offer()
        self.config = self.root / 'config.json'
        self.write_config()
        self.manifest = self.root / 'manifest.json'
        self.manifest.write_text(json.dumps(dict(schema_version=1, agent_id=self.agent_id,
            host_id='22222222-2222-4222-8222-222222222222', port=0)))
        self.identity = self.root / 'identity.json'
        self.identity_data = dict(provider='sqlite-custody-worker',
            **{name: data[name] for name in ('executable', 'executable_sha256', 'store', 'anchor', 'owner_pin')})
        self.identity.write_text(json.dumps(self.identity_data))
        self.state = self.root / 'runtime-state'

    def write_offer(self):
        Path(self.data['relay_offer']).write_text(json.dumps(self.offer))

    def write_config(self):
        self.config.write_text(json.dumps(self.data))
        self.config.chmod(0o600)

    def run_main(self, enabled=True):
        argv = ['agent', '--manifest', str(self.manifest), '--identity-config', str(self.identity),
                '--state-dir', str(self.state), '--once']
        if enabled:
            argv += ['--tunnel-config', str(self.config)]
        output = io.StringIO()
        with patch.object(sys, 'argv', argv), contextlib.redirect_stdout(output):
            entry.main()
        return json.loads(output.getvalue())

    def test_disabled_and_explicit_once_have_no_worker_network_or_tunnel_start(self):
        with patch.object(WorkerChannel, '_start', side_effect=AssertionError('worker forbidden')), \
                patch.object(TunnelRunner, 'start', side_effect=AssertionError('tunnel forbidden')), \
                patch.object(entry, 'create_server', side_effect=AssertionError('listen forbidden')):
            for enabled in (False, True):
                with self.subTest(enabled=enabled):
                    self.assertEqual(self.run_main(enabled)['agent_id'], self.agent_id)

    def test_unknown_duplicate_and_oversize_config_rejected_before_gate_or_state(self):
        for raw in (json.dumps({**self.data, 'autoGrant': True}),
                    '{"node":"x","node":"y"}', ' ' * 16385):
            with self.subTest(length=len(raw)), patch.object(entry, 'load_gate') as gate:
                self.config.write_text(raw)
                with self.assertRaises(ValueError):
                    self.run_main()
                gate.assert_not_called()
                self.assertFalse(self.state.exists())

    def test_wrong_digest_permissions_link_and_fifo_fail_before_gate(self):
        mutations = ('digest', 'permissions', 'link', 'fifo')
        for mutation in mutations:
            with self.subTest(mutation=mutation), patch.object(entry, 'load_gate') as gate:
                self.write_config()
                offer_path = Path(self.data['relay_offer'])
                if mutation == 'digest':
                    self.config.write_text(json.dumps({**self.data, 'executable_sha256': 'f' * 64}))
                elif mutation == 'permissions':
                    offer_path.chmod(0o644)
                elif mutation == 'link':
                    link = self.root / 'linked-offer'
                    link.symlink_to(offer_path)
                    self.config.write_text(json.dumps({**self.data, 'relay_offer': str(link)}))
                else:
                    fifo = self.root / 'fifo-offer'
                    os.mkfifo(fifo, 0o600)
                    self.config.write_text(json.dumps({**self.data, 'relay_offer': str(fifo)}))
                start = time.monotonic()
                with self.assertRaises(ValueError):
                    self.run_main()
                self.assertLess(time.monotonic() - start, 1)
                gate.assert_not_called()
                self.assertFalse(self.state.exists())
                offer_path.chmod(0o600)

    def test_expired_and_extended_relay_offer_fail_without_gate(self):
        for expires in (int(time.time()) - 1, int(time.time()) + 1000):
            self.offer['expiresAt'] = expires
            self.write_offer()
            with patch.object(entry, 'load_gate') as gate, self.assertRaisesRegex(ValueError, 'offer_expired'):
                self.run_main()
            gate.assert_not_called()
            self.assertFalse(self.state.exists())

    def test_context_mismatch_and_legacy_gate_deny_before_runtime_state(self):
        other = self.root / 'other-store'
        other.mkdir(mode=0o700)
        for change in ({'store': str(other)}, {'provider': 'file'}):
            self.identity.write_text(json.dumps({**self.identity_data, **change}))
            with self.assertRaisesRegex(ValueError, 'tunnel_'):
                self.run_main()
            self.assertFalse(self.state.exists())

    def test_foreign_agent_policy_denies_before_runtime_state(self):
        manifest = json.loads(self.manifest.read_bytes())
        manifest['agent_id'] = '99999999-9999-4999-8999-999999999999'
        self.manifest.write_text(json.dumps(manifest))
        with self.assertRaisesRegex(ValueError, 'tunnel_agent_mismatch'):
            self.run_main()
        self.assertFalse(self.state.exists())

    def test_data_configuration_is_redacted_and_not_portable(self):
        config = TunnelConfig.from_file(self.config)
        self.assertEqual(repr(config), 'TunnelConfig([redacted])')
        with self.assertRaises(TypeError):
            config.__reduce_ex__(5)
        runner = TunnelRunner(config)
        runner.close()
        with self.assertRaisesRegex(ValueError, 'tunnel_start_once'):
            runner.start()


if __name__ == '__main__':
    unittest.main(verbosity=2)
