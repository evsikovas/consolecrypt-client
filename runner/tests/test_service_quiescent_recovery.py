"""Current recovery/retirement metadata unit tests, NOT OS-quiescence proof.
The installed systemd/pidfd/cgroup positive remains a separate owned VM gate.
"""
import copy,hashlib,time,unittest,uuid
from consolecrypt_runner import artifact_retention as history
from consolecrypt_runner.errors import RunnerError
from consolecrypt_runner.protocol import encode
from consolecrypt_runner.store import atomic_json
import test_service_local_join_retention as fixtures

class QuiescentRecoveryTests(unittest.TestCase):
    def setUp(self):
        self.f=fixtures.ServiceLocalJoinRetentionTests();self.f.setUp();self.addCleanup(self.f.doCleanups)
        self.b=self.f.base;self.old=self.b.q.store.get(self.f.job)
        self.now=history.timestamp(self.old['updated_at'])+history.SECONDS+1
    def metadata(self):
        root=self.b.q.root.stat()
        return {'version':1,'source':'systemdInactiveCgroupAbsent','identity':str(uuid.uuid4()),'policy_digest':self.b.policy.digest,
            'runtime_device':root.st_dev,'runtime_inode':root.st_ino,'identity_device':root.st_dev,'identity_inode':root.st_ino,
            'cgroup_parent_device':root.st_dev,'cgroup_parent_inode':root.st_ino,'cgroup_device':0,'cgroup_inode':0,
            'daemon_pid':0,'observed_at':self.now}
    def test_unknown_recovery_preserves_original_result_state_evidence_cursor_and_nonce(self):
        original_result={'reason':'owned_original_diagnosis','remote_changes_may_be_partial':True}
        state=dict(self.old,result=original_result);atomic_json(self.f.directory/'state.json',state)
        nonce={p.name:p.read_bytes() for p in (self.b.q.root/'state/nonces').iterdir()}
        recovered=history.recover_quiescent(self.f.directory,state,self.metadata(),self.now)
        self.assertEqual(recovered['state'],'unknown');self.assertEqual(recovered['result'],original_result)
        self.assertEqual(recovered['local_quiescence']['original_state'],'running')
        self.assertEqual(recovered['local_quiescence']['original_updated_at'],state['updated_at'])
        self.assertEqual(recovered['history_activity_at'],state['history_activity_at'])
        self.assertEqual(recovered['sequence'],state['sequence'])
        retired,deleted=history.retire(self.f.directory,recovered,self.now,cause='service_quiescent')
        self.assertTrue(deleted);self.assertEqual(retired['state'],'unknown');self.assertEqual(retired['result'],original_result)
        self.assertEqual(set(history.view(retired)),{'version','retired_at','last_sequence','output_bytes'})
        self.assertEqual({p.name:p.read_bytes() for p in (self.b.q.root/'state/nonces').iterdir()},nonce)
        for name in ('manifest.json','approval.json','revision.json','service-custody.json','service-start.json'):self.assertTrue((self.f.directory/name).exists())
    def test_queued_recovery_is_interrupted_and_never_a_synthetic_worker_join(self):
        state=dict(self.old,state='queued',result=None);atomic_json(self.f.directory/'state.json',state)
        result=history.recover_quiescent(self.f.directory,state,self.metadata(),self.now)
        self.assertEqual(result['state'],'interrupted');self.assertIsNone(result['result'])
        self.assertEqual(result['local_quiescence']['original_state'],'queued')
        self.assertNotIn('runner_execution',result['local_quiescence'])
        self.assertEqual(history.recover_quiescent(self.f.directory,result,self.metadata(),self.now),result)
    def test_early_ttl_and_future_observation_never_delete_raw_journal(self):
        meta=self.metadata();meta['observed_at']=history.timestamp(self.old['updated_at'])+1
        result=history.recover_quiescent(self.f.directory,self.old,meta,meta['observed_at'])
        unchanged,deleted=history.retire(self.f.directory,result,self.now-2,cause='service_quiescent')
        self.assertFalse(deleted);self.assertNotIn('artifact_retirement',unchanged);self.assertTrue((self.f.directory/'events.jsonl').exists())
        bad=copy.deepcopy(result);bad['local_quiescence']['version']=True
        with self.assertRaises(RunnerError):history.validate_state(bad)
    def test_changed_original_diagnosis_or_false_terminal_receipt_is_rejected(self):
        result=history.recover_quiescent(self.f.directory,self.old,self.metadata(),self.now)
        for change in ('result','success','pid'):
            value=copy.deepcopy(result)
            if change=='result':value['result']={'replacement':'not-original'}
            elif change=='success':value['state']='succeeded'
            else:value['local_quiescence']['daemon_pid']=123
            with self.subTest(change=change),self.assertRaises(RunnerError):history.validate_state(value)
