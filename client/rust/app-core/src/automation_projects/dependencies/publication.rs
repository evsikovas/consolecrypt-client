//! Private managed first-install only. Rename and SQL are separate commits;
//! no Bridge action, execution, implicit accept, receipt rewrite or rollback.
use super::*;
use cc_automation_core::{
    CatalogPublicationAck, DependencyPublicationSource, PublishedDependencyGeneration,
    prepare_publish,
};
use futures::FutureExt;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DependencyPublicationResult {
    version: u8,
    operation_id: Uuid,
    project_id: String,
    revision: u64,
    transaction: Uuid,
    candidate_digest: String,
    // Fixed Core enum display, not an inferred power-loss/remote-stop promise.
    durability: String,
}

#[cfg(test)]
fn owned_diagnostic(
    op: &Operation,
    phase: &'static str,
    error: Option<&cc_automation_core::AutomationError>,
) {
    use std::path::{Path, PathBuf};
    let Ok(token) = std::env::var("CC_ANS05_APPCORE_TOKEN") else {
        return;
    };
    let Ok(id) = Uuid::parse_str(&token) else {
        return;
    };
    if id.is_nil()
        || id.to_string() != token
        || std::env::var("CC_ANS05_PUBLICATION_PROOF").ok().as_deref() != Some(token.as_str())
    {
        return;
    }
    let Some(root) = std::env::var_os("CC_ANS05_APPCORE_ROOT").map(PathBuf::from) else {
        return;
    };
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(4)
        .unwrap();
    let Ok(allowed) = workspace
        .join("automation-e1-next/app-core-integration/dependency-live-remote-v1/control")
        .canonicalize()
    else {
        return;
    };
    let Ok(root) = root.canonicalize() else {
        return;
    };
    if root.parent() != Some(allowed.as_path())
        || root.file_name().and_then(|n| n.to_str()) != Some(format!("owned-{token}").as_str())
    {
        return;
    }
    let Some(profile) = op
        .context
        .project
        .owner
        .dir
        .as_ref()
        .and_then(|p| p.canonicalize().ok())
    else {
        return;
    };
    if !profile.starts_with(&root) {
        return;
    }
    let (class, kind, code) = match error {
        Some(cc_automation_core::AutomationError::Io(error)) => {
            ("io", format!("{:?}", error.kind()), error.raw_os_error())
        }
        Some(cc_automation_core::AutomationError::PublicationUnsupported(_)) => {
            ("unsupported", "none".into(), None)
        }
        Some(cc_automation_core::AutomationError::Invalid(_)) => ("invalid", "none".into(), None),
        Some(_) => ("other", "none".into(), None),
        None => ("checkpoint", "none".into(), None),
    };
    eprintln!("OWNED_DEPENDENCY_PUBLICATION_DIAG phase={phase} errorClass={class} ioKind={kind} osCode={code:?}");
}

struct SqlAck {
    revision: u64,
    row_sha: String,
}
struct PublicationRetirement {
    operation: Arc<Operation>,
    nonce: Uuid,
    finished: bool,
}
impl Drop for PublicationRetirement {
    fn drop(&mut self) {
        if !self.finished {
            self.operation.abandon_publication(self.nonce);
        }
    }
}
fn unavailable() -> AppError {
    invalid("dependency_publication_recovery_required")
}
/// Final scope-only fence AFTER actual joined cleanup. OLD row/revision cannot
/// be reused after our own acknowledged CAS, and latest-global scope is never a
/// substitute for the original weak Unlocked and Session identity.
pub(super) async fn final_original_scope<T>(
    app: &AppCore,
    context: &Context,
    finish: impl FnOnce() -> T,
) -> AppResult<T> {
    let u = context.project.unlock.upgrade().ok_or_else(unavailable)?;
    let _profile = app
        .hold_session(&context.project.owner)
        .await
        .map_err(|_| unavailable())?;
    let _unlock = context
        .project
        .owner
        .hold_unlocked(&u)
        .await
        .map_err(|_| unavailable())?;
    if context.project.owner.automation.unlocked_generation()? != context.generation
        || context.project.owner.automation.stopped.is_cancelled()
    {
        return Err(unavailable());
    }
    Ok(finish()) // no await after the held original-scope linearization point
}
/// Metadata gate only. No copy, SQL mutation, acceptance or publication token.
/// It MUST run inside the same actual transaction that will select the result.
fn checked_catalog_rows(tx: &cc_storage_core::Tx<'_>, context: &Context) -> AppResult<Catalog> {
    let catalog: Catalog = tx.setting_get(PROJECTS)?.unwrap_or_default();
    let id = &context.project.project.id;
    if catalog.version != 1 || !catalog.extra.is_empty() || catalog.records.len() > 256 {
        return Err(unavailable());
    }
    let row = catalog.records.get(id).ok_or_else(unavailable)?;
    if !managed::supported_at(context.project.owner.dir.as_deref(), id, row)
        || managed::row_stamp(row)? != context.project_stamp
        || row["revision"].as_u64() != context.project.project.revision
    {
        return Err(unavailable());
    }
    let runners = tx
        .setting_get(crate::automation_runners::RUNNERS)?
        .unwrap_or_else(|| json!({"version":1,"records":{}}));
    let runner = crate::automation_runners::dependency::decode_dependency_runner(
        runners,
        &context.runner.id,
        context.runner.revision,
    )?;
    if runner.stamp != context.runner.stamp {
        return Err(unavailable());
    }
    Ok(catalog)
}

