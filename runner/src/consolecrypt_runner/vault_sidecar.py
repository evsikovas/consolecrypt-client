"""Fixed read-only mount custody for the ORIGINAL dedicated Vault sidecar.

No password, job authority, create/delete/repair or caller-supplied path. Reuse
the existing operation derivation/nofollow directory opener. Every ancestor,
phase directory and fixed executable remains FD-pinned until the actual guest
has joined. Never resolve a /proc FD anchor back into an ambient pathname.
"""
import hashlib
import os
from pathlib import Path
import stat
import sys
import shlex

from .errors import require
from .parameters import validate_parameters
from .protocol import decode, encode
from . import dependency_retention as retention

PHASES = ('syntax', 'lint', 'execution')

def trusted_client_source(phase):
    require(phase in PHASES, 'vault_binding', 'Unsupported private phase')
    module='vault_job_password_client.py' if phase=='execution' else 'vault_password_client.py'
    return ('#!/bin/sh\nexec '+shlex.quote(sys.executable)+' -I /tool/python/consolecrypt_runner/'+module+'\n').encode()


class PinnedVaultSidecar:
    def __init__(self, configured_root, binding, phase, revision_digest, manifest_digest=None):
        require(sys.platform == 'linux' and os.getuid() != 0 and phase in PHASES,
                'vault_platform', 'Private Linux Vault mount unavailable')
        checked = validate_parameters({'extra_vars': {}, 'tags': [], 'skip_tags': [], 'diff': False, 'vault': binding})['vault']
        require((phase == 'execution') == (manifest_digest is not None),
                'vault_binding', 'Vault material purpose changed')
        self._fds, self._links = [], []
        self._closed = False
        self.phase = phase
        self._metadata = None
        try:
            # Imports are delayed to avoid changing normal Helper bootstrap.
            from .dependency_helper import operation_path, _open_directory
            destination = operation_path(configured_root, checked['operation_id'], _purpose='vault_validation')
            parts = destination.parts[1:]
            require(3 <= len(parts) <= 64, 'vault_path', 'Private operation depth unsupported')
            parent = os.open('/', os.O_RDONLY | os.O_DIRECTORY | os.O_CLOEXEC)
            self._fds.append(parent)
            for index, name in enumerate(parts):
                child = _open_directory(parent, name, create=False)
                self._fds.append(child)
                info = os.fstat(child)
                if index >= len(parts)-3:
                    retention.identity(child)
                self._links.append((parent, name, child, (info.st_dev, info.st_ino), index >= len(parts)-3))
                parent = child
            for name in ('data', 'vault-'+phase):
                child = _open_directory(parent, name, create=False)
                self._fds.append(child)
                identity = retention.identity(child)
                self._links.append((parent, name, child, identity, True))
                parent = child
            self._phase = parent
            self._socket = _open_directory(parent, 'socket', create=False)
            self._fds.append(self._socket)
            self._socket_identity = retention.identity(self._socket)
            self._client = os.open('password-client', os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK | os.O_CLOEXEC, dir_fd=parent)
            self._fds.append(self._client)
            info = os.fstat(self._client)
            require(stat.S_ISREG(info.st_mode) and info.st_uid == os.getuid() and info.st_nlink == 1
                    and stat.S_IMODE(info.st_mode) == 0o500 and 0 < info.st_size <= 1024,
                    'vault_client', 'Trusted executable Vault client unavailable')
            self._client_identity = (info.st_dev, info.st_ino)
            self._client_sha = self._file_sha(self._client, 1024)
            require(self._client_sha == hashlib.sha256(trusted_client_source(phase)).hexdigest(),
                    'vault_client', 'Private client is not the fixed trusted executable')
            self._endpoint_identity = None
            metadata = self._read_metadata()
            require(metadata == {'version': 1, 'phase': phase, 'binding': checked,
                                 'revision_digest': revision_digest, 'manifest_digest': manifest_digest,
                                 'client_sha256': self._client_sha},
                    'vault_binding', 'Vault sidecar differs from signed input')
            self._metadata = metadata
            self.recheck()
        except BaseException:
            self.close()
            raise

    @staticmethod
    def _file_sha(fd, limit):
        os.lseek(fd, 0, os.SEEK_SET)
        value = os.read(fd, limit+1)
        require(len(value) <= limit, 'vault_client', 'Private executable exceeds budget')
        return hashlib.sha256(value).hexdigest()

    def _read_metadata(self):
        fd = os.open('phase.json', os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK | os.O_CLOEXEC, dir_fd=self._phase)
        try:
            info = os.fstat(fd)
            require(stat.S_ISREG(info.st_mode) and info.st_uid == os.getuid() and info.st_nlink == 1
                    and stat.S_IMODE(info.st_mode) == 0o600 and info.st_size <= 4096,
                    'vault_binding', 'Private phase metadata unavailable')
            return decode(os.read(fd, 4097), max_bytes=4096)
        finally:
            os.close(fd)

    def recheck(self):
        require(not self._closed, 'vault_closed', 'Private Vault mount closed')
        for parent, name, child, expected, private in self._links:
            actual = os.stat(name, dir_fd=parent, follow_symlinks=False)
            held = os.fstat(child)
            require(stat.S_ISDIR(actual.st_mode) and (actual.st_dev, actual.st_ino) == expected
                    and (held.st_dev, held.st_ino) == expected,
                    'vault_identity', 'Private Vault namespace changed')
            if private: retention.identity(child)
        retention._same_entry(self._phase, 'socket', self._socket_identity)
        sock = os.stat('broker.sock', dir_fd=self._socket, follow_symlinks=False)
        require(stat.S_ISSOCK(sock.st_mode) and sock.st_uid == os.getuid() and sock.st_nlink == 1 and stat.S_IMODE(sock.st_mode) == 0o600,
                'vault_identity', 'Private Vault endpoint unavailable')
        current=(sock.st_dev,sock.st_ino)
        if self._endpoint_identity is None:self._endpoint_identity=current
        require(current==self._endpoint_identity,'vault_identity','Private Vault endpoint changed')
        client = os.stat('password-client', dir_fd=self._phase, follow_symlinks=False)
        require(stat.S_ISREG(client.st_mode) and (client.st_dev, client.st_ino) == self._client_identity
                and client.st_uid == os.getuid() and client.st_nlink == 1 and stat.S_IMODE(client.st_mode) == 0o500
                and self._file_sha(self._client, 1024) == self._client_sha,
                'vault_client', 'Private Vault executable changed')
        require(self._read_metadata() == self._metadata, 'vault_binding', 'Private Vault phase changed')

    def mounts(self):
        self.recheck()
        # Pass ONLY these two verified handles into bwrap setup. The kernel
        # denies child access to parent-PID proc descriptors; no path fallback.
        target = '/run/consolecrypt-vault' if self.phase == 'execution' else '/run/consolecrypt'
        return ((self._client, '/tool/vault-password-client'), (self._socket, target))

    def inherited_fds(self):
        self.recheck()
        # No ancestry/operation/root FD reaches bwrap, and bwrap consumes these
        # setup descriptors rather than preserving them in the guest.
        return (self._client, self._socket)

    def close(self):
        self._closed = True
        while self._fds:
            os.close(self._fds.pop())

    def __enter__(self):
        self.recheck()
        return self

    def __exit__(self, *_):
        try:self.recheck()
        finally:self.close()
