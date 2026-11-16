"""Private control-plane preparation. No capability or user-checkout writer.

The native user-action gate must bind a selected source to network review before
calling prepare and bind reviewed bytes before commit. A request body alone is
not evidence of a human action. Project guest output cannot invoke this API.
"""
import base64
import hashlib
import os
from pathlib import Path
import shutil
import ssl
import stat
import sys
import tempfile
import threading
from urllib.parse import urlsplit

from .dependencies import LOCK_FILE, dependency_target, parse_lock, check_locked_dependencies
from .dependency_archive import MAX_ARCHIVE_BYTES, preview_dependency
from .errors import require
from .protocol import decode, encode, uuid_field
from .revisions import RevisionRepository
from .sandbox import bubblewrap_argv, run_bounded
from .snapshot import create_snapshot, verify_snapshot, relative_path, ProjectSnapshot
from .store import _private_directory, atomic_json, read_json

RECEIPTS = '.consolecrypt-dependency-receipts.json'
MARKER = 'publication-pending.json'
ARCHIVE_FILE = 'source-archive.bin'
DOWNLOAD_GATE = threading.BoundedSemaphore(1)


def selection_record(selection):
    require(isinstance(selection, dict) and set(selection) == {'kind', 'name', 'version', 'source'},
            'dependency_selection', 'Select exact kind, name, version and artifact source')
    # Deep copy before any blocking work. Sentinel is schema validation only,
    # never represented as the archive-derived tree digest or trusted source.
    value = decode(encode(dict(selection, tree_digest='0' * 64)))
    dependency_target(value)
    require(value['source']['url'].isascii(), 'dependency_source', 'Use an ASCII HTTPS artifact URI')
    return value


def source_origin(record):
    url = urlsplit(record['source']['url'])
    return 'https://' + url.netloc


def capture_archive(private_parent, record, cancel, *, _ca_pem=None, _timeout=50):
    """Hard external process deadline, and only an empty project is mounted.

    _ca_pem is trusted test/admin configuration; never accepted by RPC.
    """
    record = decode(encode(record)); dependency_target(record)
    require(type(_timeout) is int and 1 <= _timeout <= 50, 'dependency_deadline', 'Invalid download deadline')
    require(not cancel.is_set(), 'cancelled', 'Dependency preparation was cancelled')
    require(DOWNLOAD_GATE.acquire(blocking=False), 'dependency_busy', 'Another dependency download is running')
    try:
        with tempfile.TemporaryDirectory(prefix='dependency-download-', dir=private_parent) as temporary:
            root = Path(temporary); empty = root/'empty'; empty.mkdir(mode=0o700)
            spec = root/'input.json'; atomic_json(spec, {'record':record,'custom_ca':_ca_pem is not None})
            mounts = [(spec, '/tool/dependency-input.json')]
            if _ca_pem is not None:
                require(isinstance(_ca_pem, str) and _ca_pem.isascii() and len(_ca_pem) <= 65536,
                        'dependency_ca', 'Invalid public test certificate')
                ssl.create_default_context(cadata=_ca_pem)
                certificate = root/'ca.pem'; certificate.write_text(_ca_pem, encoding='ascii'); certificate.chmod(0o600)
                mounts.append((certificate, '/tool/dependency-ca.pem'))
            raw = bytearray()

            def receive(chunk):
                require(len(raw) + len(chunk) <= MAX_ARCHIVE_BYTES, 'dependency_archive_size', 'Download exceeds compressed budget')
                raw.extend(chunk)

            argv = bubblewrap_argv(empty, [sys.executable, '-m', 'consolecrypt_runner.dependency_worker'],
                                   extra_readonly=mounts, dependency_download=True)
            result = run_bounded(argv, timeout=_timeout, grace=0, cancel=cancel, output_limit=0, on_chunk=receive)
            require(result.exit_code == 0 and not result.cancelled and not result.timed_out and not cancel.is_set(),
                    'dependency_download_failed', 'Dependency download failed or was cancelled')
            require(raw and hashlib.sha256(raw).hexdigest() == record['source']['sha256'],
                    'dependency_archive_digest', 'Downloaded bytes differ from the selected archive SHA-256')
            return bytes(raw)
    finally:
        DOWNLOAD_GATE.release()


