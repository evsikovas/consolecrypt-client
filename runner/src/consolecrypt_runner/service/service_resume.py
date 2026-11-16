"""Existing-only restart observation. No head issuance, reservation or copier."""
import hashlib,os,stat,time
from pathlib import Path
from .cc_service_queue.queue import bounded_children,record,identifier
from .service_authority_v4 import verify_delegation,observe_owner_status
from consolecrypt_runner.errors import require
from consolecrypt_runner.protocol import encode

def directory(path):
    info=path.lstat()
    require(stat.S_ISDIR(info.st_mode) and info.st_uid==os.getuid() and info.st_mode&0o7777==0o700,
        'service_restore','Original private runtime directory required')

def regular(path,maximum):
    info=path.lstat()
    require(stat.S_ISREG(info.st_mode) and info.st_uid==os.getuid() and info.st_nlink==1
        and info.st_mode&0o7777==0o600 and info.st_size<=maximum,
        'service_restore','Original private runtime file required')

def current(scope,signed,origin):
    """Time and original observation come ONLY from actual retained custody."""
    now=int(scope.clock());policy=verify_delegation(encode(signed),scope.owner,now);body=policy.body
    require(body['access_policy']['mode']=='boundedOffline' and body.get('execution_origin')==origin
        and body['service_id']==scope.service_id and body['tenant']==scope.tenant,
        'service_restore','Original bounded offline enrollment required')
    header=scope._validate_header(record(scope.root/'service-version.json',8192))
    require(not header['revoked'] and now>=header['clockFloor'] and body['policy_revision']>=header['policyFloor']
        and body['revocation_epoch']>=header['revocationFloor'],'service_revoked','Original policy revoked or clock rolled back')
    observed=header['ownerStatus']
    require(observed is not None and observed['delegationDigest']==policy.digest,
        'service_owner_status_required','Original observed owner status required')
    status=observe_owner_status(encode(observed['statement']),policy,observed['observedAt'])
    require(not status['revoked'],'service_revoked','Original policy revoked')
    return policy,header

def preflight(scope,signed,origin,revision_id):
    """No mkdir/write/recovery; even known directory cleanup waits for this."""
    root=scope.root;directory(root)
    policy,header=current(scope,signed,origin)
    names=bounded_children(root,8,'service_restore')
    required={'service-version.json','service-template.json','state','scheduler','revisions'}
    require(required<={p.name for p in names},'service_restore','Incomplete existing runtime')
    for path in names:
        if path.name in required:continue
        require(path.name.startswith('uploads-'),'service_schema','Unknown retained runtime entry')
        identifier(path.name.removeprefix('uploads-'));directory(path)
        require(not bounded_children(path,1,'service_restore'),'service_restore','Interrupted transfer requires recovery')
    regular(root/'service-version.json',8192);regular(root/'service-template.json',6*1024*1024)
    for relative in ('state','state/jobs','state/nonces','scheduler','revisions'):directory(root/relative)
    for name in ('.lock','.owner'):regular(root/'state'/name,0)
    regular(root/'scheduler/schedule.sqlite',64*1024*1024)
    from .service_runtime import validate_template_record
    retained=validate_template_record(root,scope.owner,scope.service_id)
    identifier(revision_id)
    require(retained['revisionId']==revision_id and retained['delegation']==signed and retained['delegationDigest']==policy.digest,
        'service_enrollment_changed','Original retained enrollment differs')
    # The SAME repository/report/snapshot primitives used by ordinary enrollment.
    # No alternate report authority and no destination/tree copy.
    from consolecrypt_runner.revisions import RevisionRepository
    from consolecrypt_runner.snapshot import verify_snapshot
    from consolecrypt_runner.validation_reports import read_report
    revision=RevisionRepository(root/'revisions')
    entries=bounded_children(root/'revisions',64,'service_restore')
    require(any(p.name==retained['revisionId'] for p in entries),'service_restore','Original revision missing')
    for path in entries:
        directory(path);revision.read(path.name)
    project,snapshot=revision.read(retained['revisionId']);body=policy.body
    inputs={'revision_digest':snapshot.digest,'environment_digest':body['environment_digest'],'playbook':body['playbook'],
        'parameters_digest':body['parameters_digest'],'inventory_digest':body['inventory_digest'],
        'targets_digest':hashlib.sha256(encode(retained['targets'])).hexdigest()}
    require(snapshot.digest==body['template_digest'] and read_report(project.parent,inputs).get('valid') is True,
        'service_restore','Original complete validation required')
    verify_snapshot(project,snapshot)
    return retained,header

def schedule_check(scheduler,original,scope,signed,origin):
    highwater=scheduler.preflight_existing()
    now=int(scope.clock());current(scope,signed,origin)
    require(now>=highwater,'service_clock_rollback','Current time predates original scheduler')
    rows=scheduler.schedules()
    from .scheduler import Definition
    require(type(original) is dict and set(original)=={'id','revision','definition'},'service_schedule','Original schedule required')
    definition=Definition.parse(encode(original['definition']).decode())
    selected=next((row for row in rows if row.schedule_id==original['id']),None)
    require(selected is not None and selected.enabled and selected.revision==original['revision'] and selected.definition==definition,
        'service_schedule_changed','Original schedule cannot be replaced or enabled')
    require(scheduler.unresolved(original['id'],original['revision']) is None,
        'service_slot_unresolved','Original slot requires explicit reconciliation')

