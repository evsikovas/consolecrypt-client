import errno
import fcntl
import hashlib
import json
import os
from pathlib import Path
import stat
import subprocess
import sys
import tempfile
import threading
import unittest
from contextlib import nullcontext
from unittest.mock import patch

import test_approval_store as fixtures
from consolecrypt_runner.ephemeral_input import EphemeralJobInput, MAX_INPUT_BYTES, TARGET
from consolecrypt_runner.errors import RunnerError
from consolecrypt_runner.execution import InteractiveExecutor
from consolecrypt_runner.protocol import encode
from consolecrypt_runner.sandbox import bubblewrap_argv, run_bounded
from consolecrypt_runner.store import JobStore, atomic_json


class EphemeralJobInputTests(unittest.TestCase):
    def test_kernel_seals_private_mode_and_context_close(self):
        body = encode({'extra_vars': {'synthetic': 'private-execution-input'}})
        with EphemeralJobInput(body) as owned:
            fd = owned.inherited_fds()[0]
            info = os.fstat(fd)
            self.assertEqual(stat.S_IMODE(info.st_mode), 0o600)
            self.assertEqual(info.st_nlink, 0)
            self.assertEqual(os.pread(fd, len(body), 0), body)
            for change in (lambda: os.pwrite(fd, b'x', 0), lambda: os.ftruncate(fd, 0),
                           lambda: os.ftruncate(fd, len(body)+1)):
                with self.assertRaises(OSError) as error:
                    change()
                self.assertEqual(error.exception.errno, errno.EPERM)
        with self.assertRaises(OSError):
            os.fstat(fd)
        with self.assertRaises(RunnerError):
            owned.mounts()

    def test_oversize_and_unavailable_kernel_never_fall_back_to_file(self):
        with self.assertRaises(RunnerError):
            EphemeralJobInput(b'x'*(MAX_INPUT_BYTES+1))
        with patch('consolecrypt_runner.ephemeral_input.os.memfd_create', side_effect=OSError('synthetic unavailable')):
            with self.assertRaises(OSError):
                EphemeralJobInput(b'{}')

    def test_real_bwrap_reads_fixed_path_but_setup_fd_is_closed_in_guest(self):
        with tempfile.TemporaryDirectory(prefix='cc-owned-input-kernel-') as name:
            root = Path(name)
            script = root/'proof.py'
            script.write_text('''import errno,hashlib,os,stat,sys
from pathlib import Path
assert hashlib.sha256(Path('/tool/job-input.json').read_bytes()).hexdigest()==sys.argv[1]
assert stat.S_IMODE(Path('/tool/job-input.json').stat().st_mode)==0o600
try:Path('/tool/job-input.json').write_bytes(b'changed')
except OSError as error:assert error.errno==errno.EROFS
else:raise AssertionError('input writable')
try:os.fstat(int(sys.argv[2]))
except OSError as error:assert error.errno==errno.EBADF
else:raise AssertionError('setup descriptor leaked')
print('INPUT_OK_FD_CLOSED')
''', encoding='utf-8')
            read_fd, write_fd = os.pipe()
            try: capacity = fcntl.fcntl(write_fd, fcntl.F_GETPIPE_SZ)
            finally: os.close(read_fd); os.close(write_fd)
            body = encode({'extra_vars': {'synthetic': 'x'*max(1024*1024, capacity*2)}})
            self.assertGreater(len(body), capacity)
            print(json.dumps({'proof':'readonly-anonymous-input','bytes':len(body),
                'pipeCapacity':capacity,'sha256':hashlib.sha256(body).hexdigest()}))
            with EphemeralJobInput(body) as owned:
                fd = owned.inherited_fds()[0]
                command = [sys.executable, '/tool/proof.py', hashlib.sha256(body).hexdigest(), str(fd)]
                argv = bubblewrap_argv(root, command, extra_readonly=[(script, '/tool/proof.py')], job_input=owned)
                result = run_bounded(argv, timeout=5, grace=1, job_input=owned)
                self.assertEqual(result.exit_code, 0, result.output)
                self.assertEqual(result.output.strip(), 'INPUT_OK_FD_CLOSED')
                self.assertEqual(os.fstat(fd).st_nlink, 0)
                self.assertEqual(hashlib.sha256(os.pread(fd, len(body), 0)).digest(), hashlib.sha256(body).digest())
                with self.assertRaises(RunnerError): owned.prepare_for_spawn()
            with self.assertRaises(OSError):
                os.fstat(fd)
            self.assertFalse((root/'input.json').exists())

    def test_missing_duplicate_or_shadowed_mount_denies_before_spawn(self):
        with EphemeralJobInput(b'{}') as owned:
            fd = owned.inherited_fds()[0]
            exact = ['--ro-bind-data', str(fd), TARGET]
            for argv in ([sys.executable, '-c', 'pass'], ['bwrap', *exact, *exact]):
                with patch('consolecrypt_runner.sandbox.subprocess.Popen') as spawn:
                    with self.assertRaises(RunnerError):
                        run_bounded(argv, job_input=owned)
                    spawn.assert_not_called()
            with tempfile.TemporaryDirectory() as path:
                with self.assertRaises(RunnerError):
                    bubblewrap_argv(Path(path), [sys.executable, '-c', 'pass'],
                        extra_readonly=[(Path(path), TARGET)], job_input=owned)

    def test_success_joins_then_closes_input_without_durable_parameters(self):
        self._executor('success')

    def test_failed_child_joins_then_closes_input_without_durable_parameters(self):
        self._executor('failed')

    def test_cancelled_child_is_unknown_and_input_closes_after_join(self):
        self._executor('cancel')

    def test_launch_failure_closes_input_without_invented_completion(self):
        self._executor('launchfail')

    def _executor(self, outcome):
        # Snapshot/probe/broker preconditions are stubs; actual store, signed
        # fixture, sealed input, bwrap, child lifecycle and event parsing are real.
        fixture = fixtures.ApprovalFixture(); fixture.setUp(); verified = fixture.verify()
        actual_builder = bubblewrap_argv
        captured, errors = [], []
        ready, cancel = threading.Event(), threading.Event()
        with tempfile.TemporaryDirectory(prefix='cc-owned-input-executor-') as name:
            root = Path(name); gate = root/'gate'; gate.mkdir(mode=0o700)
            script = root/'child.py'
            script.write_text('''import json,sys,time
from pathlib import Path
spec=json.loads(Path('/tool/job-input.json').read_bytes())
assert spec['parameters']['extra_vars']['synthetic']=='private-execution-only-value'
print(json.dumps({'type':'owned_ready','payload':{}}),flush=True)
while not Path('/tool/gate/release').exists():time.sleep(0.01)
code=9 if sys.argv[1]=='failed' else 0
print(json.dumps({'type':'runner_summary','payload':{'exit_code':code,'status':'failed' if code else 'successful'}}),flush=True)
raise SystemExit(code)
''', encoding='utf-8')
            def build(workspace, command, **kwargs):
                captured.append(kwargs['job_input'].inherited_fds()[0])
                return actual_builder(workspace, [sys.executable, '/tool/child.py', outcome],
                    extra_readonly=[(script, '/tool/child.py'), (gate, '/tool/gate')], **kwargs)
            def broker(directory, *_):
                directory.mkdir(mode=0o700)
                return nullcontext()
            actual_runner = run_bounded
            def observe(*args, **kwargs):
                old = kwargs['on_chunk']
                def chunks(value):
                    old(value)
                    if b'owned_ready' in value: ready.set()
                kwargs.update(on_chunk=chunks, timeout=5, grace=0.1)
                if outcome == 'launchfail':
                    with patch('consolecrypt_runner.sandbox.subprocess.Popen', side_effect=FileNotFoundError('synthetic launch failure')):
                        return actual_runner(*args, **kwargs)
                return actual_runner(*args, **kwargs)
            with JobStore(root/'store') as store:
                state, _ = store.accept(verified)
                executor = InteractiveExecutor(None, None, store)
                parameters = {'extra_vars': {'synthetic':'private-execution-only-value'}, 'tags':[], 'skip_tags':[], 'diff':False}
                def run():
                    try: executor.run(verified, root, None, {}, parameters, cancel)
                    except BaseException as error: errors.append(error)
                try:
                    with patch('consolecrypt_runner.execution.probe', return_value={'environment_digest':'c'*64}), \
                         patch('consolecrypt_runner.execution.verify_snapshot'), \
                         patch('consolecrypt_runner.execution.JobBroker', side_effect=broker), \
                         patch('consolecrypt_runner.execution.bubblewrap_argv', side_effect=build), \
                         patch('consolecrypt_runner.execution.run_bounded', side_effect=observe):
                        worker = threading.Thread(target=run); worker.start()
                        if outcome != 'launchfail':
                            self.assertTrue(ready.wait(5), 'actual child must reach its blocked input read')
                            self.assertEqual(os.fstat(captured[0]).st_nlink, 0)
                            self.assertFalse((store.job_directory(state['job_id'])/'input.json').exists())
                            if outcome == 'cancel': cancel.set()
                            else: (gate/'release').write_bytes(b'go')
                        worker.join(10)
                        self.assertFalse(worker.is_alive())
                    self.assertEqual(errors, [])
                    self.assertEqual(len(captured), 1)
                    with self.assertRaises(OSError): os.fstat(captured[0])
                    saved = store.get(state['job_id'])
                    self.assertEqual(saved['state'], {'success':'succeeded','failed':'failed','cancel':'unknown','launchfail':'unknown'}[outcome])
                    if outcome == 'cancel': self.assertTrue(saved['result']['cancel_requested'])
                    if outcome == 'launchfail': self.assertNotIn('runner_execution', saved['result'])
                    for path in store.root.rglob('*'):
                        if path.is_file():
                            self.assertNotIn(b'private-execution-only-value', path.read_bytes())
                finally:
                    cancel.set(); (gate/'release').touch()
                    if 'worker' in locals(): worker.join(10)
                    executor.close()

    def test_process_crash_recovery_has_no_input_replay_or_named_body(self):
        fixture = fixtures.ApprovalFixture(); fixture.setUp(); verified = fixture.verify()
        with tempfile.TemporaryDirectory(prefix='cc-owned-input-restart-') as name:
            root = Path(name)/'store'
            with JobStore(root) as store:
                state, _ = store.accept(verified); job = state['job_id']
                store.transition(job, 'running')
                nonce = root/'nonces'/verified.value['nonce']; before = nonce.read_bytes()
            code = "import sys;sys.path.insert(0,sys.argv[1]);from consolecrypt_runner.ephemeral_input import EphemeralJobInput;import os;v=EphemeralJobInput(b'private-crash-only-value');os._exit(23)"
            source = Path(sys.modules[EphemeralJobInput.__module__].__file__).parent.parent
            child = subprocess.run([sys.executable, '-I', '-B', '-c', code, str(source)], timeout=5)
            self.assertEqual(child.returncode, 23)
            with JobStore(root) as store:
                store.recover_interactive()
                self.assertEqual(store.get(job)['state'], 'unknown')
                self.assertFalse(store.accept(verified)[1])
                self.assertEqual(nonce.read_bytes(), before)
                self.assertFalse((store.job_directory(job)/'input.json').exists())

    def test_known_legacy_inputs_expire_on_owner_recovery_without_nonce_or_event_loss(self):
        for phase in ('queued', 'running', 'succeeded'):
            with self.subTest(phase=phase), tempfile.TemporaryDirectory() as name:
                fixture = fixtures.ApprovalFixture(); fixture.setUp(); verified = fixture.verify()
                root = Path(name)/'store'
                with JobStore(root) as store:
                    state, _ = store.accept(verified); job = state['job_id']
                    if phase != 'queued': store.transition(job, 'running')
                    if phase == 'succeeded': store.transition(job, 'succeeded', result={'public':True})
                    store.append_event(job, 'public', {'public':True})
                    atomic_json(store.job_directory(job)/'input.json', {'extra_vars':{'synthetic':'legacy-private'}})
                    nonce = root/'nonces'/verified.value['nonce']; before = nonce.read_bytes()
                    manifest = (store.job_directory(job)/'manifest.json').read_bytes()
                with JobStore(root) as store:
                    store.recover_interactive()
                    self.assertFalse((store.job_directory(job)/'input.json').exists())
                    self.assertEqual(store.get(job)['state'], {'queued':'interrupted','running':'unknown','succeeded':'succeeded'}[phase])
                    self.assertEqual(store.events(job)['events'][0]['type'], 'public')
                    self.assertEqual(nonce.read_bytes(), before)
                    self.assertEqual((store.job_directory(job)/'manifest.json').read_bytes(), manifest)

    def test_future_state_preserved_and_legacy_symlink_never_deletes_external_file(self):
        for future in (True, False):
            with self.subTest(future=future), tempfile.TemporaryDirectory() as name:
                fixture = fixtures.ApprovalFixture(); fixture.setUp(); verified = fixture.verify()
                root = Path(name)/'store'; outside = Path(name)/'outside'; outside.write_bytes(b'outside retained')
                with JobStore(root) as store:
                    state, _ = store.accept(verified); job = state['job_id']
                    path = store.job_directory(job)/'input.json'
                    if future:
                        state.update(version=2, state='future'); atomic_json(store.job_directory(job)/'state.json', state)
                        path.write_bytes(b'future-private'); path.chmod(0o600)
                        store.recover_interactive(); self.assertEqual(path.read_bytes(), b'future-private')
                    else:
                        path.symlink_to(outside)
                        with self.assertRaises(RunnerError): store.recover_interactive()
                        self.assertTrue(path.is_symlink())
                    self.assertEqual(outside.read_bytes(), b'outside retained')

    def test_same_service_jobstore_recovery_preserves_template_and_replay_metadata(self):
        # This tests custody/recovery, not service delegation or dispatch.
        from consolecrypt_runner.service.service_custody import CustodyStore
        fixture = fixtures.ApprovalFixture(); fixture.setUp(); verified = fixture.verify()
        with tempfile.TemporaryDirectory(prefix='cc-owned-input-service-') as name:
            root = Path(name); template = root/'service-template.json'
            atomic_json(template, {'retained': 'explicit-service-restart-input'})
            template_bytes = template.read_bytes()
            with CustodyStore(root/'state') as store:
                state, _ = store.accept(verified)
                job = state['job_id']; nonce = store.root/'nonces'/verified.value['nonce']
                nonce_bytes = nonce.read_bytes()
            with CustodyStore(root/'state') as store:
                store.recover_interactive()
                self.assertEqual(store.get(job)['state'], 'interrupted')
                self.assertFalse(store.accept(verified)[1])
                self.assertEqual(nonce.read_bytes(), nonce_bytes)
                self.assertEqual(template.read_bytes(), template_bytes)
                self.assertFalse((store.job_directory(job)/'input.json').exists())
