"""Explicit root-only start-collector prerequisites for one dedicated account.

No Node/owner/grant/signing API, package download, sysctl repair or ambient-user
environment mutation. Only the installed fixed transaction invokes this file.
Failure preserves partial owned infrastructure for manual inspection.
"""
from contextlib import contextmanager
import fcntl
import grp
import hashlib
import ipaddress
import json
import os
from pathlib import Path
import pwd
import re
import selectors
import signal
import stat
import subprocess
import sys
import time
import uuid

CONFIG = Path("/etc/consolecrypt-agent/collector-runtime.json")
CATALOG = Path("/etc/consolecrypt-agent/collector-volatile.json")
OWNER = Path("/etc/consolecrypt-agent/collector-provisioned.json")
LOCK = Path("/etc/consolecrypt-agent/collector-provision.lock")
HOME = "/var/lib/consolecrypt-agent"
XDG = "/etc/consolecrypt-agent/collector-user"
ROOT = Path("/run/consolecrypt-agent/volatile")
MOUNT = r"run-consolecrypt\x2dagent-volatile.mount"
PROBE = Path("/usr/local/libexec/consolecrypt-agent-install/collector_namespace_probe.py")
PROBE_SHA256 = "43c794b216901134b7e483e92cb37a32739e36ed89e586981938f82ade0bd2c2"
PROFILE = "ubuntu24-systemd255-v1"
END = None
DISK = False
STORE = Path('/var/lib/consolecrypt-agent/metrics-disk')


def select_profile(config):
    # Administrative local file, never a wire request. No migration/adoption:
    # one fixed profile owns the same dedicated manager and one tmpfs mount.
    global DISK, CATALOG, ROOT, MOUNT, PROBE, PROBE_SHA256, PROFILE, OWNER
    need(set(config) == {'format', 'uid', 'gid'})
    if config['format'] == 'cc-agent-installed-collector-profile/v1':
        for path in (Path('/etc/consolecrypt-agent/collector-disk.json'),
                     Path('/etc/consolecrypt-agent/collector-disk-provisioned.json')):
            need(not path.exists() and not path.is_symlink(), 'collector_profile_conflict')
        return
    need(config['format'] == 'cc-agent-installed-disk-collector-profile/v1', 'collector_profile')
    DISK = True
    CATALOG = Path('/etc/consolecrypt-agent/collector-disk.json')
    OWNER = Path('/etc/consolecrypt-agent/collector-disk-provisioned.json')
    ROOT = Path('/run/consolecrypt-agent/disk')
    MOUNT = r'run-consolecrypt\x2dagent-disk.mount'
    PROBE = Path('/usr/local/libexec/consolecrypt-agent-install/collector_disk_namespace_probe.py')
    PROBE_SHA256 = '7d61c0c31cccd7207326ffac4d4a256b2cba54ad06e5289fa30f4eba8d8758ba'
    PROFILE = 'ubuntu24-systemd255-disk-v1'
    for path in (Path('/etc/consolecrypt-agent/collector-volatile.json'),
                 Path('/etc/consolecrypt-agent/collector-provisioned.json')):
        need(not path.exists() and not path.is_symlink(), 'collector_profile_conflict')


