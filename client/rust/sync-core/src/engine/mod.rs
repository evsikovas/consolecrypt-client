//! Per-vault sync engine implementing the ADR-0003 client algorithm:
//!
//! ```text
//! cycle:  [rollback recovery if pending] [snapshot if never completed]
//!         [pull first if something is queued — screens for a restored server]
//!         repeat: push outbox in batches (≤ MAX_PUSH_BATCH, same mutation_id on retry)
//!                 conflicts? → pull → resolve (policy table) → re-queue
//!         pull changes?after=last_sequence (paged, verified, applied transactionally)
//! worker: runs cycles on local changes, WS `vault_changed`, reconnects and a
//!         periodic timer; exponential backoff with jitter on errors,
//!         honouring Retry-After; stops on revocation / lost session.
//! ```
//!
//! Server rollback (restore from an older backup, ADR-0103 addendum): every
//! page, push response and observed `VaultInfo` is checked for a changed
//! vault epoch or sequences going backwards; on evidence the cycle switches
//! to [`recovery`] (full listing → reconcile → re-push) before continuing.

pub(crate) mod conflict;
mod recovery;
mod validate;

use crate::api::ApiClient;
use crate::backoff::BackoffConfig;
use crate::codec::CodecError;
use crate::error::{Disposition, StopReason, SyncError};
use crate::events::{ChangeOrigin, SyncEvent};
use crate::store::ObjectStore;
use crate::ws::WsEvent;
use cc_protocol::events::ServerEvent;
use cc_protocol::limits::{
    DEFAULT_PAGE_LIMIT, MAX_PAGE_LIMIT, MAX_PUSH_BATCH, MAX_PUSH_BODY_BYTES,
};
use cc_protocol::sync::{Change, ChangesQuery, MutationResult, PushRequest, SnapshotQuery};
use cc_protocol::{DeviceId, ErrorCode, ObjectId, Timestamp, VaultId};
use cc_storage_core::{
    OutboxEntry, PageCursor, PageOutcome, ProfileKind, PushOutcome, RemoteApplyOutcome,
    RemoteChange, RollbackReason, StorageError, SyncCursor,
};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{broadcast, watch, Notify};
use tokio::task::JoinHandle;

pub use conflict::{
    make_conflict_copy, payload_updated_at, strategy_for, ConflictStrategy, CONFLICT_COPY_SUFFIX,
};

/// Push request body budget (leave headroom under the server limit).
const PUSH_BYTES_BUDGET: usize = MAX_PUSH_BODY_BYTES - 512 * 1024;

/// Rollback recoveries started within one cycle before giving up (a server
/// that keeps flipping its epoch is retried with backoff).
const MAX_RECOVERIES_PER_CYCLE: usize = 3;

/// Whether a network step finished normally or found evidence of a server
/// rollback (recovery is then pending in storage).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[must_use]
enum Flow {
    Done,
    Rollback,
}

/// Engine configuration.
#[derive(Debug, Clone)]
pub struct SyncEngineConfig {
    /// This installation's device id (must match the access token's device).
    pub device_id: DeviceId,
    /// Mutations per push (clamped to `1..=MAX_PUSH_BATCH`).
    pub push_batch_size: usize,
    /// Page size for `changes` / `snapshot` (clamped to `1..=MAX_PAGE_LIMIT`).
    pub page_limit: u32,
    /// Retry schedule for the background worker.
    pub backoff: BackoffConfig,
    /// Background sync interval when idle (`None` = only on triggers).
    pub periodic_interval: Option<Duration>,
    /// Push → resolve rounds per cycle.
    pub max_rounds_per_cycle: usize,
}

impl SyncEngineConfig {
    /// Defaults for `device_id`.
    pub fn new(device_id: DeviceId) -> Self {
        Self {
            device_id,
            push_batch_size: MAX_PUSH_BATCH,
            page_limit: DEFAULT_PAGE_LIMIT,
            backoff: BackoffConfig::default(),
            periodic_interval: Some(Duration::from_secs(300)),
            max_rounds_per_cycle: 4,
        }
    }
}

/// Coarse engine state for the UI.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SyncPhase {
    Idle,
    Syncing,
    /// Server unreachable; retrying with backoff. Local work continues.
    Offline,
    /// Last attempt failed (server error, malformed response …); retrying.
    Error,
    /// Stopped for good (see `stop_reason`).
    Stopped,
}

