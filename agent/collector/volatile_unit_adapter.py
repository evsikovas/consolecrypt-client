"""Fixed Linux unit custody; real MetricsCollect Permit remains in Rust.

No public launcher/API. Only the proposed fixed Rust worker may materialize the
private handoff and spawn this installed child. EOF/STOP/deadline/owner death
stops the exact owned unit; terminal metadata is never an admission or ACK.
"""
import hashlib
import json
import os
from pathlib import Path
import re
import selectors
import signal
import stat
import subprocess
import sys
import time

PACKAGE = Path("/opt/consolecrypt-agent-install/opt/consolecrypt-agent/collector")
for _parent in (*reversed(PACKAGE.parents), PACKAGE):
    _st = _parent.lstat()
    if not stat.S_ISDIR(_st.st_mode) or _st.st_uid != 0 or _st.st_mode & 0o022:
        raise ValueError("volatile_package")
for _name in ("unit_spec.py", "volatile_unit_entry.py"):
    _st = (PACKAGE / _name).lstat()
    if not stat.S_ISREG(_st.st_mode) or _st.st_uid != 0 or _st.st_nlink != 1 or stat.S_IMODE(_st.st_mode) != 0o644:
        raise ValueError("volatile_package")
sys.path.insert(0, str(PACKAGE))
from unit_spec import UnitSpec, parse_show
from volatile_unit_entry import ROOT, BINARY, BINARY_SHA256, expected_binary, selected_runtime, atomic_record, identity, need, object_pairs, private_config, process_identity, read_text, regular, self_mapping

cancelled = False


def cancel_signal(*_):
    global cancelled
    cancelled = True


def command(argv, end, cap=8192, missing=False):
    # Fixed root-installed systemd utilities, never request executable/argv.
    need(time.monotonic() < end, "volatile_control_deadline")
    process = subprocess.Popen(argv, stdin=subprocess.DEVNULL, stdout=subprocess.PIPE,
                               stderr=subprocess.DEVNULL, close_fds=True, shell=False,
                               env={"LANG": "C", "XDG_RUNTIME_DIR": "/run/user/" + str(os.getuid()),
                                    "DBUS_SESSION_BUS_ADDRESS": "unix:path=/run/user/" + str(os.getuid()) + "/bus"})
    raw = bytearray()
    try:
        os.set_blocking(process.stdout.fileno(), False)
        with selectors.DefaultSelector() as poll:
            poll.register(process.stdout, selectors.EVENT_READ)
            eof = False
            while not eof:
                need(time.monotonic() < end, "volatile_control_deadline")
                for key, _ in poll.select(min(.05, max(0, end - time.monotonic()))):
                    block = os.read(key.fd, min(4096, cap + 1 - len(raw)))
                    if not block:
                        eof = True;break
                    raw.extend(block);need(len(raw) <= cap, "volatile_control_output")
            process.wait(timeout=max(.001, end - time.monotonic()))
        need(process.returncode == 0 or missing, "volatile_control_exit")
        return process.returncode, bytes(raw)
    finally:
        if process.poll() is None:
            process.kill()
        process.wait(timeout=1)
        process.stdout.close()


def manager_environment(raw):
    # Root-provisioned dedicated manager policy, not borrowed caller variables.
    expected = {"HOME": "/var/lib/consolecrypt-agent", "USER": "consolecrypt-agent",
                "LOGNAME": "consolecrypt-agent", "SHELL": "/usr/sbin/nologin", "LANG": "C",
                "PATH": "/usr/bin:/bin", "XDG_RUNTIME_DIR": "/run/user/" + str(os.getuid()),
                "SYSTEMD_ENVIRONMENT_GENERATOR_PATH": "/dev/null",
                "XDG_CONFIG_HOME": "/etc/consolecrypt-agent/collector-user"}
    pairs = [line.split("=", 1) for line in raw.decode("ascii").splitlines()]
    need(all(len(p) == 2 for p in pairs) and len(pairs) == len({p[0] for p in pairs}), "volatile_manager_environment")
    actual = dict(pairs)
    bus = actual.pop("DBUS_SESSION_BUS_ADDRESS", None)
    need(actual == expected and bus in (None, "unix:path=/run/user/" + str(os.getuid()) + "/bus"), "volatile_manager_environment")


