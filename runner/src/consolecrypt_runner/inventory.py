"""Static inventory only; no executable scripts, plugin imports or credentials."""
from __future__ import annotations

from dataclasses import dataclass, field, replace
import hashlib
import json
import re
import shlex

import yaml

from .errors import RunnerError, require
from .protocol import encode, validate_tree, uuid_field

MAX_INVENTORY_BYTES = 5 * 1024 * 1024
MAX_HOSTS = 10000
MAX_GROUPS = 1000
NAME = re.compile(r'^[A-Za-z_][A-Za-z0-9_-]{0,95}$')
CONNECTION_FIELDS = {'ansible_host', 'ansible_port', 'ansible_user'}
SAFE_ANSIBLE_VARS = {'ansible_shell_type', 'ansible_python_interpreter'}


class _UniqueLoader(yaml.SafeLoader):
    pass


def _mapping(loader, node):
    result = {}
    for key_node, value_node in node.value:
        key = loader.construct_object(key_node)
        require(isinstance(key, str) and key not in result, 'inventory_duplicate', 'Duplicate or non-string inventory key')
        result[key] = loader.construct_object(value_node)
    return result


_UniqueLoader.add_constructor(yaml.resolver.BaseResolver.DEFAULT_MAPPING_TAG, _mapping)


def safe_yaml(text: str):
    require(len(text.encode('utf-8')) <= MAX_INVENTORY_BYTES, 'inventory_size', 'Inventory exceeds byte limit')
    try:
        depth = 0
        for count, token in enumerate(yaml.scan(text)):
            require(count <= 250000, 'inventory_size', 'Inventory token count exceeds limit')
            require(not isinstance(token, (yaml.tokens.AnchorToken, yaml.tokens.AliasToken)),
                    'inventory_alias', 'YAML anchors and aliases are not enabled for static inventory')
        for event in yaml.parse(text):
            if isinstance(event, yaml.events.CollectionStartEvent): depth += 1
            elif isinstance(event, yaml.events.CollectionEndEvent): depth -= 1
            require(depth <= 32, 'inventory_depth', 'Inventory nesting exceeds limit')
        value = yaml.load(text, Loader=_UniqueLoader)
        validate_tree(value)
        return value
    except yaml.YAMLError as error:
        raise RunnerError('inventory_yaml', 'Invalid or unsupported inventory YAML') from error


def _name(name):
    require(isinstance(name, str) and NAME.fullmatch(name) is not None, 'inventory_name', 'Unsupported inventory alias or group name')
    return name


def _variables(value):
    require(value is None or isinstance(value, dict), 'inventory_vars', 'Inventory variables must be a mapping')
    result = dict(value or {})
    validate_tree(result)
    def portable(child):
        if type(child) is int:
            require(-(1<<63)<=child<(1<<63),'inventory_integer',
                    'Inventory integers must fit signed 64-bit; quote larger values to preserve them exactly')
        elif isinstance(child,dict):
            for value in child.values():portable(value)
        elif isinstance(child,list):
            for value in child:portable(value)
    portable(result)
    for key in result:
        require(isinstance(key, str) and len(key) <= 128, 'inventory_vars', 'Invalid variable name')
        require(not key.startswith('consolecrypt_'), 'inventory_reserved', 'ConsoleCrypt routing variables are reserved')
        require(not key.startswith('ansible_') or key in CONNECTION_FIELDS | SAFE_ANSIBLE_VARS,
                'inventory_credential_or_plugin', 'Credentials and custom connection options must be configured through the runner')
    return result


