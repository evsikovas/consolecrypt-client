from dataclasses import replace
import json
from pathlib import Path
import socket
import tempfile
import threading
import time
import unittest
import uuid

from test_approval_store import ApprovalFixture
from consolecrypt_runner.broker import JobBroker
from consolecrypt_runner.errors import RunnerError
from consolecrypt_runner.protocol import decode, encode
from consolecrypt_runner.sandbox import bubblewrap_argv, run_bounded
from consolecrypt_runner.target_channel import TargetChannel


class BrokerTests(ApprovalFixture):
    def setUp(self):
        super().setUp()
        self.now = int(time.time())
        self.manifest.update(issued_at=self.now, expires_at=self.now + 600, access_expires_at=self.now + 3600)
        self.grants[self.signer] = replace(self.grants[self.signer], expires_at=self.now + 3600)

    def test_guest_sees_only_its_job_socket_and_cannot_expand_the_target_scope(self):
        frames = []
        def native_send(frame):
            frames.append(frame)
            if frame['type'] == 'target_request':
                channel.deliver({'version': 1, 'type': 'target_response', 'request_id': frame['request_id'],
                                 'result': {'approved_fixture': True}, 'error': None})
        channel = TargetChannel(native_send)
        self.addCleanup(channel.close)
        verified = self.verify()
        cancel = threading.Event()
        with tempfile.TemporaryDirectory(prefix='cc-broker-test-') as folder:
            root = Path(folder)
            (root / 'project').mkdir()
            with JobBroker(root / 'transport', verified, channel, cancel):
                script = '''import json,socket,sys
with socket.socket(socket.AF_UNIX,socket.SOCK_STREAM) as s:
 s.connect('/run/consolecrypt/broker.sock')
 s.sendall((json.dumps({'version':1,'host_id':sys.argv[1],'operation':'exec','body':{'command':'fixture'}})+'\\n').encode())
 print(s.makefile('rb').readline().decode())
'''
                for host, allowed in ((self.host, True), (str(uuid.uuid4()), False)):
                    import sys
                    outcome = run_bounded(bubblewrap_argv(root / 'project', [sys.executable, '-c', script, host],
                                          socket_directory=root / 'transport'), timeout=10)
                    self.assertEqual(outcome.exit_code, 0, outcome.output)
                    response = json.loads(outcome.output)
                    self.assertEqual(response['error'] is None, allowed)
        requests = [frame for frame in frames if frame['type'] == 'target_request']
        self.assertEqual(len(requests), 1)
        self.assertEqual(requests[0]['job_id'], self.manifest['job_id'])
        self.assertEqual(requests[0]['manifest_digest'], verified.digest)
        self.assertEqual(requests[0]['host_id'], self.host)
        self.assertEqual(frames[-1]['type'], 'target_revoke')

    def test_expired_or_cancelled_access_never_reaches_the_native_channel(self):
        sent = []
        channel = TargetChannel(sent.append)
        cancel = threading.Event(); cancel.set()
        with self.assertRaises(RunnerError):
            channel.call(self.verify(), self.host, 'exec', {}, cancel=cancel)
        # A valid accepted job may outlive the ten-minute submission window,
        # but never its separately signed access lease.
        value = self.manifest.copy()
        value['access_expires_at'] = self.now - 1
        from consolecrypt_runner.approval import VerifiedManifest
        expired = VerifiedManifest(encode(value), 'f' * 64, self.signer)
        with self.assertRaises(RunnerError): channel.call(expired, self.host, 'exec', {})
        self.assertEqual(sent, [])

    def test_long_private_state_paths_do_not_exceed_unix_socket_address_limit(self):
        import sys
        if sys.platform != 'linux':
            self.fail('Linux broker acceptance fixture is required')
        verified = self.verify()
        channel = TargetChannel(lambda _: None)
        with tempfile.TemporaryDirectory(prefix='cc-long-broker-') as folder:
            root = Path(folder) / ('private-state-' + 'x' * 80) / ('job-' + 'y' * 40)
            root.mkdir(mode=0o700, parents=True)
            with JobBroker(root / 'transport', verified, channel, threading.Event()):
                self.assertTrue((root / 'transport/broker.sock').exists())
                self.assertGreater(len(str(root / 'transport/broker.sock').encode()), 108)
        channel.close()
