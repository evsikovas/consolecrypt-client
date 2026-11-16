"""Fixed inert pre-exec gate; source candidate, NOT installed or selected.

The future Rust owner already holds the actual MetricsCollect Permit. These
private lifecycle files validate a component, never confer portable authority.
"""
import hashlib
import importlib.util
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
import uuid

ROOT = Path("/run/consolecrypt-agent/disk")
BINARY = Path("/opt/consolecrypt-agent-install/opt/consolecrypt-agent/collector/prometheus-hardmode")
MAX_MEMORY = 536870912
TMPFS_BYTES = 33554432
cancelled = False
# Outside the unit (adapter imports) this remains actual host UID0. Only the
# exact supported private-user mapping gate can select overflow view inside.
_ROOT_VIEW_UID = 0
_binding_module = None


def fixed_argv(directory, wal_root, status_port):
    # Internal pure projection; actual gate retains/checks all original FDs.
    # Fixed roots and UUID-derived leaves cannot become a generic launcher.
    need(directory.parent == ROOT and re.fullmatch('[0-9a-f]{32}', directory.name)
         and uuid.UUID(hex=directory.name).version == 4, 'disk_session')
    need(wal_root.name == 'wal' and wal_root.parent.parent == Path('/var/lib/consolecrypt-agent/metrics-disk')
         and str(uuid.UUID(wal_root.parent.name)) == wal_root.parent.name
         and uuid.UUID(wal_root.parent.name).version == 8, 'disk_wal_root')
    need(type(status_port) is int and 1024 <= status_port <= 65535, 'disk_status_port')
    return [str(BINARY), '--agent', '--storage.agent.hard-budget',
            '--storage.agent.path=' + str(wal_root), '--no-storage.agent.wal-compression',
            '--config.file=' + str(directory / 'scrape.json'),
            '--web.listen-address=127.0.0.1:' + str(status_port), '--log.level=error']


def binding_types():
    global _binding_module
    if _binding_module is None:
        package = Path("/opt/consolecrypt-agent-install/opt/consolecrypt-agent/collector")
        for parent in (*reversed(package.parents), package):
            info = parent.lstat()
            need(stat.S_ISDIR(info.st_mode) and not stat.S_ISLNK(info.st_mode)
                 and info.st_uid == _ROOT_VIEW_UID and not info.st_mode & 0o022, "disk_package")
        path = package / "disk_binding.py";info = path.lstat()
        need(stat.S_ISREG(info.st_mode) and not stat.S_ISLNK(info.st_mode)
             and info.st_uid == _ROOT_VIEW_UID and info.st_nlink == 1
             and stat.S_IMODE(info.st_mode) == 0o644 and info.st_size <= 65536, "disk_binding_code")
        spec = importlib.util.spec_from_file_location("_cc_installed_disk_binding", path)
        _binding_module = importlib.util.module_from_spec(spec)
        sys.modules[spec.name] = _binding_module
        spec.loader.exec_module(_binding_module)
    return _binding_module.DiskBinding, _binding_module.BoundDiskRoot


def self_mapping(raw, expected):
    rows = [line.split() for line in raw.splitlines() if line.strip()]
    return (type(expected) is int and expected not in (0, 65534) and len(rows) == 1
            and len(rows[0]) == 3 and all(re.fullmatch("[0-9]+", x) for x in rows[0])
            and [int(x) for x in rows[0]] == [expected, expected, 1])


def set_unit_root_view():
    global _ROOT_VIEW_UID
    need(os.getuid() == os.geteuid() and os.getgid() == os.getegid()
         and self_mapping(read_text(Path("/proc/self/uid_map")), os.getuid())
         and self_mapping(read_text(Path("/proc/self/gid_map")), os.getgid())
         and read_text(Path("/proc/sys/kernel/overflowuid")) == "65534"
         and read_text(Path("/proc/sys/kernel/overflowgid")) == "65534", "disk_user_namespace")
    _ROOT_VIEW_UID = 65534  # A mapped view ONLY, never a host-root claim.


