import errno
import hashlib
import json
import os
from pathlib import Path
import sys
import tempfile
import threading
import unittest
from unittest.mock import patch

from consolecrypt_runner.errors import RunnerError
from consolecrypt_runner.protocol import encode
from consolecrypt_runner.sandbox import bubblewrap_argv, run_bounded, ProcessResult
from consolecrypt_runner.snapshot import create_snapshot
from consolecrypt_runner.validation import validate_project


MARKER = 'synthetic-validation-private-only-4532'


class ValidationRetentionTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory(prefix='cc-validation-retention-')
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        project = self.root / 'project'
        project.mkdir()
        (project / 'playbook.yml').write_text(
            '---\n- name: Input retention fixture\n  hosts: all\n  gather_facts: false\n  tasks: []\n',
            encoding='utf-8')
        self.snapshot = create_snapshot(project, self.root / 'snapshot')
        self.private = self.root / 'private'
        self.private.mkdir(mode=0o700)
        self.parameters = {'extra_vars': {'synthetic_private': MARKER},
                           'tags': [], 'skip_tags': [], 'diff': False}

    def validate(self, **kwargs):
        with patch('consolecrypt_runner.validation.probe', return_value={
                'capabilities': {'validate': True}, 'environment_digest': 'synthetic-environment'}):
            return validate_project(self.root / 'snapshot', self.snapshot, 'playbook.yml',
                                    parameters=self.parameters, private_directory=self.private, **kwargs)

    def assert_private_bytes_not_named(self):
        for path in self.root.rglob('*'):
            if path.is_file():
                self.assertNotIn(MARKER.encode(), path.read_bytes(), path.as_posix())

    def test_private_input_never_named_and_each_phase_owns_its_sealed_fd(self):
        owned = []
        descriptors = []

        def actual_data_observer(argv, **kwargs):
            self.assert_private_bytes_not_named()
            value = kwargs['validation_input']
            owned.append(value)
            fd = value.inherited_fds()[0]
            descriptors.append(fd)
            info = os.fstat(fd)
            self.assertEqual(info.st_nlink, 0)
            self.assertEqual(info.st_mode & 0o777, 0o600)
            body = json.loads(os.pread(fd, info.st_size, 0))
            self.assertEqual(body['parameters']['extra_vars']['synthetic_private'], MARKER)
            self.assertEqual(value.mounts(), ((fd, '/tool/validation-input.json'),))
            self.assertNotIn('/tool/job-input.json', argv)
            if len(owned) == 2:
                with self.assertRaises(RunnerError):
                    owned[0].mounts()
            return ProcessResult(0, '', False, False, False, 1)

        with patch('consolecrypt_runner.validation.run_bounded', side_effect=actual_data_observer):
            result = self.validate()
        self.assertTrue(result.valid)
        self.assertEqual(len(owned), 2)
        self.assertIsNot(owned[0], owned[1])
        for fd in descriptors:
            with self.assertRaises(OSError):
                os.fstat(fd)
        self.assert_private_bytes_not_named()
        self.assertEqual(list(self.private.iterdir()), [])

    def test_cancel_stops_before_second_phase_and_closes_original_input(self):
        seen = []

        def cancelled(argv, **kwargs):
            seen.append(kwargs['validation_input'])
            self.assert_private_bytes_not_named()
            return ProcessResult(-15, '', False, True, False, 1)

        with patch('consolecrypt_runner.validation.run_bounded', side_effect=cancelled):
            result = self.validate(cancel=threading.Event())
        self.assertFalse(result.valid)
        self.assertEqual(len(result.checks), 1)
        self.assertEqual(len(seen), 1)
        with self.assertRaises(RunnerError):
            seen[0].mounts()
        self.assert_private_bytes_not_named()

    def test_spawn_error_closes_input_without_leaving_a_named_copy(self):
        seen = []

        def failed(argv, **kwargs):
            seen.append(kwargs['validation_input'])
            self.assert_private_bytes_not_named()
            raise OSError('synthetic spawn failure')

        with patch('consolecrypt_runner.validation.run_bounded', side_effect=failed):
            with self.assertRaises(OSError):
                self.validate()
        with self.assertRaises(RunnerError):
            seen[0].mounts()
        self.assert_private_bytes_not_named()
        self.assertEqual(list(self.private.iterdir()), [])

    def test_memfd_failure_never_falls_back_to_named_input_or_starts_worker(self):
        with patch('consolecrypt_runner.ephemeral_input.os.memfd_create', side_effect=OSError('synthetic unsupported')):
            with patch('consolecrypt_runner.validation.run_bounded') as worker:
                with self.assertRaises(OSError):
                    self.validate()
                worker.assert_not_called()
        self.assert_private_bytes_not_named()
        self.assertEqual(list(self.private.iterdir()), [])


