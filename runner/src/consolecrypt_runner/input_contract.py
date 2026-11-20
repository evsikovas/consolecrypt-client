"""Optional declared launch inputs. Parsing happens inside the validation guest.

This is presence checking of explicit extra_vars, not Jinja execution or a claim
that every possible variable referenced by arbitrary Ansible code can be found.
"""
from pathlib import Path
import re

from .errors import require
from .protocol import decode
from .snapshot import relative_path
from .project_contracts import contract_path

INPUT_CONTRACT = '.consolecrypt-inputs.json'
MAX_CONTRACT_BYTES = 32 * 1024


def check_declared_inputs(root: Path, playbook: str, extra_vars: dict) -> int:
    selected = str(relative_path(playbook))
    path = contract_path(root, INPUT_CONTRACT)
    require(not path.is_symlink(), 'input_contract_file', 'Input contract must be a regular project file')
    if not path.exists():
        return 0
    require(path.is_file(), 'input_contract_file', 'Input contract must be a regular project file')
    with path.open('rb') as file:
        contract = decode(file.read(MAX_CONTRACT_BYTES + 1), max_bytes=MAX_CONTRACT_BYTES)
    require(isinstance(contract, dict) and set(contract) == {'version', 'playbooks'} and
            type(contract['version']) is int and contract['version'] == 1,
            'input_contract_version', 'Unsupported project input contract; review .consolecrypt-inputs.json')
    playbooks = contract['playbooks']
    require(isinstance(playbooks, dict) and len(playbooks) <= 128,
            'input_contract_shape', 'Input contract supports at most 128 playbooks')
    for name, spec in playbooks.items():
        relative_path(name)
        require(isinstance(spec, dict) and set(spec) == {'required_extra_vars'},
                'input_contract_shape', 'Each playbook contract must declare required_extra_vars')
        names = spec['required_extra_vars']
        require(isinstance(names, list) and len(names) <= 64 and
                all(isinstance(item, str) and re.fullmatch(r'[A-Za-z_][A-Za-z0-9_]{0,127}', item) and
                    not item.startswith(('ansible_', 'consolecrypt_')) for item in names),
                'input_contract_names', 'Required input names must be bounded non-connection variable identifiers')
        require(len(set(names)) == len(names), 'input_contract_names', 'Duplicate required input name')
    names = playbooks.get(selected, {}).get('required_extra_vars', [])
    missing = [name for name in names if name not in extra_vars or extra_vars[name] is None or
               isinstance(extra_vars[name], str) and not extra_vars[name].strip()]
    # Names have already passed the identifier allowlist. Never echo values or
    # raw invalid JSON/YAML into process output, public diagnostics or history.
    require(not missing, 'required_inputs', 'Missing required execution inputs: ' + ', '.join(missing))
    return len(names)
