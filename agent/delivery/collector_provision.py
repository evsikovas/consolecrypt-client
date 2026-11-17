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
import struct
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
    PROBE_SHA256 = '8c29f2b131804063c085a701c201ec7a975c9b7b0476d5a43624b4d69f957c3b'
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


ELF_CAP = 32 * 1024 * 1024
ELF_SEARCH_PATHS = ('/usr/lib/aarch64-linux-gnu/systemd', '/usr/lib/aarch64-linux-gnu',
                    '/usr/lib/systemd', '/lib/aarch64-linux-gnu', '/lib/systemd')

def vendor_elf_facts(raw):
    """Bounded ELF64/AArch64 dynamic metadata, without executing the image."""
    need(type(raw) is bytes and 64 <= len(raw) <= ELF_CAP, 'collector_arm_elf_size')
    need(raw[:7] == b'\x7fELF\x02\x01\x01', 'collector_arm_elf_format')
    header = struct.unpack_from('<HHIQQQIHHHHHH', raw, 16)
    kind, machine, version, _, phoff, _, _, ehsize, phsize, phnum, _, _, _ = header
    need(kind in (2, 3) and machine == 183 and version == 1 and ehsize == 64
         and phsize == 56 and 1 <= phnum <= 128 and phoff >= 64
         and phoff + phsize * phnum <= len(raw), 'collector_arm_elf_header')
    segments = []
    for index in range(phnum):
        tag, _, offset, address, _, size, memory, _ = struct.unpack_from('<IIQQQQQQ', raw, phoff + index * phsize)
        need(offset <= len(raw) and size <= len(raw) - offset and memory >= size, 'collector_arm_elf_segment')
        segments.append((tag, offset, address, size))
    interpreters = [p for p in segments if p[0] == 3]
    dynamics = [p for p in segments if p[0] == 2]
    need(len(interpreters) == len(dynamics) == 1, 'collector_arm_elf_dynamic_shape')
    _, offset, _, size = interpreters[0]
    need(raw[offset:offset + size] == b'/lib/ld-linux-aarch64.so.1\0', 'collector_arm_elf_interpreter')
    _, offset, _, size = dynamics[0]
    need(16 <= size <= 65536 and size % 16 == 0, 'collector_arm_elf_dynamic_size')
    tags = {}
    ended = False
    for start in range(offset, offset + size, 16):
        tag, value = struct.unpack_from('<qQ', raw, start)
        if tag == 0:
            ended = True
            break
        tags.setdefault(tag, []).append(value)
    need(ended and len(tags.get(5, [])) == len(tags.get(10, [])) == 1, 'collector_arm_elf_dynamic_strings')
    address, size = tags[5][0], tags[10][0]
    need(1 <= size <= 262144, 'collector_arm_elf_strings_size')
    matches = [off + address - addr for tag, off, addr, count in segments
               if tag == 1 and addr <= address and address - addr <= count
               and size <= count - (address - addr)]
    need(len(matches) == 1, 'collector_arm_elf_strings_mapping')
    strings = raw[matches[0]:matches[0] + size]
    def string(index):
        need(0 <= index < len(strings), 'collector_arm_elf_string_offset')
        end = strings.find(b'\0', index, min(len(strings), index + 1025))
        need(end >= index, 'collector_arm_elf_string_bound')
        try:
            return strings[index:end].decode('ascii', 'strict')
        except UnicodeDecodeError:
            raise ValueError('collector_arm_elf_string_ascii') from None
    needed = [string(index) for index in tags.get(1, [])]
    need(1 <= len(needed) <= 64 and len(set(needed)) == len(needed)
         and all(re.fullmatch(r'[A-Za-z0-9_+.\-]{1,128}', name) is not None for name in needed),
         'collector_arm_elf_needed')
    # Location-sensitive origin expansion, alternate loaders/auditors, and
    # dependency paths are unsupported. No environment/library-path workaround.
    need(not any(tag in tags for tag in (0x6ffffefa, 0x6ffffefb, 0x6ffffefc, 0x7ffffffd, 0x7fffffff)),
         'collector_arm_elf_alternate_loader')
    need(len(tags.get(30, [])) <= 1 and len(tags.get(0x6ffffffb, [])) <= 1
         and not (tags.get(30, [0])[0] & 1) and not (tags.get(0x6ffffffb, [0])[0] & 128),
         'collector_arm_elf_origin_flag')
    need(len(tags.get(15, [])) <= 1 and len(tags.get(29, [])) <= 1
         and not (15 in tags and 29 in tags), 'collector_arm_elf_search_shape')
    paths = []
    for tag in (15, 29):
        for index in tags.get(tag, []):
            paths = string(index).split(':')
            need(1 <= len(paths) <= 8 and len(set(paths)) == len(paths)
                 and all(path in ELF_SEARCH_PATHS for path in paths), 'collector_arm_elf_search_path')
    return dict(format='ELF64LE-AArch64', interpreter='/lib/ld-linux-aarch64.so.1',
                needed=needed, searchTag='RUNPATH' if 29 in tags else 'RPATH' if 15 in tags else 'none',
                searchPaths=paths, dynamicOriginTokens=False, bytes=len(raw), sha256=hashlib.sha256(raw).hexdigest())

