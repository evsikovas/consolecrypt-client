//! Teams facade (protocol 1.9, TZ teams §5–6): create a team, invite,
//! join with a compared member code, roles, leave / remove, key rotation,
//! settings and the per-member display preferences, copy / move objects.
//!
//! Every membership change is a log entry this client signs itself with
//! its per-team key after checking it against the log it verified; keys
//! are sealed only to members of that verified log.

use crate::app::AppCore;
use crate::dto::{ms, AppEvent, EditableDto, VaultSettingsDto};
use crate::error::{AppError, AppResult};
use crate::session::{Session, Unlocked};
use crate::team_dto::*;
use crate::teams::{close_team, encode_identity, membership_of, TeamHandle, TeamWarning};
use cc_crypto_core::teams::{team_member_code, TeamIdentity};
use cc_models::settings::VaultSettings;
use cc_models::team::TeamMembership;
use cc_models::{ObjectId, VaultObject};
use cc_protocol::teams::{
    AcceptTeamInvitationRequest, CreateTeamRequest, InviteTeamMemberRequest, SignedTeamLogEntry,
    SubmitTeamLogRequest, TeamInvitation, TeamInvitationStatus, TeamLogOp, TeamMemberKeys,
};
use cc_protocol::vaults::VaultRole;
use cc_protocol::{TeamInvitationId, UserId, VaultId};
use cc_storage_core::ProfileKind;
use cc_sync_core::ApiClient;
use cc_vault_core::UnlockedVault;
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

fn parse_vault(id: &str) -> AppResult<VaultId> {
    id.parse()
        .map_err(|_| AppError::invalid("vault_id", "not a valid id"))
}

fn parse_user(id: &str) -> AppResult<UserId> {
    id.parse()
        .map_err(|_| AppError::invalid("user_id", "not a valid id"))
}

fn parse_invitation(id: &str) -> AppResult<TeamInvitationId> {
    id.parse()
        .map_err(|_| AppError::invalid("invitation_id", "not a valid id"))
}

fn status_name(s: TeamInvitationStatus) -> &'static str {
    match s {
        TeamInvitationStatus::Pending => "pending",
        TeamInvitationStatus::Accepted => "accepted",
        TeamInvitationStatus::Completed => "completed",
        TeamInvitationStatus::Declined => "declined",
        TeamInvitationStatus::Cancelled => "cancelled",
        TeamInvitationStatus::Expired => "expired",
    }
}

fn invitation_dto(i: &TeamInvitation) -> TeamInvitationDto {
    TeamInvitationDto {
        invitation_id: i.invitation_id.to_string(),
        vault_id: i.vault_id.to_string(),
        inviter_email: i.inviter_email.clone(),
        invitee_email: i.invitee_email.clone(),
        role: i.role.into(),
        note: i.note.clone(),
        status: status_name(i.status).into(),
        created_at_ms: ms(&i.created_at),
        expires_at_ms: ms(&i.expires_at),
    }
}

fn code_string(code: cc_crypto_core::VerificationCode) -> String {
    code.group_strings().join(" ")
}

fn now_secs() -> i64 {
    chrono::Utc::now().timestamp()
}

/// The open session of a synced profile with its account.
struct TeamCtx {
    session: Arc<Session>,
    unlocked: Arc<Unlocked>,
    api: ApiClient,
    user_id: UserId,
}

impl AppCore {
    async fn team_ctx(&self) -> AppResult<TeamCtx> {
        let (session, unlocked) = self.unlocked().await?;
        let profile = session.profile().await?;
        if profile.kind == ProfileKind::Local {
            return Err(AppError::LocalProfile);
        }
        let user_id = profile.user_id.ok_or(AppError::LocalProfile)?;
        let api = session.api()?;
        Ok(TeamCtx {
            session,
            unlocked,
            api,
            user_id,
        })
    }

    // ---- listing ---------------------------------------------------------------

    /// Whether the profile's server offers teams (`capabilities.teams`).
    /// Offline, a profile that already has teams keeps them visible.
    pub async fn teams_supported(&self) -> AppResult<bool> {
        let (s, u) = self.unlocked().await?;
        if s.kind().await? == ProfileKind::Local {
            return Ok(false);
        }
        let has_teams = u
            .writer
            .working
            .objects()
            .iter()
            .any(|o| matches!(o, VaultObject::TeamMembership(_)));
        match s.api_opt() {
            Some(api) => match api.meta().await {
                Ok(m) => Ok(m.capabilities.teams || has_teams),
                Err(_) => Ok(has_teams),
            },
            None => Ok(has_teams),
        }
    }

    /// Teams of the profile (open ones and joins waiting for confirmation).
    pub async fn teams(&self) -> AppResult<Vec<TeamDto>> {
        let (_, u) = self.unlocked().await?;
        let mut out: Vec<TeamDto> = u
            .writer
            .working
            .objects()
            .into_iter()
            .filter_map(|o| match o {
                VaultObject::TeamMembership(m) => Some(m),
                _ => None,
            })
            .map(|m| team_dto(&u, &m))
            .collect();
        out.sort_by(|a, b| {
            (a.sort_order, a.name.to_lowercase()).cmp(&(b.sort_order, b.name.to_lowercase()))
        });
        Ok(out)
    }

