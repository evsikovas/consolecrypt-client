use super::*;
use futures::FutureExt;

pub(super) fn fs_permit() -> AppResult<OwnedSemaphorePermit> {
    registry::FS
        .get_or_init(|| Arc::new(Semaphore::new(2)))
        .clone()
        .try_acquire_owned()
        .map_err(|_| invalid("dependency_busy"))
}

impl AppCore {
    /// Explicit Native source/network review action. No bridge or Deserialize
    /// request exists yet: the caller must supply Core's validated exact pin.
    pub(crate) async fn automation_dependency_review(
        &self,
        project_id: String,
        expected_revision: u64,
        runner_id: String,
        runner_revision: u64,
        selection: DependencySourceSelection,
    ) -> AppResult<SourceReviewView> {
        self.dependency_review_inner(
            project_id,
            expected_revision,
            runner_id,
            runner_revision,
            selection,
        )
        .boxed()
        .await
    }
    async fn dependency_review_inner(
        &self,
        project_id: String,
        expected_revision: u64,
        runner_id: String,
        runner_revision: u64,
        selection: DependencySourceSelection,
    ) -> AppResult<SourceReviewView> {
        canonical(&project_id)?;
        canonical(&runner_id)?;
        let project = self.automation_project_context(&project_id).await?;
        if project.project.read_only || project.project.revision != Some(expected_revision) {
            return Err(invalid("dependency_context_changed"));
        }
        let u = project.unlock.upgrade().ok_or(AppError::VaultLocked)?;
        let (stamp, runner, generation, version, watched) = {
            let _profile = self.hold_session(&project.owner).await?;
            let _unlock = project.owner.hold_unlocked(&u).await?;
            let stamp = project_stamp(&project).await?;
            let runner = capture_runner(&project.owner, &runner_id, runner_revision).await?;
            let generation = project.owner.automation.unlocked_generation()?;
            let version = u.working().version();
            let snapshot = u.working().snapshot();
            let mut watched = BTreeMap::new();
            if let crate::automation_runners::AutomationRunnerConfig::Remote { host_id, .. } =
                &runner.configuration
            {
                watched.extend(crate::kubernetes_runtime::dependencies(
                    &snapshot,
                    crate::dto::parse_id("host_id", host_id)?,
                )?);
            }
            watched.extend(
                snapshot
                    .iter()
                    .filter(|(_, entry)| entry.kind() == cc_models::ObjectKind::KnownHost)
                    .map(|(id, entry)| (*id, entry.revision)),
            );
            if watched.len() > 4096 {
                return Err(invalid("dependency_context_budget"));
            }
            (stamp, runner, generation, version, watched)
        };
        drop(u);
        // Original Native capability + existing Core walker/preflight. No
        // project copy, artifact network or duplicate lock parser.
        let retained_io=managed::admit_io(self,&project).await?;
        let io=retained_io.clone();
        let permit = fs_permit()?;
        let source = project.root_selection.clone();
        let selected = selection.clone();
        let publication_profile = project.owner.dir.clone();
        let publication_managed = project.project.source_kind.as_deref() == Some("managed");
        let publication_project = Uuid::parse_str(&project_id).map_err(|_| invalid("dependency_invalid_request"))?;
        let (base, revision, old_lock, publication_reason) = tokio::task::spawn_blocking(move || {
            let _permit = permit;
            let _io = io;
            let base = Arc::new(source.open()?);
            let revision = base.revision().map_err(core_error)?;
            let checked = dependency_preflight(&base, &revision, &selected).map_err(core_error)?;
            let old_lock = checked.lock_sha256().map(str::to_owned);
            let publication_reason = publication_availability(
                publication_managed, publication_profile.as_deref(), publication_project, &base,
            );
            base.recheck()?;
            Ok::<_, AppError>((base, revision, old_lock, publication_reason))
        })
        .await
        .map_err(AppError::internal)??;
        project.check_after_io(self).await?;
        let context = Arc::new(Context {
            project,
            project_stamp: stamp,
            runner,
            generation,
            version,
            watched,
            base,
            revision,
            old_lock,
            selection,
        });
        let u = context.current(self).await?;
        let _profile = self.hold_session(&context.project.owner).await?;
        let _unlock = context.project.owner.hold_unlocked(&u).await?;
        // Fresh SQL recheck under final guards is the review linearization.
        if project_stamp(&context.project).await? != context.project_stamp
            || capture_runner(
                &context.project.owner,
                &context.runner.id,
                context.runner.revision,
            )
            .await?
            .stamp
                != context.runner.stamp
            || u.working().version() != context.version
            || context.project.owner.automation.unlocked_generation()? != context.generation
        {
            return Err(invalid("dependency_context_changed"));
        }
        let review = Uuid::new_v4();
        let operation = Uuid::new_v4();
        let view = SourceReviewView {
            version: 1,
            review_id: review,
            operation_id: operation,
            project_id: context.project.project.id.clone(),
            project_revision: expected_revision,
            runner_id,
            runner_revision,
            source: context.selection.clone(),
            base_digest: context.revision.digest.clone(),
            old_lock_sha256: context.old_lock.clone(),
            destination: context.selection.target(),
            network_origin: context.selection.origin(),
            publication_supported: publication_reason.is_none(),
            publication_reason,
            expires_in_seconds: 300,
        };
        context
            .project
            .owner
            .automation
            .dependencies
            .issue(Ticket {
                review,
                operation,
                context: context.clone(),
                expires: Instant::now() + Duration::from_secs(300),
                cancel: CancellationToken::new(),
            })?;
        Ok(view)
    }

