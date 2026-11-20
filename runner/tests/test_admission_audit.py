import base64
import copy
import hashlib
import os
from pathlib import Path
import tempfile
import unittest
import uuid
from unittest.mock import patch
from cryptography.hazmat.primitives.asymmetric.ed25519 import Ed25519PrivateKey
from consolecrypt_runner.approval import DOMAIN, TrustGrant, VerifiedManifest, verify_envelope
from consolecrypt_runner.protocol import encode
from consolecrypt_runner.errors import RunnerError
from consolecrypt_runner.store import JobStore
from test_approval_store import ApprovalFixture
from test_production_owner import ProductionOwnerTests


class AdmissionAuditTests(ApprovalFixture):
    def setUp(self):
        super().setUp()
        folder=tempfile.TemporaryDirectory(prefix='cc-admission-audit-')
        self.addCleanup(folder.cleanup)
        self.root=Path(folder.name)/'state'

    def admit(self,store,verified=None):
        verified=self.verify() if verified is None else verified
        return store.accept(verified)

    def test_verified_scope_persisted_before_actual_state_and_ack_without_sensitive_fields(self):
        import consolecrypt_runner.store as module
        original=module.atomic_json
        calls=[]
        def checked_state(path,value,**kwargs):
            if path.name=='state.json':
                raw=(path.parent/'admission.json').read_bytes()
                self.assertEqual(raw,self.verify().admission_audit)
                calls.append('audit-before-state')
            return original(path,value,**kwargs)
        with JobStore(self.root)as store,patch.object(module,'atomic_json',side_effect=checked_state):
            state,created=self.admit(store)
            self.assertTrue(created)
            value=store.admission_audit(state['job_id'])
            self.assertEqual(value['signer_id'],self.signer)
            self.assertEqual(value['scope'],'jobs.execute')
            self.assertEqual(value['targets'],[{'host_id':self.host,'route_digest':'e'*64}])
            raw=(store.job_directory(state['job_id'])/'admission.json').read_bytes()
            for secret in ('deployer','example.test',self.manifest['targets'][0]['credential_refs'][0],'signature','parameters','extra_vars'):
                self.assertNotIn(secret.encode(),raw)
            self.assertEqual(calls,['audit-before-state'])

    def test_real_foreign_signer_replay_retains_original_admission_after_restart(self):
        with JobStore(self.root)as store:
            first,_=self.admit(store)
            path=store.job_directory(first['job_id'])/'admission.json'
            original=path.read_bytes()
            key=Ed25519PrivateKey.generate();signer=str(uuid.uuid4())
            grant=TrustGrant(key.public_key().public_bytes_raw(),self.tenant,frozenset([self.host]),frozenset([self.project]),frozenset(['jobs.execute']),self.now+3600)
            envelope={'signer_id':signer,'manifest':copy.deepcopy(self.manifest),'signature':base64.b64encode(key.sign(DOMAIN+encode(self.manifest))).decode()}
            other=verify_envelope(envelope,{signer:grant},runner_id=self.runner,tenant_id=self.tenant,environment_digest='c'*64,now=self.now)
            self.assertEqual(self.admit(store,other),(first,False))
            self.assertEqual(path.read_bytes(),original)
        with JobStore(self.root)as store:
            self.assertEqual(store.admission_audit(first['job_id'])['signer_id'],self.signer)
            self.assertEqual(path.read_bytes(),original)
            self.assertEqual(store.get(first['job_id'])['state'],'queued')
            recovered=store.recover_interactive()
            self.assertEqual([value['job_id']for value in recovered],[first['job_id']])
            self.assertEqual(store.get(first['job_id'])['state'],'interrupted')
            self.assertEqual(path.read_bytes(),original)

    def test_legacy_missing_audit_is_not_backfilled_by_new_accept(self):
        with JobStore(self.root)as store:
            verified=self.verify();legacy=VerifiedManifest(verified.canonical,verified.digest,verified.signer_id);state,_=store.accept(legacy)
            path=store.job_directory(state['job_id'])/'admission.json'
            self.assertIsNone(store.admission_audit(state['job_id']))
            self.assertFalse(self.admit(store)[1])
            self.assertFalse(path.exists())

    def test_corrupt_scope_read_is_denied_without_rewriting_job_or_metadata(self):
        with JobStore(self.root)as store:
            state,_=self.admit(store);directory=store.job_directory(state['job_id']);path=directory/'admission.json'
            import json
            value=json.loads(path.read_bytes());value['scope']='administrator';path.write_bytes(encode(value));before=path.read_bytes();original_state=(directory/'state.json').read_bytes()
            with self.assertRaises(RunnerError):store.admission_audit(state['job_id'])
            self.assertEqual(path.read_bytes(),before)
            self.assertEqual((directory/'state.json').read_bytes(),original_state)

    def test_symlink_audit_is_refused_and_target_preserved(self):
        with JobStore(self.root)as store:
            state,_=self.admit(store);path=store.job_directory(state['job_id'])/'admission.json';path.unlink();target=self.root/'owned-target';target.write_text('unchanged');path.symlink_to(target)
            with self.assertRaises(OSError):store.admission_audit(state['job_id'])
            self.assertEqual(target.read_text(),'unchanged')
            self.assertTrue(path.is_symlink())

    def test_actual_audit_persistence_then_lost_state_ack_recovers_interrupted_without_replay(self):
        import consolecrypt_runner.admission_audit as audit
        original=audit.persist_once
        def lost(path,raw,manifest,digest):
            original(path,raw,manifest,digest)
            raise OSError('synthetic response loss before state ACK')
        with JobStore(self.root)as store,patch.object(audit,'persist_once',side_effect=lost):
            with self.assertRaises(OSError):self.admit(store)
            self.assertFalse((store.job_directory(self.manifest['job_id'])/'state.json').exists())
        with JobStore(self.root)as store:
            self.assertEqual(store.get(self.manifest['job_id'])['state'],'interrupted')
            self.assertEqual(store.admission_audit(self.manifest['job_id'])['signer_id'],self.signer)

    def test_production_digest_comes_from_actual_extra_owner_signature_without_storing_proof(self):
        fixture=ProductionOwnerTests('test_same_enrolled_manual_owner_can_verify_exact_production_only_with_extra_signature')
        fixture.setUp();self.addCleanup(fixture.doCleanups);fixture.manual();envelope=fixture.signed();verified=fixture.verify(envelope)
        with JobStore(self.root)as store:
            state,_=self.admit(store,verified);record=store.admission_audit(state['job_id'])
            self.assertEqual(record['owner_evidence_digest'],hashlib.sha256(encode(envelope['owner_confirmation'])).hexdigest())
            self.assertTrue(record['production'])
            self.assertNotIn(envelope['owner_confirmation']['signature'],encode(record).decode())

    def test_production_audit_cannot_drop_actual_owner_evidence_digest(self):
        fixture=ProductionOwnerTests('test_same_enrolled_manual_owner_can_verify_exact_production_only_with_extra_signature')
        fixture.setUp();self.addCleanup(fixture.doCleanups);fixture.manual();verified=fixture.verify(fixture.signed())
        with JobStore(self.root)as store:
            state,_=store.accept(verified);directory=store.job_directory(state['job_id']);path=directory/'admission.json'
            import json
            value=json.loads(path.read_bytes());value['owner_evidence_digest']=None;path.write_bytes(encode(value));before=path.read_bytes()
            with self.assertRaises(RunnerError):store.admission_audit(state['job_id'])
            self.assertEqual(path.read_bytes(),before)

    def test_queue_accept_automatically_audits_and_corrupt_audit_blocks_recovery_writes(self):
        from consolecrypt_runner.service.cc_service_queue.queue import QueueStore
        with QueueStore(self.root)as store:
            state,created=store.accept(self.verify());self.assertTrue(created)
            directory=store.job_directory(state['job_id']);audit=directory/'admission.json'
            self.assertEqual(store.admission_audit(state['job_id'])['signer_id'],self.signer)
        import json
        value=json.loads(audit.read_bytes());value['version']=2;audit.write_bytes(encode(value));before=(directory/'state.json').read_bytes()
        with self.assertRaises(RunnerError):QueueStore(self.root)
        self.assertEqual((directory/'state.json').read_bytes(),before)
        self.assertEqual(json.loads(audit.read_bytes())['version'],2)


if __name__=='__main__':unittest.main()
