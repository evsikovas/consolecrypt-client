"""Actual project-controlled extension code in the production Linux namespaces."""
import json
import os
from pathlib import Path
import socket
import sys
import tempfile
import unittest

from consolecrypt_runner.ephemeral_input import EphemeralJobInput, EphemeralValidationInput
from consolecrypt_runner.protocol import encode
from consolecrypt_runner.sandbox import bubblewrap_argv, run_bounded


@unittest.skipUnless(sys.platform == 'linux' and os.getuid() != 0, 'Real non-root Linux namespaces required')
class ProjectExtensionIsolationTests(unittest.TestCase):
    def setUp(self):
        self.folder = tempfile.TemporaryDirectory(prefix='cc-adversarial-extensions-')
        self.addCleanup(self.folder.cleanup)
        self.root = Path(self.folder.name)
        self.project = self.root / 'project'
        self.project.mkdir()
        self.secret = self.root / 'outside-private'
        self.secret.write_text('synthetic-outside-extension-marker')
        self.listener = socket.socket()
        self.listener.bind(('127.0.0.1', 0))
        self.listener.listen(1)
        self.addCleanup(self.listener.close)
        self.port = self.listener.getsockname()[1]
        self.probe = '''import json,os,socket
from pathlib import Path
def probe(kind):
 assert os.getuid()!=0
 assert not Path(%r).exists()
 assert 'CC_EXTENSION_PRIVATE' not in os.environ
 try:
  socket.create_connection(('127.0.0.1',%d),timeout=.2)
 except OSError: pass
 else: raise RuntimeError('escaped original network namespace')
 target=Path('/work/extension-proofs');target.mkdir(exist_ok=True)
 (target/kind).write_text('isolated')
 return 'isolated'
''' % (str(self.secret), self.port)
        self.parameters = {'extra_vars': {}, 'tags': [], 'skip_tags': [], 'diff': False}

    def write(self, name, text):
        path = self.project / name
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(text, encoding='utf-8')
        return path

    def worker(self, script, *, validation=False):
        wrapper = self.root / 'trusted-observer.py'
        wrapper.write_text(script, encoding='utf-8')
        body = {'playbook': 'playbook.yml', 'parameters': self.parameters,
                'inventory': {'all': {'hosts': {'fixture': {'ansible_connection': 'local'}}}}}
        body.update({'require_host_match': True} if validation else {'mode': 'apply', 'forks': 1})
        cls = EphemeralValidationInput if validation else EphemeralJobInput
        option = 'validation_input' if validation else 'job_input'
        os.environ['CC_EXTENSION_PRIVATE'] = 'synthetic-parent-only-marker'
        try:
            with cls(encode(body)) as private:
                result = run_bounded(bubblewrap_argv(self.project,
                    [sys.executable, '/tool/trusted-observer.py'],
                    extra_readonly=[(wrapper, '/tool/trusted-observer.py')], **{option: private}),
                    timeout=45, **{option: private})
        finally:
            os.environ.pop('CC_EXTENSION_PRIVATE', None)
        self.assertEqual(result.exit_code, 0, result.output[-8000:])
        self.assertFalse(result.timed_out or result.cancelled or result.truncated)
        self.assertNotIn('synthetic-outside-extension-marker', result.output)
        self.assertNotIn('synthetic-parent-only-marker', result.output)
        self.assertEqual(self.secret.read_text(), 'synthetic-outside-extension-marker')
        return result

    def test_run_action_lookup_callback_config_and_local_delegation_stay_isolated(self):
        self.write('action_plugins/cc_probe.py', self.probe + '''
from ansible.plugins.action import ActionBase
class ActionModule(ActionBase):
 def run(self,tmp=None,task_vars=None):
  return {'changed':False,'msg':probe('action')}
''')
        self.write('lookup_plugins/cc_probe.py', self.probe + '''
from ansible.plugins.lookup import LookupBase
class LookupModule(LookupBase):
 def run(self,terms,variables=None,**kwargs): return [probe('lookup')]
''')
        self.write('callback_plugins/cc_probe.py', self.probe + '''
from ansible.plugins.callback import CallbackBase
class CallbackModule(CallbackBase):
 CALLBACK_VERSION=2.0
 CALLBACK_TYPE='aggregate'
 CALLBACK_NAME='cc_probe'
 CALLBACK_NEEDS_ENABLED=True
 def v2_playbook_on_start(self,playbook): probe('callback')
''')
        inventory = self.write('inventory_probe.py', '#!/usr/bin/env python3\n' + self.probe + "\nprobe('inventory-script')\nprint('{}')\n")
        inventory.chmod(0o700)
        self.write('ansible.cfg', '[defaults]\ncallback_plugins=./callback_plugins\ncallbacks_enabled=cc_probe\ninventory=./inventory_probe.py\n')
        delegated = self.probe + "\nprobe('delegation')\n"
        self.write('playbook.yml', json.dumps([{'name': 'Isolation fixture', 'hosts': 'fixture', 'gather_facts': False,
            'tasks': [
                {'name': 'Actual action plugin', 'cc_probe': {}},
                {'name': 'Actual lookup plugin', 'ansible.builtin.debug': {'msg': "{{ lookup('cc_probe', 'unused') }}"}},
                {'name': 'Actual local delegation', 'delegate_to': 'localhost',
                 'ansible.builtin.command': {'argv': [sys.executable, '-c', delegated]}, 'changed_when': False}]}]))
        self.worker('''import json
from pathlib import Path
from consolecrypt_runner import execution_worker
try: execution_worker.main()
except SystemExit as result: code=result.code
assert code==0,code
proofs={p.name:p.read_text() for p in Path('/work/extension-proofs').iterdir()}
assert proofs==dict.fromkeys(('action','lookup','callback','delegation'),'isolated'),proofs
# Project config cannot replace the explicitly supplied approved inventory.
assert 'inventory-script' not in proofs
print('CC_EXTENSION_PROOF='+json.dumps(sorted(proofs)))
''')

    def test_validation_executes_untrusted_lint_rule_inside_namespace(self):
        self.write('rules/cc_probe.py', self.probe + '''
probe('validation-rule-import')
from ansiblelint.rules import AnsibleLintRule
class IsolationRule(AnsibleLintRule):
 id='fixture-isolation'
 shortdesc='Disposable isolation fixture'
 description='Actual untrusted validation plugin inside the existing sandbox.'
 severity='LOW'
 tags=['fixture']
 version_changed='1.0.0'
 def matchtask(self,task,file=None):
  probe('validation-rule-task')
  return False
''')
        self.write('.ansible-lint', 'rulesdir:\n  - rules\n')
        self.write('ansible.cfg', '[defaults]\nlocal_tmp=/work/project/.private-tmp\n')
        self.write('playbook.yml', '---\n- name: Validation isolation fixture\n  hosts: fixture\n  gather_facts: false\n  tasks:\n    - name: Fixture task\n      ansible.builtin.debug:\n        msg: fixture\n')
        self.worker('''import subprocess,sys,json
from pathlib import Path
result=subprocess.run([sys.executable,'-m','consolecrypt_runner.validation_worker','lint'],capture_output=True,timeout=35)
assert result.returncode in (0,2),result.stderr[-2048:]
proofs={p.name:p.read_text() for p in Path('/work/extension-proofs').iterdir()}
assert proofs==dict.fromkeys(('validation-rule-import','validation-rule-task'),'isolated'),proofs
print('CC_VALIDATION_EXTENSION_PROOF='+json.dumps(sorted(proofs)))
''', validation=True)
