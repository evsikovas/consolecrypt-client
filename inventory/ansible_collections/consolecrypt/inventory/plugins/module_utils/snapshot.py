"""Bounded issued static DATA. No vault, credential, API or execution authority."""
from __future__ import annotations

import argparse
import ipaddress
import json
import os
import re
import stat
import sys
import uuid

MAX_BYTES = 256 * 1024
MAX_HOSTS = 32
MAX_GROUPS = 64
FORMAT = 'consolecrypt.inventory.snapshot'
NAME = re.compile(r'[A-Za-z_][A-Za-z0-9_-]{0,95}', re.ASCII)
USER = re.compile(r'[A-Za-z0-9_.@\\-]{1,128}', re.ASCII)
RESERVED = {'all', 'ungrouped', '_meta'}


class SnapshotError(ValueError):
    """A static code only. Never incorporate input paths, JSON or exception text."""


def require(condition, code='inventory_snapshot_invalid'):
    if not condition:
        raise SnapshotError(code)


def exact(value, keys):
    require(type(value) is dict and set(value) == set(keys))


def integer(value, minimum=0, maximum=(1 << 64) - 1):
    require(type(value) is int and minimum <= value <= maximum)


def identifier(value):
    require(type(value) is str and len(value) == 36)
    try:
        parsed = uuid.UUID(value)
    except ValueError:
        raise SnapshotError('inventory_snapshot_invalid') from None
    require(parsed.int != 0 and str(parsed) == value)
    return value


def text(value, maximum):
    require(type(value) is str and 0 < len(value.encode('utf-8')) <= maximum)
    require(not any(ord(char) < 32 or ord(char) == 127 for char in value))
    require(not any(marker in value for marker in ('{{', '{%', '{#', '}}', '%}', '#}')))
    return value


def group_name(value):
    text(value, 96)
    require(NAME.fullmatch(value) is not None and value not in RESERVED)
    return value


def address(value):
    text(value, 253)
    require('%' not in value)  # Scoped IPv6 needs separately supported zone semantics.
    try:
        ipaddress.ip_address(value)
        return value
    except ValueError:
        pass
    require(value.isascii())
    labels = value[:-1].split('.') if value.endswith('.') else value.split('.')
    require(all(0 < len(label) <= 63 and label[0].isalnum() and label[-1].isalnum()
                and all(char.isalnum() or char == '-' for char in label) for label in labels))
    return value


def username(value):
    text(value, 128)
    require(USER.fullmatch(value) is not None and not value.startswith('-'))
    return value


def unique_list(value, maximum, check):
    require(type(value) is list and len(value) <= maximum)
    result = [check(item) for item in value]
    require(len(set(result)) == len(result))
    return result


def alias(host_id):
    return 'cc_' + identifier(host_id).replace('-', '')


def validate_snapshot(value):
    exact(value, ('format', 'version', 'snapshotId', 'issuedAtUnixSeconds', 'origin', 'selection', 'inventory'))
    require(value['format'] == FORMAT and type(value['version']) is int and value['version'] == 1)
    identifier(value['snapshotId'])
    integer(value['issuedAtUnixSeconds'], 1, 253402300799)
    origin = value['origin']
    exact(origin, ('kind', 'projectId', 'projectRevision', 'workingVersion'))
    require(origin['kind'] == 'native')  # Descriptive provenance, never an access grant.
    identifier(origin['projectId'])
    integer(origin['projectRevision'], 1)
    integer(origin['workingVersion'])
    selection = value['selection']
    exact(selection, ('hostIds', 'groupNames'))
    host_ids = unique_list(selection['hostIds'], MAX_HOSTS, identifier)
    require(host_ids)
    groups = unique_list(selection['groupNames'], MAX_GROUPS, group_name)
    hosts = {alias(host_id) for host_id in host_ids}
    require(not hosts.intersection(groups))
    inventory = value['inventory']
    exact(inventory, ('all', 'ungrouped', '_meta', *groups))
    exact(inventory['_meta'], ('hostvars',))
    variables = inventory['_meta']['hostvars']
    exact(variables, hosts)
    for row in variables.values():
        exact(row, ('ansible_host', 'ansible_port', 'ansible_user'))
        address(row['ansible_host'])
        integer(row['ansible_port'], 1, 65535)
        username(row['ansible_user'])
    memberships = set()
    for name in ['all', 'ungrouped', *groups]:
        row = inventory[name]
        exact(row, ('hosts', 'children', 'vars'))
        require(type(row['vars']) is dict and not row['vars'])
        row_hosts = unique_list(row['hosts'], MAX_HOSTS, lambda item: text(item, 35))
        require(set(row_hosts) <= hosts)
        children = unique_list(row['children'], MAX_GROUPS, group_name)
        if name == 'all':
            require(set(row_hosts) == hosts and set(children) == set(groups))
        else:
            require(not children)
            if name != 'ungrouped':
                memberships.update(row_hosts)
    require(set(inventory['ungrouped']['hosts']) == hosts - memberships)
    return inventory