def disk_store(catalog, uid, gid, previous):
    """Create only an absent explicitly selected root; never erase/adopt/format."""
    need(DISK)
    manifest = catalog['manifest']
    v2 = type(manifest) is dict and manifest.get('schema_version') == 2
    need(type(manifest) is dict and set(manifest) == {'schema_version', 'mode', 'targets', 'labels',
         'scrape_interval_seconds', 'receiver', 'buffer', 'profile', 'bufferMode'} | ({'runtime'} if v2 else set())
         and manifest['profile'] == 'maintained-prometheus-disk-512mib-2h-v1'
         and manifest['bufferMode'] == 'persistentNativeWal'
         and manifest['buffer'] == {'max_bytes': 536870912, 'max_age_seconds': 7200}, 'collector_disk_manifest')
    digest = bytearray(hashlib.sha256(encode(manifest)[:-1]).digest()[:16])
    digest[6] = digest[6] & 15 | 0x80
    digest[8] = digest[8] & 63 | 0x80
    need(catalog['collectorId'] == str(uuid.UUID(bytes=bytes(digest))), 'collector_disk_id')
    for p in reversed(STORE.parents):
        st = p.lstat()
        need(stat.S_ISDIR(st.st_mode) and st.st_uid in (0, uid) and not st.st_mode & 0o022,
             'collector_disk_parent')
    root = STORE / catalog['collectorId']
    if previous is None:
        need(not STORE.exists() and not STORE.is_symlink(), 'collector_disk_unowned')
        STORE.mkdir(mode=0o700); os.chown(STORE, uid, gid)
        root.mkdir(mode=0o700); os.chown(root, uid, gid)
        (root / 'wal').mkdir(mode=0o700); os.chown(root / 'wal', uid, gid)
        for p, body in ((root / 'session.lock', b''), (root / 'wal/lock', b'cc-prom-agent-hardmode/v1\n')):
            fd = os.open(p, os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW | os.O_CLOEXEC, 0o600)
            try:
                os.fchown(fd, uid, gid); need(os.write(fd, body) == len(body)); os.fsync(fd)
            finally: os.close(fd)
        for p in (root / 'wal', root, STORE, STORE.parent):
            fd = os.open(p, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW | os.O_CLOEXEC)
            try: os.fsync(fd)
            finally: os.close(fd)
    facts = {}
    for label, p in (('store', STORE), ('root', root), ('wal', root / 'wal')):
        st = p.lstat()
        need(stat.S_ISDIR(st.st_mode) and st.st_uid == uid and st.st_gid == gid
             and stat.S_IMODE(st.st_mode) == 0o700, 'collector_disk_root')
        facts[label] = [st.st_dev, st.st_ino]
    need(facts['store'][0] == facts['root'][0] == facts['wal'][0], 'collector_disk_filesystem')
    need(set(p.name for p in STORE.iterdir()) == {catalog['collectorId']}, 'collector_disk_unknown_root')
    if previous is not None:
        need(decode(previous).get('diskRoot') == facts, 'collector_disk_changed')
    return facts


def need(ok, code="collector_provision_unavailable"):
    if not ok:
        raise ValueError(code)


def ident(st):
    return (st.st_dev, st.st_ino, st.st_mode, st.st_uid, st.st_gid,
            st.st_nlink, st.st_size, st.st_mtime_ns, st.st_ctime_ns)


def parents(path):
    for p in reversed(path.parent.parents):
        st = p.lstat()
        need(stat.S_ISDIR(st.st_mode) and st.st_uid == 0 and not st.st_mode & 0o022)
    st = path.parent.lstat()
    need(stat.S_ISDIR(st.st_mode) and st.st_uid == 0 and not st.st_mode & 0o022)


def root_file(path, cap=16384, mode=None):
    parents(path)
    fd = os.open(path, os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK | os.O_CLOEXEC)
    try:
        st = os.fstat(fd)
        need(stat.S_ISREG(st.st_mode) and st.st_uid == 0 and st.st_nlink == 1
             and not st.st_mode & 0o022 and 0 <= st.st_size <= cap
             and (mode is None or stat.S_IMODE(st.st_mode) == mode))
        raw = bytearray()
        while len(raw) <= cap:
            b = os.read(fd, min(1048576, cap + 1 - len(raw)))
            if not b:
                break
            raw.extend(b)
        need(len(raw) == st.st_size and ident(os.fstat(fd)) == ident(st)
             and ident(path.lstat()) == ident(st))
        parents(path)
        return bytes(raw)
    finally:
        os.close(fd)


def pairs(rows):
    need(len(rows) == len({k for k, _ in rows}))
    return dict(rows)


def decode(raw):
    return json.loads(raw, object_pairs_hook=pairs,
                      parse_float=lambda _: (_ for _ in ()).throw(ValueError("collector_profile")))


def encode(value):
    return json.dumps(value, sort_keys=True, separators=(",", ":"), ensure_ascii=True).encode("ascii") + b"\n"


def directory(path):
    if not path.exists():
        parents(path)
        path.mkdir(mode=0o755)
        # main deliberately uses umask077. Set the public traversal mode only
        # on this freshly created, root-owned child under trusted parents.
        created = path.lstat()
        fd = os.open(path, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW | os.O_CLOEXEC)
        try:
            held = os.fstat(fd)
            need(ident(held) == ident(created) and stat.S_ISDIR(held.st_mode)
                 and held.st_uid == 0 and stat.S_IMODE(held.st_mode) in (0o700, 0o755))
            os.fchmod(fd, 0o755)
            need(ident(os.fstat(fd)) == ident(path.lstat()))
        finally:
            os.close(fd)
    st = path.lstat()
    need(stat.S_ISDIR(st.st_mode) and st.st_uid == 0 and stat.S_IMODE(st.st_mode) == 0o755)


