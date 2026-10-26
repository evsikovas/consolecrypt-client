//! Teams (protocol 1.9): this member's link to a team, kept in the member's
//! **personal** vault so every device of the member can open the team.

use crate::secret::SecretValue;
use crate::{ObjectId, Timestamp};
use cc_protocol::{TeamInvitationId, VaultId};
use serde::{Deserialize, Serialize};

/// Membership of the vault owner in one team.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TeamMembership {
    pub id: ObjectId,
    /// The team vault.
    pub team_vault_id: VaultId,
    /// Per-team identity secret (base64 of crypto-core's encoding). Never
    /// leaves the encrypted payload.
    pub identity_secret: SecretValue,
    /// Last membership-log head this member verified: `(seq, hex hash)`.
    /// A shorter or different log later means the server rolled it back.
    #[serde(default)]
    pub verified_log_seq: u64,
    #[serde(default)]
    pub verified_log_head: String,
    /// Personal colour override (palette name or `#rrggbb`); synced only
    /// between this member's devices.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub local_color: Option<String>,
    /// Sidebar position among teams.
    #[serde(default)]
    pub sort_order: i32,
    /// Collapsed / hidden in the sidebar.
    #[serde(default)]
    pub hidden: bool,
    /// Set while the join waits for an admin to compare the member code;
    /// cleared once the verified log contains this member.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub invitation_id: Option<TeamInvitationId>,
    /// Hex Ed25519 key of the admin whose member code this member compared
    /// when joining. The first verified log must add this member with an
    /// entry signed by that key, so a server cannot hand a joiner a team of
    /// its own (security review SEC-02). `None` for owners and older joins.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub join_admin_key: Option<String>,
    /// The team key this member last accepted: its epoch and the hex key
    /// commitment. The server may hand out only that key for that epoch, or
    /// a newer key whose chain links lead back to it (security review
    /// SEC-03). `None` until the first open with this check.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pinned_key_epoch: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pinned_key_commitment: Option<String>,
    pub created_at: Timestamp,
    pub updated_at: Timestamp,
}
