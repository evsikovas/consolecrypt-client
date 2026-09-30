//! Owner-online own-device admission, separate from personal vault trust.
//! A server directory, session login or unsigned status never authorizes a
//! device. The anchor confirms a whole-request code; the owner separately
//! proves target X25519 possession and signs an ordinary fresh-key rotation.
//! Callers must preserve grant/challenge checkpoints outside restored caches.
//! These pure primitives cannot detect a server withholding unseen revocation.

use crate::aead;
use crate::device::{random_x25519_secret, x25519, x25519_public};
use crate::kdf::hkdf_sha256_32;
use crate::rng::random_array;
use crate::sharing::{
    open_shared_revision, seal_shared_revision, shared_body_hash, sign_shared_manifest,
    sign_shared_mutation, verify_shared_manifest, verify_shared_mutation, SharingCryptoError,
    SharingOwnerAnchor, VerifiedSharingManifest, VerifiedSharingMutation,
};
use crate::{CryptoError, DevicePublicKeys, DeviceSecretKeys, ExposeSecret};
use cc_protocol::sharing::{
    RotateShareAccessRequest, SharedEncryptedBody, SharedRevision, SharingMember, SharingMutation,
    SharingOperation, SharingRole,
};
use cc_protocol::sharing_enrollment::*;
use cc_protocol::{Bytes, MutationId};
use ed25519_dalek::{Signature, VerifyingKey};
use sha2::{Digest, Sha256};
use std::fmt;
use subtle::ConstantTimeEq;
use uuid::Uuid;
use zeroize::Zeroizing;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum EnrollmentCryptoError {
    #[error(transparent)]
    Crypto(#[from] CryptoError),
    #[error(transparent)]
    Sharing(#[from] SharingCryptoError),
    #[error("invalid enrollment structure: {0}")]
    Structure(&'static str),
    #[error("enrollment differs from the pinned identity or access context")]
    Context,
    #[error("enrollment grant or challenge conflicts with its accepted predecessor")]
    Transition,
    #[error("enrollment is expired, inactive, exhausted or not yet valid")]
    Inactive,
    #[error("enrollment role exceeds the current anchor or signed ceiling")]
    Role,
    #[error("an independently compared whole-request code is required")]
    PairingRequired,
    #[error("the response does not prove the target encryption-key possession")]
    Possession,
}
type Result<T> = std::result::Result<T, EnrollmentCryptoError>;

fn structure<T>(value: std::result::Result<T, EnrollmentValidationError>) -> Result<T> {
    value.map_err(|error| EnrollmentCryptoError::Structure(error.0))
}
fn require(condition: bool, error: EnrollmentCryptoError) -> Result<()> {
    if condition {
        Ok(())
    } else {
        Err(error)
    }
}
fn digest(bytes: &[u8]) -> [u8; 32] {
    Sha256::digest(bytes).into()
}
fn bytes32(bytes: &Bytes) -> Result<[u8; 32]> {
    bytes.to_array().ok_or(EnrollmentCryptoError::Context)
}
fn signature(key: &[u8; 32], message: &[u8], signed: &Bytes) -> Result<()> {
    let key = VerifyingKey::from_bytes(key).map_err(|_| CryptoError::InvalidSignature)?;
    require(!key.is_weak(), CryptoError::InvalidSignature.into())?;
    let signature =
        Signature::from_slice(signed.as_slice()).map_err(|_| CryptoError::InvalidSignature)?;
    key.verify_strict(message, &signature)
        .map_err(|_| CryptoError::InvalidSignature.into())
}
fn device_keys(binding: &EnrollmentDeviceBinding) -> Result<DevicePublicKeys> {
    structure(validate_device_binding(binding))?;
    Ok(DevicePublicKeys::from_slices(
        binding.encryption_public_key.as_slice(),
        binding.signing_public_key.as_slice(),
    )?)
}
fn matches_member(binding: &EnrollmentDeviceBinding, member: &SharingMember) -> bool {
    binding.user_id == member.user_id
        && binding.device_id == member.device_id
        && binding.encryption_public_key == member.encryption_public_key
        && binding.signing_public_key == member.signing_public_key
}
fn role_allows(ceiling: SharingRole, requested: SharingRole) -> bool {
    ceiling == SharingRole::Editor || requested == SharingRole::Reader
}
fn scope_matches(scope: &EnrollmentScope, access: &VerifiedSharingManifest) -> bool {
    let manifest = access.manifest();
    scope.server_instance_id == manifest.server_instance_id
        && scope.share_id == manifest.share_id
        && scope.item_id == manifest.item_id
        && scope.kind == manifest.kind
}
fn trusted_owner(owner: &SharingOwnerAnchor, access: &VerifiedSharingManifest) -> Result<()> {
    let manifest = access.manifest();
    let member = access
        .member(owner.device_id)
        .ok_or(EnrollmentCryptoError::Context)?;
    require(
        manifest.owner_user_id == owner.user_id
            && manifest.owner_device_id == owner.device_id
            && member.user_id == owner.user_id
            && member.role == SharingRole::Editor
            && member.encryption_public_key == owner.public_keys.encryption_bytes()
            && member.signing_public_key == owner.public_keys.signing_bytes(),
        EnrollmentCryptoError::Context,
    )
}
fn owner_keys(keys: &DeviceSecretKeys, owner: &SharingOwnerAnchor) -> Result<()> {
    require(
        keys.public_keys() == owner.public_keys,
        EnrollmentCryptoError::Context,
    )
}
fn time_valid(start: i64, end: i64, now: i64) -> Result<()> {
    require(now >= start && now < end, EnrollmentCryptoError::Inactive)
}

#[derive(Debug, Clone)]
pub struct VerifiedEnrollmentGrant {
    signed: SignedSharingOwnDevicesGrantState,
    hash: [u8; 32],
}
#[derive(Debug, Clone)]
pub struct VerifiedEnrollmentRequest {
    signed: SignedSharingOwnDeviceRequest,
    hash: [u8; 32],
    grant: VerifiedEnrollmentGrant,
}
#[derive(Debug, Clone)]
pub struct VerifiedAnchorEndorsement {
    signed: SignedSharingAnchorEndorsement,
    hash: [u8; 32],
}
#[derive(Debug, Clone)]
pub struct VerifiedDeviceChallenge {
    signed: SignedSharingDeviceChallenge,
    hash: [u8; 32],
}
#[derive(Debug, Clone)]
pub struct VerifiedDeviceResponse {
    signed: SignedSharingDeviceChallengeResponse,
    hash: [u8; 32],
}
/// Only owner-memory verification against the original random material can
/// construct this wrapper. A server-provided response/status cannot mint it.
pub struct VerifiedDevicePossession {
    response: VerifiedDeviceResponse,
    challenge: VerifiedDeviceChallenge,
}
impl fmt::Debug for VerifiedDevicePossession {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("VerifiedDevicePossession(<redacted>)")
    }
}
/// Never serialize or persist this secret. Lock/restart/loss requires a new
/// challenge generation, even if the server says the old response was valid.
pub struct OwnerChallengeSecret {
    challenge: VerifiedDeviceChallenge,
    random: Zeroizing<[u8; 32]>,
}
impl fmt::Debug for OwnerChallengeSecret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("OwnerChallengeSecret(<redacted>)")
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EnrollmentChallengeCheckpoint {
    pub request_hash: [u8; 32],
    pub challenge_id: Uuid,
    pub generation: u64,
    pub hash: [u8; 32],
}
#[derive(Debug, Clone)]
pub struct VerifiedEnrollmentAcceptance {
    signed: SignedSharingOwnDeviceAcceptance,
    hash: [u8; 32],
}

macro_rules! accessors {
    ($ty:ty,$field:ident,$wire:ty) => {
        impl $ty {
            pub fn signed(&self) -> &$wire {
                &self.$field
            }
            pub fn hash(&self) -> [u8; 32] {
                self.hash
            }
        }
    };
}
accessors!(
    VerifiedEnrollmentGrant,
    signed,
    SignedSharingOwnDevicesGrantState
);
accessors!(
    VerifiedEnrollmentRequest,
    signed,
    SignedSharingOwnDeviceRequest
);
accessors!(
    VerifiedAnchorEndorsement,
    signed,
    SignedSharingAnchorEndorsement
);
accessors!(
    VerifiedDeviceChallenge,
    signed,
    SignedSharingDeviceChallenge
);
accessors!(
    VerifiedDeviceResponse,
    signed,
    SignedSharingDeviceChallengeResponse
);
accessors!(
    VerifiedEnrollmentAcceptance,
    signed,
    SignedSharingOwnDeviceAcceptance
);
impl VerifiedEnrollmentRequest {
    pub fn grant(&self) -> &VerifiedEnrollmentGrant {
        &self.grant
    }
}
impl VerifiedDeviceChallenge {
    pub fn checkpoint(&self) -> EnrollmentChallengeCheckpoint {
        EnrollmentChallengeCheckpoint {
            request_hash: bytes32(&self.signed.challenge.request_hash).expect("verified hash"),
            challenge_id: self.signed.challenge.challenge_id,
            generation: self.signed.challenge.generation,
            hash: self.hash,
        }
    }
}

/// Active renewal is legal only inside an exact pre/post access acceptance.
/// Terminal revoke is available for stale/expired grants without retargeting.
#[derive(Debug)]
pub enum GrantTransition<'a> {
    Genesis {
        access: &'a VerifiedSharingManifest,
    },
    Revoke {
        access: &'a VerifiedSharingManifest,
    },
    Acceptance {
        before: &'a VerifiedSharingManifest,
        after: &'a VerifiedSharingManifest,
        consumed: bool,
    },
}
fn immutable_grant(a: &SharingOwnDevicesGrantState, b: &SharingOwnDevicesGrantState) -> bool {
    a.format == b.format
        && a.scope == b.scope
        && a.owner_user_id == b.owner_user_id
        && a.owner_device_id == b.owner_device_id
        && a.grant_id == b.grant_id
        && a.anchor == b.anchor
        && a.role_ceiling == b.role_ceiling
        && a.mode == b.mode
        && a.not_before == b.not_before
        && a.expires_at == b.expires_at
        && a.max_admissions == b.max_admissions
}
fn bound_access(
    grant: &SharingOwnDevicesGrantState,
    access: &VerifiedSharingManifest,
) -> Result<()> {
    require(
        scope_matches(&grant.scope, access)
            && grant.access_epoch == access.manifest().access_epoch
            && grant.access_manifest_hash.as_slice() == access.hash(),
        EnrollmentCryptoError::Context,
    )?;
    let anchor = access
        .member(grant.anchor.device_id)
        .ok_or(EnrollmentCryptoError::Context)?;
    require(
        matches_member(&grant.anchor, anchor) && role_allows(anchor.role, grant.role_ceiling),
        EnrollmentCryptoError::Role,
    )
}
pub fn verify_grant_state(
    signed: &SignedSharingOwnDevicesGrantState,
    owner: &SharingOwnerAnchor,
    previous: Option<&VerifiedEnrollmentGrant>,
    transition: GrantTransition<'_>,
    now: i64,
) -> Result<VerifiedEnrollmentGrant> {
    let grant = &signed.grant;
    structure(validate_signed_grant(signed))?;
    require(
        grant.owner_user_id == owner.user_id && grant.owner_device_id == owner.device_id,
        EnrollmentCryptoError::Context,
    )?;
    signature(
        &owner.public_keys.signing,
        &structure(enrollment_grant_message(grant))?,
        &signed.signature,
    )?;
    let hash = digest(&structure(enrollment_grant_hash_input(signed))?);
    let access = match &transition {
        GrantTransition::Genesis { access } | GrantTransition::Revoke { access } => *access,
        GrantTransition::Acceptance { before, .. } => *before,
    };
    trusted_owner(owner, access)?;
    require(
        scope_matches(&grant.scope, access),
        EnrollmentCryptoError::Context,
    )?;
    if let Some(previous) = previous {
        let prior = &previous.signed.grant;
        require(
            immutable_grant(prior, grant),
            EnrollmentCryptoError::Transition,
        )?;
        if grant.grant_revision == prior.grant_revision {
            require(hash == previous.hash, EnrollmentCryptoError::Transition)?;
            return Ok(previous.clone());
        }
        require(
            prior.status == EnrollmentGrantStatus::Active
                && prior.grant_revision.checked_add(1) == Some(grant.grant_revision)
                && grant.previous_grant_state_hash.as_slice() == previous.hash,
            EnrollmentCryptoError::Transition,
        )?;
        match transition {
            GrantTransition::Genesis { .. } => return Err(EnrollmentCryptoError::Transition),
            GrantTransition::Revoke { .. } => require(
                grant.status == EnrollmentGrantStatus::Revoked
                    && grant.admitted_count == prior.admitted_count
                    && grant.access_epoch == prior.access_epoch
                    && grant.access_manifest_hash == prior.access_manifest_hash,
                EnrollmentCryptoError::Transition,
            )?,
            GrantTransition::Acceptance {
                before,
                after,
                consumed,
            } => {
                time_valid(prior.not_before, prior.expires_at, now)?;
                bound_access(prior, before)?;
                trusted_owner(owner, after)?;
                require(
                    scope_matches(&grant.scope, after)
                        && before.manifest().revision.checked_add(1)
                            == Some(after.manifest().revision)
                        && before.manifest().access_epoch.checked_add(1)
                            == Some(after.manifest().access_epoch)
                        && after.manifest().previous_manifest_hash.as_slice() == before.hash(),
                    EnrollmentCryptoError::Transition,
                )?;
                bound_access(grant, after)?;
                let count = prior
                    .admitted_count
                    .checked_add(u32::from(consumed))
                    .ok_or(EnrollmentCryptoError::Transition)?;
                require(
                    grant.admitted_count == count
                        && (consumed || grant.status == EnrollmentGrantStatus::Active)
                        && (grant.status == EnrollmentGrantStatus::Revoked
                            || count < grant.max_admissions),
                    EnrollmentCryptoError::Transition,
                )?;
            }
        }
    } else {
        require(
            matches!(transition, GrantTransition::Genesis { .. }) && grant.grant_revision == 1,
            EnrollmentCryptoError::Transition,
        )?;
        bound_access(grant, access)?;
        time_valid(grant.not_before, grant.expires_at, now)?;
    }
    Ok(VerifiedEnrollmentGrant {
        signed: signed.clone(),
        hash,
    })
}
pub fn sign_grant_state(
    keys: &DeviceSecretKeys,
    grant: SharingOwnDevicesGrantState,
    owner: &SharingOwnerAnchor,
    previous: Option<&VerifiedEnrollmentGrant>,
    transition: GrantTransition<'_>,
    now: i64,
) -> Result<SignedSharingOwnDevicesGrantState> {
    owner_keys(keys, owner)?;
    let signature = Bytes::from(keys.sign_message(&structure(enrollment_grant_message(&grant))?));
    let signed = SignedSharingOwnDevicesGrantState { grant, signature };
    verify_grant_state(&signed, owner, previous, transition, now)?;
    Ok(signed)
}
fn active_request(
    request: &VerifiedEnrollmentRequest,
    current: &VerifiedSharingManifest,
    now: i64,
) -> Result<()> {
    let grant = &request.grant.signed.grant;
    let value = &request.signed.request;
    require(
        grant.status == EnrollmentGrantStatus::Active
            && grant.admitted_count < grant.max_admissions,
        EnrollmentCryptoError::Inactive,
    )?;
    time_valid(grant.not_before, grant.expires_at, now)?;
    time_valid(value.not_before, value.expires_at, now)?;
    bound_access(grant, current)?;
    require(
        current.member(value.target.device_id).is_none(),
        EnrollmentCryptoError::Context,
    )?;
    let anchor = current
        .member(grant.anchor.device_id)
        .ok_or(EnrollmentCryptoError::Context)?;
    require(
        role_allows(anchor.role, value.requested_role)
            && role_allows(grant.role_ceiling, value.requested_role),
        EnrollmentCryptoError::Role,
    )
}
pub fn verify_device_request(
    signed: &SignedSharingOwnDeviceRequest,
    grant: &VerifiedEnrollmentGrant,
    current: &VerifiedSharingManifest,
    now: i64,
) -> Result<VerifiedEnrollmentRequest> {
    structure(validate_signed_request(signed))?;
    structure(validate_request_for_grant(
        &signed.request,
        &grant.signed.grant,
    ))?;
    require(
        signed.request.grant_state_hash.as_slice() == grant.hash,
        EnrollmentCryptoError::Context,
    )?;
    let keys = device_keys(&signed.request.target)?;
    signature(
        &keys.signing,
        &structure(enrollment_request_message(&signed.request))?,
        &signed.signature,
    )?;
    let verified = VerifiedEnrollmentRequest {
        signed: signed.clone(),
        hash: digest(&structure(enrollment_request_hash_input(signed))?),
        grant: grant.clone(),
    };
    active_request(&verified, current, now)?;
    Ok(verified)
}
pub fn sign_device_request(
    keys: &DeviceSecretKeys,
    request: SharingOwnDeviceRequest,
    grant: &VerifiedEnrollmentGrant,
    current: &VerifiedSharingManifest,
    now: i64,
) -> Result<SignedSharingOwnDeviceRequest> {
    require(
        keys.public_keys() == device_keys(&request.target)?,
        EnrollmentCryptoError::Context,
    )?;
    let signature =
        Bytes::from(keys.sign_message(&structure(enrollment_request_message(&request))?));
    let signed = SignedSharingOwnDeviceRequest { request, signature };
    verify_device_request(&signed, grant, current, now)?;
    Ok(signed)
}
pub fn enrollment_pairing_code(request: &VerifiedEnrollmentRequest) -> Result<String> {
    let hash = digest(&structure(enrollment_pairing_message(&request.hash))?);
    let mut result = String::with_capacity(71);
    for (i, byte) in hash.iter().enumerate() {
        if i > 0 && i % 4 == 0 {
            result.push('-');
        }
        use std::fmt::Write as _;
        write!(result, "{byte:02X}").expect("String formatting cannot fail");
    }
    Ok(result)
}
pub fn verify_anchor_endorsement(
    signed: &SignedSharingAnchorEndorsement,
    request: &VerifiedEnrollmentRequest,
    current: &VerifiedSharingManifest,
    now: i64,
) -> Result<VerifiedAnchorEndorsement> {
    active_request(request, current, now)?;
    structure(validate_signed_endorsement(signed))?;
    let anchor = &request.grant.signed.grant.anchor;
    require(
        signed.endorsement.request_hash.as_slice() == request.hash
            && signed.endorsement.anchor_device_id == anchor.device_id,
        EnrollmentCryptoError::Context,
    )?;
    signature(
        &device_keys(anchor)?.signing,
        &structure(enrollment_endorsement_message(&signed.endorsement))?,
        &signed.signature,
    )?;
    Ok(VerifiedAnchorEndorsement {
        signed: signed.clone(),
        hash: digest(&structure(enrollment_endorsement_hash_input(signed))?),
    })
}
pub fn endorse_device_request(
    keys: &DeviceSecretKeys,
    request: &VerifiedEnrollmentRequest,
    current: &VerifiedSharingManifest,
    confirmed_code: &str,
    now: i64,
) -> Result<SignedSharingAnchorEndorsement> {
    require(
        !confirmed_code.is_empty() && confirmed_code == enrollment_pairing_code(request)?,
        EnrollmentCryptoError::PairingRequired,
    )?;
    let anchor = &request.grant.signed.grant.anchor;
    require(
        keys.public_keys() == device_keys(anchor)?,
        EnrollmentCryptoError::Context,
    )?;
    let endorsement = SharingAnchorEndorsement {
        format: FORMAT,
        request_hash: Bytes::from(request.hash),
        anchor_device_id: anchor.device_id,
    };
    let signature =
        Bytes::from(keys.sign_message(&structure(enrollment_endorsement_message(&endorsement))?));
    let signed = SignedSharingAnchorEndorsement {
        endorsement,
        signature,
    };
    verify_anchor_endorsement(&signed, request, current, now)?;
    Ok(signed)
}
fn challenge_key(
    shared: &crate::keys::Secret32,
    challenge: &SharingDeviceChallenge,
    target: &EnrollmentDeviceBinding,
) -> Result<crate::keys::Secret32> {
    let mut salt = [0u8; 64];
    salt[..32].copy_from_slice(challenge.ephemeral_public_key.as_slice());
    salt[32..].copy_from_slice(target.encryption_public_key.as_slice());
    let info = structure(enrollment_challenge_key_info(challenge, target))?;
    Ok(hkdf_sha256_32(shared.expose_secret(), &salt, &[&info]))
}
fn challenge_links(
    signed: &SignedSharingDeviceChallenge,
    owner: &SharingOwnerAnchor,
    request: &VerifiedEnrollmentRequest,
    endorsement: &VerifiedAnchorEndorsement,
    current: &VerifiedSharingManifest,
) -> Result<VerifiedDeviceChallenge> {
    trusted_owner(owner, current)?;
    structure(validate_signed_challenge(signed))?;
    structure(validate_challenge_for_request(
        &signed.challenge,
        &request.signed.request,
    ))?;
    require(
        signed.challenge.request_hash.as_slice() == request.hash
            && signed.challenge.anchor_endorsement_hash.as_slice() == endorsement.hash
            && endorsement.signed.endorsement.request_hash.as_slice() == request.hash,
        EnrollmentCryptoError::Context,
    )?;
    signature(
        &owner.public_keys.signing,
        &structure(enrollment_challenge_message(&signed.challenge))?,
        &signed.signature,
    )?;
    Ok(VerifiedDeviceChallenge {
        signed: signed.clone(),
        hash: digest(&structure(enrollment_challenge_hash_input(signed))?),
    })
}
pub fn verify_device_challenge(
    signed: &SignedSharingDeviceChallenge,
    owner: &SharingOwnerAnchor,
    request: &VerifiedEnrollmentRequest,
    endorsement: &VerifiedAnchorEndorsement,
    current: &VerifiedSharingManifest,
    now: i64,
    previous: Option<&EnrollmentChallengeCheckpoint>,
) -> Result<VerifiedDeviceChallenge> {
    active_request(request, current, now)?;
    let verified = challenge_links(signed, owner, request, endorsement, current)?;
    time_valid(
        signed.challenge.not_before,
        signed.challenge.expires_at,
        now,
    )?;
    if let Some(previous) = previous {
        require(
            previous.request_hash == request.hash
                && !previous.challenge_id.is_nil()
                && previous.generation > 0
                && previous.hash != [0; 32],
            EnrollmentCryptoError::Transition,
        )?;
        let exact = signed.challenge.generation == previous.generation
            && verified.hash == previous.hash
            && signed.challenge.challenge_id == previous.challenge_id;
        let successor = previous.generation.checked_add(1) == Some(signed.challenge.generation)
            && signed.challenge.challenge_id != previous.challenge_id;
        require(exact || successor, EnrollmentCryptoError::Transition)?;
    }
    Ok(verified)
}
pub fn create_device_challenge(
    keys: &DeviceSecretKeys,
    owner: &SharingOwnerAnchor,
    request: &VerifiedEnrollmentRequest,
    endorsement: &VerifiedAnchorEndorsement,
    current: &VerifiedSharingManifest,
    now: i64,
    previous: Option<&EnrollmentChallengeCheckpoint>,
) -> Result<(SignedSharingDeviceChallenge, OwnerChallengeSecret)> {
    owner_keys(keys, owner)?;
    active_request(request, current, now)?;
    verify_anchor_endorsement(endorsement.signed(), request, current, now)?;
    let ephemeral = random_x25519_secret()?;
    let public = x25519_public(&ephemeral);
    let random = Zeroizing::new(random_array::<32>()?);
    let target = &request.signed.request.target;
    let shared = x25519(&ephemeral, &bytes32(&target.encryption_public_key)?)?;
    let generation = previous
        .map_or(Some(1), |prior| prior.generation.checked_add(1))
        .ok_or(EnrollmentCryptoError::Transition)?;
    let mut challenge = SharingDeviceChallenge {
        format: FORMAT,
        request_hash: Bytes::from(request.hash),
        anchor_endorsement_hash: Bytes::from(endorsement.hash),
        challenge_id: Uuid::new_v4(),
        generation,
        not_before: now,
        expires_at: now
            .checked_add(MAX_CHALLENGE_LIFETIME_SECONDS)
            .ok_or(EnrollmentCryptoError::Inactive)?
            .min(request.signed.request.expires_at),
        ephemeral_public_key: Bytes::from(public),
        nonce: Bytes::from(random_array::<24>()?),
        ciphertext: Bytes::from(Vec::new()),
    };
    let key = challenge_key(&shared, &challenge, target)?;
    let aad = structure(enrollment_challenge_aad(&challenge))?;
    challenge.ciphertext = Bytes::from(aead::seal(
        key.expose_secret(),
        &challenge
            .nonce
            .to_array()
            .ok_or(EnrollmentCryptoError::Context)?,
        &aad,
        random.as_ref(),
    )?);
    let signature =
        Bytes::from(keys.sign_message(&structure(enrollment_challenge_message(&challenge))?));
    let signed = SignedSharingDeviceChallenge {
        challenge,
        signature,
    };
    let verified =
        verify_device_challenge(&signed, owner, request, endorsement, current, now, previous)?;
    Ok((
        signed,
        OwnerChallengeSecret {
            challenge: verified,
            random,
        },
    ))
}
fn response_digest(challenge_hash: &[u8; 32], random: &[u8]) -> [u8; 32] {
    let mut input = Zeroizing::new(Vec::with_capacity(labels::RESPONSE_DIGEST.len() + 64));
    input.extend_from_slice(labels::RESPONSE_DIGEST);
    input.extend_from_slice(challenge_hash);
    input.extend_from_slice(random);
    digest(&input)
}
#[allow(clippy::too_many_arguments)]
pub fn answer_device_challenge(
    keys: &DeviceSecretKeys,
    owner: &SharingOwnerAnchor,
    request: &VerifiedEnrollmentRequest,
    endorsement: &VerifiedAnchorEndorsement,
    current: &VerifiedSharingManifest,
    signed: &SignedSharingDeviceChallenge,
    now: i64,
    previous: Option<&EnrollmentChallengeCheckpoint>,
) -> Result<SignedSharingDeviceChallengeResponse> {
    require(
        keys.public_keys() == device_keys(&request.signed.request.target)?,
        EnrollmentCryptoError::Context,
    )?;
    let challenge =
        verify_device_challenge(signed, owner, request, endorsement, current, now, previous)?;
    let shared = keys.diffie_hellman(&bytes32(&signed.challenge.ephemeral_public_key)?)?;
    let key = challenge_key(&shared, &signed.challenge, &request.signed.request.target)?;
    let random = aead::open(
        key.expose_secret(),
        &signed
            .challenge
            .nonce
            .to_array()
            .ok_or(EnrollmentCryptoError::Context)?,
        &structure(enrollment_challenge_aad(&signed.challenge))?,
        signed.challenge.ciphertext.as_slice(),
    )?;
    require(random.len() == 32, EnrollmentCryptoError::Possession)?;
    let response = SharingDeviceChallengeResponse {
        format: FORMAT,
        request_hash: Bytes::from(request.hash),
        challenge_hash: Bytes::from(challenge.hash),
        response_digest: Bytes::from(response_digest(&challenge.hash, &random)),
    };
    let signature =
        Bytes::from(keys.sign_message(&structure(enrollment_response_message(&response))?));
    Ok(SignedSharingDeviceChallengeResponse {
        response,
        signature,
    })
}
fn response_links(
    signed: &SignedSharingDeviceChallengeResponse,
    request: &VerifiedEnrollmentRequest,
    challenge: &VerifiedDeviceChallenge,
) -> Result<VerifiedDeviceResponse> {
    structure(validate_signed_response(signed))?;
    require(
        signed.response.request_hash.as_slice() == request.hash
            && signed.response.challenge_hash.as_slice() == challenge.hash,
        EnrollmentCryptoError::Context,
    )?;
    signature(
        &device_keys(&request.signed.request.target)?.signing,
        &structure(enrollment_response_message(&signed.response))?,
        &signed.signature,
    )?;
    Ok(VerifiedDeviceResponse {
        signed: signed.clone(),
        hash: digest(&structure(enrollment_response_hash_input(signed))?),
    })
}
pub fn verify_device_possession(
    signed: &SignedSharingDeviceChallengeResponse,
    pending: &OwnerChallengeSecret,
    request: &VerifiedEnrollmentRequest,
    current: &VerifiedSharingManifest,
    now: i64,
    latest: &EnrollmentChallengeCheckpoint,
) -> Result<VerifiedDevicePossession> {
    active_request(request, current, now)?;
    let challenge = &pending.challenge;
    require(
        challenge.checkpoint() == *latest,
        EnrollmentCryptoError::Transition,
    )?;
    time_valid(
        challenge.signed.challenge.not_before,
        challenge.signed.challenge.expires_at,
        now,
    )?;
    let response = response_links(signed, request, challenge)?;
    let expected = Zeroizing::new(response_digest(&challenge.hash, pending.random.as_ref()));
    require(
        bool::from(
            expected
                .as_ref()
                .ct_eq(signed.response.response_digest.as_slice()),
        ),
        EnrollmentCryptoError::Possession,
    )?;
    Ok(VerifiedDevicePossession {
        response,
        challenge: challenge.clone(),
    })
}

impl VerifiedDevicePossession {
    pub fn response(&self) -> &VerifiedDeviceResponse {
        &self.response
    }
    pub fn challenge(&self) -> &VerifiedDeviceChallenge {
        &self.challenge
    }
}

/// Signature/context verification alone does not prove X25519 possession.
/// Only the owner-memory verification above constructs VerifiedDevicePossession.
pub fn verify_device_response(
    signed: &SignedSharingDeviceChallengeResponse,
    request: &VerifiedEnrollmentRequest,
    challenge: &VerifiedDeviceChallenge,
    current: &VerifiedSharingManifest,
    now: i64,
) -> Result<VerifiedDeviceResponse> {
    active_request(request, current, now)?;
    time_valid(
        challenge.signed.challenge.not_before,
        challenge.signed.challenge.expires_at,
        now,
    )?;
    response_links(signed, request, challenge)
}

fn successor_grant(
    keys: &DeviceSecretKeys,
    owner: &SharingOwnerAnchor,
    prior: &VerifiedEnrollmentGrant,
    before: &VerifiedSharingManifest,
    after: &VerifiedSharingManifest,
    consumed: bool,
    now: i64,
) -> Result<SignedSharingOwnDevicesGrantState> {
    let mut grant = prior.signed.grant.clone();
    grant.grant_revision = grant
        .grant_revision
        .checked_add(1)
        .ok_or(EnrollmentCryptoError::Transition)?;
    grant.previous_grant_state_hash = Bytes::from(prior.hash);
    grant.access_manifest_hash = Bytes::from(after.hash());
    grant.access_epoch = after.manifest().access_epoch;
    grant.admitted_count = grant
        .admitted_count
        .checked_add(u32::from(consumed))
        .ok_or(EnrollmentCryptoError::Transition)?;
    if grant.admitted_count >= grant.max_admissions {
        grant.status = EnrollmentGrantStatus::Revoked;
    }
    sign_grant_state(
        keys,
        grant,
        owner,
        Some(prior),
        GrantTransition::Acceptance {
            before,
            after,
            consumed,
        },
        now,
    )
}

/// Owner-only builder. Opens the authenticated existing body itself, then
/// invokes the ordinary v1 fresh-DEK sealer. Callers cannot supply replacement
/// plaintext, an old DEK, an ACL delta, an unsigned proof or an owner successor.
#[allow(clippy::too_many_arguments)]
pub fn accept_own_device(
    keys: &DeviceSecretKeys,
    owner: &SharingOwnerAnchor,
    before: &VerifiedSharingManifest,
    previous: &VerifiedSharingMutation,
    body: &SharedEncryptedBody,
    request: &VerifiedEnrollmentRequest,
    endorsement: &VerifiedAnchorEndorsement,
    possession: &VerifiedDevicePossession,
    latest: &EnrollmentChallengeCheckpoint,
    latest_grant: &VerifiedEnrollmentGrant,
    other_grants: &[VerifiedEnrollmentGrant],
    now: i64,
) -> Result<AcceptOwnDeviceRequest> {
    owner_keys(keys, owner)?;
    trusted_owner(owner, before)?;
    // A previously verified request/proof is not an authorization cache.
    // The caller must supply the current durable grant head at final signing.
    require(
        latest_grant.hash == request.grant.hash
            && latest_grant.signed == request.grant.signed
            && latest_grant.signed.grant.status == EnrollmentGrantStatus::Active,
        EnrollmentCryptoError::Transition,
    )?;
    active_request(request, before, now)?;
    require(
        possession.challenge.checkpoint() == *latest,
        EnrollmentCryptoError::Transition,
    )?;
    verify_anchor_endorsement(endorsement.signed(), request, before, now)?;
    require(
        possession
            .challenge
            .signed
            .challenge
            .request_hash
            .as_slice()
            == request.hash
            && possession
                .challenge
                .signed
                .challenge
                .anchor_endorsement_hash
                .as_slice()
                == endorsement.hash,
        EnrollmentCryptoError::Context,
    )?;
    time_valid(
        possession.challenge.signed.challenge.not_before,
        possession.challenge.signed.challenge.expires_at,
        now,
    )?;
    let plain = open_shared_revision(before, previous, Some(body), owner.device_id, keys)?
        .ok_or(EnrollmentCryptoError::Context)?;
    let mut context = previous.mutation().context.clone();
    require(
        context.server_instance_id == before.manifest().server_instance_id
            && context.share_id == before.manifest().share_id
            && context.item_id == before.manifest().item_id
            && context.kind == before.manifest().kind
            && previous.mutation().manifest_hash.as_slice() == before.hash(),
        EnrollmentCryptoError::Context,
    )?;
    context.revision = context
        .revision
        .checked_add(1)
        .ok_or(EnrollmentCryptoError::Transition)?;
    context.access_epoch = context
        .access_epoch
        .checked_add(1)
        .ok_or(EnrollmentCryptoError::Transition)?;
    let mut manifest = before.manifest().clone();
    manifest.revision = manifest
        .revision
        .checked_add(1)
        .ok_or(EnrollmentCryptoError::Transition)?;
    manifest.access_epoch = context.access_epoch;
    manifest.previous_manifest_hash = Bytes::from(before.hash());
    let target = &request.signed.request.target;
    manifest.members.push(SharingMember {
        user_id: target.user_id,
        device_id: target.device_id,
        encryption_public_key: target.encryption_public_key.clone(),
        signing_public_key: target.signing_public_key.clone(),
        role: request.signed.request.requested_role,
    });
    let access = sign_shared_manifest(keys, manifest, &context, owner, Some(&before.checkpoint()))?;
    let after = verify_shared_manifest(&access, &context, owner, Some(&before.checkpoint()))?;
    let fresh_body = seal_shared_revision(&after, &context, plain.as_slice())?;
    let mutation = SharingMutation {
        context,
        mutation_id: MutationId::new(),
        base_revision: previous.mutation().context.revision,
        manifest_revision: after.manifest().revision,
        manifest_hash: Bytes::from(after.hash()),
        writer_device_id: owner.device_id,
        previous_revision_hash: Bytes::from(previous.checkpoint().hash),
        operation: SharingOperation::Put,
        body_hash: Bytes::from(shared_body_hash(&fresh_body)?),
    };
    let signed = sign_shared_mutation(
        &after,
        keys,
        mutation,
        Some(&fresh_body),
        Some(&previous.checkpoint()),
    )?;
    let after_revision = verify_shared_mutation(&after, &signed, Some(&previous.checkpoint()))?;
    let consumed = successor_grant(keys, owner, &request.grant, before, &after, true, now)?;
    require(
        other_grants.len() <= MAX_OTHER_GRANT_SUCCESSORS,
        EnrollmentCryptoError::Structure("other grants"),
    )?;
    let mut others = Vec::with_capacity(other_grants.len());
    for prior in other_grants {
        require(
            prior.signed.grant.grant_id != request.grant.signed.grant.grant_id,
            EnrollmentCryptoError::Transition,
        )?;
        others.push(successor_grant(
            keys, owner, prior, before, &after, false, now,
        )?);
    }
    others.sort_by_key(|grant| grant.grant.grant_id);
    require(
        others
            .windows(2)
            .all(|pair| pair[0].grant.grant_id != pair[1].grant.grant_id),
        EnrollmentCryptoError::Transition,
    )?;
    let acceptance = SharingOwnDeviceAcceptance {
        format: FORMAT,
        request_hash: Bytes::from(request.hash),
        anchor_endorsement_hash: Bytes::from(endorsement.hash),
        challenge_hash: Bytes::from(possession.challenge.hash),
        response_hash: Bytes::from(possession.response.hash),
        consumed_grant_state_hash: Bytes::from(request.grant.hash),
        result_access_manifest_hash: Bytes::from(after.hash()),
        result_revision_hash: Bytes::from(after_revision.checkpoint().hash),
        consumed_grant_successor_hash: Bytes::from(digest(&structure(
            enrollment_grant_hash_input(&consumed),
        )?)),
        other_grant_successor_hashes: others
            .iter()
            .map(|grant| {
                Ok(EnrollmentGrantSuccessorHash {
                    grant_id: grant.grant.grant_id,
                    state_hash: Bytes::from(digest(&structure(enrollment_grant_hash_input(
                        grant,
                    ))?)),
                })
            })
            .collect::<Result<_>>()?,
    };
    let signature =
        Bytes::from(keys.sign_message(&structure(enrollment_acceptance_message(&acceptance))?));
    let result = AcceptOwnDeviceRequest {
        rotation: RotateShareAccessRequest {
            access,
            revision: SharedRevision {
                signed,
                body: Some(fresh_body),
            },
        },
        acceptance: SignedSharingOwnDeviceAcceptance {
            acceptance,
            signature,
        },
        consumed_grant_successor: consumed,
        other_grant_successors: others,
    };
    verify_own_device_acceptance(
        &result,
        owner,
        before,
        previous,
        request,
        endorsement,
        &possession.challenge,
        &possession.response,
        other_grants,
        now,
    )?;
    Ok(result)
}

/// Authenticate public receipt metadata with an independently pinned owner.
/// This verifies only the owner's signature and canonical structure. It does
/// not establish possession, current grant authorization, chain freshness or
/// the referenced result body; activation uses the complete verifier below.
pub fn verify_acceptance_signature(
    signed: &SignedSharingOwnDeviceAcceptance,
    owner: &SharingOwnerAnchor,
) -> Result<VerifiedEnrollmentAcceptance> {
    structure(validate_signed_acceptance(signed))?;
    signature(
        &owner.public_keys.signing,
        &structure(enrollment_acceptance_message(&signed.acceptance))?,
        &signed.signature,
    )?;
    Ok(VerifiedEnrollmentAcceptance {
        signed: signed.clone(),
        hash: digest(&structure(enrollment_acceptance_hash_input(signed))?),
    })
}

/// Verify a signed receipt/rotation. This proves the owner's signed decision,
/// not independent possession to a recipient: only verify_device_possession
/// checks the owner's secret. Historical receipt verification may use its
/// original acceptance time, while activation must use the current clock.
#[allow(clippy::too_many_arguments)]
pub fn verify_own_device_acceptance(
    value: &AcceptOwnDeviceRequest,
    owner: &SharingOwnerAnchor,
    before: &VerifiedSharingManifest,
    previous: &VerifiedSharingMutation,
    request: &VerifiedEnrollmentRequest,
    endorsement: &VerifiedAnchorEndorsement,
    challenge: &VerifiedDeviceChallenge,
    response: &VerifiedDeviceResponse,
    other_grants: &[VerifiedEnrollmentGrant],
    now: i64,
) -> Result<VerifiedEnrollmentAcceptance> {
    structure(validate_accept_request(value))?;
    trusted_owner(owner, before)?;
    active_request(request, before, now)?;
    verify_anchor_endorsement(endorsement.signed(), request, before, now)?;
    challenge_links(challenge.signed(), owner, request, endorsement, before)?;
    time_valid(
        challenge.signed.challenge.not_before,
        challenge.signed.challenge.expires_at,
        now,
    )?;
    response_links(response.signed(), request, challenge)?;
    let context = &value.rotation.revision.signed.mutation.context;
    let after = verify_shared_manifest(
        &value.rotation.access,
        context,
        owner,
        Some(&before.checkpoint()),
    )?;
    let revision = verify_shared_mutation(
        &after,
        &value.rotation.revision.signed,
        Some(&previous.checkpoint()),
    )?;
    require(
        revision.mutation().operation == SharingOperation::Put
            && revision.mutation().writer_device_id == owner.device_id,
        EnrollmentCryptoError::Context,
    )?;
    let body = value
        .rotation
        .revision
        .body
        .as_ref()
        .ok_or(EnrollmentCryptoError::Context)?;
    require(
        shared_body_hash(body)?.as_slice() == revision.mutation().body_hash.as_slice(),
        EnrollmentCryptoError::Context,
    )?;
    let target = &request.signed.request.target;
    require(
        after.manifest().members.len() == before.manifest().members.len() + 1,
        EnrollmentCryptoError::Transition,
    )?;
    for member in &before.manifest().members {
        require(
            after.member(member.device_id) == Some(member),
            EnrollmentCryptoError::Transition,
        )?;
    }
    let added = after
        .member(target.device_id)
        .ok_or(EnrollmentCryptoError::Transition)?;
    require(
        matches_member(target, added) && added.role == request.signed.request.requested_role,
        EnrollmentCryptoError::Transition,
    )?;
    verify_grant_state(
        &value.consumed_grant_successor,
        owner,
        Some(&request.grant),
        GrantTransition::Acceptance {
            before,
            after: &after,
            consumed: true,
        },
        now,
    )?;
    require(
        other_grants.len() == value.other_grant_successors.len(),
        EnrollmentCryptoError::Transition,
    )?;
    let ordered_prior = other_grants
        .iter()
        .map(|prior| (prior.signed.grant.grant_id, prior))
        .collect::<std::collections::BTreeMap<_, _>>();
    require(
        ordered_prior.len() == other_grants.len()
            && !ordered_prior.contains_key(&request.grant.signed.grant.grant_id),
        EnrollmentCryptoError::Transition,
    )?;
    for (prior, successor) in ordered_prior.values().zip(&value.other_grant_successors) {
        verify_grant_state(
            successor,
            owner,
            Some(prior),
            GrantTransition::Acceptance {
                before,
                after: &after,
                consumed: false,
            },
            now,
        )?;
    }
    let acceptance = &value.acceptance.acceptance;
    require(
        acceptance.request_hash.as_slice() == request.hash
            && acceptance.anchor_endorsement_hash.as_slice() == endorsement.hash
            && acceptance.challenge_hash.as_slice() == challenge.hash
            && acceptance.response_hash.as_slice() == response.hash
            && acceptance.consumed_grant_state_hash.as_slice() == request.grant.hash
            && acceptance.result_access_manifest_hash.as_slice() == after.hash()
            && acceptance.result_revision_hash.as_slice() == revision.checkpoint().hash
            && acceptance.consumed_grant_successor_hash.as_slice()
                == digest(&structure(enrollment_grant_hash_input(
                    &value.consumed_grant_successor,
                ))?),
        EnrollmentCryptoError::Context,
    )?;
    for (expected, successor) in acceptance
        .other_grant_successor_hashes
        .iter()
        .zip(&value.other_grant_successors)
    {
        require(
            expected.grant_id == successor.grant.grant_id
                && expected.state_hash.as_slice()
                    == digest(&structure(enrollment_grant_hash_input(successor))?),
            EnrollmentCryptoError::Context,
        )?;
    }
    signature(
        &owner.public_keys.signing,
        &structure(enrollment_acceptance_message(acceptance))?,
        &value.acceptance.signature,
    )?;
    Ok(VerifiedEnrollmentAcceptance {
        signed: value.acceptance.clone(),
        hash: digest(&structure(enrollment_acceptance_hash_input(
            &value.acceptance,
        ))?),
    })
}

#[cfg(test)]
mod adversarial_tests {
    use super::*;
    use crate::sharing::sign_shared_manifest;
    use cc_protocol::sharing::{AccessManifest, SharedItemKind, SharingContext};
    use cc_protocol::{DeviceId, ObjectId, ShareId, UserId};

    struct Setup {
        owner: DeviceSecretKeys,
        target: DeviceSecretKeys,
        pin: SharingOwnerAnchor,
        access: VerifiedSharingManifest,
        request: VerifiedEnrollmentRequest,
        endorsement: VerifiedAnchorEndorsement,
        now: i64,
    }
    fn setup(low_order_target: bool) -> Setup {
        let owner = DeviceSecretKeys::generate().unwrap();
        let target = DeviceSecretKeys::generate().unwrap();
        let anchor = DeviceSecretKeys::generate().unwrap();
        let pin = SharingOwnerAnchor {
            user_id: UserId::new(),
            device_id: DeviceId::new(),
            public_keys: owner.public_keys(),
        };
        let user = UserId::new();
        let anchor_id = DeviceId::new();
        let target_id = DeviceId::new();
        let now = chrono::Utc::now().timestamp();
        let context = SharingContext {
            server_instance_id: Uuid::new_v4(),
            share_id: ShareId::new(),
            item_id: ObjectId::new(),
            revision: 1,
            access_epoch: 1,
            kind: SharedItemKind::Host,
        };
        let members = vec![
            SharingMember {
                user_id: pin.user_id,
                device_id: pin.device_id,
                encryption_public_key: owner.public_keys().encryption_bytes(),
                signing_public_key: owner.public_keys().signing_bytes(),
                role: SharingRole::Editor,
            },
            SharingMember {
                user_id: user,
                device_id: anchor_id,
                encryption_public_key: anchor.public_keys().encryption_bytes(),
                signing_public_key: anchor.public_keys().signing_bytes(),
                role: SharingRole::Reader,
            },
        ];
        let manifest = AccessManifest {
            format: 1,
            server_instance_id: context.server_instance_id,
            share_id: context.share_id,
            item_id: context.item_id,
            owner_user_id: pin.user_id,
            owner_device_id: pin.device_id,
            revision: 1,
            access_epoch: 1,
            previous_manifest_hash: Bytes::from([0; 32]),
            kind: context.kind,
            members,
        };
        let signed = sign_shared_manifest(&owner, manifest, &context, &pin, None).unwrap();
        let access = verify_shared_manifest(&signed, &context, &pin, None).unwrap();
        let scope = EnrollmentScope {
            server_instance_id: context.server_instance_id,
            share_id: context.share_id,
            item_id: context.item_id,
            kind: context.kind,
        };
        let grant = SharingOwnDevicesGrantState {
            format: 1,
            scope: scope.clone(),
            owner_user_id: pin.user_id,
            owner_device_id: pin.device_id,
            grant_id: Uuid::new_v4(),
            grant_revision: 1,
            previous_grant_state_hash: Bytes::from([0; 32]),
            status: EnrollmentGrantStatus::Active,
            anchor: EnrollmentDeviceBinding {
                user_id: user,
                device_id: anchor_id,
                encryption_public_key: anchor.public_keys().encryption_bytes(),
                signing_public_key: anchor.public_keys().signing_bytes(),
            },
            access_manifest_hash: Bytes::from(access.hash()),
            access_epoch: 1,
            role_ceiling: SharingRole::Reader,
            mode: EnrollmentMode::Manual,
            not_before: now,
            expires_at: now + 3600,
            max_admissions: 1,
            admitted_count: 0,
        };
        let signed = sign_grant_state(
            &owner,
            grant,
            &pin,
            None,
            GrantTransition::Genesis { access: &access },
            now,
        )
        .unwrap();
        let grant = verify_grant_state(
            &signed,
            &pin,
            None,
            GrantTransition::Genesis { access: &access },
            now,
        )
        .unwrap();
        let request = SharingOwnDeviceRequest {
            format: 1,
            scope,
            request_id: Uuid::new_v4(),
            grant_state_hash: Bytes::from(grant.hash()),
            access_manifest_hash: Bytes::from(access.hash()),
            access_epoch: 1,
            target: EnrollmentDeviceBinding {
                user_id: user,
                device_id: target_id,
                encryption_public_key: if low_order_target {
                    Bytes::from([0; 32])
                } else {
                    target.public_keys().encryption_bytes()
                },
                signing_public_key: target.public_keys().signing_bytes(),
            },
            requested_role: SharingRole::Reader,
            nonce: Bytes::from(random_array::<32>().unwrap()),
            not_before: now,
            expires_at: now + 600,
        };
        // An attacker owns Ed25519 but supplies an invalid X25519 key. The
        // raw crate-private signer is used only in this adversarial fixture.
        let signature =
            Bytes::from(target.sign_message(&enrollment_request_message(&request).unwrap()));
        let request = verify_device_request(
            &SignedSharingOwnDeviceRequest { request, signature },
            &grant,
            &access,
            now,
        )
        .unwrap();
        let signed = endorse_device_request(
            &anchor,
            &request,
            &access,
            &enrollment_pairing_code(&request).unwrap(),
            now,
        )
        .unwrap();
        let endorsement = verify_anchor_endorsement(&signed, &request, &access, now).unwrap();
        Setup {
            owner,
            target,
            pin,
            access,
            request,
            endorsement,
            now,
        }
    }
    #[test]
    fn a_valid_target_ed_signature_is_insufficient_for_x25519_possession() {
        let f = setup(false);
        let (challenge, secret) = create_device_challenge(
            &f.owner,
            &f.pin,
            &f.request,
            &f.endorsement,
            &f.access,
            f.now,
            None,
        )
        .unwrap();
        let verified = verify_device_challenge(
            &challenge,
            &f.pin,
            &f.request,
            &f.endorsement,
            &f.access,
            f.now,
            None,
        )
        .unwrap();
        let response = SharingDeviceChallengeResponse {
            format: 1,
            request_hash: Bytes::from(f.request.hash()),
            challenge_hash: Bytes::from(verified.hash()),
            response_digest: Bytes::from(random_array::<32>().unwrap()),
        };
        let signed = SignedSharingDeviceChallengeResponse {
            signature: Bytes::from(
                f.target
                    .sign_message(&enrollment_response_message(&response).unwrap()),
            ),
            response,
        };
        verify_device_response(&signed, &f.request, &verified, &f.access, f.now).unwrap();
        assert!(matches!(
            verify_device_possession(
                &signed,
                &secret,
                &f.request,
                &f.access,
                f.now,
                &verified.checkpoint()
            ),
            Err(EnrollmentCryptoError::Possession)
        ));
    }
    #[test]
    fn low_order_target_and_validly_signed_tampered_aead_are_rejected() {
        let f = setup(true);
        assert!(create_device_challenge(
            &f.owner,
            &f.pin,
            &f.request,
            &f.endorsement,
            &f.access,
            f.now,
            None
        )
        .is_err());
        let f = setup(false);
        let (challenge, _) = create_device_challenge(
            &f.owner,
            &f.pin,
            &f.request,
            &f.endorsement,
            &f.access,
            f.now,
            None,
        )
        .unwrap();
        for field in 0..3 {
            let mut changed = challenge.clone();
            match field {
                0 => changed.challenge.ciphertext = Bytes::from([0; 48]),
                1 => changed.challenge.nonce = Bytes::from([0; 24]),
                _ => changed.challenge.ephemeral_public_key = Bytes::from([0; 32]),
            }
            changed.signature = Bytes::from(
                f.owner
                    .sign_message(&enrollment_challenge_message(&changed.challenge).unwrap()),
            );
            verify_device_challenge(
                &changed,
                &f.pin,
                &f.request,
                &f.endorsement,
                &f.access,
                f.now,
                None,
            )
            .unwrap();
            assert!(answer_device_challenge(
                &f.target,
                &f.pin,
                &f.request,
                &f.endorsement,
                &f.access,
                &changed,
                f.now,
                None
            )
            .is_err());
        }
    }
}
