//! Teams (protocol 1.9): this member's link to a team, kept in the member's
//! **personal** vault so every device of the member can open the team.

use crate::secret::SecretValue;
use crate::{ObjectId, Timestamp};
use cc_protocol::VaultId;
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
    pub created_at: Timestamp,
    pub updated_at: Timestamp,
}