# Integrated source, not an independently installed helper or new CLI.
ARM_COPY = Path('/etc/consolecrypt-agent/collector-systemd')
ARM_POLICY = Path('/etc/apparmor.d/consolecrypt-agent-arm64-manager')
ARM_META = Path('/etc/consolecrypt-agent/collector-systemd-inputs.json')
ARM_HELPER = Path('/usr/local/libexec/consolecrypt-agent-install/collector_provision.py')
ARM_VENDOR = Path('/usr/lib/systemd/systemd')
ARM_PROFILES = Path('/sys/kernel/security/apparmor/profiles')
ARM_TRANSITION = Path('/proc/sys/kernel/apparmor_restrict_unprivileged_unconfined')
ARM_CAP_LAST = Path('/proc/sys/kernel/cap_last_cap')
ARM_ENABLED = Path('/sys/module/apparmor/parameters/enabled')


def arm_kernel_read(path, cap):
    need(path in (ARM_PROFILES, ARM_TRANSITION, ARM_CAP_LAST, ARM_ENABLED), 'collector_arm_kernel_path')
    parents(path)
    fd = os.open(path, os.O_RDONLY | os.O_NOFOLLOW | os.O_CLOEXEC)
    try:
        before = os.fstat(fd)
        need(stat.S_ISREG(before.st_mode) and before.st_uid == before.st_gid == 0
             and before.st_nlink == 1 and not before.st_mode & 0o022, 'collector_arm_kernel_file')
        if path in (ARM_TRANSITION, ARM_CAP_LAST):
            need(before.st_dev == Path('/proc').lstat().st_dev and before.st_size == 0
                 and stat.S_IMODE(before.st_mode) in ((0o644,) if path == ARM_TRANSITION else (0o444, 0o644)),
                 'collector_arm_kernel_proc')
        raw = bytearray()
        while len(raw) <= cap:
            block = os.read(fd, min(65536, cap + 1 - len(raw)))
            if not block:
                break
            raw.extend(block)
        stamp = lambda s: (s.st_dev, s.st_ino, s.st_mode, s.st_uid, s.st_gid, s.st_nlink)
        need(len(raw) <= cap and stamp(before) == stamp(os.fstat(fd)) == stamp(path.lstat()),
             'collector_arm_kernel_changed')
        parents(path)
        return bytes(raw)
    finally:
        os.close(fd)


def arm_profile_name(uid):
    need(type(uid) is int and 0 < uid <= 2147483647, 'collector_arm_uid')
    return 'consolecrypt-agent-arm64-manager-' + str(uid)


def arm_profile_body(uid):
    return ('abi <abi/4.0>,\nprofile ' + arm_profile_name(uid) + ' ' + str(ARM_COPY)
            + ' flags=(unconfined) {\n  userns,\n}\n').encode('ascii')


