"""Private service activation; SAME custody JobStore and default executor.

No public listener, password resolver or alternate execution/report path. The
Rust supervisor owns recipient keys and real JobTargetScope/TargetHandler.
"""
import hashlib
import threading
import time
import uuid
from pathlib import Path

from consolecrypt_runner.approval import TrustGrant, verify_envelope
from consolecrypt_runner.errors import require, RunnerError
from consolecrypt_runner.execution import InteractiveExecutor
from consolecrypt_runner.probe import probe, environment_receipt
from consolecrypt_runner.protocol import encode
from consolecrypt_runner.store import atomic_json
from consolecrypt_runner.target_channel import TargetChannel
from .cc_service_queue.queue import identifier, record
from .service_authority_v4 import verify_delegation
from .service_custody import ServiceCustody

TEMPLATE='service-template.json'
START='service-start.json'

def validate_template_record(root,owner,service_id):
    """Readonly supported-schema/signature gate BEFORE JobStore recovery writes.

    Verification at ORIGINAL issue time permits reading expired history; it is
    never current dispatch authority. Each start uses _policy/current floors.
    """
    from consolecrypt_runner.inventory import StaticInventory
    from consolecrypt_runner.parameters import validate_parameters,parameters_digest
    return validate_template_value(record(root/TEMPLATE,6*1024*1024),owner,service_id)

def validate_template_value(value,owner,service_id):
    # Same strict validator for descriptor-bound retirement and normal reads.
    from consolecrypt_runner.inventory import StaticInventory
    from consolecrypt_runner.parameters import validate_parameters,parameters_digest
    if type(value) is dict and value.get('version')==2:
        from .service_retention import validate_tombstone
        return validate_tombstone(value,owner,service_id)
    require(type(value) is dict and set(value)=={'version','delegation','delegationDigest','revisionId',
        'revisionDigest','inventory','parameters','targets'} and type(value['version']) is int and value['version']==1,
        'service_schema','Unsupported retained service template')
    identifier(value['revisionId'])
    issued=value.get('delegation',{}).get('body',{}).get('issued_at')
    require(type(issued) is int,'service_schema','Unsupported retained service template')
    policy=verify_delegation(encode(value['delegation']),owner,issued);body=policy.body
    require(body.get('execution_origin') is not None and body['service_id']==service_id
        and body['tenant']==dict(owner.tenant) and policy.digest==value['delegationDigest']
        and body['template_digest']==value['revisionDigest'],'service_schema','Retained template differs from owner policy')
    inventory=StaticInventory.from_snapshot(value['inventory']);parameters=validate_parameters(value['parameters'])
    targets=value['targets']
    require('vault' not in parameters and inventory.digest==body['inventory_digest']
        and parameters_digest(parameters)==body['parameters_digest'] and type(targets) is list
        and len(targets)==len(body['hosts']) and len({t['host_id'] for t in targets})==len(targets)
        and {t['host_id']:hashlib.sha256(encode(t)).hexdigest() for t in targets}=={h['id']:h['target_digest'] for h in body['hosts']},
        'service_schema','Retained template inputs differ from owner policy')
    return value

