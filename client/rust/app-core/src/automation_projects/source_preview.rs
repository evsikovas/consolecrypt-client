//! Ephemeral no-credential inventory previews, fenced to the original unlock.
use super::*;
use crate::automation_prepare::RevisionLease;
use crate::automation_runners::{AutomationRuntime, RunnerConnection};
use crate::session::Unlocked;
use cc_automation_core::{CapturedInventory, InventorySource, ProjectRevision};
use std::{
    sync::Mutex,
    time::{Duration, Instant},
};
use tokio::sync::OwnedSemaphorePermit;
use tokio_util::sync::CancellationToken;

pub(crate) struct ReadyInventory {
    pub revision: ProjectRevision,
    pub captured: CapturedInventory,
    _snapshot: RevisionLease,
}
enum PreviewState {
    Preparing,
    Ready(Arc<ReadyInventory>),
    Failed(String),
}
pub(crate) struct InventoryPreview {
    pub runner_id: String,
    project_id: String,
    project_revision: u64,
    generation: u64,
    unlock: Weak<Unlocked>,
    sources: Vec<InventorySource>,
    expires_at: i64,
    deadline: Instant,
    pub cancel: CancellationToken,
    state: Mutex<PreviewState>,
    _slot: OwnedSemaphorePermit,
}
impl Drop for InventoryPreview {
    fn drop(&mut self) {
        self.cancel.cancel();
    }
}
impl InventoryPreview {
    fn usable(&self, generation: u64) -> bool {
        self.generation == generation
            && self.deadline > Instant::now()
            && self.unlock.upgrade().is_some()
    }
    fn metadata(&self, id: Uuid) -> Value {
        let state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        let mut value = json!({"version":1,"preview_id":id,"project_id":self.project_id,
            "project_revision":self.project_revision,"runner_id":self.runner_id,"sources":self.sources,"expires_at":self.expires_at});
        match &*state {
            PreviewState::Preparing => value["state"] = json!("preparing"),
            PreviewState::Failed(code) => {
                value["state"] = json!("failed");
                value["error"] = json!(code);
            }
            PreviewState::Ready(ready) => {
                value["state"] = json!("ready");
                value["revision_digest"] = json!(ready.revision.digest);
                value["inventory_digest"] = json!(ready.captured.digest);
                value["host_count"] = json!(ready.captured.inventory.hosts.len());
                value["group_count"] = json!(ready.captured.inventory.groups.len());
                value["resolved_sources"] = json!(ready.captured.sources);
            }
        }
        value
    }
}
fn trim(runtime: &AutomationRuntime, generation: u64) {
    runtime
        .inventory_previews
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .retain(|_, entry| {
            let usable = entry.usable(generation);
            if !usable {
                entry.cancel.cancel();
            }
            usable
        });
}

