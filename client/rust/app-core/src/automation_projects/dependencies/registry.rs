use super::*;
use std::sync::{
    atomic::{AtomicBool, AtomicU8, AtomicU64, Ordering},
    OnceLock, Weak,
};
mod publication_state;
use publication_state::{JoinedCleanup, Publishing};
static ACTIVE: OnceLock<Arc<Semaphore>> = OnceLock::new();
pub(super) static FS: OnceLock<Arc<Semaphore>> = OnceLock::new();

#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(crate) enum Phase {
    Opening,
    Preparing,
    ArchiveVerified,
    CandidatePreview,
    Accepted,
    Publishing,
    Published,
    RecoveryRequired,
    Rejected,
    Cancelled,
    ContextChanged,
    Expired,
    Failed,
}
#[derive(Clone, Copy, Serialize)]
#[serde(rename_all = "camelCase")]
enum Cleanup {
    NotStarted,
    Closing,
    LocalCloseCompleted,
    RemoteStopUnconfirmed,
}
fn cleanup(value: u8) -> Cleanup {
    match value {
        1 => Cleanup::Closing,
        2 => Cleanup::LocalCloseCompleted,
        3 => Cleanup::RemoteStopUnconfirmed,
        _ => Cleanup::NotStarted,
    }
}
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DependencyView {
    version: u32,
    operation_id: Uuid,
    phase: Phase,
    error: Option<&'static str>,
    cleanup: Cleanup,
    archive_verified: bool,
    base_digest: Option<String>,
    candidate_digest: Option<String>,
    review_digest: Option<String>,
    change_count: usize,
    changes: Vec<Change>,
    changes_truncated: bool,
}
#[derive(Clone, Serialize)]
struct Change {
    path: String,
    kind: String,
}
pub(super) struct Ticket {
    pub review: Uuid,
    pub operation: Uuid,
    pub context: Arc<Context>,
    pub expires: Instant,
    pub cancel: CancellationToken,
}
pub(super) struct Resources {
    pub runtime: tokio::runtime::Handle,
    pub connection: Option<DependencyHelperConnection>,
    pub staging: Option<tempfile::TempDir>,
    pub reservation: Option<Reservation>,
    pub cleanup: Arc<AtomicU8>,
    pub remote: bool,
    pub publication_io: Mutex<Option<OwnedSemaphorePermit>>,
    pub joined: Arc<JoinedCleanup>,
}
impl Drop for Resources {
    fn drop(&mut self) {
        let connection = self.connection.take();
        let publication_io = self.publication_io.get_mut().unwrap_or_else(|e|e.into_inner()).take();
        let joined = self.joined.clone();
        let staging = self.staging.take();
        let reservation = self.reservation.take();
        if connection.is_none() && staging.is_none() && reservation.is_none() && publication_io.is_none() {
            joined.finish(true);
            return;
        }
        // Dedicated owner is explicitly closed/joined before releasing the
        // operation slot. Cleanup of an uncommitted private candidate is bounded
        // by the same process admission, even after its caller was cancelled.
        let cleanup = self.cleanup.clone();
        let remote = self.remote;
        if connection.is_some() {
            cleanup.store(1, Ordering::Release);
        }
        let registry = reservation
            .as_ref()
            .and_then(|reservation| reservation.registry.upgrade());
        // An AcceptedCandidate is Send and may be retired on a std thread.
        // Use the owning runtime, not the Drop thread's ambient context.
        let task = self.runtime.spawn(async move {
            if let Some(connection) = connection {
                connection.close().await;
                cleanup.store(if remote { 3 } else { 2 }, Ordering::Release);
            }
            tokio::task::spawn_blocking(move || {
                let staging_ok = staging.map_or(true, |temporary|temporary.close().is_ok());
                drop(reservation);
                drop(publication_io);
                joined.finish(staging_ok);
            })
            .await
            .ok();
        });
        if let Some(registry) = registry {
            registry.track_cleanup(task);
        }
    }
}
pub(super) struct Ready {
    pub preview: VerifiedDependencyPreview,
    pub candidate: Arc<ProjectRoot>,
    _resources: Resources,
}

