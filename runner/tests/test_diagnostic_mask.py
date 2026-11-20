import copy
import json
from dataclasses import replace
from pathlib import Path
import tempfile
import threading
import time
import unittest
from consolecrypt_runner.diagnostic_mask import KnownJobSecrets,project_event
from consolecrypt_runner.errors import RunnerError
from consolecrypt_runner.store import JobStore
from consolecrypt_runner.target_channel import TargetChannel
import test_approval_store as fixtures

PASSWORD='owned-known-password-42'
TOKEN='owned-token-\\\"-42'
PEM='-----BEGIN PRIVATE KEY-----\nowned-private-pem-body\n-----END PRIVATE KEY-----'

def fixture():
    f=fixtures.ApprovalFixture();f.setUp();now=int(time.time());f.now=now
    f.manifest.update(issued_at=now,expires_at=now+600,access_expires_at=now+3600)
    f.grants[f.signer]=replace(f.grants[f.signer],expires_at=now+3600)
    return f

def parameters():return {'extra_vars':{'password':PASSWORD,'token':TOKEN,'pem':PEM,'short':1},'tags':[],'skip_tags':[],'diff':False}
class ParentProjection:
    # Data-component identity projection, NOT a Native grant or success bridge.
    def _redact_event(self,verified,kind,payload,**kwargs):return copy.deepcopy(payload)
class MissingProjection:
    def _redact_event(self,*args,**kwargs):raise OSError('untrusted error '+PASSWORD)
class DiagnosticMaskTests(unittest.TestCase):
    def test_known_password_token_pem_masked_before_first_real_journal_append(self):
        f=fixture();verified=f.verify();mask=KnownJobSecrets(parameters())
        raw={'res':{'stdout':PASSWORD+' '+TOKEN+' '+PEM,'value':1},'task':'visible'}
        with tempfile.TemporaryDirectory() as folder,JobStore(Path(folder)/'state') as store:
            store.accept(verified);kind,safe=project_event(ParentProjection(),verified,'runner_on_ok',raw,mask,cancel=threading.Event())
            store.append_event(verified.value['job_id'],kind,safe)
            serialized=(store.job_directory(verified.value['job_id'])/'events.jsonl').read_text()
            for secret in (PASSWORD,TOKEN,'owned-private-pem-body'):self.assertNotIn(secret,serialized)
            self.assertEqual(safe['res']['value'],'[REDACTED]');self.assertIn('[REDACTED]',serialized)
            for name in ('manifest.json','state.json'):
                content=(store.job_directory(verified.value['job_id'])/name).read_text()
                self.assertNotIn(PASSWORD,content);self.assertNotIn('mask_values',content)
        mask.close()
    def test_no_log_facts_and_invocation_are_removed_before_callback_or_store(self):
        f=fixture();mask=KnownJobSecrets(parameters());seen=[]
        class Inspect:
            def _redact_event(self,verified,kind,payload,**kwargs):seen.append(payload);return payload
        kind,safe=project_event(Inspect(),f.verify(),'runner_on_ok',{'res':{'results':[{'_ansible_no_log':True,'stdout':'arbitrary-sensitive'}],
            'ansible_facts':{'secret':'fact-sensitive'},'invocation':{'password':'input-sensitive'}}},mask,cancel=threading.Event())
        rendered=json.dumps([seen,safe]);self.assertNotIn('arbitrary-sensitive',rendered);self.assertNotIn('fact-sensitive',rendered);self.assertNotIn('input-sensitive',rendered)
        self.assertEqual(safe['res']['results'][0],{'censored':True});mask.close()
    def test_unavailable_native_or_closed_capture_withholds_payload_and_error(self):
        f=fixture();mask=KnownJobSecrets(parameters())
        for channel in (MissingProjection(),ParentProjection()):
            if isinstance(channel,ParentProjection):mask.close()
            _,safe=project_event(channel,f.verify(),'runner_on_failed',{'res':{'stdout':PASSWORD}},mask,cancel=threading.Event())
            self.assertEqual(safe,{'censored':True,'reason':'privacy_unavailable'})
        mask.close()
    def test_structural_outcome_comes_from_original_not_censor_and_preserves_counter(self):
        f=fixture();verified=f.verify();alias=verified.value['targets'][0]['alias'];mask=KnownJobSecrets(parameters())
        class Wrong:
            def _redact_event(self,*args,**kwargs):return {'status':'successful','exit_code':0,'stats':{'ok':{alias:999}}}
        _,safe=project_event(Wrong(),verified,'runner_summary',{'status':'failed','exit_code':1,'stats':{'failures':{alias:1,'untrusted-host':3}}},mask,cancel=threading.Event())
        self.assertEqual(safe,{'status':'failed','exit_code':1,'stats':{'failures':{alias:1}}});mask.close()
    def test_own_buffers_wipe_without_claiming_python_copy_erasure(self):
        mask=KnownJobSecrets(parameters());held=list(mask.values);self.assertTrue(any(PASSWORD.encode() in v for v in held));mask.close()
        self.assertTrue(all(not any(v) for v in held));self.assertFalse(mask.live);self.assertEqual(mask.values,[])
    def test_marker_overlap_unicode_and_truncated_known_prefix_fail_closed(self):
        mask=KnownJobSecrets({'extra_vars':{'a':'[','b':']','c':'aba','d':'ab'}})
        self.assertEqual(mask.text('[abaab]'),'[REDACTED]'*4)
        self.assertEqual(mask.text('private-prefix [TRUNCATED]'),'[CENSORED]');mask.close()
    def test_guest_callable_channel_rejects_redact_operation_before_any_native_request(self):
        f=fixture();sent=[];channel=TargetChannel(sent.append)
        try:
            with self.assertRaises(RunnerError):channel.call(f.verify(),f.host,'redact_event',{'event_type':'runner_on_ok','payload':{}},cancel=threading.Event())
            self.assertEqual(sent,[])
        finally:channel.close()
    def test_private_parent_mask_timeout_does_not_revoke_job_or_exhaust_pending(self):
        f=fixture();sent=[];channel=TargetChannel(sent.append)
        try:
            with self.assertRaises(RunnerError):channel._redact_event(f.verify(),'runner_on_ok',{},cancel=threading.Event(),timeout=.01)
            self.assertEqual([v['operation'] for v in sent],['redact_event']);self.assertEqual(channel.pending,{})
            self.assertFalse(any(v['type']=='target_revoke' for v in sent))
        finally:channel.close()

if __name__=='__main__':unittest.main()
