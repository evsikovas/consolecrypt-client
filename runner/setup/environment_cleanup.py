"""Staged built-in managed Runner cleanup: Linux only, exact UUIDs, no path RPC.

The desktop must authorize an opaque one-use Native plan before invoking delete.
No function executes a managed Python, project, shell, sudo or system installer.
"""
from __future__ import annotations

import ctypes
from collections import deque
import errno
import hashlib
import json
import os
from pathlib import Path
import platform
import pwd
import re
import select
import stat
import struct
import sys
import time
import uuid

CC_CLEANUP_MAX_ENTRIES = 128
CC_CLEANUP_MAX_NODES = 100000
CC_CLEANUP_MAX_BYTES = 2 * 1024 * 1024 * 1024
CC_CLEANUP_MAX_SECONDS = 10
CC_CLEANUP_MAX_DEPTH = 64
CC_CLEANUP_NO_XDEV = 0x01
CC_CLEANUP_NO_MAGICLINKS = 0x02
CC_CLEANUP_NO_SYMLINKS = 0x04
CC_CLEANUP_BENEATH = 0x08


class _CleanupControl:
    """The parent keeps stdin OPEN after the one request line until completion.

    No heartbeat bytes are needed: EOF or any unexpected extra frame revokes
    the live lease. Poll synchronously before every filesystem mutation, rather
    than rely on scheduling a watchdog thread or remote HUP delivery.
    """
    def __init__(self, descriptor):
        self.poll = select.poll()
        self.poll.register(descriptor, select.POLLIN | select.POLLHUP | select.POLLERR)

    def check(self):
        if self.poll.poll(0):
            raise ValueError('cleanup_cancelled')


class _CleanupHow(ctypes.Structure):
    _fields_ = [('flags', ctypes.c_uint64), ('mode', ctypes.c_uint64),
                ('resolve', ctypes.c_uint64)]


def _cleanup_uuid(value):
    if not isinstance(value, str):
        raise ValueError('cleanup_invalid_id')
    try:
        parsed = uuid.UUID(value)
    except ValueError:
        raise ValueError('cleanup_invalid_id') from None
    if parsed.int == 0 or str(parsed) != value:
        raise ValueError('cleanup_invalid_id')
    return value


def _cleanup_open(parent, name, *, directory=True, no_xdev=True, readable=False):
    # Linux x86_64/aarch64 SYS_openat2=437. Kernel-enforced NO_XDEV also rejects
    # same-filesystem bind mounts. Unsupported kernels/seccomp fail closed.
    if sys.platform != 'linux' or platform.machine() not in ('x86_64', 'aarch64'):
        raise ValueError('cleanup_unsupported')
    if not isinstance(name, str) or not name or name.startswith('/') or '\x00' in name:
        raise ValueError('cleanup_unsafe_path')
    flags = os.O_CLOEXEC | os.O_NOFOLLOW | (os.O_RDONLY | os.O_DIRECTORY if directory else
                                        os.O_RDONLY | os.O_NONBLOCK if readable else os.O_PATH)
    resolve = CC_CLEANUP_BENEATH | CC_CLEANUP_NO_MAGICLINKS | CC_CLEANUP_NO_SYMLINKS
    if no_xdev:
        resolve |= CC_CLEANUP_NO_XDEV
    how = _CleanupHow(flags, 0, resolve)
    libc = ctypes.CDLL(None, use_errno=True)
    libc.syscall.restype = ctypes.c_long
    result = libc.syscall(ctypes.c_long(437), ctypes.c_int(parent),
                          ctypes.c_char_p(os.fsencode(name)), ctypes.byref(how),
                          ctypes.c_size_t(ctypes.sizeof(how)))
    if result < 0:
        code = ctypes.get_errno()
        if code in (errno.ENOSYS, errno.EINVAL, errno.EPERM):
            raise ValueError('cleanup_unsupported')
        if code == errno.EXDEV:
            raise ValueError('cleanup_mount_boundary')
        if code == errno.ENOENT:
            raise FileNotFoundError('cleanup_missing')
        raise ValueError('cleanup_unsafe_entry')
    return result


