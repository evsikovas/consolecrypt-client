"""Private Linux operation admission/disposal; NEVER sweep/reopen old roots.

Only the current exclusively created operation may be disposed. Directory FDs
pin renamed ancestors. Unknown/interrupted evidence blocks instead of repair.
"""
import os
import shutil
import stat
import sys
import uuid

from .errors import require
from .protocol import encode

if sys.platform == 'linux':
    import fcntl
else:
    fcntl = None

MAX_OPERATIONS = 4
INTENT = 'cleanup-intent.json'


def identity(fd):
    info = os.fstat(fd)
    require(stat.S_ISDIR(info.st_mode) and info.st_uid == os.getuid()
            and stat.S_IMODE(info.st_mode) == 0o700,
            'dependency_cleanup_identity', 'Private operation identity changed')
    return info.st_dev, info.st_ino


def claim(family, operation):
    """Count ALL observed entries; at most four stats, never unbounded listdir.

    The family directory is already opened O_NOFOLLOW, owned0700. flock is a
    cooperating-helper admission lock, not protection against the account owner.
    """
    require(sys.platform == 'linux', 'dependency_backend', 'Linux operation storage is required')
    identity(family)
    try:
        fcntl.flock(family, fcntl.LOCK_EX | fcntl.LOCK_NB)
    except BlockingIOError:
        require(False, 'dependency_busy', 'Another operation is being admitted')
    try:
        count = 0
        scan_fd = os.open('.', os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW | os.O_CLOEXEC, dir_fd=family)
        try:
            with os.scandir(scan_fd) as scan:
                for entry in scan:
                    # Do not scan junk looking for four valid names. A fourth retained entry
                    # fails before stat, UUID parse or any next read.
                    count += 1
                    require(count < MAX_OPERATIONS, 'dependency_retention_limit',
                            'Review retained dependency operations before preparing another')
                    try:
                        parsed = uuid.UUID(entry.name)
                    except ValueError:
                        parsed = None
                    require(parsed is not None and parsed.int and str(parsed) == entry.name,
                            'dependency_retention_unknown', 'Unknown operation evidence requires review')
                    info = entry.stat(follow_symlinks=False)
                    require(stat.S_ISDIR(info.st_mode) and info.st_uid == os.getuid()
                            and stat.S_IMODE(info.st_mode) == 0o700,
                            'dependency_retention_unknown', 'Unknown operation evidence requires review')
        finally:
            os.close(scan_fd)
        os.mkdir(operation, 0o700, dir_fd=family)  # Exclusive, never reopen.
        os.fsync(family)
        return os.open(operation, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW | os.O_CLOEXEC,
                       dir_fd=family)
    finally:
        fcntl.flock(family, fcntl.LOCK_UN)


def _same_entry(parent, name, expected):
    info = os.stat(name, dir_fd=parent, follow_symlinks=False)
    require(stat.S_ISDIR(info.st_mode) and info.st_uid == os.getuid()
            and stat.S_IMODE(info.st_mode) == 0o700
            and (info.st_dev, info.st_ino) == expected,
            'dependency_cleanup_identity', 'Private operation identity changed')


def dispose(family, operation, root, expected, data_expected, close_uploads):
    """Caller has stopped admission and drained/joined EVERY worker.

    No ambient pathname fallback. Require CPython fd-safe rmtree. Only current
    owned data is removed; crashes/old UUID roots are never reopened or swept.
    Account-owner mutation of this private namespace is outside helper trust;
    detected op/data retargets fail closed before each destructive epoch.
    """
    require(sys.platform == 'linux' and shutil.rmtree.avoids_symlink_attacks
            and os.open in os.supports_dir_fd and os.rmdir in os.supports_dir_fd,
            'dependency_cleanup_unsupported', 'FD-safe cleanup is unavailable')
    identity(family)
    require(identity(root) == expected, 'dependency_cleanup_identity', 'Private operation identity changed')
    _same_entry(family, operation, expected)
    _same_entry(root, 'data', data_expected)
    with os.scandir(root) as scan:
        for index, entry in enumerate(scan):
            require(index == 0 and entry.name == 'data', 'dependency_cleanup_recovery',
                    'Interrupted or unknown operation evidence requires review')
    # An exclusive durable intent survives interruption. Never resume a partial
    # deletion just because a later helper happens to find this UUID.
    fd = os.open(INTENT, os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW | os.O_CLOEXEC,
                 0o600, dir_fd=root)
    with os.fdopen(fd, 'wb') as output:
        output.write(encode({'version': 1, 'operation_id': operation, 'phase': 'disposing_private_data'}))
        output.flush()
        os.fsync(output.fileno())
    os.fsync(root)
    close_uploads()
    _same_entry(family, operation, expected)
    _same_entry(root, 'data', data_expected)
    shutil.rmtree('data', dir_fd=root)  # Existing fd-safe stdlib; no own tree writer/walker.
    os.fsync(root)
    _same_entry(family, operation, expected)
    # Only these two entries were ever ours. The marker is removed last; a
    # crash in the following small gap can leave an empty UUID root (counted).
    os.unlink(INTENT, dir_fd=root)
    os.fsync(root)
    os.rmdir(operation, dir_fd=family)
    os.fsync(family)
    return {'version': 1, 'operation_id': operation, 'data_disposed': True, 'operation_removed': True}
