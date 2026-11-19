"""Private filesystem/PID/network namespaces; ordinary subprocess is not a sandbox."""
from __future__ import annotations

from dataclasses import dataclass
from datetime import datetime, timezone
import os
from pathlib import Path
import selectors
import shutil
import signal
import subprocess
import sys
import threading
import time

from .errors import RunnerError, require


@dataclass(frozen=True)
class ProcessResult:
    exit_code: int
    output: str
    truncated: bool
    cancelled: bool
    timed_out: bool
    elapsed_ms: int
    spawn_observed_utc: str | None = None
    exit_observed_utc: str | None = None

    def validation_fields(self):
        """Preserve the closed validation protocol independently of new metadata."""
        return {"exit_code": self.exit_code, "output": self.output,
                "truncated": self.truncated, "cancelled": self.cancelled,
                "timed_out": self.timed_out, "elapsed_ms": self.elapsed_ms}

    def execution_interval(self):
        """Runner process observations, never proof that target changes ended.

        No defaults are inferred from acceptance, state refresh or guest output.
        Wall-clock reversal is preserved for the UI clock-skew warning; elapsed
        time is independently measured with the monotonic clock.
        """
        if self.spawn_observed_utc is None or self.exit_observed_utc is None:
            return None
        return {"version": 1, "clockSource": "runner-wall", "scope": "runner-process",
                "spawnObservedUtc": self.spawn_observed_utc,
                "exitObservedUtc": self.exit_observed_utc, "elapsedMonotonicMs": self.elapsed_ms}


def bubblewrap_argv(workspace: Path, command: list[str], *, extra_readonly=(), socket_directory=None,
                    git_download=False, dependency_download=False, vault_channel=None, job_input=None, validation_input=None):
    require(sys.platform == "linux" and os.getuid() != 0, "sandbox_unavailable", "Linux non-root sandbox required")
    executable = shutil.which("bwrap")
    require(executable is not None, "sandbox_unavailable", "bubblewrap is not installed")
    workspace = workspace.resolve(strict=True)
    require(workspace.is_dir() and not workspace.is_symlink(), "workspace_path", "Invalid job workspace")
    args = [executable, "--die-with-parent", "--new-session", "--unshare-all", "--cap-drop", "ALL", "--clearenv"]
    if git_download or dependency_download:
        trusted = 'consolecrypt_runner.git_worker' if git_download else 'consolecrypt_runner.dependency_worker'
        require(not (git_download and dependency_download) and command==[sys.executable,'-m',trusted] and socket_directory is None and vault_channel is None and job_input is None and validation_input is None,
                'download_sandbox','Network access is only for a trusted download worker')
        args+=['--share-net']
        # No user home/config/agent/target socket. Only system DNS and trust roots.
        for name in ('/etc/resolv.conf','/etc/hosts','/etc/nsswitch.conf','/etc/ssl/certs',
                     '/usr/share/ca-certificates','/usr/libexec'):
            if Path(name).exists():args+=['--ro-bind',name,name]
    # /usr/local may contain unrelated service state. Expose the runtime,
    # never an entire installation prefix merely because it is read-only.
    for path in ("/usr/bin", "/usr/sbin", "/usr/lib", "/usr/lib64", "/lib", "/lib64", "/bin",
                 "/usr/share/zoneinfo", "/usr/share/locale", "/usr/share/terminfo"):
        if Path(path).exists():
            args += ["--ro-bind", path, path]
    prefix = Path(sys.prefix).resolve()
    if not prefix.is_relative_to(Path("/usr")):
        for name in ("bin", "lib", "lib64", "pyvenv.cfg"):
            path = prefix / name
            if path.exists():
                args += ["--ro-bind", str(path), str(path)]
    for source, target in extra_readonly:
        source = Path(source).resolve(strict=True)
        require(target.startswith("/tool/"), "sandbox_mount", "Invalid trusted tool mount")
        args += ["--ro-bind", str(source), target]
    args += ["--ro-bind", str(Path(__file__).resolve().parent.parent), "/tool/python",
             "--proc", "/proc", "--dev", "/dev", "--size", str(64 * 1024 * 1024), "--tmpfs", "/tmp",
             "--size", str((384 if git_download else 256) * 1024 * 1024), "--tmpfs", "/work",
             "--ro-bind", str(workspace), "/project", "--chdir", "/work"]
    if socket_directory is not None:
        socket_directory = Path(socket_directory).resolve(strict=True)
        require(socket_directory.is_dir(), "sandbox_mount", "Invalid job transport directory")
        args += ["--ro-bind", str(socket_directory), "/run/consolecrypt"]
    require(job_input is None or validation_input is None, 'job_input', 'Ambiguous private input')
    if job_input is not None:
        from .ephemeral_input import EphemeralJobInput, TARGET
        require(type(job_input) is EphemeralJobInput and
                all(target != TARGET for _, target in extra_readonly),
                'job_input', 'Original private job input required')
        for descriptor, target in job_input.mounts():
            args += ['--ro-bind-data', str(descriptor), target]
    if validation_input is not None:
        from .ephemeral_input import EphemeralValidationInput, VALIDATION_TARGET
        require(type(validation_input) is EphemeralValidationInput and
                all(target != VALIDATION_TARGET for _, target in extra_readonly),
                'validation_inputs', 'Original private validation input required')
        for descriptor, target in validation_input.mounts():
            args += ['--ro-bind-data', str(descriptor), target]
    if vault_channel is not None:
        from .vault_sidecar import PinnedVaultSidecar
        require(isinstance(vault_channel,PinnedVaultSidecar) and (socket_directory is None or vault_channel.phase=='execution'),
                'vault_mount','Dedicated Vault channel required')
        # Opaque original FD custody, never Path.resolve() into ambient names.
        for source,target in vault_channel.mounts():
            args += ['--ro-bind-fd',str(source),target]
    for name, value in {
        "PATH": f"{sys.prefix}/bin:/usr/bin:/bin", "HOME": "/work/home", "LANG": "C.UTF-8",
        "LC_ALL": "C.UTF-8", "PYTHONDONTWRITEBYTECODE": "1", "PYTHONNOUSERSITE": "1",
        "ANSIBLE_HOME": "/work/.ansible", "ANSIBLE_LOCAL_TEMP": "/work/.ansible/tmp",
        "XDG_CACHE_HOME": "/work/.cache", "ANSIBLE_HOST_KEY_CHECKING": "True",
        "PYTHONPATH": "/tool/python", "ANSIBLE_INVENTORY_ENABLED": "host_list,yaml,ini",
        "ANSIBLE_GALAXY_SERVER_LIST": "", "ANSIBLE_FORCE_COLOR": "False",
    }.items():
        args += ["--setenv", name, value]
    return args + ["--", sys.executable, "-m", "consolecrypt_runner.child", "--", *command]