def _cleanup_owned(info, uid, *, private=False, interior=False):
    # Inner wheel/venv directories may legitimately be 0775. The owned 0700
    # boundary isolates that group access; markers/ancestors remain nonwritable.
    unsafe = 0o6002 if interior else 0o6022
    if info.st_uid != uid or (not stat.S_ISLNK(info.st_mode) and info.st_mode & unsafe):
        raise ValueError('cleanup_unsafe_owner_mode')
    if private and (not stat.S_ISDIR(info.st_mode) or stat.S_IMODE(info.st_mode) != 0o700):
        raise ValueError('cleanup_unsafe_owner_mode')
    if not (stat.S_ISDIR(info.st_mode) or stat.S_ISREG(info.st_mode) or stat.S_ISLNK(info.st_mode)):
        raise ValueError('cleanup_special_file')


def _cleanup_parent():
    uid = os.getuid()
    if uid == 0:
        raise ValueError('cleanup_non_root_required')
    identity = pwd.getpwuid(uid)
    home = Path(identity.pw_dir)
    if (not home.is_absolute() or str(home) == '/' or len(str(home)) > 3500
            or any(part in ('.', '..') for part in home.parts)):
        raise ValueError('cleanup_unsafe_home')
    root = os.open('/', os.O_RDONLY | os.O_DIRECTORY | os.O_CLOEXEC | os.O_NOFOLLOW)
    current = None
    try:
        # The explicit user's home may itself be on a separate filesystem;
        # below this anchor, no mount crossing or symbolic component is allowed.
        current = _cleanup_open(root, str(home).lstrip('/'), no_xdev=False)
        _cleanup_owned(os.fstat(current), uid)
        for component in ('.local', 'share', 'consolecrypt', 'runner-environments'):
            following = _cleanup_open(current, component, no_xdev=False)
            os.close(current)
            current = following
            _cleanup_owned(os.fstat(current), uid)
        info = os.fstat(current)
        _cleanup_owned(info, uid, private=True)
        result = {'uid': uid, 'user': identity.pw_name, 'home': str(home),
                  'parent': str(home / '.local/share/consolecrypt/runner-environments'),
                  'device': info.st_dev, 'inode': info.st_ino, 'mode': stat.S_IMODE(info.st_mode)}
        owned = current
        current = None
        return owned, result
    finally:
        os.close(root)
        if current is not None:
            os.close(current)


def _cleanup_names(parent, maximum=CC_CLEANUP_MAX_ENTRIES):
    names = []
    with os.scandir(parent) as entries:
        for entry in entries:
            if len(names) >= maximum:
                raise ValueError('cleanup_budget')
            names.append(entry.name)
    return sorted(names, key=os.fsencode)


def _cleanup_descriptor(name):
    try:
        return {'id': _cleanup_uuid(name), 'environment_id': name, 'quarantined': False}
    except ValueError:
        pass
    match = re.fullmatch(r'\.cleanup-([0-9a-f-]{36})-([0-9a-f-]{36})', name)
    if match:
        environment = _cleanup_uuid(match.group(1))
        quarantine = _cleanup_uuid(match.group(2))
        return {'id': quarantine, 'environment_id': environment, 'quarantined': True}
    return None


def _cleanup_stat(info, *, root=False):
    # Quarantine rename changes the root ctime, but not its identity/content.
    return (info.st_dev, info.st_ino, info.st_uid, info.st_mode,
            0 if root else info.st_size, 0 if root else info.st_mtime_ns,
            0 if root else info.st_ctime_ns)


