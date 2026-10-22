//! Teams (protocol 1.9, TZ teams §6): the team vaults of a synced profile.
//!
//! A team is an ordinary vault of its own (own [`ObjectStore`], codec and
//! sync engine) whose VRK reaches this member through a user envelope sealed
//! to the per-team identity kept in a personal-vault
//! [`cc_models::team::TeamMembership`] object. Its objects share the
//! session's working set, tagged with the team vault, so every facade read
//! (lists, connections, search) sees them; [`crate::writer::VaultWriter`]
//! routes writes to the vault an object belongs to.
//!
//! Trust: members, roles and keys come only from the membership log this
//! client verified itself ([`TeamLogView`]). A log that does not verify, or
//! that no longer extends the head verified earlier, pauses the team
//! (read-only, no envelopes sealed) and raises a warning.

mod open;
mod routing;

pub(crate) use open::{
    close_team, encode_identity, membership_of, open_teams, refresh_team, sync_teams,
    TeamOpenContext,
};
pub(crate) use routing::{
    ai_blocked, check_ai_allowed, check_references, infer_vault, references, team_capable,
};

use crate::codec::VaultCodec;
use crate::error::{AppError, AppResult};
use crate::writer::EngineSlot;
use cc_crypto_core::teams::{TeamIdentity, TeamLogView};
use cc_models::settings::{VaultPolicies, VaultSettings};
use cc_protocol::teams::{SignedTeamLogEntry, TeamMember};
use cc_protocol::vaults::VaultRole;
use cc_protocol::{UserId, VaultId};
use cc_sync_core::{ObjectStore, SyncEngine};
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex, RwLock};
use tokio::task::JoinHandle;

/// Why a team is paused (read-only, nothing sealed or signed).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TeamWarning {
    /// The server showed a log that does not verify (forged or broken entry).
    InvalidLog,
    /// The server showed a shorter or different log than verified before.
    LogRollback,
}

impl TeamWarning {
    pub(crate) fn code(self) -> &'static str {
        match self {
            TeamWarning::InvalidLog => "invalid_log",
            TeamWarning::LogRollback => "log_rollback",
        }
    }
}

/// Mutable view of one open team.
#[derive(Debug, Clone)]
pub(crate) struct TeamState {
    /// The verified log (prefix up to the first entry that failed).
    pub view: TeamLogView,
    pub log: Vec<SignedTeamLogEntry>,
    /// Server member listing (emails); trusted only where it matches `view`.
    pub listed: Vec<TeamMember>,
    pub key_epoch: u32,
    pub rotation_required: bool,
    pub settings: Option<VaultSettings>,
    pub warning: Option<TeamWarning>,
    /// Opened from the local cache (server unreachable).
    pub offline: bool,
}

/// One open team vault of the session.
pub(crate) struct TeamHandle {
    pub vault_id: VaultId,
    pub user_id: UserId,
    pub codec: Arc<VaultCodec>,
    pub store: ObjectStore,
    pub engine: EngineSlot,
    identity: Mutex<Option<TeamIdentity>>,
    state: RwLock<TeamState>,
    tasks: Mutex<Vec<JoinHandle<()>>>,
}

impl std::fmt::Debug for TeamHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TeamHandle")
            .field("vault_id", &self.vault_id)
            .finish_non_exhaustive()
    }
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|p| p.into_inner())
}

impl TeamHandle {
    pub(crate) fn new(
        vault_id: VaultId,
        user_id: UserId,
        codec: Arc<VaultCodec>,
        store: ObjectStore,
        identity: TeamIdentity,
        state: TeamState,
    ) -> Arc<Self> {
        Arc::new(Self {
            vault_id,
            user_id,
            codec,
            store,
            engine: Arc::new(RwLock::new(None)),
            identity: Mutex::new(Some(identity)),
            state: RwLock::new(state),
            tasks: Mutex::new(Vec::new()),
        })
    }

    pub(crate) fn state(&self) -> TeamState {
        self.state.read().unwrap_or_else(|p| p.into_inner()).clone()
    }

    pub(crate) fn update<R>(&self, f: impl FnOnce(&mut TeamState) -> R) -> R {
        f(&mut self.state.write().unwrap_or_else(|p| p.into_inner()))
    }

    /// This member's verified role (`None` once removed from the log).
    pub(crate) fn role(&self) -> Option<VaultRole> {
        self.state().view.role_of(self.user_id)
    }

