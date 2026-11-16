import base64
import hashlib
import os
from pathlib import Path
import sys
import tarfile
import tempfile
import threading
import time
import unittest
from unittest.mock import patch

from consolecrypt_runner.dependency_prepare import DependencyPreparations, capture_archive, MARKER, RECEIPTS, ARCHIVE_FILE
from consolecrypt_runner.dependency_archive import preview_dependency
from consolecrypt_runner.dependencies import check_locked_dependencies, parse_lock, LOCK_FILE
from consolecrypt_runner.errors import RunnerError
from consolecrypt_runner.revisions import RevisionRepository
from consolecrypt_runner.protocol import encode
from consolecrypt_runner.sandbox import bubblewrap_argv
from consolecrypt_runner.snapshot import create_snapshot
from test_dependency_archive import archive
import test_dependency_fetch as tls_fixture


class DependencyWorkerTests(tls_fixture.DependencyFetchTests):
    # Existing real TLS tests are intentionally also run against this fixture.
    def test_real_trusted_worker_tls_sha_and_no_project_execution(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary); root.chmod(0o700)
            raw = capture_archive(root, self.record(), threading.Event(), _ca_pem=self.ca_pem)
            self.assertEqual(raw, self.body)
            self.assertEqual(list(root.iterdir()), [])
            self.assertTrue(self.requests)

    def test_real_tls_worker_through_preview_and_reviewed_snapshot_commit(self):
        self.body = archive([('tasks/main.yml',tarfile.REGTYPE,b'---\n# bytes only\n'),
                            ('lookup_plugins/malicious.py',tarfile.REGTYPE,b'raise RuntimeError("never imported")')])
        with tempfile.TemporaryDirectory() as temporary:
            root=Path(temporary);root.chmod(0o700)
            revisions=RevisionRepository(root/'revisions')
            project=root/'source';project.mkdir(mode=0o700);(project/'play.yml').write_text('---\n')
            base=revisions.capture(project)
            manager=DependencyPreparations(revisions,root/'dependencies')
            record=self.record();selection={key:value for key,value in record.items() if key!='tree_digest'}
            body={'revision_id':base['revision_id'],'revision_digest':base['digest'],'lock_sha256':None,
                  'selection':selection,'strip_components':0,'network_origin':self.url.rsplit('/',1)[0]}
            trusted_capture=lambda parent,record,cancel:capture_archive(parent,record,cancel,_ca_pem=self.ca_pem)
            with patch('consolecrypt_runner.dependency_prepare.capture_archive',side_effect=trusted_capture):
                result=manager.request('dependency_prepare',body,threading.Event())
            identity = {key:result[key] for key in ('preparation_id','review_digest')}
            info = manager.request('dependency_archive_info', identity, threading.Event())
            transferred = bytearray()
            while len(transferred) < info['archive_bytes']:
                chunk = manager.request('dependency_archive_chunk', dict(identity,
                    archive_sha256=info['archive_sha256'],offset=len(transferred),limit=17), threading.Event())
                transferred.extend(base64.b64decode(chunk['data'],validate=True))
                self.assertEqual(chunk['eof'],len(transferred)==len(self.body))
            self.assertEqual(transferred,self.body)
            committed=manager.request('dependency_commit',{
                'preparation_id':result['preparation_id'],'review_digest':result['review_digest'],
                **{key:body[key] for key in ('revision_id','revision_digest','lock_sha256')}},threading.Event())
            tree,snapshot=revisions.read(committed['revision_id'])
            self.assertEqual(check_locked_dependencies(tree,snapshot.to_json()),1)
            self.assertEqual(snapshot.digest,result['review_digest'])

    def test_parent_hard_deadline_terminates_stalled_headers(self):
        release = threading.Event()
        self.request_hook = lambda: release.wait(8)
        started = time.monotonic()
        try:
            with tempfile.TemporaryDirectory() as temporary:
                root = Path(temporary); root.chmod(0o700)
                with self.assertRaisesRegex(RunnerError, 'download failed'):
                    capture_archive(root, self.record(), threading.Event(), _ca_pem=self.ca_pem, _timeout=1)
                self.assertEqual(list(root.iterdir()), [])
            self.assertLess(time.monotonic()-started, 4)
        finally:
            release.set()

    def test_worker_default_ca_denies_untrusted_server(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary); root.chmod(0o700)
            with self.assertRaises(RunnerError):
                capture_archive(root, self.record(), threading.Event())

    def test_network_namespace_flag_requires_exact_trusted_worker(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            with self.assertRaises(RunnerError):
                bubblewrap_argv(root, [sys.executable, 'project.py'], dependency_download=True)
            with self.assertRaises(RunnerError):
                bubblewrap_argv(root, [sys.executable,'-m','consolecrypt_runner.dependency_worker'],
                                dependency_download=True, socket_directory=root)
            argv = bubblewrap_argv(root, [sys.executable,'-m','consolecrypt_runner.dependency_worker'], dependency_download=True)
            self.assertIn('--clearenv', argv); self.assertIn('--share-net', argv)
            offline = bubblewrap_argv(root, [sys.executable, 'project.py'])
            self.assertNotIn('--share-net', offline)
            self.assertNotIn('/etc/resolv.conf', offline)


@unittest.skipUnless(os.name == 'posix' and os.getuid() != 0, 'Non-root Unix preparation')
class DependencyPreparationTests(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory(); self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name); self.root.chmod(0o700)
        self.revisions = RevisionRepository(self.root/'revisions')
        self.source = self.root/'source'; self.source.mkdir(mode=0o700)
        (self.source/'play.yml').write_text('---\n- hosts: all\n  roles: [demo]\n')
        self.base = self.revisions.capture(self.source)
        self.manager = DependencyPreparations(self.revisions, self.root/'dependencies')
        self.raw = archive([('tasks/main.yml', tarfile.REGTYPE, b'---\n# Malicious content is only bytes\n'),
                            ('lookup_plugins/evil.py', tarfile.REGTYPE, b'raise RuntimeError("must never execute")')])
        self.selection = {'kind':'role','name':'demo','version':'1.2.3',
                          'source':{'url':'https://example.test/exact.tar.gz','sha256':hashlib.sha256(self.raw).hexdigest()}}
        self.body = {'revision_id':self.base['revision_id'],'revision_digest':self.base['digest'], 'lock_sha256':None,
                     'selection':self.selection,'strip_components':0,'network_origin':'https://example.test'}
        self.cancel = threading.Event()

    def prepare(self):
        with patch('consolecrypt_runner.dependency_prepare.capture_archive', return_value=self.raw):
            return self.manager.request('dependency_prepare', self.body, self.cancel)

    def commit_body(self, result):
        return {key:self.body[key] for key in ('revision_id','revision_digest','lock_sha256')} | {
            'preparation_id':result['preparation_id'],'review_digest':result['review_digest']}

    def test_preview_computes_first_tree_pin_and_commit_preserves_base(self):
        result = self.prepare()
        self.assertNotEqual(result['record']['tree_digest'], '0'*64)
        self.assertEqual(len(self.revisions.list()), 1)  # Preview cannot run as an admitted revision.
        files = self.manager.request('dependency_files', {'preparation_id':result['preparation_id'],'offset':0,'limit':100}, self.cancel)
        self.assertTrue(any(item['path'] == RECEIPTS for item in files['entries']))
        chunk = self.manager.request('dependency_chunk', {'preparation_id':result['preparation_id'],'path':'roles/demo/tasks/main.yml',
                                      'offset':0,'limit':100}, self.cancel)
        self.assertIn(b'Malicious', base64.b64decode(chunk['data']))
        committed = self.manager.request('dependency_commit', self.commit_body(result), self.cancel)
        project, snapshot = self.revisions.read(committed['revision_id'])
        self.assertEqual(snapshot.digest, result['review_digest'])
        self.assertEqual(check_locked_dependencies(project, snapshot.to_json()), 1)
        base, original = self.revisions.read(self.base['revision_id'])
        self.assertEqual(create_snapshot(base, self.root/'base-check').digest, original.digest)
        self.assertFalse((base/LOCK_FILE).exists()); self.assertFalse((self.source/'roles').exists())
        self.assertEqual(self.manager.request('dependency_status', {'preparation_id':result['preparation_id']},self.cancel)['state'], 'committed')
        with self.assertRaises(RunnerError):
            self.manager.request('dependency_commit', self.commit_body(result), self.cancel)

    def test_archive_transfer_identity_bounds_cancel_retention_and_no_project_inclusion(self):
        result = self.prepare()
        identity = {key:result[key] for key in ('preparation_id','review_digest')}
        info = self.manager.request('dependency_archive_info',identity,self.cancel)
        self.assertEqual(info['archive_bytes'],len(self.raw))
        self.assertEqual(info['archive_sha256'],self.selection['source']['sha256'])
        request = dict(identity,archive_sha256=info['archive_sha256'],offset=0,limit=128*1024)
        chunk = self.manager.request('dependency_archive_chunk',request,self.cancel)
        self.assertEqual(base64.b64decode(chunk['data'],validate=True),self.raw)
        self.assertTrue(chunk['eof'])
        directory = self.manager.previews.root/result['preparation_id']
        self.assertTrue((directory/ARCHIVE_FILE).is_file())
        self.assertFalse((directory/'project'/ARCHIVE_FILE).exists())
        for changes in ({'review_digest':'0'*64},{'archive_sha256':'0'*64},{'offset':-1},
                        {'offset':True},{'offset':len(self.raw)},{'limit':0},{'limit':128*1024+1},
                        {'limit':True},{'path':'source-archive.bin'}):
            with self.assertRaises(RunnerError):
                self.manager.request('dependency_archive_chunk',request|changes,self.cancel)
        self.cancel.set()
        with self.assertRaises(RunnerError):self.manager.request('dependency_archive_info',identity,self.cancel)
        self.cancel.clear()
        restarted=DependencyPreparations(self.revisions,self.root/'dependencies')
        self.assertEqual(restarted.request('dependency_archive_info',identity,self.cancel),info)
        self.commit_body(result)
        (directory/ARCHIVE_FILE).write_bytes(b'truncated')
        with self.assertRaises(RunnerError):restarted.request('dependency_archive_info',identity,self.cancel)

    def test_archive_symlink_and_hardlink_replacements_are_denied(self):
        result=self.prepare();identity={key:result[key] for key in ('preparation_id','review_digest')}
        path=self.manager.previews.root/result['preparation_id']/ARCHIVE_FILE
        saved=path.with_suffix('.saved');path.rename(saved)
        path.symlink_to(saved)
        with self.assertRaises((RunnerError,OSError)):
            self.manager.request('dependency_archive_info',identity,self.cancel)
        path.unlink();os.link(saved,path)
        with self.assertRaises(RunnerError):self.manager.request('dependency_archive_info',identity,self.cancel)

    def test_origin_credentials_query_layout_and_tree_pin_input_are_rejected_before_network(self):
        for changes in ({'network_origin':'https://other.test'}, {'strip_components':True},
                        {'selection':dict(self.selection,tree_digest='a'*64)},
                        {'selection':dict(self.selection,source={'url':'https://user:secret@example.test/x','sha256':'a'*64})},
                        {'selection':dict(self.selection,source={'url':'https://example.test/x?token=secret','sha256':'a'*64})}):
            with patch('consolecrypt_runner.dependency_prepare.capture_archive') as network:
                with self.assertRaises(RunnerError):
                    self.manager.request('dependency_prepare', self.body | changes, self.cancel)
                network.assert_not_called()

    def test_existing_target_portable_collision_and_receipt_never_overwritten(self):
        for name in ('roles/demo','Roles','roles/DEMO',RECEIPTS):
            with tempfile.TemporaryDirectory(dir=self.root) as temporary:
                source = Path(temporary)/'project'; source.mkdir(mode=0o700)
                target = source/name; target.parent.mkdir(mode=0o700, parents=True, exist_ok=True)
                target.write_text('user content')
                base = self.revisions.capture(source)
                body = self.body | {'revision_id':base['revision_id'],'revision_digest':base['digest']}
                with patch('consolecrypt_runner.dependency_prepare.capture_archive') as network:
                    with self.assertRaises(RunnerError):
                        self.manager.request('dependency_prepare', body, self.cancel)
                    network.assert_not_called()
                self.assertEqual(target.read_text(), 'user content')

    def test_lock_cas_and_review_digest_mismatch(self):
        result = self.prepare()
        for changes in ({'lock_sha256':'a'*64}, {'revision_digest':'a'*64}, {'review_digest':'a'*64}):
            with self.assertRaises(RunnerError):
                self.manager.request('dependency_commit', self.commit_body(result) | changes, self.cancel)
        self.assertEqual(len(self.revisions.list()), 1)

    def test_archive_failure_does_not_publish_candidate(self):
        self.raw = archive([('tasks/main.yml',tarfile.SYMTYPE,b'')])
        self.body['selection']['source']['sha256'] = hashlib.sha256(self.raw).hexdigest()
        with self.assertRaises(RunnerError): self.prepare()
        self.assertEqual(self.manager.previews.list(), [])

    def test_interrupted_capture_retains_durable_marker_and_blocks_restart_commit(self):
        result = self.prepare(); body = self.commit_body(result)
        with patch.object(self.revisions, 'capture', side_effect=OSError('synthetic disk failure')):
            with self.assertRaises(OSError):
                self.manager.request('dependency_commit',body,self.cancel)
        marker = self.manager.previews.root/result['preparation_id']/MARKER
        self.assertTrue(marker.is_file())
        restarted = DependencyPreparations(self.revisions,self.root/'dependencies')
        self.assertEqual(restarted.request('dependency_status',{'preparation_id':result['preparation_id']},self.cancel),
                         {'state':'recovery_required','candidate_revision_ids':[]})
        with self.assertRaises(RunnerError): restarted.request('dependency_commit',body,self.cancel)
        self.assertTrue(marker.exists()); self.assertEqual(len(self.revisions.list()),1)

    def test_failure_after_admission_does_not_delete_tree_or_lock(self):
        result = self.prepare(); original_atomic = __import__('consolecrypt_runner.dependency_prepare',fromlist=['atomic_json']).atomic_json
        def fail_receipt(path, *args, **kwargs):
            if path.name == 'committed.json': raise OSError('synthetic receipt fsync failure')
            return original_atomic(path,*args,**kwargs)
        with patch('consolecrypt_runner.dependency_prepare.atomic_json',side_effect=fail_receipt):
            with self.assertRaises(OSError): self.manager.request('dependency_commit',self.commit_body(result),self.cancel)
        self.assertEqual(len(self.revisions.list()),1)  # Pending admission is hidden and unreadable.
        self.assertEqual(self.manager.request('dependency_status',{'preparation_id':result['preparation_id']},self.cancel)['state'],'recovery_required')
        candidate = next(item for item in self.revisions.root.iterdir() if item.name != self.base['revision_id'])
        self.assertTrue((candidate/'project'/LOCK_FILE).exists())
        self.assertTrue((candidate/'dependency-admission-pending.json').exists())
        with self.assertRaises(RunnerError): self.revisions.read(candidate.name)
        with self.assertRaises(RunnerError): self.revisions.capture(self.source)
        recovered=self.manager.request('dependency_recover',self.commit_body(result)|{'candidate_revision_id':candidate.name},self.cancel)
        self.assertEqual(recovered['revision_id'],candidate.name)
        project,snapshot=self.revisions.read(candidate.name)
        self.assertEqual(check_locked_dependencies(project,snapshot.to_json()),1)
        self.assertEqual(self.manager.request('dependency_status',{'preparation_id':result['preparation_id']},self.cancel)['state'],'committed')

    def test_recovery_refuses_wrong_candidate_and_tampered_candidate(self):
        result=self.prepare()
        original_atomic=__import__('consolecrypt_runner.dependency_prepare',fromlist=['atomic_json']).atomic_json
        def fail_receipt(path,*args,**kwargs):
            if path.name=='committed.json':raise OSError('synthetic')
            return original_atomic(path,*args,**kwargs)
        with patch('consolecrypt_runner.dependency_prepare.atomic_json',side_effect=fail_receipt):
            with self.assertRaises(OSError):self.manager.request('dependency_commit',self.commit_body(result),self.cancel)
        wrong=self.commit_body(result)|{'candidate_revision_id':self.base['revision_id']}
        with self.assertRaises(RunnerError):self.manager.request('dependency_recover',wrong,self.cancel)
        candidate=next(item for item in self.revisions.root.iterdir() if item.name!=self.base['revision_id'])
        (candidate/'project/roles/demo/tasks/main.yml').write_text('tampered')
        with self.assertRaises(RunnerError):
            self.manager.request('dependency_recover',self.commit_body(result)|{'candidate_revision_id':candidate.name},self.cancel)
        self.assertTrue((candidate/'dependency-admission-pending.json').exists())

    def test_collection_prepare_and_commit(self):
        self.raw=archive([('MANIFEST.json',tarfile.REGTYPE,b'{}')])
        self.body['selection']={'kind':'collection','name':'demo.tools','version':'2.0',
                               'source':{'url':'https://example.test/exact.tar.gz','sha256':hashlib.sha256(self.raw).hexdigest()}}
        result=self.prepare()
        committed=self.manager.request('dependency_commit',self.commit_body(result),self.cancel)
        project,snapshot=self.revisions.read(committed['revision_id'])
        self.assertTrue((project/'collections/ansible_collections/demo/tools/MANIFEST.json').is_file())
        self.assertEqual(check_locked_dependencies(project,snapshot.to_json()),1)

    def test_default_helper_has_no_dependency_control_authority(self):
        from consolecrypt_runner.__main__ import Helper
        helper=Helper(self.root/'default-helper')
        try:
            with self.assertRaises(RunnerError):
                helper.request('dependency_prepare',self.body,self.cancel)
            for operation in ('dependency_archive_info','dependency_archive_chunk'):
                with self.assertRaises(RunnerError):helper.request(operation,{},self.cancel)
            self.assertIsNone(helper.dependency_preparations)
        finally:helper.close()

    def test_cancelled_commit_and_modified_private_snapshot_fail_closed(self):
        result = self.prepare(); self.cancel.set()
        with self.assertRaises(RunnerError): self.manager.request('dependency_commit',self.commit_body(result),self.cancel)
        self.cancel.clear()
        project,_ = self.manager.previews.read(result['preparation_id'])
        (project/'roles/demo/tasks/main.yml').write_text('tampered')
        with self.assertRaises(RunnerError): self.manager.request('dependency_commit',self.commit_body(result),self.cancel)
        self.assertEqual(len(self.revisions.list()),1)


def load_tests(loader, tests, pattern):
    suite=unittest.TestSuite()
    for cls in (DependencyWorkerTests,DependencyPreparationTests):
        for name in sorted(cls.__dict__):
            if name.startswith('test_'):suite.addTest(cls(name))
    return suite