impl AppCore {
    /// Advisory, bounded diagnostics for the user's draft. Does not connect a
    /// target, resolve any credential value, consume the preview or approve a job.
    pub async fn automation_inventory_check(
        &self,
        id: String,
        configuration_json: String,
    ) -> AppResult<Value> {
        let id = Uuid::parse_str(&id)
            .map_err(|_| AppError::invalid("inventory_preview", "invalid preview ID"))?;
        let config =
            cc_automation_core::ExternalInventoryConfig::parse(configuration_json.as_bytes())
                .map_err(automation_error)?;
        let (owner, unlock) = self.unlocked().await?;
        let generation = owner.automation.unlocked_generation()?;
        trim(&owner.automation, generation);
        let entry = owner
            .automation
            .inventory_previews
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(&id)
            .cloned()
            .ok_or_else(|| {
                AppError::invalid("inventory_preview", "preview expired or was closed")
            })?;
        let original = entry.unlock.upgrade().ok_or(AppError::VaultLocked)?;
        if !Arc::ptr_eq(&original, &unlock)
            || !entry.usable(generation)
            || entry.cancel.is_cancelled()
        {
            return Err(AppError::VaultLocked);
        }
        if config.sources != entry.sources {
            return Err(AppError::invalid(
                "inventory_bindings",
                "source selection changed; preview again",
            ));
        }
        let ready = match &*entry.state.lock().unwrap_or_else(|e| e.into_inner()) {
            PreviewState::Ready(ready) => ready.clone(),
            _ => {
                return Err(AppError::invalid(
                    "inventory_preview",
                    "wait for a ready preview",
                ))
            }
        };
        let sources = ready
            .captured
            .inventory
            .hosts
            .iter()
            .map(|h| (h.alias.as_str(), h))
            .collect::<BTreeMap<_, _>>();
        let mut conflicts = Vec::new();
        let mut count = 0usize;
        for (index, (alias, binding)) in config.bindings.iter().enumerate() {
            if index % 64 == 0 {
                tokio::task::yield_now().await;
            }
            if !entry.usable(owner.automation.unlocked_generation()?) || entry.cancel.is_cancelled()
            {
                return Err(AppError::VaultLocked);
            }
            let mut saved = None;
            let kind = match sources.get(alias.as_str()) {
                None => Some("source_missing"),
                Some(host)
                    if binding.source != cc_automation_core::SourceEndpoint::from_host(host) =>
                {
                    Some("source_changed")
                }
                Some(_) => {
                    let host_id = crate::dto::parse_id("host_id", &binding.host_id.to_string())?;
                    if crate::inventory::host_of(&unlock, host_id).is_err() {
                        Some("saved_host_missing")
                    } else {
                        // Planner reads inventory/credential references only;
                        // no CredentialResolver or SSH connector is invoked.
                        match unlock.ssh.plan(host_id).await {
                            Err(_) => Some("saved_route_invalid"),
                            Ok(plan) => {
                                saved = Some(
                                    json!({"address":plan.target.host,"port":plan.target.port,"username":plan.username}),
                                );
                                if binding.resolution
                                    == cc_automation_core::BindingResolution::Match
                                    && (binding.source.address != plan.target.host
                                        || binding.source.port != plan.target.port
                                        || binding
                                            .source
                                            .username
                                            .as_ref()
                                            .is_some_and(|u| *u != plan.username))
                                {
                                    Some("endpoint_mismatch")
                                } else {
                                    None
                                }
                            }
                        }
                    }
                }
            };
            if let Some(kind) = kind {
                count += 1;
                if conflicts.len() < 50 {
                    conflicts.push(json!({"alias":alias,"kind":kind,"saved":saved}));
                }
            }
        }
        let _profile = self.hold_session(&owner).await?;
        let _epoch = owner.hold_unlocked(&original).await?;
        if !entry.usable(owner.automation.unlocked_generation()?) || entry.cancel.is_cancelled() {
            return Err(AppError::VaultLocked);
        }
        Ok(
            json!({"version":1,"preview_id":id,"project_id":entry.project_id,"valid":count==0,"conflict_count":count,"conflicts":conflicts}),
        )
    }

    /// Consumes one ready preview. Sources cannot appoint Host IDs; these are
    /// explicitly selected by the user and must already exist in this profile.
    pub async fn automation_inventory_commit(
        &self,
        id: String,
        configuration_json: String,
    ) -> AppResult<AutomationProjectSummary> {
        self.automation_inventory_commit_inner(id, configuration_json, None)
            .await
    }

