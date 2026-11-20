"""Owned temporary non-root Linux filesystem fixtures, no user environments."""
import importlib.util
import json
import os
from pathlib import Path
import select
import shutil
import subprocess
import sys
import tempfile
from types import SimpleNamespace
import unittest
from unittest.mock import patch
import uuid

SCRIPT = Path(__file__).with_name('environment_cleanup.py')
spec = importlib.util.spec_from_file_location('cc_cleanup', SCRIPT)
cleanup = importlib.util.module_from_spec(spec)
spec.loader.exec_module(cleanup)


class Control:
    def __init__(self):
        self.read, self.write = os.pipe()
        self.control = cleanup._CleanupControl(self.read)

    def check(self):
        self.control.check()

    def close(self):
        os.close(self.read)
        os.close(self.write)


@unittest.skipUnless(sys.platform == 'linux' and os.getuid() != 0, 'requires owned non-root Linux fixture')
class CleanupTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.home = Path(self.temporary.name)
        self.parent = self.home / '.local/share/consolecrypt/runner-environments'
        self.parent.mkdir(parents=True, mode=0o700)
        # mkdir(parents) uses the process umask for intermediate directories;
        # they need only be owned and not group/world writable.
        self.identity = patch.object(cleanup.pwd, 'getpwuid', return_value=SimpleNamespace(
            pw_dir=str(self.home), pw_name='fixture_runner'))
        self.identity.start()
        self.addCleanup(self.identity.stop)
        self.control = Control()
        self.addCleanup(self.control.close)

    def environment(self):
        root = self.parent / str(uuid.uuid4())
        root.mkdir(mode=0o700)
        (root / 'venv/bin').mkdir(parents=True, mode=0o700)
        (root / 'src').mkdir(mode=0o775)
        (root / 'src').chmod(0o775)  # Legitimate venv/wheel group-write mode.
        (root / 'state').mkdir(mode=0o700)
        (root / 'venv/bin/python').symlink_to('/usr/bin/python3')
        (root / 'src/public_module.py').write_text('# never executed\n', encoding='utf-8')
        (root / 'state/owned-state').write_text('owned temporary fixture state', encoding='utf-8')
        paths = {'python': str(root / 'venv/bin/python'), 'module_dir': str(root / 'src'),
                 'root': str(root / 'state')}
        (root / 'installation.json').write_text(json.dumps({
            'version': 1, 'paths': paths, 'package_sha256': '0' * 64,
            'package_fingerprint': '1' * 64}), encoding='utf-8')
        return root

    def plan(self, *roots):
        report = cleanup.cleanup_scan()
        wanted = {root.name for root in roots}
        selected = [row for row in report['candidates'] if row['id'] in wanted]
        self.assertEqual(len(selected), len(wanted))
        self.assertTrue(all(row['eligible'] for row in selected), selected)
        return {'action': 'cleanup_delete', 'identity': report['identity'],
                'protected_paths': [], 'selected_python': None,
                'candidates': [{'id': row['id'], 'revision': row['revision']} for row in selected]}

    def test_exact_owned_uuid_removed_and_other_environment_and_symlink_target_preserved(self):
        root, other = self.environment(), self.environment()
        outside = self.home / 'outside-sentinel'
        outside.write_text('must survive', encoding='utf-8')
        (root / 'src/external-link').symlink_to(outside)
        report = cleanup.cleanup_scan()
        self.assertFalse(report['other_profiles_checked'])
        self.assertTrue(all(row['profile_ownership'] == 'unknown' for row in report['candidates']))
        payload = self.plan(root)
        result = cleanup.cleanup_delete(payload, self.control)
        self.assertTrue(result['removed'][0]['removed'])
        self.assertFalse(root.exists())
        self.assertTrue((other / 'state/owned-state').is_file())
        self.assertEqual(outside.read_text(), 'must survive')

    def test_absent_managed_parent_scan_does_not_create_it(self):
        shutil.rmtree(self.parent)
        report = cleanup.cleanup_scan()
        self.assertIsNone(report['identity'])
        self.assertEqual(report['candidates'], [])
        self.assertFalse(self.parent.exists())

    def test_manual_directory_and_noncanonical_or_arbitrary_id_never_select_a_path(self):
        root = self.environment()
        manual = self.parent / 'manual-python'
        manual.mkdir()
        (manual / 'must-survive').write_text('manual', encoding='utf-8')
        self.assertEqual(len(cleanup.cleanup_scan()['candidates']), 1)
        payload = self.plan(root)
        for identifier in ('../manual-python', str(manual), root.name.upper(), '00000000-0000-0000-0000-000000000000'):
            with self.subTest(identifier=identifier):
                payload['candidates'][0]['id'] = identifier
                with self.assertRaises(ValueError):
                    cleanup.cleanup_delete(payload, self.control)
        self.assertTrue(root.exists())
        self.assertEqual((manual / 'must-survive').read_text(), 'manual')

    def test_receipt_and_tree_mutation_after_review_are_rejected_before_quarantine(self):
        for mutation in ('receipt', 'tree'):
            root = self.environment()
            payload = self.plan(root)
            if mutation == 'receipt':
                marker = root / 'installation.json'
                marker.write_text(marker.read_text() + ' ', encoding='utf-8')
            else:
                (root / 'state/new-file').write_text('later change', encoding='utf-8')
            with self.assertRaisesRegex(ValueError, 'cleanup_candidate_changed'):
                cleanup.cleanup_delete(payload, self.control)
            self.assertTrue(root.exists())
        self.assertFalse(any(path.name.startswith('.cleanup-') for path in self.parent.iterdir()))

    def test_all_batch_preflight_passes_before_first_directory_is_renamed(self):
        one, two = self.environment(), self.environment()
        payload = self.plan(one, two)
        payload['candidates'][1]['revision'] = 'f' * 64
        with self.assertRaisesRegex(ValueError, 'cleanup_candidate_changed'):
            cleanup.cleanup_delete(payload, self.control)
        self.assertTrue(one.exists() and two.exists())

    def test_identity_uid_and_private_boundaries_are_checked_again(self):
        root = self.environment()
        payload = self.plan(root)
        payload['identity']['inode'] += 1
        with self.assertRaisesRegex(ValueError, 'cleanup_identity_changed'):
            cleanup.cleanup_delete(payload, self.control)
        root.chmod(0o775)
        row = cleanup.cleanup_scan()['candidates'][0]
        self.assertFalse(row['eligible'])
        self.assertEqual(row['blocked_reason'], 'cleanup_unsafe_owner_mode')
        root.chmod(0o700)
        self.parent.chmod(0o775)
        with self.assertRaisesRegex(ValueError, 'cleanup_unsafe_owner_mode'):
            cleanup.cleanup_scan()

    def test_symlink_environment_marker_and_ancestor_are_not_followed(self):
        root = self.environment()
        original = root / 'installation.json'
        outside = self.home / 'marker-outside'
        original.rename(outside)
        original.symlink_to(outside)
        self.assertFalse(cleanup.cleanup_scan()['candidates'][0]['eligible'])
        root_link = self.parent / str(uuid.uuid4())
        root_link.symlink_to(root, target_is_directory=True)
        self.assertTrue(all(not row['eligible'] for row in cleanup.cleanup_scan()['candidates']))
        self.assertTrue(outside.exists())
        actual = self.home / '.local/share/consolecrypt'
        renamed = self.home / 'moved-consolecrypt'
        actual.rename(renamed)
        actual.symlink_to(renamed, target_is_directory=True)
        with self.assertRaises(ValueError):
            cleanup.cleanup_scan()

    def test_receipt_unknown_fields_versions_paths_and_duplicate_keys_are_ineligible(self):
        root = self.environment()
        marker = root / 'installation.json'
        valid = marker.read_text()
        variants = [json.loads(valid) for _ in range(3)]
        variants[0]['future'] = True
        variants[1]['version'] = True
        variants[2]['paths']['root'] = '/unapproved'
        for raw in [json.dumps(value) for value in variants] + [valid.replace('"version": 1', '"version": 1, "version": 1')]:
            marker.write_text(raw, encoding='utf-8')
            row = cleanup.cleanup_scan()['candidates'][0]
            self.assertFalse(row['eligible'])
            self.assertEqual(row['blocked_reason'], 'cleanup_invalid_receipt')

    def test_no_xdev_is_enforced_by_actual_kernel(self):
        root = os.open('/', os.O_RDONLY | os.O_DIRECTORY)
        try:
            with self.assertRaisesRegex(ValueError, 'cleanup_mount_boundary'):
                cleanup._cleanup_open(root, 'proc')
        finally:
            os.close(root)

    def test_actual_venv_interpreter_cannot_delete_its_own_environment(self):
        root = self.environment()
        # Use a real venv prefix; no pip/download or executable fixture code.
        shutil.rmtree(root / 'venv')
        subprocess.run([sys.executable, '-I', '-m', 'venv', '--copies', '--without-pip', str(root / 'venv')], check=True, timeout=30)
        code = f'''import importlib.util
from types import SimpleNamespace
s=importlib.util.spec_from_file_location('cc_cleanup',{str(SCRIPT)!r});m=importlib.util.module_from_spec(s);s.loader.exec_module(m)
m.pwd.getpwuid=lambda uid:SimpleNamespace(pw_dir={str(self.home)!r},pw_name='fixture_runner')
r=m.cleanup_scan();assert r['candidates'][0]['eligible'] is False,r
assert r['candidates'][0]['blocked_reason']=='cleanup_current_interpreter',r
'''
        result = subprocess.run([str(root / 'venv/bin/python'), '-I', '-c', code], capture_output=True, timeout=10)
        self.assertEqual(result.returncode, 0, result.stderr.decode(errors='replace')[-1500:])
        self.assertTrue((root / 'state/owned-state').is_file())

    def test_same_filesystem_bind_mount_is_rejected_in_own_unprivileged_namespace(self):
        if not Path('/usr/bin/bwrap').exists():
            self.skipTest('own fixture needs bubblewrap for same-filesystem mount acceptance')
        root = self.environment()
        source, target = self.home / 'owned-bind-source', root / 'src/mounted'
        source.mkdir()
        target.mkdir()
        (source / 'outside-must-survive').write_text('own mount sentinel', encoding='utf-8')
        code = f'''import importlib.util,os
from types import SimpleNamespace
s=importlib.util.spec_from_file_location('cc_cleanup',{str(SCRIPT)!r});m=importlib.util.module_from_spec(s);s.loader.exec_module(m)
m.pwd.getpwuid=lambda uid:SimpleNamespace(pw_dir={str(self.home)!r},pw_name='fixture_runner')
assert os.stat({str(source)!r}).st_dev==os.stat({str(target)!r}).st_dev
r=m.cleanup_scan();assert r['candidates'][0]['blocked_reason']=='cleanup_mount_boundary',r
'''
        command = ['/usr/bin/bwrap', '--unshare-all', '--die-with-parent', '--ro-bind', '/', '/',
                   '--ro-bind', str(source), str(target), '--proc', '/proc', '--dev', '/dev',
                   '--', sys.executable, '-I', '-c', code]
        result = subprocess.run(command, capture_output=True, timeout=15)
        self.assertEqual(result.returncode, 0, result.stderr.decode(errors='replace')[-1500:])
        self.assertEqual((source / 'outside-must-survive').read_text(), 'own mount sentinel')

    def test_quarantine_inode_replacement_never_deletes_wrong_tree(self):
        root = self.environment()
        payload = self.plan(root)
        rename = cleanup._cleanup_rename
        def replace(parent, source, target):
            rename(parent, source, target)
            quarantine = self.parent / target
            quarantine.rename(self.parent / 'preserved-original')
            shutil.copytree(self.parent / 'preserved-original', quarantine, symlinks=True)
            quarantine.chmod(0o700)
        with patch.object(cleanup, '_cleanup_rename', side_effect=replace):
            with self.assertRaisesRegex(ValueError, 'cleanup_quarantine_changed'):
                cleanup.cleanup_delete(payload, self.control)
        self.assertTrue((self.parent / 'preserved-original/state/owned-state').exists())
        self.assertEqual(len(list(self.parent.glob('.cleanup-*/state/owned-state'))), 1)

    def test_real_eof_lease_cancels_blocked_mid_delete_and_quarantine_requires_new_review(self):
        root = self.environment()
        payload = self.plan(root)
        code = f'''import importlib.util,json,os,sys
from types import SimpleNamespace
s=importlib.util.spec_from_file_location('cc_cleanup',{str(SCRIPT)!r});m=importlib.util.module_from_spec(s);s.loader.exec_module(m)
m.pwd.getpwuid=lambda uid:SimpleNamespace(pw_dir={str(self.home)!r},pw_name='fixture_runner')
p=json.loads(sys.stdin.buffer.readline(65537));control=m._CleanupControl(sys.stdin.fileno());original=m._cleanup_delete_tree
def blocked(directory,uid,budget,control,depth=0):
 if depth==0:
  print(json.dumps({{'phase':'quarantined'}}),flush=True)
  assert os.read(sys.stdin.fileno(),1)==b''
 return original(directory,uid,budget,control,depth)
m._cleanup_delete_tree=blocked
try:m.cleanup_delete(p,control);raise AssertionError('EOF authorized delete')
except ValueError as error:print(json.dumps({{'error':str(error)}}),flush=True)
'''
        process = subprocess.Popen([sys.executable, '-I', '-c', code], stdin=subprocess.PIPE, stdout=subprocess.PIPE)
        try:
            process.stdin.write(json.dumps(payload).encode() + b'\n')
            process.stdin.flush()
            self.assertTrue(select.select([process.stdout], [], [], 8)[0], 'fixture did not quarantine')
            self.assertEqual(json.loads(process.stdout.readline()), {'phase': 'quarantined'})
            self.assertFalse(root.exists())
            process.stdin.close()
            self.assertTrue(select.select([process.stdout], [], [], 5)[0], 'EOF did not cancel')
            self.assertEqual(json.loads(process.stdout.readline()), {'error': 'cleanup_cancelled'})
            self.assertEqual(process.wait(timeout=5), 0)
        finally:
            if process.poll() is None:
                process.kill()
                process.wait()
            process.stdout.close()
        fresh = cleanup.cleanup_scan()
        row = fresh['candidates'][0]
        self.assertTrue(row['quarantined'] and row['eligible'])
        self.assertTrue((Path(row['path']) / 'state/owned-state').is_file())
        with self.assertRaisesRegex(ValueError, 'cleanup_candidate_changed'):
            cleanup.cleanup_delete(payload, self.control)  # old ID/revision cannot retry
        refreshed = {'action': 'cleanup_delete', 'identity': fresh['identity'],
                     'protected_paths': [], 'selected_python': None,
                     'candidates': [{'id': row['id'], 'revision': row['revision']}]}
        cleanup.cleanup_delete(refreshed, self.control)
        self.assertFalse(Path(row['path']).exists())

    def test_budget_and_unexpected_live_control_bytes_fail_without_deletion(self):
        root = self.environment()
        with patch.object(cleanup, 'CC_CLEANUP_MAX_NODES', 2):
            row = cleanup.cleanup_scan()['candidates'][0]
        self.assertFalse(row['eligible'])
        self.assertEqual(row['blocked_reason'], 'cleanup_budget')
        payload = self.plan(root)
        os.write(self.control.write, b'unexpected')
        with self.assertRaisesRegex(ValueError, 'cleanup_cancelled'):
            cleanup.cleanup_delete(payload, self.control)
        self.assertTrue(root.exists())

    def test_saved_or_selected_alias_through_venv_system_symlink_is_deny_only(self):
        root = self.environment()
        alias = self.home / 'manual-python-alias'
        alias.symlink_to(root / 'venv/bin/python')
        self.assertNotIn(str(root), os.path.realpath(alias))  # final target alone is insufficient
        for selected in (False, True):
            protected = [] if selected else [str(alias)]
            selected_path = str(alias) if selected else None
            row = cleanup.cleanup_scan(protected, selected_path)['candidates'][0]
            self.assertFalse(row['eligible'])
            self.assertEqual(row['blocked_reason'], 'cleanup_selected' if selected else 'cleanup_registered')
            payload = self.plan(root)
            payload['protected_paths'] = protected
            payload['selected_python'] = selected_path
            with self.assertRaisesRegex(ValueError, row['blocked_reason']):
                cleanup.cleanup_delete(payload, self.control)
        self.assertTrue((root / 'state/owned-state').exists())


if __name__ == '__main__':
    unittest.main(verbosity=2)
