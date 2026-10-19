"""Deletion boundary tests use only owned temporary repositories and tiny files."""
import errno
import importlib.util
import json
import os
from pathlib import Path
import re
import stat
import subprocess
import sys
import tempfile
import textwrap
import unittest
from unittest.mock import patch

spec = importlib.util.spec_from_file_location('ci_cleanup', Path(__file__).with_name('cleanup-ci-workspace.py'))
cleanup = importlib.util.module_from_spec(spec)
spec.loader.exec_module(cleanup)


class CleanupTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.base = Path(self.temp.name).resolve()
        self.repo = self.base / 'workspace'
        self.make_repo(self.repo)
        self.env = {'GITHUB_ACTIONS': 'true', 'GITHUB_REPOSITORY': cleanup.REPOSITORY,
                    'GITHUB_WORKSPACE': str(self.repo), 'GITHUB_RUN_ID': '1234',
                    'GITHUB_RUN_ATTEMPT': '1', 'GITHUB_JOB': 'windows',
                    'RUNNER_TEMP': str(self.base / 'temp'),
                    'GITHUB_EVENT_NAME': 'push', 'GITHUB_REF': 'refs/tags/v0.5.1',
                    'GITHUB_SHA': cleanup.git(self.repo, 'rev-parse', 'HEAD')}

    def make_repo(self, root):
        root.mkdir(exist_ok=True)
        subprocess.run(['git', 'init', '-q', str(root)], check=True)
        subprocess.run(['git', '-C', str(root), 'remote', 'add', 'origin',
                        'https://github.com/' + cleanup.REPOSITORY], check=True)
        self.file(root / 'client/flutter/pubspec.yaml', 'name: consolecrypt')
        self.file(root / 'client/rust/Cargo.toml', '[workspace]')
        subprocess.run(['git', '-C', str(root), 'add', '.'], check=True)
        subprocess.run(['git', '-C', str(root), '-c', 'user.name=CI Fixture',
                        '-c', 'user.email=fixture@example.invalid', '-c', 'commit.gpgsign=false',
                        '-c', 'core.hooksPath=' + os.devnull, 'commit', '-qm', 'fixture'], check=True)

    def file(self, path, text='synthetic output'):
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(text)
        return path

    def validate(self, **kwargs):
        cleanup.validate_workspace(self.repo, self.env, kwargs.pop('script_root', self.repo), **kwargs)

    def test_only_generated_outputs_removed_and_repeated_cleanup_is_safe(self):
        outputs = [self.file(self.repo / relative / 'generated') for relative in cleanup.GENERATED]
        retained = [self.file(self.repo / relative, 'keep') for relative in
                    ('client/flutter/android/key.properties', '.local/receipt.json',
                     '.local/model.gguf', 'client/flutter/pubspec.lock', 'source.txt')]
        self.validate()
        for _ in range(2):
            for path in cleanup.workspace_targets(self.repo):
                cleanup.remove_owned(path)
        self.assertTrue(all(not path.exists() for path in outputs))
        self.assertTrue(all(path.read_text() == 'keep' for path in retained))

    def test_wrong_repo_workspace_and_missing_job_identity_fail_before_deletion(self):
        sentinel = self.file(self.repo / 'target/keep')
        for key, value in [('GITHUB_ACTIONS', 'false'), ('GITHUB_REPOSITORY', 'someone/other'),
                           ('GITHUB_WORKSPACE', str(self.base)), ('GITHUB_RUN_ID', '../1'),
                           ('GITHUB_JOB', '../windows')]:
            with self.subTest(key=key), patch.dict(self.env, {key: value}), self.assertRaises(ValueError):
                self.validate()
        subprocess.run(['git', '-C', str(self.repo), 'remote', 'set-url', 'origin',
                        'https://github.com/someone/other'], check=True)
        with self.assertRaises(ValueError):
            self.validate()
        self.assertTrue(sentinel.exists())

    def test_tracked_generated_path_refuses_entire_plan(self):
        source = self.file(self.repo / 'dist/actually-source.txt')
        self.file(self.repo / 'target/build.txt')
        subprocess.run(['git', '-C', str(self.repo), 'add', 'dist/actually-source.txt'], check=True)
        with self.assertRaises(ValueError):
            cleanup.workspace_targets(self.repo)
        self.assertTrue(source.exists())
        self.assertTrue((self.repo / 'target/build.txt').exists())

    def test_root_symlink_refused_nested_symlink_never_traversed(self):
        outside = self.base / 'outside'
        sentinel = self.file(outside / 'private', 'keep')
        target = self.repo / 'target'
        try:
            target.symlink_to(outside, target_is_directory=True)
        except OSError:
            self.skipTest('Creating a symlink requires Windows Developer Mode')
        with self.assertRaises(ValueError):
            cleanup.workspace_targets(self.repo)
        target.unlink()
        target.mkdir()
        (target / 'link').symlink_to(outside, target_is_directory=True)
        (target / 'broken').symlink_to(self.base / 'missing', target_is_directory=True)
        cleanup.remove_owned(target)
        self.assertEqual(sentinel.read_text(), 'keep')

    @unittest.skipUnless(os.name == 'nt', 'Windows junction boundary')
    def test_windows_nested_junction_keeps_its_target(self):
        outside = self.base / 'outside'
        sentinel = self.file(outside / 'private', 'keep')
        target = self.repo / 'target'
        target.mkdir()
        subprocess.run(['cmd', '/c', 'mklink', '/J', str(target / 'junction'), str(outside)],
                       check=True, capture_output=True)
        cleanup.remove_owned(target)
        self.assertEqual(sentinel.read_text(), 'keep')

    def test_reparse_flag_is_treated_as_a_link(self):
        class Junction:
            st_mode = stat.S_IFDIR
            st_file_attributes = 0x400
        self.assertTrue(cleanup.is_link(Junction()))

    def test_cache_reuses_failed_attempts_but_other_workspaces_are_preserved(self):
        root = cleanup.prepare_ai_cache(self.env)
        self.file(root / 'models/model.gguf')
        self.assertEqual(cleanup.prepare_ai_cache(self.env), root)
        other_env = {**self.env, 'GITHUB_RUN_ID': '5678'}
        self.assertEqual(cleanup.prepare_ai_cache(other_env), root)
        other_repo = self.base / 'other-workspace'
        self.make_repo(other_repo)
        other = cleanup.prepare_ai_cache({**other_env, 'GITHUB_WORKSPACE': str(other_repo)})
        sentinel = self.file(other / 'models/model.gguf', 'keep')
        for path in cleanup.owned_ai_cache(self.env):
            cleanup.remove_owned(path)
        self.assertFalse(root.exists())
        self.assertEqual(sentinel.read_text(), 'keep')

    def test_external_cache_cannot_be_claimed_or_deleted_without_exact_marker(self):
        root, _ = cleanup.cache_location(self.env)
        sentinel = self.file(root / 'models/model.gguf')
        with self.assertRaises(ValueError):
            cleanup.prepare_ai_cache(self.env)
        marker = self.file(root / cleanup.OWNER_FILE, json.dumps({'schema': 1, 'job': 'other'}))
        with self.assertRaises(ValueError):
            cleanup.owned_ai_cache(self.env)
        marker.unlink()
        self.assertTrue(sentinel.exists())

    def test_maintenance_uses_separate_main_tools_checkout(self):
        tools = self.repo / '.ci-maintenance-tools'
        self.make_repo(tools)
        with self.assertRaises(ValueError):
            self.validate(script_root=tools, maintenance=True)
        self.env.update(GITHUB_EVENT_NAME='workflow_dispatch', GITHUB_REF='refs/heads/main')
        self.validate(script_root=tools, maintenance=True)
        with self.assertRaises(ValueError):
            self.validate(script_root=tools)

    def test_legacy_cache_removes_known_artifacts_but_keeps_unknown_files_and_logs(self):
        root = self.base / 'legacy'
        model = self.file(root / 'models' / next(iter(cleanup.MODEL_DIGESTS)))
        chunk = self.file(root / 'downloads' / (cleanup.WINDOWS_RUNTIME_DIGEST + '.part'))
        kept = [self.file(root / name, 'keep') for name in
                ('models/other.gguf', 'downloads/unknown', 'selection.json', 'receipt.json', 'smoke.log')]
        runtime = root / 'runtime-b11739-windowsX64'
        self.file(runtime / 'llama-server.exe')
        self.file(runtime / 'verified.json', json.dumps({'archive': cleanup.WINDOWS_RUNTIME_DIGEST,
                  'executable': 'llama-server.exe', 'files': {'llama-server.exe': 'a' * 64}}))
        for path in cleanup.legacy_cache_targets(root):
            cleanup.remove_owned(path)
        self.assertFalse(model.exists())
        self.assertFalse(chunk.exists())
        self.assertFalse(runtime.exists())
        self.assertTrue(all(path.read_text() == 'keep' for path in kept))

    def test_legacy_unknown_runtime_content_and_manifest_traversal_refuse_cleanup(self):
        root = self.base / 'legacy'
        runtime = root / 'runtime-b11739-windowsX64'
        self.file(runtime / 'llama-server.exe')
        receipt = {'archive': cleanup.WINDOWS_RUNTIME_DIGEST, 'executable': 'llama-server.exe',
                   'files': {'llama-server.exe': 'a' * 64}}
        marker = self.file(runtime / 'verified.json', json.dumps(receipt))
        unknown = self.file(runtime / 'user.txt', 'keep')
        with self.assertRaises(ValueError):
            cleanup.legacy_cache_targets(root)
        unknown.unlink()
        receipt['files']['../outside'] = 'b' * 64
        marker.write_text(json.dumps(receipt))
        with self.assertRaises(ValueError):
            cleanup.legacy_cache_targets(root)
        self.assertTrue((runtime / 'llama-server.exe').exists())

    def test_readonly_generated_file_can_be_removed(self):
        file = self.file(self.repo / 'target/readonly')
        file.chmod(stat.S_IREAD)
        cleanup.remove_owned(file.parent)
        self.assertFalse(file.exists())

    def test_cleanup_retries_a_late_build_write_without_touching_other_data(self):
        target = self.file(self.repo / 'client/flutter/build/first').parent
        kept = self.file(self.repo / '.local/receipt.json', 'keep')
        rmdir = os.rmdir
        writes = []

        def late_write(name, *args, **kwargs):
            if Path(name).name == target.name and not writes:
                writes.append(self.file(target / 'late-compiler-output'))
            return rmdir(name, *args, **kwargs)

        with patch.object(cleanup.os, 'rmdir', late_write), patch.object(cleanup.time, 'sleep') as sleep:
            cleanup.remove_owned(target)
        self.assertEqual(sleep.call_count, 1)
        self.assertFalse(target.exists())
        self.assertEqual(kept.read_text(), 'keep')

    def test_cleanup_tolerates_a_directory_removed_by_another_cleaner(self):
        target = self.file(self.repo / 'client/flutter/build/first').parent
        rmdir = os.rmdir
        raced = []

        def competing_remove(name, *args, **kwargs):
            if Path(name).name == target.name and not raced:
                raced.append(True)
                rmdir(name, *args, **kwargs)
            return rmdir(name, *args, **kwargs)

        with patch.object(cleanup.os, 'rmdir', competing_remove), patch.object(cleanup.time, 'sleep'):
            cleanup.remove_owned(target)
        self.assertTrue(raced)
        self.assertFalse(target.exists())

    def test_cleanup_stops_after_bounded_retries_for_a_persistent_writer(self):
        target = self.file(self.repo / 'client/flutter/build/first').parent
        rmdir = os.rmdir
        writes = []

        def continuing_write(name, *args, **kwargs):
            if Path(name).name == target.name:
                writes.append(self.file(target / 'late-compiler-output'))
            return rmdir(name, *args, **kwargs)

        with patch.object(cleanup.os, 'rmdir', continuing_write), patch.object(cleanup.time, 'sleep') as sleep:
            with self.assertRaises(OSError) as raised:
                cleanup.remove_owned(target)
        self.assertEqual(raised.exception.errno, errno.ENOTEMPTY)
        self.assertEqual(len(writes), len(cleanup.REMOVE_RETRY_DELAYS) + 1)
        self.assertEqual(sleep.call_count, len(cleanup.REMOVE_RETRY_DELAYS))
        self.assertTrue(target.exists())

    def test_cleanup_retry_rechecks_link_boundary_and_redacts_error_paths(self):
        target = self.file(self.repo / 'client/flutter/build/first').parent
        outside = self.base / 'private-models'
        kept = self.file(outside / 'never-delete.gguf', 'keep')
        error = OSError(errno.ENOTEMPTY, 'private detail', str(kept))

        def swap_to_link(_):
            target.rename(target.with_name('retained-test-build'))
            try:
                target.symlink_to(outside, target_is_directory=True)
            except OSError:
                self.skipTest('Creating a symlink requires Windows Developer Mode')

        with patch.object(cleanup.shutil, 'rmtree', side_effect=error), patch.object(cleanup.time, 'sleep', swap_to_link):
            with self.assertRaises(ValueError):
                cleanup.remove_owned(target)
        self.assertEqual(kept.read_text(), 'keep')
        diagnostic = cleanup.error_code(error)
        self.assertEqual(diagnostic, 'OSError, errno=ENOTEMPTY')
        self.assertNotIn('private', diagnostic)

    def test_cleanup_does_not_retry_unrelated_io_errors(self):
        target = self.file(self.repo / 'client/flutter/build/first').parent
        with patch.object(cleanup.shutil, 'rmtree', side_effect=OSError(errno.EIO, 'private detail')):
            with patch.object(cleanup.time, 'sleep') as sleep, self.assertRaises(OSError):
                cleanup.remove_owned(target)
        sleep.assert_not_called()
        self.assertTrue(target.exists())

    def test_cli_legacy_flag_cannot_escape_normal_job_or_delete_outputs(self):
        script = self.repo / 'client/scripts/cleanup-ci-workspace.py'
        self.file(script, Path(cleanup.__file__).read_text())
        sentinel = self.file(self.repo / 'target/keep')
        result = subprocess.run([sys.executable, str(script), '--workspace', str(self.repo),
                                 '--legacy-windows-ai-cache'],
                                env={**os.environ, **self.env}, capture_output=True, text=True)
        self.assertNotEqual(result.returncode, 0)
        self.assertTrue(sentinel.exists())

    def test_cli_removes_owned_cache_and_outputs_after_preflight(self):
        script = self.repo / 'client/scripts/cleanup-ci-workspace.py'
        self.file(script, Path(cleanup.__file__).read_text())
        output = self.file(self.repo / 'dist/package.zip')
        cache = cleanup.prepare_ai_cache(self.env)
        self.file(cache / 'models/model.gguf')
        receipt = self.file(self.repo / '.local/receipt.json', 'keep')
        cleanup.claim_workspace(self.repo, self.env)
        published = {**self.env, 'GITHUB_JOB': 'cleanup_windows', 'CC_RELEASE_PUBLISHED': 'true'}
        result = subprocess.run([sys.executable, str(script), '--workspace', str(self.repo), '--after-publication'],
                                env={**os.environ, **published}, capture_output=True, text=True)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn('CI_CLEANUP_PASS', result.stdout)
        self.assertFalse(output.exists())
        self.assertFalse(cache.exists())
        self.assertEqual(receipt.read_text(), 'keep')

    def test_failed_build_and_diagnostic_keep_outputs_without_publication_proof(self):
        script = self.repo / 'client/scripts/cleanup-ci-workspace.py'
        self.file(script, Path(cleanup.__file__).read_text())
        output = self.file(self.repo / 'target/intermediate')
        cleanup.claim_workspace(self.repo, self.env)
        base = {**self.env, 'GITHUB_JOB': 'cleanup_windows', 'CC_RELEASE_PUBLISHED': 'true'}
        for change in ({'CC_RELEASE_PUBLISHED': 'false'}, {'GITHUB_REF': 'refs/tags/check-windows-0.5.1-1'},
                       {'GITHUB_JOB': 'windows'}, {'GITHUB_RUN_ID': '5678'}, {'GITHUB_SHA': '0' * 40}):
            with self.subTest(change=change):
                result = subprocess.run([sys.executable, str(script), '--workspace', str(self.repo), '--after-publication'],
                                        env={**os.environ, **base, **change}, capture_output=True)
                self.assertNotEqual(result.returncode, 0)
                self.assertTrue(output.exists())
        result = subprocess.run([sys.executable, str(script), '--workspace', str(self.repo)],
                                env={**os.environ, **base}, capture_output=True)
        self.assertNotEqual(result.returncode, 0)
        self.assertTrue(output.exists())

    def test_registration_rejects_wrong_source_and_unknown_marker(self):
        with self.assertRaises(ValueError):
            cleanup.claim_workspace(self.repo, {**self.env, 'GITHUB_SHA': '0' * 40})
        marker = self.file(self.repo / cleanup.WORKSPACE_OWNER_FILE, 'unknown local data')
        with self.assertRaises(ValueError):
            cleanup.claim_workspace(self.repo, self.env)
        self.assertEqual(marker.read_text(), 'unknown local data')

    def test_maintenance_keeps_outputs_until_matching_release_is_verified(self):
        tools = self.repo / '.ci-maintenance-tools'
        self.make_repo(tools)
        script = tools / 'client/scripts/cleanup-ci-workspace.py'
        self.file(script, Path(cleanup.__file__).read_text())
        output = self.file(self.repo / 'target/intermediate')
        env = {**self.env, 'GITHUB_EVENT_NAME': 'workflow_dispatch', 'GITHUB_REF': 'refs/heads/main',
               'CC_RELEASE_PUBLISHED': 'true', 'CC_PUBLISHED_TAG': 'v0.5.1',
               'CC_PUBLISHED_COMMIT': self.env['GITHUB_SHA']}
        for change in ({'CC_RELEASE_PUBLISHED': 'false'}, {'CC_PUBLISHED_TAG': 'draft'},
                       {'CC_PUBLISHED_COMMIT': '0' * 40}):
            with self.subTest(change=change):
                result = subprocess.run([sys.executable, str(script), '--workspace', str(self.repo), '--maintenance'],
                                        env={**os.environ, **env, **change}, capture_output=True)
                self.assertNotEqual(result.returncode, 0)
                self.assertTrue(output.exists())
        result = subprocess.run([sys.executable, str(script), '--workspace', str(self.repo), '--maintenance'],
                                env={**os.environ, **env}, capture_output=True)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertFalse(output.exists())

    def test_published_retry_accepts_prior_successful_attempt_but_not_newer_owner(self):
        cleanup.claim_workspace(self.repo, self.env)
        published = {**self.env, 'GITHUB_JOB': 'cleanup_windows', 'CC_RELEASE_PUBLISHED': 'true',
                     'GITHUB_RUN_ATTEMPT': '2'}
        cleanup.verify_published_workspace(self.repo, published)
        cleanup.claim_workspace(self.repo, {**self.env, 'GITHUB_RUN_ATTEMPT': '3'})
        with self.assertRaises(ValueError):
            cleanup.verify_published_workspace(self.repo, published)

    def inline_preflights(self):
        # Execute the actual inline guards, which run before trusted tools exist.
        workflow = (Path(__file__).resolve().parents[2] / '.github/workflows/runner-cleanup.yml').read_text()
        payloads = re.findall(r'^[ \t]*# BEGIN_MAINTENANCE_PREFLIGHT\n(.*?)^[ \t]*# END_MAINTENANCE_PREFLIGHT',
                              workflow, flags=re.MULTILINE | re.DOTALL)
        self.assertEqual(len(payloads), 2)
        return [textwrap.dedent(payload) for payload in payloads]

    def run_inline(self, payload):
        return subprocess.run([sys.executable, '-c', payload], env={**os.environ, **self.env},
                              capture_output=True, text=True)

    def test_inline_precheckout_guards_accept_only_absent_empty_or_expected_repository(self):
        tools = self.repo / '.ci-maintenance-tools'
        for payload in self.inline_preflights():
            self.assertEqual(self.run_inline(payload).returncode, 0)
        tools.mkdir()
        for payload in self.inline_preflights():
            self.assertEqual(self.run_inline(payload).returncode, 0)
        unknown = self.file(tools / 'user-data.txt', 'keep')
        for payload in self.inline_preflights():
            self.assertNotEqual(self.run_inline(payload).returncode, 0)
            self.assertEqual(unknown.read_text(), 'keep')
        unknown.unlink()
        self.make_repo(tools)
        for payload in self.inline_preflights():
            self.assertEqual(self.run_inline(payload).returncode, 0)
        unknown = self.file(tools / 'untracked-personal.txt', 'keep')
        for payload in self.inline_preflights():
            self.assertNotEqual(self.run_inline(payload).returncode, 0)
            self.assertEqual(unknown.read_text(), 'keep')
        unknown.unlink()
        subprocess.run(['git', '-C', str(tools), 'remote', 'set-url', 'origin',
                        'https://github.com/someone/other'], check=True)
        for payload in self.inline_preflights():
            self.assertNotEqual(self.run_inline(payload).returncode, 0)
            self.assertTrue((tools / '.git').is_dir())

    def test_inline_precheckout_guards_reject_workspace_origin_and_tools_symlink(self):
        outside = self.base / 'outside'
        sentinel = self.file(outside / 'keep', 'private fixture')
        tools = self.repo / '.ci-maintenance-tools'
        try:
            tools.symlink_to(outside, target_is_directory=True)
        except OSError:
            self.skipTest('Creating a symlink requires Windows Developer Mode')
        for payload in self.inline_preflights():
            self.assertNotEqual(self.run_inline(payload).returncode, 0)
            self.assertEqual(sentinel.read_text(), 'private fixture')
        tools.unlink()
        subprocess.run(['git', '-C', str(self.repo), 'remote', 'set-url', 'origin',
                        'https://github.com/someone/other'], check=True)
        for payload in self.inline_preflights():
            self.assertNotEqual(self.run_inline(payload).returncode, 0)

    @unittest.skipUnless(os.name == 'nt', 'Windows pre-checkout junction boundary')
    def test_inline_precheckout_guards_reject_tools_junction(self):
        outside = self.base / 'outside'
        sentinel = self.file(outside / 'keep', 'private fixture')
        tools = self.repo / '.ci-maintenance-tools'
        subprocess.run(['cmd', '/c', 'mklink', '/J', str(tools), str(outside)],
                       check=True, capture_output=True)
        for payload in self.inline_preflights():
            self.assertNotEqual(self.run_inline(payload).returncode, 0)
            self.assertEqual(sentinel.read_text(), 'private fixture')


if __name__ == '__main__':
    unittest.main()
