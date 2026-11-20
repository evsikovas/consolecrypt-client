"""Playbook-bound owner delegation v2; common v1 verifier is not a dispatch gate.

v1 remains immutable for historical negative evidence. Only its schema/common
signature/job/slot helpers are reused, never a v1 owner approval as v2 authority.
"""
import hashlib
from . import service_authority as common
from .service_authority import PinnedOwner,OriginalSlot,VerifiedDelegation,VerifiedJobEvidence,capture_original_slot,HEAD_DOMAIN,SLOT_DOMAIN
from consolecrypt_runner.snapshot import relative_path
from consolecrypt_runner.protocol import encode

DELEGATION_DOMAIN=b'ConsoleCrypt ServiceDelegation v2\0'

def delegation_shape(body):
    common.need(type(body) is dict and type(body.get('version')) is int and body['version']==2,
                'service_policy_version')
    common.need('playbook' in body and type(body['playbook']) is str,'service_playbook')
    selected=relative_path(body['playbook'])
    common.need(str(selected)==body['playbook'] and selected.suffix in ('.yml','.yaml'),'service_playbook')
    # Schema-only reuse: the required new field has been checked and only the
    # schema discriminant is adapted for the existing common bounds checker.
    # ALL remaining keys survive, so unknown fields still reject. The original
    # v2 bytes/domain are what get verified/hashed; no v1 signature is reused.
    old=dict(body);old.pop('playbook');old['version']=1
    common.delegation_shape(old)

def verify_delegation(raw,owner,now):
    common.need(type(owner) is PinnedOwner);common.integer(now,0,common.MAX_TIME)
    body=common.statement(raw,DELEGATION_DOMAIN,owner.public_key)
    delegation_shape(body);common.window(body,now,86400)
    common.need(body['signer']['public_key']!=owner.public_key.hex() and body['recipient']['public_key']!=owner.public_key.hex(),
                'service_owner_key_reuse')
    common.need(body['owner_id']==owner.owner_id and body['owner_epoch']==owner.owner_epoch and body['tenant']==owner.tenant,
                'service_owner_mismatch')
    common.need(body['expires_at']<=owner.not_after and body['policy_revision']>=owner.minimum_policy_revision
                and body['revocation_epoch']>=owner.minimum_revocation_epoch,'service_policy_rollback')
    return VerifiedDelegation(encode(body),hashlib.sha256(DELEGATION_DOMAIN+encode(body)).hexdigest(),owner)

def verify_job_evidence(delegation,trust_head,slot_grant,approval,original_slot,reference_context,current_owner,now):
    common.need(type(delegation) is VerifiedDelegation)
    body=delegation.body;delegation_shape(body)
    common.need(delegation.digest==hashlib.sha256(DELEGATION_DOMAIN+encode(body)).hexdigest(),'service_policy_version')
    common.need(type(reference_context) is dict and reference_context.get('playbook')==body['playbook'],'service_playbook')
    common.need(type(approval) is dict and type(approval.get('manifest')) is dict
                and approval['manifest'].get('playbook')==body['playbook'],'service_playbook')
    current=dict(reference_context);current.pop('playbook')
    # Existing verifier still performs every exact ref/route/credential/digest,
    # signature, fresh head, UTC slot, <=300s lease, mode/forks/isolation check.
    return common.verify_job_evidence(delegation,trust_head,slot_grant,approval,original_slot,current,current_owner,now)
