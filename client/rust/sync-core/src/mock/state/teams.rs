//! Teams in the mock server (protocol 1.9): the same rules as the real
//! server's `teams` module, in memory.

use super::{
    bad, err, not_found, sha256, validate_envelope, Audience, MResult, Principal, State, Vault,
};
use cc_protocol::envelopes::{NewEnvelope, RecipientType};
use cc_protocol::events::ServerEvent;
use cc_protocol::limits::VAULT_ACCESS_KEY_LEN;
use cc_protocol::teams::{
    team_log_hash_input, team_log_message, AcceptTeamInvitationRequest, CreateTeamRequest,
    InviteTeamMemberRequest, ListTeamInvitationsResponse, ListTeamsResponse, SignedTeamLogEntry,
    SubmitTeamLogRequest, SubmitTeamLogResponse, TeamDetail, TeamInfo, TeamInvitation,
    TeamInvitationStatus, TeamKeyChainLink, TeamKeyMaterial, TeamLogOp, TeamLogPage, TeamMember,
    TeamMemberKeys,
};
use cc_protocol::vaults::{VaultRole, VaultState};
use cc_protocol::{Bytes, ErrorCode, TeamInvitationId, Timestamp, UserId, VaultId};
use chrono::Utc;
use ed25519_dalek::{Signature, VerifyingKey};
use std::collections::{BTreeMap, HashMap};

#[derive(Debug, Clone)]
pub(super) struct MemberRec {
    pub(super) role: VaultRole,
    pub(super) keys: TeamMemberKeys,
    pub(super) joined_at: Timestamp,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct TeamState {
    pub(super) members: BTreeMap<UserId, MemberRec>,
    pub(super) log: Vec<SignedTeamLogEntry>,
    pub(super) head: [u8; 32],
    pub(super) key_epoch: u32,
    pub(super) rotation_required: bool,
    pub(super) chain: Vec<TeamKeyChainLink>,
    /// Protocol 1.12: published key commitments per epoch (stored as sent;
    /// clients must verify them anyway).
    pub(super) key_commitments: BTreeMap<u32, cc_protocol::teams::TeamKeyCommitment>,
}

#[derive(Debug, Clone)]
pub(crate) struct TeamInvitationRec {
    id: TeamInvitationId,
    vault_id: VaultId,
    inviter: UserId,
    invitee: UserId,
    role: VaultRole,
    note: Option<String>,
    status: TeamInvitationStatus,
    keys: Option<TeamMemberKeys>,
    created_at: Timestamp,
    expires_at: Timestamp,
}

fn entry_hash(e: &SignedTeamLogEntry) -> [u8; 32] {
    sha256(&team_log_hash_input(e))
}

fn signature_ok(key: &[u8], e: &SignedTeamLogEntry) -> bool {
    let Ok(key) = <[u8; 32]>::try_from(key) else {
        return false;
    };
    let (Ok(vk), Ok(sig)) = (
        VerifyingKey::from_bytes(&key),
        Signature::from_slice(e.signature.as_slice()),
    ) else {
        return false;
    };
    vk.verify_strict(&team_log_message(&e.entry), &sig).is_ok()
}

fn forbidden(message: &str) -> super::MockError {
    err(ErrorCode::Forbidden, message).with_details(serde_json::json!({"reason": "team_role"}))
}

fn conflict(message: &str) -> super::MockError {
    err(ErrorCode::Conflict, message)
}

fn admin(role: VaultRole) -> bool {
    matches!(role, VaultRole::Owner | VaultRole::Admin)
}

impl State {
    fn team_vault(&self, p: &Principal, vault_id: VaultId) -> MResult<(&Vault, &TeamState)> {
        let v = self.member_vault(p, vault_id)?;
        let t = v.team.as_ref().ok_or_else(not_found)?;
        Ok((v, t))
    }

    fn team_info(&self, v: &Vault, t: &TeamState, p: &Principal) -> TeamInfo {
        TeamInfo {
            vault_id: v.vault_id,
            role: t
                .members
                .get(&p.user_id)
                .map_or(VaultRole::Viewer, |m| m.role),
            key_epoch: t.key_epoch,
            log_seq: t.log.last().map_or(0, |e| e.entry.seq),
            log_head: Bytes::new(t.head.to_vec()),
            members_count: u32::try_from(t.members.len()).unwrap_or(u32::MAX),
            rotation_required: t.rotation_required,
            caller_trusted: self.is_trusted(p.device_id, v),
            latest_sequence: v.last_sequence,
            epoch: v.epoch,
            created_at: v.created_at,
            updated_at: v.updated_at,
        }
    }