def publish(path, raw, *, mode=0o644, previous=None, previously_owned=False):
    parents(path)
    if path.exists() or path.is_symlink():
        observed = root_file(path, max(16384, len(raw)), mode)
        if previous is not None:
            need(observed == previous, "collector_foreign_file")
        else:
            need(previously_owned and observed == raw, "collector_unowned_file")
        if observed == raw:
            return
    else:
        need(previous is None and not previously_owned, "collector_owner_changed")
    # An exclusive owned sibling is never deleted on an ambiguous failure.
    temporary = path.with_name(path.name + ".cc-pending")
    fd = os.open(temporary, os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW | os.O_CLOEXEC, mode)
    try:
        view = memoryview(raw)
        while view:
            n = os.write(fd, view)
            need(n > 0)
            view = view[n:]
        os.fchmod(fd, mode)
        os.fsync(fd)
    finally:
        os.close(fd)
    if previous is not None:
        need(root_file(path, 16384, mode) == previous)
    else:
        need(not path.exists() and not path.is_symlink())
    os.replace(temporary, path)
    need(root_file(path, max(16384, len(raw)), mode) == raw)


def command(argv, *, uid=0, gid=0, seconds=5, cap=16384, exact_missing=False):
    left = min(seconds, END - time.monotonic())
    need(left > 0, "collector_provision_deadline")
    env = {"LANG": "C", "LC_ALL": "C", "PATH": "/usr/bin:/bin"}
    if uid:
        env.update(XDG_RUNTIME_DIR="/run/user/" + str(uid),
                   DBUS_SESSION_BUS_ADDRESS="unix:path=/run/user/" + str(uid) + "/bus")
    need(signal.getsignal(signal.SIGCHLD) == signal.SIG_DFL, "collector_child_custody")
    proc = subprocess.Popen(argv, stdin=subprocess.DEVNULL, stdout=subprocess.PIPE,
                            stderr=subprocess.DEVNULL, close_fds=True, shell=False,
                            cwd="/", env=env, user=uid, group=gid, extra_groups=[])
    raw = bytearray()
    stop = time.monotonic() + left
    try:
        os.set_blocking(proc.stdout.fileno(), False)
        with selectors.DefaultSelector() as poll:
            poll.register(proc.stdout, selectors.EVENT_READ)
            done = False
            while not done:
                need(time.monotonic() < stop, "collector_provision_deadline")
                for key, _ in poll.select(min(.05, max(0, stop - time.monotonic()))):
                    block = os.read(key.fd, min(4096, cap + 1 - len(raw)))
                    if not block:
                        done = True
                        break
                    raw.extend(block)
                    need(len(raw) <= cap, "collector_control_output")
        code = proc.wait(timeout=max(.001, stop - time.monotonic()))
        need(code == 0 or exact_missing and code == 4 and bytes(raw) == b"LoadState=not-found\n", "collector_control_exit")
        return bytes(raw)
    finally:
        # No poll/reap before kill: the original child PID remains reserved.
        if proc.returncode is None:
            proc.kill()
            proc.wait(timeout=2)
        proc.stdout.close()


def system(*argv):
    return command(["/usr/bin/systemctl", "--system", "--no-pager", "--no-ask-password", *argv])


def fields(raw):
    rows = [line.split("=", 1) for line in raw.decode("ascii").splitlines()]
    need(all(len(x) == 2 for x in rows))
    return pairs(rows)


def show(unit, names):
    row = fields(system("show", unit, "--property=" + ",".join(names)))
    need(set(row) == set(names))
    return row