    /// Re-open every team from the server (or its cache).
    pub async fn refresh_teams(&self) -> AppResult<Vec<TeamDto>> {
        let (s, u) = self.unlocked().await?;
        s.open_teams(&u).await?;
        crate::teams::sync_teams(&u.writer).await;
        self.teams().await
    }

    /// Which team each team object belongs to (for chips and filters).
    pub async fn object_vaults(&self) -> AppResult<Vec<ObjectVaultDto>> {
        let (_, u) = self.unlocked().await?;
        Ok(u.writer
            .working
            .team_origins()
            .into_iter()
            .map(|(o, v)| ObjectVaultDto {
                object_id: o.to_string(),
                vault_id: v.to_string(),
            })
            .collect())
    }

    /// Verified members of a team.
    pub async fn team_members(&self, vault_id: String) -> AppResult<Vec<TeamMemberDto>> {
        let (_, u) = self.unlocked().await?;
        let team = u.writer.teams.require(parse_vault(&vault_id)?)?;
        let st = team.state();
        let listed: BTreeMap<UserId, &cc_protocol::teams::TeamMember> =
            st.listed.iter().map(|m| (m.user_id, m)).collect();
        let mut out: Vec<TeamMemberDto> = st
            .view
            .members
            .iter()
            .map(|(id, m)| TeamMemberDto {
                user_id: id.to_string(),
                email: listed.get(id).map(|l| l.email.clone()).unwrap_or_default(),
                role: m.role.into(),
                is_self: *id == team.user_id,
                joined_at_ms: listed.get(id).map(|l| ms(&l.joined_at)).unwrap_or(0),
            })
            .collect();
        out.sort_by_key(|m| (role_rank(m.role), m.email.to_lowercase()));
        Ok(out)
    }

    /// The verified membership log, newest first.
    pub async fn team_log(&self, vault_id: String) -> AppResult<Vec<TeamLogEntryDto>> {
        let (_, u) = self.unlocked().await?;
        let team = u.writer.teams.require(parse_vault(&vault_id)?)?;
        let st = team.state();
        let emails: BTreeMap<UserId, String> = st
            .listed
            .iter()
            .map(|m| (m.user_id, m.email.clone()))
            .collect();
        let email = |u: &UserId| emails.get(u).cloned().unwrap_or_default();
        let mut out: Vec<TeamLogEntryDto> = st
            .log
            .iter()
            .take_while(|e| e.entry.seq <= st.view.seq)
            .map(|signed| {
                let e = &signed.entry;
                let (op, target, role, key_epoch) = match &e.op {
                    TeamLogOp::Genesis { owner } => {
                        ("genesis", Some(owner.user_id), Some(VaultRole::Owner), None)
                    }
                    TeamLogOp::AddMember { member, role } => {
                        ("add_member", Some(member.user_id), Some(*role), None)
                    }
                    TeamLogOp::ChangeRole { user_id, role } => {
                        ("change_role", Some(*user_id), Some(*role), None)
                    }
                    TeamLogOp::RemoveMember { user_id } => {
                        ("remove_member", Some(*user_id), None, None)
                    }
                    TeamLogOp::Leave => ("leave", None, None, None),
                    TeamLogOp::TransferOwner { user_id } => (
                        "transfer_owner",
                        Some(*user_id),
                        Some(VaultRole::Owner),
                        None,
                    ),
                    TeamLogOp::RotateKey { key_epoch } => {
                        ("rotate_key", None, None, Some(*key_epoch))
                    }
                };
                TeamLogEntryDto {
                    seq: e.seq,
                    issued_at_ms: e.issued_at.saturating_mul(1000),
                    actor_user_id: e.actor_user_id.to_string(),
                    actor_email: email(&e.actor_user_id),
                    op: op.into(),
                    target_user_id: target.map(|t| t.to_string()),
                    target_email: target.map(|t| email(&t)),
                    role: role.map(Into::into),
                    key_epoch,
                }
            })
            .collect();
        out.reverse();
        Ok(out)
    }

    // ---- create / settings -------------------------------------------------------

