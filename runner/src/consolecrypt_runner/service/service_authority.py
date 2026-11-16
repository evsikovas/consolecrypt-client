"""Strict signed service policy evidence. No provisioning, dispatch or generic signer."""
from dataclasses import dataclass
from datetime import datetime, timezone
import base64
import hashlib
import re
from types import MappingProxyType
from cryptography.exceptions import InvalidSignature
from cryptography.hazmat.primitives.asymmetric.ed25519 import Ed25519PublicKey
from consolecrypt_runner.approval import TrustGrant, verify_envelope
from consolecrypt_runner.errors import RunnerError, require
from consolecrypt_runner.protocol import decode, encode
from .cc_service_queue.queue import identifier

DELEGATION_DOMAIN=b'ConsoleCrypt ServiceDelegation v1\x00'
HEAD_DOMAIN=b'ConsoleCrypt ServiceTrustHead v1\x00'
SLOT_DOMAIN=b'ConsoleCrypt ScheduledJobGrant v1\x00'
HEX=re.compile(r'^[0-9a-f]{64}$')
MAX_TIME=253402300799
MAX_INT=9007199254740991

def need(value,code='service_policy'):
    require(value,code,'Unsupported service authorization evidence')
def fields(value,names):
    need(type(value) is dict and set(value)==set(names),'service_policy_fields')
def integer(value,low=1,high=MAX_INT):need(type(value) is int and low<=value<=high)
def digest(value):need(type(value) is str and HEX.fullmatch(value) is not None)
def tenant(value):
    fields(value,['kind','id']);need(value['kind'] in ('localProfile','personalAccount'),'service_team_unsupported');identifier(value['id'])
def key(value,kind):
    fields(value,['kind','public_key','key_epoch','certificate_epoch']);need(value['kind']==kind)
    digest(value['public_key']);integer(value['key_epoch']);integer(value['certificate_epoch'])
def references(value,host=False):
    need(type(value) is list and 0<len(value)<=64)
    previous=''
    for row in value:
        fields(row,['id','revision','target_digest'] if host else ['id','revision'])
        identifier(row['id']);integer(row['revision']);need(row['id']>previous);previous=row['id']
        if host:digest(row['target_digest'])
def window(body,now,maximum):
    for name in ['issued_at','expires_at']:integer(body[name],0,MAX_TIME)
    need(body['issued_at']<=now<body['expires_at']<=body['issued_at']+maximum,'service_policy_expired')
def statement(envelope,domain,public):
    need(type(envelope) is bytes and len(envelope)<=65536,'service_policy_budget')
    value=decode(envelope,max_bytes=65536);fields(value,['body','signature'])
    body=value['body'];need(type(body) is dict)
    try:
        signature=base64.b64decode(value['signature'],validate=True);need(len(signature)==64,'service_policy_signature')
        Ed25519PublicKey.from_public_bytes(public).verify(signature,domain+encode(body))
    except (InvalidSignature,ValueError,TypeError):
        raise RunnerError('service_policy_signature','Service signature rejected') from None
    return body

def delegation_shape(body):
    fields(body,['version','id','owner_id','owner_epoch','tenant','service_id','signer','recipient',
        'project_id','project_revision','template_digest','schedule_id','schedule_revision',
        'definition_digest','inventory_digest','parameters_digest','environment_digest','hosts','credentials','modes','quotas','policy_revision','revocation_epoch',
        'issued_at','expires_at','desktop_lock_policy'])
    need(type(body['version']) is int and body['version']==1,'service_policy_version')
    for name in ['id','owner_id','service_id','project_id','schedule_id']:identifier(body[name])
    for name in ['owner_epoch','project_revision','schedule_revision','policy_revision','revocation_epoch']:integer(body[name])
    tenant(body['tenant']);key(body['signer'],'ed25519');key(body['recipient'],'x25519')
    need(body['signer']['public_key']!=body['recipient']['public_key'],'service_separate_recipient_required')
    for name in ['template_digest','definition_digest','inventory_digest','parameters_digest','environment_digest']:digest(body[name])
    references(body['hosts'],True)
    need(type(body['credentials']) is list and len(body['credentials'])<=64)
    if body['credentials']:references(body['credentials'])
    need(type(body['modes']) is list and body['modes'] in (['check'],['apply'],['apply','check']))
    fields(body['quotas'],['max_jobs','max_per_utc_day','max_concurrent','max_job_seconds','max_forks'])
    integer(body['quotas']['max_jobs'],1,128);integer(body['quotas']['max_per_utc_day'],1,body['quotas']['max_jobs'])
    need(type(body['quotas']['max_concurrent']) is int and body['quotas']['max_concurrent']==1)
    integer(body['quotas']['max_job_seconds'],1,3600);integer(body['quotas']['max_forks'],1,5)
    need(body['desktop_lock_policy']=='independentDelegatedService')