def _connection(values, alias):
    address=values.get('ansible_host',alias)
    port=values.get('ansible_port',22)
    username=values.get('ansible_user')
    require(isinstance(address,str) and 0<len(address)<=253 and not address.startswith('-') and
            re.fullmatch(r'[\w.:\[\]%-]+',address) is not None,'inventory_address','Invalid inventory endpoint')
    if isinstance(port,str) and port.isascii() and port.isdigit():port=int(port)
    require(type(port) is int and 1<=port<=65535,'inventory_port','Invalid inventory port')
    require(username is None or isinstance(username,str) and re.fullmatch(r'[A-Za-z0-9_.@\\-]{1,128}',username) and
            not username.startswith('-'),'inventory_user','Invalid inventory username')
    return {'ansible_host':address,'ansible_port':port,'ansible_user':username}


def _group_variables(value):
    variables=_variables(value)
    resolved=_connection(variables,'group_placeholder')
    return {key:resolved[key] if key in CONNECTION_FIELDS else child for key,child in variables.items()}


@dataclass(frozen=True)
class InventoryHost:
    alias: str
    address: str
    port: int
    username: str | None
    variables: dict
    sources: tuple[str, ...]
    credential_refs: tuple[str, ...] = ()
    placeholder: bool = False
    host_id: str | None = None
    # Parse-only provenance, not part of the wire snapshot. A resolved default
    # must not become an explicit override when another source is merged later.
    connection_fields: frozenset[str] = frozenset()

    def connection(self):
        return (self.address, self.port, self.username, self.credential_refs, self.host_id)