def namespace_proof(owner_pid, owner_start, uid):
    held = []
    try:
        held.append(monitor_process(owner_pid, owner_start, uid))
        # Retain SELF namespace only. A child userns cannot assume permission
        # to dereference the outside owner's proc namespace link.
        held.append(os.open("/proc/self/ns/user", os.O_RDONLY | os.O_CLOEXEC))
        own = os.fstat(held[1])
        proof = (tuple(held), owner_pid, owner_start, uid, (own.st_dev, own.st_ino))
        need(namespace_current(proof), "disk_original_namespace")
        return proof
    except BaseException:
        for fd in held: os.close(fd)
        raise


def namespace_current(proof):
    held, pid, start, uid, own = proof
    try:
        if process_identity(pid, uid)[1] != start: return False
        a, b = os.fstat(held[1]), os.stat("/proc/self/ns/user")
        if (a.st_dev, a.st_ino) != own or (b.st_dev, b.st_ino) != own: return False
        if not self_mapping(read_text(Path("/proc/self/uid_map")), os.getuid()) or not self_mapping(read_text(Path("/proc/self/gid_map")), os.getgid()): return False
        with selectors.DefaultSelector() as wait:
            wait.register(held[0], selectors.EVENT_READ)
            return not wait.select(0)
    except (OSError, ValueError): return False


def need(value, code):
    if not value:
        raise ValueError(code)


def identity(st):
    return (st.st_dev, st.st_ino, st.st_mode, st.st_uid, st.st_gid,
            st.st_nlink, st.st_size, st.st_mtime_ns, st.st_ctime_ns)


def _regular_open(path, *, uid, mode, cap):
    # No blocking FIFO opens; retain the exact FD through hash/exec.
    for parent in reversed(path.parents):
        st = parent.lstat()
        need(stat.S_ISDIR(st.st_mode) and not stat.S_ISLNK(st.st_mode)
             and st.st_uid in (_ROOT_VIEW_UID, uid) and not st.st_mode & 0o022, "disk_parent")
    fd = os.open(path, os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK | os.O_CLOEXEC)
    try:
        st = os.fstat(fd)
        need(stat.S_ISREG(st.st_mode) and st.st_uid == (_ROOT_VIEW_UID if uid == 0 else uid) and stat.S_IMODE(st.st_mode) == mode
             and st.st_nlink == 1 and 0 < st.st_size <= cap, "disk_regular")
        return fd, st
    except BaseException:
        os.close(fd)
        raise


def regular(path, *, uid, mode, cap):
    fd, st = _regular_open(path, uid=uid, mode=mode, cap=cap)
    try:
        data = b""
        while len(data) <= cap:
            chunk = os.read(fd, min(1048576, cap + 1 - len(data)))
            if not chunk:
                break
            data += chunk
        need(len(data) == st.st_size and identity(os.fstat(fd)) == identity(st)
             and identity(path.stat(follow_symlinks=False)) == identity(st),
             "disk_file_changed")
        return fd, data
    except BaseException:
        os.close(fd)
        raise

def regular_fingerprint(path, *, uid, mode, cap):
    fd, st = _regular_open(path, uid=uid, mode=mode, cap=cap)
    try:
        digest = hashlib.sha256(); total = 0; header = b""
        while total <= cap:
            chunk = os.read(fd, min(65536, cap + 1 - total))
            if not chunk: break
            if len(header) < 64: header += chunk[:64-len(header)]
            digest.update(chunk); total += len(chunk)
        need(total == st.st_size and identity(os.fstat(fd)) == identity(st)
             and identity(path.stat(follow_symlinks=False)) == identity(st), "disk_file_changed")
        return fd, (total, digest.hexdigest(), header)
    except BaseException:
        os.close(fd)
        raise



