import contextlib
import importlib.util
import io
import json
import os
from pathlib import Path
import stat
import tempfile
import types
import unittest
from unittest.mock import patch

spec = importlib.util.spec_from_file_location('live_stand', Path(__file__).with_name('live-stand.py'))
stand = importlib.util.module_from_spec(spec)
spec.loader.exec_module(stand)


class LiveStandTests(unittest.TestCase):
    def test_every_live_group_runs_only_ignored_tests_one_at_a_time(self):
        commands = []
        with patch.object(stand.subprocess, 'run', side_effect=lambda c, env: commands.append(c) or types.SimpleNamespace(returncode=0)):
            self.assertTrue(stand.run_tests(stand.SHARING_TESTS, {}))
        self.assertEqual(len(commands), 3)
        for command in commands:
            self.assertEqual(command[:2], ['cargo', 'test'])
            self.assertIn('--ignored', command)
            self.assertIn('--test-threads=1', command)
        self.assertEqual(commands[2][-1], 'live_')

    def test_the_rdp_config_is_private_and_the_password_is_never_printed(self):
        seen = {}

        def fake_run(groups, env):
            path = Path(env['CC_RDP_TEST_CONFIG'])
            seen['mode'] = stat.S_IMODE(path.stat().st_mode)
            seen['config'] = json.loads(path.read_text())
            seen['output'] = env['CC_RDP_TEST_OUTPUT_DIR']
            return True

        args = types.SimpleNamespace(host='10.0.0.5', port=3389, user='tester', domain='', certificate_sha256='ab' * 32)
        out, err = io.StringIO(), io.StringIO()
        with patch.dict(os.environ, {'CC_RDP_LIVE_PASSWORD': 'synthetic-secret'}), \
                patch.object(stand, 'run_tests', side_effect=fake_run), \
                contextlib.redirect_stdout(out), contextlib.redirect_stderr(err):
            self.assertTrue(stand.rdp(args))
        self.assertEqual(seen['mode'], 0o600)
        self.assertEqual(seen['config']['password'], 'synthetic-secret')
        self.assertIsNone(seen['config']['domain'])
        self.assertEqual(seen['config']['certificate_sha256'], 'ab' * 32)
        self.assertFalse(Path(seen['output']).exists(), 'the scratch folder is removed')
        self.assertNotIn('synthetic-secret', out.getvalue() + err.getvalue())

    def test_a_shared_secret_file_is_refused(self):
        with tempfile.TemporaryDirectory() as folder:
            secret = Path(folder) / 'rdp.env'
            secret.write_text('CC_RDP_LIVE_PASSWORD=synthetic\n')
            secret.chmod(0o644)
            with patch.object(stand, 'RDP_SECRET', secret), patch.dict(os.environ, {'CC_RDP_LIVE_PASSWORD': ''}):
                with self.assertRaises(SystemExit):
                    stand.rdp_password()
                secret.chmod(0o600)
                self.assertEqual(stand.rdp_password(), 'synthetic')

    def test_the_rdp_probe_asks_for_tls_in_a_well_formed_x224_request(self):
        sent = []

        class Socket:
            def __enter__(self):
                return self

            def __exit__(self, *exc):
                return False

            def sendall(self, data):
                sent.append(data)

            def recv(self, size):
                return bytes(19)  # Not a connection confirm: the probe stops here.

        with patch.object(stand.socket, 'create_connection', return_value=Socket()):
            with self.assertRaises(SystemExit):
                stand.certificate_fingerprint('10.0.0.5', 3389)
        request = sent[0]
        self.assertEqual(len(request), 19)
        self.assertEqual(request[:4], bytes([3, 0, 0, 19]))
        self.assertEqual(request[5], 0xE0)
        self.assertEqual(int.from_bytes(request[15:19], 'little'), 3)


if __name__ == '__main__':
    unittest.main()
