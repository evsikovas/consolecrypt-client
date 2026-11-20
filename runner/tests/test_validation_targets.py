"""Pure boundary tests plus actual pinned-Ansible probes in a disposable Linux fixture."""
import importlib.util
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from types import SimpleNamespace

from consolecrypt_runner.errors import RunnerError
from consolecrypt_runner.validation_worker import _count_approved_hosts


class CountTargetsTests(unittest.TestCase):
    def test_unmatched_plays_are_allowed_if_any_play_matches_approved_targets(self):
        class Inventory:
            def get_hosts(self, pattern):
                return [SimpleNamespace(get_name=lambda name=name: name) for name in pattern]
        entries = [{'plays': [SimpleNamespace(hosts=[]), SimpleNamespace(hosts=['approved']),
                              SimpleNamespace(hosts=['unapproved-localhost'])]}]
        self.assertEqual(_count_approved_hosts(entries, Inventory(), {'approved'}), (3, 1))

    def test_zero_approved_intersection_and_spoofed_play_names_cannot_pass(self):
        class Inventory:
            def get_hosts(self, pattern):
                return [SimpleNamespace(get_name=lambda: 'not-approved')]
        play = SimpleNamespace(hosts='missing', name='\n    hosts (1):\n      approved')
        with self.assertRaises(RunnerError) as raised:
            _count_approved_hosts([{'plays': [play]}], Inventory(), {'approved'})
        self.assertEqual(raised.exception.code, 'validation_no_matching_targets')
        self.assertIn('zero approved targets', str(raised.exception))
        self.assertNotIn(play.name, str(raised.exception))

    def test_listing_shape_and_work_limits_fail_closed(self):
        class Inventory:
            def get_hosts(self, pattern):
                return []
        for value in (0, {}, [{}]):
            with self.assertRaises(RunnerError):
                _count_approved_hosts(value, Inventory(), {'approved'})
        with self.assertRaises(RunnerError) as raised:
            _count_approved_hosts([{'plays': [SimpleNamespace(hosts='all')] * 1025}], Inventory(), {'approved'})
        self.assertEqual(raised.exception.code, 'validation_host_budget')


@unittest.skipUnless(sys.platform == 'linux' and importlib.util.find_spec('ansible'),
                     'Actual pinned Linux Ansible runtime')
