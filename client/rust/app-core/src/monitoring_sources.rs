//! V3 saved custom panels extend the same encrypted monitoring catalog.
//! Legacy v1/v2 retain their literal source/binding shapes and validation.
//! No query execution, metric persistence, credentials, auth grants or local settings.
use crate::session::Unlocked;
use crate::{AppCore, AppError, AppResult};
use cc_models::VaultObject;
use cc_models::credential::CredentialKind;
use cc_models::settings::VaultSettings;
use cc_monitoring_core::{AuthRef, SourceConfig};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;
use uuid::Uuid;
mod bindings;
mod context;
mod exporter_origin;
pub use exporter_origin::MonitoringExporterBindingOrigin;
mod panels;
pub(crate) use bindings::{
    BindingDiscoveryPlan, binding_for_request, discovery_plan_for_binding, resolve_discovery,
};
pub use bindings::{MonitoringBindingState, MonitoringBindingView};
pub(crate) use panels::panel_for_request;
pub use panels::{
    MonitoringPanelDefinition, MonitoringPanelState, MonitoringPanelView, MonitoringUnitHint,
};

pub const MAX_MONITORING_SOURCES: usize = 32;
pub const MAX_MONITORING_CATALOG_BYTES: usize = 1024 * 1024;

#[derive(Clone, Serialize)]
struct Catalog {
    version: u32,
    revision: u64,
    sources: BTreeMap<String, SourceRecord>,
    #[serde(skip_serializing_if = "Option::is_none")]
    bindings: Option<bindings::BindingCatalog>,
    #[serde(skip_serializing_if = "Option::is_none")]
    panels: Option<panels::PanelCatalog>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CatalogV1 {
    version: u32,
    revision: u64,
    sources: BTreeMap<String, SourceRecord>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CatalogV2 {
    version: u32,
    revision: u64,
    sources: BTreeMap<String, SourceRecord>,
    bindings: bindings::BindingCatalog,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CatalogV3 {
    version: u32,
    revision: u64,
    sources: BTreeMap<String, SourceRecord>,
    bindings: bindings::BindingCatalog,
    panels: panels::PanelCatalog,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct SourceRecord {
    version: u32,
    revision: u64,
    configuration: SourceConfig,
}
impl Default for Catalog {
    fn default() -> Self {
        Self {
            version: 1,
            revision: 0,
            sources: BTreeMap::new(),
            bindings: None,
            panels: None,
        }
    }
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MonitoringSourceView {
    pub id: String,
    pub revision: Option<u64>,
    pub configuration: Option<SourceConfig>,
    pub read_only: bool,
}
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MonitoringCatalogView {
    pub schema_version: Option<u64>,
    pub revision: Option<u64>,
    pub read_only: bool,
    pub sources: Vec<MonitoringSourceView>,
    pub bindings: Vec<MonitoringBindingView>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub panels: Option<Vec<MonitoringPanelView>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub panel_sync: Option<Value>,
}

fn invalid(message: &'static str) -> AppError {
    AppError::invalid("monitoring_sources", message)
}

fn canonical_id(value: &str) -> AppResult<Uuid> {
    let id = Uuid::parse_str(value).map_err(|_| invalid("invalid source UUID"))?;
    if id.is_nil() || id.hyphenated().to_string() != value {
        return Err(invalid("invalid source UUID"));
    }
    Ok(id)
}

fn settings(u: &Unlocked) -> AppResult<VaultSettings> {
    u.working()
        .all_vault_settings()
        .into_iter()
        .max_by_key(|s| s.updated_at)
        .ok_or_else(|| AppError::not_found("vault settings", "-"))
}

/// Metadata only; caller holds original profile/unlock and checks working
/// version before/after. Reuses EXACT existing settings selection +strict parser.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct MonitoringCatalogCheckpoint {
    pub settings_id: cc_protocol::ObjectId,
    pub object_revision: i64,
    pub schema: u32,
    pub revision: u64,
}
pub(crate) fn catalog_checkpoint(u: &Unlocked) -> AppResult<MonitoringCatalogCheckpoint> {
    let selected = settings(u)?;
    let parsed = parse(selected.monitoring.as_ref())?;
    let settings_id: cc_protocol::ObjectId = selected.id;
    let object_revision = u
        .working()
        .revision_of(settings_id)
        .ok_or_else(|| AppError::invalid("monitoring_sync", "context_changed"))?;
    Ok(MonitoringCatalogCheckpoint {
        settings_id,
        object_revision,
        schema: parsed.version,
        revision: parsed.revision,
    })
}

fn validate_configuration(configuration: &SourceConfig) -> AppResult<()> {
    canonical_id(&configuration.id)?;
    configuration
        .validate()
        .map_err(|_| invalid("invalid monitoring source configuration"))?;
    if configuration.name.trim().is_empty() || configuration.name.chars().any(char::is_control) {
        return Err(invalid("invalid monitoring source name"));
    }
    Ok(())
}

fn validate_refs(u: &Unlocked, configuration: &SourceConfig) -> AppResult<()> {
    if let cc_monitoring_core::Route::SshHost { host_id } | cc_monitoring_core::Route::AgentDestination { host_id, .. } = &configuration.route {
        canonical_id(host_id)?;
        let id = crate::dto::parse_id("monitoring_ssh_host", host_id)?;
        if u.working().host(id).is_none() {
            return Err(invalid("saved monitoring SSH host unavailable"));
        }
    }
    let reference = match &configuration.auth {
        AuthRef::None => return Ok(()),
        AuthRef::BasicRef { credential_id } | AuthRef::BearerRef { credential_id } => credential_id,
    };
    canonical_id(reference)?;
    let id = crate::dto::parse_id("monitoring_credential", reference)?;
    let credential = u
        .working()
        .credential(id)
        .ok_or_else(|| invalid("monitoring credential is unavailable"))?;
    if credential.kind != CredentialKind::Password || u.working().vault_of(id).is_some() {
        return Err(invalid(
            "monitoring auth requires a personal password credential reference",
        ));
    }
    Ok(())
}

/// Count serialized bytes without allocating an unbounded raw JSON copy.
fn bounded(value: &impl Serialize) -> AppResult<()> {
    struct Budget(usize);
    impl std::io::Write for Budget {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            self.0 = self
                .0
                .checked_add(bytes.len())
                .filter(|n| *n <= MAX_MONITORING_CATALOG_BYTES)
                .ok_or_else(|| std::io::Error::other("catalog budget"))?;
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    serde_json::to_writer(Budget(0), value)
        .map_err(|_| invalid("monitoring catalog byte limit exceeded"))
}

fn parse(raw: Option<&Value>) -> AppResult<Catalog> {
    let Some(raw) = raw else {
        return Ok(Catalog::default());
    };
    bounded(raw)?;
    let malformed = || invalid("monitoring catalog is read-only or malformed");
    // Separate strict wire structs are essential: v1 must reject even a new
    // `bindings:null` field rather than silently changing its old semantics.
    let catalog = match raw.get("version").and_then(Value::as_u64) {
        Some(1) => {
            let value: CatalogV1 = serde_json::from_value(raw.clone()).map_err(|_| malformed())?;
            Catalog {
                version: value.version,
                revision: value.revision,
                sources: value.sources,
                bindings: None,
                panels: None,
            }
        }
        Some(2) => {
            let value: CatalogV2 = serde_json::from_value(raw.clone()).map_err(|_| malformed())?;
            Catalog {
                version: value.version,
                revision: value.revision,
                sources: value.sources,
                bindings: Some(value.bindings),
                panels: None,
            }
        }
        Some(3) => {
            let value: CatalogV3 = serde_json::from_value(raw.clone()).map_err(|_| malformed())?;
            Catalog {
                version: value.version,
                revision: value.revision,
                sources: value.sources,
                bindings: Some(value.bindings),
                panels: Some(value.panels),
            }
        }
        _ => return Err(malformed()),
    };
    if catalog.revision == 0 || catalog.sources.len() > MAX_MONITORING_SOURCES {
        return Err(invalid("monitoring catalog is read-only or malformed"));
    }
    for (id, record) in &catalog.sources {
        canonical_id(id)?;
        if record.version != 1 || record.revision == 0 || record.configuration.id != *id {
            return Err(invalid("monitoring source is read-only or malformed"));
        }
        validate_configuration(&record.configuration)?;
    }
    if let Some(bindings) = &catalog.bindings {
        bindings::validate_catalog(bindings)?;
    }
    if let Some(panels) = &catalog.panels {
        panels::validate_catalog(panels)?;
    }
    Ok(catalog)
}

fn view(raw: Option<&Value>, u: &Unlocked) -> MonitoringCatalogView {
    match parse(raw) {
        Ok(catalog) => MonitoringCatalogView {
            schema_version: Some(catalog.version as u64),
            revision: Some(catalog.revision),
            read_only: false,
            bindings: bindings::views(&catalog, u),
            panels: panels::views(&catalog),
            panel_sync: None,
            sources: catalog
                .sources
                .into_iter()
                .map(|(id, row)| MonitoringSourceView {
                    id,
                    revision: Some(row.revision),
                    configuration: Some(row.configuration),
                    read_only: false,
                })
                .collect(),
        },
        Err(_) => MonitoringCatalogView {
            schema_version: raw.and_then(|v| v.get("version")).and_then(Value::as_u64),
            revision: raw.and_then(|v| v.get("revision")).and_then(Value::as_u64),
            read_only: true,
            // Raw unsupported records stay encrypted and intact, never passed
            // as runnable source configs or arbitrary data to the UI.
            sources: Vec::new(),
            bindings: Vec::new(),
            panels: None,
            panel_sync: None,
        },
    }
}

fn next_revision(value: u64) -> AppResult<u64> {
    value
        .checked_add(1)
        .ok_or_else(|| invalid("catalog revision exhausted"))
}

async fn persist(
    u: &Unlocked,
    mut settings: VaultSettings,
    catalog: Catalog,
) -> AppResult<MonitoringCatalogView> {
    bounded(&catalog)?;
    let raw = serde_json::to_value(catalog).map_err(AppError::internal)?;
    settings.monitoring = Some(raw.clone());
    settings.updated_at = chrono::Utc::now();
    let object = VaultObject::VaultSettings(settings);
    crate::inventory::validate_refs(u.working(), &object)?;
    u.writer.put(object).await?;
    Ok(view(Some(&raw), u))
}

/// Fresh working-set source lookup for the root-owned request runtime. The
/// caller must hold its profile/unlock lease and enforce request ownership.
/// This resolves configuration/references only, never secret material.
pub(crate) fn source_for_request(
    u: &Unlocked,
    id: &str,
    expected_revision: u64,
) -> AppResult<SourceConfig> {
    canonical_id(id)?;
    let settings = settings(u)?;
    let catalog = parse(settings.monitoring.as_ref())?;
    let row = catalog
        .sources
        .get(id)
        .ok_or_else(|| invalid("monitoring source is unavailable"))?;
    if row.revision != expected_revision {
        return Err(invalid("monitoring source changed"));
    }
    validate_refs(u, &row.configuration)?;
    Ok(row.configuration.clone())
}

impl AppCore {
    pub async fn monitoring_sources(&self) -> AppResult<MonitoringCatalogView> {
        let (owner, u) = self.unlocked().await?;
        let _profile = self.hold_session(&owner).await?;
        let _unlock = owner.hold_unlocked(&u).await?;
        let mut catalog = view(settings(&u)?.monitoring.as_ref(), &u);
        catalog.panel_sync = Some(crate::monitoring_sync::catalog_snapshot(
            &owner,
            &u,
            owner.kind().await?,
        )?);
        Ok(catalog)
    }

    pub async fn monitoring_source_create(
        &self,
        expected_catalog_revision: u64,
        mut configuration: SourceConfig,
    ) -> AppResult<MonitoringCatalogView> {
        if !configuration.id.is_empty() {
            return Err(invalid("source UUID is assigned by Native on create"));
        }
        configuration.id = Uuid::new_v4().to_string();
        validate_configuration(&configuration)?;
        let (owner, u) = self.unlocked().await?;
        let _profile = self.hold_session(&owner).await?;
        let _unlock = owner.hold_unlocked(&u).await?;
        let settings = settings(&u)?;
        let mut catalog = parse(settings.monitoring.as_ref())?;
        if catalog.revision != expected_catalog_revision {
            return Err(invalid("monitoring catalog changed"));
        }
        if catalog.sources.len() >= MAX_MONITORING_SOURCES {
            return Err(invalid("monitoring source count limit reached"));
        }
        if catalog.sources.contains_key(&configuration.id) {
            return Err(invalid("new monitoring source UUID collided"));
        }
        validate_refs(&u, &configuration)?;
        catalog.revision = next_revision(catalog.revision)?;
        catalog.sources.insert(
            configuration.id.clone(),
            SourceRecord {
                version: 1,
                revision: 1,
                configuration,
            },
        );
        persist(&u, settings, catalog).await
    }

    pub async fn monitoring_source_update(
        &self,
        id: String,
        expected_catalog_revision: u64,
        expected_source_revision: u64,
        configuration: SourceConfig,
    ) -> AppResult<MonitoringCatalogView> {
        canonical_id(&id)?;
        if configuration.id != id {
            return Err(invalid("monitoring source UUID cannot change"));
        }
        validate_configuration(&configuration)?;
        let (owner, u) = self.unlocked().await?;
        let _profile = self.hold_session(&owner).await?;
        let _unlock = owner.hold_unlocked(&u).await?;
        let settings = settings(&u)?;
        let mut catalog = parse(settings.monitoring.as_ref())?;
        if catalog.revision != expected_catalog_revision {
            return Err(invalid("monitoring catalog changed"));
        }
        let row = catalog
            .sources
            .get_mut(&id)
            .ok_or_else(|| invalid("monitoring source is unavailable"))?;
        if row.revision != expected_source_revision {
            return Err(invalid("monitoring source changed"));
        }
        validate_refs(&u, &configuration)?;
        row.revision = next_revision(row.revision)?;
        row.configuration = configuration;
        catalog.revision = next_revision(catalog.revision)?;
        persist(&u, settings, catalog).await
    }

    pub async fn monitoring_source_delete(
        &self,
        id: String,
        expected_catalog_revision: u64,
        expected_source_revision: u64,
    ) -> AppResult<MonitoringCatalogView> {
        canonical_id(&id)?;
        let (owner, u) = self.unlocked().await?;
        let _profile = self.hold_session(&owner).await?;
        let _unlock = owner.hold_unlocked(&u).await?;
        let settings = settings(&u)?;
        let mut catalog = parse(settings.monitoring.as_ref())?;
        if catalog.revision != expected_catalog_revision {
            return Err(invalid("monitoring catalog changed"));
        }
        let row = catalog
            .sources
            .get(&id)
            .ok_or_else(|| invalid("monitoring source is unavailable"))?;
        if row.revision != expected_source_revision {
            return Err(invalid("monitoring source changed"));
        }
        if catalog.bindings.as_ref().is_some_and(|bindings| {
            bindings
                .records
                .values()
                .any(|record| record.binding.source_id() == id)
        }) {
            return Err(invalid(
                "delete referencing host bindings before deleting this source",
            ));
        }
        if panels::references(&catalog, &id) {
            return Err(AppError::invalid(
                "monitoring_panels",
                "panel_source_in_use",
            ));
        }
        catalog.sources.remove(&id);
        catalog.revision = next_revision(catalog.revision)?;
        persist(&u, settings, catalog).await
    }
}

#[cfg(test)]
mod binding_tests;
#[cfg(test)]
mod tests;

#[cfg(test)]
mod panel_tests;

/// Configuration revision only for original-scope metadata capture. No route,
/// credential reference/material lookup, transport, probe or grant is produced.
pub(crate) fn source_metadata_revision(u: &Unlocked, id: &str, expected: u64) -> AppResult<()> {
    canonical_id(id)?;
    let current = settings(u)?;
    let catalog = parse(current.monitoring.as_ref())?;
    if expected == 0
        || !catalog
            .sources
            .get(id)
            .is_some_and(|row| row.revision == expected)
    {
        return Err(invalid("monitoring source changed"));
    }
    Ok(())
}
