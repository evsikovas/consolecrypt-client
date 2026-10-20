#!/usr/bin/env python3
"""Run real Kubernetes acceptance in exclusively owned Docker containers.

Public outputs contain only fixed-schema receipts. Credentials, compiler output
and cluster diagnostics stay in private runner scratch. Stop compute on exit;
retain scratch and stopped containers for diagnosis until release cleanup.
"""
import argparse
import hashlib
import importlib.util
import ipaddress
import json
import os
from pathlib import Path
import re
import signal
import stat
import subprocess
import sys
import tempfile
import time
import urllib.request
import uuid

BUILDER = "consolecrypt-linux-builder:3.47.5-rust1.98"
K3S = "rancher/k3s:v1.32.4-k3s1@sha256:1b71c68b063364bab469452eb9cc27edcb2767273f1aca5582db3586a3927483"
REGISTRY = "registry:2.8.3@sha256:a3d8aaa63ed8681a604f1dea0aa03f100d5895b6a58ace528858a7b332415373"
DOWNLOADS = {
    "flux": ("https://github.com/fluxcd/flux2/releases/download/v2.6.4/install.yaml",
             "7cd043a823666727b4b9de408346148b28741b47eb0b3e4237ab7259d4ad3ab8"),
    "argo": ("https://raw.githubusercontent.com/argoproj/argo-cd/v3.1.9/manifests/crds/application-crd.yaml",
             "277acfc67f7d5646c398be72105a80fb8b9ee79706eb08b7d985ffc713b1e2f8"),
}
# Explicit names prevent an unrelated future ignored/destructive test from
# silently joining this fixture's authority.
TESTS = (
    "isolated_k3s_flux_helm_and_streams",
    "isolated_argo_application_actions_and_production_guard",
    "isolated_thousand_pods_cold_native_table_latency",
)
OWNER_LABEL = "io.consolecrypt.kubernetes-ci.owner"
ROLE_LABEL = "io.consolecrypt.kubernetes-ci.role"
STATE_NAME = ".kubernetes-ci-owner.json"
SAFE_PHASES = {
    "prepare", "builder", "cluster", "controllers", "fixtures", "readiness",
    "native_tests", "complete", "stopped",
}


class StepFailed(RuntimeError):
    def __init__(self, step, code=1):
        super().__init__(step)
        self.step, self.code = step, code


def regular_path(path, directory=False):
    path = Path(os.path.abspath(path))
    for component in [*reversed(path.parents), path]:
        if component.is_symlink():
            raise ValueError("Symlink in owned path")
    if path.exists():
        info = path.stat()
        if directory and not stat.S_ISDIR(info.st_mode):
            raise ValueError("Expected directory")
        if not directory and (not stat.S_ISREG(info.st_mode) or info.st_nlink != 1):
            raise ValueError("Expected private regular file")
    return path


def write_private(path, value):
    path = regular_path(path)
    fd = os.open(path, os.O_WRONLY | os.O_CREAT | os.O_TRUNC | os.O_NOFOLLOW, 0o600)
    with os.fdopen(fd, "wb") as writer:
        os.fchmod(writer.fileno(), 0o600)
        writer.write(value)


def read_private(path, maximum=16384):
    path = regular_path(path)
    fd = os.open(path, os.O_RDONLY | os.O_NOFOLLOW)
    with os.fdopen(fd, "rb") as reader:
        info = os.fstat(reader.fileno())
        if not stat.S_ISREG(info.st_mode) or info.st_nlink != 1 or info.st_size > maximum:
            raise ValueError("Invalid private ownership file")
        data = reader.read(maximum + 1)
        if len(data) > maximum:
            raise ValueError("Oversized ownership file")
        return data


def load_module(name, path):
    spec = importlib.util.spec_from_file_location(name, path)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def verified_download(name):
    url, digest = DOWNLOADS[name]
    with urllib.request.urlopen(url, timeout=60) as response:
        data = response.read(2 * 1024 * 1024 + 1)
    if len(data) > 2 * 1024 * 1024 or hashlib.sha256(data).hexdigest() != digest:
        raise ValueError("Pinned controller manifest checksum mismatch")
    return data


def identity(environment):
    result = {}
    for key in ("GITHUB_REPOSITORY", "GITHUB_RUN_ID", "GITHUB_RUN_ATTEMPT", "GITHUB_JOB"):
        value = environment.get(key, "")
        if value and not re.fullmatch(r"[A-Za-z0-9_./-]{1,180}", value):
            raise ValueError("Invalid CI identity")
        result[key] = value
    return result


