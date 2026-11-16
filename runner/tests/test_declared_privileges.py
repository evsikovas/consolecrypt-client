import copy
import importlib.util
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
from types import SimpleNamespace
import unittest
from unittest.mock import patch
from contextlib import nullcontext

from consolecrypt_runner.declared_privileges import (PREFIX, MAX_BYTES, extract_summary, summarize_entries, valid_summary)
from consolecrypt_runner.validation import ValidationReport


def summary(node):
    return summarize_entries([{'plays': [SimpleNamespace(get_ds=lambda: node)]}])


class DeclaredPrivilegesTests(unittest.TestCase):
    def test_direct_declarations_never_return_literal_users_templates_or_custom_methods(self):
        value = summary({'become': True, 'become_user': 'PRIVATE_ACCOUNT', 'become_method': 'sudo',
            'vars': {'become': 'PRIVATE_VAR'}, 'tasks': [
                {'become': False, 'become_user': '{{ PRIVATE_EXPRESSION }}', 'become_method': 'PRIVATE_PLUGIN', 'debug': {'msg': 'PRIVATE_TASK'}},
                {'block': [{'become': '{{ PRIVATE_BOOL }}', 'become_method': '{{ PRIVATE_METHOD }}'}]},
            ]})
        self.assertTrue(valid_summary(value))
        self.assertEqual(value['records'][0]['become'], 'enabled')
        self.assertEqual(value['records'][0]['user'], 'literal_hidden')
        self.assertEqual(value['records'][0]['knownMethod'], 'sudo')
        self.assertEqual(value['records'][1]['become'], 'disabled')
        self.assertEqual(value['records'][1]['user'], 'dynamic')
        self.assertIsNone(value['records'][1]['knownMethod'])
        self.assertEqual(value['records'][2]['indices'], [2, 1])
        self.assertEqual(value['records'][2]['branches'], ['block'])
        self.assertEqual(value['records'][2]['become'], 'dynamic')
        self.assertNotIn('PRIVATE', json.dumps(value))

    def test_oversized_scalars_are_unknown_without_template_scan_or_module_key_split(self):
        class LongScalar(str):
            def __contains__(self, token):
                raise AssertionError('oversized scalar must not be scanned')
            def rsplit(self, *args):
                raise AssertionError('oversized key must not be split')
        value = summary({'become': LongScalar('PRIVATE'*100), 'become_user': LongScalar('PRIVATE'*100),
            'become_method': LongScalar('PRIVATE'*100), LongScalar('PRIVATE_KEY'*100): {}})
        self.assertTrue(valid_summary(value))
        row = value['records'][0]
        self.assertEqual((row['become'],row['user'],row['method']),('unknown','unknown','unknown'))
        self.assertIn('shape',value['unresolved'])
        self.assertNotIn('PRIVATE',json.dumps(value))

    def test_no_declaration_is_not_no_escalation_and_role_include_overrides_remain_unresolved(self):
        value = summary({'roles': ['PRIVATE_ROLE'], 'tasks': [{'include_tasks': '{{ PRIVATE_PATH }}'}]})
        self.assertEqual(value['records'], [])
        self.assertEqual(value['unresolved'], ['roles', 'runtime_overrides', 'task_includes'])
        self.assertNotIn('PRIVATE', json.dumps(value))
        self.assertEqual(summary({'become': 'false'})['records'][0]['become'], 'unknown')
        self.assertEqual(summary({'become': 0})['records'][0]['become'], 'unknown')

    def test_row_node_and_depth_budgets_are_explicit_and_no_dropped_data_claims_completeness(self):
        rows = summary({'tasks': [{'become': True}] * 65})
        self.assertEqual(len(rows['records']), 64)
        self.assertTrue(rows['truncated'])
        nodes = summary({'tasks': [{}] * 4097})
        self.assertTrue(nodes['truncated'])
        wide = summary({str(i): {} for i in range(129)})
        self.assertTrue(wide['truncated'])
        self.assertIn('shape', wide['unresolved'])
        deep = {'become': True}
        for _ in range(20):
            deep = {'block': [deep]}
        value = summary({'tasks': [deep]})
        self.assertTrue(value['truncated'])
        self.assertTrue(valid_summary(value))

    def test_only_exact_single_closed_frame_is_available_and_it_never_changes_diagnostics(self):
        value = summary({'become': True})
        line = PREFIX + json.dumps(value) + '\n'
        parsed, diagnostics = extract_summary('ordinary syntax diagnostics\n' + line, completed=True)
        self.assertEqual(parsed, value)
        self.assertEqual(diagnostics, 'ordinary syntax diagnostics\n')
        for output in [line + line, PREFIX + '{bad}\n', PREFIX + 'x' * (MAX_BYTES + 1),
                       PREFIX + json.dumps(value).replace('"version": 1', '"version": 1, "version": 1')]:
            parsed, _ = extract_summary(output, completed=True)
            self.assertIsNone(parsed)
        self.assertIsNone(extract_summary(line, completed=False)[0])
        # This is informational guest data, not a trusted pipe: a forged valid
        # frame can be accepted as advisory and must never affect valid/checks.
        self.assertEqual(extract_summary(line, completed=True)[0], value)

    def test_unknown_fields_shapes_versions_or_private_literals_are_discarded(self):
        value = summary({'become': True})
        cases = []
        for key, item in [('version', True), ('version', 2), ('source', 'future'), ('truncated', 1),
                          ('unresolved', []), ('rawVars', {'secret': 'PRIVATE'})]:
            other = copy.deepcopy(value)
            other[key] = item
            cases.append(other)
        for key, item in [('user', 'PRIVATE_ACCOUNT'), ('knownMethod', 'PRIVATE_PLUGIN'), ('indices', [0]),
                          ('branches', ['PRIVATE']), ('play', True), ('raw', 'PRIVATE')]:
            other = copy.deepcopy(value)
            other['records'][0][key] = item
            cases.append(other)
        for other in cases:
            self.assertFalse(valid_summary(other))
            self.assertIsNone(extract_summary(PREFIX + json.dumps(other), completed=True)[0])

    def test_malformed_structure_is_unknown_and_metadata_is_optional_on_old_reports(self):
        self.assertIsNone(summarize_entries([{}]))
        value = summary({'tasks': 'PRIVATE_NOT_A_LIST'})
        self.assertIn('shape', value['unresolved'])
        report = ValidationReport('a'*64, 'b'*64, (), False, 'playbook.yml', None, None, None)
        self.assertNotIn('declared_privileges', report.to_json())
        report = ValidationReport('a'*64, 'b'*64, (), False, 'playbook.yml', None, None, None, value)
        self.assertEqual(report.to_json()['declared_privileges'], value)
        self.assertFalse(report.to_json()['valid'])