def runtime_record(value):
    keys = {'kind', 'upstreamTag', 'platform', 'binarySha256', 'binaryBytes'}
    rows = {'linux-x86_64': ('x86_64', 62), 'linux-aarch64': ('aarch64', 183)}
    need(type(value) is dict and set(value) == keys and value['kind'] == 'prometheusHardmode'
         and value['upstreamTag'] == '3.16.0' and type(value['platform']) is str
         and value['platform'] in rows and type(value['binaryBytes']) is int
         and 64 <= value['binaryBytes'] <= 192 * 1024 * 1024
         and type(value['binarySha256']) is str
         and re.fullmatch('[0-9a-f]{64}', value['binarySha256']), 'disk_runtime_metadata')
    return rows[value['platform']]


def selected_runtime(value):
    # Parsing public metadata grants nothing. SAME Root-owned selected release
    # was recorded only after the actual installer signature verifier succeeded.
    machine, elf = runtime_record(value)
    need(sys.platform == 'linux' and os.uname().machine == machine, 'disk_selected_platform')
    code = BINARY.parent.parent
    held = []
    def capture(path, cap, mode=0o644):
        fd, raw = regular(path, uid=0, mode=mode, cap=cap)
        held.append((fd, path, identity(os.fstat(fd))))
        return raw
    try:
        current = json.loads(capture(code / 'current.json', 4096), object_pairs_hook=object_pairs)
        version = current.get('version')
        need(type(version) is str and re.fullmatch(r'[0-9]{1,6}\.[0-9]{1,6}\.[0-9]{1,6}', version), 'disk_selected_version')
        release = code / 'releases' / version
        record = json.loads(capture(release / 'installed.json', 128*1024), object_pairs_hook=object_pairs)
        name = 'delivery/release_platforms.py'
        raw = capture(release / name, 65536)
        need(record['files'].get(name) == hashlib.sha256(raw).hexdigest()
             and record['file_modes'].get(name) == '0644', 'disk_selected_helper')
        import types
        platforms = types.ModuleType('_cc_disk_selected_platforms')
        exec(compile(raw, str(release / name), 'exec'), platforms.__dict__)
        row = platforms.installed_platform(current, record)
        need(row['release'] == value['platform'] and row['elfMachine'] == elf, 'disk_selected_platform')
        name = 'collector/prometheus-hardmode'
        need(record['files'].get(name) == value['binarySha256']
             and record['file_modes'].get(name) == '0755', 'disk_selected_binary')
        path = release / name
        fd, (size, digest, header) = regular_fingerprint(path, uid=0, mode=0o755, cap=192 * 1024 * 1024)
        held.append((fd, path, identity(os.fstat(fd))))
        platforms.require_elf(header, row)
        need(size == value['binaryBytes'] and digest == value['binarySha256'], 'disk_selected_binary')
        need(all(identity(os.fstat(fd)) == before == identity(path.lstat())
                 for fd, path, before in held), 'disk_selected_changed')
        return elf
    finally:
        for fd, _, _ in held:
            os.close(fd)


def pinned_binary(expected, runtime=None):
    need(type(expected) is str and re.fullmatch("[0-9a-f]{64}", expected), "disk_binary_digest")
    for parent in reversed(BINARY.parents):
        info = parent.lstat()
        need(stat.S_ISDIR(info.st_mode) and not stat.S_ISLNK(info.st_mode)
             and info.st_uid == _ROOT_VIEW_UID and not info.st_mode & 0o022, "disk_binary_parent")
    before = BINARY.lstat()
    fd = os.open(BINARY, os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK | os.O_CLOEXEC)
    try:
        info = os.fstat(fd)
        need(identity(before) == identity(info) and stat.S_ISREG(info.st_mode)
             and info.st_uid == _ROOT_VIEW_UID and stat.S_IMODE(info.st_mode) == 0o755
             and info.st_nlink == 1 and 0 < info.st_size <= 256 * 1024 * 1024, "disk_binary")
        digest = hashlib.sha256(); total = 0; header = b""
        while True:
            part = os.read(fd, 65536)
            if not part: break
            if not header: header = part[:64]
            total += len(part); need(total <= 256 * 1024 * 1024, "disk_binary_bound")
            digest.update(part)
        machine = 62 if runtime is None else selected_runtime(runtime)
        need(runtime is None or (runtime['binarySha256'] == expected and runtime['binaryBytes'] == total), 'disk_runtime_binary')
        need(total == info.st_size and digest.hexdigest() == expected
             and header[:7] == b"\x7fELF\x02\x01\x01" and int.from_bytes(header[18:20], "little") == machine
             and identity(os.fstat(fd)) == identity(info) == identity(BINARY.lstat()), "disk_binary_changed")
        return fd
    except BaseException:
        os.close(fd)
        raise


