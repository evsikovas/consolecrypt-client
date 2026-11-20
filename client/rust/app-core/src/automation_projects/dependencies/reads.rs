//! Private review/one-use acceptance. Does not install, publish or enable jobs.
use super::*;
use futures::FutureExt;

/// Retains the registry's exact accepted roots/resources. No serialization or
/// Clone; dropping it revokes this acceptance and uses tracked owned cleanup.
/// Future publication must revalidate bytes/profile/metadata at its own gate.
pub(crate) struct AcceptedCandidate {
    operation: Arc<Operation>,
    nonce: Uuid,
}
impl std::fmt::Debug for AcceptedCandidate {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AcceptedCandidate")
            .field("operation_id", &self.operation.id)
            .finish_non_exhaustive()
    }
}
impl AcceptedCandidate {
    pub(crate) fn operation_id(&self) -> Uuid {
        self.operation.id
    }
    /// Registry/lifecycle state only, not a claim that filesystem bytes are
    /// immutable. Every later consumer must revalidate the actual roots.
    pub(crate) fn is_retained(&self) -> bool {
        self.operation.accepted_matches(self.nonce)
    }
    pub(super) async fn retain_in_registry(
        self,
        app: &AppCore,
        digest: &str,
    ) -> AppResult<DependencyView> {
        let op = self.operation.clone();
        let u = op.context.current(app).boxed().await?;
        let _profile = app.hold_session(&op.context.project.owner).await?;
        let _unlock = op.context.project.owner.hold_unlocked(&u).await?;
        op.context.check_held(&u).boxed().await?;
        let registry = op.context.project.owner.automation.dependencies.clone();
        registry.retain_acceptance(&op, digest, self.nonce)
        // self drops AFTER handoff. State's private custody flag prevents its
        // external-owner Drop from rejecting registry-retained resources.
    }
}
impl Drop for AcceptedCandidate {
    fn drop(&mut self) {
        self.operation.release_accepted(self.nonce)
    }
}

fn reviewed_digest(value: &str) -> AppResult<()> {
    if !cc_models::automation::is_digest(value) {
        return Err(invalid("dependency_invalid_review"));
    }
    Ok(())
}

pub(super) async fn operation(
    app: &AppCore,
    id: &str,
    digest: &str,
) -> AppResult<(Arc<DependencyRegistry>, Arc<Operation>)> {
    canonical(id)?;
    reviewed_digest(digest)?;
    let (owner, u) = app.unlocked().await?;
    drop(u);
    let registry = owner.automation.dependencies.clone();
    let op =
        registry.get(Uuid::parse_str(id).map_err(|_| invalid("dependency_invalid_request"))?)?;
    drop(op.context.current(app).boxed().await?);
    Ok((registry, op))
}

async fn checked_work<T: Send + 'static>(
    registry: &DependencyRegistry,
    op: &Arc<Operation>,
    digest: &str,
    read: impl FnOnce(&registry::Ready) -> AppResult<T> + Send + 'static,
) -> AppResult<T> {
    let permit = controller::fs_permit()?;
    let current = op.clone();
    // Resource Arc creation and blocking-worker registration share the closed
    // registry fence. An abandoned caller only loses plain DTO/check data.
    let result = registry.review_work(op, digest, move |ready| {
        let _permit = permit;
        if current.cancel.is_cancelled() || Instant::now() >= current.expires {
            return Err(invalid("dependency_cancelled"));
        }
        // Fresh named directory, not only a retained fd to an old renamed tree.
        let named = current
            .context
            .base
            .reopen_same_directory(&current.context.project.path)
            .map_err(core_error)?;
        ready
            .preview
            .revalidate(
                &named,
                &current.context.revision,
                &current.context.selection,
                &ready.candidate,
            )
            .map_err(core_error)?;
        let result = read(ready)?;
        ready
            .preview
            .revalidate(
                &named,
                &current.context.revision,
                &current.context.selection,
                &ready.candidate,
            )
            .map_err(core_error)?;
        current
            .context
            .base
            .reopen_same_directory(&current.context.project.path)
            .map_err(core_error)?;
        if current.cancel.is_cancelled() || Instant::now() >= current.expires {
            return Err(invalid("dependency_cancelled"));
        }
        Ok(result)
    })?;
    result
        .await
        .map_err(|_| invalid("dependency_evidence_unavailable"))?
}