def _cleanup_receipt(directory, uid, expected):
    fd = _cleanup_open(directory, 'installation.json', directory=False)
    try:
        info = os.fstat(fd)
        _cleanup_owned(info, uid)
        if not stat.S_ISREG(info.st_mode) or info.st_size > 8192:
            raise ValueError('cleanup_invalid_receipt')
        # O_PATH verifies the inode/mount without following a symlink. Reopen
        # the marker read-only and compare inode identity before reading bytes.
        readable = _cleanup_open(directory, 'installation.json', directory=False, readable=True)
        try:
            if _cleanup_stat(os.fstat(readable)) != _cleanup_stat(info):
                raise ValueError('cleanup_receipt_changed')
            raw = os.read(readable, 8193)
        finally:
            os.close(readable)
        if len(raw) > 8192:
            raise ValueError('cleanup_invalid_receipt')
        def unique(pairs):
            value = {}
            for key, item in pairs:
                if key in value:
                    raise ValueError('cleanup_invalid_receipt')
                value[key] = item
            return value
        try:
            receipt = json.loads(raw, object_pairs_hook=unique)
        except (UnicodeDecodeError, json.JSONDecodeError):
            raise ValueError('cleanup_invalid_receipt') from None
        if (not isinstance(receipt, dict) or receipt.get('version') != 1
                or type(receipt.get('version')) is not int
                or set(receipt) - {'version', 'paths', 'package_sha256', 'package_fingerprint'}
                or receipt.get('paths') != expected
                or not isinstance(receipt.get('package_sha256'), str)
                or re.fullmatch(r'[0-9a-f]{64}', receipt['package_sha256']) is None
                or receipt.get('package_fingerprint') is not None
                and (not isinstance(receipt['package_fingerprint'], str)
                     or re.fullmatch(r'[0-9a-f]{64}', receipt['package_fingerprint']) is None)):
            raise ValueError('cleanup_invalid_receipt')
        return hashlib.sha256(raw).hexdigest()
    finally:
        os.close(fd)


class _CleanupBudget:
    def __init__(self, until=None):
        self.nodes = 0
        self.bytes = 0
        self.until = until if until is not None else time.monotonic() + CC_CLEANUP_MAX_SECONDS

    def take(self, info, depth):
        self.nodes += 1
        self.bytes += info.st_size if stat.S_ISREG(info.st_mode) or stat.S_ISLNK(info.st_mode) else 0
        if (self.nodes > CC_CLEANUP_MAX_NODES or self.bytes > CC_CLEANUP_MAX_BYTES
                or depth > CC_CLEANUP_MAX_DEPTH or time.monotonic() > self.until):
            raise ValueError('cleanup_budget')


def _cleanup_walk(directory, uid, digest, budget, depth=0):
    before = os.fstat(directory)
    _cleanup_owned(before, uid, interior=True)
    budget.take(before, depth)
    digest.update(json.dumps(_cleanup_stat(before, root=depth == 0)).encode())
    names = _cleanup_names(directory, CC_CLEANUP_MAX_NODES - budget.nodes + 1)
    for name in names:
        encoded = os.fsencode(name)
        digest.update(struct.pack('>I', len(encoded)) + encoded)
        fd = _cleanup_open(directory, name, directory=False)
        try:
            info = os.fstat(fd)
            _cleanup_owned(info, uid, interior=True)
            if stat.S_ISDIR(info.st_mode):
                child = _cleanup_open(directory, name)
                try:
                    if _cleanup_stat(os.fstat(child)) != _cleanup_stat(info):
                        raise ValueError('cleanup_tree_changed')
                    _cleanup_walk(child, uid, digest, budget, depth + 1)
                finally:
                    os.close(child)
            else:
                budget.take(info, depth + 1)
                digest.update(json.dumps(_cleanup_stat(info)).encode())
        finally:
            os.close(fd)
    after = os.fstat(directory)
    if (before.st_mtime_ns, before.st_ctime_ns) != (after.st_mtime_ns, after.st_ctime_ns):
        raise ValueError('cleanup_tree_changed')


