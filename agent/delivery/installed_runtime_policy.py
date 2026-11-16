"""Fixed installed-unit policy; public root-reviewed IPs, never admission authority."""
import hashlib
import ipaddress
import json
import os
from pathlib import Path
import stat
import tempfile

UNIT = 'consolecrypt-agent.service'
DROPIN = '20-reviewed-runtime.conf'


def need(value):
    if not value:
        raise ValueError('installed runtime policy refused')


def canonical(value):
    return json.dumps(value, sort_keys=True, separators=(',', ':'), ensure_ascii=True).encode('ascii')


def strict(raw):
    def pairs(items):
        result = {}
        for key, value in items:
            need(key not in result)
            result[key] = value
        return result
    def invalid(_):
        raise ValueError('installed runtime policy refused')
    return json.loads(raw, object_pairs_hook=pairs, parse_constant=invalid, parse_float=invalid)


def config(value):
    need(type(value) is dict and set(value) == {'format', 'relayIps', 'dnsResolverIps'}
         and value['format'] == 'cc-agent-reviewed-egress/v1')
    values = []
    for field in ('relayIps', 'dnsResolverIps'):
        need(type(value[field]) is list and len(value[field]) <= 16)
        for address in value[field]:
            need(type(address) is str and 0 < len(address) <= 45 and '%' not in address)
            parsed = ipaddress.ip_address(address)
            need(str(parsed) == address and not parsed.is_unspecified and not parsed.is_multicast
                 and address != '255.255.255.255'
                 and not (parsed.version == 6 and parsed.ipv4_mapped is not None))
            values.append(address)
    need(len(values) <= 16 and len(set(values)) == len(values))
    return value


def service_ids():
    import pwd
    import grp
    user = pwd.getpwnam('consolecrypt-agent')
    group = grp.getgrnam('consolecrypt-agent')
    need(user.pw_uid > 0 and user.pw_gid == group.gr_gid > 0
         and user.pw_shell in ('/usr/sbin/nologin', '/bin/false'))
    return user.pw_uid, group.gr_gid


def collector_profile(value):
    if value is None:
        return None
    need(type(value) is dict and set(value) == {'format', 'uid', 'gid'}
         and value['format'] in ('cc-agent-installed-collector-profile/v1', 'cc-agent-installed-disk-collector-profile/v1')
         and type(value['uid']) is int and type(value['gid']) is int
         and 0 < value['uid'] <= 2147483647 and 0 < value['gid'] <= 2147483647
         and (value['uid'], value['gid']) == service_ids())
    return value


def render(configuration, profile):
    if configuration is not None:
        config(configuration)
    collector_profile(profile)
    addresses = [] if configuration is None else configuration['relayIps'] + configuration['dnsResolverIps']
    if not addresses and profile is None:
        return None
    lines = ['# Fixed root-reviewed ConsoleCrypt runtime policy. No protocol authority.']
    if profile is not None:
        # The fixed mount spelling is supplied by the reviewed provisioning port.
        mount = 'run-consolecrypt\\x2dagent-volatile.mount'
        if profile['format'] == 'cc-agent-installed-disk-collector-profile/v1':
            mount = 'run-consolecrypt\\x2dagent-disk.mount'
        dependencies = mount + ' user@' + str(profile['uid']) + '.service'
        lines += ['[Unit]', 'Requires=' + dependencies, 'After=' + dependencies]
    lines += ['[Service]']
    if profile is not None:
        lines += ['RuntimeDirectoryPreserve=yes', 'ProtectHome=tmpfs',
                  'BindReadOnlyPaths=/run/user/' + str(profile['uid'])]
    # Append only: never clear the base localhost allow or IPAddressDeny=any.
    for address in sorted(addresses, key=lambda v: (ipaddress.ip_address(v).version, ipaddress.ip_address(v).packed)):
        parsed = ipaddress.ip_address(address)
        lines += ['IPAddressAllow=' + address + ('/32' if parsed.version == 4 else '/128')]
    return ('\n'.join(lines) + '\n').encode('ascii')


def _identity(info):
    return (info.st_dev, info.st_ino, info.st_mode, info.st_uid, info.st_gid,
            info.st_nlink, info.st_size, info.st_mtime_ns, info.st_ctime_ns)


def parents(path, uid):
    need(path.is_absolute())
    for component in reversed(path.parents):
        try:
            info = component.lstat()
        except FileNotFoundError:
            continue
        need(stat.S_ISDIR(info.st_mode) and not stat.S_ISLNK(info.st_mode)
             and not getattr(info, 'st_file_attributes', 0) & 0x400)
        if uid is not None:
            need(info.st_uid == uid and not info.st_mode & 0o022)


