//! Experimental owner-online own-device enrollment (ADR-0009).
//!
//! These records carry public identities, signed decisions and an opaque
//! possession challenge. They never carry shared plaintext, a DEK, or private
//! key material. Structural validation is not signature verification, trusted
//! pairing, possession verification, freshness, grant-transition validation or
//! authorization. Crypto/client/server layers must resolve and verify every
//! referenced signed-record hash and compare their durable checkpoints.
//!
//! All timestamps are integral Unix seconds, encoded as big-endian signed
//! i64. Revisions/generations/epochs fit positive PostgreSQL i64. Expired
//! structurally valid records remain parseable for terminal revoke/recovery.
//! Canonical messages have fixed field order, raw UUID bytes, fixed-size byte
//! fields and distinct domains. Existing sharing-v1 messages are unchanged.

use crate::sharing::{
    self, RotateShareAccessRequest, SharedItemKind, SharedItemState, SharingRole,
};
use crate::{Bytes, DeviceId, ObjectId, ShareId, UserId};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use uuid::Uuid;

pub const FORMAT: u16 = 1;
pub const MAX_GRANT_LIFETIME_SECONDS: i64 = 30 * 24 * 60 * 60;
pub const MAX_REQUEST_LIFETIME_SECONDS: i64 = 15 * 60;
pub const MAX_CHALLENGE_LIFETIME_SECONDS: i64 = 5 * 60;
pub const MAX_ADMISSIONS: u32 = 16;
/// Currently active, unexpired grants bound to the current access head.
pub const MAX_GRANTS_PER_SHARE: usize = 16;
/// Stable IDs/terminal heads are retained to prevent replay or quota reset.
/// Exceeding this limit rejects new IDs; it never discards old terminal state.
pub const MAX_RETAINED_GRANTS_PER_SHARE: usize = 256;
pub const MAX_RETAINED_REQUESTS_PER_SHARE: usize = 1024;
pub const MAX_PENDING_REQUESTS_PER_GRANT: usize = 16;
pub const MAX_PENDING_REQUESTS_PER_SHARE: usize = 64;
pub const MAX_OTHER_GRANT_SUCCESSORS: usize = MAX_GRANTS_PER_SHARE - 1;
pub const MAX_PAGE_SIZE: usize = 100;

