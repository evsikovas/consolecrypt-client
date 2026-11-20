"""Private snapshots with an aggregate content budget and atomic admission."""
from pathlib import Path
import shutil
import threading
import uuid

from .errors import require
from .protocol import uuid_field
from .snapshot import MAX_BYTES, ProjectSnapshot, create_snapshot
from .store import _private_directory, atomic_json, read_json


class RevisionRepository:
    def __init__(self, root: Path, *, max_bytes=1024 * 1024 * 1024, max_revisions=64):
        self.root = root
        _private_directory(root)
        self.max_bytes, self.max_revisions = max_bytes, max_revisions
        self.lock = threading.RLock()

    def read(self, revision_id):
        uuid_field(revision_id, 'revision ID')
        root = self.root / revision_id
        require(root.is_dir(), 'revision_missing', 'Revision does not exist')
        _private_directory(root)
        require(not (root/'dependency-admission-pending.json').exists(), 'revision_pending',
                'Dependency revision admission requires explicit recovery review')
        metadata = read_json(root / 'snapshot.json', max_bytes=32 * 1024 * 1024)
        require(type(metadata.get('version')) is int and metadata['version'] == 1 and type(metadata.get('bytes')) is int and 0 <= metadata['bytes'] <= MAX_BYTES,
                'revision_metadata', 'Invalid revision metadata')
        return root / 'project', ProjectSnapshot(digest=metadata['digest'], entries=tuple(metadata['entries']), bytes=metadata['bytes'])

    def list(self):
        with self.lock:
            results = []
            for directory in self.root.iterdir():
                if (directory/'dependency-admission-pending.json').exists():
                    continue  # Not admitted; do not offer it to execution/review.
                root, snapshot = self.read(directory.name)
                results.append({'revision_id': directory.name, 'digest': snapshot.digest, 'bytes': snapshot.bytes,
                                'file_count': len(snapshot.files), 'entry_count': len(snapshot.entries)})
            return sorted(results, key=lambda item: item['revision_id'])

    def capture(self, source: Path, *, expected_digest=None, _admission_marker=None):
        with self.lock:
            require(not any((item/'dependency-admission-pending.json').exists() for item in self.root.iterdir()),
                    'revision_recovery', 'Recover interrupted dependency admission before importing another revision')
            known = self.list()
            used = sum(item['bytes'] for item in known)
            require(len(known) < self.max_revisions and used < self.max_bytes,
                    'revision_quota', 'Release unused revisions before importing another project')
            require(shutil.disk_usage(self.root).free >= 128 * 1024 * 1024,
                    'revision_disk', 'Insufficient reserved disk space for a project snapshot')
            revision_id = str(uuid.uuid4())
            destination = self.root / revision_id
            destination.mkdir(mode=0o700)
            try:
                if _admission_marker is not None:
                    atomic_json(destination/'dependency-admission-pending.json', _admission_marker)
                snapshot = create_snapshot(source, destination / 'project')
                require(used + snapshot.bytes <= self.max_bytes, 'revision_quota', 'Aggregate revision content quota exceeded')
                require(expected_digest is None or snapshot.digest == expected_digest,
                        'upload_revision', 'Transferred project differs from the native revision')
                atomic_json(destination / 'snapshot.json', snapshot.to_json(), max_bytes=32 * 1024 * 1024)
            except BaseException:
                shutil.rmtree(destination)
                raise
            return {'revision_id': revision_id, 'digest': snapshot.digest, 'file_count': len(snapshot.files),
                    'entry_count': len(snapshot.entries), 'bytes': snapshot.bytes}

    def release(self, revision_id, *, referenced):
        with self.lock:
            _, snapshot = self.read(revision_id)
            require(not referenced(revision_id, snapshot.digest), 'revision_in_use', 'A retained job references this revision')
            # Exact UUID child of an owned private root; no user source tree.
            shutil.rmtree(self.root / revision_id)
            return {'released': True}