impl AppCore {
    pub(crate) async fn automation_dependency_changes(
        &self,
        id: String,
        expected_review_digest: String,
        offset: u32,
        limit: u32,
    ) -> AppResult<Value> {
        if limit == 0 || limit > 100 || offset > 20_000 {
            return Err(invalid("dependency_invalid_page"));
        }
        let (registry, op) = operation(self, &id, &expected_review_digest)
            .boxed()
            .await?;
        let expected = expected_review_digest.clone();
        let result=checked_work(&registry,&op,&expected_review_digest,move|ready|{
            let diff=ready.preview.diff();
            Ok(json!({"review_digest":expected,"baseline_digest":diff.baseline_digest,"current_digest":diff.current_digest,
                "total":diff.changes.len(),"offset":offset,"changes":diff.changes.iter().skip(offset as usize).take(limit as usize).collect::<Vec<_>>()}))
        }).boxed().await?;
        let u = op.context.current(self).boxed().await?;
        let _profile = self.hold_session(&op.context.project.owner).await?;
        let _unlock = op.context.project.owner.hold_unlocked(&u).await?;
        op.context.check_held(&u).boxed().await?;
        // Recheck exact still-retained state without cloning its resource owner.
        registry.check_review(&op, &expected_review_digest)?;
        Ok(result)
    }

    pub(crate) async fn automation_dependency_file(
        &self,
        id: String,
        expected_review_digest: String,
        path: String,
    ) -> AppResult<cc_automation_core::ProjectFileDiff> {
        cc_automation_core::validate_project_path(&path).map_err(core_error)?;
        let (registry, op) = operation(self, &id, &expected_review_digest)
            .boxed()
            .await?;
        let current = op.clone();
        let result = checked_work(&registry, &op, &expected_review_digest, move |ready| {
            cc_automation_core::compare_file(
                &current.context.base,
                &current.context.revision,
                &ready.candidate,
                ready.preview.candidate(),
                &path,
            )
            .map_err(core_error)
        })
        .boxed()
        .await?;
        let u = op.context.current(self).boxed().await?;
        let _profile = self.hold_session(&op.context.project.owner).await?;
        let _unlock = op.context.project.owner.hold_unlocked(&u).await?;
        op.context.check_held(&u).boxed().await?;
        registry.check_review(&op, &expected_review_digest)?;
        Ok(result)
    }

    pub(crate) async fn automation_dependency_accept(
        &self,
        id: String,
        expected_review_digest: String,
    ) -> AppResult<AcceptedCandidate> {
        let (registry, op) = operation(self, &id, &expected_review_digest)
            .boxed()
            .await?;
        checked_work(&registry, &op, &expected_review_digest, |_| Ok(()))
            .boxed()
            .await?;
        let u = op.context.current(self).boxed().await?;
        let _profile = self.hold_session(&op.context.project.owner).await?;
        let _unlock = op.context.project.owner.hold_unlocked(&u).await?;
        op.context.check_held(&u).boxed().await?;
        registry.check_review(&op, &expected_review_digest)?;
        let nonce = registry.accept_review(&op, &expected_review_digest)?;
        Ok(AcceptedCandidate {
            operation: op.clone(),
            nonce,
        })
    }

    pub(crate) async fn automation_dependency_reject(
        &self,
        id: String,
        expected_review_digest: String,
    ) -> AppResult<()> {
        let (registry, op) = operation(self, &id, &expected_review_digest)
            .boxed()
            .await?;
        let u = op.context.current(self).boxed().await?;
        let _profile = self.hold_session(&op.context.project.owner).await?;
        let _unlock = op.context.project.owner.hold_unlocked(&u).await?;
        op.context.check_held(&u).boxed().await?;
        registry.check_review(&op, &expected_review_digest)?;
        op.reject(&expected_review_digest)
    }
}

impl AppCore {
    pub(super) async fn dependency_review_evidence(
        &self,
        id: String,
        digest: String,
    ) -> AppResult<Value> {
        let (registry, op) = operation(self, &id, &digest).boxed().await?;
        let current = op.clone();
        let expected = digest.clone();
        let evidence=checked_work(&registry,&op,&digest,move|ready|{
            let entry_sha=|expected:&str|ready.preview.candidate().entries.iter().find_map(|entry|match entry {
                cc_automation_core::ProjectEntry::File{path,sha256,..}if path==expected=>Some(sha256.clone()),_=>None});
            let lock=entry_sha(".consolecrypt-dependencies.json");
            let receipt=entry_sha(".consolecrypt-dependency-receipts.json");
            Ok(json!({"reviewDigest":expected,"baseDigest":current.context.revision.digest,"oldLockSha256":current.context.old_lock,
                "candidateDigest":ready.preview.candidate().digest,"candidateLockSha256":lock,"candidateReceiptSha256":receipt,
                "archiveSha256":current.context.selection.source().sha256,"archiveTreeDigest":ready.preview.archive().archive().tree().digest,
                "source":current.context.selection,"destination":current.context.selection.target(),"archiveVerified":true,
                "receiptMergeSupported":false,"published":false,"executionEnabled":false}))
        }).boxed().await?;
        let u = op.context.current(self).boxed().await?;
        let _profile = self.hold_session(&op.context.project.owner).await?;
        let _unlock = op.context.project.owner.hold_unlocked(&u).await?;
        op.context.check_held(&u).boxed().await?;
        registry.check_review(&op, &digest)?;
        Ok(evidence)
    }
}