@contextmanager
def retained_manager(row, uid):
    pid = int(row["MainPID"])
    proc = Path("/proc", str(pid))
    def observation():
        value = (proc / "stat").read_text().rsplit(") ", 1)[1].split()
        need(value[0] != "Z" and proc.stat().st_uid == uid, "collector_manager_process")
        return int(value[19])
    start = observation()
    fd = os.pidfd_open(pid)
    try:
        need(observation() == start, "collector_manager_changed")
        with selectors.DefaultSelector() as ready:
            ready.register(fd, selectors.EVENT_READ)
            need(not ready.select(0), "collector_manager_exited")
            yield start
            need(not ready.select(0) and observation() == start, "collector_manager_changed")
        final = show("user@" + str(uid) + ".service", ["ActiveState", "MainPID", "InvocationID"])
        need(final == {"ActiveState": "active", "MainPID": row["MainPID"], "InvocationID": row["InvocationID"]}, "collector_manager_changed")
    finally:
        os.close(fd)


def environment(uid):
    return {"HOME": HOME, "USER": "consolecrypt-agent", "LOGNAME": "consolecrypt-agent",
            "SHELL": "/usr/sbin/nologin", "PATH": "/usr/bin:/bin", "LANG": "C",
            "XDG_RUNTIME_DIR": "/run/user/" + str(uid), "XDG_CONFIG_HOME": XDG,
            "SYSTEMD_ENVIRONMENT_GENERATOR_PATH": "/dev/null"}


def files(uid, gid):
    env = environment(uid)
    manager = ("[Service]\nPAMName=\nEnvironment=\nEnvironmentFile=\nPassEnvironment=\n"
               + "Environment=" + " ".join(k + "=" + v for k, v in sorted(env.items()))
               + "\nDelegate=cpu memory pids\nMemoryMax=671088640\nMemorySwapMax=0\n"
                 "TasksMax=96\nCPUQuota=100%\nRestart=no\nUMask=0077\nIPAddressDeny=any\nIPAddressAllow=localhost\n")
    user = "[Manager]\nDefaultEnvironment=\nDefaultEnvironment=PATH=/usr/bin:/bin\n"
    mount = ("[Unit]\nDescription=ConsoleCrypt bounded volatile collector storage\n"
             "Before=consolecrypt-agent.service\n[Mount]\nWhat=tmpfs\n"
             "Where=/run/consolecrypt-agent/volatile\nType=tmpfs\n"
             "Options=rw,nosuid,nodev,noexec,noswap,size=33554432,nr_inodes=4096,mode=0700,uid="
             + str(uid) + ",gid=" + str(gid) + "\nDirectoryMode=0700\n")
    if DISK:
        # This is ONLY private session control tmpfs. Native WAL uses the new
        # explicitly provisioned persistent root, not a second spool/queue.
        mount = mount.replace('volatile collector storage', 'disk session control').replace(
            '/run/consolecrypt-agent/volatile', ROOT.as_posix())
    return {Path("/etc/systemd/system/user@" + str(uid) + ".service.d/60-consolecrypt-agent.conf"): manager.encode(),
            Path(XDG + "/systemd/user.conf"): user.encode(),
            Path("/etc/systemd/system") / MOUNT: mount.encode()}


def mount_fact(uid):
    rows = []
    raw = Path("/proc/self/mountinfo").read_bytes()
    need(len(raw) <= 262144)
    for line in raw.decode("ascii").splitlines():
        first, last = line.split(" - ", 1)
        a, b = first.split(), last.split()
        if a[4] == str(ROOT):
            rows.append((a, b))
    need(len(rows) == 1, "collector_mount")
    a, b = rows[0]
    need(b[0] == "tmpfs" and {"rw", "nosuid", "nodev", "noexec", "noswap"}
         <= set(a[5].split(",")) | set(b[2].split(",")), "collector_mount")
    st, fs = ROOT.lstat(), os.statvfs(ROOT)
    need(stat.S_ISDIR(st.st_mode) and st.st_uid == uid and stat.S_IMODE(st.st_mode) == 0o700
         and fs.f_blocks * fs.f_frsize == 33554432 and 0 < fs.f_files <= 4096, "collector_mount")
    return int(a[0])


