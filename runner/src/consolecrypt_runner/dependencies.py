"""Check explicitly locked, already vendored dependencies. Never install code."""
import hashlib
import os
import re
import stat
from urllib.parse import urlsplit

from .errors import require
from .protocol import decode, encode
from .project_contracts import contract_path

LOCK_FILE = '.consolecrypt-dependencies.json'
IDENTIFIER = re.compile(r'[a-z][a-z0-9_]{0,63}')
DIGEST = re.compile(r'[0-9a-f]{64}')


def dependency_target(item):
    require(isinstance(item, dict) and set(item) == {'kind', 'name', 'version', 'source', 'tree_digest'},
            'dependency_schema', 'Unsupported locked dependency record')
    kind, name, version = item['kind'], item['name'], item['version']
    require(isinstance(name, str) and isinstance(version, str) and
            re.fullmatch(r'[A-Za-z0-9][A-Za-z0-9._+-]{0,79}', version) is not None and version.lower() != 'latest',
            'dependency_version', 'Use an explicit dependency name and version')
    if kind == 'role':
        require(IDENTIFIER.fullmatch(name) is not None, 'dependency_name', 'Invalid locked role name')
        target = 'roles/' + name
    else:
        parts = name.split('.')
        require(kind == 'collection' and len(parts) == 2 and all(IDENTIFIER.fullmatch(part) for part in parts)
                and name != 'ansible.builtin', 'dependency_name', 'Invalid locked collection name')
        target = 'collections/ansible_collections/' + '/'.join(parts)
    source = item['source']
    require(isinstance(source, dict) and set(source) == {'url', 'sha256'} and
            isinstance(source['url'], str) and 0 < len(source['url']) <= 2048 and
            not any(c.isspace() or ord(c) < 32 or c == '\\' for c in source['url']),
            'dependency_source', 'Use a public HTTPS artifact source without credentials')
    try:
        url = urlsplit(source['url'])
        valid = url.scheme == 'https' and url.hostname and url.username is None and url.password is None and \
            not url.query and not url.fragment and (url.port is None or 1 <= url.port <= 65535)
    except ValueError:
        valid = False
    require(valid and isinstance(source['sha256'], str) and DIGEST.fullmatch(source['sha256']) is not None and
            isinstance(item['tree_digest'], str) and DIGEST.fullmatch(item['tree_digest']) is not None,
            'dependency_source', 'Dependency source and installed tree must have pinned SHA-256 digests')
    return target


def parse_lock(raw):
    value = decode(raw, max_bytes=64 * 1024)
    require(isinstance(value, dict) and set(value) == {'version', 'dependencies'} and
            type(value['version']) is int and value['version'] == 1 and isinstance(value['dependencies'], list)
            and len(value['dependencies']) <= 32, 'dependency_schema', 'Unsupported dependency lock format or budget')
    targets = set()
    for item in value['dependencies']:
        target = dependency_target(item)
        require(target not in targets, 'dependency_duplicate', 'Duplicate locked dependency target')
        targets.add(target)
    return value['dependencies']


def snapshot_entries(snapshot):
    require(isinstance(snapshot, dict) and type(snapshot.get('version')) is int and snapshot['version'] == 1 and
            isinstance(snapshot.get('entries'), list) and len(snapshot['entries']) <= 20000,
            'dependency_snapshot', 'Verified project snapshot metadata is required')
    entries = snapshot['entries']
    require(snapshot.get('digest') == hashlib.sha256(encode({'version': 1, 'entries': entries})).hexdigest(),
            'dependency_snapshot', 'Project snapshot metadata changed')
    return entries


def subtree_digest(snapshot, target):
    """Project the parent-verified snapshot; no path from JSON is opened."""
    return _subtree_digest(snapshot_entries(snapshot), target)


def _subtree_digest(entries, target):
    require(any(entry.get('path') == target and entry.get('type') == 'directory' for entry in entries),
            'dependency_missing', 'Locked dependency directory is missing')
    prefix = target + '/'
    projected = [dict(entry, path=entry['path'][len(prefix):]) for entry in entries if entry['path'].startswith(prefix)]
    return hashlib.sha256(encode({'version': 1, 'entries': projected})).hexdigest(), projected


def check_locked_dependencies(project, snapshot=None):
    path = contract_path(project, LOCK_FILE)
    try:
        fd = os.open(path, os.O_RDONLY | os.O_CLOEXEC | os.O_NOFOLLOW | os.O_NONBLOCK)
    except FileNotFoundError:
        return 0  # Existing projects do not acquire a network/install side effect.
    with os.fdopen(fd, 'rb') as source:
        info = os.fstat(source.fileno())
        require(stat.S_ISREG(info.st_mode) and info.st_nlink == 1 and info.st_size <= 64 * 1024,
                'dependency_lock', 'Dependency lock must be a bounded regular file')
        dependencies = parse_lock(source.read(64 * 1024 + 1))
    entries = snapshot_entries(snapshot) if dependencies else []
    for item in dependencies:
        target = dependency_target(item)
        digest, tree = _subtree_digest(entries, target)
        require(digest == item['tree_digest'], 'dependency_changed',
                'Installed dependency differs from its pinned tree digest')
        files = {entry['path'] for entry in tree if entry['type'] == 'file'}
        required = ('tasks/main.yml', 'tasks/main.yaml') if item['kind'] == 'role' else ('galaxy.yml', 'MANIFEST.json')
        require(any(name in files for name in required), 'dependency_layout',
                'Locked dependency has no supported role or collection entry point')
    return len(dependencies)