class File:
    def __init__(self, path, uid, limit, *, optional=False):
        self.path, self.uid, self.limit = Path(path), uid, limit
        parents(self.path, uid)
        try:
            before = self.path.lstat()
        except FileNotFoundError:
            need(optional)
            self.identity, self.raw = None, None
            return
        need(stat.S_ISREG(before.st_mode) and before.st_nlink == 1
             and not getattr(before, 'st_file_attributes', 0) & 0x400
             and 0 < before.st_size <= limit)
        if uid is not None:
            need(before.st_uid == uid and not before.st_mode & 0o022)
        fd = os.open(self.path, os.O_RDONLY | os.O_NONBLOCK | os.O_NOFOLLOW | os.O_CLOEXEC)
        try:
            opened = os.fstat(fd)
            need(_identity(before) == _identity(opened))
            data = bytearray()
            while len(data) <= limit:
                part = os.read(fd, min(65536, limit + 1 - len(data)))
                if not part:
                    break
                data.extend(part)
            need(len(data) == before.st_size and _identity(before) == _identity(os.fstat(fd))
                 == _identity(self.path.lstat()))
            self.identity, self.raw = _identity(before), bytes(data)
        finally:
            os.close(fd)

    def unchanged(self):
        current = File(self.path, self.uid, self.limit, optional=self.identity is None)
        need(current.identity == self.identity and current.raw == self.raw)


def directory(path, uid):
    parents(path, uid)
    try:
        info = path.lstat()
    except FileNotFoundError:
        return None
    need(stat.S_ISDIR(info.st_mode) and not getattr(info, 'st_file_attributes', 0) & 0x400)
    if uid is not None:
        need(info.st_uid == uid and not info.st_mode & 0o022)
    with os.scandir(path) as entries:
        names = []
        for item in entries:
            names.append(item.name)
            need(len(names) <= 1 and item.name == DROPIN)
    return (info.st_dev, info.st_ino, info.st_mode, info.st_uid, info.st_gid), tuple(names)


class Review:
    def __init__(self, paths, uid=0):
        self.paths, self.uid = paths, uid
        self.configuration = File(paths.egress_config, uid, 4096, optional=True)
        self.desired = None if self.configuration.raw is None else config(strict(self.configuration.raw))
        self.old_record = File(paths.runtime_record, uid, 8192, optional=True)
        self.old_dropin = File(paths.runtime_dropin, uid, 8192, optional=True)
        self.old_directory = directory(paths.runtime_dropin.parent, uid)
        need((self.old_record.raw is None) == (self.old_dropin.raw is None))
        self.profile = None
        if self.old_record.raw is not None:
            record = strict(self.old_record.raw)
            need(type(record) is dict and set(record) == {'format', 'configuration', 'configurationSha256',
                 'collectorProfile', 'dropinSha256'} and record['format'] == 'cc-agent-installed-runtime-owner/v1')
            previous = record['configuration']
            if previous is not None:
                config(previous)
            self.profile = collector_profile(record['collectorProfile'])
            expected = render(previous, self.profile)
            need(expected is not None and self.old_dropin.raw == expected
                 and record['configurationSha256'] == hashlib.sha256(canonical(previous)).hexdigest()
                 and record['dropinSha256'] == hashlib.sha256(expected).hexdigest()
                 and canonical(record) + b'\n' == self.old_record.raw)

    def check(self):
        self.configuration.unchanged()
        self.old_record.unchanged()
        self.old_dropin.unchanged()
        need(directory(self.paths.runtime_dropin.parent, self.uid) == self.old_directory)

    def expected_paths(self):
        return '' if self.old_dropin.raw is None else str(self.paths.runtime_dropin)

    def apply(self, profile):
        # Public root configuration and exact old ownership are captured BEFORE stop.
        # Root's existing installer lock serializes this two-file update. A crash
        # between publications fails closed next time; no adoption/repair fallback.
        self.check()
        profile = collector_profile(profile)
        expected = render(self.desired, profile)
        if expected is None:
            if self.old_dropin.raw is not None:
                self.old_dropin.unchanged()
                self.paths.runtime_dropin.unlink()
                self.old_record.unchanged()
                self.paths.runtime_record.unlink()
                _sync(self.paths.runtime_dropin.parent)
                _sync(self.paths.runtime_record.parent)
            return
        if self.old_directory is None:
            self.paths.runtime_dropin.parent.mkdir(mode=0o755)
        record = {'format': 'cc-agent-installed-runtime-owner/v1', 'configuration': self.desired,
                  'configurationSha256': hashlib.sha256(canonical(self.desired)).hexdigest(),
                  'collectorProfile': profile, 'dropinSha256': hashlib.sha256(expected).hexdigest()}
        _publish(self.paths.runtime_dropin, expected, 0o644, self.old_dropin)
        _publish(self.paths.runtime_record, canonical(record) + b'\n', 0o600, self.old_record)
        self.configuration.unchanged()
        current = Review(self.paths, self.uid)
        need(current.old_dropin.raw == expected and current.profile == profile)


def _sync(path):
    fd = os.open(path, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW | os.O_CLOEXEC)
    try:
        os.fsync(fd)
    finally:
        os.close(fd)


def _publish(path, raw, mode, original):
    parents(path, original.uid)
    original.unchanged()
    fd, name = tempfile.mkstemp(prefix='.cc-runtime-', dir=path.parent)
    temporary = Path(name)
    try:
        with os.fdopen(fd, 'wb') as output:
            output.write(raw)
            output.flush()
            os.fsync(output.fileno())
        temporary.chmod(mode)
        original.unchanged()
        if original.identity is None:
            # Exclusive first publication; never overwrite a racing foreign file.
            os.link(temporary, path, follow_symlinks=False)
        else:
            os.replace(temporary, path)
        _sync(path.parent)
    finally:
        temporary.unlink(missing_ok=True)
