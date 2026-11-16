"""Untrusted, informational declarations from already parsed isolated Ansible data.

Never resolves variables, imports, roles, plugins, or effective privileges.
No YAML parser is introduced; caller passes existing list-hosts Play.get_ds().
"""
import json

PREFIX = 'CC_DECLARED_PRIVILEGES_V1:'
MAX_BYTES = 32768
MAX_RECORDS = 64
MAX_NODES = 4096
MAX_DEPTH = 16
AREAS = {'play', 'pre_tasks', 'tasks', 'post_tasks', 'handlers'}
BRANCHES = {'block', 'rescue', 'always'}
METHODS = {'sudo', 'su', 'doas', 'runas', 'pfexec', 'ksu', 'machinectl', 'pbrun', 'pmrun', 'dzdo'}
BECOME = {'absent', 'enabled', 'disabled', 'dynamic', 'unknown'}
VALUES = {'absent', 'literal_hidden', 'dynamic', 'unknown'}
UNRESOLVED = {'runtime_overrides', 'roles', 'task_includes', 'shape'}
KEYS = {'version', 'source', 'records', 'unresolved', 'truncated'}
ROW_KEYS = {'play', 'area', 'indices', 'branches', 'become', 'user', 'method', 'knownMethod'}


def _dynamic(value):
    return isinstance(value, str) and len(value) <= 256 and any(token in value for token in ('{{', '{%', '{#'))


def _value(node, name):
    if name not in node:
        return 'absent'
    value = node[name]
    if isinstance(value, str) and len(value) > 256:
        return 'unknown'
    if _dynamic(value):
        return 'dynamic'
    return 'literal_hidden' if isinstance(value, str) and value else 'unknown'


def summarize_entries(entries):
    records = []
    unresolved = {'runtime_overrides'}
    nodes = 0
    truncated = False

    def visit(node, play, area, indices, branches):
        nonlocal nodes, truncated
        nodes += 1
        if nodes > MAX_NODES or len(indices) > MAX_DEPTH:
            truncated = True
            return
        if not isinstance(node, dict):
            unresolved.add('shape')
            return
        if 'roles' in node:
            unresolved.add('roles')
        for key_index, key in enumerate(node):
            if key_index >= 128:
                truncated = True
                unresolved.add('shape')
                break
            if not isinstance(key, str) or len(key) > 128:
                unresolved.add('shape')
            elif key.rsplit('.', 1)[-1] in {'include_tasks', 'import_tasks', 'include_role', 'import_role', 'include'}:
                unresolved.add('task_includes')
        if any(key in node for key in ('become', 'become_user', 'become_method')):
            if len(records) >= MAX_RECORDS:
                truncated = True
            else:
                become = ('absent' if 'become' not in node else 'enabled' if node['become'] is True else
                          'disabled' if node['become'] is False else 'dynamic' if _dynamic(node['become']) else 'unknown')
                method = _value(node, 'become_method')
                raw_method = node.get('become_method')
                records.append({'play': play, 'area': area, 'indices': list(indices), 'branches': list(branches),
                                'become': become, 'user': _value(node, 'become_user'), 'method': method,
                                'knownMethod': next((method for method in METHODS if isinstance(raw_method, str) and len(raw_method) <= 256 and raw_method == method), None)})
        containers = ('pre_tasks', 'tasks', 'post_tasks', 'handlers') if area == 'play' else ('block', 'rescue', 'always')
        for branch in containers:
            if branch not in node:
                continue
            children = node[branch]
            if not isinstance(children, list):
                unresolved.add('shape')
                continue
            for index, child in enumerate(children, 1):
                if nodes >= MAX_NODES:
                    truncated = True
                    break
                visit(child, play, branch if area == 'play' else area,
                      [index] if area == 'play' else [*indices, index],
                      [] if area == 'play' else [*branches, branch])

    play_number = 0
    if not isinstance(entries, list):
        return None
    for entry in entries:
        if not isinstance(entry, dict) or not isinstance(entry.get('plays'), list):
            return None
        for play in entry['plays']:
            play_number += 1
            if nodes >= MAX_NODES:
                truncated = True
                break
            try:
                node = play.get_ds()
            except Exception:
                unresolved.add('shape')
                continue
            visit(node, play_number, 'play', [], [])
    summary = {'version': 1, 'source': 'ansible_declared_v1', 'records': records,
               'unresolved': sorted(unresolved), 'truncated': truncated}
    return summary if valid_summary(summary) else None


def valid_summary(value):
    # Private literals and unknown schema never pass as display metadata.
    if not isinstance(value, dict) or set(value) != KEYS or type(value['version']) is not int or value['version'] != 1:
        return False
    if value['source'] != 'ansible_declared_v1' or type(value['truncated']) is not bool:
        return False
    rows, unresolved = value['records'], value['unresolved']
    if (not isinstance(rows, list) or len(rows) > MAX_RECORDS or not isinstance(unresolved, list) or
            not 1 <= len(unresolved) <= len(UNRESOLVED) or any(type(item) is not str or item not in UNRESOLVED for item in unresolved) or
            len(set(unresolved)) != len(unresolved) or 'runtime_overrides' not in unresolved):
        return False
    for row in rows:
        if not isinstance(row, dict) or set(row) != ROW_KEYS:
            return False
        if (type(row['play']) is not int or not 1 <= row['play'] <= MAX_NODES or
                type(row['area']) is not str or row['area'] not in AREAS or
                type(row['become']) is not str or row['become'] not in BECOME or
                type(row['user']) is not str or row['user'] not in VALUES or
                type(row['method']) is not str or row['method'] not in VALUES or
                row['knownMethod'] is not None and (type(row['knownMethod']) is not str or row['knownMethod'] not in METHODS)):
            return False
        indices, branches = row['indices'], row['branches']
        if (not isinstance(indices, list) or len(indices) > MAX_DEPTH or
                any(type(index) is not int or not 1 <= index <= MAX_NODES for index in indices) or
                not isinstance(branches, list) or any(type(branch) is not str or branch not in BRANCHES for branch in branches) or
                len(branches) != max(0, len(indices) - 1) or
                (row['area'] == 'play') != (len(indices) == 0) or
                row['knownMethod'] is not None and row['method'] != 'literal_hidden'):
            return False
    return len(json.dumps(value, separators=(',', ':'), ensure_ascii=True).encode()) <= MAX_BYTES


def emit_summary(summary):
    if valid_summary(summary):
        print(PREFIX + json.dumps(summary, separators=(',', ':'), ensure_ascii=True), flush=True)


def extract_summary(output, *, completed):
    # stdout remains untrusted guest diagnostics. Closed bounded metadata only;
    # never controls valid/check outcomes, permissions, quota or approval.
    candidates = []
    clean = []
    for line in output.splitlines(keepends=True):
        if line.startswith(PREFIX):
            candidates.append(line[len(PREFIX):].strip())
        else:
            clean.append(line)
    if not completed or len(candidates) != 1 or len(candidates[0].encode()) > MAX_BYTES:
        return None, ''.join(clean)
    try:
        def unique(pairs):
            result = {}
            for key, value in pairs:
                if key in result:
                    raise ValueError('duplicate metadata key')
                result[key] = value
            return result
        summary = json.loads(candidates[0], object_pairs_hook=unique)
        return (summary if valid_summary(summary) else None), ''.join(clean)
    except (ValueError, TypeError, RecursionError):
        return None, ''.join(clean)
