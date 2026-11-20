"""Real CLI/Ansible helper proof. Native-side routing below is a TEST stub,
not AppCore/Native authority acceptance (that separate ignored test is required).
"""
import base64,hashlib,io,json,os,secrets,select,stat,subprocess,sys,tempfile,threading,time,unittest,uuid
from pathlib import Path
from unittest.mock import patch
from consolecrypt_runner.vault_validation_helper import ValidationHelper,ALLOWED
from consolecrypt_runner.dependency_helper import operation_path,serve_dependency
from consolecrypt_runner.protocol import encode,read_message
from consolecrypt_runner.snapshot import create_snapshot,verify_snapshot
from consolecrypt_runner.errors import RunnerError

class ValidationModeTests(unittest.TestCase):
    def setUp(self):
        self.temp=tempfile.TemporaryDirectory(prefix='cc-validation-mode-');self.addCleanup(self.temp.cleanup)
        self.root=Path(self.temp.name);self.root.chmod(0o700);self.configured=self.root/'jobs';self.configured.mkdir(mode=0o700)
        (self.configured/'sentinel').write_bytes(b'user jobs unchanged');self.operation=str(uuid.uuid4())
    def helper(self):
        helper=ValidationHelper(self.configured,self.operation);self.addCleanup(helper.close);return helper
    def test_phase_close_does_not_cancel_lint_but_parent_cancel_revokes_both(self):
        from consolecrypt_runner.vault_validation_helper import _PhaseCancel
        parent=threading.Event();syntax=_PhaseCancel(parent);lint=_PhaseCancel(parent)
        syntax.set();self.assertTrue(syntax.is_set());self.assertFalse(parent.is_set());self.assertFalse(lint.is_set())
        parent.set();self.assertTrue(lint.wait(.1));self.assertTrue(syntax.is_set())
    def test_validation_mode_is_separate_private_no_executor_store_network_or_target(self):
        from consolecrypt_runner import __main__ as main
        with patch.object(main,'JobStore',side_effect=AssertionError('job store forbidden')),patch.object(main,'InteractiveExecutor',side_effect=AssertionError('executor forbidden')):
            helper=self.helper();report=helper.request('probe',{},threading.Event())
            self.assertEqual(report['mode'],'vault_validation');self.assertEqual(report['network_policy'],'none')
            self.assertEqual(report['root_namespace'],'.consolecrypt-validation-operations');self.assertFalse(report['execution_enabled']);self.assertFalse(report['target_dispatch_enabled'])
            self.assertFalse(hasattr(helper,'store'));self.assertIsNone(helper.dependencies)
        self.assertEqual((self.configured/'sentinel').read_bytes(),b'user jobs unchanged')
        self.assertNotEqual(operation_path(self.configured,self.operation),operation_path(self.configured,self.operation,_purpose='vault_validation'))
    def test_all_job_target_dependency_network_arbitrary_capture_routes_denied(self):
        helper=self.helper()
        for operation in ['configure_interactive','accept_job','enroll','target_request','exec','vault_state','capture','git_capture','inventory_capture','dependency_prepare','dependency_archive_chunk','dependency_dispose','prepare_run','validate']:
            with self.assertRaises(RunnerError):helper.request(operation,{},threading.Event())
            self.assertNotIn(operation,ALLOWED)
    def test_raw_target_and_unknown_private_control_frames_close_failclosed(self):
        for frame in [{'version':1,'type':'target_response','request_id':str(uuid.uuid4()),'result':{},'error':None},
                      {'version':1,'type':'vault_validation_response','request_id':str(uuid.uuid4()),'result':{},'error':None}]:
            helper=self.helper()
            with self.assertRaises(RunnerError):serve_dependency(helper,io.BytesIO(encode(frame)+b'\n'),io.BytesIO())
            helper.close();self.operation=str(uuid.uuid4())
    def test_current_operation_retarget_denies_before_any_material_channel(self):
        helper=self.helper();path=operation_path(self.configured,self.operation,_purpose='vault_validation')
        moved=path.with_name(str(uuid.uuid4()));path.rename(moved);path.mkdir(mode=0o700)
        with self.assertRaises(RunnerError):helper._current()
        self.assertEqual((self.configured/'sentinel').read_bytes(),b'user jobs unchanged')
    def test_normal_eof_disposes_only_current_validation_root_and_preserves_other_purpose(self):
        helper=self.helper();operation=operation_path(self.configured,self.operation,_purpose='vault_validation')
        other=operation_path(self.configured,str(uuid.uuid4()));other.mkdir(parents=True,mode=0o700);(other/'sentinel').write_bytes(b'other scope retained')
        self.assertTrue(serve_dependency(helper,io.BytesIO(),io.BytesIO()));helper.close(dispose=True)
        self.assertFalse(operation.exists());self.assertEqual((other/'sentinel').read_bytes(),b'other scope retained')
    def test_actual_cli_four_children_fixed0500_client_hostile_cfg_env_and_joined_dispose(self):
        from ansible.parsing.vault import VaultLib,VaultSecret
        from ansible.release import __version__
        from ansiblelint.version import __version__ as lint_version
        self.assertEqual((__version__,lint_version),('2.20.11','26.11.0'))
        password=('owned-password-'+secrets.token_hex(24)).encode();project=self.root/'project';project.mkdir(mode=0o700)
        credential=str(uuid.uuid4());reference=encode({'version':1,'ansible_vault':{'credential_id':credential}})
        (project/'.consolecrypt-secret-inputs.json').write_bytes(reference)
        encrypted=VaultLib().encrypt(b'fixture',VaultSecret(password)).decode();block='\n'.join('      '+line for line in encrypted.splitlines())
        (project/'playbook.yml').write_text('---\n- name: Owned fixed client validation\n  hosts: "{{ selected_group }}"\n  gather_facts: false\n  vars:\n    selected_group: !vault |\n'+block+'\n  tasks:\n    - name: No task execution in syntax check\n      ansible.builtin.debug:\n        msg: synthetic\n')
        (project/'ansible.cfg').write_text('[defaults]\nvault_password_file=/tool/python/consolecrypt_runner/vault_password_client.py\nvault_identity_list=evil@/work/project/evil-client\nask_vault_pass=True\n')
        evil=project/'evil-client';evil.write_text('#!/bin/sh\necho UNTRUSTED_VAULT_CLIENT_EXECUTED >&2\necho WRONG_KEY\n');evil.chmod(0o700)
        # Project-controlled Python package must not shadow trusted Vault
        # controller bootstrap after child changes cwd into the copied project.
        shadow=project/'consolecrypt_runner';shadow.mkdir(mode=0o700)
        (shadow/'__init__.py').write_text('')
        (shadow/'validation_worker.py').write_text('print("PROJECT_SHADOWED_TRUSTED_WORKER")\n')
        before=create_snapshot(project,self.root/'frozen-snapshot')
        source=Path(__file__).resolve().parents[1]/'src'
        fixed="import sys,runpy;sys.path.insert(0,sys.argv.pop(1));runpy.run_module('consolecrypt_runner',run_name='__main__')"
        environment=dict(os.environ,ANSIBLE_VAULT_PASSWORD_FILE='/not-selected',ANSIBLE_VAULT_IDENTITY_LIST='evil@/not-selected',ANSIBLE_ASK_VAULT_PASS='True')
        process=subprocess.Popen([sys.executable,'-I','-B','-c',fixed,str(source),'--validation-stdio','--root',str(self.configured),'--operation-id',self.operation],stdin=subprocess.PIPE,stdout=subprocess.PIPE,stderr=subprocess.PIPE,env=environment)
        seen=[];expected={};report=None
        def receive():
            if not select.select([process.stdout],[],[],20)[0]:raise AssertionError('Bounded private CLI response deadline')
            frame=read_message(process.stdout);self.assertIsNotNone(frame);return frame
        def send(frame):process.stdin.write(encode(frame)+b'\n');process.stdin.flush()
        def request(operation,body):
            identity=str(uuid.uuid4());send({'version':1,'request_id':identity,'operation':operation,'body':body})
            while True:
                frame=receive()
                if frame.get('type')=='vault_validation_request':
                    child=frame['body'];key=(child['phase'],child['call_index']);self.assertEqual(child,expected[key]);self.assertNotIn(key,seen)
                    # Synthetic Native fixture freshly queries actual SAME CLI
                    # peer for EVERY child; real Rust owner must enforce it.
                    fresh=request('environment_probe',{});self.assertEqual(fresh['environment_digest'],report['environment_digest'])
                    seen.append(key)
                    owned=operation_path(self.configured,self.operation,_purpose='vault_validation')
                    wrappers=list((owned/'data').glob('vault-phase-*/password-client'));self.assertEqual(len(wrappers),1)
                    info=wrappers[0].lstat();self.assertTrue(stat.S_ISREG(info.st_mode));self.assertEqual(stat.S_IMODE(info.st_mode),0o500)
                    script=wrappers[0].read_bytes();self.assertTrue(script.startswith(b'#!/bin/sh\nexec /opt/cc-runtime/bin/python -I '));self.assertNotIn(password,script)
                    send({'version':1,'type':'vault_validation_response','request_id':frame['request_id'],'result':{'password_base64':base64.b64encode(password+b'\n').decode()},'error':None})
                    continue
                self.assertEqual(frame['type'],'response');self.assertEqual(frame['request_id'],identity);self.assertIsNone(frame['error']);return frame['result']
        try:
            self.assertEqual(request('probe',{})['mode'],'vault_validation');report=request('environment_probe',{})
            transfer=request('upload_begin',{'digest':before.digest})['upload_id']
            for entry in before.entries:
                request('upload_entry',{'upload_id':transfer,'entry':entry})
                if entry['type']=='file':
                    data=(project/entry['path']).read_bytes()
                    for offset in range(0,len(data),65536):request('upload_chunk',{'upload_id':transfer,'offset':offset,'data':base64.b64encode(data[offset:offset+65536]).decode()})
            uploaded=request('upload_commit',{'upload_id':transfer});self.assertEqual(uploaded['digest'],before.digest)
            scope=str(uuid.uuid4());children=[]
            for phase in ['syntax','lint']:
                for index in range(2):
                    child={'version':1,'purpose':'ansible_vault_validation_password','scope_id':scope,'phase':phase,'call_index':index,'phase_nonce':str(uuid.uuid4()),'revision_digest':before.digest,'reference_sha256':hashlib.sha256(reference).hexdigest(),'environment_digest':report['environment_digest'],'helper_digest':report['helper_digest']}
                    expected[(phase,index)]=child;children.append(child)
            host=str(uuid.uuid4());targets=[{'host_id':host,'alias':'fixture','address':'192.0.2.1','port':22,'username':'fixture','route_digest':'e'*64,'credential_refs':[]}]
            inventory={'version':1,'hosts':[{'host_id':host,'alias':'fixture','address':'192.0.2.1','port':22,'username':'fixture','variables':{},'sources':['owned'],'credential_refs':[]}],'groups':{},'source_digests':{'owned':'f'*64}}
            result=request('validation_prepare',{'revision_id':uploaded['revision_id'],'playbook':'playbook.yml','parameters':{'extra_vars':{},'tags':[],'skip_tags':[],'diff':False},'inventory':inventory,'targets':targets,'vault_children':children})
            self.assertTrue(result['valid'],json.dumps({'seen':seen,'checks':[{'phase':c['name'],'exit':c['exit_code'],'cancelled':c['cancelled'],'timeout':c['timed_out'],'classes':[tag for text,tag in [('Cannot resolve playbook hosts','target_match'),('Vault','vault_error'),('vault','vault_error_lower'),('Scoped Vault input unavailable','channel_denied'),('Trusted executable Vault client unavailable','client_mode'),('Decryption failed','decrypt_failed'),('Traceback','python_error'),('UNTRUSTED_VAULT_CLIENT_EXECUTED','untrusted_client'),('schema','schema'),('name[','lint_name'),('fqcn','lint_fqcn'),('Syntax','syntax')] if text in c['output']]} for c in result['checks']]}));self.assertEqual(seen,[('syntax',0),('syntax',1),('lint',0),('lint',1)])
            for check in result['checks']:
                self.assertEqual(check['exit_code'],0);self.assertNotIn('UNTRUSTED_VAULT_CLIENT_EXECUTED',check['output']);self.assertNotIn(password.decode(),check['output'])
            ack=request('validation_dispose',{'operation_id':self.operation,'root_family':hashlib.sha256(self.configured.name.encode()).hexdigest()})
            self.assertTrue(ack['data_disposed'] and ack['operation_removed']);process.stdin.close();self.assertEqual(process.wait(10),0)
            self.assertFalse(operation_path(self.configured,self.operation,_purpose='vault_validation').exists())
            verify_snapshot(project,before);self.assertEqual((self.configured/'sentinel').read_bytes(),b'user jobs unchanged')
        finally:
            if process.poll() is None:process.kill();process.wait(5)
            error=process.stderr.read(65537);self.assertLessEqual(len(error),65536);self.assertNotIn(password,error)
            process.stdout.close();process.stderr.close()
