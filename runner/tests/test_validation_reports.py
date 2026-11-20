from pathlib import Path
import tempfile
import unittest
from consolecrypt_runner.errors import RunnerError
from consolecrypt_runner.validation_reports import read_report,write_report

class ValidationReportsTests(unittest.TestCase):
    def setUp(self):
        temporary=tempfile.TemporaryDirectory(prefix='cc-validation-records-')
        self.addCleanup(temporary.cleanup);self.root=Path(temporary.name)
        self.report={'revision_digest':'a'*64,'environment_digest':'b'*64,'playbook':'playbook.yml',
            'parameters_digest':'c'*64,'inventory_digest':'d'*64,'targets_digest':'e'*64,'checks':[],'valid':True}
    def test_different_parameters_or_targets_do_not_overwrite_a_previous_review(self):
        first=dict(self.report);second=dict(first,parameters_digest='f'*64)
        third=dict(first,targets_digest='0'*64)
        self.assertNotEqual(write_report(self.root,first),write_report(self.root,second))
        write_report(self.root,third)
        for expected in (first,second,third):self.assertEqual(read_report(self.root,expected),expected)
    def test_private_witnesses_and_unbounded_review_combinations_are_rejected(self):
        with self.assertRaises(RunnerError):write_report(self.root,dict(self.report,input_witnesses={'secret':'private input'}))
        for number in range(32):write_report(self.root,dict(self.report,parameters_digest=f'{number:064x}'))
        with self.assertRaises(RunnerError) as error:write_report(self.root,dict(self.report,parameters_digest=f'{32:064x}'))
        self.assertEqual(error.exception.code,'validation_quota')
        write_report(self.root,dict(self.report,parameters_digest=f'{0:064x}'))
        self.assertEqual(len(list((self.root/'validations').iterdir())),32)