def arm_profile_state(uid):
    name = arm_profile_name(uid)
    lines = arm_kernel_read(ARM_PROFILES, 131072).decode('utf-8', 'strict').splitlines()
    selected = [line for line in lines if line == name or line.startswith(name + ' (')]
    need(len(selected) <= 1, 'collector_arm_profile_duplicate')
    return None if not selected else 'ownedMode' if selected == [name + ' (unconfined)'] else 'unexpected'


def arm_parser_argv(operation):
    need(operation in ('add', 'remove'), 'collector_arm_parser_operation')
    return ['/usr/sbin/apparmor_parser', '--config-file=/dev/null', '--skip-cache',
            '--abort-on-error', '--base=/etc/apparmor.d', '--' + operation, str(ARM_POLICY)]


def arm_manager_plan(uid, gid):
    need(DISK and os.getuid() == os.geteuid() == 0, 'collector_arm_platform')
    service = show('apparmor.service', ['LoadState', 'ActiveState', 'FragmentPath', 'DropInPaths'])
    need(service == dict(LoadState='loaded', ActiveState='active',
                         FragmentPath='/usr/lib/systemd/system/apparmor.service', DropInPaths=''),
         'collector_arm_apparmor_service')
    root_file(Path(service['FragmentPath']), 16384, 0o644)
    return arm_manager_inputs(uid, gid)


def arm_manager_inputs(uid, gid):
    need(os.uname().machine == 'aarch64' and type(gid) is int and 0 < gid <= 2147483647,
         'collector_arm_platform')
    # An installer never changes this global policy. With unrestricted explicit
    # profile transitions, another UID could enter this named userns profile.
    need(arm_kernel_read(ARM_ENABLED, 8) == b'Y\n', 'collector_arm_apparmor_disabled')
    need(arm_kernel_read(ARM_TRANSITION, 8) == b'1\n', 'collector_arm_transition_policy')
    last = arm_kernel_read(ARM_CAP_LAST, 8).strip()
    need(re.fullmatch(rb'[0-9]{1,2}', last) is not None and int(last) <= 63, 'collector_arm_cap_last')
    bounding = (1 << (int(last) + 1)) - 1
    before = ARM_VENDOR.lstat()
    raw = root_file(ARM_VENDOR, ELF_CAP, 0o755)
    need(before.st_gid == 0 and ident(before) == ident(ARM_VENDOR.lstat()), 'collector_arm_vendor_changed')
    facts = vendor_elf_facts(raw)
    loader = Path(facts['interpreter']).resolve(strict=True)
    need(str(loader) == '/usr/lib/aarch64-linux-gnu/ld-linux-aarch64.so.1', 'collector_arm_loader_path')
    root_file(loader, ELF_CAP, 0o755)
    for directory in facts['searchPaths']:
        resolved = Path(directory).resolve(strict=True)
        need(str(resolved) in ELF_SEARCH_PATHS and resolved.is_dir(), 'collector_arm_library_directory')
        parents(resolved / 'cc-uncreated-library-guard')
    abi = root_file(Path('/etc/apparmor.d/abi/4.0'), 131072, 0o644)
    parser = root_file(Path('/usr/sbin/apparmor_parser'), ELF_CAP, 0o755)
    profile = arm_profile_body(uid)
    helper = root_file(ARM_HELPER, 131072, 0o644)
    systemctl = root_file(Path('/usr/bin/systemctl'), ELF_CAP, 0o755)
    python = root_file(Path('/usr/bin/python3.12'), ELF_CAP, 0o755)
    vendor_unit = root_file(Path('/usr/lib/systemd/system/user@.service'), 16384, 0o644)
    vendor_dropins = {}
    for name in ('10-login-barrier.conf', 'timeout.conf'):
        path = Path('/usr/lib/systemd/system/user@.service.d') / name
        if path.exists() or path.is_symlink():
            vendor_dropins[str(path)] = hashlib.sha256(root_file(path, 16384, 0o644)).hexdigest()
    manager_path = Path('/etc/systemd/system/user@' + str(uid) + '.service.d/60-consolecrypt-agent.conf')
    manager_bytes = files(uid, gid, arm_copy=True)[manager_path]
    descriptor = dict(format='cc-agent-arm64-manager-attachment/v1', profile=arm_profile_name(uid),
                      vendorSha256=hashlib.sha256(raw).hexdigest(), vendorBytes=len(raw),
                      copy=str(ARM_COPY), copySha256=hashlib.sha256(raw).hexdigest(), copyMode='0750', copyGid=gid,
                      policy=str(ARM_POLICY), policySha256=hashlib.sha256(profile).hexdigest(),
                      abiSha256=hashlib.sha256(abi).hexdigest(), parserSha256=hashlib.sha256(parser).hexdigest(),
                      elf=facts, managerCapBnd=bounding, helperSha256=hashlib.sha256(helper).hexdigest(),
                      pythonSha256=hashlib.sha256(python).hexdigest(),
                      systemctlSha256=hashlib.sha256(systemctl).hexdigest(),
                      vendorUnitSha256=hashlib.sha256(vendor_unit).hexdigest(), vendorDropins=vendor_dropins,
                      managerUnitSha256=hashlib.sha256(manager_bytes).hexdigest())
    public = encode(dict(descriptor, uid=uid, gid=gid))
    descriptor['inputsPath'] = str(ARM_META)
    descriptor['inputsSha256'] = hashlib.sha256(public).hexdigest()
    return dict(uid=uid, gid=gid, raw=raw, profile=profile, public=public,
                managerPath=manager_path, managerBytes=manager_bytes,
                vendorIdentity=ident(before), descriptor=descriptor)