@dataclass(frozen=True,repr=False)
class PinnedOwner:
    """Trusted Native/Agent owner-pin input; NEVER constructed from received policy."""
    owner_id:str
    owner_epoch:int
    tenant:dict
    public_key:bytes
    not_after:int
    minimum_policy_revision:int=1
    minimum_revocation_epoch:int=1
    def __post_init__(self):
        identifier(self.owner_id);integer(self.owner_epoch);tenant(self.tenant)
        need(type(self.public_key) is bytes and len(self.public_key)==32)
        integer(self.not_after,0,MAX_TIME);integer(self.minimum_policy_revision);integer(self.minimum_revocation_epoch)
        object.__setattr__(self,'tenant',MappingProxyType(dict(self.tenant)))

@dataclass(frozen=True,repr=False)
class VerifiedDelegation:
    _canonical:bytes
    digest:str
    owner:PinnedOwner
    @property
    def body(self):return decode(self._canonical,max_bytes=65536)

@dataclass(frozen=True)
class OriginalSlot:
    schedule_id:str
    schedule_revision:int
    definition_digest:str
    local_slot:str
    due_utc:int
    grace_seconds:int


def capture_original_slot(store,schedule_id,expected_revision):
    """Read SAME existing scheduler transaction; no token/release or new outbox."""
    from .scheduler import Definition, first_occurrence
    identifier(schedule_id);integer(expected_revision)
    with store.transaction():
        row=store.db.execute('SELECT revision,definition,not_before,enabled FROM schedules WHERE id=?',(schedule_id,)).fetchone()
        need(row is not None and row[0]==expected_revision and row[3]==1,'service_schedule_changed')
        current=Definition.parse(row[1]);current_digest=hashlib.sha256(row[1].encode()).hexdigest()
        slot=store.db.execute("SELECT schedule_revision,local_slot,due_utc,definition,definition_digest FROM slots WHERE schedule_id=? AND state='reserved'",(schedule_id,)).fetchone()
        need(slot is not None and slot[0]==expected_revision,'service_schedule_changed')
        original=Definition.parse(slot[3]);need(hashlib.sha256(slot[3].encode()).hexdigest()==slot[4]==current_digest,'service_schedule_changed')
        need(type(slot[1]) is str and re.fullmatch(r'\d{4}-\d\d-\d\dT\d\d:\d\d',slot[1]) is not None)
        try:local=datetime.strptime(slot[1],'%Y-%m-%dT%H:%M')
        except ValueError:raise RunnerError('service_policy','Invalid original slot') from None
        need((local.hour,local.minute)==(original.hour,original.minute))
        due=first_occurrence(original,local.date());need(type(slot[2]) is int and due==slot[2] and due>=row[2])
        return OriginalSlot(schedule_id,slot[0],slot[4],slot[1],due,current.grace_seconds)


def verify_delegation(raw,owner,now):
    need(type(owner) is PinnedOwner);integer(now,0,MAX_TIME)
    body=statement(raw,DELEGATION_DOMAIN,owner.public_key);delegation_shape(body);window(body,now,86400)
    need(body['signer']['public_key']!=owner.public_key.hex() and body['recipient']['public_key']!=owner.public_key.hex(),'service_owner_key_reuse')
    need(body['owner_id']==owner.owner_id and body['owner_epoch']==owner.owner_epoch and body['tenant']==owner.tenant,'service_owner_mismatch')
    need(body['expires_at']<=owner.not_after and body['policy_revision']>=owner.minimum_policy_revision
         and body['revocation_epoch']>=owner.minimum_revocation_epoch,'service_policy_rollback')
    return VerifiedDelegation(encode(body),hashlib.sha256(DELEGATION_DOMAIN+encode(body)).hexdigest(),owner)


