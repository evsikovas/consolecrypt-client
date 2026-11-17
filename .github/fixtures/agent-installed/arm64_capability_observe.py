"""Disposable ARM A/B diagnostic with an owned vendor-ELF exec attachment.
A uses the original vendor manager; B uses its byte-identical private copy.
Only a fresh own-UID manager ExecStart and exact owned-path profile change.
No root prefix, new capability, vendor profile, or production sandbox change.
The original guarded temporary sysctl tightening/restoration is retained.
The original sandbox prerequisite executes /usr/bin/true, not the Agent;
neither diagnostic success nor exit zero constitutes installed acceptance.
"""
import ast
import contextlib
import ctypes
import grp
import hashlib
import io
import json
import os
import pathlib
import platform
import pwd
import re
import select
import stat
import struct
import sys
import threading
import time
import types
import uuid

UID = 10002
NAME = 'consolecrypt-agent'
UNIT = 'cc-agent-collector-prerequisite.service'
PROVISION_SHA = '9b03e2af5e66c0b2dcd2e6f1618a27182e311e92b00ced89766dcbbca4c50d10'
GUEST_SHA = '302f88020c99f6ab25354704d9ed81ae33ab9430cd6782a357fb212ff72ca141'
TRANSITION_SYSCTL = '/proc/sys/kernel/apparmor_restrict_unprivileged_unconfined'
private_capture = {}
journal_after_cursor = None


def need(value, code):
    if not value:
        raise ValueError(code)


def read(path, cap):
    with pathlib.Path(path).open('rb') as stream:
        value = stream.read(cap + 1)
    need(len(value) <= cap, 'diag_read_cap')
    return value


def pinned(path, digest):
    path = pathlib.Path(path)
    for parent in (path, *path.parents):
        need(not stat.S_ISLNK(parent.lstat().st_mode), 'diag_source_symlink')
    with path.open('rb') as stream:
        before = os.fstat(stream.fileno())
        need(stat.S_ISREG(before.st_mode) and before.st_nlink == 1 and before.st_size <= 131072, 'diag_source_shape')
        value = stream.read(131073)
        stamp = lambda info: (info.st_dev, info.st_ino, info.st_size, info.st_mtime_ns, info.st_ctime_ns)
        need(stamp(before) == stamp(os.fstat(stream.fileno())) == stamp(path.lstat()), 'diag_source_changed')
    need(hashlib.sha256(value).hexdigest() == digest, 'diag_source_sha')
    return value


def save_private(name, retained):
    # Existing observer callback; private raw journal stays in memory only.
    need(name in ('namespace-probe.output', 'namespace-probe.journal')
         and name not in private_capture and len(retained) <= 8192, 'diag_private_capture')
    private_capture[name] = retained


def install_observer(guest):
    """Reuse reviewed capture, changing only the true predicate and one-query bound."""
    tree = ast.parse(guest)
    outer = next(n for n in tree.body if isinstance(n, ast.FunctionDef) and n.name == 'provision_collector_once')
    script = next(n.value.value for n in outer.body if isinstance(n, ast.Assign)
                  and any(isinstance(t, ast.Name) and t.id == 'script' for t in n.targets))
    functions = {n.name: n for n in ast.parse(script).body if isinstance(n, ast.FunctionDef)}
    names = ('namespace_command', 'persist_journal_query', 'journal_capability_stage', 'capture_namespace_journal')
    source = '\n\n'.join(ast.get_source_segment(script, functions[name]) for name in names)
    old = "argv[-6:]==['/usr/bin/python3.12','-I','-B',str(module.PROBE),'10002','10002']"
    need(source.count(old) == 1, 'diag_observer_anchor')
    source = source.replace(old, 'argv==probe_argv', 1)
    begin = source.index('    while time.monotonic()<until:')
    end = source.index('    # Retain this exact filtered command output', begin)
    source = source[:begin] + '''    remaining=until-time.monotonic()
    assert remaining>0
    journal_state['stage']='query';journal_state['queryCount']+=1
    try:raw=namespace_command(argv,uid=0,gid=0,seconds=remaining,cap=16384)
    except BaseException:
        persist_journal_query();journal_state['stage']='queryExit';raise
''' + source[end:]
    anchor = "    argv=['/usr/bin/journalctl'"
    need(source.count(anchor) == 1, 'diag_cursor_fields_anchor')
    source = source.replace(anchor, "    fields.append('__CURSOR')\n" + anchor, 1)
    anchor = "    journal_state['expected']=tuple(argv)"
    need(source.count(anchor) == 1, 'diag_cursor_query_anchor')
    source = source.replace(anchor, "    if journal_after_cursor is not None:argv.append('--after-cursor='+journal_after_cursor)\n" + anchor, 1)
    exec(compile(source, 'pinned-reviewed-journal-observer', 'exec'), globals())


def profile_fact(raw, owned_profile=None):
    value = raw.decode('ascii', 'replace').strip()
    if owned_profile is not None and value in (owned_profile, owned_profile + ' (unconfined)', owned_profile + ' (enforce)'):
        return 'ownedNamed'
    if value == 'unconfined':
        return 'unconfined'
    if value == 'unprivileged_userns' or value.startswith('unprivileged_userns ('):
        return 'unprivileged_userns'
    return 'other'


def profile_observation(raw, owned_profile=None):
    need(type(raw) is bytes and len(raw) <= 4096, 'diag_profile_observation_bound')
    category, mode = 'other', 'unspecified'
    try:
        label = raw.decode('ascii', 'strict').strip()
    except UnicodeDecodeError:
        label, mode = '', 'unknown'
    for known in ('enforce', 'complain', 'unconfined'):
        suffix = ' (' + known + ')'
        if label.endswith(suffix):
            label, mode = label[:-len(suffix)], known
            break
    if owned_profile is not None and label == owned_profile:
        category = 'ownNamed'
    elif owned_profile is not None and len(label.split('//&')) == 2 and set(label.split('//&')) == {owned_profile, 'unconfined'}:
        category = 'ownStackedUnconfined'
    elif label == 'unconfined':
        category = 'plainUnconfined'
    elif label == 'unprivileged_userns' or (owned_profile is not None and len(label.split('//&')) == 2
                                           and set(label.split('//&')) == {owned_profile, 'unprivileged_userns'}):
        category = 'restricted'
    if category == 'other' and mode == 'unspecified':
        mode = 'unknown'
    return dict(readStatus='read', category=category, mode=mode, complain=mode == 'complain',
                bytes=len(raw), sha256=hashlib.sha256(raw).hexdigest(), readOnly=True)