def arm_manager_preflight(plan, previous):
    uid = plan['uid']
    if previous is None:
        need(all(not p.exists() and not p.is_symlink() for p in (ARM_COPY, ARM_POLICY, ARM_META))
             and arm_profile_state(uid) is None, 'collector_arm_unowned_asset')
    else:
        # Caller has already compared the complete original ready descriptor.
        arm_manager_assets(plan)
        need(arm_profile_state(uid) == 'ownedMode', 'collector_arm_profile_missing')


def arm_manager_assets(plan):
    need(ident(ARM_VENDOR.lstat()) == plan['vendorIdentity']
         and root_file(ARM_VENDOR, ELF_CAP, 0o755) == plan['raw'], 'collector_arm_vendor_changed')
    need(root_file(ARM_COPY, ELF_CAP, 0o750) == plan['raw']
         and ARM_COPY.lstat().st_gid == plan['gid'], 'collector_arm_copy_changed')
    need(root_file(ARM_POLICY, 16384, 0o644) == plan['profile'], 'collector_arm_policy_changed')
    need(root_file(ARM_META, 16384, 0o644) == plan['public'], 'collector_arm_inputs_changed')
    need(root_file(plan['managerPath'], 16384, 0o644) == plan['managerBytes'], 'collector_arm_manager_unit_changed')


def arm_manager_materialize(plan, previous, configured):
    need(root_file(OWNER, 16384, 0o600) == configured, 'collector_arm_configured_owner')
    record = decode(configured)
    need(record.get('state') == 'configured' and record.get('armManager') == plan['descriptor']
         and record.get('uid') == plan['uid'] and record.get('gid') == plan['gid'],
         'collector_arm_configured_owner')
    owner_directory_fd = os.open(OWNER.parent, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW | os.O_CLOEXEC)
    try:
        os.fsync(owner_directory_fd)
    finally:
        os.close(owner_directory_fd)
    if previous is not None:
        arm_manager_assets(plan)
        need(arm_profile_state(plan['uid']) == 'ownedMode', 'collector_arm_profile_missing')
        return
    # The original configured owner record is durable BEFORE creation and load.
    # Failures remain configured/partial; there is no automatic adoption/retry.
    arm_manager_preflight(plan, None)
    parents(ARM_COPY)
    fd = os.open(ARM_COPY, os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW | os.O_CLOEXEC, 0o600)
    try:
        view = memoryview(plan['raw'])
        while view:
            count = os.write(fd, view)
            need(count > 0, 'collector_arm_copy_write')
            view = view[count:]
        os.fchown(fd, 0, plan['gid'])
        os.fchmod(fd, 0o750)
        os.fsync(fd)
    finally:
        os.close(fd)
    need(root_file(ARM_COPY, ELF_CAP, 0o750) == plan['raw']
         and ARM_COPY.lstat().st_gid == plan['gid'], 'collector_arm_copy_changed')
    publish(ARM_POLICY, plan['profile'])
    publish(ARM_META, plan['public'])
    for directory in (ARM_COPY.parent, ARM_POLICY.parent):
        directory_fd = os.open(directory, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW | os.O_CLOEXEC)
        try:
            os.fsync(directory_fd)
        finally:
            os.close(directory_fd)
    need(arm_kernel_read(ARM_TRANSITION, 8) == b'1\n', 'collector_arm_transition_policy')
    need(arm_profile_state(plan['uid']) is None, 'collector_arm_profile_collision')
    command(arm_parser_argv('add'), seconds=15)
    need(arm_profile_state(plan['uid']) == 'ownedMode', 'collector_arm_profile_load')
    arm_manager_assets(plan)


