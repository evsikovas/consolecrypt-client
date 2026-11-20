"""Actual pinned Ansible/bwrap output -> trusted-parent projection -> JobStore.
The data-only identity callback here is not a Native credential/SSH proof.
Native opaque-capture/signature/expiry tests and final owned SSH job stay separate.
"""
from dataclasses import replace
import json
from pathlib import Path
import sys
import tempfile
import threading
import time
import unittest
from consolecrypt_runner.diagnostic_mask import KnownJobSecrets,project_event
from consolecrypt_runner.protocol import encode
from consolecrypt_runner.ephemeral_input import EphemeralJobInput
from consolecrypt_runner.sandbox import bubblewrap_argv,run_bounded
from consolecrypt_runner.store import JobStore
from consolecrypt_runner.execution import InteractiveExecutor
from consolecrypt_runner.snapshot import create_snapshot
from consolecrypt_runner.probe import probe
from consolecrypt_runner.broker import JobBroker
from consolecrypt_runner.target_channel import TargetChannel
import test_approval_store as fixtures

class RuntimeMaskTests(unittest.TestCase):
    def fixture(self):
        f=fixtures.ApprovalFixture();f.setUp();now=int(time.time());f.now=now
        f.manifest.update(issued_at=now,expires_at=now+600,access_expires_at=now+3600)
        f.grants[f.signer]=replace(f.grants[f.signer],expires_at=now+3600);return f
    def test_actual_ansible_known_values_no_log_diff_and_facts_before_private_journal(self):
        password='owned-actual-password42';token='owned-actual-token42';pem='-----BEGIN PRIVATE KEY-----\nowned-actual-pem-body\n-----END PRIVATE KEY-----'
        parameters={'extra_vars':{'known_password':password,'known_token':token,'known_pem':pem},'tags':[],'skip_tags':[],'diff':True}
        with tempfile.TemporaryDirectory(prefix='cc-mask-runtime-') as folder:
            root=Path(folder);project=root/'project';project.mkdir()
            (project/'playbook.yml').write_text("""---
- hosts: fixture
  gather_facts: false
  tasks:
    - name: Explicit unprotected data for parent known masking
      ansible.builtin.debug:
        msg: "{{ known_password }} {{ known_token }} {{ known_pem }}"
    - name: Sensitive task keeps no_log and diff false
      ansible.builtin.copy:
        dest: /work/private-sensitive.txt
        content: "{{ known_token }}"
      no_log: true
      diff: false
    - name: Facts stay absent from diagnostics
      ansible.builtin.set_fact:
        sensitive_fact: undisclosed-fact-marker
""")
            body=encode({'playbook':'playbook.yml','mode':'apply','forks':5,'parameters':parameters,'inventory':{'all':{'hosts':{'fixture':{'ansible_connection':'local'}}}}})
            with EphemeralJobInput(body) as spec:
                result=run_bounded(bubblewrap_argv(project,[sys.executable,'-m','consolecrypt_runner.execution_worker'],job_input=spec),timeout=30,job_input=spec)
            self.assertEqual(result.exit_code,0,result.output)
            events=[json.loads(line) for line in result.output.splitlines()]
            self.assertIn(password,result.output,'prove output reached the real parent, not only no_log suppression')
            self.assertNotIn('undisclosed-fact-marker',result.output)
            class ParentOnly:
                def __init__(self):self.seen=[]
                def _redact_event(self,verified,kind,payload,**kwargs):
                    self.seen.append((kind,payload));return payload
                def revoke(self,verified):pass
            # Exercise the REAL execution.run callback and first durable append,
            # not a test-side copy of its projection/persistence ordering.
            snapshot=create_snapshot(project,root/'frozen-project')
            f=self.fixture();f.manifest['revision_digest']=snapshot.digest
            f.manifest['environment_digest']=probe()['environment_digest']
            verified=f.verify(environment_digest=f.manifest['environment_digest'])
            channel=ParentOnly()
            with JobStore(root/'state') as store:
                store.accept(verified)
                executor=InteractiveExecutor(None,channel,store)
                try:
                    executor.run(verified,root/'frozen-project',snapshot,
                        {'all':{'hosts':{'fixture':{'ansible_connection':'local'}}}},parameters,threading.Event())
                    state=store.get(verified.value['job_id'])
                    self.assertEqual(state['state'],'succeeded',state)
                    self.assertTrue(channel.seen,'real worker diagnostics reached actual parent callback')
                    raw=(store.job_directory(verified.value['job_id'])/'events.jsonl').read_text()
                    for secret in (password,token,'owned-actual-pem-body','undisclosed-fact-marker'):
                        self.assertNotIn(secret,raw);self.assertNotIn(secret,json.dumps(channel.seen))
                    self.assertIn('[REDACTED]',raw)
                    for name in ('manifest.json','state.json'):
                        text=(store.job_directory(verified.value['job_id'])/name).read_text();self.assertNotIn(password,text);self.assertNotIn(token,text)
                finally:executor.close()
    def test_actual_guest_socket_cannot_use_parent_only_mask_callback_or_observe_reply(self):
        f=self.fixture();verified=f.verify();sent=[];channel=TargetChannel(sent.append)
        try:
            with tempfile.TemporaryDirectory(prefix='cc-mask-broker-') as folder:
                root=Path(folder);project=root/'project';project.mkdir()
                script="""import json,socket,sys
with socket.socket(socket.AF_UNIX,socket.SOCK_STREAM) as s:
 s.connect('/run/consolecrypt/broker.sock')
 s.sendall((json.dumps({'version':1,'host_id':sys.argv[1],'operation':'redact_event','body':{'event_type':'runner_on_ok','payload':{'res':{'stdout':'credential-guess'}}}})+'\\n').encode())
 print(s.makefile('rb').readline().decode())
"""
                with JobBroker(root/'transport',verified,channel,threading.Event()):
                    result=run_bounded(bubblewrap_argv(project,[sys.executable,'-c',script,f.host],socket_directory=root/'transport'),timeout=10)
                    self.assertEqual(result.exit_code,0,result.output);response=json.loads(result.output);self.assertIsNotNone(response['error'])
            self.assertFalse(any(frame['type']=='target_request' for frame in sent),'guest operation never reached Native parent callback')
        finally:channel.close()

if __name__=='__main__':unittest.main()
