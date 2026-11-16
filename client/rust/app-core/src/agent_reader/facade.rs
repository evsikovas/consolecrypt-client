//! STAGED Native-only coordinator. No FRB/UI route, provider injection or key export.
//! Opaque reviews contain one-use state, not evidence of a human approval UI.
#[path = "facade/response.rs"]
mod response;
#[cfg(test)]
#[path = "facade/tests.rs"]
mod tests;
#[cfg(test)]
#[path = "facade/cold_ssh.rs"]
mod cold_ssh;
#[cfg(test)]
#[path = "facade/synced_tests.rs"]
mod synced_tests;

use super::authority::{
    AuthorityError, NativeReaderAuthority, ReaderRevokeSelection, RegisteredReaderAssociation,
};
use super::scope::{ReaderOriginalContext, ReaderSourceSelection};
use super::transport::{
    ObservedAgentEndpoint, PreparedReaderGet, ReaderBrokerError, ReaderCleanupObservation,
    ReaderPeerBinding, ReaderSshSelection, ReaderTransportCore, ReaderTransportError,
    SignedReaderAdmin,
};
use crate::AppCore;
use cc_crypto_core::agent_reader::*;
use cc_models::ObjectId;
use cc_storage_core::ProfileKind;
use serde_json::Value;
use std::collections::{BTreeMap, HashMap};
use std::future::Future;
use std::marker::PhantomData;
use std::sync::{Arc, Mutex, Weak};
use std::time::{Duration, Instant};
use tokio::sync::oneshot;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

type Result<T> = std::result::Result<T, Failure>;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Failure {
    Busy,
    Rejected,
    UnsupportedProfile,
    Unavailable,
    OutcomeUnknown,
    Cancelled,
    Expired,
    ResponseInvalid,
    RevokePending,
}

/// Public native metadata reference, never permission. Any restored locator is
/// revalidated through actual authority replay/witness/source checks on use.
pub(super) struct AssociationRef {
    peer: AgentReaderHash,
    association: AgentReaderHash,
}
impl AssociationRef {
    pub fn from_public_locator(peer: AgentReaderHash, association: AgentReaderHash) -> Self {
        Self { peer, association }
    }
    pub fn locator(&self) -> (AgentReaderHash, AgentReaderHash) {
        (self.peer, self.association)
    }
}
impl std::fmt::Debug for AssociationRef {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("AssociationRef([redacted])")
    }
}

#[derive(Clone, PartialEq, Eq, PartialOrd, Ord)]
struct PeerKey {
    profile: AgentReaderNativeProfileId,
    agent: AgentReaderAgentId,
    audience: AgentReaderAudienceId,
    origin: AgentReaderHash,
    pin: AgentReaderHash,
    // Stable authenticated owner namespace, NOT a session/refresh identifier.
    owner: AgentReaderHash,
}
impl PeerKey {
    fn from_binding(b: &ReaderPeerBinding, ctx: &ReaderOriginalContext) -> Self {
        Self {
            profile: b.native_profile_id,
            agent: b.agent_id,
            audience: b.audience_id,
            origin: b.node_trust_origin_hash,
            pin: b.issuer_pin_genesis_hash,
            owner: ctx.stable_owner_binding_hash(),
        }
    }
}
#[derive(Clone, Copy, PartialEq, Eq)]
enum Role {
    Read,
    Association,
    Revoke,
    Reconcile,
    Metadata,
}
struct PeerGeneration {
    value: u64,
    blocked: bool,
}
struct Entry {
    original: Option<Weak<ReaderOriginalContext>>,
    cancel: CancellationToken,
    peer: Option<PeerKey>,
    generation: u64,
    role: Role,
    quarantined: bool,
}
#[derive(Default)]
struct State {
    entries: HashMap<Uuid, Entry>,
    peers: BTreeMap<PeerKey, PeerGeneration>,
}