// Internal custody only, NEVER serialized or accepted from UI/helper. The
// nonce is one-use and all existing TTL/revoke/context fences remain binding.
struct Acceptance {
    nonce: Uuid,
    registry_owned: bool,
}
enum State<T = Ready> {
    Running(Phase),
    Ready(DependencyView, Arc<T>),
    Accepted(DependencyView, Arc<T>, Acceptance),
    Publishing(DependencyView, Arc<T>, Publishing),
    Completing(DependencyView, Uuid),
    Terminal(Phase, &'static str),
}
pub(super) struct Operation {
    pub id: Uuid,
    pub context: Arc<Context>,
    pub expires: Instant,
    pub cancel: CancellationToken,
    pub peer: Mutex<Weak<cc_automation_core::HelperPeer>>,
    pub cleanup: Arc<AtomicU8>,
    state: Mutex<State>,
    retained_transport: AtomicBool,
    watch_epoch: AtomicU64,
    pub(super) joined: Arc<JoinedCleanup>,
}
fn dispose(ready: Arc<Ready>) {
    // Private staging cleanup can be filesystem work. Keep its reservation
    // until actual Drop finishes, even if the requesting future is gone.
    drop(ready);
}
// Generic only over the retained private payload so the exact state/cleanup
// fence can be race-tested without fabricating Core's private verified witness.
fn retire<T>(
    state: &Mutex<State<T>>,
    phase: Phase,
    code: &'static str,
    dispose: impl FnOnce(Arc<T>),
) {
    let mut state = state.lock().unwrap_or_else(|e| e.into_inner());
    if matches!(*state, State::Terminal(..)) {
        return;
    }
    let previous = std::mem::replace(&mut *state, State::Terminal(phase, code));
    if let State::Ready(_, retained) | State::Accepted(_, retained, _) | State::Publishing(_, retained, _) = previous {
        // Keep the state lock until synchronous cleanup registration completes.
        dispose(retained);
    }
}
fn reviewed<'a, T>(state: &'a State<T>, digest: &str) -> AppResult<&'a Arc<T>> {
    match state {
        State::Ready(view, retained) | State::Accepted(view, retained, _)
            if view.review_digest.as_deref() == Some(digest) =>
        {
            Ok(retained)
        }
        _ => Err(invalid("dependency_review_changed")),
    }
}
fn accept_state<T>(
    state: &Mutex<State<T>>,
    digest: &str,
    live: impl FnOnce() -> bool,
) -> AppResult<Uuid> {
    let mut state = state.lock().unwrap_or_else(|e| e.into_inner());
    if !live() {
        return Err(invalid("dependency_cancelled"));
    }
    let State::Ready(view, retained) = &*state else {
        return Err(invalid("dependency_review_changed"));
    };
    if view.review_digest.as_deref() != Some(digest) {
        return Err(invalid("dependency_review_changed"));
    }
    let mut view = view.clone();
    view.phase = Phase::Accepted;
    let retained = retained.clone();
    let nonce = Uuid::new_v4();
    *state = State::Accepted(
        view,
        retained,
        Acceptance {
            nonce,
            registry_owned: false,
        },
    );
    Ok(nonce)
}
fn retain_accepted_state<T>(
    state: &Mutex<State<T>>,
    digest: &str,
    nonce: Uuid,
    live: impl FnOnce() -> bool,
) -> AppResult<DependencyView> {
    let mut state = state.lock().unwrap_or_else(|e| e.into_inner());
    if !live() {
        return Err(invalid("dependency_cancelled"));
    }
    let State::Accepted(view, _, acceptance) = &mut *state else {
        return Err(invalid("dependency_review_changed"));
    };
    if view.review_digest.as_deref() != Some(digest)
        || acceptance.nonce != nonce
        || acceptance.registry_owned
    {
        return Err(invalid("dependency_review_changed"));
    }
    acceptance.registry_owned = true; // NO root clone/Drop/new registration under locks
    Ok(view.clone())
}
fn release_accepted<T>(
    state: &Mutex<State<T>>,
    nonce: Uuid,
    cancel: impl FnOnce(),
    dispose: impl FnOnce(Arc<T>),
) {
    let mut state = state.lock().unwrap_or_else(|e| e.into_inner());
    if !matches!(&*state,State::Accepted(_,_,current) if current.nonce==nonce&&!current.registry_owned)
    {
        return;
    }
    cancel();
    if let State::Accepted(_, retained, _) = std::mem::replace(
        &mut *state,
        State::Terminal(Phase::Rejected, "dependency_rejected"),
    ) {
        dispose(retained)
    }
}
fn schedule_review<T: Send + Sync + 'static, R: Send + 'static>(
    state: &Mutex<State<T>>,
    digest: &str,
    tasks: &Mutex<Vec<tokio::task::JoinHandle<()>>>,
    worker: impl FnOnce(&T) -> AppResult<R> + Send + 'static,
) -> AppResult<tokio::sync::oneshot::Receiver<AppResult<R>>> {
    let retained = {
        let state = state.lock().unwrap_or_else(|e| e.into_inner());
        reviewed(&*state, digest)?.clone()
    };
    let mut tasks = tasks.lock().unwrap_or_else(|e| e.into_inner());
    let (tx, rx) = tokio::sync::oneshot::channel();
    tasks.retain(|task| !task.is_finished());
    tasks.push(tokio::task::spawn_blocking(move || {
        let result = worker(&retained);
        let _ = tx.send(result);
        drop(retained);
    }));
    Ok(rx)
}
// The state lock serializes this observation with the successful Ready
// handoff. Remembering ownership after State becomes Terminal is essential:
// cancellation revokes review authority immediately, while Resources still
// owns the bounded private dispose/close operation. No new RPC authority.
fn cancellation_peer<T>(
    state: &Mutex<State<T>>,
    retained: &AtomicBool,
    peer: &Mutex<Weak<cc_automation_core::HelperPeer>>,
) -> Option<Arc<cc_automation_core::HelperPeer>> {
    let _state = state.lock().unwrap_or_else(|e| e.into_inner());
    if retained.load(Ordering::Acquire) {
        return None;
    }
    peer.lock().unwrap_or_else(|e| e.into_inner()).upgrade()
}
impl Operation {
    pub(super) fn cancellation_peer(&self) -> Option<Arc<cc_automation_core::HelperPeer>> {
        cancellation_peer(&self.state, &self.retained_transport, &self.peer)
    }
    pub(super) fn accept(&self, digest: &str) -> AppResult<Uuid> {
        accept_state(&self.state, digest, || {
            !self.cancel.is_cancelled() && Instant::now() < self.expires
        })
    }
    pub(super) fn release_accepted(&self, nonce: Uuid) {
        release_accepted(&self.state, nonce, || self.cancel.cancel(), dispose);
    }
    pub(super) fn accepted_matches(&self, nonce: Uuid) -> bool {
        !self.cancel.is_cancelled()
            && Instant::now() < self.expires
            && matches!(&*self.state.lock().unwrap_or_else(|e|e.into_inner()),State::Accepted(_,_,current) if current.nonce==nonce&&!current.registry_owned)
    }
    pub(super) fn reject(&self, digest: &str) -> AppResult<()> {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        reviewed(&*state, digest)?;
        self.cancel.cancel();
        let previous = std::mem::replace(
            &mut *state,
            State::Terminal(Phase::Rejected, "dependency_rejected"),
        );
        if let State::Ready(_, retained) | State::Accepted(_, retained, _) | State::Publishing(_, retained, _) = previous {
            dispose(retained)
        }
        Ok(())
    }
    pub fn terminal(&self, phase: Phase, code: &'static str) {
        self.cancel.cancel();
        retire(&self.state, phase, code, dispose);
    }
    pub fn phase(&self, phase: Phase) {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        if matches!(*state, State::Running(_)) && !self.cancel.is_cancelled() {
            *state = State::Running(phase);
        }
    }
    pub fn ready(
        &self,
        preview: VerifiedDependencyPreview,
        candidate: Arc<ProjectRoot>,
        resources: Resources,
    ) -> AppResult<()> {
        let changes = preview
            .diff()
            .changes
            .iter()
            .take(32)
            .map(|change| Change {
                path: change.path.clone(),
                kind: change.kind.clone(),
            })
            .collect();
        let view = DependencyView {
            version: 1,
            operation_id: self.id,
            phase: Phase::CandidatePreview,
            error: None,
            cleanup: cleanup(self.cleanup.load(Ordering::Acquire)),
            archive_verified: true,
            base_digest: Some(preview.base_digest().into()),
            candidate_digest: Some(preview.candidate().digest.clone()),
            review_digest: Some(preview.review_digest().into()),
            change_count: preview.diff().changes.len(),
            changes,
            changes_truncated: preview.diff().changes.len() > 32,
        };
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        if self.cancel.is_cancelled()
            || Instant::now() >= self.expires
            || !matches!(*state, State::Running(_))
        {
            drop(state);
            dispose(Arc::new(Ready {
                preview,
                candidate,
                _resources: resources,
            }));
            return Err(invalid("dependency_cancelled"));
        }
        // A healthy fully transferred result changes cleanup ownership only
        // while holding the same state lock read by the cancellation watcher.
        self.retained_transport.store(true, Ordering::Release);
        *state = State::Ready(
            view,
            Arc::new(Ready {
                preview,
                candidate,
                _resources: resources,
            }),
        );
        Ok(())
    }
    pub fn view(&self) -> DependencyView {
        let state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        if let State::Ready(view, _) | State::Accepted(view, _, _) | State::Publishing(view, _, _) | State::Completing(view, _) = &*state {
            let mut view = view.clone();
            view.cleanup = cleanup(self.cleanup.load(Ordering::Acquire));
            return view;
        }
        let (phase, error) = match &*state {
            State::Running(phase) => (*phase, None),
            State::Terminal(phase, error) => (*phase, Some(*error)),
            _ => unreachable!(),
        };
        DependencyView {
            version: 1,
            operation_id: self.id,
            phase,
            error,
            cleanup: cleanup(self.cleanup.load(Ordering::Acquire)),
            archive_verified: false,
            base_digest: None,
            candidate_digest: None,
            review_digest: None,
            change_count: 0,
            changes: vec![],
            changes_truncated: false,
        }
    }
}
impl Drop for Operation {
    fn drop(&mut self) {
        self.cancel.cancel();
        if let State::Ready(_, ready) | State::Accepted(_, ready, _) | State::Publishing(_, ready, _) = std::mem::replace(
            self.state.get_mut().unwrap_or_else(|e| e.into_inner()),
            State::Terminal(Phase::Cancelled, "dependency_cancelled"),
        ) {
            dispose(ready)
        }
    }
}

