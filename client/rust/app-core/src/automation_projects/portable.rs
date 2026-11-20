//! Private ANS04 operation seam. No Bridge, catalog commit, Runner or vault
//! lookup. Uses the existing closed/joined FS owner, not another holder map.
use super::*;
pub(crate) mod gateway;
pub(crate) mod import_catalog;
use crate::automation_prepare::CancelOnDrop;
use crate::session::{Session, Unlocked};
use cc_automation_core::portable_package::{self, PortableChoice, PortablePackageSummary};
use cc_crypto_core::SecretString;
use secrecy::ExposeSecret;
use std::path::{Path, PathBuf};
use tokio::sync::oneshot;
use tokio_util::sync::CancellationToken;
fn invalid() -> AppError {
    AppError::invalid(
        "portable_project",
        "portable_context_or_package_unavailable",
    )
}
fn password_bound(password: &SecretString) -> AppResult<()> {
    if password.expose_secret().is_empty() || password.expose_secret().len() > 4096 {
        Err(invalid())
    } else {
        Ok(())
    }
}
fn stop(token: &CancellationToken) -> AppResult<()> {
    if token.is_cancelled() {
        Err(AppError::Cancelled)
    } else {
        Ok(())
    }
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct PortableScope {
    version: u32,
    profile_id: String,
    notification_scope_id: String,
}
impl PortableScope {
    fn captured(owner: &Session, u: &Unlocked) -> AppResult<Self> {
        Ok(Self {
            version: 1,
            profile_id: owner.profile_id.to_string(),
            notification_scope_id: u.notices.scope_id().ok_or(AppError::VaultLocked)?,
        })
    }
    fn matches(&self, owner: &Session, u: &Unlocked) -> bool {
        self.version == 1
            && self.profile_id == owner.profile_id.to_string()
            && u.notices.scope_id().as_deref() == Some(self.notification_scope_id.as_str())
    }
}
#[derive(Clone)]
struct WorkerFence {
    registry_epoch: CancellationToken,
    shutdown: tokio::sync::watch::Receiver<bool>,
    caller: CancellationToken,
    deadline: std::time::Instant,
    // Existing operation admission, retained by EVERY registered worker even
    // after caller/Subject Drop. No new global counter or holder registry.
    _admission: Arc<tokio::sync::OwnedSemaphorePermit>,
    // Field order is part of the retirement contract: capacity reference drops
    // BEFORE the final registration can mark Joined. Secret-bearing task/future
    // owners are explicitly dropped before this fence (see gateway::Invocation).
    _wide_io:Option<Arc<tokio::sync::OwnedSemaphorePermit>>,
    _owned_registration: Option<Arc<gateway::WorkRegistration>>,
}
impl WorkerFence {
    fn check(&self) -> AppResult<()> {
        if self.registry_epoch.is_cancelled()
            || *self.shutdown.borrow()
            || self.caller.is_cancelled()
            || std::time::Instant::now() >= self.deadline
        {
            return Err(AppError::Cancelled);
        }
        Ok(())
    }
    fn core_check(&self) -> cc_automation_core::Result<()> {
        self.check()
            .map_err(|_| cc_automation_core::AutomationError::Closed)
    }
}
struct Subject {
    owner: Arc<Session>,
    unlock: Weak<Unlocked>,
    scope: PortableScope,
    fence: WorkerFence,
}
impl Drop for Subject {
    fn drop(&mut self) {
        self.fence.registry_epoch.cancel();
    }
}
impl Subject {
    async fn capture(app:&AppCore,expected:&PortableScope,caller:&CancellationToken)->AppResult<Self>{
        Self::capture_bound(app,expected,caller,None).await
    }
    async fn capture_bound(
        app: &AppCore,
        expected: &PortableScope,
        caller: &CancellationToken,
        binding: Option<&gateway::OwnedBinding>,
    ) -> AppResult<Self> {
        stop(caller)?;
        if let Some(binding)=binding {return binding.subject(app,expected,caller).await;}
        let (owner, u) = app.unlocked().await?;
        let fence = {
            let _profile = app.hold_session(&owner).await?;
            let _unlock = owner.hold_unlocked(&u).await?;
            if !expected.matches(&owner, &u) {
                return Err(invalid());
            }
            let admission = owner
                .automation
                .prepare_slots
                .clone()
                .try_acquire_owned()
                .map_err(|_| invalid())?;
            WorkerFence {
                registry_epoch: owner.automation.dependencies.local_filesystem_epoch()?,
                shutdown: u.sharing_shutdown.subscribe(),
                caller: caller.clone(),
                deadline: std::time::Instant::now() + std::time::Duration::from_secs(300),
                _admission: Arc::new(admission),
                _wide_io:None,
                _owned_registration: None,
            }
        };
        fence.check()?;
        Ok(Self {
            owner,
            unlock: Arc::downgrade(&u),
            scope: expected.clone(),
            fence,
        })
    }
    async fn check(&self, app: &AppCore, cancel: &CancellationToken) -> AppResult<()> {
        self.fence.check()?;
        stop(cancel)?;
        let u = self.unlock.upgrade().ok_or(AppError::VaultLocked)?;
        let _profile = app.hold_session(&self.owner).await?;
        let _unlock = self.owner.hold_unlocked(&u).await?;
        if !self.scope.matches(&self.owner, &u) {
            return Err(invalid());
        }
        self.fence.check()?;
        stop(cancel)
    }
}
/// Existing registry registers and JOINS this FS task atomically with close.
/// Original native close cancellation and sharing_shutdown are checked before
/// ANY worker body/handle allocation, and after its result. Side-effecting
/// ciphertext writes ALSO check this same fence before create/everychunk/flush.
/// A queued worker cannot start writing after lock even if the caller lives.
async fn work<T: Send + 'static>(
    subject: &Subject,
    task: impl FnOnce(WorkerFence) -> AppResult<T> + Send + 'static,
) -> AppResult<T> {
    subject.fence.check()?;
    let io = if subject.fence._wide_io.is_some() {None}else{Some(automation_io_slot(&subject.owner)?)};
    let fence = subject.fence.clone();
    let (send, receive) = oneshot::channel();
    subject
        .owner
        .automation
        .dependencies
        .launch_worker(move || {
            let result = fence
                .check()
                .and_then(|_| task(fence.clone()))
                .and_then(|value| {
                    fence.check()?;
                    Ok(value)
                });
            // A failed send still OWNS T (which may retain root handles).
            // Dispose it before releasing this worker's final registration.
            // Success transfers T into the caller's secret-bearing invocation,
            // whose explicit Drop order retains its root registration.
            drop(send.send(result));
            drop(io);
            drop(fence);
        })?;
    receive.await.map_err(|_| invalid())?
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PortableContext {
    scope: PortableScope,
    project_id: String,
    project_revision: u64,
    revision_digest: String,
    project_stamp: String,
    choices: Vec<PortableChoice>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct PortableExportRequest {
    scope: PortableScope,
    project_id: String,
    project_revision: u64,
    revision_digest: String,
    project_stamp: String,
    selected_paths: Vec<String>,
    playbook: Option<String>,
}
impl PortableExportRequest {
    fn parse(raw: &str) -> AppResult<Self> {
        if raw.len() > 256 * 1024 {
            return Err(invalid());
        }
        let v: Self = serde_json::from_str(raw).map_err(|_| invalid())?;
        project_id(&v.project_id).map_err(|_| invalid())?;
        if v.project_revision == 0
            || (!cc_models::automation::is_digest(&v.revision_digest)
                || !cc_models::automation::is_digest(&v.project_stamp))
            || v.selected_paths.is_empty()
            || v.selected_paths.len() > 2000
            || v.selected_paths.iter().any(|p| p.len() > 1024)
        {
            return Err(invalid());
        }
        Ok(v)
    }
}
struct Source {
    subject: Subject,
    project: AutomationProjectContext,
    stamp: String,
    root: Arc<managed::OpenedProjectRoot>,
    revision: ProjectRevision,
}
impl Source {
    async fn capture(
        app: &AppCore,
        scope: &PortableScope,
        id: &str,
        revision: u64,
        cancel: &CancellationToken,
        binding: Option<&gateway::OwnedBinding>,
    ) -> AppResult<Self> {
        let subject = Subject::capture_bound(app, scope, cancel,binding).await?;
        let project = app.automation_project_context_original(id,subject.owner.clone(),subject.unlock.clone()).await?;
        if !Arc::ptr_eq(&subject.owner, &project.owner)
            || project.project.read_only
            || project.project.revision != Some(revision)
        {
            return Err(invalid());
        }
        let u = project.unlock.upgrade().ok_or(AppError::VaultLocked)?;
        let stamp = {
            let _profile = app.hold_session(&project.owner).await?;
            let _unlock = project.owner.hold_unlocked(&u).await?;
            if !scope.matches(&project.owner, &u) {
                return Err(invalid());
            }
            dependencies::project_stamp(&project)
                .await
                .map_err(|_| invalid())?
        };
        drop(u);
        let _retained_io=managed::admit_io(app,&project).await?;
        let path = project.path.clone();
        let selected_root = project.root_selection.clone();
        let token = cancel.clone();
        let (root, revision) = work(&subject, move |_fence| {
            stop(&token)?;
            let root = Arc::new(selected_root.open().map_err(|_| invalid())?);
            let revision = root.revision().map_err(|_| invalid())?;
            root.reopen_same_directory(&path).map_err(|_| invalid())?;
            root.recheck().map_err(|_|invalid())?;
            stop(&token)?;
            Ok((root, revision))
        })
        .await?;
        let value = Self {
            subject,
            project,
            stamp,
            root,
            revision,
        };
        value.check(app, cancel).await?;
        Ok(value)
    }
    async fn check(&self, app: &AppCore, cancel: &CancellationToken) -> AppResult<()> {
        self.subject.check(app, cancel).await?;
        let u = self.project.unlock.upgrade().ok_or(AppError::VaultLocked)?;
        let _profile = app.hold_session(&self.project.owner).await?;
        let _unlock = self.project.owner.hold_unlocked(&u).await?;
        if dependencies::project_stamp(&self.project)
            .await
            .map_err(|_| invalid())?
            != self.stamp
            || !self.subject.scope.matches(&self.project.owner, &u)
        {
            return Err(invalid());
        }
        self.subject.fence.check()?;
        stop(cancel)
    }
}
fn current_tree(root: &managed::OpenedProjectRoot, path: &Path, revision: &ProjectRevision) -> AppResult<()> {
    root.recheck().map_err(|_|invalid())?;
    let named = root.reopen_same_directory(path).map_err(|_| invalid())?;
    if named.revision().map_err(|_| invalid())? != *revision {
        return Err(invalid());
    }
    root.reopen_same_directory(path).map_err(|_| invalid())?;
    root.recheck().map_err(|_|invalid())?;
    Ok(())
}
fn selected_file(path: &Path) -> AppResult<(PathBuf, String)> {
    if !path.is_absolute() || path.to_str().is_none_or(|v| v.len() > 4096) {
        return Err(invalid());
    }
    #[cfg(windows)]
    {
        use std::path::{Component, Prefix};
        if !matches!(path.components().next(),Some(Component::Prefix(p)) if matches!(p.kind(),Prefix::Disk(_)|Prefix::VerbatimDisk(_)))
        {
            return Err(invalid());
        }
    }
    let parent = path.parent().ok_or_else(invalid)?.to_path_buf();
    let name = path
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(invalid)?
        .to_owned();
    Ok((parent, name))
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PortableImportView {
    version: u32,
    encrypted_sha256: String,
    summary: PortablePackageSummary,
    publication_supported: bool,
    reason: &'static str,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PortableExportOutcome {
    version: u32,
    encrypted_sha256: String,
    bytes: usize,
}
fn digest(raw: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    hex::encode(Sha256::digest(raw))
}
impl AppCore {
    pub(crate) async fn automation_portable_scope(&self) -> AppResult<PortableScope> {
        let (owner, u) = self.unlocked().await?;
        let _profile = self.hold_session(&owner).await?;
        let _unlock = owner.hold_unlocked(&u).await?;
        PortableScope::captured(&owner, &u)
    }
    pub(crate) async fn automation_portable_context(
        &self,
        scope: PortableScope,
        id: String,
        revision: u64,
        cancel: &CancellationToken,
    ) -> AppResult<PortableContext> {
        self.portable_context_bound(scope,id,revision,cancel,None).await
    }
    async fn portable_context_bound(&self,scope:PortableScope,id:String,revision:u64,cancel:&CancellationToken,binding:Option<&gateway::OwnedBinding>)->AppResult<PortableContext>{
        let _cancel = CancelOnDrop(cancel.child_token());
        let token = &_cancel.0;
        let source = Source::capture(self, &scope, &id, revision, token,binding).await?;
        let captured = source.revision.clone();
        let choices = work(&source.subject, move |_fence| {
            portable_package::selection_choices(&captured).map_err(|_| invalid())
        })
        .await?;
        source.check(self, token).await?;
        let view = PortableContext {
            scope,
            project_id: id,
            project_revision: revision,
            revision_digest: source.revision.digest.clone(),
            project_stamp: source.stamp.clone(),
            choices,
        };
        if serde_json::to_vec(&view).map_err(|_| invalid())?.len() > 512 * 1024 {
            return Err(invalid());
        }
        Ok(view)
    }
    /// Native accepts only a separate caller-typed package SecretString. No VRK,
    /// SecretRef or password persistence. Destination is explicit user-selected
    /// local FILE, never an import target. Existing files are never overwritten.
    pub(crate) async fn automation_portable_export(
        &self,
        request_json: String,
        password: SecretString,
        destination: PathBuf,
        cancel: &CancellationToken,
    ) -> AppResult<PortableExportOutcome> {
        self.portable_export_bound(request_json,password,destination,cancel,None).await
    }
    async fn portable_export_bound(&self,request_json:String,password:SecretString,destination:PathBuf,cancel:&CancellationToken,binding:Option<&gateway::OwnedBinding>)->AppResult<PortableExportOutcome>{
        password_bound(&password)?;
        let _cancel = CancelOnDrop(cancel.child_token());
        let token = &_cancel.0;
        let request = PortableExportRequest::parse(&request_json)?;
        let source = Source::capture(
            self,
            &request.scope,
            &request.project_id,
            request.project_revision,
            token,
            binding,
        )
        .await?;
        if source.revision.digest != request.revision_digest
            || source.stamp != request.project_stamp
        {
            return Err(invalid());
        }
        let root = source.root.clone();
        let revision = source.revision.clone();
        let path = source.project.path.clone();
        let token_copy = token.clone();
        let encrypted = work(&source.subject, move |_fence| {
            current_tree(&root, &path, &revision)?;
            let bytes = portable_package::encrypt_selection(
                &root,
                &revision,
                &request.selected_paths,
                request.playbook,
                &password,
                &token_copy,
            )
            .map_err(|_| invalid())?;
            current_tree(&root, &path, &revision)?;
            Ok(bytes)
        })
        .await?;
        source.check(self, token).await?;
        let (parent_path, name) = selected_file(&destination)?;
        let root = source.root.clone();
        let revision = source.revision.clone();
        let path = source.project.path.clone();
        let token_copy = token.clone();
        let outcome = work(&source.subject, move |fence| {
            current_tree(&root, &path, &revision)?;
            stop(&token_copy)?;
            let canonical_parent = std::fs::canonicalize(&parent_path).map_err(|_| invalid())?;
            let canonical_source = std::fs::canonicalize(&path).map_err(|_| invalid())?;
            if canonical_parent.starts_with(&canonical_source) {
                return Err(invalid());
            }
            let parent = ProjectRoot::open(&parent_path).map_err(|_| invalid())?;
            parent
                .reopen_same_directory(&parent_path)
                .map_err(|_| invalid())?;
            portable_package::write_encrypted_new_checked(
                &parent,
                &name,
                &encrypted,
                &token_copy,
                || fence.core_check(),
            )
            .map_err(|_| invalid())?;
            parent
                .reopen_same_directory(&parent_path)
                .map_err(|_| invalid())?;
            current_tree(&root, &path, &revision)?;
            stop(&token_copy)?;
            Ok(PortableExportOutcome {
                version: 1,
                encrypted_sha256: digest(&encrypted),
                bytes: encrypted.len(),
            })
        })
        .await?;
        source.check(self, token).await?;
        Ok(outcome)
    }
    /// Authenticated inspection only. No directory/catalog is created. Actual
    /// imported managed generation + SQL CAS is blocked until publication review.
    pub(crate) async fn automation_portable_inspect(
        &self,
        scope: PortableScope,
        file: PathBuf,
        password: SecretString,
        cancel: &CancellationToken,
    ) -> AppResult<PortableImportView> {
        self.portable_inspect_bound(scope,file,password,cancel,None).await
    }
    async fn portable_inspect_bound(&self,scope:PortableScope,file:PathBuf,password:SecretString,cancel:&CancellationToken,binding:Option<&gateway::OwnedBinding>)->AppResult<PortableImportView>{
        password_bound(&password)?;
        let _cancel = CancelOnDrop(cancel.child_token());
        let token = &_cancel.0;
        let subject = Subject::capture_bound(self, &scope, token,binding).await?;
        let (parent_path, name) = selected_file(&file)?;
        let token_copy = token.clone();
        let view = work(&subject, move |_fence| {
            stop(&token_copy)?;
            let parent = ProjectRoot::open(&parent_path).map_err(|_| invalid())?;
            let encrypted =
                portable_package::read_encrypted_file(&parent, &name).map_err(|_| invalid())?;
            parent
                .reopen_same_directory(&parent_path)
                .map_err(|_| invalid())?;
            let verified = portable_package::decrypt_package(&encrypted, &password, &token_copy)
                .map_err(|_| invalid())?;
            stop(&token_copy)?;
            Ok(PortableImportView {
                version: 1,
                encrypted_sha256: digest(&encrypted),
                summary: verified.summary(),
                publication_supported: false,
                reason: "managed_publication_required",
            })
        })
        .await?;
        subject.check(self, token).await?;
        Ok(view)
    }
}
#[cfg(test)]
mod tests;