/// Engine status snapshot.
#[derive(Debug, Clone, PartialEq)]
pub struct SyncStatus {
    pub vault_id: VaultId,
    pub phase: SyncPhase,
    /// Outbox entries waiting to be pushed.
    pub pending: u64,
    /// Unresolved conflicts.
    pub conflicts: u64,
    /// Permanently rejected mutations.
    pub failed: u64,
    pub last_error: Option<String>,
    pub stop_reason: Option<StopReason>,
    pub last_sync_at: Option<Timestamp>,
    pub last_sequence: i64,
    pub server_latest_sequence: i64,
    /// Delay until the next automatic retry.
    pub next_retry_in: Option<Duration>,
    /// Set while recovering from a server rollback (restore from an older
    /// backup): the vault is being re-downloaded and local objects the
    /// server lost are re-uploaded. Local work continues meanwhile.
    pub rollback_recovery: Option<RollbackReason>,
}

/// What one cycle did.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SyncReport {
    /// Mutations accepted (including replays).
    pub pushed: usize,
    /// Accepted results that were idempotent replays.
    pub replayed: usize,
    /// Conflict results received.
    pub conflicts: usize,
    /// Conflicts resolved automatically.
    pub resolved: usize,
    /// Remote changes written to `objects`.
    pub pulled: usize,
    /// Remote changes stashed because of pending local edits.
    pub stashed: usize,
    pub snapshot_pages: usize,
    pub change_pages: usize,
    /// Mutations moved to `Failed`.
    pub failed: usize,
    /// Server rollback recoveries completed in this cycle.
    pub rollback_recoveries: usize,
    /// Live local objects re-queued by a rollback recovery (lost by the
    /// server, or held there at an older revision).
    pub repushed: usize,
    /// Local tombstones re-queued by a rollback recovery.
    pub tombstones_reapplied: usize,
    /// Cursor after the cycle.
    pub last_sequence: i64,
}

struct EngineInner {
    store: ObjectStore,
    api: ApiClient,
    cfg: SyncEngineConfig,
    status: watch::Sender<SyncStatus>,
    urgent: Notify,
    cycle: tokio::sync::Mutex<()>,
    shutdown: watch::Sender<bool>,
    worker: std::sync::Mutex<Option<JoinHandle<()>>>,
    stopped: std::sync::Mutex<Option<StopReason>>,
}

/// Sync engine for one vault. Cheap to clone.
#[derive(Clone)]
pub struct SyncEngine {
    inner: Arc<EngineInner>,
}

impl std::fmt::Debug for SyncEngine {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SyncEngine")
            .field("vault_id", &self.vault_id())
            .field("phase", &self.inner.status.borrow().phase)
            .finish_non_exhaustive()
    }
}

fn malformed_if_invalid(e: StorageError) -> SyncError {
    match e {
        StorageError::Invalid(m) => SyncError::MalformedResponse(m),
        e => SyncError::Storage(e),
    }
}

/// Test-decrypt server bodies through the codec: caches the KEK class and
/// collects `(object, revision)` of ciphertexts that fail verification.
fn verify_changes(
    codec: &dyn crate::codec::ObjectCodec,
    changes: Vec<Change>,
) -> (Vec<RemoteChange>, Vec<(ObjectId, i64)>) {
    let mut warnings = Vec::new();
    let remote = changes
        .into_iter()
        .map(|c| {
            let hint =
                c.body
                    .as_ref()
                    .and_then(|b| match codec.decrypt(c.object_id, c.revision, b) {
                        Ok(p) => Some(p.object.kind().kek_class()),
                        Err(CodecError::Locked) => None,
                        Err(_) => {
                            warnings.push((c.object_id, c.revision));
                            None
                        }
                    });
            RemoteChange {
                object_id: c.object_id,
                revision: c.revision,
                sequence: c.sequence,
                deleted: c.deleted,
                body: c.body,
                kek_class_hint: hint,
                writer_device_id: c.writer_device_id,
                updated_at: c.updated_at,
            }
        })
        .collect();
    (remote, warnings)
}

fn is_request_rejected(e: &SyncError) -> bool {
    matches!(e, SyncError::Api(a) if a.is_code(ErrorCode::BadRequest) || a.is_code(ErrorCode::PayloadTooLarge))
}

