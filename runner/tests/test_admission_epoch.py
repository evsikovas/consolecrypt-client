"""Real signature/snapshot/store admission; a paused check races vault locking."""
from concurrent.futures import ThreadPoolExecutor
import base64
import hashlib
from pathlib import Path
import tempfile
import threading
import time
from types import SimpleNamespace
import unittest
from unittest.mock import patch
import uuid

from cryptography.hazmat.primitives.asymmetric.ed25519 import Ed25519PrivateKey
from consolecrypt_runner.approval import DOMAIN
from consolecrypt_runner.errors import RunnerError
from consolecrypt_runner.execution import InteractiveExecutor
from consolecrypt_runner.inventory import StaticInventory
from consolecrypt_runner.parameters import parameters_digest
from consolecrypt_runner.probe import probe
from consolecrypt_runner.protocol import encode
from consolecrypt_runner.snapshot import create_snapshot, verify_snapshot
from consolecrypt_runner.store import JobStore
from consolecrypt_runner.target_channel import TargetChannel
from consolecrypt_runner.validation_reports import write_report


class CapturedWorker:
    def __init__(self): self.submissions = 0
    def submit(self, *args): self.submissions += 1
    def shutdown(self, **kwargs): pass


class AdmissionEpochTests(unittest.TestCase):
    def setUp(self):
        temporary=tempfile.TemporaryDirectory(prefix='cc-admission-epoch-')
        self.addCleanup(temporary.cleanup)
        root=Path(temporary.name)
        source=root/'source';source.mkdir()
        (source/'playbook.yml').write_text('---\n- name: Epoch test\n  hosts: all\n  gather_facts: false\n  tasks: []\n')
        snapshot_root=root/'revision'/'project';snapshot_root.parent.mkdir()
        snapshot=create_snapshot(source,snapshot_root)
        self.store=JobStore(root/'state');self.addCleanup(self.store.close)
        executor=InteractiveExecutor(SimpleNamespace(revision=lambda _: (snapshot_root,snapshot)),TargetChannel(lambda _:None),self.store)
        executor.worker.shutdown(wait=True);executor.worker=CapturedWorker()
        self.executor=executor;self.addCleanup(executor.close)
        key=Ed25519PrivateKey.generate()
        tenant,runner,project,host,signer=[str(uuid.uuid4()) for _ in range(5)]
        now=int(time.time())
        environment=probe()['environment_digest']
        target={'host_id':host,'alias':'fixture','address':'example.test','port':22,'username':'runner',
                'route_digest':'a'*64,'credential_refs':[]}
        inventory={'version':1,'hosts':[{'alias':'fixture','host_id':host,'address':'example.test','port':22,
                   'username':'runner','variables':{},'sources':['fixture'],'credential_refs':[]}],
                   'groups':{},'source_digests':{'fixture':'b'*64}}
        parameters={'extra_vars':{},'tags':[],'skip_tags':[],'diff':False}
        manifest={'version':1,'job_id':str(uuid.uuid4()),'tenant_id':tenant,'runner_id':runner,'project_id':project,
                  'revision_digest':snapshot.digest,'environment_digest':environment,'inventory_digest':StaticInventory.from_snapshot(inventory).digest,
                  'parameters_digest':parameters_digest(parameters),'playbook':'playbook.yml','mode':'check',
                  'policy':{'strict_isolation':True,'cancel_on_lock':False,'timeout_seconds':60,'forks':1,'production':False},
                  'targets':[target],'issued_at':now,'expires_at':now+600,'access_expires_at':now+60,'nonce':str(uuid.uuid4())}
        report={name:manifest[name] for name in ('revision_digest','environment_digest','inventory_digest','parameters_digest','playbook')}
        report.update(valid=True,targets_digest=hashlib.sha256(encode(manifest['targets'])).hexdigest())
        write_report(snapshot_root.parent,report)
        executor.configure({'tenant_id':tenant,'runner_id':runner,'signer_id':signer,
            'public_key':base64.b64encode(key.public_key().public_bytes_raw()).decode(),
            'hosts':[host],'projects':[project],'expires_at':now+3500,'max_job_seconds':60,'transport':'native_scoped_rpc_v1'})
        executor.vault_state(False,1)
        self.body={'approval':{'signer_id':signer,'manifest':manifest,'signature':base64.b64encode(key.sign(DOMAIN+encode(manifest))).decode()},
                   'revision_id':str(uuid.uuid4()),'inventory':inventory,'parameters':parameters,'vault_generation':1}

    def assert_not_admitted(self):
        self.assertEqual(list((self.store.root/'jobs').iterdir()),[])
        self.assertEqual(self.executor.worker.submissions,0)

    def test_lock_during_verification_prevents_durable_admission_even_for_keep_running_policy(self):
        entered=threading.Event();release=threading.Event()
        def paused(*args):
            result=verify_snapshot(*args)
            entered.set()
            if not release.wait(10):raise RuntimeError('Admission race test timed out')
            return result
        with patch('consolecrypt_runner.execution.verify_snapshot',side_effect=paused),ThreadPoolExecutor(1) as worker:
            future=worker.submit(self.executor.accept,self.body)
            try:
                self.assertTrue(entered.wait(10))
                self.executor.vault_state(True,2)
            finally:release.set()
            with self.assertRaises(RunnerError) as error:future.result(timeout=10)
            self.assertEqual(error.exception.code,'vault_locked')
        self.assert_not_admitted()

    def test_unlock_does_not_revive_an_admission_from_an_old_generation(self):
        self.executor.vault_state(True,2);self.executor.vault_state(False,3)
        with self.assertRaises(RunnerError) as error:self.executor.accept(self.body)
        self.assertEqual(error.exception.code,'vault_generation')
        self.assert_not_admitted()

    def test_current_generation_durably_accepts_exactly_once(self):
        accepted=self.executor.accept(self.body)
        self.assertTrue(accepted['created'])
        repeated=self.executor.accept(self.body)
        self.assertFalse(repeated['created'])
        self.assertEqual(self.executor.worker.submissions,1)
        evidence=accepted['job']['environment']
        self.assertEqual(evidence['environment_digest'],self.body['approval']['manifest']['environment_digest'])
        self.assertEqual(evidence['identity']['packages']['ansible-core']['actual'],'2.20.11')
        self.assertEqual(repeated['job']['environment'],evidence)

    def test_empty_grant_can_read_own_history_but_cannot_start_a_new_job(self):
        self.executor.accept(self.body)
        history = InteractiveExecutor(self.executor.helper, self.executor.channel, self.store)
        self.addCleanup(history.close)
        old = self.executor
        grant = next(iter(old.grants.values()))
        approval = self.body['approval']
        history.configure({'tenant_id':old.identity['tenant_id'],'runner_id':old.identity['runner_id'],
            'signer_id':approval['signer_id'], 'public_key':base64.b64encode(grant.public_key).decode(),
            'hosts':[], 'projects':[], 'expires_at':int(time.time())+60, 'max_job_seconds':60,
            'transport':'native_scoped_rpc_v1'})
        job_id = approval['manifest']['job_id']
        history.check_owner(job_id)
        self.assertEqual(self.store.events(job_id,after=0,limit=10)['job_id'],job_id)
        with self.assertRaises(RunnerError): history.accept(self.body)
        self.assertEqual(old.worker.submissions,1)
        history.identity = dict(history.identity, tenant_id=str(uuid.uuid4()))
        with self.assertRaises(RunnerError): history.check_owner(job_id)