/// Resource containment only. No authority, credentials, responses or global
/// provider registry. AppCore clones share this one Inner-owned instance.
#[derive(Default)]
pub(crate) struct NativeReaderRuntime {
    state: Mutex<State>,
    pub(crate) auth_transitions: super::authentication::ReaderAuthTransitions,
}
impl std::fmt::Debug for NativeReaderRuntime {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("NativeReaderRuntime([redacted])")
    }
}
impl NativeReaderRuntime {
    pub(super) fn public_counts(&self) -> Result<(usize, usize)> {
        let s = self.state.lock().map_err(|_| Failure::Unavailable)?;
        Ok((
            s.entries.len(),
            s.entries.values().filter(|e| e.quarantined).count(),
        ))
    }
    pub(super) fn reserve_owner_review(self: &Arc<Self>) -> Result<ReaderOwnerSlot> {
        self.reserve().map(ReaderOwnerSlot)
    }
    fn reserve(self: &Arc<Self>) -> Result<Lease> {
        let mut s = self.state.lock().map_err(|_| Failure::Unavailable)?;
        if s.entries.len() >= 8 {
            return Err(Failure::Busy);
        }
        let id = Uuid::new_v4();
        let cancel = CancellationToken::new();
        s.entries.insert(
            id,
            Entry {
                original: None,
                cancel: cancel.clone(),
                peer: None,
                generation: 0,
                role: Role::Metadata,
                quarantined: false,
            },
        );
        Ok(Lease {
            runtime: self.clone(),
            id,
            cancel,
            deadline: Instant::now() + Duration::from_secs(60),
            pending: false,
            poisoned: false,
        })
    }
    fn bind_original(&self, id: Uuid, ctx: &Arc<ReaderOriginalContext>) -> Result<()> {
        ctx.scope_identity().check(ctx).map_err(|_| Failure::Cancelled)?;
        let mut s = self.state.lock().map_err(|_| Failure::Unavailable)?;
        let entry = s.entries.get_mut(&id).ok_or(Failure::Rejected)?;
        if entry.cancel.is_cancelled() { return Err(Failure::Cancelled); }
        if let Some(old) = entry.original.as_ref().and_then(Weak::upgrade) {
            if !Arc::ptr_eq(&old, ctx) { return Err(Failure::Rejected); }
        }
        entry.original = Some(Arc::downgrade(ctx));
        Ok(())
    }
    fn bind(&self, id: Uuid, key: PeerKey, role: Role) -> Result<()> {
        let mut s = self.state.lock().map_err(|_| Failure::Unavailable)?;
        if !s.peers.contains_key(&key) && s.peers.len() >= 128 {
            return Err(Failure::Busy);
        }
        let p = s.peers.entry(key.clone()).or_insert(PeerGeneration {
            value: 0,
            blocked: false,
        });
        if p.blocked && !matches!(role, Role::Revoke | Role::Reconcile) {
            return Err(Failure::RevokePending);
        }
        let generation = p.value;
        let e = s.entries.get_mut(&id).ok_or(Failure::Rejected)?;
        if e.peer.as_ref().is_some_and(|v| v != &key) || e.cancel.is_cancelled() {
            return Err(Failure::Cancelled);
        }
        e.peer = Some(key);
        e.role = role;
        e.generation = generation;
        Ok(())
    }
    fn fence_locked<T>(&self, id: Uuid, f: impl FnOnce() -> Result<T>) -> Result<T> {
        let s = self.state.lock().map_err(|_| Failure::Unavailable)?;
        let e = s.entries.get(&id).ok_or(Failure::Rejected)?;
        if e.cancel.is_cancelled() || e.quarantined {
            return Err(Failure::Cancelled);
        }
        if let Some(key) = &e.peer {
            let p = s.peers.get(key).ok_or(Failure::Rejected)?;
            if p.value != e.generation
                || p.blocked && !matches!(e.role, Role::Revoke | Role::Reconcile)
            {
                return Err(Failure::RevokePending);
            }
        }
        // Only short synchronous validation/signing/publication is allowed.
        // Never await or call storage/network while this mutex is held.
        f()
    }
    fn begin_revoke(&self, id: Uuid) -> Result<()> {
        let mut s = self.state.lock().map_err(|_| Failure::Unavailable)?;
        let key = s
            .entries
            .get(&id)
            .and_then(|e| e.peer.clone())
            .ok_or(Failure::Rejected)?;
        if s.entries
            .get(&id)
            .is_none_or(|e| e.role != Role::Revoke || e.cancel.is_cancelled())
        {
            return Err(Failure::Rejected);
        }
        let p = s.peers.get_mut(&key).ok_or(Failure::Rejected)?;
        p.blocked = true;
        p.value = p.value.checked_add(1).ok_or(Failure::Unavailable)?;
        let generation = p.value;
        for (other, e) in &mut s.entries {
            if e.peer.as_ref() == Some(&key) {
                if *other == id {
                    e.generation = generation;
                } else {
                    e.cancel.cancel();
                    if let Some(ctx) = e.original.as_ref().and_then(Weak::upgrade) { ctx.retire(); }
                }
            }
        }
        // The exact revoker slot survives; its independent user cancel token
        // remains effective. This role cannot publish data or unblock reads.
        Ok(())
    }
    fn unblock_after_explicit_reconcile(&self, id: Uuid) -> Result<()> {
        let mut s = self.state.lock().map_err(|_| Failure::Unavailable)?;
        let e = s.entries.get(&id).ok_or(Failure::Rejected)?;
        if e.role != Role::Reconcile || e.cancel.is_cancelled() {
            return Err(Failure::Rejected);
        }
        let key = e.peer.clone().ok_or(Failure::Rejected)?;
        let generation = e.generation;
        let p = s.peers.get_mut(&key).ok_or(Failure::Rejected)?;
        if p.value != generation {
            return Err(Failure::RevokePending);
        }
        p.value = p.value.checked_add(1).ok_or(Failure::Unavailable)?;
        p.blocked = false;
        Ok(())
    }
    pub(crate) fn shutdown(&self) {
        if let Ok(s) = self.state.lock() {
            for e in s.entries.values() {
                e.cancel.cancel();
                if let Some(ctx) = e.original.as_ref().and_then(Weak::upgrade) { ctx.retire(); }
            }
        }
    }
}
impl Drop for NativeReaderRuntime {
    fn drop(&mut self) {
        self.shutdown();
    }
}

struct Lease {
    runtime: Arc<NativeReaderRuntime>,
    id: Uuid,
    cancel: CancellationToken,
    deadline: Instant,
    pending: bool,
    poisoned: bool,
}
pub(super) struct ReaderOwnerSlot(Lease);
impl ReaderOwnerSlot {
    pub fn bind_original(&self, ctx: &Arc<ReaderOriginalContext>) -> Result<()> {
        self.0.runtime.bind_original(self.0.id, ctx)
    }
    pub fn check(&self) -> Result<()> {
        self.0.check()
    }
    pub fn cancellation(&self) -> CancellationToken {
        self.0.cancel.clone()
    }
    pub fn deadline(&self) -> Instant {
        self.0.deadline
    }
}
impl Lease {
    fn arm(&mut self) {
        self.pending = true;
    }
    fn ack(&mut self) {
        self.pending = false;
    }
    fn poison(&mut self) {
        self.poisoned = true;
    }
    fn check(&self) -> Result<()> {
        if Instant::now() >= self.deadline {
            return Err(Failure::Expired);
        }
        self.runtime.fence_locked(self.id, || Ok(()))
    }
    fn authority<T>(&mut self, value: std::result::Result<T, AuthorityError>) -> Result<T> {
        match value {
            Ok(v) => {
                self.ack();
                Ok(v)
            }
            Err(AuthorityError::Rejected | AuthorityError::Quota) => {
                self.ack();
                Err(Failure::Rejected)
            }
            Err(AuthorityError::Unavailable | AuthorityError::Unknown) => {
                self.poison();
                Err(Failure::OutcomeUnknown)
            }
        }
    }
    fn transport<T>(&mut self, value: std::result::Result<T, ReaderTransportError>) -> Result<T> {
        match value {
            Ok(v) => {
                self.ack();
                Ok(v)
            }
            Err(ReaderTransportError::BrokerRejected(ReaderBrokerError::StateUnknown)) => {
                self.poison();
                Err(Failure::OutcomeUnknown)
            }
            Err(
                ReaderTransportError::Rejected
                | ReaderTransportError::AlreadyAttempted
                | ReaderTransportError::BrokerRejected(_),
            ) => {
                self.ack();
                Err(Failure::Rejected)
            }
            Err(_) => {
                self.poison();
                Err(Failure::OutcomeUnknown)
            }
        }
    }
    fn closed(&mut self, observation: ReaderCleanupObservation) {
        if observation == ReaderCleanupObservation::ClosedObserved {
            self.ack();
        } else {
            self.poison();
        }
    }
}
impl Drop for Lease {
    fn drop(&mut self) {
        self.cancel.cancel();
        if let Ok(mut s) = self.runtime.state.lock() {
            if self.poisoned || self.pending {
                if let Some(e) = s.entries.get_mut(&self.id) {
                    e.quarantined = true;
                }
            } else {
                s.entries.remove(&self.id);
            }
        }
    }
}

