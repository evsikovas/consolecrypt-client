//! New retained import purpose. Existing export/read-only inspect are unchanged.
//! Genuine package stays zeroizing and bounded until explicit accept/retirement.
use super::*;
use cc_automation_core::portable_package::{PortableImport, PortablePackageSummary};

pub(super) struct ReadyImport {
    package: Option<PortableImport>,
    source_parent: Arc<ProjectRoot>,
    source_parent_path: PathBuf,
    source_leaf: String,
    requested_name: String,
    name: String,
    project: Uuid,
    transaction: Uuid,
    review: Uuid,
    review_digest: String,
    encrypted_sha256: String,
    summary: PortablePackageSummary,
    publication_supported: bool,
    publication_reason: Option<&'static str>,
    recovery: Option<import_catalog::Recovery>,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct ReadyView {
    version: u32,
    scope: PortableScope,
    review_id: String,
    review_digest: String,
    project_id: String,
    requested_name: String,
    name: String,
    encrypted_sha256: String,
    summary: PortablePackageSummary,
    publication_supported: bool,
    publication_reason: Option<&'static str>,
    remaining_millis: u64,
}
impl ReadyImport {
    fn view(&self, c: &Control) -> AppResult<ReadyView> {
        let remaining = c
            .deadline
            .saturating_duration_since(Instant::now())
            .as_millis()
            .min(300_000) as u64;
        if remaining == 0 || c.cancel.is_cancelled() {
            return Err(AppError::Cancelled);
        }
        Ok(ReadyView {
            version: 1,
            scope: c.scope.clone(),
            review_id: self.review.to_string(),
            review_digest: self.review_digest.clone(),
            project_id: self.project.to_string(),
            requested_name: self.requested_name.clone(),
            name: self.name.clone(),
            encrypted_sha256: self.encrypted_sha256.clone(),
            summary: self.summary.clone(),
            publication_supported: self.publication_supported,
            publication_reason: self.publication_reason,
            remaining_millis: remaining,
        })
    }
}
fn support(
    profile: Option<&Path>,
    project: Uuid,
    package: &PortableImport,
) -> (bool, Option<&'static str>) {
    let Some(profile) = profile else {
        return (false, Some("publication_location_unavailable"));
    };
    match cc_automation_core::portable_publication::check_portable_import_base(
        profile, project, package,
    ) {
        Ok(()) => (true, None),
        Err(cc_automation_core::AutomationError::PublicationUnsupported(reason)) => {
            if reason.contains("platform") || (cfg!(not(any(windows, target_os = "linux")))) {
                (false, Some("publication_platform_unsupported"))
            } else if reason.contains("names") || reason.contains("executable") {
                (false, Some("package_tree_unsupported"))
            } else {
                (false, Some("publication_location_unavailable"))
            }
        }
        Err(cc_automation_core::AutomationError::QueueFull) => {
            (false, Some("import_capacity_reached"))
        }
        Err(_) => (false, Some("publication_location_unavailable")),
    }
}
impl Control {
    pub(super) async fn prepare_import(
        self: &Arc<Self>,
        scope: PortableScope,
        source: PathBuf,
        password: SecretString,
        requested_name: String,
        name: String,
        project: Uuid,
        transaction: Uuid,
        review: Uuid,
        recovery: Option<import_catalog::Recovery>,
        binding: &OwnedBinding,
    ) -> AppResult<ReadyView> {
        password_bound(&password)?;
        self.check_original().await?;
        let subject = binding.subject(&self.app, &scope, &self.cancel).await?;
        let (parent_path, leaf) = selected_file(&source)?;
        let profile = self.owner.dir.clone();
        let token = self.cancel.clone();
        let parent_name = parent_path.clone();
        let c_scope = self.scope.clone();
        let ready=work(&subject,move |fence| {
            fence.check()?;
            let parent=Arc::new(ProjectRoot::open(&parent_path).map_err(|_|invalid())?);
            parent.reopen_same_directory(&parent_path).map_err(|_|invalid())?;
            let bytes=portable_package::read_encrypted_file(&parent,&leaf).map_err(|_|invalid())?;
            let package=portable_package::decrypt_package(&bytes,&password,&token).map_err(|_|invalid())?;
            let encrypted_sha256=package.encrypted_sha256().ok_or_else(invalid)?.to_owned();
            if encrypted_sha256!=digest(&bytes){return Err(invalid());}
            let summary=package.summary();
            let (publication_supported,publication_reason)=if let Some(recovery)=&recovery {
                let profile=profile.as_deref().ok_or_else(invalid)?;
                if encrypted_sha256!=recovery.encrypted_sha256||summary.portable_digest!=recovery.portable_digest{return Err(invalid());}
                let source=recovery_source(profile,recovery)?;
                let ack=recovery_ack(recovery)?;
                cc_automation_core::portable_publication::check_portable_project_recovery(&source,&package,&recovery.fingerprint,&ack,&token).map_err(|_|invalid())?;
                (true,None)
            }else{support(profile.as_deref(),project,&package)};
            parent.reopen_same_directory(&parent_path).map_err(|_|invalid())?;
            fence.check()?;
            let raw=serde_json::to_vec(&json!({"domain":"ConsoleCrypt PortableImportReview v1","scope":c_scope,"project":project,
                "transaction":transaction,"review":review,"source":source,"requestedName":requested_name,"name":name,
                "encryptedSha256":encrypted_sha256,"summary":summary,"recovery":recovery})).map_err(|_|invalid())?;
            let review_digest=digest(&raw);
            Ok(ReadyImport{package:Some(package),source_parent:parent,source_parent_path:parent_name,source_leaf:leaf,
                requested_name,name,project,transaction,review,review_digest,encrypted_sha256,summary,publication_supported,publication_reason,recovery})
        }).await?;
        self.check_original().await?;
        if let Some(recovery) = &ready.recovery {
            if load_recovery(self, &recovery.project.to_string())
                .await?
                .row_sha
                != recovery.row_sha
            {
                return Err(invalid());
            }
        }
        let view = ready.view(self)?;
        let mut phase = self.phase.lock().unwrap_or_else(|e| e.into_inner());
        if self.cancel.is_cancelled() || !matches!(*phase, Phase::Running) {
            return Err(AppError::Cancelled);
        }
        *phase = Phase::ReadyImport(ready);
        Ok(view)
    }
}

fn failure(c: &Control) -> AppError {
    AppError::invalid(
        "portable_import",
        if c.import_durable.load(std::sync::atomic::Ordering::Acquire) {
            "portable_import_recovery_required"
        } else {
            "portable_context_changed"
        },
    )
}
fn held(c: &Control, u: &Unlocked) -> AppResult<()> {
    if c.cancel.is_cancelled()
        || Instant::now() >= c.deadline
        || c.owner.automation.stopped.is_cancelled()
        || c.owner.automation.unlocked_generation()? != c.generation
        || !c.scope.matches(&c.owner, u)
        || *u.sharing_shutdown.borrow()
    {
        return Err(failure(c));
    }
    Ok(())
}
#[derive(Clone)]
struct SqlAck {
    revision: u64,
    row_sha: String,
}
async fn original(c: &Control) -> AppResult<()> {
    let u = c.unlock.upgrade().ok_or_else(|| failure(&c))?;
    // Cancel acquisition, NOT an already queued SQLite transaction. The outer
    // registered blocking worker remains alive until storage commits/rolls back.
    let _profile = tokio::select! {biased;_=c.cancel.cancelled()=>return Err(failure(&c)),
    _=tokio::time::sleep_until(c.deadline.into())=>return Err(failure(&c)),
    result=c.app.hold_session(&c.owner)=>result.map_err(|_|failure(&c))?};
    let _unlock = tokio::select! {biased;_=c.cancel.cancelled()=>return Err(failure(&c)),
    _=tokio::time::sleep_until(c.deadline.into())=>return Err(failure(&c)),
    result=c.owner.hold_unlocked(&u)=>result.map_err(|_|failure(&c))?};
    held(&c, &u)?;
    Ok(())
}
pub(super) async fn load_recovery(c: &Control, id: &str) -> AppResult<import_catalog::Recovery> {
    original(c).await?;
    let id = id.to_owned();
    let profile = c.owner.dir.clone().ok_or_else(invalid)?;
    let recovery = c
        .owner
        .storage()?
        .read(move |tx| {
            let catalog: Catalog = tx.setting_get(PROJECTS)?.unwrap_or_default();
            if catalog.version != 1 || !catalog.extra.is_empty() || catalog.records.len() > 256 {
                return Err(invalid());
            }
            let row = catalog.records.get(&id).ok_or_else(invalid)?;
            let capture = import_catalog::recoverable(&id, row)?;
            let expected = profile
                .join("automation")
                .join("project-revisions")
                .join(&id)
                .join(format!("generation-{}", capture.transaction));
            if row
                .pointer("/source/path")
                .and_then(Value::as_str)
                .map(std::ffi::OsStr::new)
                != Some(expected.as_os_str())
            {
                return Err(invalid());
            }
            Ok::<_, AppError>(capture)
        })
        .await?;
    original(c).await?;
    Ok(recovery)
}
fn recovery_source(
    profile: &Path,
    recovery: &import_catalog::Recovery,
) -> AppResult<cc_automation_core::portable_publication::PortablePublicationSource> {
    cc_automation_core::portable_publication::PortablePublicationSource::new(
        profile,
        recovery.project,
        recovery.operation,
        recovery.transaction,
        &recovery.reservation_sha,
    )
    .map_err(|_| invalid())
}
fn recovery_ack(
    recovery: &import_catalog::Recovery,
) -> AppResult<cc_automation_core::CatalogPublicationAck> {
    cc_automation_core::CatalogPublicationAck::new(
        recovery.project,
        recovery.transaction,
        recovery.revision,
        &recovery.row_sha,
    )
    .map_err(|_| invalid())
}
async fn sql(
    c: Arc<Control>,
    update: impl FnOnce(&mut Catalog) -> AppResult<SqlAck> + Send + 'static,
) -> AppResult<SqlAck> {
    original(&c).await?; // all strong Unlocked/guards dropped BEFORE queue/await
    let original_unlock = c.unlock.clone();
    let current = c.clone();
    let ack = c
        .owner
        .storage()?
        .write_project_publication(move |tx| {
            let mut catalog: Catalog = tx.setting_get(PROJECTS)?.unwrap_or_default();
            if catalog.version != 1 || !catalog.extra.is_empty() || catalog.records.len() > 256 {
                return Err(failure(&current));
            }
            let u = original_unlock.upgrade().ok_or_else(|| failure(&current))?;
            held(&current, &u)?;
            let ack = update(&mut catalog)?;
            held(&current, &u)?;
            current
                .import_durable
                .store(true, std::sync::atomic::Ordering::Release);
            tx.setting_set(PROJECTS, &catalog)?;
            held(&current, &u)?;
            Ok::<_, AppError>(ack)
        })
        .await
        .map_err(|_| failure(&c))?;
    original(&c).await?;
    Ok(ack)
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct InstalledView {
    version: u32,
    state: &'static str,
    scope: PortableScope,
    review_id: String,
    project_id: String,
    name: String,
    project_revision: u64,
    revision_digest: String,
    encrypted_sha256: String,
}
impl PortableOperation {
    pub async fn accept_import(
        &self,
        review_id: String,
        review_digest: String,
    ) -> AppResult<String> {
        let c = &self.control;
        c.check_original().await?;
        let review = Uuid::parse_str(&review_id).map_err(|_| invalid())?;
        if review.to_string() != review_id || !cc_models::automation::is_digest(&review_digest) {
            return Err(invalid());
        }
        let (previous, finished) = {
            let phase = c.phase.lock().unwrap_or_else(|e| e.into_inner());
            let Phase::ReadyImport(ready) = &*phase else {
                return Err(invalid());
            };
            if ready.review != review || ready.review_digest != review_digest {
                return Err(invalid());
            }
            let cohort = c.cohort.lock().unwrap_or_else(|e| e.into_inner());
            (cohort.id, cohort.finished.clone())
        };
        tokio::select! {biased;_=c.cancel.cancelled()=>return Err(failure(c)),
        _=tokio::time::sleep_until(c.deadline.into())=>return Err(failure(c)),_=finished.cancelled()=>{}}
        c.check_original().await?;
        let (ready, registration) = {
            let mut phase = c.phase.lock().unwrap_or_else(|e| e.into_inner());
            let mut cohort = c.cohort.lock().unwrap_or_else(|e| e.into_inner());
            let Phase::ReadyImport(ready) = &*phase else {
                return Err(invalid());
            };
            if cohort.id != previous
                || !cohort.done
                || c.cancel.is_cancelled()
                || ready.review != review
                || ready.review_digest != review_digest
                || !ready.publication_supported
            {
                return Err(invalid());
            }
            let wide = c
                .owner
                .automation
                .io_slots
                .clone()
                .try_acquire_many_owned(crate::automation_runners::AUTOMATION_IO_CAPACITY)
                .map_err(|_| invalid())?;
            *c.wide_io.lock().unwrap_or_else(|e| e.into_inner()) = Some(Arc::new(wide));
            if ready.recovery.is_some() {
                c.import_durable
                    .store(true, std::sync::atomic::Ordering::Release);
            }
            let Phase::ReadyImport(ready) = std::mem::replace(&mut *phase, Phase::Running) else {
                unreachable!()
            };
            let id = Uuid::new_v4();
            *cohort = Cohort {
                id,
                done: false,
                payload_retiring: false,
                finished: CancellationToken::new(),
            };
            (
                ready,
                Arc::new(WorkRegistration {
                    control: c.clone(),
                    cohort: id,
                }),
            )
        };
        let binding = OwnedBinding {
            registration: registration.clone(),
        };
        let current = c.clone();
        let future = Box::pin(async move {
            let subject = binding
                .subject(&current.app, &current.scope, &current.cancel)
                .await?;
            let runtime = tokio::runtime::Handle::current();
            let worker = current.clone();
            // SAME registered worker owns FS caps, SQL await and IO4 through
            // actual COMMIT/audit/cleanup even if this caller is aborted.
            let value = work(&subject, move |fence| {
                runtime.block_on(run(worker, ready, fence))
            })
            .await?;
            current
                .check_original()
                .await
                .map_err(|_| failure(&current))?;
            serde_json::to_string(&value).map_err(|_| invalid())
        });
        let invocation = Invocation {
            future: Some(future),
            registration: Some(registration),
        };
        let result = tokio::select! {biased;_=c.cancel.cancelled()=>Err(failure(c)),result=invocation=>result};
        result.map_err(|_| failure(c))
    }
}
async fn run(
    c: Arc<Control>,
    mut ready: ReadyImport,
    fence: WorkerFence,
) -> AppResult<InstalledView> {
    fence.check()?;
    let profile = c.owner.dir.clone().ok_or_else(invalid)?;
    ready
        .source_parent
        .reopen_same_directory(&ready.source_parent_path)
        .map_err(|_| invalid())?;
    let bytes = portable_package::read_encrypted_file(&ready.source_parent, &ready.source_leaf)
        .map_err(|_| invalid())?;
    if digest(&bytes) != ready.encrypted_sha256 {
        return Err(AppError::invalid(
            "portable_import",
            "portable_import_source_changed",
        ));
    }
    ready
        .source_parent
        .reopen_same_directory(&ready.source_parent_path)
        .map_err(|_| invalid())?;
    let package = ready.package.take().ok_or_else(invalid)?;
    if ready.recovery.is_some() {
        return run_recovery(c, ready, package, fence).await;
    }
    cc_automation_core::portable_publication::check_portable_import_base(
        &profile,
        ready.project,
        &package,
    )
    .map_err(|_| invalid())?;
    fence.check()?;
    let operation = Uuid::new_v4(); // Native publication attempt, not an imported/caller ID
    let project = ready.project;
    let transaction = ready.transaction;
    let row = import_catalog::reservation(
        &profile,
        project,
        operation,
        transaction,
        &ready.name,
        &ready.encrypted_sha256,
        &ready.summary,
        chrono::Utc::now().timestamp(),
    )?;
    let id = project.to_string();
    let reserve = sql(c.clone(), move |catalog| {
        if catalog.records.len() >= 256 || catalog.records.contains_key(&id) {
            return Err(invalid());
        }
        let row_sha = managed::row_stamp(&row)?;
        catalog.records.insert(id, row);
        Ok(SqlAck {
            revision: 1,
            row_sha,
        })
    })
    .await?;
    fence.check().map_err(|_| failure(&c))?;
    let source = cc_automation_core::portable_publication::PortablePublicationSource::new(
        &profile,
        project,
        operation,
        transaction,
        &reserve.row_sha,
    )
    .map_err(|_| failure(&c))?;
    let prepared = cc_automation_core::portable_publication::prepare_portable_project(
        source, package, &c.cancel,
    )
    .map_err(|_| failure(&c))?;
    let fingerprint = prepared.fingerprint().map_err(|_| failure(&c))?;
    let id = project.to_string();
    let fp = fingerprint.clone();
    let previous = reserve.row_sha;
    let prepared_ack = sql(c.clone(), move |catalog| {
        let row = catalog.records.get(&id).ok_or_else(invalid)?;
        let next = import_catalog::prepared(
            row,
            &previous,
            project,
            operation,
            transaction,
            &fp,
            chrono::Utc::now().timestamp(),
        )?;
        let row_sha = managed::row_stamp(&next)?;
        catalog.records.insert(id, next);
        Ok(SqlAck {
            revision: 2,
            row_sha,
        })
    })
    .await?;
    fence.check().map_err(|_| failure(&c))?;
    let ack = cc_automation_core::CatalogPublicationAck::new(
        project,
        transaction,
        prepared_ack.revision,
        &prepared_ack.row_sha,
    )
    .map_err(|_| failure(&c))?;
    let published = prepared.publish(&ack, &c.cancel).map_err(|_| failure(&c))?;
    if published.fingerprint().map_err(|_| failure(&c))? != fingerprint {
        return Err(failure(&c));
    }
    let id = project.to_string();
    let fp = fingerprint;
    let previous = prepared_ack.row_sha;
    let selected = sql(c.clone(), move |catalog| {
        let row = catalog.records.get(&id).ok_or_else(invalid)?;
        let next = import_catalog::ready(
            row,
            &previous,
            project,
            operation,
            transaction,
            &fp,
            chrono::Utc::now().timestamp(),
        )?;
        managed::validate_path(Some(&profile), &id, &next)?;
        let row_sha = managed::row_stamp(&next)?;
        catalog.records.insert(id, next);
        Ok(SqlAck {
            revision: 3,
            row_sha,
        })
    })
    .await?;
    let ack = cc_automation_core::CatalogPublicationAck::new(
        project,
        transaction,
        selected.revision,
        &selected.row_sha,
    )
    .map_err(|_| failure(&c))?;
    published
        .record_catalog_ack(&ack)
        .map_err(|_| failure(&c))?;
    published.record_complete(&ack).map_err(|_| failure(&c))?;
    drop(published);
    // No old-row check after own CAS; only original scope remains authoritative.
    original(&c).await?;
    fence.check().map_err(|_| failure(&c))?;
    Ok(InstalledView {
        version: 1,
        state: "installed",
        scope: c.scope.clone(),
        review_id: ready.review.to_string(),
        project_id: project.to_string(),
        name: ready.name,
        project_revision: 3,
        revision_digest: ready.summary.portable_digest,
        encrypted_sha256: ready.encrypted_sha256,
    })
}

async fn run_recovery(
    c: Arc<Control>,
    ready: ReadyImport,
    package: PortableImport,
    fence: WorkerFence,
) -> AppResult<InstalledView> {
    let expected = ready.recovery.as_ref().ok_or_else(invalid)?;
    let current = load_recovery(&c, &ready.project.to_string())
        .await
        .map_err(|_| failure(&c))?;
    if current.row_sha != expected.row_sha {
        return Err(failure(&c));
    }
    let profile = c.owner.dir.clone().ok_or_else(|| failure(&c))?;
    fence.check().map_err(|_| failure(&c))?;
    let published = cc_automation_core::portable_publication::recover_portable_project(
        recovery_source(&profile, &current)?,
        package,
        &current.fingerprint,
        &recovery_ack(&current)?,
        &c.cancel,
    )
    .map_err(|_| failure(&c))?;
    // A rev3 observation does not perform our own ready CAS. Re-read the
    // ACTUAL same SQL row after Core I/O before recording any audit success.
    if current.revision==3 && load_recovery(&c,&ready.project.to_string()).await.map_err(|_|failure(&c))?.row_sha!=current.row_sha {
        return Err(failure(&c));
    }
    let selected = if current.revision == 2 {
        let project = current.project;
        let operation = current.operation;
        let transaction = current.transaction;
        let id = project.to_string();
        let previous = current.row_sha;
        let fp = current.fingerprint;
        sql(c.clone(), move |catalog| {
            let row = catalog.records.get(&id).ok_or_else(invalid)?;
            let next = import_catalog::ready(
                row,
                &previous,
                project,
                operation,
                transaction,
                &fp,
                chrono::Utc::now().timestamp(),
            )?;
            managed::validate_path(Some(&profile), &id, &next)?;
            let row_sha = managed::row_stamp(&next)?;
            catalog.records.insert(id, next);
            Ok(SqlAck {
                revision: 3,
                row_sha,
            })
        })
        .await?
    } else {
        SqlAck {
            revision: current.revision,
            row_sha: current.row_sha,
        }
    };
    let ack = cc_automation_core::CatalogPublicationAck::new(
        ready.project,
        ready.transaction,
        selected.revision,
        &selected.row_sha,
    )
    .map_err(|_| failure(&c))?;
    published
        .record_catalog_ack(&ack)
        .map_err(|_| failure(&c))?;
    published.record_complete(&ack).map_err(|_| failure(&c))?;
    drop(published);
    original(&c).await?;
    fence.check().map_err(|_| failure(&c))?;
    Ok(InstalledView {
        version: 1,
        state: "installed",
        scope: c.scope.clone(),
        review_id: ready.review.to_string(),
        project_id: ready.project.to_string(),
        name: ready.name,
        project_revision: 3,
        revision_digest: ready.summary.portable_digest,
        encrypted_sha256: ready.encrypted_sha256,
    })
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod acceptance_tests;

#[cfg(test)]
mod prepared_recovery_tests;
