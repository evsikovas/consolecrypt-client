"""Trusted bootstrap inside the isolated guest. No authority in emitted events."""
import json
from pathlib import Path
import sys

import ansible_runner

from .protocol import decode, encode
from .event_privacy import diagnostic_result


def emit(kind, payload):
    raw = encode({'type': kind, 'payload': payload})
    if len(raw) > 128 * 1024:
        raw = encode({'type': kind, 'payload': {'truncated': True}})
    sys.stdout.buffer.write(raw + b'\n')
    sys.stdout.buffer.flush()


def main():
    with Path('/tool/job-input.json').open('rb') as source:
        spec = decode(source.read(6*1024*1024+1),max_bytes=6*1024*1024)
    enabled=spec.get('vault_execution',False)
    from .errors import require
    require(type(enabled) is bool,'vault_client','Invalid fixed Vault job mode')
    fixed_env={}
    if enabled:
        # Reuse the SAME trusted mode0500 path/env enforcement as validation.
        # It clears project-supplied identity/password sources; only our fixed
        # executable reads the dedicated one-call private socket.
        from .validation_worker import _vault_client
        _vault_client({'vault_validation':True})
        fixed_env={'ANSIBLE_VAULT_PASSWORD_FILE':'/tool/vault-password-client',
            'ANSIBLE_VAULT_IDENTITY_LIST':'','ANSIBLE_ASK_VAULT_PASS':'False','ANSIBLE_VAULT_ID_MATCH':'False'}
    Path('/work/runner').mkdir(mode=0o700)
    stats = {}
    def event(event):
        nonlocal stats
        kind, data = event.get('event'), event.get('event_data', {})
        if kind == 'playbook_on_stats':
            stats = {key: data[key] for key in ('ok', 'changed', 'failures', 'dark', 'skipped', 'rescued', 'ignored') if key in data}
        if kind in ('runner_on_ok', 'runner_on_failed', 'runner_on_unreachable', 'runner_on_skipped',
                    'playbook_on_task_start', 'playbook_on_play_start', 'playbook_on_stats'):
            payload = {key: data[key] for key in ('host', 'task', 'task_uuid', 'play', 'play_uuid', 'task_action', 'res') if key in data}
            result = payload.get('res')
            if 'res' in payload:
                payload['res'] = diagnostic_result(result)
            if len(encode(payload)) > 96 * 1024:
                payload['res'] = {'truncated': True, 'changed': bool(result.get('changed')) if isinstance(result, dict) else None}
            emit(kind, payload)
        return False  # No unbounded JSON event copies in the artifact directory.
    parameters = spec['parameters']
    options = []
    if spec['mode'] == 'check': options.append('--check')
    if parameters['diff']: options.append('--diff')
    if parameters['tags']: options += ['--tags', ','.join(parameters['tags'])]
    if parameters['skip_tags']: options += ['--skip-tags', ','.join(parameters['skip_tags'])]
    result = ansible_runner.run(private_data_dir='/work/runner', project_dir='/work/project',
        artifact_dir='/work/artifacts', playbook=spec['playbook'], inventory=spec['inventory'],
        extravars=parameters['extra_vars'], forks=spec['forks'], cmdline=' '.join(options),
        envvars={'ANSIBLE_CONNECTION_PLUGINS': '/tool/python/consolecrypt_runner/plugins/connection',**fixed_env},
        event_handler=event, quiet=True, settings={'suppress_output_file': True})
    emit('runner_summary', {'status': result.status, 'exit_code': result.rc, 'stats': stats})
    raise SystemExit(result.rc if type(result.rc) is int and 0 <= result.rc <= 255 else 70)


if __name__ == '__main__':
    try:
        main()
    except Exception:
        # No raw traceback/project values in persisted diagnostics. Process exit
        # without a complete Runner summary remains unknown in the parent.
        emit('worker_error', {'code': 'runner_bootstrap_failed'})
        raise SystemExit(70)
