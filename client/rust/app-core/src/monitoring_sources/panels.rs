//! Saved custom PromQL metadata only, inside the existing encrypted settings.
use super::*;
use crate::monitoring_sync::{PanelWriteAdmission, PanelWritePolicy};

pub(super) const MAX_PANELS: usize = 64;
pub(super) const MAX_QUERY_BYTES: usize = 16 * 1024;

/// User-supplied display metadata, never inferred from PromQL or used to
/// convert/normalize a value. Unknown is an explicit supported unit hint.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum MonitoringUnitHint {
    Unknown,
    Percent,
    Bytes,
    BytesPerSecond,
    Seconds,
    Count,
    PerSecond,
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MonitoringPanelDefinition {
    pub title: String,
    pub source_id: String,
    pub source_revision: u64,
    pub query: String,
    pub unit_hint: MonitoringUnitHint,
}
impl std::fmt::Debug for MonitoringPanelDefinition {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MonitoringPanelDefinition")
            .field("source_id", &self.source_id)
            .field("source_revision", &self.source_revision)
            .field("query_bytes", &self.query.len())
            .field("unit_hint", &self.unit_hint)
            .finish_non_exhaustive()
    }
}
impl MonitoringPanelDefinition {
    pub(super) fn validate(&self) -> AppResult<()> {
        canonical_id(&self.source_id).map_err(|_| error("panel_invalid"))?;
        if self.source_revision == 0
            || self.title.trim() != self.title
            || self.title.is_empty()
            || self.title.chars().count() > 120
            || self.title.chars().any(char::is_control)
            || self.query.trim().is_empty()
            || self.query.len() > MAX_QUERY_BYTES
            || self
                .query
                .chars()
                .any(|c| c.is_control() && !matches!(c, '\t' | '\r' | '\n'))
        {
            return Err(error("panel_invalid"));
        }
        Ok(())
    }
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct PanelCatalog {
    pub version: u32,
    pub records: BTreeMap<String, PanelRecord>,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct PanelRecord {
    version: u32,
    revision: u64,
    definition: MonitoringPanelDefinition,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum MonitoringPanelState {
    Current,
    StaleSource,
    MissingSource,
}
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MonitoringPanelView {
    pub id: String,
    pub revision: u64,
    pub definition: MonitoringPanelDefinition,
    pub state: MonitoringPanelState,
}

fn error(code: &'static str) -> AppError {
    AppError::invalid("monitoring_panels", code)
}
pub(super) fn validate_catalog(panels: &PanelCatalog) -> AppResult<()> {
    if panels.version != 1 || panels.records.len() > MAX_PANELS {
        return Err(error("panel_readonly"));
    }
    for (id, row) in &panels.records {
        canonical_id(id).map_err(|_| error("panel_readonly"))?;
        if row.version != 1 || row.revision == 0 {
            return Err(error("panel_readonly"));
        }
        row.definition.validate()?;
    }
    Ok(())
}
fn state(catalog: &Catalog, row: &PanelRecord) -> MonitoringPanelState {
    match catalog.sources.get(&row.definition.source_id) {
        None => MonitoringPanelState::MissingSource,
        Some(source) if source.revision != row.definition.source_revision => {
            MonitoringPanelState::StaleSource
        }
        _ => MonitoringPanelState::Current,
    }
}
pub(super) fn views(catalog: &Catalog) -> Option<Vec<MonitoringPanelView>> {
    catalog.panels.as_ref().map(|panels| {
        panels
            .records
            .iter()
            .map(|(id, row)| MonitoringPanelView {
                id: id.clone(),
                revision: row.revision,
                definition: row.definition.clone(),
                state: state(catalog, row),
            })
            .collect()
    })
}
pub(super) fn references(catalog: &Catalog, source: &str) -> bool {
    catalog.panels.as_ref().is_some_and(|panels| {
        panels
            .records
            .values()
            .any(|row| row.definition.source_id == source)
    })
}
fn current_source(catalog: &Catalog, definition: &MonitoringPanelDefinition) -> AppResult<()> {
    match catalog.sources.get(&definition.source_id) {
        None => Err(error("panel_source_missing")),
        Some(source) if source.revision != definition.source_revision => {
            Err(error("panel_source_changed"))
        }
        _ => Ok(()),
    }
}
fn row<'a>(catalog: &'a Catalog, id: &str, revision: u64) -> AppResult<&'a PanelRecord> {
    canonical_id(id).map_err(|_| error("panel_invalid"))?;
    let row = catalog
        .panels
        .as_ref()
        .and_then(|panels| panels.records.get(id))
        .ok_or_else(|| error("panel_missing"))?;
    if revision == 0 || row.revision != revision {
        return Err(error("panel_changed"));
    }
    Ok(row)
}

/// Metadata lookup only. A future runtime must hold its current profile/unlock
/// lease, then apply the existing source/auth/revision/cancellation fences.
pub(crate) fn panel_for_request(
    u: &Unlocked,
    id: &str,
    expected_revision: u64,
) -> AppResult<MonitoringPanelDefinition> {
    let settings = settings(u)?;
    let catalog = parse(settings.monitoring.as_ref())?;
    let row = row(&catalog, id, expected_revision)?;
    current_source(&catalog, &row.definition)?;
    Ok(row.definition.clone())
}

#[cfg(test)]
impl AppCore {
    pub async fn monitoring_panel_definition(
        &self,
        id: String,
        expected_panel_revision: u64,
    ) -> AppResult<MonitoringPanelDefinition> {
        let (owner, u) = self.unlocked().await?;
        let _profile = self.hold_session(&owner).await?;
        let _unlock = owner.hold_unlocked(&u).await?;
        panel_for_request(&u, &id, expected_panel_revision)
    }
    /// Explicit create is the sole v1/v2 -> v3 migration. PromQL is stored as
    /// exact user data; saving never issues a query or validates via a network.
    pub async fn monitoring_panel_create(
        &self,
        expected_catalog_revision: u64,
        definition: MonitoringPanelDefinition,
    ) -> AppResult<MonitoringCatalogView> {
        self.monitoring_panel_create_with_policy(
            expected_catalog_revision,
            definition,
            PanelWritePolicy::Released,
        )
        .await
    }
    pub(crate) async fn monitoring_panel_create_with_policy(
        &self,
        expected_catalog_revision: u64,
        mut definition: MonitoringPanelDefinition,
        policy: PanelWritePolicy,
    ) -> AppResult<MonitoringCatalogView> {
        definition.title = definition.title.trim().into();
        definition.validate()?;
        let (owner, u) = self.unlocked().await?;
        let local = {
            let _profile = self.hold_session(&owner).await?;
            let _unlock = owner.hold_unlocked(&u).await?;
            owner.kind().await? == cc_storage_core::ProfileKind::Local
        };
        let (owner, u, admission) = if local {
            (owner, u, None)
        } else {
            let admission =
                PanelWriteAdmission::prepare(self, owner, u, expected_catalog_revision, policy)
                    .await?;
            let (owner, u) = admission.original(self).await?;
            (owner, u, Some(admission))
        };
        let _profile = self.hold_session(&owner).await?;
        let _unlock = owner.hold_unlocked(&u).await?;
        let settings = settings(&u)?;
        let mut catalog = parse(settings.monitoring.as_ref())?;
        if catalog.revision != expected_catalog_revision {
            return Err(error("panel_catalog_changed"));
        }
        current_source(&catalog, &definition)?;
        let id = Uuid::new_v4().to_string();
        let panels = catalog.panels.get_or_insert_with(|| PanelCatalog {
            version: 1,
            records: BTreeMap::new(),
        });
        if panels.records.len() >= MAX_PANELS || panels.records.contains_key(&id) {
            return Err(error("panel_budget"));
        }
        panels.records.insert(
            id,
            PanelRecord {
                version: 1,
                revision: 1,
                definition,
            },
        );
        catalog
            .bindings
            .get_or_insert_with(|| bindings::BindingCatalog {
                version: 1,
                records: BTreeMap::new(),
            });
        catalog.version = 3;
        catalog.revision = next_revision(catalog.revision)?;
        // Existing writer.put serializes and pads the WHOLE ObjectPayload,
        // encrypts it and checks ciphertext size atomically before mutation.
        // Do not guess headroom from this catalog's JSON size alone.
        if let Some(admission) = admission {
            bounded(&catalog)?;
            let raw = serde_json::to_value(catalog).map_err(AppError::internal)?;
            let mut settings = settings;
            settings.monitoring = Some(raw.clone());
            settings.updated_at = chrono::Utc::now();
            let object = VaultObject::VaultSettings(settings);
            crate::inventory::validate_refs(u.working(), &object)?;
            // Worker rechecks real cursor/outbox/head and original guards at
            // dispatch. Do not hold an async profile/slot guard while queuing.
            drop(_unlock);
            drop(_profile);
            drop(u);
            drop(owner);
            return admission
                .commit(self, object, move |u| {
                    if super::settings(u)?.monitoring.as_ref() != Some(&raw) {
                        return Err(error("panel_catalog_changed"));
                    }
                    Ok(view(Some(&raw), u))
                })
                .await;
        }
        persist(&u, settings, catalog).await
    }
    pub async fn monitoring_panel_update(
        &self,
        id: String,
        expected_catalog_revision: u64,
        expected_panel_revision: u64,
        definition: MonitoringPanelDefinition,
    ) -> AppResult<MonitoringCatalogView> {
        self.monitoring_panel_update_with_policy(
            id,
            expected_catalog_revision,
            expected_panel_revision,
            definition,
            PanelWritePolicy::Released,
        )
        .await
    }
    pub(crate) async fn monitoring_panel_update_with_policy(
        &self,
        id: String,
        expected_catalog_revision: u64,
        expected_panel_revision: u64,
        mut definition: MonitoringPanelDefinition,
        policy: PanelWritePolicy,
    ) -> AppResult<MonitoringCatalogView> {
        definition.title = definition.title.trim().into();
        definition.validate()?;
        let (owner, u) = self.unlocked().await?;
        let local = {
            let _profile = self.hold_session(&owner).await?;
            let _unlock = owner.hold_unlocked(&u).await?;
            owner.kind().await? == cc_storage_core::ProfileKind::Local
        };
        let (owner, u, admission) = if local {
            (owner, u, None)
        } else {
            let admission =
                PanelWriteAdmission::prepare(self, owner, u, expected_catalog_revision, policy)
                    .await?;
            let (owner, u) = admission.original(self).await?;
            (owner, u, Some(admission))
        };
        let _profile = self.hold_session(&owner).await?;
        let _unlock = owner.hold_unlocked(&u).await?;
        let settings = settings(&u)?;
        let mut catalog = parse(settings.monitoring.as_ref())?;
        if catalog.revision != expected_catalog_revision {
            return Err(error("panel_catalog_changed"));
        }
        let next = next_revision(row(&catalog, &id, expected_panel_revision)?.revision)?;
        current_source(&catalog, &definition)?;
        let panels = catalog
            .panels
            .as_mut()
            .ok_or_else(|| error("panel_missing"))?;
        panels.records.insert(
            id,
            PanelRecord {
                version: 1,
                revision: next,
                definition,
            },
        );
        catalog.revision = next_revision(catalog.revision)?;
        if let Some(admission) = admission {
            bounded(&catalog)?;
            let raw = serde_json::to_value(catalog).map_err(AppError::internal)?;
            let mut settings = settings;
            settings.monitoring = Some(raw.clone());
            settings.updated_at = chrono::Utc::now();
            let object = VaultObject::VaultSettings(settings);
            crate::inventory::validate_refs(u.working(), &object)?;
            // Worker rechecks real cursor/outbox/head and original guards at
            // dispatch. Do not hold an async profile/slot guard while queuing.
            drop(_unlock);
            drop(_profile);
            drop(u);
            drop(owner);
            return admission
                .commit(self, object, move |u| {
                    if super::settings(u)?.monitoring.as_ref() != Some(&raw) {
                        return Err(error("panel_catalog_changed"));
                    }
                    Ok(view(Some(&raw), u))
                })
                .await;
        }
        persist(&u, settings, catalog).await
    }
    pub async fn monitoring_panel_delete(
        &self,
        id: String,
        expected_catalog_revision: u64,
        expected_panel_revision: u64,
    ) -> AppResult<MonitoringCatalogView> {
        let (owner, u) = self.unlocked().await?;
        let _profile = self.hold_session(&owner).await?;
        let _unlock = owner.hold_unlocked(&u).await?;
        let settings = settings(&u)?;
        let mut catalog = parse(settings.monitoring.as_ref())?;
        if catalog.revision != expected_catalog_revision {
            return Err(error("panel_catalog_changed"));
        }
        row(&catalog, &id, expected_panel_revision)?;
        catalog
            .panels
            .as_mut()
            .ok_or_else(|| error("panel_missing"))?
            .records
            .remove(&id);
        catalog.revision = next_revision(catalog.revision)?;
        persist(&u, settings, catalog).await
    }
}
