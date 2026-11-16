//! Explicit exact-path review of managed environments, never arbitrary delete.
use super::*;
use crate::session::{Session, Unlocked};
use cc_automation_core::{
    ManagedCleanupApproval, ManagedCleanupCandidate, ManagedCleanupIdentity,
    ManagedCleanupProtection, ManagedCleanupResult, ManagedCleanupScan, SetupTarget,
};
use sha2::{Digest, Sha256};
use std::{collections::BTreeSet, sync::atomic::Ordering, time::Instant};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ScanRequest {
    #[serde(deserialize_with = "strict_uuid")]
    operation_id: Uuid,
    target: SetupTarget,
    selected_python: Option<String>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct PlanRequest {
    #[serde(deserialize_with = "strict_uuid")]
    operation_id: Uuid,
    target: SetupTarget,
    candidate_ids: Vec<String>,
    selected_python: Option<String>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct CommitRequest {
    #[serde(deserialize_with = "strict_uuid")]
    operation_id: Uuid,
    #[serde(deserialize_with = "strict_uuid")]
    plan_token: Uuid,
    selected_python: Option<String>,
}

pub(super) struct PreparedCleanup {
    target: SetupTarget,
    selected_python: Option<String>,
    identity: ManagedCleanupIdentity,
    candidates: Vec<ManagedCleanupCandidate>,
    catalog_digest: String,
    working_version: u64,
    generation: u64,
    expires: Instant,
}
fn invalid(rule: &str) -> AppError {
    AppError::invalid("automation_runner_cleanup", rule)
}
fn error(error: cc_automation_core::AutomationError) -> AppError {
    match error {
        cc_automation_core::AutomationError::Remote(code)
            if code.starts_with("cleanup_")
                && code.len() <= 64
                && code.bytes().all(|b| b.is_ascii_lowercase() || b == b'_') =>
        {
            invalid(&code)
        }
        cc_automation_core::AutomationError::Timeout => invalid("cleanup_timeout"),
        _ => invalid("cleanup_failed"),
    }
}
fn parse<T: serde::de::DeserializeOwned>(raw: &str) -> AppResult<T> {
    if raw.len() > 32 * 1024 {
        return Err(invalid("cleanup_invalid_request"));
    }
    serde_json::from_str(raw).map_err(|_| invalid("cleanup_invalid_request"))
}
fn identifier(value: &str) -> bool {
    Uuid::parse_str(value).is_ok_and(|id| !id.is_nil() && id.hyphenated().to_string() == value)
}
fn selection(value: &Option<String>) -> AppResult<()> {
    if value
        .as_ref()
        .is_some_and(|v| !v.starts_with('/') || v.len() > 4096 || v.chars().any(char::is_control))
    {
        return Err(invalid("cleanup_invalid_request"));
    }
    Ok(())
}
fn strict_uuid<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Uuid, D::Error> {
    let value = String::deserialize(d)?;
    if !identifier(&value) {
        return Err(serde::de::Error::custom("invalid canonical UUID"));
    }
    Uuid::parse_str(&value).map_err(|_| serde::de::Error::custom("invalid canonical UUID"))
}
fn operation(value: Uuid) -> AppResult<String> {
    if value.is_nil() {
        Err(invalid("cleanup_invalid_request"))
    } else {
        Ok(value.to_string())
    }
}
fn paths(configuration: &AutomationRunnerConfig) -> Vec<String> {
    match configuration {
        AutomationRunnerConfig::Local {
            launch:
                HelperLaunch::Native {
                    python,
                    module_dir,
                    root,
                },
        } => [python, module_dir, root]
            .into_iter()
            .map(|p| p.to_string_lossy().into_owned())
            .collect(),
        AutomationRunnerConfig::Local {
            launch:
                HelperLaunch::Wsl {
                    python,
                    module_dir,
                    root,
                    ..
                },
        } => vec![python.clone(), module_dir.clone(), root.clone()],
        AutomationRunnerConfig::ServiceRemote { identity_id, .. } => vec![
            format!("/var/lib/consolecrypt-service/{identity_id}"),
            format!("/run/consolecrypt-service/{identity_id}"),
            "/opt/consolecrypt".into(),
            "/opt/cc-runtime".into(),
        ],
        AutomationRunnerConfig::Remote { helper, .. } => vec![
            helper.python.clone(),
            helper.module_dir.clone(),
            helper.root.clone(),
        ],
    }
}
fn under(path: &str, boundary: &str) -> bool {
    path == boundary
        || path
            .strip_prefix(boundary)
            .is_some_and(|suffix| suffix.starts_with('/'))
}
fn touches(configuration: &AutomationRunnerConfig, candidate: &ManagedCleanupCandidate) -> bool {
    paths(configuration).iter().any(|path| {
        under(path, &candidate.path)
            || candidate.paths.as_ref().is_some_and(|p| {
                [&p.python, &p.module_dir, &p.root]
                    .iter()
                    .any(|managed| *managed == path)
            })
    })
}
fn protection(
    configurations: &[AutomationRunnerConfig],
    owner: &Arc<Session>,
    selected: &Option<String>,
) -> AppResult<ManagedCleanupProtection> {
    // The caller holds connect_gate. Unsaved/direct connections are authority
    // too: their interpreter may reach a managed venv through a manual alias.
    // Deduplicate, but never truncate; exceeding the wire budget fails closed.
    let live = owner
        .automation
        .connections
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let paths = configurations
        .iter()
        .flat_map(paths)
        .chain(
            live.values()
                .flat_map(|connection| paths(&connection.configuration)),
        )
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    let result = ManagedCleanupProtection {
        paths,
        selected_python: selected.clone(),
    };
    result
        .validate()
        .map_err(|_| invalid("cleanup_catalog_unavailable"))?;
    Ok(result)
}
async fn catalog(owner: &Arc<Session>) -> AppResult<(Vec<AutomationRunnerConfig>, String)> {
    let catalog: Catalog = owner
        .storage()?
        .setting_get(RUNNERS)
        .await?
        .unwrap_or_default();
    if catalog.version != 1 || !catalog.extra.is_empty() || catalog.records.len() > 32 {
        return Err(invalid("cleanup_catalog_unavailable"));
    }
    let mut configurations = Vec::new();
    for (id, row) in &catalog.records {
        if !identifier(id)
            || row.as_object().is_none_or(|row| {
                row.keys().any(|key| {
                    !["version", "revision", "name", "configuration"].contains(&key.as_str())
                })
            })
        {
            return Err(invalid("cleanup_catalog_unavailable"));
        }
        let profile = summary(id, row, false);
        if profile.read_only || profile.name.as_ref().is_none_or(|name| name.len() > 1024) {
            return Err(invalid("cleanup_catalog_unavailable"));
        }
        let configuration = profile
            .configuration
            .ok_or_else(|| invalid("cleanup_catalog_unavailable"))?;
        if paths(&configuration)
            .iter()
            .any(|path| path.len() > 4096 || path.chars().any(char::is_control))
        {
            return Err(invalid("cleanup_catalog_unavailable"));
        }
        configurations.push(configuration);
    }
    let raw = serde_json::to_vec(&catalog).map_err(|_| invalid("cleanup_catalog_unavailable"))?;
    if raw.len() > 1024 * 1024 {
        return Err(invalid("cleanup_catalog_unavailable"));
    }
    Ok((configurations, hex::encode(Sha256::digest(raw))))
}
async fn busy(owner: &Arc<Session>) -> AppResult<bool> {
    if !owner
        .automation
        .live_jobs
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .is_empty()
        || !owner.automation.coordinator.unknown_jobs().is_empty()
        || owner.automation.scopes.has_scopes()
        || !owner
            .automation
            .git_imports
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .is_empty()
        || !owner
            .automation
            .inventory_previews
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .is_empty()
    {
        return Ok(true);
    }
    let Some(value) = owner
        .storage()?
        .setting_get::<Value>("automation.jobs.v1")
        .await?
    else {
        return Ok(false);
    };
    if value["version"] != 1 {
        return Err(invalid("cleanup_catalog_unavailable"));
    }
    let records = value["records"]
        .as_object()
        .ok_or_else(|| invalid("cleanup_catalog_unavailable"))?;
    if records.len() > 4096 {
        return Err(invalid("cleanup_catalog_unavailable"));
    }
    for row in records.values() {
        if row["version"] != 1 {
            return Err(invalid("cleanup_catalog_unavailable"));
        }
        let state = row["state"]
            .as_str()
            .ok_or_else(|| invalid("cleanup_catalog_unavailable"))?;
        if ![
            "succeeded",
            "failed",
            "cancelled",
            "interrupted",
            "acknowledged",
        ]
        .contains(&state)
        {
            return Ok(true);
        }
    }
    Ok(false)
}
fn protect(
    scan: &mut ManagedCleanupScan,
    configurations: &[AutomationRunnerConfig],
    owner: &Arc<Session>,
    selected: &Option<String>,
    busy: bool,
) {
    for candidate in &mut scan.candidates {
        if !candidate.eligible {
            continue;
        }
        let reason = if selected.as_ref().is_some_and(|path| {
            under(path, &candidate.path)
                || candidate.paths.as_ref().is_some_and(|p| path == &p.python)
        }) {
            Some("cleanup_selected")
        } else if configurations
            .iter()
            .any(|config| touches(config, candidate))
            || owner
                .automation
                .connections
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .values()
                .any(|connection| touches(&connection.configuration, candidate))
        {
            Some("cleanup_registered")
        } else if busy {
            Some("cleanup_busy")
        } else {
            None
        };
        if let Some(reason) = reason {
            candidate.eligible = false;
            candidate.blocked_reason = Some(reason.into());
        }
    }
}
fn candidate_view(row: &ManagedCleanupCandidate) -> Value {
    json!({"id":row.id,"environmentId":row.environment_id,"path":row.path,"paths":row.paths,
        "revision":row.revision,"receiptSha256":row.receipt_sha256,"device":row.device,"inode":row.inode,
        "nodes":row.nodes,"bytes":row.bytes,"quarantined":row.quarantined,"eligible":row.eligible,
        "blockedReason":row.blocked_reason,"profileOwnership":"unknown","containsState":true})
}
fn scan_view(scan: &ManagedCleanupScan) -> Value {
    json!({"version":1,"identity":scan.identity,"candidates":scan.candidates.iter().map(candidate_view).collect::<Vec<_>>(),"currentProfileChecked":true,"otherProfilesChecked":false})
}

enum Action {
    Scan {
        protection: ManagedCleanupProtection,
    },
    Delete {
        identity: ManagedCleanupIdentity,
        candidates: Vec<ManagedCleanupApproval>,
        protection: ManagedCleanupProtection,
    },
}
enum Output {
    Scan(ManagedCleanupScan),
    Removed(ManagedCleanupResult),
}
impl AppCore {
    async fn cleanup_on_target(
        &self,
        target: &SetupTarget,
        expected: &Arc<Unlocked>,
        action: Action,
    ) -> AppResult<Output> {
        let SetupTarget::Remote { host_id } = target else {
            return match action {
                Action::Scan { protection } => {
                    cc_automation_core::cleanup_scan_environment(target, &protection)
                        .await
                        .map(Output::Scan)
                        .map_err(error)
                }
                Action::Delete {
                    identity,
                    candidates,
                    protection,
                } => cc_automation_core::cleanup_delete_environment(
                    target,
                    &identity,
                    &candidates,
                    &protection,
                )
                .await
                .map(Output::Removed)
                .map_err(error),
            };
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
            return Err(invalid("cleanup_unsupported"));
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
                let session = remote.0.as_ref().unwrap();
                match action {
                    Action::Scan { protection } => {
                        cc_automation_core::cleanup_scan_remote_environment(session, &protection)
                            .await
                            .map(Output::Scan)
                            .map_err(error)
                    }
                    Action::Delete {
                        identity,
                        candidates,
                        protection,
                    } => cc_automation_core::cleanup_delete_remote_environment(
                        session,
                        &identity,
                        &candidates,
                        &protection,
                    )
                    .await
                    .map(Output::Removed)
                    .map_err(error),
                }
            })
            .boxed()
            .await
    }

    pub async fn automation_runner_cleanup_scan(&self, request_json: String) -> AppResult<Value> {
        let request: ScanRequest = parse(&request_json)?;
        selection(&request.selected_python)?;
        let operation = operation(request.operation_id)?;
        let (owner, u) = self.unlocked().await?;
        let (_registration, cancel) = begin_setup(&owner.automation, &operation)?;
        let mut shutdown = u.sharing_shutdown.subscribe();
        let _serial = tokio::select! {biased;_=cancel.cancelled()=>return Err(AppError::Cancelled),_=shutdown.changed()=>return Err(AppError::VaultLocked),guard=owner.automation.connect_gate.lock()=>guard};
        let (configs, _) = {
            let _profile = self.hold_session(&owner).await?;
            let _unlock = owner.hold_unlocked(&u).await?;
            catalog(&owner).await?
        };
        let scan = tokio::select! {biased;_=cancel.cancelled()=>return Err(AppError::Cancelled),_=shutdown.changed()=>return Err(AppError::VaultLocked),result=self.cleanup_on_target(&request.target,&u,Action::Scan{protection:protection(&configs,&owner,&request.selected_python)?})=>result?};
        let Output::Scan(mut scan) = scan else {
            return Err(invalid("cleanup_failed"));
        };
        let _profile = self.hold_session(&owner).await?;
        let _unlock = owner.hold_unlocked(&u).await?;
        let (current, _) = catalog(&owner).await?;
        // Use the final fresh catalog, not a stale first list if another CRUD
        // began before the shared connect gate was acquired.
        drop(configs);
        if cancel.is_cancelled() {
            return Err(AppError::Cancelled);
        }
        protect(
            &mut scan,
            &current,
            &owner,
            &request.selected_python,
            busy(&owner).await?,
        );
        Ok(scan_view(&scan))
    }

    pub async fn automation_runner_cleanup_plan(&self, request_json: String) -> AppResult<Value> {
        let request: PlanRequest = parse(&request_json)?;
        selection(&request.selected_python)?;
        if request.candidate_ids.is_empty()
            || request.candidate_ids.len() > 8
            || request.candidate_ids.iter().any(|id| !identifier(id))
            || request.candidate_ids.iter().collect::<BTreeSet<_>>().len()
                != request.candidate_ids.len()
        {
            return Err(invalid("cleanup_invalid_request"));
        }
        let operation = operation(request.operation_id)?;
        let (owner, u) = self.unlocked().await?;
        let (_registration, cancel) = begin_setup(&owner.automation, &operation)?;
        let mut shutdown = u.sharing_shutdown.subscribe();
        let _serial = tokio::select! {biased;_=cancel.cancelled()=>return Err(AppError::Cancelled),_=shutdown.changed()=>return Err(AppError::VaultLocked),guard=owner.automation.connect_gate.lock()=>guard};
        let (configs, digest, version, generation) = {
            let _profile = self.hold_session(&owner).await?;
            let _unlock = owner.hold_unlocked(&u).await?;
            let (configs, digest) = catalog(&owner).await?;
            (
                configs,
                digest,
                u.working().version(),
                owner.automation.unlocked_generation()?,
            )
        };
        let output = tokio::select! {biased;_=cancel.cancelled()=>return Err(AppError::Cancelled),_=shutdown.changed()=>return Err(AppError::VaultLocked),result=self.cleanup_on_target(&request.target,&u,Action::Scan{protection:protection(&configs,&owner,&request.selected_python)?})=>result?};
        let Output::Scan(mut scan) = output else {
            return Err(invalid("cleanup_failed"));
        };
        let _profile = self.hold_session(&owner).await?;
        let _unlock = owner.hold_unlocked(&u).await?;
        if u.working().version() != version
            || catalog(&owner).await?.1 != digest
            || owner.automation.unlocked_generation()? != generation
        {
            return Err(invalid("cleanup_context_changed"));
        }
        protect(
            &mut scan,
            &configs,
            &owner,
            &request.selected_python,
            busy(&owner).await?,
        );
        let mut candidates = Vec::new();
        for id in &request.candidate_ids {
            let row = scan
                .candidates
                .iter()
                .find(|row| row.id == *id)
                .ok_or_else(|| invalid("cleanup_candidate_changed"))?;
            if !row.eligible {
                return Err(invalid(
                    row.blocked_reason.as_deref().unwrap_or("cleanup_failed"),
                ));
            }
            candidates.push(row.clone());
        }
        let identity = scan
            .identity
            .ok_or_else(|| invalid("cleanup_candidate_changed"))?;
        if cancel.is_cancelled() {
            return Err(AppError::Cancelled);
        }
        let token = Uuid::new_v4();
        let view = json!({"version":1,"planToken":token,"expiresInSeconds":60,"target":request.target,"candidates":candidates.iter().map(candidate_view).collect::<Vec<_>>(),"currentProfileChecked":true,"otherProfilesChecked":false,"containsState":true});
        let mut plans = owner
            .automation
            .cleanup_plans
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        plans.retain(|_, p| p.expires > Instant::now());
        if plans.len() >= 16 {
            return Err(invalid("cleanup_busy"));
        }
        plans.insert(
            token,
            PreparedCleanup {
                target: request.target,
                selected_python: request.selected_python,
                identity,
                candidates,
                catalog_digest: digest,
                working_version: version,
                generation,
                expires: Instant::now() + Duration::from_secs(60),
            },
        );
        Ok(view)
    }

    pub async fn automation_runner_cleanup_commit(&self, request_json: String) -> AppResult<Value> {
        let request: CommitRequest = parse(&request_json)?;
        selection(&request.selected_python)?;
        let operation = operation(request.operation_id)?;
        let (owner, u) = self.unlocked().await?;
        // One use, including failed/cancelled attempts. No caller path/target.
        let plan = owner
            .automation
            .cleanup_plans
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(&request.plan_token)
            .ok_or_else(|| invalid("cleanup_plan_expired"))?;
        if plan.expires <= Instant::now() {
            return Err(invalid("cleanup_plan_expired"));
        }
        if plan.selected_python != request.selected_python {
            return Err(invalid("cleanup_context_changed"));
        }
        let (_registration, cancel) = begin_setup(&owner.automation, &operation)?;
        let mut shutdown = u.sharing_shutdown.subscribe();
        // Share the pause-owner mutex with profile transitions so an unrelated
        // transition cannot have its admission flag reset by this cleanup.
        let _transition = self
            .inner
            .profile_transition
            .clone()
            .try_lock_owned()
            .map_err(|_| invalid("cleanup_busy"))?;
        let _serial = tokio::select! {biased;_=cancel.cancelled()=>return Err(AppError::Cancelled),_=shutdown.changed()=>return Err(AppError::VaultLocked),guard=owner.automation.connect_gate.lock()=>guard};
        if plan.expires <= Instant::now() {
            return Err(invalid("cleanup_plan_expired"));
        }
        self.automation_initialize_jobs(&owner).await?;
        let (_paused, protection) = {
            let _profile = self.hold_session(&owner).await?;
            let _unlock = owner.hold_unlocked(&u).await?;
            if u.working().version() != plan.working_version
                || owner.automation.unlocked_generation()? != plan.generation
            {
                return Err(invalid("cleanup_context_changed"));
            }
            let (configs, digest) = catalog(&owner).await?;
            if digest != plan.catalog_digest {
                return Err(invalid("cleanup_context_changed"));
            }
            if owner
                .automation
                .admission_paused
                .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
                .is_err()
            {
                return Err(invalid("cleanup_busy"));
            }
            let paused = Pause(owner.clone());
            let mut scan = ManagedCleanupScan {
                version: 1,
                identity: Some(plan.identity.clone()),
                candidates: plan.candidates.clone(),
                other_profiles_checked: false,
            };
            protect(
                &mut scan,
                &configs,
                &owner,
                &request.selected_python,
                busy(&owner).await?,
            );
            if let Some(row) = scan.candidates.iter().find(|row| !row.eligible) {
                return Err(invalid(
                    row.blocked_reason.as_deref().unwrap_or("cleanup_failed"),
                ));
            }
            (
                paused,
                protection(&configs, &owner, &request.selected_python)?,
            )
        };
        let approvals = plan
            .candidates
            .iter()
            .map(|row| {
                Ok(ManagedCleanupApproval {
                    id: row.id.clone(),
                    revision: row
                        .revision
                        .clone()
                        .ok_or_else(|| invalid("cleanup_candidate_changed"))?,
                })
            })
            .collect::<AppResult<Vec<_>>>()?;
        if plan.expires <= Instant::now() {
            return Err(invalid("cleanup_plan_expired"));
        }
        // The approval deadline remains live through SSH setup and Python's
        // rescan/deletion. Expiry drops the stdin lease; Python checks EOF before
        // every mutation (at most one already-linearized mutation can finish).
        let deadline = tokio::time::Instant::from_std(plan.expires);
        let output = tokio::select! {biased;_=cancel.cancelled()=>return Err(AppError::Cancelled),_=shutdown.changed()=>return Err(AppError::VaultLocked),_=tokio::time::sleep_until(deadline)=>return Err(invalid("cleanup_plan_expired")),result=self.cleanup_on_target(&plan.target,&u,Action::Delete{identity:plan.identity,candidates:approvals,protection})=>result?};
        let Output::Removed(result) = output else {
            return Err(invalid("cleanup_failed"));
        };
        let _profile = self.hold_session(&owner).await?;
        let _unlock = owner.hold_unlocked(&u).await?;
        if cancel.is_cancelled() {
            return Err(AppError::Cancelled);
        }
        if u.working().version() != plan.working_version
            || catalog(&owner).await?.1 != plan.catalog_digest
            || owner.automation.unlocked_generation()? != plan.generation
        {
            return Err(invalid("cleanup_context_changed"));
        }
        Ok(json!({"version":1,"removed":result.removed,"otherProfilesChecked":false}))
    }
}
struct Pause(Arc<Session>);
impl Drop for Pause {
    fn drop(&mut self) {
        self.0
            .automation
            .admission_paused
            .store(false, Ordering::Release);
    }
}

#[cfg(test)]
#[path = "cleanup_tests.rs"]
mod tests;