def read_state(path, workspace, environment):
    state = json.loads(read_private(path))
    if state.get("schema") != 1 or state.get("workspace") != str(workspace):
        raise ValueError("Different workspace owner")
    if state.get("ci") != identity(environment):
        raise ValueError("Different CI run owner")
    if not re.fullmatch(r"[a-f0-9]{32}", state.get("owner", "")):
        raise ValueError("Invalid run owner")
    scratch = regular_path(state["scratch"], directory=True)
    if json.loads(read_private(scratch / "owner.json")) != state:
        raise ValueError("Scratch ownership does not match")
    return state


def validate_container(actual, expected, owner):
    # Never stop by a mutable/reused name: inspect the ID and all authority.
    if not re.fullmatch(r"[a-f0-9]{64}", actual.get("Id", "")):
        raise ValueError("Invalid immutable container ID")
    if expected.get("id") and actual["Id"] != expected["id"]:
        raise ValueError("Container ID changed")
    config = actual.get("Config", {})
    labels = config.get("Labels") or {}
    if (actual.get("Name") != "/" + expected["name"] or config.get("Image") != expected["image"]
            or labels.get(OWNER_LABEL) != owner or labels.get(ROLE_LABEL) != expected["role"]):
        raise ValueError("Container ownership differs")
    host = actual.get("HostConfig", {})
    if host.get("PortBindings") or host.get("NetworkMode") != expected["network"]:
        raise ValueError("Unexpected exposed ports or network")
    if bool(host.get("Privileged")) != (expected["role"] == "cluster"):
        raise ValueError("Unexpected privilege")
    mounts = [(item.get("Source"), item.get("Destination"), item.get("RW"))
              for item in actual.get("Mounts", []) if item.get("Type") == "bind"]
    if sorted(mounts) != sorted(tuple(item) for item in expected["mounts"]):
        raise ValueError("Unexpected source mounts")
    return actual["Id"]


def scale_receipts(output):
    result = []
    for line in output.splitlines():
        if not line.startswith("CC_KUBE_SCALE "):
            continue
        raw = json.loads(line.removeprefix("CC_KUBE_SCALE "))
        keys = ("sample", "pods", "pages", "page_limit", "cold_first_table_ms",
                "cold_full_table_ms", "decoded_table_bytes")
        item = {key: raw[key] for key in keys}
        if any(type(value) is not int or not 0 <= value <= 32 * 1024 * 1024 for value in item.values()):
            raise ValueError("Invalid measurement")
        if item["pods"] != 1000 or item["pages"] != 5 or item["cold_first_table_ms"] > 2000:
            raise ValueError("Scale acceptance target was not met")
        result.append(item)
    if len(result) != 3 or [item["sample"] for item in result] != [1, 2, 3]:
        raise ValueError("Missing native scale samples")
    return result


