"""Bounded file transfer from the native client; deliberately no tar extraction."""
from __future__ import annotations

import base64
from dataclasses import dataclass, field
import hashlib
import os
from pathlib import Path
import re
import shutil
import threading
import time
import uuid

from .errors import require
from .protocol import uuid_field
from .snapshot import MAX_BYTES, MAX_FILES, create_snapshot, relative_path
from .store import _private_directory, atomic_json
from .revisions import RevisionRepository


@dataclass
class Upload:
    root: Path
    expected: str
    created: float
    entries: dict = field(default_factory=dict)
    bytes: int = 0
    current: dict | None = None


class UploadManager:
    def __init__(self, root: Path, revisions: Path, *, repository=None):
        self.root, self.revisions = root, revisions
        _private_directory(root)
        self.uploads = {}
        self.lock = threading.Lock()
        self.repository = repository or RevisionRepository(revisions)

    def close(self):
        with self.lock:
            for upload in self.uploads.values():
                shutil.rmtree(upload.root)
            self.uploads.clear()

    def request(self, operation, body):
        with self.lock:
            for upload_id in list(self.uploads):
                upload = self.uploads[upload_id]
                if time.monotonic() - upload.created > 600:
                    shutil.rmtree(upload.root)
                    del self.uploads[upload_id]
            if operation == 'upload_begin':
                require(set(body) == {'digest'} and isinstance(body['digest'], str) and
                        re.fullmatch('[0-9a-f]{64}', body['digest']), 'upload_digest', 'Invalid snapshot digest')
                require(len(self.uploads) < 2, 'upload_limit', 'Too many incomplete project transfers')
                upload_id = str(uuid.uuid4())
                root = self.root / upload_id
                root.mkdir(mode=0o700)
                (root / 'project').mkdir(mode=0o700)
                self.uploads[upload_id] = Upload(root, body['digest'], time.monotonic())
                return {'upload_id': upload_id}
            upload_id = uuid_field(body.get('upload_id'), 'upload ID')
            upload = self.uploads.get(upload_id)
            require(upload is not None, 'upload_missing', 'Project transfer expired or does not exist')
            if operation == 'upload_abort':
                require(set(body) == {'upload_id'}, 'upload_fields', 'Invalid transfer abort')
                shutil.rmtree(upload.root)
                del self.uploads[upload_id]
                return {'aborted': True}
            if operation == 'upload_entry':
                require(upload.current is None, 'upload_incomplete', 'Previous file is incomplete')
                entry = body.get('entry')
                require(set(body) == {'upload_id', 'entry'} and isinstance(entry, dict), 'upload_entry', 'Invalid transfer entry')
                path = str(relative_path(entry.get('path')))
                require(path == entry['path'] and path not in upload.entries and len(upload.entries) < 20000,
                        'upload_entry', 'Duplicate, noncanonical or excessive transfer entry')
                if '/' in path:
                    parent = path.rsplit('/', 1)[0]
                    require(upload.entries.get(parent, {}).get('type') == 'directory', 'upload_parent', 'Parent directory must precede its children')
                target = upload.root / 'project' / path
                if entry.get('type') == 'directory':
                    require(set(entry) == {'path', 'type'}, 'upload_entry', 'Invalid directory fields')
                    target.mkdir(mode=0o700)
                else:
                    require(set(entry) == {'path', 'type', 'mode', 'bytes', 'sha256'} and entry['type'] == 'file',
                            'upload_entry', 'Invalid file fields')
                    require(type(entry['mode']) is int and entry['mode'] in (0o600, 0o700) and
                            type(entry['bytes']) is int and 0 <= entry['bytes'] <= MAX_BYTES - upload.bytes and
                            isinstance(entry['sha256'], str) and re.fullmatch('[0-9a-f]{64}', entry['sha256']),
                            'upload_file', 'Invalid file size, mode or hash')
                    require(sum(item['type'] == 'file' for item in upload.entries.values()) < MAX_FILES,
                            'upload_limit', 'Too many project files')
                    fd = os.open(target, os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW, entry['mode'])
                    os.close(fd)
                    upload.bytes += entry['bytes']
                    upload.current = {'path': path, 'offset': 0, 'digest': hashlib.sha256(), 'entry': dict(entry)}
                    if entry['bytes'] == 0:
                        require(entry['sha256'] == hashlib.sha256(b'').hexdigest(), 'upload_hash', 'Empty file hash mismatch')
                        upload.current = None
                upload.entries[path] = dict(entry)
                return {'accepted': True}
            if operation == 'upload_chunk':
                current = upload.current
                require(set(body) == {'upload_id', 'offset', 'data'} and current is not None and
                        type(body['offset']) is int and body['offset'] == current['offset'] and
                        isinstance(body['data'], str) and 0 < len(body['data']) <= 175000,
                        'upload_chunk', 'Invalid or out-of-order project chunk')
                try:
                    chunk = base64.b64decode(body['data'], validate=True)
                except (ValueError, TypeError):
                    require(False, 'upload_chunk', 'Invalid base64 project chunk')
                require(0 < len(chunk) <= 128 * 1024 and current['offset'] + len(chunk) <= current['entry']['bytes'],
                        'upload_chunk', 'Project chunk exceeds declared file size')
                fd = os.open(upload.root / 'project' / current['path'], os.O_WRONLY | os.O_APPEND | os.O_NOFOLLOW)
                with os.fdopen(fd, 'ab') as output:
                    output.write(chunk)
                current['digest'].update(chunk)
                current['offset'] += len(chunk)
                if current['offset'] == current['entry']['bytes']:
                    require(current['digest'].hexdigest() == current['entry']['sha256'], 'upload_hash', 'Transferred file hash mismatch')
                    upload.current = None
                return {'offset': current['offset']}
            require(operation == 'upload_commit' and set(body) == {'upload_id'}, 'upload_fields', 'Invalid transfer operation')
            require(upload.current is None, 'upload_incomplete', 'A transferred file is incomplete')
            result = self.repository.capture(upload.root / 'project', expected_digest=upload.expected)
            shutil.rmtree(upload.root)
            del self.uploads[upload_id]
            return result
