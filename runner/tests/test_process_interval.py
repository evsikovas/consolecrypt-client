import datetime
from contextlib import nullcontext
from pathlib import Path
import sys
import tempfile
import threading
import unittest
from unittest.mock import patch

from consolecrypt_runner.sandbox import ProcessResult, run_bounded
from consolecrypt_runner.store import JobStore
from consolecrypt_runner.execution import InteractiveExecutor
from consolecrypt_runner.errors import RunnerError
import test_approval_store as fixtures


def _literal_fixture_child(*args, **kwargs):
    # Existing subprocess/interval fixture stubs sandbox setup. Do not pretend
    # it mounted the production input FD; real bwrap FD closure is tested apart.
    kwargs.pop('job_input', None)
    return run_bounded(*args, **kwargs)


class ProcessIntervalTests(unittest.TestCase):
    def test_legacy_results_have_no_invented_interval(self):
        result = ProcessResult(0, '', False, False, False, 10)
        self.assertIsNone(result.execution_interval())

    def test_new_observations_never_extend_the_closed_validation_wire_shape(self):
        result = run_bounded([sys.executable, '-c', 'print("synthetic")'], timeout=5)
        self.assertIsNotNone(result.execution_interval())
        self.assertEqual(set(result.validation_fields()), {
            'exit_code', 'output', 'truncated', 'cancelled', 'timed_out', 'elapsed_ms'})
        self.assertNotIn('ObservedUtc', str(result.validation_fields()))

    def test_real_process_records_observations_independent_of_guest_output(self):
        before = datetime.datetime.now(datetime.timezone.utc)
        result = run_bounded([sys.executable, '-c',
            'print("spawnObservedUtc=1900-01-01T00:00:00Z")'], timeout=5)
        after = datetime.datetime.now(datetime.timezone.utc)
        interval = result.execution_interval()
        self.assertEqual(result.exit_code, 0)
        self.assertEqual(interval['version'], 1)
        self.assertEqual(interval['scope'], 'runner-process')
        self.assertEqual(interval['clockSource'], 'runner-wall')
        start = datetime.datetime.fromisoformat(interval['spawnObservedUtc'])
        end = datetime.datetime.fromisoformat(interval['exitObservedUtc'])
        self.assertLessEqual(before, start)
        self.assertLessEqual(start, end)
        self.assertLessEqual(end, after)
        self.assertGreaterEqual(interval['elapsedMonotonicMs'], 0)
        self.assertNotIn('1900', str(interval))

    def test_clock_reversal_is_preserved_not_replaced_with_client_time(self):
        start = datetime.datetime(2026, 11, 12, 18, tzinfo=datetime.timezone.utc)
        end = start - datetime.timedelta(seconds=20)
        with patch('consolecrypt_runner.sandbox.datetime') as clock:
            clock.now.side_effect = [start, end]
            result = run_bounded([sys.executable, '-c', 'pass'], timeout=5)
        interval = result.execution_interval()
        self.assertEqual(interval['spawnObservedUtc'], '2026-11-12T18:00:00Z')
        self.assertEqual(interval['exitObservedUtc'], '2026-11-12T17:59:40Z')
        self.assertGreaterEqual(interval['elapsedMonotonicMs'], 0)

    def test_failed_spawn_does_not_manufacture_a_process_receipt(self):
        with self.assertRaises(FileNotFoundError):
            run_bounded(['/definitely-missing-cc-owned-interval-fixture'], timeout=1)

    def test_cancelled_process_has_exit_observation_without_remote_success_claim(self):
        cancel = threading.Event()
        timer = threading.Timer(0.1, cancel.set)
        timer.start()
        try:
            result = run_bounded([sys.executable, '-c', 'import time; time.sleep(10)'],
                                 timeout=5, cancel=cancel, grace=0.1)
        finally:
            timer.cancel()
        self.assertTrue(result.cancelled)
        self.assertFalse(result.timed_out)
        self.assertIsNotNone(result.execution_interval())
        self.assertNotIn('succeeded', result.execution_interval())

    def test_timed_out_process_still_records_its_observed_exit(self):
        result = run_bounded([sys.executable, '-c', 'import time; time.sleep(10)'],
                             timeout=0.05, grace=0.05)
        self.assertTrue(result.timed_out)
        self.assertFalse(result.cancelled)
        self.assertIsNotNone(result.execution_interval())

    def test_real_executor_persists_only_the_process_produced_interval(self):
        self._executor_interval(postflight_failure=False)

    def test_postflight_failure_retains_observations_without_claiming_success(self):
        self._executor_interval(postflight_failure=True)

    def _executor_interval(self, *, postflight_failure):
        # The synthetic child emits no target commands. Snapshot/network broker
        # setup is stubbed here; subprocess, executor event handling and durable
        # JobStore are real. Separate sandbox regressions cover isolation.
        fixture = fixtures.ApprovalFixture()
        fixture.setUp()
        verified = fixture.verify()
        command = [sys.executable, '-c',
            'print(\'{"type":"runner_summary","payload":{"exit_code":0,"status":"successful",'
            '"runner_execution":{"spawnObservedUtc":"1900-01-01T00:00:00Z"}}}\')']
        with tempfile.TemporaryDirectory(prefix='cc-executor-interval-') as directory:
            with JobStore(Path(directory) / 'store') as store:
                state, _ = store.accept(verified)
                executor = InteractiveExecutor(None, None, store)
                try:
                    with patch('consolecrypt_runner.execution.probe', return_value={'environment_digest': 'c' * 64}), \
                         patch('consolecrypt_runner.execution.verify_snapshot',
                               side_effect=[None, RunnerError('snapshot_changed', 'Synthetic postflight change')]
                               if postflight_failure else None), \
                         patch('consolecrypt_runner.execution.JobBroker', return_value=nullcontext()), \
                         patch('consolecrypt_runner.execution.bubblewrap_argv', return_value=command), \
                         patch('consolecrypt_runner.execution.run_bounded', side_effect=_literal_fixture_child):
                        executor.run(verified, Path(directory), None, {}, {}, threading.Event())
                    result = store.get(state['job_id'])
                    self.assertEqual(result['state'], 'unknown' if postflight_failure else 'succeeded')
                    if postflight_failure:
                        self.assertTrue(result['result']['remote_changes_may_be_partial'])
                        self.assertFalse(result['result']['cancel_requested'])
                        self.assertFalse(result['result']['timed_out'])
                    interval = result['result']['runner_execution']
                    self.assertEqual(interval['scope'], 'runner-process')
                    self.assertEqual(interval['clockSource'], 'runner-wall')
                    self.assertNotIn('1900', str(interval))
                finally:
                    executor.close()

    def test_observed_interval_survives_durable_terminal_state_and_restart(self):
        fixture = fixtures.ApprovalFixture()
        fixture.setUp()
        result = run_bounded([sys.executable, '-c', 'pass'], timeout=5)
        interval = result.execution_interval()
        with tempfile.TemporaryDirectory(prefix='cc-process-interval-') as directory:
            root = Path(directory) / 'store'
            with JobStore(root) as store:
                state, created = store.accept(fixture.verify())
                self.assertTrue(created)
                job = state['job_id']
                self.assertNotIn('runner_execution', state)
                store.transition(job, 'running')
                store.transition(job, 'succeeded', result={'runner_execution': interval})
            with JobStore(root) as store:
                recovered = store.get(job)
                self.assertEqual(recovered['state'], 'succeeded')
                self.assertEqual(recovered['result']['runner_execution'], interval)


    def test_cancelled_child_observation_survives_real_vault_namespace_retirement(self):
        self._retired_vault_namespace(timed_out=False)

    def test_timed_out_child_observation_survives_real_vault_namespace_retirement(self):
        self._retired_vault_namespace(timed_out=True)

    def _retired_vault_namespace(self, *, timed_out):
        from contextlib import contextmanager
        import hashlib, os
        from consolecrypt_runner.dependency_helper import operation_path
        from consolecrypt_runner.store import atomic_json
        from consolecrypt_runner.vault_job_broker import VaultJobBroker
        from consolecrypt_runner.vault_sidecar import PinnedVaultSidecar, trusted_client_source
        from test_vault_job import binding, capsule, Channel
        fixture = fixtures.ApprovalFixture(); fixture.setUp(); verified = fixture.verify()
        cancel = threading.Event(); observed = []
        with tempfile.TemporaryDirectory(prefix='cc-retired-vault-') as directory:
            root = Path(directory); value = binding(); request = capsule()
            request.update(job_id=verified.value['job_id'], manifest_digest=verified.digest,
                           revision_digest=verified.value['revision_digest'])
            configured = root/'helper'; operation = operation_path(str(configured),value['operation_id'],_purpose='vault_validation')
            operation.mkdir(parents=True,mode=0o700)
            for parent in (operation.parent.parent,operation.parent):os.chmod(parent,0o700)
            data = operation/'data'; data.mkdir(mode=0o700)
            phase = data/'vault-execution'; phase.mkdir(mode=0o700)
            script=trusted_client_source('execution');client=phase/'password-client';client.write_bytes(script);client.chmod(0o500)
            atomic_json(phase/'phase.json',{'version':1,'phase':'execution','binding':value,
                'revision_digest':request['revision_digest'],'manifest_digest':request['manifest_digest'],
                'client_sha256':hashlib.sha256(script).hexdigest()})
            retired=operation.with_name(operation.name+'-retired')
            @contextmanager
            def retire_after_child():
                with PinnedVaultSidecar(str(configured),value,'execution',request['revision_digest'],request['manifest_digest']):
                    yield
                    # Real held FD/name custody recheck fails AFTER the actual
                    # child result is returned, exactly as disposal can race it.
                    operation.rename(retired)
            def actual_child(*args, **kwargs):
                kwargs['grace']=0.05
                timer=None
                if timed_out:kwargs['timeout']=0.05
                else:timer=threading.Timer(0.1,cancel.set);timer.start()
                try:
                    result=_literal_fixture_child(*args,**kwargs);observed.append(result);return result
                finally:
                    if timer is not None:timer.cancel()
            with VaultJobBroker(phase/'socket',request,Channel(),threading.Event(),keep_owned_directory=True):
                with JobStore(root/'store') as store:
                    state,_=store.accept(verified);executor=InteractiveExecutor(None,None,store)
                    try:
                        with patch('consolecrypt_runner.execution.probe',return_value={'environment_digest':'c'*64}), \
                             patch('consolecrypt_runner.execution.verify_snapshot'), \
                             patch('consolecrypt_runner.execution.JobBroker',side_effect=lambda *a,**k:retire_after_child()), \
                             patch('consolecrypt_runner.execution.bubblewrap_argv',return_value=[sys.executable,'-c','import time;time.sleep(10)']), \
                             patch('consolecrypt_runner.execution.run_bounded',side_effect=actual_child):
                            executor.run(verified,root,None,{}, {},cancel)
                        self.assertEqual(len(observed),1)
                        self.assertEqual(observed[0].timed_out,timed_out)
                        self.assertEqual(observed[0].cancelled,not timed_out)
                        saved=store.get(state['job_id']);result=saved['result']
                        self.assertEqual(saved['state'],'unknown')
                        self.assertEqual(result['reason'],'executor_lost')
                        self.assertTrue(result['remote_changes_may_be_partial'])
                        self.assertEqual(result['cancel_requested'],observed[0].cancelled)
                        self.assertEqual(result['timed_out'],observed[0].timed_out)
                        self.assertEqual(result['runner_execution'],observed[0].execution_interval())
                    finally:
                        executor.close()
                        if retired.exists():retired.rename(operation)

    def test_pre_spawn_failure_has_no_invented_process_cancellation_or_timeout(self):
        fixture=fixtures.ApprovalFixture();fixture.setUp();verified=fixture.verify()
        with tempfile.TemporaryDirectory(prefix='cc-no-child-observation-') as directory:
            with JobStore(Path(directory)/'store') as store:
                state,_=store.accept(verified);executor=InteractiveExecutor(None,None,store)
                try:
                    with patch('consolecrypt_runner.execution.probe',return_value={'environment_digest':'c'*64}), \
                         patch('consolecrypt_runner.execution.verify_snapshot'), \
                         patch('consolecrypt_runner.execution.JobBroker',return_value=nullcontext()), \
                         patch('consolecrypt_runner.execution.bubblewrap_argv',return_value=['/definitely-missing-cc-owned-child']), \
                         patch('consolecrypt_runner.execution.run_bounded',side_effect=_literal_fixture_child):
                        executor.run(verified,Path(directory),None,{}, {},threading.Event())
                    saved=store.get(state['job_id']);self.assertEqual(saved['state'],'unknown')
                    self.assertEqual(saved['result']['reason'],'executor_lost')
                    for field in ('cancel_requested','timed_out','runner_execution'):
                        self.assertNotIn(field,saved['result'])
                finally:executor.close()



if __name__ == '__main__':
    unittest.main()
