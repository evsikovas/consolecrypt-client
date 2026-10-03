"""Exercise artifact replacement with fake compilers and actual hardlinks."""
from pathlib import Path
import hashlib
import os
import shutil
import subprocess
import tempfile
import unittest

SCRIPT = Path(__file__).with_name('build-android.sh')


@unittest.skipUnless(os.name == 'posix', 'requires Bash and POSIX hardlinks')
class AndroidPackagingTest(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory(prefix='cc-android-packaging-')
        self.addCleanup(self.temporary.cleanup)
        self.base = Path(self.temporary.name)
        self.repo = self.base / 'repo'
        scripts = self.repo / 'client/scripts'
        scripts.mkdir(parents=True)
        shutil.copy2(SCRIPT, scripts / SCRIPT.name)
        (scripts / 'bump-version.py').write_text("print('0.0.2+2')\n")
        (scripts / 'verify-android-apk.py').write_text('raise SystemExit(0)\n')
        self.flutter = self.repo / 'client/flutter'
        self.flutter.mkdir()
        sdk = self.base / 'sdk'
        (sdk / 'platforms/android-36').mkdir(parents=True)
        self.executable(sdk / 'build-tools/36.0.0/apksigner', '#!/bin/sh\nexit 0\n')
        self.executable(sdk / 'build-tools/36.0.0/zipalign', '#!/bin/sh\nexit 0\n')
        self.binaries = self.base / 'bin'
        self.executable(self.binaries / 'flutter', '''#!/bin/sh
set -eu
mkdir -p build/app/outputs/flutter-apk
printf 'new test artifact' > build/app/outputs/flutter-apk/app-release.apk
''')
        self.environment = os.environ.copy()
        android_user = self.base / 'android-user'
        android_user.mkdir()
        (android_user / 'debug.keystore').write_bytes(b'synthetic marker; not opened')
        self.environment.update({
            'ANDROID_HOME': str(sdk),
            'ANDROID_USER_HOME': str(android_user),
            'PATH': str(self.binaries) + os.pathsep + self.environment['PATH'],
        })
        self.output = self.repo / 'dist/android'
        self.output.mkdir(parents=True)
        self.release = self.output / 'ConsoleCrypt-0.0.1-android-arm64.apk'
        self.release.write_bytes(b'previous test artifact')
        self.alias = self.output / 'ConsoleCrypt-android-arm64.apk'
        os.link(self.release, self.alias)
        self.old_inode = self.release.stat().st_ino

    @staticmethod
    def executable(path, content):
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(content)
        path.chmod(0o755)

    def run_packaging(self):
        return subprocess.run(
            ['bash', str(self.repo / 'client/scripts/build-android.sh')],
            env=self.environment, text=True, capture_output=True,
        )

    def test_replacing_latest_preserves_retained_hardlink(self):
        result = self.run_packaging()
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        self.assertEqual(self.release.read_bytes(), b'previous test artifact')
        self.assertEqual(self.release.stat().st_ino, self.old_inode)
        self.assertEqual(self.alias.read_bytes(), b'new test artifact')
        self.assertNotEqual(self.alias.stat().st_ino, self.old_inode)
        digest = hashlib.sha256(self.alias.read_bytes()).hexdigest()
        self.assertEqual(
            (self.output / 'ConsoleCrypt-android-arm64.apk.sha256').read_text(),
            digest + '  ConsoleCrypt-android-arm64.apk\n',
        )
        self.assertEqual((self.output / 'ConsoleCrypt.version').read_text(), '0.0.2+2\n')
        self.assertEqual(list(self.output.glob('*.tmp.*')), [])

    def test_missing_preview_signer_fails_before_compilation_without_key_creation(self):
        signer = Path(self.environment['ANDROID_USER_HOME']) / 'debug.keystore'
        signer.unlink()
        result = self.run_packaging()
        self.assertEqual(result.returncode, 1)
        self.assertIn('no key is created', result.stderr)
        self.assertFalse(signer.exists())
        self.assertFalse((self.flutter / 'build').exists())
        self.assertEqual(self.alias.read_bytes(), b'previous test artifact')

    def test_failed_copy_preserves_alias_and_cleans_staging(self):
        self.executable(self.binaries / 'cp', '#!/bin/sh\nexit 23\n')
        result = self.run_packaging()
        self.assertEqual(result.returncode, 23, result.stdout + result.stderr)
        self.assertEqual(self.release.read_bytes(), b'previous test artifact')
        self.assertEqual(self.alias.read_bytes(), b'previous test artifact')
        self.assertEqual(self.alias.stat().st_ino, self.old_inode)
        self.assertEqual(list(self.output.glob('*.tmp.*')), [])
        self.assertFalse((self.output / 'ConsoleCrypt.version').exists())
        self.assertFalse((self.output / 'ConsoleCrypt-android-arm64.apk.sha256').exists())


if __name__ == '__main__':
    unittest.main()