class Acceptance:
    def __init__(self, workspace, environment=os.environ, state=None):
        self.workspace = regular_path(workspace, directory=True)
        self.environment = dict(environment)
        self.output = self.workspace / "dist/linux/acceptance"
        regular_path(self.output, directory=True)
        self.output.mkdir(parents=True, exist_ok=True)
        self.state_file = regular_path(self.workspace / "dist" / STATE_NAME)
        self.sequence = 0
        if state is None:
            if self.state_file.exists():
                raise ValueError("Existing ownership receipt: stop/review the previous run first")
            parent = regular_path(environment.get("RUNNER_TEMP") or tempfile.gettempdir(), directory=True)
            self.scratch = Path(tempfile.mkdtemp(prefix="consolecrypt-kube-ci-", dir=parent))
            self.scratch.chmod(0o700)
            self.state = {"schema": 1, "owner": uuid.uuid4().hex, "workspace": str(self.workspace),
                          "scratch": str(self.scratch), "ci": identity(environment), "containers": []}
            self.save()
        else:
            self.state = state
            self.scratch = regular_path(state["scratch"], directory=True)
            self.sequence = max((int(path.name.split("-", 1)[0])
                                 for path in self.scratch.glob("[0-9][0-9][0-9]-*.log")), default=0)
        self.phase = "prepare"
        self.receipt = {"schema": 1, "success": False, "phase": self.phase, "tests": [],
                        "scale_samples": [], "transport": "owned_container_loopback",
                        "fixture": {"k3s": "v1.32.4-k3s1", "flux": "v2.6.4", "argo_crd": "v3.1.9"},
                        "raw_diagnostics_published": False}

    def save(self):
        value = (json.dumps(self.state, sort_keys=True) + "\n").encode()
        write_private(self.scratch / "owner.json", value)
        write_private(self.state_file, value)

    def run(self, step, args, data=None, capture=False, timeout=180):
        self.sequence += 1
        log = self.scratch / f"{self.sequence:03}-{step}.log"
        with log.open("wb") as writer:
            result = subprocess.run(args, input=data, stdout=subprocess.PIPE if capture else writer,
                                    stderr=writer, env=self.environment, timeout=timeout)
        log.chmod(0o600)
        if capture and step.startswith("test-"):
            # Keep failed test stdout too; the published receipt never includes
            # this raw diagnostic. Kubeconfig output is only saved separately.
            write_private(self.scratch / f"{self.sequence:03}-{step}.stdout.log", result.stdout)
        if result.returncode:
            raise StepFailed(step, result.returncode)
        return result.stdout if capture else b""

    def docker(self, step, *args, **kwargs):
        return self.run(step, ["docker", *args], **kwargs)

    def advance(self, phase):
        if phase not in SAFE_PHASES:
            raise ValueError("Unknown acceptance phase")
        self.phase = phase
        print("KUBERNETES_CI " + phase, flush=True)

    def create(self, role, image, network, command, mounts=(), options=()):
        name = "cc-kube-ci-" + self.state["owner"][:16] + "-" + role
        expected = {"role": role, "name": name, "image": image, "network": network,
                    "mounts": [list(item) for item in mounts], "id": None}
        self.state["containers"].append(expected)
        self.save()
        args = ["container", "create", "--name", name, "--network", network,
                "--label", OWNER_LABEL + "=" + self.state["owner"],
                "--label", ROLE_LABEL + "=" + role, *options]
        for source, destination, writable in mounts:
            regular_path(source, directory=True)
            mount = f"type=bind,source={source},target={destination}"
            args.extend(["--mount", mount + ("" if writable else ",readonly")])
        container = self.docker("create-" + role, *args, image, *command, capture=True).decode().strip()
        if not re.fullmatch(r"[a-f0-9]{64}", container):
            raise ValueError("Unexpected Docker create output")
        expected["id"] = container
        self.save()
        self.docker("start-" + role, "container", "start", container)
        return container

    def kubectl(self, *args, data=None, capture=False, timeout=180):
        return self.docker("kubectl", "exec", "-i", self.cluster, "kubectl", *args,
                           data=data, capture=capture, timeout=timeout)

    def apply(self, objects):
        self.kubectl("apply", "--server-side", "--field-manager=consolecrypt-acceptance", "-f", "-",
                     data=json.dumps({"apiVersion": "v1", "kind": "List", "items": objects}).encode())

    def wait(self, step, check, seconds=180):
        deadline = time.monotonic() + seconds
        while True:
            try:
                if check():
                    return
            except (StepFailed, subprocess.TimeoutExpired):
                pass
            if time.monotonic() >= deadline:
                raise StepFailed(step)
            time.sleep(2)

    def setup(self):
        self.run("docker-info", ["docker", "info"], timeout=30)
        stager = load_module("kube_ci_stager", self.workspace / "client/scripts/stage-linux-source.py")
        stager.stage(self.workspace, self.scratch / "source")
        self.fixtures = load_module("kube_ci_fixtures", self.workspace / "client/ci/kubernetes/fixtures.py")
        self.advance("builder")
        self.docker("build", "build", "--platform", "linux/amd64", "-t", BUILDER,
                    str(self.workspace / "client/ci/linux"), timeout=2400)
        network_name = "cc-kube-ci-" + self.state["owner"][:16]
        self.state["network"] = network_name
        self.save()
        network_id = self.docker("network", "network", "create", "--label",
                                 OWNER_LABEL + "=" + self.state["owner"], network_name, capture=True).decode().strip()
        if not re.fullmatch(r"[a-f0-9]{64}", network_id):
            raise ValueError("Unexpected network ID")
        self.advance("cluster")
        self.cluster = self.create("cluster", K3S, network_name, [
            "server", "--disable=traefik,servicelb,local-storage", "--tls-san=cc-kube-acceptance.local",
            "--cluster-cidr=10.242.0.0/16", "--service-cidr=10.243.0.0/16",
            "--write-kubeconfig-mode=600",
        ], options=("--privileged", "--cpus=4", "--memory=4g", "--tmpfs=/run", "--tmpfs=/var/run"))
        self.wait("cluster-ready", lambda: self.kubectl("get", "--raw=/readyz", capture=True).strip() == b"ok", 240)
        self.kubectl("wait", "--for=condition=Ready", "node", "--all", "--timeout=180s", timeout=190)
        address = self.docker("node-ip", "inspect", "--format",
                              "{{range .NetworkSettings.Networks}}{{.IPAddress}}{{end}}",
                              self.cluster, capture=True).decode().strip()
        ipaddress.IPv4Address(address)
        runtime = self.scratch / "runtime"
        runtime.mkdir(mode=0o700)
        kubeconfig = self.docker("kubeconfig", "exec", self.cluster, "cat",
                                 "/etc/rancher/k3s/k3s.yaml", capture=True)
        write_private(runtime / "config.yaml", kubeconfig)
        shared_network = "container:" + self.cluster
        self.git = self.create("git", BUILDER, shared_network,
                               ["python3", "/fixtures/fixtures.py", "serve-git"],
                               mounts=((str(self.workspace / "client/ci/kubernetes"), "/fixtures", False),),
                               options=("--cpus=1", "--memory=256m"))
        self.registry = self.create("registry", REGISTRY, shared_network, [],
                                    options=("--cpus=1", "--memory=256m"))
        self.wait("registry-ready", lambda: self.docker(
            "registry-ready", "exec", self.git, "python3", "-c",
            "import urllib.request; urllib.request.urlopen('http://127.0.0.1:5000/v2/', timeout=5).read()",
        ) == b"")
        self.docker("seed-oci", "exec", self.git, "python3", "/fixtures/fixtures.py", "seed-oci")
        self.advance("controllers")
        for name in ("flux", "argo"):
            manifest = verified_download(name)
            write_private(self.scratch / (name + "-pinned.yaml"), manifest)
            self.kubectl("apply", "--server-side", "--field-manager=consolecrypt-acceptance", "-f", "-", data=manifest)
        self.kubectl("wait", "--for=condition=Established", "crd/applications.argoproj.io", "--timeout=180s", timeout=190)
        for controller in ("source-controller", "kustomize-controller", "helm-controller"):
            self.kubectl("-n", "flux-system", "rollout", "status", "deployment/" + controller,
                         "--timeout=300s", timeout=310)
        self.advance("fixtures")
        self.apply(self.fixtures.base_objects(address))
        self.apply(self.fixtures.flux_objects())
        self.apply(self.fixtures.scale_objects())
        self.advance("readiness")
        for resource in ("gitrepository/fixture-source", "gitrepository/fixture-local-git",
                         "ocirepository/fixture-local-oci", "kustomization/fixture-ready",
                         "kustomization/fixture-local-ready", "kustomization/fixture-oci-ready",
                         "helmrelease/fixture-helm"):
            self.kubectl("-n", "cc-acceptance", "wait", "--for=condition=Ready", resource,
                         "--timeout=300s", timeout=310)
        self.kubectl("-n", "cc-acceptance", "wait", "--for=condition=Ready", "pod/fixture-shell",
                     "--timeout=180s", timeout=190)
        self.kubectl("-n", "cc-acceptance", "wait", "--for=condition=Ready=false",
                     "gitrepository/fixture-broken-source", "kustomization/fixture-blocked",
                     "--timeout=90s", timeout=100)
        self.wait("metrics-ready", lambda: bool(json.loads(self.kubectl(
            "get", "--raw=/apis/metrics.k8s.io/v1beta1/nodes", capture=True))["items"]), 180)
        # All fixture controllers are stable before measuring or issuing writes.
        time.sleep(2)
        self.runner = self.create("runner", BUILDER, shared_network, ["sleep", "infinity"],
                                  mounts=((str(self.scratch / "source"), "/work", True),
                                          (str(runtime), "/fixture-runtime", False)),
                                  options=("--platform=linux/amd64", "--cpus=4", "--memory=6g",
                                           "--env=CARGO_BUILD_JOBS=2"))

    def execute(self):
        self.advance("native_tests")
        for name in TESTS:
            started = time.monotonic()
            output = self.docker("test-" + name, "exec",
                                 "-e", "CC_KUBE_ACCEPTANCE_CONFIG=/fixture-runtime/config.yaml",
                                 "-e", "CC_KUBE_ACCEPTANCE_LOOPBACK=127.0.0.1:6443",
                                 self.runner, "cargo", "test", "--manifest-path", "client/rust/Cargo.toml",
                                 "--locked", "-p", "cc-k8s-core", "--test", "resources_live",
                                 name, "--", "--ignored", "--exact", "--nocapture",
                                 capture=True, timeout=2400).decode("utf-8", errors="replace")
            write_private(self.scratch / (name + ".log"), output.encode())
            if f"test {name} ... ok" not in output or "test result: ok. 1 passed;" not in output:
                raise StepFailed("native-test-receipt")
            # Rust test diagnostics use stderr; capture their separate private
            # file as well, while retaining only whitelisted timings publicly.
            if name == TESTS[-1]:
                stderr = (self.scratch / f"{self.sequence:03}-test-{name}.log").read_text(errors="replace")
                self.receipt["scale_samples"] = scale_receipts(output + "\n" + stderr)
            self.receipt["tests"].append({"name": name, "passed": True,
                                         "elapsed_seconds": round(time.monotonic() - started, 3)})
        self.advance("complete")
        self.receipt["success"] = True

    def stop(self):
        failures = []
        for expected in reversed(self.state["containers"]):
            try:
                container = expected.get("id")
                if not container:
                    container = self.docker("locate", "container", "ls", "--all", "--no-trunc",
                                            "--filter", "name=^/" + expected["name"] + "$",
                                            "--format", "{{.ID}}", capture=True).decode().strip()
                    if not container:
                        continue
                info = self.docker("inspect", "container", "inspect", "--format",
                                   '{"Id":{{json .Id}},"Name":{{json .Name}},"Config":{"Image":{{json .Config.Image}},"Labels":{{json .Config.Labels}}},"HostConfig":{"PortBindings":{{json .HostConfig.PortBindings}},"NetworkMode":{{json .HostConfig.NetworkMode}},"Privileged":{{json .HostConfig.Privileged}}},"Mounts":{{json .Mounts}}}',
                                   container, capture=True)
                immutable = validate_container(json.loads(info), expected, self.state["owner"])
                try:
                    self.docker("logs-" + expected["role"], "logs", "--tail=300", immutable, timeout=30)
                except (OSError, StepFailed, subprocess.SubprocessError):
                    # An unsupported log driver must not leave owned compute
                    # running. Identity was already validated above.
                    failures.append(expected["role"])
                self.docker("stop-" + expected["role"], "container", "stop", "--time=10", immutable, timeout=30)
            except (OSError, ValueError, StepFailed, subprocess.SubprocessError):
                failures.append(expected["role"])
        if failures:
            raise StepFailed("owned-container-stop")

    def publish_receipt(self):
        self.receipt["phase"] = self.phase
        data = json.dumps(self.receipt, indent=2, sort_keys=True) + "\n"
        write_private(self.output / "kubernetes-acceptance.json", data.encode())
        lines = ["Kubernetes CI acceptance: " + ("PASS" if self.receipt["success"] else "FAIL"),
                 "Phase: " + self.phase, "Fixtures: k3s v1.32.4-k3s1, Flux v2.6.4, Argo Application CRD v3.1.9",
                 "No production credentials or raw cluster output are included."]
        for test in self.receipt["tests"]:
            lines.append(f"PASS {test['name']} ({test['elapsed_seconds']}s)")
        for sample in self.receipt["scale_samples"]:
            lines.append("SCALE " + json.dumps(sample, sort_keys=True))
        write_private(self.output / "kubernetes-acceptance.log", ("\n".join(lines) + "\n").encode())


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--workspace", type=Path, default=Path(__file__).resolve().parents[2])
    parser.add_argument("--stop", action="store_true", help="Stop only resources owned by this exact CI run")
    args = parser.parse_args()
    workspace = regular_path(args.workspace, directory=True)
    state_path = workspace / "dist" / STATE_NAME
    if args.stop:
        if not state_path.exists():
            return
        task = Acceptance(workspace, state=read_state(state_path, workspace, os.environ))
        task.stop()
        print("KUBERNETES_CI owned containers stopped; scratch retained")
        return
    task = Acceptance(workspace)
    def interrupted(_signal, _frame):
        raise StepFailed("interrupted")
    signal.signal(signal.SIGTERM, interrupted)
    signal.signal(signal.SIGINT, interrupted)
    failure = None
    try:
        task.setup()
        task.execute()
    except (OSError, ValueError, RuntimeError, subprocess.SubprocessError) as error:
        failure = error
        task.receipt["success"] = False
    finally:
        try:
            task.stop()
        except (OSError, ValueError, RuntimeError, subprocess.SubprocessError) as error:
            failure = failure or error
            task.receipt["success"] = False
        task.publish_receipt()
    if failure:
        print("KUBERNETES_CI failed in " + task.phase + "; private scratch retained.", file=sys.stderr)
        raise SystemExit(1)
    print("KUBERNETES_CI PASS; private scratch retained until release cleanup")


if __name__ == "__main__":
    try:
        main()
    except (OSError, ValueError, RuntimeError, subprocess.SubprocessError):
        print("KUBERNETES_CI setup or owned cleanup refused; no unowned resources changed.", file=sys.stderr)
        raise SystemExit(1)