/// A dropped waiter cancels but never aborts its owned cleanup workflow.
pub(super) struct NativeReaderOperation<T> {
    original: Option<Weak<ReaderOriginalContext>>,
    rx: Option<oneshot::Receiver<Result<T>>>,
    cancel: CancellationToken,
    finished: bool,
}
impl<T> std::fmt::Debug for NativeReaderOperation<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("NativeReaderOperation([redacted])")
    }
}
impl<T> NativeReaderOperation<T> {
    pub(super) fn from_original(mut self, ctx: &Arc<ReaderOriginalContext>) -> Self {
        self.original = Some(Arc::downgrade(ctx));
        self
    }
    pub(super) fn cancellation(&self) -> CancellationToken {
        self.cancel.clone()
    }
    pub fn cancel(&self) {
        self.cancel.cancel();
        if let Some(ctx) = self.original.as_ref().and_then(Weak::upgrade) { ctx.retire(); }
    }
    pub async fn wait(mut self) -> Result<T> {
        let rx = self.rx.take().ok_or(Failure::Rejected)?;
        match rx.await {
            Ok(v) => {
                self.finished = true;
                v
            }
            Err(_) => Err(Failure::OutcomeUnknown),
        }
    }
}
impl<T> Drop for NativeReaderOperation<T> {
    fn drop(&mut self) {
        if !self.finished {
            self.cancel();
        }
    }
}
fn launch<T: Send + 'static>(
    cancel: CancellationToken,
    work: impl Future<Output = Result<T>> + Send + 'static,
) -> NativeReaderOperation<T> {
    let (tx, rx) = oneshot::channel();
    tokio::spawn(async move {
        let result = work.await;
        let _ = tx.send(result);
    });
    NativeReaderOperation {
        original: None,
        rx: Some(rx),
        cancel,
        finished: false,
    }
}

pub(super) struct NativeReaderSelection {
    pub original: Option<Arc<ReaderOriginalContext>>,
    pub source: ReaderSourceSelection,
    pub known_host_id: ObjectId,
}
impl std::fmt::Debug for NativeReaderSelection {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("NativeReaderSelection([redacted])")
    }
}
pub(super) struct ProvisionedAssociationSelection {
    pub pin: AgentReaderIssuerPinV1,
    pub target: AgentReaderTargetDescriptorV1,
    pub binding: ReaderPeerBinding,
    pub installed_head: AgentReaderHeadV1,
}
fn local_only(kind: &ProfileKind, has_user: bool, has_account_api: bool) -> Result<()> {
    if *kind != ProfileKind::Local || has_user || has_account_api {
        Err(Failure::UnsupportedProfile)
    } else {
        Ok(())
    }
}
// Shape routing only. Synced authentication/Owner/Active/personal provenance
// MUST be established by the reviewed Root capture before authority/SSH work.
// This does not accept a bearer, cached UUID, frontend bool or provider grant.
fn supported_profile_shape(
    kind: &ProfileKind,
    has_user: bool,
    has_account_api: bool,
    has_server: bool,
) -> Result<()> {
    match kind {
        ProfileKind::Local => {
            local_only(kind, has_user, has_account_api)?;
            if has_server { return Err(Failure::Rejected); }
            Ok(())
        }
        ProfileKind::Synced if has_user && has_account_api && has_server => Ok(()),
        ProfileKind::Synced => Err(Failure::UnsupportedProfile),
    }
}

