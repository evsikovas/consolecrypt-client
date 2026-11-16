//! Device-local Runner profiles and explicitly opened private control channels.
//! No automatic installation, registration, target authentication or job start.
pub(crate) const AUTOMATION_IO_CAPACITY: u32 = 4;

use crate::automation_projects::automation_error;
use crate::{AppCore, AppError, AppResult};
use cc_automation_core::{
    EnvironmentProbe, HelperLaunch, HelperPeer, HelperProcess, JobScopeRegistry, RemoteHelperConfig,
};
use cc_models::host::SshBackend;
use cc_ssh_core::SshSession;
use futures::FutureExt;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
    time::Duration,
};
use uuid::Uuid;

pub(crate) const RUNNERS: &str = "automation.runners.v1";
mod cleanup;
pub(crate) mod dependency;
pub(crate) mod owned_setup;
mod service_results;
mod validation;
mod windows_prerequisites;

// SSH login names may be aliases/realm-qualified names. The scanner reports
// the effective passwd identity, so compare consecutive scanner identities,
// never a requested SSH principal to pwd.getpwuid().pw_name.
fn prerequisite_identity<'a>(
    report: &'a Value,
    command: &str,
    expected: Option<&str>,
) -> AppResult<&'a str> {
    if report["prerequisite_command"].as_str() != Some(command) {
        return Err(AppError::invalid(
            "automation_runner",
            "prerequisites_changed",
        ));
    }
    if report["non_root"] != json!(true) {
        return Err(AppError::invalid("automation_runner", "non_root_required"));
    }
    let user = report["user"]
        .as_str()
        .filter(|user| !user.is_empty() && user.len() <= 128 && !user.chars().any(char::is_control))
        .ok_or_else(|| AppError::invalid("automation_runner", "ssh_account_changed"))?;
    if expected.is_some_and(|expected| expected != user) {
        return Err(AppError::invalid(
            "automation_runner",
            "ssh_account_changed",
        ));
    }
    Ok(user)
}

struct SetupOperation {
    cancel: tokio_util::sync::CancellationToken,
    started: bool,
    expires: std::time::Instant,
    // Cancellation-only weak projection. Not a second authority/holder map.
    owned: Option<std::sync::Weak<dyn owned_setup::OwnedSetupCancellation>>,
}

/// Setup never borrows a user's terminal/pool session. Dropping its operation
/// closes the dedicated SSH route, delivering EOF to the remote installer.
struct SetupSshGuard(Option<Arc<SshSession>>);
impl Drop for SetupSshGuard {
    fn drop(&mut self) {
        let Some(remote) = self.0.take() else {
            return;
        };
        tokio::spawn(async move {
            let _ = remote.disconnect().await;
        });
    }
}

struct SetupRegistration<'a> {
    runtime: &'a AutomationRuntime,
    id: Uuid,
}
impl Drop for SetupRegistration<'_> {
    fn drop(&mut self) {
        self.runtime
            .setup_operations
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(&self.id);
    }
}

fn begin_setup<'a>(
    runtime: &'a AutomationRuntime,
    operation_id: &str,
) -> AppResult<(SetupRegistration<'a>, tokio_util::sync::CancellationToken)> {
    let id = Uuid::parse_str(operation_id)
        .map_err(|_| AppError::invalid("automation_runner", "invalid setup identifier"))?;
    let cancel = {
        let mut operations = runtime
            .setup_operations
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        operations.retain(|_, op| {
            op.owned.is_some() || op.started || op.expires > std::time::Instant::now()
        });
        if let Some(op) = operations.get(&id) {
            if op.owned.is_some() {
                return Err(AppError::invalid(
                    "automation_runner",
                    "owned operation requires issued handle",
                ));
            }
            if !op.started {
                operations.remove(&id);
                return Err(AppError::Cancelled);
            }
            return Err(AppError::invalid(
                "automation_runner",
                "setup identifier already active",
            ));
        }
        if operations.len() >= 16 {
            return Err(AppError::invalid(
                "automation_runner",
                "setup limit reached",
            ));
        }
        let cancel = tokio_util::sync::CancellationToken::new();
        operations.insert(
            id,
            SetupOperation {
                cancel: cancel.clone(),
                started: true,
                expires: std::time::Instant::now() + Duration::from_secs(60),
                owned: None,
            },
        );
        cancel
    };
    Ok((SetupRegistration { runtime, id }, cancel))
}

