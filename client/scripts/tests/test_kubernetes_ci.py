"""Test CI ownership, failure retention and the actual local Git fixture."""
import importlib.util
import io
import json
import os
from pathlib import Path
import shutil
import socket
import subprocess
import sys
import tempfile
import time
import unittest
from unittest.mock import patch
import urllib.request

ROOT = Path(__file__).resolve().parents[2]


def load(name, path):
    spec = importlib.util.spec_from_file_location(name, path)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


CI = load("kubernetes_ci", ROOT / "scripts/ci-kubernetes-linux.py")
FIXTURES = load("kubernetes_fixtures", ROOT / "ci/kubernetes/fixtures.py")


class OwnershipTest(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(
            prefix="cc-kube-ci-unit-", dir=Path(tempfile.gettempdir()).resolve())
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.workspace = self.root / "public source"
        self.workspace.mkdir()
        self.env = {"RUNNER_TEMP": str(self.root),
                    "GITHUB_REPOSITORY": "fixture/public", "GITHUB_RUN_ID": "1",
                    "GITHUB_RUN_ATTEMPT": "1", "GITHUB_JOB": "linux-kubernetes"}
        self.task = CI.Acceptance(self.workspace, self.env)
        self.expected = {
            "role": "cluster", "name": "fixture", "image": CI.K3S,
            "network": "owned-network", "id": "a" * 64, "mounts": [],
        }
        self.actual = {
            "Id": "a" * 64, "Name": "/fixture",
            "Config": {"Image": CI.K3S, "Labels": {
                CI.OWNER_LABEL: self.task.state["owner"], CI.ROLE_LABEL: "cluster",
            }},
            "HostConfig": {"NetworkMode": "owned-network", "PortBindings": {}, "Privileged": True},
            "Mounts": [],
        }

    def test_owner_is_bound_to_ci_attempt_and_scratch(self):
        path = self.workspace / "dist" / CI.STATE_NAME
        self.assertEqual(CI.read_state(path, self.workspace, self.env), self.task.state)
        with self.assertRaises(ValueError):
            CI.read_state(path, self.workspace, {**self.env, "GITHUB_RUN_ATTEMPT": "2"})
        self.assertEqual((self.task.scratch / "owner.json").stat().st_mode & 0o777, 0o600)
        with self.assertRaises(ValueError):
            CI.Acceptance(self.workspace, self.env)

    def test_symlink_state_and_mount_escape_are_rejected(self):
        original = self.task.scratch / "owner.json"
        replacement = self.root / "owner-copy"
        replacement.write_bytes(original.read_bytes())
        original.unlink()
        original.symlink_to(replacement)
        with self.assertRaises(ValueError):
            CI.read_state(self.task.state_file, self.workspace, self.env)
        self.actual["Mounts"] = [{"Type": "bind", "Source": "/home/private", "Destination": "/work", "RW": True}]
        with self.assertRaises(ValueError):
            CI.validate_container(self.actual, self.expected, self.task.state["owner"])

    def test_wrong_owner_image_network_and_host_port_are_never_stopped(self):
        for change in [
            lambda info: info["Config"]["Labels"].update({CI.OWNER_LABEL: "someone-else"}),
            lambda info: info["Config"].update({"Image": "production-image"}),
            lambda info: info["HostConfig"].update({"NetworkMode": "host"}),
            lambda info: info["HostConfig"].update({"PortBindings": {"6443/tcp": [{"HostPort": "6443"}]}}),
            lambda info: info.update({"Id": "b" * 64}),
        ]:
            actual = json.loads(json.dumps(self.actual))
            change(actual)
            with self.assertRaises(ValueError):
                CI.validate_container(actual, self.expected, self.task.state["owner"])

    def test_cleanup_only_stops_inspected_id_and_retains_scratch(self):
        marker = self.task.scratch / "failed-compiler-output"
        marker.write_text("private runtime data")
        self.task.state["containers"] = [self.expected]
        calls = []
        def docker(step, *args, **_kwargs):
            calls.append((step, args))
            if step == "inspect":
                return json.dumps(self.actual).encode()
            return b""
        self.task.docker = docker
        self.task.stop()
        stop = next(args for step, args in calls if step == "stop-cluster")
        self.assertEqual(stop, ("container", "stop", "--time=10", "a" * 64))
        self.assertFalse(any("rm" in args or "prune" in args for _, args in calls))
        self.assertTrue(marker.exists())
        self.actual["Config"]["Labels"][CI.OWNER_LABEL] = "replacement"
        calls.clear()
        with self.assertRaises(CI.StepFailed):
            self.task.stop()
        self.assertFalse(any(step.startswith("stop-") for step, _ in calls))

    def test_receipt_has_no_runtime_payload_or_credentials(self):
        marker = os.urandom(24).hex()
        (self.task.scratch / "raw.log").write_text(marker)
        self.task.phase = "fixtures"
        self.task.publish_receipt()
        for name in ("kubernetes-acceptance.json", "kubernetes-acceptance.log"):
            self.assertNotIn(marker, (self.task.output / name).read_text())
        receipt = json.loads((self.task.output / "kubernetes-acceptance.json").read_text())
        self.assertFalse(receipt["success"])
        self.assertFalse(receipt["raw_diagnostics_published"])

    def test_logging_failure_still_stops_only_owned_container(self):
        self.task.state["containers"] = [self.expected]
        calls = []
        def docker(step, *args, **_kwargs):
            calls.append((step, args))
            if step == "inspect":
                return json.dumps(self.actual).encode()
            if step.startswith("logs-"):
                raise CI.StepFailed("log-driver")
            return b""
        self.task.docker = docker
        with self.assertRaises(CI.StepFailed):
            self.task.stop()
        self.assertEqual([args[-1] for step, args in calls if step == "stop-cluster"], ["a" * 64])

    def test_failed_native_test_output_is_retained_privately(self):
        with self.assertRaises(CI.StepFailed):
            self.task.run("test-failure", [sys.executable, "-c",
                          "print('synthetic assertion detail'); raise SystemExit(1)"], capture=True)
        paths = list(self.task.scratch.glob("*-test-failure.stdout.log"))
        self.assertEqual(len(paths), 1)
        self.assertEqual(paths[0].stat().st_mode & 0o777, 0o600)
        self.assertIn("synthetic assertion detail", paths[0].read_text())
        self.task.publish_receipt()
        self.assertNotIn("synthetic assertion detail",
                         (self.task.output / "kubernetes-acceptance.log").read_text())

    def test_pinned_download_rejects_modified_manifests(self):
        for url, digest in CI.DOWNLOADS.values():
            self.assertTrue(url.startswith("https://"))
            self.assertRegex(digest, r"^[a-f0-9]{64}$")
        with patch.object(CI.urllib.request, "urlopen", return_value=io.BytesIO(b"tampered")):
            with self.assertRaises(ValueError):
                CI.verified_download("flux")

    def test_scale_receipt_is_complete_and_drops_arbitrary_fields(self):
        lines = []
        for sample in range(1, 4):
            lines.append("CC_KUBE_SCALE " + json.dumps({
                "sample": sample, "pods": 1000, "pages": 5, "page_limit": 200,
                "cold_first_table_ms": 100, "cold_full_table_ms": 500, "decoded_table_bytes": 1000,
                "unexpected_secret": "never publish",
            }))
        values = CI.scale_receipts("\n".join(lines))
        self.assertEqual(len(values), 3)
        self.assertNotIn("never publish", json.dumps(values))
        with self.assertRaises(ValueError):
            CI.scale_receipts("\n".join(lines[:2]))
        with self.assertRaises(ValueError):
            CI.scale_receipts("\n".join(lines).replace('"cold_first_table_ms": 100', '"cold_first_table_ms": 2100'))


class FixtureTest(unittest.TestCase):
    def test_scale_pods_cannot_start_containers(self):
        objects = FIXTURES.scale_objects()
        self.assertEqual(len(objects), 1001)
        for pod in objects[1:]:
            self.assertEqual(pod["metadata"]["namespace"], "cc-acceptance-scale")
            self.assertEqual(pod["spec"]["schedulingGates"], [{"name": "consolecrypt.dev/acceptance"}])
            self.assertFalse(pod["spec"]["automountServiceAccountToken"])

    def test_sources_are_local_and_endpoints_cannot_use_loopback(self):
        objects = FIXTURES.flux_objects()
        for item in objects:
            if item["kind"] in ("GitRepository", "OCIRepository"):
                self.assertIn(".cc-acceptance.svc:", item["spec"]["url"])
        self.assertTrue(any(item["kind"] == "HelmRelease" for item in objects))
        for address in ("127.0.0.1", "0.0.0.0", "::1"):
            with self.assertRaises(ValueError):
                FIXTURES.base_objects(address)

    @unittest.skipUnless(shutil.which("git"), "git required for the real HTTP clone fixture")
    def test_actual_git_http_backend_serves_chart_and_kustomization_without_push(self):
        with tempfile.TemporaryDirectory(prefix="cc-git-fixture-") as temporary:
            root = Path(temporary)
            with socket.socket() as listener:
                listener.bind(("127.0.0.1", 0))
                port = listener.getsockname()[1]
            command = (
                "import importlib.util,sys;from pathlib import Path;"
                "s=importlib.util.spec_from_file_location('fixture',sys.argv[1]);"
                "m=importlib.util.module_from_spec(s);s.loader.exec_module(m);"
                "m.serve_git(Path(sys.argv[2]),int(sys.argv[3]),'127.0.0.1')"
            )
            process = subprocess.Popen(
                [sys.executable, "-c", command, str(ROOT / "ci/kubernetes/fixtures.py"),
                 str(root / "repo.git"), str(port)], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
            )
            try:
                url = f"http://127.0.0.1:{port}/repo.git"
                deadline = time.monotonic() + 10
                while True:
                    try:
                        urllib.request.urlopen(url + "/info/refs?service=git-upload-pack", timeout=1).close()
                        break
                    except OSError:
                        if time.monotonic() > deadline:
                            self.fail("local Git fixture did not start")
                        time.sleep(0.05)
                subprocess.run(["git", "-c", "credential.helper=", "clone", "--quiet", url, str(root / "checkout")],
                               check=True, capture_output=True, timeout=20,
                               env={**os.environ, "GIT_CONFIG_GLOBAL": os.devnull, "GIT_CONFIG_NOSYSTEM": "1"})
                self.assertTrue((root / "checkout/charts/podinfo/Chart.yaml").is_file())
                service = json.loads((root / "checkout/kustomize/service.yaml").read_text())
                self.assertEqual(service["spec"]["ports"][0]["port"], 9898)
                with self.assertRaises(urllib.error.HTTPError) as denied:
                    urllib.request.urlopen(url + "/info/refs?service=git-receive-pack", timeout=1)
                denied.exception.close()
            finally:
                process.terminate()
                process.wait(timeout=10)


if __name__ == "__main__":
    unittest.main()
