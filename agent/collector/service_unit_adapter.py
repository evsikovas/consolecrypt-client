"""Specific finite service adapter. Same actual OwnedUnit/pidfd/cgroup custody."""
import hashlib
import json
import os
from pathlib import Path
import re
import selectors
import signal
import stat
import sys
import time
PACKAGE=Path("/opt/consolecrypt-agent-install/opt/consolecrypt-agent/collector")
for _p in (*reversed(PACKAGE.parents),PACKAGE):
    _s=_p.lstat()
    if not stat.S_ISDIR(_s.st_mode)or stat.S_ISLNK(_s.st_mode)or _s.st_uid!=0 or _s.st_mode&0o022:raise ValueError("service_package")
for _n in ("service_unit_entry.py","service_unit_spec.py","service_binding.py","disk_unit_adapter.py","disk_unit_entry.py","disk_unit_spec.py","disk_binding.py"):
    _s=(PACKAGE/_n).lstat()
    if not stat.S_ISREG(_s.st_mode)or _s.st_uid!=0 or _s.st_nlink!=1 or stat.S_IMODE(_s.st_mode)!=0o644:raise ValueError("service_package")
sys.path.insert(0,str(PACKAGE))
from disk_unit_adapter import OwnedUnit,atomic_record,command,identity,need,object_pairs,proc_start,regular,manager_environment
from service_unit_entry import ROOT,BINARY,binding_types,private_config,read_text,_record_start_failure,_failure_no_late_producer
from service_unit_spec import ServiceUnitSpec
cancelled=False


class ServiceOwnedUnit(OwnedUnit):
    def __init__(self, *args):
        super().__init__(*args)
        self.retained_public_files = {}

    def remember_public_file(self, name):
        modes = {"launch-owner.json": 0o400, "preexec.json": 0o600,
                 "prometheus-exec.json": 0o600, "prometheus-exit.json": 0o600, "stop.request": 0o400}
        need(name in modes, "disk_cleanup_leaf")
        fd, _ = regular(self.directory / name, uid=os.getuid(), mode=modes[name], cap=4096)
        try:
            m = os.fstat(fd)
            row = {"name": name, "dev": m.st_dev, "ino": m.st_ino, "mode": modes[name]}
            need(self.retained_public_files.get(name, row) == row, "disk_cleanup_leaf")
            self.retained_public_files[name] = row
        finally:
            os.close(fd)

    def running(self, end):
        result = super().running(end)
        self.remember_public_file("preexec.json")
        self.remember_public_file("prometheus-exec.json")
        return result

    def request_entry_stop(self):
        super().request_entry_stop()
        self.remember_public_file("stop.request")

    def observe_entry_exit(self):
        super().observe_entry_exit()
        if (self.directory / "prometheus-exit.json").exists():
            self.remember_public_file("prometheus-exit.json")

    def retire_private_namespace(self, result, end):
        # This child owns OS teardown, NOT the parent's credential-file owner.
        # Never unlink its tracked secret/control leaves from this process.
        need(time.monotonic() < end and identity(self.directory.lstat())[:5] == self.directory_identity[:5],
             "disk_cleanup_directory")
        for name in tuple(self.retained_public_files):
            self.remember_public_file(name)
        result["retainedPublicFiles"] = [self.retained_public_files[name] for name in sorted(self.retained_public_files)]
        result["privateNamespaceRemoved"] = False
        return result
def cancel_signal(*_):
    global cancelled
    cancelled=True