def object_pairs(pairs):
    need(len(pairs) == len({k for k, _ in pairs}), "disk_duplicate")
    return dict(pairs)


def read_text(path, cap=8192):
    with path.open("rb", buffering=0) as source:
        raw = bytearray()
        while len(raw) <= cap:
            block = source.read(min(65536, cap + 1 - len(raw)))
            if not block:
                break
            raw.extend(block)
    need(len(raw) <= cap, "disk_kernel_bound")
    return raw.decode("ascii").strip()


def process_identity(pid, uid):
    root = Path("/proc") / str(pid)
    need(root.stat().st_uid == uid, "disk_process_uid")
    value = read_text(root / "stat").rsplit(") ", 1)[1].split()
    need(value[0] != "Z", "disk_process_exited")
    return int(value[1]), int(value[19])


def monitor_process(pid, start, uid, parent=None):
    before = process_identity(pid, uid)
    need(before[1] == start and (parent is None or before[0] == parent), "disk_process_changed")
    fd = os.pidfd_open(pid)
    try:
        need(process_identity(pid, uid) == before, "disk_process_changed")
        return fd
    except BaseException:
        os.close(fd)
        raise


def atomic_record(directory, leaf, value, mode=0o600):
    need(leaf in {"launch-owner.json", "preexec.json", "prometheus-exec.json", "prometheus-exit.json", "stop.request"}, "disk_record_leaf")
    raw = json.dumps(value, sort_keys=True, separators=(",", ":"), ensure_ascii=True).encode("ascii")
    need(len(raw) <= 4096, "disk_record_bound")
    target, temporary = directory / leaf, directory / ("." + leaf + ".pending")
    need(not target.exists(), "disk_record_exists")
    fd = os.open(temporary, os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW | os.O_CLOEXEC, mode)
    try:
        view = memoryview(raw)
        while view:
            n = os.write(fd, view);need(n > 0, "disk_record_write");view = view[n:]
        os.fchmod(fd, mode);os.fsync(fd)
    finally:
        os.close(fd)
    need(not target.exists(), "disk_record_exists")
    os.replace(temporary, target)


def mount_id(path):
    fd = os.open(path, os.O_PATH | os.O_DIRECTORY | os.O_NOFOLLOW | os.O_CLOEXEC)
    try:
        pairs = [line.split(":", 1) for line in read_text(Path("/proc/self/fdinfo") / str(fd)).splitlines()]
        fields = dict((k, v.strip()) for k, v in pairs)
        need(fields["mnt_id"].isdigit(), "disk_mount_identity")
        return fields["mnt_id"]
    finally:
        os.close(fd)


