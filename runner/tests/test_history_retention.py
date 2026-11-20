"""Component crash injection; never a claim of executing a real target job."""
from datetime import datetime, timezone
import os
from pathlib import Path
import tempfile
import sys
import math
import unittest
from unittest.mock import patch

from test_approval_store import ApprovalFixture
from consolecrypt_runner import artifact_retention as history
from consolecrypt_runner.errors import RunnerError
from consolecrypt_runner.protocol import encode, decode
from consolecrypt_runner.store import JobStore, atomic_json, read_json
from consolecrypt_runner.sandbox import run_bounded
from consolecrypt_runner.revisions import RevisionRepository


class HistoryRetentionTests(unittest.TestCase):
    def setUp(self):
        self.fixture = ApprovalFixture()
        self.fixture.setUp()
        self.temporary = tempfile.TemporaryDirectory(prefix='cc-history-retention-')
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name) / 'store'
        self.clock = self.fixture.now
        self.wall = patch('consolecrypt_runner.store.time.time', side_effect=lambda: self.clock)
        self.wall.start()
        self.addCleanup(self.wall.stop)
        self.utc = patch('consolecrypt_runner.store.utc_now', side_effect=lambda:
                         datetime.fromtimestamp(self.clock, timezone.utc).isoformat().replace('+00:00', 'Z'))
        self.utc.start()
        self.addCleanup(self.utc.stop)

    def job(self, store, terminal='succeeded'):
        verified = self.fixture.verify()
        job = verified.value['job_id']
        store.accept(verified)
        event = store.append_event(job, 'task', {'stdout': 'synthetic-private-output'})
        if terminal is not None:
            store.transition(job, 'running')
            store.transition(job, terminal, result={'exit_code': 0})
        return job, verified, event

    def test_expiry_is_exact_and_preserves_manifest_nonce_cursor_and_summary(self):
        with JobStore(self.root) as store:
            job, verified, event = self.job(store)
            directory = store.job_directory(job)
            manifest = (directory / 'manifest.json').read_bytes()
            original = read_json(directory / 'state.json')
            store.retire_history(now=self.clock + history.SECONDS - 1)
            self.assertTrue((directory / 'events.jsonl').exists())
            store.retire_history(now=self.clock + history.SECONDS)
            self.assertFalse((directory / 'events.jsonl').exists())
            state = store.get(job)
            self.assertEqual(state['sequence'], event['sequence'])
            self.assertEqual(state['output_bytes'], original['output_bytes'])
            self.assertEqual(state['result'], original['result'])
            self.assertEqual((directory / 'manifest.json').read_bytes(), manifest)
            page = store.events(job, after=0)
            self.assertEqual(page['events'], [])
            self.assertEqual(page['cursor'], 0)
            self.assertEqual(page['artifact_retirement']['last_sequence'], 1)
            self.assertEqual(store._total_output_bytes, 0)
            replay, newly_accepted = store.accept(verified)
            self.assertFalse(newly_accepted)
            self.assertEqual(replay['artifact_retirement'], state['artifact_retirement'])
            self.assertEqual(len(list((self.root / 'jobs').iterdir())), 1)

    def test_retired_reconcile_closes_new_summary_and_keeps_original_digest(self):
        import hashlib
        with JobStore(self.root) as store:
            job,_,_=self.job(store,terminal='unknown');self.clock+=history.SECONDS
            store.retire_history(now=self.clock);before=store.get(job)
            original=before['artifact_retirement']['original_result_sha256']
            result={'exit_code':7,'summary':{'status':'failed','exit_code':7,'stdout':'legacy-reconcile-secret'},'remote_changes_may_be_partial':True}
            current=store.transition(job,'failed',reconciliation=True,result=result)
            self.assertEqual(current['state'],'failed');self.assertEqual(current['result']['exit_code'],7)
            self.assertTrue(current['result']['remote_changes_may_be_partial'])
            self.assertNotIn('legacy-reconcile-secret',encode(current).decode())
            self.assertEqual(current['artifact_retirement']['original_result_sha256'],original)
            self.assertEqual(current['artifact_retirement']['retained_result_sha256'],hashlib.sha256(encode(current['result'])).hexdigest())
            self.assertEqual(current['sequence'],before['sequence']);self.assertEqual(current['history_activity_at'],before['history_activity_at'])
            self.assertFalse((store.job_directory(job)/'events.jsonl').exists())
            changed=dict(current,result={'ack':'forged'})
            with self.assertRaises(RunnerError):history.validate_state(changed)
            self.assertFalse(history.payload_reference_expired(changed,read_json(store.job_directory(job)/'manifest.json'),self.clock))

    def test_running_and_backward_clock_do_not_retire_output(self):
        with JobStore(self.root) as store:
            job, _, _ = self.job(store, terminal=None)
            directory = store.job_directory(job)
            raw = (directory / 'events.jsonl').read_bytes()
            store.retire_history(now=self.clock + 2 * history.SECONDS)
            self.assertEqual((directory / 'events.jsonl').read_bytes(), raw)
            store.transition(job, 'running')
            store.transition(job, 'failed')
            store.retire_history(now=self.clock - 1)
            self.assertEqual((directory / 'events.jsonl').read_bytes(), raw)

    def test_unknown_result_reconciles_without_reopening_retired_journal(self):
        with JobStore(self.root) as store:
            job, _, _ = self.job(store, terminal='unknown')
            store.retire_history(now=self.clock + history.SECONDS)
            original = store.get(job)
            self.clock += history.SECONDS + 10
            current = store.transition(job, 'succeeded', reconciliation=True, result={'ack': 'observed'})
            before=dict(original['artifact_retirement']);after=dict(current['artifact_retirement'])
            self.assertEqual(after.pop('retained_result_sha256'),__import__('hashlib').sha256(encode(current['result'])).hexdigest())
            before.pop('retained_result_sha256');self.assertEqual(after,before)
            self.assertEqual(current['history_activity_at'], original['history_activity_at'])
            self.assertFalse((store.job_directory(job) / 'events.jsonl').exists())
            with self.assertRaises(RunnerError):
                store.append_event(job, 'new', {})

    def injected_unlink_failure(self, after):
        with JobStore(self.root) as store:
            job, _, _ = self.job(store)
            original_count = store._total_output_bytes
            unlink = os.unlink
            def fail(name, *args, **kwargs):
                if name == 'events.jsonl':
                    if after:
                        unlink(name, *args, **kwargs)
                    raise OSError('synthetic crash boundary')
                return unlink(name, *args, **kwargs)
            with patch.object(history.os, 'unlink', side_effect=fail):
                with self.assertRaises(OSError):
                    store.retire_history(now=self.clock + history.SECONDS)
            self.assertGreater(original_count, 0)
            self.assertIn('artifact_retirement', read_json(store.job_directory(job) / 'state.json'))
            # Same live helper, not only a fresh process: quota must reclaim once.
            store.get(job)
            self.assertEqual(store._total_output_bytes, 0)
            store.get(job)
            self.assertEqual(store._total_output_bytes, 0)

    def test_marker_before_unlink_failure_reclaims_quota_once_in_same_helper(self):
        self.injected_unlink_failure(False)

    def test_unlink_before_directory_fsync_failure_reclaims_quota_once_in_same_helper(self):
        self.injected_unlink_failure(True)

    def test_restart_completes_marker_and_keeps_unrelated_live_bytes(self):
        with JobStore(self.root) as store:
            job, _, _ = self.job(store)
            unlink = os.unlink
            def fail(name, *args, **kwargs):
                if name == 'events.jsonl':
                    raise OSError('synthetic crash boundary')
                return unlink(name, *args, **kwargs)
            with patch.object(history.os, 'unlink', side_effect=fail):
                with self.assertRaises(OSError):
                    store.retire_history(now=self.clock + history.SECONDS)
        with JobStore(self.root) as reopened:
            self.assertEqual(reopened.get(job)['sequence'], 1)
            self.assertEqual(reopened._total_output_bytes, 0)
            self.assertFalse((reopened.job_directory(job) / 'events.jsonl').exists())
            # A new signed job is independent; it cannot replay the retired one.
            self.fixture.setUp()
            other, _, _ = self.job(reopened, terminal=None)
            count = reopened._total_output_bytes
            reopened.get(job)
            self.assertGreater(count, 0)
            self.assertEqual(reopened._total_output_bytes, count)
            self.assertTrue((reopened.job_directory(other) / 'events.jsonl').exists())

    def test_unknown_state_and_boolean_event_version_are_not_deletion_authority(self):
        with JobStore(self.root) as store:
            job, _, _ = self.job(store)
            directory = store.job_directory(job)
            original = read_json(directory / 'state.json')
            future = dict(original, future_schema=True)
            atomic_json(directory / 'state.json', future)
            with self.assertRaises(RunnerError):
                store.retire_history(now=self.clock + history.SECONDS)
            self.assertTrue((directory / 'events.jsonl').exists())
            atomic_json(directory / 'state.json', original)
            event = decode((directory / 'events.jsonl').read_bytes())
            event['version'] = True
            raw = encode(event) + b'\n'
            (directory / 'events.jsonl').write_bytes(raw)
            changed = dict(original, output_bytes=len(raw))
            atomic_json(directory / 'state.json', changed)
            with self.assertRaises(RunnerError):
                history.retire(directory, changed, self.clock + history.SECONDS)
            self.assertTrue((directory / 'events.jsonl').exists())

    def test_symlink_and_hardlink_artifacts_are_preserved_not_followed(self):
        with JobStore(self.root) as store:
            job, _, _ = self.job(store)
            directory = store.job_directory(job)
            state = read_json(directory / 'state.json')
            events = directory / 'events.jsonl'
            foreign = directory.parent / 'synthetic-foreign'
            foreign.write_bytes(events.read_bytes())
            foreign.chmod(0o600)
            for link in ('symlink', 'hardlink'):
                events.unlink()
                events.symlink_to(foreign) if link == 'symlink' else os.link(foreign, events)
                with self.assertRaises((OSError, RunnerError)):
                    history.retire(directory, state, self.clock + history.SECONDS)
                self.assertTrue(events.is_symlink() if link == 'symlink' else events.exists())
                self.assertIn(b'synthetic-private-output', foreign.read_bytes())

    def test_expired_known_joined_job_releases_snapshot_without_erasing_approval(self):
        source=Path(self.temporary.name)/'source'
        source.mkdir()
        (source/'playbook.yml').write_text('---\n- hosts: all\n  tasks: []\n# synthetic retained source\n')
        repository=RevisionRepository(Path(self.temporary.name)/'revisions')
        self.clock=int(datetime.now(timezone.utc).timestamp())
        self.fixture.now=self.clock
        self.fixture.manifest.update(issued_at=self.clock,expires_at=self.clock+600,access_expires_at=self.clock+3600)
        with JobStore(self.root) as store:
            snapshot=repository.capture(source)
            self.fixture.manifest['revision_digest']=snapshot['digest']
            verified=self.fixture.verify();job=verified.value['job_id']
            store.accept(verified,revision_id=snapshot['revision_id'])
            store.append_event(job,'task',{'stdout':'synthetic old output'})
            store.transition(job,'running')
            # Actual local process observations, not invented joined=true DATA.
            process=run_bounded([sys.executable,'-c','pass'],timeout=5)
            self.assertEqual(process.exit_code,0)
            self.clock=math.ceil(datetime.now(timezone.utc).timestamp())
            outcome={'exit_code':process.exit_code,'summary':{'status':'successful'},
                'timed_out':process.timed_out,'cancel_requested':process.cancelled,
                'remote_changes_may_be_partial':False,'elapsed_ms':process.elapsed_ms,
                'runner_execution':process.execution_interval()}
            store.transition(job,'succeeded',result=outcome)
            self.assertTrue(store.references_revision(snapshot['revision_id'],snapshot['digest']))
            self.clock+=history.SECONDS
            store.retire_history(now=self.clock)
            self.assertFalse(store.references_revision(snapshot['revision_id'],snapshot['digest']))
            self.assertTrue(repository.release(snapshot['revision_id'],referenced=store.references_revision)['released'])
            self.assertTrue((store.job_directory(job)/'manifest.json').exists())
            self.assertTrue((store.job_directory(job)/'revision.json').exists())
            self.assertIn('synthetic retained source',(source/'playbook.yml').read_text())

    def test_unresolved_or_unknown_join_metadata_never_releases_snapshot_reference(self):
        with JobStore(self.root) as store:
            job,_,_=self.job(store,terminal='unknown')
            directory=store.job_directory(job)
            self.clock+=history.SECONDS
            store.retire_history(now=self.clock)
            state=store.get(job);manifest=read_json(directory/'manifest.json')
            self.assertFalse(history.payload_reference_expired(state,manifest,self.clock))
            for result in ({'reason':'helper_restarted'}, {'started':True}, {'future_join':True}):
                changed=dict(state,state='succeeded',result=result)
                self.assertFalse(history.payload_reference_expired(changed,manifest,self.clock))

    def test_reclaiming_one_old_journal_keeps_other_active_job_byte_charge(self):
        with JobStore(self.root) as store:
            old,_,_=self.job(store)
            first=store._total_output_bytes
            self.fixture.setUp()
            active,_,_=self.job(store,terminal=None)
            live=store._total_output_bytes-first
            self.assertGreater(live,0)
            store.retire_history(now=self.clock+history.SECONDS)
            self.assertEqual(store._total_output_bytes,live)
            for _ in range(3):store.get(old)
            self.assertEqual(store._total_output_bytes,live)
            self.assertTrue((store.job_directory(active)/'events.jsonl').exists())

    def test_marker_must_bind_original_activity_not_just_a_past_timestamp(self):
        with JobStore(self.root) as store:
            job,_,_=self.job(store)
            directory=store.job_directory(job)
            unlink=os.unlink
            def fail(name,*args,**kwargs):
                if name=='events.jsonl':raise OSError('synthetic boundary before delete')
                return unlink(name,*args,**kwargs)
            with patch.object(history.os,'unlink',side_effect=fail):
                with self.assertRaises(OSError):store.retire_history(now=self.clock+history.SECONDS)
            state=read_json(directory/'state.json')
            state['artifact_retirement']['activity_at']-=1
            atomic_json(directory/'state.json',state)
            with self.assertRaises(RunnerError):store.get(job)
            self.assertTrue((directory/'events.jsonl').exists())

    def test_integer_zero_never_claims_cancelled_before_start(self):
        with JobStore(self.root) as store:
            verified=self.fixture.verify();job=verified.value['job_id']
            store.accept(verified)
            store.transition(job,'cancelled',result={'started':False})
            self.clock+=history.SECONDS
            store.retire_history(now=self.clock)
            directory=store.job_directory(job)
            state=store.get(job);manifest=read_json(directory/'manifest.json')
            self.assertTrue(history.payload_reference_expired(state,manifest,self.clock))
            for raw in (0,None,'false',{},True):
                changed=dict(state,result={'started':raw})
                self.assertFalse(history.payload_reference_expired(changed,manifest,self.clock))
