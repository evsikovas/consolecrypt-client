//! Private prerequisite for Ansible Vault. No bridge/AI/Helper/executor wiring.
//! The reference file is data; only explicit Native credential selection can
//! issue a review. Job material requires a verified signature; validation has
//! a distinct private original-context/phase-bound issuer, never a fake job.
use super::*;
use cc_automation_core::{VaultInputBinding, VaultPasswordLease, VerifiedJob};
use cc_models::{credential::CredentialKind, secret::SecretKind, ObjectId};
use std::time::{Duration, Instant};
use tokio::sync::OwnedSemaphorePermit;
use tokio_util::sync::CancellationToken;

fn invalid() -> AppError {
    AppError::invalid("automation_vault", "scoped_password_unavailable")
}

fn rebind(root:&ProjectRoot,revision:&ProjectRevision,selected:Uuid)->AppResult<VaultInputBinding> {
    if selected.is_nil() { root.bind_ephemeral_vault_input(revision) } else { root.bind_vault_input(revision,selected) }
        .map_err(|_|invalid())
}

/// No Unlocked, KEK, secret material, general resolver or serializable DTO.
/// Existing bounded review/IO slots are retained until release/consumption.
pub(crate) struct VaultCredentialReview {
    project: AutomationProjectContext,
    stamp: String,
    generation: u64,
    version: u64, // Retained ONLY for the private owner original-input fence.
    metadata: Option<crate::working_set::VaultCredentialMetadata>,
    root: Arc<managed::OpenedProjectRoot>,
    revision: ProjectRevision,
    binding: VaultInputBinding,
    secret: Option<ObjectId>,
    ephemeral: Option<cc_models::secret::SecretValue>,
    fences: BTreeMap<ObjectId, i64>,
    deadline: Instant,
    revoked: CancellationToken,
    _slot: OwnedSemaphorePermit,
}
impl Drop for VaultCredentialReview {
    fn drop(&mut self) {
        self.revoked.cancel();
    }
}

/// Sealed owner for the future fixed-purpose transport. Intentionally has NO
/// extraction/serialization/AI method or broker registration in this slice.
/// A future send must recheck this original context, including SQL + filesystem,
/// and scope lock revocation independently of the target cancel_on_lock policy.
pub(crate) struct NativeVaultPassword {
    review: Arc<VaultCredentialReview>,
    lease: VaultPasswordLease,
}
impl std::fmt::Debug for NativeVaultPassword {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NativeVaultPassword")
            .finish_non_exhaustive()
    }
}

// Same actual SQL catalog/readers as the existing project operations. Called
// under original profile/unlock guards. No filesystem I/O is done under them.
async fn stamp(context: &AutomationProjectContext) -> AppResult<String> {
    use sha2::{Digest, Sha256};
    let catalog: Catalog = context
        .owner
        .storage()?
        .setting_get(PROJECTS)
        .await?
        .unwrap_or_default();
    let row = catalog
        .records
        .get(&context.project.id)
        .ok_or_else(invalid)?;
    if catalog.version != 1
        || !catalog.extra.is_empty()
        || !supported(row)
        || row["revision"].as_u64() != context.project.revision
        || row.pointer("/source/path").and_then(Value::as_str) != context.path.to_str()
        || groups::from_row(row)? != context.inventory
        || source_bindings::load(&context.owner, &context.project.id, row).await?
            != context.external_inventory
    {
        return Err(invalid());
    }
    let raw = serde_json::to_vec(row).map_err(|_| invalid())?;
    if raw.len() > 1024 * 1024 {
        return Err(invalid());
    }
    Ok(hex::encode(Sha256::digest(raw)))
}

