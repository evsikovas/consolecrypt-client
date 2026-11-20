"""One execution's sealed anonymous input; data only, not a job grant.

No named file, raw parameters in diagnostics, fallback storage or holder map.
The existing execution ExitStack retains this FD through sandbox/sidecar close.
"""
import fcntl
import os
import stat
import sys

from .errors import require

MAX_INPUT_BYTES = 6 * 1024 * 1024
TARGET = '/tool/job-input.json'
VALIDATION_TARGET = '/tool/validation-input.json'


class EphemeralJobInput:
    def __init__(self, body):
        self._fd = None
        self._used = False
        require(type(body) is bytes and 0 < len(body) <= MAX_INPUT_BYTES,
                'job_input', 'Private job input exceeds its budget')
        require(sys.platform == 'linux' and hasattr(os, 'memfd_create') and
                all(hasattr(fcntl, name) for name in ('F_ADD_SEALS', 'F_GET_SEALS',
                    'F_SEAL_WRITE', 'F_SEAL_GROW', 'F_SEAL_SHRINK', 'F_SEAL_SEAL')),
                'job_input', 'Sealed private job input unavailable')
        self._seals = (fcntl.F_SEAL_WRITE | fcntl.F_SEAL_GROW |
                       fcntl.F_SEAL_SHRINK | fcntl.F_SEAL_SEAL)
        fd = os.memfd_create('cc-job-input', os.MFD_CLOEXEC | os.MFD_ALLOW_SEALING)
        try:
            os.fchmod(fd, 0o600)
            offset = 0
            while offset < len(body):
                count = os.write(fd, body[offset:])
                require(count > 0, 'job_input', 'Private job input write failed')
                offset += count
            fcntl.fcntl(fd, fcntl.F_ADD_SEALS, self._seals)
            info = os.fstat(fd)
            self._identity = (info.st_dev, info.st_ino)
            self._size = len(body)
            self._fd = fd
            self._check()
        except BaseException:
            self._fd = None
            os.close(fd)
            raise

    def _check(self):
        require(self._fd is not None, 'job_input', 'Private job input closed')
        info = os.fstat(self._fd)
        require(stat.S_ISREG(info.st_mode) and info.st_uid == os.getuid() and
                info.st_nlink == 0 and stat.S_IMODE(info.st_mode) == 0o600 and
                (info.st_dev, info.st_ino) == self._identity and info.st_size == self._size and
                fcntl.fcntl(self._fd, fcntl.F_GET_SEALS) & self._seals == self._seals and
                fcntl.fcntl(self._fd, fcntl.F_GETFD) & fcntl.FD_CLOEXEC,
                'job_input', 'Private job input identity or seals changed')

    def mounts(self):
        self._check()
        return ((self._fd, TARGET),)

    def inherited_fds(self):
        self._check()
        return (self._fd,)

    def prepare_for_spawn(self):
        self._check()
        require(not self._used, 'job_input', 'Private job input already handed off')
        # --ro-bind-data consumes a bounded copy from this anonymous FD; it
        # cannot use --ro-bind-fd's named-dentry lookup for a memfd. Rewind the
        # SAME sealed object once immediately before Popen, never reopen a path.
        os.lseek(self._fd, 0, os.SEEK_SET)
        self._used = True
        return (self._fd,)

    def close(self):
        fd, self._fd = getattr(self, '_fd', None), None
        if fd is not None:
            os.close(fd)

    def __enter__(self):
        try:
            self._check()
            return self
        except BaseException:
            self.close()
            raise

    def __exit__(self, *_):
        self.close()

    def __del__(self):
        try:
            self.close()
        except OSError:
            pass


class EphemeralValidationInput(EphemeralJobInput):
    """A single syntax/lint phase's input, with a fixed separate guest path.

    It conveys data only. Sandbox entry points require this exact type and
    cannot accept it as an execution input or change its destination.
    """
    def mounts(self):
        self._check()
        return ((self._fd, VALIDATION_TARGET),)
