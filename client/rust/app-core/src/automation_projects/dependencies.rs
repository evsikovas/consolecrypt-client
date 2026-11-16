//! Private Native source-review/prepare controller. No bridge, publish or jobs.
use super::*;
use crate::automation_runners::dependency::{capture_runner, DependencyRunner};
use crate::session::Unlocked;
use cc_automation_core::{
    dependency_preflight, DependencyBasePreflight, DependencyHelperConnection,
    DependencySourceSelection, VerifiedDependencyPreview,
};
use std::{
    sync::Mutex,
    time::{Duration, Instant},
};
use tokio::sync::{OwnedSemaphorePermit, Semaphore};
use tokio_util::sync::CancellationToken;

mod registry;
pub(crate) use registry::{DependencyRegistry, DependencyView, Phase};
use registry::{Operation, Reservation, Ticket};
mod controller;
mod publication;
mod reconciliation;
mod reads;
pub(crate) use reads::AcceptedCandidate;

fn invalid(code: &'static str) -> AppError {
    AppError::invalid("automation_dependency", code)
}
fn core_error(error: cc_automation_core::AutomationError) -> AppError {
    match error {
        cc_automation_core::AutomationError::Invalid(
            "existing dependency receipts require the future merge workflow",
        ) => invalid("dependency_existing_receipt_unsupported"),
        cc_automation_core::AutomationError::Closed => invalid("dependency_cancelled"),
        _ => invalid("dependency_evidence_unavailable"),
    }
}
fn canonical(value: &str) -> AppResult<()> {
    let id = Uuid::parse_str(value).map_err(|_| invalid("dependency_invalid_request"))?;
    if id.is_nil() || id.to_string() != value {
        return Err(invalid("dependency_invalid_request"));
    }
    Ok(())
}
fn hash(value: &impl Serialize) -> AppResult<String> {
    use sha2::{Digest, Sha256};
    let raw = serde_json::to_vec(value).map_err(AppError::internal)?;
    if raw.len() > 1024 * 1024 {
        return Err(invalid("dependency_context_budget"));
    }
    Ok(hex::encode(Sha256::digest(raw)))
}

/// Captured data only. Does not retain an unlock/credential resolver or claim
/// a user action from a guest tag. Only the private Native review entry issues it.
struct Context {
    project: AutomationProjectContext,
    project_stamp: String,
    runner: DependencyRunner,
    generation: u64,
    version: u64,
    watched: BTreeMap<cc_models::ObjectId, i64>,
    base: Arc<managed::OpenedProjectRoot>,
    revision: ProjectRevision,
    old_lock: Option<String>,
    selection: DependencySourceSelection,
}
impl Context {
    /// Caller holds the original session/unlock guards; never recursively
    /// acquire them here and never perform filesystem work under those guards.
    async fn check_held(&self, u: &Unlocked) -> AppResult<()> {
        if self.project.owner.automation.unlocked_generation()? != self.generation
            || u.working().version() != self.version
            || project_stamp(&self.project).await? != self.project_stamp
            || capture_runner(&self.project.owner, &self.runner.id, self.runner.revision)
                .await?
                .stamp
                != self.runner.stamp
        {
            return Err(invalid("dependency_context_changed"));
        }
        Ok(())
    }
    async fn current(&self, app: &AppCore) -> AppResult<Arc<Unlocked>> {
        let u = self.project.unlock.upgrade().ok_or(AppError::VaultLocked)?;
        let _profile = app.hold_session(&self.project.owner).await?;
        let _unlock = self.project.owner.hold_unlocked(&u).await?;
        self.check_held(&u).await?;
        drop(_unlock);
        drop(_profile);
        Ok(u)
    }
}
/// Called only under the original active-session/unlock guards. SQL project
/// changes do not bump WorkingSet version; do not recurse into context loader.
pub(super) async fn project_stamp(context: &AutomationProjectContext) -> AppResult<String> {
    let catalog: Catalog = context
        .owner
        .storage()?
        .setting_get(PROJECTS)
        .await?
        .unwrap_or_default();
    let row = catalog
        .records
        .get(&context.project.id)
        .ok_or_else(|| invalid("dependency_context_changed"))?;
    if catalog.version != 1
        || !catalog.extra.is_empty()
        || !supported(row)
        || row["revision"].as_u64() != context.project.revision
        || row.pointer("/source/path").and_then(Value::as_str) != context.path.to_str()
        || groups::from_row(row)? != context.inventory
        || source_bindings::load(&context.owner, &context.project.id, row).await?
            != context.external_inventory
    {
        return Err(invalid("dependency_context_changed"));
    }
    hash(row)
}
fn preflight_matches(context: &Context, preflight: &DependencyBasePreflight) -> bool {
    preflight.base_digest() == context.revision.digest
        && preflight.lock_sha256() == context.old_lock.as_deref()
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SourceReviewView {
    version: u32,
    review_id: Uuid,
    operation_id: Uuid,
    project_id: String,
    project_revision: u64,
    runner_id: String,
    runner_revision: u64,
    source: DependencySourceSelection,
    base_digest: String,
    old_lock_sha256: Option<String>,
    destination: String,
    network_origin: String,
    publication_supported: bool,
    publication_reason: Option<&'static str>,
    expires_in_seconds: u32,
}

#[cfg(test)]
mod tests;

mod facade;