    pub(super) async fn automation_inventory_commit_inner(
        &self,
        id: String,
        configuration_json: String,
        selection: Option<source_bindings::SourceSelection>,
    ) -> AppResult<AutomationProjectSummary> {
        let id = Uuid::parse_str(&id)
            .map_err(|_| AppError::invalid("inventory_preview", "invalid preview ID"))?;
        let config =
            cc_automation_core::ExternalInventoryConfig::parse(configuration_json.as_bytes())
                .map_err(automation_error)?;
        let (owner, unlock) = self.unlocked().await?;
        let generation = owner.automation.unlocked_generation()?;
        trim(&owner.automation, generation);
        let entry = owner
            .automation
            .inventory_previews
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(&id)
            .cloned()
            .ok_or_else(|| {
                AppError::invalid("inventory_preview", "preview expired or was closed")
            })?;
        let original = entry.unlock.upgrade().ok_or(AppError::VaultLocked)?;
        if !Arc::ptr_eq(&original, &unlock)
            || !entry.usable(generation)
            || entry.cancel.is_cancelled()
        {
            return Err(AppError::VaultLocked);
        }
        // Scanning a user-selected filesystem can stall. Keep no vault key or
        // general credential resolver alive while waiting for that worker.
        drop(original);
        drop(unlock);
        let ready = match &*entry.state.lock().unwrap_or_else(|e| e.into_inner()) {
            PreviewState::Ready(ready) => ready.clone(),
            _ => {
                return Err(AppError::invalid(
                    "inventory_preview",
                    "wait for a ready preview",
                ))
            }
        };
        if config.sources != entry.sources {
            return Err(AppError::invalid(
                "inventory_bindings",
                "source selection changed; preview again",
            ));
        }
        config
            .validate_capture(&ready.captured.inventory)
            .map_err(automation_error)?;
        let context = self.automation_project_context(&entry.project_id).await?;
        let inventory = if let Some(selection) = selection {
            if selection.project_id != entry.project_id
                || selection.expected_revision != entry.project_revision
            {
                return Err(AppError::invalid(
                    "inventory_selection",
                    "preview belongs to another project revision",
                ));
            }
            Some(source_bindings::selected_inventory(
                context.inventory,
                selection.saved_hosts,
            )?)
        } else {
            None
        };
        if !Arc::ptr_eq(&context.owner, &owner)
            || context.project.revision != Some(entry.project_revision)
        {
            return Err(AppError::invalid(
                "inventory_bindings",
                "project changed; preview again",
            ));
        }
        let path = context.path;
        let io_slot = automation_io_slot(&owner)?;
        let current = tokio::task::spawn_blocking(move || {
            let _io_slot = io_slot;
            // Reopen the current configured path; the old preview handle must
            // not hide replacement of the complete project directory.
            ProjectRoot::open(&path)?.revision()
        })
        .await
        .map_err(|_| AppError::invalid("inventory_bindings", "filesystem worker stopped"))?
        .map_err(automation_error)?;
        if current.digest != ready.revision.digest {
            return Err(AppError::invalid(
                "inventory_bindings",
                "project files changed; preview again",
            ));
        }
        let original = entry.unlock.upgrade().ok_or(AppError::VaultLocked)?;
        let _profile = self.hold_session(&owner).await?;
        let _epoch = owner.hold_unlocked(&original).await?;
        if owner.automation.unlocked_generation()? != generation
            || !entry.usable(generation)
            || entry.cancel.is_cancelled()
        {
            return Err(AppError::VaultLocked);
        }
        for binding in config.bindings.values() {
            crate::inventory::host_of(
                &original,
                crate::dto::parse_id("host_id", &binding.host_id.to_string())?,
            )?;
        }
        if let Some(inventory) = &inventory {
            for id in inventory.selected_hosts.as_ref().unwrap() {
                crate::inventory::host_of(
                    &original,
                    crate::dto::parse_id("host_id", &id.to_string())?,
                )?;
            }
        }
        {
            let mut entries = owner
                .automation
                .inventory_previews
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            if !entries
                .get(&id)
                .is_some_and(|current| Arc::ptr_eq(current, &entry))
            {
                return Err(AppError::invalid(
                    "inventory_preview",
                    "preview was already consumed or closed",
                ));
            }
            entries.remove(&id);
        }
        entry.cancel.cancel();
        source_bindings::save_documents(
            &owner,
            entry.project_id.clone(),
            entry.project_revision,
            Some(config),
            inventory,
        )
        .await
    }

