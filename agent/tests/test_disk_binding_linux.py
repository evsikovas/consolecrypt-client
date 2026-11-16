"""Owned Linux component only: requires a fresh fixed STORE_ROOT fixture mount.

The outer existing fixture supplies this isolated empty uid-owned mount. These
tests observe actual flock/inode behavior; they are not persistent-disk/systemd
or Actor acceptance. No arbitrary path arguments or kernel proof booleans.
"""
from pathlib import Path
import dataclasses
import importlib.util
import os
import shutil
import stat
import sys
import unittest


@unittest.skipUnless(sys.platform == 'linux' and os.environ.get('CC_OWNED_DISK_BINDING_TEST') == '1', 'explicit owned Linux mount fixture required')
class RealRootLocks(unittest.TestCase):
    def setUp(self):
        path = Path(__file__).resolve().parents[1] / 'collector/disk_binding.py'
        spec = importlib.util.spec_from_file_location('cc_disk_linux_binding', path)
        self.disk = importlib.util.module_from_spec(spec); sys.modules[spec.name] = self.disk; spec.loader.exec_module(self.disk)
        self.assertGreater(os.getuid(), 0)
        base = self.disk.STORE_ROOT
        info = base.lstat()
        self.assertTrue(stat.S_ISDIR(info.st_mode) and info.st_uid == os.getuid() and stat.S_IMODE(info.st_mode) == 0o700)
        self.assertEqual(list(base.iterdir()), [])
        # Full fixed synthetic public binding, not an authority credential.
        sys.path.insert(0, str(Path(__file__).parent))
        try:
            from test_disk_binding_data import original
            self.binding = original()
        finally: sys.path.pop(0)
        self.root = base / self.binding.collector_id; self.root.mkdir(mode=0o700)
        self.inode = self.root.stat().st_ino
        (self.root / 'wal').mkdir(mode=0o700)
        for leaf, raw in [('session.lock', b''), ('wal/lock', self.disk.LAYOUT)]:
            fd = os.open(self.root / leaf, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
            try: self.assertEqual(os.write(fd, raw), len(raw))
            finally: os.close(fd)

    def tearDown(self):
        self.assertEqual(self.root.stat(follow_symlinks=False).st_ino, self.inode)
        self.assertFalse(self.root.is_symlink())
        shutil.rmtree(self.root) # exact newly created owned fixture subtree only

    def test_actual_session_and_native_wal_locks_exclude_second_owner(self):
        import fcntl
        owner = self.disk.BoundDiskRoot(self.binding)
        try:
            with self.assertRaises(BlockingIOError): self.disk.BoundDiskRoot(self.binding)
            owner.bind_fresh_empty(); owner.check_binding(); owner.check_no_native_writer()
            fd = os.open(self.root / 'wal/lock', os.O_RDONLY)
            try:
                with self.assertRaises(BlockingIOError): fcntl.flock(fd, fcntl.LOCK_EX | fcntl.LOCK_NB)
                owner.release_native_probe(); fcntl.flock(fd, fcntl.LOCK_EX | fcntl.LOCK_NB)
                with self.assertRaises(BlockingIOError): owner.check_no_native_writer()
            finally: os.close(fd)
        finally: owner.close()

    def test_reopen_requires_original_binding_and_never_rewrites_new_certificate(self):
        owner = self.disk.BoundDiskRoot(self.binding)
        try: owner.bind_fresh_empty()
        finally: owner.close()
        raw = (self.root / 'binding.json').read_bytes()
        owner = self.disk.BoundDiskRoot(dataclasses.replace(self.binding, certificate_sha256='f' * 64))
        try:
            with self.assertRaisesRegex(ValueError, 'disk_binding_changed'): owner.check_binding()
            self.assertEqual((self.root / 'binding.json').read_bytes(), raw)
        finally: owner.close()

    def test_missing_native_layout_is_refusal_without_repair_or_binding_write(self):
        (self.root / 'wal/lock').unlink()
        owner = self.disk.BoundDiskRoot(self.binding)
        try:
            with self.assertRaises(ValueError): owner.bind_fresh_empty()
            self.assertFalse((self.root / 'binding.json').exists())
            self.assertFalse((self.root / 'wal/lock').exists())
        finally: owner.close()


if __name__ == '__main__': unittest.main()
