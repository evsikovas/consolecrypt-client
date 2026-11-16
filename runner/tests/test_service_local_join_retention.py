"""Actual child wait + genuine signatures/private custody, metadata-only cleanup.
No SSH dispatch/enrollment success is manufactured; remote uncertainty remains.
"""
import base64,copy,hashlib,math,subprocess,sys,threading,time,unittest,uuid
from datetime import datetime,timezone
from pathlib import Path
from unittest.mock import patch
import service_execution_fixtures as service_fixtures
import test_service_template_retirement as template_fixtures
from consolecrypt_runner import artifact_retention as history
from consolecrypt_runner.approval import DOMAIN,TrustGrant,verify_envelope
from consolecrypt_runner.errors import RunnerError
from consolecrypt_runner.protocol import encode,decode
from consolecrypt_runner.sandbox import run_bounded
from consolecrypt_runner.store import atomic_json
from consolecrypt_runner.service.service_authority_v4 import DELEGATION_DOMAIN,HEAD_DOMAIN,verify_delegation
from consolecrypt_runner.service.service_custody import logical_job_id,_binding,ServiceCustody
from consolecrypt_runner.service.service_retention import retire_joined,retire_existing

class ServiceLocalJoinRetentionTests(unittest.TestCase):
    def setUp(self):
        self.base=template_fixtures.TemplateRetirementTests()
        now=int(time.time());anchor=now-now%60-360
        # Same maintained fixture/real keys/scheduler, aligned to the ACTUAL
        # local process clock so no wait interval timestamps are invented.
        with patch.object(service_fixtures,'NOW',anchor),patch.object(template_fixtures,'NOW',anchor):self.base.setUp()
        b=self.base;self.addCleanup(b.tearDown);b.clock=now
        self.source=b.root/'sensitive-source';self.source.mkdir()
        (self.source/'playbook.yml').write_text('---\n- hosts: all\n  gather_facts: false\n  tasks: []\n# owned-sensitive-revision-marker\n')
        captured=b.helper.revisions.capture(self.source);b.revision=captured['revision_id'];b.f.body['template_digest']=captured['digest']
        b.signed=decode(service_fixtures.signed(b.f.owner,DELEGATION_DOMAIN,b.f.body))
        b.policy=verify_delegation(encode(b.signed),b.f.pin,b.f.body['issued_at'])
        b.value.update(delegation=b.signed,delegationDigest=b.policy.digest,revisionId=b.revision,revisionDigest=captured['digest'])
        atomic_json(b.q.root/'service-template.json',b.value,max_bytes=6*1024*1024)
        b.f.head.update(delegation_digest=b.policy.digest,issued_at=now-1,expires_at=now+120)
        b.q.record_owner_status(b.policy,service_fixtures.signed(b.f.owner,HEAD_DOMAIN,b.f.head),b.runtime.executor)
        self.manifest,self.job,self.directory=self.create_job(now)
    def create_job(self,now):
        b=self.base;body=b.f.body;local=datetime.fromtimestamp(b.f.future,timezone.utc).strftime('%Y-%m-%dT%H:%M')
        job=logical_job_id(b.f.tenant,b.f.service_id,b.f.project,b.f.schedule,local)
        manifest={'version':1,'job_id':job,'tenant_id':b.f.tenant['id'],'runner_id':b.f.service_id,'project_id':b.f.project,
            'revision_digest':body['template_digest'],'inventory_digest':body['inventory_digest'],'parameters_digest':body['parameters_digest'],
            'environment_digest':body['environment_digest'],'playbook':body['playbook'],'mode':'check',
            'policy':{'strict_isolation':True,'cancel_on_lock':False,'production':False,'timeout_seconds':120,'forks':1},
            'targets':[b.f.target],'issued_at':now,'expires_at':now+120,'access_expires_at':now+120,'nonce':str(uuid.uuid4())}
        approval={'signer_id':b.f.service_id,'manifest':manifest,'signature':base64.b64encode(b.f.service.sign(DOMAIN+encode(manifest))).decode()}
        grant=TrustGrant(bytes.fromhex(body['signer']['public_key']),b.f.tenant['id'],frozenset([b.f.host]),frozenset([b.f.project]),frozenset(['jobs.execute']),body['expires_at'],120)
        verified=verify_envelope(approval,{b.f.service_id:grant},runner_id=b.f.service_id,tenant_id=b.f.tenant['id'],environment_digest=body['environment_digest'],now=now)
        b.q.store.accept(verified,revision_id=b.revision);directory=b.q.store.job_directory(job)
        binding={'version':1,'tenant':b.f.tenant,'serviceId':b.f.service_id,'projectId':b.f.project,'scheduleId':b.f.schedule,
            'scheduleRevision':body['schedule_revision'],'definitionDigest':body['definition_digest'],'localSlot':local,'dueUtc':b.f.future,
            'jobId':job,'manifestDigest':verified.digest,'delegationDigest':b.policy.digest,'admittedAt':now,'policyRevision':1,'revocationEpoch':1}
        atomic_json(directory/'approval.json',approval);atomic_json(directory/'service-custody.json',_binding(binding),max_bytes=8192)
        atomic_json(directory/'service-start.json',{'version':1,'jobId':job,'manifestDigest':verified.digest,'delegationDigest':b.policy.digest,'revisionId':b.revision,'targetOutcome':'uncertain'},max_bytes=8192)
        b.q.store.transition(job,'running');b.q.store.append_event(job,'task',{'stdout':'owned old raw diagnostic'})
        return manifest,job,directory
    def finish(self,process=None,post_wait=False):
        b=self.base
        if process is None:process=run_bounded([sys.executable,'-I','-B','-c','pass'],timeout=5)
        self.assertIsNotNone(process.execution_interval())
        if post_wait:
            outcome={'reason':'executor_lost','cancel_requested':process.cancelled,'timed_out':process.timed_out,
                'remote_changes_may_be_partial':True,'runner_execution':process.execution_interval()}
        else:
            outcome={'exit_code':process.exit_code,'summary':None,'timed_out':process.timed_out,'cancel_requested':process.cancelled,
                'remote_changes_may_be_partial':True,'elapsed_ms':process.elapsed_ms,'runner_execution':process.execution_interval()}
        b.q.store.transition(self.job,'unknown',result=outcome)
        if b.clock<b.f.body['expires_at']:b.clock=max(b.clock,math.ceil(time.time()))
        return b.q.store.get(self.job)
    def raw(self):return (self.base.q.root/'service-template.json').read_bytes()
    def assert_denied_unchanged(self,code):
        b=self.base;old=self.raw()
        with self.assertRaises(RunnerError) as error:retire_joined(b.q,b.identity,joined=True)
        self.assertEqual(error.exception.code,code);self.assertEqual(old,self.raw())
    def test_joined_local_worker_remote_unknown_retires_template_and_exact_revision_only_after_raw_ttl(self):
        b=self.base;state=self.finish();outcome=copy.deepcopy(state['result']);start=(self.directory/'service-start.json').read_bytes()
        nonce={p.name:p.read_bytes() for p in (b.q.root/'state/nonces').iterdir()}
        b.expire();b.runtime.close()
        self.assertEqual(decode(self.raw())['version'],2);self.assertNotIn(b'ONLY_OWNED_RETENTION_CANARY_42',self.raw())
        self.assertEqual((self.directory/'service-start.json').read_bytes(),start)
        activity=history.timestamp(state['history_activity_at']);b.clock=activity+history.SECONDS-1
        self.assertTrue(b.runtime.references_revision(b.revision,b.f.body['template_digest']))
        with self.assertRaises(RunnerError):b.helper.revisions.release(b.revision,referenced=b.runtime.references_revision)
        b.clock=activity+history.SECONDS
        self.assertFalse(b.runtime.references_revision(b.revision,b.f.body['template_digest']))
        retained=b.q.store.get(self.job);self.assertEqual(retained['state'],'unknown');self.assertEqual(retained['result'],outcome)
        self.assertEqual(retained['sequence'],state['sequence']);self.assertFalse((self.directory/'events.jsonl').exists())
        self.assertTrue(b.helper.revisions.release(b.revision,referenced=b.runtime.references_revision)['released'])
        for name in ('manifest.json','approval.json','revision.json','service-custody.json','service-start.json'):self.assertTrue((self.directory/name).is_file())
        self.assertEqual({p.name:p.read_bytes() for p in (b.q.root/'state/nonces').iterdir()},nonce)
        self.assertIn('owned-sensitive-revision-marker',(self.source/'playbook.yml').read_text())
    def test_actual_running_child_refuses_then_same_observed_join_allows_cleanup(self):
        b=self.base;cancel=threading.Event();entered=threading.Event();results=[]
        def child():results.append(run_bounded([sys.executable,'-I','-B','-c',"import time;print('owned-running',flush=True);time.sleep(30)"],timeout=40,cancel=cancel,grace=1,on_chunk=lambda _:entered.set()))
        thread=threading.Thread(target=child);thread.start()
        try:
            self.assertTrue(entered.wait(3));b.expire();self.assert_denied_unchanged('service_retention_worker_active')
        finally:cancel.set();thread.join(5)
        self.assertFalse(thread.is_alive());self.assertEqual(len(results),1);self.assertTrue(results[0].cancelled)
        self.finish(results[0]);b.runtime.close();self.assertEqual(decode(self.raw())['version'],2)
    def test_no_wait_or_unknown_future_receipts_are_not_joined(self):
        b=self.base;state=self.finish();original=copy.deepcopy(state);b.expire()
        variants=[({'reason':'helper_restarted'},'service_retention_join_missing'),
            (dict(state['result'],runner_execution={**state['result']['runner_execution'],'version':True}),'service_retention_join_invalid'),
            (dict(state['result'],cancel_requested=0),'service_retention_join_invalid'),
            (dict(state['result'],future_join=True),'service_retention_join_invalid')]
        for result,code in variants:
            with self.subTest(code=code):
                atomic_json(self.directory/'state.json',dict(original,result=result));self.assert_denied_unchanged(code)
        atomic_json(self.directory/'state.json',original)
    def test_historical_signature_original_ref_and_chronology_are_mandatory(self):
        b=self.base;self.finish();b.expire()
        for name,change,code in [
            ('approval.json',lambda v:v.update(signature=base64.b64encode(bytes(64)).decode()),'approval_signature'),
            ('revision.json',lambda v:v.update(revision_id=str(uuid.uuid4())),'service_retention_binding'),
            ('service-custody.json',lambda v:v.update(admittedAt=b.f.body['expires_at']),'untrusted_signer'),
            ('state.json',lambda v:v['result']['runner_execution'].update(exitObservedUtc='2000-01-01T00:00:00Z'),'service_retention_join_invalid')]:
            with self.subTest(name=name):
                p=self.directory/name;old=p.read_bytes();value=decode(old);change(value);atomic_json(p,value)
                try:self.assert_denied_unchanged(code)
                finally:p.write_bytes(old)
    def test_genuine_revoke_and_postwait_unknown_cleanup_never_changes_remote_outcome(self):
        b=self.base;state=self.finish(post_wait=True);old=copy.deepcopy(state['result'])
        head=dict(b.f.head,revoked=True,revocation_epoch=2,issued_at=b.clock-1,expires_at=b.clock+100)
        b.q.record_owner_status(b.policy,service_fixtures.signed(b.f.owner,HEAD_DOMAIN,head),b.runtime.executor)
        b.runtime.close();retired=decode(self.raw());self.assertEqual(retired['cause'],'knownRevocation')
        self.assertEqual(b.q.store.get(self.job)['result'],old)
        self.assertEqual(decode((self.directory/'service-start.json').read_bytes())['targetOutcome'],'uncertain')
    def test_lock_free_orphan_is_not_a_wait_receipt(self):
        b=self.base;child=subprocess.Popen([sys.executable,'-I','-B','-c','import time;time.sleep(30)'],stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL)
        try:
            b.q.store.transition(self.job,'unknown',result={'reason':'helper_restarted'})
            # Close only the real new executor/owner. The deliberately unjoined
            # owned child demonstrates that lock absence is insufficient.
            b.runtime.executor.close();b.helper.close();b.q.close();b.expire()
            scope=ServiceCustody._restore_scope(b.q.root,b.f.pin,b.f.service_id);scope.clock=lambda:b.clock
            old=self.raw()
            with self.assertRaises(RunnerError) as error:retire_existing(scope,b.identity)
            self.assertEqual(error.exception.code,'service_retention_join_missing');self.assertEqual(old,self.raw());self.assertIsNone(child.poll())
        finally:child.terminate();child.wait(timeout=5)