class ServiceExecutionRuntime:
    def __init__(self, custody, helper, channel, original_origin):
        require(type(custody) is ServiceCustody and helper.store is custody.store
            and helper.root == custody.root and type(channel) is TargetChannel,
            'service_scope','Original custody helper and target channel required')
        # Immutable copy; Native already observed and explicitly reviewed this
        # exact enrolled service identity. Rust independently derives it too.
        self.origin=encode(original_origin)
        self.custody,self.helper=custody,helper
        self.executor=InteractiveExecutor(helper,channel,custody.store)
        self.executor.identity={'runner_id':custody.service_id,'tenant_id':custody.tenant['id']}
        self.closed=False
        original=custody.root.lstat()
        self._retention_runtime=(original.st_dev,original.st_ino)
        self.clock_epoch=str(uuid.uuid4())
        self.reservation=None # One actual selected schedule, not a replay map.

    def due(self,signed,scheduler):
        from .scheduler import Clock,Status
        policy,_=self._policy(signed);body=policy.body
        with self.custody.store._mutex:
            header=self.custody._validate_header(record(self.custody.root/'service-version.json',8192))
            current=int(self.custody.clock())
            require(not self.closed and not header['revoked'] and current>=header['clockFloor']
                and body['policy_revision']>=header['policyFloor'] and body['revocation_epoch']>=header['revocationFloor'],
                'service_revoked','Current service policy unavailable')
            require(self.reservation is None,'service_slot_unresolved','Original slot still requires a terminal result')
            retained=self._active_template()
            require(retained['delegationDigest']==policy.digest,'service_enrollment_changed','Original template required')
            tick=scheduler.tick(body['schedule_id'],body['schedule_revision'],
                Clock(current,time.monotonic_ns(),self.clock_epoch))
            if tick.status is not Status.RESERVED:return {'status':tick.status.value,'slot':None,'targets':None}
            self.reservation=tick.reservation
            original=scheduler.unresolved(body['schedule_id'],body['schedule_revision'])
            require(original is not None and original.definition_digest==body['definition_digest'],
                'service_schedule_changed','Original definition changed')
            # Only data leaves. Actual one-use token stays in this original
            # runtime; a restart can observe but cannot reconstruct/release it.
            definition=next(v.definition for v in scheduler.schedules() if v.schedule_id==body['schedule_id'])
            return {'status':'reserved','slot':{'schedule_id':body['schedule_id'],'schedule_revision':original.original_revision,
                'definition_digest':original.definition_digest,'local_slot':original.local_slot,'due_utc':original.due_utc,
                'grace_seconds':definition.grace_seconds},'targets':retained['targets']}

    def finish_slot(self,job_id,scheduler):
        identifier(job_id)
        with self.custody.store._mutex:
            require(self.reservation is not None,'service_slot_unresolved','No original live reservation')
            state=self.custody.store.get(job_id)
            start=record(self.custody.store.job_directory(job_id)/START,8192)
            require(start.get('targetOutcome')=='known','service_reconciliation_required','Target outcome remains unconfirmed')
            binding=record(self.custody.store.job_directory(job_id)/'service-custody.json',8192)
            r=self.reservation
            require(binding['scheduleId']==r.schedule_id and binding['localSlot']==r.local_slot
                and binding['scheduleRevision']==r.schedule_revision and binding['definitionDigest']==r.definition_digest,
                'service_slot','Terminal job differs from original live slot')
            known=state['state'] in ('succeeded','failed') and type(state['result']) is dict
            if known:
                result=state['result']
                known=result.get('remote_changes_may_be_partial') is False and result.get('timed_out') is False
                known=known and result.get('cancel_requested') is False and type(result.get('summary')) is dict
            not_started=state['state']=='cancelled' and state['result']=={'started':False}
            require(known or not_started,'service_reconciliation_required','Unknown/partial result cannot release a schedule')
            scheduler.resolve(r,'finished' if known else 'not_started')
            self.reservation=None
            return {'resolved':True,'jobId':job_id}

    def observe_target_end(self,job_id,manifest_digest,outcome):
        """Original Rust TargetHandler reports its own closed scope, not UI.

        This is outcome DATA and can never start/retry/reconstruct a job grant.
        An uncertain/abandoned scope is sticky and leaves the slot unresolved.
        """
        identifier(job_id)
        self.executor.check_owner(job_id)
        require(outcome in ('known','uncertain'),'service_fields','Invalid target observation')
        with self.custody.store._mutex:
            path=self.custody.store.job_directory(job_id)/START;value=record(path,8192)
            state=self.custody.store.get(job_id)
            require(state['state'] not in ('queued','running','cancelling') and state['manifest_digest']==manifest_digest
                and value['manifestDigest']==manifest_digest,'service_scope','Original terminal job required')
            old=value['targetOutcome']
            require(old=='pending' or old==outcome,'service_reconciliation_required','Target uncertainty cannot be cleared')
            value['targetOutcome']=outcome;atomic_json(path,value,max_bytes=8192)
            return {'recorded':True,'targetOutcome':outcome}

    def job_status(self,job_id):
        identifier(job_id)
        self.executor.check_owner(job_id)
        with self.custody.store._mutex:
            state=self.custody.store.get(job_id);start=record(self.custody.store.job_directory(job_id)/START,8192)
            view=dict(state,runner_state=state['state'],target_outcome=start['targetOutcome'])
            if state['state'] in ('succeeded','failed','cancelled','interrupted','unknown') and start['targetOutcome']!='known':
                view['state']='unknown'
            return view

    def _active_template(self):
        retained=validate_template_record(self.custody.root,self.custody.owner,self.custody.service_id)
        require(retained['version']==1,'service_template_retired','Retired inputs cannot resume or enroll')
        return retained

    def _policy(self, signed):
        require(type(signed) is dict,'service_policy','Signed policy object required')
        policy=verify_delegation(encode(signed),self.custody.owner,int(self.custody.clock()))
        body=policy.body
        require(body.get('execution_origin') is not None and encode(body['execution_origin'])==self.origin
            and body['service_id']==self.custody.service_id and body['tenant']==self.custody.tenant,
            'service_origin','Explicit original service-local review required')
        environment=probe()
        require(environment['system']=='Linux' and environment['non_root'] is True
            and environment['environment_digest']==body['execution_origin']['environment_digest']
            and environment['helper_digest']==body['execution_origin']['helper_digest']
            and environment['capabilities'].get('trusted_worker_bootstrap_v1') is True
            and environment['capabilities']['validate'] and environment['sandbox']['network'],
            'service_origin','Service runtime differs from original review')
        return policy,environment

    def enroll_template(self, signed, revision_id, inventory, parameters):
        """Pin the already transferred/validated immutable template BEFORE ACK.

        The existing upload/revision repository is used. This never copies a
        source or imports a path. One root has one explicit enrollment; changes
        need a new reviewed root rather than replacing an old template.
        """
        require(not self.closed,'service_closed','Service runtime closed')
        policy,environment=self._policy(signed)
        identifier(revision_id)
        # Verify through the same complete report reader used by normal jobs.
        from consolecrypt_runner.inventory import StaticInventory
        from consolecrypt_runner.parameters import validate_parameters,parameters_digest
        from consolecrypt_runner.snapshot import verify_snapshot
        from consolecrypt_runner.validation_reports import read_report
        root,snapshot=self.helper.revision(revision_id)
        parsed=StaticInventory.from_snapshot(inventory);checked=validate_parameters(parameters)
        require('vault' not in checked,'service_vault_unavailable','Service Vault execution is not provisioned')
        body=policy.body
        require(snapshot.digest==body['template_digest'] and parsed.digest==body['inventory_digest']
            and parameters_digest(checked)==body['parameters_digest'],
            'service_template_changed','Template differs from approved policy')
        # Targets are stored only after their digest matches every owner-signed
        # host. They originate from the actual service-purpose Native review.
        report_inputs={'revision_digest':snapshot.digest,'environment_digest':body['environment_digest'],
            'playbook':body['playbook'],'parameters_digest':body['parameters_digest'],
            'inventory_digest':body['inventory_digest']}
        # Exact target list is enrolled separately with the original policy.
        return policy,root,snapshot,parsed,checked,report_inputs,environment

    def enroll(self, signed, revision_id, inventory, parameters, targets):
        with self.helper.revisions.lock:
            from consolecrypt_runner.snapshot import verify_snapshot
            from consolecrypt_runner.validation_reports import read_report
            policy,root,snapshot,parsed,checked,inputs,_=self.enroll_template(signed,revision_id,inventory,parameters)
            body=policy.body
            require(type(targets) is list and len(targets)==len(body['hosts']) and 0<len(targets)<=32,
                'service_targets','Exact reviewed targets required')
            from .service_authority import references
            expected={h['id']:h['target_digest'] for h in body['hosts']}
            require(len({t['host_id'] for t in targets})==len(targets)
                and {t['host_id']:hashlib.sha256(encode(t)).hexdigest() for t in targets}==expected,
                'service_targets','Targets differ from owner review')
            inputs['targets_digest']=hashlib.sha256(encode(targets)).hexdigest()
            report=read_report(root.parent,inputs)
            require(report.get('valid') is True,'validation_changed','Normal complete validation required')
            verify_snapshot(root,snapshot)
            value={'version':1,'delegation':signed,'delegationDigest':policy.digest,'revisionId':revision_id,
                'revisionDigest':snapshot.digest,'inventory':inventory,'parameters':checked,'targets':targets}
            require(len(encode(value))<=6*1024*1024,'service_template','Template metadata exceeds limit')
            path=self.custody.root/TEMPLATE
            with self.custody.store._mutex:
                self._policy(signed)
                if path.exists() or path.is_symlink():
                    require(record(path,6*1024*1024)==value,'service_enrollment_changed','Original template cannot be replaced')
                else:atomic_json(path,value,max_bytes=6*1024*1024)
            return {'enrolled':True,'delegationDigest':policy.digest,'revisionId':revision_id}

    def resume(self,signed,scheduler,revision_id):
        """Existing template/status only; never re-observe an expired head."""
        from .service_resume import current
        with self.helper.revisions.lock,self.custody.store._mutex:
            policy,_=self._policy(signed)
            current(self.custody,signed,policy.body['execution_origin'])
            retained=self._active_template()
            require(retained['revisionId']==revision_id and retained['delegation']==signed and retained['delegationDigest']==policy.digest,
                'service_enrollment_changed','Original retained policy required')
            require(scheduler.unresolved(policy.body['schedule_id'],policy.body['schedule_revision']) is None,
                'service_slot_unresolved','Original slot requires explicit reconciliation')
            self.custody._scheduler_floor(scheduler,int(self.custody.clock()))
            # Same report/snapshot/target validation; existing-value CAS makes
            # this a read-only enrollment observation, not a second install.
            return self.enroll(signed,retained['revisionId'],retained['inventory'],retained['parameters'],retained['targets'])

    def references_revision(self,revision_id,digest):
        path=self.custody.root/TEMPLATE
        if path.exists() or path.is_symlink():
            value=validate_template_record(self.custody.root,self.custody.owner,self.custody.service_id)
            # A retired DATA tombstone preserves the reference/audit without
            # pinning the plaintext payload. Live-job refs still independently pin.
            if value['revisionId']==revision_id and value['revisionDigest']==digest:
                if value['version']==1:return True
                from .service_retention import ended_template_references
                # Same supported raw history retirement; never changes outcome,
                # replay or pin metadata. Reacquire the existing store lock only
                # AFTER this call, not nested flock/unlock contexts.
                self.custody.store.retire_history(now=int(self.custody.clock()))
                with self.custody.store.locked():
                    return ended_template_references(self.custody,self._retention_runtime,revision_id,digest)
        return self.custody.store.references_revision(revision_id,digest)

    def start(self,signed,head,slot,approval,scheduler,current_refs,current_owner):
        require(not self.closed,'service_closed','Service runtime closed')
        policy,environment=self._policy(signed)
        retained=self._active_template()
        require(retained['delegation']==signed and retained['delegationDigest']==policy.digest,
            'service_enrollment_changed','Original enrolled policy required')
        body=policy.body
        grant=TrustGrant(bytes.fromhex(body['signer']['public_key']),body['tenant']['id'],
            frozenset(h['id'] for h in body['hosts']),frozenset([body['project_id']]),frozenset(['jobs.execute']),
            body['expires_at'],body['quotas']['max_job_seconds'])
        verified=verify_envelope(approval,{body['service_id']:grant},runner_id=body['service_id'],
            tenant_id=body['tenant']['id'],environment_digest=environment['environment_digest'],now=int(self.custody.clock()))
        submission={'revision_id':retained['revisionId'],'inventory':retained['inventory'],'parameters':retained['parameters']}
        with self.helper.revisions.lock:
            root,snapshot,inventory,parameters=self.executor._checked_inputs(verified,submission,environment)
            require('vault' not in parameters,'service_vault_unavailable','Service Vault execution is not provisioned')
            require(verified.value['targets']==retained['targets'],'service_targets','Original service routes required')
            with self.executor.lock,self.custody.store._mutex:
                # All policy/slot/UTC floor/revocation checks repeat at the
                # actual start boundary; this is not a created=False shortcut.
                ack=self.custody.accept(policy,head,slot,approval,scheduler,current_refs,current_owner)
                job=ack['jobId'];directory=self.custody.store.job_directory(job)
                state=self.custody.store.get(job)
                require(not self.closed and state['state']=='queued' and job not in self.executor.pending
                    and not (directory/START).exists() and not (directory/START).is_symlink(),
                    'service_already_started','Original queued slot cannot execute twice')
                require(len(self.executor.pending)<min(64,body['quotas']['max_concurrent']),
                    'service_concurrency','Service execution concurrency reached')
                ref={'version':1,'revision_id':retained['revisionId'],'revision_digest':snapshot.digest}
                if (directory/'revision.json').exists():
                    require(record(directory/'revision.json')==ref,'revision_reference','Original snapshot reference changed')
                else:atomic_json(directory/'revision.json',ref)
                state['environment']=environment_receipt(environment,verified.value['environment_digest'])
                atomic_json(directory/'state.json',state)
                atomic_json(directory/START,{'version':1,'jobId':job,'manifestDigest':verified.digest,
                    'delegationDigest':policy.digest,'revisionId':retained['revisionId'],'targetOutcome':'pending'})
                cancel=threading.Event();self.executor.pending[job]=(cancel,verified)
                try:self.executor.worker.submit(self.executor.run,verified,root,snapshot,inventory,parameters,cancel)
                except BaseException:
                    self.executor.pending.pop(job,None);cancel.set();self.executor.channel.revoke(verified)
                    self.custody.store.transition(job,'interrupted',result={'reason':'service_start_interrupted'})
                    raise
                return {'started':True,'jobId':job,'state':state['state']}

    def close(self):
        with self.executor.lock:self.closed=True
        self.executor.close() # Real worker/process/JobBroker join, then caller closes store.
        path=self.custody.root/TEMPLATE
        if path.exists() or path.is_symlink():
            from .service_retention import retire_joined
            # serve() can already have closed the SAME store before its
            # outer finally repeats runtime.close(). Join remains mandatory;
            # optional raw retirement cannot reopen/adopt a closed owner.
            with self.custody.store._mutex:
                if self.custody.store._lock_fd is None:return
                with self.custody.store.locked():
                    retire_joined(self.custody,self._retention_runtime,joined=True)
