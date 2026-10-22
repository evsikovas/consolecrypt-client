//! Teams (protocol 1.9): team vaults shared by several accounts.
//!
//! JSON results: `TeamDto`, `TeamMemberDto`, `TeamInvitationDto`,
//! `TeamJoinRequestDto`, `TeamJoinDto`, `TeamLogEntryDto`,
//! `ObjectVaultDto`, `VaultSettingsDto`, `GroupDto` (see
//! `cc_app_core::team_dto`). Roles are `owner` | `admin` | `editor` |
//! `viewer`.

use crate::api::error::BridgeError;
use crate::state::{from_json, to_json, with_core};
use cc_app_core::team_dto::TeamRoleDto;
use cc_app_core::{GroupDto, VaultSettingsDto};

fn role(s: &str) -> Result<TeamRoleDto, BridgeError> {
    Ok(match s.trim() {
        "owner" => TeamRoleDto::Owner,
        "admin" => TeamRoleDto::Admin,
        "editor" => TeamRoleDto::Editor,
        "viewer" => TeamRoleDto::Viewer,
        _ => return Err(BridgeError::invalid("role", "unknown role")),
    })
}

/// `Vec<TeamDto>` of the open profile (sidebar order).
pub async fn teams_list() -> Result<String, BridgeError> {
    with_core(move |c| async move { to_json(&c.teams().await?) }).await
}

/// Re-open every team from the server and sync it → `Vec<TeamDto>`.
pub async fn teams_refresh() -> Result<String, BridgeError> {
    with_core(move |c| async move { to_json(&c.refresh_teams().await?) }).await
}

/// `Vec<ObjectVaultDto>`: the team of every team object.
pub async fn teams_object_vaults() -> Result<String, BridgeError> {
    with_core(move |c| async move { to_json(&c.object_vaults().await?) }).await
}

/// Create a team owned by this account → `TeamDto`.
pub async fn teams_create(
    name: String,
    color: Option<String>,
    icon: Option<String>,
    description: Option<String>,
) -> Result<String, BridgeError> {
    with_core(
        move |c| async move { to_json(&c.create_team(name, color, icon, description).await?) },
    )
    .await
}

/// `Vec<TeamMemberDto>` from the verified membership log.
pub async fn teams_members(vault_id: String) -> Result<String, BridgeError> {
    with_core(move |c| async move { to_json(&c.team_members(vault_id).await?) }).await
}

/// `Vec<TeamLogEntryDto>`, newest first.
pub async fn teams_log(vault_id: String) -> Result<String, BridgeError> {
    with_core(move |c| async move { to_json(&c.team_log(vault_id).await?) }).await
}

/// `VaultSettingsDto` of a team.
pub async fn teams_settings(vault_id: String) -> Result<String, BridgeError> {
    with_core(move |c| async move { to_json(&c.team_settings(vault_id).await?) }).await
}

/// Save team settings (admins) → `VaultSettingsDto`.
pub async fn teams_save_settings(
    vault_id: String,
    settings_json: String,
) -> Result<String, BridgeError> {
    let settings: VaultSettingsDto = from_json("settings", &settings_json)?;
    with_core(move |c| async move { to_json(&c.save_team_settings(vault_id, settings).await?) })
        .await
}

/// This member's colour override, order and visibility → `TeamDto`.
pub async fn teams_set_preferences(
    vault_id: String,
    local_color: Option<String>,
    sort_order: i32,
    hidden: bool,
) -> Result<String, BridgeError> {
    with_core(move |c| async move {
        to_json(
            &c.set_team_preferences(vault_id, local_color, sort_order, hidden)
                .await?,
        )
    })
    .await
}

/// Invite by email (admins). The server never reveals whether the account
/// exists; `note` is readable by the server.
pub async fn teams_invite(
    vault_id: String,
    email: String,
    role_name: String,
    note: Option<String>,
) -> Result<(), BridgeError> {
    let role = role(&role_name)?;
    with_core(move |c| async move { Ok(c.invite_team_member(vault_id, email, role, note).await?) })
        .await
}

