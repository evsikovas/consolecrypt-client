"""Fixed validation-only process: no jobs, targets, resolver or network worker."""
from contextlib import contextmanager
from pathlib import Path
import os,shlex,sys,tempfile,threading,time
from .dependency_helper import DependencyHelper,READ,UPLOAD
from .errors import require
from .probe import probe
from .protocol import encode
from .inventory import StaticInventory
from .parameters import validate_parameters
from .validation import validate_project
from .store import atomic_json
from .vault_validation_channel import VaultValidationChannel
from .vault_validation_broker import VaultValidationBroker,_capsule
from .vault_ephemeral import reference_matches
from . import dependency_retention as retention
ALLOWED=READ|UPLOAD|{'probe','environment_probe','revision_release','validation_prepare','validation_dispose',
    'vault_sidecar_begin','vault_sidecar_finish_validation','vault_sidecar_material'}
class _PhaseCancel:
    """Per-phase close cannot cancel another phase; parent cancellation still
    immediately revokes every active phase. No password/material is cached."""
    def __init__(self,parent):self.parent=parent;self.local=threading.Event()
    def is_set(self):return self.local.is_set() or self.parent.is_set()
    def set(self):self.local.set()
    def wait(self,timeout=None):
        deadline=time.monotonic()+timeout if timeout is not None else None
        while not self.is_set():
            if deadline is not None and time.monotonic()>=deadline:return False
            self.local.wait(.05 if deadline is None else min(.05,max(0,deadline-time.monotonic())))
        return True