async fn filesystem<T: Send + 'static>(
    registry: &DependencyRegistry,
    work: impl FnOnce() -> AppResult<T> + Send + 'static,
) -> AppResult<T> {
    let permit = controller::fs_permit()?;
    let (send, receive) = tokio::sync::oneshot::channel();
    registry.launch_worker(move || {
        let _permit = permit;
        let _ = send.send(work());
    })?;
    receive.await.map_err(|_| unavailable())?
}

impl AppCore {
    /// Accept is a different, prior explicit transition. This consumes ONLY
    /// an existing registry-retained acceptance, never a request approval flag.
    pub(crate) async fn automation_dependency_publish(
        &self,
        operation: String,
        review_digest: String,
    ) -> AppResult<DependencyPublicationResult> {
        let (registry, op) = reads::operation(self, &operation, &review_digest).await?;
        if op.context.project.project.source_kind.as_deref() != Some("managed")
            || op.context.project.owner.dir.is_none()
        {
            return Err(invalid("dependency_publication_unsupported"));
        }
        let io = op
            .context
            .project
            .owner
            .automation
            .io_slots
            .clone()
            .try_acquire_many_owned(crate::automation_runners::AUTOMATION_IO_CAPACITY)
            .map_err(|_| invalid("dependency_publication_busy"))?;
        drop(op.context.current(self).boxed().await?);
        let app = self.clone();
        let current = op.clone();
        let owned_registry = registry.clone();
        let receive = registry.publish_work(&op, &review_digest, io, move |ready, nonce| {
            async move {
                let mut retirement = PublicationRetirement {
                    operation: current.clone(),
                    nonce,
                    finished: false,
                };
                let result = publish(&app, &owned_registry, &current, &ready, nonce).await;
                #[cfg(test)]
                if result.is_err() {
                    owned_diagnostic(&current, "pipelineReturnedError", None);
                }
                let acknowledged = result.is_ok();
                let provisional = current.finish_publication(nonce, acknowledged);
                // The state no longer retains Ready. Any earlier review worker
                // still retains its own tracked clone and the SAME I/O custody.
                drop(ready);
                let joined =
                    tokio::time::timeout(Duration::from_secs(30), current.joined_cleanup())
                        .await
                        .unwrap_or(false);
                let success = provisional
                    && joined
                    && final_original_scope(&app, &current.context, || {
                        current.complete_publication_cleanup(nonce, true)
                    })
                    .await
                    .unwrap_or(false);
                if !success {
                    current.abandon_publication(nonce);
                }
                retirement.finished = true;
                if !success {
                    return Err(unavailable());
                }
                result
            }
            .boxed()
        })?;
        let _caller = crate::automation_prepare::CancelOnDrop(op.cancel.clone());
        receive.await.map_err(|_| unavailable())?
    }
}