def run_bounded(argv: list[str], *, timeout=120, cancel: threading.Event | None = None,
                output_limit=1024 * 1024, grace=15, environment=None, on_chunk=None, vault_channel=None,
                job_input=None, validation_input=None, input_bytes=None) -> ProcessResult:
    require(os.name == "posix", "unsupported_backend", "Runner process control requires Unix")
    require(0 < timeout <= 86400 and 0 <= output_limit <= 50 * 1024 * 1024,
            "process_limit", "Invalid process limits")
    start = time.monotonic()
    deadline = start + timeout
    cancelled = timed_out = truncated = False
    output = bytearray()
    require(job_input is None or validation_input is None, 'job_input', 'Ambiguous private input')
    require(input_bytes is None or (type(input_bytes) is bytes and len(input_bytes) <= 6 * 1024 * 1024
            and job_input is None and validation_input is None and vault_channel is None),
            'job_input', 'Invalid private pipe input')
    descriptor_options = {}
    inherited = []
    if vault_channel is not None:
        from .vault_sidecar import PinnedVaultSidecar
        require(isinstance(vault_channel, PinnedVaultSidecar), 'vault_mount', 'Original Vault FD custody required')
        for descriptor, target in vault_channel.mounts():
            expected=['--ro-bind-fd',str(descriptor),target]
            require(any(argv[i:i+3]==expected for i in range(len(argv)-2)),
                    'vault_mount','Fixed Vault descriptor mount changed')
        inherited.extend(vault_channel.inherited_fds())
    if job_input is not None:
        from .ephemeral_input import EphemeralJobInput, TARGET
        require(type(job_input) is EphemeralJobInput, 'job_input', 'Original private job input required')
        for descriptor, target in job_input.mounts():
            expected = ['--ro-bind-data', str(descriptor), target]
            require(sum(argv[i:i+3] == expected for i in range(len(argv)-2)) == 1 and
                    argv.count(TARGET) == 1, 'job_input', 'Private job input mount changed')
        inherited.extend(job_input.prepare_for_spawn())
    if validation_input is not None:
        from .ephemeral_input import EphemeralValidationInput, VALIDATION_TARGET
        require(type(validation_input) is EphemeralValidationInput,
                'validation_inputs', 'Original private validation input required')
        for descriptor, target in validation_input.mounts():
            expected = ['--ro-bind-data', str(descriptor), target]
            require(sum(argv[i:i+3] == expected for i in range(len(argv)-2)) == 1 and
                    argv.count(VALIDATION_TARGET) == 1,
                    'validation_inputs', 'Private validation input mount changed')
        inherited.extend(validation_input.prepare_for_spawn())
    if inherited:
        require(len(inherited) == len(set(inherited)), 'job_input', 'Duplicate private setup descriptor')
        descriptor_options['pass_fds'] = tuple(inherited)
    process = subprocess.Popen(argv, stdin=subprocess.DEVNULL if input_bytes is None else subprocess.PIPE,
                               stdout=subprocess.PIPE, stderr=subprocess.STDOUT,
                               start_new_session=True, close_fds=True, env=environment, **descriptor_options)
    spawn_observed_utc = datetime.now(timezone.utc).isoformat().replace("+00:00", "Z")
    terminate_at = None
    with selectors.DefaultSelector() as selector:
        os.set_blocking(process.stdout.fileno(), False)
        selector.register(process.stdout, selectors.EVENT_READ)
        try:
            if process.stdin is not None:
                os.set_blocking(process.stdin.fileno(), False)
                selector.register(process.stdin, selectors.EVENT_WRITE)
                remaining = memoryview(input_bytes)
            while selector.get_map() or process.poll() is None:
                now = time.monotonic()
                if terminate_at is None and ((cancel and cancel.is_set()) or now >= deadline):
                    cancelled = bool(cancel and cancel.is_set())
                    timed_out = not cancelled
                    terminate_at = now + grace
                    try:
                        os.killpg(process.pid, signal.SIGTERM)
                    except ProcessLookupError:
                        pass
                if terminate_at is not None and now >= terminate_at:
                    try:
                        os.killpg(process.pid, signal.SIGKILL)
                    except ProcessLookupError:
                        pass
                    # Kernel PID namespaces kill descendants when their init
                    # exits. Do not wait indefinitely for a corrupt pipe writer.
                    if now >= terminate_at + 2:
                        break
                for key, _ in selector.select(timeout=0.1):
                    if key.fileobj is process.stdin:
                        try:
                            if remaining:
                                remaining = remaining[os.write(key.fd, remaining[:65536]):]
                        except BlockingIOError:
                            continue
                        except BrokenPipeError:
                            remaining = remaining[:0]
                        if not remaining:
                            selector.unregister(key.fileobj)
                            process.stdin.close()
                        continue
                    chunk = os.read(key.fileobj.fileno(), 65536)
                    if not chunk:
                        selector.unregister(key.fileobj)
                        continue
                    if on_chunk is not None:
                        on_chunk(chunk)
                    available = output_limit - len(output)
                    output.extend(chunk[:max(0, available)])
                    truncated |= len(chunk) > available
            if process.poll() is None:
                os.killpg(process.pid, signal.SIGKILL)
            code = process.wait(timeout=5)
        finally:
            if process.poll() is None:
                try:
                    os.killpg(process.pid, signal.SIGKILL)
                except ProcessLookupError:
                    pass
                process.wait(timeout=5)
            process.stdout.close()
            if process.stdin is not None:
                process.stdin.close()
    exit_observed_utc = datetime.now(timezone.utc).isoformat().replace("+00:00", "Z")
    return ProcessResult(code, output.decode("utf-8", errors="replace"), truncated, cancelled, timed_out,
                         int((time.monotonic() - start) * 1000), spawn_observed_utc, exit_observed_utc)


def probe_sandbox():
    if sys.platform == "darwin":
        from .macos_sandbox import probe_macos_sandbox
        return probe_macos_sandbox()
    if sys.platform != "linux" or os.getuid() == 0 or not shutil.which("bwrap"):
        return {"filesystem": False, "pid": False, "network": False, "reason": "sandbox_unavailable"}
    import tempfile
    with tempfile.TemporaryDirectory(prefix="cc-sandbox-probe-") as temporary:
        try:
            argv = bubblewrap_argv(Path(temporary), ["/usr/bin/true"])
            result = run_bounded(argv, timeout=5, output_limit=4096, grace=1)
            supported = result.exit_code == 0
            return {"filesystem": supported, "pid": supported, "network": supported,
                    "reason": None if supported else "namespace_unavailable"}
        except (OSError, RunnerError):
            return {"filesystem": False, "pid": False, "network": False, "reason": "sandbox_unavailable"}
