import gzip
import hashlib
import io
import os
from pathlib import Path
import tarfile
import tempfile
import unittest
from unittest.mock import patch

from consolecrypt_runner.dependency_archive import staged_dependency
from consolecrypt_runner.errors import RunnerError
from consolecrypt_runner.snapshot import create_snapshot


def archive(entries, *, compressed=True):
    output = io.BytesIO()
    with tarfile.open(fileobj=output, mode='w', format=tarfile.PAX_FORMAT) as writer:
        for name, kind, content in entries:
            member = tarfile.TarInfo(name)
            member.type = kind
            if kind in (tarfile.SYMTYPE, tarfile.LNKTYPE):
                member.linkname = '../../outside'
            if kind == tarfile.REGTYPE:
                member.size = len(content)
                member.mode = 0o6755 if name.endswith('.sh') else 0o644
                writer.addfile(member, io.BytesIO(content))
            else:
                writer.addfile(member)
    return gzip.compress(output.getvalue()) if compressed else output.getvalue()


@unittest.skipUnless(os.name == 'posix' and os.getuid() != 0, 'Non-root Unix staging helper')
class DependencyArchiveTests(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory(prefix='cc-archive-proof-')
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name)
        self.staging = self.root/'staging'
        self.staging.mkdir(mode=0o700)
        self.original = self.root/'original'
        (self.original/'tasks').mkdir(parents=True)
        (self.original/'empty').mkdir()
        (self.original/'tasks/main.yml').write_bytes(b'---\n- ansible.builtin.debug:\n    msg: synthetic\n')
        (self.original/'helper.sh').write_bytes(b'#!/bin/sh\nexit 0\n')
        (self.original/'helper.sh').chmod(0o700)
        self.snapshot = create_snapshot(self.original, self.root/'snapshot')
        self.entries = [
            ('empty', tarfile.DIRTYPE, b''),
            ('tasks/main.yml', tarfile.REGTYPE, (self.original/'tasks/main.yml').read_bytes()),
            ('tasks', tarfile.DIRTYPE, b''),  # Explicit directory after its child.
            ('helper.sh', tarfile.REGTYPE, (self.original/'helper.sh').read_bytes()),
        ]

    def record(self, raw, **changes):
        return dict({'kind': 'role', 'name': 'demo', 'version': '1.2.3',
            'source': {'url': 'https://example.test/pinned.tar.gz', 'sha256': hashlib.sha256(raw).hexdigest()},
            'tree_digest': self.snapshot.digest}, **changes)

    def denied(self, raw, record=None, **options):
        with self.assertRaises(RunnerError):
            with staged_dependency(raw, record or self.record(raw), self.staging, **options):
                self.fail('Invalid archive became a staged dependency')
        self.assertEqual(list(self.staging.iterdir()), [])

    def test_pinned_role_matches_independent_snapshot_normalizes_modes_and_cleans_up(self):
        for compressed in (False, True):
            raw = archive(self.entries, compressed=compressed)
            with patch('socket.create_connection', side_effect=AssertionError('Offline stage')) as network:
                with staged_dependency(raw, self.record(raw), self.staging) as (tree, snapshot, receipt):
                    independent = create_snapshot(tree, self.root/('verify-' + str(compressed)))
                    self.assertEqual(snapshot.digest, self.snapshot.digest)
                    self.assertEqual(snapshot.digest, independent.digest)
                    self.assertEqual(receipt['target'], 'roles/demo')
                    self.assertEqual(receipt['source']['sha256'], hashlib.sha256(raw).hexdigest())
                    self.assertEqual((tree/'helper.sh').stat().st_mode & 0o7777, 0o700)
                    self.assertTrue((tree/'empty').is_dir())
                    held = tree
                network.assert_not_called()
            self.assertFalse(held.exists())
            self.assertEqual(list(self.staging.iterdir()), [])

    def test_wrapper_policy_is_explicit_single_root_and_not_part_of_tree_digest(self):
        entries = [('demo-1.2.3', tarfile.DIRTYPE, b'')]
        entries += [('demo-1.2.3/' + name, kind, data) for name, kind, data in self.entries]
        raw = archive(entries)
        with staged_dependency(raw, self.record(raw), self.staging, strip_components=1) as (_, snapshot, receipt):
            self.assertEqual(snapshot.digest, self.snapshot.digest)
            self.assertEqual(receipt['strip_components'], 1)
        self.denied(raw)
        self.denied(archive(entries + [('second/tasks/main.yml', tarfile.REGTYPE, b'---')]), strip_components=1)
        self.denied(archive([('file', tarfile.REGTYPE, b'not-a-wrapper')]), strip_components=1)
        self.denied(raw, strip_components=True)
        self.denied(raw, strip_components=2)

    def test_archive_hash_tree_pin_and_plainly_malformed_content_fail_without_publication(self):
        raw = archive(self.entries)
        wrong = self.record(raw)
        wrong['source']['sha256'] = '0' * 64
        with patch('tarfile.open', side_effect=AssertionError('Hash must precede parser')):
            self.denied(raw, wrong)
        self.denied(raw, self.record(raw, tree_digest='0'*64))
        self.denied(b'not a tar file')
        self.denied(raw[:-6])
        self.denied(archive(self.entries, compressed=False) + b'unexpected-hidden-payload')

    def test_links_special_files_duplicates_parent_replacement_and_portable_collisions_are_denied(self):
        for kind in (tarfile.SYMTYPE, tarfile.LNKTYPE, tarfile.FIFOTYPE, tarfile.CHRTYPE,
                     tarfile.BLKTYPE, tarfile.GNUTYPE_SPARSE):
            with self.subTest(kind=kind):
                self.denied(archive(self.entries + [('extra', kind, b'')]))
        for extra in (
            [('tasks/main.yml', tarfile.REGTYPE, b'replacement')],
            [('tasks', tarfile.REGTYPE, b'file-replaces-dir')],
            [('parent', tarfile.REGTYPE, b'file'), ('parent/child', tarfile.REGTYPE, b'child')],
            [('Tasks/main.yml', tarfile.REGTYPE, b'case-collision')],
            [('empty', tarfile.DIRTYPE, b'')],
        ):
            with self.subTest(extra=extra):
                self.denied(archive(self.entries + extra))
        for path in ('../outside', '/absolute', 'a/../../outside', 'a\\outside', 'C:/outside',
                     './tasks/main.yml', '.git/config', 'a//b', 'NUL.txt', 'aux', 'name.',
                     'name ', 'a\nb', 'a\x01b', 'a?b', 'e\u0301.txt', 'a/'*33+'b'):
            with self.subTest(path=path):
                self.denied(archive(self.entries + [(path, tarfile.REGTYPE, b'never-write')]))
        self.assertFalse((self.root/'outside').exists())

    def test_budgets_cover_compressed_bytes_expanded_metadata_inferred_dirs_and_file_payloads(self):
        raw = archive(self.entries)
        with patch('consolecrypt_runner.dependency_archive.MAX_ARCHIVE_BYTES', len(raw)-1):
            self.denied(raw)
        with patch('consolecrypt_runner.dependency_archive.MAX_TAR_BYTES', 1024):
            self.denied(raw)
        with patch('consolecrypt_runner.dependency_archive.MAX_BYTES', 2):
            self.denied(raw)
        with patch('consolecrypt_runner.dependency_archive.MAX_FILES', 1):
            self.denied(raw)
        deep = archive([('a/b/c/d/tasks/main.yml', tarfile.REGTYPE, b'---')])
        with patch('consolecrypt_runner.dependency_archive.MAX_FILES', 2):
            self.denied(deep)

    def test_forged_pax_and_gnu_sizes_are_bounded_before_parser_allocation(self):
        for kind in (tarfile.XHDTYPE, tarfile.XGLTYPE, tarfile.GNUTYPE_LONGNAME, tarfile.GNUTYPE_LONGLINK):
            member = tarfile.TarInfo('metadata')
            member.type = kind
            member.size = 2**60
            header = member.tobuf(format=tarfile.GNU_FORMAT)
            with self.subTest(kind=kind):
                self.denied(header + bytes(1024))

    def test_caller_cannot_replace_the_pinned_tree_while_parser_work_is_in_flight(self):
        raw = archive(self.entries)
        record = self.record(raw, tree_digest='0'*64)
        original = tarfile.open

        def mutate(*args, **kwargs):
            record['tree_digest'] = self.snapshot.digest
            return original(*args, **kwargs)

        with patch('consolecrypt_runner.dependency_archive.tarfile.open', side_effect=mutate):
            self.denied(raw, record)

    def test_caller_failure_cleans_only_own_stage_and_shared_parent_is_rejected(self):
        sentinel = self.root/'keep'
        sentinel.write_text('user-owned-synthetic-marker')
        raw = archive(self.entries)
        with self.assertRaisesRegex(RuntimeError, 'caller-aborted'):
            with staged_dependency(raw, self.record(raw), self.staging):
                raise RuntimeError('caller-aborted')
        self.assertEqual(list(self.staging.iterdir()), [])
        self.assertEqual(sentinel.read_text(), 'user-owned-synthetic-marker')
        self.staging.chmod(0o755)
        self.denied(raw)
        self.staging.chmod(0o700)
        alias = self.root/'stage-link'
        alias.symlink_to(self.staging, target_is_directory=True)
        with self.assertRaises(RunnerError):
            with staged_dependency(raw, self.record(raw), alias):
                self.fail('Symlink parent was accepted')

    def test_collection_layout_and_wrong_entry_point_are_checked(self):
        source = self.root/'collection'
        source.mkdir()
        data = b'namespace: example\nname: demo\nversion: 1.2.3\n'
        (source/'galaxy.yml').write_bytes(data)
        snapshot = create_snapshot(source, self.root/'collection-snapshot')
        raw = archive([('galaxy.yml', tarfile.REGTYPE, data)])
        record = self.record(raw, kind='collection', name='example.demo', tree_digest=snapshot.digest)
        with staged_dependency(raw, record, self.staging) as (_, _, receipt):
            self.assertEqual(receipt['target'], 'collections/ansible_collections/example/demo')
        self.denied(raw, self.record(raw, tree_digest=snapshot.digest))
