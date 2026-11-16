"""Retained version evidence is bound to approval, bounded and detached."""
import copy
import hashlib
from pathlib import Path
import tempfile
import unittest

from consolecrypt_runner.errors import RunnerError
from consolecrypt_runner.probe import environment_receipt
from consolecrypt_runner.protocol import encode
from consolecrypt_runner.store import JobStore
import test_approval_store as approval_tests


def report():
    identity={'helper':'0.1.0','protocol':1,'python':'3.12.3','system':'Linux','machine':'x86_64',
              'packages':{'ansible-core':{'expected':'2.20.11','actual':'2.20.11','compatible':True}},
              'installed':{'ansible-core':'2.20.11'},'helper_digest':'a'*64,
              'dependency_lock':'b'*64,'dependency_files_digest':'c'*64}
    return dict(identity,environment_digest=hashlib.sha256(encode(identity)).hexdigest(),
                process_environment={'API_TOKEN':'not-for-history'},home='/private/login/home')


class ReceiptTests(unittest.TestCase):
    def test_only_approved_identity_is_retained_and_mutation_does_not_change_it(self):
        source=report();receipt=environment_receipt(source,source['environment_digest'])
        self.assertNotIn(b'not-for-history',encode(receipt))
        self.assertNotIn(b'/private/login/home',encode(receipt))
        source['installed']['ansible-core']='changed'
        self.assertEqual(receipt['identity']['installed']['ansible-core'],'2.20.11')
        with self.assertRaises(RunnerError):environment_receipt(source,source['environment_digest'])
        with self.assertRaises(RunnerError):environment_receipt(report(),'d'*64)
        with self.assertRaises(RunnerError):environment_receipt({},'d'*64)

    def test_oversize_report_is_rejected_even_with_a_correct_digest(self):
        source=report();source['installed']['oversize']='x'*(64*1024)
        identity={k:v for k,v in source.items() if k not in ('environment_digest','process_environment','home')}
        source['environment_digest']=hashlib.sha256(encode(identity)).hexdigest()
        with self.assertRaisesRegex(RunnerError,'exceeds limit'):
            environment_receipt(source,source['environment_digest'])


class RetainedReceiptTests(unittest.TestCase):
    def test_reopen_keeps_original_versions_and_mismatch_admits_nothing(self):
        fixture=approval_tests.ApprovalFixture();fixture.setUp()
        source=report();fixture.manifest['environment_digest']=source['environment_digest']
        verified=fixture.verify(environment_digest=source['environment_digest'])
        with tempfile.TemporaryDirectory() as folder:
            root=Path(folder)/'state'
            with JobStore(root) as store:
                changed=copy.deepcopy(source);changed['python']='3.13.0'
                with self.assertRaises(RunnerError):store.accept(verified,environment=changed)
                self.assertEqual(list((root/'jobs').iterdir()),[])
                self.assertEqual(list((root/'nonces').iterdir()),[])
                first,created=store.accept(verified,environment=source)
                self.assertTrue(created)
                self.assertEqual(store.accept(verified,environment=source),(first,False))
                store.transition(first['job_id'],'running')
                store.transition(first['job_id'],'succeeded',result={'exit_code':0})
            with JobStore(root) as reopened:
                retained=reopened.get(first['job_id'])
                self.assertEqual(retained['environment'],first['environment'])
                self.assertEqual(retained['result'],{'exit_code':0})
                self.assertFalse(reopened.accept(verified,environment=source)[1])