/// No response digest helper receives the decrypted challenge secret here:
/// crypto-core constructs that hash in a zeroizing allocation using this label.
pub mod labels {
    pub const GRANT: &[u8] = b"consolecrypt/sharing/enrollment/v1/grant\0";
    pub const REQUEST: &[u8] = b"consolecrypt/sharing/enrollment/v1/request\0";
    pub const ENDORSEMENT: &[u8] = b"consolecrypt/sharing/enrollment/v1/endorsement\0";
    pub const CHALLENGE: &[u8] = b"consolecrypt/sharing/enrollment/v1/challenge\0";
    pub const RESPONSE: &[u8] = b"consolecrypt/sharing/enrollment/v1/response\0";
    pub const ACCEPTANCE: &[u8] = b"consolecrypt/sharing/enrollment/v1/acceptance\0";
    pub const GRANT_HASH: &[u8] = b"consolecrypt/sharing/enrollment/v1/grant-hash\0";
    pub const REQUEST_HASH: &[u8] = b"consolecrypt/sharing/enrollment/v1/request-hash\0";
    pub const ENDORSEMENT_HASH: &[u8] = b"consolecrypt/sharing/enrollment/v1/endorsement-hash\0";
    pub const CHALLENGE_HASH: &[u8] = b"consolecrypt/sharing/enrollment/v1/challenge-hash\0";
    pub const RESPONSE_HASH: &[u8] = b"consolecrypt/sharing/enrollment/v1/response-hash\0";
    pub const ACCEPTANCE_HASH: &[u8] = b"consolecrypt/sharing/enrollment/v1/acceptance-hash\0";
    pub const CHALLENGE_HEADER: &[u8] = b"consolecrypt/sharing/enrollment/v1/challenge-header\0";
    pub const CHALLENGE_AAD: &[u8] = b"consolecrypt/sharing/enrollment/v1/challenge-aad\0";
    pub const CHALLENGE_KEY: &[u8] = b"consolecrypt/sharing/enrollment/v1/challenge-key\0";
    pub const RESPONSE_DIGEST: &[u8] = b"consolecrypt/sharing/enrollment/v1/response-digest\0";
    pub const PAIRING: &[u8] = b"consolecrypt/sharing/enrollment/v1/pairing\0";
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EnrollmentScope {
    pub server_instance_id: Uuid,
    pub share_id: ShareId,
    pub item_id: ObjectId,
    pub kind: SharedItemKind,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EnrollmentDeviceBinding {
    pub user_id: UserId,
    pub device_id: DeviceId,
    pub encryption_public_key: Bytes,
    pub signing_public_key: Bytes,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EnrollmentGrantStatus {
    Active,
    Revoked,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EnrollmentMode {
    Manual,
    Automatic,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SharingOwnDevicesGrantState {
    pub format: u16,
    pub scope: EnrollmentScope,
    pub owner_user_id: UserId,
    pub owner_device_id: DeviceId,
    pub grant_id: Uuid,
    pub grant_revision: u64,
    pub previous_grant_state_hash: Bytes,
    pub status: EnrollmentGrantStatus,
    pub anchor: EnrollmentDeviceBinding,
    /// Unchanged v1 unsigned-canonical manifest hash, signature checked separately.
    pub access_manifest_hash: Bytes,
    pub access_epoch: u64,
    pub role_ceiling: SharingRole,
    pub mode: EnrollmentMode,
    pub not_before: i64,
    pub expires_at: i64,
    pub max_admissions: u32,
    pub admitted_count: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SignedSharingOwnDevicesGrantState {
    pub grant: SharingOwnDevicesGrantState,
    pub signature: Bytes,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SharingOwnDeviceRequest {
    pub format: u16,
    pub scope: EnrollmentScope,
    pub request_id: Uuid,
    pub grant_state_hash: Bytes,
    pub access_manifest_hash: Bytes,
    pub access_epoch: u64,
    pub target: EnrollmentDeviceBinding,
    pub requested_role: SharingRole,
    pub nonce: Bytes,
    pub not_before: i64,
    pub expires_at: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SignedSharingOwnDeviceRequest {
    pub request: SharingOwnDeviceRequest,
    pub signature: Bytes,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SharingAnchorEndorsement {
    pub format: u16,
    pub request_hash: Bytes,
    pub anchor_device_id: DeviceId,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SignedSharingAnchorEndorsement {
    pub endorsement: SharingAnchorEndorsement,
    pub signature: Bytes,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SharingDeviceChallenge {
    pub format: u16,
    pub request_hash: Bytes,
    pub anchor_endorsement_hash: Bytes,
    pub challenge_id: Uuid,
    pub generation: u64,
    pub not_before: i64,
    pub expires_at: i64,
    pub ephemeral_public_key: Bytes,
    pub nonce: Bytes,
    /// AEAD of fresh 32-byte random material plus the 16-byte tag. Never a DEK.
    pub ciphertext: Bytes,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SignedSharingDeviceChallenge {
    pub challenge: SharingDeviceChallenge,
    pub signature: Bytes,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SharingDeviceChallengeResponse {
    pub format: u16,
    pub request_hash: Bytes,
    pub challenge_hash: Bytes,
    /// Domain-separated hash of the complete signed challenge hash and random.
    pub response_digest: Bytes,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SignedSharingDeviceChallengeResponse {
    pub response: SharingDeviceChallengeResponse,
    pub signature: Bytes,
}

/// The ID makes canonical sorting and correspondence to supplied successors
/// explicit. Hashes cover the complete signed state, not just unsigned fields.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EnrollmentGrantSuccessorHash {
    pub grant_id: Uuid,
    pub state_hash: Bytes,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SharingOwnDeviceAcceptance {
    pub format: u16,
    pub request_hash: Bytes,
    pub anchor_endorsement_hash: Bytes,
    pub challenge_hash: Bytes,
    pub response_hash: Bytes,
    pub consumed_grant_state_hash: Bytes,
    /// Unchanged v1 unsigned-canonical manifest/mutation hashes.
    pub result_access_manifest_hash: Bytes,
    pub result_revision_hash: Bytes,
    pub consumed_grant_successor_hash: Bytes,
    pub other_grant_successor_hashes: Vec<EnrollmentGrantSuccessorHash>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SignedSharingOwnDeviceAcceptance {
    pub acceptance: SharingOwnDeviceAcceptance,
    pub signature: Bytes,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PublishOwnDevicesGrantRequest {
    pub grant: SignedSharingOwnDevicesGrantState,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OwnDevicesGrantPage {
    pub items: Vec<SignedSharingOwnDevicesGrantState>,
    pub next_after: Option<Uuid>,
    pub has_more: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OwnDevicesGrantHistoryPage {
    pub states: Vec<SignedSharingOwnDevicesGrantState>,
    pub latest_revision: u64,
    pub has_more: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SubmitOwnDeviceRequest {
    /// Lookup hint only: callers verify the request's signed grant-state hash
    /// against this exact stored/verified grant before authorizing anything.
    pub grant_id: Uuid,
    pub request: SignedSharingOwnDeviceRequest,
    pub endorsement: SignedSharingAnchorEndorsement,
}

/// Server status is display metadata; it never proves possession/acceptance.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OwnDeviceRequestStatus {
    Pending,
    Challenged,
    Responded,
    Accepted,
    Denied,
    Expired,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OwnDeviceRequestState {
    pub grant_id: Uuid,
    pub request: SignedSharingOwnDeviceRequest,
    pub endorsement: SignedSharingAnchorEndorsement,
    pub status: OwnDeviceRequestStatus,
    pub challenge: Option<SignedSharingDeviceChallenge>,
    pub response: Option<SignedSharingDeviceChallengeResponse>,
    pub acceptance: Option<SignedSharingOwnDeviceAcceptance>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OwnDeviceRequestPage {
    pub items: Vec<OwnDeviceRequestState>,
    pub next_after: Option<Uuid>,
    pub has_more: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PublishOwnDeviceChallengeRequest {
    pub challenge: SignedSharingDeviceChallenge,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SubmitOwnDeviceChallengeResponseRequest {
    pub response: SignedSharingDeviceChallengeResponse,
}

/// Atomic ordinary-v1 rotation, immutable receipt and mandatory consumed
/// grant successor. Other successor hashes/states must correspond one-to-one.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AcceptOwnDeviceRequest {
    pub rotation: RotateShareAccessRequest,
    pub acceptance: SignedSharingOwnDeviceAcceptance,
    pub consumed_grant_successor: SignedSharingOwnDevicesGrantState,
    pub other_grant_successors: Vec<SignedSharingOwnDevicesGrantState>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OwnDeviceAcceptanceResult {
    pub state: SharedItemState,
    pub acceptance: SignedSharingOwnDeviceAcceptance,
    pub consumed_grant_successor: SignedSharingOwnDevicesGrantState,
    pub other_grant_successors: Vec<SignedSharingOwnDevicesGrantState>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("invalid sharing enrollment payload: {0}")]
pub struct EnrollmentValidationError(pub &'static str);
type Result<T> = std::result::Result<T, EnrollmentValidationError>;

fn require(condition: bool, field: &'static str) -> Result<()> {
    if condition {
        Ok(())
    } else {
        Err(EnrollmentValidationError(field))
    }
}
fn format(format: u16) -> Result<()> {
    require(format == FORMAT, "format")
}
fn positive(value: u64, field: &'static str) -> Result<()> {
    require(value > 0 && value <= i64::MAX as u64, field)
}
fn hash(value: &Bytes, field: &'static str) -> Result<()> {
    require(value.len() == 32 && value.as_slice() != [0; 32], field)
}
fn signature(value: &Bytes) -> Result<()> {
    require(value.len() == 64, "signature")
}
fn lifetime(start: i64, end: i64, max: i64) -> Result<()> {
    require(
        start > 0 && end.checked_sub(start).is_some_and(|d| d > 0 && d <= max),
        "lifetime",
    )
}
fn nested_lifetime(start: i64, end: i64, parent_start: i64, parent_end: i64) -> Result<()> {
    require(
        start >= parent_start && end <= parent_end,
        "nested_lifetime",
    )
}
fn role_tag(role: SharingRole) -> u8 {
    match role {
        SharingRole::Reader => 1,
        SharingRole::Editor => 2,
    }
}
fn kind_tag(kind: SharedItemKind) -> u8 {
    match kind {
        SharedItemKind::Host => 1,
        SharedItemKind::Group => 2,
        SharedItemKind::Snippet => 3,
        SharedItemKind::Secret => 4,
    }
}

pub fn validate_scope(scope: &EnrollmentScope) -> Result<()> {
    require(
        !scope.server_instance_id.is_nil()
            && scope.share_id != ShareId::NIL
            && scope.item_id != ObjectId::NIL,
        "scope",
    )
}
pub fn validate_device_binding(device: &EnrollmentDeviceBinding) -> Result<()> {
    require(
        device.user_id != UserId::NIL && device.device_id != DeviceId::NIL,
        "device_id",
    )?;
    require(
        device.encryption_public_key.len() == 32 && device.signing_public_key.len() == 32,
        "public_key",
    )
}
pub fn validate_grant(grant: &SharingOwnDevicesGrantState) -> Result<()> {
    format(grant.format)?;
    validate_scope(&grant.scope)?;
    require(
        grant.owner_user_id != UserId::NIL
            && grant.owner_device_id != DeviceId::NIL
            && !grant.grant_id.is_nil(),
        "grant_id",
    )?;
    positive(grant.grant_revision, "grant_revision")?;
    positive(grant.access_epoch, "access_epoch")?;
    require(
        grant.previous_grant_state_hash.len() == 32,
        "previous_grant_state_hash",
    )?;
    if grant.grant_revision == 1 {
        require(
            grant.previous_grant_state_hash.as_slice() == [0; 32]
                && grant.admitted_count == 0
                && grant.status == EnrollmentGrantStatus::Active,
            "grant_genesis",
        )?;
    } else {
        hash(
            &grant.previous_grant_state_hash,
            "previous_grant_state_hash",
        )?;
    }
    validate_device_binding(&grant.anchor)?;
    hash(&grant.access_manifest_hash, "access_manifest_hash")?;
    lifetime(
        grant.not_before,
        grant.expires_at,
        MAX_GRANT_LIFETIME_SECONDS,
    )?;
    require(
        (1..=MAX_ADMISSIONS).contains(&grant.max_admissions)
            && grant.admitted_count <= grant.max_admissions,
        "admissions",
    )?;
    require(
        grant.status != EnrollmentGrantStatus::Active
            || grant.admitted_count < grant.max_admissions,
        "exhausted_active_grant",
    )
}
pub fn validate_signed_grant(grant: &SignedSharingOwnDevicesGrantState) -> Result<()> {
    validate_grant(&grant.grant)?;
    signature(&grant.signature)
}
pub fn validate_request(request: &SharingOwnDeviceRequest) -> Result<()> {
    format(request.format)?;
    validate_scope(&request.scope)?;
    require(!request.request_id.is_nil(), "request_id")?;
    hash(&request.grant_state_hash, "grant_state_hash")?;
    hash(&request.access_manifest_hash, "access_manifest_hash")?;
    positive(request.access_epoch, "access_epoch")?;
    validate_device_binding(&request.target)?;
    require(request.nonce.len() == 32, "request_nonce")?;
    lifetime(
        request.not_before,
        request.expires_at,
        MAX_REQUEST_LIFETIME_SECONDS,
    )
}
pub fn validate_signed_request(request: &SignedSharingOwnDeviceRequest) -> Result<()> {
    validate_request(&request.request)?;
    signature(&request.signature)
}
/// Cross-record structure only. Callers additionally verify the grant hash,
/// signatures, current anchor keys/role, active status and current time.
pub fn validate_request_for_grant(
    request: &SharingOwnDeviceRequest,
    grant: &SharingOwnDevicesGrantState,
) -> Result<()> {
    validate_request(request)?;
    validate_grant(grant)?;
    require(
        request.scope == grant.scope
            && request.access_epoch == grant.access_epoch
            && request.access_manifest_hash == grant.access_manifest_hash,
        "grant_request_context",
    )?;
    require(
        request.target.user_id == grant.anchor.user_id
            && request.target.device_id != grant.anchor.device_id
            && role_tag(request.requested_role) <= role_tag(grant.role_ceiling),
        "target_role_account",
    )?;
    nested_lifetime(
        request.not_before,
        request.expires_at,
        grant.not_before,
        grant.expires_at,
    )
}
pub fn validate_endorsement(endorsement: &SharingAnchorEndorsement) -> Result<()> {
    format(endorsement.format)?;
    hash(&endorsement.request_hash, "request_hash")?;
    require(
        endorsement.anchor_device_id != DeviceId::NIL,
        "anchor_device_id",
    )
}
pub fn validate_signed_endorsement(endorsement: &SignedSharingAnchorEndorsement) -> Result<()> {
    validate_endorsement(&endorsement.endorsement)?;
    signature(&endorsement.signature)
}
pub fn validate_challenge_header(challenge: &SharingDeviceChallenge) -> Result<()> {
    format(challenge.format)?;
    hash(&challenge.request_hash, "request_hash")?;
    hash(
        &challenge.anchor_endorsement_hash,
        "anchor_endorsement_hash",
    )?;
    require(!challenge.challenge_id.is_nil(), "challenge_id")?;
    positive(challenge.generation, "generation")?;
    lifetime(
        challenge.not_before,
        challenge.expires_at,
        MAX_CHALLENGE_LIFETIME_SECONDS,
    )?;
    require(
        challenge.ephemeral_public_key.len() == 32 && challenge.nonce.len() == 24,
        "challenge_key_nonce",
    )
}
pub fn validate_challenge(challenge: &SharingDeviceChallenge) -> Result<()> {
    validate_challenge_header(challenge)?;
    require(challenge.ciphertext.len() == 48, "challenge_ciphertext")
}
pub fn validate_signed_challenge(challenge: &SignedSharingDeviceChallenge) -> Result<()> {
    validate_challenge(&challenge.challenge)?;
    signature(&challenge.signature)
}
pub fn validate_challenge_for_request(
    challenge: &SharingDeviceChallenge,
    request: &SharingOwnDeviceRequest,
) -> Result<()> {
    validate_challenge(challenge)?;
    validate_request(request)?;
    nested_lifetime(
        challenge.not_before,
        challenge.expires_at,
        request.not_before,
        request.expires_at,
    )
}
pub fn validate_response(response: &SharingDeviceChallengeResponse) -> Result<()> {
    format(response.format)?;
    hash(&response.request_hash, "request_hash")?;
    hash(&response.challenge_hash, "challenge_hash")?;
    hash(&response.response_digest, "response_digest")
}
pub fn validate_signed_response(response: &SignedSharingDeviceChallengeResponse) -> Result<()> {
    validate_response(&response.response)?;
    signature(&response.signature)
}
pub fn validate_acceptance(acceptance: &SharingOwnDeviceAcceptance) -> Result<()> {
    format(acceptance.format)?;
    for (value, field) in [
        (&acceptance.request_hash, "request_hash"),
        (
            &acceptance.anchor_endorsement_hash,
            "anchor_endorsement_hash",
        ),
        (&acceptance.challenge_hash, "challenge_hash"),
        (&acceptance.response_hash, "response_hash"),
        (
            &acceptance.consumed_grant_state_hash,
            "consumed_grant_state_hash",
        ),
        (
            &acceptance.result_access_manifest_hash,
            "result_access_manifest_hash",
        ),
        (&acceptance.result_revision_hash, "result_revision_hash"),
        (
            &acceptance.consumed_grant_successor_hash,
            "consumed_grant_successor_hash",
        ),
    ] {
        hash(value, field)?;
    }
    require(
        acceptance.other_grant_successor_hashes.len() <= MAX_OTHER_GRANT_SUCCESSORS,
        "grant_successors",
    )?;
    let mut ids = BTreeSet::new();
    let mut hashes = BTreeSet::new();
    for successor in &acceptance.other_grant_successor_hashes {
        require(
            !successor.grant_id.is_nil() && ids.insert(successor.grant_id),
            "duplicate_grant_successor",
        )?;
        hash(&successor.state_hash, "grant_successor_hash")?;
        require(
            successor.state_hash != acceptance.consumed_grant_successor_hash
                && hashes.insert(successor.state_hash.as_slice()),
            "duplicate_grant_successor_hash",
        )?;
    }
    Ok(())
}
pub fn validate_signed_acceptance(acceptance: &SignedSharingOwnDeviceAcceptance) -> Result<()> {
    validate_acceptance(&acceptance.acceptance)?;
    signature(&acceptance.signature)
}

pub fn validate_submission(submission: &SubmitOwnDeviceRequest) -> Result<()> {
    require(!submission.grant_id.is_nil(), "grant_id")?;
    validate_signed_request(&submission.request)?;
    validate_signed_endorsement(&submission.endorsement)
}
pub fn validate_request_state(state: &OwnDeviceRequestState) -> Result<()> {
    validate_submission(&SubmitOwnDeviceRequest {
        grant_id: state.grant_id,
        request: state.request.clone(),
        endorsement: state.endorsement.clone(),
    })?;
    if let Some(challenge) = &state.challenge {
        validate_signed_challenge(challenge)?;
        validate_challenge_for_request(&challenge.challenge, &state.request.request)?;
        require(
            challenge.challenge.request_hash == state.endorsement.endorsement.request_hash,
            "challenge_request_hash",
        )?;
    }
    if let Some(response) = &state.response {
        validate_signed_response(response)?;
        require(
            state.challenge.is_some()
                && response.response.request_hash == state.endorsement.endorsement.request_hash,
            "response_request_hash",
        )?;
    }
    if let Some(acceptance) = &state.acceptance {
        validate_signed_acceptance(acceptance)?;
        require(
            state.response.is_some()
                && acceptance.acceptance.request_hash == state.endorsement.endorsement.request_hash,
            "acceptance_request_hash",
        )?;
        let response = state
            .response
            .as_ref()
            .ok_or(EnrollmentValidationError("acceptance_response"))?;
        let challenge = state
            .challenge
            .as_ref()
            .ok_or(EnrollmentValidationError("acceptance_challenge"))?;
        require(
            acceptance.acceptance.challenge_hash == response.response.challenge_hash
                && acceptance.acceptance.anchor_endorsement_hash
                    == challenge.challenge.anchor_endorsement_hash
                && acceptance.acceptance.consumed_grant_state_hash
                    == state.request.request.grant_state_hash,
            "acceptance_transcript_hash",
        )?;
    }
    require(
        match state.status {
            OwnDeviceRequestStatus::Pending => {
                state.challenge.is_none() && state.response.is_none() && state.acceptance.is_none()
            }
            OwnDeviceRequestStatus::Challenged => {
                state.challenge.is_some() && state.response.is_none() && state.acceptance.is_none()
            }
            OwnDeviceRequestStatus::Responded => {
                state.challenge.is_some() && state.response.is_some() && state.acceptance.is_none()
            }
            OwnDeviceRequestStatus::Accepted => {
                state.challenge.is_some() && state.response.is_some() && state.acceptance.is_some()
            }
            OwnDeviceRequestStatus::Denied | OwnDeviceRequestStatus::Expired => {
                state.acceptance.is_none()
            }
        },
        "request_status",
    )
}

fn page_cursor(length: usize, next: Option<Uuid>, has_more: bool) -> Result<()> {
    require(
        length <= MAX_PAGE_SIZE
            && (has_more == next.is_some())
            && next.is_none_or(|id| !id.is_nil())
            && (!has_more || length > 0),
        "page_cursor",
    )
}
pub fn validate_grant_page(page: &OwnDevicesGrantPage, scope: &EnrollmentScope) -> Result<()> {
    validate_scope(scope)?;
    page_cursor(page.items.len(), page.next_after, page.has_more)?;
    let mut previous = None;
    for signed in &page.items {
        validate_signed_grant(signed)?;
        require(
            signed.grant.scope == *scope && previous.is_none_or(|id| id < signed.grant.grant_id),
            "grant_page_order_context",
        )?;
        previous = Some(signed.grant.grant_id);
    }
    require(!page.has_more || previous == page.next_after, "page_cursor")
}
pub fn validate_grant_history_page(
    page: &OwnDevicesGrantHistoryPage,
    scope: &EnrollmentScope,
    grant_id: Uuid,
    after_revision: u64,
) -> Result<()> {
    validate_scope(scope)?;
    require(
        !grant_id.is_nil() && after_revision <= i64::MAX as u64,
        "history_cursor",
    )?;
    positive(page.latest_revision, "latest_revision")?;
    require(
        page.states.len() <= MAX_PAGE_SIZE && page.latest_revision >= after_revision,
        "grant_history",
    )?;
    let mut revision = after_revision;
    for signed in &page.states {
        validate_signed_grant(signed)?;
        require(
            signed.grant.scope == *scope
                && signed.grant.grant_id == grant_id
                && revision.checked_add(1) == Some(signed.grant.grant_revision),
            "grant_history_order_context",
        )?;
        revision = signed.grant.grant_revision;
    }
    require(
        revision <= page.latest_revision
            && page.has_more == (revision < page.latest_revision)
            && (!page.has_more || !page.states.is_empty()),
        "grant_history_cursor",
    )
}
pub fn validate_request_page(page: &OwnDeviceRequestPage, scope: &EnrollmentScope) -> Result<()> {
    validate_scope(scope)?;
    page_cursor(page.items.len(), page.next_after, page.has_more)?;
    let mut previous = None;
    for state in &page.items {
        validate_request_state(state)?;
        require(
            state.request.request.scope == *scope
                && previous.is_none_or(|id| id < state.request.request.request_id),
            "request_page_order_context",
        )?;
        previous = Some(state.request.request.request_id);
    }
    require(!page.has_more || previous == page.next_after, "page_cursor")
}
pub fn validate_accept_request(request: &AcceptOwnDeviceRequest) -> Result<()> {
    sharing::validate_state(&SharedItemState {
        access: request.rotation.access.clone(),
        revision: request.rotation.revision.clone(),
    })
    .map_err(|_| EnrollmentValidationError("rotation"))?;
    validate_signed_acceptance(&request.acceptance)?;
    validate_signed_grant(&request.consumed_grant_successor)?;
    let consumed = &request.consumed_grant_successor.grant;
    let access = &request.rotation.access.manifest;
    require(
        consumed.grant_revision > 1
            && consumed.admitted_count > 0
            && consumed.scope.server_instance_id == access.server_instance_id
            && consumed.scope.share_id == access.share_id
            && consumed.scope.item_id == access.item_id
            && consumed.scope.kind == access.kind
            && consumed.access_epoch == access.access_epoch
            && consumed.owner_user_id == access.owner_user_id
            && consumed.owner_device_id == access.owner_device_id,
        "acceptance_rotation_context",
    )?;
    require(
        consumed.access_manifest_hash == request.acceptance.acceptance.result_access_manifest_hash,
        "acceptance_result_manifest_hash",
    )?;
    require(
        request.other_grant_successors.len()
            == request
                .acceptance
                .acceptance
                .other_grant_successor_hashes
                .len(),
        "acceptance_successors",
    )?;
    let mut ids = BTreeSet::new();
    for signed in &request.other_grant_successors {
        validate_signed_grant(signed)?;
        let other = &signed.grant;
        require(
            other.grant_revision > 1
                && other.grant_id != consumed.grant_id
                && ids.insert(other.grant_id)
                && other.scope == consumed.scope
                && other.owner_user_id == consumed.owner_user_id
                && other.owner_device_id == consumed.owner_device_id
                && other.access_epoch == consumed.access_epoch
                && other.access_manifest_hash == consumed.access_manifest_hash,
            "acceptance_successor_context",
        )?;
    }
    let receipt_ids: BTreeSet<_> = request
        .acceptance
        .acceptance
        .other_grant_successor_hashes
        .iter()
        .map(|h| h.grant_id)
        .collect();
    require(ids == receipt_ids, "acceptance_successor_ids")
}
pub fn validate_acceptance_result(result: &OwnDeviceAcceptanceResult) -> Result<()> {
    validate_accept_request(&AcceptOwnDeviceRequest {
        rotation: RotateShareAccessRequest {
            access: result.state.access.clone(),
            revision: result.state.revision.clone(),
        },
        acceptance: result.acceptance.clone(),
        consumed_grant_successor: result.consumed_grant_successor.clone(),
        other_grant_successors: result.other_grant_successors.clone(),
    })
}

fn scope_bytes(out: &mut Vec<u8>, scope: &EnrollmentScope) {
    out.extend_from_slice(scope.server_instance_id.as_bytes());
    out.extend_from_slice(scope.share_id.as_bytes());
    out.extend_from_slice(scope.item_id.as_bytes());
    out.push(kind_tag(scope.kind));
}
fn device_bytes(out: &mut Vec<u8>, device: &EnrollmentDeviceBinding) {
    out.extend_from_slice(device.user_id.as_bytes());
    out.extend_from_slice(device.device_id.as_bytes());
    out.extend_from_slice(device.encryption_public_key.as_slice());
    out.extend_from_slice(device.signing_public_key.as_slice());
}
pub fn enrollment_grant_message(grant: &SharingOwnDevicesGrantState) -> Result<Vec<u8>> {
    validate_grant(grant)?;
    let mut out = labels::GRANT.to_vec();
    out.extend_from_slice(&grant.format.to_be_bytes());
    scope_bytes(&mut out, &grant.scope);
    out.extend_from_slice(grant.owner_user_id.as_bytes());
    out.extend_from_slice(grant.owner_device_id.as_bytes());
    out.extend_from_slice(grant.grant_id.as_bytes());
    out.extend_from_slice(&grant.grant_revision.to_be_bytes());
    out.extend_from_slice(grant.previous_grant_state_hash.as_slice());
    out.push(match grant.status {
        EnrollmentGrantStatus::Active => 1,
        EnrollmentGrantStatus::Revoked => 2,
    });
    device_bytes(&mut out, &grant.anchor);
    out.extend_from_slice(grant.access_manifest_hash.as_slice());
    out.extend_from_slice(&grant.access_epoch.to_be_bytes());
    out.push(role_tag(grant.role_ceiling));
    out.push(match grant.mode {
        EnrollmentMode::Manual => 1,
        EnrollmentMode::Automatic => 2,
    });
    out.extend_from_slice(&grant.not_before.to_be_bytes());
    out.extend_from_slice(&grant.expires_at.to_be_bytes());
    out.extend_from_slice(&grant.max_admissions.to_be_bytes());
    out.extend_from_slice(&grant.admitted_count.to_be_bytes());
    Ok(out)
}
pub fn enrollment_request_message(request: &SharingOwnDeviceRequest) -> Result<Vec<u8>> {
    validate_request(request)?;
    let mut out = labels::REQUEST.to_vec();
    out.extend_from_slice(&request.format.to_be_bytes());
    scope_bytes(&mut out, &request.scope);
    out.extend_from_slice(request.request_id.as_bytes());
    out.extend_from_slice(request.grant_state_hash.as_slice());
    out.extend_from_slice(request.access_manifest_hash.as_slice());
    out.extend_from_slice(&request.access_epoch.to_be_bytes());
    device_bytes(&mut out, &request.target);
    out.push(role_tag(request.requested_role));
    out.extend_from_slice(request.nonce.as_slice());
    out.extend_from_slice(&request.not_before.to_be_bytes());
    out.extend_from_slice(&request.expires_at.to_be_bytes());
    Ok(out)
}
pub fn enrollment_endorsement_message(endorsement: &SharingAnchorEndorsement) -> Result<Vec<u8>> {
    validate_endorsement(endorsement)?;
    let mut out = labels::ENDORSEMENT.to_vec();
    out.extend_from_slice(&endorsement.format.to_be_bytes());
    out.extend_from_slice(endorsement.request_hash.as_slice());
    out.extend_from_slice(endorsement.anchor_device_id.as_bytes());
    Ok(out)
}
fn challenge_header_bytes(out: &mut Vec<u8>, challenge: &SharingDeviceChallenge) {
    out.extend_from_slice(&challenge.format.to_be_bytes());
    out.extend_from_slice(challenge.request_hash.as_slice());
    out.extend_from_slice(challenge.anchor_endorsement_hash.as_slice());
    out.extend_from_slice(challenge.challenge_id.as_bytes());
    out.extend_from_slice(&challenge.generation.to_be_bytes());
    out.extend_from_slice(&challenge.not_before.to_be_bytes());
    out.extend_from_slice(&challenge.expires_at.to_be_bytes());
    out.extend_from_slice(challenge.ephemeral_public_key.as_slice());
    out.extend_from_slice(challenge.nonce.as_slice());
}
/// Allows an empty draft ciphertext: ciphertext/signature are excluded to
/// avoid an AEAD/signature cycle. Complete signing still requires 48 bytes.
pub fn enrollment_challenge_header(challenge: &SharingDeviceChallenge) -> Result<Vec<u8>> {
    validate_challenge_header(challenge)?;
    let mut out = labels::CHALLENGE_HEADER.to_vec();
    challenge_header_bytes(&mut out, challenge);
    Ok(out)
}
pub fn enrollment_challenge_aad(challenge: &SharingDeviceChallenge) -> Result<Vec<u8>> {
    let mut out = labels::CHALLENGE_AAD.to_vec();
    out.extend_from_slice(&enrollment_challenge_header(challenge)?);
    Ok(out)
}
pub fn enrollment_challenge_key_info(
    challenge: &SharingDeviceChallenge,
    target: &EnrollmentDeviceBinding,
) -> Result<Vec<u8>> {
    validate_device_binding(target)?;
    let mut out = labels::CHALLENGE_KEY.to_vec();
    out.extend_from_slice(&enrollment_challenge_header(challenge)?);
    device_bytes(&mut out, target);
    Ok(out)
}
pub fn enrollment_challenge_message(challenge: &SharingDeviceChallenge) -> Result<Vec<u8>> {
    validate_challenge(challenge)?;
    let mut out = labels::CHALLENGE.to_vec();
    challenge_header_bytes(&mut out, challenge);
    out.extend_from_slice(challenge.ciphertext.as_slice());
    Ok(out)
}
pub fn enrollment_response_message(response: &SharingDeviceChallengeResponse) -> Result<Vec<u8>> {
    validate_response(response)?;
    let mut out = labels::RESPONSE.to_vec();
    out.extend_from_slice(&response.format.to_be_bytes());
    out.extend_from_slice(response.request_hash.as_slice());
    out.extend_from_slice(response.challenge_hash.as_slice());
    out.extend_from_slice(response.response_digest.as_slice());
    Ok(out)
}
pub fn enrollment_acceptance_message(acceptance: &SharingOwnDeviceAcceptance) -> Result<Vec<u8>> {
    validate_acceptance(acceptance)?;
    let mut out = labels::ACCEPTANCE.to_vec();
    out.extend_from_slice(&acceptance.format.to_be_bytes());
    for value in [
        &acceptance.request_hash,
        &acceptance.anchor_endorsement_hash,
        &acceptance.challenge_hash,
        &acceptance.response_hash,
        &acceptance.consumed_grant_state_hash,
        &acceptance.result_access_manifest_hash,
        &acceptance.result_revision_hash,
        &acceptance.consumed_grant_successor_hash,
    ] {
        out.extend_from_slice(value.as_slice());
    }
    out.extend_from_slice(&(acceptance.other_grant_successor_hashes.len() as u32).to_be_bytes());
    let mut others: Vec<_> = acceptance.other_grant_successor_hashes.iter().collect();
    others.sort_by_key(|h| h.grant_id);
    for other in others {
        out.extend_from_slice(other.grant_id.as_bytes());
        out.extend_from_slice(other.state_hash.as_slice());
    }
    Ok(out)
}

fn signed_hash_input(domain: &[u8], message: Vec<u8>, signature: &Bytes) -> Result<Vec<u8>> {
    self::signature(signature)?;
    let mut out = domain.to_vec();
    out.extend_from_slice(&message);
    out.extend_from_slice(signature.as_slice());
    Ok(out)
}
pub fn enrollment_grant_hash_input(signed: &SignedSharingOwnDevicesGrantState) -> Result<Vec<u8>> {
    signed_hash_input(
        labels::GRANT_HASH,
        enrollment_grant_message(&signed.grant)?,
        &signed.signature,
    )
}
pub fn enrollment_request_hash_input(signed: &SignedSharingOwnDeviceRequest) -> Result<Vec<u8>> {
    signed_hash_input(
        labels::REQUEST_HASH,
        enrollment_request_message(&signed.request)?,
        &signed.signature,
    )
}
pub fn enrollment_endorsement_hash_input(
    signed: &SignedSharingAnchorEndorsement,
) -> Result<Vec<u8>> {
    signed_hash_input(
        labels::ENDORSEMENT_HASH,
        enrollment_endorsement_message(&signed.endorsement)?,
        &signed.signature,
    )
}
pub fn enrollment_challenge_hash_input(signed: &SignedSharingDeviceChallenge) -> Result<Vec<u8>> {
    signed_hash_input(
        labels::CHALLENGE_HASH,
        enrollment_challenge_message(&signed.challenge)?,
        &signed.signature,
    )
}
pub fn enrollment_response_hash_input(
    signed: &SignedSharingDeviceChallengeResponse,
) -> Result<Vec<u8>> {
    signed_hash_input(
        labels::RESPONSE_HASH,
        enrollment_response_message(&signed.response)?,
        &signed.signature,
    )
}
pub fn enrollment_acceptance_hash_input(
    signed: &SignedSharingOwnDeviceAcceptance,
) -> Result<Vec<u8>> {
    signed_hash_input(
        labels::ACCEPTANCE_HASH,
        enrollment_acceptance_message(&signed.acceptance)?,
        &signed.signature,
    )
}
/// Hash this input in crypto-core to display/compare a pairing code. The
/// argument is the hash of the complete validated target-signed request.
pub fn enrollment_pairing_message(signed_request_hash: &[u8; 32]) -> Result<Vec<u8>> {
    require(*signed_request_hash != [0; 32], "request_hash")?;
    let mut out = labels::PAIRING.to_vec();
    out.extend_from_slice(signed_request_hash);
    Ok(out)
}