    /// One-use review id is consumed before any admission/preflight attempt.
    /// Returns immediately; no queue, automatic retry or publication.
    pub(crate) async fn automation_dependency_start(
        &self,
        review: String,
    ) -> AppResult<DependencyView> {
        canonical(&review)?;
        let (owner, u) = self.unlocked().await?;
        let ticket = owner.automation.dependencies.consume(
            Uuid::parse_str(&review).map_err(|_| invalid("dependency_invalid_request"))?,
        )?;
        drop(u);
        let current = ticket.context.current(self).await?;
        if !current
            .working()
            .watch_revisions(ticket.context.watched.clone(), ticket.cancel.clone())
        {
            return Err(AppError::VaultLocked);
        }
        drop(current);
        let (op, reservation) = owner.automation.dependencies.admit(ticket)?;
        let fs = match fs_permit() {
            Ok(permit) => permit,
            Err(error) => {
                op.terminal(Phase::Failed, "dependency_busy");
                return Err(error);
            }
        };
        watch(self.clone(), &op);
        let view = op.view();
        let app = self.clone();
        let runtime = tokio::runtime::Handle::current();
        // This closure owns ALL resources. Dropping/aborting a caller never
        // releases admission while synchronous hashing/cleanup still runs.
        owner.automation.dependencies.launch_worker(move || {
            let _fs = fs;
            runtime.block_on(async move {
                let work = work(&app, &op, reservation).boxed().await;
                if let Err(error) = work {
                    let publication_unsupported = matches!(&error, AppError::InvalidInput { field, reason }
                        if field == "automation_dependency" && reason == "dependency_publication_unsupported");
                    op.terminal(
                        if op.cancel.is_cancelled() {
                            Phase::Cancelled
                        } else {
                            Phase::Failed
                        },
                        if op.cancel.is_cancelled() {
                            "dependency_cancelled"
                        } else if publication_unsupported {
                            "dependency_publication_unsupported"
                        } else {
                            "dependency_evidence_unavailable"
                        },
                    );
                }
            });
        })?;
        Ok(view)
    }

    pub(crate) async fn automation_dependency_status(
        &self,
        operation: String,
    ) -> AppResult<DependencyView> {
        canonical(&operation)?;
        let (owner, u) = self.unlocked().await?;
        drop(u);
        let op = owner
            .automation
            .dependencies
            .get(Uuid::parse_str(&operation).map_err(|_| invalid("dependency_invalid_request"))?)?;
        if op.is_published_metadata() {
            // Historical bounded DATA only; diff/accept/publish keep full fences.
            return publication::final_original_scope(self,&op.context,||op.view()).await;
        }
        drop(op.context.current(self).await?);
        if op.cancel.is_cancelled() {
            op.terminal(Phase::Cancelled, "dependency_cancelled")
        }
        Ok(op.view())
    }
    pub(crate) async fn automation_dependency_cancel(&self, operation: String) -> AppResult<()> {
        canonical(&operation)?;
        let (owner, u) = self.unlocked().await?;
        let _profile = self.hold_session(&owner).await?;
        let _unlock = owner.hold_unlocked(&u).await?;
        owner
            .automation
            .dependencies
            .cancel(Uuid::parse_str(&operation).map_err(|_| invalid("dependency_invalid_request"))?)
    }
}

fn watch(app: AppCore, op: &Arc<Operation>) {
    let weak = Arc::downgrade(op);
    tokio::spawn(async move {
        loop {
            let Some(op) = weak.upgrade() else { return };
            if Instant::now() >= op.expires {
                op.terminal(Phase::Expired, "dependency_expired");
            }
            if op.cancel.is_cancelled() {
                // Active/failed/incomplete exchanges remain fail-closed.
                // After successful Ready handoff, Resources alone owns bounded
                // disposal/close; do not race it by aborting its healthy peer.
                if let Some(peer) = op.cancellation_peer() {
                    peer.close().await;
                }
                return;
            }
            let Some(generation) = op.watch_generation() else { return; };
            if op.context.project.owner.automation.stopped.is_cancelled()
                || op.context.current(&app).await.is_err()
            {
                // A stale OLD-row result cannot retire an internally committing
                // or completed one-use publication. External changes still do.
                op.retire_watched(generation);
                continue;
            }
            let cancel = op.cancel.clone();
            let remaining = op.expires.saturating_duration_since(Instant::now());
            drop(op);
            tokio::select! {_ = cancel.cancelled()=>{},_ = tokio::time::sleep(remaining.min(Duration::from_millis(200)))=>{}}
        }
    });
}