def installed_volatile_catalog(catalog):
    # The fixed signed launcher validates ALL selected release files, normal
    # Agent ELF, copied platform helper, and selected vs stable collector bytes.
    # It creates no unit/process and cannot choose caller architecture.
    code=Path('/opt/consolecrypt-agent-install/opt/consolecrypt-agent')
    launch=code/'launch.py';raw=root_file(launch,131072,0o644)
    import types
    module=types.ModuleType('_cc_provision_selected_launcher');module.__file__=str(launch)
    exec(compile(raw,str(launch),'exec'),module.__dict__)
    binary=module.verified_identity_binary();release=binary.parent.parent
    record=decode(root_file(release/'installed.json',128*1024,0o644))
    platforms,row=module._selected_platform(code,release,record)
    module._stable_collector(code,release,record)
    manifest=catalog.get('manifest')
    keys={'schema_version','mode','targets','labels','scrape_interval_seconds','receiver','buffer','runtime'}
    expected=dict(kind='vmagent',upstreamTag=platforms.VERSION,platform=row['release'],
        binarySha256=row['binarySha256'],binaryBytes=row['observedBytes'])
    need(type(manifest)is dict and set(manifest)==keys and type(manifest['schema_version'])is int
         and manifest['schema_version']==2 and manifest['runtime']==expected,'collector_selected_runtime')
    return row['elfMachine']


def installed_disk_catalog(catalog):
    code = Path('/opt/consolecrypt-agent-install/opt/consolecrypt-agent')
    launch = code / 'launch.py';raw = root_file(launch, 131072, 0o644)
    import types
    module = types.ModuleType('_cc_provision_selected_disk_launcher');module.__file__ = str(launch)
    exec(compile(raw, str(launch), 'exec'), module.__dict__)
    binary = module.verified_identity_binary();release = binary.parent.parent
    record = decode(root_file(release / 'installed.json', 128*1024, 0o644))
    platforms, row = module._selected_platform(code, release, record)
    module._stable_disk(code, release, record)
    name = 'collector/prometheus-hardmode'
    payload = module._collector_bytes(release / name, 0o755, 192 * 1024 * 1024)
    platforms.require_elf(payload, row)
    expected = dict(kind='prometheusHardmode', upstreamTag='3.16.0', platform=row['release'],
                    binarySha256=hashlib.sha256(payload).hexdigest(), binaryBytes=len(payload))
    manifest = catalog.get('manifest')
    keys = {'schema_version', 'mode', 'targets', 'labels', 'scrape_interval_seconds',
            'receiver', 'buffer', 'profile', 'bufferMode', 'runtime'}
    need(type(manifest) is dict and set(manifest) == keys and type(manifest['schema_version']) is int
         and manifest['schema_version'] == 2 and manifest['runtime'] == expected
         and catalog.get('prometheusSha256') == expected['binarySha256']
         and record['files'].get(name) == expected['binarySha256'], 'collector_selected_disk_runtime')
    return row['elfMachine']


