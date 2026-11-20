from __future__ import annotations

from dataclasses import asdict, dataclass
from pathlib import Path
import sys
import threading
import tempfile
import hashlib
from contextlib import ExitStack

from .errors import require
from .probe import probe
from .sandbox import bubblewrap_argv, run_bounded
from .snapshot import ProjectSnapshot, relative_path, verify_snapshot
from .parameters import validate_parameters, parameters_digest
from .protocol import encode
from .dependencies import LOCK_FILE
from .declared_privileges import extract_summary
from .ephemeral_input import EphemeralValidationInput
from .diagnostic_mask import KnownJobSecrets


@dataclass(frozen=True)
class ValidationReport:
    revision_digest: str
    environment_digest: str
    checks: tuple[dict, ...]
    valid: bool
    playbook: str
    parameters_digest: str | None
    inventory_digest: str | None
    targets_digest: str | None
    declared_privileges: dict | None = None

    def to_json(self):
        value = asdict(self)
        if value['declared_privileges'] is None:
            del value['declared_privileges']
        return value


def validate_project(root: Path, snapshot: ProjectSnapshot, playbook: str, *, cancel: threading.Event | None = None,
                     parameters=None, inventory=None, targets=None, vault_phase_factory=None, private_directory=None,
                     vault_sidecar_factory=None):
    path = relative_path(playbook)
    require(any(entry['path'] == str(path) and entry['type'] == 'file' for entry in snapshot.entries),
            "playbook_missing", "Selected playbook is not in this revision")
    environment = probe()
    require(environment['capabilities']['validate'], "validation_unavailable", "Prepared isolated validation environment required")
    verify_snapshot(root, snapshot)
    checks = []
    declared_privileges = None
    bound_parameters = parameters_digest(parameters) if parameters is not None else None
    parameters = validate_parameters(parameters or {'extra_vars': {}, 'tags': [], 'skip_tags': [], 'diff': False})
    bound_inventory = inventory.digest if inventory is not None else None
    require((inventory is None) == (targets is None), 'validation_targets', 'Resolved validation targets and inventory are required together')
    bound_targets = hashlib.sha256(encode(targets)).hexdigest() if targets is not None else None
    runtime_inventory = inventory.runtime_inventory({'targets': targets}) if inventory is not None else {'all': {'hosts': {}}}
    commands = ('syntax', 'lint')
    with tempfile.TemporaryDirectory(prefix='cc-validation-inputs-',dir=private_directory) as temporary, ExitStack() as privacy_scope:
        known=KnownJobSecrets(parameters);privacy_scope.callback(known.close)
        body=encode({'playbook': playbook, 'parameters': parameters, 'inventory': runtime_inventory,
                     'require_host_match': targets is not None,**({'vault_validation':True} if vault_phase_factory is not None or vault_sidecar_factory is not None else {})})
        require(len(body)<=6*1024*1024,'validation_inputs','Private validation input exceeds its budget')
        mounts = []
        if any(entry['path'] == LOCK_FILE for entry in snapshot.entries):
            metadata = Path(temporary) / 'project-snapshot.json'
            snapshot_body = encode(snapshot.to_json())
            require(len(snapshot_body) <= 32 * 1024 * 1024, 'dependency_snapshot', 'Snapshot metadata exceeds budget')
            metadata.write_bytes(snapshot_body)
            metadata.chmod(0o600)
            mounts.append((metadata, '/tool/project-snapshot.json'))
        for name in commands:
            command = [sys.executable, '-m', 'consolecrypt_runner.validation_worker', name]
            if sys.platform == 'darwin':
                from .macos_sandbox import run_macos_worker
                require(vault_phase_factory is None and vault_sidecar_factory is None,
                        'validation_unavailable', 'One-shot Vault validation requires the Linux Runner')
                result = run_macos_worker(root, command, tool_directory=temporary, input_bytes=body,
                                          timeout=120, cancel=cancel, output_limit=128*1024)
            else:
                with ExitStack() as phase_scope:
                    inputs = phase_scope.enter_context(EphemeralValidationInput(body))
                    broker=phase_scope.enter_context(vault_phase_factory(name)) if vault_phase_factory is not None else None
                    channel=phase_scope.enter_context(vault_sidecar_factory(name)) if vault_sidecar_factory is not None else None
                    require(broker is None or channel is None,'vault_phase','Ambiguous Vault password source')
                    phase_mounts=mounts+([(broker.client_script,'/tool/vault-password-client')] if broker is not None else [])
                    argv=bubblewrap_argv(root,command,extra_readonly=phase_mounts,socket_directory=broker.directory if broker is not None else None,vault_channel=channel,validation_input=inputs)
                    if broker is not None:
                        split=argv.index('--');argv[split:split]=['--setenv','ANSIBLE_VAULT_PASSWORD_FILE','/tool/vault-password-client']
                    result=run_bounded(argv,timeout=120,cancel=cancel,output_limit=128*1024,grace=1 if broker is not None else 15,vault_channel=channel,validation_input=inputs)
            fields = result.validation_fields()
            if name == 'syntax':
                completed = result.exit_code == 0 and not (result.cancelled or result.timed_out or result.truncated)
                declared_privileges, fields['output'] = extract_summary(result.output, completed=completed)
                if vault_phase_factory is not None or vault_sidecar_factory is not None:
                    declared_privileges = None
            # Raw checks are persisted before any signed job exists. Known
            # vars stay in this original trusted validation-parent memory.
            # Vault bytes remain private to the existing sidecar: without
            # exposing/resolving them, withhold its diagnostic text entirely.
            if result.truncated or result.cancelled or result.timed_out or 'vault' in parameters or vault_phase_factory is not None or vault_sidecar_factory is not None:
                fields['output']='[CENSORED]'
            else:
                try:fields['output']=known.text(fields['output'])
                except Exception:fields['output']='[CENSORED]'
            checks.append({"name": name, **fields})
            if result.cancelled or result.timed_out:
                break
    verify_snapshot(root, snapshot)
    valid = len(checks) == len(commands) and all(item['exit_code'] == 0 and not item['cancelled'] and
            not item['timed_out'] and not item['truncated'] for item in checks)
    return ValidationReport(snapshot.digest, environment['environment_digest'], tuple(checks), valid, playbook,
                            bound_parameters, bound_inventory, bound_targets, declared_privileges)
