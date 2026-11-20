import copy
import json
import os
from pathlib import Path
import tempfile
import unittest

from consolecrypt_runner.errors import RunnerError
from consolecrypt_runner.project_requirements import REQUIREMENTS_FILE, check_project_requirements, parse_requirements
from consolecrypt_runner.dependencies import LOCK_FILE


def document():
    return {'version': 1, 'playbooks': {'playbook.yml': {
        'required_extra_vars': ['fixture_input'],
        'required_dependencies': [{'kind': 'role', 'name': 'demo', 'version': '1.2.3'}]}}}


class ReviewedRequirementsTests(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory(prefix='cc-reviewed-requirements-')
        self.addCleanup(temporary.cleanup)
        self.project = Path(temporary.name)

    def save(self, value):
        (self.project / REQUIREMENTS_FILE).write_text(json.dumps(value))

    def test_only_explicit_selected_requirements_apply_and_values_never_appear_in_diagnostics(self):
        self.assertEqual(check_project_requirements(self.project, 'playbook.yml', {}), 0)
        value = document(); value['playbooks']['playbook.yml']['required_dependencies'] = []
        self.save(value)
        self.assertEqual(check_project_requirements(self.project, 'other.yml', {}), 0)
        for values in ({'other': 'private-marker'}, {'fixture_input': None}, {'fixture_input': '  '}):
            with self.assertRaises(RunnerError) as error:
                check_project_requirements(self.project, 'playbook.yml', values)
            self.assertIn('fixture_input', str(error.exception)); self.assertNotIn('private-marker', str(error.exception))
        for valid in (0, False, 'set'):
            self.assertEqual(check_project_requirements(self.project, 'playbook.yml', {'fixture_input': valid}), 1)

    def test_missing_or_different_pin_never_satisfies_proposed_dependency(self):
        self.save(document())
        variables = {'fixture_input': 'set'}
        with self.assertRaisesRegex(RunnerError, 'dependency lock'):
            check_project_requirements(self.project, 'playbook.yml', variables)
        lock = {'version': 1, 'dependencies': [{'kind': 'role', 'name': 'demo', 'version': '2.0.0',
            'source': {'url': 'https://example.test/archive.tar.gz', 'sha256': 'a'*64}, 'tree_digest': 'b'*64}]}
        (self.project/LOCK_FILE).write_text(json.dumps(lock))
        with self.assertRaisesRegex(RunnerError, 'pinned version'):
            check_project_requirements(self.project, 'playbook.yml', variables)
        lock['dependencies'][0]['version'] = '1.2.3'
        (self.project/LOCK_FILE).write_text(json.dumps(lock))
        self.assertEqual(check_project_requirements(self.project, 'playbook.yml', variables), 2)

    def test_schema_authority_names_duplicate_pins_and_future_fields_fail_closed(self):
        def spec(value): return value['playbooks']['playbook.yml']
        for mutation in (
            lambda v: v.update(version=2), lambda v: v.update(version=True),
            lambda v: v.update(unknown=True), lambda v: v.update(playbooks={'../outside': spec(v)}),
            lambda v: spec(v).update(required_extra_vars=['ansible_host']),
            lambda v: spec(v).update(required_extra_vars=['consolecrypt_scope']),
            lambda v: spec(v).update(required_extra_vars=['x', 'x']),
            lambda v: spec(v).update(required_extra_vars=['x']*65),
            lambda v: spec(v).update(required_dependencies=spec(v)['required_dependencies']*2),
            lambda v: spec(v)['required_dependencies'][0].update(version='latest'),
            lambda v: spec(v)['required_dependencies'][0].update(name='../escape'),
            lambda v: spec(v)['required_dependencies'][0].update(kind='collection', name='ansible.builtin'),
            lambda v: spec(v)['required_dependencies'][0].update(url='https://example.test'),
        ):
            value = copy.deepcopy(document()); mutation(value)
            with self.subTest(value=value), self.assertRaises(RunnerError): parse_requirements(json.dumps(value).encode())
        for raw in (b'{"version":1,"version":1,"playbooks":{}}', b' '*65537):
            with self.assertRaises(RunnerError): parse_requirements(raw)

    @unittest.skipUnless(os.name == 'posix', 'Unix no-follow contract reads')
    def test_links_special_files_and_wrong_case_are_never_ignored_or_followed(self):
        outside = self.project/'source.json'; outside.write_text(json.dumps(document()))
        target = self.project/REQUIREMENTS_FILE
        target.symlink_to(outside)
        with self.assertRaises(OSError): check_project_requirements(self.project, 'playbook.yml', {})
        target.unlink(); os.link(outside, target)
        with self.assertRaises(RunnerError): check_project_requirements(self.project, 'playbook.yml', {})
        target.unlink(); os.mkfifo(target)
        with self.assertRaises(RunnerError): check_project_requirements(self.project, 'playbook.yml', {})
        target.unlink(); target.with_name(target.name.upper()).write_text(json.dumps(document()))
        with self.assertRaisesRegex(RunnerError, 'exact lowercase'):
            check_project_requirements(self.project, 'playbook.yml', {})