def manager_facts(pid, owned_profile=None):
    status = read(pathlib.Path('/proc') / str(pid) / 'status', 16384).decode('ascii')
    rows = {line.split(':', 1)[0]: line.split(':', 1)[1].strip() for line in status.splitlines() if ':' in line}
    need(rows['Uid'].split() == [str(UID)] * 4 and rows['Gid'].split() == [str(UID)] * 4, 'diag_manager_identity')
    result = {}
    for field in ('CapBnd', 'CapPrm', 'CapEff', 'CapAmb'):
        need(re.fullmatch('[0-9a-f]{16}', rows[field]) is not None, 'diag_capability_shape')
        result[field] = int(rows[field], 16)
    try:
        raw = read(pathlib.Path('/proc') / str(pid) / 'attr/current', 4096)
        result['apparmorProfile'] = profile_fact(raw, owned_profile)
        result['apparmorObservation'] = profile_observation(raw, owned_profile)
    except OSError:
        result['apparmorProfile'] = 'unavailable'
        result['apparmorObservation'] = dict(readStatus='unavailable', category='other', mode='unknown',
                                            complain=False, bytes=None, sha256=None, readOnly=True)
    return result


def kernel_facts():
    facts = {}
    for key, path in (
        ('restrictUnprivilegedUserns', '/proc/sys/kernel/apparmor_restrict_unprivileged_userns'),
        ('restrictUnprivilegedUnconfined', '/proc/sys/kernel/apparmor_restrict_unprivileged_unconfined'),
        ('unprivilegedUsernsClone', '/proc/sys/kernel/unprivileged_userns_clone'),
        ('maxUserNamespaces', '/proc/sys/user/max_user_namespaces'),
    ):
        try:
            value = read(path, 32).strip()
            facts[key] = int(value) if re.fullmatch(rb'[0-9]{1,10}', value) else None
        except FileNotFoundError:
            facts[key] = None
    try:
        value = read('/sys/module/apparmor/parameters/enabled', 8).strip()
        facts['apparmorEnabled'] = True if value == b'Y' else False if value == b'N' else None
    except FileNotFoundError:
        facts['apparmorEnabled'] = None
    return facts


def identity():
    user, group = pwd.getpwnam(NAME), grp.getgrnam(NAME)
    need(user == pwd.getpwuid(UID) and group == grp.getgrgid(UID)
         and (user.pw_uid, user.pw_gid, user.pw_dir, user.pw_shell)
         == (UID, UID, '/var/lib/consolecrypt-agent', '/usr/sbin/nologin')
         and group.gr_gid == UID and group.gr_mem == [], 'diag_identity_changed')
    return user, group


def sandbox_argv(provision):
    tree = ast.parse(provision)
    function = next(n for n in tree.body if isinstance(n, ast.FunctionDef) and n.name == 'provision')
    assignment = next(n for n in ast.walk(function) if isinstance(n, ast.Assign)
                      and len(n.targets) == 1 and isinstance(n.targets[0], ast.Name) and n.targets[0].id == 'argv')
    private_users = next(n for n in ast.walk(function) if isinstance(n, ast.If)
                         and isinstance(n.test, ast.Name) and n.test.id == 'DISK'
                         and any(isinstance(x, ast.Call) and isinstance(x.func, ast.Attribute)
                                 and isinstance(x.func.value, ast.Name) and x.func.value.id == 'argv'
                                 for x in ast.walk(n)))
    scope = dict(ROOT=module.ROOT, PROBE=module.PROBE, uid=UID, gid=UID, unit=UNIT, DISK=True)
    exec(compile(ast.Module(body=[assignment, private_users], type_ignores=[]), 'exact-production-sandbox-argv', 'exec'), scope)
    argv = scope['argv']
    need(argv[-6:] == ['/usr/bin/python3.12', '-I', '-B', str(module.PROBE), str(UID), str(UID)], 'diag_sandbox_anchor')
    return argv[:-6] + ['/usr/bin/true']


def sysctl_stamp(info):
    return (info.st_dev, info.st_ino, info.st_mode, info.st_uid, info.st_gid, info.st_nlink)


def sysctl_path_identity():
    # The only writable knob is fixed here; no caller-supplied path or key.
    path = pathlib.Path(TRANSITION_SYSCTL)
    parents = (path.parent, *path.parent.parents)
    facts = []
    for parent in parents:
        info = parent.lstat()
        need(stat.S_ISDIR(info.st_mode) and info.st_uid == info.st_gid == 0
             and not info.st_mode & 0o022, 'diag_sysctl_root_parent')
        # Ancestor nlink counts child entries, not physical directory identity.
        facts.append(sysctl_stamp(info)[:5])
    info = path.lstat()
    need(stat.S_ISREG(info.st_mode) and info.st_uid == info.st_gid == 0
         and stat.S_IMODE(info.st_mode) == 0o644 and info.st_nlink == 1
         and info.st_size == 0, 'diag_sysctl_root_inode')
    proc = pathlib.Path('/proc').lstat()
    need(info.st_dev == proc.st_dev, 'diag_sysctl_not_procfs_device')
    namespace = pathlib.Path('/proc/self/ns/mnt').stat()
    return (sysctl_stamp(info), tuple(facts), (namespace.st_dev, namespace.st_ino))


def sysctl_guard(state):
    need(state.get('fd') is not None and state.get('identity') is not None, 'diag_sysctl_identity_missing')
    need(sysctl_stamp(os.fstat(state['fd'])) == state['identity'][0]
         and sysctl_path_identity() == state['identity'], 'diag_sysctl_identity_changed')


def sysctl_bytes(state):
    sysctl_guard(state)
    os.lseek(state['fd'], 0, os.SEEK_SET)
    raw = os.read(state['fd'], 8)
    need(raw in (b'0\n', b'1\n'), 'diag_sysctl_exact_bytes')
    sysctl_guard(state)
    return raw