@dataclass
class StaticInventory:
    hosts: dict[str, InventoryHost] = field(default_factory=dict)
    groups: dict[str, dict] = field(default_factory=dict)
    source_digests: dict[str, str] = field(default_factory=dict)

    def add_host(self, alias, variables, source):
        _name(alias)
        require(alias not in ('all','ungrouped','_meta'),'inventory_name','Inventory host uses a reserved group or metadata name')
        placeholder = not variables
        variables = _variables(variables)
        explicit=frozenset(CONNECTION_FIELDS.intersection(variables))
        connection=_connection(variables,alias)
        for key in CONNECTION_FIELDS:variables.pop(key,None)
        host = InventoryHost(alias,connection['ansible_host'],connection['ansible_port'],connection['ansible_user'],
                             variables,(source,),placeholder=placeholder,connection_fields=explicit)
        if alias in self.hosts:
            previous = self.hosts[alias]
            if previous.sources == (source,) and placeholder:
                return  # Empty same-source group membership does not retarget.
            if previous.sources == (source,) and previous.placeholder:
                self.hosts[alias] = host
                return
            require(previous.connection() == host.connection() and previous.variables == host.variables,
                    'inventory_conflict', 'Alias has conflicting endpoints, credentials or variables')
            host = replace(host,sources=tuple(sorted(set(previous.sources+(source,)))),
                           connection_fields=previous.connection_fields|host.connection_fields)
        require(alias in self.hosts or len(self.hosts) < MAX_HOSTS, 'inventory_hosts', 'Inventory host count exceeds limit')
        self.hosts[alias] = host

    def group(self, name):
        _name(name)
        require(name!='_meta','inventory_group','Inventory group uses a reserved JSON metadata name')
        require(name in self.groups or len(self.groups) < MAX_GROUPS, 'inventory_groups', 'Inventory group count exceeds limit')
        return self.groups.setdefault(name, {'hosts': [], 'children': [], 'vars': {}})

    def validate_groups(self):
        require('_meta' not in self.groups,'inventory_group','Inventory group uses a reserved JSON metadata name')
        parents={name:set() for name in self.groups}
        for name,group in self.groups.items():
            for child in group['children']:
                require(child in self.groups and child!='all','inventory_group','Inventory child group is undefined or reserved')
                parents[child].add(name)
        remaining={name:len(values) for name,values in parents.items()}
        queue=[name for name,count in remaining.items() if count==0]
        ancestors={name:set() for name in self.groups};depth={name:0 for name in self.groups}
        for name in queue:
            for child in set(self.groups[name]['children']):
                ancestors[child].update(ancestors[name]);ancestors[child].add(name)
                depth[child]=max(depth[child],depth[name]+1)
                require(depth[child]<=32,'inventory_depth','Inventory group nesting exceeds limit')
                remaining[child]-=1
                if remaining[child]==0:queue.append(child)
        require(len(queue)==len(self.groups),'inventory_cycle','Inventory group cycle')
        return ancestors

    def resolve_connections(self):
        """Materialize visible endpoints; keep raw defaults for later merges.

        Parent -> child -> explicit host follows Ansible precedence. Unlike
        alphabetical sibling ordering, conflicting routing values need an
        explicit host value. Serialized snapshots contain no group retargeting.
        """
        ancestors=self.validate_groups()
        require(not self.hosts.keys()&self.groups.keys(),'inventory_alias_group_conflict',
                'An inventory alias is also a group name; select distinct names before approval')
        if 'all' in self.groups:
            for name in ancestors:
                if name!='all':ancestors[name].add('all')
        direct={alias:set() for alias in self.hosts}
        for name,group in self.groups.items():
            for alias in group['hosts']:
                require(alias in direct,'inventory_host','Group references an undefined host')
                direct[alias].add(name)
        work=0
        attributes={'ansible_host':'address','ansible_port':'port','ansible_user':'username'}
        for alias,host in self.hosts.items():
            applicable={'all'} if 'all' in self.groups else set()
            for name in direct[alias]:
                applicable.add(name);applicable.update(ancestors[name]);work+=len(ancestors[name])+1
            values={key:getattr(host,attribute) for key,attribute in attributes.items() if key in host.connection_fields}
            for key in CONNECTION_FIELDS-host.connection_fields:
                candidates={name:self.groups[name]['vars'][key] for name in applicable if key in self.groups[name]['vars']}
                shadowed=set()
                for name in candidates:
                    shadowed.update(ancestors[name]);work+=len(ancestors[name])+1
                selected={value for name,value in candidates.items() if name not in shadowed}
                require(len(selected)<=1,'inventory_inheritance_conflict',
                        'Inherited connection parameters conflict; set an explicit host value before approval')
                if selected:values[key]=next(iter(selected))
            require(work<=5000000,'inventory_resolution_budget','Inventory inheritance exceeds its resolution budget')
            resolved=_connection(values,alias)
            self.hosts[alias]=replace(host,address=resolved['ansible_host'],port=resolved['ansible_port'],username=resolved['ansible_user'])

    def normalized_groups(self):
        return {name:{**group,'vars':{key:value for key,value in group['vars'].items() if key not in CONNECTION_FIELDS}}
                for name,group in self.groups.items()}

    def standard_json(self):
        self.resolve_connections()
        return {**self.normalized_groups(), '_meta': {'hostvars': {alias: {
            'ansible_host': host.address, 'ansible_port': host.port,
            **({'ansible_user': host.username} if host.username else {}), **host.variables,
        } for alias, host in self.hosts.items()}}}

    @property
    def digest(self):
        return hashlib.sha256(encode(self.to_snapshot())).hexdigest()

    def to_snapshot(self):
        self.resolve_connections()
        return {'version': 1, 'hosts': [{
            'alias': host.alias, 'host_id': host.host_id, 'address': host.address, 'port': host.port,
            'username': host.username, 'variables': host.variables, 'sources': list(host.sources),
            'credential_refs': list(host.credential_refs),
        } for host in sorted(self.hosts.values(), key=lambda item: item.alias)],
            'groups': self.normalized_groups(), 'source_digests': self.source_digests}

    @classmethod
    def from_snapshot(cls, value):
        require(isinstance(value, dict) and set(value) == {'version', 'hosts', 'groups', 'source_digests'} and
                type(value['version']) is int and value['version'] == 1 and isinstance(value['hosts'], list) and
                len(value['hosts']) <= MAX_HOSTS and isinstance(value['groups'], dict) and len(value['groups']) <= MAX_GROUPS and
                isinstance(value['source_digests'], dict), 'inventory_snapshot', 'Invalid normalized inventory snapshot')
        result = cls()
        for source, digest in value['source_digests'].items():
            require(isinstance(source, str) and 0 < len(source) <= 1024 and isinstance(digest, str) and
                    re.fullmatch('[0-9a-f]{64}', digest), 'inventory_source', 'Invalid inventory provenance')
        result.source_digests = dict(value['source_digests'])
        for host in value['hosts']:
            require(isinstance(host, dict) and set(host) == {'alias', 'host_id', 'address', 'port', 'username', 'variables', 'sources', 'credential_refs'},
                    'inventory_snapshot', 'Invalid normalized host')
            require(isinstance(host['sources'], list) and 0 < len(host['sources']) <= 64 and
                    all(isinstance(source, str) and source in result.source_digests for source in host['sources']),
                    'inventory_source', 'Host source is not in inventory provenance')
            require(isinstance(host['alias'], str) and host['alias'] not in result.hosts, 'inventory_conflict', 'Invalid or duplicate normalized host')
            if host['host_id'] is not None: uuid_field(host['host_id'], 'bound host ID')
            refs = host['credential_refs']
            require(isinstance(refs, list) and len(refs) <= 64, 'inventory_credentials', 'Invalid inventory credential references')
            for reference in refs: uuid_field(reference, 'credential reference')
            variables = _variables(host['variables'])
            require(not any(name in CONNECTION_FIELDS for name in variables), 'inventory_vars', 'Connection fields must be resolved explicitly')
            result.add_host(host['alias'], {**variables, 'ansible_host': host['address'], 'ansible_port': host['port'],
                **({'ansible_user': host['username']} if host['username'] is not None else {})}, host['sources'][0])
            from dataclasses import replace
            result.hosts[host['alias']] = replace(result.hosts[host['alias']], sources=tuple(host['sources']),
                host_id=host['host_id'], credential_refs=tuple(refs))
        for name, group in value['groups'].items():
            _name(name)
            require(isinstance(group, dict) and set(group) == {'hosts', 'children', 'vars'} and
                    isinstance(group['hosts'], list) and len(group['hosts']) <= MAX_HOSTS and
                    all(isinstance(alias, str) and alias in result.hosts for alias in group['hosts']) and
                    isinstance(group['children'], list) and len(group['children']) <= MAX_GROUPS and
                    all(isinstance(child, str) for child in group['children']), 'inventory_group', 'Invalid normalized group')
            variables = _variables(group['vars'])
            require(not any(key in CONNECTION_FIELDS for key in variables), 'inventory_inheritance', 'Resolve inherited connection parameters before approval')
            result.groups[name] = {'hosts': list(group['hosts']), 'children': list(group['children']), 'vars': variables}
        result.validate_groups()
        require(len(encode(result.to_snapshot())) <= MAX_INVENTORY_BYTES, 'inventory_size', 'Normalized inventory exceeds its budget')
        return result

    def runtime_inventory(self, manifest):
        self.resolve_connections()
        selected = {}
        for target in manifest['targets']:
            host = self.hosts.get(target['alias'])
            require(host is not None and host.host_id == target['host_id'] and host.address == target['address'] and
                    host.port == target['port'] and host.username in (None, target['username']) and
                    list(host.credential_refs) == target['credential_refs'], 'inventory_binding', 'Inventory differs from approved target binding')
            selected[host.alias] = {**host.variables, 'ansible_host': target['address'], 'ansible_port': target['port'],
                'ansible_user': target['username'], 'ansible_connection': 'consolecrypt_rpc', 'consolecrypt_host_id': target['host_id']}
        output = {'all': {'hosts': selected}}
        for name, group in self.normalized_groups().items():
            if name == 'all':
                output['all']['vars'] = group['vars']
            else:
                output[name] = {'hosts': {alias: {} for alias in group['hosts'] if alias in selected},
                                'children': {child: {} for child in group['children']}, 'vars': group['vars']}
        return output


