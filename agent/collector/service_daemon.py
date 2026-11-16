"""Original installed child owner. No policy, key, grant, or payload queue here."""
import os
from pathlib import Path
import signal
import subprocess
import time

stopping = False


def stop_requested(*_):
    global stopping
    stopping = True


def run_verified(binary):
    """Called only by the SAME verified stable launcher, never a caller argv."""
    if (os.name != "posix" or os.getuid() == 0 or type(binary) is not type(Path("/"))
            or binary.name != "cc-agent-identity"
            or binary.parent.parent.parent.parent != Path("/opt/consolecrypt-agent-install/opt/consolecrypt-agent")):
        raise ValueError("service_fixed_binary")
    signal.signal(signal.SIGTERM, stop_requested)
    signal.signal(signal.SIGINT, stop_requested)
    child = subprocess.Popen([str(binary), "worker-collector-service-sqlite"],
                             stdin=subprocess.PIPE, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
                             env={"LANG": "C.UTF-8"}, cwd="/", close_fds=True)
    try:
        while child.poll() is None and not stopping:
            time.sleep(.05)
        if child.poll() is not None:
            return child.wait()
        # Original pipe EOF asks the exact worker to stop/join its owned unit;
        # it is not targeted signed revocation and cannot renew any authority.
        child.stdin.close()
        try:
            return child.wait(timeout=12)
        except subprocess.TimeoutExpired:
            child.kill()
            child.wait(timeout=2)
            raise ValueError("service_worker_cleanup_unknown")
    finally:
        if child.stdin is not None and not child.stdin.closed:
            child.stdin.close()
        if child.poll() is None:
            child.kill()
            child.wait(timeout=2)


if __name__ == "__main__":
    raise SystemExit("service_requires_verified_launcher")
