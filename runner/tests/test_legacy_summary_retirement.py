"""Genuine signed-job/private-file legacy TTL data, not a new execution grant.
Quiescence case tests actual production serialization/projection only; installed
OS proof remains separate, never manufactured by these synthetic metadata.
"""
import copy,hashlib,os,unittest
from consolecrypt_runner import artifact_retention as history
from consolecrypt_runner.protocol import encode,decode
from consolecrypt_runner.store import atomic_json
import test_service_local_join_retention as local_fixtures
import test_service_quiescent_recovery as recovery_fixtures

SECRET='LEGACY_SUMMARY_SECRET_DO_NOT_RETAIN_42'
class LegacySummaryRetirementTests(unittest.TestCase):
    def setUp(self):
        self.f=local_fixtures.ServiceLocalJoinRetentionTests();self.f.setUp();self.addCleanup(self.f.doCleanups);self.b=self.f.base
        self.state=self.f.finish();self.alias=self.f.manifest['targets'][0]['alias']
        raw=copy.deepcopy(self.state['result'])
        raw['summary']={'status':'successful','exit_code':0,'stdout':SECRET,'arbitrary':{SECRET:SECRET},
            'stats':{'ok':{self.alias:1,'unapproved-alias':999},'failures':{self.alias:0},'extra':{self.alias:SECRET}}}
        self.state=dict(self.state,result=raw);atomic_json(self.f.directory/'state.json',self.state)
        self.original=copy.deepcopy(raw);self.original_hash=hashlib.sha256(encode(raw)).hexdigest()
        self.now=history.timestamp(self.state['history_activity_at'])+history.SECONDS+1
        self.assertIn(SECRET,(self.f.directory/'state.json').read_text())
    def assert_retained(self,state):
        self.assertEqual(state['state'],'unknown')
        self.assertEqual({k:v for k,v in state['result'].items() if k!='summary'},
            {k:v for k,v in self.original.items() if k!='summary'})
        self.assertEqual(state['result']['summary'],{'status':'successful','exit_code':0,
            'stats':{'ok':{self.alias:1},'failures':{self.alias:0}}})
        marker=state['artifact_retirement'];self.assertEqual(marker['original_result_sha256'],self.original_hash)
        self.assertEqual(marker['retained_result_sha256'],hashlib.sha256(encode(state['result'])).hexdigest())
        self.assertEqual(state['sequence'],self.state['sequence']);self.assertEqual(state['output_bytes'],self.state['output_bytes'])
        for p in self.b.q.root.rglob('*.json'):self.assertNotIn(SECRET,p.read_text(),p.name)
        self.assertNotIn(SECRET,encode(state).decode());self.assertFalse((self.f.directory/'events.jsonl').exists())
    def test_legacy_guest_summary_is_removed_from_state_and_audit_without_changing_outer_outcome(self):
        state,deleted=history.retire(self.f.directory,self.state,self.now)
        self.assertTrue(deleted);self.assert_retained(state)
        again,deleted=history.retire(self.f.directory,state,self.now+1)
        self.assertFalse(deleted);self.assertEqual(again,state)
    def test_existing_old_retirement_marker_upgrades_raw_summary_without_resetting_cursor_or_hash(self):
        fd=os.open(self.f.directory,os.O_RDONLY|os.O_DIRECTORY|os.O_NOFOLLOW|os.O_CLOEXEC)
        try:_,used,sequence,digest=history._events(fd,self.f.job)
        finally:os.close(fd)
        marker={'version':1,'retired_at':self.now,'activity_at':history.timestamp(self.state['history_activity_at']),
            'last_sequence':sequence,'output_bytes':used,'events_sha256':digest,'cause':'age_limit'}
        old=dict(self.state,artifact_retirement=marker);atomic_json(self.f.directory/'state.json',old)
        state,deleted=history.retire(self.f.directory,old,self.now)
        self.assertTrue(deleted);self.assert_retained(state)
        self.assertEqual({k:state['artifact_retirement'][k] for k in marker},marker)
    def test_quiescent_summary_projection_keeps_original_raw_result_hash_without_copying_raw_diagnosis(self):
        # Reuse the already-authored closed metadata fixture. This is not a
        # forged OS-positive: no producer, stop or raw revision erase is called.
        metadata=recovery_fixtures.QuiescentRecoveryTests.metadata(self)
        recovered=history.recover_quiescent(self.f.directory,self.state,metadata,self.now)
        self.assertEqual(recovered['local_quiescence']['original_result_sha256'],self.original_hash)
        state,deleted=history.retire(self.f.directory,recovered,self.now,cause='service_quiescent')
        self.assertTrue(deleted);self.assert_retained(state)
        self.assertEqual(state['local_quiescence']['original_result_sha256'],self.original_hash)
        history.validate_state(state)

    def test_changed_legacy_journal_denies_before_any_summary_or_state_rewrite(self):
        from consolecrypt_runner.errors import RunnerError
        fd=os.open(self.f.directory,os.O_RDONLY|os.O_DIRECTORY|os.O_NOFOLLOW|os.O_CLOEXEC)
        try:_,used,sequence,digest=history._events(fd,self.f.job)
        finally:os.close(fd)
        marker={'version':1,'retired_at':self.now,'activity_at':history.timestamp(self.state['history_activity_at']),
            'last_sequence':sequence,'output_bytes':used,'events_sha256':digest,'cause':'age_limit'}
        old=dict(self.state,artifact_retirement=marker);atomic_json(self.f.directory/'state.json',old)
        state_before=(self.f.directory/'state.json').read_bytes()
        path=self.f.directory/'events.jsonl';raw=path.read_bytes();changed=raw.replace(b'owned old raw diagnostic',b'owned new raw diagnostic')
        self.assertNotEqual(raw,changed);self.assertEqual(len(raw),len(changed));path.write_bytes(changed)
        with self.assertRaises(RunnerError) as error:history.retire(self.f.directory,old,self.now)
        self.assertEqual(error.exception.code,'history_corrupt')
        self.assertEqual((self.f.directory/'state.json').read_bytes(),state_before);self.assertEqual(path.read_bytes(),changed)
