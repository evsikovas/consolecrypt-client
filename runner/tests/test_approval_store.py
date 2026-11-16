import base64
import copy
import os
from pathlib import Path
import tempfile
import unittest
import uuid
from unittest.mock import patch

from cryptography.hazmat.primitives.asymmetric.ed25519 import Ed25519PrivateKey

from consolecrypt_runner.approval import DOMAIN, TrustGrant, verify_envelope
from consolecrypt_runner.errors import RunnerError
from consolecrypt_runner.protocol import encode
from consolecrypt_runner.store import JobStore, atomic_json, read_json


class ApprovalFixture(unittest.TestCase):
    def setUp(self):
        self.key = Ed25519PrivateKey.generate()
        self.signer, self.tenant, self.runner, self.project, self.host = [str(uuid.uuid4()) for _ in range(5)]
        self.now = 1_800_000_000
        self.grants = {self.signer: TrustGrant(self.key.public_key().public_bytes_raw(), self.tenant,
                      frozenset([self.host]), frozenset([self.project]), frozenset(['jobs.execute']), self.now + 3600)}
        self.manifest = {'version': 1, 'job_id': str(uuid.uuid4()), 'tenant_id': self.tenant, 'runner_id': self.runner,
            'project_id': self.project, 'revision_digest': 'a' * 64, 'inventory_digest': 'b' * 64,
            'environment_digest': 'c' * 64, 'parameters_digest': 'd' * 64, 'playbook': 'playbook.yml', 'mode': 'apply',
            'policy': {'strict_isolation': True, 'cancel_on_lock': True, 'timeout_seconds': 3600, 'forks': 5, 'production': False},
            'targets': [{'host_id': self.host, 'alias': 'cc_' + self.host.replace('-', ''), 'address': 'example.test',
                         'port': 22, 'username': 'deployer', 'route_digest': 'e' * 64, 'credential_refs': [str(uuid.uuid4())]}],
            'issued_at': self.now, 'expires_at': self.now + 600, 'access_expires_at': self.now + 3600, 'nonce': str(uuid.uuid4())}

    def signed(self, manifest=None):
        manifest = copy.deepcopy(self.manifest if manifest is None else manifest)
        return {'signer_id': self.signer, 'manifest': manifest,
                'signature': base64.b64encode(self.key.sign(DOMAIN + encode(manifest))).decode()}

    def verify(self, envelope=None, **overrides):
        return verify_envelope(envelope or self.signed(), self.grants,
            **({'runner_id': self.runner, 'tenant_id': self.tenant, 'environment_digest': 'c' * 64, 'now': self.now} | overrides))

    def test_signature_binds_every_field_and_verified_bytes_are_immutable(self):
        envelope = self.signed()
        verified = self.verify(envelope)
        envelope['manifest']['mode'] = 'check'
        self.assertEqual(verified.value['mode'], 'apply')
        verified.value['targets'].clear()
        self.assertEqual(len(verified.value['targets']), 1)
        with self.assertRaisesRegex(RunnerError, 'signature'): self.verify(envelope)
        with self.assertRaises(RunnerError): self.verify(now=self.now + 601)
        with self.assertRaises(RunnerError): self.verify(runner_id=str(uuid.uuid4()))
        with self.assertRaises(RunnerError): self.verify(environment_digest='f' * 64)
        for field, changed in (('playbook', 'other.yml'), ('parameters_digest', 'f' * 64)):
            envelope = self.signed()
            envelope['manifest'][field] = changed
            with self.subTest(field=field), self.assertRaisesRegex(RunnerError, 'signature'):
                self.verify(envelope)

    def test_even_a_valid_signature_cannot_expand_its_enrolled_grant(self):
        for change in ('host', 'project', 'tenant', 'production', 'timeout', 'parallelism', 'extra-field'):
            manifest = copy.deepcopy(self.manifest)
            if change == 'host': manifest['targets'][0]['host_id'] = str(uuid.uuid4())
            elif change == 'project': manifest['project_id'] = str(uuid.uuid4())
            elif change == 'tenant': manifest['tenant_id'] = str(uuid.uuid4())
            elif change == 'production': manifest['policy']['production'] = True
            elif change == 'timeout': manifest['policy']['timeout_seconds'] = 86400
            elif change == 'parallelism': manifest['policy']['forks'] = True
            else: manifest['model_says_skip_approval'] = True
            with self.subTest(change=change), self.assertRaises(RunnerError): self.verify(self.signed(manifest))