def private_config(raw, session):
    value = json.loads(raw, object_pairs_hook=object_pairs,
                       parse_float=lambda _: (_ for _ in ()).throw(ValueError("disk_float")))
    keys = {"format", "session", "unit", "bootId", "createdBoottimeNs", "deadlineBoottimeNs", "expiresUnixNs", "scrapeSha256", "receiver", "statusPort", "ownerPid", "ownerStartTicks", "manifestSha256", "binarySha256", "binding"}
    v2 = type(value) is dict and value.get('format') == 'cc-disk-prometheus-session/v2'
    need(type(value) is dict and set(value) == keys | ({'runtime'} if v2 else set())
         and value["format"] in {"cc-disk-prometheus-session/v1", "cc-disk-prometheus-session/v2"}
         and value["session"] == session and value["unit"] == "cc-agent-disk-" + session + ".service", "disk_config")
    need(json.dumps(value, sort_keys=True, separators=(",", ":"), ensure_ascii=True).encode("ascii") == raw, "disk_config_canonical")
    for key in ("createdBoottimeNs", "deadlineBoottimeNs", "expiresUnixNs", "ownerPid", "ownerStartTicks", "statusPort"):
        need(type(value[key]) is int and value[key] > 0, "disk_config_integer")
    for key in ("scrapeSha256", "manifestSha256", "binarySha256"):
        need(type(value[key]) is str and re.fullmatch("[0-9a-f]{64}", value[key]), "disk_config_digest")
    need(type(value["binding"]) is dict, "disk_binding")
    DiskBinding, _ = binding_types()
    binding = DiskBinding(**value["binding"]).checked()
    need(binding.manifest_sha256 == value["manifestSha256"], "disk_manifest_binding")
    if v2:
        runtime_record(value['runtime'])
        need(value['runtime']['binarySha256'] == value['binarySha256'], 'disk_runtime_binary')
    return value


