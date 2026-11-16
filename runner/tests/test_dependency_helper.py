import base64
import hashlib
import io
import json
import os
from pathlib import Path
import subprocess
import sys
import tarfile
import tempfile
import threading
import unittest
from unittest.mock import patch
import uuid

from consolecrypt_runner.dependency_helper import DependencyHelper, operation_path, serve_dependency, ALLOWED
from consolecrypt_runner.errors import RunnerError
from consolecrypt_runner.protocol import encode
from test_dependency_archive import archive


class DependencyHelperTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix='cc-dependency-mode-')
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.root.chmod(0o700)
        self.configured = self.root/'jobs'
        self.configured.mkdir(mode=0o700)
        (self.configured/'user-sentinel').write_bytes(b'unchanged')
        self.operation = str(uuid.uuid4())

    def helper(self):
        helper = DependencyHelper(self.configured, self.operation)
        self.addCleanup(helper.close)
        return helper

    def upload(self, helper):
        raw = b'---\n'
        entry = {'type':'file','path':'play.yml','mode':0o600,'bytes':len(raw),
                 'sha256':hashlib.sha256(raw).hexdigest()}
        digest = hashlib.sha256(encode({'version':1,'entries':[entry]})).hexdigest()
        cancel = threading.Event()
        transfer = helper.request('upload_begin', {'digest':digest}, cancel)['upload_id']
        helper.request('upload_entry', {'upload_id':transfer,'entry':entry}, cancel)
        helper.request('upload_chunk', {'upload_id':transfer,'offset':0,'data':base64.b64encode(raw).decode()}, cancel)
        return helper.request('upload_commit', {'upload_id':transfer}, cancel)

    def test_no_job_store_executor_targets_or_default_root_mutation(self):
        from consolecrypt_runner import __main__ as main
        with patch.object(main, 'JobStore', side_effect=AssertionError('job store forbidden')), \
             patch.object(main, 'InteractiveExecutor', side_effect=AssertionError('executor forbidden')), \
             patch.object(main, 'TargetChannel', side_effect=AssertionError('target channel forbidden')):
            helper = self.helper()
            self.assertFalse(hasattr(helper, 'store'))
            self.assertFalse(hasattr(helper, 'executor'))
            result = helper.request('probe', {}, threading.Event())
            self.assertEqual(result['mode'], 'dependency_prepare')
            self.assertEqual(result['mode_version'], 1)
            self.assertEqual(result['operation_id'], self.operation)
            self.assertIs(result['execution_enabled'], False)
            self.assertIs(result['target_dispatch_enabled'], False)
            base = self.upload(helper)
            page = helper.request('files', {'revision_id':base['revision_id'],'offset':0,'limit':100}, threading.Event())
            self.assertEqual(page['total'], 1)
        self.assertEqual([p.name for p in self.configured.iterdir()], ['user-sentinel'])
        self.assertEqual((self.configured/'user-sentinel').read_bytes(), b'unchanged')

    def test_all_job_enrollment_target_git_inventory_capture_commit_recover_operations_denied(self):
        helper = self.helper()
        for operation in ('configure_interactive','accept_job','job_get','job_cancel','job_events','vault_state',
                          'enroll','enroll_job','target_request','exec','validate','prepare_run','capture',
                          'git_capture','inventory_capture','inventory_upload_begin',
                          'dependency_commit','dependency_recover','revision_list'):
            with self.assertRaises(RunnerError) as error:
                helper.request(operation, {}, threading.Event())
            self.assertEqual(error.exception.code, 'unsupported_operation')
            self.assertNotIn(operation, ALLOWED)

    def test_raw_target_frames_never_reach_a_target_channel(self):
        helper = self.helper()
        for kind in ('target_request','target_response','target_revoke'):
            with self.assertRaises(RunnerError):
                serve_dependency(helper, io.BytesIO(encode({'version':1,'type':kind})+b'\n'), io.BytesIO())

    def test_fixed_prepare_archive_roundtrip_and_no_commit_or_project_execution(self):
        helper = self.helper()
        base = self.upload(helper)
        raw = archive([('tasks/main.yml',tarfile.REGTYPE,b'---\n'),
                       ('lookup_plugins/evil.py',tarfile.REGTYPE,b'raise RuntimeError("never imported")')])
        selection = {'kind':'role','name':'demo','version':'1.2.3',
                     'source':{'url':'https://source.example.test/pinned.tar.gz',
                               'sha256':hashlib.sha256(raw).hexdigest()}}
        body = {'revision_id':base['revision_id'],'revision_digest':base['digest'],
                'lock_sha256':None,'selection':selection,'strip_components':0,
                'network_origin':'https://source.example.test'}
        with patch('consolecrypt_runner.dependency_prepare.capture_archive', return_value=raw) as download:
            prepared = helper.request('dependency_prepare', body, threading.Event())
            with self.assertRaises(RunnerError):
                helper.request('dependency_prepare', body, threading.Event())
            self.assertEqual(download.call_count, 1)
        identity = {key:prepared[key] for key in ('preparation_id','review_digest')}
        info = helper.request('dependency_archive_info', identity, threading.Event())
        result = bytearray()
        while len(result) < info['archive_bytes']:
            chunk = helper.request('dependency_archive_chunk', dict(identity,
                archive_sha256=selection['source']['sha256'],offset=len(result),limit=19), threading.Event())
            result.extend(base64.b64decode(chunk['data'], validate=True))
        self.assertEqual(bytes(result), raw)
        self.assertEqual(helper.request('dependency_status', {'preparation_id':prepared['preparation_id']},
                                        threading.Event())['state'], 'review_required')
        self.assertEqual(len(helper.revisions.list()), 1)
        self.assertFalse(any(helper.root.rglob('committed.json')))

    def test_existing_same_uuid_and_other_operation_are_preserved(self):
        helper = self.helper()
        path = operation_path(self.configured, self.operation)
        (path/'retained').write_bytes(b'own retained crash evidence')
        before = sorted(str(p.relative_to(path)) for p in path.rglob('*'))
        with self.assertRaises(FileExistsError):
            DependencyHelper(self.configured, self.operation)
        self.assertEqual(sorted(str(p.relative_to(path)) for p in path.rglob('*')), before)
        other = DependencyHelper(self.configured, str(uuid.uuid4()))
        try:
            self.assertNotEqual(os.fstat(other._fd).st_ino, os.fstat(helper._fd).st_ino)
        finally:other.close()
        self.assertEqual((path/'retained').read_bytes(), b'own retained crash evidence')

    def test_unsafe_root_namespace_family_and_ancestors_fail_without_chmod_or_cleanup(self):
        outside = self.root/'outside';outside.mkdir(mode=0o700)
        (outside/'sentinel').write_bytes(b'unchanged')
        linked = self.root/'linked';linked.symlink_to(outside, target_is_directory=True)
        for configured in (linked/'jobs', linked):
            with self.assertRaises((OSError, RunnerError)):
                DependencyHelper(configured, str(uuid.uuid4()))
        path = operation_path(self.configured, self.operation)
        namespace = path.parent.parent
        namespace.mkdir(mode=0o755)
        with self.assertRaises(RunnerError):DependencyHelper(self.configured, self.operation)
        self.assertEqual(namespace.stat().st_mode & 0o777, 0o755)
        namespace.chmod(0o700)
        path.parent.symlink_to(outside, target_is_directory=True)
        with self.assertRaises(OSError):DependencyHelper(self.configured, self.operation)
        self.assertEqual([p.name for p in outside.iterdir()], ['sentinel'])

    def test_parent_replacement_cannot_redirect_reused_upload_or_snapshot_writes(self):
        parent = self.root/'parent';parent.mkdir(mode=0o700)
        configured = parent/'jobs';configured.mkdir(mode=0o700)
        helper = DependencyHelper(configured, self.operation)
        self.addCleanup(helper.close)
        opened = os.fstat(helper._fd)
        moved = self.root/'moved'
        parent.rename(moved)
        outside = self.root/'outside';outside.mkdir(mode=0o700)
        (outside/'sentinel').write_bytes(b'unchanged')
        parent.symlink_to(outside, target_is_directory=True)
        base = self.upload(helper)
        tree, snapshot = helper.revisions.read(base['revision_id'])
        self.assertEqual((tree/'play.yml').read_bytes(), b'---\n')
        self.assertEqual(snapshot.digest, base['digest'])
        self.assertEqual((os.fstat(helper._fd).st_dev,os.fstat(helper._fd).st_ino),
                         (opened.st_dev,opened.st_ino))
        self.assertEqual([p.name for p in outside.iterdir()], ['sentinel'])
        self.assertTrue((moved/'.consolecrypt-dependency-operations').is_dir())

    def test_invalid_ids_relative_and_dot_roots_and_root_account_fail_closed(self):
        for operation in ('0'*32, str(uuid.UUID(int=0)), 'AAAAAAAA-AAAA-4AAA-8AAA-AAAAAAAAAAAA'):
            with self.assertRaises(RunnerError):operation_path(self.configured, operation)
        for value in ('relative','/','/tmp/../jobs','/tmp/./jobs','/tmp//jobs'):
            with self.assertRaises(RunnerError):operation_path(value, self.operation)
        with patch('consolecrypt_runner.dependency_helper.os.getuid', return_value=0):
            with self.assertRaises(RunnerError):DependencyHelper(self.configured, self.operation)

    def cli(self, args, frames):
        module = str(Path(__file__).resolve().parents[1]/'src')
        bootstrap = "import sys,runpy;sys.path.insert(0,sys.argv.pop(1));runpy.run_module('consolecrypt_runner',run_name='__main__')"
        return subprocess.run([sys.executable,'-I','-B','-c',bootstrap,module,*args],
            input=b''.join(encode(frame)+b'\n' for frame in frames),capture_output=True,timeout=20)

    def test_actual_cli_mode_identity_rejection_and_default_gate_unchanged(self):
        request = lambda operation: {'version':1,'request_id':str(uuid.uuid4()),'operation':operation,'body':{}}
        result = self.cli(['--dependency-stdio','--root',str(self.configured),'--operation-id',self.operation],
                          [request('probe'), request('accept_job'), request('dependency_commit')])
        self.assertEqual(result.returncode, 0, result.stderr)
        replies = [json.loads(line) for line in result.stdout.splitlines()]
        self.assertEqual(len(replies),3)
        self.assertEqual(next(row['result'] for row in replies if row['result'])['operation_id'],self.operation)
        self.assertEqual(sum(row['error'] is not None for row in replies),2)
        probe = next(row['result'] for row in replies if row['result'])
        self.assertEqual(probe['root_family'], hashlib.sha256(b'jobs').hexdigest())
        self.assertEqual(probe['root_namespace'], '.consolecrypt-dependency-operations')
        self.assertEqual(probe['protocol'],1)
        target = self.cli(['--dependency-stdio','--root',str(self.configured),'--operation-id',str(uuid.uuid4())],
                          [{'version':1,'type':'target_response','secret':'SYNTHETIC_MUST_NOT_ECHO'}])
        self.assertEqual(target.returncode,65)
        self.assertEqual(target.stdout,b'')
        self.assertEqual(target.stderr,b'')
        ordinary = self.cli(['--stdio','--root',str(self.root/'normal-default')], [request('dependency_prepare')])
        self.assertEqual(ordinary.returncode,0,ordinary.stderr)
        self.assertEqual(json.loads(ordinary.stdout)['error']['code'],'unsupported_operation')
        invalid = self.cli(['--dependency-stdio','--stdio','--root',str(self.configured),
                            '--operation-id',str(uuid.uuid4())], [])
        self.assertEqual(invalid.returncode,65)
        self.assertEqual(invalid.stdout,b'')

    def test_actual_default_probe_has_no_dependency_mode_or_capability(self):
        result = self.cli(['--probe'], [])
        self.assertEqual(result.returncode,0,result.stderr)
        probe = json.loads(result.stdout)
        self.assertNotIn('mode',probe)
        self.assertNotIn('operation_id',probe)
        self.assertFalse(any('dependency' in key for key in probe['capabilities']))


if __name__ == '__main__':unittest.main()
