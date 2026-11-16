"""Private same-identity disk-root custody. No Permit, queue or replay implementation.

The Rust MetricsCollect owner supplies only already verified binding metadata
after Actor admission. Existing records never authorize a launch. An absent or
foreign record in a nonempty root is a refusal, never an adoption/migration.
"""
from dataclasses import asdict, dataclass
import json
import os
from pathlib import Path
import re
import stat
import uuid

STORE_ROOT = Path("/var/lib/consolecrypt-agent/metrics-disk")
LAYOUT = b"cc-prom-agent-hardmode/v1\n"


def root_view_uid():
    def text(name):
        with Path(name).open("rb") as stream:
            raw = stream.read(1025)
        need(len(raw) <= 1024, "disk_namespace_bound")
        return raw.decode("ascii").split()
    uid, gid = os.getuid(), os.getgid()
    actual_uid, actual_gid = text("/proc/self/uid_map"), text("/proc/self/gid_map")
    if actual_uid == actual_gid == ["0", "0", "4294967295"]:
        return 0
    need(uid not in (0, 65534) and gid not in (0, 65534)
         and actual_uid == [str(uid), str(uid), "1"] and actual_gid == [str(gid), str(gid), "1"]
         and text("/proc/sys/kernel/overflowuid") == ["65534"]
         and text("/proc/sys/kernel/overflowgid") == ["65534"], "disk_namespace")
    return 65534  # Mapped view only, never a host-root ownership assertion.


def need(value, code):
    if not value:
        raise ValueError(code)


def identity(info):
    return (info.st_dev, info.st_ino, info.st_mode, info.st_uid, info.st_gid,
            info.st_nlink, info.st_size, info.st_mtime_ns, info.st_ctime_ns)


def object_pairs(rows):
    need(len(rows) == len({key for key, _ in rows}), "disk_duplicate")
    return dict(rows)


def canonical(value):
    return json.dumps(value, sort_keys=True, separators=(",", ":"), ensure_ascii=True).encode("ascii")


def exclusive(fd):
    import fcntl  # Only actual Linux file custody needs this boundary.
    fcntl.flock(fd, fcntl.LOCK_EX | fcntl.LOCK_NB)


def checked_uuid(value):
    need(type(value) is str, "disk_id")
    try:
        parsed = uuid.UUID(value)
    except ValueError as error:
        raise ValueError("disk_id") from error
    need(parsed.int != 0 and str(parsed) == value, "disk_id")
    return parsed


@dataclass(frozen=True)
class DiskBinding:
    owner_kind: str
    owner_id: str
    owner_deployment_id: str
    owner_signing_key_sha256: str
    agent_id: str
    node_key_fingerprint: str
    certificate_sha256: str
    collector_id: str
    manifest_sha256: str

    def checked(self):
        need(self.owner_kind in {"localProfile", "personalAccount"}, "disk_owner_kind")
        for value in (self.owner_id, self.owner_deployment_id, self.agent_id):
            checked_uuid(value)
        resource = checked_uuid(self.collector_id)
        need(resource.version == 8, "disk_collector_id")
        for value in (self.owner_signing_key_sha256, self.node_key_fingerprint,
                      self.certificate_sha256, self.manifest_sha256):
            need(type(value) is str and re.fullmatch("[0-9a-f]{64}", value), "disk_binding_digest")
        address = bytearray.fromhex(self.manifest_sha256)[:16]
        address[6] = (address[6] & 15) | 0x80
        address[8] = (address[8] & 63) | 0x80
        need(str(uuid.UUID(bytes=bytes(address))) == self.collector_id, "disk_manifest_address")
        return self

    def bytes(self):
        self.checked()
        return canonical({"format": "cc-agent-disk-binding/v1", **asdict(self)})

    def check_bytes(self, raw):
        need(type(raw) is bytes and raw == self.bytes(), "disk_binding_changed")


def read_regular(parent_fd, leaf, uid, mode, cap):
    named = os.stat(leaf, dir_fd=parent_fd, follow_symlinks=False)
    fd = os.open(leaf, os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK | os.O_CLOEXEC, dir_fd=parent_fd)
    try:
        info = os.fstat(fd)
        need(identity(info) == identity(named) and stat.S_ISREG(info.st_mode)
             and info.st_uid == uid and stat.S_IMODE(info.st_mode) == mode
             and info.st_nlink == 1 and info.st_size <= cap, "disk_regular")
        data = bytearray()
        while len(data) <= cap:
            part = os.read(fd, min(4096, cap + 1 - len(data)))
            if not part:
                break
            data.extend(part)
        need(len(data) == info.st_size and identity(os.fstat(fd)) == identity(info)
             and identity(os.stat(leaf, dir_fd=parent_fd, follow_symlinks=False)) == identity(info), "disk_file_changed")
        return fd, bytes(data)
    except BaseException:
        os.close(fd)
        raise