    /// Create a team owned by this account (TZ teams criterion 1).
    pub async fn create_team(
        &self,
        name: String,
        color: Option<String>,
        icon: Option<String>,
        description: Option<String>,
    ) -> AppResult<TeamDto> {
        let cx = self.team_ctx().await?;
        let now = chrono::Utc::now();
        let mut settings = VaultSettingsDto::from_model(&VaultSettings {
            id: ObjectId::new(),
            vault_name: name,
            terminal_history_mode: Default::default(),
            default_privacy_profile: Default::default(),
            sync_ai_conversations: false,
            color: None,
            icon: None,
            description: None,
            policies: Default::default(),
            sharing_policy: Default::default(),
            created_at: now,
            updated_at: now,
        });
        settings.color = color;
        settings.icon = icon;
        settings.description = description;
        let settings = settings.to_model(ObjectId::new(), None)?;

        let vault_id = VaultId::new();
        let identity = TeamIdentity::generate().map_err(|e| AppError::Crypto(e.to_string()))?;
        let keys = identity.member_keys(cx.user_id);
        let vault = UnlockedVault::new_team(vault_id)?;
        let genesis = identity
            .sign_entry(
                vault_id,
                cx.user_id,
                None,
                now_secs(),
                TeamLogOp::Genesis {
                    owner: keys.clone(),
                },
            )
            .map_err(|e| AppError::Crypto(e.to_string()))?;
        let key_commitment =
            identity.sign_key_commitment(vault_id, cx.user_id, 0, vault.key_commitment());
        let req = CreateTeamRequest {
            vault_id,
            key_commitment: Some(key_commitment),
            vault_access_key: vault.vault_access_key(),
            genesis: genesis.clone(),
            owner_envelope: vault.seal_for_member(&keys)?,
            device_envelope: vault.trust_envelope_for(&cx.session.identity())?,
        };
        // The membership goes first: a team the server created must never
        // lose the only copy of its owner key.
        let membership = TeamMembership {
            id: ObjectId::new(),
            team_vault_id: vault_id,
            identity_secret: encode_identity(&identity),
            verified_log_seq: 0,
            verified_log_head: hex(&cc_crypto_core::teams::team_entry_hash(&genesis)),
            local_color: None,
            sort_order: next_sort_order(&cx.unlocked),
            hidden: false,
            invitation_id: None,
            join_admin_key: None,
            pinned_key_epoch: None,
            pinned_key_commitment: None,
            created_at: now,
            updated_at: now,
        };
        let membership_id = membership.id;
        cx.unlocked
            .writer
            .put(VaultObject::TeamMembership(membership))
            .await?;
        if let Err(e) = cx.api.create_team(&req).await {
            let _ = cx.unlocked.writer.delete(membership_id).await;
            return Err(e.into());
        }
        cx.session.refresh_team(&cx.unlocked, vault_id).await?;
        cx.unlocked
            .writer
            .put_team_settings(vault_id, settings)
            .await?;
        self.team(vault_id).await
    }

    async fn team(&self, vault_id: VaultId) -> AppResult<TeamDto> {
        self.teams()
            .await?
            .into_iter()
            .find(|t| t.vault_id == vault_id.to_string())
            .ok_or_else(|| AppError::not_found("team", vault_id))
    }

    /// Team settings (everyone reads; admins save).
    pub async fn team_settings(&self, vault_id: String) -> AppResult<VaultSettingsDto> {
        let (_, u) = self.unlocked().await?;
        let team = u.writer.teams.require(parse_vault(&vault_id)?)?;
        team.state()
            .settings
            .as_ref()
            .map(VaultSettingsDto::from_model)
            .ok_or_else(|| AppError::not_found("team_settings", vault_id))
    }

    /// Save team settings: name, colour, icon, description, policies.
    pub async fn save_team_settings(
        &self,
        vault_id: String,
        settings: VaultSettingsDto,
    ) -> AppResult<VaultSettingsDto> {
        let (_, u) = self.unlocked().await?;
        let vid = parse_vault(&vault_id)?;
        let team = u.writer.teams.require(vid)?;
        let existing = team.state().settings;
        let id = existing
            .as_ref()
            .map(|s| s.id)
            .unwrap_or_else(ObjectId::new);
        let mut model = settings.to_model(id, existing.as_ref())?;
        model.updated_at = chrono::Utc::now();
        u.writer.put_team_settings(vid, model).await?;
        self.inner.ctx.emit(AppEvent::TeamsChanged);
        self.team_settings(vault_id).await
    }

    /// This member's display preferences (synced to own devices only).
    pub async fn set_team_preferences(
        &self,
        vault_id: String,
        local_color: Option<String>,
        sort_order: i32,
        hidden: bool,
    ) -> AppResult<TeamDto> {
        let (_, u) = self.unlocked().await?;
        let vid = parse_vault(&vault_id)?;
        let mut m =
            membership_of(&u.writer, vid).ok_or_else(|| AppError::not_found("team", vid))?;
        let color = local_color
            .map(|c| c.trim().to_owned())
            .filter(|c| !c.is_empty());
        if color
            .as_deref()
            .is_some_and(|c| !cc_models::settings::valid_color(c))
        {
            return Err(AppError::invalid("color", "a palette name or #rrggbb"));
        }
        m.local_color = color;
        m.sort_order = sort_order;
        m.hidden = hidden;
        m.updated_at = chrono::Utc::now();
        u.writer.put(VaultObject::TeamMembership(m)).await?;
        self.inner.ctx.emit(AppEvent::TeamsChanged);
        self.team(vid).await
    }

    // ---- invitations -------------------------------------------------------------

    /// Invite an account by email (admins). The server never says whether
    /// the account exists. `note` is readable by the server.
    pub async fn invite_team_member(
        &self,
        vault_id: String,
        email: String,
        role: TeamRoleDto,
        note: Option<String>,
    ) -> AppResult<()> {
        let cx = self.team_ctx().await?;
        let vid = parse_vault(&vault_id)?;
        let team = cx.unlocked.writer.teams.require(vid)?;
        let me = team.require_admin()?;
        let role = VaultRole::from(role);
        if role == VaultRole::Owner || (role == VaultRole::Admin && me != VaultRole::Owner) {
            return Err(AppError::invalid("role", "not_allowed"));
        }
        let email = crate::validate::email(&email)?;
        let note = note.map(|n| n.trim().to_owned()).filter(|n| !n.is_empty());
        cx.api
            .invite_team_member(vid, &InviteTeamMemberRequest { email, role, note })
            .await?;
        Ok(())
    }