/// `Vec<TeamInvitationDto>` of a team (admins).
pub async fn teams_invitations(vault_id: String) -> Result<String, BridgeError> {
    with_core(move |c| async move { to_json(&c.team_invitations(vault_id).await?) }).await
}

/// `Vec<TeamInvitationDto>` addressed to this account.
pub async fn teams_incoming_invitations() -> Result<String, BridgeError> {
    with_core(move |c| async move { to_json(&c.incoming_team_invitations().await?) }).await
}

/// Accept an invitation → `TeamJoinDto` (the code to compare).
pub async fn teams_accept_invitation(invitation_id: String) -> Result<String, BridgeError> {
    with_core(move |c| async move { to_json(&c.accept_team_invitation(invitation_id).await?) })
        .await
}

/// Decline / cancel an invitation, or reject a join whose code differs.
pub async fn teams_close_invitation(invitation_id: String) -> Result<(), BridgeError> {
    with_core(move |c| async move { Ok(c.close_team_invitation(invitation_id).await?) }).await
}

/// `Vec<TeamJoinRequestDto>`: joins waiting for this admin's code check.
pub async fn teams_join_requests(vault_id: String) -> Result<String, BridgeError> {
    with_core(move |c| async move { to_json(&c.team_join_requests(vault_id).await?) }).await
}

/// "Codes match": add the member → `TeamDto`.
pub async fn teams_confirm_member(
    vault_id: String,
    invitation_id: String,
) -> Result<String, BridgeError> {
    with_core(
        move |c| async move { to_json(&c.confirm_team_member(vault_id, invitation_id).await?) },
    )
    .await
}

pub async fn teams_change_role(
    vault_id: String,
    user_id: String,
    role_name: String,
) -> Result<String, BridgeError> {
    let role = role(&role_name)?;
    with_core(move |c| async move { to_json(&c.change_team_role(vault_id, user_id, role).await?) })
        .await
}

pub async fn teams_transfer_owner(
    vault_id: String,
    user_id: String,
) -> Result<String, BridgeError> {
    with_core(move |c| async move { to_json(&c.transfer_team_owner(vault_id, user_id).await?) })
        .await
}

/// Remove a member and rotate the team key → `TeamDto`.
pub async fn teams_remove_member(vault_id: String, user_id: String) -> Result<String, BridgeError> {
    with_core(move |c| async move { to_json(&c.remove_team_member(vault_id, user_id).await?) })
        .await
}

pub async fn teams_leave(vault_id: String) -> Result<(), BridgeError> {
    with_core(move |c| async move { Ok(c.leave_team(vault_id).await?) }).await
}

/// Rotate the team key (admins) → `TeamDto`.
pub async fn teams_rotate_key(vault_id: String) -> Result<String, BridgeError> {
    with_core(move |c| async move { to_json(&c.rotate_team_key(vault_id).await?) }).await
}

/// Create a group directly in a team (`GroupDto` JSON, empty id).
pub async fn teams_create_group(
    vault_id: String,
    group_json: String,
) -> Result<String, BridgeError> {
    let mut group: GroupDto = from_json("group", &group_json)?;
    group.id.clear();
    with_core(move |c| async move { to_json(&c.create_team_group(vault_id, group).await?) }).await
}

/// Copy objects (and what they reference) into a team, or into the
/// personal vault with `target_vault_id = None` → new ids (JSON list).
pub async fn teams_copy_objects(
    ids: Vec<String>,
    target_vault_id: Option<String>,
) -> Result<String, BridgeError> {
    with_core(move |c| async move { to_json(&c.copy_to_vault(ids, target_vault_id).await?) }).await
}

/// Move objects: copy, send, then delete the originals → new ids.
pub async fn teams_move_objects(
    ids: Vec<String>,
    target_vault_id: Option<String>,
) -> Result<String, BridgeError> {
    with_core(move |c| async move { to_json(&c.move_to_vault(ids, target_vault_id).await?) }).await
}
