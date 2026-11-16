from dataclasses import asdict
import json
import os
from pathlib import Path
import socket
import subprocess
import sys
import tempfile
import threading
import unittest
import uuid

from consolecrypt_runner.probe import probe
from consolecrypt_runner.sandbox import bubblewrap_argv, run_bounded
from consolecrypt_runner.snapshot import create_snapshot
from consolecrypt_runner.validation import validate_project


@unittest.skipUnless(sys.platform == 'linux' and os.getuid() != 0, 'Real non-root Linux namespaces')
class SandboxTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory(prefix='cc-sandbox-test-')
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        self.project = self.root / 'project'
        self.project.mkdir()

    def run_python(self, code, **kwargs):
        return run_bounded(bubblewrap_argv(self.project, [sys.executable, '-c', code]), **kwargs)

    def test_private_host_files_environment_and_network_are_unavailable(self):
        secret = self.root / 'outside-private'
        secret.write_text('synthetic-private-content')
        server = socket.socket()
        server.bind(('127.0.0.1', 0))
        server.listen(1)
        self.addCleanup(server.close)
        code = f'''import os,socket,pathlib
assert os.getuid() != 0
assert not pathlib.Path({str(secret)!r}).exists()
assert 'CC_TEST_PRIVATE' not in os.environ
try:
    socket.create_connection(('127.0.0.1',{server.getsockname()[1]}),timeout=0.5)
except OSError:
    print('ISOLATED')
else:
    raise RuntimeError('Network escaped')
'''
        os.environ['CC_TEST_PRIVATE'] = 'synthetic-env-secret'
        try:
            result = self.run_python(code, timeout=10)
        finally:
            os.environ.pop('CC_TEST_PRIVATE', None)
        self.assertEqual(result.exit_code, 0, result.output)
        self.assertIn('ISOLATED', result.output)

    def test_only_own_job_is_cancelled_and_output_is_bounded(self):
        sibling = subprocess.Popen([sys.executable, '-c', 'import time;time.sleep(30)'])
        self.addCleanup(lambda: sibling.poll() is None and sibling.terminate())
        cancel = threading.Event()
        timer = threading.Timer(0.5, cancel.set)
        timer.start()
        self.addCleanup(timer.cancel)
        result = self.run_python('import time;print("started",flush=True);time.sleep(30)', cancel=cancel, grace=1, timeout=10)
        self.assertTrue(result.cancelled)
        self.assertFalse(result.timed_out)
        self.assertIsNone(sibling.poll())
        sibling.terminate()
        sibling.wait(timeout=5)
        result = self.run_python('print("x"*20000)', output_limit=100, timeout=10)
        self.assertEqual(len(result.output), 100)
        self.assertTrue(result.truncated)

    def test_unrelated_system_prefix_files_are_not_exposed(self):
        marker = Path('/usr/local/consolecrypt-synthetic-private')
        self.assertEqual(marker.read_text().strip(), 'synthetic-system-directory-data-not-a-real-secret')
        result = self.run_python("from pathlib import Path; assert not Path('/usr/local/consolecrypt-synthetic-private').exists()", timeout=10)
        self.assertEqual(result.exit_code, 0, result.output)

    def test_snapshot_cannot_be_modified_by_project_execution(self):
        (self.project / 'input.txt').write_text('approved')
        result = self.run_python('from pathlib import Path;Path("input.txt").write_text("modified");print("ok")', timeout=10)
        self.assertEqual(result.exit_code, 0, result.output)
        self.assertEqual((self.project / 'input.txt').read_text(), 'approved')

    def test_actual_ansible_runner_emits_task_results_in_the_namespace(self):
        (self.project / 'playbook.yml').write_text('''---
- name: Runner fixture
  hosts: localhost
  gather_facts: false
  tasks:
    - name: Fixture result
      ansible.builtin.debug:
        msg: runner-fixture-ok
''')
        code = '''import ansible_runner,json,os
os.mkdir('/work/run',0o700)
def event(data):
    print(json.dumps({'event':data.get('event'),'task':data.get('event_data',{}).get('task')}),flush=True)
    return False
r=ansible_runner.run(private_data_dir='/work/run',project_dir='/work/project',
    playbook='playbook.yml',inventory='localhost,',quiet=True,
    event_handler=event,settings={'suppress_output_file':True})
print(json.dumps({'status':r.status,'rc':r.rc}),flush=True)
raise SystemExit(r.rc)
'''
        result = self.run_python(code, timeout=30)
        self.assertEqual(result.exit_code, 0, result.output)
        self.assertIn('runner_on_ok', result.output)
        self.assertIn('successful', result.output)

    def test_real_syntax_and_lint_results_belong_to_exact_revision(self):
        (self.project / 'playbook.yml').write_text('''---
- name: Validate fixture
  hosts: all
  gather_facts: false
  tasks:
    - name: Display fixture
      ansible.builtin.debug:
        msg: fixture
''')
        snapshot = create_snapshot(self.project, self.root / 'snapshot')
        report = validate_project(self.root / 'snapshot', snapshot, 'playbook.yml')
        self.assertTrue(report.valid, report.to_json())
        self.assertEqual(report.revision_digest, snapshot.digest)
        (self.project / 'playbook.yml').write_text('- hosts: [\n')
        bad_snapshot = create_snapshot(self.project, self.root / 'bad-snapshot')
        bad = validate_project(self.root / 'bad-snapshot', bad_snapshot, 'playbook.yml')
        self.assertFalse(bad.valid)
        self.assertNotEqual(report.revision_digest, bad.revision_digest)

    def test_declared_inputs_are_checked_in_the_real_guest_before_any_target_credentials(self):
        (self.project/'playbook.yml').write_text('---\n- name: Required input fixture\n  hosts: all\n  gather_facts: false\n  tasks: []\n')
        (self.project/'.consolecrypt-inputs.json').write_text(json.dumps({'version':1,'playbooks':{
            'playbook.yml':{'required_extra_vars':['fixture_input']}}}))
        snapshot=create_snapshot(self.project,self.root/'input-snapshot')
        parameters={'extra_vars':{'another_input':'private-value-not-for-diagnostics'},'tags':[],'skip_tags':[],'diff':False}
        missing=validate_project(self.root/'input-snapshot',snapshot,'playbook.yml',parameters=parameters)
        self.assertFalse(missing.valid)
        self.assertIn('Missing required execution inputs: fixture_input',missing.checks[0]['output'])
        self.assertNotIn('private-value-not-for-diagnostics',json.dumps(missing.to_json()))
        parameters['extra_vars']['fixture_input']=False
        valid=validate_project(self.root/'input-snapshot',snapshot,'playbook.yml',parameters=parameters)
        self.assertTrue(valid.valid,valid.to_json())
        self.assertNotEqual(valid.parameters_digest,missing.parameters_digest)

    def test_reviewed_ai_requirements_are_bound_to_revision_and_checked_with_real_locked_tree(self):
        from consolecrypt_runner.dependencies import subtree_digest
        from consolecrypt_runner.project_requirements import REQUIREMENTS_FILE
        (self.project/'playbook.yml').write_text('---\n- name: AI review fixture\n  hosts: all\n  gather_facts: false\n  roles:\n    - demo\n')
        role = self.project/'roles/demo/tasks'; role.mkdir(parents=True)
        (role/'main.yml').write_text('---\n- name: Verify fixture input\n  ansible.builtin.assert:\n    that: fixture_input == 7\n')
        original = create_snapshot(self.project, self.root/'original')
        digest, _ = subtree_digest(original.to_json(), 'roles/demo')
        lock = {'version': 1, 'dependencies': [{'kind': 'role', 'name': 'demo', 'version': '1.2.3',
            'source': {'url': 'https://example.test/role.tar.gz', 'sha256': 'a'*64}, 'tree_digest': digest}]}
        (self.project/'.consolecrypt-dependencies.json').write_text(json.dumps(lock))
        requirements = {'version': 1, 'playbooks': {'playbook.yml': {'required_extra_vars': ['fixture_input'],
            'required_dependencies': [{'kind': 'role', 'name': 'demo', 'version': '1.2.3'}]}}}
        (self.project/REQUIREMENTS_FILE).write_text(json.dumps(requirements))
        snapshot = create_snapshot(self.project, self.root/'reviewed')
        parameters = {'extra_vars': {}, 'tags': [], 'skip_tags': [], 'diff': False}
        missing = validate_project(self.root/'reviewed', snapshot, 'playbook.yml', parameters=parameters)
        self.assertFalse(missing.valid); self.assertIn('fixture_input', missing.checks[0]['output'])
        parameters['extra_vars']['fixture_input'] = 7
        valid = validate_project(self.root/'reviewed', snapshot, 'playbook.yml', parameters=parameters)
        self.assertTrue(valid.valid, valid.to_json()); self.assertEqual(valid.revision_digest, snapshot.digest)
        requirements['playbooks']['playbook.yml']['required_dependencies'][0]['version'] = '2.0.0'
        (self.project/REQUIREMENTS_FILE).write_text(json.dumps(requirements))
        changed = create_snapshot(self.project, self.root/'changed')
        wrong = validate_project(self.root/'changed', changed, 'playbook.yml', parameters=parameters)
        self.assertFalse(wrong.valid); self.assertIn('pinned version differs', wrong.checks[0]['output'])
        self.assertNotEqual(wrong.revision_digest, valid.revision_digest)
        # Membership is insufficient: even a matching pin cannot bless edits.
        (self.project/REQUIREMENTS_FILE).write_text(json.dumps(dict(requirements, playbooks={})))
        (role/'main.yml').write_text('---\n# changed dependency\n')
        edited = create_snapshot(self.project, self.root/'edited')
        denied = validate_project(self.root/'edited', edited, 'playbook.yml', parameters=parameters)
        self.assertFalse(denied.valid); self.assertIn('pinned tree digest', denied.checks[0]['output'])

    def test_private_input_witness_binds_float_variables_without_persisting_them_in_report(self):
        from dataclasses import replace
        from consolecrypt_runner.__main__ import Helper
        from consolecrypt_runner.inventory import parse_ini_inventory
        from consolecrypt_runner.protocol import decode, encode
        import hashlib
        (self.project / 'playbook.yml').write_text('''---
- name: Witness fixture
  hosts: all
  gather_facts: false
  tasks:
    - name: Display fixture
      ansible.builtin.debug:
        msg: fixture
''')
        helper = Helper(self.root / 'helper')
        self.addCleanup(helper.close)
        captured = helper.request('capture', {'source': str(self.project)}, threading.Event())
        inventory = parse_ini_inventory('service ansible_host=192.0.2.1 ansible_user=deployer threshold=1e-7', 'native-fixture')
        host_id = str(uuid.uuid4())
        inventory.hosts['service'] = replace(inventory.hosts['service'],host_id=host_id)
        parameters = {'extra_vars':{'threshold':1e-7,'large':1e20},'tags':[],'skip_tags':[],'diff':False}
        targets = [{'host_id':host_id,'alias':'service','address':'192.0.2.1','port':22,'username':'deployer','route_digest':'a'*64,'credential_refs':[]}]
        report = helper.request('prepare_run', {'revision_id':captured['revision_id'],'playbook':'playbook.yml',
            'parameters':parameters,'inventory':inventory.to_snapshot(),'targets':targets},threading.Event())
        self.assertTrue(report['valid'],report['checks'])
        for name, expected in [('parameters',parameters),('inventory',inventory.to_snapshot())]:
            witness = report['input_witnesses'][name]
            self.assertEqual(decode(witness['canonical_json'].encode()),expected)
            self.assertEqual(hashlib.sha256(witness['canonical_json'].encode()).hexdigest(),report[name+'_digest'])
        from consolecrypt_runner.validation_reports import read_report
        stored = read_report(helper.root/'revisions'/captured['revision_id'],report)
        self.assertNotIn('input_witnesses',stored)
        alternate = helper.request('prepare_run', {'revision_id':captured['revision_id'],'playbook':'playbook.yml',
            'parameters':dict(parameters,extra_vars={'threshold':2e-7,'large':1e20}),
            'inventory':inventory.to_snapshot(),'targets':targets},threading.Event())
        self.assertTrue(alternate['valid'],alternate['checks'])
        self.assertNotEqual(alternate['parameters_digest'],report['parameters_digest'])
        self.assertEqual(read_report(helper.root/'revisions'/captured['revision_id'],report),stored)
        self.assertEqual(read_report(helper.root/'revisions'/captured['revision_id'],alternate)['parameters_digest'],alternate['parameters_digest'])

    def test_locked_role_is_checked_in_guest_and_changed_vendored_code_blocks_preflight(self):
        from consolecrypt_runner.dependencies import subtree_digest, LOCK_FILE
        (self.project/'playbook.yml').write_text('---\n- name: Locked role fixture\n  hosts: all\n  gather_facts: false\n  roles:\n    - role: locked_demo\n')
        role=self.project/'roles/locked_demo';(role/'tasks').mkdir(parents=True)
        (role/'tasks/main.yml').write_text('---\n- name: Display fixture\n  ansible.builtin.debug:\n    msg: locked-role-fixture\n')
        baseline=create_snapshot(self.project,self.root/'baseline')
        digest,_=subtree_digest(baseline.to_json(),'roles/locked_demo')
        (self.project/LOCK_FILE).write_text(json.dumps({'version':1,'dependencies':[{
            'kind':'role','name':'locked_demo','version':'1.2.3',
            'source':{'url':'https://example.test/declared-not-downloaded.tar.gz','sha256':'a'*64},'tree_digest':digest}]}))
        valid_tree=self.root/'valid-locked';snapshot=create_snapshot(self.project,valid_tree)
        valid=validate_project(valid_tree,snapshot,'playbook.yml')
        self.assertTrue(valid.valid,valid.to_json())
        self.assertIn('locked dependency trees verified (1)',valid.checks[0]['output'])
        (role/'tasks/main.yml').write_text('---\n- name: Changed fixture\n  ansible.builtin.debug:\n    msg: changed\n')
        changed_tree=self.root/'changed-locked';changed=create_snapshot(self.project,changed_tree)
        denied=validate_project(changed_tree,changed,'playbook.yml')
        self.assertFalse(denied.valid)
        self.assertIn('pinned tree digest',denied.checks[0]['output'])
        self.assertNotIn('declared-not-downloaded',json.dumps(denied.to_json()))

    def test_archive_quarantine_runs_inside_the_real_namespace_without_host_writes(self):
        import base64
        import hashlib
        import tarfile
        from test_dependency_archive import archive
        raw=archive([('tasks/main.yml',tarfile.REGTYPE,b'---\n[]\n')])
        bad=archive([('../../escape',tarfile.REGTYPE,b'not allowed')])
        expected=self.root/'expected-archive'
        (expected/'tasks').mkdir(parents=True)
        (expected/'tasks/main.yml').write_bytes(b'---\n[]\n')
        expected_snapshot=create_snapshot(expected,self.root/'expected-snapshot')
        record={'kind':'role','name':'quarantine_fixture','version':'1.0.0',
                'source':{'url':'https://example.invalid/fixture.tar.gz',
                          'sha256':hashlib.sha256(raw).hexdigest()},
                'tree_digest':expected_snapshot.digest}
        marker=self.root/'outside-archive-marker';marker.write_text('host unchanged')
        code=f'''import base64,hashlib,json
from pathlib import Path
from consolecrypt_runner.dependency_archive import staged_dependency
from consolecrypt_runner.errors import RunnerError
from consolecrypt_runner.snapshot import create_snapshot
assert not Path({str(marker)!r}).exists()
base=Path('/work/archive-work');base.mkdir(mode=0o700)
raw=base64.b64decode({base64.b64encode(raw).decode()!r})
record=json.loads({json.dumps(record)!r})
with staged_dependency(raw,record,base) as (tree,snapshot,receipt):
    assert snapshot==create_snapshot(tree,base/'independent')
    assert receipt['tree_digest']==record['tree_digest']
bad=base64.b64decode({base64.b64encode(bad).decode()!r})
record['source']['sha256']=hashlib.sha256(bad).hexdigest()
try:
    with staged_dependency(bad,record,base):
        raise AssertionError('Traversal was accepted')
except RunnerError:pass
assert not Path('/work/escape').exists()
print('ARCHIVE-ISOLATED')
'''
        result=self.run_python(code,timeout=30)
        self.assertEqual(result.exit_code,0,result.output)
        self.assertIn('ARCHIVE-ISOLATED',result.output)
        self.assertEqual(marker.read_text(),'host unchanged')
        self.assertEqual(list(self.project.iterdir()),[])


if __name__ == '__main__': unittest.main()