    /// Invitations of a team (admins): pending and accepted ones.
    pub async fn team_invitations(&self, vault_id: String) -> AppResult<Vec<TeamInvitationDto>> {
        let cx = self.team_ctx().await?;
        let l = cx.api.team_invitations(parse_vault(&vault_id)?).await?;
        Ok(l.invitations.iter().map(invitation_dto).collect())
    }

    /// Invitations to this account.
    pub async fn incoming_team_invitations(&self) -> AppResult<Vec<TeamInvitationDto>> {
        let cx = self.team_ctx().await?;
        let l = cx.api.incoming_team_invitations().await?;
        Ok(l.invitations
            .iter()
            .filter(|i| i.status == TeamInvitationStatus::Pending)
            .map(invitation_dto)
            .collect())
    }

    /// Accept an invitation: create this member's team key and show the
    /// code to compare with the inviting admin (ТР-6).
    pub async fn accept_team_invitation(&self, invitation_id: String) -> AppResult<TeamJoinDto> {
        let cx = self.team_ctx().await?;
        let inv_id = parse_invitation(&invitation_id)?;
        let invitation = cx
            .api
            .incoming_team_invitations()
            .await?
            .invitations
            .into_iter()
            .find(|i| i.invitation_id == inv_id)
            .ok_or_else(|| AppError::not_found("invitation", inv_id))?;
        let vid = invitation.vault_id;
        if membership_of(&cx.unlocked.writer, vid).is_some_and(|m| m.invitation_id.is_none()) {
            return Err(AppError::AlreadyExists("team".into()));
        }
        let identity = TeamIdentity::generate().map_err(|e| AppError::Crypto(e.to_string()))?;
        let keys = identity.member_keys(cx.user_id);
        let now = chrono::Utc::now();
        // Keep the key before the server learns it (any own device can
        // finish the join).
        let existing = membership_of(&cx.unlocked.writer, vid);
        let membership = TeamMembership {
            id: existing.as_ref().map(|m| m.id).unwrap_or_default(),
            team_vault_id: vid,
            identity_secret: encode_identity(&identity),
            verified_log_seq: 0,
            verified_log_head: String::new(),
            local_color: None,
            sort_order: next_sort_order(&cx.unlocked),
            hidden: false,
            invitation_id: Some(inv_id),
            join_admin_key: None,
            pinned_key_epoch: None,
            pinned_key_commitment: None,
            created_at: now,
            updated_at: now,
        };
        cx.unlocked
            .writer
            .put(VaultObject::TeamMembership(membership.clone()))
            .await?;
        let accepted = cx
            .api
            .accept_team_invitation(
                inv_id,
                &AcceptTeamInvitationRequest {
                    encryption_key: keys.encryption_key.clone(),
                    signing_key: keys.signing_key.clone(),
                },
            )
            .await?;
        let admin_key: [u8; 32] = accepted
            .inviter_signing_key
            .to_array()
            .ok_or_else(|| AppError::Crypto("inviter key is malformed".into()))?;
        // The code shown below covers this admin key: the first verified log
        // must add us with an entry this key signed (SEC-02).
        cx.unlocked
            .writer
            .put(VaultObject::TeamMembership(TeamMembership {
                join_admin_key: Some(hex(&admin_key)),
                updated_at: chrono::Utc::now(),
                ..membership
            }))
            .await?;
        self.inner.ctx.emit(AppEvent::TeamsChanged);
        Ok(TeamJoinDto {
            invitation_id: inv_id.to_string(),
            vault_id: vid.to_string(),
            inviter_email: accepted.inviter_email,
            code: code_string(team_member_code(vid, &admin_key, &keys)),
        })
    }

    /// Decline an incoming invitation, or cancel / reject one of a team.
    pub async fn close_team_invitation(&self, invitation_id: String) -> AppResult<()> {
        let cx = self.team_ctx().await?;
        let inv_id = parse_invitation(&invitation_id)?;
        cx.api.close_team_invitation(inv_id).await?;
        // A pending join of our own for that invitation is void now.
        let pending = cx
            .unlocked
            .writer
            .working
            .objects()
            .into_iter()
            .find_map(|o| match o {
                VaultObject::TeamMembership(m) if m.invitation_id == Some(inv_id) => Some(m.id),
                _ => None,
            });
        if let Some(id) = pending {
            cx.unlocked.writer.delete(id).await?;
        }
        self.inner.ctx.emit(AppEvent::TeamsChanged);
        Ok(())
    }

