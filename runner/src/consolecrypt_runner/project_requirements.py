"""Reviewed project requirements; checks only, never installs or resolves secrets.

Separate from manually maintained inputs/lock files: both constraints apply.
Validation executes this code only inside the project validation namespace.
"""
import os
import re
import stat

from .dependencies import LOCK_FILE, IDENTIFIER, parse_lock
from .errors import require
from .project_contracts import contract_path
from .protocol import decode
from .snapshot import relative_path

REQUIREMENTS_FILE = '.consolecrypt-requirements.json'


def read_contract(project, name, limit):
    path = contract_path(project, name)
    try:
        fd = os.open(path, os.O_RDONLY | os.O_CLOEXEC | os.O_NOFOLLOW | os.O_NONBLOCK)
    except FileNotFoundError:
        return None
    with os.fdopen(fd, 'rb') as source:
        info = os.fstat(source.fileno())
        require(stat.S_ISREG(info.st_mode) and info.st_nlink == 1 and info.st_size <= limit,
                'project_requirements', 'Project contract must be a bounded regular file')
        return source.read(limit + 1)


def dependency_key(item):
    require(isinstance(item, dict) and set(item) == {'kind', 'name', 'version'},
            'project_requirements', 'Unsupported required dependency record')
    kind, name, version = item['kind'], item['name'], item['version']
    require(isinstance(name, str) and isinstance(version, str) and
            re.fullmatch(r'[A-Za-z0-9][A-Za-z0-9._+-]{0,79}', version) is not None and version.lower() != 'latest',
            'project_requirements', 'Required dependencies need exact versions')
    parts = name.split('.')
    require((kind == 'role' and IDENTIFIER.fullmatch(name) is not None) or
            (kind == 'collection' and len(parts) == 2 and name != 'ansible.builtin' and
             all(IDENTIFIER.fullmatch(part) is not None for part in parts)),
            'project_requirements', 'Required dependency name is invalid')
    return kind, name


def parse_requirements(raw):
    value = decode(raw, max_bytes=64 * 1024)
    require(isinstance(value, dict) and set(value) == {'version', 'playbooks'} and
            type(value['version']) is int and value['version'] == 1 and
            isinstance(value['playbooks'], dict) and len(value['playbooks']) <= 128,
            'project_requirements', 'Unsupported reviewed project requirements')
    for playbook, spec in value['playbooks'].items():
        relative_path(playbook)
        require(isinstance(spec, dict) and set(spec) == {'required_extra_vars', 'required_dependencies'},
                'project_requirements', 'Unsupported playbook requirements')
        names, dependencies = spec['required_extra_vars'], spec['required_dependencies']
        require(isinstance(names, list) and len(names) <= 64 and
                all(isinstance(name, str) and re.fullmatch(r'[A-Za-z_][A-Za-z0-9_]{0,127}', name) is not None and
                    not name.startswith(('ansible_', 'consolecrypt_')) for name in names) and
                len(set(names)) == len(names), 'project_requirements', 'Required variable names are invalid')
        require(isinstance(dependencies, list) and len(dependencies) <= 32,
                'project_requirements', 'Too many required dependencies')
        keys = [dependency_key(item) for item in dependencies]
        require(len(set(keys)) == len(keys), 'project_requirements', 'Duplicate required dependency')
    return value['playbooks']


def check_project_requirements(project, playbook, extra_vars):
    selected = str(relative_path(playbook))
    raw = read_contract(project, REQUIREMENTS_FILE, 64 * 1024)
    if raw is None:
        return 0
    playbooks = parse_requirements(raw)
    spec = playbooks.get(selected)
    if spec is None:
        return 0
    names = spec['required_extra_vars']
    missing = [name for name in names if name not in extra_vars or extra_vars[name] is None or
               isinstance(extra_vars[name], str) and not extra_vars[name].strip()]
    require(not missing, 'required_inputs', 'Missing required execution inputs: ' + ', '.join(missing))
    required = spec['required_dependencies']
    if required:
        locked = read_contract(project, LOCK_FILE, 64 * 1024)
        require(locked is not None, 'required_dependencies', 'Required dependencies have no reviewed dependency lock')
        # The worker verifies ALL installed lock trees before this membership
        # check. A name/version declaration alone cannot attest downloaded code.
        installed = {dependency_key({k: item[k] for k in ('kind', 'name', 'version')}): item['version']
                     for item in parse_lock(locked)}
        require(all(installed.get(dependency_key(item)) == item['version'] for item in required),
                'required_dependencies', 'Required dependency is missing or its pinned version differs')
    return len(names) + len(required)
