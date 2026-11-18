use super::*;
use tokio_util::sync::CancellationToken;

const HISTORY: &str = "automation.project-versions.v1";
const MAX_VERSIONS_PER_PROJECT: usize = 24;

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Version {
    id: String,
    name: String,
    created_at_ms: i64,
    baseline: baseline::Baseline,
}

impl Version {
    fn metadata(&self) -> Value {
        json!({"id":self.id,"name":self.name,"created_at_ms":self.created_at_ms,
               "digest":self.baseline.digest,"bytes":self.baseline.bytes})
    }
}

type History = BTreeMap<String, Vec<Version>>;

fn unavailable() -> AppError {
    AppError::invalid(
        "automation_versions",
        "saved project version is unavailable",
    )
}

impl AppCore {
    pub async fn automation_project_versions(&self, id: String) -> AppResult<Value> {
        let context = self.automation_project_context(&id).await?;
        let unlock = context.unlock.upgrade().ok_or(AppError::VaultLocked)?;
        let _profile = self.hold_session(&context.owner).await?;
        let _unlock = context.owner.hold_unlocked(&unlock).await?;
        context.check_catalog_held().await?;
        let history: History = context
            .owner
            .storage()?
            .setting_get(HISTORY)
            .await?
            .unwrap_or_default();
        let rows: Vec<_> = history
            .get(&id)
            .into_iter()
            .flatten()
            .rev()
            .map(Version::metadata)
            .collect();
        Ok(json!(rows))
    }

    pub async fn automation_project_save_version(
        &self,
        id: String,
        digest: String,
        name: String,
    ) -> AppResult<()> {
        let name = valid_name(&name)?;
        if !cc_models::automation::is_digest(&digest) {
            return Err(unavailable());
        }
        let context = self.automation_project_context(&id).await?;
        let owner = &context.owner;
        let profile = owner.dir.as_ref().ok_or_else(unavailable)?;
        let retained_io = managed::admit_io(self, &context).await?;
        let baseline_slot = owner
            .automation
            .baseline_slots
            .clone()
            .try_acquire_owned()
            .map_err(|_| unavailable())?;
        let current: History = owner
            .storage()?
            .setting_get(HISTORY)
            .await?
            .unwrap_or_default();
        if current
            .get(&id)
            .is_some_and(|rows| rows.len() >= MAX_VERSIONS_PER_PROJECT)
        {
            return Err(AppError::invalid(
                "automation_versions",
                "project version limit reached",
            ));
        }
        let cancel = cancel_on_lock(&context)?;
        let _done = cancel.clone().drop_guard();
        let source = context.root_selection.clone();
        let path = baseline::base(profile);
        let io_slot = retained_io.clone();
        let captured = tokio::task::spawn_blocking(move || -> AppResult<_> {
            let _slots = (baseline_slot, io_slot);
            let root = source.open()?;
            let revision = root.revision().map_err(automation_error)?;
            if revision.digest != digest {
                return Err(automation_error(
                    cc_automation_core::AutomationError::FileChanged,
                ));
            }
            let captured = baseline::capture(&path, &root, &revision, &cancel)?;
            root.recheck()?;
            Ok(captured)
        })
        .await
        .map_err(|_| AppError::internal("project version worker stopped"))??;

        let unlock = context.unlock.upgrade().ok_or(AppError::VaultLocked)?;
        let _profile = self.hold_session(owner).await?;
        let _unlock = owner.hold_unlocked(&unlock).await?;
        context.check_catalog_held().await?;
        owner
            .storage()?
            .write(move |tx| {
                let mut history: History = tx.setting_get(HISTORY)?.unwrap_or_default();
                let rows = history.entry(id).or_default();
                if rows.len() >= MAX_VERSIONS_PER_PROJECT {
                    return Err(AppError::invalid(
                        "automation_versions",
                        "project version limit reached",
                    ));
                }
                rows.push(Version {
                    id: Uuid::new_v4().to_string(),
                    name,
                    created_at_ms: chrono::Utc::now().timestamp_millis(),
                    baseline: captured,
                });
                tx.setting_set(HISTORY, &history)?;
                Ok::<_, AppError>(())
            })
            .await
    }

    pub async fn automation_project_open_version(
        &self,
        id: String,
        version_id: String,
        name: String,
    ) -> AppResult<AutomationProjectSummary> {
        let name = valid_name(&name)?;
        project_id(&version_id)?;
        let context = self.automation_project_context(&id).await?;
        let owner = &context.owner;
        let profile = owner.dir.clone().ok_or_else(unavailable)?;
        let retained_io = managed::admit_io(self, &context).await?;
        let history: History = owner
            .storage()?
            .setting_get(HISTORY)
            .await?
            .unwrap_or_default();
        let version = history
            .get(&id)
            .and_then(|rows| rows.iter().find(|row| row.id == version_id))
            .cloned()
            .ok_or_else(unavailable)?;
        let cancel = cancel_on_lock(&context)?;
        let _done = cancel.clone().drop_guard();
        let new_id = Uuid::new_v4().to_string();
        let project_parent = profile.join("automation").join("projects");
        let folder_name = new_id.clone();
        let io_slot = retained_io.clone();
        let folder = tokio::task::spawn_blocking(move || -> AppResult<_> {
            let _io_slot = io_slot;
            let (source, expected) = baseline::open_verified(&profile, &version.baseline)?;
            std::fs::create_dir_all(&project_parent)?;
            if std::fs::symlink_metadata(&project_parent)?
                .file_type()
                .is_symlink()
            {
                return Err(unavailable());
            }
            let folder = tempfile::Builder::new()
                .prefix(&folder_name)
                .rand_bytes(0)
                .tempdir_in(&project_parent)?;
            let target = ProjectRoot::open(folder.path()).map_err(automation_error)?;
            source
                .freeze_to(&expected, &target, &cancel)
                .map_err(automation_error)?;
            Ok(folder)
        })
        .await
        .map_err(|_| AppError::internal("project version copy worker stopped"))??;
        let unlock = context.unlock.upgrade().ok_or(AppError::VaultLocked)?;
        let _profile = self.hold_session(owner).await?;
        let _unlock = owner.hold_unlocked(&unlock).await?;
        context.check_catalog_held().await?;
        let now = chrono::Utc::now().timestamp();
        let row = json!({"version":1,"name":name,"source":{"kind":"managed","path":folder.path()},
                          "revision":1,"created_at":now,"updated_at":now});
        let result = summary(&new_id, &row, false);
        owner
            .storage()?
            .write(move |tx| {
                let mut catalog: Catalog = tx.setting_get(PROJECTS)?.unwrap_or_default();
                if catalog.version != 1 || catalog.records.len() >= 256 {
                    return Err(unavailable());
                }
                catalog.records.insert(new_id, row);
                tx.setting_set(PROJECTS, &catalog)?;
                // Like baseline capture, publish only a fully copied tree.
                let _path = folder.keep();
                Ok::<_, AppError>(())
            })
            .await?;
        Ok(result)
    }
}

fn cancel_on_lock(context: &AutomationProjectContext) -> AppResult<CancellationToken> {
    let unlock = context.unlock.upgrade().ok_or(AppError::VaultLocked)?;
    let mut shutdown = unlock.sharing_shutdown.subscribe();
    if *shutdown.borrow() {
        return Err(AppError::VaultLocked);
    }
    let cancel = CancellationToken::new();
    let operation = cancel.clone();
    tokio::spawn(async move {
        tokio::select! {
            _ = shutdown.changed() => operation.cancel(),
            _ = operation.cancelled() => {},
        }
    });
    Ok(cancel)
}
