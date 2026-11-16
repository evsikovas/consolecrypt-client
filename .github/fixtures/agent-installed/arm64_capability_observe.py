"""One disposable ARM pre-exec discriminator; never an installed Agent acceptance.

Reuses the pinned production profile/command custody and the reviewed finite
journal observer. It never changes AppArmor, sysctls, capabilities, or sandboxes.
The sole executable substitution is /usr/bin/true after the original sandbox
arguments. Exit zero here is NOT a successful production namespace probe.
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
import sys
import threading
import time
import types

UID = 10002
NAME = 'consolecrypt-agent'
UNIT = 'cc-agent-collector-prerequisite.service'
PROVISION_SHA = '9b03e2af5e66c0b2dcd2e6f1618a27182e311e92b00ced89766dcbbca4c50d10'
GUEST_SHA = '302f88020c99f6ab25354704d9ed81ae33ab9430cd6782a357fb212ff72ca141'
private_capture = {}


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
    exec(compile(source, 'pinned-reviewed-journal-observer', 'exec'), globals())


def profile_fact(raw):
    value = raw.decode('ascii', 'replace').strip()
    if value == 'unconfined':
        return 'unconfined'
    if value == 'unprivileged_userns' or value.startswith('unprivileged_userns ('):
        return 'unprivileged_userns'
    return 'other'


def manager_facts(pid):
    status = read(pathlib.Path('/proc') / str(pid) / 'status', 16384).decode('ascii')
    rows = {line.split(':', 1)[0]: line.split(':', 1)[1].strip() for line in status.splitlines() if ':' in line}
    need(rows['Uid'].split() == [str(UID)] * 4 and rows['Gid'].split() == [str(UID)] * 4, 'diag_manager_identity')
    result = {}
    for field in ('CapBnd', 'CapPrm', 'CapEff', 'CapAmb'):
        need(re.fullmatch('[0-9a-f]{16}', rows[field]) is not None, 'diag_capability_shape')
        result[field] = int(rows[field], 16)
    try:
        result['apparmorProfile'] = profile_fact(read(pathlib.Path('/proc') / str(pid) / 'attr/current', 4096))
    except OSError:
        result['apparmorProfile'] = 'unavailable'
    return result


def kernel_facts():
    facts = {}
    for key, path in (
        ('restrictUnprivilegedUserns', '/proc/sys/kernel/apparmor_restrict_unprivileged_userns'),
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


def main():
    global module, original_command, namespace_observed, journal_state, probe_argv
    need(sys.argv[1:] == ['--execute-disposable-arm-preexec-only'], 'diag_explicit_mode')
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
    module.END = time.monotonic() + 180
    module.select_profile(dict(format='cc-agent-installed-disk-collector-profile/v1', uid=UID, gid=UID))
    original_command = module.command
    namespace_observed = {}
    journal_state = dict(stage='notStarted', expected=(), snapshot={}, raw=b'', persisted=False, queryCount=0)
    install_observer(guest)
    report = dict(format='cc-arm64-preexec-capability-observation/v1', installedAcceptance=False,
                  productionNamespaceProbePassed=False, executable='/usr/bin/true', cleanupJoined=False,
                  observationCompleted=False, AppArmorPolicyChanged=False, sysctlsChanged=False)
    created_dirs, created_files = [], []
    owned_identity = None
    group_created = False
    manager_started = False
    group_attempted = False
    user_attempted = False
    user_removed = False
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
        existing = module.show('user@10002.service', ['FragmentPath', 'DropInPaths'])
        need(existing['FragmentPath'] == '/usr/lib/systemd/system/user@.service'
             and len(existing['DropInPaths'].split()) == len(vendor_dropins)
             and set(existing['DropInPaths'].split()) == vendor_dropins, 'diag_foreign_dropin')
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
        module.system('daemon-reload')
        manager_started = True  # Own creation/readback precedes the start attempt.
        module.system('start', 'user@10002.service')
        manager = module.show('user@10002.service', ['ActiveState', 'MainPID', 'InvocationID', 'ControlGroup', 'DropInPaths'])
        need(manager['ActiveState'] == 'active' and manager['ControlGroup'] == '/user.slice/user-10002.slice/user@10002.service', 'diag_manager_start')
        expected_dropins = vendor_dropins | {'/etc/systemd/system/user@10002.service.d/60-consolecrypt-agent.conf'}
        need(len(manager['DropInPaths'].split()) == len(expected_dropins)
             and set(manager['DropInPaths'].split()) == expected_dropins, 'diag_manager_dropin')
        with module.retained_manager(manager, UID):
            need(identity() == owned_identity, 'diag_identity_changed')
            systemd = pathlib.Path('/usr/lib/systemd/systemd')
            module.root_file(systemd, 32 * 1024 * 1024, 0o755)
            executable = (pathlib.Path('/proc') / manager['MainPID'] / 'exe').stat()
            need((executable.st_dev, executable.st_ino) == (systemd.stat().st_dev, systemd.stat().st_ino), 'diag_manager_executable')
            report['manager'] = manager_facts(int(manager['MainPID']))
            report['kernel'] = kernel_facts()
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
            report['observationCompleted'] = True
    except BaseException as error:
        report['errorClass'] = type(error).__name__ if type(error).__name__ in ('ValueError', 'OSError', 'KeyError', 'AssertionError', 'TimeoutExpired', 'FileNotFoundError', 'PermissionError') else 'Other'
        code = error.args[0] if len(error.args) == 1 and type(error.args[0]) is str else ''
        report['errorCode'] = code if re.fullmatch(r'(?:diag|collector)_[a-z_]{1,80}', code) else 'unclassified'
    finally:
        try:
            # Separate reserved cleanup time; no collector/grant is created here.
            module.END = time.monotonic() + 45
            if owned_identity is not None:
                need(identity() == owned_identity, 'diag_cleanup_identity')
                if manager_started:
                    original_command(['/usr/bin/systemctl', '--system', '--no-pager', '--no-ask-password',
                                      'stop', 'user@10002.service', 'user-runtime-dir@10002.service'], seconds=30)
                    for unit in ('user@10002.service', 'user-runtime-dir@10002.service'):
                        need(module.show(unit, ['ActiveState', 'MainPID']) == {'ActiveState': 'inactive', 'MainPID': '0'}, 'diag_cleanup_manager')
                    need(not pathlib.Path('/run/user/10002').exists() and not pathlib.Path('/run/user/10002').is_symlink(), 'diag_cleanup_runtime')
                    group = pathlib.Path('/sys/fs/cgroup/user.slice/user-10002.slice')
                    for path in group.rglob('cgroup.procs') if group.exists() else ():
                        need(not read(path, 65536).strip(), 'diag_cleanup_cgroup')
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
        except BaseException:
            report['cleanupJoined'] = False
        private_capture.clear()
        print(json.dumps(report, sort_keys=True), flush=True)
    return 0 if report['observationCompleted'] and report['cleanupJoined'] else 1


if __name__ == '__main__':
    raise SystemExit(main())