def tighten_transition_sysctl(state):
    state['identity'] = sysctl_path_identity()
    state['fd'] = os.open(TRANSITION_SYSCTL, os.O_RDWR | os.O_NOFOLLOW | os.O_CLOEXEC)
    sysctl_guard(state)
    state['original'] = sysctl_bytes(state)
    if state['original'] == b'1\n':
        return  # Existing one is a true no-op, with no restoration write.
    need(sysctl_bytes(state) == b'0\n', 'diag_sysctl_tighten_compare')
    os.lseek(state['fd'], 0, os.SEEK_SET)
    state['writeAttempted'] = True
    need(os.write(state['fd'], b'1\n') == 2, 'diag_sysctl_tighten_short_write')
    state['recordedChange'] = True
    need(sysctl_bytes(state) == b'1\n', 'diag_sysctl_tighten_readback')


def restore_transition_sysctl(state, profile_absent, manager_stopped):
    need(profile_absent and manager_stopped, 'diag_sysctl_restore_resources_live')
    need(state.get('original') in (b'0\n', b'1\n'), 'diag_sysctl_original_unknown')
    current = sysctl_bytes(state)
    if state['recordedChange']:
        need(state['writeAttempted'] and state['original'] == b'0\n' and current == b'1\n',
             'diag_sysctl_restore_compare')
        # Check the held original inode/root path again immediately before writing.
        sysctl_guard(state)
        os.lseek(state['fd'], 0, os.SEEK_SET)
        need(os.write(state['fd'], state['original']) == 2, 'diag_sysctl_restore_short_write')
        need(sysctl_bytes(state) == state['original'], 'diag_sysctl_restore_readback')
    else:
        need(not state['writeAttempted'] and current == state['original'], 'diag_sysctl_unrecorded_state')
    state['restored'] = True


def ab_candidate(baseline):
    kernel, manager, journal = (baseline.get(key, {}) for key in ('kernel', 'manager', 'journal'))
    return (baseline.get('observationCompleted') is True
            and baseline.get('managerEnvironmentOriginalEquality') is True
            and baseline.get('preexecExit') == 218
            and journal.get('validated') is True and journal.get('capabilityStages') == ['boundingDrop']
            and kernel.get('apparmorEnabled') is True and kernel.get('restrictUnprivilegedUserns') == 1
            and kernel.get('restrictUnprivilegedUnconfined') == 1
            and kernel.get('unprivilegedUsernsClone') == 1
            and manager.get('apparmorProfile') == 'unconfined')


# Integrated into the disposable fixture by prepare_elf_attachment.py.
VENDOR_SYSTEMD = '/usr/lib/systemd/systemd'
ELF_CAP = 32 * 1024 * 1024
ELF_SEARCH_PATHS = ('/usr/lib/aarch64-linux-gnu/systemd', '/usr/lib/aarch64-linux-gnu',
                    '/usr/lib/systemd', '/lib/aarch64-linux-gnu', '/lib/systemd')


def entrypoint_path(name):
    need(re.fullmatch(r'cc-agent-arm64-ab-[0-9a-f]{32}', name) is not None, 'diag_profile_name')
    return pathlib.Path('/etc/consolecrypt-agent') / (name + '.systemd')


def vendor_elf_facts(raw):
    """Bounded ELF64/AArch64 dynamic metadata, without executing the image."""
    need(type(raw) is bytes and 64 <= len(raw) <= ELF_CAP, 'diag_elf_size')
    need(raw[:7] == b'\x7fELF\x02\x01\x01', 'diag_elf_format')
    header = struct.unpack_from('<HHIQQQIHHHHHH', raw, 16)
    kind, machine, version, _, phoff, _, _, ehsize, phsize, phnum, _, _, _ = header
    need(kind in (2, 3) and machine == 183 and version == 1 and ehsize == 64
         and phsize == 56 and 1 <= phnum <= 128 and phoff >= 64
         and phoff + phsize * phnum <= len(raw), 'diag_elf_header')
    segments = []
    for index in range(phnum):
        tag, _, offset, address, _, size, memory, _ = struct.unpack_from('<IIQQQQQQ', raw, phoff + index * phsize)
        need(offset <= len(raw) and size <= len(raw) - offset and memory >= size, 'diag_elf_segment')
        segments.append((tag, offset, address, size))
    interpreters = [p for p in segments if p[0] == 3]
    dynamics = [p for p in segments if p[0] == 2]
    need(len(interpreters) == len(dynamics) == 1, 'diag_elf_dynamic_shape')
    _, offset, _, size = interpreters[0]
    need(raw[offset:offset + size] == b'/lib/ld-linux-aarch64.so.1\0', 'diag_elf_interpreter')
    _, offset, _, size = dynamics[0]
    need(16 <= size <= 65536 and size % 16 == 0, 'diag_elf_dynamic_size')
    tags = {}
    ended = False
    for start in range(offset, offset + size, 16):
        tag, value = struct.unpack_from('<qQ', raw, start)
        if tag == 0:
            ended = True
            break
        tags.setdefault(tag, []).append(value)
    need(ended and len(tags.get(5, [])) == len(tags.get(10, [])) == 1, 'diag_elf_dynamic_strings')
    address, size = tags[5][0], tags[10][0]
    need(1 <= size <= 262144, 'diag_elf_strings_size')
    matches = [off + address - addr for tag, off, addr, count in segments
               if tag == 1 and addr <= address and address - addr <= count
               and size <= count - (address - addr)]
    need(len(matches) == 1, 'diag_elf_strings_mapping')
    strings = raw[matches[0]:matches[0] + size]
    def string(index):
        need(0 <= index < len(strings), 'diag_elf_string_offset')
        end = strings.find(b'\0', index, min(len(strings), index + 1025))
        need(end >= index, 'diag_elf_string_bound')
        try:
            return strings[index:end].decode('ascii', 'strict')
        except UnicodeDecodeError:
            raise ValueError('diag_elf_string_ascii') from None
    needed = [string(index) for index in tags.get(1, [])]
    need(1 <= len(needed) <= 64 and len(set(needed)) == len(needed)
         and all(re.fullmatch(r'[A-Za-z0-9_+.\-]{1,128}', name) is not None for name in needed),
         'diag_elf_needed')
    # Location-sensitive origin expansion, alternate loaders/auditors, and
    # dependency paths are unsupported. No environment/library-path workaround.
    need(not any(tag in tags for tag in (0x6ffffefa, 0x6ffffefb, 0x6ffffefc, 0x7ffffffd, 0x7fffffff)),
         'diag_elf_alternate_loader')
    need(len(tags.get(30, [])) <= 1 and len(tags.get(0x6ffffffb, [])) <= 1
         and not (tags.get(30, [0])[0] & 1) and not (tags.get(0x6ffffffb, [0])[0] & 128),
         'diag_elf_origin_flag')
    need(len(tags.get(15, [])) <= 1 and len(tags.get(29, [])) <= 1
         and not (15 in tags and 29 in tags), 'diag_elf_search_shape')
    paths = []
    for tag in (15, 29):
        for index in tags.get(tag, []):
            paths = string(index).split(':')
            need(1 <= len(paths) <= 8 and len(set(paths)) == len(paths)
                 and all(path in ELF_SEARCH_PATHS for path in paths), 'diag_elf_search_path')
    return dict(format='ELF64LE-AArch64', interpreter='/lib/ld-linux-aarch64.so.1',
                needed=needed, searchTag='RUNPATH' if 29 in tags else 'RPATH' if 15 in tags else 'none',
                searchPaths=paths, dynamicOriginTokens=False, bytes=len(raw), sha256=hashlib.sha256(raw).hexdigest())