    fn team_members(&self, t: &TeamState) -> Vec<TeamMember> {
        t.members
            .iter()
            .map(|(user, m)| TeamMember {
                user_id: *user,
                email: self
                    .users
                    .get(user)
                    .map(|u| u.email.clone())
                    .unwrap_or_default(),
                role: m.role,
                encryption_key: m.keys.encryption_key.clone(),
                signing_key: m.keys.signing_key.clone(),
                joined_at: m.joined_at,
            })
            .collect()
    }

    pub(crate) fn create_team(
        &mut self,
        p: Principal,
        req: CreateTeamRequest,
    ) -> MResult<TeamInfo> {
        if self.vaults.contains_key(&req.vault_id) {
            return Err(err(ErrorCode::AlreadyExists, "vault id already exists"));
        }
        if req.vault_access_key.len() != VAULT_ACCESS_KEY_LEN {
            return Err(bad("vault access key must be 32 bytes"));
        }
        req.genesis.validate().map_err(|_| bad("invalid genesis"))?;
        let TeamLogOp::Genesis { owner } = &req.genesis.entry.op else {
            return Err(bad("the first entry must be genesis"));
        };
        if req.genesis.entry.vault_id != req.vault_id
            || req.genesis.entry.actor_user_id != p.user_id
            || !signature_ok(req.genesis.entry.actor_signing_key.as_slice(), &req.genesis)
        {
            return Err(err(ErrorCode::Forbidden, "genesis rejected"));
        }
        validate_envelope(&req.owner_envelope, RecipientType::User, Some(p.user_id.0))?;
        validate_envelope(
            &req.device_envelope,
            RecipientType::Device,
            Some(p.device_id.0),
        )?;
        let now = Utc::now();
        let mut members = BTreeMap::new();
        members.insert(
            p.user_id,
            MemberRec {
                role: VaultRole::Owner,
                keys: owner.clone(),
                joined_at: now,
            },
        );
        self.vaults.insert(
            req.vault_id,
            Vault {
                vault_id: req.vault_id,
                owner: p.user_id,
                vak_hash: sha256(req.vault_access_key.as_slice()),
                state: VaultState::Active,
                created_at: now,
                updated_at: now,
                last_sequence: 0,
                epoch: uuid::Uuid::new_v4(),
                objects: HashMap::new(),
                mutations: HashMap::new(),
                envelopes: Vec::new(),
                team: Some(TeamState {
                    members,
                    head: entry_hash(&req.genesis),
                    log: vec![req.genesis],
                    key_commitments: req
                        .key_commitment
                        .clone()
                        .filter(|_| self.cfg.team_key_commitment)
                        .map(|kc| (0, kc))
                        .into_iter()
                        .collect(),
                    ..TeamState::default()
                }),
            },
        );
        self.store_envelope(req.vault_id, req.owner_envelope, p.device_id);
        self.store_envelope(req.vault_id, req.device_envelope, p.device_id);
        let (v, t) = self.team_vault(&p, req.vault_id)?;
        Ok(self.team_info(v, t, &p))
    }

    pub(crate) fn list_teams(&self, p: Principal) -> ListTeamsResponse {
        let mut teams: Vec<TeamInfo> = self
            .vaults
            .values()
            .filter(|v| v.state != VaultState::Deleted)
            .filter_map(|v| {
                let t = v.team.as_ref()?;
                t.members
                    .contains_key(&p.user_id)
                    .then(|| self.team_info(v, t, &p))
            })
            .collect();
        teams.sort_by_key(|t| t.vault_id);
        ListTeamsResponse { teams }
    }

    pub(crate) fn get_team(&self, p: Principal, vault_id: VaultId) -> MResult<TeamDetail> {
        let (v, t) = self.team_vault(&p, vault_id)?;
        Ok(TeamDetail {
            team: self.team_info(v, t, &p),
            members: self.team_members(t),
        })
    }