    pub async fn automation_inventory_start(
        &self,
        project_id: String,
        runner_id: String,
        sources_json: String,
    ) -> AppResult<Value> {
        crate::dto::parse_id("runner_id", &runner_id)?;
        if sources_json.len() > 16 * 1024 {
            return Err(AppError::invalid(
                "inventory_sources",
                "source selection exceeds limit",
            ));
        }
        let sources: Vec<InventorySource> = serde_json::from_str(&sources_json)
            .map_err(|_| AppError::invalid("inventory_sources", "invalid source selection"))?;
        if sources.is_empty() || sources.len() > 8 {
            return Err(AppError::invalid(
                "inventory_sources",
                "select one to eight static files",
            ));
        }
        for source in &sources {
            cc_automation_core::validate_project_path(&source.path).map_err(automation_error)?;
        }
        let context = self.automation_project_context(&project_id).await?;
        let owner = context.owner.clone();
        let runtime = owner.automation.clone();
        let generation = runtime.unlocked_generation()?;
        trim(&runtime, generation);
        let runner = runtime
            .connections
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(&runner_id)
            .cloned()
            .filter(|runner| !runner.peer.is_closed())
            .ok_or_else(|| {
                AppError::invalid("inventory_runner", "connect the selected Runner first")
            })?;
        if runner
            .probe
            .pointer("/capabilities/static_inventory_sources")
            .and_then(Value::as_bool)
            != Some(true)
            || ["filesystem", "pid", "network"].iter().any(|name| {
                runner
                    .probe
                    .pointer(&format!("/sandbox/{name}"))
                    .and_then(Value::as_bool)
                    != Some(true)
            })
        {
            return Err(AppError::invalid(
                "inventory_runner",
                "isolated static inventory parsing is unavailable",
            ));
        }
        let slot = runtime
            .inventory_slots
            .clone()
            .try_acquire_owned()
            .map_err(|_| {
                AppError::invalid("inventory_preview", "close an existing preview first")
            })?;
        let prepare = runtime
            .prepare_slots
            .clone()
            .try_acquire_owned()
            .map_err(|_| AppError::invalid("inventory_preview", "preparation limit reached"))?;
        let io_slot = automation_io_slot(&owner)?;
        let original = context.unlock.upgrade().ok_or(AppError::VaultLocked)?;
        let mut shutdown = original.sharing_shutdown.subscribe();
        if *shutdown.borrow() {
            return Err(AppError::VaultLocked);
        }
        let id = Uuid::new_v4();
        let entry = Arc::new(InventoryPreview {
            runner_id,
            project_id,
            project_revision: context.project.revision.ok_or(AppError::VaultLocked)?,
            generation,
            unlock: context.unlock.clone(),
            sources,
            expires_at: chrono::Utc::now().timestamp() + 600,
            deadline: Instant::now() + Duration::from_secs(600),
            cancel: CancellationToken::new(),
            state: Mutex::new(PreviewState::Preparing),
            _slot: slot,
        });
        let _profile = self.hold_session(&owner).await?;
        let _epoch = owner.hold_unlocked(&original).await?;
        if runtime.unlocked_generation()? != generation {
            return Err(AppError::VaultLocked);
        }
        runtime
            .inventory_previews
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(id, entry.clone());
        drop(_epoch);
        drop(_profile);
        drop(original);
        let response = entry.metadata(id);
        let weak_runtime = Arc::downgrade(&runtime);
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_secs(600)).await;
            if let Some(runtime) = weak_runtime.upgrade() {
                if let Some(entry) = runtime
                    .inventory_previews
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .remove(&id)
                {
                    entry.cancel.cancel();
                }
            }
        });
        tokio::spawn(async move {
            let cancel = entry.cancel.clone();
            let stopped = runtime.stopped.clone();
            let work = build(
                context.path,
                context.git_metadata_sha256,
                runner,
                entry.sources.clone(),
                cancel.clone(),
                prepare,
                io_slot,
            );
            let result = tokio::select! {biased;
                _=shutdown.changed()=>Err(AppError::VaultLocked),_=cancel.cancelled()=>Err(AppError::VaultLocked),
                _=stopped.cancelled()=>Err(AppError::VaultLocked),
                result=tokio::time::timeout(Duration::from_secs(300),work)=>result.unwrap_or_else(|_|Err(AppError::invalid("inventory_preview","preview timed out"))),
            };
            if result.is_err() {
                entry.cancel.cancel();
            }
            if runtime.unlocked_generation().ok() != Some(generation) || !entry.usable(generation) {
                return;
            }
            let entries = runtime
                .inventory_previews
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            if !entries
                .get(&id)
                .is_some_and(|current| Arc::ptr_eq(current, &entry))
            {
                return;
            }
            *entry.state.lock().unwrap_or_else(|e| e.into_inner()) = match result {
                Ok(value) => PreviewState::Ready(Arc::new(value)),
                Err(error) => PreviewState::Failed(match error {
                    AppError::InvalidInput { field, reason }
                        if field == "automation_runner"
                            && reason.len() <= 80
                            && reason.bytes().all(|b| {
                                b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_'
                            }) =>
                    {
                        reason
                    }
                    _ => "inventory_preview_failed".into(),
                }),
            };
        });
        Ok(response)
    }

    pub async fn automation_inventory_page(
        &self,
        id: String,
        query: String,
        offset: u32,
        limit: u16,
    ) -> AppResult<Value> {
        let id = Uuid::parse_str(&id)
            .map_err(|_| AppError::invalid("inventory_preview", "invalid preview ID"))?;
        if query.len() > 256
            || query.chars().any(char::is_control)
            || !(1..=50).contains(&limit)
            || offset > 10000
        {
            return Err(AppError::invalid(
                "inventory_preview",
                "invalid inventory page",
            ));
        }
        let (owner, unlocked) = self.unlocked().await?;
        let generation = owner.automation.unlocked_generation()?;
        trim(&owner.automation, generation);
        let entry = owner
            .automation
            .inventory_previews
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(&id)
            .cloned()
            .ok_or_else(|| {
                AppError::invalid("inventory_preview", "preview expired or was closed")
            })?;
        let original = entry.unlock.upgrade().ok_or(AppError::VaultLocked)?;
        if !Arc::ptr_eq(&original, &unlocked) {
            return Err(AppError::VaultLocked);
        }
        let _profile = self.hold_session(&owner).await?;
        let _epoch = owner.hold_unlocked(&original).await?;
        if !entry.usable(owner.automation.unlocked_generation()?) {
            return Err(AppError::VaultLocked);
        }
        let mut value = entry.metadata(id);
        let state = entry.state.lock().unwrap_or_else(|e| e.into_inner());
        if let PreviewState::Ready(ready) = &*state {
            let query = query.trim().to_lowercase();
            let matching = ready
                .captured
                .inventory
                .hosts
                .iter()
                .filter(|host| {
                    host.alias.to_ascii_lowercase().contains(&query)
                        || host.address.to_lowercase().contains(&query)
                        || host
                            .username
                            .as_deref()
                            .unwrap_or_default()
                            .to_ascii_lowercase()
                            .contains(&query)
                })
                .collect::<Vec<_>>();
            if offset as usize > matching.len() {
                return Err(AppError::invalid(
                    "inventory_preview",
                    "page offset exceeds the result",
                ));
            }
            let rows = matching
                .iter()
                .skip(offset as usize)
                .take(limit as usize)
                .map(|host| {
                    json!({"alias":host.alias,"address":host.address,
                "port":host.port,"username":host.username,"sources":host.sources})
                })
                .collect::<Vec<_>>();
            value["total"] = json!(matching.len());
            value["cursor"] = json!(offset as usize + rows.len());
            value["hosts"] = json!(rows);
        }
        Ok(value)
    }
    pub async fn automation_inventory_discard(&self, id: String) -> AppResult<()> {
        let id = Uuid::parse_str(&id)
            .map_err(|_| AppError::invalid("inventory_preview", "invalid preview ID"))?;
        let (owner, _) = self.unlocked().await?;
        if let Some(entry) = owner
            .automation
            .inventory_previews
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(&id)
        {
            entry.cancel.cancel();
        }
        Ok(())
    }
}