def parse_yaml_inventory(text, source, *, resolve=True):
    data = safe_yaml(text)
    require(isinstance(data, dict) and 'plugin' not in data, 'inventory_plugin', 'Only static YAML inventory is supported')
    inventory = StaticInventory()
    inventory.source_digests[source] = hashlib.sha256(text.encode('utf-8')).hexdigest()
    def visit(name, content, depth=0):
        require(depth <= 32, 'inventory_depth', 'Inventory nesting exceeds limit')
        require(content is None or isinstance(content, dict), 'inventory_group', 'Inventory group must be a mapping')
        content = content or {}
        require(set(content) <= {'hosts', 'children', 'vars'}, 'inventory_plugin', 'Unsupported inventory group fields')
        group = inventory.group(name)
        variables = _group_variables(content.get('vars'))
        require(not group['vars'] or not variables or group['vars'] == variables, 'inventory_conflict', 'Conflicting group variables')
        group['vars'].update(variables)
        hosts = content.get('hosts') or {}
        require(isinstance(hosts, dict), 'inventory_hosts', 'Inventory hosts must be a mapping')
        for alias, values in hosts.items():
            inventory.add_host(alias, values, source)
            if alias not in group['hosts']: group['hosts'].append(alias)
        children = content.get('children') or {}
        require(isinstance(children, dict), 'inventory_group', 'Inventory children must be a mapping')
        for child, values in children.items():
            if child not in group['children']: group['children'].append(child)
            visit(child, values, depth + 1)
    for name, content in data.items(): visit(name, content)
    if resolve:inventory.resolve_connections()
    return inventory