def arm_read_loaded_unit(plan):
    """One fixed readonly query, preserving the existing unprivileged identity."""
    uid, gid = plan['uid'], plan['gid']
    need(os.getuid() == os.geteuid() == uid > 0 and os.getgid() == os.getegid() == gid > 0,
         'collector_arm_query_identity')
    binary = Path('/usr/bin/systemctl')
    before = ident(binary.lstat())
    need(hashlib.sha256(root_file(binary, ELF_CAP, 0o755)).hexdigest()
         == plan['descriptor']['systemctlSha256'], 'collector_arm_systemctl_changed')
    argv = ['/usr/bin/systemctl', '--system', '--no-pager', '--no-ask-password', 'show',
            'user@' + str(uid) + '.service', '--property=FragmentPath', '--property=DropInPaths']
    need(signal.getsignal(signal.SIGCHLD) == signal.SIG_DFL, 'collector_arm_query_custody')
    proc = subprocess.Popen(argv, stdin=subprocess.DEVNULL, stdout=subprocess.PIPE,
                            stderr=subprocess.STDOUT, close_fds=True, shell=False,
                            env={'LANG': 'C', 'LC_ALL': 'C', 'PATH': '/usr/bin:/bin'})
    raw = bytearray()
    deadline = time.monotonic() + 1
    try:
        os.set_blocking(proc.stdout.fileno(), False)
        with selectors.DefaultSelector() as ready:
            ready.register(proc.stdout, selectors.EVENT_READ)
            while True:
                left = deadline - time.monotonic()
                need(left > 0 and bool(ready.select(left)), 'collector_arm_query_timeout')
                block = os.read(proc.stdout.fileno(), min(4096, 8193 - len(raw)))
                if not block:
                    break
                raw.extend(block)
                need(len(raw) <= 8192, 'collector_arm_query_cap')
        left = deadline - time.monotonic()
        need(left > 0 and proc.wait(timeout=left) == 0, 'collector_arm_query_failed')
    finally:
        try:
            if proc.poll() is None:
                proc.kill()
            proc.wait(timeout=1)
        finally:
            proc.stdout.close()
    need(ident(binary.lstat()) == before
         and hashlib.sha256(root_file(binary, ELF_CAP, 0o755)).hexdigest()
         == plan['descriptor']['systemctlSha256'], 'collector_arm_systemctl_changed')
    observed = fields(bytes(raw))
    need(set(observed) == {'FragmentPath', 'DropInPaths'}, 'collector_arm_query_shape')
    return observed


def arm_loaded_unit(plan):
    observed = arm_read_loaded_unit(plan)
    need(observed['FragmentPath'] == '/usr/lib/systemd/system/user@.service', 'collector_arm_unit_fragment')
    paths = observed['DropInPaths'].split()
    expected = set(plan['descriptor']['vendorDropins']) | {str(plan['managerPath'])}
    need(len(paths) == len(expected) and set(paths) == expected, 'collector_arm_unit_dropins')


