//! One dedicated sidecar retained by the EXISTING execution review/job owner.
//! No new registry, public ID authority, general resolver or password callback.
use super::owner::{Resources, VaultValidationOperations};
use super::*;
use crate::automation_runners::dependency::{capture_runner, DependencyRunner};
use cc_automation_core::{
    AdmittedVaultPasswordLease, RunParameters, ValidationHelperConnection, ValidationPeerObserver,
    VaultJobControlRequest, VaultJobResponder, VaultPasswordPurpose, VaultPasswordRequest,
    VaultRunBinding, VaultValidationControlFuture, VaultValidationControlRequest,
    VaultValidationHandler, VaultValidationResponder, VerifiedJob,
};
use std::sync::{Mutex, OnceLock, Weak};
use tokio::sync::{mpsc, oneshot, Notify};

enum Phase {
    Validation,
    Review {
        parameters_digest: String,
        valid: bool,
    },
    Material {
        request: VaultPasswordRequest,
        lease: Option<AdmittedVaultPasswordLease>,
        authority: CancellationToken,
        deadline: Instant,
    },
    Retired,
}
enum Command {
    FinishValidation(oneshot::Sender<AppResult<()>>),
    OpenMaterial(oneshot::Sender<AppResult<()>>),
}
struct PendingAck(Option<CancellationToken>);
impl Drop for PendingAck {
    fn drop(&mut self) {
        if let Some(cancel) = self.0.take() {
            cancel.cancel();
        }
    }
}
struct Control {
    app: Weak<crate::app::Inner>,
    registry: Arc<VaultValidationOperations>,
    operation: Uuid,
    runner: Arc<DependencyRunner>,
    scope: Mutex<Option<Arc<NativeVaultValidation>>>,
    observer: OnceLock<ValidationPeerObserver>,
    binding: OnceLock<VaultRunBinding>,
    phase: Mutex<Phase>,
    cancel: CancellationToken,
    joined: CancellationToken,
    changed: Notify,
    review_deadline: Instant,
    retained_transport: std::sync::atomic::AtomicBool,
}
impl Control {
    fn scope(&self) -> AppResult<Arc<NativeVaultValidation>> {
        self.scope
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
            .ok_or_else(invalid)
    }
    fn deadline(&self) -> Instant {
        match &*self.phase.lock().unwrap_or_else(|e| e.into_inner()) {
            Phase::Material { deadline, .. } => *deadline,
            _ => self.review_deadline,
        }
    }
    fn retire(&self) {
        self.cancel.cancel();
        let old = std::mem::replace(
            &mut *self.phase.lock().unwrap_or_else(|e| e.into_inner()),
            Phase::Retired,
        );
        if let Phase::Material { authority, .. } = &old {
            authority.cancel();
        }
        drop(old); // password zeroization is outside every state/map mutex
        let scope = self.scope.lock().unwrap_or_else(|e| e.into_inner()).take();
        if let Some(scope) = scope {
            scope.revoke();
        }
        self.changed.notify_one();
    }
}
impl VaultValidationHandler for Control {
    fn revoke(&self) {
        self.retire();
    }
    fn handle(
        &self,
        request: VaultValidationControlRequest,
        mut responder: VaultValidationResponder,
    ) -> VaultValidationControlFuture {
        let scope = if matches!(
            *self.phase.lock().unwrap_or_else(|e| e.into_inner()),
            Phase::Validation
        ) {
            self.scope().ok()
        } else {
            None
        };
        let app = self.app.clone();
        let observer = self.observer.get().cloned();
        let runner = self.runner.clone();
        let cancel = self.cancel.clone();
        Box::pin(async move {
            let result: AppResult<()> = async {
                if cancel.is_cancelled() {
                    return Err(invalid());
                }
                let app = AppCore {
                    inner: app.upgrade().ok_or_else(invalid)?,
                };
                let scope = scope.ok_or_else(invalid)?;
                let observed = responder
                    .bound_runtime(
                        observer
                            .ok_or_else(invalid)?
                            .observe()
                            .await
                            .map_err(|_| invalid())?,
                    )
                    .map_err(|_| invalid())?;
                let mut password = scope.phase(&app, &request.body, observed).await?;
                let u = scope
                    .review
                    .project
                    .unlock
                    .upgrade()
                    .ok_or(AppError::VaultLocked)?;
                let _profile = app.hold_session(&scope.review.project.owner).await?;
                let _unlock = scope.review.project.owner.hold_unlocked(&u).await?;
                let current =
                    capture_runner(&scope.review.project.owner, &runner.id, runner.revision)
                        .await?;
                if current.stamp != runner.stamp || cancel.is_cancelled() {
                    return Err(invalid());
                }
                scope.review.check_held(&u).await?;
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
    fn handle_job(
        &self,
        request: VaultJobControlRequest,
        mut responder: VaultJobResponder,
    ) -> VaultValidationControlFuture {
        // Consume the ONLY already-issued lease before any Native wait. Neither
        // this handler nor a locked job can re-open the general secret store.
        let lease = {
            let mut phase = self.phase.lock().unwrap_or_else(|e| e.into_inner());
            match &mut *phase {
                Phase::Material { lease, .. } => lease.take(),
                _ => None,
            }
        };
        let observer = self.observer.get().cloned();
        let binding = self.binding.get().cloned();
        Box::pin(async move {
            let result: AppResult<()> = async {
                let mut lease = lease.ok_or_else(invalid)?;
                let runtime = responder
                    .bound_runtime(
                        observer
                            .ok_or_else(invalid)?
                            .observe()
                            .await
                            .map_err(|_| invalid())?,
                    )
                    .map_err(|_| invalid())?;
                let binding = binding.ok_or_else(invalid)?;
                if runtime.environment_digest() != binding.environment_digest()
                    || runtime.helper_digest() != binding.helper_digest()
                {
                    return Err(invalid());
                }
                responder
                    .password(&mut lease, &request.body)
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

/// Retained by PreparedReview then the same admitted job. Drop requests cleanup;
/// joined is signalled only after actual owned handler/peer/staging retirement.
pub(crate) struct VaultJobSession {
    control: Arc<Control>,
    commands: mpsc::Sender<Command>,
}
impl Drop for VaultJobSession {
    fn drop(&mut self) {
        self.control.retire();
    }
}
impl VaultJobSession {
    pub(crate) fn binding(&self) -> AppResult<VaultRunBinding> {
        self.control.binding.get().cloned().ok_or_else(invalid)
    }
    pub(crate) async fn validated(
        &self,
        proof: VerifiedValidation,
    ) -> AppResult<VerifiedValidation> {
        let scope = self.control.scope()?;
        let proof = scope.seal_result(proof)?; // trusted marker BEFORE any UI/AI output
        {
            let mut phase = self.control.phase.lock().unwrap_or_else(|e| e.into_inner());
            if self.control.cancel.is_cancelled() || !matches!(*phase, Phase::Validation) {
                return Err(invalid());
            }
            *phase = Phase::Review {
                parameters_digest: proof.parameters_digest().into(),
                valid: proof.is_valid(),
            };
        }
        let (send, receive) = oneshot::channel();
        self.commands
            .try_send(Command::FinishValidation(send))
            .map_err(|_| invalid())?;
        tokio::select! {biased;_=self.control.cancel.cancelled()=>Err(invalid()),result=receive=>result.map_err(|_|invalid())?}?;
        Ok(proof)
    }
    pub(crate) async fn resolve_pending_job(
        &self,
        app: &AppCore,
        verified: &VerifiedJob,
        job_cancel: &CancellationToken,
    ) -> AppResult<NativeVaultPassword> {
        let scope = self.control.scope()?;
        {
            let phase = self.control.phase.lock().unwrap_or_else(|e| e.into_inner());
            if !matches!(&*phase,Phase::Review{parameters_digest,valid:true} if parameters_digest==&verified.manifest().parameters_digest)
                || self.control.cancel.is_cancelled()
            {
                return Err(invalid());
            }
        }
        scope
            .review
            .resolve_for_job_shared(app, verified, job_cancel)
            .await
    }
    pub(crate) async fn check_before_material(
        &self,
        u: &crate::session::Unlocked,
    ) -> AppResult<()> {
        let scope = self.control.scope()?;
        let current = capture_runner(
            &scope.review.project.owner,
            &self.control.runner.id,
            self.control.runner.revision,
        )
        .await?;
        if current.stamp != self.control.runner.stamp {
            return Err(invalid());
        }
        // SQL first, exact selected credential/secret metadata LAST. No await
        // follows before the caller's synchronous sealed-material transition.
        scope.review.check_held(u).await
    }
    pub(crate) fn material_live(&self) -> bool {
        !self.control.cancel.is_cancelled()
            && matches!(&*self.control.phase.lock().unwrap_or_else(|e|e.into_inner()),
            Phase::Material{deadline,..} if Instant::now()<*deadline)
    }
    /// Caller holds ORIGINAL profile/unlock guards after actual process-atomic job ACK
    /// and live_jobs insertion. No await/secret destruction under registry lock.
    pub(crate) fn commit_material(
        &self,
        verified: &VerifiedJob,
        password: NativeVaultPassword,
        u: &crate::session::Unlocked,
        diagnostic:&cc_ssh_core::DiagnosticCapture,
    ) -> AppResult<oneshot::Receiver<AppResult<()>>> {
        let scope = self.control.scope()?;
        let request = VaultPasswordRequest {
            version: 1,
            purpose: VaultPasswordPurpose::AnsibleVaultPassword,
            job_id: verified.manifest().job_id,
            manifest_digest: verified.digest().into(),
            revision_digest: verified.manifest().revision_digest.clone(),
            reference_sha256: scope.review.binding.reference_sha256().into(),
        };
        let NativeVaultPassword { review, lease } = password;
        lease.retain_diagnostic(diagnostic).map_err(|_|invalid())?;
        let authority = CancellationToken::new();
        // Metadata-only revocation survives via the existing parked guard
        // mechanism. It retains no Unlocked/resolver and is separate from the
        // original validation-scope token that is deliberately retired below.
        if !u
            .working()
            .watch_revisions(scope.review.fences.clone(), authority.clone())
        {
            return Err(invalid());
        }
        let mut sealed = Some(
            lease
                .into_admitted_job(verified, authority.clone())
                .map_err(|_| invalid())?,
        );
        drop(review);
        let deadline = sealed.as_ref().ok_or_else(invalid)?.deadline();
        let control = &self.control;
        control.registry.commit_material(control.operation,verified,||{
            let mut phase=control.phase.lock().unwrap_or_else(|e|e.into_inner());
            if control.cancel.is_cancelled()||!matches!(&*phase,Phase::Review{parameters_digest,valid:true} if parameters_digest==&verified.manifest().parameters_digest){return Err(invalid());}
            *phase=Phase::Material{request,lease:sealed.take(),authority,deadline};Ok(())
        })?;
        drop(sealed);
        let (send, receive) = oneshot::channel();
        if self.commands.try_send(Command::OpenMaterial(send)).is_err() {
            self.control.retire();
            return Err(invalid());
        }
        self.control.changed.notify_one();
        Ok(receive)
    }
    pub(crate) async fn close_and_join(&self) {
        self.control.retire();
        self.control.joined.cancelled().await;
    }
}

impl AppCore {
    pub(crate) async fn automation_open_vault_job_session(
        &self,
        project_id: String,
        project_revision: u64,
        digest: String,
        credential: String,
        runner_id: String,
        runner_revision: u64,
        playbook: String,
        root: Arc<managed::OpenedProjectRoot>,
        revision: ProjectRevision,
        cancel: &CancellationToken,
    ) -> AppResult<VaultJobSession> {
        let review = self
            .automation_vault_capture(project_id, project_revision, digest, credential, cancel)
            .await?;
        self.open_vault_review_session(review,runner_id,runner_revision,playbook,root,revision,cancel).await
    }

    pub(crate) async fn automation_open_ephemeral_vault_job_session(
        &self, original:AutomationProjectContext, digest:String, password:cc_models::secret::SecretValue,
        runner_id:String, runner_revision:u64, playbook:String,
        root:Arc<managed::OpenedProjectRoot>, revision:ProjectRevision,cancel:&CancellationToken,
    )->AppResult<VaultJobSession> {
        let review=self.automation_ephemeral_vault_capture(original,digest,password,cancel).await?;
        self.open_vault_review_session(review,runner_id,runner_revision,playbook,root,revision,cancel).await
    }

    async fn open_vault_review_session(
        &self, review:VaultCredentialReview, runner_id:String, runner_revision:u64, playbook:String,
        root:Arc<managed::OpenedProjectRoot>,revision:ProjectRevision,cancel:&CancellationToken,
    )->AppResult<VaultJobSession> {
        let registry = review.project.owner.automation.vault_validation.clone();
        let runner = {
            let u = review
                .project
                .unlock
                .upgrade()
                .ok_or(AppError::VaultLocked)?;
            let _profile = self.hold_session(&review.project.owner).await?;
            let _unlock = review.project.owner.hold_unlocked(&u).await?;
            review.check_held(&u).await?;
            Arc::new(capture_runner(&review.project.owner, &runner_id, runner_revision).await?)
        };
        let app = self.clone();
        let retained_registry = registry.clone();
        let (receive,operation_cancel)=registry.launch_job(runner_id,cancel,move|operation,cancel,mut resources,send|async move {
            let control=Arc::new(Control{app:Arc::downgrade(&app.inner),registry:retained_registry.clone(),operation,runner:runner.clone(),
                scope:Mutex::new(None),observer:OnceLock::new(),binding:OnceLock::new(),phase:Mutex::new(Phase::Validation),
                cancel:cancel.clone(),joined:CancellationToken::new(),changed:Notify::new(),review_deadline:Instant::now()+Duration::from_secs(300),
                retained_transport:std::sync::atomic::AtomicBool::new(false)});
            let (commands,mut requests)=mpsc::channel(2);
            let result:AppResult<()>=async {
                retained_registry.bind_control(operation,control.clone())?;
                let connection=app.open_validation_owner(&runner,&review.project.unlock,operation,&cancel,control.clone()).await?;
                let observer=connection.observer();control.observer.set(observer.clone()).map_err(|_|invalid())?;
                resources.connection=Some(connection);
                let watched=control.clone();let transport=observer.clone();let stop=resources.watcher_stop.clone();
                resources.watcher=Some(tokio::spawn(async move {
                    loop {
                        tokio::select!{biased;_=stop.cancelled()=>break,
                            _=watched.cancel.cancelled()=>{
                                if !watched.retained_transport.load(std::sync::atomic::Ordering::Acquire){transport.cancel();}break;
                            },
                            _=tokio::time::sleep_until(watched.deadline().into())=>{
                                watched.retire();
                                if !watched.retained_transport.load(std::sync::atomic::Ordering::Acquire){transport.cancel();}break;
                            },_=watched.changed.notified()=>continue}
                    }
                }));
                let io=automation_io_slot(&review.project.owner)?;
                let upload_peer=observer.clone();let handle=tokio::runtime::Handle::current();
                let uploaded=tokio::task::spawn_blocking(move||{
                    let _io=io;root.recheck().map_err(|_|invalid())?;
                    let uploaded=handle.block_on(upload_peer.upload(&root,&revision)).map_err(|_|invalid())?;
                    root.recheck().map_err(|_|invalid())?;Ok::<_,AppError>(uploaded)
                }).await.map_err(|_|invalid())??;
                let runtime=observer.observe().await.map_err(|_|invalid())?;
                let scope=review.begin_validation(&app,playbook,runtime).await?;
                let observed=observer.observe().await.map_err(|_|invalid())?;
                let binding=VaultRunBinding::from_native(&scope.review.binding,&observed,scope.scope,operation).map_err(|_|invalid())?;
                control.binding.set(binding.clone()).map_err(|_|invalid())?;
                *control.scope.lock().unwrap_or_else(|e|e.into_inner())=Some(scope.clone());resources.scope=Some(scope.clone());
                let children=[VaultValidationPhase::Syntax,VaultValidationPhase::Lint].into_iter()
                    .flat_map(|phase|(0..2).map(move|i|(phase,i))).map(|(phase,i)|scope.request_call(phase,i)).collect::<AppResult<Vec<_>>>()?;
                resources.connection.as_ref().ok_or_else(invalid)?.open_default_validation_sidecar(&uploaded,&binding,children).await.map_err(|_|invalid())?;
                // Healthy Ready handoff may retain trusted disposal only. A
                // failed/partial exchange never reaches this sticky transition.
                {let phase=control.phase.lock().unwrap_or_else(|e|e.into_inner());
                    if cancel.is_cancelled()||!matches!(*phase,Phase::Validation){return Err(invalid());}
                    control.retained_transport.store(true,std::sync::atomic::Ordering::Release);}
                let session=VaultJobSession{control:control.clone(),commands};
                if send.send(Ok(session)).is_err(){return Err(invalid());}
                loop {
                    let deadline=control.deadline();
                    let command=tokio::select!{biased;_=cancel.cancelled()=>break,
                        _=tokio::time::sleep_until(deadline.into())=>{control.retire();break;},
                        command=requests.recv()=>match command{Some(c)=>c,None=>break}};
                    match command {
                        Command::FinishValidation(done)=>{
                            let result=resources.connection.as_ref().ok_or_else(invalid)?.finish_default_validation_sidecar().await.map_err(|_|invalid());
                            let failed=result.is_err();let _=done.send(result);if failed{break;}
                        },
                        Command::OpenMaterial(done)=>{
                            // The retired original scope cannot issue material.
                            let original=control.scope.lock().unwrap_or_else(|e|e.into_inner()).take();
                            if let Some(original)=original {original.revoke();}drop(resources.scope.take());
                            let capsule={let phase=control.phase.lock().unwrap_or_else(|e|e.into_inner());
                                match &*phase{Phase::Material{request,..}=>VaultPasswordRequest{version:request.version,purpose:VaultPasswordPurpose::AnsibleVaultPassword,
                                    job_id:request.job_id,manifest_digest:request.manifest_digest.clone(),revision_digest:request.revision_digest.clone(),reference_sha256:request.reference_sha256.clone()},_=>return Err(invalid())}};
                            let result=resources.connection.as_ref().ok_or_else(invalid)?.open_admitted_job_sidecar(&capsule).await.map_err(|_|invalid());
                            let failed=result.is_err();let _=done.send(result);if failed{break;}
                        },
                    }
                }
                Ok(())
            }.await;
            // send may have been consumed by a successful ACK; errors before it
            // drop the sender and return fixed unavailability to the caller.
            let _=result;
            control.retire();drop(resources.scope.take());
            let retained=resources.finish().await;drop(retained);
            control.joined.cancel();
        })?;
        let mut waiting = PendingAck(Some(operation_cancel));
        let result = receive.await.map_err(|_| invalid())?;
        if result.is_ok() {
            waiting.0.take();
        }
        result
    }
}