class ExistingValidationTransportTests(unittest.TestCase):
    def run_report(self, *, vault=False, valid=True):
        from consolecrypt_runner import validation
        from consolecrypt_runner.sandbox import ProcessResult
        value = summary({'become': True, 'become_user': 'PRIVATE_USER'})
        syntax = ProcessResult(0 if valid else 1, PREFIX + json.dumps(value) + '\nsyntax diagnostics', False, False, False, 1)
        lint = ProcessResult(0, 'lint diagnostics', False, False, False, 1)
        with tempfile.TemporaryDirectory(prefix='cc-declared-transport-') as temporary:
            root = Path(temporary)
            snapshot = SimpleNamespace(entries=[{'path': 'playbook.yml', 'type': 'file'}],digest='a'*64)
            with patch.object(validation, 'probe', return_value={'capabilities': {'validate': True}, 'environment_digest': 'b'*64}), \
                 patch.object(validation, 'verify_snapshot') as verify, \
                 patch.object(validation, 'bubblewrap_argv', return_value=['trusted-existing-worker','--']), \
                 patch.object(validation, 'run_bounded', side_effect=[syntax, lint]):
                broker = SimpleNamespace(client_script=root/'client', directory=root/'broker')
                report = validation.validate_project(root,snapshot,'playbook.yml',private_directory=root,
                    vault_phase_factory=(lambda name: nullcontext(broker)) if vault else None)
                self.assertEqual(verify.call_count,2)
        return report

    def test_existing_report_transport_adds_advisory_without_changing_checks_or_validity(self):
        report = self.run_report()
        self.assertTrue(report.valid)
        self.assertTrue(valid_summary(report.declared_privileges))
        self.assertEqual(report.checks[0]['output'],'syntax diagnostics')
        self.assertNotIn('PRIVATE',json.dumps(report.to_json()))
        failed = self.run_report(valid=False)
        self.assertFalse(failed.valid)
        self.assertIsNone(failed.declared_privileges)
        self.assertNotIn('declared_privileges',failed.to_json())

    def test_vault_context_strips_advisory_frame_and_never_persists_decrypted_declaration_metadata(self):
        report = self.run_report(vault=True)
        self.assertTrue(report.valid)
        self.assertIsNone(report.declared_privileges)
        self.assertNotIn('declared_privileges',report.to_json())
        self.assertNotIn(PREFIX,report.checks[0]['output'])


