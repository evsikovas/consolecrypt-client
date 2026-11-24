"""Exercise the SSH stdin/EOF protocol without credentials or a live registry."""
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import time
import unittest


HELPER = Path(__file__).resolve().parents[2] / "rust/app-core/src/docker_runtime/buildx_remote.py"
DIGEST = "sha256:" + "a" * 64
FAKE_CLI = """
import json, os, stat, sys, time
from pathlib import Path
args = sys.argv[1:]
if args == ["context", "inspect", "--format", "{{.Endpoints.docker.Host}}"]:
    print("unix:///run/user/1001/docker.sock")
    sys.exit(0)
if args[-1] == "version":
    sys.exit(0)
directory = Path(os.environ["DOCKER_CONFIG"])
config = directory / "config.json"
request = dict(args=args, directory=str(directory), mode=stat.S_IMODE(config.stat().st_mode),
               auth=json.loads(config.read_text())["auths"], pid=os.getpid())
Path(os.environ["RECORD"]).write_text(json.dumps(request))
if os.environ["MODE"] == "wait":
    time.sleep(30)
if os.environ["MODE"] == "fail":
    sys.exit(1)
Path(args[args.index("--metadata-file") + 1]).write_text(json.dumps({"containerimage.digest": "sha256:" + "a" * 64}))
"""


@unittest.skipUnless(os.name == "posix", "SSH build hosts run a POSIX shell")
class RemoteBuildTests(unittest.TestCase):
    def setUp(self):
        temp = tempfile.TemporaryDirectory()
        self.addCleanup(temp.cleanup)
        self.root = Path(temp.name).resolve()
        self.project = self.root / "project with spaces; echo example"
        self.project.mkdir()
        (self.project / "Dockerfile").write_text("FROM scratch\n")
        cli = self.root / "docker"
        cli.write_text("#!" + sys.executable + "\n" + FAKE_CLI)
        cli.chmod(0o700)
        self.record = self.root / "record.json"

    def start(self, mode):
        env = dict(os.environ, PATH=str(self.root) + os.pathsep + os.environ["PATH"],
                   TMPDIR=str(self.root), MODE=mode, RECORD=str(self.record), DOCKER_CONTEXT="fixture")
        request = dict(directory=str(self.project), platforms=["linux/amd64", "linux/arm64"],
                       endpoint=None, image="example.test/team/app:local", registry="example.test", auth="fixture")
        process = subprocess.Popen([sys.executable, "-I", str(HELPER)], env=env,
                                   stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
        def cleanup():
            if process.poll() is None:
                process.kill()
            process.wait()
            for stream in (process.stdin, process.stdout, process.stderr):
                stream.close()
        self.addCleanup(cleanup)
        process.stdin.write((json.dumps(request) + "\n").encode())
        process.stdin.flush()
        return process

    def test_two_platforms_from_exact_server_folder(self):
        process = self.start("success")
        self.assertEqual(process.wait(timeout=15), 0)
        self.assertEqual(process.stdout.read().decode().strip(), DIGEST)
        data = json.loads(self.record.read_text())
        directory = Path(data["directory"])
        self.assertEqual(data["args"], ["--host", "unix:///run/user/1001/docker.sock", "buildx", "build",
                                        "--platform", "linux/amd64,linux/arm64", "-t", "example.test/team/app:local",
                                        "--push", "--metadata-file", str(directory / "result.json"), str(self.project)])
        self.assertEqual(data["auth"], {"example.test": {"auth": "fixture"}})
        self.assertEqual(data["mode"], 0o600)
        self.assertFalse(directory.exists())

    def test_failed_build_drops_temporary_config(self):
        process = self.start("fail")
        self.assertEqual(process.wait(timeout=15), 1)
        self.assertEqual(process.stdout.read(), b"")
        self.assertFalse(Path(json.loads(self.record.read_text())["directory"]).exists())

    def test_disconnect_stops_build_and_cleans_up(self):
        process = self.start("wait")
        deadline = time.monotonic() + 10
        while not self.record.exists() and time.monotonic() < deadline:
            time.sleep(0.05)
        self.assertTrue(self.record.exists())
        process.stdin.close()
        self.assertEqual(process.wait(timeout=5), 1)
        data = json.loads(self.record.read_text())
        self.assertFalse(Path(data["directory"]).exists())
        with self.assertRaises(ProcessLookupError):
            os.kill(data["pid"], 0)