def _cleanup_protection(paths, selected):
    if (not isinstance(paths, list) or len(paths) > 96 or
            any(not isinstance(path, str) or not path.startswith('/') or len(path) > 4096
                or any(ord(char) < 32 for char in path) for path in paths)
            or sum(len(path.encode()) for path in paths) > 16384
            or selected is not None and (not isinstance(selected, str) or not selected.startswith('/')
                                         or len(selected) > 4096 or any(ord(c) < 32 for c in selected))):
        raise ValueError('cleanup_invalid_request')


def _cleanup_touches(path, boundaries, until):
    # Deny-only resolution of Native-saved references. Record intermediate link
    # targets too: alias -> managed/venv/bin/python -> /usr/bin/python must still
    # protect the managed environment. No file content or executable is opened.
    pending = deque(path.split('/'))
    current = '/'
    links = steps = 0
    while pending:
        steps += 1
        if steps > 1024 or links > 40 or time.monotonic() > until:
            raise ValueError('cleanup_catalog_unavailable')
        component = pending.popleft()
        if component in ('', '.'):
            continue
        if component == '..':
            current = os.path.dirname(current)
            continue
        candidate = os.path.join(current, component)
        if any(candidate == boundary or candidate.startswith(boundary + '/') for boundary in boundaries):
            return True
        try:
            info = os.lstat(candidate)
        except FileNotFoundError:
            current = candidate
            continue
        except OSError:
            raise ValueError('cleanup_catalog_unavailable') from None
        if stat.S_ISLNK(info.st_mode):
            if candidate.startswith(('/proc/', '/sys/', '/dev/')):
                raise ValueError('cleanup_catalog_unavailable')
            target = os.readlink(candidate)
            links += 1
            if target.startswith('/'):
                current = '/'
            pending.extendleft(reversed(target.split('/')))
        else:
            current = candidate
    return False


def _cleanup_inspect(parent, owner, name, until=None, protected_paths=None, selected_python=None):
    descriptor = _cleanup_descriptor(name)
    if descriptor is None:
        raise ValueError('cleanup_invalid_id')
    directory = _cleanup_open(parent, name)
    try:
        info = os.fstat(directory)
        _cleanup_owned(info, owner['uid'], private=True)
        original = owner['parent'] + '/' + descriptor['environment_id']
        actual_path = owner['parent'] + '/' + name
        deadline = until if until is not None else time.monotonic() + CC_CLEANUP_MAX_SECONDS
        boundaries = (original, actual_path)
        for interpreter in (sys.executable, sys.prefix, sys.exec_prefix):
            for current in (os.path.abspath(interpreter), os.path.realpath(interpreter)):
                if any(current == boundary or current.startswith(boundary + '/')
                       for boundary in (original, actual_path)):
                    raise ValueError('cleanup_current_interpreter')
        if selected_python is not None and _cleanup_touches(selected_python, boundaries, deadline):
            raise ValueError('cleanup_selected')
        if any(_cleanup_touches(path, boundaries, deadline) for path in protected_paths or []):
            raise ValueError('cleanup_registered')
        expected = {'python': original + '/venv/bin/python', 'module_dir': original + '/src',
                    'root': original + '/state'}
        receipt = _cleanup_receipt(directory, owner['uid'], expected)
        if not descriptor['quarantined']:
            for component in ('venv', 'src', 'state'):
                child = _cleanup_open(directory, component)
                try:
                    _cleanup_owned(os.fstat(child), owner['uid'], interior=True)
                finally:
                    os.close(child)
        budget = _CleanupBudget(until)
        digest = hashlib.sha256()
        _cleanup_walk(directory, owner['uid'], digest, budget)
        revision = hashlib.sha256(json.dumps({'owner': owner, 'identity': _cleanup_stat(info, root=True),
                                             'receipt': receipt, 'tree': digest.hexdigest()},
                                            sort_keys=True, separators=(',', ':')).encode()).hexdigest()
        return {**descriptor, 'path': owner['parent'] + '/' + name, 'paths': expected,
                'revision': revision, 'receipt_sha256': receipt,
                'device': info.st_dev, 'inode': info.st_ino,
                'nodes': budget.nodes, 'bytes': budget.bytes, 'eligible': True,
                'profile_ownership': 'unknown', 'contains_state': True}
    finally:
        os.close(directory)


