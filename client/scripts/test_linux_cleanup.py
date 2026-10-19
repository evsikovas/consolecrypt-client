"""Linux cancellation ownership tests; synthetic dirs and a fake Docker only."""

import importlib.util
import json
import os
from pathlib import Path
import subprocess
import tempfile
import unittest
from unittest.mock import patch


spec = importlib.util.spec_from_file_location('linux_cleanup', Path(__file__).with_name('cleanup-linux-job.py'))
linux = importlib.util.module_from_spec(spec)
spec.loader.exec_module(linux)


class LinuxCleanupTest(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        self.base = Path(temporary.name).resolve()
        self.workspace = self.base / 'workspace'
        self.workspace.mkdir()
        self.runner_temp = self.base / 'runner-temp'
        self.runner_temp.mkdir()
        self.env = {'GITHUB_ACTIONS': 'true', 'GITHUB_REPOSITORY': linux.cleanup.REPOSITORY,
                    'GITHUB_RUN_ID': '321', 'GITHUB_RUN_ATTEMPT': '2', 'GITHUB_JOB': 'linux',
                    'RUNNER_TEMP': str(self.runner_temp)}
        self.root = linux.prepare(self.workspace, self.env)
        _, self.owner = linux.ownership(self.workspace, self.env)
        (self.root / 'source').mkdir()
        (self.root / 'source/generated').write_text('synthetic build')
        self.container_id = 'a' * 64
        self.inspection = [self.container_id, '/' + self.owner['container'], linux.IMAGE,
                           linux.labels(self.owner), [{'Type': 'bind', 'Source': str(self.root / 'source'),
                           'Destination': '/work', 'RW': True}]]
        self.calls = []

    def docker(self, *arguments):
        self.calls.append(arguments)
        if arguments[:2] == ('container', 'ls'):
            return self.container_id or ''
        if arguments[:2] == ('container', 'inspect'):
            return json.dumps(self.inspection)
        if arguments[:2] == ('container', 'rm'):
            self.assertEqual(arguments, ('container', 'rm', '--force', self.container_id))
            self.container_id = None
            return ''
        raise AssertionError('Unexpected Docker mutation')

    def test_exact_owned_builder_and_scratch_removed_after_shell_has_gone(self):
        other = self.runner_temp / 'other-job'
        other.mkdir()
        kept = other / 'model.gguf'
        kept.write_text('keep')
        # Cleanup has no in-process state from the producer; only the persisted
        # receipt and a fresh Docker inspect authorize the immutable ID.
        with patch.object(linux, 'docker', self.docker):
            linux.finish(self.workspace, self.env)
            linux.finish(self.workspace, self.env)
        self.assertFalse(self.root.exists())
        self.assertEqual(kept.read_text(), 'keep')
        self.assertEqual(sum(call[:2] == ('container', 'rm') for call in self.calls), 1)

    def test_wrong_labels_image_name_id_or_mount_never_remove_anything(self):
        original = json.loads(json.dumps(self.inspection))
        variants = []
        for key in linux.labels(self.owner):
            changed = json.loads(json.dumps(original))
            changed[3][key] = 'other-job'
            variants.append(changed)
        for index, value in [(0, 'b' * 64), (1, '/other'), (2, 'foreign-image')]:
            changed = json.loads(json.dumps(original))
            changed[index] = value
            variants.append(changed)
        for key, value in [('Type', 'volume'), ('Source', str(self.base)), ('Destination', '/other'), ('RW', False)]:
            changed = json.loads(json.dumps(original))
            changed[4][0][key] = value
            variants.append(changed)
        variants.append(original[:4] + [original[4] * 2])
        for index, inspection in enumerate(variants):
            with self.subTest(index=index), patch.object(linux, 'docker', self.docker):
                self.inspection = inspection
                with self.assertRaises(ValueError):
                    linux.finish(self.workspace, self.env)
            self.assertTrue((self.root / 'source/generated').exists())
            self.assertFalse(any(call[:2] == ('container', 'rm') for call in self.calls))

    def test_tampered_or_missing_receipt_refuses_existing_builder(self):
        marker = self.root / linux.OWNER_FILE
        marker.write_text(json.dumps({**self.owner, 'run': '999'}))
        with patch.object(linux, 'docker', self.docker), self.assertRaises(ValueError):
            linux.finish(self.workspace, self.env)
        marker.unlink()
        with patch.object(linux, 'docker', self.docker), self.assertRaises(FileNotFoundError):
            linux.finish(self.workspace, self.env)
        self.assertTrue((self.root / 'source/generated').exists())
        self.assertFalse(any(call[:2] == ('container', 'rm') for call in self.calls))

    def test_already_removed_container_still_cleans_owned_scratch(self):
        self.container_id = None
        with patch.object(linux, 'docker', self.docker):
            linux.finish(self.workspace, self.env)
        self.assertFalse(self.root.exists())
        self.assertFalse(any(call[:2] == ('container', 'rm') for call in self.calls))

    def test_successful_remove_never_deletes_scratch_reused_by_a_new_container(self):
        def replacement(*arguments):
            result = self.docker(*arguments)
            if arguments[:2] == ('container', 'rm'):
                self.container_id = 'b' * 64
            return result

        with patch.object(linux, 'docker', replacement), self.assertRaises(ValueError):
            linux.finish(self.workspace, self.env)
        self.assertTrue((self.root / 'source/generated').exists())
        removals = [call for call in self.calls if call[:2] == ('container', 'rm')]
        self.assertEqual(removals, [('container', 'rm', '--force', 'a' * 64)])

    def test_docker_failure_keeps_scratch_for_inspection(self):
        with patch.object(linux, 'docker', side_effect=subprocess.CalledProcessError(1, 'docker')):
            with self.assertRaises(subprocess.CalledProcessError):
                linux.finish(self.workspace, self.env)
        self.assertTrue((self.root / linux.OWNER_FILE).exists())

    def test_auto_remove_wins_inspect_or_remove_without_leaving_scratch(self):
        for stage in ('inspect', 'rm'):
            with self.subTest(stage=stage):
                if not self.root.exists():
                    linux.prepare(self.workspace, self.env)
                    (self.root / 'source').mkdir()
                self.container_id = 'a' * 64

                def disappearing(*arguments):
                    if arguments[:2] == ('container', stage):
                        self.container_id = None
                        raise subprocess.CalledProcessError(1, 'docker')
                    return self.docker(*arguments)

                with patch.object(linux, 'docker', disappearing):
                    linux.finish(self.workspace, self.env)
                self.assertFalse(self.root.exists())
                self.assertTrue(any('id=' + 'a' * 64 in call for call in self.calls))

    def test_daemon_failure_existing_id_or_reused_name_cannot_hide_remove_errors(self):
        for stage in ('inspect', 'rm'):
            for outcome in ('daemon-error', 'same-id', 'reused-name'):
                with self.subTest(stage=stage, outcome=outcome):
                    self.container_id = 'a' * 64
                    failed = False

                    def refuse(*arguments):
                        nonlocal failed
                        if arguments[:2] == ('container', stage):
                            failed = True
                            raise subprocess.CalledProcessError(1, 'docker')
                        if failed:
                            if outcome == 'daemon-error':
                                raise subprocess.CalledProcessError(1, 'docker')
                            if outcome == 'reused-name':
                                if 'id=' + 'a' * 64 in arguments:
                                    return ''
                                return 'b' * 64
                        return self.docker(*arguments)

                    with patch.object(linux, 'docker', refuse), self.assertRaises(subprocess.CalledProcessError):
                        linux.finish(self.workspace, self.env)
                    self.assertTrue((self.root / 'source/generated').exists())

    def test_duplicate_prepare_and_wrong_job_identity_cannot_claim_state(self):
        with self.assertRaises(FileExistsError):
            linux.prepare(self.workspace, self.env)
        for change in [{'GITHUB_JOB': 'macos'}, {'RUNNER_TEMP': '../temp'}, {'GITHUB_REPOSITORY': 'other/repo'}]:
            with self.subTest(change=change), self.assertRaises(ValueError):
                linux.finish(self.workspace, {**self.env, **change})
        self.assertEqual(linux.read_owner(self.root / linux.OWNER_FILE), self.owner)

    @unittest.skipIf(os.name == 'nt', 'Linux-only filesystem ownership')
    def test_source_owner_and_temp_links_refused_without_touching_outside(self):
        outside = self.base / 'outside'
        outside.mkdir()
        kept = outside / 'keep'
        kept.write_text('private synthetic fixture')
        source = self.root / 'source'
        source.rename(self.root / 'original-source')
        source.symlink_to(outside, target_is_directory=True)
        with patch.object(linux, 'docker', self.docker), self.assertRaises(ValueError):
            linux.finish(self.workspace, self.env)
        source.unlink()
        (self.root / 'original-source').rename(source)
        marker = self.root / linux.OWNER_FILE
        marker.rename(self.root / 'original-owner')
        marker.symlink_to(kept)
        with patch.object(linux, 'docker', self.docker), self.assertRaises(ValueError):
            linux.finish(self.workspace, self.env)
        linked_temp = self.base / 'linked-temp'
        linked_temp.symlink_to(self.runner_temp, target_is_directory=True)
        with self.assertRaises(ValueError):
            linux.finish(self.workspace, {**self.env, 'RUNNER_TEMP': str(linked_temp)})
        self.assertEqual(kept.read_text(), 'private synthetic fixture')
        self.assertFalse(any(call[:2] == ('container', 'rm') for call in self.calls))


if __name__ == '__main__':
    unittest.main()
