//! Fixed new-project source factory, not an installer/executor or release grant.
use super::*;
use crate::agent_reader::OriginalSessionAuth;
use crate::automation_prepare::CancelOnDrop;
use crate::session::{Session, Unlocked};
use cc_automation_core::{AgentInstallOperation, AgentInstallerSeed};
use std::time::{Duration, Instant};
use tokio_util::sync::CancellationToken;

fn changed() -> AppError {
    AppError::invalid("agent_installer", "agent_installer_context_changed")
}
fn unavailable() -> AppError {
    AppError::invalid("agent_installer", "agent_installer_unavailable")
}
fn id(raw: &str) -> AppResult<Uuid> {
    let v = Uuid::parse_str(raw).map_err(|_| changed())?;
    if v.is_nil() || v.to_string() != raw {
        return Err(changed());
    }
    Ok(v)
}
#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Frame {
    version: u8,
    profile_id: String,
    notification_scope_id: String,
}
impl Frame {
    fn validate(&self) -> AppResult<()> {
        id(&self.profile_id)?;
        if self.version != 1
            || self.notification_scope_id.is_empty()
            || self.notification_scope_id.len() > 256
            || self.notification_scope_id.chars().any(char::is_control)
        {
            return Err(changed());
        }
        Ok(())
    }
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Create {
    version: u8,
    profile_id: String,
    notification_scope_id: String,
    name: String,
    operation: AgentInstallOperation,
    start: bool,
    host_id: String,
    expected_host_revision: String,
    artifact_path: Option<String>,
    manifest_path: Option<String>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct MonitoringNodeCreate {
    version: u8,
    profile_id: String,
    notification_scope_id: String,
    name: String,
    operation: cc_automation_core::monitoring_node_plan::MonitoringNodeOperation,
    host_id: String,
    expected_host_revision: String,
    plan: cc_automation_core::monitoring_node_plan::MonitoringNodePlan,
}
fn parse<T: serde::de::DeserializeOwned>(raw: &str) -> AppResult<T> {
    if raw.is_empty() || raw.len() > 16 * 1024 {
        return Err(changed());
    }
    serde_json::from_str(raw).map_err(|_| changed())
}
struct Original {
    app: Weak<crate::app::Inner>,
    owner: Weak<Session>,
    unlock: Weak<Unlocked>,
    auth: OriginalSessionAuth,
    frame: Frame,
    version: u64,
    until: Instant,
    cancel: CancellationToken,
}
impl Original {
    async fn capture(
        app: &AppCore,
        frame: Frame,
        cancel: CancellationToken,
    ) -> AppResult<Arc<Self>> {
        frame.validate()?;
        let auth = OriginalSessionAuth::capture(app)?;
        let until = Instant::now() + Duration::from_secs(120);
        let (owner, u) = tokio::select! {biased;
            _=auth.cancelled()=>return Err(changed()),
            _=tokio::time::sleep_until(until.into())=>return Err(unavailable()),
            result=app.unlocked()=>result?,
        };
        let original = Arc::new(Self {
            app: Arc::downgrade(&app.inner),
            owner: Arc::downgrade(&owner),
            unlock: Arc::downgrade(&u),
            auth,
            frame,
            version: u.working().version(),
            until,
            cancel,
        });
        original.run(|| Ok(()))?;
        Ok(original)
    }
    /// Same nonblocking original guards through each short callback. In a SQL
    /// worker the callback wraps db.project_publication_write THROUGH commit.
    /// No latest profile lookup, lock acquisition retry or storage reentry.
    fn run<T>(&self, f: impl FnOnce() -> AppResult<T>) -> AppResult<T> {
        self.auth.try_with_current(|| {
            if self.cancel.is_cancelled() || Instant::now() >= self.until {
                return Err(changed());
            }
            let app = self.app.upgrade().ok_or_else(changed)?;
            let owner = self.owner.upgrade().ok_or_else(changed)?;
            let u = self.unlock.upgrade().ok_or(AppError::VaultLocked)?;
            let active = app.active.try_lock().map_err(|_| changed())?;
            if !active.as_ref().is_some_and(|s| Arc::ptr_eq(s, &owner)) {
                return Err(changed());
            }
            owner.try_with_unlocked(&u, || {
                if *u.sharing_shutdown.borrow()
                    || owner.profile_id.to_string() != self.frame.profile_id
                    || u.notices.scope_id().as_deref()
                        != Some(self.frame.notification_scope_id.as_str())
                {
                    return Err(changed());
                }
                u.working()
                    .tunnel_io_at_version(self.version, f)
                    .ok_or_else(changed)?
            })
        })
    }
    fn check(&self) -> AppResult<()> {
        self.run(|| Ok(()))
    }
    fn choices(&self) -> AppResult<Value> {
        // The WorkingSet method takes its own read guard, so do not call it
        // recursively inside tunnel_io_at_version. Recheck its exact version.
        self.check()?;
        let u = self.unlock.upgrade().ok_or(AppError::VaultLocked)?;
        let rows = u
            .working()
            .agent_installer_host_choices()
            .ok_or_else(unavailable)?;
        let value=json!(rows.into_iter().map(|(id,revision,name)|json!({"hostId":id,"hostRevision":revision.to_string(),"name":name})).collect::<Vec<_>>());
        self.run(|| Ok(value))
    }
}
impl AppCore {
    /// Prepare a fixed monitoring-node playbook in a NEW local project. Target
    /// installation still requires the ordinary immutable Signed Run review.
    pub async fn automation_monitoring_node_create_json(
        &self,
        request_json: &str,
    ) -> AppResult<String> {
        let request: MonitoringNodeCreate = parse(request_json)?;
        request.plan.validate().map_err(automation_error)?;
        let name = valid_name(&request.name)?;
        let host = id(&request.host_id)?;
        let revision = request
            .expected_host_revision
            .parse::<i64>()
            .map_err(|_| changed())?;
        if revision <= 0
            || revision.to_string() != request.expected_host_revision
            || name != request.name
        {
            return Err(changed());
        }
        let cancel = CancellationToken::new();
        let _cancel = CancelOnDrop(cancel.clone());
        let original = Original::capture(
            self,
            Frame {
                version: request.version,
                profile_id: request.profile_id,
                notification_scope_id: request.notification_scope_id,
            },
            cancel,
        )
        .await?;
        if !original.choices()?.as_array().is_some_and(|rows| {
            rows.iter().any(|row| {
                row["hostId"] == json!(host)
                    && row["hostRevision"] == json!(request.expected_host_revision)
            })
        }) {
            return Err(changed());
        }
        let owner = original.owner.upgrade().ok_or_else(changed)?;
        let profile = owner.dir.clone().ok_or_else(unavailable)?;
        let storage = owner.storage()?;
        let slot = automation_io_slot(&owner)?;
        drop(owner);
        let project = Uuid::new_v4();
        let worker = original.clone();
        let seed = tokio::task::spawn_blocking(move || {
            let _slot = slot;
            let seed = AgentInstallerSeed::create_monitoring_node(
                &profile,
                project,
                request.operation,
                &request.plan,
                || {
                    worker
                        .check()
                        .map_err(|_| cc_automation_core::AutomationError::Closed)
                },
            )
            .map_err(automation_error)?;
            seed.recheck().map_err(automation_error)?;
            worker.check()?;
            Ok::<_, AppError>(seed)
        })
        .await
        .map_err(|_| unavailable())??;
        let mut inventory = cc_automation_core::ProjectInventory::default();
        inventory
            .groups
            .insert("monitoring_nodes".into(), vec![host]);
        publish_seed_with_inventory(
            original, storage, seed, project, name, host, revision, inventory,
        )
        .await
    }

    /// Explicit read-only dialog action. No SSH, feature query or installation.
    pub async fn automation_agent_installer_context_json(
        &self,
        request_json: &str,
    ) -> AppResult<String> {
        let frame: Frame = parse(request_json)?;
        let cancel = CancellationToken::new();
        let _cancel = CancelOnDrop(cancel.clone());
        let original = Original::capture(self, frame, cancel).await?;
        let hosts = original.choices()?;
        original.run(|| {
            serde_json::to_string(&json!({"version":1,"profileId":original.frame.profile_id,
            "notificationScopeId":original.frame.notification_scope_id,"hosts":hosts}))
            .map_err(|_| unavailable())
        })
    }
    /// Creates only reviewed source in a genuine NEW managed project. Ordinary
    /// Run/PreparedReview remains the sole path to target execution.
    pub async fn automation_agent_installer_create_json(
        &self,
        request_json: &str,
    ) -> AppResult<String> {
        let request: Create = parse(request_json)?;
        let frame = Frame {
            version: request.version,
            profile_id: request.profile_id,
            notification_scope_id: request.notification_scope_id,
        };
        let name = valid_name(&request.name)?;
        let host = id(&request.host_id)?;
        let revision = request
            .expected_host_revision
            .parse::<i64>()
            .map_err(|_| changed())?;
        if revision <= 0
            || revision.to_string() != request.expected_host_revision
            || name != request.name
            || request.operation.needs_release()
                != (request.artifact_path.is_some() && request.manifest_path.is_some())
            || !request.operation.needs_release()
                && (request.artifact_path.is_some() || request.manifest_path.is_some())
        {
            return Err(changed());
        }
        let cancel = CancellationToken::new();
        let _cancel = CancelOnDrop(cancel.clone());
        let original = Original::capture(self, frame, cancel).await?;
        let choices = original.choices()?;
        if !choices.as_array().is_some_and(|rows| {
            rows.iter().any(|r| {
                r["hostId"] == json!(host)
                    && r["hostRevision"] == json!(request.expected_host_revision)
            })
        }) {
            return Err(changed());
        }
        let owner = original.owner.upgrade().ok_or_else(changed)?;
        let profile = owner.dir.clone().ok_or_else(unavailable)?;
        let storage = owner.storage()?;
        let slot = automation_io_slot(&owner)?;
        drop(owner);
        let project = Uuid::new_v4();
        let artifact = request.artifact_path.map(PathBuf::from);
        let manifest = request.manifest_path.map(PathBuf::from);
        let worker = original.clone();
        // The fixed worker owns its existing IO permit through actual OS join,
        // including when the caller/route disappears. Drop cancels its checks.
        let seed = tokio::task::spawn_blocking(move || {
            let _slot = slot;
            let seed = AgentInstallerSeed::create(
                &profile,
                project,
                request.operation,
                request.start,
                artifact.as_deref().zip(manifest.as_deref()),
                |raw, actual, bytes| {
                    let signed = cc_agent_node_wire::Signed::<
                            cc_agent_node_wire::ReleaseManifest,
                        >::from_json(raw)
                        .map_err(|_| {
                            cc_automation_core::AutomationError::Invalid(
                                "agent_installer_release_input",
                            )
                        })?;
                    if signed.body.protocol != 1
                        || signed.body.artifact_bytes != bytes
                        || hex::encode(signed.body.artifact_sha256.as_bytes()) != actual
                    {
                        return Err(cc_automation_core::AutomationError::Invalid(
                            "agent_installer_release_input",
                        ));
                    }
                    // from_json checks the closed original public schema only.
                    // No imported signer pin and NO verify/trust assertion.
                    Ok(())
                },
                || {
                    worker
                        .check()
                        .map_err(|_| cc_automation_core::AutomationError::Closed)
                },
            )
            .map_err(automation_error)?;
            // All source bytes are rechecked in the SAME bounded OS worker,
            // outside auth/unlock/WorkingSet guards and outside SQL.
            seed.recheck().map_err(automation_error)?;
            worker.check()?;
            Ok::<_, AppError>(seed)
        })
        .await
        .map_err(|_| unavailable())??;
        publish_seed(original, storage, seed, project, name, host, revision).await
    }
}

// Same production publication body, factored for the actual held-SQL causal.
// The cached digest is an observed source snapshot, not deployment authority;
// ordinary Signed Run independently captures its immutable source revision.
async fn publish_seed(
    original: Arc<Original>,
    storage: cc_storage_core::Storage,
    seed: AgentInstallerSeed,
    project: Uuid,
    name: String,
    host: Uuid,
    revision: i64,
) -> AppResult<String> {
    publish_seed_with_inventory(
        original,
        storage,
        seed,
        project,
        name,
        host,
        revision,
        cc_automation_core::ProjectInventory::default(),
    )
    .await
}

async fn publish_seed_with_inventory(
    original: Arc<Original>,
    storage: cc_storage_core::Storage,
    seed: AgentInstallerSeed,
    project: Uuid,
    name: String,
    host: Uuid,
    revision: i64,
    inventory: cc_automation_core::ProjectInventory,
) -> AppResult<String> {
    original.check()?;
    let now = chrono::Utc::now().timestamp();
    if now <= 0 {
        return Err(unavailable());
    }
    let selected = source_bindings::selected_inventory(inventory, vec![host])?;
    let row = json!({"version":1,"name":name,"source":{"kind":"managed","path":seed.path()},"revision":1,
    "created_at":now,"updated_at":now,"inventory":selected});
    let project_id = project.to_string();
    let record = row.clone();
    let checked = original.clone();
    // Retain the exact prepared root/ancestor capabilities until the SQL
    // transaction has joined; no path-only reopen becomes publication.
    let queued=storage.call(move|db|checked.run(|| {
    seed.recheck_identity().map_err(automation_error)?;
    let result=db.project_publication_write(|tx| {
        if checked.cancel.is_cancelled()||Instant::now()>=checked.until {return Err(changed());}
        let mut catalog:Catalog=tx.setting_get(PROJECTS)?.unwrap_or_default();
        if catalog.version!=1||!catalog.extra.is_empty()||catalog.records.len()>=256||catalog.records.contains_key(&project_id) {return Err(changed());}
        catalog.records.insert(project_id.clone(),record);
        if checked.cancel.is_cancelled()||Instant::now()>=checked.until {return Err(changed());}
        tx.setting_set(PROJECTS,&catalog)?;
        Ok::<_,AppError>(json!({"version":1,"project":summary(&project_id,&row,false),
            "projectDigest":seed.revision().digest,"claimedRelease":seed.claimed_release(),"actualRuntimeVersion":null,
            "hostId":host,"hostRevision":revision.to_string(),"playbook":"playbook.yml"}))
    });
    result.map_err(|e|match e {
        cc_storage_core::ProjectPublicationWriteError::Rejected(e)=>e,
        _=>AppError::invalid("agent_installer","agent_installer_project_outcome_unknown"),
    })
}));
    // SQL waiting is not OS worker custody. Bound the wait by the SAME
    // original cancellation/auth/deadline. The queued callback separately
    // checks those original fences before any eventual mutation.
    let mut shutdown = original
        .unlock
        .upgrade()
        .ok_or(AppError::VaultLocked)?
        .sharing_shutdown
        .subscribe();
    if *shutdown.borrow() {
        return Err(changed());
    }
    let result = tokio::select! {biased;
        _=original.cancel.cancelled()=>return Err(changed()),
        _=original.auth.cancelled()=>return Err(changed()),
        _=shutdown.changed()=>return Err(changed()),
        _=tokio::time::sleep_until(original.until.into())=>return Err(unavailable()),
        result=queued=>result?,
    };
    original.run(|| {
        let mut response = result;
        response["profileId"] = json!(original.frame.profile_id);
        response["notificationScopeId"] = json!(original.frame.notification_scope_id);
        serde_json::to_string(&response).map_err(|_| unavailable())
    })
}

#[cfg(test)]
mod tests;
