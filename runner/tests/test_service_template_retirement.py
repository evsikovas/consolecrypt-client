"""Real signatures/private files/locks/worker join; not Native enrollment authority."""
import copy,hashlib,os,subprocess,sys,tempfile,threading,time,unittest,uuid
from concurrent.futures import ThreadPoolExecutor
from pathlib import Path
from unittest.mock import patch
from service_execution_fixtures import Fixture,NOW,signed
from consolecrypt_runner.service.service_authority_v4 import DELEGATION_DOMAIN,HEAD_DOMAIN,verify_delegation
from consolecrypt_runner.service.service_custody import ServiceCustody
from consolecrypt_runner.service.service_runtime import ServiceExecutionRuntime,validate_template_record
from consolecrypt_runner.service.service_retention import retire_joined,retire_existing,bootstrap_retirement
from consolecrypt_runner.__main__ import Helper
from consolecrypt_runner.inventory import StaticInventory
from consolecrypt_runner.parameters import parameters_digest
from consolecrypt_runner.protocol import encode,decode
from consolecrypt_runner.store import atomic_json
from consolecrypt_runner.target_channel import TargetChannel
from consolecrypt_runner.errors import RunnerError

class TemplateRetirementTests(unittest.TestCase):
    def setUp(self):
        self.temp=tempfile.TemporaryDirectory(prefix='owned-template-retention-');self.root=Path(self.temp.name)
        self.f=Fixture(self.root/'schedule');self.clock=NOW
        self.q=ServiceCustody(self.root/'custody',self.f.pin,self.f.service_id,clock=lambda:self.clock)
        self.f.store.close();(self.root/'schedule').rename(self.q.root/'scheduler')
        from consolecrypt_runner.service.scheduler import Store
        self.f.store=Store.open_existing(self.q.root/'scheduler')
        self.helper=Helper(self.q.root,_service_store=self.q.store)
        body=self.f.body
        inventory={'version':1,'hosts':[{'alias':self.f.target['alias'],'host_id':self.f.host,'address':self.f.target['address'],
            'port':22,'username':self.f.target['username'],'variables':{},'sources':['fixture'],'credential_refs':self.f.target['credential_refs']}],
            'groups':{},'source_digests':{'fixture':'b'*64}}
        parameters={'extra_vars':{'owned_fixture_secret':'ONLY_OWNED_RETENTION_CANARY_42'},'tags':[],'skip_tags':[],'diff':False}
        body['inventory_digest']=StaticInventory.from_snapshot(inventory).digest;body['parameters_digest']=parameters_digest(parameters)
        body['execution_origin']={'version':1,'service_id':self.f.service_id,'runner_revision':1,'runner_configuration_digest':'1'*64,
            'signer_key_sha256':hashlib.sha256(bytes.fromhex(body['signer']['public_key'])).hexdigest(),
            'recipient_key_sha256':hashlib.sha256(bytes.fromhex(body['recipient']['public_key'])).hexdigest(),
            'environment_digest':body['environment_digest'],'helper_digest':'f'*64,'known_hosts_digest':'2'*64,'backend':'linuxNativeSsh','network':'serviceLocal'}
        self.policy=verify_delegation(signed(self.f.owner,DELEGATION_DOMAIN,body),self.f.pin,NOW)
        self.signed=decode(signed(self.f.owner,DELEGATION_DOMAIN,body));self.revision=str(uuid.uuid4())
        # Private component INPUT fixture, not a claimed Native/validation ACK.
        # Production validator rechecks actual signature+all input digests.
        self.value={'version':1,'delegation':self.signed,'delegationDigest':self.policy.digest,'revisionId':self.revision,
            'revisionDigest':body['template_digest'],'inventory':inventory,'parameters':parameters,'targets':[self.f.target]}
        atomic_json(self.q.root/'service-template.json',self.value,max_bytes=6*1024*1024)
        self.runtime=ServiceExecutionRuntime(self.q,self.helper,TargetChannel(lambda _:None),body['execution_origin'])
        self.identity=(self.q.root.stat().st_dev,self.q.root.stat().st_ino)
        self.f.head['delegation_digest']=self.policy.digest
        self.q.record_owner_status(self.policy,signed(self.f.owner,HEAD_DOMAIN,self.f.head),self.runtime.executor)
    def tearDown(self):
        try:self.runtime.executor.close()
        finally:
            self.helper.close();self.q.close();self.f.store.close();self.temp.cleanup()
    def data(self):return (self.q.root/'service-template.json').read_bytes()
    def expire(self):self.clock=self.f.body['expires_at']
    def closed_scope(self):
        self.runtime.close();self.helper.close();self.q.close()
        scope=ServiceCustody._restore_scope(self.q.root,self.f.pin,self.f.service_id);scope.clock=lambda:self.clock
        return scope
    def test_actual_expiry_removes_bound_plaintext_only_and_is_idempotent(self):
        before={p.relative_to(self.q.root).as_posix():p.read_bytes() for p in self.q.root.rglob('*') if p.is_file()}
        self.expire();self.runtime.close();value=validate_template_record(self.q.root,self.f.pin,self.f.service_id)
        self.assertEqual(value['version'],2);self.assertEqual(value['cause'],'policyExpired')
        for name in ('inventory','parameters','targets'):self.assertNotIn(name,value)
        self.assertNotIn(b'ONLY_OWNED_RETENTION_CANARY_42',self.data())
        self.assertEqual(value['delegation'],self.signed)
        for relative,raw in before.items():
            if relative!='service-template.json':self.assertEqual((self.q.root/relative).read_bytes(),raw)
        first=self.data();self.runtime.close();self.assertEqual(first,self.data())
        with self.assertRaises(RunnerError):self.runtime._active_template()
    def test_active_policy_and_missing_join_do_not_delete_inputs(self):
        old=self.data();self.runtime.close();self.assertEqual(old,self.data())
        self.expire()
        with self.assertRaises(RunnerError):retire_joined(self.q,self.identity,joined=False)
        self.assertEqual(old,self.data())
    def test_genuine_known_revoke_retires_but_local_boolean_does_not(self):
        path=self.q.root/'service-version.json';original=path.read_bytes();header=decode(original);header['revoked']=True;atomic_json(path,header,max_bytes=8192)
        old=self.data()
        with self.assertRaises(RunnerError):self.runtime.close()
        self.assertEqual(old,self.data());path.write_bytes(original)
        head=dict(self.f.head,revoked=True,revocation_epoch=2,issued_at=NOW-1,expires_at=NOW+100)
        self.q.record_owner_status(self.policy,signed(self.f.owner,HEAD_DOMAIN,head),self.runtime.executor)
        self.runtime.close();value=validate_template_record(self.q.root,self.f.pin,self.f.service_id)
        self.assertEqual(value['cause'],'knownRevocation');self.assertTrue(value['revocationStatus']['statement']['body']['revoked'])
    def test_real_worker_and_child_are_joined_before_any_template_retirement(self):
        self.expire();entered=threading.Event();release=threading.Event()
        def worker():
            child=subprocess.Popen([sys.executable,'-I','-B','-c','import sys;sys.stdin.buffer.read()'],stdin=subprocess.PIPE,stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL)
            entered.set();release.wait(5);child.stdin.close();self.assertEqual(child.wait(timeout=3),0)
        self.runtime.executor.worker.submit(worker);self.assertTrue(entered.wait(3));old=self.data();errors=[]
        def close():
            try:self.runtime.close()
            except BaseException as e:errors.append(e)
        closer=threading.Thread(target=close);closer.start();time.sleep(.03)
        self.assertTrue(closer.is_alive());self.assertEqual(old,self.data());release.set();closer.join(5)
        self.assertFalse(closer.is_alive());self.assertEqual(errors,[]);self.assertEqual(decode(self.data())['version'],2)
    def test_existing_only_refuses_live_owner_then_retires_without_recovery(self):
        self.expire();old=self.data()
        with self.assertRaises((RunnerError,OSError)):retire_existing(self.q,self.identity)
        self.assertEqual(old,self.data())
        # Normal stop while still eligible leaves original inputs, then later
        # expired startup must clean them without opening/recovering JobStore.
        self.clock=NOW;scope=self.closed_scope();self.expire()
        with patch('consolecrypt_runner.store.JobStore._recover_journals',side_effect=AssertionError('no recovery allowed')):
            self.assertTrue(retire_existing(scope,self.identity)['retired'])
        first=self.data();self.assertTrue(retire_existing(scope,self.identity)['alreadyRetired']);self.assertEqual(first,self.data())
    def test_changed_identity_signature_future_schema_and_unknown_worker_are_preserved(self):
        self.clock=NOW;scope=self.closed_scope();self.expire();old=self.data()
        with self.assertRaises(RunnerError):retire_existing(scope,(self.identity[0],self.identity[1]+1))
        self.assertEqual(old,self.data())
        for change in ('future','signature'):
            value=decode(old)
            if change=='future':value['version']=999
            else:value['delegation']['body']['project_id']=str(uuid.uuid4())
            atomic_json(self.q.root/'service-template.json',value,max_bytes=6*1024*1024);current=self.data()
            with self.assertRaises(RunnerError):retire_existing(scope,self.identity)
            self.assertEqual(current,self.data())
        (self.q.root/'service-template.json').write_bytes(old)
        directory=self.q.root/'state/jobs'/str(uuid.uuid4());directory.mkdir(mode=0o700)
        atomic_json(directory/'state.json',{'version':999})
        with self.assertRaises((RunnerError,OSError)):retire_existing(scope,self.identity)
        self.assertEqual(old,self.data())
    def test_private_initial_frame_has_no_executor_target_or_recovery(self):
        self.clock=NOW;self.closed_scope()
        body={'version':1,'owner':{'owner_id':self.f.pin.owner_id,'owner_epoch':1,'tenant':self.f.tenant,'public_key':self.f.pin.public_key.hex(),
            'not_after':self.f.pin.not_after,'minimum_policy_revision':1,'minimum_revocation_epoch':1},
            'origin':self.f.body['execution_origin'],'delegation':self.signed,'revision_id':self.revision,
            'identity':{'identity_id':str(uuid.uuid4()),'key_epoch':1,'signing':self.f.body['signer']['public_key'],'recipient':self.f.body['recipient']['public_key']},
            'runtime_identity':{'device':self.identity[0],'inode':self.identity[1]}}
        # Original policy is genuinely expired under the actual current UTC.
        self.assertGreater(int(time.time()),self.f.body['expires_at'])
        with (patch('consolecrypt_runner.execution.InteractiveExecutor.__init__',side_effect=AssertionError('no executor')),
              patch('consolecrypt_runner.store.JobStore.__init__',side_effect=AssertionError('no admission/recovery'))):
            import io
            from consolecrypt_runner.service.entry import serve
            request_id=str(uuid.uuid4());source=io.BytesIO(encode({'version':1,'request_id':request_id,'operation':'retire_expired_bootstrap','body':body})+b'\n');output=io.BytesIO()
            serve(self.q.root,source,output);response=decode(output.getvalue().strip())
            self.assertEqual(response['request_id'],request_id);self.assertIsNone(response['error']);self.assertTrue(response['result']['retired'])
        self.assertEqual(validate_template_record(self.q.root,self.f.pin,self.f.service_id)['version'],2)

    def test_future_root_artifact_is_preserved_without_template_overwrite(self):
        self.expire();future=self.q.root/'future-owned-metadata.json';future.write_bytes(b'{"version":999}')
        old=self.data()
        with self.assertRaises(RunnerError):self.runtime.close()
        self.assertEqual(old,self.data());self.assertEqual(future.read_bytes(),b'{"version":999}')
    def test_original_scheduler_highwater_rollback_never_deletes_plaintext(self):
        self.expire();old=self.data()
        with self.f.store.transaction():self.f.store.db.execute('UPDATE clock_state SET highwater=? WHERE id=1',(self.clock+1,))
        database=(self.q.root/'scheduler/schedule.sqlite').read_bytes()
        with self.assertRaises(RunnerError):self.runtime.close()
        self.assertEqual(old,self.data());self.assertEqual(database,(self.q.root/'scheduler/schedule.sqlite').read_bytes())
