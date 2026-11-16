"""Fixed socket transport dispatch and pinned bridge loader; no authority mocks."""
import hashlib,importlib.util,json,stat,sys,types,unittest
from pathlib import Path
from unittest import mock
DELIVERY=Path(__file__).resolve().parents[1]
spec=importlib.util.spec_from_file_location('installed_launch_under_test',DELIVERY/'launch.py')
launch=importlib.util.module_from_spec(spec);spec.loader.exec_module(launch)

class AcceptedEnrollmentDispatch(unittest.TestCase):
    def test_socket_uses_verified_worker_and_managed_adapter_instead_of_socket_exec(self):
        worker=Path('/verified/releases/0.1.0/bin/cc-agent-identity')
        with mock.patch.object(launch.sys,'argv',['launch.py','--enrollment-worker']), \
             mock.patch.object(launch,'verified_identity_binary',return_value=worker) as verified, \
             mock.patch.object(launch.os,'fstat',return_value=types.SimpleNamespace(st_mode=stat.S_IFSOCK)), \
             mock.patch.object(launch,'_managed_enrollment_worker',return_value=7) as bridge, \
             mock.patch.object(launch.os,'execve',side_effect=AssertionError('socket must not reach Node stdio')):
            with self.assertRaises(SystemExit) as exit:launch.main()
        self.assertEqual(exit.exception.code,7);verified.assert_called_once_with();bridge.assert_called_once_with(worker)
    def test_existing_fifo_entry_keeps_fixed_argv_and_minimal_environment(self):
        worker=Path('/verified/releases/0.1.0/bin/cc-agent-identity')
        class Executed(Exception):pass
        with mock.patch.object(launch.sys,'argv',['launch.py','--enrollment-worker']), \
             mock.patch.object(launch,'verified_identity_binary',return_value=worker), \
             mock.patch.object(launch.os,'fstat',return_value=types.SimpleNamespace(st_mode=stat.S_IFIFO)), \
             mock.patch.object(launch,'_managed_enrollment_worker',side_effect=AssertionError('unexpected bridge')), \
             mock.patch.object(launch.os,'execve',side_effect=Executed()) as execute:
            with self.assertRaises(Executed):launch.main()
        execute.assert_called_once_with(str(worker),[str(worker),'worker-node-enroll-sqlite'],{'LANG':'C.UTF-8'})
    def test_runtime_fingerprint_identity_and_readback_precede_bridge_execution(self):
        bridge=b'def _accepted_original(worker): return 73\n'
        policy={'format':'cc-agent-managed-exporter-control/v1','serviceUid':10002,'serviceGid':10002,
                'bridgeSha256':hashlib.sha256(bridge).hexdigest()}
        def call(config,changed=False):
            reads=[]
            def held(path,mode,cap):
                reads.append(str(path))
                if str(path).endswith('managed-exporter-runtime.json'):return json.dumps(config).encode()
                return bridge+b'#changed\n'if changed and len(reads)>2 else bridge
            with mock.patch.object(launch,'_collector_bytes',side_effect=held), \
                 mock.patch.object(launch.os,'getuid',return_value=10002,create=True),mock.patch.object(launch.os,'geteuid',return_value=10002,create=True), \
                 mock.patch.object(launch.os,'getgid',return_value=10002,create=True),mock.patch.object(launch.os,'getegid',return_value=10002,create=True):
                return launch._managed_enrollment_worker(Path('/verified/bin/cc-agent-identity'))
        self.assertEqual(call(policy),73)
        wrong=dict(policy,bridgeSha256='0'*64)
        with self.assertRaisesRegex(ValueError,'bridge changed'):call(wrong)
        with self.assertRaisesRegex(ValueError,'bridge changed'):call(policy,changed=True)
        with self.assertRaisesRegex(ValueError,'identity changed'):call(dict(policy,serviceUid=10001))

if __name__=='__main__':unittest.main()
