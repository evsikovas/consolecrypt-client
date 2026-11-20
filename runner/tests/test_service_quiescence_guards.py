"""Real FD/pidfd/liveness negatives; no fake positive systemd enrollment.
Full installed OS-quiescence proof remains owned-VM-only, not this component.
"""
import os,subprocess,sys,tempfile,time,unittest,uuid
from pathlib import Path
from types import SimpleNamespace
from unittest.mock import patch
from consolecrypt_runner.errors import RunnerError
from consolecrypt_runner.protocol import decode
from consolecrypt_runner.store import atomic_json
from consolecrypt_runner.service import service_quiescence as os_guard
from consolecrypt_runner.service.service_retention import retire_quiescent
import test_service_local_join_retention as fixtures

class QuiescenceGuardTests(unittest.TestCase):
    def setUp(self):
        self.f=fixtures.ServiceLocalJoinRetentionTests();self.f.setUp();self.addCleanup(self.f.doCleanups);self.b=self.f.base
    def test_live_policy_denies_before_stop_probe_or_lock(self):
        with patch.object(os_guard,'_ObservedQuiescence',side_effect=AssertionError('no stop/probe')) as constructor:
            with self.assertRaises(RunnerError) as error:
                os_guard.retire_fixed(self.b.q,str(uuid.uuid4()),SimpleNamespace(original=self.b.q.root.stat()),None,None,None,None,self.b.policy.digest)
            self.assertEqual(error.exception.code,'service_retention_policy_live');constructor.assert_not_called()
    def test_future_nonce_denies_before_any_service_stop(self):
        self.b.clock=self.b.f.body['expires_at']+31*86400
        path=next((self.b.q.root/'state/nonces').iterdir());old=path.read_bytes();value=decode(old);value['expires_at']=self.b.clock+1;atomic_json(path,value)
        try:
            with patch.object(os_guard,'_ObservedQuiescence',side_effect=AssertionError('no stop/probe')) as constructor:
                with self.assertRaises(RunnerError) as error:
                    os_guard.retire_fixed(self.b.q,str(uuid.uuid4()),SimpleNamespace(original=self.b.q.root.stat()),None,None,None,None,self.b.policy.digest)
                self.assertEqual(error.exception.code,'service_retention_future_reference');constructor.assert_not_called()
        finally:path.write_bytes(old)
    def test_json_join_boolean_cannot_replace_actual_os_custody(self):
        old=self.f.raw()
        with self.assertRaises(RunnerError) as error:retire_quiescent(self.b.q,self.b.identity,{'joined':True},('not-authority','not-authority'))
        self.assertEqual(error.exception.code,'service_retention_quiescence');self.assertEqual(old,self.f.raw())
    def test_actual_pidfd_denies_live_child_then_observes_that_same_process_exit(self):
        self.assertTrue(hasattr(os,'pidfd_open'),'supported Linux pidfd required')
        child=subprocess.Popen([sys.executable,'-I','-B','-c','import sys;sys.stdin.buffer.read()'],stdin=subprocess.PIPE,stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL)
        fd=os.pidfd_open(child.pid,0)
        try:
            with self.assertRaises(RunnerError):os_guard._wait_original_exit(fd,time.monotonic()+.01)
            self.assertIsNone(child.poll());child.stdin.close();self.assertEqual(child.wait(timeout=3),0)
            os_guard._wait_original_exit(fd,time.monotonic()+1)
        finally:
            os.close(fd)
            if child.poll() is None:child.kill();child.wait(timeout=3)
    def test_constructor_failure_releases_original_kernel_fd_and_untrusted_parent_is_denied(self):
        fd=os.open('/sys/fs/cgroup',os.O_RDONLY|os.O_DIRECTORY|os.O_CLOEXEC)
        key=SimpleNamespace(public_identity=lambda:(_ for _ in ()).throw(OSError('owned failure')))
        with patch.object(os_guard,'_cgroup_parent',return_value=fd):
            with self.assertRaises(OSError):os_guard._ObservedQuiescence(None,str(uuid.uuid4()),None,key,None,None,None)
        with self.assertRaises(OSError):os.fstat(fd)
        with tempfile.TemporaryDirectory() as folder:
            with self.assertRaises(RunnerError):os_guard._root_directory(Path(folder))