def proc_start(pid, live=True):
    root = Path("/proc") / str(pid)
    need(root.stat().st_uid == os.getuid(), "volatile_pid_uid")
    row = read_text(root / "stat").rsplit(") ", 1)[1].split()
    need(not live or row[0] != "Z", "volatile_pid_dead")
    return int(row[19])


def duration_us(value):
    compact = value.replace(" ", "")
    tokens = re.findall(r"([0-9]+)(min|ms|us|s)", compact)
    need(tokens and "".join(a + b for a, b in tokens) == compact, "volatile_runtime_readback")
    units = {"us": 1, "ms": 1000, "s": 1000000, "min": 60000000}
    return sum(int(n) * units[u] for n, u in tokens)


class OwnedUnit:
    def __init__(self, spec, directory, directory_identity, binary_sha256):
        self.spec, self.directory, self.directory_identity = spec, directory, directory_identity
        self.binary_sha256=binary_sha256
        self.invocation = self.cgroup = self.pid = self.start = self.pidfd = None
        self.child_pid = self.child_start = self.child_pidfd = None
        self.entry_stopped = False
        self.entry_namespace_fd = self.adapter_namespace_fd = None
        self.attempted = False
        self.cleanup_state = {"unitStopped": False, "cgroupEmpty": False, "leaderExited": False, "privateNamespaceRemoved": False}

    def snapshot(self, end, *, identity_only=False):
        _, raw = command(self.spec.show_argv(), min(end, time.monotonic() + 1), missing=True)
        row = parse_show(raw, self.spec, check_limits=not identity_only)
        if row is None:
            return None, None
        if not identity_only:
            need(duration_us(row["RuntimeMaxUSec"]) == self.spec.runtime_seconds * 1000000, "volatile_runtime_readback")
        pid = int(row["MainPID"])
        need(row["ControlGroup"].startswith("/") and ".." not in row["ControlGroup"]
             and row["ControlGroup"].rsplit("/", 1)[-1] == self.spec.name, "volatile_cgroup_identity")
        if self.invocation is None:
            # Fresh exact name+Description(session/config hash) binds once even
            # on failed start; this is cleanup custody, never running admission.
            if pid > 1:
                self.start = proc_start(pid, live=False);self.pidfd = os.pidfd_open(pid)
                need(proc_start(pid, live=False) == self.start, "volatile_pid_changed")
            self.invocation, self.cgroup, self.pid = row["InvocationID"], row["ControlGroup"], pid
        need(row["InvocationID"] == self.invocation and row["ControlGroup"] == self.cgroup, "volatile_unit_replaced")
        if pid:
            need(pid == self.pid and proc_start(pid, live=False) == self.start, "volatile_pid_replaced")
        if identity_only:
            return row, None
        cg = Path("/sys/fs/cgroup") / self.cgroup.lstrip("/")
        if not cg.exists():
            need(pid == 0, "volatile_live_cgroup_missing")
            return row, None
        actual = {n: read_text(cg / n) for n in ("memory.max", "memory.swap.max", "pids.max", "cpu.max", "memory.current", "memory.peak", "memory.events", "pids.events", "cgroup.events", "cgroup.procs")}
        need(actual["memory.max"] == "536870912" and actual["memory.swap.max"] == "0"
             and actual["pids.max"] == "64" and actual["cpu.max"] == "100000 100000", "volatile_caps_changed")
        need(0 <= int(actual["memory.current"]) <= 536870912 and 0 < int(actual["memory.peak"]) <= 536870912, "volatile_memory")
        for filename, required in (("memory.events", {"low", "high", "max", "oom", "oom_kill"}), ("pids.events", {"max"})):
            pairs = [line.split() for line in actual[filename].splitlines()]
            need(all(len(pair) == 2 for pair in pairs) and len(pairs) == len({pair[0] for pair in pairs}), "volatile_resource_events")
            events = dict(pairs)
            need(required <= set(events) and all(re.fullmatch("[0-9]+", n) for n in events.values()), "volatile_resource_events")
            fatal = {"oom", "oom_kill"} if filename == "memory.events" else {"max"}
            need(all(events[key] == "0" for key in fatal), "volatile_resource_failed")
        return row, actual

    def running(self, end):
        row, actual = self.snapshot(end)
        need(row is not None and actual is not None and self.pidfd is not None
             and row["ActiveState"] == "active" and row["SubState"] == "running", "volatile_start_unknown")
        # Outside host namespace validates the exact original entry namespace.
        # The entry itself has no permission/need to inspect ancestor userns.
        need(read_text(Path("/proc/self/uid_map")).split() == ["0", "0", "4294967295"]
             and read_text(Path("/proc/self/gid_map")).split() == ["0", "0", "4294967295"], "volatile_host_namespace")
        if self.adapter_namespace_fd is None:
            self.adapter_namespace_fd = os.open("/proc/self/ns/user", os.O_RDONLY | os.O_CLOEXEC)
        if self.entry_namespace_fd is None:
            self.entry_namespace_fd = os.open("/proc/" + str(self.pid) + "/ns/user", os.O_RDONLY | os.O_CLOEXEC)
        own = os.fstat(self.adapter_namespace_fd); entry = os.fstat(self.entry_namespace_fd)
        named = os.stat("/proc/" + str(self.pid) + "/ns/user")
        need((own.st_dev, own.st_ino) != (entry.st_dev, entry.st_ino)
             and (named.st_dev, named.st_ino) == (entry.st_dev, entry.st_ino)
             and self_mapping(read_text(Path("/proc") / str(self.pid) / "uid_map"), os.getuid())
             and self_mapping(read_text(Path("/proc") / str(self.pid) / "gid_map"), os.getgid())
             and proc_start(self.pid) == self.start, "volatile_private_namespace")
        fd, raw = regular(self.directory / "preexec.json", uid=os.getuid(), mode=0o600, cap=4096)
        os.close(fd);receipt = json.loads(raw, object_pairs_hook=object_pairs)
        expected = {"format", "session", "invocationId", "pid", "cgroup", "binarySha256", "mountId", "kernelLimits", "manifestSha256", "configSha256"}
        need(set(receipt) == expected and receipt["format"] == "cc-volatile-preexec/v1"
             and receipt["session"] == self.spec.session and receipt["configSha256"] == self.spec.config_sha256
             and receipt["invocationId"] == self.invocation and receipt["pid"] == self.pid
             and receipt["cgroup"] == self.cgroup and receipt["binarySha256"] == self.binary_sha256, "volatile_preexec_identity")
        # MainPID remains the fixed Python entry watchdog. It is NOT vmagent.
        interpreter = Path("/usr/bin/python3.12").stat(follow_symlinks=False)
        actual_entry = (Path("/proc") / str(self.pid) / "exe").stat()
        need((actual_entry.st_dev, actual_entry.st_ino) == (interpreter.st_dev, interpreter.st_ino)
             and proc_start(self.pid) == self.start, "volatile_entry_not_execed")
        fd, raw = regular(self.directory / "vmagent-exec.json", uid=os.getuid(), mode=0o600, cap=4096)
        os.close(fd);executed = json.loads(raw, object_pairs_hook=object_pairs)
        keys = {"format", "session", "invocationId", "unit", "entryPid", "entryStartTicks", "childPid", "childStartTicks", "childExeDev", "childExeIno", "vmagentSha256", "configSha256"}
        need(set(executed) == keys and executed["format"] == "cc-volatile-vmagent-exec/v1"
             and executed["session"] == self.spec.session and executed["configSha256"] == self.spec.config_sha256
             and executed["unit"] == self.spec.name and executed["invocationId"] == self.invocation
             and executed["entryPid"] == self.pid and executed["entryStartTicks"] == self.start
             and executed["vmagentSha256"] == self.binary_sha256
             and all(type(executed[k]) is int for k in ("entryPid", "entryStartTicks", "childPid", "childStartTicks", "childExeDev", "childExeIno"))
             and type(executed["childPid"]) is int and executed["childPid"] > 1
             and type(executed["childStartTicks"]) is int, "volatile_child_identity")
        parent, start = process_identity(executed["childPid"], os.getuid())
        need(parent == self.pid and start == executed["childStartTicks"], "volatile_child_identity")
        if self.child_pidfd is None:
            self.child_pidfd = os.pidfd_open(executed["childPid"])
            self.child_pid, self.child_start = executed["childPid"], start
        need(self.child_pid == executed["childPid"] and self.child_start == start
             and process_identity(self.child_pid, os.getuid()) == (self.pid, self.child_start), "volatile_child_changed")
        # This observer remains OUTSIDE the unit's private user namespace.
        # Actual host-root ownership/fullhash is mandatory here, never UID65534.
        binary_fd, binary_raw = regular(BINARY, uid=0, mode=0o755, cap=32 * 1024 * 1024)
        try:
            need(hashlib.sha256(binary_raw).hexdigest() == self.binary_sha256, "volatile_host_binary")
            binary = os.fstat(binary_fd)
            observed = (Path("/proc") / str(self.child_pid) / "exe").stat()
            need((observed.st_dev, observed.st_ino) == (binary.st_dev, binary.st_ino)
                 == (executed["childExeDev"], executed["childExeIno"]), "volatile_vmagent_not_execed")
        finally:
            os.close(binary_fd)
        need({int(x) for x in actual["cgroup.procs"].splitlines()} == {self.pid, self.child_pid}, "volatile_owned_membership")
        return row, actual

    def request_entry_stop(self):
        value = {"format": "cc-volatile-stop-request/v1", "session": self.spec.session, "configSha256": self.spec.config_sha256}
        path = self.directory / "stop.request"
        if path.exists():
            fd, raw = regular(path, uid=os.getuid(), mode=0o400, cap=4096)
            os.close(fd);need(json.loads(raw, object_pairs_hook=object_pairs) == value, "volatile_stop_request")
        else:
            atomic_record(self.directory, "stop.request", value, 0o400)

    def observe_entry_exit(self):
        self.entry_stopped = False
        try:
            fd, raw = regular(self.directory / "vmagent-exit.json", uid=os.getuid(), mode=0o600, cap=4096)
            os.close(fd);value = json.loads(raw, object_pairs_hook=object_pairs)
            keys = {"format", "session", "configSha256", "invocationId", "entryPid", "entryStartTicks", "childPid", "childStartTicks", "outcome", "childJoined", "childExit"}
            self.entry_stopped = bool(set(value) == keys and value["format"] == "cc-volatile-vmagent-exit/v1"
                and value["session"] == self.spec.session and value["configSha256"] == self.spec.config_sha256
                and value["invocationId"] == self.invocation and value["entryPid"] == self.pid and value["entryStartTicks"] == self.start
                and value["childPid"] == self.child_pid and value["childStartTicks"] == self.child_start
                and value["outcome"] == "stopped" and value["childJoined"] is True and type(value["childExit"]) is int)
        except (OSError, ValueError, TypeError):
            pass  # Missing/invalid observed cleanup can never become stopped.

    def close(self, end):
        result = self.cleanup_state
        if not self.attempted:
            return result
        # Cleanup does not re-authorize sends. It uses a separate <=10s budget.
        # Failed cap readback must never prevent killing a correctly-owned unit.
        row, _ = self.snapshot(end, identity_only=True)
        requested = False
        if row is not None:
            need(identity(self.directory.lstat())[:5] == self.directory_identity[:5], "volatile_cleanup_directory")
            try:self.request_entry_stop();requested = True
            except (OSError, ValueError):pass  # Full tmpfs cannot block owned kill.
        request_end = min(end - 3, time.monotonic() + (2 if requested else 0))
        forced = False
        while time.monotonic() < end:
            row, _ = self.snapshot(end, identity_only=True)
            need(self.cgroup is not None, "volatile_never_bound_unit")
            cg = Path("/sys/fs/cgroup") / self.cgroup.lstrip("/")
            raw = None
            if cg.exists():
                raw = {k: read_text(cg / k) for k in ("cgroup.events", "cgroup.procs")}
            events = {} if raw is None else dict(line.split() for line in raw["cgroup.events"].splitlines())
            empty = raw is None or events.get("populated") == "0" and raw["cgroup.procs"] == ""
            if (row is None or row["MainPID"] == "0") and empty:
                need(self.pidfd is not None, "volatile_leader_not_captured")
                with selectors.DefaultSelector() as exited:
                    exited.register(self.pidfd, selectors.EVENT_READ)
                    result["leaderExited"] = bool(exited.select(0))
                    if self.child_pidfd is not None:
                        exited.unregister(self.pidfd);exited.register(self.child_pidfd, selectors.EVENT_READ)
                        result["leaderExited"] = result["leaderExited"] and bool(exited.select(0))
                if result["leaderExited"]:
                    result.update(unitStopped=True, cgroupEmpty=True);break
            if row is not None and not forced and time.monotonic() >= request_end:
                # Fresh exact InvocationID/PID-start fence precedes the same
                # previous whole-group SIGKILL fallback, never a caller signal.
                self.snapshot(end, identity_only=True)
                command(self.spec.stop_argv(), min(end, time.monotonic() + 2));forced = True
            time.sleep(.02)
        need(result["unitStopped"] and result["cgroupEmpty"] and result["leaderExited"], "volatile_cleanup_unknown")
        self.observe_entry_exit()
        need(identity(self.directory.lstat())[:5] == self.directory_identity[:5], "volatile_cleanup_directory")
        entries = 0
        for base, directories, files in os.walk(self.directory, topdown=False, followlinks=False):
            need(time.monotonic() < end, "volatile_cleanup_deadline")
            for name in files:
                entries += 1;need(entries <= 4096, "volatile_cleanup_entries")
                p = Path(base) / name;st = p.lstat()
                need(stat.S_ISREG(st.st_mode) and st.st_uid == os.getuid() and st.st_nlink == 1, "volatile_cleanup_leaf")
                p.unlink()
            for name in directories:
                entries += 1;need(entries <= 4096, "volatile_cleanup_entries")
                p = Path(base) / name;st = p.lstat()
                need(stat.S_ISDIR(st.st_mode) and st.st_uid == os.getuid(), "volatile_cleanup_leaf");p.rmdir()
        self.directory.rmdir();result["privateNamespaceRemoved"] = True
        return result