def provision():
    need(sys.platform == "linux" and os.getuid() == os.geteuid() == 0 and os.uname().machine in {"x86_64","aarch64"})
    config = decode(root_file(CONFIG, mode=0o600))
    select_profile(config)
    uid, gid = config["uid"], config["gid"]
    need(type(uid) is int and type(gid) is int and 0 < uid <= 2147483647 and 0 < gid <= 2147483647)
    account = pwd.getpwnam("consolecrypt-agent")
    need((account.pw_uid, account.pw_gid, account.pw_dir, account.pw_shell) == (uid, gid, HOME, "/usr/sbin/nologin"))
    users, groups = pwd.getpwall(), grp.getgrall()
    need(len(users) <= 4096 and len(groups) <= 4096
         and [x.pw_name for x in users if x.pw_uid == uid] == ["consolecrypt-agent"]
         and [x.gr_name for x in groups if x.gr_gid == gid] == ["consolecrypt-agent"]
         and all(not g.gr_mem or set(g.gr_mem) <= {"consolecrypt-agent"} for g in groups if g.gr_gid == gid))
    os_release = root_file(Path("/usr/lib/os-release"), 8192)
    need(b'ID=ubuntu\n' in os_release and b'VERSION_ID="24.04"\n' in os_release, "collector_platform")
    need(command(["/usr/bin/systemctl", "--version"]).splitlines()[0].split()[1] == b"255", "collector_systemd")
    # Trust is the explicitly configured root catalog pin, never a measurement
    # silently adopted as new trust. Full catalog/manifest validation stays in Rust.
    catalog = decode(root_file(CATALOG))
    keys = {"format", "collectorId", "manifest", "adapterSha256", "entrySha256", "specSha256", "interpreterSha256"}
    need(type(catalog) is dict and set(catalog) == keys | ({'prometheusSha256', 'bindingSha256'} if DISK else set())
         and catalog["format"] in ({'cc-agent-disk-collector/v1', 'cc-agent-disk-collector/v2'} if DISK else {'cc-agent-volatile-collector/v1','cc-agent-volatile-collector/v2'}))
    if DISK:
        need((catalog['format'] == 'cc-agent-disk-collector/v2')
             == (catalog.get('manifest', {}).get('schema_version') == 2), 'collector_disk_catalog_version')
    if catalog['format'] == 'cc-agent-disk-collector/v2':
        selected_machine = installed_disk_catalog(catalog)
    elif catalog['format']=='cc-agent-volatile-collector/v2':
        selected_machine=installed_volatile_catalog(catalog)
    else:
        need(os.uname().machine=='x86_64','collector_legacy_platform')
        selected_machine=62
    pin = catalog["interpreterSha256"]
    need(type(pin) is str and re.fullmatch("[0-9a-f]{64}", pin))
    python = root_file(Path("/usr/bin/python3.12"), 32 * 1024 * 1024, 0o755)
    need(python[:7] == b"\x7fELF\x02\x01\x01" and int.from_bytes(python[18:20],'little')==selected_machine
         and hashlib.sha256(python).hexdigest() == pin, "collector_interpreter_pin")
    probe_bytes = root_file(PROBE, 16384, 0o644)
    need(hashlib.sha256(probe_bytes).hexdigest() == PROBE_SHA256, "collector_probe_source")
    old_product = show("consolecrypt-agent.service", ["ActiveState", "MainPID"])
    need(old_product["ActiveState"] == "inactive" and old_product["MainPID"] == "0", "collector_original_not_joined")
    assets = files(uid, gid)
    descriptor = {"format": "cc-agent-collector-provision-owner/v1", "profile": PROFILE, "uid": uid, "gid": gid,
                  "assets": {str(p): hashlib.sha256(b).hexdigest() for p, b in assets.items()}}
    vendor = Path("/usr/lib/systemd/system/user@.service")
    vendor_bytes = root_file(vendor, 16384, 0o644)
    descriptor["vendorSha256"] = hashlib.sha256(vendor_bytes).hexdigest()
    # Ubuntu24's two fixed vendor drop-ins are supported OS inputs, not
    # administrator overrides. No other vendor or /etc drop-in is admitted.
    vendor_dropins = {}
    for name in ("10-login-barrier.conf", "timeout.conf"):
        path = Path("/usr/lib/systemd/system/user@.service.d") / name
        if path.exists() or path.is_symlink():
            body = root_file(path, 16384, 0o644)
            vendor_dropins[str(path)] = hashlib.sha256(body).hexdigest()
    descriptor["vendorDropins"] = vendor_dropins
    manager_unit = "user@" + str(uid) + ".service"
    dropin = "/etc/systemd/system/" + manager_unit + ".d/60-consolecrypt-agent.conf"
    previous = root_file(OWNER, mode=0o600) if OWNER.exists() or OWNER.is_symlink() else None
    if DISK and previous is not None:
        descriptor['diskRoot'] = disk_store(catalog, uid, gid, previous)
    existing = show(manager_unit, ["ActiveState", "MainPID", "FragmentPath", "DropInPaths"])
    need(existing["FragmentPath"] == str(vendor), "collector_foreign_manager_unit")
    def dropins_match(value, include_owned):
        observed = value.split()
        expected = set(vendor_dropins)
        if include_owned:
            expected.add(dropin)
        return len(observed) == len(set(observed)) and set(observed) == expected
    if previous is not None:
        record = decode(previous)
        need(record == dict(descriptor, state="ready"), "collector_partial_state")
        need(dropins_match(existing["DropInPaths"], True), "collector_foreign_manager_dropin")
        for path, content in assets.items():
            need(root_file(path, 16384, 0o644) == content, "collector_owned_asset_changed")
    else:
        need(existing["ActiveState"] == "inactive" and existing["MainPID"] == "0"
             and dropins_match(existing["DropInPaths"], False), "collector_foreign_manager")
        # No bare equal files can become ownership evidence after a crash.
        need(all(not path.exists() and not path.is_symlink() for path in assets), "collector_unowned_asset")
        mountinfo = Path("/proc/self/mountinfo").read_bytes()
        need(len(mountinfo) <= 262144)
        need(not any(line.split(" - ", 1)[0].split()[4] == str(ROOT)
                     for line in mountinfo.decode("ascii").splitlines()), "collector_foreign_mount")
    if DISK and previous is None:
        descriptor['diskRoot'] = disk_store(catalog, uid, gid, None)
    for p in assets:
        chain = []
        parent = p.parent
        while not parent.exists():
            chain.append(parent)
            parent = parent.parent
        for d in reversed(chain):
            directory(d)
        publish(p, assets[p], previously_owned=previous is not None)
    # Fixed runtime parent is either absent or the dedicated service's existing
    # nonwritable-to-others RuntimeDirectory. No existing path is chmod/chowned.
    parent = ROOT.parent
    if not parent.exists():
        directory(parent)
    pst = parent.lstat()
    need(stat.S_ISDIR(pst.st_mode) and pst.st_uid in (0, uid) and not pst.st_mode & 0o022
         and (pst.st_uid == uid or stat.S_IMODE(pst.st_mode) == 0o755), "collector_runtime_parent")
    if not ROOT.exists():
        ROOT.mkdir(mode=0o700)
    elif previous is None:
        st = ROOT.lstat()
        need(stat.S_ISDIR(st.st_mode) and st.st_uid in (0, uid) and stat.S_IMODE(st.st_mode) == 0o700 and not any(ROOT.iterdir()))
    configured = encode(dict(descriptor, state="configured"))
    publish(OWNER, configured, mode=0o600, previous=previous)
    system("daemon-reload")
    system("start", MOUNT, "user@" + str(uid) + ".service")
    manager = show("user@" + str(uid) + ".service", ["ActiveState", "MainPID", "InvocationID", "ControlGroup", "Delegate", "MemoryMax", "MemorySwapMax", "TasksMax", "CPUQuotaPerSecUSec", "PAMName", "DelegateControllers", "IPAddressDeny", "IPAddressAllow", "FragmentPath", "DropInPaths"])
    need(manager["ActiveState"] == "active" and re.fullmatch("[0-9a-f]{32}", manager["InvocationID"])
         and manager["MainPID"].isdecimal() and int(manager["MainPID"]) > 1 and manager["Delegate"] == "yes"
         and manager["MemoryMax"] == "671088640" and manager["MemorySwapMax"] == "0"
         and manager["TasksMax"] == "96" and manager["CPUQuotaPerSecUSec"] == "1s" and manager["PAMName"] == "", "collector_manager_profile")
    need(manager["FragmentPath"] == str(vendor) and dropins_match(manager["DropInPaths"], True), "collector_foreign_manager_unit")
    need(set(manager["DelegateControllers"].split()) == {"cpu", "memory", "pids"}, "collector_delegation")
    need({str(ipaddress.ip_network(x)) for x in manager["IPAddressDeny"].split()} == {"0.0.0.0/0", "::/0"}
         and {str(ipaddress.ip_network(x)) for x in manager["IPAddressAllow"].split()} == {"127.0.0.0/8", "::1/128"}, "collector_egress_policy")
    # These are requested/effective policy facts, not proof that the kernel has
    # attached/enforced cgroup BPF. The actual installation egress gate is separate.
    with retained_manager(manager, uid) as manager_start:
        cg = "/user.slice/user-" + str(uid) + ".slice/user@" + str(uid) + ".service"
        need(manager["ControlGroup"] == cg)
        systemd = Path("/usr/lib/systemd/systemd")
        root_file(systemd, 32 * 1024 * 1024, 0o755)
        pidroot = Path("/proc", manager["MainPID"])
        actual_exe = (pidroot / "exe").stat()
        need((actual_exe.st_dev, actual_exe.st_ino) == (systemd.stat().st_dev, systemd.stat().st_ino), "collector_manager_process")
        controllers = set(Path("/sys/fs/cgroup" + cg + "/cgroup.subtree_control").read_text().split())
        need({"cpu", "memory", "pids"} <= controllers, "collector_delegation")
        runtime = Path("/run/user") / str(uid)
        st, bus = runtime.lstat(), (runtime / "bus").lstat()
        need(stat.S_ISDIR(st.st_mode) and st.st_uid == uid and stat.S_IMODE(st.st_mode) == 0o700
             and stat.S_ISSOCK(bus.st_mode) and bus.st_uid == uid and stat.S_IMODE(bus.st_mode) == 0o666 and bus.st_nlink == 1, "collector_user_bus")
        socket_identity = ident(bus)
        actual = fields(command(["/usr/bin/systemctl", "--user", "show-environment"], uid=uid, gid=gid))
        expected = environment(uid)
        if "DBUS_SESSION_BUS_ADDRESS" in actual:
            expected["DBUS_SESSION_BUS_ADDRESS"] = "unix:path=/run/user/" + str(uid) + "/bus"
        need(actual == expected, "collector_manager_environment")
        mid = mount_fact(uid)
        # Real same-user sandbox prerequisite. It cannot start vmagent or touch
        # authority; no sysctl/capability fallback is available if namespaces fail.
        unit = "cc-agent-collector-prerequisite.service"
        absent = command(["/usr/bin/systemctl", "--user", "show", unit, "--property=LoadState"], uid=uid, gid=gid, exact_missing=True)
        need(absent.strip() == b"LoadState=not-found", "collector_probe_collision")
        argv = ["/usr/bin/systemd-run", "--user", "--quiet", "--wait", "--pipe", "--collect", "--unit=" + unit,
                "--property=Type=exec", "--property=RuntimeMaxSec=5s", "--property=MemoryMax=64M", "--property=MemorySwapMax=0",
                "--property=TasksMax=8", "--property=CPUQuota=10%", "--property=NoNewPrivileges=yes", "--property=ProtectSystem=strict",
                "--property=PrivateDevices=yes", "--property=ProtectHome=yes",
                "--property=ReadWritePaths=" + str(ROOT), "--", "/usr/bin/python3.12", "-I", "-B", str(PROBE), str(uid), str(gid)]
        if DISK:
            argv.insert(argv.index('--'), '--property=PrivateUsers=yes')
        need(command(argv, uid=uid, gid=gid, seconds=10).strip() == b"CC_COLLECTOR_NAMESPACE_OK", "collector_userns_unsupported")
        need(ident((runtime / "bus").lstat()) == socket_identity, "collector_bus_changed")
        result = {"format": "cc-agent-collector-provision-result/v1", "profile": PROFILE, "uid": uid, "gid": gid,
                "managerInvocationId": manager["InvocationID"], "managerPid": int(manager["MainPID"]), "managerStartTicks": manager_start,
                "mountId": mid, "environmentSha256": hashlib.sha256(encode(actual)).hexdigest(),
                "controllers": ["cpu", "memory", "pids"], "namespaceProbeExit": 0}
    need(hashlib.sha256(root_file(vendor, 16384, 0o644)).hexdigest() == descriptor["vendorSha256"], "collector_vendor_changed")
    for name in ("10-login-barrier.conf", "timeout.conf"):
        path = Path("/usr/lib/systemd/system/user@.service.d") / name
        if path.exists() or path.is_symlink():
            need(str(path) in vendor_dropins and hashlib.sha256(root_file(path, 16384, 0o644)).hexdigest()
                 == vendor_dropins[str(path)], "collector_vendor_changed")
        else:
            need(str(path) not in vendor_dropins, "collector_vendor_changed")
    # The retained-manager context completes its final pidfd/start/invocation
    # checks BEFORE ready is published. Any failed attempt leaves configured.
    publish(OWNER, encode(dict(descriptor, state="ready")), mode=0o600, previous=configured)
    return result


def main():
    global END
    END = time.monotonic() + 55
    need(sys.argv[1:] == ["--provision"])
    os.umask(0o077)
    parents(LOCK)
    fd = os.open(LOCK, os.O_RDWR | os.O_CREAT | os.O_NOFOLLOW | os.O_CLOEXEC, 0o600)
    try:
        st = os.fstat(fd)
        need(stat.S_ISREG(st.st_mode) and st.st_uid == 0 and st.st_nlink == 1 and stat.S_IMODE(st.st_mode) == 0o600 and st.st_size == 0)
        fcntl.flock(fd, fcntl.LOCK_EX | fcntl.LOCK_NB)
        raw = encode(provision())
        need(len(raw) <= 4096)
        sys.stdout.buffer.write(raw)
    finally:
        os.close(fd)


if __name__ == "__main__":
    try:
        main()
    except BaseException:
        # Deliberately no raw path/environment/exception, nor automatic rollback
        # of a partly-created root-owned manager or mount after an unknown exit.
        print("collector_provision_unavailable", file=sys.stderr)
        raise SystemExit(1)