def run(session, digest):
    need(re.fullmatch("[0-9a-f]{32}", session) and re.fullmatch("[0-9a-f]{64}", digest), "disk_handoff")
    directory = ROOT / session;directory_identity = identity(directory.lstat())
    fd, raw = regular(directory / "session.json", uid=os.getuid(), mode=0o400, cap=65536)
    os.close(fd);need(hashlib.sha256(raw).hexdigest() == digest, "disk_handoff_hash")
    config = private_config(raw, session)
    incoming, outgoing = os.fstat(0), os.fstat(1)
    need(stat.S_ISFIFO(incoming.st_mode) and stat.S_ISFIFO(outgoing.st_mode)
         and incoming.st_uid == outgoing.st_uid == os.getuid()
         and (incoming.st_dev, incoming.st_ino) != (outgoing.st_dev, outgoing.st_ino), "disk_private_pipes")
    need(config["session"] == session and config["unit"] == "cc-agent-disk-" + session + ".service", "disk_handoff_unit")
    need(type(config["ownerPid"]) is int and config["ownerPid"] == os.getppid()
         and proc_start(config["ownerPid"]) == config["ownerStartTicks"], "disk_original_parent")
    owner = os.pidfd_open(config["ownerPid"])
    try:
        need(os.getppid() == config["ownerPid"] and proc_start(config["ownerPid"]) == config["ownerStartTicks"], "disk_original_parent_changed")
    except BaseException:
        os.close(owner)
        raise
    remaining = config["deadlineBoottimeNs"] - time.clock_gettime_ns(time.CLOCK_BOOTTIME)
    need(0 <= time.clock_gettime_ns(time.CLOCK_BOOTTIME) - config["createdBoottimeNs"] < 10_000_000_000, "disk_start_window")
    # Reached only after the real Rust Actor admitted its same original Permit.
    # Bind no old/unowned WAL: only an explicit, provisioned empty root may get
    # first metadata. Old retained roots must match every original public fact.
    DiskBinding, BoundDiskRoot = binding_types()
    bound = BoundDiskRoot(DiskBinding(**config["binding"]))
    try:
        with selectors.DefaultSelector() as original:
            original.register(owner, selectors.EVENT_READ)
            need(not original.select(0) and config["deadlineBoottimeNs"] > time.clock_gettime_ns(time.CLOCK_BOOTTIME)
                 and config["expiresUnixNs"] > time.time_ns(), "disk_expired")
        try: bound.check_binding()
        except FileNotFoundError: bound.bind_fresh_empty()
        bound.check_no_native_writer()
    finally:
        bound.close()
    need(config["deadlineBoottimeNs"] > time.clock_gettime_ns(time.CLOCK_BOOTTIME)
         and config["expiresUnixNs"] > time.time_ns(), "disk_expired")
    spec = ServiceUnitSpec.from_private_handoff(session, config["binding"]["original"]["collector_id"], digest, remaining)
    unit = ServiceOwnedUnit(spec, directory, directory_identity, config["binarySha256"], config["statusPort"], config.get('runtime'))
    terminal = {"format": "cc-disk-unit-result/v1", "outcome": "unknown", "lossUnknown": True, "cleanup": None}
    failure = None; stage = "manager_environment"
    try:
        atomic_record(directory, "launch-owner.json", {
            "format": "cc-disk-launch-owner/v1", "session": session, "configSha256": digest,
            "pid": os.getpid(), "startTicks": proc_start(os.getpid()),
            "parentPid": config["ownerPid"], "parentStartTicks": config["ownerStartTicks"]}, 0o400)
        unit.remember_public_file("launch-owner.json")
        _, manager_raw = command(["/usr/bin/systemctl", "--user", "show-environment"], time.monotonic() + 1)
        manager_environment(manager_raw)
        # No replacement/adoption: refuse ANY unit at this fresh UUID name.
        stage = "unit_absent"
        code, before = command(["/usr/bin/systemctl", "--user", "--no-pager", "show", spec.name, "--property=LoadState"], time.monotonic() + 1, missing=True)
        need(before.strip() == b"LoadState=not-found", "disk_existing_unit")
        # Preprovisioned user-manager/delegation/sandbox support is mandatory;
        # failure to establish it cannot be fixed or elevated by this adapter.
        stage = "unit_start"
        unit.attempted = True
        command(spec.start_argv(), time.monotonic() + 10)
        stage = "unit_running"
        start_end = min(time.monotonic() + 10, time.monotonic() + (config["createdBoottimeNs"] + 10_000_000_000 - time.clock_gettime_ns(time.CLOCK_BOOTTIME)) / 1e9)
        while True:
            try:
                row, observed = unit.running(start_end);break
            except FileNotFoundError:
                need(time.monotonic() < start_end and not cancelled, "disk_start_unknown")
                time.sleep(.02)
            except ValueError as error:
                need(str(error) in {"disk_start_unknown", "disk_prometheus_not_execed"}, "disk_start_denied")
                need(time.monotonic() < start_end and not cancelled, "disk_start_unknown")
                time.sleep(.02)
        ready = {"format": "cc-collector-service-ready/v1", "session": session,
                 "unit": spec.name, "prometheusSha256": config["binarySha256"]}
        sys.stdout.write(json.dumps(ready, sort_keys=True, separators=(",", ":")) + "\n");sys.stdout.flush()
        os.set_blocking(0, False)
        pending = bytearray()
        with selectors.DefaultSelector() as wait:
            wait.register(0, selectors.EVENT_READ);wait.register(owner, selectors.EVENT_READ)
            while True:
                if cancelled:return terminal
                if time.clock_gettime_ns(time.CLOCK_BOOTTIME) >= config["deadlineBoottimeNs"] or time.time_ns() >= config["expiresUnixNs"]:
                    terminal["outcome"] = "expired";break
                row, observed = unit.snapshot(time.monotonic() + 1)
                if row["ActiveState"] != "active":break
                for key, _ in wait.select(.1):
                    if key.fd == owner:
                        terminal["outcome"] = "ownerLost";return terminal
                    need(identity(os.fstat(0))[:6] == identity(incoming)[:6], "disk_pipe_replaced")
                    block = os.read(0, 6 - len(pending))
                    if not block:return terminal
                    pending.extend(block)
                    need(len(pending) <= 5 and b"STOP\n".startswith(pending), "disk_private_control")
                    if pending == b"STOP\n":
                        terminal["outcome"] = "stopRequested";return terminal
    except BaseException as error:
        failure = error
        terminal["errorCode"] = str(error) if type(error) is ValueError and re.fullmatch("disk_[a-z_]+", str(error)) else "disk_unknown"
    finally:
        close_end = time.monotonic() + 10
        observed_unit = None
        if failure is not None and unit.attempted:
            try:
                # Read same exact owned unit BEFORE stop; one call consumes the
                # original ten-second cleanup budget, never a fresh extension.
                from disk_unit_spec import parse_show
                _, status_raw = command(spec.show_argv(),min(close_end,time.monotonic()+.5),missing=True)
                observed_unit = parse_show(status_raw,spec,check_limits=False)
            except BaseException:pass
        try:terminal["cleanup"] = unit.close(close_end)
        except BaseException:terminal["cleanup"] = dict(unit.cleanup_state, unknown=True)
        cleanup = terminal["cleanup"] or {}
        no_late_producer = _failure_no_late_producer(unit.attempted,cleanup)
        if failure is not None and no_late_producer:
            _record_start_failure(directory,"adapter",stage,failure,observed_unit)
            try:
                unit.remember_public_file("prometheus-exit.json")
                terminal["cleanup"]["retainedPublicFiles"] = [unit.retained_public_files[name] for name in sorted(unit.retained_public_files)]
            except BaseException:pass
        terminal["entryStoppedObserved"] = unit.entry_stopped
        for fd in (unit.entry_namespace_fd, unit.adapter_namespace_fd):
            if fd is not None: os.close(fd)
        if unit.child_pidfd is not None:os.close(unit.child_pidfd)
        if unit.pidfd is not None:os.close(unit.pidfd)
        os.close(owner)
    return terminal