    /// Install new team settings (and their AI policy in the working set).
    pub(crate) fn set_settings(
        &self,
        working: &crate::working_set::WorkingSet,
        settings: Option<VaultSettings>,
    ) {
        let blocked = settings
            .as_ref()
            .is_some_and(|s| s.policies.block_ai_context);
        self.update(|st| st.settings = settings);
        working.set_ai_blocked(self.vault_id, blocked);
    }

    pub(crate) fn policies(&self) -> VaultPolicies {
        self.state()
            .settings
            .map(|s| s.policies)
            .unwrap_or_default()
    }

    /// Writes to this team are allowed (editor+, log trusted).
    pub(crate) fn require_writable(&self) -> AppResult<()> {
        let s = self.state();
        if s.warning.is_some() {
            return Err(AppError::TeamUntrusted(
                s.warning.map(TeamWarning::code).unwrap_or_default().into(),
            ));
        }
        match s.view.role_of(self.user_id) {
            Some(VaultRole::Owner | VaultRole::Admin | VaultRole::Editor) => Ok(()),
            _ => Err(AppError::TeamReadOnly),
        }
    }

    /// Membership administration is allowed (admin+, log trusted).
    pub(crate) fn require_admin(&self) -> AppResult<VaultRole> {
        self.require_writable()?;
        match self.role() {
            Some(r @ (VaultRole::Owner | VaultRole::Admin)) => Ok(r),
            _ => Err(AppError::TeamReadOnly),
        }
    }

    /// Run `f` with the per-team identity (signing, opening envelopes).
    pub(crate) fn with_identity<R>(
        &self,
        f: impl FnOnce(&TeamIdentity) -> AppResult<R>,
    ) -> AppResult<R> {
        let g = lock(&self.identity);
        let id = g.as_ref().ok_or(AppError::VaultLocked)?;
        f(id)
    }

    pub(crate) fn engine(&self) -> Option<SyncEngine> {
        self.engine
            .read()
            .unwrap_or_else(|p| p.into_inner())
            .clone()
    }

    pub(crate) fn push_task(&self, t: JoinHandle<()>) {
        lock(&self.tasks).push(t);
    }

    /// Stop sync and drop key material.
    pub(crate) async fn shutdown(&self) {
        let engine = self
            .engine
            .write()
            .unwrap_or_else(|p| p.into_inner())
            .take();
        if let Some(e) = engine {
            e.stop().await;
        }
        for t in lock(&self.tasks).drain(..) {
            t.abort();
        }
        lock(&self.identity).take();
        self.codec.clear();
    }
}

/// The open teams of an unlocked session.
#[derive(Debug, Default)]
pub(crate) struct TeamRegistry {
    teams: RwLock<BTreeMap<VaultId, Arc<TeamHandle>>>,
    /// Serializes opening / closing teams (unlock, WebSocket events and
    /// facade calls may race).
    pub(crate) gate: tokio::sync::Mutex<()>,
}

impl TeamRegistry {
    pub(crate) fn get(&self, vault_id: VaultId) -> Option<Arc<TeamHandle>> {
        self.teams
            .read()
            .unwrap_or_else(|p| p.into_inner())
            .get(&vault_id)
            .cloned()
    }

    pub(crate) fn require(&self, vault_id: VaultId) -> AppResult<Arc<TeamHandle>> {
        self.get(vault_id)
            .ok_or_else(|| AppError::not_found("team", vault_id))
    }

    pub(crate) fn all(&self) -> Vec<Arc<TeamHandle>> {
        self.teams
            .read()
            .unwrap_or_else(|p| p.into_inner())
            .values()
            .cloned()
            .collect()
    }

    pub(crate) fn insert(&self, team: Arc<TeamHandle>) -> Option<Arc<TeamHandle>> {
        self.teams
            .write()
            .unwrap_or_else(|p| p.into_inner())
            .insert(team.vault_id, team)
    }

    pub(crate) fn remove(&self, vault_id: VaultId) -> Option<Arc<TeamHandle>> {
        self.teams
            .write()
            .unwrap_or_else(|p| p.into_inner())
            .remove(&vault_id)
    }

    /// Lock: stop every team and drop its keys.
    pub(crate) async fn shutdown(&self) {
        let all: Vec<_> =
            std::mem::take(&mut *self.teams.write().unwrap_or_else(|p| p.into_inner()))
                .into_values()
                .collect();
        for t in all {
            t.shutdown().await;
        }
    }
}