    pub(crate) fn team_log(
        &self,
        p: Principal,
        vault_id: VaultId,
        after: Option<u64>,
    ) -> MResult<TeamLogPage> {
        let (_, t) = self.team_vault(&p, vault_id)?;
        let entries: Vec<SignedTeamLogEntry> = t
            .log
            .iter()
            .filter(|e| after.is_none_or(|a| e.entry.seq > a))
            .cloned()
            .collect();
        Ok(TeamLogPage {
            head_seq: t.log.last().map_or(0, |e| e.entry.seq),
            entries,
            has_more: false,
        })
    }

    pub(crate) fn team_key(&self, p: Principal, vault_id: VaultId) -> MResult<TeamKeyMaterial> {
        let (v, t) = self.team_vault(&p, vault_id)?;
        let envelope = v
            .envelopes
            .iter()
            .find(|e| {
                e.recipient_type == RecipientType::User
                    && e.recipient_id == Some(p.user_id.0)
                    && e.revoked_at.is_none()
            })
            .cloned()
            .ok_or_else(not_found)?;
        let mut chain = t.chain.clone();
        chain.sort_by_key(|l| std::cmp::Reverse(l.key_epoch));
        Ok(TeamKeyMaterial {
            key_epoch: t.key_epoch,
            envelope,
            chain,
            key_commitment: t.key_commitments.get(&t.key_epoch).cloned(),
        })
    }

    fn invitation_dto(&self, r: &TeamInvitationRec) -> TeamInvitation {
        let email = |u: &UserId| {
            self.users
                .get(u)
                .map(|x| x.email.clone())
                .unwrap_or_default()
        };
        let inviter_key = self
            .vaults
            .get(&r.vault_id)
            .and_then(|v| v.team.as_ref())
            .and_then(|t| t.members.get(&r.inviter))
            .map(|m| m.keys.signing_key.clone())
            .unwrap_or_default();
        let open = matches!(
            r.status,
            TeamInvitationStatus::Pending | TeamInvitationStatus::Accepted
        );
        TeamInvitation {
            invitation_id: r.id,
            vault_id: r.vault_id,
            inviter_user_id: r.inviter,
            inviter_email: email(&r.inviter),
            inviter_signing_key: inviter_key,
            invitee_user_id: r.invitee,
            invitee_email: email(&r.invitee),
            role: r.role,
            note: r.note.clone(),
            status: if open && r.expires_at <= Utc::now() {
                TeamInvitationStatus::Expired
            } else {
                r.status
            },
            member_keys: r.keys.clone(),
            created_at: r.created_at,
            expires_at: r.expires_at,
        }
    }

    pub(crate) fn invite(
        &mut self,
        p: Principal,
        vault_id: VaultId,
        req: InviteTeamMemberRequest,
    ) -> MResult<()> {
        let (_, t) = self.team_vault(&p, vault_id)?;
        let me = t
            .members
            .get(&p.user_id)
            .map(|m| m.role)
            .ok_or_else(not_found)?;
        if !admin(me) {
            return Err(forbidden("team admins only"));
        }
        if req.role == VaultRole::Owner || (req.role == VaultRole::Admin && me != VaultRole::Owner)
        {
            return Err(forbidden("role not allowed"));
        }
        let invitee = self.emails.get(&req.email.trim().to_lowercase()).copied();
        let Some(invitee) = invitee else {
            return Ok(());
        };
        if t.members.contains_key(&invitee)
            || self.team_invitations.values().any(|r| {
                r.vault_id == vault_id
                    && r.invitee == invitee
                    && matches!(
                        r.status,
                        TeamInvitationStatus::Pending | TeamInvitationStatus::Accepted
                    )
            })
        {
            return Ok(());
        }
        let now = Utc::now();
        let id = TeamInvitationId::new();
        self.team_invitations.insert(
            id,
            TeamInvitationRec {
                id,
                vault_id,
                inviter: p.user_id,
                invitee,
                role: req.role,
                note: req
                    .note
                    .map(|n| n.trim().to_owned())
                    .filter(|n| !n.is_empty()),
                status: TeamInvitationStatus::Pending,
                keys: None,
                created_at: now,
                expires_at: now + chrono::Duration::days(7),
            },
        );
        self.emit_to(
            Audience::User(invitee),
            ServerEvent::TeamInvitationReceived { invitation_id: id },
        );
        Ok(())
    }