impl AppCore {
    pub(crate) async fn automation_ephemeral_vault_capture(
        &self, project:AutomationProjectContext, expected_digest:String,
        password:cc_models::secret::SecretValue, operation_cancel:&CancellationToken,
    )->AppResult<VaultCredentialReview> {
        let value=password.expose_secret();
        if value.is_empty() || value.len()>4096 || value.trim()!=value ||
            value.chars().any(|c|c.is_control() || matches!(c,'\u{2028}'|'\u{2029}')) || operation_cancel.is_cancelled() {
            return Err(invalid());
        }
        if project.project.read_only {return Err(invalid());}
        let slot=project.owner.automation.review_slots.clone().try_acquire_owned().map_err(|_|invalid())?;
        let u=project.unlock.upgrade().ok_or(AppError::VaultLocked)?;
        let (captured_stamp,generation,version)={
            let _profile=self.hold_session(&project.owner).await?;
            let _unlock=project.owner.hold_unlocked(&u).await?;
            (stamp(&project).await?,project.owner.automation.unlocked_generation()?,u.working().version())
        };
        drop(u);
        let retained_io=managed::admit_io(self,&project).await?;
        let io=retained_io.clone();let selection=project.root_selection.clone();let path=project.path.clone();
        let (root,revision,binding)=tokio::task::spawn_blocking(move||{
            let _io=io;let root=Arc::new(selection.open()?);let revision=root.revision().map_err(|_|invalid())?;
            if revision.digest!=expected_digest {return Err(invalid());}
            let binding=root.bind_ephemeral_vault_input(&revision).map_err(|_|invalid())?;
            root.reopen_same_directory(&path).map_err(|_|invalid())?;root.recheck()?;
            Ok::<_,AppError>((root,revision,binding))
        }).await.map_err(|_|invalid())??;
        let review=VaultCredentialReview{project,stamp:captured_stamp,generation,version,metadata:None,root,revision,binding,
            secret:None,ephemeral:Some(password),fences:BTreeMap::new(),deadline:Instant::now()+Duration::from_secs(300),
            revoked:operation_cancel.child_token(),_slot:slot};
        let u=review.project.unlock.upgrade().ok_or(AppError::VaultLocked)?;
        let _profile=self.hold_session(&review.project.owner).await?;
        let _unlock=review.project.owner.hold_unlocked(&u).await?;
        review.check_held(&u).await?;
        drop(_unlock);
        drop(_profile);
        Ok(review)
    }
    /// Internal explicit-selection action; not a deserializable bridge request.
    /// expected_digest comes from Native's already captured project preview.
    pub(crate) async fn automation_vault_capture(
        &self,
        project_id: String,
        expected_catalog_revision: u64,
        expected_digest: String,
        explicitly_selected: String,
        operation_cancel: &CancellationToken,
    ) -> AppResult<VaultCredentialReview> {
        let selected = Uuid::parse_str(&explicitly_selected).map_err(|_| invalid())?;
        if selected.is_nil()
            || selected.to_string() != explicitly_selected
            || !cc_models::automation::is_digest(&expected_digest)
            || operation_cancel.is_cancelled()
        {
            return Err(invalid());
        }
        let credential_id = crate::dto::parse_id("credential_id", &explicitly_selected)?;
        let project = self.automation_project_context(&project_id).await?;
        if project.project.read_only || project.project.revision != Some(expected_catalog_revision)
        {
            return Err(invalid());
        }
        let slot = project
            .owner
            .automation
            .review_slots
            .clone()
            .try_acquire_owned()
            .map_err(|_| invalid())?;
        let u = project.unlock.upgrade().ok_or(AppError::VaultLocked)?;
        let revoked = operation_cancel.child_token();
        let (captured_stamp, generation, version, fences, secret, metadata) = {
            let _profile = self.hold_session(&project.owner).await?;
            let _unlock = project.owner.hold_unlocked(&u).await?;
            let metadata = u
                .working()
                .vault_credential_metadata(credential_id)
                .ok_or_else(invalid)?;
            let secret = metadata.secret();
            let fences = metadata.fences();
            (
                stamp(&project).await?,
                project.owner.automation.unlocked_generation()?,
                u.working().version(),
                fences,
                secret,
                metadata,
            )
        };
        drop(u); // No strong unlock is retained while filesystem work runs.
        let retained_io = managed::admit_io(self, &project).await?;
        let io = retained_io.clone();
        let path = project.path.clone();
        let selected_root = project.root_selection.clone();
        let (root, revision, binding) = tokio::task::spawn_blocking(move || {
            let _io = io;
            let root = Arc::new(selected_root.open().map_err(|_| invalid())?);
            let revision = root.revision().map_err(|_| invalid())?;
            root.recheck().map_err(|_| invalid())?;
            root.reopen_same_directory(&path).map_err(|_| invalid())?;
            if revision.digest != expected_digest {
                return Err(invalid());
            }
            let binding = root
                .bind_vault_input(&revision, selected)
                .map_err(|_| invalid())?;
            root.recheck().map_err(|_| invalid())?;
            root.reopen_same_directory(&path).map_err(|_| invalid())?;
            Ok::<_, AppError>((root, revision, binding))
        })
        .await
        .map_err(|_| invalid())??;
        let review = VaultCredentialReview {
            project,
            stamp: captured_stamp,
            generation,
            version,
            metadata: Some(metadata),
            root,
            revision,
            binding,
            secret: Some(secret),
            ephemeral: None,
            fences,
            deadline: Instant::now() + Duration::from_secs(300),
            revoked,
            _slot: slot,
        };
        let u = review
            .project
            .unlock
            .upgrade()
            .ok_or(AppError::VaultLocked)?;
        let _profile = self.hold_session(&review.project.owner).await?;
        let _unlock = review.project.owner.hold_unlocked(&u).await?;
        review.check_held(&u).await?;
        drop(_unlock);
        drop(_profile);
        Ok(review)
    }
}