def manager_launch(path):
    row = module.show('user@10002.service', ['ExecStart', 'User', 'Group', 'NoNewPrivileges',
                                          'CapabilityBoundingSet', 'AmbientCapabilities'])
    prefix = '{ path=' + str(path) + ' ; argv[]=' + str(path) + ' --user ; ignore_errors=no ; '
    value = row.pop('ExecStart')
    need(len(value) <= 4096 and value.startswith(prefix) and value.endswith(' }')
         and value.count('{') == value.count('}') == 1, 'diag_manager_execstart')
    need(row['User'] == str(UID) and row['Group'] in ('', str(UID))
         and row['NoNewPrivileges'] in ('yes', 'no'), 'diag_manager_launch_policy')
    return row


def verify_entrypoint(state):
    path = state['path']
    need(path == entrypoint_path(state['name']) and state['ready'], 'diag_entrypoint_state')
    vendor = pathlib.Path(VENDOR_SYSTEMD)
    need(module.ident(vendor.lstat()) == state['vendorIdentity'], 'diag_vendor_changed')
    need(module.root_file(vendor, ELF_CAP, 0o755) == state['raw'], 'diag_vendor_bytes_changed')
    need(module.ident(path.lstat()) == state['identity'] and path.lstat().st_gid == UID,
         'diag_entrypoint_identity')
    need(module.root_file(path, ELF_CAP, 0o750) == state['raw'], 'diag_entrypoint_bytes')
    need(module.ident(path.lstat()) == state['identity']
         and module.ident(vendor.lstat()) == state['vendorIdentity'], 'diag_entrypoint_changed')


def vendor_snapshot():
    vendor = pathlib.Path(VENDOR_SYSTEMD)
    before = vendor.lstat()
    raw = module.root_file(vendor, ELF_CAP, 0o755)
    need(before.st_gid == 0 and module.ident(before) == module.ident(vendor.lstat()), 'diag_vendor_changed')
    facts = vendor_elf_facts(raw)
    loader = pathlib.Path(facts['interpreter']).resolve(strict=True)
    need(str(loader) == '/usr/lib/aarch64-linux-gnu/ld-linux-aarch64.so.1', 'diag_elf_loader_path')
    module.root_file(loader, ELF_CAP, 0o755)
    for directory in facts['searchPaths']:
        resolved = pathlib.Path(directory).resolve(strict=True)
        need(str(resolved) in ELF_SEARCH_PATHS and resolved.is_dir(), 'diag_elf_library_directory')
        module.parents(resolved / 'cc-uncreated-library-guard')
    return raw, module.ident(before), facts


def publish_entrypoint(state, report):
    path = state['path']
    need(path == entrypoint_path(state['name']) and not state['created'], 'diag_entrypoint_state')
    module.parents(path)
    need(not path.exists() and not path.is_symlink(), 'diag_entrypoint_collision')
    raw, vendor_identity, facts = vendor_snapshot()
    state.update(raw=raw, vendorIdentity=vendor_identity, facts=facts)
    report['temporaryEntrypointAbsent'] = False
    fd = os.open(path, os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW | os.O_CLOEXEC, 0o600)
    try:
        created = os.fstat(fd)
        state.update(created=True, device=created.st_dev, inode=created.st_ino)
        view = memoryview(raw)
        while view:
            count = os.write(fd, view)
            need(count > 0, 'diag_entrypoint_write')
            state['written'] += count
            view = view[count:]
        os.fchown(fd, 0, UID)
        os.fchmod(fd, 0o750)
        os.fsync(fd)
        state['identity'] = module.ident(os.fstat(fd))
    finally:
        os.close(fd)  # A writable descriptor must not survive into exec.
    state['ready'] = True
    verify_entrypoint(state)
    report['managerEntrypoint'] = dict(facts, byteIdenticalToVendor=True, rootOwned=True,
                                     gid=UID, mode='0750', exclusiveCreation=True)


def retire_entrypoint(state, manager_stopped, profile_absent, report):
    need(manager_stopped and profile_absent, 'diag_entrypoint_retire_live')
    path = state['path']
    need(path == entrypoint_path(state['name']), 'diag_entrypoint_state')
    if state['created'] and not state['retired']:
        info = path.lstat()
        need((info.st_dev, info.st_ino) == (state['device'], state['inode'])
             and stat.S_ISREG(info.st_mode) and info.st_uid == 0 and info.st_nlink == 1
             and info.st_gid in (0, UID) and stat.S_IMODE(info.st_mode) in (0o600, 0o750),
             'diag_entrypoint_cleanup_identity')
        need(module.root_file(path, ELF_CAP) == state['raw'][:state['written']], 'diag_entrypoint_cleanup_bytes')
        if state['ready']:
            need(module.ident(info) == state['identity'], 'diag_entrypoint_cleanup_changed')
        path.unlink()
        state['retired'] = True
    need(not path.exists() and not path.is_symlink(), 'diag_entrypoint_cleanup_absent')
    report['temporaryEntrypointAbsent'] = True


def verify_manager_entrypoint(pid, state):
    verify_entrypoint(state)
    executable = (pathlib.Path('/proc') / str(pid) / 'exe').stat()
    need((executable.st_dev, executable.st_ino) == (state['device'], state['inode']), 'diag_manager_executable')
    need(read(pathlib.Path('/proc') / str(pid) / 'cmdline', 4096)
         == str(state['path']).encode('ascii') + b'\0--user\0', 'diag_manager_cmdline')
    need(manager_launch(state['path']) == state['originalLaunch'], 'diag_manager_launch_changed')