struct Base {
    app: Weak<crate::app::Inner>,
    ctx: Arc<ReaderOriginalContext>,
    retire_ctx: bool,
    authority: NativeReaderAuthority,
    lease: Lease,
    working_version: u64,
}
impl Base {
    async fn capture(
        app: AppCore,
        mut lease: Lease,
        selection: &NativeReaderSelection,
    ) -> Result<Self> {
        // Cancel the ORIGINAL attempt at its pre-acquisition absolute bound.
        // This does not abort the owned workflow or release quarantined slots;
        // finally cleanup continues. Root scope's own bound may only shorten it.
        let deadline = lease.deadline;
        let cancel = lease.cancel.clone();
        tokio::spawn(async move {
            tokio::select! {biased;
                _=cancel.cancelled()=>{},
                _=tokio::time::sleep_until(tokio::time::Instant::from_std(deadline))=>cancel.cancel(),
            }
        });
        // Closed Local/PersonalSynced shape preflight is not authentication.
        // The following Root capture performs ACTUAL account/owner validation.
        // A timed-out acquisition is still quarantined; no fallback or retry.
        // A borrowed public attempt is checked BEFORE profile/auth/probe I/O.
        if let Some(ctx) = &selection.original {
            ctx.scope_identity().check(ctx).map_err(|_| Failure::Cancelled)?;
            let v = ctx.snapshot();
            if v.native_profile_id != selection.source.profile_id
                || v.native_host_id != selection.source.host_id
                || v.native_source_id != selection.source.source_id
                || v.native_source_revision != selection.source.source_revision
                || v.native_binding_id != selection.source.binding_id
                || v.native_binding_revision != selection.source.binding_revision { return Err(Failure::Rejected); }
            lease.runtime.bind_original(lease.id, ctx)?;
        }
        lease.arm();
        let initial = async {
            if let Some(ctx) = &selection.original {
                ctx.check(&app).await.map_err(|_| Failure::Cancelled)?;
            }
            let owner = app.session().await.map_err(|_| Failure::Unavailable)?;
            let profile = owner.profile().await.map_err(|_| Failure::Unavailable)?;
            supported_profile_shape(
                &profile.kind,
                profile.user_id.is_some(),
                owner.api_opt().is_some(),
                profile.server_url.is_some(),
            )?;
            if owner
                .storage()
                .map_err(|_| Failure::Unavailable)?
                .path()
                .is_none()
            {
                return Err(Failure::Rejected);
            }
            Ok(())
        };
        let preflight = tokio::select! {biased;
            _=lease.cancel.cancelled()=>Err(Failure::Cancelled),
            _=tokio::time::sleep_until(tokio::time::Instant::from_std(lease.deadline))=>Err(Failure::OutcomeUnknown),
            result=initial=>result,
        };
        if let Err(e) = preflight {
            if matches!(e, Failure::UnsupportedProfile | Failure::Rejected) {
                lease.ack();
            } else {
                lease.poison();
            }
            return Err(e);
        }
        let retire_ctx = selection.original.is_none();
        let ctx = if let Some(ctx) = &selection.original {
            ctx.check(&app).await.map_err(|_| Failure::Cancelled)?;
            ctx.clone()
        } else {
            match ReaderOriginalContext::capture(&app, selection.source.clone(), &lease.cancel).await {
                Ok(v) => Arc::new(v),
                Err(_) => { lease.poison(); return Err(Failure::OutcomeUnknown); }
            }
        };
        let authority = match NativeReaderAuthority::for_original_context(&app, &ctx).await {
            Ok(v) => v,
            Err(_) => {
                lease.poison();
                return Err(Failure::OutcomeUnknown);
            }
        };
        let working_version = ctx
            .with_held(&app, |held| Ok(held.unlock.working().version()))
            .await
            .map_err(|_| {
                lease.poison();
                Failure::OutcomeUnknown
            })?;
        lease.ack();
        Ok(Self {
            app: Arc::downgrade(&app.inner),
            ctx,
            retire_ctx,
            authority,
            lease,
            working_version,
        })
    }
    fn app(&self) -> Result<AppCore> {
        // Prepared reviews cannot keep the original root alive while idle.
        // Upgrade only this root for bounded work; cleanup needs no root.
        self.ctx.scope_identity().check(&self.ctx).map_err(|_| Failure::Cancelled)?;
        Ok(AppCore { inner: self.app.upgrade().ok_or(Failure::Cancelled)? })
    }
    fn belongs_to(&self, app: &AppCore) -> Result<()> {
        if !Weak::ptr_eq(&self.app, &Arc::downgrade(&app.inner)) {
            return Err(Failure::Rejected);
        }
        self.lease.check()
    }
    async fn observed(
        &mut self,
        selection: &NativeReaderSelection,
    ) -> Result<ObservedAgentEndpoint> {
        self.lease.arm();
        let ssh = self
            .ctx
            .with_held(&self.app()?, |held| {
                let host = held
                    .unlock
                    .working()
                    .known_host(selection.known_host_id)
                    .ok_or_else(|| {
                        crate::AppError::invalid("agent_reader", "reader_trust_unavailable")
                    })?;
                // Actual async Native planner is cloned and run without the guards.
                Ok((held.unlock.ssh.planner.clone(), host))
            })
            .await
            .map_err(|_| {
                self.lease.poison();
                Failure::OutcomeUnknown
            })?;
        let plan = tokio::select! {biased;
            _=self.ctx.cancelled()=>return Err(Failure::Cancelled),
            result=ssh.0.plan(crate::dto::parse_id("agent_reader_host",&self.ctx.snapshot().native_host_id).map_err(|_|Failure::Rejected)?)=>
                result.map_err(|_|Failure::Rejected)?,
        };
        let selection = self
            .ctx
            .with_held(&self.app()?, |held| {
                ReaderSshSelection::from_original_context(&held, plan, ssh.1).map_err(|_| {
                    crate::AppError::invalid("agent_reader", "reader_trust_unavailable")
                })
            })
            .await
            .map_err(|_| {
                self.lease.poison();
                Failure::OutcomeUnknown
            })?;
        let result =
            ReaderTransportCore::probe_unapproved_once(&self.app()?, &self.ctx, selection).await;
        // A failed probe consumes its owned endpoint on Drop. There is no
        // positive close acknowledgement available to this caller, even for
        // an otherwise known protocol denial. Keep its slot quarantined.
        if result.is_err() {
            self.lease.poison();
        }
        self.lease.transport(result)
    }
    async fn verified(
        &mut self,
        selection: &NativeReaderSelection,
        binding: ReaderPeerBinding,
        head: &AgentReaderHeadV1,
    ) -> Result<ReaderTransportCore> {
        let observed = self.observed(selection).await?;
        self.lease.arm();
        let result = observed
            .probe_verified_once(&self.app()?, &self.ctx, binding, head)
            .await;
        if result.is_err() {
            self.lease.poison();
        }
        self.lease.transport(result)
    }
    async fn final_fence(&self, expires: u64) -> Result<()> {
        self.ctx
            .with_held(&self.app()?, |held| {
                if held.unlock.working().version() != self.working_version {
                    return Err(crate::AppError::invalid(
                        "agent_reader",
                        "reader_metadata_changed",
                    ));
                }
                self.lease
                    .runtime
                    .fence_locked(self.lease.id, || {
                        self.lease.check_without_lock()?;
                        let now = self.ctx.now().map_err(|_| Failure::Expired)?;
                        if now.timestamp() < 0 || now.timestamp() as u64 >= expires {
                            return Err(Failure::Expired);
                        }
                        Ok(())
                    })
                    .map_err(|_| {
                        crate::AppError::invalid("agent_reader", "reader_publication_unavailable")
                    })
            })
            .await
            .map_err(|_| Failure::Cancelled)?;
        self.lease.runtime.fence_locked(self.lease.id, || {
            self.ctx
                .scope_identity()
                .check(&self.ctx)
                .map_err(|_| Failure::Cancelled)?;
            let now = self.ctx.now().map_err(|_| Failure::Expired)?;
            if now.timestamp() < 0 || now.timestamp() as u64 >= expires {
                return Err(Failure::Expired);
            }
            self.lease.check_without_lock()?;
            Ok(())
        })
    }
}
impl Lease {
    fn check_without_lock(&self) -> Result<()> {
        if self.cancel.is_cancelled() {
            return Err(Failure::Cancelled);
        }
        if Instant::now() >= self.deadline {
            return Err(Failure::Expired);
        }
        Ok(())
    }
}

enum Payload {
    Issuer,
    Association {
        selection: ProvisionedAssociationSelection,
        transport: ReaderTransportCore,
    },
    Read {
        registered: RegisteredReaderAssociation,
        transport: ReaderTransportCore,
    },
    Revoke {
        reference: AssociationRef,
        kind: ReaderRevokeSelection,
        registered: RegisteredReaderAssociation,
        transport: ReaderTransportCore,
    },
    Reconcile {
        registered: RegisteredReaderAssociation,
        transport: ReaderTransportCore,
    },
}
pub(super) struct IssuerReview;
pub(super) struct AssociationReview;
pub(super) struct ReadReview;
pub(super) struct RevokeReview;
pub(super) struct ReconcileReview;
/// Owned across the explicit Native caller boundary. Borrowed primitive caps
/// are created only after these owned fields reach the final coordinator task.
pub(super) struct NativeReview<K: Send + 'static> {
    base: Option<Base>,
    payload: Option<Payload>,
    kind: PhantomData<K>,
}
impl<K: Send + 'static> std::fmt::Debug for NativeReview<K> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("NativeReview([redacted])")
    }
}
impl<K: Send + 'static> NativeReview<K> {
    pub(super) fn uses_original(&self, ctx: &Arc<ReaderOriginalContext>) -> bool {
        self.base.as_ref().is_some_and(|b| Arc::ptr_eq(&b.ctx, ctx))
    }
    pub(super) fn deadline(&self) -> Result<Instant> {
        Ok(self.base.as_ref().ok_or(Failure::Rejected)?.lease.deadline)
    }
    fn new(base: Base, payload: Payload) -> Self {
        Self {
            base: Some(base),
            payload: Some(payload),
            kind: PhantomData,
        }
    }
    fn take(mut self) -> Result<(Base, Payload)> {
        Ok((
            self.base.take().ok_or(Failure::Rejected)?,
            self.payload.take().ok_or(Failure::Rejected)?,
        ))
    }
    /// Borrowed exact public review material. No transport handle, key or
    /// permission is exposed. A caller still needs an explicit Native UI gate.
    pub fn original_snapshot(&self) -> Result<&super::scope::ReaderContextSnapshot> {
        Ok(self.base.as_ref().ok_or(Failure::Rejected)?.ctx.snapshot())
    }
    pub fn association_selection(&self) -> Option<&ProvisionedAssociationSelection> {
        match self.payload.as_ref()? {
            Payload::Association { selection, .. } => Some(selection),
            _ => None,
        }
    }
    pub fn registered_selection(&self) -> Option<&RegisteredReaderAssociation> {
        match self.payload.as_ref()? {
            Payload::Read { registered, .. } | Payload::Reconcile { registered, .. } | Payload::Revoke { registered, .. } => {
                Some(registered)
            }
            _ => None,
        }
    }
}
async fn close_payload(base: &mut Base, payload: &mut Payload) {
    match payload {
        Payload::Association { transport, .. }
        | Payload::Read { transport, .. }
        | Payload::Reconcile { transport, .. }
        | Payload::Revoke { transport, .. } => {
            base.lease.arm();
            let observation = transport.close().await;
            base.lease.closed(observation);
        }
        _ => {}
    }
    if base.retire_ctx { base.ctx.retire(); }
}
impl<K: Send + 'static> Drop for NativeReview<K> {
    fn drop(&mut self) {
        if let (Some(mut base), Some(mut payload)) = (self.base.take(), self.payload.take()) {
            base.lease.cancel.cancel();
            if matches!(&payload, Payload::Issuer) {
                // Issuer preparation owns metadata only: no transport/material
                // exists to join. Release synchronously, including outside a
                // Tokio runtime. Preserve existing poisoned/pending quarantine.
                if base.retire_ctx { base.ctx.retire(); }
                drop(payload);
                drop(base);
                return;
            }
            // Arm before spawning: even an unpolled/dropped cleanup task
            // cannot release the slot while transport close is unobserved.
            base.lease.arm();
            if let Ok(runtime) = tokio::runtime::Handle::try_current() {
                runtime.spawn(async move {
                    close_payload(&mut base, &mut payload).await;
                });
            } else {
                base.lease.poison();
            }
        }
    }
}

