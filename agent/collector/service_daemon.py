"""Original installed child owner. No policy, key, grant, or payload queue here."""
import os
import json
import sys
from pathlib import Path
import signal
import subprocess
import time

stopping = False


def stop_requested(*_):
    global stopping
    stopping = True


# Closed fieldless Rust Error names from identity/src/lib.rs; never echo input.
_WORKER_ERRORS=frozenset(("InvalidWire","InvalidKey","InvalidSignature","InvalidTime","Expired",
    "WrongContext","WrongEpoch","WrongCheckpoint","UnsupportedTeam","Replay","NotFound","WrongState",
    "InvalidToken","WrongClaim","ScopeDenied","CapabilityDenied","MissingFreshness","Revoked","Quota",
    "Conflict","Storage","Entropy","KeyReuse","UncertainCommit"))

class _WorkerStderr:
    def __init__(self):
        self.raw=bytearray();self.overflow=False

    def accept(self,block):
        if self.overflow:return
        if len(self.raw)+len(block)>4096:
            self.raw.clear();self.overflow=True
        else:self.raw.extend(block)

    def drain(self,stream):
        # At most64KiB per original poll: constant storage and no reader thread.
        # Excess bytes are drained/discarded, never persisted or logged.
        for _ in range(16):
            try:block=os.read(stream.fileno(),4096)
            except BlockingIOError:return
            except OSError:
                self.raw.clear();self.overflow=True;return
            if not block:return
            self.accept(block)

    def code(self):
        if self.overflow or not self.raw:return "unknown"
        def unique(pairs):
            result={}
            for key,value in pairs:
                if key in result:raise ValueError("duplicate")
                result[key]=value
            return result
        try:
            value=json.loads(self.raw,object_pairs_hook=unique)
            if (type(value)is dict and set(value)=={"event","code"}
                    and value["event"]=="identityDenied" and type(value["code"])is str
                    and value["code"]in _WORKER_ERRORS):return value["code"]
        except (ValueError,UnicodeError,TypeError,RecursionError):pass
        return "unknown"

    def report(self,child):
        # Only static classification and original OS identities/status leave here.
        try:
            print("CC_AGENT_COLLECTOR_WORKER_EXIT "+json.dumps({"errorCode":self.code(),
                "workerExit":child.returncode,"workerPid":child.pid,"parentPid":os.getpid()},
                sort_keys=True,separators=(",",":")),file=sys.stderr,flush=True)
        except OSError:pass # Diagnostic sink failure cannot change worker status.


def run_verified(binary):
    """Called only by the SAME verified stable launcher, never a caller argv."""
    if (os.name != "posix" or os.getuid() == 0 or type(binary) is not type(Path("/"))
            or binary.name != "cc-agent-identity"
            or binary.parent.parent.parent.parent != Path("/opt/consolecrypt-agent-install/opt/consolecrypt-agent")):
        raise ValueError("service_fixed_binary")
    signal.signal(signal.SIGTERM, stop_requested)
    signal.signal(signal.SIGINT, stop_requested)
    child = subprocess.Popen([str(binary), "worker-collector-service-sqlite"],
                             stdin=subprocess.PIPE, stdout=subprocess.DEVNULL, stderr=subprocess.PIPE,
                             env={"LANG": "C.UTF-8"}, cwd="/", close_fds=True)
    observed=_WorkerStderr()
    try:
        os.set_blocking(child.stderr.fileno(),False)
        while child.poll() is None and not stopping:
            observed.drain(child.stderr)
            time.sleep(.05)
        if child.poll() is not None:
            return child.wait()
        # Original pipe EOF asks the exact worker to stop/join its owned unit;
        # it is not targeted signed revocation and cannot renew any authority.
        child.stdin.close()
        try:
            end=time.monotonic()+12
            while child.poll() is None:
                observed.drain(child.stderr)
                remaining=end-time.monotonic()
                if remaining<=0:raise subprocess.TimeoutExpired(child.args,12)
                time.sleep(min(.05,remaining))
            return child.wait()
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
        try:
            observed.drain(child.stderr)
            observed.report(child)
        finally:child.stderr.close()


if __name__ == "__main__":
    raise SystemExit("service_requires_verified_launcher")
