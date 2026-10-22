//! UI values for selective sharing. Directory identity is never trusted until
//! a human explicitly confirms the complete instance/account/device key code.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SharingKindDto {
    Host,
    Snippet,
    Group,
    Secret,
    KubeContext,
    DbConnection,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SharingRoleDto {
    Reader,
    Editor,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SharingTrustDto {
    Unverified,
    Verified,
    Blocked,
    Deleted,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SharingIdentityDto {
    pub server_instance_id: String,
    pub user_id: String,
    pub device_id: String,
    /// Hexadecimal public keys, not secret material.
    pub encryption_public_key: String,
    pub signing_public_key: String,
    /// SHA-256 code binds the instance, account, device and both public keys.
    pub verification_code: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SharingRecipientDto {
    pub user_id: String,
    pub devices: Vec<SharingIdentityDto>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SharingGrantDto {
    pub identity: SharingIdentityDto,
    pub role: SharingRoleDto,
    /// Entered/confirmed by the human after an independent comparison.
    pub confirmed_code: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SharingMemberDto {
    pub identity: SharingIdentityDto,
    pub role: SharingRoleDto,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SharingStatusDto {
    pub enabled: bool,
    pub locked: bool,
    pub server_instance_id: Option<String>,
    pub identity: Option<SharingIdentityDto>,
    pub pending: u32,
    pub blocked: u32,
    pub supports_groups: bool,
    pub supports_secrets: bool,
    pub supports_kube_contexts: bool,
    pub supports_db_connections: bool,
    pub supports_owner_online_enrollment_v1: bool,
    /// Server protocol 1.8 `capabilities.share_leave`: recipients may leave.
    #[serde(default)]
    pub supports_leave: bool,
    /// Server protocol 1.8 `capabilities.share_restore`: owner-deleted items
    /// stay restorable for the server's restore window.
    #[serde(default)]
    pub supports_restore: bool,
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SharingItemDto {
    pub share_id: String,
    pub item_id: String,
    pub kind: SharingKindDto,
    pub owner_user_id: String,
    pub owner_device_id: String,
    pub revision: i64,
    pub access_epoch: u64,
    pub owned: bool,
    pub role: Option<SharingRoleDto>,
    pub trust: SharingTrustDto,
    pub blocked_reason: Option<String>,
    /// Transient strict projection JSON, present only while unlocked and
    /// after signature, chain and pinned-owner verification. Never an LLM API.
    pub preview_json: Option<String>,
    pub members: Vec<SharingIdentityDto>,
    pub member_roles: Vec<SharingMemberDto>,
    /// Owner view: accounts that left this share (protocol 1.8). They are
    /// already omitted from `members`/`member_roles`, so the next access
    /// change drops them from the signed manifest.
    #[serde(default)]
    pub departed_user_ids: Vec<String>,
}

impl std::fmt::Debug for SharingItemDto {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SharingItemDto")
            .field("share_id", &self.share_id)
            .field("trust", &self.trust)
            .field("preview_json", &"<redacted>")
            .finish_non_exhaustive()
    }
}

impl Drop for SharingItemDto {
    fn drop(&mut self) {
        use zeroize::Zeroize as _;
        self.preview_json.zeroize();
    }
}

/// The shared list plus how many entries could not be read (malformed, or
/// their signatures, trust state or ciphertext did not verify). Those are
/// skipped and never shown or used; the rest of the list stays usable.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SharingListDto {
    pub items: Vec<SharingItemDto>,
    pub unreadable: u32,
}

/// An owner-deleted item that can still be restored (protocol 1.8). Its
/// preview is the same masked metadata as `SharingItemDto.preview_json`.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SharingDeletedItemDto {
    pub share_id: String,
    pub kind: SharingKindDto,
    pub preview_json: Option<String>,
    /// Milliseconds since the Unix epoch.
    pub deleted_at_ms: i64,
    pub restorable_until_ms: i64,
    /// Recipient devices a restore shares the item with again.
    pub recipients: u32,
}

impl std::fmt::Debug for SharingDeletedItemDto {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SharingDeletedItemDto")
            .field("share_id", &self.share_id)
            .field("preview_json", &"<redacted>")
            .finish_non_exhaustive()
    }
}

impl Drop for SharingDeletedItemDto {
    fn drop(&mut self) {
        use zeroize::Zeroize as _;
        self.preview_json.zeroize();
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SharingInvitationDto {
    pub item: SharingItemDto,
    pub owner: SharingIdentityDto,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SharingOutboxStateDto {
    Pending,
    /// CAS, removed access or changed rights requires a new human decision.
    Blocked,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SharingOutboxEntryDto {
    pub mutation_id: String,
    pub share_id: String,
    pub state: SharingOutboxStateDto,
    /// Static machine-readable reason. Never server-provided body/text.
    pub reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SharingOutboxDto {
    pub entries: Vec<SharingOutboxEntryDto>,
}