class BoundDiskRoot:
    """One retained root+session lock; deliberately has no launch/grant method."""
    def __init__(self, binding):
        binding.checked()
        self.binding, self.uid = binding, os.getuid()
        self.root_fd = self.session_fd = self.wal_fd = self.wal_probe_fd = None
        need(self.uid != 0 and os.geteuid() == self.uid, "disk_uid")
        self.path = STORE_ROOT / binding.collector_id
        try:
            root_uid = root_view_uid()
            for parent in (*reversed(self.path.parents), self.path):
                info = parent.lstat()
                need(stat.S_ISDIR(info.st_mode) and not stat.S_ISLNK(info.st_mode)
                     and info.st_uid in {root_uid, self.uid} and not info.st_mode & 0o022, "disk_parent")
            self.root_fd = os.open(self.path, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW | os.O_CLOEXEC)
            info = os.fstat(self.root_fd)
            need(info.st_uid == self.uid and stat.S_IMODE(info.st_mode) == 0o700, "disk_root")
            self.root_identity = (info.st_dev, info.st_ino)
            self.session_fd, lock = read_regular(self.root_fd, "session.lock", self.uid, 0o600, 0)
            need(lock == b"", "disk_session_lock")
            exclusive(self.session_fd)
            self.wal_fd = os.open("wal", os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW | os.O_CLOEXEC, dir_fd=self.root_fd)
            wal = os.fstat(self.wal_fd)
            need(wal.st_uid == self.uid and stat.S_IMODE(wal.st_mode) == 0o700, "disk_wal_root")
            self.wal_identity = (wal.st_dev, wal.st_ino)
            self.check()
        except BaseException:
            self.close()
            raise

    def check(self):
        need(self.root_fd is not None and self.session_fd is not None and self.wal_fd is not None, "disk_closed")
        held, named = os.fstat(self.root_fd), self.path.stat(follow_symlinks=False)
        need((held.st_dev, held.st_ino) == (named.st_dev, named.st_ino) == self.root_identity
             and stat.S_ISDIR(named.st_mode) and named.st_uid == self.uid and stat.S_IMODE(named.st_mode) == 0o700, "disk_root_changed")
        wal, wal_named = os.fstat(self.wal_fd), os.stat("wal", dir_fd=self.root_fd, follow_symlinks=False)
        need((wal.st_dev, wal.st_ino) == (wal_named.st_dev, wal_named.st_ino) == self.wal_identity
             and wal.st_dev == held.st_dev and stat.S_ISDIR(wal_named.st_mode)
             and wal_named.st_uid == self.uid and stat.S_IMODE(wal_named.st_mode) == 0o700, "disk_wal_changed")
        # Only bounded binding metadata lives outside the native guarded WAL
        # root; no second payload spool, ACK journal or hidden staging tree.
        names = set(os.listdir(self.root_fd))
        need({"session.lock", "wal"} <= names <= {"session.lock", "wal", "binding.json"}, "disk_root_entries")
        lock = os.fstat(self.session_fd)
        need(identity(lock) == identity(os.stat("session.lock", dir_fd=self.root_fd, follow_symlinks=False)), "disk_session_changed")

    def check_binding(self):
        self.check()
        fd, raw = read_regular(self.root_fd, "binding.json", self.uid, 0o600, 2048)
        try:
            self.binding.check_bytes(raw)
        finally:
            os.close(fd)
        self.check()

    def bind_fresh_empty(self):
        # Caller already owns the actual Actor Permit. Root/lock provisioning
        # is a separate explicit install operation; this never creates a root.
        self.check()
        need(set(os.listdir(self.root_fd)) == {"session.lock", "wal"}
             and set(os.listdir(self.wal_fd)) == {"lock"}, "disk_unbound_nonempty")
        self.check_no_native_writer()
        try:
            raw = self.binding.bytes()
            fd = os.open("binding.json", os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW | os.O_CLOEXEC,
                         0o600, dir_fd=self.root_fd)
            try:
                pending = memoryview(raw)
                while pending:
                    count = os.write(fd, pending)
                    need(count > 0, "disk_binding_write")
                    pending = pending[count:]
                os.fchmod(fd, 0o600)
                os.fsync(fd)
            finally:
                os.close(fd)
            os.fsync(self.root_fd)
            self.check_binding()
        finally:
            self.release_native_probe()

    def check_no_native_writer(self):
        # This is ONLY a real lock-exclusion observation. It is not a claimed
        # process join. The actual unit owner separately waits its original
        # child/pidfd/cgroup before another launch may use this same root.
        self.check()
        need(self.wal_probe_fd is None, "disk_native_probe_already_held")
        fd, raw = read_regular(self.wal_fd, "lock", self.uid, 0o600, len(LAYOUT))
        try:
            need(raw == LAYOUT, "disk_native_layout")
            exclusive(fd)
            self.wal_probe_fd = fd
        except BaseException:
            os.close(fd)
            raise
        self.check()

    def release_native_probe(self):
        # Release only immediately before fixed native exec, while this same
        # association session lock remains held by the real unit supervisor.
        if self.wal_probe_fd is not None:
            os.close(self.wal_probe_fd)
            self.wal_probe_fd = None

    def close(self):
        for name in ("wal_probe_fd", "wal_fd", "session_fd", "root_fd"):
            fd = getattr(self, name, None)
            if fd is not None:
                os.close(fd)
                setattr(self, name, None)