def cleanup_scan(protected_paths=None, selected_python=None):
    protected_paths = [] if protected_paths is None else protected_paths
    _cleanup_protection(protected_paths, selected_python)
    try:
        parent, owner = _cleanup_parent()
    except FileNotFoundError:
        return {'version': 1, 'identity': None, 'candidates': [], 'other_profiles_checked': False}
    try:
        candidates = []
        until = time.monotonic() + CC_CLEANUP_MAX_SECONDS
        for name in _cleanup_names(parent):
            descriptor = _cleanup_descriptor(name)
            if descriptor is None:
                continue
            try:
                candidates.append(_cleanup_inspect(parent, owner, name, until, protected_paths, selected_python))
            except (ValueError, OSError) as error:
                reason = str(error) if isinstance(error, ValueError) else 'cleanup_unsafe_entry'
                candidates.append({**descriptor, 'path': owner['parent'] + '/' + name,
                                   'eligible': False, 'blocked_reason': reason})
        result = {'version': 1, 'identity': owner, 'candidates': candidates, 'other_profiles_checked': False}
        if len(json.dumps(result).encode()) > 60000:
            raise ValueError('cleanup_budget')
        return result
    finally:
        os.close(parent)


def _cleanup_rename(parent, source, destination):
    libc = ctypes.CDLL(None, use_errno=True)
    try:
        rename = libc.renameat2
    except AttributeError:
        raise ValueError('cleanup_unsupported') from None
    rename.argtypes = [ctypes.c_int, ctypes.c_char_p, ctypes.c_int, ctypes.c_char_p, ctypes.c_uint]
    rename.restype = ctypes.c_int
    if rename(parent, os.fsencode(source), parent, os.fsencode(destination), 1) != 0:
        raise ValueError('cleanup_quarantine_failed')


def _cleanup_delete_tree(directory, uid, budget, control, depth=0):
    control.check()
    _cleanup_owned(os.fstat(directory), uid, interior=True)
    # Keep the receipt until every other entry is gone, allowing an explicit
    # fresh review/resume of a partially deleted quarantine after a crash.
    names = _cleanup_names(directory, CC_CLEANUP_MAX_NODES)
    if depth == 0 and 'installation.json' in names:
        names.remove('installation.json')
        names.append('installation.json')
    for name in names:
        fd = _cleanup_open(directory, name, directory=False)
        try:
            info = os.fstat(fd)
            _cleanup_owned(info, uid, interior=True)
            budget.take(info, depth)
            if stat.S_ISDIR(info.st_mode):
                child = _cleanup_open(directory, name)
                try:
                    if _cleanup_stat(os.fstat(child)) != _cleanup_stat(info):
                        raise ValueError('cleanup_tree_changed')
                    _cleanup_delete_tree(child, uid, budget, control, depth + 1)
                finally:
                    os.close(child)
                current = os.stat(name, dir_fd=directory, follow_symlinks=False)
                if (current.st_dev, current.st_ino) != (info.st_dev, info.st_ino):
                    raise ValueError('cleanup_tree_changed')
                control.check()
                os.rmdir(name, dir_fd=directory)
            else:
                current = os.stat(name, dir_fd=directory, follow_symlinks=False)
                if (current.st_dev, current.st_ino) != (info.st_dev, info.st_ino):
                    raise ValueError('cleanup_tree_changed')
                control.check()
                os.unlink(name, dir_fd=directory)  # Symlink inode only; no follow.
        finally:
            os.close(fd)