@unittest.skipUnless(os.name == 'posix', 'Unix helper')
class StoreTests(ApprovalFixture):
    def setUp(self):
        super().setUp()
        self.temporary = tempfile.TemporaryDirectory(prefix='cc-store-test-')
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name) / 'runner'

    def test_durable_acceptance_deduplicates_without_new_execution(self):
        with JobStore(self.root) as store:
            verified = self.verify()
            first, created = store.accept(verified)
            self.assertTrue(created)
            self.assertEqual(store.accept(verified), (first, False))
            other = copy.deepcopy(self.manifest)
            other['mode'] = 'check'
            with self.assertRaisesRegex(RunnerError, 'another manifest'): store.accept(self.verify(self.signed(other)))
            other['job_id'] = str(uuid.uuid4())
            with self.assertRaisesRegex(RunnerError, 'nonce'): store.accept(self.verify(self.signed(other)))
            with self.assertRaisesRegex(RunnerError, 'already owns'): JobStore(self.root)

    def test_duplicate_content_snapshots_are_pinned_by_exact_id_and_legacy_is_conservative(self):
        from consolecrypt_runner.revisions import RevisionRepository
        with tempfile.TemporaryDirectory() as folder:
            root = Path(folder)
            source = root / 'source'; source.mkdir()
            (source / 'playbook.yml').write_text('---\n- hosts: all\n  tasks: []\n')
            repository = RevisionRepository(root / 'revisions')
            first, second = repository.capture(source), repository.capture(source)
            self.assertEqual(first['digest'], second['digest'])
            manifest = copy.deepcopy(self.manifest)
            manifest['revision_digest'] = first['digest']
            verified = self.verify(self.signed(manifest))
            with JobStore(root / 'state') as store:
                store.accept(verified, revision_id=first['revision_id'])
                with self.assertRaises(RunnerError):
                    repository.release(first['revision_id'], referenced=store.references_revision)
                with self.assertRaises(RunnerError):
                    store.accept(verified, revision_id=second['revision_id'])
                self.assertTrue(repository.release(second['revision_id'], referenced=store.references_revision)['released'])
            with JobStore(root / 'state') as store:
                self.assertTrue(store.references_revision(first['revision_id'], first['digest']))
                # Old jobs had only the content digest. Do not guess their ID.
                (store.job_directory(manifest['job_id']) / 'revision.json').unlink()
                third = repository.capture(source)
                with self.assertRaises(RunnerError):
                    repository.release(third['revision_id'], referenced=store.references_revision)

    def test_events_mask_escaped_short_and_numeric_secrets_and_bound_output(self):
        with JobStore(self.root, output_limit=1200) as store:
            state, _ = store.accept(self.verify())
            job_id = state['job_id']
            store.append_event(job_id, 'task', {'text': 'secret\\"token', 'value': 1}, secrets=['secret\\"token', '1'])
            events = store.events(job_id)['events']
            self.assertEqual(events[0]['payload'], {'text': '[REDACTED]', 'value': '[REDACTED]'})
            for _ in range(20): store.append_event(job_id, 'stdout', {'line': 'x' * 100})
            self.assertTrue(store.get(job_id)['output_truncated'])
            self.assertLessEqual((store.job_directory(job_id) / 'events.jsonl').stat().st_size, 1200)
            cursor = store.events(job_id, limit=1)['cursor']
            self.assertTrue(all(item['sequence'] > cursor for item in store.events(job_id, after=cursor)['events']))
            store.transition(job_id, 'running')
            result = {'changed_hosts': ['fixture'], 'unknown': True}
            store.transition(job_id, 'unknown', result=result)
            self.assertEqual(store.get(job_id)['result'], result)
            with self.assertRaisesRegex(RunnerError, 'evidence'): store.transition(job_id, 'succeeded')

    def test_recovery_never_replays_and_repairs_fsynced_event_cursor(self):
        with JobStore(self.root) as store:
            state, _ = store.accept(self.verify())
            job_id = state['job_id']
            store.transition(job_id, 'running')
            store.append_event(job_id, 'task', {'changed': True})
            path = store.job_directory(job_id) / 'state.json'
            stale = read_json(path)
            stale.update(sequence=0, output_bytes=0)
            atomic_json(path, stale)
        with JobStore(self.root) as reopened:
            self.assertEqual(reopened.get(job_id)['sequence'], 1)
            recovered = reopened.recover_interactive()
            self.assertEqual(recovered[0]['state'], 'unknown')
            self.assertEqual(reopened.append_event(job_id, 'connection', {'recovered': True})['sequence'], 2)

    def test_global_output_and_job_count_quotas_survive_restart_without_losing_summary(self):
        first = self.verify()
        other = copy.deepcopy(self.manifest)
        other.update(job_id=str(uuid.uuid4()), nonce=str(uuid.uuid4()))
        second = self.verify(self.signed(other))
        with JobStore(self.root, total_output_limit=1024, max_jobs=2) as store:
            store.accept(first)
            self.assertIsNotNone(store.append_event(first.value['job_id'], 'task', {'output': 'x' * 600}))
            store.accept(second)
            self.assertIsNone(store.append_event(second.value['job_id'], 'task', {'output': 'y' * 600}))
            self.assertTrue(store.get(second.value['job_id'])['output_truncated'])
            store.transition(second.value['job_id'], 'running')
            store.transition(second.value['job_id'], 'succeeded', result={'changed': 1})
        with JobStore(self.root, total_output_limit=1024, max_jobs=2) as reopened:
            self.assertEqual(reopened.get(second.value['job_id'])['result'], {'changed': 1})
            self.assertFalse(reopened.accept(first)[1])
            self.assertIsNone(reopened.append_event(second.value['job_id'], 'task', {'output': 'z' * 600}))
            other.update(job_id=str(uuid.uuid4()), nonce=str(uuid.uuid4()))
            with self.assertRaisesRegex(RunnerError, 'Retained job limit'):
                reopened.accept(self.verify(self.signed(other)))
            self.assertFalse((self.root / 'nonces' / other['nonce']).exists())

    def test_late_cursor_reads_a_bounded_tail_before_and_after_reopen(self):
        verified=self.verify();job=verified.value['job_id']
        with JobStore(self.root) as store:
            store.accept(verified)
            for number in range(1024):store.append_event(job,'task',{'number':number,'output':'x'*4096})
            self.assertGreater((store.job_directory(job)/'events.jsonl').stat().st_size,4*1024*1024)
            self._assert_bounded_page(store,job)
        with JobStore(self.root) as reopened:self._assert_bounded_page(reopened,job)

    def _assert_bounded_page(self,store,job):
        original=os.fdopen;scanned=[0]
        class Counted:
            def __init__(self,source):self.source=source
            def __enter__(self):return self
            def __exit__(self,*args):return self.source.__exit__(*args)
            def __getattr__(self,name):return getattr(self.source,name)
            def readline(self,*args):
                value=self.source.readline(*args);scanned[0]+=len(value);return value
        def instrumented(*args,**kwargs):return Counted(original(*args,**kwargs))
        with patch('consolecrypt_runner.store.os.fdopen',side_effect=instrumented):
            page=store.events(job,after=1000,limit=10)
        self.assertEqual([row['sequence'] for row in page['events']],list(range(1001,1011)))
        self.assertEqual(page['events'][0]['payload']['number'],1000)
        self.assertLess(scanned[0],1024*1024,'A late cursor must not reread the complete multi-megabyte journal')

    def test_partial_acceptance_recovers_as_interrupted_and_does_not_reexecute(self):
        verified = self.verify()
        with JobStore(self.root) as store:
            state, _ = store.accept(verified)
            (store.job_directory(state['job_id']) / 'state.json').unlink()
            empty = store.job_directory(str(uuid.uuid4()))
            empty.mkdir(mode=0o700)
        with JobStore(self.root) as reopened:
            recovered, created = reopened.accept(verified)
            self.assertFalse(created)
            self.assertEqual(recovered['state'], 'interrupted')
            self.assertEqual(recovered['result']['reason'], 'acceptance_interrupted')
            self.assertFalse(empty.exists())
            self.assertEqual(reopened.recover_interactive(), [])


if __name__ == '__main__': unittest.main()