    pub(crate) fn incoming_invitations(&self, p: Principal) -> ListTeamInvitationsResponse {
        let mut invitations: Vec<TeamInvitation> = self
            .team_invitations
            .values()
            .filter(|r| r.invitee == p.user_id)
            .map(|r| self.invitation_dto(r))
            .filter(|d| {
                matches!(
                    d.status,
                    TeamInvitationStatus::Pending | TeamInvitationStatus::Accepted
                )
            })
            .collect();
        invitations.sort_by_key(|i| i.created_at);
        ListTeamInvitationsResponse { invitations }
    }

    pub(crate) fn team_invitations(
        &self,
        p: Principal,
        vault_id: VaultId,
    ) -> MResult<ListTeamInvitationsResponse> {
        let (_, t) = self.team_vault(&p, vault_id)?;
        if !t.members.get(&p.user_id).is_some_and(|m| admin(m.role)) {
            return Err(forbidden("team admins only"));
        }
        let mut invitations: Vec<TeamInvitation> = self
            .team_invitations
            .values()
            .filter(|r| r.vault_id == vault_id)
            .map(|r| self.invitation_dto(r))
            .filter(|d| {
                matches!(
                    d.status,
                    TeamInvitationStatus::Pending | TeamInvitationStatus::Accepted
                )
            })
            .collect();
        invitations.sort_by_key(|i| i.created_at);
        Ok(ListTeamInvitationsResponse { invitations })
    }

    pub(crate) fn accept_invitation(
        &mut self,
        p: Principal,
        id: TeamInvitationId,
        req: AcceptTeamInvitationRequest,
    ) -> MResult<TeamInvitation> {
        let keys = TeamMemberKeys {
            user_id: p.user_id,
            encryption_key: req.encryption_key,
            signing_key: req.signing_key,
        };
        keys.validate().map_err(|_| bad("invalid member keys"))?;
        let r = self.team_invitations.get_mut(&id).ok_or_else(not_found)?;
        if r.invitee != p.user_id {
            return Err(not_found());
        }
        if r.status != TeamInvitationStatus::Pending || r.expires_at <= Utc::now() {
            return Err(conflict("the invitation is no longer pending"));
        }
        r.status = TeamInvitationStatus::Accepted;
        r.keys = Some(keys);
        let (inviter, vault_id) = (r.inviter, r.vault_id);
        let dto = self.invitation_dto(&self.team_invitations[&id]);
        self.emit_to(
            Audience::User(inviter),
            ServerEvent::TeamJoinRequested {
                vault_id,
                invitation_id: id,
            },
        );
        Ok(dto)
    }

    pub(crate) fn close_invitation(&mut self, p: Principal, id: TeamInvitationId) -> MResult<()> {
        let r = self
            .team_invitations
            .get(&id)
            .ok_or_else(not_found)?
            .clone();
        let status = if r.invitee == p.user_id {
            TeamInvitationStatus::Declined
        } else {
            let (_, t) = self.team_vault(&p, r.vault_id)?;
            if !t.members.get(&p.user_id).is_some_and(|m| admin(m.role)) {
                return Err(forbidden("team admins only"));
            }
            TeamInvitationStatus::Cancelled
        };
        if let Some(r) = self.team_invitations.get_mut(&id) {
            if matches!(
                r.status,
                TeamInvitationStatus::Pending | TeamInvitationStatus::Accepted
            ) {
                r.status = status;
            }
        }
        Ok(())
    }