def cleanup_delete(payload, control):
    control.check()
    if (not isinstance(payload, dict) or set(payload) != {'action', 'identity', 'candidates', 'protected_paths', 'selected_python'}
            or payload['action'] != 'cleanup_delete' or not isinstance(payload['candidates'], list)
            or not 1 <= len(payload['candidates']) <= 8):
        raise ValueError('cleanup_invalid_request')
    _cleanup_protection(payload['protected_paths'], payload['selected_python'])
    parent, owner = _cleanup_parent()
    held = []
    try:
        until = time.monotonic() + CC_CLEANUP_MAX_SECONDS
        if owner != payload['identity']:
            raise ValueError('cleanup_identity_changed')
        names = _cleanup_names(parent)
        by_id = {}
        for name in names:
            descriptor = _cleanup_descriptor(name)
            if descriptor is not None:
                if descriptor['id'] in by_id:
                    raise ValueError('cleanup_ambiguous_id')
                by_id[descriptor['id']] = name
        seen = set()
        nodes = 0
        total_bytes = 0
        # Verify and hold every selected inode before any quarantine mutation.
        for requested in payload['candidates']:
            if (not isinstance(requested, dict) or set(requested) != {'id', 'revision'}
                    or re.fullmatch(r'[0-9a-f]{64}', str(requested['revision'])) is None):
                raise ValueError('cleanup_invalid_request')
            identifier = _cleanup_uuid(requested['id'])
            if identifier in seen or identifier not in by_id:
                raise ValueError('cleanup_candidate_changed')
            seen.add(identifier)
            name = by_id[identifier]
            candidate = _cleanup_inspect(parent, owner, name, until, payload['protected_paths'], payload['selected_python'])
            if candidate['revision'] != requested['revision']:
                raise ValueError('cleanup_candidate_changed')
            nodes += candidate['nodes']
            total_bytes += candidate['bytes']
            if nodes > CC_CLEANUP_MAX_NODES or total_bytes > CC_CLEANUP_MAX_BYTES:
                raise ValueError('cleanup_budget')
            directory = _cleanup_open(parent, name)
            info = os.fstat(directory)
            if (info.st_dev, info.st_ino) != (candidate['device'], candidate['inode']):
                os.close(directory)
                raise ValueError('cleanup_candidate_changed')
            held.append((name, candidate, directory))
        result = []
        budget = _CleanupBudget(until)
        for name, candidate, directory in held:
            before_nodes, before_bytes = budget.nodes, budget.bytes
            quarantine = name if candidate['quarantined'] else '.cleanup-' + candidate['environment_id'] + '-' + str(uuid.uuid4())
            if not candidate['quarantined']:
                control.check()
                _cleanup_rename(parent, name, quarantine)
            actual = _cleanup_inspect(parent, owner, quarantine, until, payload['protected_paths'], payload['selected_python'])
            if (actual['device'], actual['inode'], actual['receipt_sha256'], actual['revision']) != (
                    candidate['device'], candidate['inode'], candidate['receipt_sha256'], candidate['revision']):
                raise ValueError('cleanup_quarantine_changed')
            _cleanup_delete_tree(directory, owner['uid'], budget, control)
            current = os.stat(quarantine, dir_fd=parent, follow_symlinks=False)
            if (current.st_dev, current.st_ino) != (candidate['device'], candidate['inode']):
                raise ValueError('cleanup_quarantine_changed')
            control.check()
            os.rmdir(quarantine, dir_fd=parent)
            result.append({'id': candidate['id'], 'removed': True,
                           'nodes': budget.nodes - before_nodes, 'bytes': budget.bytes - before_bytes})
        return {'version': 1, 'removed': result, 'other_profiles_checked': False}
    finally:
        for _, _, directory in held:
            os.close(directory)
        os.close(parent)
