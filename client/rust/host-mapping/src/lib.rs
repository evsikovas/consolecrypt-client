//! Pure, bounded host-to-metric identity selection. No auth, network or resolver.
//!
//! A binding records a native/user decision; metric labels are observations,
//! never proof that an exporter owns a native host identity. Discovery input
//! must be identity candidates, not arbitrary metric time series.
use serde::{de, Deserialize, Deserializer, Serialize};
use std::{collections::BTreeMap, fmt};
use uuid::Uuid;

pub type Labels = BTreeMap<String, String>;
pub const MAX_SOURCE_ID_BYTES: usize = 256;
pub const MAX_SELECTOR_LABELS: usize = 32;
pub const MAX_LABEL_NAME_BYTES: usize = 128;
pub const MAX_LABEL_VALUE_BYTES: usize = 4096;
pub const MAX_SELECTOR_BYTES: usize = 16 * 1024;
pub const MAX_QUERY_BYTES: usize = 16 * 1024;
pub const MAX_CANDIDATES: usize = 1000;
pub const MAX_CANDIDATE_LABELS: usize = 64;
pub const MAX_DISCOVERY_BYTES: usize = 16 * 1024 * 1024;
pub const MAX_BINDING_JSON_BYTES: usize = 128 * 1024;
pub const MAX_ENROLLMENT_LABEL_BYTES: usize = 128;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MappingError {
    InvalidBinding,
    InvalidHostId,
    InvalidSourceId,
    InvalidSelector,
    InvalidMetric,
    InvalidDiscovery,
    BudgetExceeded,
    SourceMismatch,
    InvalidEnrollment,
    ConflictingMatcher,
}
impl fmt::Display for MappingError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::InvalidBinding => "Invalid host binding schema or revision",
            Self::InvalidHostId => "Invalid native host UUID",
            Self::InvalidSourceId => "Invalid monitoring source identifier",
            Self::InvalidSelector => "Invalid exact label selector",
            Self::InvalidMetric => "Invalid metric identifier",
            Self::InvalidDiscovery => "Invalid identity discovery labels",
            Self::BudgetExceeded => "Host mapping budget exceeded",
            Self::SourceMismatch => "Observed monitoring source differs from the binding",
            Self::InvalidEnrollment => "Invalid native enrollment labels",
            Self::ConflictingMatcher => "Panel matcher overlaps an explicit host selector",
        })
    }
}
impl std::error::Error for MappingError {}

fn valid_source(value: &str) -> bool {
    !value.is_empty() && value.len() <= MAX_SOURCE_ID_BYTES
}
fn label_name(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= MAX_LABEL_NAME_BYTES
        && value
            .bytes()
            .enumerate()
            .all(|(i, b)| b == b'_' || b.is_ascii_alphabetic() || (i > 0 && b.is_ascii_digit()))
}
fn metric_name(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= MAX_LABEL_NAME_BYTES
        && value.bytes().enumerate().all(|(i, b)| {
            b == b'_' || b == b':' || b.is_ascii_alphabetic() || (i > 0 && b.is_ascii_digit())
        })
}
fn selector_valid(labels: &Labels) -> Result<(), MappingError> {
    if labels.is_empty() || labels.len() > MAX_SELECTOR_LABELS {
        return Err(MappingError::InvalidSelector);
    }
    let mut bytes = 0usize;
    for (name, value) in labels {
        if !label_name(name)
            || name == "__name__"
            || value.is_empty()
            || value.len() > MAX_LABEL_VALUE_BYTES
        {
            return Err(MappingError::InvalidSelector);
        }
        bytes += name.len() + value.len();
        if bytes > MAX_SELECTOR_BYTES {
            return Err(MappingError::BudgetExceeded);
        }
    }
    Ok(())
}
fn parse_host(value: &str) -> Result<Uuid, MappingError> {
    let id = Uuid::parse_str(value).map_err(|_| MappingError::InvalidHostId)?;
    if id.is_nil() || value != id.hyphenated().to_string() {
        return Err(MappingError::InvalidHostId);
    }
    Ok(id)
}

