"""Exercise the CI exit trap with an isolated repo and a fake Docker process."""

import hashlib
import json
import os
from pathlib import Path
import shutil
import signal
import subprocess
import tempfile
import time
import unittest


SOURCE = Path(__file__).resolve().parents[1] / "ci-linux.sh"
JOB = 900
VERSION = f"0.2.5+{JOB}"
PACKAGE_PREFIX = f"ConsoleCrypt-{VERSION}-linux-x64"
SAFE_RECEIPT = {
    "schema": 1,
    "success": False,
    "suites": [],
    "mock_services": False,
    "in_memory_secure_store": False,
    "raw_diagnostics_saved": False,
}
PAYLOADS = {
    f"{PACKAGE_PREFIX}.deb": b"simulated DEB package\n",
    f"{PACKAGE_PREFIX}.rpm": b"simulated RPM package\n",
}

STAGER = """import argparse
from pathlib import Path
parser = argparse.ArgumentParser()
parser.add_argument('--output', required=True, type=Path)
args = parser.parse_args()
args.output.mkdir(parents=True)
(args.output / 'tracked-source').write_text('public synthetic source')
"""

DOCKER = r'''#!/usr/bin/env python3
import hashlib
import json
import os
from pathlib import Path
import signal
import sys

state_path = Path(os.environ['CC_CI_TEST_STATE'])
state = json.loads(state_path.read_text()) if state_path.exists() else {'calls': []}
state['calls'].append(' '.join(sys.argv[1:3]) if sys.argv[1] == 'container' else sys.argv[1])
if sys.argv[1] == 'run':
    state['run_pid'] = os.getpid()
    state['container_name'] = sys.argv[sys.argv.index('--name') + 1]
    state['labels'] = dict(sys.argv[index + 1].split('=', 1) for index, arg in enumerate(sys.argv) if arg == '--label')
    mount = sys.argv[sys.argv.index('--mount') + 1]
    fields = dict(part.split('=', 1) for part in mount.split(','))
    assert fields['type'] == 'bind' and fields['target'] == '/work'
    source = Path(fields['source'])
    state['source'] = str(source)
    state['compiler_diagnostics'] = '--compiler-diagnostics' in sys.argv[-1]
    build_env = sys.argv[sys.argv.index('--env') + 1]
    build_number = os.environ.get('CC_BUILD_NUMBER') or os.environ['CI_JOB_ID']
    assert build_env == 'CC_BUILD_NUMBER=' + build_number
    outside = Path(os.environ['CC_CI_TEST_OUTSIDE'])
    mode = os.environ.get('CC_CI_TEST_RECEIPT_MODE', 'regular')
    if mode == 'dist_symlink':
        linked_dist = outside / 'linked-dist'
        linked_dist.mkdir()
        (source / 'dist').symlink_to(linked_dist, target_is_directory=True)
    output = source / 'dist/linux'
    acceptance = output / 'acceptance'
    if mode == 'acceptance_symlink':
        output.mkdir(parents=True)
        linked_acceptance = outside / 'linked-acceptance'
        linked_acceptance.mkdir()
        acceptance.symlink_to(linked_acceptance, target_is_directory=True)
    else:
        acceptance.mkdir(parents=True)
    safe = {
        'schema': 1, 'success': os.environ['CC_CI_TEST_DOCKER_EXIT'] == '0',
        'suites': [], 'mock_services': False,
        'in_memory_secure_store': False, 'raw_diagnostics_saved': False,
    }
    receipt = acceptance / 'linux-ffi-integration.json'
    if mode == 'receipt_symlink':
        receipt.symlink_to(outside / 'outside-marker.json')
    elif mode == 'receipt_fifo':
        os.mkfifo(receipt, 0o600)
    elif mode == 'receipt_hardlink':
        os.link(outside / 'outside-marker.json', receipt)
    else:
        if mode == 'receipt_unexpected_field':
            safe['unexpected_runtime_data'] = os.environ['CC_CI_TEST_PRIVATE_MARKER']
        receipt.write_text(json.dumps(safe) + '\n')
    marker = os.environ['CC_CI_TEST_PRIVATE_MARKER'].encode()
    (acceptance / 'linux-ffi-integration.log').write_bytes(marker)
    private = acceptance / 'fixture-home/keyrings'
    private.mkdir(parents=True)
    (private / 'login.keyring').write_bytes(marker)
    (acceptance / 'other-private.json').write_bytes(marker)
    (source / 'raw-diagnostic').write_bytes(marker)
    version = '0.2.5+' + build_number
    prefix = 'ConsoleCrypt-' + version + '-linux-x64'
    payloads = {
        prefix + '.deb': b'simulated DEB package\n',
        prefix + '.rpm': b'simulated RPM package\n',
    }
    artifact_mode = os.environ.get('CC_CI_TEST_ARTIFACT_MODE', 'regular')
    if artifact_mode == 'package_symlink':
        payloads[prefix + '.deb'] += marker
    for name, contents in payloads.items():
        destination = output / name
        if name.endswith('.deb') and artifact_mode == 'package_symlink':
            outside_package = outside / 'outside-package.deb'
            outside_package.write_bytes(contents)
            destination.symlink_to(outside_package)
        elif name.endswith('.deb') and artifact_mode == 'package_fifo':
            os.mkfifo(destination, 0o600)
        else:
            destination.write_bytes(contents)
    sums = ''.join(
        hashlib.sha256(contents).hexdigest() + '  ' + name + '\n'
        for name, contents in payloads.items()
    )
    sidecar = output / (prefix + '.SHA256SUMS')
    if artifact_mode == 'sidecar_symlink':
        outside_sidecar = outside / 'outside-checksums'
        outside_sidecar.write_text(sums)
        sidecar.symlink_to(outside_sidecar)
    else:
        sidecar.write_text(sums)
    manifest = {
        'version': version, 'platform': 'linux-x64', 'deb_version': '0.2.5-' + build_number,
        'rpm_version': '0.2.5', 'rpm_release': build_number,
        'native_sha256': {name: hashlib.sha256(name.encode()).hexdigest() for name in (
            'consolecrypt', 'lib/libapp.so', 'lib/libflutter_linux_gtk.so', 'lib/libcc_bridge.so',
        )},
        'packages': {name: {'bytes': len(contents), 'sha256': hashlib.sha256(contents).hexdigest()}
                     for name, contents in payloads.items()},
    }
    if artifact_mode == 'manifest_unexpected_field':
        manifest['unexpected_runtime_data'] = marker.decode()
    (output / (prefix + '.json')).write_text(json.dumps(manifest) + '\n')
    (output / 'other-private.json').write_bytes(marker)
    (output / 'ConsoleCrypt.version').write_text(version + '\n')
elif sys.argv[1] == 'rm':
    assert sys.argv[2:] == ['--force', state['container_name']]
    state['removed_owned_container'] = True
elif sys.argv[1:3] == ['container', 'ls']:
    if state.get('container_name') and not state.get('removed_owned_container'):
        print('a' * 64)
elif sys.argv[1:3] == ['container', 'inspect']:
    print(json.dumps(['a' * 64, '/' + state['container_name'],
          'consolecrypt-linux-builder:3.47.5-rust1.98', state['labels'],
          [{'Type': 'bind', 'Source': state['source'], 'Destination': '/work', 'RW': True}]]))
elif sys.argv[1:3] == ['container', 'rm']:
    assert sys.argv[3:] == ['--force', 'a' * 64]
    state['removed_owned_container'] = True
    if os.environ.get('CC_CI_TEST_RUN_SIGNAL') == 'wait':
        os.kill(state['run_pid'], signal.SIGTERM)
next_state = state_path.with_suffix('.next')
next_state.write_text(json.dumps(state) + '\n')
next_state.replace(state_path)
if sys.argv[1] == 'run' and os.environ.get('CC_CI_TEST_RUN_SIGNAL') == 'term':
    os.kill(os.getpid(), signal.SIGTERM)
if sys.argv[1] == 'run' and os.environ.get('CC_CI_TEST_RUN_SIGNAL') == 'wait':
    while True:
        signal.pause()
raise SystemExit(int(os.environ['CC_CI_TEST_DOCKER_EXIT']) if sys.argv[1] == 'run' else 0)
'''


