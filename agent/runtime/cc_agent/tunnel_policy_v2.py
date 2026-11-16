"""Captured V2 policy DATA; only the original owner Lease/Actor grants access."""
import hashlib
import os
from pathlib import Path
import stat

from integration.reader_files import read_reader_file
from .tunnel import Policy, POLICY_DOMAIN, TunnelUnavailable, _canonical, _strict, _validate_policy


def _identity(info):
    return (info.st_dev, info.st_ino, info.st_mode, info.st_uid, info.st_nlink,
            info.st_size, info.st_mtime_ns, info.st_ctime_ns)


class PolicyV2:
    """Immutable full-wrapper commitment with the original V1 route's TLS view.

    This object is not a Permit, a drain observation, or proof of an admission.
    """
    __slots__ = ('_path', '_raw', '_identity', '_hash')

    def __init__(self, path):
        self._path = Path(path)
        if (not self._path.is_absolute() or '..' in self._path.parts
                or not str(self._path).isascii() or self._path.resolve(strict=True) != self._path):
            raise TunnelUnavailable()
        before = self._path.lstat()
        if (not stat.S_ISREG(before.st_mode) or before.st_nlink != 1
                or before.st_uid not in {0, os.getuid()} or before.st_mode & 0o022):
            raise TunnelUnavailable()
        self._raw = read_reader_file(self._path, 16384)
        self._identity = _identity(before)
        value = _strict(self._raw)
        if (type(value) is not dict or set(value) != {'format', 'route', 'resumeOriginal'}
                or value['format'] != 'cc-agent-tunnel-route-policy/v2'
                or type(value['resumeOriginal']) is not bool):
            raise TunnelUnavailable()
        _validate_policy(value['route'])
        self._hash = hashlib.sha256(POLICY_DOMAIN + _canonical(value)).hexdigest()
        self.check()

    def check(self):
        if (self._path.resolve(strict=True) != self._path
                or _identity(self._path.lstat()) != self._identity
                or read_reader_file(self._path, 16384) != self._raw
                or _identity(self._path.lstat()) != self._identity):
            raise TunnelUnavailable()

    @property
    def digest(self):
        return self._hash

    def require_mode(self, mode):
        self.check()
        if (type(mode) is not str or mode not in ('admit', 'resume')
                or mode == 'resume' and not self.wrapper()['resumeOriginal']):
            raise TunnelUnavailable()

    def value(self):
        # Existing TLS code consumes the exact V1 route, not a fabricated policy.
        return _strict(self._raw)['route']

    def wrapper(self):
        return _strict(self._raw)

    def __repr__(self):
        return 'TunnelPolicyV2([redacted])'

    def __reduce_ex__(self, protocol):
        raise TypeError('tunnel_policy_not_portable')


def load_policy(path):
    value = _strict(read_reader_file(Path(path), 16384))
    if type(value) is dict and value.get('format') == 'cc-agent-tunnel-route-policy/v2':
        return PolicyV2(path)
    return Policy(path)  # Original V1 validation and behavior stay unchanged.
