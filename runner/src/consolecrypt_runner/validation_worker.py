"""Runs only inside the guest, including parsing project-controlled lint config."""
import json
import os
from pathlib import Path
import sys
import uuid
import stat

from .errors import RunnerError, require
from .inventory import safe_yaml
from .input_contract import check_declared_inputs
from .project_requirements import check_project_requirements
from .dependencies import check_locked_dependencies, LOCK_FILE
from .project_contracts import contract_path
from .protocol import decode
from .declared_privileges import summarize_entries, emit_summary


def _count_approved_hosts(entries, inventory, approved_aliases):
    """Count actual Ansible objects, never parse project-controlled CLI text."""
    require(isinstance(entries, list), 'validation_host_resolution', 'Ansible did not return a host listing')
    matched = set()
    plays = work = 0
    for entry in entries:
        require(isinstance(entry, dict) and isinstance(entry.get('plays'), list),
                'validation_host_resolution', 'Ansible returned an unsupported host listing')
        for play in entry['plays']:
            plays += 1
            require(plays <= 1024, 'validation_host_budget', 'Playbook target listing exceeds its play limit')
            hosts = inventory.get_hosts(play.hosts)
            work += len(hosts)
            require(work <= 1000000, 'validation_host_budget', 'Playbook target listing exceeds its host limit')
            matched.update(host.get_name() for host in hosts if host.get_name() in approved_aliases)
    require(matched, 'validation_no_matching_targets',
            "Playbook matches zero approved targets. Check its 'hosts' group/pattern and the selected "
            'inventory group membership; select matching hosts or correct the playbook. No hosts or groups were added.')
    return plays, len(matched)


def check_playbook_targets(playbook, inventory_path, variables_path, approved_aliases, *, vault_client=None, on_declared=None):
    """Guest-only pinned Ansible --list-hosts semantics, without any task queue.

    Uses the same parser, extra-vars, imported plays and template validation as
    the real CLI. The private API is deliberately guarded by the exact runtime
    version and exercised against the pinned image. Project code remains in the
    existing no-network/no-credentials namespace and outer process budgets.
    """
    require(isinstance(approved_aliases, (set, frozenset)) and
            0 < len(approved_aliases) <= 10000 and
            all(isinstance(alias, str) and 0 < len(alias) <= 96 for alias in approved_aliases),
            'validation_targets', 'Approved validation targets are missing or invalid')
    try:
        # Import the CLI first, as required by Ansible's controller setup.
        from ansible.cli import CLI
        from ansible.cli.playbook import PlaybookCLI
        from ansible import context
        from ansible.release import __version__
        from ansible.executor.playbook_executor import PlaybookExecutor
        from ansible.plugins.loader import add_all_plugin_dirs
        from ansible.utils.collection_loader import AnsibleCollectionConfig

        require(__version__ == '2.20.11', 'validation_ansible_version',
                'Target matching requires the pinned Ansible 2.20.11 runtime')
        argv=['ansible-playbook','--list-hosts','-i',str(inventory_path),'--extra-vars','@'+str(variables_path)]
        if vault_client is not None:
            require(vault_client=='/tool/vault-password-client','vault_client','Invalid trusted Vault client')
            # One trusted environment source only. Adding the same CLI file
            # duplicates DEFAULT_VAULT_PASSWORD_FILE in pinned Ansible2.20.
        cli=PlaybookCLI([*argv,'--',playbook])
        # CLI._play_prereqs otherwise starts an SSH agent even for list-hosts.
        # Listing needs the inventory/variables, never connection authority.
        cli.USES_CONNECTION = False
        CLI.run(cli)
        basedir = str(Path(playbook).resolve().parent)
        add_all_plugin_dirs(basedir)
        AnsibleCollectionConfig.playbook_paths = [basedir]
        loader, inventory, variables = cli._play_prereqs()
        require(context.CLIARGS.get('listhosts') is True and not context.CLIARGS.get('syntax'),
                'validation_host_resolution', 'Ansible target listing mode is unavailable')
        executor = PlaybookExecutor(playbooks=context.CLIARGS['args'], inventory=inventory,
                                    variable_manager=variables, loader=loader, passwords={})
        require(executor._tqm is None, 'validation_host_resolution',
                'Target listing must not create an Ansible task executor')
        entries = executor.run()
        counts = _count_approved_hosts(entries, inventory, approved_aliases)
        if on_declared is not None:
            try:
                on_declared(summarize_entries(entries))
            except Exception:
                # Optional informational metadata never changes target validation.
                pass
        return counts
    except RunnerError:
        raise
    except Exception as error:
        # Avoid echoing arbitrary extra-vars, templated patterns or file content.
        raise RunnerError('validation_host_resolution',
                          'Cannot resolve playbook hosts against approved inventory; check host templates and execution inputs') from error


