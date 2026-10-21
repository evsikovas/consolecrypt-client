//! Device-local presentation preferences, encrypted with the profile database.
use crate::{AppCore, AppError, AppResult};
use cc_k8s_core::resources::ResourceRef;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KubeViewPreferences {
    pub workspace: Option<WorkspacePreferences>,
    pub tables: BTreeMap<String, TablePreferences>,
    #[serde(default)]
    pub gitops: Option<GitOpsPreferences>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GitOpsPreferences {
    pub notifications: bool,
    pub reminders: Vec<GitOpsReminder>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GitOpsReminder {
    pub resource: ResourceRef,
    pub due_at_ms: i64,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkspacePreferences {
    pub height: f64,
    pub open: bool,
    pub tab: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TablePreferences {
    pub widths: BTreeMap<String, f64>,
    pub hidden: Vec<String>,
    pub sort: String,
    pub ascending: bool,
}
#[derive(Debug, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum KubeViewPreferencePatch {
    Workspace(WorkspacePreferences),
    Table {
        resource_kind: String,
        preferences: TablePreferences,
    },
    Gitops {
        preferences: GitOpsPreferences,
    },
}
fn valid_label(value: &str) -> bool {
    !value.is_empty() && value.len() <= 512 && !value.chars().any(char::is_control)
}
impl KubeViewPreferencePatch {
    fn validate(&self) -> bool {
        match self {
            Self::Workspace(value) => {
                value.height.is_finite()
                    && (120.0..=1200.0).contains(&value.height)
                    && ["terminal", "logs", "ai", "output"].contains(&value.tab.as_str())
            }
            Self::Table {
                resource_kind,
                preferences,
            } => {
                valid_label(resource_kind)
                    && preferences.widths.len() <= 64
                    && preferences.hidden.len() <= 64
                    && valid_label(&preferences.sort)
                    && preferences.widths.iter().all(|(key, width)| {
                        valid_label(key) && width.is_finite() && (80.0..=600.0).contains(width)
                    })
                    && preferences.hidden.iter().all(|key| valid_label(key))
            }
            Self::Gitops { preferences } => {
                let mut seen = BTreeSet::new();
                preferences.reminders.len() <= 32
                    && preferences.reminders.iter().all(|reminder| {
                        let resource = &reminder.resource;
                        let api = &resource.api;
                        // Stored references are navigation hints, never executable actions.
                        // Use the same bounded shape as discovery without persisting bodies.
                        (api.group.is_empty() || valid_label(&api.group))
                            && valid_label(&api.version)
                            && valid_label(&api.resource)
                            && valid_label(&api.kind)
                            && api.verbs.len() <= 16
                            && api.verbs.iter().all(|verb| valid_label(verb))
                            && valid_label(&resource.name)
                            && resource.namespace.as_deref().is_none_or(valid_label)
                            && (0..=253_402_300_799_999).contains(&reminder.due_at_ms)
                            && seen.insert((
                                api.key(),
                                resource.namespace.as_deref(),
                                resource.name.as_str(),
                            ))
                    })
            }
        }
    }
}
impl AppCore {
    pub async fn kube_view_preferences(
        &self,
        context_id: String,
    ) -> AppResult<KubeViewPreferences> {
        let (owner, unlocked) = self.unlocked().await?;
        let id = crate::dto::parse_id("context_id", &context_id)?;
        if unlocked.working().kube_context(id).is_none() {
            return Err(AppError::not_found("kube_context", id));
        }
        let value = owner
            .storage()?
            .setting_get(format!("kubernetes.view.{id}"))
            .await?
            .unwrap_or_default();
        let _profile = self.hold_session(&owner).await?;
        let _generation = owner.hold_unlocked(&unlocked).await?;
        Ok(value)
    }
    pub async fn kube_view_preferences_update(
        &self,
        context_id: String,
        patch: KubeViewPreferencePatch,
    ) -> AppResult<()> {
        if !patch.validate() {
            return Err(AppError::invalid("kubernetes", "invalid_view_preferences"));
        }
        let (owner, unlocked) = self.unlocked().await?;
        let id = crate::dto::parse_id("context_id", &context_id)?;
        if unlocked.working().kube_context(id).is_none() {
            return Err(AppError::not_found("kube_context", id));
        }
        let key = format!("kubernetes.view.{id}");
        let _profile = self.hold_session(&owner).await?;
        let _generation = owner.hold_unlocked(&unlocked).await?;
        owner
            .storage()?
            .write(move |tx| {
                let mut current: KubeViewPreferences = tx.setting_get(&key)?.unwrap_or_default();
                match patch {
                    KubeViewPreferencePatch::Workspace(workspace) => {
                        current.workspace = Some(workspace)
                    }
                    KubeViewPreferencePatch::Table {
                        resource_kind,
                        preferences,
                    } => {
                        if current.tables.len() >= 64
                            && !current.tables.contains_key(&resource_kind)
                        {
                            return Err(cc_storage_core::StorageError::Invalid(
                                "too_many_view_preferences".into(),
                            ));
                        }
                        current.tables.insert(resource_kind, preferences);
                    }
                    KubeViewPreferencePatch::Gitops { preferences } => {
                        current.gitops = Some(preferences);
                    }
                }
                tx.setting_set(&key, &current)
            })
            .await?;
        Ok(())
    }
}