impl VaultCredentialReview {
    async fn password_value(&self,u:&crate::session::Unlocked)->AppResult<cc_models::secret::SecretValue> {
        if let Some(password)=&self.ephemeral {return Ok(password.clone());}
        let secret=u.writer.read_secret(self.secret.ok_or_else(invalid)?).await.map_err(|_|invalid())?;
        if secret.kind!=SecretKind::Password {return Err(invalid());}
        Ok(secret.value)
    }
    async fn check_held(&self, u: &crate::session::Unlocked) -> AppResult<()> {
        // SQL can await while benign duplicate refreshes advance the global
        // WorkingSet counter. No awaited work may follow the exact selected
        // metadata/lifecycle checks below. Ordinary owner input policy remains
        // separate: owner.rs still compares its captured original.version.
        if stamp(&self.project).await? != self.stamp
            || self.revoked.is_cancelled()
            || Instant::now() >= self.deadline
            || self.project.owner.automation.unlocked_generation()? != self.generation
            || self.metadata.as_ref().is_some_and(|metadata|u.working()
                .vault_credential_metadata(metadata.credential()).as_ref()!=Some(metadata))
            || self
                .fences
                .iter()
                .any(|(id, rev)| u.working().revision_of(*id) != Some(*rev))
        {
            return Err(invalid());
        }
        Ok(())
    }

    /// This first slice stops at a sealed Native owner; it cannot enable a
    /// helper capability or send a password. Default jobs remain unchanged.
    pub(crate) async fn resolve_for_job(
        self,
        app: &AppCore,
        verified: &VerifiedJob,
        job_cancel: &CancellationToken,
    ) -> AppResult<NativeVaultPassword> {
        Arc::new(self)
            .resolve_for_job_shared(app, verified, job_cancel)
            .await
    }
    async fn resolve_for_job_shared(
        self: &Arc<Self>,
        app: &AppCore,
        verified: &VerifiedJob,
        job_cancel: &CancellationToken,
    ) -> AppResult<NativeVaultPassword> {
        if verified.manifest().project_id.to_string() != self.project.project.id
            || verified.manifest().revision_digest != self.binding.revision_digest()
            || verified.is_access_expired()
            || job_cancel.is_cancelled()
        {
            return Err(invalid());
        }
        let u = self.project.unlock.upgrade().ok_or(AppError::VaultLocked)?;
        {
            let _profile = app.hold_session(&self.project.owner).await?;
            let _unlock = self.project.owner.hold_unlocked(&u).await?;
            self.check_held(&u).await?;
        }
        drop(u);
        // Same walker/capability, no new secret input parser or ambient path.
        let io = automation_io_slot(&self.project.owner)?;
        let root = self.root.clone();
        let revision = self.revision.clone();
        let selected = self.binding.credential_id();
        let current_path = self.project.path.clone();
        tokio::task::spawn_blocking(move || {
            let _io = io;
            root.recheck().map_err(|_| invalid())?;
            let current = root
                .reopen_same_directory(&current_path)
                .map_err(|_| invalid())?;
            rebind(&current,&revision,selected)?;
            root.reopen_same_directory(&current_path)
                .map_err(|_| invalid())?;
            root.recheck().map_err(|_| invalid())
        })
        .await
        .map_err(|_| invalid())??;
        let u = self.project.unlock.upgrade().ok_or(AppError::VaultLocked)?;
        {
            let _profile = app.hold_session(&self.project.owner).await?;
            let _unlock = self.project.owner.hold_unlocked(&u).await?;
            self.check_held(&u).await?;
            if !u
                .working()
                .watch_revisions(self.fences.clone(), self.revoked.clone())
            {
                return Err(invalid());
            }
        }
        // Only actual encrypted Secret storage, not WorkingSet or snapshot.
        let secret = tokio::select! {
            _ = self.revoked.cancelled() => return Err(invalid()),
            _ = job_cancel.cancelled() => return Err(invalid()),
            result = self.password_value(&u) => result?,
        };
        let _profile = app.hold_session(&self.project.owner).await?;
        let _unlock = self.project.owner.hold_unlocked(&u).await?;
        self.check_held(&u).await?;
        if job_cancel.is_cancelled() {
            return Err(invalid());
        }
        let lease = VaultPasswordLease::issue(
            verified,
            &self.binding,
            secret,
            self.revoked.clone(),
            job_cancel.clone(),
            Duration::from_secs(30),
        )
        .map_err(|_| invalid())?;
        // No plaintext copies, background task or untracked shutdown resource.
        drop(_unlock);
        drop(_profile);
        Ok(NativeVaultPassword {
            review: self.clone(),
            lease,
        })
    }
}

#[cfg(test)]
mod tests;
#[cfg(test)]
mod ephemeral_tests;

// Validation is a separate Native purpose, NOT a fake signed job/target scope.
// No public Bridge/Helper dispatch is enabled by this private component.
mod validation;
pub(crate) use validation::NativeVaultValidation;

pub(crate) use validation::owner::VaultValidationOperations;

pub(crate) use validation::job::VaultJobSession;

pub(crate) mod selection;
