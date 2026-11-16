"""Fixed inert pre-exec gate; source candidate, NOT installed or selected.

The future Rust owner already holds the actual MetricsCollect Permit. These
private lifecycle files validate a component, never confer portable authority.
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
import uuid

ROOT = Path("/run/consolecrypt-agent/volatile")
BINARY = Path("/opt/consolecrypt-agent-install/opt/consolecrypt-agent/collector/vmagent-prod")
BINARY_SHA256 = "2039bc05b6ad5c96b34b72801908896bf8a6a7bcab8ae32120cbb8e6217ef6f6"
MAX_MEMORY = 536870912
TMPFS_BYTES = 33554432
cancelled = False
# Outside the unit (adapter imports) this remains actual host UID0. Only the
# exact supported private-user mapping gate can select overflow view inside.
_ROOT_VIEW_UID = 0


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
         and read_text(Path("/proc/sys/kernel/overflowgid")) == "65534", "volatile_user_namespace")
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
        need(namespace_current(proof), "volatile_original_namespace")
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


def regular(path, *, uid, mode, cap):
    # No blocking FIFO opens; retain the exact FD through hash/exec.
    for parent in reversed(path.parents):
        st = parent.lstat()
        need(stat.S_ISDIR(st.st_mode) and not stat.S_ISLNK(st.st_mode)
             and st.st_uid in (_ROOT_VIEW_UID, uid) and not st.st_mode & 0o022, "volatile_parent")
    fd = os.open(path, os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK | os.O_CLOEXEC)
    try:
        st = os.fstat(fd)
        need(stat.S_ISREG(st.st_mode) and st.st_uid == (_ROOT_VIEW_UID if uid == 0 else uid) and stat.S_IMODE(st.st_mode) == mode
             and st.st_nlink == 1 and 0 < st.st_size <= cap, "volatile_regular")
        data = b""
        while len(data) <= cap:
            chunk = os.read(fd, min(1048576, cap + 1 - len(data)))
            if not chunk:
                break
            data += chunk
        need(len(data) == st.st_size and identity(os.fstat(fd)) == identity(st)
             and identity(path.stat(follow_symlinks=False)) == identity(st),
             "volatile_file_changed")
        return fd, data
    except BaseException:
        os.close(fd)
        raise


def object_pairs(pairs):
    need(len(pairs) == len({k for k, _ in pairs}), "volatile_duplicate")
    return dict(pairs)


def read_text(path, cap=8192):
    with path.open("rb", buffering=0) as source:
        raw = source.read(cap + 1)
    need(len(raw) <= cap, "volatile_kernel_bound")
    return raw.decode("ascii").strip()


def process_identity(pid, uid):
    root = Path("/proc") / str(pid)
    need(root.stat().st_uid == uid, "volatile_process_uid")
    value = read_text(root / "stat").rsplit(") ", 1)[1].split()
    need(value[0] != "Z", "volatile_process_exited")
    return int(value[1]), int(value[19])


def monitor_process(pid, start, uid, parent=None):
    before = process_identity(pid, uid)
    need(before[1] == start and (parent is None or before[0] == parent), "volatile_process_changed")
    fd = os.pidfd_open(pid)
    try:
        need(process_identity(pid, uid) == before, "volatile_process_changed")
        return fd
    except BaseException:
        os.close(fd)
        raise


def atomic_record(directory, leaf, value, mode=0o600):
    need(leaf in {"launch-owner.json", "preexec.json", "vmagent-exec.json", "vmagent-exit.json", "stop.request"}, "volatile_record_leaf")
    raw = json.dumps(value, sort_keys=True, separators=(",", ":"), ensure_ascii=True).encode("ascii")
    need(len(raw) <= 4096, "volatile_record_bound")
    target, temporary = directory / leaf, directory / ("." + leaf + ".pending")
    need(not target.exists(), "volatile_record_exists")
    fd = os.open(temporary, os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW | os.O_CLOEXEC, mode)
    try:
        view = memoryview(raw)
        while view:
            n = os.write(fd, view);need(n > 0, "volatile_record_write");view = view[n:]
        os.fchmod(fd, mode);os.fsync(fd)
    finally:
        os.close(fd)
    need(not target.exists(), "volatile_record_exists")
    os.replace(temporary, target)


def mount_id(path):
    fd = os.open(path, os.O_PATH | os.O_DIRECTORY | os.O_NOFOLLOW | os.O_CLOEXEC)
    try:
        pairs = [line.split(":", 1) for line in read_text(Path("/proc/self/fdinfo") / str(fd)).splitlines()]
        fields = dict((k, v.strip()) for k, v in pairs)
        need(fields["mnt_id"].isdigit(), "volatile_mount_identity")
        return fields["mnt_id"]
    finally:
        os.close(fd)


def private_config(raw, session):
    value = json.loads(raw, object_pairs_hook=object_pairs,
                       parse_float=lambda _: (_ for _ in ()).throw(ValueError("volatile_float")))
    keys = {"format", "session", "unit", "bootId", "createdBoottimeNs", "deadlineBoottimeNs", "expiresUnixNs", "scrapeSha256", "receiver", "statusPort", "ownerPid", "ownerStartTicks", "manifestSha256"}
    v2=type(value)is dict and value.get('format')=='cc-volatile-vmagent-session/v2'
    need(type(value) is dict and set(value) == keys | ({'runtime'} if v2 else set())
         and value["format"] in {"cc-volatile-vmagent-session/v1","cc-volatile-vmagent-session/v2"}
         and value["session"] == session and value["unit"] == "cc-agent-volatile-" + session + ".service", "volatile_config")
    need(json.dumps(value, sort_keys=True, separators=(",", ":"), ensure_ascii=True).encode("ascii") == raw, "volatile_config_canonical")
    for key in ("createdBoottimeNs", "deadlineBoottimeNs", "expiresUnixNs", "ownerPid", "ownerStartTicks", "statusPort"):
        need(type(value[key]) is int and value[key] > 0, "volatile_config_integer")
    for key in ("scrapeSha256", "manifestSha256"):
        need(type(value[key]) is str and re.fullmatch("[0-9a-f]{64}", value[key]), "volatile_config_digest")
    if v2: runtime_record(value['runtime'])
    return value


def runtime_record(value):
    keys={'kind','upstreamTag','platform','binarySha256','binaryBytes'}
    rows={
        'linux-x86_64':('x86_64',62,BINARY_SHA256,23209584),
        'linux-aarch64':('aarch64',183,'564346d8acd0979b62c2671ad586d63b243a925307bfa54d0d5f94bd50dda4f5',21299768),
    }
    need(type(value)is dict and set(value)==keys and value['kind']=='vmagent'
         and value['upstreamTag']=='v1.156.0' and type(value['platform'])is str
         and value['platform']in rows,'volatile_runtime_metadata')
    row=rows[value['platform']]
    need(type(value['binaryBytes'])is int and value['binaryBytes']==row[3]
         and value['binarySha256']==row[2],'volatile_runtime_metadata')
    return row


def expected_binary(config):
    if config['format']=='cc-volatile-vmagent-session/v2':return runtime_record(config['runtime'])
    need(os.uname().machine=='x86_64','volatile_legacy_platform')
    return ('x86_64',62,BINARY_SHA256,23209584)


def selected_runtime(config):
    expected=expected_binary(config)
    need(sys.platform=='linux' and os.uname().machine==expected[0],'volatile_platform')
    if config['format']=='cc-volatile-vmagent-session/v1':return expected
    code=BINARY.parent.parent;held=[]
    def read(path,cap):
        fd,raw=regular(path,uid=0,mode=0o644,cap=cap)
        held.append((fd,path,identity(os.fstat(fd))));return raw
    try:
        current=json.loads(read(code/'current.json',4096),object_pairs_hook=object_pairs)
        version=current.get('version')
        need(type(version)is str and re.fullmatch(r'[0-9]{1,6}\.[0-9]{1,6}\.[0-9]{1,6}',version),'volatile_selected_version')
        release=code/'releases'/version
        record=json.loads(read(release/'installed.json',32768),object_pairs_hook=object_pairs)
        need(record.get('format')=='cc-agent-installed-release/v2'
             and record.get('files',{}).get('collector/vmagent-prod')==expected[2]
             and record.get('file_modes',{}).get('collector/vmagent-prod')=='0755','volatile_selected_release')
        name='delivery/release_platforms.py';raw=read(release/name,65536)
        need(record['files'].get(name)==hashlib.sha256(raw).hexdigest()
             and record['file_modes'].get(name)=='0644','volatile_selected_platform_code')
        import types
        module=types.ModuleType('_cc_volatile_selected_platforms')
        exec(compile(raw,str(release/name),'exec'),module.__dict__)
        row=module.installed_platform(current,record)
        need((row['machine'],row['elfMachine'],row['binarySha256'],row['observedBytes'])==expected,'volatile_selected_platform')
        fd,raw=regular(release/'collector/vmagent-prod',uid=0,mode=0o755,cap=23209584)
        held.append((fd,release/'collector/vmagent-prod',identity(os.fstat(fd))))
        need(len(raw)==expected[3] and hashlib.sha256(raw).hexdigest()==expected[2]
             and raw[:7]==b"\x7fELF\x02\x01\x01" and int.from_bytes(raw[18:20],'little')==expected[1],'volatile_selected_binary')
        need(all(identity(os.fstat(fd))==stamp==identity(path.lstat())for fd,path,stamp in held),'volatile_selected_release_changed')
        return expected
    finally:
        for fd,_,_ in held:os.close(fd)


def gate(session, config_hash):
    need(sys.platform == "linux", "volatile_platform")
    need(re.fullmatch("[0-9a-f]{32}", session) and uuid.UUID(hex=session).version == 4
         and re.fullmatch("[0-9a-f]{64}", config_hash), "volatile_input")
    uid = os.getuid()
    need(uid != 0 and os.geteuid() == uid, "volatile_uid")
    set_unit_root_view()
    directory = ROOT / session
    st = directory.lstat()
    need(stat.S_ISDIR(st.st_mode) and st.st_uid == uid and stat.S_IMODE(st.st_mode) == 0o700, "volatile_directory")
    proof = None; binary_fd = None; completed = False
    config_fd, raw = regular(directory / "session.json", uid=uid, mode=0o400, cap=65536)
    try:
        need(hashlib.sha256(raw).hexdigest() == config_hash, "volatile_config_hash")
        value = private_config(raw, session)
        expected=selected_runtime(value)
        need(value["bootId"] == read_text(Path("/proc/sys/kernel/random/boot_id")), "volatile_boot")
        for key in ("createdBoottimeNs", "deadlineBoottimeNs", "expiresUnixNs"):
            need(type(value[key]) is int, "volatile_deadline")
        now_boot = time.clock_gettime_ns(time.CLOCK_BOOTTIME)
        need(0 <= now_boot - value["createdBoottimeNs"] < 10_000_000_000
             and 0 < value["deadlineBoottimeNs"] - value["createdBoottimeNs"] <= 300_000_000_000, "volatile_start_window")
        left = value["deadlineBoottimeNs"] - now_boot
        need(0 < left <= 300_000_000_000 and time.time_ns() < value["expiresUnixNs"] <= time.time_ns() + 300_000_000_000, "volatile_expired")
        need(type(value["ownerPid"]) is int and value["ownerPid"] > 1
             and type(value["ownerStartTicks"]) is int and value["ownerStartTicks"] > 0
             and re.fullmatch("[0-9a-f]{64}", value["manifestSha256"]), "volatile_owner")
        owner = Path("/proc") / str(value["ownerPid"])
        need(owner.stat().st_uid == uid, "volatile_owner_uid")
        owner_stat = read_text(owner / "stat").rsplit(") ", 1)[1].split()
        need(owner_stat[0] != "Z" and int(owner_stat[19]) == value["ownerStartTicks"], "volatile_owner_changed")
        proof = namespace_proof(value["ownerPid"], value["ownerStartTicks"], uid)
        need(type(value["statusPort"]) is int and 1024 <= value["statusPort"] <= 65535, "volatile_port")
        need(type(value["receiver"]) is str and re.fullmatch(r"http://127\.0\.0\.1:[1-9][0-9]{0,4}/api/v1/(?:write|push)", value["receiver"])
             and 1 <= int(value["receiver"].split(":")[2].split("/")[0]) <= 65535, "volatile_receiver")
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
        need(len(mount) == 1, "volatile_mount")
        a, z = mount[0];options = set(a[5].split(",")) | set(z[2].split(","))
        need(z[0] == "tmpfs" and {"rw", "nosuid", "nodev", "noexec", "noswap"} <= options, "volatile_mount_options")
        need(len(roots) == 1 and "ro" in roots[0][5].split(","), "volatile_readonly_system")
        fs = os.statvfs(directory)
        need(fs.f_blocks * fs.f_frsize == TMPFS_BYTES and 0 < fs.f_files <= 4096
             and ROOT.stat().st_dev == st.st_dev, "volatile_mount_size")
        c = read_text(Path("/proc/self/cgroup"))
        need(c.startswith("0::/") and "\n" not in c and ".." not in c, "volatile_cgroup")
        cpath = c[3:];need(cpath.rsplit("/", 1)[-1] == value["unit"], "volatile_cgroup_unit")
        cg = Path("/sys/fs/cgroup") / cpath.lstrip("/")
        need(read_text(cg / "cgroup.type") == "domain", "volatile_cgroup_type")
        expected = {"memory.max": str(MAX_MEMORY), "memory.swap.max": "0", "pids.max": "64", "cpu.max": "100000 100000"}
        need(all(read_text(cg / k) == v for k, v in expected.items()), "volatile_kernel_limits")
        need(read_text(cg / "cgroup.procs") == str(os.getpid()), "volatile_initial_membership")
        for filename, required in (("memory.events", {"low", "high", "max", "oom", "oom_kill"}), ("pids.events", {"max"})):
            pairs = [line.split() for line in read_text(cg / filename).splitlines()]
            need(all(len(pair) == 2 for pair in pairs) and len(pairs) == len({x[0] for x in pairs}), "volatile_events")
            events = dict(pairs);need(required <= set(events) and all(re.fullmatch("[0-9]+", n) for n in events.values()), "volatile_events")
            need(all(int(events[k]) == 0 for k in required), "volatile_prior_resource_failure")
        invocation = os.environ.get("INVOCATION_ID", "")
        need(re.fullmatch("[0-9a-f]{32}", invocation), "volatile_invocation")
        scrape_fd, scrape = regular(directory / "scrape.json", uid=uid, mode=0o400, cap=65536)
        os.close(scrape_fd)
        need(hashlib.sha256(scrape).hexdigest() == value["scrapeSha256"], "volatile_scrape_hash")
        binary_fd, binary = regular(BINARY, uid=0, mode=0o755, cap=80 * 1024 * 1024)
        need(len(binary)==expected[3] and hashlib.sha256(binary).hexdigest()==expected[2]
             and binary[:7]==b"\x7fELF\x02\x01\x01" and int.from_bytes(binary[18:20],'little')==expected[1], "volatile_binary")
        need(not (directory / "queue").exists(), "volatile_old_queue")
        (directory / "queue").mkdir(mode=0o700)
        # No payload before actual cap/mount checks. Parent compares this to
        # systemd InvocationID/MainPID/start identity before publishing running.
        receipt = {"format": "cc-volatile-preexec/v1", "session": session, "invocationId": invocation,
                   "pid": os.getpid(), "cgroup": cpath, "binarySha256": expected[2],
                   "mountId": int(a[0]), "kernelLimits": expected,
                   "manifestSha256": value["manifestSha256"], "configSha256": config_hash}
        atomic_record(directory, "preexec.json", receipt)
        need(value["deadlineBoottimeNs"] > time.clock_gettime_ns(time.CLOCK_BOOTTIME)
             and value["expiresUnixNs"] > time.time_ns()
             and time.clock_gettime_ns(time.CLOCK_BOOTTIME) < value["createdBoottimeNs"] + 10_000_000_000, "volatile_expired")
        argv = [str(BINARY), "-promscrape.config=" + str(directory / "scrape.json"),
                "-httpListenAddr=127.0.0.1:" + str(value["statusPort"]), "-remoteWrite.url=" + value["receiver"],
                "-remoteWrite.tmpDataPath=" + str(directory / "queue"),
                "-remoteWrite.disableOnDiskQueue=true", "-remoteWrite.dropSamplesOnOverload=true",
                "-remoteWrite.queues=1", "-remoteWrite.inmemoryQueues=0", "-remoteWrite.forcePromProto=true",
                "-memory.allowedBytes=67108864", "-remoteWrite.retryMinInterval=1s", "-remoteWrite.retryMaxInterval=10s",
                "-remoteWrite.flushInterval=1s", "-remoteWrite.sendTimeout=3s", "-remoteWrite.tlsHandshakeTimeout=3s",
                "-remoteWrite.maxHourlySeries=50000", "-remoteWrite.maxDailySeries=50000",
                "-maxLabelsPerTimeseries=64", "-maxLabelNameLen=128", "-maxLabelValueLen=256", "-loggerLevel=ERROR"]
        os.chdir(directory)
        need(namespace_current(proof), "volatile_original_namespace")
        completed = True
        return binary_fd, argv, proof
    finally:
        os.close(config_fd)
        if not completed:
            if binary_fd is not None: os.close(binary_fd)
            if proof is not None:
                for fd in proof[0]: os.close(fd)


def main():
    need(len(sys.argv) == 3 and sys.argv[1].startswith("--session=") and sys.argv[2].startswith("--config-sha256="), "volatile_argv")
    signal.signal(signal.SIGTERM, cancel_signal);signal.signal(signal.SIGINT, cancel_signal)
    return supervise(sys.argv[1][10:], sys.argv[2][16:])


def cancel_signal(*_):
    global cancelled
    cancelled = True


def supervise(session, config_hash):
    binary_fd, argv, namespace = gate(session, config_hash)
    directory = ROOT / session;uid = os.getuid()
    config_fd = owner_fd = adapter_fd = child_fd = None
    child = None;child_start = None;joined = False;stopped = False;child_exit = None;kill_sent = False
    entry_pid = os.getpid();entry_start = process_identity(entry_pid, uid)[1]
    invocation = os.environ["INVOCATION_ID"]
    try:
        config_fd, raw = regular(directory / "session.json", uid=uid, mode=0o400, cap=65536)
        need(hashlib.sha256(raw).hexdigest() == config_hash, "volatile_config_changed")
        value = private_config(raw, session);config_identity = identity(os.fstat(config_fd))
        owner_fd = monitor_process(value["ownerPid"], value["ownerStartTicks"], uid)
        launch_fd, launch_raw = regular(directory / "launch-owner.json", uid=uid, mode=0o400, cap=4096)
        os.close(launch_fd);launch = json.loads(launch_raw, object_pairs_hook=object_pairs)
        need(set(launch) == {"format", "session", "configSha256", "pid", "startTicks", "parentPid", "parentStartTicks"}
             and launch["format"] == "cc-volatile-launch-owner/v1" and launch["session"] == session
             and launch["configSha256"] == config_hash and launch["parentPid"] == value["ownerPid"]
             and launch["parentStartTicks"] == value["ownerStartTicks"]
             and type(launch["pid"]) is int and launch["pid"] > 1 and type(launch["startTicks"]) is int,
             "volatile_launch_owner")
        adapter_fd = monitor_process(launch["pid"], launch["startTicks"], uid, value["ownerPid"])

        def original_live():
            if not namespace_current(namespace) or cancelled or value["deadlineBoottimeNs"] <= time.clock_gettime_ns(time.CLOCK_BOOTTIME) or value["expiresUnixNs"] <= time.time_ns():
                return False
            if identity(os.fstat(config_fd)) != config_identity or identity((directory / "session.json").lstat()) != config_identity:
                return False
            with selectors.DefaultSelector() as poll:
                poll.register(owner_fd, selectors.EVENT_READ);poll.register(adapter_fd, selectors.EVENT_READ)
                return not poll.select(0)

        need(original_live() and time.clock_gettime_ns(time.CLOCK_BOOTTIME) < value["createdBoottimeNs"] + 10_000_000_000, "volatile_expired")
        # One fixed verified ELF, not a request executable. exec follows this
        # held descriptor only; the same original capped unit contains both.
        child = subprocess.Popen(argv, executable="/proc/self/fd/" + str(binary_fd),
                                 pass_fds=(binary_fd,), close_fds=True, shell=False,
                                 stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
                                 cwd=directory, env={"LANG": "C", "GOMAXPROCS": "1"})
        child_parent, child_start = process_identity(child.pid, uid)
        need(child_parent == entry_pid, "volatile_child_parent")
        child_fd = monitor_process(child.pid, child_start, uid, entry_pid)
        binary = os.fstat(binary_fd);actual = (Path("/proc") / str(child.pid) / "exe").stat()
        need((actual.st_dev, actual.st_ino) == (binary.st_dev, binary.st_ino) and original_live(), "volatile_child_exec")
        cgroup = read_text(Path("/proc/self/cgroup"))[3:]
        members = {int(x) for x in read_text(Path("/sys/fs/cgroup") / cgroup.lstrip("/") / "cgroup.procs").splitlines()}
        need(members == {entry_pid, child.pid}, "volatile_child_membership")
        atomic_record(directory, "vmagent-exec.json", {
            "format": "cc-volatile-vmagent-exec/v1", "session": session, "invocationId": invocation,
            "unit": value["unit"], "entryPid": entry_pid, "entryStartTicks": entry_start,
            "childPid": child.pid, "childStartTicks": child_start,
            "childExeDev": actual.st_dev, "childExeIno": actual.st_ino,
            "vmagentSha256": expected_binary(value)[2], "configSha256": config_hash})
        while child.poll() is None:
            # Unknown/expiry/owner loss wins over a simultaneous Stop request.
            # BOOTTIME advances across suspend. Detection/kill is scheduled OS
            # work, not a zero-latency or zero-expired-byte guarantee on resume.
            if not original_live():break
            request = directory / "stop.request"
            if request.exists():
                request_fd, request_raw = regular(request, uid=uid, mode=0o400, cap=4096)
                os.close(request_fd);request_value = json.loads(request_raw, object_pairs_hook=object_pairs)
                need(request_value == {"format": "cc-volatile-stop-request/v1", "session": session, "configSha256": config_hash}, "volatile_stop_request")
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
                atomic_record(directory, "vmagent-exit.json", {
                    "format": "cc-volatile-vmagent-exit/v1", "session": session, "configSha256": config_hash,
                    "invocationId": invocation, "entryPid": entry_pid, "entryStartTicks": entry_start,
                    "childPid": child.pid, "childStartTicks": child_start,
                    "outcome": "stopped" if stopped and joined else "unknown", "childJoined": joined, "childExit": child_exit})
        except BaseException:stopped = False
        for fd in (*namespace[0], child_fd, adapter_fd, owner_fd, config_fd, binary_fd):
            if fd is not None:os.close(fd)
    return 0 if stopped and joined else 1


if __name__ == "__main__":
    raise SystemExit(main())
