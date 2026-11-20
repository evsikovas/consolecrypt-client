"""Visible static pattern expansion. Unsupported Ansible syntax fails closed."""
from __future__ import annotations

from fnmatch import fnmatchcase
import re

from .errors import require
from .inventory import StaticInventory


def resolve_pattern(inventory: StaticInventory, pattern: str, *, limit: str | None = None):
    inventory.validate_groups()
    all_hosts = set(inventory.hosts)
    cache = {}

    def group_hosts(name):
        if name == 'all':
            return all_hosts
        if name in cache:
            return cache[name]
        group = inventory.groups[name]
        result = set(group['hosts'])
        for child in group['children']:
            result.update(group_hosts(child))
        cache[name] = result
        return result

    grouped = set()
    for name in inventory.groups:
        if name not in ('all', 'ungrouped'):
            grouped.update(group_hosts(name))
    cache['ungrouped'] = all_hosts - grouped

    def expand(value):
        require(isinstance(value, str) and 0 < len(value) <= 4096 and
                not any(c.isspace() or ord(c) < 32 for c in value), 'inventory_pattern', 'Invalid host pattern')
        tokens = re.split('[:,]', value)
        require(0 < len(tokens) <= 256 and all(tokens), 'inventory_pattern', 'Empty or excessive pattern terms')
        include, intersect, exclude = [], [], []
        for token in tokens:
            modifier = token[0] if token[0] in '&!' else ''
            atom = token[1:] if modifier else token
            require(0 < len(atom) <= 256 and re.fullmatch(r'[A-Za-z0-9_.*?-]+', atom),
                    'inventory_pattern_unsupported', 'Only aliases, groups, globs, union, intersection and exclusion are supported')
            selected = {host for host in all_hosts if fnmatchcase(host, atom)}
            for group in set(inventory.groups) | {'all', 'ungrouped'}:
                if fnmatchcase(group, atom):
                    selected.update(group_hosts(group))
            (intersect if modifier == '&' else exclude if modifier == '!' else include).append(selected)
        result = set().union(*include) if include else set(all_hosts)
        for selected in intersect:
            result.intersection_update(selected)
        for selected in exclude:
            result.difference_update(selected)
        require(result <= all_hosts, 'inventory_group', 'Group references an undefined host')
        return result

    selected = expand(pattern)
    if limit is not None:
        selected.intersection_update(expand(limit))
    return [inventory.hosts[alias] for alias in sorted(selected)]
