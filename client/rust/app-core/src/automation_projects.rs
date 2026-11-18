//! Device-local project metadata in this profile's SQLCipher database. Ordinary
//! playbooks remain files. No new object kind is sent to an older sync server.
use crate::{AppCore, AppError, AppResult};
use cc_automation_core::{ProjectFile, ProjectRevision, ProjectRoot};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    collections::BTreeMap,
    path::PathBuf,
    sync::{Arc, Weak},
};
use uuid::Uuid;

const PROJECTS: &str = "automation.projects.v1";
mod git;
mod baseline;
mod versions;
mod diff;
mod editor;
mod groups;
pub(crate) mod managed;
pub(crate) mod folder_bindings;
mod export_fence;
pub(crate) mod portable;
pub(crate) mod vault;
mod vault_file;
pub(crate) mod dependencies;
pub(crate) mod service_delegation;
pub(crate) mod execution_credentials;
mod metadata_sync;
mod source_bindings;
mod source_preview;
pub(crate) use source_preview::InventoryPreview;

pub(crate) struct GitImportActive {
    pub runner_id: String,
    pub cancel: tokio_util::sync::CancellationToken,
}

pub(crate) fn automation_io_slot(
    owner: &crate::session::Session,
) -> AppResult<tokio::sync::OwnedSemaphorePermit> {
    owner
        .automation
        .io_slots
        .clone()
        .try_acquire_owned()
        .map_err(|_| AppError::invalid("automation_io", "filesystem worker limit reached"))
}