    /// Check an append against the team state (same rules as the server).
    fn check_append(t: &TeamState, actor: UserId, e: &SignedTeamLogEntry) -> MResult<()> {
        e.validate().map_err(|_| bad("invalid log entry"))?;
        let last = t.log.last().map_or(0, |l| l.entry.seq);
        if e.entry.seq != last + 1 || e.entry.previous_hash.as_slice() != t.head {
            return Err(conflict("the team log changed"));
        }
        if e.entry.actor_user_id != actor {
            return Err(forbidden("entries are signed by the caller"));
        }
        let me = t
            .members
            .get(&actor)
            .ok_or_else(|| forbidden("not a member"))?;
        if me.keys.signing_key != e.entry.actor_signing_key
            || !signature_ok(me.keys.signing_key.as_slice(), e)
        {
            return Err(forbidden("log entry signature is invalid"));
        }
        let role = |u: &UserId| t.members.get(u).map(|m| m.role);
        let ok = match &e.entry.op {
            TeamLogOp::Genesis { .. } => false,
            TeamLogOp::AddMember { member, .. } => {
                admin(me.role) && role(&member.user_id).is_none()
            }
            TeamLogOp::ChangeRole { user_id, .. } | TeamLogOp::RemoveMember { user_id } => {
                admin(me.role)
                    && matches!(role(user_id), Some(r) if r != VaultRole::Owner
                        && (r != VaultRole::Admin || me.role == VaultRole::Owner))
            }
            TeamLogOp::Leave => me.role != VaultRole::Owner,
            TeamLogOp::TransferOwner { user_id } => {
                me.role == VaultRole::Owner && *user_id != actor && role(user_id).is_some()
            }
            TeamLogOp::RotateKey { key_epoch } => admin(me.role) && *key_epoch == t.key_epoch + 1,
        };
        if !ok {
            return Err(forbidden("log entry not allowed"));
        }
        Ok(())
    }

    fn revoke_team_member(&mut self, vault_id: VaultId, user: UserId) {
        let devices: Vec<uuid::Uuid> = self
            .devices
            .values()
            .filter(|d| d.user_id == user)
            .map(|d| d.reg.device_id.0)
            .collect();
        let now = Utc::now();
        if let Some(v) = self.vaults.get_mut(&vault_id) {
            for e in v.envelopes.iter_mut().filter(|e| e.revoked_at.is_none()) {
                let mine = match e.recipient_type {
                    RecipientType::User => e.recipient_id == Some(user.0),
                    RecipientType::Device => e.recipient_id.is_some_and(|d| devices.contains(&d)),
                    _ => false,
                };
                if mine {
                    e.revoked_at = Some(now);
                }
            }
            if let Some(t) = v.team.as_mut() {
                t.members.remove(&user);
                t.rotation_required = true;
            }
        }
    }