def run(session, digest):
    need(re.fullmatch("[0-9a-f]{32}", session) and re.fullmatch("[0-9a-f]{64}", digest), "volatile_handoff")
    directory = ROOT / session;directory_identity = identity(directory.lstat())
    fd, raw = regular(directory / "session.json", uid=os.getuid(), mode=0o400, cap=65536)
    os.close(fd);need(hashlib.sha256(raw).hexdigest() == digest, "volatile_handoff_hash")
    config = private_config(raw, session)
    binary_sha256=selected_runtime(config)[2]
    incoming, outgoing = os.fstat(0), os.fstat(1)
    need(stat.S_ISFIFO(incoming.st_mode) and stat.S_ISFIFO(outgoing.st_mode)
         and incoming.st_uid == outgoing.st_uid == os.getuid()
         and (incoming.st_dev, incoming.st_ino) != (outgoing.st_dev, outgoing.st_ino), "volatile_private_pipes")
    need(config["session"] == session and config["unit"] == "cc-agent-volatile-" + session + ".service", "volatile_handoff_unit")
    need(type(config["ownerPid"]) is int and config["ownerPid"] == os.getppid()
         and proc_start(config["ownerPid"]) == config["ownerStartTicks"], "volatile_original_parent")
    owner = os.pidfd_open(config["ownerPid"])
    try:
        need(os.getppid() == config["ownerPid"] and proc_start(config["ownerPid"]) == config["ownerStartTicks"], "volatile_original_parent_changed")
    except BaseException:
        os.close(owner)
        raise
    remaining = config["deadlineBoottimeNs"] - time.clock_gettime_ns(time.CLOCK_BOOTTIME)
    need(0 <= time.clock_gettime_ns(time.CLOCK_BOOTTIME) - config["createdBoottimeNs"] < 10_000_000_000, "volatile_start_window")
    spec = UnitSpec.from_private_handoff(session, digest, remaining)
    unit = OwnedUnit(spec, directory, directory_identity, binary_sha256)
    terminal = {"format": "cc-volatile-unit-result/v1", "outcome": "unknown", "lossUnknown": True, "cleanup": None}
    try:
        atomic_record(directory, "launch-owner.json", {
            "format": "cc-volatile-launch-owner/v1", "session": session, "configSha256": digest,
            "pid": os.getpid(), "startTicks": proc_start(os.getpid()),
            "parentPid": config["ownerPid"], "parentStartTicks": config["ownerStartTicks"]}, 0o400)
        _, manager_raw = command(["/usr/bin/systemctl", "--user", "show-environment"], time.monotonic() + 1)
        manager_environment(manager_raw)
        # No replacement/adoption: refuse ANY unit at this fresh UUID name.
        code, before = command(["/usr/bin/systemctl", "--user", "--no-pager", "show", spec.name, "--property=LoadState"], time.monotonic() + 1, missing=True)
        need(before.strip() == b"LoadState=not-found", "volatile_existing_unit")
        # Preprovisioned user-manager/delegation/sandbox support is mandatory;
        # failure to establish it cannot be fixed or elevated by this adapter.
        unit.attempted = True
        command(spec.start_argv(), time.monotonic() + 10)
        start_end = min(time.monotonic() + 10, time.monotonic() + (config["createdBoottimeNs"] + 10_000_000_000 - time.clock_gettime_ns(time.CLOCK_BOOTTIME)) / 1e9)
        while True:
            try:
                row, observed = unit.running(start_end);break
            except FileNotFoundError:
                need(time.monotonic() < start_end and not cancelled, "volatile_start_unknown")
                time.sleep(.02)
            except ValueError as error:
                need(str(error) in {"volatile_start_unknown", "volatile_vmagent_not_execed"}, "volatile_start_denied")
                need(time.monotonic() < start_end and not cancelled, "volatile_start_unknown")
                time.sleep(.02)
        ready = {"format": "cc-volatile-collector-ready/v1", "session": session,
                 "unit": spec.name, "vmagentSha256": binary_sha256}
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
                    need(identity(os.fstat(0))[:6] == identity(incoming)[:6], "volatile_pipe_replaced")
                    block = os.read(0, 6 - len(pending))
                    if not block:return terminal
                    pending.extend(block)
                    need(len(pending) <= 5 and b"STOP\n".startswith(pending), "volatile_private_control")
                    if pending == b"STOP\n":
                        terminal["outcome"] = "stopRequested";return terminal
    except BaseException as error:
        terminal["errorCode"] = str(error) if type(error) is ValueError and re.fullmatch("volatile_[a-z_]+", str(error)) else "volatile_unknown"
    finally:
        try:terminal["cleanup"] = unit.close(time.monotonic() + 10)
        except BaseException:terminal["cleanup"] = dict(unit.cleanup_state, unknown=True)
        terminal["entryStoppedObserved"] = unit.entry_stopped
        for fd in (unit.entry_namespace_fd, unit.adapter_namespace_fd):
            if fd is not None: os.close(fd)
        if unit.child_pidfd is not None:os.close(unit.child_pidfd)
        if unit.pidfd is not None:os.close(unit.pidfd)
        os.close(owner)
    return terminal


def main():
    need(len(sys.argv) == 3 and sys.argv[1].startswith("--session=") and sys.argv[2].startswith("--config-sha256="), "volatile_argv")
    signal.signal(signal.SIGTERM, cancel_signal);signal.signal(signal.SIGINT, cancel_signal)
    result = run(sys.argv[1][10:], sys.argv[2][16:])
    cleanup = result["cleanup"] or {}
    final = {"format": "cc-volatile-collector-exit/v1", "session": sys.argv[1][10:],
             "unit": "cc-agent-volatile-" + sys.argv[1][10:] + ".service",
             "unitEmpty": cleanup.get("cgroupEmpty", False), "leaderExited": cleanup.get("leaderExited", False),
             "handlesClosed": True,
             "outcome": "stopped" if result["outcome"] == "stopRequested" and result.get("entryStoppedObserved") and cleanup.get("privateNamespaceRemoved") else "unknown"}
    raw = json.dumps(final, sort_keys=True, separators=(",", ":"))
    need(len(raw) <= 4096, "volatile_terminal_bound")
    sys.stdout.write(raw + "\n");sys.stdout.flush()
    return 0 if final["outcome"] == "stopped" else 1


if __name__ == "__main__":
    raise SystemExit(main())
