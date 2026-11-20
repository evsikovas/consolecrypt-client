"""Actual signed policy + snapshot/report/SQLite/JobStore; not Native dispatch."""
import copy,hashlib,os,stat,unittest
from pathlib import Path
from unittest.mock import patch
import service_execution_fixtures as execution_fixture
from service_execution_fixtures import Fixture,NOW,signed
from consolecrypt_runner.service.scheduler import Store,Clock,SchedulerError
from consolecrypt_runner.service.service_resume import preflight,schedule_check,current
from consolecrypt_runner.service.service_authority_v4 import HEAD_DOMAIN
from consolecrypt_runner.service.service_custody import ServiceCustody
from consolecrypt_runner.errors import RunnerError
from consolecrypt_runner.protocol import encode,decode

class ResumeTests(unittest.TestCase):
    def setUp(self):
        # Reuse the real existing signature/snapshot/report fixture, before its
        # explicit reserve action. No fake execution/Ready/capsule witness.
        with patch.object(Fixture,'reserve',return_value=None):execution_fixture.ServiceExecutionFixture.setUp(self)
        self.schedule={'id':self.f.schedule,'revision':1}
        # Definition.encoded is the existing strict wire representation.
        self.schedule['definition']=decode(next(v.definition for v in self.f.store.schedules() if v.schedule_id==self.f.schedule).encoded().encode())
        self.origin=self.f.body['execution_origin']
    def tearDown(self):execution_fixture.ServiceExecutionFixture.tearDown(self)
    def tree(self):
        return {p.relative_to(self.q.root).as_posix():p.read_bytes() for p in self.q.root.rglob('*') if stat.S_ISREG(p.lstat().st_mode)}
    def checked(self):
        result=preflight(self.q,self.signed,self.origin,self.revision)
        schedule_check(self.f.store,self.schedule,self.q,self.signed,self.origin)
        return result
    def test_resume_after_original_head_expiry_does_not_renew_or_write(self):
        self.assertGreater(self.clock,self.f.head['expires_at'])
        before=self.tree();header=before['service-version.json']
        self.checked();ack=self.runtime.resume(self.signed,self.f.store,self.revision)
        self.assertTrue(ack['enrolled']);self.assertEqual(before,self.tree())
        self.assertEqual(header,self.tree()['service-version.json']);self.assertEqual(self.worker.calls,[])
    def test_each_missing_component_is_rejected_without_creation(self):
        for relative in ['service-version.json','service-template.json','scheduler/schedule.sqlite','state/.lock','state/.owner','state/jobs','state/nonces','revisions']:
            path=self.q.root/relative;held=path.with_name(path.name+'.owned-held')
            path.rename(held)
            try:
                before=self.tree()
                with self.assertRaises((RunnerError,OSError,SchedulerError)):self.checked()
                self.assertFalse(path.exists());self.assertEqual(before,self.tree())
            finally:held.rename(path)
    def test_known_revocation_denies_before_snapshot_or_recovery(self):
        head=dict(self.f.head,revoked=True,revocation_epoch=2,issued_at=self.clock-1,expires_at=self.clock+100)
        self.q.record_owner_status(self.f.delegation,signed(self.f.owner,HEAD_DOMAIN,head),self.runtime.executor)
        before=self.tree()
        with patch('consolecrypt_runner.snapshot.verify_snapshot',side_effect=AssertionError('revoke must precede filesystem walk')):
            with self.assertRaises(RunnerError) as raised:self.checked()
        self.assertEqual(raised.exception.code,'service_revoked');self.assertEqual(before,self.tree());self.assertEqual(self.worker.calls,[])
    def test_clock_and_policy_floor_and_expiry_never_reset(self):
        from consolecrypt_runner.store import atomic_json
        path=self.q.root/'service-version.json';original=path.read_bytes()
        for change in [{'clockFloor':self.clock+1},{'policyFloor':2},{'revocationFloor':2}]:
            header=decode(original);header.update(change);atomic_json(path,header,max_bytes=8192)
            before=self.tree()
            with self.assertRaises(RunnerError):self.checked()
            self.assertEqual(before,self.tree());path.write_bytes(original)
        self.clock=self.f.body['expires_at'];before=self.tree()
        with self.assertRaises(RunnerError):self.checked()
        self.assertEqual(before,self.tree())
    def test_reserved_original_slot_stays_blocked_with_same_token_and_bytes(self):
        self.f.reserve();before=self.tree()
        with self.assertRaises(RunnerError) as raised:self.checked()
        self.assertEqual(raised.exception.code,'service_slot_unresolved');self.assertEqual(before,self.tree())
        self.assertIsNotNone(self.f.store.unresolved(self.f.schedule,1));self.assertEqual(before,self.tree())
    def test_future_template_and_header_are_not_rewritten(self):
        for relative in ['service-version.json','service-template.json']:
            path=self.q.root/relative;old=path.read_bytes();value=decode(old);value['version']=99;path.write_bytes(encode(value))
            try:
                before=self.tree()
                with self.assertRaises(RunnerError):self.checked()
                self.assertEqual(before,self.tree())
            finally:path.write_bytes(old)
    def test_foreign_origin_and_altered_snapshot_deny_without_worker(self):
        before=self.tree()
        with self.assertRaises(RunnerError):preflight(self.q,self.signed,dict(self.origin,network='desktop'),self.revision)
        self.assertEqual(before,self.tree())
        (self.report/'project/playbook.yml').write_text('different\n');before=self.tree()
        with self.assertRaises(RunnerError):self.checked()
        self.assertEqual(before,self.tree());self.assertEqual(self.worker.calls,[])
    def test_existing_database_open_never_creates_or_adopts_missing_or_empty(self):
        root=self.root/'missing-scheduler'
        with self.assertRaises((OSError,SchedulerError)):Store.open_existing(root)
        self.assertFalse(root.exists())
        root.mkdir(mode=0o700);file=root/'schedule.sqlite';file.write_bytes(b'');file.chmod(0o600)
        with self.assertRaises(SchedulerError):Store.open_existing(root)
        self.assertEqual(file.read_bytes(),b'');self.assertEqual([p.name for p in root.iterdir()],['schedule.sqlite'])
    def test_future_database_or_definition_denies_before_jobstore_recovery(self):
        db=self.f.store.db;path=self.q.root/'scheduler/schedule.sqlite'
        db.execute('PRAGMA user_version=2');before=path.read_bytes()
        with self.assertRaises(SchedulerError):self.f.store.preflight_existing()
        self.assertEqual(before,path.read_bytes());db.execute('PRAGMA user_version=1')
        old=db.execute('SELECT definition FROM schedules WHERE id=?',(self.f.schedule,)).fetchone()[0]
        future=decode(old.encode());future['version']=2;db.execute('UPDATE schedules SET definition=? WHERE id=?',(encode(future).decode(),self.f.schedule));before=self.tree()
        with self.assertRaises(SchedulerError):self.checked()
        self.assertEqual(before,self.tree())
    def test_interrupted_or_unknown_upload_metadata_is_preserved(self):
        upload=next(self.q.root.glob('uploads-*'));(upload/'unknown-future').write_bytes(b'preserve')
        before=self.tree()
        with self.assertRaises(RunnerError):self.checked()
        self.assertEqual(before,self.tree())


    def bootstrap_frame(self):
        import uuid
        return {'version':1,'request_id':str(uuid.uuid4()),'operation':'service_resume_bootstrap','body':{
            'version':1,'owner':{'owner_id':self.f.pin.owner_id,'owner_epoch':self.f.pin.owner_epoch,
                'tenant':dict(self.f.pin.tenant),'public_key':self.f.pin.public_key.hex(),'not_after':self.f.pin.not_after,
                'minimum_policy_revision':1,'minimum_revocation_epoch':1},
            'origin':self.origin,'schedule':self.schedule,'delegation':self.signed,'revision_id':self.revision}}
    def test_actual_existing_bootstrap_and_resume_keep_original_header_and_database(self):
        import io,uuid
        from consolecrypt_runner.service.entry import serve
        self.runtime.close();self.helper.close();self.q.close();self.f.store.close()
        first=self.bootstrap_frame();second={'version':1,'request_id':str(uuid.uuid4()),'operation':'service_resume','body':{'delegation':self.signed,'revision_id':self.revision}}
        import threading
        acknowledged=threading.Event()
        class UntilAck(io.BytesIO):
            def readline(stream,*args):
                line=super().readline(*args)
                if not line:
                    if not acknowledged.wait(10):raise AssertionError('Actual response before owned EOF deadline')
                return line
        class ObservedOutput(io.BytesIO):
            def write(stream,raw):
                count=super().write(raw)
                if decode(raw)['request_id']==second['request_id']:acknowledged.set()
                return count
        source=UntilAck(encode(first)+b'\n'+encode(second)+b'\n');output=ObservedOutput();before=self.tree()
        original_init=ServiceCustody.__init__
        def original_fixture_clock(scope,*args,**kwargs):
            kwargs['clock']=lambda:self.clock
            original_init(scope,*args,**kwargs)
        # The existing constructor's default clock captured the builtin at
        # module import; inject the SAME synthetic fixture clock explicitly.
        with patch('time.time',return_value=self.clock),patch.object(ServiceCustody,'__init__',original_fixture_clock):
            serve(self.q.root,source,output)
        replies=[decode(raw) for raw in output.getvalue().splitlines()]
        self.assertEqual(len(replies),2);self.assertEqual(replies[1]['error'],None)
        self.assertTrue(replies[0]['result']['bootstrapped']);self.assertTrue(replies[1]['result']['enrolled'])
        self.assertEqual(replies[1]['error'],None);self.assertEqual(before,self.tree());self.assertEqual(self.worker.calls,[])
    def test_actual_future_schema_bootstrap_cannot_construct_or_recover_jobstore(self):
        import io
        from consolecrypt_runner.service.entry import serve
        self.runtime.close();self.helper.close();self.q.close();self.f.store.close()
        path=self.q.root/'service-template.json';original=path.read_bytes();value=decode(original);value['version']=2;path.write_bytes(encode(value));before=self.tree()
        try:
            with patch('time.time',return_value=self.clock),patch('consolecrypt_runner.service.service_custody.CustodyStore',side_effect=AssertionError('future preflight must precede store construction')):
                with self.assertRaises(RunnerError):serve(self.q.root,io.BytesIO(encode(self.bootstrap_frame())+b'\n'),io.BytesIO())
            self.assertEqual(before,self.tree())
        finally:
            # Restore only this owned synthetic corruption AFTER its strict
            # preservation check; ordinary teardown must not adopt future data.
            path.write_bytes(original)
