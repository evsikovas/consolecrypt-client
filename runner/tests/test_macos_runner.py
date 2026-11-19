"""Run with the managed Python 3.12 environment on an Apple Silicon Mac."""
from pathlib import Path
import sys
import tempfile
import threading
import unittest

from consolecrypt_runner.inventory_sources import capture_inventory
from consolecrypt_runner.macos_sandbox import run_macos_worker
from consolecrypt_runner.probe import probe
from consolecrypt_runner.protocol import encode
from consolecrypt_runner.revisions import RevisionRepository
from consolecrypt_runner.validation import validate_project


@unittest.skipUnless(sys.platform == 'darwin', 'Native macOS required')
class MacRunnerTests(unittest.TestCase):
    def test_validate_and_execute_playbook_inside_verified_worker(self):
        environment = probe()
        self.assertTrue(environment['capabilities']['macos_worker_sandbox_v1'], environment['sandbox'])
        self.assertFalse(environment['sandbox']['pid'])
        with tempfile.TemporaryDirectory(prefix='cc-native-test-') as folder:
            root = Path(folder)
            project = root / 'project'
            project.mkdir()
            (project / 'playbook.yml').write_text('''---
- name: Native Mac Runner fixture
  hosts: all
  gather_facts: false
  tasks:
    - name: Display result
      ansible.builtin.debug:
        msg: consolecrypt-mac-runner-ok
''', encoding='utf-8')
            (project / 'hosts.ini').write_text('[servers]\nfixture ansible_host=192.0.2.10\n', encoding='utf-8')
            repository = RevisionRepository(root / 'revisions')
            captured = repository.capture(project)
            revision_id = captured['revision_id']
            revision, snapshot = repository.read(revision_id)
            inventory_preview = capture_inventory(repository, revision_id,
                                                  [{'path': 'hosts.ini', 'format': 'ini'}], threading.Event())
            self.assertEqual(inventory_preview['host_count'], 1)
            report = validate_project(revision, snapshot, 'playbook.yml')
            self.assertTrue(report.valid, report.to_json())
            parameters = {'extra_vars': {}, 'tags': [], 'skip_tags': [], 'diff': False}
            inventory = {'all': {'hosts': {'localhost': {'ansible_connection': 'local'}}}}
            body = encode({'playbook': 'playbook.yml', 'mode': 'run', 'forks': 1,
                           'inventory': inventory, 'parameters': parameters})
            worker = run_macos_worker(revision, [sys.executable, '-m', 'consolecrypt_runner.execution_worker'],
                                      input_bytes=body, timeout=45)
            self.assertEqual(worker.exit_code, 0, worker.output)
            self.assertIn('runner_on_ok', worker.output)
            self.assertIn('consolecrypt-mac-runner-ok', worker.output)