#[derive(Default)]
struct Maps {
    closed: bool,
    local_fs_epoch: CancellationToken,
    tickets: BTreeMap<Uuid, Ticket>,
    operations: BTreeMap<Uuid, Arc<Operation>>,
    projects: BTreeMap<String, Uuid>,
    cancelled: BTreeMap<Uuid, Instant>,
}
#[derive(Default)]
pub(crate) struct DependencyRegistry {
    maps: Mutex<Maps>,
    workers: Mutex<Vec<tokio::task::JoinHandle<()>>>,
    cleanups: Mutex<Vec<tokio::task::JoinHandle<()>>>,
}
pub(super) struct Reservation {
    registry: Weak<DependencyRegistry>,
    project: String,
    operation: Uuid,
    _global: OwnedSemaphorePermit,
}
impl Drop for Reservation {
    fn drop(&mut self) {
        if let Some(registry) = self.registry.upgrade() {
            let mut maps = registry.maps.lock().unwrap_or_else(|e| e.into_inner());
            if maps.projects.get(&self.project) == Some(&self.operation) {
                maps.projects.remove(&self.project);
            }
        }
    }
}
impl DependencyRegistry {
    pub(super) fn check_review(&self, op: &Arc<Operation>, digest: &str) -> AppResult<()> {
        let maps = self.maps.lock().unwrap_or_else(|e| e.into_inner());
        if maps.closed
            || !maps
                .operations
                .get(&op.id)
                .is_some_and(|current| Arc::ptr_eq(current, op))
            || op.cancel.is_cancelled()
            || Instant::now() >= op.expires
        {
            return Err(invalid("dependency_cancelled"));
        }
        let state = op.state.lock().unwrap_or_else(|e| e.into_inner());
        reviewed(&*state, digest).map(|_| ())
    }
    pub(super) fn accept_review(&self, op: &Arc<Operation>, digest: &str) -> AppResult<Uuid> {
        let maps = self.maps.lock().unwrap_or_else(|e| e.into_inner());
        if maps.closed
            || !maps
                .operations
                .get(&op.id)
                .is_some_and(|current| Arc::ptr_eq(current, op))
        {
            return Err(invalid("dependency_cancelled"));
        }
        op.accept(digest)
    }
    /// Clone retained roots only while atomically registering a tracked worker.
    /// No resource-owning clone may escape into an untracked caller future.
    pub(super) fn retain_acceptance(
        &self,
        op: &Arc<Operation>,
        digest: &str,
        nonce: Uuid,
    ) -> AppResult<DependencyView> {
        let maps = self.maps.lock().unwrap_or_else(|e| e.into_inner());
        if maps.closed
            || !maps
                .operations
                .get(&op.id)
                .is_some_and(|current| Arc::ptr_eq(current, op))
        {
            return Err(invalid("dependency_cancelled"));
        }
        retain_accepted_state(&op.state, digest, nonce, || {
            !op.cancel.is_cancelled() && Instant::now() < op.expires
        })
    }
    pub(super) fn review_work<T: Send + 'static>(
        &self,
        op: &Arc<Operation>,
        digest: &str,
        worker: impl FnOnce(&Ready) -> AppResult<T> + Send + 'static,
    ) -> AppResult<tokio::sync::oneshot::Receiver<AppResult<T>>> {
        let maps = self.maps.lock().unwrap_or_else(|e| e.into_inner());
        if maps.closed
            || !maps
                .operations
                .get(&op.id)
                .is_some_and(|current| Arc::ptr_eq(current, op))
            || op.cancel.is_cancelled()
            || Instant::now() >= op.expires
        {
            return Err(invalid("dependency_cancelled"));
        }
        schedule_review(&op.state, digest, &self.workers, worker)
    }
    /// Cancellation only, never a job/dependency/publication grant. The old
    /// epoch stays revoked across unlock; only a freshly captured original
    /// Native scope can register new local filesystem work.
    pub(crate) fn local_filesystem_epoch(&self) -> AppResult<CancellationToken> {
        let maps = self.maps.lock().unwrap_or_else(|e| e.into_inner());
        if maps.closed { return Err(AppError::VaultLocked); }
        Ok(maps.local_fs_epoch.child_token())
    }
    pub fn reopen(&self) {
        let mut maps = self.maps.lock().unwrap_or_else(|e| e.into_inner());
        if maps.closed { maps.local_fs_epoch = CancellationToken::new(); }
        maps.closed = false;
    }
    pub fn launch_worker(&self, worker: impl FnOnce() + Send + 'static) -> AppResult<()> {
        // Register the worker atomically with close admission. Session shutdown
        // cannot miss a newly started blocking task between spawn and tracking.
        let maps = self.maps.lock().unwrap_or_else(|e| e.into_inner());
        if maps.closed {
            return Err(AppError::VaultLocked);
        }
        let mut tasks = self.workers.lock().unwrap_or_else(|e| e.into_inner());
        tasks.retain(|task| !task.is_finished());
        tasks.push(tokio::task::spawn_blocking(worker));
        Ok(())
    }
    fn track_cleanup(&self, task: tokio::task::JoinHandle<()>) {
        let mut tasks = self.cleanups.lock().unwrap_or_else(|e| e.into_inner());
        tasks.retain(|task| !task.is_finished());
        tasks.push(task);
    }
    pub async fn close_and_wait(&self) {
        self.close();
        let workers = std::mem::take(&mut *self.workers.lock().unwrap_or_else(|e| e.into_inner()));
        for worker in workers {
            let _ = worker.await;
        }
        // Resource Drop registers cleanup before a worker returns. No session
        // transition guard or registry mutex is held across these awaits.
        loop {
            let tasks =
                std::mem::take(&mut *self.cleanups.lock().unwrap_or_else(|e| e.into_inner()));
            if tasks.is_empty() {
                break;
            }
            for task in tasks {
                let _ = task.await;
            }
        }
    }
    fn trim(&self) {
        let expired = {
            let mut maps = self.maps.lock().unwrap_or_else(|e| e.into_inner());
            let now = Instant::now();
            maps.tickets
                .retain(|_, ticket| ticket.expires > now && !ticket.cancel.is_cancelled());
            maps.cancelled.retain(|_, expires| *expires > now);
            let expired = maps
                .operations
                .iter()
                .filter(|(_, op)| op.expires <= now)
                .map(|(id, _)| *id)
                .collect::<Vec<_>>();
            expired
                .into_iter()
                .filter_map(|id| maps.operations.remove(&id))
                .collect::<Vec<_>>()
        };
        for op in expired {
            op.terminal(Phase::Expired, "dependency_expired")
        }
    }
    pub fn issue(&self, ticket: Ticket) -> AppResult<()> {
        self.trim();
        let mut maps = self.maps.lock().unwrap_or_else(|e| e.into_inner());
        if maps.closed {
            return Err(AppError::VaultLocked);
        }
        if maps.tickets.len() >= 8 {
            return Err(invalid("dependency_review_budget"));
        }
        maps.tickets.insert(ticket.review, ticket);
        Ok(())
    }
    pub fn consume(&self, id: Uuid) -> AppResult<Ticket> {
        self.trim();
        let mut maps = self.maps.lock().unwrap_or_else(|e| e.into_inner());
        let ticket = maps
            .tickets
            .remove(&id)
            .ok_or_else(|| invalid("dependency_review_expired"))?;
        if maps.cancelled.contains_key(&ticket.operation) || ticket.cancel.is_cancelled() {
            return Err(invalid("dependency_cancelled"));
        }
        Ok(ticket)
    }
    pub fn admit(self: &Arc<Self>, ticket: Ticket) -> AppResult<(Arc<Operation>, Reservation)> {
        let global = ACTIVE
            .get_or_init(|| Arc::new(Semaphore::new(2)))
            .clone()
            .try_acquire_owned()
            .map_err(|_| invalid("dependency_busy"))?;
        self.trim();
        let mut maps = self.maps.lock().unwrap_or_else(|e| e.into_inner());
        let project = ticket.context.project.project.id.clone();
        if maps.closed
            || maps.projects.contains_key(&project)
            || maps.operations.len() >= 16
            || maps.cancelled.contains_key(&ticket.operation)
            || ticket.cancel.is_cancelled()
            || ticket.expires <= Instant::now()
        {
            return Err(invalid("dependency_busy"));
        }
        let op = Arc::new(Operation {
            id: ticket.operation,
            context: ticket.context,
            expires: ticket.expires,
            cancel: ticket.cancel,
            peer: Mutex::new(Weak::new()),
            cleanup: Arc::new(AtomicU8::new(0)),
            state: Mutex::new(State::Running(Phase::Opening)),
            retained_transport: AtomicBool::new(false),
            watch_epoch: AtomicU64::new(0),
            joined: Arc::default(),
        });
        maps.projects.insert(project.clone(), op.id);
        maps.operations.insert(op.id, op.clone());
        Ok((
            op.clone(),
            Reservation {
                registry: Arc::downgrade(self),
                project,
                operation: op.id,
                _global: global,
            },
        ))
    }
    pub fn get(&self, id: Uuid) -> AppResult<Arc<Operation>> {
        self.trim();
        self.maps
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .operations
            .get(&id)
            .cloned()
            .ok_or_else(|| invalid("dependency_operation_unavailable"))
    }
    pub fn cancel(&self, id: Uuid) -> AppResult<()> {
        self.trim();
        let op = {
            let mut maps = self.maps.lock().unwrap_or_else(|e| e.into_inner());
            let pending = maps.tickets.values().find(|ticket| ticket.operation == id);
            let known = pending.is_some() || maps.operations.contains_key(&id);
            if let Some(ticket) = pending {
                ticket.cancel.cancel();
            }
            // Tombstone quota applies only to unknown early cancellation IDs;
            // it must never prevent cancellation of an admitted operation.
            if !known {
                if maps.cancelled.len() >= 16 && !maps.cancelled.contains_key(&id) {
                    return Err(invalid("dependency_cancel_budget"));
                }
                maps.cancelled
                    .insert(id, Instant::now() + Duration::from_secs(60));
            }
            maps.operations.get(&id).cloned()
        };
        if let Some(op) = op {
            op.terminal(Phase::Cancelled, "dependency_cancelled")
        }
        Ok(())
    }
    pub fn revoke_runner(&self, runner: &str) {
        let operations = {
            let mut maps = self.maps.lock().unwrap_or_else(|e| e.into_inner());
            maps.tickets.retain(|_, ticket| {
                if ticket.context.runner.id == runner {
                    ticket.cancel.cancel();
                    false
                } else {
                    true
                }
            });
            maps.operations
                .values()
                .filter(|op| op.context.runner.id == runner)
                .cloned()
                .collect::<Vec<_>>()
        };
        for op in operations {
            op.terminal(Phase::ContextChanged, "dependency_context_changed")
        }
    }
    pub fn close(&self) {
        let operations = {
            let mut maps = self.maps.lock().unwrap_or_else(|e| e.into_inner());
            maps.closed = true;
            maps.local_fs_epoch.cancel();
            for ticket in maps.tickets.values() {
                ticket.cancel.cancel()
            }
            maps.tickets.clear();
            maps.cancelled.clear();
            std::mem::take(&mut maps.operations)
                .into_values()
                .collect::<Vec<_>>()
        };
        for op in operations {
            op.terminal(Phase::Cancelled, "dependency_cancelled")
        }
    }
}

