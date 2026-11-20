import json
from pathlib import Path
import tempfile
import unittest

from consolecrypt_runner.errors import RunnerError
from consolecrypt_runner.input_contract import check_declared_inputs,INPUT_CONTRACT


class InputContractTests(unittest.TestCase):
    def setUp(self):
        self.temp=tempfile.TemporaryDirectory(prefix='cc-input-contract-')
        self.addCleanup(self.temp.cleanup)
        self.root=Path(self.temp.name)

    def write(self,names):
        (self.root/INPUT_CONTRACT).write_text(json.dumps({'version':1,'playbooks':{
            'playbook.yml':{'required_extra_vars':names}}}),encoding='utf-8')

    def test_legacy_projects_are_unchanged_and_only_selected_playbook_inputs_are_required(self):
        self.assertEqual(check_declared_inputs(self.root,'playbook.yml',{}),0)
        self.write(['required_fixture'])
        self.assertEqual(check_declared_inputs(self.root,'other.yml',{}),0)
        self.assertEqual(check_declared_inputs(self.root,'playbook.yml',{'required_fixture':False}),1)
        self.assertEqual(check_declared_inputs(self.root,'playbook.yml',{'required_fixture':0}),1)

    def test_absent_null_or_blank_input_is_denied_without_echoing_other_values(self):
        self.write(['required_fixture'])
        for values in ({'other':'private-marker'},{'required_fixture':None},{'required_fixture':'   '}):
            with self.subTest(values=values),self.assertRaises(RunnerError) as error:
                check_declared_inputs(self.root,'playbook.yml',values)
            self.assertIn('required_fixture',str(error.exception))
            self.assertNotIn('private-marker',str(error.exception))

    def test_future_unknown_fields_duplicates_and_traversal_fail_closed(self):
        path=self.root/INPUT_CONTRACT
        cases=[{'version':2,'playbooks':{}},{'version':True,'playbooks':{}},
               {'version':1,'playbooks':{},'future':True},
               {'version':1,'playbooks':{'../outside':{'required_extra_vars':[]}}}]
        for body in cases:
            path.write_text(json.dumps(body))
            with self.assertRaises(RunnerError):check_declared_inputs(self.root,'playbook.yml',{})
        path.write_text('{"version":1,"version":1,"playbooks":{}}')
        with self.assertRaises(RunnerError):check_declared_inputs(self.root,'playbook.yml',{})
        with self.assertRaises(RunnerError):check_declared_inputs(self.root,'../outside',{})

    def test_authority_names_invalid_identifiers_and_excess_declarations_are_rejected(self):
        for names in (['ansible_host'],['consolecrypt_grant'],['x','x'],['invalid\nname'],[None],
                      ['x'+str(i) for i in range(65)]):
            self.write(names)
            with self.assertRaises(RunnerError):check_declared_inputs(self.root,'playbook.yml',{})
        (self.root/INPUT_CONTRACT).write_bytes(b' '*32769)
        with self.assertRaises(RunnerError):check_declared_inputs(self.root,'playbook.yml',{})

    def test_wrong_case_cannot_silently_lose_required_inputs_on_unix(self):
        (self.root/INPUT_CONTRACT.upper()).write_text('{"version":1,"playbooks":{}}')
        with self.assertRaisesRegex(RunnerError,'exact lowercase'):
            check_declared_inputs(self.root,'playbook.yml',{})