def gate(session, config_hash):
    need(sys.platform == "linux" and os.uname().machine in {'x86_64', 'aarch64'}, "disk_platform")
    need(re.fullmatch("[0-9a-f]{32}", session) and uuid.UUID(hex=session).version == 4
         and re.fullmatch("[0-9a-f]{64}", config_hash), "disk_input")
    uid = os.getuid()
    need(uid != 0 and os.geteuid() == uid, "disk_uid")
    set_unit_root_view()
    directory = ROOT / session
    st = directory.lstat()
    need(stat.S_ISDIR(st.st_mode) and st.st_uid == uid and stat.S_IMODE(st.st_mode) == 0o700, "disk_directory")
    proof = None; binary_fd = None; disk = None; completed = False
    config_fd, raw = regular(directory / "session.json", uid=uid, mode=0o400, cap=65536)
    try:
        need(hashlib.sha256(raw).hexdigest() == config_hash, "disk_config_hash")
        value = private_config(raw, session)
        need(value.get('runtime') is not None or os.uname().machine == 'x86_64', 'disk_legacy_platform')
        need(value["bootId"] == read_text(Path("/proc/sys/kernel/random/boot_id")), "disk_boot")
        for key in ("createdBoottimeNs", "deadlineBoottimeNs", "expiresUnixNs"):
            need(type(value[key]) is int, "disk_deadline")
        now_boot = time.clock_gettime_ns(time.CLOCK_BOOTTIME)
        need(0 <= now_boot - value["createdBoottimeNs"] < 10_000_000_000
             and 0 < value["deadlineBoottimeNs"] - value["createdBoottimeNs"] <= 300_000_000_000, "disk_start_window")
        left = value["deadlineBoottimeNs"] - now_boot
        need(0 < left <= 300_000_000_000 and time.time_ns() < value["expiresUnixNs"] <= time.time_ns() + 300_000_000_000, "disk_expired")
        need(type(value["ownerPid"]) is int and value["ownerPid"] > 1
             and type(value["ownerStartTicks"]) is int and value["ownerStartTicks"] > 0
             and re.fullmatch("[0-9a-f]{64}", value["manifestSha256"]), "disk_owner")
        owner = Path("/proc") / str(value["ownerPid"])
        need(owner.stat().st_uid == uid, "disk_owner_uid")
        owner_stat = read_text(owner / "stat").rsplit(") ", 1)[1].split()
        need(owner_stat[0] != "Z" and int(owner_stat[19]) == value["ownerStartTicks"], "disk_owner_changed")
        proof = namespace_proof(value["ownerPid"], value["ownerStartTicks"], uid)
        need(type(value["statusPort"]) is int and 1024 <= value["statusPort"] <= 65535, "disk_port")
        need(type(value["receiver"]) is str and re.fullmatch(r"http://127\.0\.0\.1:[1-9][0-9]{0,4}/api/v1/(?:write|push)", value["receiver"])
             and 1 <= int(value["receiver"].split(":")[2].split("/")[0]) <= 65535, "disk_receiver")
        # No claim that loopback itself prevents HTTP redirects. A separately
        # verified nonredirecting installed receiver is a mandatory admission prerequisite.
        mount = []
        roots = []
        actual_mount, actual_root = mount_id(directory), mount_id(Path("/"))
        for line in read_text(Path("/proc/self/mountinfo"), 262144).splitlines():
            first, last = line.split(" - ", 1);a, z = first.split(), last.split()
            if a[0] == actual_mount and a[4] in (str(ROOT), str(directory)):
                mount.append((a, z))
            if a[0] == actual_root and a[4] == "/":
                roots.append(a)
        need(len(mount) == 1, "disk_mount")
        a, z = mount[0];options = set(a[5].split(",")) | set(z[2].split(","))
        need(z[0] == "tmpfs" and {"rw", "nosuid", "nodev", "noexec", "noswap"} <= options, "disk_mount_options")
        need(len(roots) == 1 and "ro" in roots[0][5].split(","), "disk_readonly_system")
        fs = os.statvfs(directory)
        need(fs.f_blocks * fs.f_frsize == TMPFS_BYTES and 0 < fs.f_files <= 4096
             and ROOT.stat().st_dev == st.st_dev, "disk_mount_size")
        c = read_text(Path("/proc/self/cgroup"))
        need(c.startswith("0::/") and "\n" not in c and ".." not in c, "disk_cgroup")
        cpath = c[3:];need(cpath.rsplit("/", 1)[-1] == value["unit"], "disk_cgroup_unit")
        cg = Path("/sys/fs/cgroup") / cpath.lstrip("/")
        need(read_text(cg / "cgroup.type") == "domain", "disk_cgroup_type")
        expected = {"memory.max": str(MAX_MEMORY), "memory.swap.max": "0", "pids.max": "64", "cpu.max": "100000 100000"}
        need(all(read_text(cg / k) == v for k, v in expected.items()), "disk_kernel_limits")
        need(read_text(cg / "cgroup.procs") == str(os.getpid()), "disk_initial_membership")
        for filename, required in (("memory.events", {"low", "high", "max", "oom", "oom_kill"}), ("pids.events", {"max"})):
            pairs = [line.split() for line in read_text(cg / filename).splitlines()]
            need(all(len(pair) == 2 for pair in pairs) and len(pairs) == len({x[0] for x in pairs}), "disk_events")
            events = dict(pairs);need(required <= set(events) and all(re.fullmatch("[0-9]+", n) for n in events.values()), "disk_events")
            need(all(int(events[k]) == 0 for k in required), "disk_prior_resource_failure")
        invocation = os.environ.get("INVOCATION_ID", "")
        need(re.fullmatch("[0-9a-f]{32}", invocation), "disk_invocation")
        scrape_fd, scrape = regular(directory / "scrape.json", uid=uid, mode=0o400, cap=65536)
        os.close(scrape_fd)
        need(hashlib.sha256(scrape).hexdigest() == value["scrapeSha256"], "disk_scrape_hash")
        binary_fd = pinned_binary(value["binarySha256"], value.get('runtime'))
        DiskBinding, BoundDiskRoot = binding_types()
        disk = BoundDiskRoot(DiskBinding(**value["binding"]))
        # The actual Permit-owning outside adapter binds a genuinely empty
        # provisioned root before unit start. This unit can write WAL only;
        # its read-only parent cannot create or replace binding metadata.
        disk.check_binding()
        disk.check_no_native_writer()
        # No payload before actual cap/mount checks. Parent compares this to
        # systemd InvocationID/MainPID/start identity before publishing running.
        receipt = {"format": "cc-disk-preexec/v1", "session": session, "invocationId": invocation,
                   "pid": os.getpid(), "cgroup": cpath, "binarySha256": value["binarySha256"],
                   "mountId": int(a[0]), "kernelLimits": expected,
                   "manifestSha256": value["manifestSha256"], "configSha256": config_hash}
        atomic_record(directory, "preexec.json", receipt)
        need(value["deadlineBoottimeNs"] > time.clock_gettime_ns(time.CLOCK_BOOTTIME)
             and value["expiresUnixNs"] > time.time_ns()
             and time.clock_gettime_ns(time.CLOCK_BOOTTIME) < value["createdBoottimeNs"] + 10_000_000_000, "disk_expired")
        argv = fixed_argv(directory, disk.path / 'wal', value['statusPort'])
        os.chdir(directory)
        need(namespace_current(proof), "disk_original_namespace")
        completed = True
        return binary_fd, argv, proof, disk
    finally:
        os.close(config_fd)
        if not completed:
            if disk is not None: disk.close()
            if binary_fd is not None: os.close(binary_fd)
            if proof is not None:
                for fd in proof[0]: os.close(fd)


