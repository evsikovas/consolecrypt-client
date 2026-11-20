"""Fixed explicit owner agreement; NEVER promotes service/Node enrollment.

This is a second purpose-bound signature, not another human, not a password
verifier and not permission derived from a JSON source/approval flag.
"""
import base64
import hashlib
import uuid

from cryptography.exceptions import InvalidSignature
from cryptography.hazmat.primitives.asymmetric.ed25519 import Ed25519PublicKey

from .errors import RunnerError, require
from .protocol import encode, uuid_field

OWNER_DOMAIN = b'ConsoleCrypt Production OwnerConfirmation v1\x00'
OWNER_POLICY = 'ownerConfirmationVaultPassphraseV1'
FIELDS = frozenset({'version', 'policy', 'signer_id', 'manifest_digest', 'signature'})
_ISSUER = object()


class ManualInteractiveGrant:
    """Opaque wrapper issued only after the separate fixed manual operation.

    Plain TrustGrant/configure_interactive, including its service-internal use,
    is intentionally insufficient. Nothing on the wire deserializes this type.
    """
    __slots__ = ('_grant',)

    def __init__(self, issuer, original):
        if issuer is not _ISSUER:
            raise TypeError('manual configuration issuer required')
        object.__setattr__(self, '_grant', original)

    def __setattr__(self, name, value):
        raise AttributeError('original manual grant is immutable')

    def __getattr__(self, name):
        return getattr(self._grant, name)


def configured_manual_owner(original):
    # Called ONLY by InteractiveExecutor.configure_manual_owner after comparing
    # every original enrollment field; no scope/grant replacement or renewal.
    if type(original) is ManualInteractiveGrant:
        return original
    return ManualInteractiveGrant(_ISSUER, original)


def original_manual_grant(original):
    return original._grant if type(original) is ManualInteractiveGrant else original


def verify_owner_confirmation(envelope, manifest, original_grant, signer, job_domain):
    require(type(original_grant) is ManualInteractiveGrant, 'additional_approval_required',
            'Production requires the original manual owner scope')
    proof = envelope.get('owner_confirmation')
    require(type(proof) is dict and set(proof) == FIELDS and type(proof['version']) is int
            and proof['version'] == 1 and proof['policy'] == OWNER_POLICY,
            'additional_approval_required', 'Invalid owner confirmation policy')
    require(uuid_field(proof['signer_id'], 'owner signer') == signer,
            'additional_approval_required', 'Owner confirmation signer differs')
    digest = hashlib.sha256(job_domain + encode(manifest)).digest()
    require(proof['manifest_digest'] == digest.hex(), 'additional_approval_required',
            'Owner confirmation differs from the frozen manifest')
    try:
        require(type(proof['signature']) is str and len(proof['signature']) == 88,
                'additional_approval_required', 'Invalid owner confirmation signature')
        signature = base64.b64decode(proof['signature'], validate=True)
        require(len(signature) == 64, 'additional_approval_required', 'Invalid owner confirmation signature')
        raw = OWNER_DOMAIN + b'\x01' + uuid.UUID(signer).bytes + digest
        Ed25519PublicKey.from_public_bytes(original_grant.public_key).verify(signature, raw)
    except (InvalidSignature, ValueError, TypeError) as error:
        raise RunnerError('additional_approval_required', 'Invalid owner confirmation signature') from error
