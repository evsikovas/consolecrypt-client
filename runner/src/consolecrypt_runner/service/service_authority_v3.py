"""Strict owner delegation v3: original all-hop/proxy/key-trust references.

No signer, material, storage or dispatch capability. Older signed policies are
not admitted. Shared schema/job verifier code remains historical, not a fallback.
"""
import hashlib
from . import service_authority as common
from . import service_authority_v2 as shape_v2
from .service_authority import PinnedOwner,OriginalSlot,VerifiedDelegation,VerifiedJobEvidence,capture_original_slot,HEAD_DOMAIN,SLOT_DOMAIN
from consolecrypt_runner.protocol import encode

DELEGATION_DOMAIN=b'ConsoleCrypt ServiceDelegation v3\0'
KINDS=frozenset({'host','group','jump_profile','proxy','credential','secret','tunnel'})

def route_inputs(value):
    common.fields(value,['dependencies','key_trust'])
    common.need(type(value['dependencies']) is list and type(value['key_trust']) is list
        and value['dependencies'] and value['key_trust']
        and len(value['dependencies'])+len(value['key_trust'])<=512,'service_route_inputs')
    previous_ids=set()
    for field in ['dependencies','key_trust']:
        previous=''
        for row in value[field]:
            common.fields(row,['id','revision','kind']);common.identifier(row['id']);common.integer(row['revision'])
            common.need(type(row['kind']) is str and row['id']>previous and row['id'] not in previous_ids,'service_route_inputs')
            common.need(row['kind']=='known_host' if field=='key_trust' else row['kind'] in KINDS,'service_route_inputs')
            previous=row['id'];previous_ids.add(row['id'])

def delegation_shape(body):
    common.need(type(body) is dict and type(body.get('version')) is int and body['version']==3,'service_policy_version')
    common.need('route_inputs' in body,'service_route_inputs');route_inputs(body['route_inputs'])
    previous=dict(body);previous.pop('route_inputs');previous['version']=2
    # Only common SCHEMA checks are projected. Signature/digest use original v3
    # bytes/domain. Remaining unknown fields are never dropped.
    shape_v2.delegation_shape(previous)
    refs={row['id']:row for row in body['route_inputs']['dependencies']}
    for field,kind in [('hosts','host'),('credentials','credential')]:
        for row in body[field]:
            common.need(row['id'] in refs and refs[row['id']]['revision']==row['revision']
                        and refs[row['id']]['kind']==kind,'service_route_inputs')

def verify_delegation(raw,owner,now):
    common.need(type(owner) is PinnedOwner);common.integer(now,0,common.MAX_TIME)
    body=common.statement(raw,DELEGATION_DOMAIN,owner.public_key);delegation_shape(body);common.window(body,now,86400)
    common.need(body['signer']['public_key']!=owner.public_key.hex() and body['recipient']['public_key']!=owner.public_key.hex(),'service_owner_key_reuse')
    common.need(body['owner_id']==owner.owner_id and body['owner_epoch']==owner.owner_epoch and body['tenant']==owner.tenant,'service_owner_mismatch')
    common.need(body['expires_at']<=owner.not_after and body['policy_revision']>=owner.minimum_policy_revision
                and body['revocation_epoch']>=owner.minimum_revocation_epoch,'service_policy_rollback')
    return VerifiedDelegation(encode(body),hashlib.sha256(DELEGATION_DOMAIN+encode(body)).hexdigest(),owner)

def verify_job_evidence(delegation,trust_head,slot_grant,approval,original_slot,reference_context,current_owner,now):
    common.need(type(delegation) is VerifiedDelegation)
    body=delegation.body;delegation_shape(body)
    common.need(delegation.digest==hashlib.sha256(DELEGATION_DOMAIN+encode(body)).hexdigest(),'service_policy_version')
    common.need(type(reference_context) is dict and 'route_inputs' in reference_context,'service_route_changed')
    route_inputs(reference_context['route_inputs'])
    common.need(reference_context['route_inputs']==body['route_inputs'],'service_route_changed')
    common.need(reference_context.get('playbook')==body['playbook'] and type(approval) is dict
        and type(approval.get('manifest')) is dict and approval['manifest'].get('playbook')==body['playbook'],'service_playbook')
    current=dict(reference_context);current.pop('route_inputs');current.pop('playbook')
    # Exact original route refs plus all existing deep signed-head/job/slot/ref/
    # lease/budget checks. This result still cannot authorize_dispatch().
    return common.verify_job_evidence(delegation,trust_head,slot_grant,approval,original_slot,current,current_owner,now)