    pub(crate) fn submit_team_log(
        &mut self,
        p: Principal,
        vault_id: VaultId,
        req: SubmitTeamLogRequest,
    ) -> MResult<SubmitTeamLogResponse> {
        let (v, t) = self.team_vault(&p, vault_id)?;
        Self::check_vak(v, &req.vault_access_key)?;
        Self::check_append(t, p.user_id, &req.entry)?;
        let members_now: Vec<UserId> = t.members.keys().copied().collect();
        let key_epoch = t.key_epoch;
        let mut removed = None;
        let mut rotated = None;
        let mut rotated_commitment = None;
        let mut new_member: Option<(UserId, MemberRec)> = None;
        let mut user_envelopes: Vec<NewEnvelope> = Vec::new();
        match &req.entry.entry.op {
            TeamLogOp::AddMember { member, role } => {
                let inv_id = req
                    .invitation_id
                    .ok_or_else(|| bad("invitation_id is required"))?;
                let inv = self.team_invitations.get(&inv_id).ok_or_else(not_found)?;
                if inv.vault_id != vault_id
                    || inv.status != TeamInvitationStatus::Accepted
                    || inv.invitee != member.user_id
                    || inv.inviter != p.user_id
                    || inv.keys.as_ref() != Some(member)
                    || inv.role != *role
                {
                    return Err(conflict("the entry does not match the accepted invitation"));
                }
                let [env] = req.envelopes.as_slice() else {
                    return Err(bad("exactly one user envelope is required"));
                };
                validate_envelope(env, RecipientType::User, Some(member.user_id.0))?;
                user_envelopes.push(env.clone());
                new_member = Some((
                    member.user_id,
                    MemberRec {
                        role: *role,
                        keys: member.clone(),
                        joined_at: Utc::now(),
                    },
                ));
                if let Some(r) = self.team_invitations.get_mut(&inv_id) {
                    r.status = TeamInvitationStatus::Completed;
                }
            }
            TeamLogOp::RotateKey { key_epoch: next } => {
                let vak = req
                    .new_vault_access_key
                    .as_ref()
                    .ok_or_else(|| bad("new_vault_access_key is required"))?;
                if vak.len() != VAULT_ACCESS_KEY_LEN {
                    return Err(bad("vault access key must be 32 bytes"));
                }
                let link = req
                    .chain_link
                    .clone()
                    .ok_or_else(|| bad("chain_link is required"))?;
                if link.key_epoch != *next || link.validate().is_err() {
                    return Err(bad("invalid chain link"));
                }
                rotated_commitment = req
                    .key_commitment
                    .clone()
                    .filter(|kc| kc.key_epoch == *next && self.cfg.team_key_commitment);
                let mut got: Vec<UserId> = Vec::new();
                for env in &req.envelopes {
                    let user = env
                        .recipient_id
                        .map(UserId::from)
                        .ok_or_else(|| bad("recipient"))?;
                    validate_envelope(env, RecipientType::User, Some(user.0))?;
                    got.push(user);
                }
                let mut expected = members_now.clone();
                expected.sort();
                got.sort();
                if expected != got {
                    return Err(conflict("rotation must cover exactly the current members"));
                }
                user_envelopes = req.envelopes.clone();
                rotated = Some((*next, sha256(vak.as_slice()), link));
            }
            TeamLogOp::RemoveMember { user_id } => removed = Some(*user_id),
            TeamLogOp::Leave => removed = Some(p.user_id),
            _ => {}
        }
        let head = entry_hash(&req.entry);
        let seq = req.entry.entry.seq;
        let v = self.vaults.get_mut(&vault_id).ok_or_else(not_found)?;
        let t = v.team.as_mut().ok_or_else(not_found)?;
        match &req.entry.entry.op {
            TeamLogOp::ChangeRole { user_id, role } => {
                if let Some(m) = t.members.get_mut(user_id) {
                    m.role = *role;
                }
            }
            TeamLogOp::TransferOwner { user_id } => {
                if let Some(m) = t.members.get_mut(&p.user_id) {
                    m.role = VaultRole::Admin;
                }
                if let Some(m) = t.members.get_mut(user_id) {
                    m.role = VaultRole::Owner;
                }
                v.owner = *user_id;
            }
            _ => {}
        }
        let t = v.team.as_mut().ok_or_else(not_found)?;
        if let Some((user, rec)) = new_member {
            t.members.insert(user, rec);
        }
        if let Some((epoch, vak_hash, link)) = rotated.clone() {
            t.key_epoch = epoch;
            if let Some(kc) = rotated_commitment.take() {
                t.key_commitments.insert(epoch, kc);
            }
            t.rotation_required = false;
            t.chain.push(link);
            v.vak_hash = vak_hash;
        }
        let t = v.team.as_mut().ok_or_else(not_found)?;
        t.log.push(req.entry.clone());
        t.head = head;
        v.updated_at = Utc::now();
        let _ = key_epoch;
        for env in user_envelopes {
            self.store_envelope(vault_id, env, p.device_id);
        }
        if let Some(user) = removed {
            self.revoke_team_member(vault_id, user);
        }
        self.emit_to(
            Audience::Vault(vault_id),
            ServerEvent::TeamMembershipChanged {
                vault_id,
                log_seq: seq,
            },
        );
        if let Some(user) = removed {
            self.emit_to(Audience::User(user), ServerEvent::TeamRemoved { vault_id });
        }
        if let Some((epoch, _, _)) = rotated {
            self.emit_to(
                Audience::Vault(vault_id),
                ServerEvent::TeamKeyRotated {
                    vault_id,
                    key_epoch: epoch,
                },
            );
        }
        let v = self.vaults.get(&vault_id).ok_or_else(not_found)?;
        let t = v.team.as_ref().ok_or_else(not_found)?;
        let team = t
            .members
            .contains_key(&p.user_id)
            .then(|| self.team_info(v, t, &p));
        let members = if team.is_some() {
            self.team_members(t)
        } else {
            Vec::new()
        };
        Ok(SubmitTeamLogResponse {
            log_seq: seq,
            team,
            members,
        })
    }

    /// Test hook: a hostile server replaces the team's log with `log` (a
    /// genesis of its own and an `AddMember` of `keys`) and hands that user
    /// an envelope of a key it chose.
    pub(crate) fn forge_team(
        &mut self,
        vault_id: VaultId,
        log: Vec<SignedTeamLogEntry>,
        keys: TeamMemberKeys,
        envelope: NewEnvelope,
    ) -> bool {
        {
            let Some(t) = self.vaults.get_mut(&vault_id).and_then(|v| v.team.as_mut()) else {
                return false;
            };
            let Some(last) = log.last() else {
                return false;
            };
            t.head = entry_hash(last);
            t.log = log;
            t.key_epoch = 0;
            t.chain.clear();
            t.members.insert(
                keys.user_id,
                MemberRec {
                    role: VaultRole::Editor,
                    keys,
                    joined_at: Utc::now(),
                },
            );
        }
        self.store_envelope(vault_id, envelope, cc_protocol::DeviceId::new());
        true
    }

