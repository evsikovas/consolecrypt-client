"""Trusted control channel. Guest stdout is never interpreted as these frames."""
from dataclasses import dataclass, field
import threading
import time
import uuid

from .approval import VerifiedManifest
from .errors import RunnerError, require
from .protocol import uuid_field


@dataclass
class Pending:
    job_id: str
    diagnostic: bool = False
    ready: threading.Event = field(default_factory=threading.Event)
    result: object = None
    error: object = None


class TargetChannel:
    def __init__(self, send):
        self.send, self.lock = send, threading.Lock()
        self.pending, self.closed = {}, False

    def close(self):
        with self.lock:
            self.closed = True
            for pending in self.pending.values():
                pending.error = RunnerError('connection_lost', 'Native target channel closed')
                pending.ready.set()

    def deliver(self, response):
        require(set(response) == {'version', 'type', 'request_id', 'result', 'error'} and
                type(response['version']) is int and response['version'] == 1 and response['type'] == 'target_response',
                'target_response', 'Invalid native target response')
        request_id = uuid_field(response['request_id'], 'target request ID')
        error = response['error']
        if error is not None:
            require(isinstance(error, dict) and set(error) == {'code', 'message'} and
                    isinstance(error['code'], str) and 0 < len(error['code']) <= 80 and
                    all(c in 'abcdefghijklmnopqrstuvwxyz0123456789_' for c in error['code']) and
                    isinstance(error['message'], str) and len(error['message']) <= 4096,
                    'target_response', 'Invalid native target error')
        with self.lock:
            pending = self.pending.get(request_id)
            if pending is not None:
                require(not pending.ready.is_set(), 'target_response', 'Duplicate native response')
                pending.result = response['result']
                pending.error = RunnerError(error['code'], 'Native scoped operation failed') if error else None
                pending.ready.set()

    def revoke(self, verified: VerifiedManifest):
        manifest = verified.value
        try:
            if not self.closed:
                self.send({'version': 1, 'type': 'target_revoke', 'job_id': manifest['job_id'], 'manifest_digest': verified.digest})
        except OSError:
            self.close()
        with self.lock:
            for pending in self.pending.values():
                if pending.job_id == manifest['job_id']:
                    pending.error = RunnerError('cancelled', 'Job access revoked')
                    pending.ready.set()

    def call(self, verified: VerifiedManifest, host_id, operation, body, *, cancel=None, timeout=120):
        # Guest broker stays closed to exactly its existing executable ops.
        require(operation in ('exec','put_chunk','get_chunk','file_close'),'target_operation','Unsupported native operation')
        return self._exchange(verified,host_id,operation,body,cancel=cancel,timeout=timeout,revoke_on_timeout=True)

    def _redact_event(self, verified, event_type, payload, *, cancel=None, timeout=5):
        # Trusted Helper parent ONLY. This method/response is never routed to
        # JobBroker, guest sockets, namespace mounts or inherited guest FDs.
        targets=verified.value['targets']
        require(targets and type(payload) is dict,'privacy_scope','Original job diagnostics required')
        return self._exchange(verified,targets[0]['host_id'],'redact_event',
            {'event_type':event_type,'payload':payload},cancel=cancel,timeout=min(timeout,5),revoke_on_timeout=False)

    def _exchange(self, verified, host_id, operation, body, *, cancel, timeout, revoke_on_timeout):
        manifest = verified.value
        uuid_field(host_id, 'target host ID')
        require(host_id in {target['host_id'] for target in manifest['targets']}, 'target_denied', 'Host is outside the approved job')
        require(time.time() < manifest['access_expires_at'], 'approval_expired', 'Job access expired')
        require(operation in ('exec', 'put_chunk', 'get_chunk', 'file_close','redact_event') and isinstance(body, dict),
                'target_operation', 'Unsupported native operation')
        require(not cancel or not cancel.is_set(), 'cancelled', 'Job cancellation requested')
        diagnostic=operation=='redact_event'
        pending = Pending(manifest['job_id'],diagnostic=diagnostic)
        request_id = str(uuid.uuid4())
        with self.lock:
            # One diagnostic slot cannot consume the original five executable slots.
            count=sum(p.diagnostic==diagnostic for p in self.pending.values())
            require(not self.closed and count < (1 if diagnostic else 5), 'target_busy', 'Native target channel is closed or busy')
            self.pending[request_id] = pending
        try:
            self.send({'version': 1, 'type': 'target_request', 'request_id': request_id,
                       'job_id': manifest['job_id'], 'manifest_digest': verified.digest,
                       'host_id': host_id, 'operation': operation, 'body': body})
            deadline = time.monotonic() + min(timeout, manifest['policy']['timeout_seconds'],
                                              max(0, manifest['access_expires_at'] - time.time()))
            while not pending.ready.wait(.1):
                if (cancel and cancel.is_set()) or time.monotonic() >= deadline:
                    if revoke_on_timeout:self.revoke(verified)
                    raise RunnerError('target_unknown', 'Native operation was cancelled or timed out; remote result may be unknown')
            if pending.error:
                raise pending.error
            return pending.result
        finally:
            with self.lock:
                self.pending.pop(request_id, None)