fn runner_error(error: cc_automation_core::AutomationError) -> AppError {
    match error {
        cc_automation_core::AutomationError::Io(error)
            if error.kind() == std::io::ErrorKind::NotFound =>
        {
            AppError::invalid("automation_runner", "helper_missing")
        }
        other => automation_error(other),
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum AutomationRunnerConfig {
    Local {
        launch: HelperLaunch,
    },
    Remote {
        host_id: String,
        helper: RemoteHelperConfig,
    },
    ServiceRemote {
        host_id: String,
        identity_id: String,
    },
}

impl AutomationRunnerConfig {
    pub(crate) fn service_remote(&self) -> AppResult<(&str, &str)> {
        self.validate()?;
        match self {
            Self::ServiceRemote {
                host_id,
                identity_id,
            } => Ok((host_id.as_str(), identity_id.as_str())),
            _ => Err(AppError::invalid(
                "service_owner",
                "service_runner_required",
            )),
        }
    }
    pub(crate) fn validate(&self) -> AppResult<()> {
        match self {
            Self::Local { launch } => {
                launch.command().map_err(automation_error)?;
            }
            Self::Remote { host_id, helper } => {
                crate::dto::parse_id("host_id", host_id)?;
                helper.command().map_err(automation_error)?;
            }
            Self::ServiceRemote {
                host_id,
                identity_id,
            } => {
                crate::dto::parse_id("host_id", host_id)?;
                cc_automation_core::ServiceRemoteConfig {
                    identity_id: identity_id.clone(),
                }
                .command()
                .map_err(automation_error)?;
            }
        }
        Ok(())
    }
}

#[derive(Serialize, Deserialize)]
struct Catalog {
    version: u32,
    records: BTreeMap<String, Value>,
    #[serde(flatten)]
    extra: BTreeMap<String, Value>,
}
impl Default for Catalog {
    fn default() -> Self {
        Self {
            version: 1,
            records: BTreeMap::new(),
            extra: BTreeMap::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct AutomationRunnerProfile {
    pub id: String,
    pub name: Option<String>,
    pub revision: Option<u64>,
    pub configuration: Option<AutomationRunnerConfig>,
    pub read_only: bool,
}

/// Read-only same catalog parser for the original service factory SQL fence.
/// Unknown rows remain untouched and cannot become writable service authority.
pub(crate) fn stored_runner(
    tx: &cc_storage_core::Tx<'_>,
    id: &str,
) -> AppResult<AutomationRunnerProfile> {
    let parsed = Uuid::parse_str(id)
        .map_err(|_| AppError::invalid("automation_runner", "unsupported Runner profile"))?;
    if parsed.is_nil() || parsed.to_string() != id {
        return Err(AppError::invalid(
            "automation_runner",
            "unsupported Runner profile",
        ));
    }
    let catalog: Catalog = tx.setting_get(RUNNERS)?.unwrap_or_default();
    if catalog.version != 1 || !catalog.extra.is_empty() {
        return Err(AppError::invalid(
            "automation_runner",
            "unsupported Runner profile",
        ));
    }
    let row = catalog
        .records
        .get(id)
        .ok_or_else(|| AppError::not_found("automation Runner", id))?;
    let closed = row.as_object().is_some_and(|value| {
        value
            .keys()
            .all(|key| ["version", "revision", "name", "configuration"].contains(&key.as_str()))
    });
    let profile = summary(id, row, !closed);
    if profile.read_only {
        return Err(AppError::invalid(
            "automation_runner",
            "unsupported Runner profile",
        ));
    }
    profile
        .configuration
        .as_ref()
        .ok_or_else(|| AppError::invalid("automation_runner", "unsupported Runner profile"))?
        .validate()?;
    Ok(profile)
}

fn summary(id: &str, row: &Value, read_only: bool) -> AutomationRunnerProfile {
    let configuration = row
        .get("configuration")
        .cloned()
        .and_then(|value| serde_json::from_value(value).ok());
    let revision = row.get("revision").and_then(Value::as_u64);
    let supported = row.get("version").and_then(Value::as_u64) == Some(1)
        && configuration.is_some()
        && revision.is_some_and(|value| value > 0);
    AutomationRunnerProfile {
        id: id.into(),
        name: row.get("name").and_then(Value::as_str).map(str::to_owned),
        revision,
        configuration,
        read_only: read_only || !supported,
    }
}

/// A connection contains no vault resolver or Arc<Unlocked>. Dedicated remote
/// SSH sessions and local helper processes belong to this profile alone.
pub(crate) struct RunnerConnection {
    pub id: String,
    pub revision: u64,
    pub configuration: AutomationRunnerConfig,
    pub peer: Arc<HelperPeer>,
    pub probe: Value,
    pub remote: Option<Arc<SshSession>>,
    local: Option<HelperProcess>,
    pub authorization: tokio::sync::Mutex<Option<cc_automation_core::EnrolledJobGrant>>,
}

const SERVICE_SOURCE_FIELD: &str = "_native_service_source_v1";
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ServiceConnectSource {
    version: u8,
    mutation: u64,
    scope: String,
}
impl ServiceConnectSource {
    fn capture(u: &crate::session::Unlocked) -> AppResult<Self> {
        Ok(Self {
            version: 1,
            mutation: u.working().version(),
            scope: u.notices.scope_id().ok_or_else(|| {
                AppError::invalid("service_owner", "service_source_changed_reconnect")
            })?,
        })
    }
    fn current(&self, u: &crate::session::Unlocked) -> AppResult<()> {
        if self.version != 1
            || self.mutation != u.working().mutation_stamp()
            || u.notices.scope_id().as_deref() != Some(self.scope.as_str())
            || *u.sharing_shutdown.borrow()
        {
            return Err(AppError::invalid(
                "service_owner",
                "service_source_changed_reconnect",
            ));
        }
        Ok(())
    }
}
async fn service_route_original(
    unlocked: &Arc<crate::session::Unlocked>,
    host_id: &str,
) -> AppResult<crate::sharing_bindings::PreparedSharingConnection> {
    let id = crate::dto::parse_id("host_id", host_id)?;
    let mut plan = unlocked.ssh.plan(id).await?;
    if plan.backend != SshBackend::Native {
        return Err(AppError::invalid(
            "service_owner",
            "service_native_route_required",
        ));
    }
    for hop in plan.all_hops() {
        let host = crate::inventory::host_of(unlocked, hop.host_id)?;
        if host.backend != SshBackend::Native
            || crate::sharing_bindings::host_sharing_binding(&host)?.is_some()
        {
            return Err(AppError::invalid(
                "service_owner",
                "service_shared_route_unsupported",
            ));
        }
    }
    plan.forwards.clear();
    plan.agent_forwarding = false;
    Ok(crate::sharing_bindings::PreparedSharingConnection {
        unlocked: unlocked.clone(),
        plan,
    })
}
fn public_probe(value: &Value) -> Value {
    let mut value = value.clone();
    if let Some(object) = value.as_object_mut() {
        object.remove(SERVICE_SOURCE_FIELD);
    }
    value
}
impl RunnerConnection {
    pub(crate) fn service_source_current(&self, u: &crate::session::Unlocked) -> AppResult<()> {
        if !matches!(
            self.configuration,
            AutomationRunnerConfig::ServiceRemote { .. }
        ) {
            return Err(AppError::invalid(
                "service_owner",
                "service_runner_required",
            ));
        }
        let captured: ServiceConnectSource =
            serde_json::from_value(self.probe.get(SERVICE_SOURCE_FIELD).cloned().ok_or_else(
                || AppError::invalid("service_owner", "service_source_changed_reconnect"),
            )?)
            .map_err(|_| AppError::invalid("service_owner", "service_source_changed_reconnect"))?;
        captured.current(u)
    }
    pub async fn close(&self) {
        if let Some(local) = &self.local {
            local.close().await;
        } else {
            self.peer.close().await;
        }
        if let Some(remote) = &self.remote {
            let _ = remote.disconnect().await;
        }
    }
}

pub(crate) const PREPARE_LIMIT: u32 = 2;

pub(crate) struct AutomationRuntime {
    pub dependencies: Arc<crate::automation_projects::dependencies::DependencyRegistry>,
    pub vault_validation: Arc<crate::automation_projects::vault::VaultValidationOperations>,
    pub scopes: Arc<JobScopeRegistry>,
    pub connections: Mutex<BTreeMap<String, Arc<RunnerConnection>>>,
    /// Do not hold a vault transition guard during potentially slow networking.
    pub connect_gate: tokio::sync::Mutex<()>,
    setup_operations: Mutex<BTreeMap<Uuid, SetupOperation>>,
    setup_owned_state: std::sync::atomic::AtomicU8,
    cleanup_plans: Mutex<BTreeMap<Uuid, cleanup::PreparedCleanup>>,
    lifecycle: Mutex<(u64, bool)>,
    pub reviews: Mutex<BTreeMap<Uuid, crate::automation_prepare::PreparedReview>>,
    pub ai_reviews: Mutex<BTreeMap<Uuid, crate::ai::AutomationAiPrepared>>,
    pub ai_active: Mutex<BTreeMap<Uuid, tokio_util::sync::CancellationToken>>,
    pub git_imports: Mutex<BTreeMap<String, crate::automation_projects::GitImportActive>>,
    pub git_import_slots: Arc<tokio::sync::Semaphore>,
    pub baseline_slots: Arc<tokio::sync::Semaphore>,
    pub inventory_previews:
        Mutex<BTreeMap<Uuid, Arc<crate::automation_projects::InventoryPreview>>>,
    pub inventory_slots: Arc<tokio::sync::Semaphore>,
    pub prepare_slots: Arc<tokio::sync::Semaphore>,
    pub review_slots: Arc<tokio::sync::Semaphore>,
    pub hop_slots: Arc<tokio::sync::Semaphore>,
    pub io_slots: Arc<tokio::sync::Semaphore>,
    pub coordinator: Arc<cc_automation_core::JobCoordinator>,
    pub jobs_initialized: tokio::sync::Mutex<bool>,
    pub stopped: tokio_util::sync::CancellationToken,
    pub live_jobs: Mutex<BTreeMap<Uuid, (tokio_util::sync::CancellationToken, bool)>>,
    pub admission_paused: std::sync::atomic::AtomicBool,
}

impl Default for AutomationRuntime {
    fn default() -> Self {
        Self {
            dependencies: Arc::default(),
            vault_validation: Arc::default(),
            scopes: Arc::default(),
            connections: Mutex::default(),
            connect_gate: tokio::sync::Mutex::default(),
            setup_operations: Mutex::default(),
            setup_owned_state: std::sync::atomic::AtomicU8::new(1),
            cleanup_plans: Mutex::default(),
            lifecycle: Mutex::new((0, true)),
            reviews: Mutex::default(),
            ai_reviews: Mutex::default(),
            ai_active: Mutex::default(),
            git_imports: Mutex::default(),
            git_import_slots: Arc::new(tokio::sync::Semaphore::new(2)),
            baseline_slots: Arc::new(tokio::sync::Semaphore::new(1)),
            inventory_previews: Mutex::default(),
            inventory_slots: Arc::new(tokio::sync::Semaphore::new(4)),
            prepare_slots: Arc::new(tokio::sync::Semaphore::new(PREPARE_LIMIT as usize)),
            review_slots: Arc::new(tokio::sync::Semaphore::new(4)),
            hop_slots: Arc::new(tokio::sync::Semaphore::new(128)),
            io_slots: Arc::new(tokio::sync::Semaphore::new(AUTOMATION_IO_CAPACITY as usize)),
            coordinator: Arc::default(),
            jobs_initialized: tokio::sync::Mutex::new(false),
            stopped: Default::default(),
            live_jobs: Mutex::default(),
            admission_paused: Default::default(),
        }
    }
}

impl AutomationRuntime {
    fn clear_inventory_previews(&self) {
        let entries = std::mem::take(
            &mut *self
                .inventory_previews
                .lock()
                .unwrap_or_else(|e| e.into_inner()),
        );
        for entry in entries.values() {
            entry.cancel.cancel();
        }
    }
    pub fn unlocked_generation(&self) -> AppResult<u64> {
        let state = self.lifecycle.lock().unwrap_or_else(|e| e.into_inner());
        if state.1 {
            Err(AppError::VaultLocked)
        } else {
            Ok(state.0)
        }
    }
    pub fn vault_state(&self, locked: bool) {
        if locked {
            self.cancel_owned_setup(false);
            self.dependencies.close();
            self.vault_validation.cancel_on_lock();
            self.cleanup_plans
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .clear();
            self.clear_inventory_previews();
            for import in self
                .git_imports
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .values()
            {
                import.cancel.cancel();
            }
            for cancel in self
                .ai_active
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .values()
            {
                cancel.cancel();
            }
            self.scopes.vault_locked();
            for (cancel, cancel_on_lock) in self
                .live_jobs
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .values()
            {
                if *cancel_on_lock {
                    cancel.cancel();
                }
            }
        }
        if !locked {
            self.dependencies.reopen();
            self.vault_validation.reopen();
        }
        let generation = {
            let mut state = self
                .lifecycle
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            state.0 = state.0.saturating_add(1);
            state.1 = locked;
            state.0
        };
        if locked {
            self.ai_reviews
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .clear();
            self.reviews
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .clear();
        }
        if !locked {
            let _ = self.setup_owned_state.compare_exchange(
                1,
                0,
                std::sync::atomic::Ordering::AcqRel,
                std::sync::atomic::Ordering::Acquire,
            );
        }
        let peers: Vec<_> = self
            .connections
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .values()
            .filter(|value| {
                !matches!(
                    value.configuration,
                    AutomationRunnerConfig::ServiceRemote { .. }
                )
            })
            .map(|value| value.peer.clone())
            .collect();
        for peer in peers {
            tokio::spawn(async move {
                let _ = peer
                    .request(
                        "vault_state",
                        json!({"locked":locked,"generation":generation}),
                        Duration::from_secs(5),
                    )
                    .await;
            });
        }
    }

    pub async fn close(&self) {
        // Synchronously wipe registered idle bodies BEFORE any worker join.
        let owned_joined = self.cancel_owned_setup(true);
        self.dependencies.close_and_wait().await;
        for joined in owned_joined {
            joined.cancelled().await;
        }
        self.vault_validation.close_and_wait().await;
        self.stopped.cancel();
        self.cleanup_plans
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clear();
        self.clear_inventory_previews();
        for import in self
            .git_imports
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .values()
        {
            import.cancel.cancel();
        }
        for cancel in self
            .ai_active
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .values()
        {
            cancel.cancel();
        }
        self.ai_reviews
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clear();
        self.reviews
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clear();
        self.scopes.cancel_all();
        let connections = std::mem::take(
            &mut *self
                .connections
                .lock()
                .unwrap_or_else(|error| error.into_inner()),
        );
        let mut tasks = tokio::task::JoinSet::new();
        for (_, connection) in connections {
            tasks.spawn(async move {
                connection.close().await;
            });
        }
        while tasks.join_next().await.is_some() {}
    }
}

impl AppCore {
    async fn automation_setup_on_target(
        &self,
        target: &cc_automation_core::SetupTarget,
        install: bool,
        expected: &Arc<crate::session::Unlocked>,
    ) -> AppResult<Value> {
        let cc_automation_core::SetupTarget::Remote { host_id } = target else {
            return cc_automation_core::setup_environment(target, install)
                .await
                .map_err(runner_error);
        };
        crate::dto::parse_id("host_id", host_id)?;
        let mut prepared = self.sharing_prepare_connection(host_id).boxed().await?;
        if !Arc::ptr_eq(expected, &prepared.unlocked) {
            return Err(AppError::VaultLocked);
        }
        if prepared.plan.backend != SshBackend::Native
            || prepared.plan.all_hops().into_iter().any(|hop| {
                crate::inventory::host_of(expected, hop.host_id)
                    .map_or(true, |host| host.backend != SshBackend::Native)
            })
        {
            return Err(AppError::invalid(
                "automation_runner",
                "Runner needs a native SSH route",
            ));
        }
        prepared.plan.forwards.clear();
        prepared.plan.agent_forwarding = false;
        prepared
            .run(async {
                let remote = SetupSshGuard(Some(Arc::new(
                    expected
                        .ssh
                        .connector
                        .connect(&prepared.plan)
                        .boxed()
                        .await?,
                )));
                cc_automation_core::setup_remote_environment(remote.0.as_ref().unwrap(), install)
                    .await
                    .map_err(runner_error)
            })
            .boxed()
            .await
    }

    /// Explicit button action: open the exact reviewed system-package command
    /// in a NEW visible terminal. sudo authentication remains in that PTY.
    pub async fn automation_runner_prerequisite_terminal(
        &self,
        operation_id: String,
        target: cc_automation_core::SetupTarget,
        reviewed_command: String,
    ) -> AppResult<String> {
        use cc_automation_core::SetupTarget;
        use cc_terminal_core::{
            TerminalSize, TerminalTarget,
            local::{LocalCommand, open_command},
        };
        let script = cc_automation_core::prerequisite_script(&reviewed_command)
            .map_err(|_| AppError::invalid("automation_runner", "invalid_prerequisite_command"))?;
        if !matches!(target, SetupTarget::Remote { .. }) {
            target.command().map_err(automation_error)?;
        }
        let (owner, unlocked) = self.unlocked().await?;
        let (_registration, cancel) = begin_setup(&owner.automation, &operation_id)?;
        let mut shutdown = unlocked.sharing_shutdown.subscribe();
        let report = tokio::select! { biased;
            _ = cancel.cancelled() => return Err(AppError::Cancelled),
            _ = shutdown.changed() => return Err(AppError::VaultLocked),
            result = self.automation_setup_on_target(&target, false, &unlocked) => result?,
        };
        let scanned_user = prerequisite_identity(&report, &reviewed_command, None)?;
        if let SetupTarget::Remote { host_id } = &target {
            let mut prepared = self.sharing_prepare_connection(host_id).boxed().await?;
            if !Arc::ptr_eq(&unlocked, &prepared.unlocked)
                || prepared.plan.backend != SshBackend::Native
            {
                return Err(AppError::invalid(
                    "automation_runner",
                    "ssh_environment_changed",
                ));
            }
            if prepared.plan.all_hops().into_iter().any(|hop| {
                crate::inventory::host_of(&unlocked, hop.host_id)
                    .map_or(true, |host| host.backend != SshBackend::Native)
            }) {
                return Err(AppError::invalid(
                    "automation_runner",
                    "ssh_route_unsupported",
                ));
            }
            prepared.plan.forwards.clear();
            prepared.plan.agent_forwarding = false;
            let command = format!("exec /bin/sh -c '{}'", script.replace('\'', "'\"'\"'"));
            let operation = prepared.run(async {
                let mut remote = SetupSshGuard(Some(Arc::new(
                    unlocked
                        .ssh
                        .connector
                        .connect(&prepared.plan)
                        .boxed()
                        .await?,
                )));
                let actual =
                    cc_automation_core::setup_remote_environment(remote.0.as_ref().unwrap(), false)
                        .await
                        .map_err(runner_error)?;
                prerequisite_identity(&actual, &reviewed_command, Some(scanned_user))?;
                let channel = remote
                    .0
                    .as_ref()
                    .unwrap()
                    .open_command_pty(
                        cc_ssh_core::PtyRequest {
                            cols: 100,
                            rows: 30,
                            ..Default::default()
                        },
                        &command,
                    )
                    .await?;
                let _profile = self.hold_session(&owner).await?;
                let _unlock = owner.hold_unlocked(&unlocked).await?;
                if cancel.is_cancelled() {
                    return Err(AppError::Cancelled);
                }
                let session = remote.0.as_ref().unwrap().clone();
                let id = unlocked
                    .ssh
                    .terminals
                    .open_channel(
                        TerminalTarget::Ssh {
                            host_id: crate::dto::parse_id("host_id", host_id)?,
                        },
                        format!("Runner · {}", prepared.plan.name),
                        TerminalSize {
                            cols: 100,
                            rows: 30,
                        },
                        cc_terminal_core::OpenedShell {
                            channel,
                            session: Some(session),
                            lifetime: None,
                        },
                    )
                    .await?;
                remote.0.take(); // Ownership transferred to the terminal manager.
                unlocked.ssh.terminals.suppress_command_tracking(id)?;
                if cancel.is_cancelled() {
                    let _ = unlocked.ssh.terminals.remove(id).await;
                    return Err(AppError::Cancelled);
                }
                Ok(id.to_string())
            });
            return tokio::select! { biased;
                _ = cancel.cancelled() => Err(AppError::Cancelled),
                _ = shutdown.changed() => Err(AppError::VaultLocked),
                result = operation => result,
            };
        }
        let (executable, arguments, distribution) = match target {
            SetupTarget::Wsl { distribution, user } => {
                let user = user.filter(|name| name != "root").ok_or_else(|| {
                    AppError::invalid("automation_runner", "select a non-root user")
                })?;
                if report["wsl2"] != json!(true) || report["user"].as_str() != Some(user.as_str()) {
                    return Err(AppError::invalid(
                        "automation_runner",
                        "selected WSL environment changed",
                    ));
                }
                let system = std::path::PathBuf::from(
                    std::env::var_os("SystemRoot").unwrap_or_else(|| "C:\\Windows".into()),
                );
                (
                    system.join("System32/wsl.exe"),
                    vec![
                        "--distribution".into(),
                        distribution.clone(),
                        "--user".into(),
                        user,
                        "--cd".into(),
                        "~".into(),
                        "--exec".into(),
                        "/bin/sh".into(),
                        "-c".into(),
                        script,
                    ],
                    Some(distribution),
                )
            }
            SetupTarget::Native if cfg!(target_os = "linux") => (
                std::path::PathBuf::from("/bin/sh"),
                vec!["-c".into(), script],
                None,
            ),
            _ => {
                return Err(AppError::invalid(
                    "automation_runner",
                    "prerequisite_terminal_unavailable",
                ));
            }
        };
        let directory = executable.parent().unwrap().to_path_buf();
        let title = format!("Runner · {}", distribution.as_deref().unwrap_or("Linux"));
        let _profile = self.hold_session(&owner).await?;
        let _unlock = owner.hold_unlocked(&unlocked).await?;
        if cancel.is_cancelled() {
            return Err(AppError::Cancelled);
        }
        let opened = open_command(
            LocalCommand {
                executable,
                arguments,
                directory,
                environment: crate::kubernetes_cli::clean_environment(),
            },
            TerminalSize {
                cols: 100,
                rows: 30,
            },
        )
        .await?;
        if cancel.is_cancelled() {
            if let Some(lifetime) = &opened.lifetime {
                lifetime.close();
            }
            return Err(AppError::Cancelled);
        }
        let id = unlocked
            .ssh
            .terminals
            .open_channel(
                TerminalTarget::Local {
                    profile_id: "runner-prerequisites".into(),
                    shell: "sh".into(),
                    display_name: title.clone(),
                    distribution,
                },
                title,
                TerminalSize {
                    cols: 100,
                    rows: 30,
                },
                opened,
            )
            .await?;
        Ok(id.to_string())
    }
    /// Diagnostics are read-only. Installation is a distinct, explicit UI
    /// action, limited to a new private environment of a non-root Linux user.
    pub async fn automation_runner_setup(
        &self,
        operation_id: String,
        target: cc_automation_core::SetupTarget,
        install: bool,
    ) -> AppResult<Value> {
        let (owner, unlocked) = self.unlocked().await?;
        let (_registration, cancel) = begin_setup(&owner.automation, &operation_id)?;
        let mut shutdown = unlocked.sharing_shutdown.subscribe();
        let _serial = tokio::select! {
            biased;
            _ = cancel.cancelled() => return Err(AppError::Cancelled),
            _ = shutdown.changed() => return Err(AppError::VaultLocked),
            guard = owner.automation.connect_gate.lock() => guard,
        };
        {
            let _profile = self.hold_session(&owner).await?;
            let _unlock = owner.hold_unlocked(&unlocked).await?;
        }
        let result = tokio::select! {
            biased;
            _ = cancel.cancelled() => return Err(AppError::Cancelled),
            _ = shutdown.changed() => return Err(AppError::VaultLocked),
            result = self.automation_setup_on_target(&target, install, &unlocked) => result?,
        };
        let _profile = self.hold_session(&owner).await?;
        let _unlock = owner.hold_unlocked(&unlocked).await?;
        Ok(result)
    }

    pub async fn automation_runner_cancel_setup(&self, operation_id: String) -> AppResult<()> {
        let (owner, unlocked) = self.unlocked().await?;
        let id = Uuid::parse_str(&operation_id)
            .map_err(|_| AppError::invalid("automation_runner", "invalid setup identifier"))?;
        let _profile = self.hold_session(&owner).await?;
        let _unlock = owner.hold_unlocked(&unlocked).await?;
        let mut operations = owner
            .automation
            .setup_operations
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        operations.retain(|_, op| {
            op.owned.is_some() || op.started || op.expires > std::time::Instant::now()
        });
        if let Some(op) = operations.get(&id) {
            if op.owned.is_some() {
                return Err(AppError::invalid(
                    "automation_runner",
                    "owned operation requires issued handle",
                ));
            }
            op.cancel.cancel();
        } else if operations.len() < 16 {
            // A cancellation can reach FRB before its corresponding start.
            // Retain a short-lived tombstone; never cancel a different setup.
            let cancel = tokio_util::sync::CancellationToken::new();
            cancel.cancel();
            operations.insert(
                id,
                SetupOperation {
                    cancel,
                    started: false,
                    expires: std::time::Instant::now() + Duration::from_secs(60),
                    owned: None,
                },
            );
        }
        Ok(())
    }
    pub async fn automation_runner_disconnect(&self, id: String) -> AppResult<()> {
        let (owner, unlocked) = self.unlocked().await?;
        self.automation_initialize_jobs(&owner).await?;
        let _serial = owner.automation.connect_gate.lock().await;
        let connection = {
            let _profile = self.hold_session(&owner).await?;
            let _unlock = owner.hold_unlocked(&unlocked).await?;
            if owner
                .automation
                .git_imports
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .values()
                .any(|import| import.runner_id == id)
                || owner
                    .automation
                    .inventory_previews
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .values()
                    .any(|entry| entry.runner_id == id)
                || owner.automation.scopes.has_scopes()
                || owner.automation.coordinator.holds_runner(
                    Uuid::parse_str(&id).map_err(|_| {
                        AppError::invalid("automation_runner", "invalid identifier")
                    })?,
                )
            {
                return Err(AppError::invalid(
                    "automation_runner",
                    "wait for active jobs or reconcile unknown results before disconnecting",
                ));
            }
            owner.automation.dependencies.revoke_runner(&id);
            owner.automation.vault_validation.revoke_runner(&id);
            owner
                .automation
                .connections
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .remove(&id)
        };
        if let Some(connection) = connection {
            connection.close().await;
        }
        Ok(())
    }

    pub async fn automation_runner_forget(
        &self,
        id: String,
        expected_revision: u64,
    ) -> AppResult<()> {
        let (owner, unlocked) = self.unlocked().await?;
        let _serial = owner.automation.connect_gate.lock().await;
        let _profile = self.hold_session(&owner).await?;
        let _unlock = owner.hold_unlocked(&unlocked).await?;
        if owner
            .automation
            .connections
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .contains_key(&id)
        {
            return Err(AppError::invalid(
                "automation_runner",
                "disconnect the Runner before removing its profile",
            ));
        }
        let dependency_runner = id.clone();
        owner
            .storage()?
            .write(move |transaction| {
                let mut catalog: Catalog = transaction.setting_get(RUNNERS)?.unwrap_or_default();
                let row = catalog
                    .records
                    .get(&id)
                    .ok_or_else(|| AppError::not_found("automation Runner", &id))?;
                let profile = summary(&id, row, catalog.version != 1);
                if profile.read_only || profile.revision != Some(expected_revision) {
                    return Err(AppError::invalid(
                        "automation_runner",
                        "Runner profile changed or has a newer schema",
                    ));
                }
                catalog.records.remove(&id);
                transaction.setting_set(RUNNERS, &catalog)?;
                Ok::<_, AppError>(())
            })
            .await?;
        owner
            .automation
            .dependencies
            .revoke_runner(&dependency_runner);
        Ok(())
    }

    pub async fn automation_runners(&self) -> AppResult<Value> {
        let (owner, unlocked) = self.unlocked().await?;
        let catalog: Catalog = owner
            .storage()?
            .setting_get(RUNNERS)
            .await?
            .unwrap_or_default();
        let profiles: Vec<_> = catalog
            .records
            .iter()
            .map(|(id, row)| summary(id, row, catalog.version != 1))
            .collect();
        let _profile = self.hold_session(&owner).await?;
        let _unlock = owner.hold_unlocked(&unlocked).await?;
        Ok(
            json!({"schema_version":catalog.version,"read_only":catalog.version != 1,"device_local":true,"runners":profiles}),
        )
    }

    pub async fn automation_runner_create(
        &self,
        name: String,
        configuration: AutomationRunnerConfig,
    ) -> AppResult<AutomationRunnerProfile> {
        configuration.validate()?;
        let name = name.trim();
        if name.is_empty() || name.chars().count() > 120 || name.chars().any(char::is_control) {
            return Err(AppError::invalid("runner_name", "invalid name"));
        }
        let id = Uuid::new_v4().to_string();
        let row = json!({"version":1,"name":name,"revision":1,"configuration":configuration});
        let result = summary(&id, &row, false);
        let (owner, unlocked) = self.unlocked().await?;
        let _serial = owner.automation.connect_gate.lock().await;
        let _profile = self.hold_session(&owner).await?;
        let _unlock = owner.hold_unlocked(&unlocked).await?;
        owner
            .storage()?
            .write(move |transaction| {
                let mut catalog: Catalog = transaction.setting_get(RUNNERS)?.unwrap_or_default();
                if catalog.version != 1 || catalog.records.len() >= 32 {
                    return Err(AppError::invalid(
                        "runner_catalog",
                        "read-only catalog or Runner limit reached",
                    ));
                }
                catalog.records.insert(id, row);
                transaction.setting_set(RUNNERS, &catalog)?;
                Ok::<_, AppError>(())
            })
            .await?;
        Ok(result)
    }

    /// Editing is metadata-only and never reconnects/spawns a process. An open
    /// connection must be explicitly disconnected using the job-aware gate.
    pub async fn automation_runner_update(
        &self,
        id: String,
        expected_revision: u64,
        name: String,
        configuration: AutomationRunnerConfig,
    ) -> AppResult<AutomationRunnerProfile> {
        configuration.validate()?;
        let name = name.trim().to_owned();
        if name.is_empty() || name.chars().count() > 120 || name.chars().any(char::is_control) {
            return Err(AppError::invalid("runner_name", "invalid name"));
        }
        crate::dto::parse_id("runner_id", &id)?;
        let (owner, unlocked) = self.unlocked().await?;
        let _serial = owner.automation.connect_gate.lock().await;
        let _profile = self.hold_session(&owner).await?;
        let _unlock = owner.hold_unlocked(&unlocked).await?;
        if owner
            .automation
            .connections
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .contains_key(&id)
        {
            return Err(AppError::invalid(
                "automation_runner",
                "disconnect the Runner before editing its profile",
            ));
        }
        let dependency_runner = id.clone();
        let result = owner
            .storage()?
            .write(move |tx| {
                let mut catalog: Catalog = tx.setting_get(RUNNERS)?.unwrap_or_default();
                let read_only = catalog.version != 1;
                let row = catalog
                    .records
                    .get_mut(&id)
                    .ok_or_else(|| AppError::not_found("automation Runner", &id))?;
                let previous = summary(&id, row, read_only);
                if previous.read_only || previous.revision != Some(expected_revision) {
                    return Err(AppError::invalid(
                        "automation_runner",
                        "Runner profile changed or has a newer schema",
                    ));
                }
                let next = expected_revision
                    .checked_add(1)
                    .ok_or_else(|| AppError::invalid("automation_runner", "revision exhausted"))?;
                row["name"] = json!(name);
                row["configuration"] =
                    serde_json::to_value(configuration).map_err(AppError::internal)?;
                row["revision"] = json!(next);
                let result = summary(&id, row, false);
                tx.setting_set(RUNNERS, &catalog)?;
                Ok::<_, AppError>(result)
            })
            .await?;
        owner
            .automation
            .dependencies
            .revoke_runner(&dependency_runner);
        Ok(result)
    }

    pub async fn automation_runner_connect(&self, id: String) -> AppResult<Value> {
        self.automation_runner_connect_inner(id).boxed().await
    }

    async fn automation_runner_connect_inner(&self, id: String) -> AppResult<Value> {
        let (owner, unlocked) = self.unlocked().await?;
        let (_, probe) = self
            .automation_runner_connect_original_inner(id.clone(), owner, unlocked, None)
            .boxed()
            .await?;
        Ok(json!({"runner_id":id,"probe":public_probe(&probe)}))
    }
    pub(crate) async fn automation_service_connect_original(
        &self,
        owner: Arc<crate::session::Session>,
        weak: std::sync::Weak<crate::session::Unlocked>,
        runner_id: &str,
        expected_revision: u64,
        expected_configuration: &AutomationRunnerConfig,
    ) -> AppResult<Arc<RunnerConnection>> {
        expected_configuration.service_remote()?;
        let unlocked = weak.upgrade().ok_or(AppError::VaultLocked)?;
        let (connection, _) = self
            .automation_runner_connect_original_inner(
                runner_id.to_string(),
                owner,
                unlocked,
                Some((expected_revision, expected_configuration.clone())),
            )
            .boxed()
            .await?;
        Ok(connection)
    }
    async fn automation_runner_connect_original_inner(
        &self,
        id: String,
        owner: Arc<crate::session::Session>,
        unlocked: Arc<crate::session::Unlocked>,
        expected: Option<(u64, AutomationRunnerConfig)>,
    ) -> AppResult<(Arc<RunnerConnection>, Value)> {
        {
            let _profile = self.hold_session(&owner).await?;
            let _unlock = owner.hold_unlocked(&unlocked).await?;
        }
        let _serial = owner.automation.connect_gate.lock().await;
        let catalog: Catalog = owner
            .storage()?
            .setting_get(RUNNERS)
            .await?
            .unwrap_or_default();
        let row = catalog
            .records
            .get(&id)
            .ok_or_else(|| AppError::not_found("automation Runner", &id))?;
        let profile = summary(&id, row, catalog.version != 1);
        if profile.read_only {
            return Err(AppError::invalid(
                "automation_runner",
                "unsupported Runner profile",
            ));
        }
        let configuration = profile
            .configuration
            .ok_or_else(|| AppError::invalid("automation_runner", "missing configuration"))?;
        configuration.validate()?;
        if let Some((revision, captured)) = &expected {
            configuration.service_remote()?;
            if profile.revision != Some(*revision)
                || serde_json::to_value(&configuration).map_err(AppError::internal)?
                    != serde_json::to_value(captured).map_err(AppError::internal)?
            {
                return Err(AppError::invalid(
                    "service_owner",
                    "service_source_changed_reconnect",
                ));
            }
        }
        let existing = owner
            .automation
            .connections
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .get(&id)
            .cloned();
        if let Some(existing) = existing {
            if existing.revision == profile.revision.unwrap_or_default()
                && !existing.peer.is_closed()
            {
                if matches!(configuration, AutomationRunnerConfig::ServiceRemote { .. }) {
                    existing.service_source_current(&unlocked)?;
                    if serde_json::to_value(&configuration).map_err(AppError::internal)?
                        != serde_json::to_value(&existing.configuration)
                            .map_err(AppError::internal)?
                    {
                        return Err(AppError::invalid(
                            "service_owner",
                            "service_source_changed_reconnect",
                        ));
                    }
                }
                let mut shutdown = unlocked.sharing_shutdown.subscribe();
                let raw = tokio::select! {
                    biased;
                    _ = shutdown.changed() => return Err(AppError::VaultLocked),
                    result = existing.peer.request("probe",json!({}),Duration::from_secs(60)) => result.map_err(automation_error)?,
                };
                let service = if let AutomationRunnerConfig::ServiceRemote { identity_id, .. } =
                    &existing.configuration
                {
                    Some(
                        cc_automation_core::ServiceRemoteConfig {
                            identity_id: identity_id.clone(),
                        }
                        .observe(&existing.peer, raw.clone())
                        .await
                        .map_err(automation_error)?,
                    )
                } else {
                    None
                };
                let mut probe =
                    serde_json::to_value(EnvironmentProbe::parse(raw).map_err(automation_error)?)
                        .map_err(AppError::internal)?;
                if let Some(service) = service {
                    probe
                        .as_object_mut()
                        .ok_or_else(|| {
                            AppError::invalid("automation_runner", "invalid service observation")
                        })?
                        .insert(
                            "service_observation".into(),
                            serde_json::to_value(service).map_err(AppError::internal)?,
                        );
                }
                let _profile = self.hold_session(&owner).await?;
                let _unlock = owner.hold_unlocked(&unlocked).await?;
                if matches!(configuration, AutomationRunnerConfig::ServiceRemote { .. }) {
                    existing.service_source_current(&unlocked)?;
                }
                return Ok((existing, probe));
            }
            return Err(AppError::invalid(
                "automation_runner",
                "disconnect the previous Runner channel first",
            ));
        }
        if owner
            .automation
            .connections
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .len()
            >= 16
        {
            return Err(AppError::invalid(
                "automation_runner",
                "connection limit reached",
            ));
        }
        let service_source =
            if matches!(configuration, AutomationRunnerConfig::ServiceRemote { .. }) {
                Some(ServiceConnectSource::capture(&unlocked)?)
            } else {
                None
            };
        let mut shutdown = unlocked.sharing_shutdown.subscribe();
        let connect = async {
            let (peer, local, remote) = match &configuration {
                AutomationRunnerConfig::Local { launch } => {
                    let local = launch
                        .spawn_with_targets(Some(owner.automation.scopes.clone()))
                        .await
                        .map_err(runner_error)?;
                    (local.peer.clone(), Some(local), None)
                }
                AutomationRunnerConfig::Remote { host_id, .. }
                | AutomationRunnerConfig::ServiceRemote { host_id, .. } => {
                    let mut prepared =
                        if matches!(configuration, AutomationRunnerConfig::ServiceRemote { .. }) {
                            service_route_original(&unlocked, host_id).await?
                        } else {
                            self.sharing_prepare_connection(host_id).boxed().await?
                        };
                    if !Arc::ptr_eq(&unlocked, &prepared.unlocked) {
                        return Err(AppError::VaultLocked);
                    }
                    if prepared.plan.backend != SshBackend::Native {
                        return Err(AppError::invalid(
                            "automation_runner",
                            "Runner needs a native SSH route",
                        ));
                    }
                    for hop in prepared.plan.all_hops() {
                        let host = crate::inventory::host_of(&unlocked, hop.host_id)?;
                        if host.backend != SshBackend::Native {
                            return Err(AppError::invalid(
                                "automation_runner",
                                "unsupported jump backend",
                            ));
                        }
                    }
                    prepared.plan.forwards.clear();
                    prepared.plan.agent_forwarding = false;
                    if let Some(stamp) = &service_source {
                        let saved = owner
                            .storage()?
                            .read({
                                let id = id.clone();
                                move |tx| stored_runner(tx, &id)
                            })
                            .await?;
                        if saved.revision != profile.revision
                            || serde_json::to_value(saved.configuration)
                                .map_err(AppError::internal)?
                                != serde_json::to_value(&configuration)
                                    .map_err(AppError::internal)?
                        {
                            return Err(AppError::invalid(
                                "service_owner",
                                "service_source_changed_reconnect",
                            ));
                        }
                        {
                            let _profile = self.hold_session(&owner).await?;
                            let _unlock = owner.hold_unlocked(&unlocked).await?;
                            stamp.current(&unlocked)?;
                        }
                    }
                    let remote = prepared
                        .run(async {
                            Ok(Arc::new(
                                unlocked
                                    .ssh
                                    .connector
                                    .connect(&prepared.plan)
                                    .boxed()
                                    .await?,
                            ))
                        })
                        .boxed()
                        .await?;
                    let peer = match &configuration {
                        AutomationRunnerConfig::Remote { helper, .. } => helper
                            .connect_with_targets(&remote, Some(owner.automation.scopes.clone()))
                            .await
                            .map_err(automation_error)?,
                        AutomationRunnerConfig::ServiceRemote { identity_id, .. } => {
                            cc_automation_core::ServiceRemoteConfig {
                                identity_id: identity_id.clone(),
                            }
                            .connect(&remote)
                            .await
                            .map_err(automation_error)?
                        }
                        _ => {
                            return Err(AppError::invalid(
                                "automation_runner",
                                "unsupported Runner profile",
                            ));
                        }
                    };
                    (peer, None, Some(remote))
                }
            };
            let raw = peer
                .request("probe", json!({}), Duration::from_secs(60))
                .await
                .map_err(automation_error)?;
            let service =
                if let AutomationRunnerConfig::ServiceRemote { identity_id, .. } = &configuration {
                    Some(
                        cc_automation_core::ServiceRemoteConfig {
                            identity_id: identity_id.clone(),
                        }
                        .observe(&peer, raw.clone())
                        .await
                        .map_err(automation_error)?,
                    )
                } else {
                    None
                };
            let mut probe =
                serde_json::to_value(EnvironmentProbe::parse(raw).map_err(automation_error)?)
                    .map_err(AppError::internal)?;
            if let Some(service) = service {
                probe
                    .as_object_mut()
                    .ok_or_else(|| {
                        AppError::invalid("automation_runner", "invalid service observation")
                    })?
                    .insert(
                        "service_observation".into(),
                        serde_json::to_value(service).map_err(AppError::internal)?,
                    );
            }
            if let Some(stamp) = &service_source {
                stamp.current(&unlocked)?;
                probe
                    .as_object_mut()
                    .ok_or_else(|| {
                        AppError::invalid("service_owner", "service_source_changed_reconnect")
                    })?
                    .insert(
                        SERVICE_SOURCE_FIELD.into(),
                        serde_json::to_value(stamp).map_err(AppError::internal)?,
                    );
            }
            let (generation, locked) = *owner
                .automation
                .lifecycle
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            if locked {
                return Err(AppError::VaultLocked);
            }
            if !matches!(configuration, AutomationRunnerConfig::ServiceRemote { .. }) {
                peer.request(
                    "vault_state",
                    json!({"locked":false,"generation":generation}),
                    Duration::from_secs(5),
                )
                .await
                .map_err(automation_error)?;
            }
            Ok::<_, AppError>(Arc::new(RunnerConnection {
                id: id.clone(),
                revision: profile.revision.unwrap_or_default(),
                configuration: configuration.clone(),
                peer,
                probe,
                local,
                remote,
                authorization: tokio::sync::Mutex::new(None),
            }))
        }
        .boxed();
        let connection = tokio::select! {
            biased;
            _ = shutdown.changed() => return Err(AppError::VaultLocked),
            result = connect => result?,
        };
        let _profile = self.hold_session(&owner).await?;
        let _unlock = owner.hold_unlocked(&unlocked).await?;
        if matches!(
            connection.configuration,
            AutomationRunnerConfig::ServiceRemote { .. }
        ) {
            connection.service_source_current(&unlocked)?;
        }
        owner
            .automation
            .connections
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .insert(id.clone(), connection.clone());
        Ok((connection.clone(), connection.probe.clone()))
    }
}

#[cfg(test)]
mod stack_tests {
    use super::*;
    #[test]
    fn prerequisite_identity_is_effective_account_not_requested_ssh_login() {
        let command = "sudo apt-get update && sudo apt-get install --no-install-recommends python3-venv bubblewrap";
        let report = json!({"user":"linux_account","non_root":true,"prerequisite_command":command});
        let effective = prerequisite_identity(&report, command, None).unwrap();
        // A principal such as linux_account@example.org may legitimately map
        // to this passwd name. Only the actual subsequent scanner is compared.
        assert_eq!(
            prerequisite_identity(&report, command, Some(effective)).unwrap(),
            "linux_account"
        );
        let other =
            json!({"user":"different_account","non_root":true,"prerequisite_command":command});
        assert!(prerequisite_identity(&other, command, Some(effective)).is_err());
        for invalid_report in [
            json!({"user":"linux_account","non_root":false,"prerequisite_command":command}),
            json!({"user":"linux_account","non_root":true,"prerequisite_command":null}),
            json!({"user":"","non_root":true,"prerequisite_command":command}),
        ] {
            assert!(prerequisite_identity(&invalid_report, command, Some(effective)).is_err());
        }
    }
    #[tokio::test]
    async fn setup_cancel_before_start_and_exact_pending_operation_never_launch_a_process() {
        let folder = tempfile::tempdir().unwrap();
        let app = AppCore::new(crate::AppConfig::for_tests(
            folder.path().to_string_lossy().into_owned(),
        ))
        .unwrap();
        app.create_local_profile(
            "Setup cancellation".into(),
            "correct horse setup cancellation 42".into(),
        )
        .await
        .unwrap();
        let early = Uuid::new_v4().to_string();
        app.automation_runner_cancel_setup(early.clone())
            .await
            .unwrap();
        assert!(matches!(
            app.automation_runner_setup(early, cc_automation_core::SetupTarget::Native, true)
                .await,
            Err(AppError::Cancelled)
        ));
        let (owner, _) = app.unlocked().await.unwrap();
        let gate = owner.automation.connect_gate.lock().await;
        let id = Uuid::new_v4();
        let pending = tokio::spawn({
            let app = app.clone();
            async move {
                app.automation_runner_setup(
                    id.to_string(),
                    cc_automation_core::SetupTarget::Native,
                    true,
                )
                .await
            }
        });
        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                if owner
                    .automation
                    .setup_operations
                    .lock()
                    .unwrap()
                    .contains_key(&id)
                {
                    break;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        app.automation_runner_cancel_setup(Uuid::new_v4().to_string())
            .await
            .unwrap();
        assert!(!pending.is_finished());
        app.automation_runner_cancel_setup(id.to_string())
            .await
            .unwrap();
        assert!(matches!(
            tokio::time::timeout(Duration::from_secs(5), pending)
                .await
                .unwrap()
                .unwrap(),
            Err(AppError::Cancelled)
        ));
        assert!(
            !owner
                .automation
                .setup_operations
                .lock()
                .unwrap()
                .contains_key(&id)
        );
        drop(gate);
        drop(owner);
        app.shutdown().await.unwrap();
    }
    #[tokio::test]
    async fn connection_state_machine_fits_a_desktop_thread_even_with_multiple_runners() {
        let folder = tempfile::tempdir().unwrap();
        let app = AppCore::new(crate::AppConfig::for_tests(
            folder.path().to_string_lossy().into_owned(),
        ))
        .unwrap();
        let future = app.automation_runner_connect(Uuid::new_v4().to_string());
        let bytes = std::mem::size_of_val(&future);
        assert!(
            bytes <= 64 * 1024,
            "Runner connection future is {bytes} bytes; nested auth operations must stay on the heap"
        );
    }
}

#[cfg(test)]
#[path = "automation_runners/bootstrap_tests.rs"]
mod bootstrap_tests;

#[cfg(test)]
#[path = "automation_runners/production_owner_tests.rs"]
mod production_owner_tests;

#[cfg(test)]
mod service_remote_tests;
