"""Execution options are data and part of the signed approval, never raw argv."""
import hashlib
import re

from .errors import require
from .protocol import encode


def validate_parameters(value):
    legacy={'extra_vars', 'tags', 'skip_tags', 'diff'}
    require(isinstance(value, dict) and set(value) in (legacy,legacy|{'vault'}),
            'parameters_shape', 'Invalid execution parameter fields')
    require(isinstance(value['extra_vars'], dict) and type(value['diff']) is bool,
            'parameters_shape', 'Variables must be an object and diff a boolean')
    if 'vault' in value:
        binding=value['vault']
        require(isinstance(binding,dict) and set(binding)=={'version','scope_id','operation_id','reference_sha256','environment_digest','helper_digest'}
                and type(binding['version']) is int and binding['version']==1 and not value['diff'],
                'vault_binding','Unsupported signed Vault execution binding')
        from .protocol import uuid_field
        import uuid
        for name in ('scope_id','operation_id'):
            uuid_field(binding[name],name)
            require(uuid.UUID(binding[name]).int!=0,'vault_binding','Invalid signed Vault execution scope')
        for name in ('reference_sha256','environment_digest','helper_digest'):
            require(isinstance(binding[name],str) and re.fullmatch('[0-9a-f]{64}',binding[name]) is not None,
                    'vault_binding','Invalid signed Vault execution digest')
    raw = encode(value['extra_vars'])
    require(len(raw) <= 64 * 1024 and len(value['extra_vars']) <= 256,
            'parameters_limit', 'Execution variables exceed limit')
    for name in value['extra_vars']:
        require(re.fullmatch(r'[A-Za-z_][A-Za-z0-9_]{0,127}', name) is not None and
                not name.startswith(('ansible_', 'consolecrypt_')),
                'reserved_variable', 'Connection and authority variables cannot be overridden by execution parameters')
    for name in ('tags', 'skip_tags'):
        tags = value[name]
        require(isinstance(tags, list) and len(tags) <= 64 and all(isinstance(tag, str) for tag in tags),
                'parameters_tags', 'Tags must be a unique bounded list')
        require(len(set(tags)) == len(tags), 'parameters_tags', 'Duplicate execution tag')
        require(all(isinstance(tag, str) and re.fullmatch('[A-Za-z0-9_.-]{1,128}', tag) and
                    not tag.startswith('-') for tag in tags), 'parameters_tags', 'Invalid execution tag')
    # JSON roundtrip yields an owned copy; mutable caller data is never retained.
    from .protocol import decode
    return decode(encode(value))


def parameters_digest(value):
    return hashlib.sha256(encode(validate_parameters(value))).hexdigest()