async fn build(
    path: PathBuf,
    expected_git: Option<String>,
    runner: Arc<RunnerConnection>,
    sources: Vec<InventorySource>,
    cancel: CancellationToken,
    prepare: OwnedSemaphorePermit,
    io_slot: OwnedSemaphorePermit,
) -> AppResult<ReadyInventory> {
    let handle = tokio::runtime::Handle::current();
    tokio::task::spawn_blocking(move||{
        let _prepare=prepare;let _io_slot=io_slot;
        if cancel.is_cancelled(){return Err(AppError::VaultLocked);}
        let root=Arc::new(ProjectRoot::open(&path).map_err(automation_error)?);
        if expected_git.is_some()&&root.git_source().map_err(automation_error)?.map(|(_,sha)|sha)!=expected_git {
            return Err(AppError::invalid("inventory_preview","Git source metadata changed"));
        }
        let revision=root.revision().map_err(automation_error)?;
        handle.block_on(async move {
            let uploaded=tokio::select! {biased;_=cancel.cancelled()=>return Err(AppError::VaultLocked),
                result=root.upload(&revision,&runner.peer)=>result.map_err(automation_error)?,};
            let snapshot=RevisionLease::new(uploaded,&runner.peer);
            let captured=CapturedInventory::fetch(&runner.peer,&snapshot.uploaded,&revision,&sources,&cancel).await.map_err(automation_error)?;
            if cancel.is_cancelled(){return Err(AppError::VaultLocked);}
            Ok(ReadyInventory {revision,captured,_snapshot:snapshot})
        })
    }).await.map_err(|_|AppError::invalid("inventory_preview","filesystem worker stopped"))?
}

