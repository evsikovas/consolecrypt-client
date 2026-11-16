//! Dedicated dependency-mode transport; never borrow a job connection/session.
use super::*;
use crate::session::{Session, Unlocked};
use cc_automation_core::DependencyHelperConnection;
use tokio_util::sync::CancellationToken;

pub(crate) struct DependencyRunner {
    pub id: String,
    pub revision: u64,
    pub configuration: AutomationRunnerConfig,
    pub stamp: String,
}

/// Caller holds active-session and unlock guards. SQL metadata is read fresh.
pub(crate) async fn capture_runner(
    owner: &Session,
    id: &str,
    revision: u64,
) -> AppResult<DependencyRunner> {
    let catalog = owner
        .storage()?
        .setting_get(RUNNERS)
        .await?
        .unwrap_or_else(|| serde_json::json!({"version":1,"records":{}}));
    decode_dependency_runner(catalog, id, revision)
}
/// Same pure decoder for fresh SQL reads inside publication's transaction.
/// No recursive storage queue, credentials, connection or network access.
pub(crate) fn decode_dependency_runner(
    raw: serde_json::Value,
    id: &str,
    revision: u64,
) -> AppResult<DependencyRunner> {
    if serde_json::to_vec(&raw).map_err(AppError::internal)?.len() > 3 * 1024 * 1024 {
        return Err(AppError::invalid(
            "automation_dependency",
            "dependency_context_budget",
        ));
    }
    let catalog: Catalog = serde_json::from_value(raw)
        .map_err(|_| AppError::invalid("automation_dependency", "dependency_context_changed"))?;
    let row = catalog
        .records
        .get(id)
        .ok_or_else(|| AppError::invalid("automation_dependency", "dependency_context_changed"))?;
    let profile = summary(id, row, catalog.version != 1 || !catalog.extra.is_empty());
    if profile.read_only
        || profile.revision != Some(revision)
        || catalog.records.len() > 32
        || row.as_object().is_none_or(|fields| {
            fields.keys().any(|key| {
                !["version", "name", "revision", "configuration"].contains(&key.as_str())
            })
        })
    {
        return Err(AppError::invalid(
            "automation_dependency",
            "dependency_context_changed",
        ));
    }
    let configuration = profile
        .configuration
        .ok_or_else(|| AppError::invalid("automation_dependency", "dependency_context_changed"))?;
    configuration.validate()?;
    use sha2::{Digest, Sha256};
    let bytes = serde_json::to_vec(row).map_err(AppError::internal)?;
    if bytes.len() > 64 * 1024 {
        return Err(AppError::invalid(
            "automation_dependency",
            "dependency_context_budget",
        ));
    }
    Ok(DependencyRunner {
        id: id.into(),
        revision,
        configuration,
        stamp: hex::encode(Sha256::digest(bytes)),
    })
}

impl AppCore {
    pub(crate) async fn open_private_runner_session(
        &self,
        host_id: &str,
        original: &std::sync::Weak<Unlocked>,
        cancel: &CancellationToken,
    ) -> AppResult<cc_ssh_core::SshSession> {
        self.open_private_runner_session_fenced(host_id, original, cancel, None, || Ok(()))
            .await
    }
    /// Original readonly consumer admission after planning and before dial.
    /// Private Native only; this is no public callback or authority surface.
    pub(crate) async fn open_private_runner_session_fenced<F>(
        &self,
        host_id: &str,
        original: &std::sync::Weak<Unlocked>,
        cancel: &CancellationToken,
        deadline: Option<std::time::Instant>,
        admission: F,
    ) -> AppResult<cc_ssh_core::SshSession>
    where
        F: FnOnce() -> AppResult<()> + Send,
    {
        let mut prepared = if let Some(deadline) = deadline {
            tokio::select! {biased;
                _=cancel.cancelled()=>return Err(AppError::Cancelled),
                _=tokio::time::sleep_until(tokio::time::Instant::from_std(deadline))=>
                    return Err(AppError::invalid("automation_dependency","dependency_open_timeout")),
                value=self.sharing_prepare_connection(host_id).boxed()=>value?,
            }
        } else {
            self.sharing_prepare_connection(host_id).boxed().await?
        };
        let expected = original.upgrade().ok_or(AppError::VaultLocked)?;
        if !Arc::ptr_eq(&expected, &prepared.unlocked) || cancel.is_cancelled() {
            return Err(AppError::VaultLocked);
        }
        if prepared.plan.backend != SshBackend::Native {
            return Err(AppError::invalid(
                "automation_dependency",
                "dependency_route_unsupported",
            ));
        }
        for hop in prepared.plan.all_hops() {
            if crate::inventory::host_of(&expected, hop.host_id)?.backend != SshBackend::Native {
                return Err(AppError::invalid(
                    "automation_dependency",
                    "dependency_route_unsupported",
                ));
            }
        }
        drop(expected);
        prepared.plan.forwards.clear();
        prepared.plan.agent_forwarding = false;
        // NEW owned non-Clone session, consumed immediately by typed
        // factory. Never RunnerConnection.remote or shared pool.
        let deadline =
            deadline.unwrap_or_else(|| std::time::Instant::now() + Duration::from_secs(120));
        if std::time::Instant::now() >= deadline || cancel.is_cancelled() {
            return Err(AppError::Cancelled);
        }
        admission()?; // EXACT planned endpoint/credentials are immutable below.
        let session = tokio::select! {biased;
            _=cancel.cancelled()=>return Err(AppError::Cancelled),
            _=tokio::time::sleep_until(tokio::time::Instant::from_std(deadline))=>
                return Err(AppError::invalid("automation_dependency","dependency_open_timeout")),
            result=prepared.run(async {
                Ok(prepared.unlocked.ssh.connector.connect(&prepared.plan).boxed().await?)
            }).boxed()=>result?,
        };
        drop(prepared); // no unlock retained over dependency fetch
        if cancel.is_cancelled() || std::time::Instant::now() >= deadline {
            let _ = tokio::time::timeout(Duration::from_secs(2), session.disconnect()).await;
            return Err(AppError::Cancelled);
        }
        Ok(session)
    }
    /// Rust-private typed factory, with no public arbitrary RPC/session input.
    pub(crate) async fn open_dependency_owner(
        &self,
        runner: &DependencyRunner,
        original: &std::sync::Weak<Unlocked>,
        operation: Uuid,
        cancel: &CancellationToken,
    ) -> AppResult<DependencyHelperConnection> {
        let open = async {
            match &runner.configuration {
                AutomationRunnerConfig::Local { launch } => launch
                    .spawn_dependency(operation, cancel)
                    .await
                    .map_err(runner_error),
                AutomationRunnerConfig::ServiceRemote { .. } => Err(AppError::invalid(
                    "automation_runner",
                    "service_runner_purpose_required",
                )),
                AutomationRunnerConfig::Remote { host_id, helper } => {
                    let session = self
                        .open_private_runner_session(host_id, original, cancel)
                        .await?;
                    helper
                        .connect_dependency(session, operation, cancel)
                        .await
                        .map_err(runner_error)
                }
            }
        };
        // Factory consumes cancellation internally, closes its owned resources,
        // then returns. Do not select/drop it: admission remains retained until
        // the dependency-cleanup-v1 factory has finished its bounded close.
        open.await
    }
}