def arm_verify_start(phase):
    # systemd executes this fixed check as the existing unprivileged service UID.
    # It never opens root-only owner/config/lock; only the fixed readonly query runs.
    need(phase in ('pre', 'post') and sys.platform == 'linux', 'collector_arm_platform')
    account = pwd.getpwnam('consolecrypt-agent')
    group = grp.getgrnam('consolecrypt-agent')
    uid, gid = account.pw_uid, account.pw_gid
    need(os.getuid() == os.geteuid() == uid > 0 and os.getgid() == os.getegid() == gid > 0
         and group.gr_gid == gid and account.pw_dir == HOME and account.pw_shell == '/usr/sbin/nologin'
         and set(group.gr_mem) <= {'consolecrypt-agent'}, 'collector_arm_verify_identity')
    users, groups = pwd.getpwall(), grp.getgrall()
    need(len(users) <= 4096 and len(groups) <= 4096
         and [u.pw_name for u in users if u.pw_uid == uid] == ['consolecrypt-agent']
         and [g.gr_name for g in groups if g.gr_gid == gid] == ['consolecrypt-agent'],
         'collector_arm_verify_identity')
    plan = arm_manager_inputs(uid, gid)
    arm_manager_assets(plan)
    arm_loaded_unit(plan)
    need(arm_kernel_read(ARM_TRANSITION, 8) == b'1\n', 'collector_arm_transition_policy')

    if phase == 'post':
        mainpid = os.environ.get('MAINPID')
        need(type(mainpid) is str, 'collector_arm_mainpid')
        need(re.fullmatch('[1-9][0-9]{0,9}', mainpid) is not None and 1 < int(mainpid) < 2**31,
             'collector_arm_mainpid')
        proc = Path('/proc') / mainpid
        need(proc.stat().st_uid == uid, 'collector_arm_manager_ids')
        start = (proc / 'stat').read_text().rsplit(') ', 1)[1].split()
        need(start[0] != 'Z' and len(start) > 19, 'collector_arm_manager_process')
        fd = os.pidfd_open(int(mainpid))
        try:
            with selectors.DefaultSelector() as ready:
                ready.register(fd, selectors.EVENT_READ)
                need(not ready.select(0), 'collector_arm_manager_process')
                arm_manager_proc_facts(plan, proc)
                end = (proc / 'stat').read_text().rsplit(') ', 1)[1].split()
                need(not ready.select(0) and end[0] != 'Z' and end[19] == start[19]
                     and proc.stat().st_uid == uid, 'collector_arm_manager_process')
        finally:
            os.close(fd)


def arm_manager_process(plan, proc):
    arm_manager_assets(plan)
    need(arm_kernel_read(ARM_TRANSITION, 8) == b'1\n', 'collector_arm_transition_policy')
    arm_manager_proc_facts(plan, proc)
    policy = show('user@' + str(plan['uid']) + '.service', ['User', 'Group', 'NoNewPrivileges', 'AmbientCapabilities'])
    need(policy['User'] == str(plan['uid']) and policy['Group'] in ('', str(plan['gid']))
         and policy['AmbientCapabilities'] == '' and policy['NoNewPrivileges'] in ('yes', 'no'),
         'collector_arm_manager_launch_policy')
    with (proc / 'status').open('rb') as stream:
        status = stream.read(16385)
    need(len(status) <= 16384, 'collector_arm_manager_status')
    rows = dict(line.split(':', 1) for line in status.decode('ascii', 'strict').splitlines() if ':' in line)
    need(rows['NoNewPrivs'].strip() == ('1' if policy['NoNewPrivileges'] == 'yes' else '0'),
         'collector_arm_manager_launch_policy')