/// Private validated telemetry. No Serialize/Clone/general JSON publication.
pub(super) struct NativePrivateReaderData {
    dataset: response::PrivateDataset,
    expires_at: u64,
    publication: Arc<ReaderOriginalContext>,
    _slot: Lease,
}
impl Drop for NativePrivateReaderData {
    fn drop(&mut self) {
        self.publication.retire();
    }
}
impl std::fmt::Debug for NativePrivateReaderData {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("NativePrivateReaderData([redacted])")
    }
}
impl NativePrivateReaderData {
    pub(super) fn uses_original(&self, ctx: &Arc<ReaderOriginalContext>) -> bool {
        Arc::ptr_eq(&self.publication, ctx)
    }
    pub(super) async fn consume_json(&self, app: &AppCore) -> Result<String> {
        self._slot.check()?;
        let value = self
            .publication
            .with_held(app, |_| {
                let now = self.publication.now()?;
                if now.timestamp() < 0 || now.timestamp() as u64 >= self.expires_at {
                    return Err(crate::AppError::invalid(
                        "agent_reader",
                        "reader_publication_expired",
                    ));
                }
                serde_json::to_string(&self.dataset.value).map_err(|_| {
                    crate::AppError::invalid("agent_reader", "reader_response_invalid")
                })
            })
            .await
            .map_err(|_| Failure::Cancelled)?;
        self._slot.check()?;
        let now = self.publication.now().map_err(|_| Failure::Expired)?;
        if now.timestamp() < 0 || now.timestamp() as u64 >= self.expires_at || value.len() > 262144
        {
            return Err(Failure::Expired);
        }
        Ok(value)
    }
    pub fn target_hash(&self) -> AgentReaderHash {
        self.dataset.target
    }
    pub fn expires_at(&self) -> u64 {
        self.expires_at
    }
    /// Main Native consumer must retain its original UI/session publication
    /// fence. A request proof is not an Agent-process response signature.
    pub fn private_dataset(&self) -> &Value {
        &self.dataset.value
    }
}

/// Metadata-only observation from the authenticated dedicated SSH endpoint.
/// Neither this shape nor its existence is an issuer/Agent approval.
pub(super) struct NativeObservedReaderEndpoint {
    endpoint: super::transport::ObservedReaderEndpointInfo,
    ssh_key: AgentReaderHash,
    original: Weak<ReaderOriginalContext>,
}
impl std::fmt::Debug for NativeObservedReaderEndpoint {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("NativeObservedReaderEndpoint([redacted])")
    }
}
impl NativeObservedReaderEndpoint {
    pub(super) fn uses_original(&self, ctx: &Arc<ReaderOriginalContext>) -> bool {
        self.original.upgrade().is_some_and(|v| Arc::ptr_eq(&v, ctx))
    }
    pub fn endpoint(&self) -> &super::transport::ObservedReaderEndpointInfo {
        &self.endpoint
    }
    pub fn ssh_key_digest(&self) -> AgentReaderHash {
        self.ssh_key
    }
}

