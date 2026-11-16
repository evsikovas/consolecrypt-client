from pathlib import Path
import tempfile
import unittest

from consolecrypt_runner.errors import RunnerError
from consolecrypt_runner.integrity import dependency_fingerprint


class Distribution:
    metadata = {'Name': 'fixture-dependency'}
    version = '1.0.0'
    files = ['module.py']

    def __init__(self, root):
        self.root = root

    def locate_file(self, relative):
        return self.root / relative


class DependencyIntegrityTests(unittest.TestCase):
    def test_content_and_mode_changes_invalidate_the_same_reported_version(self):
        with tempfile.TemporaryDirectory() as folder:
            root = Path(folder)
            file = root / 'module.py'
            file.write_bytes(b'VALUE = 1\n')
            distribution = Distribution(root)
            original = dependency_fingerprint([distribution], root)
            self.assertEqual(original, dependency_fingerprint([distribution], root))
            file.write_bytes(b'VALUE = 2\n')
            self.assertNotEqual(original, dependency_fingerprint([distribution], root))

    def test_dependency_cannot_add_an_outside_file_to_the_fingerprint(self):
        with tempfile.TemporaryDirectory() as folder:
            root = Path(folder)
            (root / 'private').mkdir()
            (root / 'module.py').write_text('outside')
            distribution = Distribution(root / 'private')
            distribution.files = ['../module.py']
            with self.assertRaises(RunnerError):
                dependency_fingerprint([distribution], root / 'private')