    /// Accepted invitations this admin issued: compare each code with the
    /// invitee's screen (ТР-7).
    pub async fn team_join_requests(&self, vault_id: String) -> AppResult<Vec<TeamJoinRequestDto>> {
        let cx = self.team_ctx().await?;
        let vid = parse_vault(&vault_id)?;
        let team = cx.unlocked.writer.teams.require(vid)?;
        team.require_admin()?;
        let own_key = team.with_identity(|i| Ok(i.signing_key()))?;
        let list = cx.api.team_invitations(vid).await?;
        let mut shown = cx
            .unlocked
            .team_joins_shown
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        // Joins no longer waiting are forgotten. A join already shown keeps
        // the keys and role first shown: the code on screen never changes
        // under the admin, and a server that changes them later is refused
        // at confirmation (SEC-01).
        shown.retain(|id, s| {
            s.vault_id != vid || list.invitations.iter().any(|i| i.invitation_id == *id)
        });
        let mut requests = Vec::new();
        for i in &list.invitations {
            if i.status != TeamInvitationStatus::Accepted || i.inviter_user_id != cx.user_id {
                continue;
            }
            let Some(keys) = i
                .member_keys
                .as_ref()
                .filter(|k| k.user_id == i.invitee_user_id)
            else {
                continue;
            };
            let first = shown
                .entry(i.invitation_id)
                .or_insert_with(|| ShownJoin {
                    vault_id: vid,
                    keys: keys.clone(),
                    role: i.role,
                })
                .clone();
            requests.push(TeamJoinRequestDto {
                invitation_id: i.invitation_id.to_string(),
                vault_id: vid.to_string(),
                invitee_email: i.invitee_email.clone(),
                role: first.role.into(),
                code: code_string(team_member_code(vid, &own_key, &first.keys)),
            });
        }
        Ok(requests)
    }

    /// "Codes match": add the member to the log and seal the team key to
    /// the key the code was computed from (ТР-7). Codes that do not match
    /// → [`AppCore::close_team_invitation`] instead.
    pub async fn confirm_team_member(
        &self,
        vault_id: String,
        invitation_id: String,
    ) -> AppResult<TeamDto> {
        let cx = self.team_ctx().await?;
        let vid = parse_vault(&vault_id)?;
        let inv_id = parse_invitation(&invitation_id)?;
        let team = cx.unlocked.writer.teams.require(vid)?;
        let me = team.require_admin()?;
        // Seal only to the keys and role the code shown to this admin was
        // computed from — never to what the server answers now (SEC-01).
        let shown = cx
            .unlocked
            .team_joins_shown
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .get(&inv_id)
            .filter(|s| s.vault_id == vid)
            .cloned()
            .ok_or_else(|| {
                AppError::Approval("list the join requests and compare the codes first".into())
            })?;
        let inv = cx
            .api
            .team_invitations(vid)
            .await?
            .invitations
            .into_iter()
            .find(|i| i.invitation_id == inv_id)
            .ok_or_else(|| AppError::not_found("invitation", inv_id))?;
        if inv.status != TeamInvitationStatus::Accepted || inv.invitee_user_id != shown.keys.user_id
        {
            return Err(AppError::invalid("invitation_id", "not_accepted"));
        }
        if inv.member_keys.as_ref() != Some(&shown.keys) || inv.role != shown.role {
            forget_shown(&cx.unlocked, inv_id);
            return Err(AppError::Approval(
                "the member's keys or role changed after the code was shown; compare the codes again"
                    .into(),
            ));
        }
        let ShownJoin { keys, role, .. } = shown;
        if role == VaultRole::Owner || (role == VaultRole::Admin && me != VaultRole::Owner) {
            return Err(AppError::invalid("role", "not_allowed"));
        }
        // The new member opens only a key an admin vouched for (SEC-03b):
        // publish the current epoch's commitment first if it is missing.
        crate::teams::ensure_key_commitment(&cx.api, &team, cx.user_id).await?;
        let envelope = team.codec.vault()?.seal_for_member(&keys)?;
        submit(
            &cx,
            &team,
            TeamLogOp::AddMember { member: keys, role },
            Some(inv_id),
            vec![envelope],
        )
        .await?;
        forget_shown(&cx.unlocked, inv_id);
        self.team(vid).await
    }

    // ---- roles / leave / remove ------------------------------------------------

    pub async fn change_team_role(
        &self,
        vault_id: String,
        user_id: String,
        role: TeamRoleDto,
    ) -> AppResult<TeamDto> {
        let cx = self.team_ctx().await?;
        let vid = parse_vault(&vault_id)?;
        let team = cx.unlocked.writer.teams.require(vid)?;
        team.require_admin()?;
        let role = VaultRole::from(role);
        if role == VaultRole::Owner {
            return Err(AppError::invalid("role", "use_transfer_owner"));
        }
        submit(
            &cx,
            &team,
            TeamLogOp::ChangeRole {
                user_id: parse_user(&user_id)?,
                role,
            },
            None,
            Vec::new(),
        )
        .await?;
        self.team(vid).await
    }

    /// Hand the team over to another member (owner only; becomes admin).
    pub async fn transfer_team_owner(
        &self,
        vault_id: String,
        user_id: String,
    ) -> AppResult<TeamDto> {
        let cx = self.team_ctx().await?;
        let vid = parse_vault(&vault_id)?;
        let team = cx.unlocked.writer.teams.require(vid)?;
        if team.require_admin()? != VaultRole::Owner {
            return Err(AppError::TeamReadOnly);
        }
        submit(
            &cx,
            &team,
            TeamLogOp::TransferOwner {
                user_id: parse_user(&user_id)?,
            },
            None,
            Vec::new(),
        )
        .await?;
        self.team(vid).await
    }

