import base64
import json
from pathlib import Path
import tempfile
import threading
import time
import unittest
import uuid
from unittest.mock import patch

from consolecrypt_runner.__main__ import Helper
from consolecrypt_runner.approval import VerifiedManifest
from consolecrypt_runner.errors import RunnerError
from consolecrypt_runner.execution import InteractiveExecutor
from consolecrypt_runner.protocol import encode
import test_approval_store as fixtures


class JobAdmissionViewTests(unittest.TestCase):
    def setUp(self):
        self.fixture=fixtures.ApprovalFixture('test_signature_binds_every_field_and_verified_bytes_are_immutable')
        self.fixture.setUp()
        temporary=tempfile.TemporaryDirectory(prefix='cc-job-audit-view-');self.addCleanup(temporary.cleanup)
        self.helper=Helper(Path(temporary.name)/'helper');self.addCleanup(self.helper.close)
        self.helper.executor=InteractiveExecutor(self.helper,None,self.helper.store)
        self.configuration={'tenant_id':self.fixture.tenant,'runner_id':self.fixture.runner,'signer_id':self.fixture.signer,
            'public_key':base64.b64encode(self.fixture.key.public_key().public_bytes_raw()).decode(),
            'hosts':[self.fixture.host],'projects':[self.fixture.project],'expires_at':int(time.time())+300,
            'max_job_seconds':300,'transport':'native_scoped_rpc_v1'}

    def configure(self,**changes):
        self.helper.request('configure_interactive',self.configuration|changes,threading.Event())

    def read(self):
        return self.helper.request('job_get',{'job_id':self.fixture.manifest['job_id']},threading.Event())

    def test_actual_verified_admission_is_returned_after_owner_check_without_job_or_file_write(self):
        verified=self.fixture.verify();self.helper.store.accept(verified);self.configure()
        directory=self.helper.store.job_directory(self.fixture.manifest['job_id'])
        before={p.name:p.read_bytes()for p in directory.iterdir()if p.is_file()}
        state=self.helper.store.get(self.fixture.manifest['job_id']);response=self.read()
        self.assertEqual(response.pop('admission_audit'),{'version':1,'status':'recorded','record':json.loads(verified.admission_audit)})
        self.assertEqual(response,state)
        self.assertEqual({p.name:p.read_bytes()for p in directory.iterdir()if p.is_file()},before)

    def test_corrupt_audit_is_unavailable_without_losing_the_actual_job_result(self):
        self.helper.store.accept(self.fixture.verify());self.configure()
        directory=self.helper.store.job_directory(self.fixture.manifest['job_id']);path=directory/'admission.json'
        value=json.loads(path.read_bytes());value['scope']='secret-injected-purpose';path.write_bytes(encode(value));raw=path.read_bytes()
        actual=self.helper.store.get(self.fixture.manifest['job_id']);response=self.read()
        self.assertEqual(response.pop('admission_audit'),{'version':1,'status':'unavailable'})
        self.assertEqual(response,actual);self.assertEqual(path.read_bytes(),raw)
        self.assertNotIn('secret-injected-purpose',json.dumps(response))

    def test_legacy_audit_missing_is_not_backfilled_by_the_current_caller(self):
        v=self.fixture.verify();old=VerifiedManifest(v.canonical,v.digest,v.signer_id)
        self.helper.store.accept(old);self.configure()
        self.assertEqual(self.read()['admission_audit'],{'version':1,'status':'unrecorded'})
        self.assertFalse((self.helper.store.job_directory(self.fixture.manifest['job_id'])/'admission.json').exists())

    def test_foreign_profile_is_denied_before_any_audit_read(self):
        self.helper.store.accept(self.fixture.verify());self.configure(tenant_id=str(uuid.uuid4()))
        with patch.object(self.helper.store,'admission_audit',side_effect=AssertionError('audit read before owner fence')):
            with self.assertRaises(RunnerError):self.read()

    def test_audit_paths_or_scope_in_request_are_rejected(self):
        self.helper.store.accept(self.fixture.verify());self.configure()
        with self.assertRaises(RunnerError):
            self.helper.request('job_get',{'job_id':self.fixture.manifest['job_id'],'admission_audit':'/unowned'},threading.Event())
