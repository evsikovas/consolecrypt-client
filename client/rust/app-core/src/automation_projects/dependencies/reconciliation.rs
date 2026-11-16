//! Metadata-only completion for a generation ALREADY selected by actual SQL.
//! No new install, publisher, archive witness, network or destructive recovery.
use super::*;
use cc_automation_core::{
    CatalogPublicationAck, DependencyPublicationSource, PublicationAuditObservation,
    PublicationRecoveryReference, reconcile_selected_catalog,
};

fn recovery() -> AppError {
    invalid("dependency_publication_recovery_required")
}
fn live(cancel: &CancellationToken, until: Instant) -> AppResult<()> {
    if cancel.is_cancelled() || Instant::now() >= until {
        Err(recovery())
    } else {
        Ok(())
    }
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DependencyReconciliationView {
    version: u8,
    project_id: String,
    selected_revision: u64,
    transaction: Uuid,
    observation: &'static str,
}
struct SelectedAudit {
    marker: managed::PublicationMarker,
    row_sha: String,
}
async fn capture_selected(
    app: &AppCore,
    project: &Arc<AutomationProjectContext>,
    expected: u64,
) -> AppResult<SelectedAudit> {
    with_selected(app, project, expected, Ok).await
}
async fn with_selected<T>(
    app: &AppCore,
    project: &Arc<AutomationProjectContext>,
    expected: u64,
    complete: impl FnOnce(SelectedAudit) -> AppResult<T>,
) -> AppResult<T> {
    let u = project.unlock.upgrade().ok_or_else(recovery)?;
    let _profile = app
        .hold_session(&project.owner)
        .await
        .map_err(|_| recovery())?;
    let _unlock = project
        .owner
        .hold_unlocked(&u)
        .await
        .map_err(|_| recovery())?;
    let original = project.clone();
    let selected = project
        .owner
        .storage()?
        .write(move |tx| -> AppResult<SelectedAudit> {
            let catalog: Catalog = tx.setting_get(PROJECTS)?.unwrap_or_default();
            let id = &original.project.id;
            let row = catalog.records.get(id).ok_or_else(recovery)?;
            if catalog.version != 1
                || !catalog.extra.is_empty()
                || catalog.records.len() > 256
                || !managed::supported_at(original.owner.dir.as_deref(), id, row)
                || row["revision"].as_u64() != Some(expected)
                || managed::row_stamp(row)? != original.catalog_row_sha256
            {
                return Err(recovery());
            }
            let marker = managed::marker(row)?.ok_or_else(recovery)?;
            if marker.installed_revision != expected {
                return Err(recovery());
            }
            Ok(SelectedAudit {
                marker,
                row_sha: managed::row_stamp(row)?,
            })
        })
        .await?;
    // The final response can be linearized synchronously under the ORIGINAL
    // held scope, after this fresh same-tx row read, with no later await.
    complete(selected)
}
impl AppCore {
    pub(crate) async fn automation_dependency_reconcile_selected(
        &self,
        project_id: String,
        expected_revision: u64,
    ) -> AppResult<DependencyReconciliationView> {
        canonical(&project_id)?;
        if expected_revision < 2 {
            return Err(recovery());
        }
        let project = Arc::new(self.automation_project_context(&project_id).await?);
        let registry = project.owner.automation.dependencies.clone();
        let epoch = registry.local_filesystem_epoch()?;
        let cancel = epoch.child_token();
        let _caller = crate::automation_prepare::CancelOnDrop(cancel.clone());
        let until = Instant::now() + Duration::from_secs(60);
        let io = project
            .owner
            .automation
            .io_slots
            .clone()
            .try_acquire_many_owned(crate::automation_runners::AUTOMATION_IO_CAPACITY)
            .map_err(|_| invalid("dependency_publication_busy"))?;
        project.check_after_io(self).await?;
        let selected = capture_selected(self, &project, expected_revision).await?;
        live(&cancel, until)?;
        let fs = controller::fs_permit()?;
        let app = self.clone();
        let runtime = tokio::runtime::Handle::current();
        let (send, receive) = tokio::sync::oneshot::channel();
        let worker_cancel = cancel.clone();
        registry.launch_worker(move || {
            // No caller Drop/lock releases these while synchronous Core I/O
            // still runs. This is the existing owner registry, not a new map.
            let mut permits = Some((io, fs));
            let mut response = Some(send);
            let cancel = worker_cancel;
            let result = runtime.block_on(async {
                live(&cancel, until)?;
                project.check_after_io(&app).await.map_err(|_| recovery())?;
                live(&cancel, until)?;
                let profile = project.owner.dir.as_deref().ok_or_else(recovery)?;
                let project_uuid = Uuid::parse_str(&project_id).map_err(|_| recovery())?;
                let source = DependencyPublicationSource::new(
                    profile,
                    "managed",
                    project_uuid,
                    Uuid::new_v4(),
                    selected
                        .marker
                        .installed_revision
                        .checked_sub(1)
                        .ok_or_else(recovery)?,
                    &selected.marker.previous_row_sha256,
                )
                .map_err(|_| recovery())?;
                let reference = PublicationRecoveryReference::new(
                    selected.marker.transaction,
                    selected.marker.operation,
                )
                .map_err(|_| recovery())?;
                let ack = CatalogPublicationAck::new(
                    project_uuid,
                    selected.marker.transaction,
                    expected_revision,
                    &selected.row_sha,
                )
                .map_err(|_| recovery())?;
                // The only side effect allowed is bounded identical audit
                // observations. Cannot select/delete/rename/fetch/create roots.
                let observation = reconcile_selected_catalog(
                    source,
                    reference,
                    &ack,
                    &selected.marker.candidate_digest,
                    &selected.marker.publication_fingerprint,
                )
                .map_err(|_| recovery())?;
                with_selected(&app, &project, expected_revision, |_| {
                    live(&cancel, until)?;
                    let result = DependencyReconciliationView {
                        version: 1,
                        project_id,
                        selected_revision: expected_revision,
                        transaction: selected.marker.transaction,
                        observation: match observation {
                            PublicationAuditObservation::FilesystemPublished => {
                                "filesystemPublished"
                            }
                            PublicationAuditObservation::CatalogCommitted => "catalogCommitted",
                            PublicationAuditObservation::Complete => "complete",
                        },
                    };
                    // All synchronous Core work and final original-scope SQL
                    // checks are done. Release capacity BEFORE the response,
                    // preventing a healthy sequential call from seeing busy.
                    drop(permits.take());
                    if let Some(send) = response.take() {
                        let _ = send.send(Ok(result));
                    }
                    Ok(())
                })
                .await
            });
            drop(permits.take());
            if let Err(error) = result {
                if let Some(send) = response.take() {
                    let _ = send.send(Err(error));
                }
            }
        })?;
        tokio::select! {
            biased;
            _ = cancel.cancelled() => Err(recovery()),
            _ = tokio::time::sleep_until(until.into()) => Err(recovery()),
            result = receive => result.map_err(|_| recovery())?,
        }
    }
}

#[cfg(test)]
mod tests;