def profile_body(name):
    need(re.fullmatch(r'cc-agent-arm64-ab-[0-9a-f]{32}', name) is not None, 'diag_profile_name')
    attachment = entrypoint_path(name)
    return ('abi <abi/4.0>,\nprofile ' + name + ' ' + str(attachment)
            + ' flags=(unconfined) {\n  userns,\n}\n').encode('ascii')


def profile_state(name):
    profile_body(name)  # Validate before using the generated name as a selector.
    rows = read('/sys/kernel/security/apparmor/profiles', 131072).decode('utf-8', 'strict').splitlines()
    selected = [line for line in rows if line == name or line.startswith(name + ' (')]
    need(len(selected) <= 1, 'diag_profile_duplicate')
    if not selected:
        return None
    return 'expected' if selected[0] in (name + ' (unconfined)', name + ' (enforce)') else 'unexpected'


def parser_argv(operation, path):
    need(operation in ('add', 'remove'), 'diag_parser_operation')
    return ['/usr/sbin/apparmor_parser', '--config-file=/dev/null', '--skip-cache',
            '--abort-on-error', '--base=/etc/apparmor.d', '--' + operation, str(path)]


def publish_registered(path, raw, created_files):
    need(not path.exists() and not path.is_symlink(), 'diag_profile_file_collision')
    module.publish(path, raw)
    info = path.lstat()
    created_files.append((path, info.st_dev, info.st_ino, hashlib.sha256(raw).hexdigest()))


def remove_registered(path, created_files):
    selected = [item for item in created_files if item[0] == path]
    if not selected:
        need(not path.exists() and not path.is_symlink(), 'diag_profile_file_unowned')
        return
    need(len(selected) == 1, 'diag_profile_file_duplicate')
    _, dev, ino, digest = selected[0]
    info = path.lstat()
    need((info.st_dev, info.st_ino) == (dev, ino)
         and hashlib.sha256(module.root_file(path, 16384, 0o644)).hexdigest() == digest, 'diag_profile_file_changed')
    path.unlink()
    created_files.remove(selected[0])


def load_temporary_profile(state, created_files):
    need(profile_state(state['name']) is None, 'diag_profile_already_loaded')
    module.root_file(pathlib.Path('/usr/sbin/apparmor_parser'), 32 * 1024 * 1024, 0o755)
    abi = module.root_file(pathlib.Path('/etc/apparmor.d/abi/4.0'), 131072, 0o644)
    state['abiSha256'] = hashlib.sha256(abi).hexdigest()
    publish_registered(state['profileFile'], profile_body(state['name']), created_files)
    state['addAttempted'] = True
    try:
        original_command(parser_argv('add', state['profileFile']), seconds=15)
    finally:
        # Fresh absent name + our single --add attempt establishes ownership,
        # including a parser failure after the kernel accepted that one profile.
        state['loadedObserved'] = profile_state(state['name']) is not None
    need(state['loadedObserved'] and profile_state(state['name']) == 'expected', 'diag_profile_load_readback')
    need(hashlib.sha256(module.root_file(pathlib.Path('/etc/apparmor.d/abi/4.0'), 131072, 0o644)).hexdigest()
         == state['abiSha256'], 'diag_profile_abi_changed')


def baseline_cursor():
    rows = private_capture.get('namespace-probe.journal', b'').splitlines()
    need(bool(rows), 'diag_baseline_journal_empty')
    cursor = json.loads(rows[-1]).get('__CURSOR')
    need(type(cursor) is str and re.fullmatch(r'[A-Za-z0-9;=_-]{1,512}', cursor) is not None, 'diag_baseline_cursor')
    return cursor


def retire_temporary_profile(state, created_files, expected_dropins, original_apparmor, stopped, report):
    need(stopped, 'diag_profile_retire_manager_live')
    try:
        remove_registered(state['dropin'], created_files)
        module.system('daemon-reload')
        restored = module.show('user@10002.service', ['DropInPaths', 'AppArmorProfile'])
        need(set(restored['DropInPaths'].split()) == expected_dropins
             and len(restored['DropInPaths'].split()) == len(expected_dropins)
             and restored['AppArmorProfile'] == original_apparmor, 'diag_profile_restore_readback')
        need(hashlib.sha256(module.root_file(state['originalDropin'], 16384, 0o644)).hexdigest()
             == state['originalDropinSha256'], 'diag_original_profile_changed')
        need(manager_launch(VENDOR_SYSTEMD) == state['entrypoint']['originalLaunch'], 'diag_launch_restore')
        report['originalUnitPolicyRestored'] = True
    finally:
        if state['addAttempted'] and profile_state(state['name']) is not None:
            need(state['loadedObserved'], 'diag_profile_load_ownership_unknown')
            raw = module.root_file(state['profileFile'], 16384, 0o644)
            need(raw == profile_body(state['name']), 'diag_profile_source_changed')
            original_command(parser_argv('remove', state['profileFile']), seconds=15)
        need(profile_state(state['name']) is None, 'diag_profile_unload_readback')
        report['temporaryProfileAbsent'] = True


def stop_owned_manager():
    original_command(['/usr/bin/systemctl', '--system', '--no-pager', '--no-ask-password',
                      'stop', 'user@10002.service', 'user-runtime-dir@10002.service'], seconds=30)
    for unit in ('user@10002.service', 'user-runtime-dir@10002.service'):
        need(module.show(unit, ['ActiveState', 'MainPID']) == {'ActiveState': 'inactive', 'MainPID': '0'}, 'diag_cleanup_manager')
    need(not pathlib.Path('/run/user/10002').exists() and not pathlib.Path('/run/user/10002').is_symlink(), 'diag_cleanup_runtime')
    group = pathlib.Path('/sys/fs/cgroup/user.slice/user-10002.slice')
    for path in group.rglob('cgroup.procs') if group.exists() else ():
        need(not read(path, 65536).strip(), 'diag_cleanup_cgroup')