def main():
    need(len(sys.argv) == 3 and sys.argv[1].startswith("--session=") and sys.argv[2].startswith("--config-sha256="), "disk_argv")
    signal.signal(signal.SIGTERM, cancel_signal);signal.signal(signal.SIGINT, cancel_signal)
    return supervise(sys.argv[1][10:], sys.argv[2][16:])


def cancel_signal(*_):
    global cancelled
    cancelled = True


def supervise(session, config_hash):
    binary_fd, argv, namespace, disk = gate(session, config_hash)
    directory = ROOT / session;uid = os.getuid()
    config_fd = owner_fd = adapter_fd = child_fd = None
    child = None;child_start = None;joined = False;stopped = False;child_exit = None;kill_sent = False
    entry_pid = os.getpid();entry_start = process_identity(entry_pid, uid)[1]
    invocation = os.environ["INVOCATION_ID"]
    try:
        config_fd, raw = regular(directory / "session.json", uid=uid, mode=0o400, cap=65536)
        need(hashlib.sha256(raw).hexdigest() == config_hash, "disk_config_changed")
        value = private_config(raw, session);config_identity = identity(os.fstat(config_fd))
        owner_fd = monitor_process(value["ownerPid"], value["ownerStartTicks"], uid)
        launch_fd, launch_raw = regular(directory / "launch-owner.json", uid=uid, mode=0o400, cap=4096)
        os.close(launch_fd);launch = json.loads(launch_raw, object_pairs_hook=object_pairs)
        need(set(launch) == {"format", "session", "configSha256", "pid", "startTicks", "parentPid", "parentStartTicks"}
             and launch["format"] == "cc-disk-launch-owner/v1" and launch["session"] == session
             and launch["configSha256"] == config_hash and launch["parentPid"] == value["ownerPid"]
             and launch["parentStartTicks"] == value["ownerStartTicks"]
             and type(launch["pid"]) is int and launch["pid"] > 1 and type(launch["startTicks"]) is int,
             "disk_launch_owner")
        adapter_fd = monitor_process(launch["pid"], launch["startTicks"], uid, value["ownerPid"])

        def original_live():
            disk.check_binding()
            if not namespace_current(namespace) or cancelled or value["deadlineBoottimeNs"] <= time.clock_gettime_ns(time.CLOCK_BOOTTIME) or value["expiresUnixNs"] <= time.time_ns():
                return False
            if identity(os.fstat(config_fd)) != config_identity or identity((directory / "session.json").lstat()) != config_identity:
                return False
            with selectors.DefaultSelector() as poll:
                poll.register(owner_fd, selectors.EVENT_READ);poll.register(adapter_fd, selectors.EVENT_READ)
                return not poll.select(0)

        need(original_live() and time.clock_gettime_ns(time.CLOCK_BOOTTIME) < value["createdBoottimeNs"] + 10_000_000_000, "disk_expired")
        # One fixed verified ELF, not a request executable. exec follows this
        # held descriptor only; the same original capped unit contains both.
        disk.release_native_probe()
        need(original_live(), "disk_expired")
        child = subprocess.Popen(argv, executable="/proc/self/fd/" + str(binary_fd),
                                 pass_fds=(binary_fd,), close_fds=True, shell=False,
                                 stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
                                 cwd=directory, env={"LANG": "C", "GOMAXPROCS": "1", "GOMEMLIMIT": "256MiB"})
        child_parent, child_start = process_identity(child.pid, uid)
        need(child_parent == entry_pid, "disk_child_parent")
        child_fd = monitor_process(child.pid, child_start, uid, entry_pid)
        binary = os.fstat(binary_fd);actual = (Path("/proc") / str(child.pid) / "exe").stat()
        need((actual.st_dev, actual.st_ino) == (binary.st_dev, binary.st_ino) and original_live(), "disk_child_exec")
        cgroup = read_text(Path("/proc/self/cgroup"))[3:]
        members = {int(x) for x in read_text(Path("/sys/fs/cgroup") / cgroup.lstrip("/") / "cgroup.procs").splitlines()}
        need(members == {entry_pid, child.pid}, "disk_child_membership")
        atomic_record(directory, "prometheus-exec.json", {
            "format": "cc-disk-prometheus-exec/v1", "session": session, "invocationId": invocation,
            "unit": value["unit"], "entryPid": entry_pid, "entryStartTicks": entry_start,
            "childPid": child.pid, "childStartTicks": child_start,
            "childExeDev": actual.st_dev, "childExeIno": actual.st_ino,
            "prometheusSha256": value["binarySha256"], "configSha256": config_hash})
        while child.poll() is None:
            # Unknown/expiry/owner loss wins over a simultaneous Stop request.
            # BOOTTIME advances across suspend. Detection/kill is scheduled OS
            # work, not a zero-latency or zero-expired-byte guarantee on resume.
            if not original_live():break
            request = directory / "stop.request"
            if request.exists():
                request_fd, request_raw = regular(request, uid=uid, mode=0o400, cap=4096)
                os.close(request_fd);request_value = json.loads(request_raw, object_pairs_hook=object_pairs)
                need(request_value == {"format": "cc-disk-stop-request/v1", "session": session, "configSha256": config_hash}, "disk_stop_request")
                stopped = True;break
            with selectors.DefaultSelector() as poll:
                poll.register(owner_fd, selectors.EVENT_READ);poll.register(adapter_fd, selectors.EVENT_READ)
                poll.register(child_fd, selectors.EVENT_READ);poll.select(.02)
        if child.poll() is None:
            try:signal.pidfd_send_signal(child_fd, signal.SIGKILL);kill_sent = True
            except ProcessLookupError:pass
        child_exit = child.wait(timeout=1);joined = True
        events = dict(line.split() for line in read_text(Path("/sys/fs/cgroup") / cgroup.lstrip("/") / "memory.events").splitlines())
        stopped = stopped and original_live() and kill_sent and child_exit == -signal.SIGKILL and events.get("oom") == "0" and events.get("oom_kill") == "0"
    except BaseException:
        stopped = False
    finally:
        if child is not None and not joined:
            try:
                if child.poll() is None:
                    if child_fd is not None:signal.pidfd_send_signal(child_fd, signal.SIGKILL)
                    else:child.kill()  # Only this unreaped Popen-owned child.
                child_exit = child.wait(timeout=1);joined = True
            except BaseException:stopped = False
        try:
            if child is not None:
                atomic_record(directory, "prometheus-exit.json", {
                    "format": "cc-disk-prometheus-exit/v1", "session": session, "configSha256": config_hash,
                    "invocationId": invocation, "entryPid": entry_pid, "entryStartTicks": entry_start,
                    "childPid": child.pid, "childStartTicks": child_start,
                    "outcome": "stopped" if stopped and joined else "unknown", "childJoined": joined, "childExit": child_exit})
        except BaseException:stopped = False
        for fd in (*namespace[0], child_fd, adapter_fd, owner_fd, config_fd, binary_fd):
            if fd is not None:os.close(fd)
        disk.close()
    return 0 if stopped and joined else 1


if __name__ == "__main__":
    raise SystemExit(main())
