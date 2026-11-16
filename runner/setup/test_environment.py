import importlib.util
from pathlib import Path
from types import SimpleNamespace
import tempfile
import unittest
from unittest.mock import patch

spec = importlib.util.spec_from_file_location('cc_setup', Path(__file__).with_name('environment.py'))
setup = importlib.util.module_from_spec(spec)
spec.loader.exec_module(setup)


class SetupTests(unittest.TestCase):
    def test_noexec_home_is_not_a_ready_installation_location(self):
        with patch.object(setup.os, 'statvfs', return_value=SimpleNamespace(f_flag=setup.os.ST_NOEXEC)):
            report = setup.scan()
        self.assertIn('executable_home', report['missing'])
        self.assertFalse(report['install_supported'])

    def test_scan_does_not_create_installation_directories(self):
        with tempfile.TemporaryDirectory() as temporary:
            parent = Path(temporary) / 'not-created'
            with patch.object(setup, 'managed_root', return_value=parent):
                report = setup.scan()
            self.assertEqual(report['version'], 1)
            self.assertFalse(parent.exists())

    def test_install_refuses_missing_prerequisites_before_writing(self):
        with patch.object(setup, 'scan', return_value={'install_supported': False}), \
             patch.object(setup, 'private_directory', side_effect=AssertionError('must not write')):
            with self.assertRaisesRegex(ValueError, 'prerequisites_missing'):
                setup.install({'files': {}})

    def test_wsl1_cannot_bypass_install_prerequisite_check(self):
        with patch.object(setup, 'scan', return_value={'install_supported': True, 'wsl2': False}), \
             patch.object(setup, 'private_directory', side_effect=AssertionError('must not write')):
            with self.assertRaisesRegex(ValueError, 'prerequisites_missing'):
                setup.install({'require_wsl2': True, 'files': {}})

    def test_entire_package_is_validated_before_directory_creation(self):
        files = {f'src/consolecrypt_runner/{number}.py': '' for number in range(10)}
        files.update({'requirements-linux-x86_64-py312.lock': '', 'src/consolecrypt_runner/__main__.py': ''})
        for path in ('../escape.py', '/absolute.py', 'src/consolecrypt_runner/../escape.py',
                     'src\\consolecrypt_runner\\bad.py', 'src/consolecrypt_runner/private.key'):
            with self.subTest(path=path), \
                 patch.object(setup, 'scan', return_value={'install_supported': True}), \
                 patch.object(setup, 'private_directory', side_effect=AssertionError('must not write')):
                with self.assertRaisesRegex(ValueError, 'invalid_package'):
                    setup.install({'files': {**files, path: ''}})

    def test_invalid_update_fingerprint_is_rejected_before_writing(self):
        files = {f'src/consolecrypt_runner/{number}.py': '' for number in range(10)}
        files.update({'requirements-linux-x86_64-py312.lock': '', 'src/consolecrypt_runner/__main__.py': ''})
        for value in ('bad', '../path', 'F' * 64, 123):
            with self.subTest(value=value), patch.object(setup, 'scan', return_value={'install_supported': True}), \
                 patch.object(setup, 'private_directory', side_effect=AssertionError('must not write')):
                with self.assertRaisesRegex(ValueError, 'invalid_package'):
                    setup.install({'files': files, 'package_fingerprint': value})

    def test_private_parent_rejects_symlinks_and_group_writable_directory(self):
        if setup.os.getuid() == 0:
            self.skipTest('must run as a non-root user')
        with tempfile.TemporaryDirectory() as temporary:
            home = Path(temporary)
            identity = SimpleNamespace(pw_dir=str(home))
            with patch.object(setup.pwd, 'getpwuid', return_value=identity):
                created = setup.private_directory(home / 'private' / 'runner')
                self.assertEqual(created.stat().st_mode & 0o777, 0o700)
                (home / 'link').symlink_to(home / 'private', target_is_directory=True)
                with self.assertRaisesRegex(ValueError, 'unsafe_directory'):
                    setup.private_directory(home / 'link' / 'other')
                (home / 'private').chmod(0o770)
                with self.assertRaisesRegex(ValueError, 'unsafe_directory'):
                    setup.private_directory(home / 'private' / 'other')


if __name__ == '__main__':
    unittest.main(verbosity=2)
