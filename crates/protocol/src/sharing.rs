//! Experimental selective-object sharing v1 (ADR-0008).
//!
//! No plaintext or private keys cross this contract. A capability advertised
//! by an enabled server is required; protocol version alone grants nothing.
//! Canonical encodings validate structure before signing or verifying.

use crate::{Bytes, DeviceId, MutationId, ObjectId, ShareId, UserId};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use uuid::Uuid;

pub const CAPABILITY: &str = "object_sharing_v1";
pub const FORMAT: u16 = 1;
pub const MAX_MEMBERS: usize = 64;
pub const MAX_CIPHERTEXT_BYTES: usize = 1024 * 1024 + 16;
pub const MAX_PAGE_SIZE: u32 = 100;
pub const API_PATH: &str = "/v1/shares";

pub mod labels {
    pub const MANIFEST: &[u8] = b"consolecrypt/sharing/v1/access-manifest\0";
    pub const MUTATION: &[u8] = b"consolecrypt/sharing/v1/mutation\0";
    pub const BODY: &[u8] = b"consolecrypt/sharing/v1/body\0";
    pub const OBJECT_AAD: &[u8] = b"consolecrypt/sharing/v1/object-aad\0";
    pub const ENVELOPE_AAD: &[u8] = b"consolecrypt/sharing/v1/envelope-aad\0";
    pub const ENVELOPE_KEY: &[u8] = b"consolecrypt/sharing/v1/envelope-key\0";
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SharedItemKind {
    Host,
    Group,
    Snippet,
    /// Requires independently advertised explicit-secret support.
    Secret,
}

impl SharedItemKind {
    const fn tag(self) -> u8 {
        match self {
            Self::Host => 1,
            Self::Group => 2,
            Self::Snippet => 3,
            Self::Secret => 4,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SharingRole {
    Reader,
    Editor,
}

impl SharingRole {
    pub const fn can_write(self) -> bool {
        matches!(self, Self::Editor)
    }

    const fn tag(self) -> u8 {
        match self {
            Self::Reader => 1,
            Self::Editor => 2,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SharingContext {
    pub server_instance_id: Uuid,
    pub share_id: ShareId,
    pub item_id: ObjectId,
    pub revision: i64,
    pub access_epoch: u64,
    pub kind: SharedItemKind,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SharingMember {
    pub user_id: UserId,
    pub device_id: DeviceId,
    pub encryption_public_key: Bytes,
    pub signing_public_key: Bytes,
    pub role: SharingRole,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AccessManifest {
    pub format: u16,
    pub server_instance_id: Uuid,
    pub share_id: ShareId,
    pub item_id: ObjectId,
    pub owner_user_id: UserId,
    pub owner_device_id: DeviceId,
    pub revision: u64,
    pub access_epoch: u64,
    /// SHA-256 of the canonical previous manifest; all zero for genesis.
    pub previous_manifest_hash: Bytes,
    pub kind: SharedItemKind,
    pub members: Vec<SharingMember>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SignedAccessManifest {
    pub manifest: AccessManifest,
    pub signature: Bytes,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SharedKeyEnvelope {
    pub recipient_device_id: DeviceId,
    pub ephemeral_public_key: Bytes,
    pub nonce: Bytes,
    /// AEAD ciphertext of a 32-byte DEK, plus its 16-byte authentication tag.
    pub ciphertext: Bytes,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SharedEncryptedBody {
    pub format: u16,
    pub ciphertext: Bytes,
    pub nonce: Bytes,
    pub envelopes: Vec<SharedKeyEnvelope>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SharingOperation {
    Put,
    Delete,
}

impl SharingOperation {
    const fn tag(self) -> u8 {
        match self {
            Self::Put => 1,
            Self::Delete => 2,
        }
    }
}

/// Persistently signed header. History can bridge checkpoints using this
/// header without exposing previous ciphertext or per-device key envelopes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SharingMutation {
    pub context: SharingContext,
    pub mutation_id: MutationId,
    pub base_revision: i64,
    pub manifest_revision: u64,
    pub manifest_hash: Bytes,
    pub writer_device_id: DeviceId,
    pub previous_revision_hash: Bytes,
    pub operation: SharingOperation,
    /// SHA-256 of sharing_body_message; all zero only for a tombstone.
    pub body_hash: Bytes,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SignedSharingMutation {
    pub mutation: SharingMutation,
    pub signature: Bytes,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SharedRevision {
    pub signed: SignedSharingMutation,
    pub body: Option<SharedEncryptedBody>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CreateShareRequest {
    pub access: SignedAccessManifest,
    pub revision: SharedRevision,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PutSharedRevisionRequest {
    pub revision: SharedRevision,
}

/// Membership changes require a freshly encrypted next revision in the
/// same transaction. Both expected revisions are in the signed payloads.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RotateShareAccessRequest {
    pub access: SignedAccessManifest,
    pub revision: SharedRevision,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SharedItemState {
    pub access: SignedAccessManifest,
    pub revision: SharedRevision,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SharingCapabilities {
    pub enabled: bool,
    /// A stable persisted UUID of this server instance, not an origin string.
    pub server_instance_id: Uuid,
    pub format: u16,
    pub max_members: u32,
    pub max_ciphertext_bytes: u32,
    #[serde(default)]
    pub supports_groups: bool,
    #[serde(default)]
    pub supports_secrets: bool,
    #[serde(default)]
    pub supports_owner_online_enrollment_v1: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ShareListPage {
    /// Latest states visible to the requesting device, ordered by share ID.
    pub items: Vec<SharedItemState>,
    pub next_after: Option<ShareId>,
    pub has_more: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ShareHistoryPage {
    /// Strictly ascending and gap-free after the requested manifest revision.
    pub manifests: Vec<SignedAccessManifest>,
    /// Strictly ascending and gap-free after the requested object revision.
    pub revisions: Vec<SignedSharingMutation>,
    pub latest_manifest_revision: u64,
    pub latest_revision: i64,
    pub has_more: bool,
}

/// Directory lookup is discovery, not cryptographic trust. The owner must
/// compare verification codes before adding any returned device.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SharingRecipient {
    pub user_id: UserId,
    pub devices: Vec<SharingMember>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("invalid sharing payload: {0}")]
pub struct SharingValidationError(pub &'static str);

fn require(condition: bool, field: &'static str) -> Result<(), SharingValidationError> {
    if condition {
        Ok(())
    } else {
        Err(SharingValidationError(field))
    }
}

pub fn validate_context(c: &SharingContext) -> Result<(), SharingValidationError> {
    require(!c.server_instance_id.is_nil(), "server_instance_id")?;
    require(c.share_id != ShareId::NIL, "share_id")?;
    require(c.item_id != ObjectId::NIL, "item_id")?;
    require(c.revision > 0, "revision")?;
    require(
        c.access_epoch > 0 && c.access_epoch <= i64::MAX as u64,
        "access_epoch",
    )
}

pub fn validate_manifest(m: &AccessManifest) -> Result<(), SharingValidationError> {
    require(m.format == FORMAT, "format")?;
    validate_context(&SharingContext {
        server_instance_id: m.server_instance_id,
        share_id: m.share_id,
        item_id: m.item_id,
        revision: 1,
        access_epoch: m.access_epoch,
        kind: m.kind,
    })?;
    require(
        m.owner_user_id != UserId::NIL && m.owner_device_id != DeviceId::NIL,
        "owner",
    )?;
    require(
        m.revision > 0 && m.revision <= i64::MAX as u64,
        "manifest_revision",
    )?;
    require(m.revision == m.access_epoch, "manifest_epoch")?;
    require(
        m.previous_manifest_hash.len() == 32,
        "previous_manifest_hash",
    )?;
    require((1..=MAX_MEMBERS).contains(&m.members.len()), "members")?;
    let mut seen = BTreeSet::new();
    let mut owner = false;
    for member in &m.members {
        validate_member(member)?;
        require(seen.insert(member.device_id), "duplicate_device")?;
        if member.device_id == m.owner_device_id {
            require(
                member.user_id == m.owner_user_id && member.role.can_write(),
                "owner_role",
            )?;
            owner = true;
        }
    }
    require(owner, "missing_owner")?;
    if m.revision == 1 {
        require(
            m.access_epoch == 1 && m.previous_manifest_hash.as_slice() == [0; 32],
            "genesis",
        )?;
    }
    Ok(())
}

pub fn validate_member(m: &SharingMember) -> Result<(), SharingValidationError> {
    require(
        m.user_id != UserId::NIL && m.device_id != DeviceId::NIL,
        "member_id",
    )?;
    require(m.encryption_public_key.len() == 32, "encryption_public_key")?;
    require(m.signing_public_key.len() == 32, "signing_public_key")
}

pub fn validate_body(b: &SharedEncryptedBody) -> Result<(), SharingValidationError> {
    require(b.format == FORMAT, "format")?;
    require(
        (16..=MAX_CIPHERTEXT_BYTES).contains(&b.ciphertext.len()),
        "ciphertext",
    )?;
    require(b.nonce.len() == 24, "nonce")?;
    require((1..=MAX_MEMBERS).contains(&b.envelopes.len()), "envelopes")?;
    let mut seen = BTreeSet::new();
    for e in &b.envelopes {
        require(
            e.recipient_device_id != DeviceId::NIL,
            "recipient_device_id",
        )?;
        require(seen.insert(e.recipient_device_id), "duplicate_envelope")?;
        require(e.ephemeral_public_key.len() == 32, "ephemeral_public_key")?;
        require(e.nonce.len() == 24 && e.ciphertext.len() == 48, "envelope")?;
    }
    Ok(())
}

pub fn validate_mutation(m: &SharingMutation) -> Result<(), SharingValidationError> {
    validate_context(&m.context)?;
    require(
        m.mutation_id != MutationId::NIL && m.writer_device_id != DeviceId::NIL,
        "mutation_id",
    )?;
    require(
        m.base_revision >= 0 && m.base_revision.checked_add(1) == Some(m.context.revision),
        "base_revision",
    )?;
    require(
        m.manifest_revision > 0 && m.manifest_revision <= i64::MAX as u64,
        "manifest_revision",
    )?;
    require(
        m.manifest_hash.len() == 32
            && m.previous_revision_hash.len() == 32
            && m.body_hash.len() == 32,
        "hash",
    )?;
    if m.base_revision == 0 {
        require(
            m.previous_revision_hash.as_slice() == [0; 32],
            "revision_genesis",
        )?;
    }
    require(
        match m.operation {
            SharingOperation::Put => m.body_hash.as_slice() != [0; 32],
            SharingOperation::Delete => m.body_hash.as_slice() == [0; 32] && m.base_revision > 0,
        },
        "operation_hash",
    )
}

/// Structural binding only: callers must additionally verify Ed25519 and
/// SHA-256 values and compare durable checkpoints before accepting state.
pub fn validate_state(state: &SharedItemState) -> Result<(), SharingValidationError> {
    let a = &state.access;
    let r = &state.revision;
    let m = &r.signed.mutation;
    let c = &m.context;
    validate_manifest(&a.manifest)?;
    validate_mutation(m)?;
    require(
        a.signature.len() == 64 && r.signed.signature.len() == 64,
        "signature",
    )?;
    let access = &a.manifest;
    require(
        c.server_instance_id == access.server_instance_id
            && c.share_id == access.share_id
            && c.item_id == access.item_id
            && c.kind == access.kind,
        "manifest_context",
    )?;
    require(
        c.access_epoch == access.access_epoch && m.manifest_revision == access.revision,
        "manifest_revision",
    )?;
    require(
        access
            .members
            .iter()
            .any(|member| member.device_id == m.writer_device_id && member.role.can_write()),
        "writer_role",
    )?;
    match (&r.body, m.operation) {
        (Some(body), SharingOperation::Put) => {
            validate_body(body)?;
            let members: BTreeSet<_> = access.members.iter().map(|m| m.device_id).collect();
            let recipients: BTreeSet<_> = body
                .envelopes
                .iter()
                .map(|e| e.recipient_device_id)
                .collect();
            require(members == recipients, "envelope_members")?;
        }
        (None, SharingOperation::Delete) => {}
        _ => return Err(SharingValidationError("operation_body")),
    }
    Ok(())
}

fn context_bytes(out: &mut Vec<u8>, c: &SharingContext) {
    out.extend_from_slice(c.server_instance_id.as_bytes());
    out.extend_from_slice(c.share_id.as_bytes());
    out.extend_from_slice(c.item_id.as_bytes());
    out.extend_from_slice(&c.revision.to_be_bytes());
    out.extend_from_slice(&c.access_epoch.to_be_bytes());
    out.push(c.kind.tag());
}

pub fn sharing_object_aad(c: &SharingContext) -> Result<Vec<u8>, SharingValidationError> {
    validate_context(c)?;
    let mut out = labels::OBJECT_AAD.to_vec();
    out.extend_from_slice(&FORMAT.to_be_bytes());
    context_bytes(&mut out, c);
    Ok(out)
}

pub fn sharing_envelope_aad(
    c: &SharingContext,
    member: &SharingMember,
    ephemeral: &[u8; 32],
) -> Result<Vec<u8>, SharingValidationError> {
    validate_context(c)?;
    validate_member(member)?;
    let mut out = labels::ENVELOPE_AAD.to_vec();
    out.extend_from_slice(&FORMAT.to_be_bytes());
    context_bytes(&mut out, c);
    out.extend_from_slice(member.user_id.as_bytes());
    out.extend_from_slice(member.device_id.as_bytes());
    out.extend_from_slice(member.encryption_public_key.as_slice());
    out.extend_from_slice(member.signing_public_key.as_slice());
    out.extend_from_slice(ephemeral);
    Ok(out)
}

pub fn sharing_manifest_message(m: &AccessManifest) -> Result<Vec<u8>, SharingValidationError> {
    validate_manifest(m)?;
    let mut out = labels::MANIFEST.to_vec();
    out.extend_from_slice(&m.format.to_be_bytes());
    out.extend_from_slice(m.server_instance_id.as_bytes());
    out.extend_from_slice(m.share_id.as_bytes());
    out.extend_from_slice(m.item_id.as_bytes());
    out.extend_from_slice(m.owner_user_id.as_bytes());
    out.extend_from_slice(m.owner_device_id.as_bytes());
    out.extend_from_slice(&m.revision.to_be_bytes());
    out.extend_from_slice(&m.access_epoch.to_be_bytes());
    out.extend_from_slice(m.previous_manifest_hash.as_slice());
    out.push(m.kind.tag());
    out.extend_from_slice(&(m.members.len() as u32).to_be_bytes());
    let mut members: Vec<_> = m.members.iter().collect();
    members.sort_by_key(|member| member.device_id);
    for member in members {
        out.extend_from_slice(member.user_id.as_bytes());
        out.extend_from_slice(member.device_id.as_bytes());
        out.extend_from_slice(member.encryption_public_key.as_slice());
        out.extend_from_slice(member.signing_public_key.as_slice());
        out.push(member.role.tag());
    }
    Ok(out)
}

pub fn sharing_body_message(b: &SharedEncryptedBody) -> Result<Vec<u8>, SharingValidationError> {
    validate_body(b)?;
    let mut out = labels::BODY.to_vec();
    out.extend_from_slice(&b.format.to_be_bytes());
    out.extend_from_slice(&(b.ciphertext.len() as u32).to_be_bytes());
    out.extend_from_slice(b.ciphertext.as_slice());
    out.extend_from_slice(b.nonce.as_slice());
    out.extend_from_slice(&(b.envelopes.len() as u32).to_be_bytes());
    let mut envelopes: Vec<_> = b.envelopes.iter().collect();
    envelopes.sort_by_key(|envelope| envelope.recipient_device_id);
    for e in envelopes {
        out.extend_from_slice(e.recipient_device_id.as_bytes());
        out.extend_from_slice(e.ephemeral_public_key.as_slice());
        out.extend_from_slice(e.nonce.as_slice());
        out.extend_from_slice(e.ciphertext.as_slice());
    }
    Ok(out)
}

pub fn sharing_mutation_message(m: &SharingMutation) -> Result<Vec<u8>, SharingValidationError> {
    validate_mutation(m)?;
    let mut out = labels::MUTATION.to_vec();
    out.extend_from_slice(&FORMAT.to_be_bytes());
    context_bytes(&mut out, &m.context);
    out.extend_from_slice(m.mutation_id.as_bytes());
    out.extend_from_slice(&m.base_revision.to_be_bytes());
    out.extend_from_slice(&m.manifest_revision.to_be_bytes());
    out.extend_from_slice(m.manifest_hash.as_slice());
    out.extend_from_slice(m.writer_device_id.as_bytes());
    out.extend_from_slice(m.previous_revision_hash.as_slice());
    out.push(m.operation.tag());
    out.extend_from_slice(m.body_hash.as_slice());
    Ok(out)
}
