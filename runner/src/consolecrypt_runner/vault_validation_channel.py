"""Dedicated trusted control stdin only. Never parse guest stdout as authority.
No default Helper reader installs this channel; validation-only mode owns it.
"""
from dataclasses import dataclass,field
import threading,time,uuid
from .errors import RunnerError,require
from .protocol import uuid_field,encode
from .vault_validation_broker import _capsule,checked_password_response
@dataclass
class Pending:
    ready:threading.Event=field(default_factory=threading.Event)
    result:object=field(default=None,repr=False)
    error:object=field(default=None,repr=False)
    kind:str='vault_validation_response'
class VaultValidationChannel:
    def __init__(self):self._send=None;self.lock=threading.Lock();self.pending={};self.closed=False
    def attach_sender(self,send):
        with self.lock:
            require(self._send is None and not self.closed,'vault_channel','Private control already attached')
            self._send=send
    def deliver(self,response):
        require(isinstance(response,dict) and set(response)=={'version','type','request_id','result','error'} and
                type(response['version']) is int and response['version']==1 and response['type'] in ('vault_validation_response','vault_job_response') and
                len(encode(response))<=8192,'vault_control','Invalid private Vault response')
        identity=uuid_field(response['request_id'],'request ID')
        with self.lock:
            pending=self.pending.get(identity)
            require(pending is not None and not pending.ready.is_set() and pending.kind==response['type'],'vault_control','Unknown or repeated private Vault response')
            if response['error'] is not None:
                # Raw native diagnostic/body is never forwarded or echoed.
                require(isinstance(response['error'],dict) and set(response['error'])=={'code','message'},'vault_control','Invalid private Vault error')
                pending.error=RunnerError('vault_unavailable','Scoped Vault input unavailable')
            else:pending.result=checked_password_response(response['result'])
            pending.ready.set()
    def call(self,capsule,*,cancel,timeout):
        return self._call(_capsule(capsule),'vault_validation_request','vault_validation_response',cancel,timeout)
    def call_job(self,capsule,*,cancel,timeout):
        from .vault_job_broker import job_capsule
        return self._call(job_capsule(capsule),'vault_job_request','vault_job_response',cancel,timeout)
    def _call(self,request,kind,response_kind,cancel,timeout):
        identity=str(uuid.uuid4());pending=Pending(kind=response_kind)
        with self.lock:
            require(not self.closed and self._send is not None and not self.pending,'vault_channel','Private control closed or busy')
            self.pending[identity]=pending;send=self._send
        try:
            send({'version':1,'type':kind,'request_id':identity,'body':request})
            deadline=time.monotonic()+min(timeout,5)
            while not pending.ready.wait(.05):
                require(not cancel.is_set() and time.monotonic()<deadline,'vault_unavailable','Scoped Vault input unavailable')
            if pending.error:raise pending.error
            require(not cancel.is_set(),'vault_unavailable','Scoped Vault input unavailable')
            return pending.result
        finally:
            with self.lock:self.pending.pop(identity,None)
    def revoke(self):
        # Per-phase broker close must not revoke a later phase. Wake current
        # pending call only; Native original scope +one-use grants are authority.
        with self.lock:
            for pending in self.pending.values():
                pending.error=RunnerError('cancelled','Scoped Vault input unavailable');pending.ready.set()
    def close(self):
        with self.lock:self.closed=True
        self.revoke()
