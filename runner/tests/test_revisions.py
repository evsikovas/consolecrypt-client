from pathlib import Path
import tempfile
import unittest
import uuid

from consolecrypt_runner.errors import RunnerError
from consolecrypt_runner.revisions import RevisionRepository


class RevisionQuotaTests(unittest.TestCase):
    def test_total_budget_and_job_references_prevent_silent_eviction(self):
        with tempfile.TemporaryDirectory() as folder:
            root = Path(folder)
            source = root / 'source'; source.mkdir()
            (source / 'file').write_bytes(b'abc')
            repository = RevisionRepository(root / 'revisions', max_bytes=5)
            first = repository.capture(source)
            with self.assertRaises(RunnerError): repository.capture(source)
            self.assertEqual([item['revision_id'] for item in repository.list()], [first['revision_id']])
            with self.assertRaises(RunnerError): repository.release(first['revision_id'], referenced=lambda *_: True)
            self.assertTrue(repository.release(first['revision_id'], referenced=lambda *_: False)['released'])
            self.assertEqual(repository.list(), [])

    def test_failed_capture_and_unknown_reads_do_not_leak_partial_revisions(self):
        with tempfile.TemporaryDirectory() as folder:
            root = Path(folder)
            source = root / 'source'; source.mkdir()
            (source / 'file').write_bytes(b'abc')
            repository = RevisionRepository(root / 'revisions')
            with self.assertRaises(RunnerError): repository.read(str(uuid.uuid4()))
            with self.assertRaises(RunnerError): repository.capture(source, expected_digest='f' * 64)
            self.assertEqual(list(repository.root.iterdir()), [])
