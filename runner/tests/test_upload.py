import base64
import hashlib
from pathlib import Path
import tempfile
import unittest

from consolecrypt_runner.errors import RunnerError
from consolecrypt_runner.snapshot import create_snapshot
from consolecrypt_runner.upload import UploadManager


class UploadTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix='cc-upload-test-')
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.source = self.root / 'source'
        self.source.mkdir()
        (self.source / 'папка с пробелами').mkdir()
        (self.source / 'папка с пробелами/плейбук.yml').write_bytes(b'---\r\n- hosts: all\r\n')
        (self.source / 'empty').write_bytes(b'')
        (self.source / 'emptydir').mkdir()
        self.snapshot = create_snapshot(self.source, self.root / 'snapshot')
        self.revisions = self.root / 'revisions'
        self.revisions.mkdir(mode=0o700)
        self.manager = UploadManager(self.root / 'uploads', self.revisions)
        self.addCleanup(self.manager.close)

    def transfer(self):
        upload = self.manager.request('upload_begin', {'digest': self.snapshot.digest})
        for entry in self.snapshot.entries:
            self.manager.request('upload_entry', {**upload, 'entry': entry})
            if entry['type'] == 'file' and entry['bytes']:
                data = (self.source / entry['path']).read_bytes()
                self.manager.request('upload_chunk', {**upload, 'offset': 0, 'data': base64.b64encode(data).decode()})
        return upload

    def test_transfer_preserves_unicode_empty_directories_crlf_and_exact_revision(self):
        result = self.manager.request('upload_commit', self.transfer())
        self.assertEqual(result['digest'], self.snapshot.digest)
        captured = self.revisions / result['revision_id'] / 'project'
        self.assertEqual((captured / 'папка с пробелами/плейбук.yml').read_bytes(), b'---\r\n- hosts: all\r\n')
        self.assertTrue((captured / 'emptydir').is_dir())
        with self.assertRaises(RunnerError): self.manager.request('upload_commit', {'upload_id': result['revision_id']})

    def test_incomplete_out_of_order_traversal_hash_and_quota_are_rejected(self):
        upload = self.manager.request('upload_begin', {'digest': self.snapshot.digest})
        with self.assertRaises(RunnerError):
            self.manager.request('upload_entry', {**upload, 'entry': {'type': 'directory', 'path': '../outside'}})
        entry = {'type': 'file', 'path': 'payload', 'mode': 0o600, 'bytes': 3, 'sha256': hashlib.sha256(b'abc').hexdigest()}
        self.manager.request('upload_entry', {**upload, 'entry': entry})
        with self.assertRaises(RunnerError): self.manager.request('upload_commit', upload)
        with self.assertRaises(RunnerError): self.manager.request('upload_chunk', {**upload, 'offset': 1, 'data': 'YWJj'})
        with self.assertRaises(RunnerError): self.manager.request('upload_chunk', {**upload, 'offset': 0, 'data': 'eHh4'})
        self.manager.request('upload_abort', upload)
        upload = self.manager.request('upload_begin', {'digest': self.snapshot.digest})
        self.manager.request('upload_begin', {'digest': self.snapshot.digest})
        with self.assertRaises(RunnerError): self.manager.request('upload_begin', {'digest': self.snapshot.digest})
        with self.assertRaises(RunnerError):
            self.manager.request('upload_entry', {**upload, 'entry': entry | {'bytes': 101 * 1024 * 1024}})

    def test_modified_complete_project_cannot_commit_with_old_native_digest(self):
        upload = self.transfer()
        (self.manager.uploads[upload['upload_id']].root / 'project/empty').write_text('tampered')
        with self.assertRaises(RunnerError): self.manager.request('upload_commit', upload)
        self.assertEqual(list(self.revisions.iterdir()), [])