    /// `PUT /v1/teams/{id}/key-commitment` (protocol 1.12): an owner or
    /// admin publishes the commitment of the current epoch.
    pub(crate) fn put_team_key_commitment(
        &mut self,
        p: Principal,
        vault_id: VaultId,
        req: cc_protocol::teams::PutTeamKeyCommitmentRequest,
    ) -> MResult<()> {
        if !self.cfg.team_key_commitment {
            return Err(not_found());
        }
        let (v, t) = self.team_vault(&p, vault_id)?;
        Self::check_vak(v, &req.vault_access_key)?;
        let me = t.members.get(&p.user_id).ok_or_else(not_found)?;
        if !admin(me.role) {
            return Err(err(ErrorCode::Forbidden, "team admins only"));
        }
        if req.key_commitment.key_epoch != t.key_epoch
            || req.key_commitment.signer_user_id != p.user_id
        {
            return Err(bad("key commitment of another epoch or signer"));
        }
        let epoch = t.key_epoch;
        if let Some(t) = self.vaults.get_mut(&vault_id).and_then(|v| v.team.as_mut()) {
            t.key_commitments.insert(epoch, req.key_commitment);
        }
        Ok(())
    }

    /// Test hook: a hostile server drops every published key commitment.
    pub(crate) fn strip_team_key_commitments(&mut self, vault_id: VaultId) -> bool {
        match self.vaults.get_mut(&vault_id).and_then(|v| v.team.as_mut()) {
            Some(t) => {
                t.key_commitments.clear();
                true
            }
            None => false,
        }
    }

    /// Test hook: a hostile server replaces a member's user envelope with
    /// one of a key it chose, keeping the key epoch.
    pub(crate) fn plant_team_user_envelope(
        &mut self,
        vault_id: VaultId,
        envelope: NewEnvelope,
    ) -> bool {
        if !self.vaults.get(&vault_id).is_some_and(|v| v.team.is_some()) {
            return false;
        }
        self.store_envelope(vault_id, envelope, cc_protocol::DeviceId::new());
        true
    }

    /// Test hook: the keys an invitee sent when accepting.
    pub(crate) fn team_invitation_keys(
        &self,
        invitation_id: TeamInvitationId,
    ) -> Option<TeamMemberKeys> {
        self.team_invitations.get(&invitation_id)?.keys.clone()
    }

    /// Test hook: a hostile server swaps the keys/role of an invitation.
    pub(crate) fn swap_team_invitation_keys(
        &mut self,
        invitation_id: TeamInvitationId,
        keys: TeamMemberKeys,
        role: Option<VaultRole>,
    ) -> bool {
        let Some(r) = self.team_invitations.get_mut(&invitation_id) else {
            return false;
        };
        r.keys = Some(keys);
        if let Some(role) = role {
            r.role = role;
        }
        true
    }

    /// Test hook: a hostile server adds `user` with `keys` without any
    /// admin signature (the forged entry carries a garbage signature).
    pub(crate) fn inject_team_member(&mut self, vault_id: VaultId, keys: TeamMemberKeys) -> bool {
        let Some(t) = self.vaults.get_mut(&vault_id).and_then(|v| v.team.as_mut()) else {
            return false;
        };
        let Some(last) = t.log.last().cloned() else {
            return false;
        };
        let mut forged = last.clone();
        forged.entry.seq = last.entry.seq + 1;
        forged.entry.previous_hash = Bytes::new(t.head.to_vec());
        forged.entry.op = TeamLogOp::AddMember {
            member: keys.clone(),
            role: VaultRole::Editor,
        };
        forged.signature = Bytes::new(vec![7; 64]);
        t.head = entry_hash(&forged);
        t.log.push(forged);
        t.members.insert(
            keys.user_id,
            MemberRec {
                role: VaultRole::Editor,
                keys,
                joined_at: Utc::now(),
            },
        );
        true
    }
}