    /// Remove a member, then rotate the key so they cannot read anything
    /// written afterwards (ТР-14, ТР-15).
    pub async fn remove_team_member(
        &self,
        vault_id: String,
        user_id: String,
    ) -> AppResult<TeamDto> {
        let cx = self.team_ctx().await?;
        let vid = parse_vault(&vault_id)?;
        let team = cx.unlocked.writer.teams.require(vid)?;
        team.require_admin()?;
        let user = parse_user(&user_id)?;
        if user == cx.user_id {
            return Err(AppError::invalid("user_id", "use_leave"));
        }
        submit(
            &cx,
            &team,
            TeamLogOp::RemoveMember { user_id: user },
            None,
            Vec::new(),
        )
        .await?;
        let team = cx.unlocked.writer.teams.require(vid)?;
        rotate(&cx, &team).await?;
        self.team(vid).await
    }

    /// Leave a team (not the owner: transfer ownership first).
    pub async fn leave_team(&self, vault_id: String) -> AppResult<()> {
        let cx = self.team_ctx().await?;
        let vid = parse_vault(&vault_id)?;
        let team = cx.unlocked.writer.teams.require(vid)?;
        if team.role() == Some(VaultRole::Owner) {
            return Err(AppError::invalid("role", "owner_cannot_leave"));
        }
        if team.state().warning.is_some() {
            return Err(AppError::TeamUntrusted(
                team.state()
                    .warning
                    .map(TeamWarning::code)
                    .unwrap_or_default()
                    .into(),
            ));
        }
        let (head, actor) = (head_of(&team), cx.user_id);
        let entry = team.with_identity(|i| {
            i.sign_entry(vid, actor, Some(head), now_secs(), TeamLogOp::Leave)
                .map_err(|e| AppError::Crypto(e.to_string()))
        })?;
        cx.api
            .submit_team_log(vid, &request(&team, entry, None, Vec::new())?)
            .await?;
        close_team(&cx.session, &cx.unlocked, vid, true).await;
        self.inner.ctx.emit(AppEvent::TeamsChanged);
        Ok(())
    }

    /// Rotate the team key now (admins; also offered when
    /// `rotation_required`).
    pub async fn rotate_team_key(&self, vault_id: String) -> AppResult<TeamDto> {
        let cx = self.team_ctx().await?;
        let vid = parse_vault(&vault_id)?;
        let team = cx.unlocked.writer.teams.require(vid)?;
        team.require_admin()?;
        rotate(&cx, &team).await?;
        self.team(vid).await
    }

    // ---- copy / move -----------------------------------------------------------

    /// Create a group directly in a team. Everything created inside a team
    /// group (hosts, subgroups, …) lands in that team.
    pub async fn create_team_group(
        &self,
        vault_id: String,
        group: crate::dto::GroupDto,
    ) -> AppResult<crate::dto::GroupDto> {
        let (_, u) = self.unlocked().await?;
        let vid = parse_vault(&vault_id)?;
        if !group.id.trim().is_empty() {
            return Err(AppError::invalid("id", "must_be_empty"));
        }
        let id = ObjectId::new();
        let model = group.to_model(id, None)?;
        u.writer
            .put_in(Some(vid), VaultObject::Group(model))
            .await?;
        u.working()
            .group(id)
            .map(|g| crate::dto::GroupDto::from_model(&g))
            .ok_or_else(|| AppError::internal("object vanished after save"))
    }

    /// Copy objects (with everything they reference) into a team, or into
    /// the personal vault (`target_vault_id = None`). Returns the new ids
    /// of `ids` in order.
    pub async fn copy_to_vault(
        &self,
        ids: Vec<String>,
        target_vault_id: Option<String>,
    ) -> AppResult<Vec<String>> {
        let (_, u) = self.unlocked().await?;
        let target = target_vault_id.as_deref().map(parse_vault).transpose()?;
        let ids = ids
            .iter()
            .map(|i| crate::dto::parse_id("id", i))
            .collect::<AppResult<Vec<ObjectId>>>()?;
        copy_objects(&u, &ids, target).await
    }

    /// Move objects: copy them (and their dependencies) into the target
    /// vault, send the copies, then delete the originals (ТР-23).
    pub async fn move_to_vault(
        &self,
        ids: Vec<String>,
        target_vault_id: Option<String>,
    ) -> AppResult<Vec<String>> {
        let (s, u) = self.unlocked().await?;
        let target = target_vault_id.as_deref().map(parse_vault).transpose()?;
        let ids = ids
            .iter()
            .map(|i| crate::dto::parse_id("id", i))
            .collect::<AppResult<Vec<ObjectId>>>()?;
        let new_ids = copy_objects(&u, &ids, target).await?;
        let engine = match target {
            Some(v) => u.writer.teams.require(v)?.engine(),
            None => u.engine(),
        };
        if let Some(e) = engine {
            e.sync_now().await?;
        } else if s.kind().await? == ProfileKind::Synced {
            return Err(AppError::Offline("the copies were not sent yet".into()));
        }
        for id in ids {
            u.writer.delete(id).await?;
        }
        Ok(new_ids)
    }
}