def observe_phase(provision, expected_dropins, owned_identity, owned_profile=None, baseline=None, phase_report=None, entrypoint=None):
    global namespace_observed, journal_state, probe_argv
    namespace_observed = {}
    journal_state = dict(stage='notStarted', expected=(), snapshot={}, raw=b'', persisted=False, queryCount=0)
    private_capture.clear()
    report = phase_report if phase_report is not None else {}
    need(type(report) is dict and not report, 'diag_phase_report_shape')
    report['observationCompleted'] = False
    manager = module.show('user@10002.service', ['ActiveState', 'MainPID', 'InvocationID', 'ControlGroup', 'DropInPaths'])
    need(manager['ActiveState'] == 'active' and manager['ControlGroup'] == '/user.slice/user-10002.slice/user@10002.service', 'diag_manager_start')
    need(len(manager['DropInPaths'].split()) == len(expected_dropins)
         and set(manager['DropInPaths'].split()) == expected_dropins, 'diag_manager_dropin')
    with module.retained_manager(manager, UID):
        need(identity() == owned_identity, 'diag_identity_changed')
        if entrypoint is None:
            systemd = pathlib.Path('/usr/lib/systemd/systemd')
            module.root_file(systemd, 32 * 1024 * 1024, 0o755)
            executable = (pathlib.Path('/proc') / manager['MainPID'] / 'exe').stat()
            need((executable.st_dev, executable.st_ino) == (systemd.stat().st_dev, systemd.stat().st_ino), 'diag_manager_executable')
        else:
            verify_manager_entrypoint(int(manager['MainPID']), entrypoint)
        report['manager'] = manager_facts(int(manager['MainPID']), owned_profile)
        if owned_profile is not None:
            observation = report['manager']['apparmorObservation']
            need(report['manager']['apparmorProfile'] == 'ownedNamed'
                 and observation['readStatus'] == 'read' and observation['category'] == 'ownNamed'
                 and observation['mode'] == 'unconfined' and observation['complain'] is False,
                 'diag_standalone_profile_not_applied')
        report['kernel'] = kernel_facts()
        need(report['kernel'].get('restrictUnprivilegedUnconfined') == 1,
             'diag_transition_not_tightened')
        if baseline is not None:
            need(report['kernel'] == baseline['kernel'], 'diag_kernel_facts_changed')
            need(all(report['manager'][key] == baseline['manager'][key]
                     for key in ('CapBnd', 'CapPrm', 'CapEff', 'CapAmb')), 'diag_manager_capabilities_changed')
        actual = module.fields(original_command(['/usr/bin/systemctl', '--user', 'show-environment'], uid=UID, gid=UID))
        expected = module.environment(UID)
        if 'DBUS_SESSION_BUS_ADDRESS' in actual:
            expected['DBUS_SESSION_BUS_ADDRESS'] = 'unix:path=/run/user/10002/bus'
        need(actual == expected, 'diag_manager_environment')
        report['managerEnvironmentOriginalEquality'] = True
        missing = original_command(['/usr/bin/systemctl', '--user', 'show', UNIT, '--property=LoadState'], uid=UID, gid=UID, exact_missing=True)
        need(missing == b'LoadState=not-found\n', 'diag_probe_collision')
        probe_argv = sandbox_argv(provision)
        report['productionSandboxArgvSha256'] = hashlib.sha256(json.dumps(probe_argv[:-1], separators=(',', ':')).encode()).hexdigest()
        report['completeArgvSha256'] = hashlib.sha256(json.dumps(probe_argv, separators=(',', ':')).encode()).hexdigest()
        public = io.StringIO()
        with contextlib.redirect_stderr(public):
            try:
                need(namespace_command(probe_argv, uid=UID, gid=UID, seconds=10) == b'', 'diag_true_output')
                namespace_observed['exit'] = 0
            except ValueError as error:
                need(error.args == ('collector_control_exit',) and 'exit' in namespace_observed, 'diag_original_probe_error')
            capture_namespace_journal()
        need(len(public.getvalue().encode()) <= 8192, 'diag_projection_cap')
        for line in public.getvalue().splitlines():
            need(len(line) <= 4096, 'diag_projection_line')
            if line.startswith('CC_AGENT_NAMESPACE_JOURNAL '):
                report['journal'] = json.loads(line.split(' ', 1)[1])
            elif line.startswith('CC_AGENT_NAMESPACE_PROCESS '):
                report['originalProcess'] = json.loads(line.split(' ', 1)[1])
            else:
                raise ValueError('diag_projection_prefix')
        need(journal_state['queryCount'] == 1 and journal_state['stage'] == 'complete', 'diag_single_journal')
        report['preexecExit'] = namespace_observed['exit']
        report['journalQueryCount'] = 1
        if entrypoint is not None:
            verify_manager_entrypoint(int(manager['MainPID']), entrypoint)
            report['ownedVendorCopyVerifiedBeforeAndAfter'] = True
    report['managerInvocationId'] = manager['InvocationID']
    report['observationCompleted'] = True
    return report