def arm_manager_proc_facts(plan, proc):
    actual = (proc / 'exe').stat()
    expected = ARM_COPY.lstat()
    need((actual.st_dev, actual.st_ino) == (expected.st_dev, expected.st_ino), 'collector_arm_manager_executable')
    with (proc / 'cmdline').open('rb') as stream:
        cmdline = stream.read(4097)
    need(cmdline == str(ARM_COPY).encode('ascii') + b'\0--user\0', 'collector_arm_manager_cmdline')
    with (proc / 'attr/current').open('rb') as stream:
        label = stream.read(4097)
    need(label == (arm_profile_name(plan['uid']) + ' (unconfined)\n').encode('ascii'),
         'collector_arm_manager_profile')
    with (proc / 'cgroup').open('rb') as stream:
        cgroup = stream.read(4097)
    base = '/user.slice/user-' + str(plan['uid']) + '.slice/user@' + str(plan['uid']) + '.service'
    need(cgroup in (('0::' + base + '\n').encode('ascii'), ('0::' + base + '/init.scope\n').encode('ascii')),
         'collector_arm_manager_cgroup')
    with (proc / 'status').open('rb') as stream:
        status = stream.read(16385)
    need(len(status) <= 16384, 'collector_arm_manager_status')
    rows = dict(line.split(':', 1) for line in status.decode('ascii', 'strict').splitlines() if ':' in line)
    need(rows['Uid'].split() == [str(plan['uid'])] * 4 and rows['Gid'].split() == [str(plan['gid'])] * 4,
         'collector_arm_manager_ids')
    need(all(re.fullmatch('[0-9a-f]{16}', rows[key].strip()) is not None
             and int(rows[key], 16) == 0 for key in ('CapPrm', 'CapEff', 'CapAmb')),
         'collector_arm_manager_capabilities')
    need(re.fullmatch('[0-9a-f]{16}', rows['CapBnd'].strip()) is not None
         and int(rows['CapBnd'], 16) == plan['descriptor']['managerCapBnd'], 'collector_arm_manager_bounding')


def files(uid, gid, *, arm_copy=False):
    env = environment(uid)
    manager = ("[Service]\nPAMName=\nEnvironment=\nEnvironmentFile=\nPassEnvironment=\n"
               "UnsetEnvironment=SGX_AESM_ADDR\n"
               + "Environment=" + " ".join(k + "=" + v for k, v in sorted(env.items()))
               + "\nDelegate=cpu memory pids\nMemoryMax=671088640\nMemorySwapMax=0\n"
                 "TasksMax=96\nCPUQuota=100%\nRestart=no\nUMask=0077\nIPAddressDeny=any\nIPAddressAllow=localhost\n")
    need(type(arm_copy) is bool, 'collector_arm_asset_mode')
    if arm_copy:
        manager += ('[Unit]\nRequires=apparmor.service\nAfter=apparmor.service\n[Service]\nExecStart=\n'
                    + 'ExecStart=' + str(ARM_COPY) + ' --user\n'
                    + 'ExecStartPre=/usr/bin/python3.12 -I -B ' + str(ARM_HELPER) + ' --verify-arm-manager pre\n'
                    + 'ExecStartPost=/usr/bin/python3.12 -I -B ' + str(ARM_HELPER) + ' --verify-arm-manager post\n')
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
            # A root-owned empty regular unit masks only this dedicated UID's SSH agent.
            Path(XDG + "/systemd/user/gpg-agent-ssh.socket"): b"",
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
    arm = arm_manager_plan(uid, gid) if DISK and selected_machine == 183 else None
    assets = files(uid, gid, arm_copy=arm is not None)
    descriptor = {"format": "cc-agent-collector-provision-owner/v1", "profile": PROFILE, "uid": uid, "gid": gid,
                  "assets": {str(p): hashlib.sha256(b).hexdigest() for p, b in assets.items()}}
    if arm is not None:
        descriptor['armManager'] = arm['descriptor']
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
    if arm is not None:
        arm_manager_preflight(arm, previous)
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
    if arm is not None:
        arm_manager_materialize(arm, previous, configured)
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
        if arm is None:
            need((actual_exe.st_dev, actual_exe.st_ino) == (systemd.stat().st_dev, systemd.stat().st_ino), "collector_manager_process")
        else:
            arm_manager_process(arm, pidroot)
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
        if arm is not None:
            arm_manager_process(arm, pidroot)
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
    if sys.argv[1:] in (['--verify-arm-manager', 'pre'], ['--verify-arm-manager', 'post']):
        arm_verify_start(sys.argv[2])
        return
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
