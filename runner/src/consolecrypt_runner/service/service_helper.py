"""Private supervisor-owned service endpoint over existing helper framing.

The owner pin/origin are inherited ORIGINAL enrollment, not a network policy's
self-asserted identity. No key or password is accepted by this Python endpoint.
"""
from consolecrypt_runner.__main__ import Helper,serve
from consolecrypt_runner.errors import require,RunnerError
from consolecrypt_runner.protocol import encode
from .service_authority_v4 import verify_delegation
from .service_custody import ServiceCustody
from .service_runtime import ServiceExecutionRuntime

class ServiceHelper(Helper):
    def __init__(self,custody,origin,scheduler):
        self.custody,self.origin,self.scheduler=custody,origin,scheduler
        self.runtime=None
        super().__init__(custody.root,_service_store=custody.store)

    def bind_executor(self,targets):
        require(self.runtime is None,'service_scope','Service endpoint already bound')
        self.runtime=ServiceExecutionRuntime(self.custody,self,targets,self.origin)
        return self.runtime.executor

    def request(self,operation,body,cancel):
        require(self.runtime is not None,'service_scope','Original service executor required')
        if operation=='service_enroll':
            require(set(body)=={'delegation','initial_status','revision_id','inventory','parameters','targets'},
                'service_fields','Invalid service template enrollment')
            policy=verify_delegation(encode(body['delegation']),self.custody.owner,int(self.custody.clock()))
            self.custody.record_owner_status(policy,encode(body['initial_status']),self.executor)
            return self.runtime.enroll(body['delegation'],body['revision_id'],body['inventory'],body['parameters'],body['targets'])
        if operation=='service_resume':
            require(set(body)=={'delegation','revision_id'},'service_fields','Invalid service restoration')
            return self.runtime.resume(body['delegation'],self.scheduler,body['revision_id'])
        if operation=='service_start':
            require(set(body)=={'delegation','head','slot','approval','current_refs'},
                'service_fields','Invalid service activation')
            require(not cancel.is_set(),'cancelled','Service activation cancelled')
            # Current refs are supplied by the original Rust supervisor after
            # real capsule/session/known-host checks; caller cannot pass a grant
            # flag or choose a new root/target/credential here.
            return self.runtime.start(body['delegation'],None if body['head'] is None else encode(body['head']),
                encode(body['slot']),body['approval'],self.scheduler,body['current_refs'],self.custody.owner)
        if operation=='service_due':
            require(set(body)=={'delegation'},'service_fields','Invalid service schedule query')
            return self.runtime.due(body['delegation'],self.scheduler)
        if operation=='service_finish_slot':
            require(set(body)=={'job_id'},'service_fields','Invalid service terminal observation')
            return self.runtime.finish_slot(body['job_id'],self.scheduler)
        if operation=='service_target_end':
            require(set(body)=={'job_id','manifest_digest','outcome'},'service_fields','Invalid target observation')
            return self.runtime.observe_target_end(body['job_id'],body['manifest_digest'],body['outcome'])
        if operation=='service_job_status':
            require(set(body)=={'job_id'},'service_fields','Invalid service job query')
            return self.runtime.job_status(body['job_id'])
        if operation=='service_owner_status':
            require(set(body)=={'delegation','statement'},'service_fields','Invalid owner status')
            policy=verify_delegation(encode(body['delegation']),self.custody.owner,int(self.custody.clock()))
            return self.custody.record_owner_status(policy,encode(body['statement']),self.executor)
        if operation=='revision_release':
            require(set(body)=={'revision_id'},'revision_fields','Invalid revision release')
            return self.revisions.release(body['revision_id'],referenced=self.runtime.references_revision)
        if operation in ('configure_interactive','configure_manual_owner','accept_job','vault_state','capture','git_capture'):
            raise RunnerError('service_purpose','Explicit service review and activation required')
        # Same upload, inventory, validation, job/events/cancel implementation.
        require(operation in {'probe','upload_begin','upload_entry','upload_chunk','upload_abort','upload_commit',
            'prepare_run','read_chunk','read_file','files','revision_list','revision_release','inventory_capture','inventory_chunk',
            'inventory_upload_begin','inventory_upload_chunk','inventory_upload_commit','inventory_upload_abort',
            'job_get','job_events','job_cancel'},'service_operation','Unsupported service operation')
        return super().request(operation,body,cancel)

def serve_service(custody,origin,scheduler,source,output):
    helper=ServiceHelper(custody,origin,scheduler)
    try:serve(helper,source,output,_executor_factory=helper.bind_executor)
    finally:
        try:
            if helper.runtime is not None:helper.runtime.close()
        finally:helper.close()