def parse_ini_inventory(text, source, *, resolve=True):
    require(len(text.encode('utf-8')) <= MAX_INVENTORY_BYTES, 'inventory_size', 'Inventory exceeds byte limit')
    inventory = StaticInventory()
    inventory.source_digests[source] = hashlib.sha256(text.encode('utf-8')).hexdigest()
    section, kind = 'ungrouped', 'hosts'
    inventory.group(section)
    for line in text.splitlines():
        line = line.strip()
        if not line or line.startswith(('#', ';')): continue
        if line.startswith('['):
            require(line.endswith(']'), 'inventory_ini', 'Invalid inventory section')
            pieces = line[1:-1].split(':')
            require(len(pieces) <= 2, 'inventory_ini', 'Unsupported inventory section')
            section = _name(pieces[0]); kind = pieces[1] if len(pieces) == 2 else 'hosts'
            require(kind in ('hosts', 'vars', 'children'), 'inventory_plugin', 'Inventory scripts/plugins are not enabled')
            inventory.group(section)
            continue
        try:
            pieces = shlex.split(line, comments=True)
        except ValueError as error:
            raise RunnerError('inventory_ini', 'Invalid inventory quoting') from error
        if not pieces: continue
        group = inventory.group(section)
        if kind == 'children':
            require(len(pieces) == 1, 'inventory_ini', 'Invalid child group')
            child = _name(pieces[0]); inventory.group(child)
            if child not in group['children']: group['children'].append(child)
        elif kind == 'vars':
            require(len(pieces) == 1 and '=' in pieces[0], 'inventory_ini', 'Invalid group variable')
            key, value = pieces[0].split('=', 1)
            values = _group_variables({key: value})
            require(key not in group['vars'], 'inventory_duplicate', 'Duplicate group variable')
            group['vars'].update(values)
        else:
            values = {}
            for piece in pieces[1:]:
                require('=' in piece, 'inventory_ini', 'Invalid host variable')
                key, value = piece.split('=', 1)
                require(key not in values, 'inventory_duplicate', 'Duplicate host variable')
                try: values[key] = json.loads(value)
                except ValueError: values[key] = value
            inventory.add_host(pieces[0], values, source)
            if pieces[0] not in group['hosts']: group['hosts'].append(pieces[0])
    if resolve:inventory.resolve_connections()
    return inventory


