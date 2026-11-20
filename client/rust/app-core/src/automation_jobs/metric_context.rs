//! Saved Native job metadata, not a new execution/identity witness or grant.
use super::{Catalog, JOBS, MAX_NOTIFICATION_REVISION, row_identity};
use crate::{AppError, AppResult, session::Session};
use cc_models::automation::{JOB_SIGNATURE_DOMAIN, JobManifest};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use uuid::Uuid;

fn invalid() -> AppError {
    AppError::invalid("monitoring_exporter", "exporter_job_changed")
}
#[derive(Clone, PartialEq, Eq)]
pub(crate) struct JobMetricContext {
    pub(crate) job: Uuid,
    pub(crate) revision: u64,
    pub(crate) project: Uuid,
    pub(crate) revision_digest: String,
    manifest_digest: String,
}
/// Caller holds the SAME original session/unlock guards. No guest result,
/// caller status, signer/ref material or arbitrary output is accepted/returned.
pub(crate) async fn capture(
    owner: &Session,
    profile: Uuid,
    job: Uuid,
    revision: u64,
    host: Uuid,
) -> AppResult<JobMetricContext> {
    if revision == 0 || revision > MAX_NOTIFICATION_REVISION {
        return Err(invalid());
    }
    let catalog: Catalog = owner
        .storage()?
        .setting_get(JOBS)
        .await?
        .unwrap_or_default();
    if catalog.version != 1 || !catalog.extra.is_empty() || catalog.records.len() > 2048 {
        return Err(invalid());
    }
    let row = catalog.records.get(&job.to_string()).ok_or_else(invalid)?;
    let (stored_job, runner, hosts) = row_identity(row).map_err(|_| invalid())?;
    if stored_job != job
        || row["state"] != "succeeded"
        || row["notification_revision"].as_u64() != Some(revision)
    {
        return Err(invalid());
    }
    let manifest: JobManifest =
        serde_json::from_value(row["manifest"].clone()).map_err(|_| invalid())?;
    let raw = manifest.canonical_bytes().map_err(|_| invalid())?;
    let mut digest = Sha256::new();
    digest.update(JOB_SIGNATURE_DOMAIN);
    digest.update(raw);
    let manifest_digest = hex::encode(digest.finalize());
    let declared: BTreeSet<_> = hosts.iter().copied().collect();
    let approved: BTreeSet<_> = manifest
        .targets
        .iter()
        .map(|target| target.host_id)
        .collect();
    if manifest.job_id != job
        || manifest.runner_id != runner
        || manifest.tenant_id != profile
        || row["project_id"] != serde_json::json!(manifest.project_id)
        || row["manifest_digest"] != manifest_digest
        || declared.len() != hosts.len()
        || declared != approved
        || !approved.contains(&host)
    {
        return Err(invalid());
    }
    Ok(JobMetricContext {
        job,
        revision,
        project: manifest.project_id,
        revision_digest: manifest.revision_digest,
        manifest_digest,
    })
}