fn role_rank(r: TeamRoleDto) -> u8 {
    match r {
        TeamRoleDto::Owner => 0,
        TeamRoleDto::Admin => 1,
        TeamRoleDto::Editor => 2,
        TeamRoleDto::Viewer => 3,
    }
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

fn next_sort_order(u: &Unlocked) -> i32 {
    u.writer
        .working
        .objects()
        .iter()
        .filter_map(|o| match o {
            VaultObject::TeamMembership(m) => Some(m.sort_order),
            _ => None,
        })
        .max()
        .map_or(0, |m| m.saturating_add(1))
}

fn team_dto(u: &Unlocked, m: &TeamMembership) -> TeamDto {
    let team = u.writer.teams.get(m.team_vault_id);
    let st = team.as_ref().map(|t| t.state());
    let settings = st.as_ref().and_then(|s| s.settings.clone());
    let team_color = settings.as_ref().and_then(|s| s.color.clone());
    TeamDto {
        vault_id: m.team_vault_id.to_string(),
        name: settings
            .as_ref()
            .map(|s| s.vault_name.clone())
            .unwrap_or_default(),
        color: m.local_color.clone().or_else(|| team_color.clone()),
        team_color,
        local_color: m.local_color.clone(),
        icon: settings.as_ref().and_then(|s| s.icon.clone()),
        description: settings.as_ref().and_then(|s| s.description.clone()),
        policies: settings.map(|s| s.policies).unwrap_or_default(),
        role: team.as_ref().and_then(|t| t.role()).map(Into::into),
        members_count: st
            .as_ref()
            .map(|s| u32::try_from(s.view.members.len()).unwrap_or(u32::MAX))
            .unwrap_or(0),
        key_epoch: st.as_ref().map(|s| s.key_epoch).unwrap_or(0),
        rotation_required: st.as_ref().is_some_and(|s| s.rotation_required),
        warning: st
            .as_ref()
            .and_then(|s| s.warning)
            .map(|w| w.code().to_owned()),
        offline: st.as_ref().is_some_and(|s| s.offline),
        pending: m.invitation_id.is_some(),
        open: team.is_some(),
        sort_order: m.sort_order,
        hidden: m.hidden,
        object_count: u32::try_from(u.writer.working.ids_in(m.team_vault_id).len())
            .unwrap_or(u32::MAX),
    }
}

fn head_of(team: &TeamHandle) -> (u64, [u8; 32]) {
    let st = team.state();
    (st.view.seq, st.view.head)
}

fn request(
    team: &TeamHandle,
    entry: SignedTeamLogEntry,
    invitation_id: Option<TeamInvitationId>,
    envelopes: Vec<cc_protocol::envelopes::NewEnvelope>,
) -> AppResult<SubmitTeamLogRequest> {
    Ok(SubmitTeamLogRequest {
        entry,
        vault_access_key: team.codec.vault()?.vault_access_key(),
        invitation_id,
        envelopes,
        new_vault_access_key: None,
        chain_link: None,
        key_commitment: None,
    })
}

/// Sign `op` after the verified head, check it against the verified log
/// locally, submit it and re-read the team.
async fn submit(
    cx: &TeamCtx,
    team: &Arc<TeamHandle>,
    op: TeamLogOp,
    invitation_id: Option<TeamInvitationId>,
    envelopes: Vec<cc_protocol::envelopes::NewEnvelope>,
) -> AppResult<()> {
    let vid = team.vault_id;
    let head = head_of(team);
    let entry = team.with_identity(|i| {
        i.sign_entry(vid, cx.user_id, Some(head), now_secs(), op)
            .map_err(|e| AppError::Crypto(e.to_string()))
    })?;
    // Same rules the other members will apply: never submit what they
    // would reject.
    let mut check = team.state().view;
    check
        .apply(&entry)
        .map_err(|_| AppError::invalid("operation", "not_allowed"))?;
    cx.api
        .submit_team_log(vid, &request(team, entry, invitation_id, envelopes)?)
        .await?;
    cx.session.refresh_team(&cx.unlocked, vid).await
}

/// Key rotation (ТР-15): a new VRK for the remaining verified members, the
/// previous key chained under it, then every live object re-encrypted.
async fn rotate(cx: &TeamCtx, team: &Arc<TeamHandle>) -> AppResult<()> {
    let vid = team.vault_id;
    // A `team_key_rotated` event must not re-open the team between the
    // server accepting the new epoch and this codec switching to it.
    let gate = cx.unlocked.writer.teams.gate.lock().await;
    let st = team.state();
    let epoch = st.view.key_epoch + 1;
    let current = team.codec.vault()?;
    let (next, link) = current.rotate_team(epoch)?;
    let members: Vec<TeamMemberKeys> = st.view.members.values().map(|m| m.keys.clone()).collect();
    let envelopes = members
        .iter()
        .map(|k| next.seal_for_member(k))
        .collect::<Result<Vec<_>, _>>()?;
    let head = (st.view.seq, st.view.head);
    let entry = team.with_identity(|i| {
        i.sign_entry(
            vid,
            cx.user_id,
            Some(head),
            now_secs(),
            TeamLogOp::RotateKey { key_epoch: epoch },
        )
        .map_err(|e| AppError::Crypto(e.to_string()))
    })?;
    let mut check = st.view.clone();
    check
        .apply(&entry)
        .map_err(|_| AppError::invalid("operation", "not_allowed"))?;
    let key_commitment = team.with_identity(|i| {
        Ok(i.sign_key_commitment(vid, cx.user_id, epoch, next.key_commitment()))
    })?;
    let req = SubmitTeamLogRequest {
        entry,
        vault_access_key: current.vault_access_key(),
        invitation_id: None,
        envelopes,
        new_vault_access_key: Some(next.vault_access_key()),
        chain_link: Some(link),
        key_commitment: Some(key_commitment),
    };
    cx.api.submit_team_log(vid, &req).await?;
    drop(current);
    team.codec.rotate(next)?;
    team.update(|s| s.key_epoch = epoch);
    drop(gate);
    // Re-encrypt every live object under the new epoch.
    let (objects, _) = team.store.list().await?;
    let payloads: Vec<_> = objects.into_iter().map(|d| d.payload).collect();
    for chunk in payloads.chunks(100) {
        match team.engine() {
            Some(e) => {
                e.put_batch(chunk.to_vec()).await?;
            }
            None => {
                team.store.put_batch(chunk.to_vec()).await?;
            }
        }
    }
    if let Some(e) = team.engine() {
        if let Err(err) = e.sync_now().await {
            tracing::warn!(team = %vid, error = %err, "re-encrypted objects not sent yet");
        }
    }
    cx.session.refresh_team(&cx.unlocked, vid).await
}

/// Copy `ids` and the objects they reference into `target`, with fresh
/// ids and references rewritten. Objects already in `target` are reused.
async fn copy_objects(
    u: &Unlocked,
    ids: &[ObjectId],
    target: Option<VaultId>,
) -> AppResult<Vec<String>> {
    let ws = &u.writer.working;
    if let Some(t) = target {
        u.writer.teams.require(t)?.require_writable()?;
    }
    // Closure over references that are not in the target yet.
    let mut order: Vec<ObjectId> = Vec::new();
    let mut seen: BTreeSet<ObjectId> = BTreeSet::new();
    let mut stack: Vec<ObjectId> = ids.to_vec();
    while let Some(id) = stack.pop() {
        if !seen.insert(id) {
            continue;
        }
        let kind = ws
            .kind_of(id)
            .ok_or_else(|| AppError::not_found("object", id))?;
        if !crate::teams::team_capable(kind) {
            if ids.contains(&id) {
                return Err(AppError::invalid("vault", "personal_only"));
            }
            continue;
        }
        if ws.vault_of(id) == target && !ids.contains(&id) {
            continue;
        }
        order.push(id);
        let object = read_object(u, id).await?;
        for r in crate::teams::references(ws, &object) {
            if ws.kind_of(r).is_some_and(crate::teams::team_capable) && ws.vault_of(r) != target {
                stack.push(r);
            }
        }
    }
    let remap: BTreeMap<ObjectId, ObjectId> =
        order.iter().map(|id| (*id, ObjectId::new())).collect();
    let mut copies = Vec::with_capacity(order.len());
    for id in &order {
        let object = read_object(u, *id).await?;
        copies.push(rewrite_ids(&object, &remap)?);
    }
    u.writer.put_batch_in(target, copies).await?;
    Ok(ids
        .iter()
        .map(|id| remap.get(id).copied().unwrap_or(*id).to_string())
        .collect())
}

async fn read_object(u: &Unlocked, id: ObjectId) -> AppResult<VaultObject> {
    match u.writer.readback(id).await? {
        crate::working_set::Readback::Live(d) => Ok(d.payload.object),
        _ => Err(AppError::not_found("object", id)),
    }
}

/// The object with every id in `remap` replaced (its own id included).
fn rewrite_ids(
    object: &VaultObject,
    remap: &BTreeMap<ObjectId, ObjectId>,
) -> AppResult<VaultObject> {
    fn walk(v: &mut serde_json::Value, remap: &BTreeMap<String, String>) {
        match v {
            serde_json::Value::String(s) => {
                if let Some(n) = remap.get(s.as_str()) {
                    *s = n.clone();
                }
            }
            serde_json::Value::Array(a) => a.iter_mut().for_each(|x| walk(x, remap)),
            serde_json::Value::Object(m) => m.values_mut().for_each(|x| walk(x, remap)),
            _ => {}
        }
    }
    let strings: BTreeMap<String, String> = remap
        .iter()
        .map(|(a, b)| (a.to_string(), b.to_string()))
        .collect();
    let mut v = serde_json::to_value(object).map_err(AppError::internal)?;
    walk(&mut v, &strings);
    serde_json::from_value(v).map_err(AppError::internal)
}

/// A join request as shown to the admin: the member code covers exactly
/// these keys and this role (security review SEC-01).
#[derive(Debug, Clone)]
pub(crate) struct ShownJoin {
    vault_id: VaultId,
    keys: TeamMemberKeys,
    role: VaultRole,
}

fn forget_shown(unlocked: &Unlocked, invitation_id: TeamInvitationId) {
    unlocked
        .team_joins_shown
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .remove(&invitation_id);
}
