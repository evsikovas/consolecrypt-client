"""Trusted bootstrap inside the isolated guest. No authority in emitted events."""
from pathlib import Path
import subprocess
import sys

import ansible_runner

from .protocol import decode, encode
from .event_privacy import diagnostic_result
from .worker_paths import input_source, WORK, PROJECT


def emit(kind, payload):
    raw = encode({'type': kind, 'payload': payload})
    if len(raw) > 128 * 1024:
        raw = encode({'type': kind, 'payload': {'truncated': True}})
    sys.stdout.buffer.write(raw + b'\n')
    sys.stdout.buffer.flush()


def run_without_pty(options):
    """Use ansible-runner's inventory and event protocol over an ordinary pipe."""
    from ansible_runner.interface import init_runner
    from ansible_runner.utils import OutputEventFilter

    runner = init_runner(**options)
    config = runner.config
    (Path(config.artifact_dir) / 'job_events').mkdir(mode=0o700)
    events = OutputEventFilter(None, runner.event_callback, suppress_ansible_output=True)
    # Inherit the worker's process group so a ConsoleCrypt cancel also stops
    # Ansible. Stream events as they arrive without access to device PTYs.
    with subprocess.Popen(config.command, cwd=config.cwd, env=config.env,
                          stdin=subprocess.DEVNULL, stdout=subprocess.PIPE,
                          stderr=subprocess.STDOUT, encoding='utf-8', errors='replace') as process:
        limit = 1024 * 1024
        while line := process.stdout.readline(limit + 1):
            if len(line) > limit:
                process.kill()
                raise ValueError('Runner output line exceeds the worker limit')
            events.write(line)
        events.close()
        runner.rc = process.wait()
    runner.status = 'successful' if runner.rc == 0 else 'failed'
    return runner


def main():
    with input_source('job-input.json') as source:
        spec = decode(source.read(6*1024*1024+1),max_bytes=6*1024*1024)
    enabled=spec.get('vault_execution',False)
    from .errors import require
    require(type(enabled) is bool,'vault_client','Invalid fixed Vault job mode')
    fixed_env={}
    sudo_enabled=spec.get('become_native',False)
    require(type(sudo_enabled) is bool,'become_native','Invalid fixed Native sudo mode')
    if enabled:
        # Reuse the SAME trusted mode0500 path/env enforcement as validation.
        # It clears project-supplied identity/password sources; only our fixed
        # executable reads the dedicated one-call private socket.
        from .validation_worker import _vault_client
        _vault_client({'vault_validation':True})
        fixed_env={'ANSIBLE_VAULT_PASSWORD_FILE':'/tool/vault-password-client',
            'ANSIBLE_VAULT_IDENTITY_LIST':'','ANSIBLE_ASK_VAULT_PASS':'False','ANSIBLE_VAULT_ID_MATCH':'False'}
    fixed_env['CC_NATIVE_SUDO']='1' if sudo_enabled else '0'
    (WORK / 'runner').mkdir(mode=0o700)
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
    runner_options = dict(private_data_dir=str(WORK / 'runner'), project_dir=str(PROJECT),
        artifact_dir=str(WORK / 'artifacts'), playbook=spec['playbook'], inventory=spec['inventory'],
        extravars=parameters['extra_vars'], forks=spec['forks'], cmdline=' '.join(options),
        envvars={'ANSIBLE_CONNECTION_PLUGINS': str(Path(__file__).resolve().parent / 'plugins/connection'), **fixed_env},
        event_handler=event, quiet=True, settings={'suppress_output_file': True})
    result = run_without_pty(runner_options) if sys.platform == 'darwin' else ansible_runner.run(**runner_options)
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