#[cfg(test)]
mod tests {
    use super::*;
    const PASSWORD: &str = "correct horse inventory scope fixture 42";
    // Synthetic lifecycle only. Real normalization/transfer is exercised by
    // inventory_project_smoke against the owned WSL helper.
    async fn fixture_entry(
        owner: &Arc<crate::session::Session>,
        unlock: &Arc<Unlocked>,
    ) -> Arc<InventoryPreview> {
        Arc::new(InventoryPreview {
            runner_id: Uuid::new_v4().to_string(),
            project_id: Uuid::new_v4().to_string(),
            project_revision: 1,
            generation: owner.automation.unlocked_generation().unwrap(),
            unlock: Arc::downgrade(unlock),
            sources: vec![InventorySource {
                path: "inventory.ini".into(),
                format: cc_automation_core::InventoryFormat::Ini,
            }],
            expires_at: chrono::Utc::now().timestamp() + 600,
            deadline: Instant::now() + Duration::from_secs(600),
            cancel: CancellationToken::new(),
            state: Mutex::new(PreviewState::Preparing),
            _slot: owner
                .automation
                .inventory_slots
                .clone()
                .acquire_owned()
                .await
                .unwrap(),
        })
    }
    #[tokio::test]
    async fn inventory_previews_are_cancelled_on_lock_and_old_ids_never_reappear() {
        let folder = tempfile::tempdir().unwrap();
        let app = AppCore::new(crate::AppConfig::for_tests(
            folder.path().to_string_lossy().into_owned(),
        ))
        .unwrap();
        app.create_local_profile("Inventory scope".into(), PASSWORD.into())
            .await
            .unwrap();
        let (owner, unlock) = app.unlocked().await.unwrap();
        let entry = fixture_entry(&owner, &unlock).await;
        let id = Uuid::new_v4();
        owner
            .automation
            .inventory_previews
            .lock()
            .unwrap()
            .insert(id, entry.clone());
        assert_eq!(
            app.automation_inventory_page(id.to_string(), String::new(), 0, 50)
                .await
                .unwrap()["state"],
            "preparing"
        );
        drop(unlock);
        app.lock().await.unwrap();
        assert!(entry.cancel.is_cancelled());
        assert!(owner
            .automation
            .inventory_previews
            .lock()
            .unwrap()
            .is_empty());
        assert!(app
            .automation_inventory_page(id.to_string(), String::new(), 0, 50)
            .await
            .is_err());
        app.unlock_with_passphrase(PASSWORD.into()).await.unwrap();
        assert!(app
            .automation_inventory_page(id.to_string(), String::new(), 0, 50)
            .await
            .is_err());
        app.create_local_profile("Another profile".into(), PASSWORD.into())
            .await
            .unwrap();
        assert!(app
            .automation_inventory_page(id.to_string(), String::new(), 0, 50)
            .await
            .is_err());
        app.shutdown().await.unwrap();
    }
    #[tokio::test]
    async fn failed_previews_remain_diagnostic_until_discard_but_expired_ids_are_removed() {
        let folder = tempfile::tempdir().unwrap();
        let app = AppCore::new(crate::AppConfig::for_tests(
            folder.path().to_string_lossy().into_owned(),
        ))
        .unwrap();
        app.create_local_profile("Preview diagnostics".into(), PASSWORD.into())
            .await
            .unwrap();
        let (owner, unlock) = app.unlocked().await.unwrap();
        let entry = fixture_entry(&owner, &unlock).await;
        let id = Uuid::new_v4();
        *entry.state.lock().unwrap() =
            PreviewState::Failed("inventory_inheritance_conflict".into());
        entry.cancel.cancel();
        owner
            .automation
            .inventory_previews
            .lock()
            .unwrap()
            .insert(id, entry.clone());
        assert_eq!(
            app.automation_inventory_page(id.to_string(), String::new(), 0, 50)
                .await
                .unwrap()["state"],
            "failed"
        );
        app.automation_inventory_discard(id.to_string())
            .await
            .unwrap();
        assert!(app
            .automation_inventory_page(id.to_string(), String::new(), 0, 50)
            .await
            .is_err());
        let mut expired = fixture_entry(&owner, &unlock).await;
        Arc::get_mut(&mut expired).unwrap().deadline = Instant::now() - Duration::from_secs(1);
        let expired_id = Uuid::new_v4();
        owner
            .automation
            .inventory_previews
            .lock()
            .unwrap()
            .insert(expired_id, expired.clone());
        assert!(app
            .automation_inventory_page(expired_id.to_string(), String::new(), 0, 50)
            .await
            .is_err());
        assert!(expired.cancel.is_cancelled());
        app.shutdown().await.unwrap();
    }
}
