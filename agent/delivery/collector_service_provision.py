"""Explicit installed finite-service prerequisite. No grant, queue or key API.

Invoked only by the reviewed administrator role AFTER signed disk installation.
Existing unknown files/units are refused rather than adopted or repaired.
"""
import fcntl
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import stat
import sys
import time

CODE = Path('/opt/consolecrypt-agent-install/opt/consolecrypt-agent')
HELPER = Path('/usr/local/libexec/consolecrypt-agent-install/collector_provision.py')
UNIT = Path('/etc/systemd/system/consolecrypt-agent-collector.service')
CATALOG = Path('/etc/consolecrypt-agent/collector-service.json')
OWNER = Path('/etc/consolecrypt-agent/collector-service-provisioned.json')
LOCK = Path('/etc/consolecrypt-agent/collector-service-provision.lock')
RECIPIENT = Path('/var/lib/consolecrypt-agent/collector-credentials')
NAMES = ('collector/service_unit_adapter.py', 'collector/service_unit_entry.py',
         'collector/service_unit_spec.py', 'collector/service_binding.py', 'collector/service_daemon.py')


def need(value):
    if not value:
        raise ValueError('collector_service_provision_refused')


def trusted(path, cap=65536, mode=0o644):
    for parent in path.parents:
        m = parent.lstat()
        need(stat.S_ISDIR(m.st_mode) and m.st_uid == 0 and not m.st_mode & 0o022)
    fd = os.open(path, os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK | os.O_CLOEXEC)
    try:
        before = os.fstat(fd)
        identity = lambda m: (m.st_dev, m.st_ino, m.st_uid, m.st_mode, m.st_nlink,
                              m.st_size, m.st_ctime_ns, m.st_mtime_ns)
        need(stat.S_ISREG(before.st_mode) and before.st_nlink == 1 and before.st_uid == 0
             and stat.S_IMODE(before.st_mode) == mode and 0 < before.st_size <= cap)
        raw = bytearray()
        while len(raw) <= cap:
            block = os.read(fd, min(65536, cap + 1 - len(raw)))
            if not block:
                break
            raw.extend(block)
        need(len(raw) == before.st_size and identity(before) == identity(os.fstat(fd))
             == identity(path.lstat()))
        return bytes(raw)
    finally:
        os.close(fd)


def module(path, name):
    raw = trusted(path, 131072)
    spec = importlib.util.spec_from_file_location(name, path)
    value = importlib.util.module_from_spec(spec)
    # Compile the exact captured Root-owned bytes, not a second path read.
    exec(compile(raw, str(path), 'exec'), value.__dict__)
    return value


