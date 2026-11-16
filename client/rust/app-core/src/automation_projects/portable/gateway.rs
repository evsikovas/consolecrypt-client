//! Original-issued private portable owner. The runtime holds only a cancellation
//! Weak in its EXISTING <=16 setup map. There is no ID lookup/start authority.
use super::*;
mod facade;
mod transfer;
use crate::automation_runners::owned_setup::{self, OwnedSetupCancellation, OwnedSetupSlot};
use std::{
    future::Future,
    pin::Pin,
    sync::Mutex,
    task::{Context, Poll},
    time::{Duration, Instant},
};
pub use transfer::PortablePasswordTransfer;

enum Body {
    Context {
        scope: PortableScope,
        project: String,
        revision: u64,
    },
    Export {
        request: String,
        password: SecretString,
        destination: PathBuf,
    },
    Inspect {
        scope: PortableScope,
        file: PathBuf,
        password: SecretString,
    },
    PrepareImport {
        scope: PortableScope,
        source: PathBuf,
        password: SecretString,
        requested_name: String,
        name: String,
        project: Uuid,
        transaction: Uuid,
        review: Uuid,
    },
    RecoverImport {
        scope: PortableScope,
        source: PathBuf,
        password: SecretString,
        project: String,
    },
    #[cfg(test)]
    Probe(tests::PayloadProbe),
}
enum Phase {
    Idle(Body),
    Running,
    ReadyImport(import::ReadyImport),
    Retiring,
    Joined,
}
// Every actual worker cohort is fenced separately. Late prepare completion
// cannot retire a later publication cohort; no extra map or admission grant.
struct Cohort {
    id: Uuid,
    done: bool,
    payload_retiring: bool,
    finished: CancellationToken,
}
mod import;
struct Control {
    app: AppCore,
    owner: Arc<Session>,
    unlock: Weak<Unlocked>,
    scope: PortableScope,
    generation: u64,
    deadline: Instant,
    cancel: CancellationToken,
    joined: CancellationToken,
    phase: Mutex<Phase>,
    cohort: Mutex<Cohort>,
    slot: Mutex<Option<OwnedSetupSlot>>,
    admission: Mutex<Option<Arc<tokio::sync::OwnedSemaphorePermit>>>,
    wide_io: Mutex<Option<Arc<tokio::sync::OwnedSemaphorePermit>>>,
    import_durable: std::sync::atomic::AtomicBool,
}
impl Control {
    fn retire(&self) {
        // No phase/setup-map lock is held while dropping owned resources.
        let slot = self.slot.lock().unwrap_or_else(|e| e.into_inner()).take();
        drop(slot);
        let admission = self
            .admission
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .take();
        drop(admission);
        let wide = self
            .wide_io
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .take();
        drop(wide);
        *self.phase.lock().unwrap_or_else(|e| e.into_inner()) = Phase::Joined;
        self.joined.cancel();
    }
    fn cancel(&self) {
        self.cancel.cancel();
        enum Retained {
            Body(Body),
            Ready(import::ReadyImport),
        }
        let retained = {
            let mut phase = self.phase.lock().unwrap_or_else(|e| e.into_inner());
            if matches!(*phase, Phase::Idle(_) | Phase::ReadyImport(_)) {
                let mut cohort = self.cohort.lock().unwrap_or_else(|e| e.into_inner());
                cohort.payload_retiring = true;
                match std::mem::replace(&mut *phase, Phase::Retiring) {
                    Phase::Idle(body) => Some(Retained::Body(body)),
                    Phase::ReadyImport(ready) => Some(Retained::Ready(ready)),
                    _ => unreachable!(),
                }
            } else {
                None
            }
        };
        if let Some(retained) = retained {
            // Explicit payload-first order. Final worker Drop may mark done
            // concurrently, but may NOT retire while zeroization is in flight.
            match retained {
                Retained::Body(body) => drop(body),
                Retained::Ready(ready) => drop(ready),
            }
            let retire = {
                let _phase = self.phase.lock().unwrap_or_else(|e| e.into_inner());
                let mut cohort = self.cohort.lock().unwrap_or_else(|e| e.into_inner());
                cohort.payload_retiring = false;
                cohort.done
            };
            if retire {
                self.retire();
            }
        }
    }
    fn workers_finished(&self, id: Uuid) {
        let retire = {
            let mut phase = self.phase.lock().unwrap_or_else(|e| e.into_inner());
            let mut cohort = self.cohort.lock().unwrap_or_else(|e| e.into_inner());
            if cohort.id != id {
                return;
            }
            cohort.done = true;
            cohort.finished.cancel();
            match &*phase {
                Phase::ReadyImport(_) | Phase::Joined => false,
                Phase::Running => {
                    *phase = Phase::Retiring;
                    true
                }
                Phase::Retiring => !cohort.payload_retiring,
                Phase::Idle(_) => false,
            }
        };
        if retire {
            self.retire();
        }
    }
    async fn check_original(&self) -> AppResult<()> {
        if self.cancel.is_cancelled() || Instant::now() >= self.deadline {
            return Err(AppError::Cancelled);
        }
        let u = self.unlock.upgrade().ok_or(AppError::VaultLocked)?;
        let _profile = self.app.hold_session(&self.owner).await?;
        let _unlock = self.owner.hold_unlocked(&u).await?;
        if self.owner.automation.unlocked_generation()? != self.generation
            || !self.scope.matches(&self.owner, &u)
        {
            return Err(invalid());
        }
        stop(&self.cancel)
    }
    fn take(self: &Arc<Self>) -> AppResult<(Body, Arc<WorkRegistration>)> {
        let mut phase = self.phase.lock().unwrap_or_else(|e| e.into_inner());
        if self.cancel.is_cancelled()
            || Instant::now() >= self.deadline
            || !matches!(*phase, Phase::Idle(_))
        {
            return Err(invalid());
        }
        let Phase::Idle(body) = std::mem::replace(&mut *phase, Phase::Running) else {
            unreachable!()
        };
        let id = Uuid::new_v4();
        *self.cohort.lock().unwrap_or_else(|e| e.into_inner()) = Cohort {
            id,
            done: false,
            payload_retiring: false,
            finished: CancellationToken::new(),
        };
        Ok((
            body,
            Arc::new(WorkRegistration {
                control: self.clone(),
                cohort: id,
            }),
        ))
    }
}
impl OwnedSetupCancellation for Control {
    fn cancel_owned(&self) {
        self.cancel();
    }
    fn joined_token(&self) -> CancellationToken {
        self.joined.clone()
    }
}
impl Drop for Control {
    fn drop(&mut self) {
        self.cancel.cancel();
        // May run outside Tokio. Do not rely on implicit struct field order for
        // the idle secret and setup-map reservation.
        let body = std::mem::replace(
            self.phase.get_mut().unwrap_or_else(|e| e.into_inner()),
            Phase::Retiring,
        );
        drop(body);
        drop(
            self.slot
                .get_mut()
                .unwrap_or_else(|e| e.into_inner())
                .take(),
        );
        drop(
            self.admission
                .get_mut()
                .unwrap_or_else(|e| e.into_inner())
                .take(),
        );
        drop(
            self.wide_io
                .get_mut()
                .unwrap_or_else(|e| e.into_inner())
                .take(),
        );
        self.joined.cancel();
    }
}
/// Exactly one shared registration per invocation. Every WorkerFence retains
/// an Arc. Caller cancellation never pretends its blocking workers joined.
pub(super) struct WorkRegistration {
    control: Arc<Control>,
    cohort: Uuid,
}
impl Drop for WorkRegistration {
    fn drop(&mut self) {
        self.control.workers_finished(self.cohort);
    }
}
#[derive(Clone)]
pub(super) struct OwnedBinding {
    registration: Arc<WorkRegistration>,
}
impl OwnedBinding {
    pub(super) async fn subject(
        &self,
        app: &AppCore,
        expected: &PortableScope,
        caller: &CancellationToken,
    ) -> AppResult<Subject> {
        let c = &self.registration.control;
        c.check_original().await?;
        let u = c.unlock.upgrade().ok_or(AppError::VaultLocked)?;
        let _profile = app.hold_session(&c.owner).await?;
        let _unlock = c.owner.hold_unlocked(&u).await?;
        if !expected.matches(&c.owner, &u)
            || c.owner.automation.unlocked_generation()? != c.generation
        {
            return Err(invalid());
        }
        let admission = c
            .admission
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .as_ref()
            .cloned()
            .ok_or_else(invalid)?;
        let fence = WorkerFence {
            registry_epoch: c.owner.automation.dependencies.local_filesystem_epoch()?,
            shutdown: u.sharing_shutdown.subscribe(),
            caller: caller.clone(),
            deadline: c.deadline,
            _admission: admission,
            _wide_io: c
                .wide_io
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .as_ref()
                .cloned(),
            _owned_registration: Some(self.registration.clone()),
        };
        fence.check()?;
        Ok(Subject {
            owner: c.owner.clone(),
            unlock: c.unlock.clone(),
            scope: expected.clone(),
            fence,
        })
    }
}
/// The secret-bearing future is always dropped BEFORE its root registration.
/// Worker closures that already took the password retain their own fence Arc.
struct Invocation {
    future: Option<Pin<Box<dyn Future<Output = AppResult<String>> + Send>>>,
    registration: Option<Arc<WorkRegistration>>,
}
impl Future for Invocation {
    type Output = AppResult<String>;
    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let result = self
            .future
            .as_mut()
            .expect("invocation polled after completion")
            .as_mut()
            .poll(cx);
        if result.is_ready() {
            drop(self.future.take());
            drop(self.registration.take());
        }
        result
    }
}
impl Drop for Invocation {
    fn drop(&mut self) {
        drop(self.future.take());
        drop(self.registration.take());
    }
}
/// Private issued object, no Clone/Serialize/public ID. A future Bridge wrapper
/// may put THIS in an opaque Arc; never resolve it through a later global Core.
pub struct PortableOperation {
    control: Arc<Control>,
}
impl std::fmt::Debug for PortableOperation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("PortableOperation([OPAQUE])")
    }
}
impl Drop for PortableOperation {
    fn drop(&mut self) {
        self.control.cancel();
    }
}
impl PortableOperation {
    pub fn cancel(&self) {
        self.control.cancel();
    }
    pub async fn joined(&self) {
        self.control.joined.cancelled().await;
    }
    pub async fn start(&self) -> AppResult<String> {
        if let Err(error) = self.control.check_original().await {
            self.control.cancel();
            return Err(error);
        }
        let (body, registration) = self.control.take()?;
        let binding = OwnedBinding {
            registration: registration.clone(),
        };
        let c = self.control.clone();
        let future = Box::pin(async move {
            let value = match body {
                Body::Context {
                    scope,
                    project,
                    revision,
                } => serde_json::to_value(
                    c.app
                        .portable_context_bound(scope, project, revision, &c.cancel, Some(&binding))
                        .await?,
                )
                .map_err(|_| invalid())?,
                Body::Export {
                    request,
                    password,
                    destination,
                } => serde_json::to_value(
                    c.app
                        .portable_export_bound(
                            request,
                            password,
                            destination,
                            &c.cancel,
                            Some(&binding),
                        )
                        .await?,
                )
                .map_err(|_| invalid())?,
                Body::Inspect {
                    scope,
                    file,
                    password,
                } => serde_json::to_value(
                    c.app
                        .portable_inspect_bound(scope, file, password, &c.cancel, Some(&binding))
                        .await?,
                )
                .map_err(|_| invalid())?,
                Body::PrepareImport {
                    scope,
                    source,
                    password,
                    requested_name,
                    name,
                    project,
                    transaction,
                    review,
                } => serde_json::to_value(
                    c.prepare_import(
                        scope,
                        source,
                        password,
                        requested_name,
                        name,
                        project,
                        transaction,
                        review,
                        None,
                        &binding,
                    )
                    .await?,
                )
                .map_err(|_| invalid())?,
                Body::RecoverImport {
                    scope,
                    source,
                    password,
                    project,
                } => {
                    let recovery = import::load_recovery(&c, &project).await?;
                    serde_json::to_value(
                        c.prepare_import(
                            scope,
                            source,
                            password,
                            recovery.name.clone(),
                            recovery.name.clone(),
                            recovery.project,
                            recovery.transaction,
                            Uuid::new_v4(),
                            Some(recovery),
                            &binding,
                        )
                        .await?,
                    )
                    .map_err(|_| invalid())?
                }
                #[cfg(test)]
                Body::Probe(_) => return Err(invalid()),
            };
            c.check_original().await?;
            let json = serde_json::to_string(&value).map_err(|_| invalid())?;
            if json.len() > 512 * 1024 {
                return Err(invalid());
            }
            Ok(json)
        });
        let invocation = Invocation {
            future: Some(future),
            registration: Some(registration),
        };
        tokio::select! {biased; _=self.control.cancel.cancelled()=>Err(AppError::Cancelled),result=invocation=>result}
    }
}
impl AppCore {
    async fn portable_register(
        &self,
        scope: PortableScope,
        body: Body,
        ttl: Duration,
    ) -> AppResult<PortableOperation> {
        self.portable_register_build(scope, move || Ok(body), ttl)
            .await
    }
    async fn portable_register_build(
        &self,
        scope: PortableScope,
        body: impl FnOnce() -> AppResult<Body> + Send,
        ttl: Duration,
    ) -> AppResult<PortableOperation> {
        if ttl.is_zero() || ttl > Duration::from_secs(300) {
            return Err(invalid());
        }
        let (owner, u) = self.unlocked().await?;
        let _profile = self.hold_session(&owner).await?;
        let _unlock = owner.hold_unlocked(&u).await?;
        if !scope.matches(&owner, &u) {
            return Err(invalid());
        }
        let generation = owner.automation.unlocked_generation()?;
        let admission = owner
            .automation
            .prepare_slots
            .clone()
            .try_acquire_owned()
            .map_err(|_| invalid())?;
        let c = Arc::new(Control {
            app: self.clone(),
            owner: owner.clone(),
            unlock: Arc::downgrade(&u),
            scope,
            generation,
            deadline: Instant::now() + ttl,
            cancel: CancellationToken::new(),
            joined: CancellationToken::new(),
            phase: Mutex::new(Phase::Idle(body()?)),
            cohort: Mutex::new(Cohort {
                id: Uuid::nil(),
                done: true,
                payload_retiring: false,
                finished: CancellationToken::new(),
            }),
            slot: Mutex::new(None),
            admission: Mutex::new(Some(Arc::new(admission))),
            wide_io: Mutex::new(None),
            import_durable: std::sync::atomic::AtomicBool::new(false),
        });
        let identity: Arc<dyn OwnedSetupCancellation> = c.clone();
        // Hold slot binding before the map Weak becomes visible. A concurrent
        // runtime callback is outside the map and waits for this binding.
        let mut slot = c.slot.lock().unwrap_or_else(|e| e.into_inner());
        match owned_setup::register(
            &owner,
            Arc::downgrade(&identity),
            c.cancel.clone(),
            c.deadline,
        ) {
            Ok(value) => *slot = Some(value),
            Err(error) => {
                drop(slot);
                return Err(error);
            }
        }
        drop(slot);
        drop(identity);
        let weak = Arc::downgrade(&c);
        let joined = c.joined.clone();
        let until = c.deadline;
        tokio::spawn(async move {
            tokio::select! { biased;
                _ = joined.cancelled() => {},
                _ = tokio::time::sleep_until(until.into()) => { if let Some(c) = weak.upgrade() { c.cancel(); } }
            }
        });
        Ok(PortableOperation { control: c })
    }
    pub(crate) async fn portable_register_context(
        &self,
        scope: PortableScope,
        project: String,
        revision: u64,
    ) -> AppResult<PortableOperation> {
        project_id(&project)?;
        if revision == 0 {
            return Err(invalid());
        }
        self.portable_register(
            scope.clone(),
            Body::Context {
                scope,
                project,
                revision,
            },
            Duration::from_secs(300),
        )
        .await
    }
    pub(crate) async fn portable_register_export(
        &self,
        request: String,
        password: SecretString,
        destination: PathBuf,
    ) -> AppResult<PortableOperation> {
        password_bound(&password)?;
        selected_file(&destination)?;
        let parsed = PortableExportRequest::parse(&request)?;
        self.portable_register(
            parsed.scope,
            Body::Export {
                request,
                password,
                destination,
            },
            Duration::from_secs(300),
        )
        .await
    }
    pub(crate) async fn portable_register_inspect(
        &self,
        scope: PortableScope,
        file: PathBuf,
        password: SecretString,
    ) -> AppResult<PortableOperation> {
        password_bound(&password)?;
        selected_file(&file)?;
        self.portable_register(
            scope.clone(),
            Body::Inspect {
                scope,
                file,
                password,
            },
            Duration::from_secs(300),
        )
        .await
    }
}
#[cfg(test)]
mod tests;