/// Canonical camelCase version-1 DTO. Fields stay private so all instances are
/// validated. Deserialization rejects unknown fields and duplicate labels.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HostBinding {
    version: u32,
    host_id: String,
    source_id: String,
    revision: u64,
    selector: Labels,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct BindingWire {
    version: u32,
    host_id: String,
    source_id: String,
    revision: u64,
    #[serde(deserialize_with = "unique_selector")]
    selector: Labels,
}

fn unique_selector<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Labels, D::Error> {
    struct Unique;
    impl<'de> de::Visitor<'de> for Unique {
        type Value = Labels;
        fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.write_str("a bounded exact selector without duplicate keys")
        }
        fn visit_map<A: de::MapAccess<'de>>(self, mut map: A) -> Result<Labels, A::Error> {
            let mut values = Labels::new();
            let mut bytes = 0usize;
            while let Some((name, value)) = map.next_entry::<String, String>()? {
                bytes = bytes.saturating_add(name.len()).saturating_add(value.len());
                if values.len() >= MAX_SELECTOR_LABELS
                    || bytes > MAX_SELECTOR_BYTES
                    || values.insert(name, value).is_some()
                {
                    return Err(de::Error::custom("Invalid bounded exact selector"));
                }
            }
            Ok(values)
        }
    }
    deserializer.deserialize_map(Unique)
}

impl<'de> Deserialize<'de> for HostBinding {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = BindingWire::deserialize(deserializer)?;
        if value.version != 1 {
            return Err(de::Error::custom("Unsupported host binding version"));
        }
        Self::new(
            &value.host_id,
            &value.source_id,
            value.revision,
            value.selector,
        )
        .map_err(de::Error::custom)
    }
}

impl HostBinding {
    pub fn new(
        host_id: &str,
        source_id: &str,
        revision: u64,
        selector: Labels,
    ) -> Result<Self, MappingError> {
        parse_host(host_id)?;
        if !valid_source(source_id) {
            return Err(MappingError::InvalidSourceId);
        }
        if revision == 0 {
            return Err(MappingError::InvalidBinding);
        }
        selector_valid(&selector)?;
        Ok(Self {
            version: 1,
            host_id: host_id.into(),
            source_id: source_id.into(),
            revision,
            selector,
        })
    }
    /// Bounded wire entry point. A general serde deserializer still validates
    /// structure, but its caller must impose its own frame/allocation budget.
    pub fn from_json(raw: &[u8]) -> Result<Self, MappingError> {
        if raw.len() > MAX_BINDING_JSON_BYTES {
            return Err(MappingError::BudgetExceeded);
        }
        serde_json::from_slice(raw).map_err(|_| MappingError::InvalidBinding)
    }
    pub fn host_id(&self) -> Uuid {
        parse_host(&self.host_id).expect("validated host UUID")
    }
    pub fn source_id(&self) -> &str {
        &self.source_id
    }
    pub fn revision(&self) -> u64 {
        self.revision
    }
    pub fn selector(&self) -> &Labels {
        &self.selector
    }

    /// Render a metric identifier and sorted exact-equality matchers only.
    /// Values use JSON string escaping; no input becomes a PromQL operator.
    pub fn render_metric(&self, metric: &str) -> Result<String, MappingError> {
        if !metric_name(metric) {
            return Err(MappingError::InvalidMetric);
        }
        let mut query = format!("{metric}{{");
        for (index, (name, value)) in self.selector.iter().enumerate() {
            if index > 0 {
                query.push(',');
            }
            query.push_str(name);
            query.push('=');
            let escaped =
                serde_json::to_string(value).map_err(|_| MappingError::InvalidSelector)?;
            if query.len() + escaped.len() + 1 > MAX_QUERY_BYTES {
                return Err(MappingError::BudgetExceeded);
            }
            query.push_str(&escaped);
        }
        query.push('}');
        Ok(query)
    }