def _unique(pairs):
    result = {}
    for key, value in pairs:
        require(key not in result)
        result[key] = value
    return result


def parse_snapshot(raw):
    require(type(raw) is bytes and 0 < len(raw) <= MAX_BYTES, 'inventory_snapshot_size')
    try:
        value = json.loads(raw.decode('utf-8'), object_pairs_hook=_unique,
                           parse_constant=lambda unused: require(False))
        return validate_snapshot(value)
    except (UnicodeError, ValueError, TypeError, RecursionError, OverflowError):
        raise SnapshotError('inventory_snapshot_invalid') from None


def read_snapshot(path):
    # Linux/WSL control nodes: walk every component with held directory FDs.
    # Unsupported platforms fail explicitly instead of a racy symlink fallback.
    require(os.name == 'posix' and hasattr(os, 'O_NOFOLLOW') and os.open in os.supports_dir_fd,
            'inventory_snapshot_platform')
    require(type(path) is str and 0 < len(os.fsencode(path)) <= 4096
            and not any(ord(char) < 32 or ord(char) == 127 for char in path), 'inventory_snapshot_file')
    require('..' not in path.split('/'), 'inventory_snapshot_file')
    components = [part for part in path.split('/') if part not in ('', '.')]
    require(components, 'inventory_snapshot_file')
    directory = descriptor = None
    try:
        directory = os.open('/' if path.startswith('/') else '.', os.O_RDONLY | os.O_DIRECTORY | os.O_CLOEXEC)
        for component in components[:-1]:
            following = os.open(component, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW | os.O_CLOEXEC,
                                dir_fd=directory)
            os.close(directory)
            directory = following
        descriptor = os.open(components[-1], os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK | os.O_CLOEXEC,
                             dir_fd=directory)
        before = os.fstat(descriptor)
        require(stat.S_ISREG(before.st_mode), 'inventory_snapshot_file')
        require(0 < before.st_size <= MAX_BYTES, 'inventory_snapshot_size')
        chunks = []
        remaining = MAX_BYTES + 1
        while remaining:
            chunk = os.read(descriptor, remaining)
            if not chunk:
                break
            chunks.append(chunk)
            remaining -= len(chunk)
        raw = b''.join(chunks)
        after = os.fstat(descriptor)
        require((before.st_dev, before.st_ino, before.st_size, before.st_mtime_ns, before.st_ctime_ns)
                == (after.st_dev, after.st_ino, after.st_size, after.st_mtime_ns, after.st_ctime_ns)
                and len(raw) == before.st_size, 'inventory_snapshot_changed')
        return parse_snapshot(raw)
    except OSError:
        raise SnapshotError('inventory_snapshot_file') from None
    finally:
        if descriptor is not None:
            os.close(descriptor)
        if directory is not None:
            os.close(directory)


def main(argv=None):
    class BoundedParser(argparse.ArgumentParser):
        def error(self, unused):
            self.exit(2, 'inventory_snapshot_arguments\n')
    parser = BoundedParser(description='Read an explicitly issued static ConsoleCrypt inventory snapshot.')
    parser.add_argument('--snapshot', required=True)
    mode = parser.add_mutually_exclusive_group(required=True)
    mode.add_argument('--list', action='store_true')
    mode.add_argument('--host')
    arguments = parser.parse_args(argv)
    try:
        inventory = read_snapshot(arguments.snapshot)
        output = inventory if arguments.list else inventory['_meta']['hostvars'].get(arguments.host, {})
        sys.stdout.write(json.dumps(output, ensure_ascii=True, sort_keys=True, separators=(',', ':')) + '\n')
        return 0
    except SnapshotError as error:
        sys.stderr.write(str(error) + '\n')
        return 2


if __name__ == '__main__':
    raise SystemExit(main())