#[cfg(test)]
mod retirement_tests {
    use super::*;
    fn review_view() -> DependencyView {
        DependencyView {
            version: 1,
            operation_id: Uuid::new_v4(),
            phase: Phase::CandidatePreview,
            error: None,
            cleanup: Cleanup::NotStarted,
            archive_verified: false,
            base_digest: None,
            candidate_digest: None,
            review_digest: Some("a".repeat(64)),
            change_count: 0,
            changes: vec![],
            changes_truncated: false,
        }
    }
    #[tokio::test]
    async fn accepted_nonce_is_one_use_and_retains_admission_until_its_own_drop() {
        struct Payload {
            count: Arc<std::sync::atomic::AtomicUsize>,
            _permit: OwnedSemaphorePermit,
        }
        impl Drop for Payload {
            fn drop(&mut self) {
                self.count.fetch_add(1, Ordering::SeqCst);
            }
        }
        let semaphore = Arc::new(Semaphore::new(1));
        let count = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let state = Mutex::new(State::Ready(
            review_view(),
            Arc::new(Payload {
                count: count.clone(),
                _permit: semaphore.clone().try_acquire_owned().unwrap(),
            }),
        ));
        assert!(accept_state(&state, &"b".repeat(64), || true).is_err());
        assert!(accept_state(&state, &"a".repeat(64), || false).is_err());
        let nonce = accept_state(&state, &"a".repeat(64), || true).unwrap();
        assert!(accept_state(&state, &"a".repeat(64), || true).is_err());
        assert_eq!(semaphore.available_permits(), 0);
        release_accepted(
            &state,
            Uuid::new_v4(),
            || panic!("foreign nonce revoked owner"),
            drop,
        );
        assert_eq!(count.load(Ordering::SeqCst), 0);
        let cancelled = std::sync::atomic::AtomicBool::new(false);
        release_accepted(
            &state,
            nonce,
            || cancelled.store(true, Ordering::SeqCst),
            drop,
        );
        assert!(cancelled.load(Ordering::SeqCst));
        assert_eq!(count.load(Ordering::SeqCst), 1);
        assert_eq!(semaphore.available_permits(), 1);
        release_accepted(&state, nonce, || panic!("duplicate cleanup"), drop);
        assert!(accept_state(&state, &"a".repeat(64), || true).is_err());
    }
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn ready_accept_racing_lifecycle_retirement_never_resurrects_retained_resources() {
        struct Payload(Arc<std::sync::atomic::AtomicUsize>);
        impl Drop for Payload {
            fn drop(&mut self) {
                self.0.fetch_add(1, Ordering::SeqCst);
            }
        }
        for _ in 0..16 {
            let count = Arc::new(std::sync::atomic::AtomicUsize::new(0));
            let state = Arc::new(Mutex::new(State::Ready(
                review_view(),
                Arc::new(Payload(count.clone())),
            )));
            let barrier = Arc::new(std::sync::Barrier::new(2));
            let cancelled = Arc::new(std::sync::atomic::AtomicBool::new(false));
            let accept = tokio::task::spawn_blocking({
                let state = state.clone();
                let barrier = barrier.clone();
                let cancelled = cancelled.clone();
                move || {
                    barrier.wait();
                    accept_state(&state, &"a".repeat(64), || {
                        !cancelled.load(Ordering::Acquire)
                    })
                }
            });
            let retire = tokio::task::spawn_blocking({
                let state = state.clone();
                move || {
                    barrier.wait();
                    cancelled.store(true, Ordering::Release);
                    retire(
                        &state,
                        Phase::ContextChanged,
                        "dependency_context_changed",
                        drop,
                    )
                }
            });
            let _ = accept.await.unwrap();
            retire.await.unwrap();
            assert!(matches!(
                *state.lock().unwrap(),
                State::Terminal(Phase::ContextChanged, _)
            ));
            assert_eq!(count.load(Ordering::SeqCst), 1);
        }
    }
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn abandoned_review_worker_retains_accepted_owner_until_tracked_cleanup_is_joined() {
        struct CleanupData {
            registry: Weak<DependencyRegistry>,
            entered: tokio::sync::oneshot::Sender<()>,
            finish: tokio::sync::oneshot::Receiver<()>,
            permit: OwnedSemaphorePermit,
        }
        struct Payload(Mutex<Option<CleanupData>>);
        impl Drop for Payload {
            fn drop(&mut self) {
                let data = self.0.get_mut().unwrap().take().unwrap();
                let registry = data.registry.upgrade().unwrap();
                registry.track_cleanup(tokio::spawn(async move {
                    data.entered.send(()).unwrap();
                    data.finish.await.unwrap();
                    drop(data.permit);
                }));
            }
        }
        let registry = Arc::new(DependencyRegistry::default());
        let semaphore = Arc::new(Semaphore::new(1));
        let (cleanup_entered, cleanup_wait) = tokio::sync::oneshot::channel();
        let (finish, finished) = tokio::sync::oneshot::channel();
        let state = Mutex::new(State::Ready(
            review_view(),
            Arc::new(Payload(Mutex::new(Some(CleanupData {
                registry: Arc::downgrade(&registry),
                entered: cleanup_entered,
                finish: finished,
                permit: semaphore.clone().try_acquire_owned().unwrap(),
            })))),
        ));
        let nonce = accept_state(&state, &"a".repeat(64), || true).unwrap();
        let (entered, waiting) = tokio::sync::oneshot::channel();
        let (release, released) = std::sync::mpsc::channel();
        // The production scheduling helper owns the retained Arc, and caller
        // abandonment drops only the plain result receiver.
        let output = schedule_review(&state, &"a".repeat(64), &registry.workers, move |_| {
            entered.send(()).unwrap();
            released.recv().unwrap();
            Ok(())
        })
        .unwrap();
        drop(output);
        waiting.await.unwrap();
        release_accepted(&state, nonce, || {}, drop);
        assert_eq!(semaphore.available_permits(), 0);
        let closing = tokio::spawn({
            let registry = registry.clone();
            async move { registry.close_and_wait().await }
        });
        tokio::task::yield_now().await;
        assert!(!closing.is_finished());
        release.send(()).unwrap();
        cleanup_wait.await.unwrap();
        assert!(!closing.is_finished());
        assert_eq!(semaphore.available_permits(), 0);
        finish.send(()).unwrap();
        closing.await.unwrap();
        assert_eq!(semaphore.available_permits(), 1);
    }
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn accepted_retirement_from_std_thread_uses_owner_runtime_and_joins_resource_cleanup() {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(1)
            .max_blocking_threads(1)
            .enable_all()
            .build()
            .unwrap();
        let (entered, waiting) = std::sync::mpsc::channel();
        let (release, released) = std::sync::mpsc::channel();
        let blocker = runtime.spawn_blocking(move || {
            entered.send(()).unwrap();
            released.recv().unwrap();
        });
        waiting.recv_timeout(Duration::from_secs(2)).unwrap();
        let registry = Arc::new(DependencyRegistry::default());
        let semaphore = Arc::new(Semaphore::new(1));
        let project = Uuid::new_v4().to_string();
        let id = Uuid::new_v4();
        registry
            .maps
            .lock()
            .unwrap()
            .projects
            .insert(project.clone(), id);
        let staging = tempfile::Builder::new()
            .prefix("cc-accepted-drop-fixture-")
            .tempdir()
            .unwrap();
        let path = staging.path().to_path_buf();
        let resources = Resources {
            runtime: runtime.handle().clone(),
            connection: None,
            staging: Some(staging),
            reservation: Some(Reservation {
                registry: Arc::downgrade(&registry),
                project: project.clone(),
                operation: id,
                _global: semaphore.clone().try_acquire_owned().unwrap(),
            }),
            cleanup: Arc::new(AtomicU8::new(0)),
            remote: false,
            publication_io: Mutex::new(None),
            joined: Arc::default(),
        };
        // Real Resources/TempDir/reservation, with no fabricated artifact or
        // connection. Exercise the same accepted-retirement Drop path from a
        // thread that has no current Tokio runtime.
        let state = Arc::new(Mutex::new(State::Ready(review_view(), Arc::new(resources))));
        let nonce = accept_state(&state, &"a".repeat(64), || true).unwrap();
        std::thread::spawn(move || {
            assert!(tokio::runtime::Handle::try_current().is_err());
            release_accepted(&state, nonce, || {}, drop);
        })
        .join()
        .unwrap();
        assert!(
            path.exists(),
            "owned cleanup must use the occupied runtime blocking queue"
        );
        assert_eq!(semaphore.available_permits(), 0);
        assert!(registry
            .maps
            .lock()
            .unwrap()
            .projects
            .contains_key(&project));
        let closing = tokio::spawn({
            let registry = registry.clone();
            async move { registry.close_and_wait().await }
        });
        tokio::task::yield_now().await;
        assert!(!closing.is_finished());
        release.send(()).unwrap();
        blocker.await.unwrap();
        closing.await.unwrap();
        assert!(!path.exists());
        assert_eq!(semaphore.available_permits(), 1);
        assert!(!registry
            .maps
            .lock()
            .unwrap()
            .projects
            .contains_key(&project));
        runtime.shutdown_background();
    }
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn ready_retirement_registers_cleanup_before_concurrent_close_can_drain() {
        retirement_race(false).await;
    }
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn accepted_retirement_registers_cleanup_before_concurrent_close_can_drain() {
        retirement_race(true).await;
    }
    async fn retirement_race(accepted: bool) {
        let registry = Arc::new(DependencyRegistry::default());
        let view = DependencyView {
            version: 1,
            operation_id: Uuid::new_v4(),
            phase: Phase::CandidatePreview,
            error: None,
            cleanup: Cleanup::NotStarted,
            archive_verified: true,
            base_digest: None,
            candidate_digest: None,
            review_digest: Some("a".repeat(64)),
            change_count: 0,
            changes: vec![],
            changes_truncated: false,
        };
        // Unit payload is not an artifact witness. Exercise the production
        // generic state-retirement fence without a Core verification bypass.
        let state = Arc::new(Mutex::new(State::Ready(view, Arc::new(()))));
        if accepted {
            accept_state(&state, &"a".repeat(64), || true).unwrap();
        }
        let (entered_tx, entered_rx) = tokio::sync::oneshot::channel();
        let (register_tx, register_rx) = std::sync::mpsc::channel();
        let (cleanup_entered_tx, cleanup_entered_rx) = tokio::sync::oneshot::channel();
        let (finish_tx, finish_rx) = tokio::sync::oneshot::channel();
        let producer = tokio::task::spawn_blocking({
            let state = state.clone();
            let registry = registry.clone();
            move || {
                retire(
                    &state,
                    Phase::Cancelled,
                    "dependency_cancelled",
                    move |_| {
                        entered_tx.send(()).unwrap();
                        register_rx.recv().unwrap();
                        registry.track_cleanup(tokio::spawn(async move {
                            cleanup_entered_tx.send(()).unwrap();
                            finish_rx.await.unwrap();
                        }));
                    },
                );
            }
        });
        entered_rx.await.unwrap();
        assert!(
            state.try_lock().is_err(),
            "terminal state must not be visible before registration"
        );
        let (closing_tx, closing_rx) = tokio::sync::oneshot::channel();
        let runtime = tokio::runtime::Handle::current();
        let closer = tokio::task::spawn_blocking({
            let state = state.clone();
            let registry = registry.clone();
            move || {
                closing_tx.send(()).unwrap();
                retire(
                    &state,
                    Phase::Cancelled,
                    "dependency_cancelled",
                    |_| unreachable!(),
                );
                runtime.block_on(registry.close_and_wait());
            }
        });
        closing_rx.await.unwrap();
        assert!(!closer.is_finished());
        register_tx.send(()).unwrap();
        producer.await.unwrap();
        cleanup_entered_rx.await.unwrap();
        assert!(
            !closer.is_finished(),
            "registered cleanup must be joined before close returns"
        );
        finish_tx.send(()).unwrap();
        closer.await.unwrap();
    }
}

#[cfg(test)]
#[path = "tests/live_remote.rs"]
mod live_remote;

#[cfg(test)]
#[path = "tests/retirement.rs"]
mod retained_transport_tests;

#[cfg(test)]
#[path = "tests/facade_retention.rs"]
mod facade_retention_tests;