async fn sleep_opt(d: Option<Duration>) {
    match d {
        Some(d) => tokio::time::sleep(d).await,
        None => std::future::pending::<()>().await,
    }
}

impl SyncEngine {
    /// Create an engine for `store`'s vault. Fails with
    /// [`SyncError::LocalProfile`] in a local-only profile.
    pub async fn new(
        store: ObjectStore,
        api: ApiClient,
        mut cfg: SyncEngineConfig,
    ) -> Result<Self, SyncError> {
        let kind = store.storage().read(|tx| tx.profile_kind()).await?;
        if kind == ProfileKind::Local {
            return Err(SyncError::LocalProfile);
        }
        cfg.push_batch_size = cfg.push_batch_size.clamp(1, MAX_PUSH_BATCH);
        cfg.page_limit = cfg.page_limit.clamp(1, MAX_PAGE_LIMIT);
        let vault_id = store.vault_id();
        let (status, _) = watch::channel(SyncStatus {
            vault_id,
            phase: SyncPhase::Idle,
            pending: 0,
            conflicts: 0,
            failed: 0,
            last_error: None,
            stop_reason: None,
            last_sync_at: None,
            last_sequence: 0,
            server_latest_sequence: 0,
            next_retry_in: None,
            rollback_recovery: None,
        });
        let (shutdown, _) = watch::channel(false);
        let engine = Self {
            inner: Arc::new(EngineInner {
                store,
                api,
                cfg,
                status,
                urgent: Notify::new(),
                cycle: tokio::sync::Mutex::new(()),
                shutdown,
                worker: std::sync::Mutex::new(None),
                stopped: std::sync::Mutex::new(None),
            }),
        };
        engine.refresh_status().await?;
        Ok(engine)
    }

    /// Vault served by this engine.
    pub fn vault_id(&self) -> VaultId {
        self.inner.store.vault_id()
    }

    /// The local object store (mutations, decrypting reads, events).
    pub fn store(&self) -> &ObjectStore {
        &self.inner.store
    }

    /// Local create/update (see [`ObjectStore::put`]); wakes the worker.
    pub async fn put(
        &self,
        payload: cc_models::ObjectPayload,
    ) -> Result<cc_storage_core::LocalMutationOutcome, SyncError> {
        let r = self.inner.store.put(payload).await?;
        self.refresh_status().await?;
        Ok(r)
    }

    /// Local delete (see [`ObjectStore::delete`]); wakes the worker.
    pub async fn delete(
        &self,
        object_id: ObjectId,
    ) -> Result<cc_storage_core::LocalMutationOutcome, SyncError> {
        let r = self.inner.store.delete(object_id).await?;
        self.refresh_status().await?;
        Ok(r)
    }

    /// Current status.
    pub fn status(&self) -> SyncStatus {
        self.inner.status.borrow().clone()
    }

    /// Status updates.
    pub fn subscribe_status(&self) -> watch::Receiver<SyncStatus> {
        self.inner.status.subscribe()
    }

    /// Object / conflict / lifecycle events.
    pub fn subscribe_events(&self) -> broadcast::Receiver<SyncEvent> {
        self.inner.store.subscribe()
    }

    /// Whether the engine stopped for good, and why.
    pub fn stop_reason(&self) -> Option<StopReason> {
        *self.lock_stopped()
    }