def _vault_client(spec):
    enabled=spec.get('vault_validation',False)
    require(type(enabled) is bool,'vault_client','Invalid private validation mode')
    if not enabled:return None
    path=Path('/tool/vault-password-client')
    info=path.lstat()
    require(stat.S_ISREG(info.st_mode) and stat.S_IMODE(info.st_mode)==0o500 and os.access(path,os.X_OK),
            'vault_client','Trusted executable Vault client unavailable')
    # Env overrides project ansible.cfg. Empty identity list prevents alternate
    # executable/plaintext password sources; labels remain hints, not resolvers.
    os.environ['ANSIBLE_VAULT_PASSWORD_FILE']=str(path)
    os.environ['ANSIBLE_VAULT_IDENTITY_LIST']=''
    os.environ['ANSIBLE_ASK_VAULT_PASS']='False'
    os.environ['ANSIBLE_VAULT_ID_MATCH']='False'
    return str(path)

def main():
    with Path('/tool/validation-input.json').open('rb') as source:
        spec = decode(source.read(6*1024*1024+1),max_bytes=6*1024*1024)
    vault_client=_vault_client(spec)
    directory = Path('/work/validation')
    directory.mkdir(mode=0o700)
    inventory = directory / 'inventory.json'
    variables = directory / 'variables.json'
    inventory.write_text(json.dumps(spec['inventory']))
    variables.write_text(json.dumps(spec['parameters']['extra_vars']))
    os.environ['ANSIBLE_INVENTORY'] = str(inventory)
    if sys.argv[1] == 'syntax':
        snapshot = None
        if contract_path(Path('/work/project'), LOCK_FILE).exists():
            with Path('/tool/project-snapshot.json').open('rb') as metadata:
                snapshot = decode(metadata.read(32 * 1024 * 1024 + 1), max_bytes=32 * 1024 * 1024)
        dependencies = check_locked_dependencies(Path('/work/project'), snapshot)
        if dependencies:
            print(f'ConsoleCrypt: locked dependency trees verified ({dependencies})', flush=True)
        count=check_declared_inputs(Path('/work/project'),spec['playbook'],spec['parameters']['extra_vars'])
        count += check_project_requirements(Path('/work/project'),spec['playbook'],spec['parameters']['extra_vars'])
        if count:
            print(f'ConsoleCrypt: required execution inputs satisfied ({count})',flush=True)
        require(type(spec.get('require_host_match', False)) is bool,
                'validation_targets', 'Invalid validation target matching mode')
        if spec.get('require_host_match', False):
            selected = spec['inventory'].get('all', {}).get('hosts', {})
            require(isinstance(selected, dict), 'validation_targets', 'Approved validation inventory is invalid')
            plays, hosts = check_playbook_targets(spec['playbook'], inventory, variables, set(selected),vault_client=vault_client,on_declared=emit_summary)
            print(f'ConsoleCrypt: playbook target match verified ({hosts} approved hosts across {plays} plays)', flush=True)
        argv = [str(Path(sys.prefix) / 'bin/ansible-playbook'), '--syntax-check', '-i', str(inventory),
                '--extra-vars', '@' + str(variables), '--', spec['playbook']]
    else:
        config = {}
        for candidate in ('.ansible-lint', '.ansible-lint.yml', '.ansible-lint.yaml', '.config/ansible-lint.yml', '.config/ansible-lint.yaml'):
            path = Path('/work/project') / candidate
            if path.is_file():
                require(path.stat().st_size <= 1024 * 1024, 'lint_config_size', 'Lint configuration exceeds limit')
                config = safe_yaml(path.read_text(encoding='utf-8'))
                require(isinstance(config, dict), 'lint_config', 'Lint configuration must be a mapping')
                break
        extra_vars = config.get('extra_vars', {})
        require(isinstance(extra_vars, dict), 'lint_config', 'Lint extra_vars must be a mapping')
        config['extra_vars'] = {**extra_vars, **spec['parameters']['extra_vars']}
        # Keep relative rule/exclude paths relative to the copied project.
        generated = Path('/work/project') / ('.cc-lint-' + uuid.uuid4().hex + '.json')
        with generated.open('x', encoding='utf-8') as output:
            json.dump(config, output)
        argv = [str(Path(sys.prefix) / 'bin/ansible-lint'), '--offline', '--nocolor', '--format', 'json',
                '-c', str(generated), '--', spec['playbook']]
    os.execv(argv[0], argv)


if __name__ == '__main__':
    try:
        main()
    except RunnerError as error:
        print('ConsoleCrypt: '+str(error),file=sys.stderr)
        raise SystemExit(65)
    except (UnicodeError, ValueError, OSError):
        print('ConsoleCrypt: invalid isolated validation inputs', file=sys.stderr)
        raise SystemExit(65)