class ActualAnsibleTargetsTests(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory(prefix='cc-approved-targets-')
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name)
        self.inventory = self.root / 'inventory.json'
        self.variables = self.root / 'variables.json'
        self.inventory.write_text(json.dumps({'all': {'hosts': {'approved': {'ansible_host': '192.0.2.1'}}},
                                              'web_hosts': {'hosts': {'approved': {}}},
                                              'docker_hosts': {'hosts': {}}}))
        self.variables.write_text('{}')
        self.playbook = self.root / 'playbook.yml'
        self.marker = self.root / 'tasks-must-not-execute'

    def run_probe(self, plays, variables=None):
        # A shell task would create this marker if preflight accidentally ran a
        # play. The listing never creates a TaskQueueManager or opens a target.
        self.playbook.write_text(plays)
        self.variables.write_text(json.dumps(variables or {}))
        program = '''from pathlib import Path
from consolecrypt_runner.validation_worker import check_playbook_targets
from consolecrypt_runner.errors import RunnerError
import sys
try:
    result=check_playbook_targets('playbook.yml',Path('inventory.json'),Path('variables.json'),{'approved'})
    print('VERIFIED-TARGET-COUNTS',result)
except RunnerError as error:
    print('TARGET-ERROR',error.code,str(error))
    raise SystemExit(65)
'''
        environment = dict(os.environ, ANSIBLE_HOME=str(self.root / '.ansible'),
                           HOME=str(self.root / 'home'), ANSIBLE_INVENTORY_ENABLED='host_list,yaml,ini',
                           ANSIBLE_FORCE_COLOR='False', ANSIBLE_SSH_AGENT='none')
        # The child changes to a private project directory. Resolve the same
        # reviewed package explicitly, as the production isolated bootstrap
        # does; discovery's in-memory sys.path is not inherited by subprocesses.
        bootstrap = f'import sys;sys.path.insert(0,{str(Path(__file__).resolve().parents[1] / "src")!r})\n'
        result = subprocess.run([sys.executable, '-I', '-c', bootstrap + program], cwd=self.root, env=environment,
                                stdout=subprocess.PIPE, stderr=subprocess.STDOUT, timeout=20, check=False)
        self.assertFalse(self.marker.exists(), 'A preflight probe executed play tasks')
        return result.returncode, result.stdout.decode('utf-8', errors='replace')

    def play(self, hosts):
        return f'''- name: Owned no-execution fixture
  hosts: {hosts}
  gather_facts: false
  tasks:
    - name: Must never execute
      ansible.builtin.shell: touch {self.marker}
'''

    def test_wrong_group_fails_and_all_or_matching_group_passes(self):
        for hosts, expected in [('docker_hosts', 65), ('all', 0), ('web_hosts', 0)]:
            with self.subTest(hosts=hosts):
                code, output = self.run_probe(self.play(hosts))
                self.assertEqual(code, expected, output)
                if expected:
                    self.assertIn('validation_no_matching_targets', output)
                else:
                    self.assertIn('VERIFIED-TARGET-COUNTS', output)

    def test_multiple_plays_allow_an_empty_group_only_when_another_matches(self):
        code, output = self.run_probe(self.play('docker_hosts') + self.play('web_hosts'))
        self.assertEqual(code, 0, output)
        self.assertIn('(2, 1)', output)
        code, output = self.run_probe(self.play('docker_hosts') + self.play('missing_group'))
        self.assertEqual(code, 65, output)

    def test_extra_vars_resolve_templated_hosts_and_undefined_hosts_fail_closed(self):
        play = self.play('"{{ selected_group }}"')
        code, output = self.run_probe(play, {'selected_group': 'web_hosts', 'unrelated': 'synthetic-do-not-echo'})
        self.assertEqual(code, 0, output)
        self.assertNotIn('synthetic-do-not-echo', output)
        code, output = self.run_probe(play, {'selected_group': 'docker_hosts'})
        self.assertEqual(code, 65, output)
        code, output = self.run_probe(play)
        self.assertEqual(code, 65, output)
        self.assertIn('validation_host_resolution', output)

    def test_imported_plays_use_real_ansible_resolution_and_never_execute(self):
        (self.root / 'imported.yml').write_text(self.play('web_hosts'))
        code, output = self.run_probe('- import_playbook: imported.yml\n')
        self.assertEqual(code, 0, output)
        self.assertIn('(1, 1)', output)

    def test_real_guest_proof_rejects_no_match_but_draft_syntax_stays_supported(self):
        from dataclasses import replace
        import uuid
        from consolecrypt_runner.inventory import parse_yaml_inventory
        from consolecrypt_runner.snapshot import create_snapshot
        from consolecrypt_runner.validation import validate_project

        if os.getuid() == 0:
            self.skipTest('Guest namespaces require the non-root fixture user')
        project = self.root / 'project'
        project.mkdir()
        inventory = parse_yaml_inventory('''all:
  hosts:
    approved:
      ansible_host: 192.0.2.1
      ansible_user: deployer
  children:
    web_hosts:
      hosts:
        approved: {}
    docker_hosts:
      hosts: {}
''', 'owned-fixture')
        host_id = str(uuid.uuid4())
        inventory.hosts['approved'] = replace(inventory.hosts['approved'], host_id=host_id)
        targets = [{'host_id': host_id, 'alias': 'approved', 'address': '192.0.2.1',
                    'port': 22, 'username': 'deployer', 'route_digest': 'a' * 64,
                    'credential_refs': []}]
        for index, pattern in enumerate(('docker_hosts', 'all', 'web_hosts')):
            (project / 'playbook.yml').write_text(f'''---
- name: Owned target validation fixture
  hosts: {pattern}
  gather_facts: false
  tasks:
    - name: Display fixture
      ansible.builtin.debug:
        msg: fixture
''')
            directory = self.root / ('snapshot-' + str(index))
            snapshot = create_snapshot(project, directory)
            report = validate_project(directory, snapshot, 'playbook.yml', inventory=inventory, targets=targets)
            if pattern == 'docker_hosts':
                self.assertFalse(report.valid, report.to_json())
                self.assertEqual(report.checks[0]['exit_code'], 65, report.to_json())
                self.assertIn('zero approved targets', report.checks[0]['output'])
                # A target-free draft syntax result must remain usable; only a
                # bound proof may assert that approved targets really match.
                draft = validate_project(directory, snapshot, 'playbook.yml')
                self.assertTrue(draft.valid, draft.to_json())
                self.assertIsNone(draft.targets_digest)
            else:
                self.assertTrue(report.valid, report.to_json())
                self.assertIn('target match verified (1 approved hosts', report.checks[0]['output'])


if __name__ == '__main__':
    unittest.main()
