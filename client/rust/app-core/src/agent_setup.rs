//! Original-owner Agent wizard preflight. No project factory or metrics server.
//!
//! Release/destination admission is deliberately closed until authenticated
//! product catalogs exist. Caller JSON cannot supply keys, commands or grants.
use super::scope::OriginalSessionDeviceIdentity as OriginalScope;
use crate::{AppCore, AppError, AppResult};
use cc_models::ObjectId;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    sync::{
        Arc, Weak,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

const MAX_REQUEST: usize = 4096;
const PROFILE: &str = "linux_basic";
fn invalid(rule: &'static str) -> AppError {
    AppError::invalid("agent_setup", rule)
}

#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Frame {
    version: u8,
    profile_id: String,
    notification_scope_id: String,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Request {
    version: u8,
    profile_id: String,
    notification_scope_id: String,
    host_id: String,
    expected_host_revision: String,
    monitoring_profile_id: String,
    #[serde(default)]
    destination_ref: Option<String>,
}
fn parse<T: serde::de::DeserializeOwned>(raw: &str) -> AppResult<T> {
    if raw.is_empty() || raw.len() > MAX_REQUEST {
        return Err(invalid("invalid_request"));
    }
    serde_json::from_str(raw).map_err(|_| invalid("invalid_request"))
}
fn revision(raw: &str) -> AppResult<i64> {
    raw.parse::<i64>()
        .ok()
        .filter(|n| *n > 0 && n.to_string() == raw)
        .ok_or_else(|| invalid("invalid_host_revision"))
}
fn host_id(raw: &str) -> AppResult<ObjectId> {
    let id = crate::dto::parse_id("host", raw)?;
    if id.to_string() != raw {
        return Err(invalid("invalid_host_id"));
    }
    Ok(id)
}
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct HostRef {
    id: String,
    revision: String,
    name: String,
    address: String,
}
struct Context {
    core: Weak<crate::app::Inner>,
    scope: Arc<OriginalScope>,
    frame: Frame,
    cancel: CancellationToken,
}
impl Context {
    async fn capture(app: &AppCore, frame: Frame) -> AppResult<Self> {
        if frame.version != 1 {
            return Err(invalid("invalid_request"));
        }
        let cancel = CancellationToken::new();
        let scope = Arc::new(
            OriginalScope::capture_expected(
                app,
                &cancel,
                Duration::from_secs(300),
                &frame.profile_id,
                &frame.notification_scope_id,
            )
            .await?,
        );
        Ok(Self {
            core: Arc::downgrade(&app.inner),
            scope,
            frame,
            cancel,
        })
    }
    fn app(&self) -> AppResult<AppCore> {
        self.scope.check_current()?;
        Ok(AppCore {
            inner: self
                .core
                .upgrade()
                .ok_or_else(|| invalid("context_changed"))?,
        })
    }
    async fn hosts(&self) -> AppResult<Vec<HostRef>> {
        let app = self.app()?;
        self.scope
            .with_resources(&app, |_, u, _| {
                let w = u.working();
                let choices = w
                    .native_association_host_choices()
                    .ok_or_else(|| invalid("unsupported_host"))?;
                choices
                    .into_iter()
                    .map(|(id, revision, name)| {
                        let host = w.host(id).ok_or_else(|| invalid("context_changed"))?;
                        Ok(HostRef {
                            id: id.to_string(),
                            revision: revision.to_string(),
                            name,
                            address: host.address.clone(),
                        })
                    })
                    .collect()
            })
            .await
    }
    async fn validate_host(&self, host: &HostRef) -> AppResult<()> {
        if self.hosts().await?.iter().any(|h| {
            h.id == host.id
                && h.revision == host.revision
                && h.name == host.name
                && h.address == host.address
        }) {
            Ok(())
        } else {
            Err(invalid("context_changed"))
        }
    }
    fn public_frame(&self, value: &mut Value) {
        value["version"] = json!(1);
        value["profileId"] = json!(self.frame.profile_id);
        value["notificationScopeId"] = json!(self.frame.notification_scope_id);
    }
}

/// Frozen Native preflight, retaining weak original identity/session only.
/// It currently owns no network carrier, task, process or remote mutation.
pub struct AgentSetupOperation {
    context: Context,
    host: HostRef,
    plan: Value,
    cancelled: AtomicBool,
}
impl std::fmt::Debug for AgentSetupOperation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("AgentSetupOperation([redacted])")
    }
}
impl Drop for AgentSetupOperation {
    fn drop(&mut self) {
        self.cancel();
    }
}
impl AgentSetupOperation {
    pub async fn review_json(&self) -> AppResult<String> {
        if self.cancelled.load(Ordering::SeqCst) {
            return Err(invalid("cancelled"));
        }
        self.context.validate_host(&self.host).await?;
        serde_json::to_string(&self.plan).map_err(|_| invalid("invalid_plan"))
    }
    /// Closed until a genuine independent public release catalog is admitted.
    /// No caller-approved boolean, adjacent candidate key or file is a grant.
    pub async fn install_software_json(&self) -> AppResult<String> {
        self.review_json().await?;
        Err(invalid("release_unavailable"))
    }
    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::SeqCst);
        self.context.cancel.cancel();
    }
    pub async fn joined_json(&self) -> AppResult<String> {
        // This exact operation has not acquired a carrier or started a task.
        // Future mutating stages must replace this with their actual join.
        Ok(
            json!({"version":1,"kind":"agentSetupCleanup","closedObserved":true,
            "remoteMutationAttempted":false})
            .to_string(),
        )
    }
}

