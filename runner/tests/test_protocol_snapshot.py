import io
import os
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch
import uuid

from consolecrypt_runner.errors import RunnerError
from consolecrypt_runner.protocol import decode, encode, read_request, MAX_FRAME_BYTES
from consolecrypt_runner.snapshot import create_snapshot, relative_path, verify_snapshot


class ProtocolTests(unittest.TestCase):
    def test_bounds_duplicate_fields_non_finite_and_future_version(self):
        self.assertEqual(decode(encode({'entries': ({'path': 'playbook.yml'},)})),
                         {'entries': [{'path': 'playbook.yml'}]})
        for raw in (b'{"x":1,"x":2}', b'{"x":NaN}', b'{"x":Infinity}', b'[' * 40 + b'0' + b']' * 40,
                    b' ' * (MAX_FRAME_BYTES + 1), b'\xff'):
            with self.subTest(raw=raw[:32]), self.assertRaises(RunnerError):
                decode(raw)
        request = {"version": 1, "request_id": str(uuid.uuid4()), "operation": "probe", "body": {}}
        self.assertEqual(read_request(io.BytesIO(encode(request) + b'\n')), request)
        for changed in ({**request, "version": True}, {**request, "version": 2}, {**request, "extra": 1}):
            with self.assertRaises(RunnerError):
                read_request(io.BytesIO(encode(changed) + b'\n'))
        with self.assertRaises(RunnerError):
            read_request(io.BytesIO(encode(request)))


@unittest.skipUnless(os.name == "posix", "Unix helper")
class SnapshotTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix="cc-snapshot-test-")
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.source = self.root / "project with spaces"
        self.source.mkdir(mode=0o700)

    def capture(self, name="snapshot", **limits):
        return create_snapshot(self.source, self.root / name, **limits)

    def test_all_used_files_empty_directories_executable_bits_and_crlf_are_bound(self):
        (self.source / '.git').mkdir()
        (self.source / '.git/config').write_text('ignored')
        (self.source / 'empty').mkdir()
        script = self.source / 'script.sh'
        script.write_bytes(b'#!/bin/sh\r\necho ok\r\n')
        script.chmod(0o700)
        (self.source / 'playbook.yml').write_text('- hosts: all\n  gather_facts: false\n')
        first = self.capture()
        self.assertEqual(len(first.files), 2)
        self.assertNotIn('.git', [entry['path'] for entry in first.entries])
        self.assertEqual((self.root / 'snapshot/script.sh').read_bytes(), script.read_bytes())
        self.assertEqual((self.root / 'snapshot/script.sh').stat().st_mode & 0o777, 0o700)
        self.assertEqual(first.digest, self.capture('same').digest)
        script.chmod(0o600)
        self.assertNotEqual(first.digest, self.capture('different-mode').digest)
        (self.source / 'empty').rmdir()
        self.assertNotEqual(first.digest, self.capture('different-directory').digest)
        verify_snapshot(self.root / 'snapshot', first)
        (self.root / 'snapshot/playbook.yml').write_text('- hosts: other\n')
        with self.assertRaisesRegex(RunnerError, 'changed since approval'):
            verify_snapshot(self.root / 'snapshot', first)

    def test_links_and_special_files_are_never_followed(self):
        outside = self.root / 'outside-secret'
        outside.write_text('synthetic-do-not-copy')
        for kind in ('symlink', 'hardlink', 'fifo'):
            path = self.source / 'entry'
            if kind == 'symlink': path.symlink_to(outside)
            elif kind == 'hardlink': os.link(outside, path)
            else: os.mkfifo(path)
            with self.subTest(kind=kind), self.assertRaises(RunnerError):
                self.capture(kind)
            path.unlink()

    def test_directory_swap_cannot_open_a_secret_outside_the_source(self):
        allowed = self.source / 'sub'
        allowed.mkdir()
        (allowed / 'data').write_text('allowed')
        outside = self.root / 'outside'
        outside.mkdir()
        (outside / 'data').write_text('synthetic-secret')
        real_open = os.open
        swapped = False
        def swap(path, flags, *args, **kwargs):
            nonlocal swapped
            if path == 'data' and not swapped:
                swapped = True
                allowed.rename(self.root / 'moved-allowed')
                allowed.symlink_to(outside, target_is_directory=True)
            return real_open(path, flags, *args, **kwargs)
        with patch('consolecrypt_runner.snapshot.os.open', side_effect=swap), self.assertRaises(RunnerError):
            self.capture('race')
        copied = self.root / 'race/sub/data'
        if copied.exists(): self.assertNotEqual(copied.read_text(), 'synthetic-secret')

    def test_limits_paths_and_snapshot_outside_source(self):
        for path in ('../secret', '/absolute', 'a/../secret', 'a//b', 'a\\b', 'C:secret', '.git/config'):
            with self.subTest(path=path), self.assertRaises(RunnerError): relative_path(path)
        (self.source / 'large').write_bytes(b'x' * 20)
        with self.assertRaises(RunnerError): self.capture('too-large', max_bytes=10)
        with self.assertRaises(RunnerError): self.capture('too-many', max_files=0)
        with self.assertRaises(RunnerError): create_snapshot(self.source, self.source / 'nested')


if __name__ == '__main__': unittest.main()