    /// Add fixed panel matchers without replacing any explicit identity label.
    /// Even an equal repeated label is rejected so callers cannot silently
    /// reinterpret user selectors. The combined selector keeps all budgets.
    pub fn render_metric_with_exact(
        &self,
        metric: &str,
        extra: &Labels,
    ) -> Result<String, MappingError> {
        if extra.keys().any(|name| self.selector.contains_key(name)) {
            return Err(MappingError::ConflictingMatcher);
        }
        if extra.len() + self.selector.len() > MAX_SELECTOR_LABELS {
            return Err(MappingError::BudgetExceeded);
        }
        if !extra.is_empty() {
            selector_valid(extra)?;
        }
        let mut merged = self.selector.clone();
        merged.extend(
            extra
                .iter()
                .map(|(key, value)| (key.clone(), value.clone())),
        );
        Self::new(&self.host_id, &self.source_id, self.revision, merged)?.render_metric(metric)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "camelCase", deny_unknown_fields)]
pub enum Resolution {
    Missing,
    Resolved {
        #[serde(rename = "candidateIndex")]
        candidate_index: usize,
    },
    Ambiguous {
        #[serde(rename = "candidateIndices")]
        candidate_indices: Vec<usize>,
    },
}

/// Match an explicit binding against identity discovery candidates from one
/// observed source. Do not deduplicate candidates or pass arbitrary series:
/// two equal label maps remain two ambiguous identities.
pub fn resolve(
    binding: &HostBinding,
    observed_source_id: &str,
    candidates: &[Labels],
) -> Result<Resolution, MappingError> {
    if !valid_source(observed_source_id) {
        return Err(MappingError::InvalidSourceId);
    }
    if observed_source_id != binding.source_id {
        return Err(MappingError::SourceMismatch);
    }
    if candidates.len() > MAX_CANDIDATES {
        return Err(MappingError::BudgetExceeded);
    }
    let mut total = 0usize;
    let mut matched = Vec::new();
    for (index, candidate) in candidates.iter().enumerate() {
        if candidate.len() > MAX_CANDIDATE_LABELS {
            return Err(MappingError::BudgetExceeded);
        }
        for (name, value) in candidate {
            // Discovery may include unrelated Unicode label names accepted by
            // newer backends. They remain opaque and cannot become selectors.
            if name.is_empty() || name.len() > 1024 || value.len() > MAX_LABEL_VALUE_BYTES {
                return Err(MappingError::InvalidDiscovery);
            }
            total = total.saturating_add(name.len()).saturating_add(value.len());
            if total > MAX_DISCOVERY_BYTES {
                return Err(MappingError::BudgetExceeded);
            }
        }
        if binding
            .selector
            .iter()
            .all(|(name, value)| candidate.get(name) == Some(value))
        {
            matched.push(index);
        }
    }
    Ok(match matched.len() {
        0 => Resolution::Missing,
        1 => Resolution::Resolved {
            candidate_index: matched[0],
        },
        _ => Resolution::Ambiguous {
            candidate_indices: matched,
        },
    })
}

/// Output of a trusted Native enrollment call, deliberately not Deserialize.
/// This type supplies labels only; it grants no authentication or host binding.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AgentEnrollmentLabels {
    labels: Labels,
}
impl AgentEnrollmentLabels {
    pub fn as_labels(&self) -> &Labels {
        &self.labels
    }
    pub fn into_labels(self) -> Labels {
        self.labels
    }
}

/// Native must call only after authorizing Agent enrollment for this host UUID.
/// Received exporter labels must never be passed as a substitute for that
/// trusted decision. UUID/site/environment labels are not enrollment evidence.
pub fn labels_for_trusted_enrollment(
    host_id: Uuid,
    site: &str,
    environment: &str,
) -> Result<AgentEnrollmentLabels, MappingError> {
    let valid = |value: &str| {
        !value.is_empty()
            && value.len() <= MAX_ENROLLMENT_LABEL_BYTES
            && !value.chars().any(char::is_control)
    };
    if host_id.is_nil() || !valid(site) || !valid(environment) {
        return Err(MappingError::InvalidEnrollment);
    }
    Ok(AgentEnrollmentLabels {
        labels: BTreeMap::from([
            ("cc_host_id".into(), host_id.hyphenated().to_string()),
            ("site".into(), site.into()),
            ("environment".into(), environment.into()),
        ]),
    })
}

#[cfg(test)]
mod tests;