class ValidationAnonymousKernelTests(unittest.TestCase):
    def test_actual_bwrap_reads_large_readonly_validation_input_and_closes_setup_fd(self):
        from consolecrypt_runner.ephemeral_input import EphemeralValidationInput
        body = encode({'synthetic': MARKER + 'x' * (1024 * 1024)})
        with tempfile.TemporaryDirectory(prefix='cc-validation-kernel-') as name:
            root = Path(name)
            with EphemeralValidationInput(body) as value:
                fd = value.inherited_fds()[0]
                code = '''import errno,hashlib,os,sys
from pathlib import Path
p=Path('/tool/validation-input.json')
assert hashlib.sha256(p.read_bytes()).hexdigest()==sys.argv[1]
assert p.stat().st_mode & 0o777==0o600
try:p.write_bytes(b'changed')
except OSError as e:assert e.errno==errno.EROFS
else:raise AssertionError('writable input')
try:os.fstat(int(sys.argv[2]))
except OSError as e:assert e.errno==errno.EBADF
else:raise AssertionError('setup fd leaked')
assert not Path('/tool/job-input.json').exists()
print('VALIDATION_INPUT_OK')
'''
                argv = bubblewrap_argv(root, [sys.executable, '-c', code,
                    hashlib.sha256(body).hexdigest(), str(fd)], validation_input=value)
                result = run_bounded(argv, validation_input=value, timeout=5, grace=1)
                self.assertEqual(result.exit_code, 0, result.output)
                self.assertEqual(result.output.strip(), 'VALIDATION_INPUT_OK')
                with self.assertRaises(RunnerError):
                    value.prepare_for_spawn()
            with self.assertRaises(OSError):
                os.fstat(fd)
            self.assertEqual(list(root.iterdir()), [])

    def test_fixed_validation_mount_cannot_be_replaced_duplicated_or_used_as_execution(self):
        from consolecrypt_runner.ephemeral_input import EphemeralJobInput, EphemeralValidationInput, VALIDATION_TARGET
        with tempfile.TemporaryDirectory() as name:
            root = Path(name)
            with EphemeralValidationInput(b'{}') as value, EphemeralJobInput(b'{}') as execution:
                for kwargs in ({'job_input': value}, {'validation_input': execution},
                               {'validation_input': value, 'job_input': execution},
                               {'validation_input': value, 'extra_readonly': [(root, VALIDATION_TARGET)]}):
                    with self.assertRaises(RunnerError):
                        bubblewrap_argv(root, [sys.executable, '-c', 'pass'], **kwargs)
                fd = value.inherited_fds()[0]
                exact = ['--ro-bind-data', str(fd), VALIDATION_TARGET]
                for argv in (['bwrap', *exact, *exact], [sys.executable, '-c', 'pass']):
                    with patch('consolecrypt_runner.sandbox.subprocess.Popen') as spawn:
                        with self.assertRaises(RunnerError):
                            run_bounded(argv, validation_input=value)
                        spawn.assert_not_called()


if __name__ == '__main__':
    unittest.main()