def merge_inventories(sources: list[StaticInventory], *, resolve=True):
    merged = StaticInventory()
    for source in sources:
        for key, digest in source.source_digests.items():
            require(key not in merged.source_digests or merged.source_digests[key] == digest,
                    'inventory_source_changed', 'Inventory source changed during merge')
            merged.source_digests[key] = digest
        for alias, host in source.hosts.items():
            previous = merged.hosts.get(alias)
            if previous is not None:
                require(previous.credential_refs==host.credential_refs and previous.host_id==host.host_id and
                        previous.variables==host.variables,
                        'inventory_conflict', 'Alias conflicts between sources; select an explicit resolution')
                attributes={'ansible_host':'address','ansible_port':'port','ansible_user':'username'}
                declared={}
                for key,attribute in attributes.items():
                    if key in previous.connection_fields and key in host.connection_fields:
                        require(getattr(previous,attribute)==getattr(host,attribute),'inventory_conflict',
                                'Alias conflicts between sources; select an explicit resolution')
                    if key in host.connection_fields:declared[key]=getattr(host,attribute)
                    elif key in previous.connection_fields:declared[key]=getattr(previous,attribute)
                connection=_connection(declared,alias)
                host = replace(host,address=connection['ansible_host'],port=connection['ansible_port'],username=connection['ansible_user'],
                    sources=tuple(sorted(set(previous.sources+host.sources))),
                    connection_fields=previous.connection_fields|host.connection_fields)
            require(alias in merged.hosts or len(merged.hosts) < MAX_HOSTS, 'inventory_hosts', 'Too many inventory hosts')
            merged.hosts[alias] = host
        for name, values in source.groups.items():
            group = merged.group(name)
            require(all(key not in group['vars'] or group['vars'][key]==value for key,value in values['vars'].items()),
                    'inventory_conflict', 'Group variables conflict between sources')
            group['vars'].update(values['vars'])
            group['hosts'] = sorted(set(group['hosts'] + values['hosts']))
            group['children'] = sorted(set(group['children'] + values['children']))
    if resolve:merged.resolve_connections()
    return merged


def apply_variable_sources(inventory, sources):
    """Static directory vars, lexically ordered; never imports a vars plugin.

    Sources are (path, scope, target, UTF-8 text). Unknown targets are explicit
    errors instead of silently creating a group/host or losing configuration.
    """
    for path,scope,target,text in sorted(sources):
        _name(target)
        values=_group_variables(safe_yaml(text))
        inventory.source_digests[path]=hashlib.sha256(text.encode('utf-8')).hexdigest()
        if scope=='group_vars':
            require(target=='all' or target in inventory.groups,'inventory_vars_target','Group vars refer to an undefined group')
            inventory.group(target)['vars'].update(values)
        else:
            require(scope=='host_vars' and target in inventory.hosts,'inventory_vars_target','Host vars refer to an undefined host')
            host=inventory.hosts[target]
            attributes={'ansible_host':'address','ansible_port':'port','ansible_user':'username'}
            declared={key:getattr(host,attribute) for key,attribute in attributes.items() if key in host.connection_fields}
            declared.update({key:value for key,value in values.items() if key in CONNECTION_FIELDS})
            connection=_connection(declared,host.alias)
            inventory.hosts[target]=replace(host,address=connection['ansible_host'],port=connection['ansible_port'],username=connection['ansible_user'],
                variables={**host.variables,**{key:value for key,value in values.items() if key not in CONNECTION_FIELDS}},
                connection_fields=frozenset(declared),sources=tuple(sorted(set(host.sources+(path,)))))
    inventory.resolve_connections()
    return inventory
