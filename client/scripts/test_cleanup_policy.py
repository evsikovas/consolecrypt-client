"""Exercise the release publication boundary with fake GitHub commands."""
import os
from pathlib import Path
import re
import subprocess
import tempfile
import textwrap
import unittest


ROOT = Path(__file__).resolve().parents[2]


def jobs(name):
    text = (ROOT / '.github/workflows' / name).read_text().split('\njobs:\n', 1)[1]
    matches = list(re.finditer(r'^  ([a-z_]+):\n', text, re.MULTILINE))
    return {m[1]: text[m.end():matches[i + 1].start() if i + 1 < len(matches) else len(text)]
            for i, m in enumerate(matches)}


class CleanupPolicyTest(unittest.TestCase):
    def test_native_jobs_keep_intermediates_and_only_publication_schedules_cleanup(self):
        release = jobs('client.yml')
        for name in ('windows', 'macos', 'android', 'ios'):
            with self.subTest(job=name):
                self.assertIn('clean: false', release[name])
                self.assertRegex(release[name], r'--claim-workspace|-ClaimWorkspace')
                self.assertNotRegex(release[name], r'--after-publication|-AfterPublication|--maintenance')
        for name in ('cleanup_windows', 'cleanup_macos'):
            self.assertIn('needs: publish', release[name])
            self.assertIn("if: always() && needs.publish.outputs.published == 'true'", release[name])
            self.assertNotIn('actions/checkout', release[name])
            self.assertRegex(release[name], r'--after-publication|-AfterPublication')
        self.assertNotIn('cleanup-ci-workspace', release['linux'])
        self.assertIn('cleanup-linux-job', release['linux'])
        diagnostic = (ROOT / '.github/workflows/windows-native-check.yml').read_text()
        self.assertIn('group: consolecrypt-client-packages', diagnostic)
        self.assertIn('clean: false', jobs('windows-native-check.yml')['windows'])
        self.assertNotRegex(diagnostic, r'--after-publication|-AfterPublication|--maintenance')

    @unittest.skipIf(os.name == 'nt', 'Publication runs in hosted Linux bash')
    def test_publication_proof_is_written_only_after_successful_publication(self):
        publish = jobs('client.yml')['publish']
        block = publish.split('id: release\n        run: |\n', 1)[1].split('\n      - name:', 1)[0]
        command = textwrap.dedent(block)
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            fake = root / 'gh'
            fake.write_text('#!/bin/sh\n[ "$1" = release ] || exit 99\n'
                            '[ "$2" != "$FAIL_STAGE" ] || exit 42\nexit 0\n')
            fake.chmod(0o700)
            for stage in ('create', 'edit', ''):
                with self.subTest(stage=stage):
                    proof = root / ('proof-' + (stage or 'success'))
                    result = subprocess.run(['bash', '-euo', 'pipefail', '-c', command], cwd=root,
                        env={**os.environ, 'PATH': str(root) + os.pathsep + os.environ['PATH'],
                             'GITHUB_REF_NAME': 'v9.8.7', 'RELEASE_VERSION': '9.8.7',
                             'GITHUB_OUTPUT': str(proof), 'FAIL_STAGE': stage}, capture_output=True)
                    if stage:
                        self.assertNotEqual(result.returncode, 0)
                        self.assertFalse(proof.exists())
                    else:
                        self.assertEqual(result.returncode, 0, result.stderr)
                        self.assertEqual(proof.read_text(), 'published=true\n')


if __name__ == '__main__':
    unittest.main()
