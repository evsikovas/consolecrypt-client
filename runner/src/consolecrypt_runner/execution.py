"""Interactive lifecycle only. No schedule, daemon or implicit retry."""
import base64
from concurrent.futures import ThreadPoolExecutor
from contextlib import ExitStack
import hashlib
from pathlib import Path
import sys
import threading
import time

from .approval import TrustGrant, verify_envelope
from .production_owner import configured_manual_owner, original_manual_grant, OWNER_POLICY
from .broker import JobBroker
from .errors import RunnerError, require
from .inventory import StaticInventory
from .diagnostic_mask import KnownJobSecrets,project_event
from .inventory_artifacts import read_artifact
from .ephemeral_input import EphemeralJobInput
from .parameters import parameters_digest, validate_parameters
from .probe import probe
from .protocol import decode, encode, uuid_field
from .sandbox import bubblewrap_argv, run_bounded
from .snapshot import verify_snapshot
from .store import atomic_json, read_json
from .validation_reports import read_report


class InteractiveExecutor:
    def __init__(self, helper, channel, store):
        self.helper, self.channel, self.store = helper, channel, store
        self.identity, self.grants = None, None
        self.lock = threading.RLock()
        self.pending = {}
        self.vault_is_locked = False
        self.vault_generation = 0
        # Conservatively serialize all interactive jobs, including check mode:
        # check_mode:false tasks can change targets too. Never overlap targets.
        self.worker = ThreadPoolExecutor(max_workers=1, thread_name_prefix='cc-job')

    @staticmethod
    def _configuration(value):
        require(set(value) == {'tenant_id', 'runner_id', 'signer_id', 'public_key', 'hosts', 'projects', 'expires_at', 'max_job_seconds', 'transport'}
                and value['transport'] == 'native_scoped_rpc_v1', 'interactive_config', 'Unsupported interactive scope')
        for name in ('tenant_id', 'runner_id', 'signer_id'): uuid_field(value[name], name)
        for name in ('hosts', 'projects'):
            # Empty sets permit reading this owner's old journal but grant no
            # executable project/target. Native target scopes remain separate.
            require(isinstance(value[name], list) and len(value[name]) <= 10000, 'interactive_config', 'Invalid interactive grant')
            for item in value[name]: uuid_field(item, name)
        try:
            public_key = base64.b64decode(value['public_key'], validate=True)
        except (TypeError, ValueError):
            raise RunnerError('interactive_key', 'Invalid interactive public key')
        require(len(public_key) == 32 and type(value['expires_at']) is int and
                time.time() < value['expires_at'] <= time.time() + 3600 and type(value['max_job_seconds']) is int and
                1 <= value['max_job_seconds'] <= 3600, 'interactive_config', 'Interactive grant exceeds one hour')
        return ({name: value[name] for name in ('tenant_id', 'runner_id')}, value['signer_id'],
                TrustGrant(public_key, value['tenant_id'], frozenset(value['hosts']), frozenset(value['projects']),
                           frozenset(['jobs.execute']), value['expires_at'], value['max_job_seconds']))

    def configure(self, value):
        identity, signer, grant = self._configuration(value)
        with self.lock:
            require(self.identity is None, 'interactive_config', 'Interactive trust cannot be replaced in an active session')
            self.identity, self.grants = identity, {signer: grant}
        return {'configured': True, 'persistent': False}

    def configure_manual_owner(self, value):
        identity, signer, expected = self._configuration(value)
        with self.lock:
            require(not self.vault_is_locked and self.identity == identity and self.grants is not None
                    and set(self.grants) == {signer}, 'interactive_config', 'Original manual owner scope required')
            current = self.grants[signer]
            require(original_manual_grant(current) == expected, 'interactive_config',
                    'Manual configuration cannot replace or renew enrollment')
            self.grants[signer] = configured_manual_owner(current)
        return {'configured': True, 'persistent': False, 'owner_policy': OWNER_POLICY}

    def accept(self, body):
        require(set(body) in ({'approval', 'revision_id', field, 'parameters'}|extra
            for field in ('inventory','inventory_ref') for extra in (set(),{'vault_generation'})), 'job_fields', 'Invalid job submission')
        require(self.identity is not None, 'interactive_config', 'Interactive scope must be configured first')
        require(not self.vault_is_locked, 'vault_locked', 'New jobs cannot be accepted while the vault is locked')
        environment = probe()
        require(environment['capabilities']['validate'] and environment['sandbox']['network'], 'execution_unavailable', 'Isolated execution environment is unavailable')
        verified = verify_envelope(body['approval'], self.grants, **self.identity, environment_digest=environment['environment_digest'])
        root, snapshot, runtime_inventory, parameters = self._checked_inputs(verified, body, environment)
        manifest = verified.value
        with self.lock:
            # Probe, signature and snapshot verification may take time. A vault
            # transition in that interval must invalidate this admission too.
            require(time.time() < min(manifest['expires_at'], manifest['access_expires_at']),
                    'approval_expired', 'Approval expired before durable admission')
            require(not self.vault_is_locked, 'vault_locked', 'New jobs cannot be accepted while the vault is locked')
            require(self.vault_generation == 0 or (type(body.get('vault_generation')) is int and
                    body['vault_generation'] == self.vault_generation), 'vault_generation', 'Vault generation changed before admission')
            require(len(self.pending) < 64, 'job_queue', 'Interactive job queue is full')
            state, created = self.store.accept(verified, revision_id=body['revision_id'], environment=environment)
            if created:
                cancel = threading.Event()
                self.pending[manifest['job_id']] = (cancel, verified)
                atomic_json(self.store.job_directory(manifest['job_id']) / 'approval.json', body['approval'])
                self.worker.submit(self.run, verified, root, snapshot, runtime_inventory, parameters, cancel)
            return {'created': created, 'job': state}

    def _checked_inputs(self, verified, body, environment):
        """Same immutable input/report checks for both normal and service starts.

        This returns data only. The caller must separately admit a genuinely
        signed job in its existing store under its current authority fences.
        """
        manifest = verified.value
        root, snapshot = self.helper.revision(body['revision_id'])
        inventory = read_artifact(self.helper.revisions,body['revision_id'],body['inventory_ref'])[0] if 'inventory_ref' in body else StaticInventory.from_snapshot(body['inventory'])
        parameters = validate_parameters(body['parameters'])
        if 'vault' in parameters:
            require(environment['capabilities'].get('vault_job_v1') is True,'vault_runtime','Update Runner for fixed Vault job input')
            reference=next((f for f in snapshot.files if f['path']=='.consolecrypt-secret-inputs.json'),None)
            require(reference is not None and reference['sha256']==parameters['vault']['reference_sha256']
                    and environment['environment_digest']==parameters['vault']['environment_digest']
                    and environment['helper_digest']==parameters['vault']['helper_digest'],
                    'vault_binding','Signed Vault reference/runtime differs')
        require(snapshot.digest == manifest['revision_digest'] and inventory.digest == manifest['inventory_digest'] and
                parameters_digest(parameters) == manifest['parameters_digest'], 'job_changed', 'Job inputs differ from approved inputs')
        runtime_inventory = inventory.runtime_inventory(manifest)
        validation_inputs = dict(manifest, targets_digest=hashlib.sha256(encode(manifest['targets'])).hexdigest())
        report = read_report(root.parent, validation_inputs)
        require(report.get('valid') is True and all(report.get(key) == manifest[key] for key in
                ('revision_digest', 'environment_digest', 'parameters_digest', 'inventory_digest', 'playbook')) and
                report.get('targets_digest') == hashlib.sha256(encode(manifest['targets'])).hexdigest(),
                'validation_changed', 'Complete checks of this exact playbook, options and targets are required')
        verify_snapshot(root, snapshot)
        return root, snapshot, runtime_inventory, parameters

    def cancel(self, job_id):
        uuid_field(job_id, 'job ID')
        with self.lock:
            pending = self.pending.get(job_id)
            if pending:
                pending[0].set()
                self.channel.revoke(pending[1])
                current = self.store.get(job_id)
                if current['state'] == 'running': self.store.transition(job_id, 'cancelling')
        return self.store.get(job_id)

    def check_owner(self, job_id):
        require(self.identity is not None, 'interactive_config', 'An interactive identity is required')
        manifest = read_json(self.store.job_directory(uuid_field(job_id, 'job ID')) / 'manifest.json')
        require(all(manifest[key] == value for key, value in self.identity.items()), 'job_identity', 'Job belongs to another profile or runner')

    def vault_state(self, locked, generation=None):
        require(type(locked) is bool, 'vault_state', 'Vault state must be a boolean')
        with self.lock:
            if generation is None:
                require(self.vault_generation == 0, 'vault_generation', 'Sequenced vault state is required')
            else:
                require(type(generation) is int and 0 < generation < 2**64,
                        'vault_generation', 'Invalid vault generation')
                if generation < self.vault_generation:
                    return {'locked': self.vault_is_locked, 'generation': self.vault_generation}
                require(generation != self.vault_generation or self.vault_is_locked == locked,
                        'vault_generation', 'Conflicting vault state for the same generation')
                self.vault_generation = generation
            self.vault_is_locked = locked
            if locked:
                for cancel, verified in self.pending.values():
                    if verified.value['policy']['cancel_on_lock']:
                        cancel.set()
                        self.channel.revoke(verified)
            return {'locked': locked, 'generation': self.vault_generation}

    def run(self, verified, root, snapshot, inventory, parameters, cancel):
        manifest, summary = verified.value, None
        job_id = manifest['job_id']
        buffer, discarding = bytearray(), False
        diagnostic_mask=KnownJobSecrets(parameters)
        def chunks(chunk):
            nonlocal summary, discarding
            for piece in chunk.splitlines(keepends=True):
                if not discarding: buffer.extend(piece)
                if len(buffer) > 1024 * 1024:
                    buffer.clear(); discarding = True
                if piece.endswith(b'\n'):
                    if not discarding:
                        try:
                            event = decode(bytes(buffer))
                            if isinstance(event, dict) and set(event) == {'type', 'payload'} and isinstance(event['type'], str) and isinstance(event['payload'], dict):
                                kind,payload=project_event(self.channel,verified,event['type'],event['payload'],diagnostic_mask,cancel=cancel)
                                if kind == 'runner_summary': summary = payload
                                self.store.append_event(job_id, kind, payload)
                        except RunnerError:
                            pass  # Guest stdout is data, never a control frame.
                    buffer.clear(); discarding = False
        result = None  # No interval exists before an actual child result.
        try:
            if cancel.is_set():
                self.store.transition(job_id, 'cancelled', result={'started': False})
                return
            require(time.time() < manifest['access_expires_at'], 'access_expired', 'Queued job access expired')
            current_environment = probe()
            require(current_environment['environment_digest'] == manifest['environment_digest'], 'environment_changed', 'Environment changed while job was queued')
            verify_snapshot(root, snapshot)
            directory = self.store.job_directory(job_id)
            input_body = encode({'playbook': manifest['playbook'], 'mode': manifest['mode'], 'forks': manifest['policy']['forks'],
                                 'inventory': inventory, 'parameters': parameters,
                                 **({'vault_execution':True} if 'vault' in parameters else {})})
            with ExitStack() as execution_scope:
                inputs = execution_scope.enter_context(EphemeralJobInput(input_body))
                del input_body
                self.store.transition(job_id, 'running')
                execution_scope.enter_context(JobBroker(directory / 'transport', verified, self.channel, cancel))
                vault=None
                if 'vault' in parameters:
                    from .vault_sidecar import PinnedVaultSidecar
                    vault=execution_scope.enter_context(PinnedVaultSidecar(self.helper.root,parameters['vault'],'execution',snapshot.digest,verified.digest))
                result = run_bounded(bubblewrap_argv(root, [sys.executable, '-m', 'consolecrypt_runner.execution_worker'],
                    socket_directory=directory / 'transport',vault_channel=vault,job_input=inputs),
                    timeout=min(manifest['policy']['timeout_seconds'], max(1, manifest['access_expires_at'] - time.time())),
                    cancel=cancel, output_limit=1024 * 1024, on_chunk=chunks,vault_channel=vault,job_input=inputs)
            verify_snapshot(root, snapshot)
            known = summary is not None and type(summary.get('exit_code')) is int and summary['exit_code'] == result.exit_code
            outcome = {'exit_code': result.exit_code, 'summary': summary, 'timed_out': result.timed_out,
                       'cancel_requested': result.cancelled, 'remote_changes_may_be_partial': result.cancelled or result.timed_out or not known,
                       'elapsed_ms': result.elapsed_ms}
            interval = result.execution_interval()
            if interval is not None:
                outcome['runner_execution'] = interval
            if result.cancelled or result.timed_out:
                current = self.store.get(job_id)
                if current['state'] == 'running': self.store.transition(job_id, 'cancelling')
                self.store.transition(job_id, 'unknown', result=outcome)
            elif not known:
                self.store.transition(job_id, 'unknown', result=outcome)
            else:
                succeeded = result.exit_code == 0 and summary.get('status') == 'successful'
                self.store.transition(job_id, 'succeeded' if succeeded else 'failed', result=outcome)
        except Exception:
            current = self.store.get(job_id)
            if current['state'] == 'queued': self.store.transition(job_id, 'interrupted', result={'reason': 'preflight_changed'})
            elif current['state'] in ('running', 'cancelling'):
                failure = {'reason': 'executor_lost', 'remote_changes_may_be_partial': True}
                if result is not None:
                    # Post-run custody checks can fail after a joined child.
                    # Retain ONLY actual process observations, never infer
                    # cancellation/timeout from a later request or exception.
                    failure['cancel_requested'] = result.cancelled
                    failure['timed_out'] = result.timed_out
                interval = result.execution_interval() if result is not None else None
                if interval is not None:
                    failure['runner_execution'] = interval
                self.store.transition(job_id, 'unknown', result=failure)
        finally:
            diagnostic_mask.close()
            with self.lock: self.pending.pop(job_id, None)

    def close(self):
        with self.lock:
            for cancel, verified in self.pending.values():
                cancel.set()
                self.channel.revoke(verified)
        self.worker.shutdown(wait=True, cancel_futures=False)
