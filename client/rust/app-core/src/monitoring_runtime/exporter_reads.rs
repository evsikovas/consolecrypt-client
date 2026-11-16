//! Explicit bounded observations through the existing saved-source runtime.
use super::ReadAction;
use crate::automation_jobs::metric_context::{self, JobMetricContext};
use crate::{
    AppCore, AppError, AppResult,
    session::{Session, Unlocked},
};
use cc_monitoring_core::{
    MonitoringClient,
    exporter_verification::{ExporterKind, ExporterSelector, ExporterVerificationPlan},
};
use serde::Deserialize;
use serde_json::{Value, json};
use std::sync::Arc;
use uuid::Uuid;
fn invalid(rule: &'static str) -> AppError {
    AppError::invalid("monitoring_exporter", rule)
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct ContextRequest {
    version: u32,
    profile_id: String,
    host_id: String,
    job_id: String,
    job_revision: u64,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct VerificationRequest {
    version: u32,
    profile_id: String,
    notification_scope_id: String,
    host_id: String,
    host_revision: u64,
    job_id: String,
    job_revision: u64,
    exporter: ExporterKind,
    selector: ExporterSelector,
    max_age_seconds: u16,
}
fn id(raw: &str) -> AppResult<Uuid> {
    let id = Uuid::parse_str(raw).map_err(|_| invalid("exporter_request_invalid"))?;
    if id.is_nil() || id.to_string() != raw {
        return Err(invalid("exporter_request_invalid"));
    }
    Ok(id)
}
fn revision(raw: u64) -> AppResult<()> {
    if raw == 0 || raw > 9_007_199_254_740_991 {
        return Err(invalid("exporter_request_invalid"));
    }
    Ok(())
}
fn host(u: &Unlocked, raw: &str) -> AppResult<u64> {
    let object = crate::dto::parse_id("monitoring_exporter", raw)?;
    let (revision, kind) = u
        .working()
        .correlation_host_metadata(object)
        .ok_or_else(|| invalid("exporter_host_unavailable"))?;
    let revision = u64::try_from(revision).map_err(|_| invalid("exporter_host_unavailable"))?;
    if kind != "ssh" || revision == 0 || revision > 9_007_199_254_740_991 {
        return Err(invalid("exporter_host_unavailable"));
    }
    Ok(revision)
}
fn original(owner: &Session, u: &Unlocked, profile: &str) -> AppResult<String> {
    id(profile)?;
    if owner.profile_id.to_string() != profile {
        return Err(invalid("exporter_context_changed"));
    }
    u.notices.scope_id().ok_or(AppError::VaultLocked)
}
fn view(
    profile: &str,
    scope: &str,
    host: &str,
    host_revision: u64,
    source: &str,
    source_revision: u64,
    job: &JobMetricContext,
) -> Value {
    json!({"version":1,"profileId":profile,"notificationScopeId":scope,"hostId":host,"hostRevision":host_revision,"protocol":"ssh",
        "sourceId":source,"sourceRevision":source_revision,"jobId":job.job,"jobRevision":job.revision,
        "projectId":job.project,"revisionDigest":job.revision_digest})
}

/// This branch runs BEFORE generic route/auth/client creation. The context is
/// original metadata only, not a successful metric read or binding permission.
pub(super) async fn context(
    app: &AppCore,
    owner: &Arc<Session>,
    u: &Arc<Unlocked>,
    request: &ReadAction,
    source: &str,
    source_revision: u64,
) -> AppResult<Option<Value>> {
    let ReadAction::ExporterContext(input) = request else {
        return Ok(None);
    };
    if input.version != 1 {
        return Err(invalid("exporter_unsupported_version"));
    }
    id(&input.host_id)?;
    let job = id(&input.job_id)?;
    revision(input.job_revision)?;
    revision(source_revision)?;
    let _profile = app.hold_session(owner).await?;
    let _unlock = owner.hold_unlocked(u).await?;
    let scope = original(owner, u, &input.profile_id)?;
    let job = metric_context::capture(
        owner,
        id(&input.profile_id)?,
        job,
        input.job_revision,
        id(&input.host_id)?,
    )
    .await?;
    let before = u.working().version();
    crate::monitoring_sources::source_metadata_revision(u, source, source_revision)?;
    let host_revision = host(u, &input.host_id)?;
    if before != u.working().version() {
        return Err(invalid("exporter_context_changed"));
    }
    let mut result = view(
        &input.profile_id,
        &scope,
        &input.host_id,
        host_revision,
        source,
        source_revision,
        &job,
    );
    result["kind"] = json!("exporterContext");
    Ok(Some(result))
}

pub(super) struct ExporterRead {
    profile: String,
    scope: String,
    host: String,
    host_revision: u64,
    source: String,
    source_revision: u64,
    job: JobMetricContext,
    kind: ExporterKind,
    plan: ExporterVerificationPlan,
}
impl ExporterRead {
    pub(super) async fn prepare(
        app: &AppCore,
        owner: &Arc<Session>,
        u: &Arc<Unlocked>,
        action: &ReadAction,
        source: &str,
        source_revision: u64,
    ) -> AppResult<Option<Self>> {
        let ReadAction::ExporterVerification(input) = action else {
            return Ok(None);
        };
        if input.version != 1 {
            return Err(invalid("exporter_unsupported_version"));
        }
        id(&input.notification_scope_id)?;
        id(&input.host_id)?;
        revision(input.host_revision)?;
        revision(input.job_revision)?;
        revision(source_revision)?;
        let plan = ExporterVerificationPlan::new(
            input.exporter,
            input.selector.clone(),
            input.max_age_seconds,
        )
        .map_err(|_| invalid("exporter_plan_invalid"))?;
        let _profile = app.hold_session(owner).await?;
        let _unlock = owner.hold_unlocked(u).await?;
        let scope = original(owner, u, &input.profile_id)?;
        if scope != input.notification_scope_id || host(u, &input.host_id)? != input.host_revision {
            return Err(invalid("exporter_context_changed"));
        }
        let job = metric_context::capture(
            owner,
            id(&input.profile_id)?,
            id(&input.job_id)?,
            input.job_revision,
            id(&input.host_id)?,
        )
        .await?;
        Ok(Some(Self {
            profile: input.profile_id.clone(),
            scope,
            host: input.host_id.clone(),
            host_revision: input.host_revision,
            source: source.into(),
            source_revision,
            job,
            kind: input.exporter,
            plan,
        }))
    }
    pub(super) fn host_id(&self) -> &str {
        &self.host
    }
    pub(super) fn host_revision(&self) -> u64 {
        self.host_revision
    }
    /// Caller holds original session/unlock. Fresh SQL read, no network.
    pub(super) async fn check(&self, owner: &Session, u: &Unlocked) -> AppResult<()> {
        let current = metric_context::capture(
            owner,
            id(&self.profile)?,
            self.job.job,
            self.job.revision,
            id(&self.host)?,
        )
        .await?;
        if current != self.job {
            return Err(invalid("exporter_job_changed"));
        }
        // Background readbacks can change the WorkingSet while SQL awaits,
        // even at an equal revision. No awaited work may follow these fences.
        if original(owner, u, &self.profile)? != self.scope
            || host(u, &self.host)? != self.host_revision
        {
            return Err(invalid("exporter_context_changed"));
        }
        Ok(())
    }
    pub(super) async fn execute(&self, client: &MonitoringClient) -> Value {
        let observation = self.plan.execute(client).await;
        let mut result = view(
            &self.profile,
            &self.scope,
            &self.host,
            self.host_revision,
            &self.source,
            self.source_revision,
            &self.job,
        );
        result["kind"] = json!("exporterVerification");
        result["exporter"] = json!(self.kind);
        result["observation"] = json!(observation);
        result
    }
}

#[cfg(test)]
mod tests;

// Reuse only the original live helper when AppCore is a non-test dependency.
#[cfg(all(feature = "test-harness", not(test)))]
#[path = "exporter_reads/tests/live_cadvisor.rs"]
pub(crate) mod live_cadvisor_harness;
