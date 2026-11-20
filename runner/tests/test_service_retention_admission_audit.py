"""Actual automatic store admission + retention; audit DATA never grants work."""
import unittest,uuid
from consolecrypt_runner.admission_audit import derive
from consolecrypt_runner.errors import RunnerError
from consolecrypt_runner.protocol import decode,encode
import test_service_local_join_retention as fixtures

class RetentionAuditCompatibilityTests(unittest.TestCase):
    def setUp(self):
        self.f=fixtures.ServiceLocalJoinRetentionTests();self.f.setUp();self.addCleanup(self.f.doCleanups)
        self.b=self.f.base;self.f.finish();self.b.expire()
        self.audit=self.f.directory/'admission.json';self.assertTrue(self.audit.is_file(),'actual verify+store.accept produced audit automatically')
    def test_original_actual_admission_sidecar_is_preserved_during_raw_cleanup(self):
        old=self.audit.read_bytes();self.b.runtime.close()
        self.assertEqual(decode(self.f.raw())['version'],2);self.assertEqual(old,self.audit.read_bytes())
        self.assertEqual(decode(old)['signer_id'],self.b.f.service_id)
    def test_same_schema_foreign_signer_is_denied_without_deleting_or_replacing_metadata(self):
        old=self.audit.read_bytes();value=decode(old);self.audit.write_bytes(derive(self.f.manifest,value['manifest_digest'],str(uuid.uuid4()),None))
        changed=self.audit.read_bytes();raw=self.f.raw()
        try:
            with self.assertRaises(RunnerError) as error:self.b.runtime.close()
            self.assertEqual(error.exception.code,'service_retention_binding');self.assertEqual(raw,self.f.raw());self.assertEqual(changed,self.audit.read_bytes())
        finally:self.audit.write_bytes(old)
    def test_future_audit_version_stays_unchanged_and_blocks_raw_retirement(self):
        old=self.audit.read_bytes();value=decode(old);value['version']=999;self.audit.write_bytes(encode(value));changed=self.audit.read_bytes();raw=self.f.raw()
        try:
            with self.assertRaises(RunnerError) as error:self.b.runtime.close()
            self.assertEqual(error.exception.code,'admission_audit');self.assertEqual(raw,self.f.raw());self.assertEqual(changed,self.audit.read_bytes())
        finally:self.audit.write_bytes(old)
