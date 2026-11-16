"""Private inherited supervisor pipe, never a public enrollment listener."""
import os,sys,time,uuid
from pathlib import Path
from .service_authority import PinnedOwner
from .service_custody import ServiceCustody
from .service_helper import serve_service
from .scheduler import Store,Definition,Clock
from consolecrypt_runner.errors import require
from consolecrypt_runner.protocol import read_request,write_frame,encode

def serve(root,source,output):
    require(sys.platform=='linux' and os.getuid()!=0,'service_backend','Dedicated Linux non-root service required')
    request=read_request(source)
    if request is not None and request['operation']=='retire_expired_bootstrap':
        from .service_retention import bootstrap_retirement
        result=bootstrap_retirement(root,request['body'])
        write_frame(output,{'version':1,'type':'response','request_id':request['request_id'],'result':result,'error':None})
        return
    require(request is not None and request['operation'] in ('service_bootstrap','service_resume_bootstrap'),'service_bootstrap','Original supervisor pipe required')
    restoring=request['operation']=='service_resume_bootstrap'
    body=request['body']
    require(type(body) is dict and set(body)==({'version','owner','origin','schedule','delegation','revision_id'} if restoring else {'version','owner','origin','schedule'}),'service_bootstrap','Invalid service bootstrap')
    require(type(body['version']) is int and body['version']==1,'service_bootstrap','Unsupported service bootstrap')
    owner=body['owner']
    require(type(owner) is dict and set(owner)=={'owner_id','owner_epoch','tenant','public_key','not_after',
        'minimum_policy_revision','minimum_revocation_epoch'},'service_bootstrap','Original owner pin required')
    pin=PinnedOwner(owner['owner_id'],owner['owner_epoch'],owner['tenant'],bytes.fromhex(owner['public_key']),
        owner['not_after'],owner['minimum_policy_revision'],owner['minimum_revocation_epoch'])
    origin=body['origin']
    # Full descriptor matching is repeated against the actual signed policy,
    # current helper probe and capsule-derived Rust sessions before each start.
    require(type(origin) is dict and origin.get('backend')=='linuxNativeSsh' and origin.get('network')=='serviceLocal',
        'service_origin','Original service-local enrollment required')
    custody=None;scheduler=None
    try:
        schedule=body['schedule']
        require(type(schedule) is dict and set(schedule)=={'id','revision','definition'},'service_schedule','Original schedule required')
        definition=Definition.parse(encode(schedule['definition']).decode())
        if restoring:
            from .service_resume import preflight,schedule_check
            scope=ServiceCustody._restore_scope(root,pin,origin['service_id'])
            preflight(scope,body['delegation'],origin,body['revision_id'])
            scheduler=Store.open_existing(root/'scheduler')
            schedule_check(scheduler,schedule,scope,body['delegation'],origin)
            # All retained service/scheduler schemas and current original
            # revocation/clock facts passed BEFORE JobStore recovery writes.
            custody=ServiceCustody(root,pin,origin['service_id'],_existing_only=True)
        else:
            custody=ServiceCustody(root,pin,origin['service_id'])
            scheduler=Store(root/'scheduler')
            current=next((v for v in scheduler.schedules() if v.schedule_id==schedule['id']),None)
            if current is None:
                require(type(schedule['revision']) is int and schedule['revision']==1,'service_schedule','Fresh schedule revision required')
                scheduler.save(schedule['id'],0,definition,Clock(int(time.time()),time.monotonic_ns(),str(uuid.uuid4())))
            else:
                require(current.revision==schedule['revision'] and current.definition==definition,
                    'service_schedule','Existing schedule cannot be silently replaced')
        write_frame(output,{'version':1,'type':'response','request_id':request['request_id'],
            'result':{'bootstrapped':True},'error':None})
        serve_service(custody,origin,scheduler,source,output)
    finally:
        if scheduler is not None:scheduler.close()
        if custody is not None:custody.close()