class ValidationHelper(DependencyHelper):
    dispose_operation='validation_dispose'
    def __init__(self,configured_root,operation_id):
        self.private_control=VaultValidationChannel()
        self._sidecar_lock=threading.RLock()
        self._sidecar_brokers={}
        self._sidecar_binding=None
        self._sidecar_snapshot=None
        self._sidecar_validation_finished=False
        self._sidecar_material_started=False
        super().__init__(configured_root,operation_id,_purpose='vault_validation')
    def close(self,*,dispose=False):
        self.private_control.close()
        self._close_sidecar_brokers()
        super().close(dispose=dispose)
    def _close_sidecar_brokers(self,phases=None):
        with self._sidecar_lock:
            names=list(self._sidecar_brokers) if phases is None else list(phases)
            brokers=[self._sidecar_brokers.pop(name) for name in names if name in self._sidecar_brokers]
        # Never join a handler/server under its state lock. Socket/script files
        # remain in our bounded operation until EXISTING fd-safe disposal.
        for broker in brokers:broker.close()
    def _dispose(self):
        self._close_sidecar_brokers()
        return super()._dispose()
    def _current(self):
        require(retention.identity(self._fd)==self._identity,'vault_identity','Private operation changed')
        retention._same_entry(self._family_fd,self.operation_id,self._identity)
        retention._same_entry(self._fd,'data',self._data_identity)
    def request(self,operation,body,cancel):
        require(operation in ALLOWED,'unsupported_operation','Operation unavailable in validation mode')
        if operation in ('vault_sidecar_begin','vault_sidecar_finish_validation','vault_sidecar_material'):
            return self._sidecar(operation,body,cancel)
        if operation=='validation_dispose':
            require(set(body)=={'operation_id','root_family'} and body['operation_id']==self.operation_id and body['root_family']==self.root_family,
                    'vault_identity','Dispose only current validation operation')
            return self._dispose()
        if operation=='environment_probe':
            require(not body,'request_fields','Environment probe takes no arguments');self._current();return probe()
        if operation!='validation_prepare':return super().request(operation,body,cancel)
        require(set(body)=={'revision_id','playbook','parameters','inventory','targets','vault_children'} and len(encode(body))<=512*1024,
                'validation_fields','Invalid private Vault validation request')
        with self._prepare_lock:
            require(not self._prepare_started,'validation_once','Validation requires a fresh operation');self._prepare_started=True
        root,snapshot=self.revision(body['revision_id']);self._current()
        children=body['vault_children'];require(isinstance(children,list) and len(children)==4,'vault_phase','Four distinct child grants required')
        children=list(map(_capsule,children));first=children[0]
        require([(r['phase'],r['call_index']) for r in children]==[('syntax',0),('syntax',1),('lint',0),('lint',1)] and
                len({r['phase_nonce'] for r in children})==4 and all(r['scope_id']==first['scope_id'] and
                r['revision_digest']==snapshot.digest and r['reference_sha256']==first['reference_sha256'] and
                r['helper_digest']==first['helper_digest'] and r['environment_digest']==first['environment_digest'] for r in children),
                'vault_phase','Vault grants differ from the frozen revision')
        require(reference_matches(snapshot,first['reference_sha256']),'vault_phase','Vault reference differs')
        report=probe();require(report['environment_digest']==first['environment_digest'] and report['helper_digest']==first['helper_digest'],
                'vault_runtime','Vault runtime changed')
        @contextmanager
        def phase_factory(phase):
            self._current()
            with tempfile.TemporaryDirectory(prefix='vault-phase-',dir=self.root) as temporary:
                directory=Path(temporary);directory.chmod(0o700)
                wrapper=directory/'password-client';wrapper.write_text('#!/bin/sh\nexec '+shlex.quote(sys.executable)+' -I /tool/python/consolecrypt_runner/vault_password_client.py\n');wrapper.chmod(0o500)
                helper=self
                class Checked:
                    def call(self,capsule,*,cancel,timeout):
                        helper._current()
                        from .snapshot import verify_snapshot
                        verify_snapshot(root,snapshot)
                        return helper.private_control.call(capsule,cancel=cancel,timeout=timeout)
                    def revoke(self):helper.private_control.revoke()
                with VaultValidationBroker(directory/'socket',[r for r in children if r['phase']==phase],Checked(),_PhaseCancel(cancel)) as broker:
                    broker.client_script=wrapper;yield broker
        inventory=StaticInventory.from_snapshot(body['inventory'])
        report=validate_project(root,snapshot,body['playbook'],cancel=cancel,parameters=body['parameters'],inventory=inventory,
            targets=body['targets'],vault_phase_factory=phase_factory,private_directory=self.root)
        # NEVER write raw potentially decrypted diagnostics to a report file.
        result=report.to_json();result['input_witnesses']={
            'parameters':{'digest':report.parameters_digest,'canonical_json':encode(validate_parameters(body['parameters'])).decode()},
            'inventory':{'digest':report.inventory_digest,'canonical_json':encode(inventory.to_snapshot()).decode()}}
        return result

    def _open_sidecar_broker(self,phase,capsules,cancel,manifest_digest=None):
        from .vault_sidecar import trusted_client_source
        from .vault_job_broker import VaultJobBroker
        import hashlib
        directory=self.root/('vault-'+phase)
        directory.mkdir(mode=0o700) # fixed purpose, exclusive and never reopened
        script=trusted_client_source(phase)
        fd=os.open(directory/'password-client',os.O_WRONLY|os.O_CREAT|os.O_EXCL|os.O_NOFOLLOW|os.O_CLOEXEC,0o500)
        with os.fdopen(fd,'wb') as output:
            output.write(script);output.flush();os.fsync(output.fileno())
        root,snapshot=self._sidecar_snapshot
        helper=self
        class Checked:
            def _check(self):
                from .snapshot import verify_snapshot
                helper._current();verify_snapshot(root,snapshot)
                report=probe()
                require(report['environment_digest']==helper._sidecar_binding['environment_digest']
                        and report['helper_digest']==helper._sidecar_binding['helper_digest'],
                        'vault_runtime','Vault runtime changed')
            def call(self,capsule,*,cancel,timeout):
                self._check();return helper.private_control.call(capsule,cancel=cancel,timeout=timeout)
            def call_job(self,capsule,*,cancel,timeout):
                self._check();return helper.private_control.call_job(capsule,cancel=cancel,timeout=timeout)
            def revoke(self):helper.private_control.revoke()
        scoped=_PhaseCancel(cancel)
        broker=(VaultJobBroker(directory/'socket',capsules[0],Checked(),scoped,keep_owned_directory=True)
            if phase=='execution' else VaultValidationBroker(directory/'socket',capsules,Checked(),scoped,keep_owned_directory=True))
        try:
            atomic_json(directory/'phase.json',{'version':1,'phase':phase,'binding':self._sidecar_binding,
                'revision_digest':snapshot.digest,'manifest_digest':manifest_digest,'client_sha256':hashlib.sha256(script).hexdigest()})
            self._current()
            self._sidecar_brokers[phase]=broker
        except BaseException:
            broker.close();raise

    def _sidecar(self,operation,body,cancel):
        with self._sidecar_lock:
            self._current()
            if operation=='vault_sidecar_begin':
                require(isinstance(body,dict) and set(body)=={'revision_id','binding','children'} and len(encode(body))<=8192,
                        'vault_sidecar','Invalid private validation sidecar')
                with self._prepare_lock:
                    require(not self._prepare_started,'validation_once','Use a new private operation')
                    self._prepare_started=True
                binding=validate_parameters({'extra_vars':{},'tags':[],'skip_tags':[],'diff':False,'vault':body['binding']})['vault']
                require(binding['operation_id']==self.operation_id,'vault_sidecar','Private operation differs')
                root,snapshot=self.revision(body['revision_id'])
                children=body['children']
                require(isinstance(children,list) and len(children)==4,'vault_sidecar','Four validation grants required')
                children=list(map(_capsule,children))
                require([(c['phase'],c['call_index']) for c in children]==[('syntax',0),('syntax',1),('lint',0),('lint',1)]
                        and len({c['phase_nonce'] for c in children})==4
                        and all(c['scope_id']==binding['scope_id'] and c['revision_digest']==snapshot.digest and
                            all(c[k]==binding[k] for k in ('reference_sha256','environment_digest','helper_digest')) for c in children),
                        'vault_sidecar','Validation grants differ from signed metadata')
                require(reference_matches(snapshot,binding['reference_sha256']),
                        'vault_sidecar','Snapshot reference differs')
                self._sidecar_binding=binding;self._sidecar_snapshot=(root,snapshot)
                self._open_sidecar_broker('syntax',children[:2],cancel)
                self._open_sidecar_broker('lint',children[2:],cancel)
                return {'version':1,'operation_id':self.operation_id,'phases':['syntax','lint']}
            if operation=='vault_sidecar_finish_validation':
                require(not body and self._sidecar_binding is not None and not self._sidecar_validation_finished,
                        'vault_sidecar','Validation sidecar already retired')
                self._sidecar_validation_finished=True
                # _close_sidecar_brokers reacquires only this RLock; close calls
                # must happen outside it, so take the exact two owners first.
                brokers=[self._sidecar_brokers.pop(p) for p in ('syntax','lint') if p in self._sidecar_brokers]
            else:
                from .vault_job_broker import job_capsule
                require(isinstance(body,dict) and set(body)=={'capsule'} and self._sidecar_validation_finished
                        and not self._sidecar_material_started,'vault_sidecar','Material sidecar unavailable')
                capsule=job_capsule(body['capsule']);snapshot=self._sidecar_snapshot[1]
                require(capsule['reference_sha256']==self._sidecar_binding['reference_sha256']
                        and capsule['revision_digest']==snapshot.digest,'vault_sidecar','Material capsule differs')
                self._sidecar_material_started=True
                self._open_sidecar_broker('execution',[capsule],cancel,capsule['manifest_digest'])
                return {'version':1,'operation_id':self.operation_id,'phase':'execution'}
        for broker in brokers:broker.close()
        return {'version':1,'operation_id':self.operation_id,'validation_closed':True}