def main():
    global module, original_command, namespace_observed, journal_state, probe_argv, journal_after_cursor
    need(sys.argv[1:] == ['--execute-disposable-arm-elf-attachment-ab-with-tightening'], 'diag_explicit_mode')
    need(os.getuid() == 0 and platform.machine() == 'aarch64'
         and os.environ.get('GITHUB_ACTIONS') == 'true'
         and os.environ.get('RUNNER_ENVIRONMENT') == 'github-hosted', 'diag_disposable_arm_only')
    need(b'ID=ubuntu\n' in read('/etc/os-release', 4096)
         and b'VERSION_ID="24.04"' in read('/etc/os-release', 4096), 'diag_ubuntu24_only')
    need(ctypes.CDLL(None, use_errno=True).prctl(36, 1, 0, 0, 0) == 0, 'diag_subreaper')
    os.umask(0o077)
    here = pathlib.Path(__file__).resolve().parent
    provision = pinned(here.parents[2] / 'agent/delivery/collector_provision.py', PROVISION_SHA)
    guest = pinned(here / 'guest_lifecycle.py', GUEST_SHA)
    module = types.ModuleType('owned_production_collector_provision')
    module.__file__ = str(here.parents[2] / 'agent/delivery/collector_provision.py')
    exec(compile(provision, module.__file__, 'exec'), module.__dict__)
    module.END = time.monotonic() + 140
    module.select_profile(dict(format='cc-agent-installed-disk-collector-profile/v1', uid=UID, gid=UID))
    original_command = module.command
    namespace_observed = {}
    journal_state = dict(stage='notStarted', expected=(), snapshot={}, raw=b'', persisted=False, queryCount=0)
    install_observer(guest)
    report = dict(format='cc-arm64-preexec-elf-attachment-ab-observation/v1', installedAcceptance=False,
                  productionNamespaceProbePassed=False, executable='/usr/bin/true', cleanupJoined=False,
                  observationCompleted=False, causalExperimentCompleted=False, causalEffectObserved=False,
                  temporaryProfileLoaded=False, temporaryProfileAbsent=True, temporaryEntrypointAbsent=True,
                  originalUnitPolicyRestored=True, existingProfilesChanged=False, otherSysctlsChanged=False,
                  temporarySysctlChangeRecorded=False, temporarySysctlRestored=True)
    created_dirs, created_files = [], []
    owned_identity = None
    group_created = False
    manager_started = False
    group_attempted = False
    user_attempted = False
    user_removed = False
    temporary = None
    transition = None
    journal_after_cursor = None
    roots = [pathlib.Path(p) for p in ('/etc/consolecrypt-agent', '/var/lib/consolecrypt-agent',
             '/run/consolecrypt-agent', '/etc/systemd/system/user@10002.service.d')]
    def directory(path, private=False):
        need(not path.exists() and not path.is_symlink(), 'diag_foreign_directory')
        if private:
            path.mkdir(mode=0o700)
            os.chown(path, UID, UID)
        else:
            module.directory(path)
        info = path.lstat()
        created_dirs.append((path, info.st_dev, info.st_ino, info.st_uid, info.st_gid, stat.S_IMODE(info.st_mode)))
    try:
        for lookup, key in ((pwd.getpwnam, NAME), (pwd.getpwuid, UID), (grp.getgrnam, NAME), (grp.getgrgid, UID)):
            try:
                lookup(key)
            except KeyError:
                continue
            raise ValueError('diag_foreign_identity')
        need(all(not p.exists() and not p.is_symlink() for p in roots), 'diag_foreign_root')
        need(not pathlib.Path('/run/user/10002').exists() and not pathlib.Path('/run/user/10002').is_symlink(), 'diag_foreign_runtime')
        vendor_dropins = set()
        for name in ('10-login-barrier.conf', 'timeout.conf'):
            path = pathlib.Path('/usr/lib/systemd/system/user@.service.d') / name
            if path.exists() or path.is_symlink():
                module.root_file(path, 16384, 0o644)
                vendor_dropins.add(str(path))
        existing = module.show('user@10002.service', ['FragmentPath', 'DropInPaths', 'AppArmorProfile'])
        need(existing['FragmentPath'] == '/usr/lib/systemd/system/user@.service'
             and len(existing['DropInPaths'].split()) == len(vendor_dropins)
             and set(existing['DropInPaths'].split()) == vendor_dropins
             and existing['AppArmorProfile'] == '', 'diag_foreign_dropin')
        module.root_file(pathlib.Path(existing['FragmentPath']), 16384, 0o644)
        for unit in ('user@10002.service', 'user-runtime-dir@10002.service'):
            row = module.show(unit, ['ActiveState', 'MainPID'])
            need(row == {'ActiveState': 'inactive', 'MainPID': '0'}, 'diag_foreign_manager')
        group_attempted = True
        original_command(['/usr/sbin/groupadd', '--gid', str(UID), NAME])
        need(grp.getgrnam(NAME) == grp.getgrgid(UID) and grp.getgrnam(NAME).gr_mem == [], 'diag_group_readback')
        group_created = True
        user_attempted = True
        original_command(['/usr/sbin/useradd', '--no-log-init', '--uid', str(UID), '--gid', str(UID), '--no-create-home',
                          '--home-dir', '/var/lib/consolecrypt-agent', '--shell', '/usr/sbin/nologin', NAME])
        owned_identity = identity()
        for path in (roots[0], roots[1], roots[2], roots[3],
                     pathlib.Path(module.XDG), pathlib.Path(module.XDG) / 'systemd', pathlib.Path(module.XDG) / 'systemd/user'):
            directory(path)
        directory(module.ROOT, private=True)
        assets = module.files(UID, UID)
        for path, raw in assets.items():
            if path.name.endswith('.mount'):
                continue  # No production tmpfs/mount acceptance is claimed.
            module.publish(path, raw)
            info = path.lstat()
            created_files.append((path, info.st_dev, info.st_ino, hashlib.sha256(raw).hexdigest()))
        # All fresh-VM/account/root ownership guards precede this test-only change.
        transition = dict(fd=None, identity=None, original=None, writeAttempted=False,
                          recordedChange=False, restored=False)
        report['temporarySysctlRestored'] = False
        tighten_transition_sysctl(transition)
        module.system('daemon-reload')
        manager_started = True  # Own creation/readback precedes the start attempt.
        module.system('start', 'user@10002.service')
        expected_dropins = vendor_dropins | {'/etc/systemd/system/user@10002.service.d/60-consolecrypt-agent.conf'}
        report['baseline'] = {}
        observe_phase(provision, expected_dropins, owned_identity, phase_report=report['baseline'])
        if ab_candidate(report['baseline']):
            # A true causal experiment, not an inference of AppArmor causality.
            # Unsupported parser/ABI/profile operation fails explicitly; no fallback.
            journal_after_cursor = baseline_cursor()
            name = 'cc-agent-arm64-ab-' + uuid.uuid4().hex
            dropin = roots[3] / '70-consolecrypt-arm64-userns.conf'
            original_dropin = roots[3] / '60-consolecrypt-agent.conf'
            entrypoint = dict(name=name, path=entrypoint_path(name), created=False, ready=False,
                              retired=False, written=0, originalLaunch=manager_launch(VENDOR_SYSTEMD))
            temporary = dict(name=name, entrypoint=entrypoint, profileFile=roots[0] / (name + '.apparmor'), dropin=dropin,
                             originalDropin=original_dropin,
                             originalDropinSha256=hashlib.sha256(assets[original_dropin]).hexdigest(),
                             addAttempted=False, loadedObserved=False)
            stop_owned_manager()
            publish_entrypoint(entrypoint, report)
            report['temporaryProfileAbsent'] = False
            load_temporary_profile(temporary, created_files)
            report['temporaryProfileLoaded'] = True
            report['originalUnitPolicyRestored'] = False
            publish_registered(dropin, ('[Service]\nExecStart=\nExecStart='
                               + str(entrypoint['path']) + ' --user\n').encode('ascii'), created_files)
            module.system('daemon-reload')
            module.system('start', 'user@10002.service')
            report['namedProfile'] = {}
            observe_phase(provision, expected_dropins | {str(dropin)}, owned_identity,
                          owned_profile=name, baseline=report['baseline'], phase_report=report['namedProfile'], entrypoint=entrypoint)
            need(report['namedProfile']['managerInvocationId'] != report['baseline']['managerInvocationId'], 'diag_manager_not_restarted')
            need(report['namedProfile']['completeArgvSha256'] == report['baseline']['completeArgvSha256'], 'diag_ab_argv_changed')
            report['causalExperimentCompleted'] = True
            report['causalEffectObserved'] = report['namedProfile']['preexecExit'] == 0
            report['journalQueriesTotal'] = report['baseline']['journalQueryCount'] + report['namedProfile']['journalQueryCount']
        else:
            report['abSkippedReason'] = ('unprivilegedProfileTransitionRestrictionNotEnabled'
                                         if report['baseline']['kernel'].get('restrictUnprivilegedUnconfined') != 1
                                         else 'baselineOutsideAppArmorCandidate')
        report['observationCompleted'] = True
    except BaseException as error:
        report['errorClass'] = type(error).__name__ if type(error).__name__ in ('ValueError', 'OSError', 'KeyError', 'AssertionError', 'TimeoutExpired', 'FileNotFoundError', 'PermissionError') else 'Other'
        code = error.args[0] if len(error.args) == 1 and type(error.args[0]) is str else ''
        report['errorCode'] = code if re.fullmatch(r'(?:diag|collector)_[a-z_]{1,80}', code) else 'unclassified'
    finally:
        manager_stopped = not manager_started
        try:
            # Separate reserved cleanup time; no collector/grant is created here.
            module.END = time.monotonic() + 75
            if owned_identity is not None:
                need(identity() == owned_identity, 'diag_cleanup_identity')
                stopped = False
                if manager_started:
                    stop_owned_manager()
                    stopped = True
                    manager_stopped = True
                if temporary is not None:
                    retire_temporary_profile(temporary, created_files, expected_dropins,
                                             existing['AppArmorProfile'], stopped, report)
                    retire_entrypoint(temporary['entrypoint'], stopped,
                                      profile_state(temporary['name']) is None, report)
                for path, dev, ino, digest in reversed(created_files):
                    info = path.lstat()
                    need((info.st_dev, info.st_ino) == (dev, ino)
                         and hashlib.sha256(module.root_file(path, 16384, 0o644)).hexdigest() == digest, 'diag_cleanup_file_changed')
                    path.unlink()
                for path, dev, ino, uid, gid, mode in reversed(created_dirs):
                    info = path.lstat()
                    need((info.st_dev, info.st_ino, info.st_uid, info.st_gid, stat.S_IMODE(info.st_mode))
                         == (dev, ino, uid, gid, mode) and stat.S_ISDIR(info.st_mode), 'diag_cleanup_directory_changed')
                    path.rmdir()  # Never recursive; unknown content fails closed.
                need(all(not path.exists() and not path.is_symlink() for path in [*roots, module.ROOT]),
                     'diag_cleanup_root_remains')
                if manager_started:
                    module.system('daemon-reload')
                need(identity() == owned_identity, 'diag_cleanup_identity')
                original_command(['/usr/sbin/userdel', NAME])
                for lookup, key in ((pwd.getpwnam, NAME), (pwd.getpwuid, UID)):
                    try:
                        lookup(key)
                    except KeyError:
                        continue
                    raise ValueError('diag_cleanup_user_remains')
                user_removed = True
                owned_identity = None
            need(not user_attempted or user_removed, 'diag_cleanup_user_unowned')
            need(not group_attempted or group_created, 'diag_cleanup_group_unowned')
            if group_created:
                try:
                    group = grp.getgrnam(NAME)
                except KeyError:
                    try:
                        grp.getgrgid(UID)
                    except KeyError:
                        group = None
                    else:
                        raise ValueError('diag_cleanup_group_changed')
                if group is not None:
                    need(group == grp.getgrgid(UID) and group.gr_gid == UID and group.gr_mem == [], 'diag_cleanup_group_changed')
                    original_command(['/usr/sbin/groupdel', NAME])
                for lookup, key in ((grp.getgrnam, NAME), (grp.getgrgid, UID)):
                    try:
                        lookup(key)
                    except KeyError:
                        continue
                    raise ValueError('diag_cleanup_group_remains')
            report['cleanupJoined'] = True
        except BaseException as error:
            report['cleanupJoined'] = False
            code = error.args[0] if len(error.args) == 1 and type(error.args[0]) is str else ''
            report['cleanupErrorCode'] = code if re.fullmatch(r'(?:diag|collector)_[a-z_]{1,80}', code) else 'unclassified'
        # Independent copy retirement also runs if unit/profile restoration failed.
        if temporary is not None and not report['temporaryEntrypointAbsent']:
            try:
                retire_entrypoint(temporary['entrypoint'], manager_stopped,
                                  profile_state(temporary['name']) is None, report)
            except BaseException as error:
                report['cleanupJoined'] = False
                code = error.args[0] if len(error.args) == 1 and type(error.args[0]) is str else ''
                report['entrypointCleanupErrorCode'] = code if re.fullmatch(r'diag_[a-z_]{1,80}', code) else 'unclassified'
        # Independent finalizer: an unrelated file/account cleanup failure never
        # skips restoration of a provably owned knob, nor permits a live profile.
        if transition is not None:
            report['temporarySysctlOriginalValue'] = (0 if transition['original'] == b'0\n'
                                                      else 1 if transition['original'] == b'1\n' else None)
            report['temporarySysctlChangeRecorded'] = transition['recordedChange']
            try:
                restore_transition_sysctl(transition,
                    profile_absent=(report['temporaryProfileAbsent'] is True
                                    and (temporary is None or profile_state(temporary['name']) is None)),
                    manager_stopped=manager_stopped)
                report['temporarySysctlRestored'] = transition['restored']
            except BaseException as error:
                report['cleanupJoined'] = False
                code = error.args[0] if len(error.args) == 1 and type(error.args[0]) is str else ''
                report['sysctlCleanupErrorCode'] = code if re.fullmatch(r'diag_[a-z_]{1,80}', code) else 'unclassified'
            finally:
                if transition['fd'] is not None:
                    try:
                        os.close(transition['fd'])
                    except OSError:
                        report['cleanupJoined'] = False
                        report.setdefault('sysctlCleanupErrorCode', 'diag_sysctl_close')
                    finally:
                        transition['fd'] = None
        private_capture.clear()
        print(json.dumps(report, sort_keys=True), flush=True)
    return 0 if report['observationCompleted'] and report['cleanupJoined'] else 1


if __name__ == '__main__':
    raise SystemExit(main())
