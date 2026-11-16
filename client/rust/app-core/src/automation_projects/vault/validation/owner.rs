//! Private operation-owned validation. No bridge/dispatch/job approval.
use super::*;
use crate::automation_runners::dependency::{capture_runner, DependencyRunner};
use cc_automation_core::{
    RunParameters, ValidationHelperConnection, ValidationPeerObserver,
    VaultValidationControlFuture, VaultValidationControlRequest, VaultValidationHandler,
    VaultValidationResponder,
};
use cc_models::automation::ApprovedTarget;
use serde::Serialize;
use std::{
    collections::BTreeMap,
    future::Future,
    sync::{Mutex, OnceLock, Weak},
};
use tokio::sync::{oneshot, OwnedSemaphorePermit, Semaphore};
static ACTIVE: OnceLock<Arc<Semaphore>> = OnceLock::new();
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum ValidationCleanup {
    LocalCloseCompleted,
    RemoteStopUnconfirmed,
}
#[derive(Serialize)]
pub(crate) struct VaultValidationOutcome {
    pub validation: VerifiedValidation,
    pub cleanup: ValidationCleanup,
}
/// Rust-only Native-owned projection, never Deserialize from UI/helper. Target
/// data is validation input, NOT SSH/job authority; no target is contacted.
pub(crate) struct VaultValidationInput {
    pub project_id: String,
    pub project_revision: u64,
    pub revision_digest: String,
    pub credential_id: String,
    pub runner_id: String,
    pub runner_revision: u64,
    pub playbook: String,
    pub parameters: RunParameters,
    pub inventory: Value,
    pub targets: Vec<ApprovedTarget>,
}
struct Entry {
    runner: String,
    cancel: CancellationToken,
    // None is always original-unlocked validation authority. Only actual
    // admitted signed-job policy can retain previously sealed material.
    material_policy: Option<(Uuid, bool)>,
    control: Option<Weak<dyn VaultValidationHandler>>,
}
#[derive(Default)]
struct State {
    closed: bool,
    entries: BTreeMap<Uuid, Entry>,
    workers: Vec<tokio::task::JoinHandle<()>>,
    cleanups: Vec<tokio::task::JoinHandle<()>>,
}
#[derive(Default)]
pub(crate) struct VaultValidationOperations {
    state: Mutex<State>,
}
pub(super) struct Reservation {
    registry: Weak<VaultValidationOperations>,
    operation: Uuid,
    _global: OwnedSemaphorePermit,
}
impl Drop for Reservation {
    fn drop(&mut self) {
        if let Some(registry) = self.registry.upgrade() {
            // Normal retirement is NOT external cancellation. The worker
            // retains this entry through cleanup and final result handoff so
            // lock/profile/Runner/caller cancellation can still fence it.
            registry
                .state
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .entries
                .remove(&self.operation);
        }
    }
}
pub(super) struct Resources {
    runtime: tokio::runtime::Handle,
    pub(super) connection: Option<ValidationHelperConnection>,
    pub(super) staging: Option<tempfile::TempDir>,
    pub(super) scope: Option<Arc<NativeVaultValidation>>,
    pub(super) watcher: Option<tokio::task::JoinHandle<()>>,
    pub(super) watcher_stop: CancellationToken,
    reservation: Option<Reservation>,
}
impl Resources {
    fn schedule(&mut self) -> Option<oneshot::Receiver<Option<Reservation>>> {
        if self.connection.is_none()
            && self.staging.is_none()
            && self.reservation.is_none()
            && self.scope.is_none()
            && self.watcher.is_none()
        {
            return None;
        }
        // Fence material synchronously while trusted disposal remains alive.
        // This child revoke must NOT cancel the operation/transport watcher.
        if let Some(scope) = &self.scope {
            scope.revoke();
        }
        let connection = self.connection.take();
        let staging = self.staging.take();
        let scope = self.scope.take();
        let watcher = self.watcher.take();
        let stop = self.watcher_stop.clone();
        let reservation = self.reservation.take();
        let registry = reservation.as_ref().and_then(|r| r.registry.upgrade());
        let (send, receive) = oneshot::channel();
        // Register the cleanup before yielding. An aborted caller/worker cannot
        // release its slot while an in-progress close future gets dropped.
        let cleanup = async move {
            if let Some(connection) = connection {
                connection.close().await;
            }
            stop.cancel();
            if let Some(watcher) = watcher {
                let _ = watcher.await;
            }
            drop(scope);
            let _ = tokio::task::spawn_blocking(move || drop(staging)).await;
            // Keep entry AND both admission slots through worker's final
            // cancellation check/result send. If caller was dropped, sending
            // fails and safely drops reservation here AFTER joined cleanup.
            let _ = send.send(reservation);
        };
        if let Some(registry) = registry {
            let mut state = registry.state.lock().unwrap_or_else(|e| e.into_inner());
            state.cleanups.retain(|task| !task.is_finished());
            state.cleanups.push(self.runtime.spawn(cleanup));
        } else {
            self.runtime.spawn(cleanup);
        }
        Some(receive)
    }
    pub(super) async fn finish(&mut self) -> Option<Reservation> {
        match self.schedule() {
            Some(done) => done.await.ok().flatten(),
            None => None,
        }
    }
}
impl Drop for Resources {
    fn drop(&mut self) {
        let _ = self.schedule();
    }
}
impl VaultValidationOperations {
    pub fn cancel_on_lock(&self) {
        let controls = {
            let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
            state.closed = true;
            state
                .entries
                .values()
                .filter(|entry| entry.material_policy.is_none_or(|(_, on_lock)| on_lock))
                .filter_map(|entry| {
                    entry.cancel.cancel();
                    entry.control.as_ref().and_then(Weak::upgrade)
                })
                .collect::<Vec<_>>()
        };
        for control in controls {
            control.revoke();
        }
    }
    pub(super) fn bind_control(
        &self,
        operation: Uuid,
        control: Arc<dyn VaultValidationHandler>,
    ) -> AppResult<()> {
        let bound = {
            let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
            if state.closed {
                false
            } else if let Some(entry) = state.entries.get_mut(&operation) {
                if entry.cancel.is_cancelled() || entry.control.is_some() {
                    false
                } else {
                    entry.control = Some(Arc::downgrade(&control));
                    true
                }
            } else {
                false
            }
        };
        if bound {
            Ok(())
        } else {
            control.revoke();
            Err(invalid())
        }
    }
    pub(super) fn commit_material(
        &self,
        operation: Uuid,
        verified: &cc_automation_core::VerifiedJob,
        install: impl FnOnce() -> AppResult<()>,
    ) -> AppResult<()> {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        if state.closed || verified.is_access_expired() {
            return Err(invalid());
        }
        let entry = state.entries.get_mut(&operation).ok_or_else(invalid)?;
        if entry.cancel.is_cancelled() || entry.material_policy.is_some() {
            return Err(invalid());
        }
        // install only moves an already sealed value; callers keep failed
        // payload destruction outside this map lock. No callbacks/await/join.
        install()?;
        entry.material_policy = Some((
            verified.manifest().job_id,
            verified.manifest().policy.cancel_on_lock,
        ));
        Ok(())
    }
    pub fn cancel_all(&self) {
        let controls = {
            let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
            state.closed = true;
            state
                .entries
                .values()
                .filter_map(|entry| {
                    entry.cancel.cancel();
                    entry.control.as_ref().and_then(Weak::upgrade)
                })
                .collect::<Vec<_>>()
        };
        // Password/body destruction and callbacks are NEVER under the map lock.
        for control in controls {
            control.revoke();
        }
    }
    pub fn reopen(&self) {
        self.state.lock().unwrap_or_else(|e| e.into_inner()).closed = false;
    }
    pub fn revoke_runner(&self, runner: &str) {
        let controls = {
            let state = self.state.lock().unwrap_or_else(|e| e.into_inner());
            state
                .entries
                .values()
                .filter(|entry| entry.runner == runner)
                .filter_map(|entry| {
                    entry.cancel.cancel();
                    entry.control.as_ref().and_then(Weak::upgrade)
                })
                .collect::<Vec<_>>()
        };
        for control in controls {
            control.revoke();
        }
    }
    fn launch<F, Fut>(
        self: &Arc<Self>,
        runner: String,
        external: &CancellationToken,
        worker: F,
    ) -> AppResult<(
        oneshot::Receiver<AppResult<VaultValidationOutcome>>,
        CancellationToken,
    )>
    where
        F: FnOnce(
                Uuid,
                CancellationToken,
                Resources,
                oneshot::Sender<AppResult<VaultValidationOutcome>>,
            ) -> Fut
            + Send
            + 'static,
        Fut: Future<Output = ()> + Send + 'static,
    {
        self.launch_typed(runner, external, worker)
    }
    pub(super) fn launch_job<T, F, Fut>(
        self: &Arc<Self>,
        runner: String,
        external: &CancellationToken,
        worker: F,
    ) -> AppResult<(oneshot::Receiver<AppResult<T>>, CancellationToken)>
    where
        T: Send + 'static,
        F: FnOnce(Uuid, CancellationToken, Resources, oneshot::Sender<AppResult<T>>) -> Fut
            + Send
            + 'static,
        Fut: Future<Output = ()> + Send + 'static,
    {
        self.launch_typed(runner, external, worker)
    }
    fn launch_typed<T, F, Fut>(
        self: &Arc<Self>,
        runner: String,
        external: &CancellationToken,
        worker: F,
    ) -> AppResult<(oneshot::Receiver<AppResult<T>>, CancellationToken)>
    where
        T: Send + 'static,
        F: FnOnce(Uuid, CancellationToken, Resources, oneshot::Sender<AppResult<T>>) -> Fut
            + Send
            + 'static,
        Fut: Future<Output = ()> + Send + 'static,
    {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        if state.closed || state.entries.len() >= 2 || external.is_cancelled() {
            return Err(invalid());
        }
        let global = ACTIVE
            .get_or_init(|| Arc::new(Semaphore::new(2)))
            .clone()
            .try_acquire_owned()
            .map_err(|_| invalid())?;
        let operation = Uuid::new_v4();
        let cancel = external.child_token();
        let (send, receive) = oneshot::channel();
        state.entries.insert(
            operation,
            Entry {
                runner,
                cancel: cancel.clone(),
                material_policy: None,
                control: None,
            },
        );
        let resources = Resources {
            runtime: tokio::runtime::Handle::current(),
            connection: None,
            staging: None,
            scope: None,
            watcher: None,
            watcher_stop: CancellationToken::new(),
            reservation: Some(Reservation {
                registry: Arc::downgrade(self),
                operation,
                _global: global,
            }),
        };
        state.workers.retain(|worker| !worker.is_finished());
        // Same admission mutex as close; no spawned-but-unregistered interval.
        let work_cancel = cancel.clone();
        state.workers.push(tokio::spawn(async move {
            // Evaluate the private worker constructor AFTER registration too;
            // panic/drop cannot recursively retire resources under this mutex.
            worker(operation, work_cancel, resources, send).await;
        }));

        Ok((receive, cancel))
    }
    pub async fn close_and_wait(&self) {
        self.cancel_all();
        let workers =
            std::mem::take(&mut self.state.lock().unwrap_or_else(|e| e.into_inner()).workers);
        for worker in workers {
            let _ = worker.await;
        }
        loop {
            let cleanups = std::mem::take(
                &mut self
                    .state
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .cleanups,
            );
            if cleanups.is_empty() {
                break;
            }
            for cleanup in cleanups {
                let _ = cleanup.await;
            }
        }
    }
}
// Shared exact terminal transition; tests use () rather than fabricating a
// validated Ansible witness. Caller retains reservation through this +send.
fn completed<T>(
    result: AppResult<T>,
    cancel: &CancellationToken,
    retained: &Option<Reservation>,
) -> AppResult<T> {
    if retained.is_none() || cancel.is_cancelled() {
        Err(invalid())
    } else {
        result
    }
}
struct Original {
    owner: Arc<crate::session::Session>,
    unlock: Weak<crate::session::Unlocked>,
    generation: u64,
    version: u64,
}
struct Waiter(CancellationToken);
impl Drop for Waiter {
    fn drop(&mut self) {
        self.0.cancel();
    }
}
#[derive(Clone)]
struct Control {
    app: Weak<crate::app::Inner>,
    runner: Arc<DependencyRunner>,
    scope: Arc<OnceLock<Weak<NativeVaultValidation>>>,
    observer: Arc<OnceLock<ValidationPeerObserver>>,
    cancel: CancellationToken,
}
async fn runner_current(scope: &NativeVaultValidation, runner: &DependencyRunner) -> AppResult<()> {
    let fresh = capture_runner(&scope.review.project.owner, &runner.id, runner.revision).await?;
    if fresh.stamp != runner.stamp {
        return Err(invalid());
    }
    Ok(())
}
impl VaultValidationHandler for Control {
    fn revoke(&self) {
        if let Some(scope) = self.scope.get().and_then(Weak::upgrade) {
            scope.revoke();
        }
    }
    fn handle(
        &self,
        request: VaultValidationControlRequest,
        mut responder: VaultValidationResponder,
    ) -> VaultValidationControlFuture {
        let control = self.clone();
        Box::pin(async move {
            let result: AppResult<()> = async {
                if control.cancel.is_cancelled() {
                    return Err(invalid());
                }
                responder.ensure_live().map_err(|_| invalid())?;
                let app = AppCore {
                    inner: control.app.upgrade().ok_or_else(invalid)?,
                };
                let scope = control
                    .scope
                    .get()
                    .and_then(Weak::upgrade)
                    .ok_or_else(invalid)?;
                let observer = control.observer.get().ok_or_else(invalid)?;
                let runtime = responder
                    .bound_runtime(observer.observe().await.map_err(|_| invalid())?)
                    .map_err(|_| invalid())?;
                let mut password = scope.phase(&app, &request.body, runtime).await?;
                let u = scope
                    .review
                    .project
                    .unlock
                    .upgrade()
                    .ok_or(AppError::VaultLocked)?;
                let _profile = app.hold_session(&scope.review.project.owner).await?;
                let _unlock = scope.review.project.owner.hold_unlocked(&u).await?;
                scope.review.check_held(&u).await?;
                runner_current(&scope, &control.runner).await?;
                if control.cancel.is_cancelled() {
                    return Err(invalid());
                }
                // Final original guards cover actual bounded private write.
                responder
                    .password(&mut password.lease, &request.body)
                    .await
                    .map_err(|_| invalid())
            }
            .await;
            if result.is_err() {
                let _ = responder.denied().await;
            }
        })
    }
}
impl AppCore {
    pub(crate) async fn automation_vault_validate_private(
        &self,
        input: VaultValidationInput,
        cancel: &CancellationToken,
    ) -> AppResult<VaultValidationOutcome> {
        input.parameters.validate().map_err(|_| invalid())?;
        if input.targets.is_empty()
            || input.targets.len() > 100
            || serde_json::to_vec(&input.inventory).map_or(true, |v| v.len() > 256 * 1024)
        {
            return Err(invalid());
        }
        let project = self.automation_project_context(&input.project_id).await?;
        let original = {
            let u = project.unlock.upgrade().ok_or(AppError::VaultLocked)?;
            let _profile = self.hold_session(&project.owner).await?;
            let _unlock = project.owner.hold_unlocked(&u).await?;
            Original {
                owner: project.owner.clone(),
                unlock: Arc::downgrade(&u),
                generation: project.owner.automation.unlocked_generation()?,
                version: u.working().version(),
            }
        };
        let registry = project.owner.automation.vault_validation.clone();
        drop(project);
        let app = self.clone();
        let runner = input.runner_id.clone();
        let (receive, cancel) = registry.launch(
            runner,
            cancel,
            move |operation, cancel, mut resources, send| async move {
                let result = work(&app, input, original, operation, &cancel, &mut resources).await;
                // No value leaves before owned peer/handlers/temp cleanup join. A
                // dropped caller cancels authority, worker still owns admission.
                let _retained = resources.finish().await;
                let _ = send.send(completed(result, &cancel, &_retained));
            },
        )?;
        let _waiter = Waiter(cancel);
        receive.await.map_err(|_| invalid())?
    }
}
async fn work(
    app: &AppCore,
    input: VaultValidationInput,
    original: Original,
    operation: Uuid,
    cancel: &CancellationToken,
    resources: &mut Resources,
) -> AppResult<VaultValidationOutcome> {
    if cancel.is_cancelled() {
        return Err(invalid());
    }
    let review = app
        .automation_vault_capture(
            input.project_id,
            input.project_revision,
            input.revision_digest,
            input.credential_id,
            cancel,
        )
        .await?;
    // Never recapture another profile/unlock after an awaited admission/call.
    let original_unlock = original.unlock.upgrade().ok_or(AppError::VaultLocked)?;
    let review_unlock = review
        .project
        .unlock
        .upgrade()
        .ok_or(AppError::VaultLocked)?;
    if !Arc::ptr_eq(&original.owner, &review.project.owner)
        || !Arc::ptr_eq(&original_unlock, &review_unlock)
        || review.generation != original.generation
        || review.version != original.version
    {
        return Err(invalid());
    }
    drop(original_unlock);
    drop(review_unlock);
    drop(original);
    let runner = {
        let u = review
            .project
            .unlock
            .upgrade()
            .ok_or(AppError::VaultLocked)?;
        let _profile = app.hold_session(&review.project.owner).await?;
        let _unlock = review.project.owner.hold_unlocked(&u).await?;
        review.check_held(&u).await?;
        Arc::new(
            capture_runner(
                &review.project.owner,
                &input.runner_id,
                input.runner_revision,
            )
            .await?,
        )
    };
    let control = Arc::new(Control {
        app: Arc::downgrade(&app.inner),
        runner: runner.clone(),
        scope: Arc::default(),
        observer: Arc::default(),
        cancel: cancel.clone(),
    });
    let connection = app
        .open_validation_owner(
            &runner,
            &review.project.unlock,
            operation,
            cancel,
            control.clone(),
        )
        .await?;
    let observer = connection.observer();
    control
        .observer
        .set(observer.clone())
        .map_err(|_| invalid())?;
    resources.connection = Some(connection);
    let watched = observer.clone();
    let watched_cancel = cancel.clone();
    let stop = resources.watcher_stop.clone();
    resources.watcher = Some(tokio::spawn(async move {
        tokio::select! {biased;_=stop.cancelled()=>(),_=watched_cancel.cancelled()=>watched.cancel()}
    }));
    // Same actual ProjectRoot freeze/walker; no second writer or source trust.
    let io = automation_io_slot(&review.project.owner)?;
    let root = review.root.clone();
    let revision = review.revision.clone();
    let frozen_cancel = cancel.clone();
    let (staging, frozen) = tokio::task::spawn_blocking(move || {
        let _io = io;
        let staging = tempfile::Builder::new()
            .prefix("cc-vault-validation-")
            .tempdir()
            .map_err(|_| invalid())?;
        let frozen = ProjectRoot::open(staging.path()).map_err(|_| invalid())?;
        root.freeze_to(&revision, &frozen, &frozen_cancel)
            .map_err(|_| invalid())?;
        Ok::<_, AppError>((staging, frozen))
    })
    .await
    .map_err(|_| invalid())??;
    resources.staging = Some(staging);
    let uploaded = resources
        .connection
        .as_ref()
        .ok_or_else(invalid)?
        .upload(&frozen, &review.revision)
        .await
        .map_err(|_| invalid())?;
    let runtime = observer.observe().await.map_err(|_| invalid())?;
    let scope = review
        .begin_validation(app, input.playbook.clone(), runtime)
        .await?;
    control
        .scope
        .set(Arc::downgrade(&scope))
        .map_err(|_| invalid())?;
    resources.scope = Some(scope.clone());
    let children = [VaultValidationPhase::Syntax, VaultValidationPhase::Lint]
        .into_iter()
        .flat_map(|phase| (0..2).map(move |index| (phase, index)))
        .map(|(phase, index)| scope.request_call(phase, index))
        .collect::<AppResult<Vec<_>>>()?;
    let raw = resources
        .connection
        .as_ref()
        .ok_or_else(invalid)?
        .prepare(
            &uploaded,
            &input.playbook,
            &input.parameters,
            &input.inventory,
            &input.targets,
            children,
        )
        .await
        .map_err(|_| invalid())?;
    let io = automation_io_slot(&scope.review.project.owner)?;
    let retained = scope.clone();
    tokio::task::spawn_blocking(move || {
        let _io = io;
        let current = retained
            .review
            .root
            .reopen_same_directory(&retained.review.project.path)
            .map_err(|_| invalid())?;
        let binding = current
            .bind_vault_input(
                &retained.review.revision,
                retained.review.binding.credential_id(),
            )
            .map_err(|_| invalid())?;
        retained
            .review
            .root
            .reopen_same_directory(&retained.review.project.path)
            .map_err(|_| invalid())?;
        Ok::<_, AppError>(binding)
    })
    .await
    .map_err(|_| invalid())??;
    let u = scope
        .review
        .project
        .unlock
        .upgrade()
        .ok_or(AppError::VaultLocked)?;
    let _profile = app.hold_session(&scope.review.project.owner).await?;
    let _unlock = scope.review.project.owner.hold_unlocked(&u).await?;
    scope.review.check_held(&u).await?;
    runner_current(&scope, &runner).await?;
    if cancel.is_cancelled() {
        return Err(invalid());
    }
    let validation = raw.seal(&scope.marker).map_err(|_| invalid())?;
    let cleanup = if matches!(
        runner.configuration,
        crate::automation_runners::AutomationRunnerConfig::Remote { .. }
    ) {
        ValidationCleanup::RemoteStopUnconfirmed
    } else {
        ValidationCleanup::LocalCloseCompleted
    };
    Ok(VaultValidationOutcome {
        validation,
        cleanup,
    })
}
#[cfg(test)]
mod tests;