impl AppCore {
    pub(super) fn reader_prepare_issuer_initialization_from_original(&self, mut selection: NativeReaderSelection, original: Arc<ReaderOriginalContext>) -> Result<NativeReaderOperation<NativeReview<IssuerReview>>> {
        original.scope_identity().check(&original).map_err(|_|Failure::Cancelled)?;
        selection.original = Some(original.clone());
        self.reader_prepare_issuer_initialization(selection).map(|op| op.from_original(&original))
    }
    pub(super) fn reader_observe_endpoint_once_from_original(&self, mut selection: NativeReaderSelection, original: Arc<ReaderOriginalContext>) -> Result<NativeReaderOperation<NativeObservedReaderEndpoint>> {
        original.scope_identity().check(&original).map_err(|_|Failure::Cancelled)?;
        selection.original = Some(original.clone());
        self.reader_observe_endpoint_once(selection).map(|op| op.from_original(&original))
    }
    pub(super) fn reader_prepare_association_from_original(&self, mut selection: NativeReaderSelection, original: Arc<ReaderOriginalContext>, expected: ProvisionedAssociationSelection) -> Result<NativeReaderOperation<NativeReview<AssociationReview>>> {
        original.scope_identity().check(&original).map_err(|_|Failure::Cancelled)?;
        selection.original = Some(original.clone());
        self.reader_prepare_association(selection, expected).map(|op| op.from_original(&original))
    }
    pub(super) fn reader_prepare_read_from_original(&self, mut selection: NativeReaderSelection, original: Arc<ReaderOriginalContext>, reference: AssociationRef) -> Result<NativeReaderOperation<NativeReview<ReadReview>>> {
        original.scope_identity().check(&original).map_err(|_|Failure::Cancelled)?;
        selection.original = Some(original.clone());
        self.reader_prepare_read(selection, reference).map(|op| op.from_original(&original))
    }
    pub(super) fn reader_prepare_revoke_from_original(&self, mut selection: NativeReaderSelection, original: Arc<ReaderOriginalContext>, reference: AssociationRef, kind: ReaderRevokeSelection) -> Result<NativeReaderOperation<NativeReview<RevokeReview>>> {
        original.scope_identity().check(&original).map_err(|_|Failure::Cancelled)?;
        selection.original = Some(original.clone());
        self.reader_prepare_revoke(selection, reference, kind).map(|op| op.from_original(&original))
    }
    pub(super) fn reader_prepare_reconcile_from_original(&self, mut selection: NativeReaderSelection, original: Arc<ReaderOriginalContext>, reference: AssociationRef) -> Result<NativeReaderOperation<NativeReview<ReconcileReview>>> {
        original.scope_identity().check(&original).map_err(|_|Failure::Cancelled)?;
        selection.original = Some(original.clone());
        self.reader_prepare_reconcile(selection, reference).map(|op| op.from_original(&original))
    }
    /// Native-internal only. The eventual explicit UI/approval call boundary
    /// is separate; these methods are not registered in FRB, CLI or an API.
    pub(super) fn reader_prepare_issuer_initialization(
        &self,
        selection: NativeReaderSelection,
    ) -> Result<NativeReaderOperation<NativeReview<IssuerReview>>> {
        let lease = self.inner.reader_native.reserve()?;
        let cancel = lease.cancel.clone();
        let app = self.clone();
        Ok(launch(cancel, async move {
            let base = Base::capture(app, lease, &selection).await?;
            Ok(NativeReview::new(base, Payload::Issuer))
        }))
    }
    pub(super) fn reader_confirm_issuer_initialization_once(
        &self,
        review: NativeReview<IssuerReview>,
    ) -> Result<NativeReaderOperation<super::authority::ReaderIssuerPublic>> {
        review
            .base
            .as_ref()
            .ok_or(Failure::Rejected)?
            .belongs_to(self)?;
        let (mut base, mut payload) = review.take()?;
        let cancel = base.lease.cancel.clone();
        Ok(launch(cancel, async move {
            let result = async {
                if !matches!(payload, Payload::Issuer) {
                    return Err(Failure::Rejected);
                }
                base.lease.check()?;
                base.lease.arm();
                let value = base
                    .authority
                    .initialize_issuer_explicit(&base.app()?, &base.ctx)
                    .await;
                base.lease.authority(value)
            }
            .await;
            close_payload(&mut base, &mut payload).await;
            result
        }))
    }
    pub(super) fn reader_observe_endpoint_once(
        &self,
        selection: NativeReaderSelection,
    ) -> Result<NativeReaderOperation<NativeObservedReaderEndpoint>> {
        let lease = self.inner.reader_native.reserve()?;
        let cancel = lease.cancel.clone();
        let app = self.clone();
        Ok(launch(cancel, async move {
            let mut base = Base::capture(app, lease, &selection).await?;
            let mut endpoint = base.observed(&selection).await?;
            let result = NativeObservedReaderEndpoint {
                endpoint: endpoint.endpoint().clone(),
                ssh_key: endpoint.ssh_server_key_digest(),
                original: Arc::downgrade(&base.ctx),
            };
            base.lease.arm();
            base.lease.closed(endpoint.close().await);
            if base.lease.poisoned {
                if base.retire_ctx { base.ctx.retire(); }
                return Err(Failure::OutcomeUnknown);
            }
            base.lease.arm();
            let fence = base.final_fence(253402300799).await;
            if fence.is_ok() {
                base.lease.ack();
            }
            if base.retire_ctx { base.ctx.retire(); }
            fence?;
            Ok(result)
        }))
    }
    pub(super) fn reader_prepare_association(
        &self,
        selection: NativeReaderSelection,
        expected: ProvisionedAssociationSelection,
    ) -> Result<NativeReaderOperation<NativeReview<AssociationReview>>> {
        let lease = self.inner.reader_native.reserve()?;
        let cancel = lease.cancel.clone();
        let app = self.clone();
        Ok(launch(cancel, async move {
            let mut base = Base::capture(app, lease, &selection).await?;
            base.lease.runtime.bind(
                base.lease.id,
                PeerKey::from_binding(&expected.binding, &base.ctx),
                Role::Association,
            )?;
            let transport = base
                .verified(
                    &selection,
                    expected.binding.clone(),
                    &expected.installed_head,
                )
                .await?;
            let mut review = NativeReview::new(
                base,
                Payload::Association {
                    selection: expected,
                    transport,
                },
            );
            let b = review.base.as_mut().ok_or(Failure::Rejected)?;
            let Some(Payload::Association {
                selection,
                transport,
            }) = review.payload.as_ref()
            else {
                return Err(Failure::Rejected);
            };
            b.lease.check()?;
            b.lease.arm();
            let result = b
                .authority
                .review_association(
                    &b.app()?,
                    &b.ctx,
                    selection.pin.clone(),
                    selection.target.clone(),
                    transport.peer(),
                )
                .await;
            b.lease.authority(result)?; // Temporary borrowed cap is dropped.
            Ok(review)
        }))
    }
    pub(super) fn reader_approve_association_once(
        &self,
        review: NativeReview<AssociationReview>,
    ) -> Result<NativeReaderOperation<AssociationRef>> {
        review
            .base
            .as_ref()
            .ok_or(Failure::Rejected)?
            .belongs_to(self)?;
        let (mut base, mut payload) = review.take()?;
        let cancel = base.lease.cancel.clone();
        Ok(launch(cancel, async move {
            let result = async {
                let Payload::Association {
                    selection,
                    transport,
                } = &mut payload
                else {
                    return Err(Failure::Rejected);
                };
                base.lease.check()?;
                base.lease.arm();
                let cap = base
                    .authority
                    .review_association(
                        &base.app()?,
                        &base.ctx,
                        selection.pin.clone(),
                        selection.target.clone(),
                        transport.peer(),
                    )
                    .await;
                let cap = base.lease.authority(cap)?;
                base.lease.arm();
                let pending = base
                    .authority
                    .approve_association_once(&base.app()?, &base.ctx, cap)
                    .await;
                let pending = base.lease.authority(pending)?;
                let association = match &pending.statement {
                    SignedReaderAdmin::InstallAssociation { signed } => {
                        signed.body.body_digest().map_err(|_| Failure::Rejected)?
                    }
                    _ => return Err(Failure::Rejected),
                };
                base.lease.check()?;
                base.lease.arm();
                let reply = transport
                    .install_signed_admin_once(&base.app()?, &base.ctx, &pending.statement)
                    .await;
                let reply = base.lease.transport(reply)?;
                base.lease.arm();
                let confirmed = base
                    .authority
                    .confirm_admin_applied(
                        &base.app()?,
                        &base.ctx,
                        pending.peer_digest,
                        transport.peer(),
                        &reply,
                    )
                    .await;
                base.lease.authority(confirmed)?;
                Ok(AssociationRef {
                    peer: pending.peer_digest,
                    association,
                })
            }
            .await;
            close_payload(&mut base, &mut payload).await;
            if base.lease.poisoned && result.is_ok() {
                Err(Failure::OutcomeUnknown)
            } else {
                result
            }
        }))
    }
    pub(super) fn reader_prepare_read(
        &self,
        selection: NativeReaderSelection,
        reference: AssociationRef,
    ) -> Result<NativeReaderOperation<NativeReview<ReadReview>>> {
        let lease = self.inner.reader_native.reserve()?;
        let cancel = lease.cancel.clone();
        let app = self.clone();
        Ok(launch(cancel, async move {
            let mut base = Base::capture(app, lease, &selection).await?;
            base.lease.arm();
            let registered = base
                .authority
                .registered_association(&base.app()?, &base.ctx, reference.peer, reference.association)
                .await;
            let registered = base.lease.authority(registered)?;
            base.lease.runtime.bind(
                base.lease.id,
                PeerKey::from_binding(&registered.transport_binding, &base.ctx),
                Role::Read,
            )?;
            let transport = base
                .verified(
                    &selection,
                    registered.transport_binding.clone(),
                    &registered.installed_head,
                )
                .await?;
            Ok(NativeReview::new(
                base,
                Payload::Read {
                    registered,
                    transport,
                },
            ))
        }))
    }
    pub(super) fn reader_read_once(
        &self,
        review: NativeReview<ReadReview>,
    ) -> Result<NativeReaderOperation<NativePrivateReaderData>> {
        review
            .base
            .as_ref()
            .ok_or(Failure::Rejected)?
            .belongs_to(self)?;
        let (mut base, mut payload) = review.take()?;
        let cancel = base.lease.cancel.clone();
        Ok(launch(cancel, async move {
            let result = run_read(&mut base, &mut payload).await;
            // Dispatch already closes; this observes the stored result and
            // closes all earlier failure paths. Do not retire ctx before the
            // final post-cleanup source/time/publication fence.
            if let Payload::Read { transport, .. } = &mut payload {
                base.lease.arm();
                base.lease.closed(transport.close().await);
            }
            let result = match result {
                Ok((dataset, expires)) if !base.lease.poisoned => {
                    base.lease.arm();
                    match base.final_fence(expires).await {
                        Ok(()) => {
                            base.lease.ack();
                            Ok((dataset, expires))
                        }
                        Err(e) => Err(e),
                    }
                }
                Ok(_) => Err(Failure::OutcomeUnknown),
                Err(e) => Err(e),
            };
            let (dataset, expires_at) = match result {
                Ok(value) => value,
                Err(error) => {
                    if base.retire_ctx { base.ctx.retire(); }
                    return Err(error);
                }
            };
            Ok(NativePrivateReaderData {
                dataset,
                expires_at,
                publication: base.ctx,
                _slot: base.lease,
            })
        }))
    }
    pub(super) fn reader_prepare_revoke(
        &self,
        selection: NativeReaderSelection,
        reference: AssociationRef,
        kind: ReaderRevokeSelection,
    ) -> Result<NativeReaderOperation<NativeReview<RevokeReview>>> {
        let lease = self.inner.reader_native.reserve()?;
        let cancel = lease.cancel.clone();
        let app = self.clone();
        Ok(launch(cancel, async move {
            let mut base = Base::capture(app, lease, &selection).await?;
            base.lease.arm();
            let registered = base
                .authority
                .registered_association(&base.app()?, &base.ctx, reference.peer, reference.association)
                .await;
            let registered = base.lease.authority(registered)?;
            base.lease.runtime.bind(
                base.lease.id,
                PeerKey::from_binding(&registered.transport_binding, &base.ctx),
                Role::Revoke,
            )?;
            let transport = base
                .verified(
                    &selection,
                    registered.transport_binding.clone(),
                    &registered.installed_head,
                )
                .await?;
            Ok(NativeReview::new(
                base,
                Payload::Revoke {
                    reference,
                    kind,
                    registered,
                    transport,
                },
            ))
        }))
    }
    pub(super) fn reader_revoke_once(
        &self,
        review: NativeReview<RevokeReview>,
    ) -> Result<NativeReaderOperation<()>> {
        review
            .base
            .as_ref()
            .ok_or(Failure::Rejected)?
            .belongs_to(self)?;
        let (mut base, mut payload) = review.take()?;
        let cancel = base.lease.cancel.clone();
        Ok(launch(cancel, async move {
            let result = async {
                let Payload::Revoke {
                    reference,
                    kind,
                    transport,
                    ..
                } = &mut payload
                else {
                    return Err(Failure::Rejected);
                };
                base.lease.check()?;
                // This is irreversible local cancellation before ANY signed
                // revoke I/O; own exact slot survives, user cancel does not.
                base.lease.runtime.begin_revoke(base.lease.id)?;
                let selection = match kind {
                    ReaderRevokeSelection::Association => ReaderRevokeSelection::Association,
                    ReaderRevokeSelection::OriginalDevice => ReaderRevokeSelection::OriginalDevice,
                };
                base.lease.arm();
                let cap = base
                    .authority
                    .review_revocation(
                        &base.app()?,
                        &base.ctx,
                        reference.peer,
                        reference.association,
                        selection,
                    )
                    .await;
                let cap = base.lease.authority(cap)?;
                base.lease.arm();
                let pending = base
                    .authority
                    .approve_revoke_once(&base.app()?, &base.ctx, cap)
                    .await;
                let pending = base.lease.authority(pending)?;
                base.lease.check()?;
                base.lease.arm();
                let reply = transport
                    .install_signed_admin_once(&base.app()?, &base.ctx, &pending.statement)
                    .await;
                let reply = base.lease.transport(reply)?;
                base.lease.arm();
                let confirmed = base
                    .authority
                    .confirm_admin_applied(
                        &base.app()?,
                        &base.ctx,
                        pending.peer_digest,
                        transport.peer(),
                        &reply,
                    )
                    .await;
                base.lease.authority(confirmed)
            }
            .await;
            close_payload(&mut base, &mut payload).await;
            if base.lease.poisoned && result.is_ok() {
                Err(Failure::OutcomeUnknown)
            } else {
                result
            }
        }))
    }
    /// Explicit known-chain reconciliation only. The getter must already
    /// validate equal signed/installed heads and a live association. Pending
    /// or entirely revoked state cannot be repaired/adopted by this method.
    pub(super) fn reader_prepare_reconcile(
        &self,
        selection: NativeReaderSelection,
        reference: AssociationRef,
    ) -> Result<NativeReaderOperation<NativeReview<ReconcileReview>>> {
        let lease = self.inner.reader_native.reserve()?;
        let cancel = lease.cancel.clone();
        let app = self.clone();
        Ok(launch(cancel, async move {
            let mut base = Base::capture(app, lease, &selection).await?;
            base.lease.arm();
            let registered = base
                .authority
                .registered_association(&base.app()?, &base.ctx, reference.peer, reference.association)
                .await;
            let registered = base.lease.authority(registered)?;
            base.lease.runtime.bind(
                base.lease.id,
                PeerKey::from_binding(&registered.transport_binding, &base.ctx),
                Role::Reconcile,
            )?;
            let transport = base
                .verified(
                    &selection,
                    registered.transport_binding.clone(),
                    &registered.installed_head,
                )
                .await?;
            Ok(NativeReview::new(
                base,
                Payload::Reconcile {
                    registered,
                    transport,
                },
            ))
        }))
    }
    pub(super) fn reader_reconcile_once(
        &self,
        review: NativeReview<ReconcileReview>,
    ) -> Result<NativeReaderOperation<()>> {
        review
            .base
            .as_ref()
            .ok_or(Failure::Rejected)?
            .belongs_to(self)?;
        let (mut base, mut payload) = review.take()?;
        let cancel = base.lease.cancel.clone();
        Ok(launch(cancel, async move {
            let result = async {
                let Payload::Reconcile {
                    registered,
                    transport,
                } = &mut payload
                else {
                    return Err(Failure::Rejected);
                };
                base.lease.check()?;
                base.lease.arm();
                let reply = transport.observe_head_once(&base.app()?, &base.ctx).await;
                let reply = base.lease.transport(reply)?;
                if reply
                    .metadata_for(transport.peer())
                    .map_err(|_| Failure::Rejected)?
                    != &registered.installed_head
                {
                    return Err(Failure::RevokePending);
                }
                base.lease.arm();
                base.lease.closed(transport.close().await);
                if base.lease.poisoned {
                    return Err(Failure::OutcomeUnknown);
                }
                base.lease.arm();
                base.final_fence(registered.association.body.expires_at.seconds())
                    .await?;
                base.lease.ack();
                base.lease
                    .runtime
                    .unblock_after_explicit_reconcile(base.lease.id)
            }
            .await;
            close_payload(&mut base, &mut payload).await;
            result
        }))
    }
}

