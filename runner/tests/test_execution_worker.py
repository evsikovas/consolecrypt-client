import json
from pathlib import Path
import sys
import tempfile
import unittest

from consolecrypt_runner.protocol import encode
from consolecrypt_runner.ephemeral_input import EphemeralJobInput
from consolecrypt_runner.sandbox import bubblewrap_argv, run_bounded


class ExecutionWorkerTests(unittest.TestCase):
    def test_actual_worker_emits_structured_results_and_respects_no_log(self):
        with tempfile.TemporaryDirectory(prefix='cc-worker-test-') as folder:
            root = Path(folder)
            project = root / 'project'; project.mkdir()
            (project / 'playbook.yml').write_text('''---
- name: Worker fixture
  hosts: fixture
  gather_facts: false
  tasks:
    - name: Visible task
      ansible.builtin.debug:
        msg: public-worker-proof
    - name: Private fact callback
      ansible.builtin.set_fact:
        callback_secret: fact-result-sensitive-marker
    - name: Nested callback no_log
      ansible.builtin.assert:
        that: callback_secret | length > 0
        quiet: true
    - name: Nested callback output
      ansible.builtin.debug:
        msg:
          results:
            - _ansible_no_log: true
              stdout: nested-result-sensitive-marker
    - name: Censored task
      ansible.builtin.debug:
        msg: fixture-sensitive-marker
      no_log: true
''')
            body = encode({'playbook': 'playbook.yml', 'mode': 'apply', 'forks': 5,
                'parameters': {'extra_vars': {'synthetic_private': 'private-variable-lifetime-marker'}, 'tags': [], 'skip_tags': [], 'diff': False},
                'inventory': {'all': {'hosts': {'fixture': {'ansible_connection': 'local'}}}}})
            # Inspect actual Ansible Runner reconstruction BEFORE the real
            # namespace exits, without exporting file bodies or task results.
            proof = root/'artifact_lifetime.py'
            proof.write_text('''import errno,json,os,stat
from pathlib import Path
from consolecrypt_runner import execution_worker
try: execution_worker.main()
except SystemExit as result: code=result.code
assert code==0
extra=Path('/work/runner/env/extravars')
inventory=Path('/work/runner/inventory/hosts.json')
assert b'private-variable-lifetime-marker' in extra.read_bytes()
assert b'ansible_connection' in inventory.read_bytes()
assert stat.S_IMODE(extra.stat().st_mode)==0o600
assert stat.S_IMODE(inventory.stat().st_mode)==0o600
assert stat.S_IMODE(Path('/work/runner').stat().st_mode)==0o700
assert any(p.is_file() for p in Path('/work/artifacts').rglob('*'))
mounts=Path('/proc/self/mountinfo').read_text().splitlines()
for target in ('/work','/tmp'):
 assert any(line.split()[4]==target and line.split(' - ')[1].split()[0]=='tmpfs' for line in mounts)
print(json.dumps({'type':'artifact_lifetime','payload':{'private_files':True,'work_tmpfs':True,'tmp_tmpfs':True}}))
''', encoding='utf-8')
            with EphemeralJobInput(body) as spec:
                result = run_bounded(bubblewrap_argv(project, [sys.executable, '/tool/artifact_lifetime.py'],
                    extra_readonly=[(proof,'/tool/artifact_lifetime.py')], job_input=spec), timeout=30, job_input=spec)
            self.assertEqual(result.exit_code, 0, result.output)
            events = [json.loads(line) for line in result.output.splitlines()]
            self.assertTrue(any(event['type'] == 'runner_on_ok' for event in events))
            summary = [event for event in events if event['type']=='runner_summary']
            self.assertEqual(summary[0]['payload']['status'], 'successful')
            self.assertEqual(events[-1]['type'], 'artifact_lifetime')
            self.assertNotIn('fixture-sensitive-marker', result.output)
            self.assertNotIn('fact-result-sensitive-marker', result.output)
            self.assertNotIn('nested-result-sensitive-marker', result.output)
            self.assertIn('public-worker-proof', result.output)
            self.assertNotIn('private-variable-lifetime-marker', result.output)
            self.assertFalse((root/'work').exists())
            self.assertFalse((root/'runner').exists())
            self.assertFalse((root/'artifacts').exists())
