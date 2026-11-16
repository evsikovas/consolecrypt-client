//! Metadata-only bindings and scoped helpers for the authorized request runtime.
use super::*;
use cc_host_mapping::{HostBinding, Labels, Resolution};
use cc_monitoring_core::{QueryData, QueryOutcome, SampleValue};

pub(super) const MAX_BINDINGS: usize = 256;
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct BindingCatalog {
    pub version: u32,
    pub records: BTreeMap<String, BindingRecord>,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct BindingRecord {
    version: u32,
    name: String,
    source_revision: u64,
    pub binding: HostBinding,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum MonitoringBindingState {
    Current,
    StaleSource,
    MissingSource,
    MissingHost,
}
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MonitoringBindingView {
    pub id: String,
    pub name: String,
    pub revision: u64,
    pub source_revision: u64,
    pub binding: HostBinding,
    pub state: MonitoringBindingState,
}
fn name(value: &str) -> AppResult<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() || trimmed.chars().count() > 120 || trimmed.chars().any(char::is_control)
    {
        return Err(invalid("invalid host binding name"));
    }
    Ok(trimmed.into())
}
fn host_exists(u: &Unlocked, id: Uuid) -> bool {
    let Ok(id) = crate::dto::parse_id("monitoring_host", &id.to_string()) else {
        return false;
    };
    u.working().host(id).is_some() || u.working().rdp_host(id).is_some()
}
pub(super) fn validate_catalog(catalog: &BindingCatalog) -> AppResult<()> {
    if catalog.version != 1 || catalog.records.len() > MAX_BINDINGS {
        return Err(invalid(
            "host bindings are read-only or exceed their budget",
        ));
    }
    let mut identities = std::collections::BTreeSet::new();
    for (id, row) in &catalog.records {
        canonical_id(id)?;
        canonical_id(row.binding.source_id())?;
        if row.version != 1 || row.source_revision == 0 || name(&row.name)? != row.name {
            return Err(invalid("host binding is read-only or malformed"));
        }
        // No one-binding-per-host restriction: node_exporter, cAdvisor and
        // application exporter selectors may coexist under the same source.
        let key = (
            row.binding.host_id(),
            row.binding.source_id().to_owned(),
            row.binding.selector().clone(),
        );
        if !identities.insert(key) {
            return Err(invalid("duplicate exact host/source selector"));
        }
    }
    Ok(())
}
fn state(catalog: &Catalog, u: &Unlocked, row: &BindingRecord) -> MonitoringBindingState {
    if !host_exists(u, row.binding.host_id()) {
        return MonitoringBindingState::MissingHost;
    }
    match catalog.sources.get(row.binding.source_id()) {
        None => MonitoringBindingState::MissingSource,
        Some(source) if source.revision != row.source_revision => {
            MonitoringBindingState::StaleSource
        }
        _ => MonitoringBindingState::Current,
    }
}
pub(super) fn views(catalog: &Catalog, u: &Unlocked) -> Vec<MonitoringBindingView> {
    catalog
        .bindings
        .as_ref()
        .map(|bindings| {
            bindings
                .records
                .iter()
                .map(|(id, row)| MonitoringBindingView {
                    id: id.clone(),
                    name: row.name.clone(),
                    revision: row.binding.revision(),
                    source_revision: row.source_revision,
                    binding: row.binding.clone(),
                    state: state(catalog, u, row),
                })
                .collect()
        })
        .unwrap_or_default()
}
fn source_revision(catalog: &Catalog, id: &str, expected: u64) -> AppResult<()> {
    canonical_id(id)?;
    if !catalog
        .sources
        .get(id)
        .is_some_and(|row| row.revision == expected && expected > 0)
    {
        return Err(invalid("monitoring source changed or is unavailable"));
    }
    Ok(())
}
fn row(catalog: &Catalog, id: &str, expected_revision: u64) -> AppResult<BindingRecord> {
    canonical_id(id)?;
    let row = catalog
        .bindings
        .as_ref()
        .and_then(|bindings| bindings.records.get(id))
        .ok_or_else(|| invalid("host binding is unavailable"))?;
    if row.binding.revision() != expected_revision || expected_revision == 0 {
        return Err(invalid("host binding changed"));
    }
    Ok(row.clone())
}
fn no_duplicate(catalog: &Catalog, except: Option<&str>, binding: &HostBinding) -> AppResult<()> {
    if catalog.bindings.as_ref().is_some_and(|bindings| {
        bindings.records.iter().any(|(id, row)| {
            Some(id.as_str()) != except
                && row.binding.host_id() == binding.host_id()
                && row.binding.source_id() == binding.source_id()
                && row.binding.selector() == binding.selector()
        })
    }) {
        return Err(invalid("duplicate exact host/source selector"));
    }
    Ok(())
}

impl AppCore {
    /// The user's explicit first CreateBinding atomically migrates v1 -> v2.
    /// Source-only writes never migrate v1 or add technical enablement fields.
    pub async fn monitoring_binding_create(
        &self,
        expected_catalog_revision: u64,
        name_value: String,
        host_id: String,
        source_id: String,
        expected_source_revision: u64,
        selector: Labels,
    ) -> AppResult<MonitoringCatalogView> {
        self.monitoring_binding_create_with_exporter_origin(
            expected_catalog_revision,
            name_value,
            host_id,
            source_id,
            expected_source_revision,
            selector,
            None,
        )
        .await
    }
    /// Same explicit metadata writer/CAS; optional exporter-origin data adds
    /// current successful-job/target and original unlock/host fences only.
    pub async fn monitoring_binding_create_with_exporter_origin(
        &self,
        expected_catalog_revision: u64,
        name_value: String,
        host_id: String,
        source_id: String,
        expected_source_revision: u64,
        selector: Labels,
        exporter_origin: Option<MonitoringExporterBindingOrigin>,
    ) -> AppResult<MonitoringCatalogView> {
        let name = name(&name_value)?;
        let binding = HostBinding::new(&host_id, &source_id, 1, selector)
            .map_err(|_| invalid("invalid explicit host binding"))?;
        let (owner, u) = self.unlocked().await?;
        let _profile = self.hold_session(&owner).await?;
        let _unlock = owner.hold_unlocked(&u).await?;
        if let Some(origin) = &exporter_origin {
            origin
                .check(&owner, &u, &host_id, &source_id, expected_source_revision)
                .await?;
        }
        // Settings/source CAS is captured after the fresh job SQL await.
        let settings = settings(&u)?;
        let mut catalog = parse(settings.monitoring.as_ref())?;
        if catalog.revision != expected_catalog_revision {
            return Err(invalid("monitoring catalog changed"));
        }
        source_revision(&catalog, &source_id, expected_source_revision)?;
        if !host_exists(&u, binding.host_id()) {
            return Err(invalid("native host is unavailable"));
        }
        no_duplicate(&catalog, None, &binding)?;
        let id = Uuid::new_v4().to_string();
        let bindings = catalog.bindings.get_or_insert_with(|| BindingCatalog {
            version: 1,
            records: BTreeMap::new(),
        });
        if bindings.records.len() >= MAX_BINDINGS || bindings.records.contains_key(&id) {
            return Err(invalid("host binding count limit or UUID collision"));
        }
        bindings.records.insert(
            id,
            BindingRecord {
                version: 1,
                name,
                source_revision: expected_source_revision,
                binding,
            },
        );
        catalog.version = catalog.version.max(2);
        catalog.revision = next_revision(catalog.revision)?;
        persist(&u, settings, catalog).await
    }
    /// Explicit rereview may update the selected host/source/selector; a
    /// changed source revision never silently repairs an existing binding.
    pub async fn monitoring_binding_update(
        &self,
        id: String,
        expected_catalog_revision: u64,
        expected_binding_revision: u64,
        name_value: String,
        host_id: String,
        source_id: String,
        expected_source_revision: u64,
        selector: Labels,
    ) -> AppResult<MonitoringCatalogView> {
        let name = name(&name_value)?;
        let (owner, u) = self.unlocked().await?;
        let _profile = self.hold_session(&owner).await?;
        let _unlock = owner.hold_unlocked(&u).await?;
        let settings = settings(&u)?;
        let mut catalog = parse(settings.monitoring.as_ref())?;
        if catalog.revision != expected_catalog_revision {
            return Err(invalid("monitoring catalog changed"));
        }
        let old = row(&catalog, &id, expected_binding_revision)?;
        source_revision(&catalog, &source_id, expected_source_revision)?;
        let binding = HostBinding::new(
            &host_id,
            &source_id,
            next_revision(old.binding.revision())?,
            selector,
        )
        .map_err(|_| invalid("invalid explicit host binding"))?;
        if !host_exists(&u, binding.host_id()) {
            return Err(invalid("native host is unavailable"));
        }
        no_duplicate(&catalog, Some(&id), &binding)?;
        catalog
            .bindings
            .as_mut()
            .ok_or_else(|| invalid("host bindings are unavailable"))?
            .records
            .insert(
                id,
                BindingRecord {
                    version: 1,
                    name,
                    source_revision: expected_source_revision,
                    binding,
                },
            );
        catalog.revision = next_revision(catalog.revision)?;
        persist(&u, settings, catalog).await
    }
    /// Orphans stay explicitly removable; no host/source existence is required
    /// for delete, but catalog/binding schema and both revisions still apply.
    pub async fn monitoring_binding_delete(
        &self,
        id: String,
        expected_catalog_revision: u64,
        expected_binding_revision: u64,
    ) -> AppResult<MonitoringCatalogView> {
        let (owner, u) = self.unlocked().await?;
        let _profile = self.hold_session(&owner).await?;
        let _unlock = owner.hold_unlocked(&u).await?;
        let settings = settings(&u)?;
        let mut catalog = parse(settings.monitoring.as_ref())?;
        if catalog.revision != expected_catalog_revision {
            return Err(invalid("monitoring catalog changed"));
        }
        row(&catalog, &id, expected_binding_revision)?;
        catalog
            .bindings
            .as_mut()
            .ok_or_else(|| invalid("host bindings are unavailable"))?
            .records
            .remove(&id);
        catalog.revision = next_revision(catalog.revision)?;
        persist(&u, settings, catalog).await
    }
}

/// Fresh metadata lookup for root's later runtime adapter. Caller retains the
/// current profile/unlock lease and runtime source/credential revision watcher.
pub(crate) fn binding_for_request(
    u: &Unlocked,
    id: &str,
    expected_binding_revision: u64,
    expected_source_revision: u64,
) -> AppResult<HostBinding> {
    let catalog = parse(settings(u)?.monitoring.as_ref())?;
    let row = row(&catalog, id, expected_binding_revision)?;
    if row.source_revision != expected_source_revision
        || state(&catalog, u, &row) != MonitoringBindingState::Current
    {
        return Err(invalid(
            "host binding is stale or its host/source is unavailable",
        ));
    }
    source_for_request(u, row.binding.source_id(), expected_source_revision)?;
    Ok(row.binding)
}

/// Native-only request plan, deliberately not Deserialize. Labels/response
/// data cannot change its fixed metric/scoped selector, time or source identity.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct BindingDiscoveryPlan {
    binding_id: String,
    binding_revision: u64,
    source_id: String,
    source_revision: u64,
    expression: String,
    selected_end: f64,
}
impl BindingDiscoveryPlan {
    pub(crate) fn source_id(&self) -> &str {
        &self.source_id
    }
    pub(crate) fn source_revision(&self) -> u64 {
        self.source_revision
    }
    pub(crate) fn expression(&self) -> &str {
        &self.expression
    }
    pub(crate) fn selected_end(&self) -> f64 {
        self.selected_end
    }
}
pub(crate) fn discovery_plan_for_binding(
    u: &Unlocked,
    id: &str,
    binding_revision: u64,
    source_revision: u64,
    selected_end: f64,
) -> AppResult<BindingDiscoveryPlan> {
    if !selected_end.is_finite() || selected_end.abs() > 253_402_300_799.0 {
        return Err(invalid("invalid identity discovery time"));
    }
    let binding = binding_for_request(u, id, binding_revision, source_revision)?;
    let expression = binding
        .render_metric("up")
        .map_err(|_| invalid("invalid scoped identity discovery selector"))?;
    Ok(BindingDiscoveryPlan {
        binding_id: id.into(),
        binding_revision,
        source_id: binding.source_id().into(),
        source_revision,
        expression,
        selected_end,
    })
}
/// Resolve a COMPLETE actual `up` vector requested by the fixed plan, not an
/// arbitrary panel result. Keep original rows, including duplicate label maps.
pub(crate) fn resolve_discovery(
    u: &Unlocked,
    plan: &BindingDiscoveryPlan,
    observed_source: &str,
    observed_source_revision: u64,
    observed_end: f64,
    response: &QueryOutcome,
) -> AppResult<Resolution> {
    if observed_source != plan.source_id
        || observed_source_revision != plan.source_revision
        || !observed_end.is_finite()
        || observed_end != plan.selected_end
    {
        return Err(invalid(
            "identity discovery source or selected time changed",
        ));
    }
    let binding = binding_for_request(
        u,
        &plan.binding_id,
        plan.binding_revision,
        plan.source_revision,
    )?;
    if response.status != "success"
        || response.http_status != 200
        || response.partial
        || response.error.is_some()
        || !response.warnings.is_empty()
    {
        return Err(invalid("identity discovery is incomplete or failed"));
    }
    let Some(QueryData::Vector { series }) = &response.data else {
        return Err(invalid("identity discovery requires an instant up vector"));
    };
    if series.len() > cc_host_mapping::MAX_CANDIDATES {
        return Err(invalid("identity discovery candidate budget exceeded"));
    }
    let mut candidates = Vec::with_capacity(series.len());
    let mut bytes = 0usize;
    for candidate in series {
        if candidate.labels.get("__name__").map(String::as_str) != Some("up") {
            return Err(invalid("identity discovery returned a different metric"));
        }
        if candidate.samples.len() != 1 {
            return Err(invalid("invalid identity discovery sample"));
        }
        let sample = &candidate.samples[0];
        if !sample.timestamp.is_finite()
            || (sample.timestamp - plan.selected_end).abs() > 0.001
            || !matches!(sample.value,SampleValue::Finite{value}if value==0.0||value==1.0)
        {
            return Err(invalid("invalid identity discovery timestamp or up value"));
        }
        if candidate.labels.len() > cc_host_mapping::MAX_CANDIDATE_LABELS {
            return Err(invalid("identity discovery label budget exceeded"));
        }
        for (key, value) in &candidate.labels {
            if key.is_empty()
                || key.len() > 1024
                || value.len() > cc_host_mapping::MAX_LABEL_VALUE_BYTES
            {
                return Err(invalid("invalid identity discovery labels"));
            }
            bytes = bytes.saturating_add(key.len()).saturating_add(value.len());
            if bytes > cc_host_mapping::MAX_DISCOVERY_BYTES {
                return Err(invalid("identity discovery byte budget exceeded"));
            }
        }
        candidates.push(candidate.labels.clone());
    }
    cc_host_mapping::resolve(&binding, observed_source, &candidates)
        .map_err(|_| invalid("invalid identity discovery candidates"))
}