class DependencyPreparations:
    def __init__(self, revisions, root):
        self.revisions = revisions
        self.root = Path(root); _private_directory(self.root)
        self.previews = RevisionRepository(self.root/'previews', max_revisions=8, max_bytes=512*1024*1024)
        self.lock = threading.RLock()

    def request(self, operation, body, cancel):
        # Freeze the private-channel caller's request before blocking I/O.
        body = decode(encode(body))
        with self.lock:
            if operation == 'dependency_prepare':
                require(set(body) == {'revision_id','revision_digest','lock_sha256','selection','strip_components','network_origin'},
                        'dependency_fields', 'Invalid explicit dependency preparation request')
                return self.prepare(body, cancel)
            if operation == 'dependency_commit':
                require(set(body) == {'preparation_id','review_digest','revision_id','revision_digest','lock_sha256'},
                        'dependency_fields', 'Invalid reviewed dependency commit request')
                return self.commit(body, cancel)
            if operation == 'dependency_recover':
                require(set(body) == {'preparation_id','review_digest','revision_id','revision_digest','lock_sha256','candidate_revision_id'},
                        'dependency_fields', 'Invalid explicit dependency recovery request')
                return self.recover(body, cancel)
            if operation in ('dependency_archive_info', 'dependency_archive_chunk'):
                return self.archive_transfer(operation, body, cancel)
            if operation == 'dependency_files':
                require(set(body) == {'preparation_id','offset','limit'} and type(body['offset']) is int and body['offset'] >= 0
                        and type(body['limit']) is int and 1 <= body['limit'] <= 100,
                        'dependency_fields', 'Invalid dependency review page')
                _, snapshot = self.previews.read(body['preparation_id'])
                return {'entries':list(snapshot.entries[body['offset']:body['offset']+body['limit']]), 'total':len(snapshot.entries)}
            if operation == 'dependency_chunk':
                require(set(body) == {'preparation_id','path','offset','limit'} and type(body['offset']) is int and body['offset'] >= 0
                        and type(body['limit']) is int and 1 <= body['limit'] <= 128*1024,
                        'dependency_fields', 'Invalid dependency review chunk')
                root, snapshot = self.previews.read(body['preparation_id'])
                # All entries are in private helper-owned snapshots, not user paths.
                path = relative_path(body['path']).as_posix()
                entry = next((item for item in snapshot.files if item['path'] == path), None)
                require(entry is not None and body['offset'] <= entry['bytes'], 'dependency_file', 'Invalid review file or offset')
                with (root/path).open('rb') as source:
                    source.seek(body['offset']); raw = source.read(body['limit'])
                return {'review_digest':snapshot.digest,'path':path,'offset':body['offset'],
                        'data':base64.b64encode(raw).decode(), 'eof':body['offset']+len(raw)==entry['bytes']}
            if operation == 'dependency_status':
                require(set(body) == {'preparation_id'}, 'dependency_fields', 'Invalid dependency status query')
                uuid_field(body['preparation_id'])
                root = self.previews.root/body['preparation_id']
                require(root.is_dir(), 'dependency_missing', 'Dependency preparation does not exist')
                if (root/MARKER).exists():
                    pending=[]
                    for item in self.revisions.root.iterdir():
                        marker=item/'dependency-admission-pending.json'
                        if marker.exists() and read_json(marker).get('preparation_id') == body['preparation_id']:
                            pending.append(item.name)
                    if (root/'committed.json').exists():
                        pending.append(read_json(root/'committed.json')['revision_id'])
                    return {'state':'recovery_required','candidate_revision_ids':sorted(set(pending))}
                if (root/'committed.json').exists():
                    return {'state':'committed', 'revision':read_json(root/'committed.json')}
                return {'state':'review_required'}
            require(False, 'unsupported_operation', 'Private dependency operation is unavailable')

    def archive_transfer(self, operation, body, cancel):
        fields = {'preparation_id', 'review_digest'}
        if operation == 'dependency_archive_chunk':
            fields |= {'archive_sha256', 'offset', 'limit'}
        require(set(body) == fields, 'dependency_fields', 'Invalid archive transfer request')
        uuid_field(body['preparation_id'])
        require(not cancel.is_set(), 'cancelled', 'Dependency transfer was cancelled')
        directory = self.previews.root/body['preparation_id']
        require(directory.is_dir(), 'dependency_missing', 'Dependency preparation does not exist')
        _private_directory(directory)
        metadata = read_json(directory/'preparation.json')
        require(body['review_digest'] == metadata['review_digest'], 'dependency_review', 'Archive candidate changed')
        # A retained archive is helper-owned evidence, not a project file and not
        # a runnable revision. The Native side independently hashes/parses bytes.
        witness = metadata.get('archive')
        require(isinstance(witness, dict) and set(witness) == {'bytes', 'sha256'}
                and type(witness['bytes']) is int and 0 < witness['bytes'] <= MAX_ARCHIVE_BYTES
                and witness['sha256'] == metadata['record']['source']['sha256'],
                'dependency_archive', 'Preparation has no exact retained archive')
        result = {'version':1, 'preparation_id':body['preparation_id'], 'review_digest':metadata['review_digest'],
                  'archive_sha256':witness['sha256'], 'archive_bytes':witness['bytes']}
        fd = os.open(directory/ARCHIVE_FILE, os.O_RDONLY | os.O_NOFOLLOW | os.O_CLOEXEC)
        with os.fdopen(fd, 'rb') as source:
            info = os.fstat(source.fileno())
            require(stat.S_ISREG(info.st_mode) and info.st_uid == os.getuid() and info.st_nlink == 1
                    and info.st_mode & 0o077 == 0 and info.st_size == witness['bytes'],
                    'dependency_archive', 'Retained archive is missing or changed')
            if operation == 'dependency_archive_info':
                return result
            require(body['archive_sha256'] == witness['sha256'] and type(body['offset']) is int
                    and 0 <= body['offset'] < witness['bytes'] and type(body['limit']) is int
                    and 1 <= body['limit'] <= 128*1024, 'dependency_fields', 'Invalid archive chunk bounds')
            source.seek(body['offset']); raw = source.read(min(body['limit'], witness['bytes']-body['offset']))
            require(raw and not cancel.is_set(), 'cancelled', 'Dependency transfer was cancelled')
            return dict(result, offset=body['offset'], data=base64.b64encode(raw).decode('ascii'),
                        eof=body['offset']+len(raw) == witness['bytes'])

    def base(self, body):
        root, snapshot = self.revisions.read(body['revision_id'])
        require(snapshot.digest == body['revision_digest'], 'dependency_revision', 'Project revision changed')
        verify_snapshot(root, snapshot)
        entries = {entry['path']:entry for entry in snapshot.entries}
        # Portable case aliases and a reserved receipt are not overwritten.
        aliases = {path.casefold():path for path in entries}
        require(LOCK_FILE.casefold() not in aliases or aliases[LOCK_FILE.casefold()] == LOCK_FILE,
                'dependency_lock', 'Dependency lock path has a portable case collision')
        entry = entries.get(LOCK_FILE)
        require((entry['sha256'] if entry and entry['type'] == 'file' else None) == body['lock_sha256'],
                'dependency_lock', 'Dependency lock changed')
        records = parse_lock((root/LOCK_FILE).read_bytes()) if entry else []
        check_locked_dependencies(root,snapshot.to_json())
        return root, snapshot, entries, records

    def prepare(self, body, cancel):
        record = selection_record(body['selection'])
        require(body['network_origin'] == source_origin(record), 'dependency_network', 'Review the exact HTTPS download origin')
        require(type(body['strip_components']) is int and body['strip_components'] in (0,1),
                'dependency_archive_layout', 'Select an explicit archive layout')
        with self.revisions.lock:
            base, original, entries, records = self.base(body)
            target = dependency_target(record)
            target_parts = target.split('/')
            for index in range(1, len(target_parts)+1):
                path = '/'.join(target_parts[:index])
                for name, entry in entries.items():
                    if name.casefold() == path.casefold():
                        require(index < len(target_parts) and name == path and entry['type'] == 'directory',
                                'dependency_conflict', 'Dependency target already exists or collides')
            require(not any(dependency_target(item) == target for item in records), 'dependency_conflict', 'Dependency is already locked')
            require(not any(path.casefold() == RECEIPTS.casefold() for path in entries),
                    'dependency_receipt_conflict', 'Existing provenance requires the future merge workflow')
            require(len(records) < 32, 'dependency_schema', 'Dependency lock count limit reached')
        # Immutable base remains bound by digest; no project network exposure.
        raw = capture_archive(self.root, record, cancel)
        selection = {key:value for key,value in record.items() if key != 'tree_digest'}
        with preview_dependency(raw, selection, self.root, strip_components=body['strip_components']) as (tree, tree_snapshot, receipt):
            require(not cancel.is_set(), 'cancelled', 'Dependency preparation was cancelled')
            record['tree_digest'] = tree_snapshot.digest
            provenance = dict(receipt, kind=record['kind'], name=record['name'], dependency_version=record['version'],
                              verification='helper_https_sha256_v1', origin=source_origin(record))
            with tempfile.TemporaryDirectory(prefix='dependency-candidate-', dir=self.root) as temporary:
                candidate = Path(temporary)/'project'
                copied = create_snapshot(base, candidate)
                require(copied.digest == original.digest, 'dependency_revision', 'Project changed during preparation')
                destination = candidate/target
                destination.parent.mkdir(mode=0o700, parents=True, exist_ok=True)
                shutil.copytree(tree, destination)
                atomic_json(candidate/LOCK_FILE, {'version':1,'dependencies':records+[record]}, max_bytes=64*1024)
                atomic_json(candidate/RECEIPTS, {'version':1,'receipts':[provenance]}, max_bytes=64*1024)
                parse_lock((candidate/LOCK_FILE).read_bytes())
                with self.previews.lock:
                    captured = self.previews.capture(candidate)
                    metadata = {'version':1, 'base_revision_id':body['revision_id'], 'base_digest':original.digest,
                                'base_lock_sha256':body['lock_sha256'], 'review_digest':captured['digest'],
                                'record':record,'provenance':provenance,
                                'archive':{'bytes':len(raw),'sha256':hashlib.sha256(raw).hexdigest()}}
                    directory = self.previews.root/captured['revision_id']
                    # Exclusive create; retained evidence survives interruptions.
                    # Metadata is written last, so an incomplete archive never
                    # becomes transferable or a successful prepare response.
                    fd = os.open(directory/ARCHIVE_FILE, os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW, 0o600)
                    with os.fdopen(fd, 'wb') as output:
                        output.write(raw); output.flush(); os.fsync(output.fileno())
                    atomic_json(directory/'preparation.json', metadata)
                return {'preparation_id':captured['revision_id'], 'review_digest':captured['digest'],
                        'file_count':captured['file_count'],'entry_count':captured['entry_count'],'bytes':captured['bytes'],
                        'record':record,'provenance':provenance}

    def commit(self, body, cancel):
        uuid_field(body['preparation_id'])
        directory = self.previews.root/body['preparation_id']
        require(not (directory/MARKER).exists(), 'dependency_recovery', 'Interrupted publication needs explicit recovery review')
        require(not (directory/'committed.json').exists(), 'dependency_committed', 'Preparation was already committed')
        metadata = read_json(directory/'preparation.json')
        require(body['review_digest'] == metadata['review_digest'] and body['revision_id'] == metadata['base_revision_id']
                and body['revision_digest'] == metadata['base_digest'] and body['lock_sha256'] == metadata['base_lock_sha256'],
                'dependency_review', 'Reviewed preparation or base differs from this commit')
        with self.revisions.lock:
            self.base(body)
            root, snapshot = self.previews.read(body['preparation_id'])
            require(snapshot.digest == body['review_digest'], 'dependency_review', 'Review snapshot changed')
            verify_snapshot(root, snapshot)
            require(not cancel.is_set(), 'cancelled', 'Dependency publication was cancelled')
            # Durable intent before multi-file capture. Never guess/roll back an
            # interrupted state. Existing user trees and locks are never written.
            atomic_json(directory/MARKER, {'version':1,'review_digest':snapshot.digest})
            result = self.revisions.capture(root, expected_digest=snapshot.digest,
                      _admission_marker={'version':1,'preparation_id':body['preparation_id'],'review_digest':snapshot.digest})
            atomic_json(directory/'committed.json', result)
            admitted = self.revisions.root/result['revision_id']
            (admitted/'dependency-admission-pending.json').unlink()
            fd = os.open(admitted, os.O_RDONLY|os.O_DIRECTORY|os.O_NOFOLLOW)
            try: os.fsync(fd)
            finally: os.close(fd)
            (directory/MARKER).unlink()
            fd = os.open(directory, os.O_RDONLY|os.O_DIRECTORY|os.O_NOFOLLOW)
            try: os.fsync(fd)
            finally: os.close(fd)
            return result

    def recover(self, body, cancel):
        """Explicit private control action. Never deletes tree/lock or guesses.

        Currently resumes an exact retained pending candidate. A crash before
        a complete candidate snapshot is available needs operator inspection.
        """
        uuid_field(body['preparation_id']); uuid_field(body['candidate_revision_id'])
        directory=self.previews.root/body['preparation_id']
        require((directory/MARKER).exists(), 'dependency_recovery', 'No interrupted preparation to recover')
        metadata=read_json(directory/'preparation.json')
        require(body['review_digest']==metadata['review_digest'] and body['revision_id']==metadata['base_revision_id']
                and body['revision_digest']==metadata['base_digest'] and body['lock_sha256']==metadata['base_lock_sha256'],
                'dependency_review', 'Recovery selection differs from the reviewed preparation')
        with self.revisions.lock:
            self.base(body)
            candidate=self.revisions.root/body['candidate_revision_id']
            pending=candidate/'dependency-admission-pending.json'
            if pending.exists():
                marker=read_json(pending)
                require(marker=={'version':1,'preparation_id':body['preparation_id'],'review_digest':body['review_digest']},
                        'dependency_recovery', 'Recovery candidate differs from this preparation')
            else:
                require((directory/'committed.json').exists() and
                        read_json(directory/'committed.json').get('revision_id')==body['candidate_revision_id'],
                        'dependency_recovery', 'Recovery has no exact retained publication witness')
            raw=read_json(candidate/'snapshot.json',max_bytes=32*1024*1024)
            snapshot=ProjectSnapshot(raw['digest'],tuple(raw['entries']),raw['bytes'])
            require(snapshot.digest==body['review_digest'], 'dependency_review', 'Recovery snapshot digest differs')
            verify_snapshot(candidate/'project',snapshot)
            require(not cancel.is_set(), 'cancelled', 'Dependency recovery was cancelled')
            result={'revision_id':body['candidate_revision_id'],'digest':snapshot.digest,'bytes':snapshot.bytes,
                    'file_count':len(snapshot.files),'entry_count':len(snapshot.entries)}
            if (directory/'committed.json').exists():
                require(read_json(directory/'committed.json')==result, 'dependency_recovery', 'Recovery receipt differs')
            atomic_json(directory/'committed.json',result)
            if pending.exists():pending.unlink()
            fd=os.open(candidate,os.O_RDONLY|os.O_DIRECTORY|os.O_NOFOLLOW)
            try:os.fsync(fd)
            finally:os.close(fd)
            (directory/MARKER).unlink()
            fd=os.open(directory,os.O_RDONLY|os.O_DIRECTORY|os.O_NOFOLLOW)
            try:os.fsync(fd)
            finally:os.close(fd)
            return result
