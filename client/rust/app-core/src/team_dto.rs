//! DTOs of the teams facade (protocol 1.9, TZ teams §6).

use cc_models::settings::VaultPolicies;
use cc_protocol::vaults::VaultRole;
use serde::{Deserialize, Serialize};

/// A member's role in a team.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TeamRoleDto {
    Owner,
    Admin,
    Editor,
    Viewer,
}

impl From<VaultRole> for TeamRoleDto {
    fn from(r: VaultRole) -> Self {
        match r {
            VaultRole::Owner => TeamRoleDto::Owner,
            VaultRole::Admin => TeamRoleDto::Admin,
            VaultRole::Editor => TeamRoleDto::Editor,
            VaultRole::Viewer => TeamRoleDto::Viewer,
        }
    }
}

impl From<TeamRoleDto> for VaultRole {
    fn from(r: TeamRoleDto) -> Self {
        match r {
            TeamRoleDto::Owner => VaultRole::Owner,
            TeamRoleDto::Admin => VaultRole::Admin,
            TeamRoleDto::Editor => VaultRole::Editor,
            TeamRoleDto::Viewer => VaultRole::Viewer,
        }
    }
}

/// One team of the profile, as this member sees it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TeamDto {
    pub vault_id: String,
    /// Team name from the team's settings (empty until they are readable).
    pub name: String,
    /// Colour to display: the personal override, else the team colour.
    pub color: Option<String>,
    /// Colour the admins chose for everyone.
    pub team_color: Option<String>,
    /// This member's personal override (synced between own devices only).
    pub local_color: Option<String>,
    pub icon: Option<String>,
    pub description: Option<String>,
    pub policies: VaultPolicies,
    /// `None` while the join waits for confirmation or the team is closed.
    pub role: Option<TeamRoleDto>,
    pub members_count: u32,
    pub key_epoch: u32,
    /// A member left or was removed: an admin should rotate the key.
    pub rotation_required: bool,
    /// `invalid_log` | `log_rollback`: paused, read-only.
    pub warning: Option<String>,
    /// Opened from the local cache (server unreachable).
    pub offline: bool,
    /// Join accepted here, an admin still has to compare the code.
    pub pending: bool,
    /// Open in this session (keys available, objects loaded).
    pub open: bool,
    pub sort_order: i32,
    pub hidden: bool,
    /// Live objects of the team in the working set.
    pub object_count: u32,
}

/// A member from the verified membership log.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TeamMemberDto {
    pub user_id: String,
    /// From the server listing (not part of the signed log).
    pub email: String,
    pub role: TeamRoleDto,
    pub is_self: bool,
    pub joined_at_ms: i64,
}

/// An invitation (incoming, or of a team for its admins).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TeamInvitationDto {
    pub invitation_id: String,
    pub vault_id: String,
    pub inviter_email: String,
    pub invitee_email: String,
    pub role: TeamRoleDto,
    /// Plain text the server can read.
    pub note: Option<String>,
    /// `pending` | `accepted` | `completed` | `declined` | `cancelled` | `expired`.
    pub status: String,
    pub created_at_ms: i64,
    pub expires_at_ms: i64,
}

/// An accepted invitation waiting for this admin: compare `code` with the
/// one on the invitee's screen, then confirm or reject.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TeamJoinRequestDto {
    pub invitation_id: String,
    pub vault_id: String,
    pub invitee_email: String,
    pub role: TeamRoleDto,
    /// Member code computed from this admin's own key and the invitee's key.
    pub code: String,
}

/// Result of accepting an invitation: the code to read to the admin.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TeamJoinDto {
    pub invitation_id: String,
    pub vault_id: String,
    pub inviter_email: String,
    pub code: String,
}

/// One verified membership-log entry (team settings → history).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TeamLogEntryDto {
    pub seq: u64,
    pub issued_at_ms: i64,
    pub actor_user_id: String,
    pub actor_email: String,
    /// `genesis` | `add_member` | `change_role` | `remove_member` | `leave`
    /// | `transfer_owner` | `rotate_key`.
    pub op: String,
    pub target_user_id: Option<String>,
    pub target_email: Option<String>,
    pub role: Option<TeamRoleDto>,
    pub key_epoch: Option<u32>,
}

/// Which team an object belongs to (objects absent from the list are
/// personal).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ObjectVaultDto {
    pub object_id: String,
    pub vault_id: String,
}
