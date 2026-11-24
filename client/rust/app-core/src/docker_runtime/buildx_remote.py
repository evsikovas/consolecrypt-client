"""Run on the selected SSH host. stdin stays open for the lifetime of the build."""
import json
import os
from pathlib import Path
import re
import select
import shutil
import signal
import subprocess
import sys
import tempfile
import time


def run():
    os.umask(0o077)
    line = sys.stdin.buffer.readline(262145)
    if len(line) > 262144 or not line.endswith(b"\n"):
        return 2
    request = json.loads(line)
    project = Path(request["directory"]).resolve()
    if not project.is_dir() or not (project / "Dockerfile").is_file():
        return 2
    docker = shutil.which("docker")
    if not docker:
        return 3
    # dial-stdio follows the account's CLI context. Resolve it before replacing
    # DOCKER_CONFIG so the build uses the same engine as the container list.
    endpoint = request["endpoint"]
    if not endpoint:
        endpoint = os.environ.get("DOCKER_HOST") if not os.environ.get("DOCKER_CONTEXT") else None
        if not endpoint:
            context = subprocess.run(
                [docker, "context", "inspect", "--format", "{{.Endpoints.docker.Host}}"],
                stdout=subprocess.PIPE, stderr=subprocess.DEVNULL, universal_newlines=True,
                timeout=15, check=True,
            )
            endpoint = context.stdout.strip()
    if not endpoint.startswith("unix:///"):
        return 3
    with tempfile.TemporaryDirectory(prefix="cc-buildx-") as directory:
        root = Path(directory).resolve()
        if project == root or project in root.parents:
            return 2
        env = os.environ.copy()
        original = Path(env.get("DOCKER_CONFIG") or Path.home() / ".docker")
        for key in ("DOCKER_CONTEXT", "DOCKER_HOST", "DOCKER_AUTH_CONFIG", "DOCKER_TLS_VERIFY",
                    "DOCKER_CERT_PATH", "BUILDX_BUILDER", "BUILDKIT_HOST"):
            env.pop(key, None)
        env["DOCKER_CONFIG"] = str(root)
        env["BUILDX_METADATA_PROVENANCE"] = "disabled"
        config = {"auths": {request["registry"]: {"auth": request["auth"]}}}
        if original.is_absolute():
            config["cliPluginsExtraDirs"] = [str(original / "cli-plugins")]
            if (original / "buildx").is_dir():
                env.setdefault("BUILDX_CONFIG", str(original / "buildx"))
        with (root / "config.json").open("x") as file:
            json.dump(config, file)
        cli = [docker, "--host", endpoint, "buildx"]
        if subprocess.run(cli + ["version"], env=env, stdin=subprocess.DEVNULL,
                          stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, timeout=15).returncode:
            return 3
        meta = root / "result.json"
        command = cli + ["build", "--platform", ",".join(request["platforms"]), "-t", request["image"],
                         "--push", "--metadata-file", str(meta), str(project)]
        process = subprocess.Popen(command, env=env, stdin=subprocess.DEVNULL, start_new_session=True,
                                   stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        try:
            deadline = time.monotonic() + 3600
            while process.poll() is None:
                if time.monotonic() >= deadline or select.select([sys.stdin], [], [], 0.2)[0]:
                    return 1
            if process.returncode:
                return 1
        finally:
            if process.poll() is None:
                try:
                    os.killpg(process.pid, signal.SIGKILL)
                except ProcessLookupError:
                    pass
                process.wait()
        if meta.stat().st_size > 1048576:
            return 1
        digest = json.loads(meta.read_text()).get("containerimage.digest", "")
        if not re.fullmatch(r"sha256:[a-f0-9]{64}", digest):
            return 1
        print(digest)
    return 0


def disconnected(*_):
    raise InterruptedError()


if __name__ == "__main__":
    signal.signal(signal.SIGHUP, disconnected)
    signal.signal(signal.SIGTERM, disconnected)
    try:
        sys.exit(run())
    except (OSError, ValueError, KeyError, subprocess.SubprocessError):
        sys.exit(1)
