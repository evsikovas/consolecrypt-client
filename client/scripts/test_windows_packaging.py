"""Exercise the release script with isolated fake compilers, never real builds."""
from pathlib import Path
import os
import shutil
import subprocess
import tempfile
import textwrap
import unittest
import zipfile

ROOT = Path(__file__).resolve().parents[2]


@unittest.skipUnless(os.name == 'posix' and shutil.which('pwsh'), 'requires PowerShell on a POSIX host')
class WindowsPackagingTest(unittest.TestCase):
    def test_bundle_and_compiler_failure(self):
        with tempfile.TemporaryDirectory(prefix='cc-windows-script-') as temporary:
            base = Path(temporary)
            repo = base / 'repo'
            scripts = repo / 'client/scripts'
            scripts.mkdir(parents=True)
            for name in ('build-windows.ps1', 'bump-version.py'):
                shutil.copy2(ROOT / 'client/scripts' / name, scripts / name)
            for name in ('client/flutter/pubspec.yaml', 'client/flutter/lib/app/app_info.dart', 'LICENSE-MIT', 'LICENSE-APACHE'):
                target = repo / name
                target.parent.mkdir(parents=True, exist_ok=True)
                shutil.copy2(ROOT / name, target)
            binaries = base / 'bin'
            binaries.mkdir()

            def executable(path, content):
                path.parent.mkdir(parents=True, exist_ok=True)
                path.write_text(textwrap.dedent(content).lstrip())
                path.chmod(0o755)

            for name in ('cargo', 'rustup', 'perl'):
                executable(binaries / name, '#!/bin/sh\nexit 0\n')
            executable(binaries / 'python', '#!/bin/sh\nexec python3 "$@"\n')
            executable(binaries / 'flutter', '''
                #!/usr/bin/env python3
                import os, sys, pathlib
                if os.environ.get('CC_FAKE_FLUTTER_FAIL') == '1':
                    sys.exit(23)
                if sys.argv[1] == 'build':
                    folder = pathlib.Path('build/windows/x64/runner/Release')
                    for name in ('ConsoleCrypt.exe', 'flutter_windows.dll', 'cc_bridge.dll', 'data/icudtl.dat'):
                        target = folder / name
                        target.parent.mkdir(parents=True, exist_ok=True)
                        target.write_text('test bundle')
            ''')
            executable(binaries / 'ISCC.exe', '''
                #!/usr/bin/env python3
                import pathlib, sys
                args = dict(a[2:].split('=', 1) for a in sys.argv[1:] if a.startswith('/D'))
                target = pathlib.Path(args['OutputDir']) / (args['OutputName'] + '.exe')
                target.write_text('test compiler output only')
            ''')
            vs = base / 'VS'
            runtime = vs / 'VC/Redist/MSVC/14.44.12345/x64/Microsoft.VC143.CRT'
            runtime.mkdir(parents=True)
            for name in ('msvcp140.dll', 'vcruntime140.dll', 'vcruntime140_1.dll'):
                (runtime / name).write_text('test runtime')
            program_files = base / 'ProgramFiles'
            executable(program_files / 'Microsoft Visual Studio/Installer/vswhere.exe',
                       '#!/bin/sh\nprintf "%s\\n" "$CC_FAKE_VS"\n')
            env = os.environ.copy()
            env.update({'PATH': str(binaries) + ':' + env['PATH'],
                        'ProgramFiles(x86)': str(program_files), 'CC_FAKE_VS': str(vs)})
            command = [shutil.which('pwsh'), '-NoProfile', '-NonInteractive', '-File',
                       str(scripts / 'build-windows.ps1'), '-Installer', '-NoCli']
            result = subprocess.run(command, env=env, text=True, capture_output=True)
            self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
            output = repo / 'dist/windows'
            installers = list(output.glob('*-setup.exe'))
            self.assertEqual(len(installers), 1)
            self.assertTrue(installers[0].with_suffix('.exe.sha256').exists())
            with zipfile.ZipFile(output / 'ConsoleCrypt-windows.zip') as archive:
                self.assertTrue({'ConsoleCrypt.exe', 'cc_bridge.dll', 'vcruntime140_1.dll',
                                 'data/icudtl.dat'} <= set(archive.namelist()))
            version = (output / 'ConsoleCrypt.version').read_text()
            env['CC_FAKE_FLUTTER_FAIL'] = '1'
            result = subprocess.run(command, env=env, text=True, capture_output=True)
            self.assertNotEqual(result.returncode, 0)
            self.assertIn('23', result.stdout + result.stderr)
            self.assertEqual((output / 'ConsoleCrypt.version').read_text(), version)


if __name__ == '__main__':
    unittest.main()