def verify_head(raw,delegation,now):
    body=statement(raw,HEAD_DOMAIN,delegation.owner.public_key)
    fields(body,['version','owner_id','owner_epoch','tenant','service_id','delegation_digest','policy_revision',
                 'revocation_epoch','signer_epoch','recipient_epoch','certificate_epoch','recipient_certificate_epoch','revoked','issued_at','expires_at'])
    need(type(body['version']) is int and body['version']==1,'service_policy_version');window(body,now,300)
    identifier(body['owner_id']);identifier(body['service_id']);integer(body['owner_epoch']);tenant(body['tenant']);digest(body['delegation_digest'])
    approved=delegation.body
    need(body['owner_id']==delegation.owner.owner_id and body['owner_epoch']==delegation.owner.owner_epoch
         and body['tenant']==delegation.owner.tenant and body['service_id']==approved['service_id']
         and body['delegation_digest']==delegation.digest,'service_owner_mismatch')
    for name in ['policy_revision','revocation_epoch','signer_epoch','recipient_epoch','certificate_epoch','recipient_certificate_epoch']:integer(body[name])
    need(body['policy_revision']==approved['policy_revision'] and body['revocation_epoch']==approved['revocation_epoch']
         and body['signer_epoch']==approved['signer']['key_epoch']
         and body['recipient_epoch']==approved['recipient']['key_epoch']
         and body['certificate_epoch']==approved['signer']['certificate_epoch']
         and body['recipient_certificate_epoch']==approved['recipient']['certificate_epoch'],'service_policy_rollback')
    need(type(body['revoked']) is bool and not body['revoked'],'service_revoked')
    return body

@dataclass(frozen=True,repr=False)
class VerifiedJobEvidence:
    manifest_digest:str
    job_id:str
    delegation_digest:str
    slot:OriginalSlot
    expires_at:int
    # No authority/material/secret-bearing or dispatch method is minted.
    def authorize_dispatch(self):
        raise RunnerError('service_provisioning_required','Scoped recipient provisioning and durable replay/quota admission required')


def _current_delegation_body(delegation,original_slot,current_owner,now):
    need(type(delegation) is VerifiedDelegation and type(original_slot) is OriginalSlot)
    integer(now,0,MAX_TIME);need(type(current_owner) is PinnedOwner)
    body=delegation.body;window(body,now,86400)
    need(current_owner.owner_id==delegation.owner.owner_id and current_owner.owner_epoch==delegation.owner.owner_epoch
         and current_owner.public_key==delegation.owner.public_key and current_owner.tenant==delegation.owner.tenant
         and now<current_owner.not_after and body['expires_at']<=current_owner.not_after
         and body['policy_revision']>=current_owner.minimum_policy_revision
         and body['revocation_epoch']>=current_owner.minimum_revocation_epoch,'service_policy_rollback')
    return body


def verify_job_evidence(delegation,trust_head,slot_grant,approval,original_slot,reference_context,current_owner,now):
    # Original strict fresh-head validation order remains unchanged.
    _current_delegation_body(delegation,original_slot,current_owner,now)
    head=verify_head(trust_head,delegation,now)
    return _verify_job_evidence_until(delegation,slot_grant,approval,original_slot,reference_context,current_owner,now,head['expires_at'])