class LinuxCiRetentionTest(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory(prefix="consolecrypt-ci-test-")
        self.addCleanup(self.temporary.cleanup)
        self.folder = Path(self.temporary.name).resolve()
        self.repo = self.folder / "synthetic repo"
        scripts = self.repo / "client/scripts"
        scripts.mkdir(parents=True)
        shutil.copy2(SOURCE, scripts / "ci-linux.sh")
        shutil.copy2(SOURCE.with_name("retain-linux-receipt.py"), scripts / "retain-linux-receipt.py")
        shutil.copy2(SOURCE.with_name("test-linux-native.py"), scripts / "test-linux-native.py")
        shutil.copy2(SOURCE.with_name("export-linux-artifacts.py"), scripts / "export-linux-artifacts.py")
        for name in ('cleanup-linux-job.py', 'cleanup-ci-workspace.py'):
            shutil.copy2(SOURCE.with_name(name), scripts / name)
        flutter = self.repo / "client/flutter"
        flutter.mkdir()
        (flutter / "pubspec.yaml").write_text("name: synthetic_fixture\nversion: 0.2.5+81\n")
        (scripts / "stage-linux-source.py").write_text(STAGER, encoding="utf-8")
        tools = self.folder / "fake tools"
        tools.mkdir()
        docker = tools / "docker"
        docker.write_text(DOCKER, encoding="utf-8")
        docker.chmod(0o700)
        self.temp_dir = self.folder / "ci temp"
        self.temp_dir.mkdir()
        home = self.folder / "isolated home"
        home.mkdir()
        self.state_path = self.folder / "fake-docker-state.json"
        self.marker = os.urandom(32).hex()  # runtime-only, never assertion output
        self.outside = self.folder / "outside-staging"
        self.outside.mkdir()
        self.outside_marker = self.outside / "outside-marker.json"
        self.outside_contents = json.dumps(SAFE_RECEIPT | {"private_fixture": self.marker}).encode()
        self.outside_marker.write_bytes(self.outside_contents)
        self.env = {
            "PATH": str(tools) + os.pathsep + os.environ["PATH"],
            "HOME": str(home),
            "TMPDIR": str(self.temp_dir),
            "CI_JOB_ID": str(JOB),
            "CC_CI_TEST_STATE": str(self.state_path),
            "CC_CI_TEST_PRIVATE_MARKER": self.marker,
            "CC_CI_TEST_OUTSIDE": str(self.outside),
            "GIT_CONFIG_NOSYSTEM": "1",
            "GIT_CONFIG_GLOBAL": os.devnull,
        }
        for arguments in (
            ["init", "-q"],
            ["add", "--", "client/scripts/ci-linux.sh", "client/scripts/stage-linux-source.py",
             "client/scripts/retain-linux-receipt.py", "client/scripts/test-linux-native.py",
             "client/scripts/export-linux-artifacts.py", "client/flutter/pubspec.yaml"],
            ["-c", "core.hooksPath=" + os.devnull, "-c", "commit.gpgsign=false",
             "-c", "user.name=Integration Fixture", "-c", "user.email=fixture@example.invalid",
             "commit", "-q", "-m", "synthetic CI fixture"],
        ):
            subprocess.run(["git", "-C", str(self.repo), *arguments], env=self.env,
                           check=True, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
        self.source_commit = subprocess.check_output(
            ["git", "-C", str(self.repo), "rev-parse", "HEAD"], env=self.env, text=True,
        ).strip()

    def test_interrupted_docker_cli_removes_only_its_owned_container(self):
        self.env['CC_CI_TEST_RUN_SIGNAL'] = 'term'
        result, state = self.run_ci(0)
        self.assertEqual(result.returncode, 143)
        self.assertTrue(state['removed_owned_container'])

    def test_github_cleanup_survives_a_build_shell_killed_without_exit_trap(self):
        rust = self.repo / 'client/rust'
        rust.mkdir()
        (rust / 'Cargo.toml').write_text('[workspace]\n')
        subprocess.run(['git', '-C', str(self.repo), 'remote', 'add', 'origin',
                        'https://github.com/evsikovas/consolecrypt-client'], check=True)
        env = self.env | {'GITHUB_ACTIONS': 'true', 'GITHUB_REPOSITORY': 'evsikovas/consolecrypt-client',
                         'GITHUB_RUN_ID': '12345', 'GITHUB_RUN_ATTEMPT': '2', 'GITHUB_JOB': 'linux',
                         'GITHUB_WORKSPACE': str(self.repo.resolve()), 'RUNNER_TEMP': str(self.temp_dir.resolve()),
                         'CC_CI_TEST_RUN_SIGNAL': 'wait', 'CC_CI_TEST_DOCKER_EXIT': '0'}
        process = subprocess.Popen(['bash', str(self.repo / 'client/scripts/ci-linux.sh')],
                                   cwd=self.repo, env=env, start_new_session=True,
                                   stdout=subprocess.PIPE, stderr=subprocess.PIPE)
        builder_pid = None
        try:
            deadline = time.monotonic() + 10
            while time.monotonic() < deadline:
                if self.state_path.exists():
                    state = json.loads(self.state_path.read_text())
                    if state.get('run_pid'):
                        builder_pid = state['run_pid']
                        break
                time.sleep(0.02)
            else:
                self.fail('Synthetic builder did not start')
            # SIGKILL proves that cleanup does not depend on a shell exit trap.
            process.kill()
            process.wait(timeout=5)
            cleanup_result = subprocess.run(
                ['python3', str(self.repo / 'client/scripts/cleanup-linux-job.py'), '--workspace', str(self.repo.resolve())],
                env=env, capture_output=True, timeout=10)
            self.assertEqual(cleanup_result.returncode, 0, cleanup_result.stderr.decode())
            self.assertIn(b'LINUX_JOB_CLEANUP_PASS', cleanup_result.stdout)
            self.assertFalse(self.marker.encode() in cleanup_result.stdout + cleanup_result.stderr)
            state = json.loads(self.state_path.read_text())
            self.assertTrue(state['removed_owned_container'])
            self.assertEqual(state['labels'], {'io.consolecrypt.ci.repository': 'evsikovas/consolecrypt-client',
                'io.consolecrypt.ci.run': '12345', 'io.consolecrypt.ci.attempt': '2', 'io.consolecrypt.ci.job': 'linux'})
            self.assertFalse(Path(state['source']).parent.exists())
            self.assertIn('container inspect', state['calls'])
            self.assertIn('container rm', state['calls'])
        finally:
            if process.poll() is None:
                process.kill()
            if builder_pid is not None and not state.get('removed_owned_container'):
                try:
                    os.kill(builder_pid, signal.SIGKILL)
                except ProcessLookupError:
                    pass
            process.communicate(timeout=5)

    def run_ci(self, docker_exit, *, receipt_mode="regular", artifact_mode="regular"):
        self.state_path.unlink(missing_ok=True)
        command = ["bash", str(self.repo / "client/scripts/ci-linux.sh")]
        process = subprocess.Popen(
            command, cwd=self.repo,
            env=self.env | {"CC_CI_TEST_DOCKER_EXIT": str(docker_exit),
                            "CC_CI_TEST_RECEIPT_MODE": receipt_mode,
                            "CC_CI_TEST_ARTIFACT_MODE": artifact_mode},
            stdout=subprocess.PIPE, stderr=subprocess.PIPE, start_new_session=True,
        )
        try:
            stdout, stderr = process.communicate(timeout=15)
        except subprocess.TimeoutExpired:
            # A FIFO regression must not leave an orphaned retention process.
            try:
                os.killpg(process.pid, signal.SIGKILL)
            except ProcessLookupError:
                pass
            process.communicate()
            self.fail("isolated CI fixture exceeded its bounded execution time")
        result = subprocess.CompletedProcess(command, process.returncode, stdout, stderr)
        self.assertFalse(self.marker.encode() in result.stdout + result.stderr,
                         "private fixture payload must never reach process output")
        state = json.loads(self.state_path.read_text())
        self.assertEqual(state["calls"], ["info", "build", "run", "rm"])
        self.assertEqual(state['container_name'], Path(state['source']).parent.name)
        self.assertTrue(state['removed_owned_container'])
        self.assertFalse(Path(state["source"]).parent.exists(), "CI staging must be removed")
        self.assertEqual(list(self.temp_dir.iterdir()), [])
        self.assertTrue(self.outside_marker.read_bytes() == self.outside_contents,
                        "outside-staging fixture must remain unchanged")
        return result, state

    def retained_files(self):
        output = self.repo / "dist/linux"
        return {path.relative_to(output).as_posix(): path for path in output.rglob("*") if path.is_file()}

    def check_receipt(self, files, *, success=False):
        receipt = files["acceptance/linux-ffi-integration.json"]
        self.assertEqual(json.loads(receipt.read_text()), SAFE_RECEIPT | {"success": success})
        self.assertFalse(any(self.marker.encode() in path.read_bytes() for path in files.values()),
                         "only safe receipt/package files may be retained")

    def test_github_build_number_works_without_legacy_job_id(self):
        self.env['CC_BUILD_NUMBER'] = str(JOB)
        del self.env['CI_JOB_ID']
        result, _ = self.run_ci(0)
        self.assertEqual(result.returncode, 0)
        self.assertEqual(self.retained_files()['ConsoleCrypt.version'].read_text(), VERSION + '\n')

    def test_arc_staging_uses_shared_runner_volume(self):
        shared = self.folder / 'shared work temp'
        shared.mkdir()
        self.env['RUNNER_TEMP'] = str(shared)
        result, state = self.run_ci(0)
        self.assertEqual(result.returncode, 0)
        self.assertEqual(Path(state['source']).parent.parent, shared)
        self.assertEqual(list(shared.iterdir()), [])

    def test_docker_failure_retains_only_safe_receipt_and_original_exit(self):
        result, _ = self.run_ci(23)
        self.assertEqual(result.returncode, 23)
        files = self.retained_files()
        self.assertEqual(set(files), {"acceptance/linux-ffi-integration.json"})
        self.check_receipt(files)

    def test_success_retains_packages_sidecars_and_finalizes_source_identity(self):
        result, state = self.run_ci(0)
        self.assertEqual(result.returncode, 0)
        self.assertTrue(state["compiler_diagnostics"])
        files = self.retained_files()
        expected = set(PAYLOADS) | {
            f"{PACKAGE_PREFIX}.SHA256SUMS", f"{PACKAGE_PREFIX}.json",
            "ConsoleCrypt.version", "acceptance/linux-ffi-integration.json",
        }
        self.assertEqual(set(files), expected)
        self.check_receipt(files, success=True)
        for name, contents in PAYLOADS.items():
            self.assertEqual(files[name].read_bytes(), contents)
        sums = "".join(hashlib.sha256(contents).hexdigest() + "  " + name + "\n"
                       for name, contents in PAYLOADS.items())
        self.assertEqual(files[f"{PACKAGE_PREFIX}.SHA256SUMS"].read_text(), sums)
        self.assertEqual(files["ConsoleCrypt.version"].read_text(), VERSION + "\n")
        metadata = json.loads(files[f"{PACKAGE_PREFIX}.json"].read_text())
        self.assertEqual(set(metadata), {"version", "platform", "deb_version", "rpm_version", "rpm_release",
                                        "native_sha256", "packages", "source_commit", "ci_job_id"})
        self.assertEqual(metadata["version"], VERSION)
        self.assertEqual(metadata["source_commit"], self.source_commit)
        self.assertEqual(metadata["ci_job_id"], JOB)
        self.assertEqual(metadata["platform"], "linux-x64")
        self.assertEqual(metadata["deb_version"], f"0.2.5-{JOB}")
        self.assertEqual(metadata["rpm_version"], "0.2.5")
        self.assertEqual(metadata["rpm_release"], str(JOB))
        self.assertEqual(set(metadata["native_sha256"]), {
            "consolecrypt", "lib/libapp.so", "lib/libflutter_linux_gtk.so", "lib/libcc_bridge.so",
        })
        self.assertEqual(metadata["packages"], {
            name: {"bytes": len(contents), "sha256": hashlib.sha256(contents).hexdigest()}
            for name, contents in PAYLOADS.items()
        })

    def test_receipt_copy_failure_never_replaces_original_docker_exit(self):
        destination = self.repo / "dist/linux/acceptance"
        destination.parent.mkdir(parents=True)
        destination.write_text("blocked destination\n")
        result, _ = self.run_ci(23)
        self.assertEqual(result.returncode, 23)
        self.assertEqual(destination.read_text(), "blocked destination\n")

    def test_receipt_copy_failure_turns_success_into_failure_and_cleans_stage(self):
        destination = self.repo / "dist/linux/acceptance"
        destination.parent.mkdir(parents=True)
        destination.write_text("blocked destination\n")
        result, _ = self.run_ci(0)
        self.assertEqual(result.returncode, 1)
        self.assertEqual(destination.read_text(), "blocked destination\n")

    def test_source_symlinks_fifo_and_hardlink_never_export_outside_payload(self):
        for mode in ("receipt_symlink", "acceptance_symlink", "dist_symlink",
                     "receipt_fifo", "receipt_hardlink"):
            with self.subTest(mode=mode):
                result, _ = self.run_ci(23, receipt_mode=mode)
                self.assertEqual(result.returncode, 23)
                self.assertEqual(set(self.retained_files()), set())
                output = self.repo / "dist"
                if output.exists():
                    shutil.rmtree(output)

    def test_final_destination_symlink_is_replaced_without_following_target(self):
        destination = self.repo / "dist/linux/acceptance/linux-ffi-integration.json"
        destination.parent.mkdir(parents=True)
        destination.symlink_to(self.outside_marker)
        result, _ = self.run_ci(23)
        self.assertEqual(result.returncode, 23)
        self.assertFalse(destination.is_symlink())
        self.assertEqual(set(self.retained_files()), {"acceptance/linux-ffi-integration.json"})
        self.check_receipt(self.retained_files())
        self.assertEqual(destination.stat().st_mode & 0o777, 0o600)

    def test_valid_privacy_headers_cannot_export_unexpected_raw_field(self):
        result, _ = self.run_ci(23, receipt_mode="receipt_unexpected_field")
        self.assertEqual(result.returncode, 23)
        self.assertEqual(set(self.retained_files()), set())

    def test_destination_directory_symlinks_are_rejected_without_writing_outside(self):
        for parts in (("dist",), ("dist", "linux"), ("dist", "linux", "acceptance")):
            with self.subTest(parts=parts):
                destination = self.repo.joinpath(*parts)
                destination.parent.mkdir(parents=True, exist_ok=True)
                outside_directory = self.outside / ("destination-" + "-".join(parts))
                outside_directory.mkdir()
                marker = outside_directory / "linux-ffi-integration.json"
                marker.write_bytes(self.outside_contents)
                destination.symlink_to(outside_directory, target_is_directory=True)
                result, _ = self.run_ci(23)
                self.assertEqual(result.returncode, 23)
                self.assertTrue(destination.is_symlink())
                self.assertTrue(marker.read_bytes() == self.outside_contents,
                                "symlink destination must not be modified")
                self.assertEqual(set(outside_directory.iterdir()), {marker})
                destination.unlink()
                output = self.repo / "dist"
                if output.exists():
                    shutil.rmtree(output)

    def test_successful_docker_cannot_export_package_links_fifo_or_raw_manifest(self):
        for mode in ("package_symlink", "sidecar_symlink", "package_fifo", "manifest_unexpected_field"):
            with self.subTest(mode=mode):
                result, _ = self.run_ci(0, artifact_mode=mode)
                self.assertEqual(result.returncode, 1)
                files = self.retained_files()
                self.assertEqual(set(files), {"acceptance/linux-ffi-integration.json"})
                self.check_receipt(files, success=True)
                shutil.rmtree(self.repo / "dist")

    def test_successful_docker_source_directory_link_exports_nothing(self):
        result, _ = self.run_ci(0, receipt_mode="dist_symlink")
        self.assertEqual(result.returncode, 1)
        self.assertEqual(set(self.retained_files()), set())

    def test_successful_docker_destination_parent_links_never_write_outside(self):
        for parts in (("dist",), ("dist", "linux")):
            with self.subTest(parts=parts):
                destination = self.repo.joinpath(*parts)
                destination.parent.mkdir(parents=True, exist_ok=True)
                outside_directory = self.outside / ("package-destination-" + "-".join(parts))
                outside_directory.mkdir()
                marker = outside_directory / "outside-marker"
                marker.write_bytes(self.outside_contents)
                destination.symlink_to(outside_directory, target_is_directory=True)
                result, _ = self.run_ci(0)
                self.assertEqual(result.returncode, 1)
                self.assertTrue(destination.is_symlink())
                self.assertTrue(marker.read_bytes() == self.outside_contents)
                self.assertEqual(set(outside_directory.iterdir()), {marker})
                destination.unlink()
                output = self.repo / "dist"
                if output.exists():
                    shutil.rmtree(output)

    def test_package_destination_link_is_replaced_without_writing_outside(self):
        destination = self.repo / "dist/linux" / f"{PACKAGE_PREFIX}.deb"
        destination.parent.mkdir(parents=True)
        destination.symlink_to(self.outside_marker)
        result, _ = self.run_ci(0)
        self.assertEqual(result.returncode, 0)
        self.assertFalse(destination.is_symlink())
        self.assertEqual(destination.read_bytes(), PAYLOADS[destination.name])
        self.assertEqual(destination.stat().st_mode & 0o777, 0o600)
        self.check_receipt(self.retained_files(), success=True)


if __name__ == "__main__":
    unittest.main()