async fn work(app: &AppCore, op: &Arc<Operation>, reservation: Reservation) -> AppResult<()> {
    if op.cancel.is_cancelled() || Instant::now() >= op.expires {
        return Err(AppError::Cancelled);
    }
    let checked = dependency_preflight(
        &op.context.base,
        &op.context.revision,
        &op.context.selection,
    )
    .map_err(core_error)?;
    if !preflight_matches(&op.context, &checked) {
        return Err(invalid("dependency_context_changed"));
    }
    drop(op.context.current(app).await?);
    if op.context.project.project.source_kind.as_deref() == Some("managed") {
        // Review's serialized availability is DATA only. Repeat real admission
        // before ANY owner factory/SSH/upload/download, in this bounded worker.
        let retained_io = managed::admit_io(app, &op.context.project).await?;
        let id=Uuid::parse_str(&op.context.project.project.id).map_err(|_| invalid("dependency_invalid_request"))?;
        if publication_availability(true, op.context.project.owner.dir.as_deref(), id, &op.context.base).is_some() {
            return Err(invalid("dependency_publication_unsupported"));
        }
        op.context.base.recheck()?;
        drop(op.context.current(app).await?);
        drop(retained_io);
    }
    let mut resources = registry::Resources {
        runtime: tokio::runtime::Handle::current(),
        connection: None,
        staging: None,
        reservation: Some(reservation),
        cleanup: op.cleanup.clone(),
        publication_io: Mutex::new(None),
        joined: op.joined.clone(),
        remote: matches!(
            &op.context.runner.configuration,
            crate::automation_runners::AutomationRunnerConfig::Remote { .. }
        ),
    };
    let owner = app
        .open_dependency_owner(
            &op.context.runner,
            &op.context.project.unlock,
            op.id,
            &op.cancel,
        )
        .boxed()
        .await?;
    *op.peer.lock().unwrap_or_else(|e| e.into_inner()) =
        Arc::downgrade(owner.private_control_peer());
    resources.connection = Some(owner);
    let owner = resources
        .connection
        .as_ref()
        .expect("owned dependency connection");
    if op.cancel.is_cancelled() {
        return Err(AppError::Cancelled);
    }
    drop(op.context.current(app).await?);
    op.phase(Phase::Preparing);
    resources.staging = Some(
        tempfile::Builder::new()
            .prefix("cc-dependency-preview-")
            .tempdir()
            .map_err(AppError::from)?,
    );
    let root = Arc::new(
        ProjectRoot::open(resources.staging.as_ref().expect("owned staging").path())
            .map_err(core_error)?,
    );
    let preview = owner
        .prepare_preview(
            &op.context.base,
            &op.context.revision,
            &op.context.selection,
            &root,
            &op.cancel,
        )
        .boxed()
        .await
        .map_err(core_error)?;
    op.phase(Phase::ArchiveVerified);
    let checked = dependency_preflight(
        &op.context.base,
        &op.context.revision,
        &op.context.selection,
    )
    .map_err(core_error)?;
    if !preflight_matches(&op.context, &checked) {
        return Err(invalid("dependency_context_changed"));
    }
    let u = op.context.current(app).await?;
    let _profile = app.hold_session(&op.context.project.owner).await?;
    let _unlock = op.context.project.owner.hold_unlocked(&u).await?;
    if project_stamp(&op.context.project).await? != op.context.project_stamp
        || capture_runner(
            &op.context.project.owner,
            &op.context.runner.id,
            op.context.runner.revision,
        )
        .await?
        .stamp
            != op.context.runner.stamp
        || u.working().version() != op.context.version
        || op.context.project.owner.automation.unlocked_generation()? != op.context.generation
    {
        return Err(invalid("dependency_context_changed"));
    }
    // Final guarded metadata fence is the result linearization point. This
    // snapshot grants no commit authority and must be rehashed before publish.
    op.ready(preview, root, resources)
}

/// Fixed display observation only: never serializable approval or an authority
/// to skip the next read-only check, publication checks, or source review.
fn publication_availability(managed: bool, profile: Option<&std::path::Path>, project: Uuid, root: &ProjectRoot) -> Option<&'static str> {
    if !managed { return Some("managed_project_required"); }
    let Some(profile)=profile else { return Some("publication_location_unavailable"); };
    match cc_automation_core::check_dependency_publication_base(profile, project, root) {
        Ok(()) => None,
        Err(cc_automation_core::AutomationError::PublicationUnsupported("excluded .git entry")) => Some("excluded_project_entries"),
        Err(cc_automation_core::AutomationError::PublicationUnsupported("unsupported publication platform")) => Some("publication_platform_unsupported"),
        Err(_) => Some("publication_location_unavailable"),
    }
}