    fn lock_stopped(&self) -> std::sync::MutexGuard<'_, Option<StopReason>> {
        self.inner.stopped.lock().unwrap_or_else(|p| p.into_inner())
    }

    /// Ask the background worker to sync as soon as possible (bypasses a
    /// pending backoff delay).
    pub fn trigger(&self) {
        self.inner.urgent.notify_one();
    }

    // ---- lifecycle ------------------------------------------------------------------

    /// Start the background worker (idempotent).
    pub fn start(&self) {
        let mut w = self.inner.worker.lock().unwrap_or_else(|p| p.into_inner());
        if w.as_ref().is_some_and(|h| !h.is_finished()) || self.stop_reason().is_some() {
            return;
        }
        let this = self.clone();
        *w = Some(tokio::spawn(async move { this.run_worker().await }));
    }

    /// Stop the worker and wait for a running cycle to finish (bounded).
    /// Afterwards `sync_now` fails with `Stopped(Shutdown)`.
    pub async fn stop(&self) {
        self.halt(StopReason::Shutdown);
        let handle = self
            .inner
            .worker
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .take();
        if let Some(mut h) = handle {
            if tokio::time::timeout(Duration::from_secs(5), &mut h)
                .await
                .is_err()
            {
                // Every storage step is atomic, so aborting mid-cycle is safe.
                h.abort();
            }
        }
        let _ = tokio::time::timeout(Duration::from_secs(5), self.inner.cycle.lock()).await;
    }

    /// Mark the engine stopped (first reason wins), notify, stop the worker.
    fn halt(&self, reason: StopReason) {
        {
            let mut s = self.lock_stopped();
            if s.is_some() {
                return;
            }
            *s = Some(reason);
        }
        tracing::info!(vault_id = %self.vault_id(), %reason, "sync engine stopped");
        let _ = self.inner.shutdown.send(true);
        self.inner.status.send_modify(|st| {
            st.phase = SyncPhase::Stopped;
            st.stop_reason = Some(reason);
            st.next_retry_in = None;
        });
        self.inner.store.emit(SyncEvent::Stopped {
            vault_id: self.vault_id(),
            reason,
        });
    }

    /// Follow a WebSocket event stream (see [`crate::EventStream`]):
    /// `vault_changed` with a newer sequence and reconnects trigger a pull;
    /// revocation of this device stops the engine.
    pub fn attach_events(&self, mut rx: broadcast::Receiver<WsEvent>) -> JoinHandle<()> {
        let this = self.clone();
        tokio::spawn(async move {
            let mut shutdown = this.inner.shutdown.subscribe();
            loop {
                if this.stop_reason().is_some() {
                    break;
                }
                tokio::select! {
                    _ = shutdown.changed() => break,
                    ev = rx.recv() => match ev {
                        Ok(ev) => this.handle_ws_event(&ev),
                        Err(broadcast::error::RecvError::Lagged(_)) => this.trigger(),
                        Err(broadcast::error::RecvError::Closed) => break,
                    },
                }
            }
        })
    }

    /// React to one WebSocket event.
    pub fn handle_ws_event(&self, ev: &WsEvent) {
        match ev {
            WsEvent::Connected => self.trigger(),
            WsEvent::Event(e) => self.handle_server_event(e),
            WsEvent::Terminated { reason } => match reason {
                StopReason::DeviceRevoked | StopReason::ReauthRequired => self.halt(*reason),
                _ => {}
            },
            WsEvent::Disconnected { .. } => {}
        }
    }

    /// React to one server event.
    pub fn handle_server_event(&self, ev: &ServerEvent) {
        match ev {
            ServerEvent::VaultChanged {
                vault_id,
                latest_sequence,
            } if *vault_id == self.vault_id() => {
                // Lower than our cursor may mean a restored server: the pull
                // checks and starts a rollback recovery if so.
                if *latest_sequence != self.inner.status.borrow().last_sequence {
                    self.trigger();
                }
            }
            ServerEvent::DeviceRevoked { device_id } if *device_id == self.inner.cfg.device_id => {
                self.halt(StopReason::DeviceRevoked);
            }
            _ => {}
        }
    }

    async fn run_worker(self) {
        let mut shutdown = self.inner.shutdown.subscribe();
        let mut attempt: u32 = 0;
        let mut wait: Option<Duration> = Some(Duration::ZERO);
        loop {
            if *shutdown.borrow() {
                break;
            }
            let in_backoff = attempt > 0;
            let periodic = if wait.is_none() {
                self.inner.cfg.periodic_interval
            } else {
                None
            };
            tokio::select! {
                biased;
                r = shutdown.changed() => {
                    if r.is_err() || *shutdown.borrow() { break; }
                    continue;
                }
                _ = self.inner.urgent.notified() => {}
                _ = self.inner.store.inner.local_changes.notified(), if !in_backoff => {}
                _ = sleep_opt(wait) => {}
                _ = sleep_opt(periodic) => {}
            }
            match self.sync_now().await {
                Ok(_) => {
                    attempt = 0;
                    wait = None;
                }
                Err(e) => match e.disposition() {
                    Disposition::Stop(_) => break,
                    Disposition::Retry { after, .. } => {
                        attempt = attempt.saturating_add(1);
                        let d = self.inner.cfg.backoff.delay(attempt, after);
                        tracing::debug!(
                            attempt,
                            delay_ms = d.as_millis() as u64,
                            "sync retry scheduled"
                        );
                        self.inner.status.send_modify(|s| s.next_retry_in = Some(d));
                        wait = Some(d);
                    }
                },
            }
        }
    }

    // ---- one cycle -------------------------------------------------------------------

    /// Run one full sync cycle now (push, resolve, pull). Updates status;
    /// stops the engine on fatal errors (revoked device, lost session …).
    pub async fn sync_now(&self) -> Result<SyncReport, SyncError> {
        if let Some(r) = self.stop_reason() {
            return Err(SyncError::Stopped(r));
        }
        let result = self.cycle().await;
        match &result {
            Ok(_) => {
                self.inner.status.send_modify(|s| {
                    s.phase = SyncPhase::Idle;
                    s.last_error = None;
                    s.next_retry_in = None;
                });
            }
            Err(e) => {
                let msg = e.to_string();
                let vault_id = self.vault_id();
                let _ = self
                    .inner
                    .store
                    .storage()
                    .write(move |tx| {
                        tx.record_sync_attempt(vault_id, chrono::Utc::now(), Some(&msg))
                    })
                    .await;
                match e.disposition() {
                    Disposition::Stop(reason) => self.halt(reason),
                    Disposition::Retry { offline, .. } => {
                        tracing::debug!(error = %e, offline, "sync cycle failed");
                        let msg = e.to_string();
                        self.inner.status.send_modify(|s| {
                            s.phase = if offline {
                                SyncPhase::Offline
                            } else {
                                SyncPhase::Error
                            };
                            s.last_error = Some(msg);
                        });
                    }
                }
            }
        }
        let _ = self.refresh_status().await;
        result
    }

    async fn cycle(&self) -> Result<SyncReport, SyncError> {
        let _guard = self.inner.cycle.lock().await;
        if let Some(r) = self.stop_reason() {
            return Err(SyncError::Stopped(r));
        }
        self.inner
            .status
            .send_modify(|s| s.phase = SyncPhase::Syncing);
        let mut report = SyncReport::default();
        let mut recoveries = 0;
        'cycle: loop {
            if self.cursor().await?.recovery.is_some() {
                recoveries += 1;
                if recoveries > MAX_RECOVERIES_PER_CYCLE {
                    return Err(SyncError::MalformedResponse(
                        "server keeps rolling back its state".into(),
                    ));
                }
                self.recover(&mut report).await?;
            }
            if !self.cursor().await?.snapshot_complete
                && self.snapshot(&mut report).await? == Flow::Rollback
            {
                continue 'cycle;
            }
            // Before pushing, make sure the server is still the one we know
            // (epoch / sequence check on a pull): pushing an edit onto a
            // restored server could silently overwrite another device's
            // post-restore write that happens to carry our base revision.
            if self.has_queued().await? && self.pull(&mut report).await? == Flow::Rollback {
                continue 'cycle;
            }
            for _ in 0..self.inner.cfg.max_rounds_per_cycle.max(1) {
                if self.push_all(&mut report).await? == Flow::Rollback {
                    continue 'cycle;
                }
                let vault_id = self.vault_id();
                let conflicted = self
                    .inner
                    .store
                    .storage()
                    .read(move |tx| tx.conflicted_objects(vault_id))
                    .await?;
                if conflicted.is_empty() {
                    break;
                }
                if self.pull(&mut report).await? == Flow::Rollback {
                    continue 'cycle;
                }
                if !self.resolve(&conflicted, &mut report).await? {
                    break;
                }
            }
            if self.pull(&mut report).await? == Flow::Rollback {
                continue 'cycle;
            }
            break;
        }
        let vault_id = self.vault_id();
        let cursor = self
            .inner
            .store
            .storage()
            .write(move |tx| {
                tx.record_sync_attempt(vault_id, chrono::Utc::now(), None)?;
                tx.get_sync_cursor(vault_id)
            })
            .await?;
        report.last_sequence = cursor.last_sequence;
        tracing::debug!(vault_id = %vault_id, ?report, "sync cycle done");
        Ok(report)
    }

    async fn has_queued(&self) -> Result<bool, SyncError> {
        let vault_id = self.vault_id();
        Ok(self
            .inner
            .store
            .storage()
            .read(move |tx| tx.outbox_counts(vault_id))
            .await?
            .queued
            > 0)
    }

    async fn cursor(&self) -> Result<SyncCursor, SyncError> {
        let vault_id = self.vault_id();
        Ok(self
            .inner
            .store
            .storage()
            .read(move |tx| tx.get_sync_cursor(vault_id))
            .await?)
    }

    async fn refresh_status(&self) -> Result<(), SyncError> {
        let vault_id = self.vault_id();
        let (counts, cursor) = self
            .inner
            .store
            .storage()
            .read(move |tx| {
                Ok::<_, StorageError>((tx.outbox_counts(vault_id)?, tx.get_sync_cursor(vault_id)?))
            })
            .await?;
        self.inner.status.send_modify(|s| {
            s.pending = counts.queued;
            s.conflicts = counts.conflict;
            s.failed = counts.failed;
            s.last_sequence = cursor.last_sequence;
            s.server_latest_sequence = cursor.server_latest_sequence;
            s.last_sync_at = cursor.last_sync_at;
            s.rollback_recovery = cursor.recovery.as_ref().map(|r| r.reason);
        });
        Ok(())
    }

    // ---- push -------------------------------------------------------------------------

    async fn push_all(&self, report: &mut SyncReport) -> Result<Flow, SyncError> {
        let vault_id = self.vault_id();
        let max = self.inner.cfg.push_batch_size;
        // Each iteration moves every batch entry out of `Queued` (accepted,
        // conflicted or failed), so this terminates; the cap is a safety net.
        for _ in 0..100_000 {
            let batch = self
                .inner
                .store
                .storage()
                .write(move |tx| tx.outbox_begin_batch(vault_id, max, PUSH_BYTES_BUDGET))
                .await?;
            if batch.is_empty() {
                return Ok(Flow::Done);
            }
            match self.push_batch(&batch, report).await {
                Ok(Flow::Done) => {}
                Ok(Flow::Rollback) => return Ok(Flow::Rollback),
                Err(e) if is_request_rejected(&e) => {
                    // Isolate the offending mutation(s); keep the rest flowing.
                    for entry in &batch {
                        match self.push_batch(std::slice::from_ref(entry), report).await {
                            Ok(Flow::Done) => {}
                            Ok(Flow::Rollback) => return Ok(Flow::Rollback),
                            Err(e) if is_request_rejected(&e) => {
                                self.mark_failed(entry, &e).await?;
                                report.failed += 1;
                            }
                            Err(e) => return Err(e),
                        }
                    }
                }
                Err(e) => {
                    self.note_push_error(&batch, &e).await;
                    return Err(e);
                }
            }
        }
        Ok(Flow::Done)
    }

    async fn mark_failed(&self, entry: &OutboxEntry, e: &SyncError) -> Result<(), SyncError> {
        let id = entry.mutation_id;
        let msg = e.to_string();
        tracing::warn!(object_id = %entry.object_id, error = %msg, "mutation rejected by server");
        self.inner
            .store
            .storage()
            .write(move |tx| tx.outbox_record_error(id, &msg, true))
            .await?;
        Ok(())
    }

    async fn note_push_error(&self, batch: &[OutboxEntry], e: &SyncError) {
        let ids: Vec<_> = batch.iter().map(|e| e.mutation_id).collect();
        let msg = e.to_string();
        let _ = self
            .inner
            .store
            .storage()
            .write(move |tx| {
                for id in ids {
                    tx.outbox_record_error(id, &msg, false)?;
                }
                Ok::<_, StorageError>(())
            })
            .await;
    }

    async fn push_batch(
        &self,
        batch: &[OutboxEntry],
        report: &mut SyncReport,
    ) -> Result<Flow, SyncError> {
        let vault_id = self.vault_id();
        let req = PushRequest {
            vault_id,
            device_id: self.inner.cfg.device_id,
            mutations: batch.iter().map(OutboxEntry::to_mutation).collect(),
        };
        let resp = self.inner.api.push(&req).await?;
        if resp.results.len() != batch.len()
            || resp
                .results
                .iter()
                .zip(batch)
                .any(|(r, e)| r.mutation_id() != e.mutation_id)
        {
            return Err(SyncError::MalformedResponse(
                "push results do not match the request".into(),
            ));
        }
        let latest = resp.latest_sequence;
        let results = resp.results.clone();
        let (outcomes, rollback) = self
            .inner
            .store
            .storage()
            .write(move |tx| {
                // Judge the response against what we held before it.
                let evidence = tx.push_rollback_evidence(vault_id, latest, &results)?;
                let outcomes = tx.apply_push_results(vault_id, latest, &results)?;
                let rollback = match evidence {
                    Some(reason) => {
                        Some((reason, tx.begin_rollback_recovery(vault_id, reason, None)?))
                    }
                    None => None,
                };
                Ok::<_, StorageError>((outcomes, rollback))
            })
            .await
            .map_err(malformed_if_invalid)?;
        for (o, r) in outcomes.iter().zip(&resp.results) {
            match *o {
                PushOutcome::Committed {
                    object_id,
                    applied_remote,
                    ..
                } => {
                    report.pushed += 1;
                    if matches!(r, MutationResult::Accepted { replayed: true, .. }) {
                        report.replayed += 1;
                    }
                    if let Some((revision, deleted)) = applied_remote {
                        self.inner.store.emit(SyncEvent::ObjectChanged {
                            vault_id,
                            object_id,
                            revision,
                            deleted,
                            origin: ChangeOrigin::Remote,
                        });
                    }
                }
                PushOutcome::Conflicted { .. } => report.conflicts += 1,
                PushOutcome::Unknown { .. } => {}
            }
        }
        if let Some((reason, started)) = rollback {
            self.rollback_detected(reason, started);
            return Ok(Flow::Rollback);
        }
        Ok(Flow::Done)
    }

    // ---- pull ---------------------------------------------------------------------------

    async fn pull(&self, report: &mut SyncReport) -> Result<Flow, SyncError> {
        let vault_id = self.vault_id();
        let limit = self.inner.cfg.page_limit;
        let mut resnapshots = 0;
        for _ in 0..1_000_000 {
            let cursor = self.cursor().await?;
            let after = cursor.last_sequence;
            let q = ChangesQuery {
                vault_id,
                after,
                limit: Some(limit),
            };
            let resp = match self.inner.api.changes(&q).await {
                Ok(r) => r,
                Err(e) if e.is_code(ErrorCode::Gone) && resnapshots == 0 => {
                    // Below the tombstone horizon: start over with a snapshot.
                    resnapshots += 1;
                    tracing::info!(vault_id = %vault_id, "changes cursor gone; re-snapshotting");
                    self.inner
                        .store
                        .storage()
                        .write(move |tx| tx.reset_for_resnapshot(vault_id))
                        .await?;
                    if self.snapshot(report).await? == Flow::Rollback {
                        return Ok(Flow::Rollback);
                    }
                    continue;
                }
                Err(e) => return Err(e.into()),
            };
            if let Some(reason) = cursor.rollback_evidence(resp.epoch, resp.latest_sequence) {
                self.begin_recovery(reason, resp.epoch).await?;
                return Ok(Flow::Rollback);
            }
            validate::changes_page(&resp, after, limit)?;
            let page = PageCursor::Changes {
                next_after: resp.next_after,
                latest_sequence: resp.latest_sequence,
            };
            let has_more = resp.has_more;
            self.apply_page(resp.changes, page, resp.epoch, report)
                .await?;
            report.change_pages += 1;
            if !has_more {
                return Ok(Flow::Done);
            }
        }
        Ok(Flow::Done)
    }

    async fn snapshot(&self, report: &mut SyncReport) -> Result<Flow, SyncError> {
        let vault_id = self.vault_id();
        let limit = self.inner.cfg.page_limit;
        for _ in 0..1_000_000 {
            let cursor = self.cursor().await?;
            if cursor.snapshot_complete {
                return Ok(Flow::Done);
            }
            let q = SnapshotQuery {
                vault_id,
                cursor: cursor.snapshot_cursor,
                limit: Some(limit),
            };
            let resp = self.inner.api.snapshot(&q).await?;
            if let Some(reason) = cursor.rollback_evidence(resp.epoch, resp.latest_sequence) {
                self.begin_recovery(reason, resp.epoch).await?;
                return Ok(Flow::Rollback);
            }
            validate::snapshot_page(&resp, q.cursor, limit)?;
            let page = PageCursor::Snapshot {
                next_cursor: resp.next_cursor,
                latest_sequence: resp.latest_sequence,
            };
            let out = self
                .apply_page(resp.objects, page, resp.epoch, report)
                .await?;
            report.snapshot_pages += 1;
            if out.snapshot_completed {
                self.inner
                    .store
                    .emit(SyncEvent::SnapshotCompleted { vault_id });
                return Ok(Flow::Done);
            }
        }
        Ok(Flow::Done)
    }

    /// Verify ciphertexts through the codec (KEK class hint, integrity
    /// warnings) and apply the page + cursor in one transaction.
    async fn apply_page(
        &self,
        changes: Vec<Change>,
        page: PageCursor,
        epoch: Option<uuid::Uuid>,
        report: &mut SyncReport,
    ) -> Result<PageOutcome, SyncError> {
        let vault_id = self.vault_id();
        let codec = self.inner.store.codec().clone();
        let (outcome, warnings) = self
            .inner
            .store
            .storage()
            .write(move |tx| -> Result<_, SyncError> {
                let (remote, warnings) = verify_changes(codec.as_ref(), changes);
                tx.note_server_epoch(vault_id, epoch)
                    .map_err(malformed_if_invalid)?;
                let out = tx
                    .apply_remote_page(vault_id, &remote, page)
                    .map_err(malformed_if_invalid)?;
                Ok((out, warnings))
            })
            .await?;
        for c in &outcome.changes {
            match *c {
                RemoteApplyOutcome::Applied {
                    object_id,
                    revision,
                    deleted,
                } => {
                    report.pulled += 1;
                    self.inner.store.emit(SyncEvent::ObjectChanged {
                        vault_id,
                        object_id,
                        revision,
                        deleted,
                        origin: ChangeOrigin::Remote,
                    });
                }
                RemoteApplyOutcome::Stashed { .. } => report.stashed += 1,
                RemoteApplyOutcome::Skipped { .. } => {}
            }
        }
        for object_id in &outcome.removed {
            self.inner.store.emit(SyncEvent::ObjectRemoved {
                vault_id,
                object_id: *object_id,
            });
        }
        self.emit_integrity_warnings(warnings);
        if let Some(c) = &outcome.cursor {
            let (last, latest) = (c.last_sequence, c.server_latest_sequence);
            self.inner.status.send_modify(|s| {
                s.last_sequence = last;
                s.server_latest_sequence = latest;
            });
        }
        Ok(outcome)
    }

    // ---- conflicts -------------------------------------------------------------------------

    async fn resolve(&self, ids: &[ObjectId], report: &mut SyncReport) -> Result<bool, SyncError> {
        let vault_id = self.vault_id();
        let mut progressed = false;
        for &object_id in ids {
            let codec = self.inner.store.codec().clone();
            let r = self
                .inner
                .store
                .storage()
                .write(move |tx| -> Result<_, SyncError> {
                    let Some(ctx) = tx.conflict_context(vault_id, object_id)? else {
                        return Ok(None);
                    };
                    let Some(d) = conflict::decide(&ctx, codec.as_ref())? else {
                        return Ok(None);
                    };
                    let out = tx.apply_resolution(vault_id, object_id, d.resolution)?;
                    Ok(Some((d.label, d.kek_class, out)))
                })
                .await;
            match r {
                Ok(Some((label, kek_class, out))) => {
                    progressed = true;
                    report.resolved += 1;
                    tracing::info!(%object_id, resolution = ?label, "conflict resolved");
                    self.inner.store.emit(SyncEvent::ObjectChanged {
                        vault_id,
                        object_id,
                        revision: out.revision,
                        deleted: out.deleted,
                        origin: ChangeOrigin::ConflictResolution,
                    });
                    if let Some((copy_id, _)) = out.copy {
                        self.inner.store.emit(SyncEvent::ObjectChanged {
                            vault_id,
                            object_id: copy_id,
                            revision: 1,
                            deleted: false,
                            origin: ChangeOrigin::ConflictResolution,
                        });
                    }
                    self.inner.store.emit(SyncEvent::ConflictResolved {
                        vault_id,
                        object_id,
                        resolution: label,
                        conflict_copy: out.copy.map(|c| c.0),
                        kek_class,
                    });
                }
                Ok(None) => {
                    tracing::debug!(%object_id, "conflict waiting for server state");
                }
                Err(SyncError::Codec(e)) => {
                    tracing::info!(%object_id, error = %e, "conflict left unresolved for now");
                }
                Err(e) => return Err(e),
            }
        }
        Ok(progressed)
    }
}
