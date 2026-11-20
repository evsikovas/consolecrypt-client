from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
import uuid

from consolecrypt_runner.protocol import decode, encode
import consolecrypt_runner


class BootstrapTests(unittest.TestCase):
    def test_initialization_failure_returns_safe_error_for_the_original_request(self):
        with tempfile.TemporaryDirectory(prefix='cc-bootstrap-test-') as folder:
            root = Path(folder) / 'public-state'
            root.mkdir(mode=0o755)
            root.chmod(0o755)
            request_id = str(uuid.uuid4())
            request = {'version': 1, 'request_id': request_id, 'operation': 'probe', 'body': {}}
            bootstrap = "import sys,runpy;sys.path.insert(0,sys.argv.pop(1));runpy.run_module('consolecrypt_runner',run_name='__main__')"
            module_dir = str(Path(consolecrypt_runner.__file__).resolve().parent.parent)
            result = subprocess.run([sys.executable, '-I', '-c', bootstrap, module_dir, '--stdio', '--root', str(root)],
                input=encode(request) + b'\n', stdout=subprocess.PIPE, stderr=subprocess.PIPE, timeout=10)
            self.assertEqual(result.returncode, 65, result.stderr.decode('utf-8',errors='replace'))
            response = decode(result.stdout.strip())
            self.assertEqual(response['request_id'], request_id)
            self.assertEqual(response['error']['code'], 'private_storage')
            self.assertNotIn(str(root).encode(), result.stdout + result.stderr)
            self.assertEqual(result.stderr, b'')