fn profiles() -> Value {
    json!([{"id":PROFILE,"requiresCollection":true,"pairingPurpose":"metrics",
    "available":true}])
}
fn plan(id: Uuid, host: &HostRef, destination: Option<&str>) -> Value {
    json!({"kind":"agentSetupPlan","id":id,"host":host,"monitoringProfileId":PROFILE,
        "destinationRef":destination,"release":null,"canProceed":false,
        "requiresInstall":false,"requiresPairing":false,"requiresCollection":true,
        "certificateLifetimeSeconds":0,"actions":[],"blocker":"releaseUnavailable",
        "blockers":["releaseUnavailable","destinationUnavailable"],
        "observations":{"software":"notObserved","node":"notObserved",
            "collectorRuntime":"notObserved","graphData":"notObserved"},
        "remoteMutationAttempted":false})
}
impl AppCore {
    pub async fn agent_setup_context_json(&self, raw: &str) -> AppResult<String> {
        let context = Context::capture(self, parse(raw)?).await?;
        let hosts = context.hosts().await?;
        let mut value = json!({"kind":"agentSetupContext","hosts":hosts,"profiles":profiles(),
            "releases":[],"destinations":[],"releaseState":"unavailable",
            "destinationState":"unconfigured"});
        context.public_frame(&mut value);
        context.scope.check_current()?;
        Ok(value.to_string())
    }
    pub async fn agent_setup_prepare_json(&self, raw: &str) -> AppResult<AgentSetupOperation> {
        let request: Request = parse(raw)?;
        let id = host_id(&request.host_id)?;
        let revision = revision(&request.expected_host_revision)?;
        if request.monitoring_profile_id != PROFILE {
            return Err(invalid("unsupported_profile"));
        }
        // There is no independently approved receiver registry yet. A query
        // source UUID supplied by the UI cannot grant receiver access.
        if request.destination_ref.is_some() {
            return Err(invalid("destination_unavailable"));
        }
        let context = Context::capture(
            self,
            Frame {
                version: request.version,
                profile_id: request.profile_id,
                notification_scope_id: request.notification_scope_id,
            },
        )
        .await?;
        let host = context
            .hosts()
            .await?
            .into_iter()
            .find(|h| h.id == id.to_string() && h.revision == revision.to_string())
            .ok_or_else(|| invalid("unsupported_host"))?;
        let mut plan = plan(Uuid::new_v4(), &host, None);
        context.public_frame(&mut plan);
        context.scope.check_current()?;
        Ok(AgentSetupOperation {
            context,
            host,
            plan,
            cancelled: AtomicBool::new(false),
        })
    }
}

#[cfg(test)]
#[path = "agent_setup/tests.rs"]
mod tests;