def _verify_job_evidence_until(delegation,slot_grant,approval,original_slot,reference_context,current_owner,now,access_deadline):
    body=_current_delegation_body(delegation,original_slot,current_owner,now)
    integer(access_deadline,1,MAX_TIME);need(now<access_deadline<=body['expires_at'],'service_policy_expired')
    fields(reference_context,['service_id','signer_epoch','recipient_epoch','certificate_epoch','project_id',
        'project_revision','template_digest','hosts','credentials','recipient_certificate_epoch','environment_digest','inventory_digest','parameters_digest'])
    expected={'service_id':body['service_id'],'signer_epoch':body['signer']['key_epoch'],
        'recipient_epoch':body['recipient']['key_epoch'],'certificate_epoch':body['signer']['certificate_epoch'],
        'project_id':body['project_id'],'project_revision':body['project_revision'],'template_digest':body['template_digest'],
        'hosts':body['hosts'],'credentials':body['credentials'],
        'inventory_digest':body['inventory_digest'],'parameters_digest':body['parameters_digest'],'environment_digest':body['environment_digest']}
    expected['recipient_certificate_epoch']=body['recipient']['certificate_epoch']
    for name in ['signer_epoch','recipient_epoch','certificate_epoch','recipient_certificate_epoch','project_revision']:integer(reference_context[name])
    digest(reference_context['environment_digest']);references(reference_context['hosts'],True)
    if reference_context['credentials']:references(reference_context['credentials'])
    need(all(reference_context[k]==v for k,v in expected.items()),'service_reference_changed')
    slot=statement(slot_grant,SLOT_DOMAIN,bytes.fromhex(body['signer']['public_key']))
    fields(slot,['version','tenant','service_id','delegation_digest','policy_revision','revocation_epoch',
        'project_id','project_revision','template_digest','hosts','credentials','schedule_id','schedule_revision',
        'definition_digest','local_slot','due_utc','job_id','manifest_digest','nonce','issued_at','expires_at'])
    need(type(slot['version']) is int and slot['version']==1,'service_policy_version');window(slot,now,300)
    for name in ['job_id','nonce','service_id','project_id','schedule_id']:identifier(slot[name])
    tenant(slot['tenant']);digest(slot['delegation_digest']);digest(slot['manifest_digest']);digest(slot['template_digest']);digest(slot['definition_digest'])
    for name in ['policy_revision','revocation_epoch','project_revision','schedule_revision']:integer(slot[name])
    integer(slot['due_utc'],0,MAX_TIME);references(slot['hosts'],True)
    need(type(slot['credentials']) is list)
    if slot['credentials']:references(slot['credentials'])
    need(slot['tenant']==body['tenant'] and slot['service_id']==body['service_id'] and slot['delegation_digest']==delegation.digest)
    for name in ['policy_revision','revocation_epoch','project_id','project_revision','template_digest','hosts','credentials','schedule_id','schedule_revision','definition_digest']:
        need(slot[name]==body[name],'service_reference_changed')
    need(original_slot.schedule_id==body['schedule_id'] and original_slot.schedule_revision==body['schedule_revision']
         and original_slot.definition_digest==body['definition_digest'] and slot['local_slot']==original_slot.local_slot
         and slot['due_utc']==original_slot.due_utc and original_slot.due_utc<=now<=original_slot.due_utc+original_slot.grace_seconds,
         'service_schedule_changed')
    need(slot['expires_at']<=min(body['expires_at'],access_deadline),'service_policy_expired')
    grant=TrustGrant(bytes.fromhex(body['signer']['public_key']),body['tenant']['id'],
        frozenset(x['id'] for x in body['hosts']),frozenset([body['project_id']]),frozenset(['jobs.execute']),
        body['expires_at'],body['quotas']['max_job_seconds'])
    # Reuse the REAL existing signed JobManifest verifier, never caller success.
    manifest=verify_envelope(approval,{body['service_id']:grant},runner_id=body['service_id'],
        tenant_id=body['tenant']['id'],environment_digest=reference_context['environment_digest'],now=now)
    value=manifest.value
    need(manifest.digest==slot['manifest_digest'] and value['job_id']==slot['job_id']
         and value['project_id']==body['project_id'] and value['revision_digest']==body['template_digest']
         and value['inventory_digest']==body['inventory_digest'] and value['parameters_digest']==body['parameters_digest']
         and value['environment_digest']==body['environment_digest'] and value['mode'] in body['modes'],'service_reference_changed')
    need(value['issued_at']<=now and value['expires_at']<=value['issued_at']+300
         and value['access_expires_at']<=min(slot['expires_at'],access_deadline,body['expires_at'])
         and value['policy']['forks']<=body['quotas']['max_forks']
         and value['policy']['strict_isolation'] is True and value['policy']['cancel_on_lock'] is False,
         'service_job_lease')
    targets={t['host_id']:t for t in value['targets']};need(set(targets)=={h['id'] for h in body['hosts']},'service_reference_changed')
    credentials={c['id'] for c in body['credentials']}
    for host in body['hosts']:
        target=targets[host['id']]
        need(hashlib.sha256(encode(target)).hexdigest()==host['target_digest']
             and set(target['credential_refs'])<=credentials,'service_reference_changed')
    return VerifiedJobEvidence(manifest.digest,value['job_id'],delegation.digest,original_slot,
                               min(value['access_expires_at'],slot['expires_at'],access_deadline))
