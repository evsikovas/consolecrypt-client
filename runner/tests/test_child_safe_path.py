"""Real default bubblewrap bootstrap: project must not replace trusted workers."""
import json,resource,sys,tempfile,unittest
from pathlib import Path
from consolecrypt_runner.sandbox import bubblewrap_argv,run_bounded
class ChildSafePathTests(unittest.TestCase):
    def project(self,root):
        project=root/'project';project.mkdir(mode=0o700);shadow=project/'consolecrypt_runner';shadow.mkdir()
        (shadow/'__init__.py').write_text('')
        (shadow/'validation_worker.py').write_text('print("FORGED_VALIDATION_WORKER_SUCCESS")\n')
        (shadow/'execution_worker.py').write_text('print("FORGED_EXECUTION_WORKER_SUCCESS")\n')
        (project/'playbook.yml').write_text('---\n- name: Must check actual target match\n  hosts: unmatched_fixture\n  gather_facts: false\n  tasks: []\n')
        return project
    def test_project_package_cannot_forge_default_validation_success(self):
        with tempfile.TemporaryDirectory() as temporary:
            root=Path(temporary);project=self.project(root);inputs=root/'inputs.json'
            inputs.write_text(json.dumps({'playbook':'playbook.yml','parameters':{'extra_vars':{},'tags':[],'skip_tags':[],'diff':False},'inventory':{'all':{'hosts':{'fixture':{}}}},'require_host_match':True}))
            result=run_bounded(bubblewrap_argv(project,[sys.executable,'-m','consolecrypt_runner.validation_worker','syntax'],extra_readonly=[(inputs,'/tool/validation-input.json')]),timeout=15,output_limit=16*1024,grace=1)
            self.assertNotEqual(result.exit_code,0,'Project package forged validation exit0')
            self.assertNotIn('FORGED_VALIDATION_WORKER_SUCCESS',result.output)
            self.assertIn('zero approved targets',result.output)
    def test_project_package_cannot_replace_default_execution_module(self):
        with tempfile.TemporaryDirectory() as temporary:
            project=self.project(Path(temporary))
            code="import consolecrypt_runner.execution_worker as w;print(w.__file__)"
            result=run_bounded(bubblewrap_argv(project,[sys.executable,'-c',code]),timeout=10,output_limit=4096,grace=1)
            self.assertEqual(result.exit_code,0);self.assertEqual(result.output.strip(),'/tool/python/consolecrypt_runner/execution_worker.py')
    def test_safe_path_preserves_trusted_pythonpath_cwd_argv_and_normal_file_budget(self):
        with tempfile.TemporaryDirectory() as temporary:
            project=self.project(Path(temporary))
            code="import json,os,sys,resource;print(json.dumps({'safe':sys.flags.safe_path,'pythonpath':os.environ['PYTHONPATH'],'cwd':os.getcwd(),'argv':sys.argv[1:],'file_limit':resource.getrlimit(resource.RLIMIT_FSIZE)[0]}))"
            result=run_bounded(bubblewrap_argv(project,[sys.executable,'-c',code,'owned-argument']),timeout=10,output_limit=4096,grace=1)
            self.assertEqual(result.exit_code,0);value=json.loads(result.output)
            self.assertTrue(value['safe']);self.assertEqual(value['pythonpath'],'/tool/python');self.assertEqual(value['cwd'],'/work/project');self.assertEqual(value['argv'],['owned-argument']);self.assertEqual(value['file_limit'],64*1024*1024)
    def test_trusted_child_keeps_existing_git_argv_128mib_classification(self):
        import os
        from unittest.mock import patch
        from consolecrypt_runner import child
        # Actual child code path, only exec/filesystem setup substituted: no
        # second classification implementation and no changed worker argv.
        limits=[];environment={};command=[sys.executable,'-m','consolecrypt_runner.git_worker']
        with patch.object(sys,'argv',['child','--',*command]),patch.object(child.resource,'setrlimit',side_effect=lambda kind,value:limits.append((kind,value))),patch.object(child.Path,'mkdir'),patch.object(child.shutil,'copytree'),patch.object(child.os,'chdir'),patch.object(child.os,'umask'),patch.object(child.os,'environ',environment),patch.object(child.os,'execv') as execute:
            child.main()
        self.assertEqual(next(value for kind,value in limits if kind==resource.RLIMIT_FSIZE),(128*1024*1024,128*1024*1024));self.assertEqual(environment['PYTHONSAFEPATH'],'1');execute.assert_called_once_with(sys.executable,command)