#[derive(Clone)]
pub(crate) struct AutomationProjectContext {
    pub owner: Arc<crate::session::Session>,
    pub unlock: Weak<crate::session::Unlocked>,
    pub path: PathBuf,
    pub catalog_row_sha256: String,
    pub root_selection: managed::RootSelection,
    pub project: AutomationProjectSummary,
    pub git_metadata_sha256: Option<String>,
    pub inventory: cc_automation_core::ProjectInventory,
    pub external_inventory: Option<cc_automation_core::ExternalInventoryConfig>,
    pub baseline: Option<baseline::Baseline>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
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

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AutomationProjectSummary {
    pub id: String,
    pub name: Option<String>,
    pub source_kind: Option<String>,
    pub path: Option<String>,
    pub revision: Option<u64>,
    pub read_only: bool,
    pub import_state: Option<String>,
    pub import_error: Option<String>,
    pub git_source: Option<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AutomationProjectList {
    pub schema_version: u32,
    pub read_only: bool,
    pub device_local: bool,
    pub projects: Vec<AutomationProjectSummary>,
}

pub(crate) fn automation_error(error: cc_automation_core::AutomationError) -> AppError {
    match error {
        cc_automation_core::AutomationError::FileChanged => {
            AppError::invalid("automation_file", "changed")
        }
        cc_automation_core::AutomationError::Remote(code) => {
            AppError::invalid("automation_runner", code)
        }
        cc_automation_core::AutomationError::Closed => {
            AppError::invalid("automation_runner", "helper_unavailable")
        }
        cc_automation_core::AutomationError::Io(error) => AppError::from(error),
        other => AppError::invalid("automation", other.to_string()),
    }
}

fn project_id(id: &str) -> AppResult<()> {
    if Uuid::parse_str(id)
        .ok()
        .is_none_or(|parsed| parsed.to_string() != id)
    {
        return Err(AppError::invalid(
            "project_id",
            "invalid project identifier",
        ));
    }
    Ok(())
}

fn valid_name(name: &str) -> AppResult<String> {
    let name = name.trim();
    if name.is_empty() || name.chars().count() > 120 || name.chars().any(char::is_control) {
        return Err(AppError::invalid(
            "project_name",
            "use a nonempty name up to 120 characters",
        ));
    }
    Ok(name.into())
}

fn supported(row: &Value) -> bool {
    managed::marker(row).is_ok()
        && portable::import_catalog::ordinary_supported(row)
        && groups::from_row(row).is_ok()
        && baseline::from_row(row).is_ok()
        && source_bindings::supported_marker(row)
        && execution_credentials::supported_row_version(row)
        && matches!(
            row.pointer("/source/kind").and_then(Value::as_str),
            Some("directory" | "managed" | "git")
        )
        && row
            .pointer("/source/path")
            .and_then(Value::as_str)
            .is_some()
        && row
            .get("revision")
            .and_then(Value::as_u64)
            .is_some_and(|version| version > 0)
        && (row.pointer("/source/kind").and_then(Value::as_str) != Some("git")
            || row.pointer("/source/status").and_then(Value::as_str) == Some("ready")
                && row
                    .pointer("/source/git/metadata_sha256")
                    .and_then(Value::as_str)
                    .is_some_and(cc_models::automation::is_digest))
}

fn summary(id: &str, row: &Value, read_only: bool) -> AutomationProjectSummary {
    AutomationProjectSummary {
        id: id.into(),
        name: row.get("name").and_then(Value::as_str).map(str::to_owned),
        source_kind: row
            .pointer("/source/kind")
            .and_then(Value::as_str)
            .map(str::to_owned),
        path: row
            .pointer("/source/path")
            .and_then(Value::as_str)
            .map(str::to_owned),
        revision: row.get("revision").and_then(Value::as_u64),
        read_only: read_only || !supported(row),
        import_state: row
            .pointer("/source/status")
            .and_then(Value::as_str)
            .map(str::to_owned),
        import_error: row
            .pointer("/source/error")
            .and_then(Value::as_str)
            .map(str::to_owned),
        git_source: row.pointer("/source/git").cloned(),
    }
}

impl AppCore {
    pub async fn automation_projects(&self) -> AppResult<AutomationProjectList> {
        let (owner, unlocked) = self.unlocked().await?;
        self.automation_recover_git_imports(&owner).await?;
        let document: Catalog = owner
            .storage()?
            .setting_get(PROJECTS)
            .await?
            .unwrap_or_default();
        let _profile = self.hold_session(&owner).await?;
        let _generation = owner.hold_unlocked(&unlocked).await?;
        Ok(AutomationProjectList {
            schema_version: document.version,
            read_only: document.version != 1,
            device_local: true,
            projects: document
                .records
                .iter()
                .map(|(id, row)| summary(id, row, document.version != 1 || managed::validate_path(owner.dir.as_deref(), id, row).is_err()))
                .collect(),
        })
    }

    /// A missing directory means a new managed workspace. An existing directory
    /// must be explicitly selected by the user; model proposals cannot choose it.
    pub async fn automation_project_create(
        &self,
        name: String,
        directory: Option<String>,
    ) -> AppResult<AutomationProjectSummary> {
        self.create_automation_project(name, directory, false, None).await
    }

    /// Initialize a new project in a managed or explicitly selected empty folder.
    pub async fn automation_project_scaffold(
        &self,
        name: String,
        directory: Option<String>,
        role: Option<String>,
    ) -> AppResult<AutomationProjectSummary> {
        if role.as_deref().is_some_and(|value| {
            value.is_empty() || value.len() > 64 || !value.as_bytes()[0].is_ascii_lowercase()
                || !value.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
        }) {
            return Err(AppError::invalid("role", "use a lowercase role identifier"));
        }
        self.create_automation_project(name, directory, true, role).await
    }

    async fn create_automation_project(
        &self,
        name: String,
        directory: Option<String>,
        initialize_directory: bool,
        role: Option<String>,
    ) -> AppResult<AutomationProjectSummary> {
        let name = valid_name(&name)?;
        let (owner, unlocked) = self.unlocked().await?;
        let id = Uuid::new_v4().to_string();
        let (kind, path) = match directory {
            Some(path) => ("directory", PathBuf::from(path)),
            None => {
                let profile = owner.dir.as_ref().ok_or_else(|| {
                    AppError::invalid(
                        "project_source",
                        "select a directory for an in-memory profile",
                    )
                })?;
                let path = profile.join("automation").join("projects").join(&id);
                ("managed", path)
            }
        };
        let project_path = path.clone();
        let managed = kind == "managed";
        let epoch = Arc::downgrade(&unlocked);
        drop(unlocked);
        let io_slot = automation_io_slot(&owner)?;
        tokio::task::spawn_blocking(move || -> AppResult<()> {
            let _io_slot=io_slot;
            if managed { std::fs::create_dir_all(&project_path)?; }
            let project = ProjectRoot::open(&project_path).map_err(automation_error)?;
            let revision = project.revision().map_err(automation_error)?;
            if initialize_directory && !revision.entries.is_empty() {
                return Err(AppError::invalid("project_source", "project_folder_not_empty"));
            }
            if managed || initialize_directory {
                let playbook = if let Some(role) = role {
                    project.create_role(&role).map_err(automation_error)?;
                    format!("---\n- name: Apply project role\n  hosts: all\n  roles:\n    - {role}\n")
                } else {
                    "---\n- name: New automation project\n  hosts: all\n  gather_facts: false\n  tasks:\n    - name: Display a message\n      ansible.builtin.debug:\n        msg: Project is ready for editing\n".to_owned()
                };
                project.write_file("playbook.yml", None, &playbook).map_err(automation_error)?;
            }
            Ok(())
        }).await.map_err(|_| AppError::internal("project source worker stopped"))??;
        let row = json!({"version":1,"name":name,"source":{"kind":kind,"path":path},"revision":1,
            "created_at":chrono::Utc::now().timestamp(),"updated_at":chrono::Utc::now().timestamp()});
        let result = summary(&id, &row, false);
        let unlocked = epoch.upgrade().ok_or(AppError::VaultLocked)?;
        let _profile = self.hold_session(&owner).await?;
        let _generation = owner.hold_unlocked(&unlocked).await?;
        owner
            .storage()?
            .write(move |transaction| {
                let mut document: Catalog = transaction.setting_get(PROJECTS)?.unwrap_or_default();
                if document.version != 1 || document.records.len() >= 256 {
                    return Err(AppError::invalid(
                        "automation_projects",
                        "catalog is read-only or its project limit was reached",
                    ));
                }
                document.records.insert(id, row);
                transaction.setting_set(PROJECTS, &document)?;
                Ok::<_, AppError>(())
            })
            .await?;
        Ok(result)
    }

    pub async fn automation_project_rename(
        &self,
        id: String,
        expected_revision: u64,
        name: String,
    ) -> AppResult<AutomationProjectSummary> {
        project_id(&id)?;
        let name = valid_name(&name)?;
        let (owner, unlocked) = self.unlocked().await?;
        let profile_path = owner.dir.clone();
        let _profile = self.hold_session(&owner).await?;
        let _generation = owner.hold_unlocked(&unlocked).await?;
        owner
            .storage()?
            .write(move |transaction| {
                let mut document: Catalog = transaction.setting_get(PROJECTS)?.unwrap_or_default();
                if document.version != 1 {
                    return Err(AppError::invalid(
                        "automation_projects",
                        "newer catalog is read-only",
                    ));
                }
                let row = document
                    .records
                    .get_mut(&id)
                    .ok_or_else(|| AppError::not_found("automation project", &id))?;
                if !managed::supported_at(profile_path.as_deref(), &id, row)
                    || row.get("revision").and_then(Value::as_u64) != Some(expected_revision)
                    || expected_revision == u64::MAX
                {
                    return Err(AppError::invalid(
                        "automation_project",
                        "project changed or uses a newer schema",
                    ));
                }
                row["name"] = Value::String(name);
                row["revision"] = Value::from(expected_revision + 1);
                row["updated_at"] = Value::from(chrono::Utc::now().timestamp());
                let result = summary(&id, row, false);
                transaction.setting_set(PROJECTS, &document)?;
                Ok::<_, AppError>(result)
            })
            .await
    }

    pub(crate) async fn automation_project_context(
        &self,
        id: &str,
    ) -> AppResult<AutomationProjectContext> {
        let (owner, unlocked) = self.unlocked().await?;
        self.automation_project_context_original(id, owner, Arc::downgrade(&unlocked)).await
    }

    // The existing loader, bound to an issued original owner. Never recaptures
    // a later global Core/session after a portable operation ACK.
    pub(crate) async fn automation_project_context_original(
        &self, id:&str, owner:Arc<crate::session::Session>, unlock:Weak<crate::session::Unlocked>,
    )->AppResult<AutomationProjectContext> {
        project_id(id)?;
        let unlocked=unlock.upgrade().ok_or(AppError::VaultLocked)?;
        {
            let _profile=self.hold_session(&owner).await?;
            let _unlock=owner.hold_unlocked(&unlocked).await?;
        }
        drop(unlocked);
        let document: Catalog = owner
            .storage()?
            .setting_get(PROJECTS)
            .await?
            .unwrap_or_default();
        let row = document
            .records
            .get(id)
            .ok_or_else(|| AppError::not_found("automation project", id))?;
        if document.version != 1 || !supported(row) {
            return Err(AppError::invalid(
                "automation_project",
                "unsupported project schema",
            ));
        }
        managed::validate_path(owner.dir.as_deref(), id, row)?;
        let catalog_row_sha256 = managed::row_stamp(row)?;
        let root_selection = managed::RootSelection::capture(owner.dir.as_deref(), id, row)?;
        let path = PathBuf::from(
            row.pointer("/source/path")
                .and_then(Value::as_str)
                .unwrap_or_default(),
        );
        let project = summary(id, row, false);
        let inventory = groups::from_row(row)?;
        let baseline = baseline::from_row(row)?;
        let external_inventory = source_bindings::load(&owner, id, row).await?;
        let git_metadata_sha256 = row
            .pointer("/source/git/metadata_sha256")
            .and_then(Value::as_str)
            .map(str::to_owned);
        let unlocked=unlock.upgrade().ok_or(AppError::VaultLocked)?;
        let _profile = self.hold_session(&owner).await?;
        let _generation = owner.hold_unlocked(&unlocked).await?;
        drop(_generation);
        drop(_profile);
        Ok(AutomationProjectContext {
            owner,
            unlock,
            path,
            catalog_row_sha256,
            root_selection,
            project,
            git_metadata_sha256,
            inventory,
            external_inventory,
            baseline,
        })
    }

    pub async fn automation_project_files(&self, id: String) -> AppResult<ProjectRevision> {
        let context = self.automation_project_context(&id).await?;
        let retained_io = managed::admit_io(self, &context).await?;
        let io_slot = retained_io.clone();
        let source = context.root_selection.clone();
        let result = tokio::task::spawn_blocking(move || -> AppResult<_> {
            let _io_slot = io_slot;
            let root = source.open()?;
            let value = root.revision().map_err(automation_error)?;
            root.recheck()?;
            Ok(value)
        }).await.map_err(|_|AppError::internal("project scanner stopped"))??;
        context.check_after_io(self).await?;
        drop(retained_io);
        Ok(result)
    }

    pub async fn automation_project_file(
        &self,
        id: String,
        path: String,
    ) -> AppResult<ProjectFile> {
        let context = self.automation_project_context(&id).await?;
        let retained_io = managed::admit_io(self, &context).await?;
        let io_slot = retained_io.clone();
        let source = context.root_selection.clone();
        let result = tokio::task::spawn_blocking(move || -> AppResult<_> {
            let _io_slot = io_slot;
            let root = source.open()?;
            let value = root.read_file(&path).map_err(automation_error)?;
            root.recheck()?;
            Ok(value)
        }).await.map_err(|_|AppError::internal("project reader stopped"))??;
        context.check_after_io(self).await?;
        drop(retained_io);
        Ok(result)
    }

    pub async fn automation_project_save(
        &self,
        id: String,
        path: String,
        expected_sha256: Option<String>,
        text: String,
    ) -> AppResult<ProjectFile> {
        let context = self.automation_project_context(&id).await?;
        // This one-file capability has already been authorized. Never hold a
        // vault transition guard across disk I/O: even a stalled filesystem
        // must not block locking. The result is fenced to the original profile.
        let retained_io = managed::admit_io(self, &context).await?;
        let io_slot = retained_io.clone();
        let source = context.root_selection.clone();
        let result = tokio::task::spawn_blocking(move || -> AppResult<_> {
            let _io_slot = io_slot;
            let root = source.open()?;
            let value = root.write_file(&path, expected_sha256.as_deref(), &text).map_err(automation_error)?;
            root.recheck()?;
            Ok(value)
        }).await.map_err(|_|AppError::internal("project editor stopped"))??;
        context.check_after_io(self).await?;
        drop(retained_io);
        Ok(result)
    }

    pub async fn automation_project_move_file(
        &self,
        id: String,
        path: String,
        directory: String,
        expected_sha256: String,
    ) -> AppResult<()> {
        let context = self.automation_project_context(&id).await?;
        let retained_io = managed::admit_io(self, &context).await?;
        let io_slot = retained_io.clone();
        let source = context.root_selection.clone();
        tokio::task::spawn_blocking(move || -> AppResult<()> {
            let _io_slot = io_slot;
            let root = source.open()?;
            root.recheck()?;
            root.move_file(&path, &directory, &expected_sha256).map_err(automation_error)?;
            root.recheck()?;
            Ok(())
        }).await.map_err(|_| AppError::internal("project move worker stopped"))??;
        context.check_after_io(self).await?;
        drop(retained_io);
        Ok(())
    }

    pub async fn automation_project_create_role(
        &self,
        id: String,
        name: String,
    ) -> AppResult<ProjectFile> {
        let context = self.automation_project_context(&id).await?;
        let retained_io = managed::admit_io(self, &context).await?;
        let io_slot = retained_io.clone();
        let source = context.root_selection.clone();
        let result = tokio::task::spawn_blocking(move || -> AppResult<_> {
            let _io_slot = io_slot;
            let root = source.open()?;
            let value = root.create_role(&name).map_err(automation_error)?;
            root.recheck()?;
            Ok(value)
        }).await.map_err(|_|AppError::internal("role scaffolder stopped"))??;
        context.check_after_io(self).await?;
        drop(retained_io);
        Ok(result)
    }
}