def main():
    need(len(sys.argv) == 3 and sys.argv[1].startswith("--session=") and sys.argv[2].startswith("--config-sha256="), "disk_argv")
    signal.signal(signal.SIGTERM, cancel_signal);signal.signal(signal.SIGINT, cancel_signal)
    result = run(sys.argv[1][10:], sys.argv[2][16:])
    cleanup = result["cleanup"] or {}
    final = {"format": "cc-collector-service-exit/v1", "session": sys.argv[1][10:],
             "unit": "cc-agent-disk-" + sys.argv[1][10:] + ".service",
             "unitEmpty": cleanup.get("cgroupEmpty", False), "leaderExited": cleanup.get("leaderExited", False),
             "unitStopped": cleanup.get("unitStopped", False),
             "privateNamespaceRemoved": cleanup.get("privateNamespaceRemoved", False),
             "cleanupUnknown": cleanup.get("unknown", False),
             "handlesClosed": True,
             "retainedPublicFiles": cleanup.get("retainedPublicFiles", []),
             "outcome": "stopped" if result["outcome"] == "stopRequested" and result.get("entryStoppedObserved") and cleanup.get("privateNamespaceRemoved") else "unknown"}
    raw = json.dumps(final, sort_keys=True, separators=(",", ":"))
    need(len(raw) <= 4096, "disk_terminal_bound")
    sys.stdout.write(raw + "\n");sys.stdout.flush()
    return 0 if final["outcome"] == "stopped" else 1


if __name__ == "__main__":
    raise SystemExit(main())