def provision(operation='prepare'):
    need(operation in ('prepare', 'disable'))
    need(sys.platform == 'linux' and os.getuid() == os.geteuid() == 0 and os.uname().machine in {'x86_64', 'aarch64'})
    base = module(HELPER, 'cc_fixed_collector_provision')
    base.END = time.monotonic() + 55
    profile = base.decode(base.root_file(Path('/etc/consolecrypt-agent/collector-runtime.json'), mode=0o600))
    base.select_profile(profile)
    need(base.DISK)
    uid, gid = profile['uid'], profile['gid']
    need(type(uid) is int and type(gid) is int and uid > 0 and gid > 0)
    owner = base.decode(base.root_file(Path('/etc/consolecrypt-agent/collector-disk-provisioned.json'), mode=0o600))
    need(owner['state'] == 'ready' and owner['profile'] == 'ubuntu24-systemd255-disk-v1'
         and owner['uid'] == uid and owner['gid'] == gid)
    launcher = module(CODE / 'launch.py', 'cc_fixed_service_launcher')
    binary = launcher.verified_identity_binary()
    release = binary.parent.parent
    record = base.decode(trusted(release / 'installed.json', 128*1024))
    for name in NAMES:
        raw = trusted(CODE / name)
        need(record['files'].get(name) == hashlib.sha256(raw).hexdigest()
             and record['file_modes'].get(name) == '0644'
             and raw == trusted(release / name))
    template = trusted(release / 'delivery/consolecrypt-agent-collector.service.in', 16384)
    need(record['files'].get('delivery/consolecrypt-agent-collector.service.in') == hashlib.sha256(template).hexdigest()
         and record['file_modes'].get('delivery/consolecrypt-agent-collector.service.in') == '0644'
         and template.count(b'@SERVICE_UID@') == 3)
    unit = template.replace(b'@SERVICE_UID@', str(uid).encode('ascii'))
    disk = base.decode(base.root_file(Path('/etc/consolecrypt-agent/collector-disk.json')))
    need((disk.get('format') == 'cc-agent-disk-collector/v2')
         == (disk.get('manifest', {}).get('schema_version') == 2))
    if disk.get('format') == 'cc-agent-disk-collector/v2':
        base.installed_disk_catalog(disk)
    else:
        need(disk.get('format') == 'cc-agent-disk-collector/v1' and os.uname().machine == 'x86_64')
    manifest = disk['manifest']
    need(manifest['profile'] == 'maintained-prometheus-disk-512mib-2h-v1'
         and manifest['bufferMode'] == 'persistentNativeWal')
    digest = hashlib.sha256(base.encode(manifest)[:-1]).hexdigest()
    descriptor = {'format': 'cc-agent-finite-collector-service/v1', 'manifestSha256': digest}
    for name, key in zip(NAMES[:4], ('adapterSha256', 'entrySha256', 'specSha256', 'bindingSha256')):
        descriptor[key] = record['files'][name]
    catalog = base.encode(descriptor)
    marker = {'format': 'cc-agent-finite-collector-provision-owner/v1', 'unitSha256': hashlib.sha256(unit).hexdigest(),
              'catalogSha256': hashlib.sha256(catalog).hexdigest(), 'uid': uid, 'gid': gid}
    previous = trusted(OWNER, mode=0o600) if OWNER.exists() or OWNER.is_symlink() else None
    state = base.show(UNIT.name, ['LoadState', 'FragmentPath', 'DropInPaths', 'ActiveState', 'MainPID'])
    need(not state['DropInPaths'])
    if previous is None:
        need(operation == 'prepare' and state['LoadState'] == 'not-found' and state['MainPID'] == '0'
             and all(not p.exists() and not p.is_symlink() for p in (UNIT, CATALOG)))
    # This is ONLY an explicitly created absent private recipient directory.
    # No existing key/partial directory is adopted, chmodded or regenerated.
    for parent in RECIPIENT.parents:
        m = parent.lstat()
        need(stat.S_ISDIR(m.st_mode) and m.st_uid == 0 and not m.st_mode & 0o022)
    if previous is None:
        need(not RECIPIENT.exists() and not RECIPIENT.is_symlink())
        RECIPIENT.mkdir(mode=0o700)
        fd = os.open(RECIPIENT, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW | os.O_CLOEXEC)
        try:
            m = os.fstat(fd)
            need(stat.S_ISDIR(m.st_mode) and m.st_uid == 0 and stat.S_IMODE(m.st_mode) == 0o700
                 and (m.st_dev, m.st_ino) == (RECIPIENT.lstat().st_dev, RECIPIENT.lstat().st_ino))
            os.fchown(fd, uid, gid)
            os.fsync(fd)
        finally:
            os.close(fd)
    m = RECIPIENT.lstat()
    need(stat.S_ISDIR(m.st_mode) and m.st_uid == uid and m.st_gid == gid and stat.S_IMODE(m.st_mode) == 0o700)
    marker['recipientDirectory'] = {'dev': m.st_dev, 'ino': m.st_ino, 'uid': uid, 'gid': gid}
    if previous is None:
        need(state['LoadState'] == 'not-found' and state['MainPID'] == '0'
             and all(not p.exists() and not p.is_symlink() for p in (UNIT, CATALOG)))
    else:
        need(base.decode(previous) == dict(marker, state='ready')
             and trusted(UNIT, 16384) == unit and trusted(CATALOG) == catalog
             and state['FragmentPath'] == str(UNIT))
    if operation == 'disable':
        need(previous is not None)
        base.system('disable', '--now', UNIT.name)
        actual = base.show(UNIT.name, ['ActiveState', 'SubState', 'MainPID', 'FragmentPath', 'DropInPaths'])
        need((actual['ActiveState'], actual['SubState']) == ('inactive', 'dead') and actual['MainPID'] == '0'
             and actual['FragmentPath'] == str(UNIT) and not actual['DropInPaths']
             and trusted(UNIT, 16384) == unit and trusted(CATALOG) == catalog)
        return {'format': 'cc-agent-finite-collector-provision-result/v1', 'prerequisiteDisabled': True,
                'runtimeState': 'notObserved', 'signedPolicyRevoked': False, 'retainedDataErased': False}
    configured = base.encode(dict(marker, state='configured'))
    base.publish(OWNER, configured, mode=0o600, previous=previous)
    if previous is None:
        base.publish(UNIT, unit)
        base.publish(CATALOG, catalog)
    base.system('daemon-reload')
    base.system('enable', UNIT.name)
    base.system('start', UNIT.name)
    actual = base.show(UNIT.name, ['ActiveState', 'MainPID', 'FragmentPath', 'DropInPaths'])
    need(actual['ActiveState'] == 'active' and actual['MainPID'].isdecimal() and int(actual['MainPID']) > 1
         and actual['FragmentPath'] == str(UNIT) and not actual['DropInPaths']
         and trusted(UNIT, 16384) == unit and trusted(CATALOG) == catalog)
    base.publish(OWNER, base.encode(dict(marker, state='ready')), mode=0o600, previous=configured)
    return {'format': 'cc-agent-finite-collector-provision-result/v1', 'prerequisiteReady': True,
            'configurationRecorded': False, 'collectorRunning': False, 'serviceAuthority': False}


def main():
    need(sys.argv[1:] in (['--provision'], ['--disable']))
    operation = 'prepare' if sys.argv[1:] == ['--provision'] else 'disable'
    os.umask(0o077)
    for p in LOCK.parents:
        m = p.lstat()
        need(stat.S_ISDIR(m.st_mode) and m.st_uid == 0 and not m.st_mode & 0o022)
    fd = os.open(LOCK, os.O_RDWR | os.O_CREAT | os.O_NOFOLLOW | os.O_CLOEXEC, 0o600)
    try:
        m = os.fstat(fd)
        need(stat.S_ISREG(m.st_mode) and m.st_uid == 0 and m.st_nlink == 1 and m.st_size == 0
             and stat.S_IMODE(m.st_mode) == 0o600)
        fcntl.flock(fd, fcntl.LOCK_EX | fcntl.LOCK_NB)
        sys.stdout.buffer.write(json.dumps(provision(operation), sort_keys=True, separators=(',', ':')).encode('ascii') + b'\n')
    finally:
        os.close(fd)


if __name__ == '__main__':
    try:
        main()
    except BaseException:
        print('collector_service_provision_refused', file=sys.stderr)
        raise SystemExit(65)