async fn publish(
    app: &AppCore,
    registry: &DependencyRegistry,
    op: &Arc<Operation>,
    ready: &Arc<registry::Ready>,
    nonce: Uuid,
) -> AppResult<DependencyPublicationResult> {
    let captured = op.clone();
    let retained = ready.clone();
    let profile = op
        .context
        .project
        .owner
        .dir
        .clone()
        .ok_or_else(unavailable)?;
    let mut published: PublishedDependencyGeneration = filesystem(registry, move || {
        if captured.cancel.is_cancelled() || Instant::now() >= captured.expires {
            return Err(unavailable());
        }
        let named = captured
            .context
            .base
            .reopen_same_directory(&captured.context.project.path)
            .map_err(|_| unavailable())?;
        let source = DependencyPublicationSource::new(
            &profile,
            "managed",
            Uuid::parse_str(&captured.context.project.project.id).map_err(|_| unavailable())?,
            captured.id,
            captured
                .context
                .project
                .project
                .revision
                .ok_or_else(unavailable)?,
            &captured.context.project_stamp,
        )
        .map_err(|_| unavailable())?;
        #[cfg(test)]
        owned_diagnostic(&captured, "beforeCorePreparePublish", None);
        let mut result = prepare_publish(
            source,
            &named,
            &captured.context.revision,
            &captured.context.selection,
            &retained.candidate,
            &retained.preview,
            &captured.cancel,
        )
        .map_err(|_error| {
            #[cfg(test)]
            owned_diagnostic(&captured, "corePreparePublishError", Some(&_error));
            unavailable()
        })?;
        // All checks still bind immutable OLD catalog facts; do not substitute
        // the newly installed row's revision/hash into the Core witness.
        result
            .recheck(
                &named,
                &captured.context.revision,
                &captured.context.selection,
                &retained.candidate,
                &retained.preview,
            )
            .map_err(|_error| {
                #[cfg(test)]
                owned_diagnostic(&captured, "corePublishedRecheckError", Some(&_error));
                unavailable()
            })?;
        Ok(result)
    })
    .await?;
    #[cfg(test)]
    owned_diagnostic(op, "coreFilesystemPublished", None);
    let project = op.context.project.project.id.clone();
    let transaction = published.transaction();
    let target = published.target_path().to_path_buf();
    let candidate_digest = published.candidate_digest().to_owned();
    let durability = format!("{:?}", published.durability());
    let publication_fingerprint = published.audit_fingerprint().map_err(|_| unavailable())?;
    let owner = op.context.project.owner.clone();
    let u = op
        .context
        .project
        .unlock
        .upgrade()
        .ok_or_else(unavailable)?;
    let ack = {
        let _profile = app.hold_session(&owner).await.map_err(|_| unavailable())?;
        let _unlock = owner.hold_unlocked(&u).await.map_err(|_| unavailable())?;
        let current = op.clone();
        let original = Arc::downgrade(&u);
        let profile_path = owner.dir.clone();
        let committed_digest = candidate_digest.clone();
        let reviewed_digest = ready.preview.review_digest().to_owned();
        // SAME SQL transaction reads BOTH catalogs, then synchronous original
        // fences. No recursive async capture_runner/project_stamp storage call.
        owner
            .storage()?
            .write(move |tx| -> AppResult<SqlAck> {
                let mut catalog = checked_catalog_rows(tx, &current.context)?;
                let row = catalog.records.get_mut(&project).ok_or_else(unavailable)?;
                let u = original.upgrade().ok_or_else(unavailable)?;
                if current
                    .context
                    .project
                    .owner
                    .automation
                    .stopped
                    .is_cancelled()
                    || current
                        .context
                        .project
                        .owner
                        .automation
                        .unlocked_generation()?
                        != current.context.generation
                    || u.working().version() != current.context.version
                {
                    return Err(unavailable());
                }
                let revision = current
                    .context
                    .project
                    .project
                    .revision
                    .ok_or_else(unavailable)?
                    .checked_add(1)
                    .ok_or_else(unavailable)?;
                row["source"]["path"] = json!(target);
                row["revision"] = json!(revision);
                row["updated_at"] = json!(chrono::Utc::now().timestamp());
                row["publication"] = serde_json::to_value(managed::PublicationMarker {
                    version: 1,
                    transaction,
                    operation: current.id,
                    installed_revision: revision,
                    base_digest: current.context.revision.digest.clone(),
                    candidate_digest: committed_digest,
                    review_digest: reviewed_digest,
                    previous_row_sha256: current.context.project_stamp.clone(),
                    publication_fingerprint,
                })
                .map_err(|_| unavailable())?;
                managed::validate_path(profile_path.as_deref(), &project, row)?;
                let row_sha = managed::row_stamp(row)?;
                if u.working().version() != current.context.version {
                    return Err(unavailable());
                }
                current.begin_sql_commit(nonce)?;
                tx.setting_set(PROJECTS, &catalog)?;
                Ok(SqlAck { revision, row_sha })
            })
            .await?
    };
    #[cfg(test)]
    owned_diagnostic(op, "sqlAcknowledged", None);
    drop(u); // no vault keys across audit filesystem work
    let current = op.clone();
    let retained = ready.clone();
    let revision = ack.revision;
    filesystem(registry, move || {
        let native_ack = CatalogPublicationAck::new(
            Uuid::parse_str(&current.context.project.project.id).map_err(|_| unavailable())?,
            transaction,
            revision,
            &ack.row_sha,
        )
        .map_err(|_| unavailable())?;
        published
            .record_catalog_ack(
                &native_ack,
                &current.context.base,
                &current.context.revision,
                &current.context.selection,
                &retained.candidate,
                &retained.preview,
            )
            .map_err(|_| unavailable())?;
        published
            .record_complete(
                &native_ack,
                &current.context.base,
                &current.context.revision,
                &current.context.selection,
                &retained.candidate,
                &retained.preview,
            )
            .map_err(|_| unavailable())?;
        Ok(())
    })
    .await?;
    #[cfg(test)]
    owned_diagnostic(op, "auditComplete", None);
    Ok(DependencyPublicationResult {
        version: 1,
        operation_id: op.id,
        project_id: op.context.project.project.id.clone(),
        revision,
        transaction,
        candidate_digest,
        durability,
    })
}

#[cfg(test)]
mod tests;