async fn run_read(
    base: &mut Base,
    payload: &mut Payload,
) -> Result<(response::PrivateDataset, u64)> {
    let Payload::Read {
        registered,
        transport,
    } = payload
    else {
        return Err(Failure::Rejected);
    };
    let app = base.app()?;
    let Base {
        ctx,
        authority,
        lease,
        ..
    } = base;
    let app = &app;
    lease.check()?;
    let association = registered
        .association
        .body
        .body_digest()
        .map_err(|_| Failure::Rejected)?;
    let operation = AgentReaderOperationId::new(&Uuid::new_v4().to_string())
        .map_err(|_| Failure::Unavailable)?;
    lease.arm();
    let reserved = authority
        .reserve_action_once(app, ctx, registered.peer_digest, association, operation)
        .await;
    let mut action = Some(lease.authority(reserved)?);
    let result = async {
        lease.check()?;
        lease.arm();
        let request = authority
            .challenge_request_once(
                app,
                ctx,
                action.as_mut().ok_or(Failure::Rejected)?,
                transport.peer(),
            )
            .await;
        let request = lease.authority(request)?;
        lease.check()?;
        lease.arm();
        let challenge = transport.challenge_once(app, ctx, &request).await;
        let challenge = lease.transport(challenge)?;
        lease.check()?;
        lease.arm();
        let issued = authority
            .issue_grant_once(
                app,
                ctx,
                action.take().ok_or(Failure::Rejected)?,
                transport.peer(),
                &challenge,
            )
            .await;
        let issued = lease.authority(issued)?;
        action = Some(issued.retirement);
        let packet = issued.packet;
        let expires = packet.grant.body.expires_at.seconds();
        let packet_bytes =
            zeroize::Zeroizing::new(serde_json::to_vec(&packet).map_err(|_| Failure::Rejected)?);
        lease.check()?;
        lease.arm();
        let prepared = ctx
            .with_held(app, |held| {
                lease
                    .runtime
                    .fence_locked(lease.id, || {
                        lease.check_without_lock()?;
                        let now = ctx.now().map_err(|_| Failure::Expired)?;
                        let expected = AgentReaderExpectedV1 {
                            issuer_pin: registered.issuer_pin.clone(),
                            current_head: packet.grant.body.head.clone(),
                            association_body: registered.association.body.clone(),
                            target: registered.target.clone(),
                        };
                        let input =
                            AgentReaderValidatedInputsV1::from_packet(&packet_bytes, expected, now)
                                .map_err(|_| Failure::Rejected)?;
                        let proof = held
                            .identity
                            .sign_agent_reader_proof_v1(
                                input,
                                ctx.now().map_err(|_| Failure::Expired)?,
                            )
                            .map_err(|_| Failure::Rejected)?;
                        let bundle = AgentReaderBundleV1 {
                            association: packet.association.clone(),
                            grant: packet.grant.clone(),
                            proof,
                            freshness: packet.freshness.clone(),
                        };
                        PreparedReaderGet::after_original_device_sign(
                            &held,
                            transport.peer(),
                            &registered.target,
                            bundle,
                        )
                        .map_err(|_| Failure::Rejected)
                    })
                    .map_err(|_| {
                        crate::AppError::invalid("agent_reader", "reader_signing_unavailable")
                    })
            })
            .await
            .map_err(|_| {
                lease.poison();
                Failure::OutcomeUnknown
            })?;
        lease.ack();
        lease.check()?;
        lease.arm();
        let response = transport.dispatch_once(app, ctx, prepared).await;
        let response = lease.transport(response)?;
        lease.closed(transport.cleanup_observation());
        if response.status() != 200
            || response.expires_at() != expires
            || response.target_hash()
                != registered
                    .target
                    .descriptor_hash()
                    .map_err(|_| Failure::Rejected)?
        {
            return Err(Failure::ResponseInvalid);
        }
        let dataset = response::decode(response.private_body(), &registered.target)?;
        Ok((dataset, expires))
    }
    .await;
    // Best effort durable retirement while ctx is live. On error from a
    // consumed grant-creation cap, the reservation/tombstone is still burned;
    // no fake claim that Retired was persisted on every failure path.
    if let Some(action) = action {
        lease.arm();
        let retired = authority.retire_local(app, ctx, action).await;
        let cleanup = lease.authority(retired);
        if result.is_ok() {
            cleanup?;
        }
    }
    result
}
