"""Fixed finite service unit. Actor authority stays in the original Rust daemon.

Reuses the existing installed disk OS gate/supervisor; no alternate WAL/queue.
The original interactive entry and its300s checks remain byte-unchanged.
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


# Failure-only observation in an EXISTING allowed public leaf. This different
# format cannot satisfy either preexec or authenticated process-exit admission.
_START_STAGE = "module"
_START_DIAGNOSTIC_ALLOWED = True
_START_STAGES = ('module', 'package', 'imports', 'platform', 'input', 'uid', 'namespace', 'directory', 'config_open', 'config', 'boot', 'start_window', 'deadline', 'owner', 'namespace_proof', 'target', 'mount', 'cgroup', 'invocation', 'scrape', 'binary', 'binding', 'preexec', 'argv', 'manager_environment', 'unit_absent', 'unit_start', 'unit_running')
_START_CODES = ('disk_argv', 'disk_binary', 'disk_binary_bound', 'disk_binary_changed', 'disk_binary_digest', 'disk_binary_parent', 'disk_binding', 'disk_binding_changed', 'disk_binding_code', 'disk_binding_digest', 'disk_binding_write', 'disk_boot', 'disk_caps_changed', 'disk_cgroup', 'disk_cgroup_identity', 'disk_cgroup_type', 'disk_cgroup_unit', 'disk_child_changed', 'disk_child_exec', 'disk_child_identity', 'disk_child_membership', 'disk_child_parent', 'disk_cleanup_deadline', 'disk_cleanup_directory', 'disk_cleanup_entries', 'disk_cleanup_leaf', 'disk_cleanup_unknown', 'disk_closed', 'disk_collector', 'disk_collector_id', 'disk_config', 'disk_config_canonical', 'disk_config_changed', 'disk_config_digest', 'disk_config_hash', 'disk_config_integer', 'disk_control_deadline', 'disk_control_exit', 'disk_control_output', 'disk_deadline', 'disk_directory', 'disk_duplicate', 'disk_entry_not_execed', 'disk_events', 'disk_existing_unit', 'disk_expired', 'disk_file_changed', 'disk_float', 'disk_handoff', 'disk_handoff_hash', 'disk_handoff_unit', 'disk_host_namespace', 'disk_id', 'disk_initial_membership', 'disk_input', 'disk_invocation', 'disk_kernel_bound', 'disk_kernel_limits', 'disk_launch_owner', 'disk_leader_not_captured', 'disk_legacy_platform', 'disk_live_cgroup_missing', 'disk_manager_environment', 'disk_manifest_address', 'disk_manifest_binding', 'disk_memory', 'disk_mount', 'disk_mount_identity', 'disk_mount_options', 'disk_mount_size', 'disk_namespace', 'disk_namespace_bound', 'disk_native_layout', 'disk_native_probe_already_held', 'disk_never_bound_unit', 'disk_original_namespace', 'disk_original_parent', 'disk_original_parent_changed', 'disk_original_window', 'disk_owned_membership', 'disk_owner', 'disk_owner_changed', 'disk_owner_kind', 'disk_owner_uid', 'disk_package', 'disk_parent', 'disk_pid_changed', 'disk_pid_dead', 'disk_pid_replaced', 'disk_pid_uid', 'disk_pipe_replaced', 'disk_platform', 'disk_port', 'disk_preexec_identity', 'disk_prior_resource_failure', 'disk_private_control', 'disk_private_namespace', 'disk_private_pipes', 'disk_process_changed', 'disk_process_exited', 'disk_process_uid', 'disk_prometheus_not_execed', 'disk_readiness_deadline', 'disk_readonly_system', 'disk_receiver', 'disk_record_bound', 'disk_record_exists', 'disk_record_leaf', 'disk_record_write', 'disk_regular', 'disk_resource_events', 'disk_resource_failed', 'disk_root', 'disk_root_changed', 'disk_root_entries', 'disk_runtime', 'disk_runtime_binary', 'disk_runtime_metadata', 'disk_runtime_readback', 'disk_scrape_hash', 'disk_selected_binary', 'disk_selected_changed', 'disk_selected_helper', 'disk_selected_platform', 'disk_selected_version', 'disk_session', 'disk_session_changed', 'disk_session_lock', 'disk_start_denied', 'disk_start_unknown', 'disk_start_window', 'disk_status_port', 'disk_stop_request', 'disk_terminal_bound', 'disk_uid', 'disk_unbound_nonempty', 'disk_unit_exit_policy', 'disk_unit_identity', 'disk_unit_limits', 'disk_unit_observation', 'disk_unit_replaced', 'disk_unknown', 'disk_user_namespace', 'disk_wal_changed', 'disk_wal_root', 'service_binding', 'service_collector', 'service_config_digest', 'service_disk_binding', 'service_disk_binding_changed', 'service_disk_digest', 'service_disk_expiry', 'service_disk_id', 'service_disk_revision', 'service_entry', 'service_original_window', 'service_package', 'service_policy_expiry', 'service_runtime', 'service_session', 'service_user_namespace')
_START_STATES = frozenset(("active", "inactive", "failed", "activating", "deactivating",
    "running", "dead", "exited", "start", "start-post", "stop", "stop-sigterm", "stop-sigkill",
    "success", "exit-code", "signal", "core-dump", "timeout", "oom-kill", "resources",
    "start-limit-hit", "not-found", "loaded", "error", "unknown"))


_START_MOUNT = None
_MOUNT_NUMBERS = ('fdMountId','rootFdMountId','matchedFdIdRows','acceptedPathRows','matchedRootIdRows',
    'rootPathRows','sessionPathRows','fsBytes','fsFiles','directoryDevMajor','directoryDevMinor','rootDevMajor','rootDevMinor')
_MOUNT_BOOLEANS = ('fdPathIsRoot','fdPathIsSession','fdPathIsOther','fdFsTmpfs','rootReadOnly','rootDirectorySameDev',
    'rw','nosuid','nodev','noexec','noswap')

def _closed_mount_witness(value):
    if type(value)is not dict or set(value)!=set(_MOUNT_NUMBERS+_MOUNT_BOOLEANS):return None
    if any(value[k]is not None and (type(value[k])is not int or not 0<=value[k]<=2**64-1)for k in _MOUNT_NUMBERS):return None
    if any(value[k]is not None and type(value[k])is not bool for k in _MOUNT_BOOLEANS):return None
    return dict(value)

def _mount_failure_witness(rows,actual_mount,actual_root,root,directory,directory_stat):
    # SAME already-read mountinfo, no raw path/options leave this helper. Only
    # metadata reads; failures remain absent/unknown, never authority or zero.
    try:
        if len(rows)>4096:return None
        selected=[(a,z)for a,z in rows if a[0]==actual_mount]
        roots=[a for a,z in rows if a[0]==actual_root and a[4]=='/']
        item=selected[0]if len(selected)==1 else None
        flags=(set(item[0][5].split(','))|set(item[1][2].split(',')))if item is not None else None
        filesystem=os.statvfs(directory);root_stat=root.stat()
        value=dict(fdMountId=int(actual_mount),rootFdMountId=int(actual_root),matchedFdIdRows=len(selected),
            acceptedPathRows=sum(a[4]in(str(root),str(directory))for a,z in selected),matchedRootIdRows=len(roots),
            rootPathRows=sum(a[4]==str(root)for a,z in rows),sessionPathRows=sum(a[4]==str(directory)for a,z in rows),
            fdPathIsRoot=item[0][4]==str(root)if item is not None else None,
            fdPathIsSession=item[0][4]==str(directory)if item is not None else None,
            fdPathIsOther=item[0][4]not in(str(root),str(directory))if item is not None else None,
            fdFsTmpfs=item[1][0]=='tmpfs'if item is not None else None,
            rootReadOnly='ro'in roots[0][5].split(',')if len(roots)==1 else None,
            fsBytes=filesystem.f_blocks*filesystem.f_frsize,fsFiles=filesystem.f_files,
            directoryDevMajor=os.major(directory_stat.st_dev),directoryDevMinor=os.minor(directory_stat.st_dev),
            rootDevMajor=os.major(root_stat.st_dev),rootDevMinor=os.minor(root_stat.st_dev),
            rootDirectorySameDev=root_stat.st_dev==directory_stat.st_dev)
        for name in ('rw','nosuid','nodev','noexec','noswap'):value[name]=name in flags if flags is not None else None
        return _closed_mount_witness(value)
    except BaseException:return None

def _closed_start_failure(source, stage, error, unit=None, mount=None):
    code = str(error) if type(error) is ValueError and str(error) in _START_CODES else "unknown"
    number = error.errno if isinstance(error, OSError) else None
    number = number if type(number) is int and 0 <= number <= 4095 else None
    safe = {}
    if type(unit) is dict:
        for key in ("LoadState", "ActiveState", "SubState", "Result", "ExecMainCode", "ExecMainStatus"):
            value = unit.get(key)
            if type(value) is str and value in _START_STATES:safe[key] = value
            elif type(value) is str and value.isascii() and value.isdecimal() and len(value) <= 10:
                safe[key] = int(value)
    record = {"format":"cc-collector-service-start-failure/v1",
        "source":source if source in ("adapter", "entry") else "unknown",
        "stage":stage if stage in _START_STAGES else "unknown", "code":code, "errno":number,
        "unit":safe}
    if mount is not None:
        witness=_closed_mount_witness(mount)
        if witness is not None:record["mount"]=witness
    return record

def _record_start_failure(directory, source, stage, error, unit=None, mount=None):
    # Never replace an original process-exit receipt or an earlier failure.
    # O_EXCL, held directory/leaf identities, UID/mode/nlink, and <=2KiB apply.
    parent_fd = fd = None; leaf = None; published = False
    try:
        if directory.parent != Path("/run/consolecrypt-agent/disk") or re.fullmatch("[0-9a-f]{32}", directory.name) is None:return False
        m = directory.lstat()
        if not stat.S_ISDIR(m.st_mode) or m.st_uid != os.getuid() or stat.S_IMODE(m.st_mode) != 0o700:return False
        parent_fd = os.open(directory, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW | os.O_CLOEXEC)
        held = os.fstat(parent_fd)
        if (m.st_dev,m.st_ino) != (held.st_dev,held.st_ino):return False
        raw = json.dumps(_closed_start_failure(source,stage,error,unit,mount),sort_keys=True,separators=(",",":"),ensure_ascii=True).encode("ascii")
        if len(raw) > 2048:return False
        fd = os.open("prometheus-exit.json",os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW | os.O_CLOEXEC,0o600,dir_fd=parent_fd)
        os.fchmod(fd,0o600)
        leaf = os.fstat(fd)
        if not stat.S_ISREG(leaf.st_mode) or leaf.st_uid != os.getuid() or leaf.st_nlink != 1:return False
        pending = memoryview(raw)
        while pending:
            count = os.write(fd,pending)
            if count <= 0:return False
            pending = pending[count:]
        os.fsync(fd)
        after = os.stat("prometheus-exit.json",dir_fd=parent_fd,follow_symlinks=False)
        actual = os.fstat(fd)
        published = ((after.st_dev,after.st_ino) == (leaf.st_dev,leaf.st_ino) == (actual.st_dev,actual.st_ino)
            and actual.st_nlink == 1 and actual.st_uid == os.getuid() and stat.S_IMODE(actual.st_mode) == 0o600
            and actual.st_size == len(raw)
            and (directory.lstat().st_dev,directory.lstat().st_ino) == (held.st_dev,held.st_ino))
        return published
    except BaseException:return False # Observation can never replace the primary error.
    finally:
        if fd is not None and leaf is not None and not published:
            try:
                named = os.stat("prometheus-exit.json",dir_fd=parent_fd,follow_symlinks=False)
                if (named.st_dev,named.st_ino) == (leaf.st_dev,leaf.st_ino):
                    os.unlink("prometheus-exit.json",dir_fd=parent_fd)
            except BaseException:pass
        if fd is not None:
            try:os.close(fd)
            except BaseException:pass
        if parent_fd is not None:
            try:os.close(parent_fd)
            except BaseException:pass

def _entry_start_failure(kind, error, traceback):
    try:
        if _START_DIAGNOSTIC_ALLOWED and len(sys.argv) == 3 and sys.argv[1].startswith("--session="):
            _record_start_failure(Path("/run/consolecrypt-agent/disk") / sys.argv[1][10:],"entry",_START_STAGE,error,mount=globals().get("_START_MOUNT"))
    except BaseException:pass

if __name__ == "__main__":sys.excepthook = _entry_start_failure

def _failure_no_late_producer(attempted, cleanup):
    return (attempted is False or type(cleanup) is dict
        and cleanup.get("unitStopped") is True and cleanup.get("cgroupEmpty") is True
        and cleanup.get("leaderExited") is True and cleanup.get("unknown") is not True)

_START_STAGE = "package"
PACKAGE=Path("/opt/consolecrypt-agent-install/opt/consolecrypt-agent/collector")
def _root_view():
    uid,gid=os.getuid(),os.getgid()
    a=Path("/proc/self/uid_map").read_text().split();b=Path("/proc/self/gid_map").read_text().split()
    if a==b==["0","0","4294967295"]:return 0
    if uid in (0,65534)or gid in (0,65534)or a!=[str(uid),str(uid),"1"]or b!=[str(gid),str(gid),"1"]:
        raise ValueError("service_user_namespace")
    if Path("/proc/sys/kernel/overflowuid").read_text().strip()!="65534"or Path("/proc/sys/kernel/overflowgid").read_text().strip()!="65534":
        raise ValueError("service_user_namespace")
    return 65534
_root=_root_view()
for _p in (*reversed(PACKAGE.parents),PACKAGE):
    _s=_p.lstat()
    if not stat.S_ISDIR(_s.st_mode)or stat.S_ISLNK(_s.st_mode)or _s.st_uid!=_root or _s.st_mode&0o022:raise ValueError("service_package")
for _n in ("disk_unit_entry.py","disk_binding.py","service_binding.py"):
    _s=(PACKAGE/_n).lstat()
    if not stat.S_ISREG(_s.st_mode)or _s.st_uid!=_root or _s.st_nlink!=1 or stat.S_IMODE(_s.st_mode)!=0o644:raise ValueError("service_package")
sys.path.insert(0,str(PACKAGE))
_START_STAGE = "imports"
from disk_unit_entry import (ROOT,BINARY,MAX_MEMORY,TMPFS_BYTES,atomic_record,binding_types as _old_binding_types,
    fixed_argv,identity,monitor_process,mount_id,namespace_current,namespace_proof,need,object_pairs,
    pinned_binary,process_identity,read_text,regular,set_unit_root_view,runtime_record)
from service_binding import ServiceDiskBinding
MAX_LIFETIME_NS=86400*1_000_000_000
cancelled=False

def binding_types():
    Original,BoundDiskRoot=_old_binding_types()
    def parsed_binding(**value):
        need(type(value.get("original"))is dict,"service_binding")
        args=dict(value);args["original"]=Original(**args["original"])
        return ServiceDiskBinding(**args).checked()
    return parsed_binding,BoundDiskRoot

def private_config(raw, session):
    value = json.loads(raw, object_pairs_hook=object_pairs,
                       parse_float=lambda _: (_ for _ in ()).throw(ValueError("disk_float")))
    keys = {"format", "session", "unit", "bootId", "createdBoottimeNs", "deadlineBoottimeNs", "expiresUnixNs", "scrapeSha256", "receiver", "statusPort", "ownerPid", "ownerStartTicks", "manifestSha256", "binarySha256", "binding"}
    v2 = type(value) is dict and value.get('format') == 'cc-collector-service-session/v2'
    need(type(value) is dict and set(value) == keys | ({'runtime'} if v2 else set())
         and value["format"] in {"cc-collector-service-session/v1", "cc-collector-service-session/v2"}
         and value["session"] == session and value["unit"] == "cc-agent-disk-" + session + ".service", "disk_config")
    need(json.dumps(value, sort_keys=True, separators=(",", ":"), ensure_ascii=True).encode("ascii") == raw, "disk_config_canonical")
    for key in ("createdBoottimeNs", "deadlineBoottimeNs", "expiresUnixNs", "ownerPid", "ownerStartTicks", "statusPort"):
        need(type(value[key]) is int and value[key] > 0, "disk_config_integer")
    for key in ("scrapeSha256", "manifestSha256", "binarySha256"):
        need(type(value[key]) is str and re.fullmatch("[0-9a-f]{64}", value[key]), "disk_config_digest")
    need(type(value["binding"]) is dict, "disk_binding")
    DiskBinding, _ = binding_types()
    binding = DiskBinding(**value["binding"]).checked()
    need(binding.original.manifest_sha256 == value["manifestSha256"], "disk_manifest_binding")
    need(0 < value["expiresUnixNs"] <= binding.expires_unix_seconds * 1_000_000_000, "service_policy_expiry")
    if v2:
        runtime_record(value['runtime'])
        need(value['runtime']['binarySha256'] == value['binarySha256'], 'disk_runtime_binary')
    return value


def gate(session, config_hash):
    global _START_STAGE, _START_MOUNT
    _START_STAGE = "platform"
    need(sys.platform == "linux" and os.uname().machine in {'x86_64', 'aarch64'}, "disk_platform")
    _START_STAGE = "input"
    need(re.fullmatch("[0-9a-f]{32}", session) and uuid.UUID(hex=session).version == 4
         and re.fullmatch("[0-9a-f]{64}", config_hash), "disk_input")
    _START_STAGE = "uid"
    uid = os.getuid()
    need(uid != 0 and os.geteuid() == uid, "disk_uid")
    _START_STAGE = "namespace"
    set_unit_root_view()
    _START_STAGE = "directory"
    directory = ROOT / session
    st = directory.lstat()
    need(stat.S_ISDIR(st.st_mode) and st.st_uid == uid and stat.S_IMODE(st.st_mode) == 0o700, "disk_directory")
    proof = None; binary_fd = None; disk = None; completed = False
    _START_STAGE = "config_open"
    config_fd, raw = regular(directory / "session.json", uid=uid, mode=0o400, cap=65536)
    try:
        need(hashlib.sha256(raw).hexdigest() == config_hash, "disk_config_hash")
        _START_STAGE = "config"
        value = private_config(raw, session)
        need(value.get('runtime') is not None or os.uname().machine == 'x86_64', 'disk_legacy_platform')
        _START_STAGE = "boot"
        need(value["bootId"] == read_text(Path("/proc/sys/kernel/random/boot_id")), "disk_boot")
        for key in ("createdBoottimeNs", "deadlineBoottimeNs", "expiresUnixNs"):
            need(type(value[key]) is int, "disk_deadline")
        _START_STAGE = "start_window"
        now_boot = time.clock_gettime_ns(time.CLOCK_BOOTTIME)
        need(0 <= now_boot - value["createdBoottimeNs"] < 10_000_000_000
             and 0 < value["deadlineBoottimeNs"] - value["createdBoottimeNs"] <= MAX_LIFETIME_NS, "disk_start_window")
        _START_STAGE = "deadline"
        left = value["deadlineBoottimeNs"] - now_boot
        need(0 < left <= MAX_LIFETIME_NS and time.time_ns() < value["expiresUnixNs"] <= time.time_ns() + MAX_LIFETIME_NS, "disk_expired")
        _START_STAGE = "owner"
        need(type(value["ownerPid"]) is int and value["ownerPid"] > 1
             and type(value["ownerStartTicks"]) is int and value["ownerStartTicks"] > 0
             and re.fullmatch("[0-9a-f]{64}", value["manifestSha256"]), "disk_owner")
        owner = Path("/proc") / str(value["ownerPid"])
        need(owner.stat().st_uid == uid, "disk_owner_uid")
        owner_stat = read_text(owner / "stat").rsplit(") ", 1)[1].split()
        need(owner_stat[0] != "Z" and int(owner_stat[19]) == value["ownerStartTicks"], "disk_owner_changed")
        _START_STAGE = "namespace_proof"
        proof = namespace_proof(value["ownerPid"], value["ownerStartTicks"], uid)
        _START_STAGE = "target"
        need(type(value["statusPort"]) is int and 1024 <= value["statusPort"] <= 65535, "disk_port")
        need(type(value["receiver"]) is str and re.fullmatch(r"http://127\.0\.0\.1:[1-9][0-9]{0,4}/api/v1/(?:write|push)", value["receiver"])
             and 1 <= int(value["receiver"].split(":")[2].split("/")[0]) <= 65535, "disk_receiver")
        # No claim that loopback itself prevents HTTP redirects. A separately
        # verified nonredirecting installed receiver is a mandatory admission prerequisite.
        _START_STAGE = "mount"
        mount = []
        roots = []
        observed_mount_rows = []
        actual_mount, actual_root = mount_id(directory), mount_id(Path("/"))
        for line in read_text(Path("/proc/self/mountinfo"), 262144).splitlines():
            first, last = line.split(" - ", 1);a, z = first.split(), last.split()
            observed_mount_rows.append((a,z))
            if a[0] == actual_mount and a[4] in (str(ROOT), str(directory)):
                mount.append((a, z))
            if a[0] == actual_root and a[4] == "/":
                roots.append(a)
        _START_MOUNT = _mount_failure_witness(observed_mount_rows,actual_mount,actual_root,ROOT,directory,st)
        need(len(mount) == 1, "disk_mount")
        a, z = mount[0];options = set(a[5].split(",")) | set(z[2].split(","))
        need(z[0] == "tmpfs" and {"rw", "nosuid", "nodev", "noexec", "noswap"} <= options, "disk_mount_options")
        need(len(roots) == 1 and "ro" in roots[0][5].split(","), "disk_readonly_system")
        fs = os.statvfs(directory)
        need(fs.f_blocks * fs.f_frsize == TMPFS_BYTES and 0 < fs.f_files <= 4096
             and ROOT.stat().st_dev == st.st_dev, "disk_mount_size")
        _START_STAGE = "cgroup"
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
        _START_STAGE = "invocation"
        invocation = os.environ.get("INVOCATION_ID", "")
        need(re.fullmatch("[0-9a-f]{32}", invocation), "disk_invocation")
        _START_STAGE = "scrape"
        scrape_fd, scrape = regular(directory / "scrape.json", uid=uid, mode=0o400, cap=65536)
        os.close(scrape_fd)
        need(hashlib.sha256(scrape).hexdigest() == value["scrapeSha256"], "disk_scrape_hash")
        _START_STAGE = "binary"
        binary_fd = pinned_binary(value["binarySha256"], value.get('runtime'))
        _START_STAGE = "binding"
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
        _START_STAGE = "preexec"
        atomic_record(directory, "preexec.json", receipt)
        need(value["deadlineBoottimeNs"] > time.clock_gettime_ns(time.CLOCK_BOOTTIME)
             and value["expiresUnixNs"] > time.time_ns()
             and time.clock_gettime_ns(time.CLOCK_BOOTTIME) < value["createdBoottimeNs"] + 10_000_000_000, "disk_expired")
        _START_STAGE = "argv"
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
    global _START_DIAGNOSTIC_ALLOWED
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
        _START_DIAGNOSTIC_ALLOWED = False # No diagnostic leaf once a real exit producer can exist.
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
