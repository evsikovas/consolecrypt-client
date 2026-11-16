//! Portable metadata in SAME encrypted VaultSettings. Never files, local roots,
//! Runner launch settings, grants, job state, credential contents or auto-import.
use super::*;
use crate::monitoring_sync::{PanelWritePolicy, SettingsWriteAdmission};
use cc_models::{VaultObject, settings::VaultSettings};
use std::collections::BTreeSet;
const MAX_BYTES: usize = 256 * 1024;
const MAX_RECORDS: usize = 128;
fn invalid() -> AppError {
    AppError::invalid("automation_metadata", "metadata_changed_or_read_only")
}
fn uuid(raw: &str) -> AppResult<Uuid> {
    let id = Uuid::parse_str(raw).map_err(|_| invalid())?;
    if id.is_nil() || id.to_string() != raw {
        return Err(invalid());
    }
    Ok(id)
}
fn bounded(value: &impl Serialize) -> AppResult<()> {
    struct Budget(usize);
    impl std::io::Write for Budget {
        fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
            self.0 = self
                .0
                .checked_add(b.len())
                .filter(|n| *n <= MAX_BYTES)
                .ok_or_else(|| std::io::Error::other("metadata budget"))?;
            Ok(b.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    serde_json::to_writer(Budget(0), value).map_err(|_| invalid())
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Document {
    version: u8,
    revision: u64,
    records: BTreeMap<String, Record>,
}
impl Default for Document {
    fn default() -> Self {
        Self {
            version: 1,
            revision: 0,
            records: BTreeMap::new(),
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct GitReference {
    url: String,
    reference: String,
    commit: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Record {
    version: u8,
    revision: u64,
    name: String,
    source_kind: String,
    revision_digest: String,
    files: usize,
    bytes: u64,
    git: Option<GitReference>,
    inventory: cc_automation_core::ProjectInventory,
    execution_credentials: Option<Value>,
}
fn policy(raw: Option<&Value>) -> AppResult<Option<execution_credentials::Policy>> {
    match raw {
        None => Ok(None),
        Some(value) => {
            execution_credentials::from_row(&json!({"version":2,"execution_credentials":value}))
        }
    }
}
impl Record {
    fn validate(&self) -> AppResult<()> {
        if self.version != 1
            || self.revision == 0
            || self.revision > 9_007_199_254_740_991
            || valid_name(&self.name)? != self.name
            || !cc_models::automation::is_digest(&self.revision_digest)
            || self.files > 10_000
            || self.bytes > 100 * 1024 * 1024
        {
            return Err(invalid());
        }
        self.inventory.validate().map_err(automation_error)?;
        if references(self)?.len() > 512 {
            return Err(invalid());
        }
        match (&self.git, self.source_kind.as_str()) {
            (None, "snapshot") => {}
            (Some(git), "git") => {
                cc_automation_core::GitProjectSource {
                    version: 1,
                    kind: "git".into(),
                    url: git.url.clone(),
                    reference: git.reference.clone(),
                    commit: git.commit.clone(),
                    ca_sha256: None,
                    executable_paths: vec![],
                }
                .validate()
                .map_err(automation_error)?;
            }
            _ => return Err(invalid()),
        }
        policy(self.execution_credentials.as_ref())?;
        bounded(self)
    }
}
fn parse(raw: Option<&Value>) -> AppResult<Document> {
    let Some(raw) = raw else {
        return Ok(Document::default());
    };
    bounded(raw)?;
    let value: Document = serde_json::from_value(raw.clone()).map_err(|_| invalid())?;
    if value.version != 1
        || value.revision > 9_007_199_254_740_991
        || value.records.len() > MAX_RECORDS
        || serde_json::to_value(&value).map_err(|_| invalid())? != *raw
    {
        return Err(invalid());
    }
    for (id, record) in &value.records {
        uuid(id)?;
        record.validate()?;
    }
    Ok(value)
}
fn settings(u: &crate::session::Unlocked) -> AppResult<VaultSettings> {
    u.working()
        .all_vault_settings()
        .into_iter()
        .max_by_key(|s| s.updated_at)
        .ok_or_else(invalid)
}
// Reference metadata only. Missing/foreign IDs never invoke a resolver or grant.
fn references(record: &Record) -> AppResult<BTreeSet<cc_models::ObjectId>> {
    let mut result = BTreeSet::new();
    for id in record
        .inventory
        .groups
        .values()
        .flatten()
        .chain(record.inventory.selected_hosts.iter().flatten())
    {
        result.insert((*id).into());
    }
    if let Some(policy) = policy(record.execution_credentials.as_ref())? {
        for env in policy.environments {
            for item in env.overrides {
                result.insert(crate::dto::parse_id("metadata_host", &item.host_id)?);
                result.insert(crate::dto::parse_id(
                    "metadata_credential",
                    &item.credential_id,
                )?);
            }
        }
    }
    Ok(result)
}
fn view(u: &crate::session::Unlocked, raw: Option<&Value>) -> AppResult<Value> {
    let doc = match parse(raw) {
        Ok(v) => v,
        Err(_) => {
            return Ok(
                json!({"version":1,"read_only":true,"records":[],"files_transferred":false}),
            );
        }
    };
    let mut records = Vec::new();
    for (id, row) in doc.records {
        let unresolved = references(&row)?
            .into_iter()
            .filter(|id| u.working().revision_of(*id).is_none())
            .map(|id| id.to_string())
            .collect::<Vec<_>>();
        records.push(json!({"id":id,"metadata":row,"read_only":true,"unresolved_references":unresolved,"local_root_bound":false,"runnable":false}));
    }
    Ok(
        json!({"version":1,"revision":doc.revision,"read_only":false,"records":records,"files_transferred":false,
        "released_preservation_floor":crate::settings_preservation::RELEASED_SETTINGS_PRESERVATION_FLOOR}),
    )
}
async fn observed(u: &crate::session::Unlocked, current: &VaultSettings) -> AppResult<Value> {
    let mut value = view(u, current.automation.as_ref())?;
    let state = u.store().stored(current.id).await?.ok_or_else(invalid)?;
    if state.local_state == cc_storage_core::LocalState::Conflict {
        value["read_only"] = json!(true);
        value["sync_state"] = json!("reconciliation_required");
        value["reason"] = json!("settings_catalog_conflict_requires_reconciliation");
    }
    Ok(value)
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Publish {
    version: u8,
    profile_id: String,
    notification_scope_id: String,
    project_id: String,
    project_revision: u64,
    settings_id: String,
    settings_revision: i64,
    catalog_revision: u64,
    expected_reference_revision: u64,
}
impl Publish {
    fn validate(&self) -> AppResult<()> {
        uuid(&self.profile_id)?;
        uuid(&self.project_id)?;
        uuid(&self.settings_id)?;
        if self.version != 1
            || self.project_revision == 0
            || self.settings_revision <= 0
            || self.notification_scope_id.is_empty()
            || self.notification_scope_id.len() > 256
            || self.notification_scope_id.chars().any(char::is_control)
        {
            return Err(invalid());
        }
        Ok(())
    }
    fn check(
        &self,
        owner: &crate::session::Session,
        u: &crate::session::Unlocked,
        s: &VaultSettings,
    ) -> AppResult<()> {
        self.validate()?;
        if self.profile_id != owner.profile_id.to_string()
            || u.notices.scope_id().as_deref() != Some(self.notification_scope_id.as_str())
            || self.settings_id != s.id.to_string()
            || u.working().revision_of(s.id) != Some(self.settings_revision)
            || parse(s.automation.as_ref())?.revision != self.catalog_revision
        {
            return Err(invalid());
        }
        Ok(())
    }
}
fn check_project_tx(
    tx: &cc_storage_core::Tx<'_>,
    project: &str,
    expected: &str,
) -> Result<(), cc_sync_core::SyncError> {
    let catalog: Catalog = tx.setting_get(PROJECTS)?.unwrap_or_default();
    let changed = || cc_sync_core::SyncError::Invalid("automation_metadata_project_changed".into());
    let row = catalog.records.get(project).ok_or_else(changed)?;
    if catalog.version != 1
        || !catalog.extra.is_empty()
        || managed::row_stamp(row).map_err(|_| changed())? != expected
    {
        return Err(changed());
    }
    Ok(())
}
fn finish_read(
    auth: &crate::agent_reader::OriginalSessionAuth,
    u: &crate::session::Unlocked,
    mutation: u64,
    until: std::time::Instant,
    value: Value,
) -> AppResult<Value> {
    auth.try_with_current(|| {
        u.working()
            .tunnel_io_at_version(mutation, || {
                if std::time::Instant::now() >= until || *u.sharing_shutdown.borrow() {
                    return Err(invalid());
                }
                // Metadata is already a closed immutable value, not a live authority.
                // Bound its encoding while original auth/version guards are held.
                struct Budget(usize);
                impl std::io::Write for Budget {
                    fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
                        self.0 = self
                            .0
                            .checked_add(b.len())
                            .filter(|n| *n <= 512 * 1024)
                            .ok_or_else(|| std::io::Error::other("metadata response budget"))?;
                        Ok(b.len())
                    }
                    fn flush(&mut self) -> std::io::Result<()> {
                        Ok(())
                    }
                }
                serde_json::to_writer(Budget(0), &value).map_err(|_| invalid())?;
                Ok(value)
            })
            .ok_or_else(invalid)?
    })
}
impl AppCore {
    /// Read-only references. Remote records never become device-local projects.
    pub async fn automation_project_metadata_catalog(&self) -> AppResult<Value> {
        let auth = crate::agent_reader::OriginalSessionAuth::capture(self)?;
        let until = std::time::Instant::now() + std::time::Duration::from_secs(30);
        let (owner, u) = tokio::select! {biased;_=auth.cancelled()=>return Err(invalid()),_=tokio::time::sleep_until(until.into())=>return Err(invalid()),result=self.unlocked()=>result?,};
        let mut shutdown = u.sharing_shutdown.subscribe();
        let work = async {
            let _profile = self.hold_session(&owner).await?;
            let _epoch = owner.hold_unlocked(&u).await?;
            let mutation = u.working().mutation_stamp();
            let current = settings(&u)?;
            let catalog = observed(&u, &current).await?;
            let value = json!({"version":1,"profile_id":owner.profile_id,"notification_scope_id":u.notices.scope_id().ok_or(AppError::VaultLocked)?,"catalog":catalog});
            finish_read(&auth, &u, mutation, until, value)
        };
        tokio::select! {biased;_=auth.cancelled()=>Err(invalid()),_=shutdown.changed()=>Err(AppError::VaultLocked),_=tokio::time::sleep_until(until.into())=>Err(invalid()),result=work=>result,}
    }
    /// Original device-local project plus encrypted reference catalog observation.
    /// Opening it never opts in, pulls, binds a root or creates a job.
    pub async fn automation_project_metadata_context(
        &self,
        project_id: String,
    ) -> AppResult<Value> {
        let auth = crate::agent_reader::OriginalSessionAuth::capture(self)?;
        let until = std::time::Instant::now() + std::time::Duration::from_secs(30);
        let (owner, u) = tokio::select! {biased;_=auth.cancelled()=>return Err(invalid()),_=tokio::time::sleep_until(until.into())=>return Err(invalid()),result=self.unlocked()=>result?,};
        let mut shutdown = u.sharing_shutdown.subscribe();
        let weak = Arc::downgrade(&u);
        drop(u);
        let work = async {
            let context = self
                .automation_project_context_original(&project_id, owner, weak)
                .await?;
            let u = context.unlock.upgrade().ok_or(AppError::VaultLocked)?;
            let _profile = self.hold_session(&context.owner).await?;
            let _epoch = context.owner.hold_unlocked(&u).await?;
            let mutation = u.working().mutation_stamp();
            let current = settings(&u)?;
            let catalog = observed(&u, &current).await?;
            let reconciliation = catalog.get("sync_state").and_then(Value::as_str)
                == Some("reconciliation_required");
            let kind = context.owner.kind().await?;
            let value = json!({"version":1,"frame":{"version":1,"profile_id":context.owner.profile_id,"notification_scope_id":u.notices.scope_id().ok_or(AppError::VaultLocked)?,
                "project_id":project_id,"project_revision":context.project.revision,"settings_id":current.id,"settings_revision":u.working().revision_of(current.id).ok_or_else(invalid)?,
                "catalog_revision":parse(current.automation.as_ref()).ok().map(|v|v.revision)},"catalog":catalog,
                "sync_state":if reconciliation{"reconciliation_required"}else if kind==cc_storage_core::ProfileKind::Local{"local_only"}else if crate::settings_preservation::RELEASED_SETTINGS_PRESERVATION_FLOOR.is_none(){"floor_undeclared"}else{"explicit_admission_required"}});
            finish_read(&auth, &u, mutation, until, value)
        };
        tokio::select! {biased;_=auth.cancelled()=>Err(invalid()),_=shutdown.changed()=>Err(AppError::VaultLocked),_=tokio::time::sleep_until(until.into())=>Err(invalid()),result=work=>result,}
    }
    pub async fn automation_project_metadata_publish(&self, input: String) -> AppResult<Value> {
        self.automation_project_metadata_publish_with_policy(input, PanelWritePolicy::Released)
            .await
    }
    pub(crate) async fn automation_project_metadata_publish_with_policy(
        &self,
        input: String,
        choice: PanelWritePolicy,
    ) -> AppResult<Value> {
        if input.len() > 2048 {
            return Err(invalid());
        }
        let request: Publish = serde_json::from_str(&input).map_err(|_| invalid())?;
        request.validate()?;
        let auth = crate::agent_reader::OriginalSessionAuth::capture(self)?;
        let until = std::time::Instant::now() + std::time::Duration::from_secs(30);
        let (owner, u) = tokio::select! {biased;
            _=auth.cancelled()=>return Err(invalid()),
            _=tokio::time::sleep_until(until.into())=>return Err(invalid()),
            result=self.unlocked()=>result?,
        };
        let mut shutdown = u.sharing_shutdown.subscribe();
        let work = async {
            let current = settings(&u)?;
            request.check(&owner, &u, &current)?;
            if owner.kind().await? != cc_storage_core::ProfileKind::Synced {
                return Err(AppError::LocalProfile);
            }
            // Declared floor/account opt-in/cursor/head are checked BEFORE any file I/O.
            let admission = SettingsWriteAdmission::prepare_settings(
                self,
                owner,
                u,
                request.settings_revision,
                choice,
                auth.clone(),
                until,
            )
            .await?;
            let (owner, u) = admission.original(self).await?;
            request.check(&owner, &u, &settings(&u)?)?;
            let weak = Arc::downgrade(&u);
            drop(u);
            let context = self
                .automation_project_context_original(&request.project_id, owner, weak)
                .await?;
            if context.project.revision != Some(request.project_revision) {
                return Err(invalid());
            }
            let retained = managed::admit_io(self, &context).await?;
            let io = retained.clone();
            let selected = context.root_selection.clone();
            let revision = tokio::task::spawn_blocking(move || -> AppResult<_> {
                let _io = io;
                let root = selected.open()?;
                let revision = root.revision().map_err(automation_error)?;
                root.recheck()?;
                Ok(revision)
            })
            .await
            .map_err(|_| invalid())??;
            context.check_after_io(self).await?;
            drop(retained);
            let (owner, u) = admission.original(self).await?;
            let current = settings(&u)?;
            request.check(&owner, &u, &current)?;
            let local: Catalog = owner
                .storage()?
                .setting_get(PROJECTS)
                .await?
                .unwrap_or_default();
            let row = local.records.get(&request.project_id).ok_or_else(invalid)?;
            if local.version != 1
                || !local.extra.is_empty()
                || managed::row_stamp(row)? != context.catalog_row_sha256
            {
                return Err(invalid());
            }
            let mut catalog = parse(current.automation.as_ref())?;
            let previous = catalog
                .records
                .get(&request.project_id)
                .map_or(0, |v| v.revision);
            if previous != request.expected_reference_revision
                || previous >= 9_007_199_254_740_991
                || catalog.revision >= 9_007_199_254_740_991
                || previous == 0 && catalog.records.len() >= MAX_RECORDS
            {
                return Err(invalid());
            }
            let git = if context.project.source_kind.as_deref() == Some("git") {
                let source = context.project.git_source.as_ref().ok_or_else(invalid)?;
                Some(GitReference {
                    url: source["url"].as_str().ok_or_else(invalid)?.into(),
                    reference: source["reference"].as_str().ok_or_else(invalid)?.into(),
                    commit: source["commit"].as_str().ok_or_else(invalid)?.into(),
                })
            } else {
                None
            };
            let record = Record {
                version: 1,
                revision: previous + 1,
                name: context.project.name.clone().ok_or_else(invalid)?,
                source_kind: if git.is_some() { "git" } else { "snapshot" }.into(),
                revision_digest: revision.digest,
                files: revision
                    .entries
                    .iter()
                    .filter(|v| matches!(v, cc_automation_core::ProjectEntry::File { .. }))
                    .count(),
                bytes: revision.bytes,
                git,
                inventory: context.inventory.clone(),
                execution_credentials: execution_credentials::from_row(row)?
                    .map(serde_json::to_value)
                    .transpose()
                    .map_err(|_| invalid())?,
            };
            record.validate()?;
            catalog.records.insert(request.project_id, record);
            catalog.revision += 1;
            bounded(&catalog)?;
            let raw = serde_json::to_value(catalog).map_err(|_| invalid())?;
            let mut next = current;
            next.automation = Some(raw.clone());
            next.updated_at = chrono::Utc::now();
            // No root/settings/credential sidecar is written. SAME encrypted object +
            // outbox dispatch remains guarded by original auth/epoch/cursor/head CAS.
            let original_project = context.project.id.clone();
            let published_project = original_project.clone();
            let published_profile = context.owner.profile_id;
            let published_scope = request.notification_scope_id.clone();
            let original_row = context.catalog_row_sha256.clone();
            drop(u);
            drop(owner);
            admission
                .commit_checked(
                    self,
                    VaultObject::VaultSettings(next),
                    move |tx| check_project_tx(tx, &original_project, &original_row),
                    move |u| {
                        if settings(u)?.automation.as_ref() != Some(&raw) {
                            return Err(invalid());
                        }
                        Ok(json!({"version":1,"profile_id":published_profile,"notification_scope_id":published_scope,"project_id":published_project,"catalog":view(u,Some(&raw))?}))
                    },
                )
                .await
        };
        tokio::select! {biased;
            _=auth.cancelled()=>Err(invalid()),
            _=shutdown.changed()=>Err(AppError::VaultLocked),
            _=tokio::time::sleep_until(until.into())=>Err(invalid()),
            result=work=>result,
        }
    }
}
#[cfg(test)]
mod tests;