@unittest.skipUnless(sys.platform == 'linux' and importlib.util.find_spec('ansible'), 'Existing pinned offline runtime')
class ActualParsedAnsibleTests(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory(prefix='cc-declared-')
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name)
        (self.root/'inventory.json').write_text(json.dumps({'all': {'hosts': {'approved': {}}}}))
        (self.root/'variables.json').write_text('{}')
        self.marker = self.root/'MUST_NOT_EXECUTE'

    def probe(self, text, *, callback_failure=False):
        (self.root/'playbook.yml').write_text(text)
        paths = [str(path) for path in __import__('consolecrypt_runner').__path__]
        bootstrap = "import sys;sys.path.insert(0,'/main/runner/src');import consolecrypt_runner;consolecrypt_runner.__path__[:]=%r\n" % paths
        code = """from pathlib import Path
from consolecrypt_runner.validation_worker import check_playbook_targets
from consolecrypt_runner.declared_privileges import emit_summary
cb = (lambda value: (_ for _ in ()).throw(ValueError('PRIVATE_CALLBACK'))) if %r else emit_summary
counts = check_playbook_targets('playbook.yml',Path('inventory.json'),Path('variables.json'),{'approved'},on_declared=cb)
print('COUNTS', counts)
""" % callback_failure
        environment = dict(os.environ, HOME=str(self.root/'home'), ANSIBLE_HOME=str(self.root/'.ansible'),
            ANSIBLE_SSH_AGENT='none', ANSIBLE_FORCE_COLOR='False', ANSIBLE_INVENTORY_ENABLED='host_list,yaml,ini')
        result = subprocess.run([sys.executable, '-I', '-c', bootstrap+code],cwd=self.root,env=environment,
            stdout=subprocess.PIPE,stderr=subprocess.STDOUT,timeout=20,check=False)
        self.assertFalse(self.marker.exists(), 'No task executor or target execution permitted')
        output = result.stdout.decode('utf-8',errors='replace')
        self.assertEqual(result.returncode,0,output)
        self.assertIn('COUNTS (1, 1)',output)
        return extract_summary(output,completed=True)[0]

    def play(self):
        return """- hosts: all
  gather_facts: false
  become: true
  become_user: PRIVATE_ACCOUNT
  become_method: sudo
  tasks:
    - ansible.builtin.shell: touch %s
      become: false
      become_user: '{{ PRIVATE_VARIABLE }}'
    - block:
        - ansible.builtin.debug: {msg: PRIVATE_MESSAGE}
          become_method: '{{ PRIVATE_METHOD }}'
      become: true
""" % self.marker

    def test_real_pinned_loaded_plays_emit_bounded_declarations_without_running_tasks(self):
        value = self.probe(self.play())
        self.assertIsNotNone(value)
        self.assertEqual(value['records'][0]['become'],'enabled')
        self.assertEqual(value['records'][0]['knownMethod'],'sudo')
        self.assertTrue(any(row['user']=='dynamic' for row in value['records']))
        self.assertNotIn('PRIVATE',json.dumps(value))

    def test_existing_imported_play_resolution_is_reused_and_not_new_role_task_execution(self):
        (self.root/'imported.yml').write_text(self.play())
        value = self.probe('- import_playbook: imported.yml\n')
        self.assertTrue(valid_summary(value))
        self.assertEqual(value['records'][0]['play'],1)

    def test_optional_summary_failure_does_not_change_existing_target_validation(self):
        self.assertIsNone(self.probe(self.play(),callback_failure=True))


if __name__ == '__main__':
    unittest.main()
