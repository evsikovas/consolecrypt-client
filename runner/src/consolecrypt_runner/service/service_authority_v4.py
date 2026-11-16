"""Strict explicit owner bounded-offline v4, same primitive/deep job verifier."""
import hashlib
from . import service_authority as common
from . import service_authority_v3 as shape_v3
from .service_authority import PinnedOwner,OriginalSlot,VerifiedDelegation,VerifiedJobEvidence,capture_original_slot,HEAD_DOMAIN,SLOT_DOMAIN
from consolecrypt_runner.protocol import encode
DELEGATION_DOMAIN=b'ConsoleCrypt ServiceDelegation v4\0'

def access_policy(value,expires):
    common.fields(value,['mode','offline_not_after'])
    common.need(type(value['mode']) is str,'service_access_policy')
    if value['mode']=='freshOwnerHead':
        common.need(value['offline_not_after'] is None,'service_access_policy')
    else:
        common.need(value['mode']=='boundedOffline','service_access_policy')
        common.integer(value['offline_not_after'],1,common.MAX_TIME)
        common.need(value['offline_not_after']==expires,'service_access_policy')

def delegation_shape(body):
    common.need(type(body) is dict and type(body.get('version')) is int and body['version']==4,'service_policy_version')
    common.need('access_policy' in body,'service_access_policy');access_policy(body['access_policy'],body.get('expires_at'))
    previous=dict(body);previous.pop('access_policy');previous['version']=3
    if 'execution_origin' in previous:
        execution_origin(previous.pop('execution_origin'),body)
    shape_v3.delegation_shape(previous) # Schema only; never accepts/signs v3.

def execution_origin(value,body):
    common.fields(value,['version','service_id','runner_revision','runner_configuration_digest','signer_key_sha256',
        'recipient_key_sha256','environment_digest','helper_digest','known_hosts_digest','backend','network'])
    common.need(type(value['version']) is int and value['version']==1 and value['service_id']==body['service_id']
        and value['backend']=='linuxNativeSsh' and value['network']=='serviceLocal','service_origin')
    common.integer(value['runner_revision']);common.identifier(value['service_id'])
    for name in ['runner_configuration_digest','signer_key_sha256','recipient_key_sha256','environment_digest','helper_digest','known_hosts_digest']:
        common.digest(value[name])
    common.need(value['environment_digest']==body['environment_digest']
        and value['signer_key_sha256']==hashlib.sha256(bytes.fromhex(body['signer']['public_key'])).hexdigest()
        and value['recipient_key_sha256']==hashlib.sha256(bytes.fromhex(body['recipient']['public_key'])).hexdigest(),'service_origin')

def verify_delegation(raw,owner,now):
    common.need(type(owner) is PinnedOwner);common.integer(now,0,common.MAX_TIME)
    body=common.statement(raw,DELEGATION_DOMAIN,owner.public_key);delegation_shape(body);common.window(body,now,86400)
    common.need(body['signer']['public_key']!=owner.public_key.hex() and body['recipient']['public_key']!=owner.public_key.hex(),'service_owner_key_reuse')
    common.need(body['owner_id']==owner.owner_id and body['owner_epoch']==owner.owner_epoch and body['tenant']==owner.tenant,'service_owner_mismatch')
    common.need(body['expires_at']<=owner.not_after and body['policy_revision']>=owner.minimum_policy_revision and body['revocation_epoch']>=owner.minimum_revocation_epoch,'service_policy_rollback')
    return VerifiedDelegation(encode(body),hashlib.sha256(DELEGATION_DOMAIN+encode(body)).hexdigest(),owner)

def verify_job_evidence(delegation,trust_head,slot_grant,approval,original_slot,reference_context,current_owner,now):
    common.need(type(delegation) is VerifiedDelegation)
    body=delegation.body;delegation_shape(body)
    common.need(delegation.digest==hashlib.sha256(DELEGATION_DOMAIN+encode(body)).hexdigest(),'service_policy_version')
    common.need(type(reference_context) is dict and 'route_inputs' in reference_context,'service_route_changed')
    shape_v3.route_inputs(reference_context['route_inputs']);common.need(reference_context['route_inputs']==body['route_inputs'],'service_route_changed')
    common.need(reference_context.get('playbook')==body['playbook'] and type(approval) is dict and type(approval.get('manifest')) is dict and approval['manifest'].get('playbook')==body['playbook'],'service_playbook')
    if body['access_policy']['mode']=='freshOwnerHead':
        deadline=common.verify_head(trust_head,delegation,now)['expires_at']
    else:
        # No synthetic owner head, current-time renewal or expired-head fallback.
        common.need(trust_head is None,'service_access_policy')
        deadline=body['access_policy']['offline_not_after']
    current=dict(reference_context);current.pop('route_inputs');current.pop('playbook')
    return common._verify_job_evidence_until(delegation,slot_grant,approval,original_slot,current,current_owner,now,deadline)

def retained_owner_status(raw,owner,service_id,delegation_digest,observed_at):
    common.need(type(owner) is PinnedOwner)
    body=common.statement(raw,HEAD_DOMAIN,owner.public_key)
    common.fields(body,['version','owner_id','owner_epoch','tenant','service_id','delegation_digest','policy_revision',
        'revocation_epoch','signer_epoch','recipient_epoch','certificate_epoch','recipient_certificate_epoch','revoked','issued_at','expires_at'])
    common.need(type(body['version']) is int and body['version']==1,'service_policy_version');common.window(body,observed_at,300)
    common.need(body['owner_id']==owner.owner_id and body['owner_epoch']==owner.owner_epoch and body['tenant']==owner.tenant
        and body['service_id']==service_id and body['delegation_digest']==delegation_digest
        and observed_at<owner.not_after and body['expires_at']<=owner.not_after,'service_owner_mismatch')
    for name in ['policy_revision','revocation_epoch','signer_epoch','recipient_epoch','certificate_epoch','recipient_certificate_epoch']:common.integer(body[name])
    common.need(type(body['revoked']) is bool,'service_revoked')
    return body

def observe_owner_status(raw,delegation,now):
    # Retained facts never become a replacement fresh-head grant.
    common.need(type(delegation) is VerifiedDelegation)
    approved=delegation.body;delegation_shape(approved)
    body=retained_owner_status(raw,delegation.owner,approved['service_id'],delegation.digest,now)
    common.need(body['policy_revision']>=approved['policy_revision'] and body['revocation_epoch']>=approved['revocation_epoch']
        and body['signer_epoch']==approved['signer']['key_epoch'] and body['recipient_epoch']==approved['recipient']['key_epoch']
        and body['certificate_epoch']==approved['signer']['certificate_epoch'] and body['recipient_certificate_epoch']==approved['recipient']['certificate_epoch'],'service_policy_rollback')
    return body
