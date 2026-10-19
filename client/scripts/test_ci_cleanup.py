"""Deletion boundary tests use only owned temporary repositories and tiny files."""
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
                    'GITHUB_EVENT_NAME': 'push', 'GITHUB_REF': 'refs/tags/v0.5.1'}

    def make_repo(self, root):
        root.mkdir(exist_ok=True)
        subprocess.run(['git', 'init', '-q', str(root)], check=True)
        subprocess.run(['git', '-C', str(root), 'remote', 'add', 'origin',
                        'https://github.com/' + cleanup.REPOSITORY], check=True)
        self.file(root / 'client/flutter/pubspec.yaml', 'name: consolecrypt')
        self.file(root / 'client/rust/Cargo.toml', '[workspace]')

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

    def test_current_job_cache_is_marked_and_other_jobs_are_preserved(self):
        root = cleanup.prepare_ai_cache(self.env)
        self.file(root / 'models/model.gguf')
        self.assertEqual(cleanup.prepare_ai_cache(self.env), root)
        other_env = {**self.env, 'GITHUB_RUN_ID': '5678'}
        other = cleanup.prepare_ai_cache(other_env)
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
        result = subprocess.run([sys.executable, str(script), '--workspace', str(self.repo)],
                                env={**os.environ, **self.env}, capture_output=True, text=True)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn('CI_CLEANUP_PASS', result.stdout)
        self.assertFalse(output.exists())
        self.assertFalse(cache.exists())
        self.assertEqual(receipt.read_text(), 'keep')

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
        subprocess.run(['git', '-C', str(tools), 'add', '.'], check=True)
        subprocess.run(['git', '-C', str(tools), '-c', 'core.hooksPath=' + os.devnull,
                        '-c', 'commit.gpgsign=false', '-c', 'user.name=CI Fixture',
                        '-c', 'user.email=fixture@example.invalid',
                        'commit', '-qm', 'Owned synthetic tools'], check=True)
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
