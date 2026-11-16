//! Fixed queries yield observations, never exporter/host identity attestation.
use crate::{Error, ErrorCode, MonitoringClient, QueryData, QueryOutcome, SampleValue};
use serde::{Deserialize, Serialize, de};
use std::{collections::BTreeMap, fmt};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ExporterKind {
    NodeExporter,
    Cadvisor,
}

/// Exact nonempty reviewed labels. No PromQL, authority or received labels.
#[derive(Clone)]
pub struct ExporterSelector(BTreeMap<String, String>);
impl fmt::Debug for ExporterSelector {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("ExporterSelector(<redacted>)")
    }
}
impl ExporterSelector {
    pub fn new(labels: BTreeMap<String, String>) -> Result<Self, Error> {
        if labels.is_empty()
            || labels.len() > 32
            || !labels.contains_key("job")
            || !labels.contains_key("instance")
        {
            return Err(invalid());
        }
        let mut bytes = 0;
        for (key, value) in &labels {
            bytes += key.len() + value.len();
            let mut chars = key.bytes();
            let name = chars
                .next()
                .is_some_and(|c| c.is_ascii_alphabetic() || c == b'_')
                && chars.all(|c| c.is_ascii_alphanumeric() || c == b'_');
            if key.len() > 128
                || !name
                || key == "__name__"
                || value.is_empty()
                || value.len() > 4096
                || value.chars().any(|c| c.is_control())
                || bytes > 16384
            {
                return Err(invalid());
            }
        }
        Ok(Self(labels))
    }
}
impl<'de> Deserialize<'de> for ExporterSelector {
    fn deserialize<D: de::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct Visitor;
        impl<'de> de::Visitor<'de> for Visitor {
            type Value = ExporterSelector;
            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                f.write_str("bounded exact labels")
            }
            fn visit_map<A: de::MapAccess<'de>>(
                self,
                mut input: A,
            ) -> Result<Self::Value, A::Error> {
                let mut labels = BTreeMap::new();
                let mut bytes = 0;
                while let Some((key, value)) = input.next_entry::<String, String>()? {
                    bytes += key.len() + value.len();
                    if labels.len() >= 32 || bytes > 16384 || labels.insert(key, value).is_some() {
                        return Err(de::Error::custom("invalid exporter selector"));
                    }
                }
                ExporterSelector::new(labels)
                    .map_err(|_| de::Error::custom("invalid exporter selector"))
            }
        }
        deserializer.deserialize_map(Visitor)
    }
}
fn invalid() -> Error {
    Error::new(
        ErrorCode::InvalidQuery,
        "Invalid exporter verification plan",
    )
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Probe {
    Up,
    UpAge,
    CpuAge,
    MemoryAge,
    AvailableAge,
}
pub struct ExporterVerificationPlan {
    kind: ExporterKind,
    selector: ExporterSelector,
    max_age: u16,
    queries: Vec<(Probe, String)>,
}
impl fmt::Debug for ExporterVerificationPlan {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ExporterVerificationPlan")
            .field("kind", &self.kind)
            .finish_non_exhaustive()
    }
}
impl ExporterVerificationPlan {
    pub fn new(
        kind: ExporterKind,
        selector: ExporterSelector,
        max_age: u16,
    ) -> Result<Self, Error> {
        if !(5..=300).contains(&max_age)
            || (kind == ExporterKind::Cadvisor && selector.0.contains_key("image"))
        {
            return Err(invalid());
        }
        let labels = selector
            .0
            .iter()
            .map(|(key, value)| {
                format!(
                    "{key}={}",
                    serde_json::to_string(value).expect("String is JSON")
                )
            })
            .collect::<Vec<_>>()
            .join(",");
        let metric = |name: &str| {
            format!(
                "{name}{{{labels}{}}}",
                if name.starts_with("container_") {
                    ",image!=\"\""
                } else {
                    ""
                }
            )
        };
        let age = |name: &str| format!("max(time() - timestamp({}))", metric(name));
        let mut queries = vec![
            (Probe::Up, metric("up")),
            (Probe::UpAge, age("up")),
            (
                Probe::CpuAge,
                age(if kind == ExporterKind::NodeExporter {
                    "node_cpu_seconds_total"
                } else {
                    "container_cpu_usage_seconds_total"
                }),
            ),
            (
                Probe::MemoryAge,
                age(if kind == ExporterKind::NodeExporter {
                    "node_memory_MemTotal_bytes"
                } else {
                    "container_memory_working_set_bytes"
                }),
            ),
        ];
        if kind == ExporterKind::NodeExporter {
            queries.push((Probe::AvailableAge, age("node_memory_MemAvailable_bytes")));
        }
        if queries.iter().any(|(_, q)| q.len() > 16384) {
            return Err(invalid());
        }
        Ok(Self {
            kind,
            selector,
            max_age,
            queries,
        })
    }
    pub async fn execute(&self, client: &MonitoringClient) -> ExporterObservation {
        let mut outcomes = Vec::with_capacity(self.queries.len());
        for (probe, query) in &self.queries {
            let outcome = match client.query(query, None).await {
                Ok(value) => value,
                Err(_) => {
                    return ExporterObservation::Unavailable {
                        reason: ExporterUnavailable::ReadFailed,
                    };
                }
            };
            outcomes.push((*probe, outcome));
        }
        self.observe(&outcomes)
            .unwrap_or_else(|reason| ExporterObservation::Unavailable { reason })
    }
    fn observe(
        &self,
        outcomes: &[(Probe, QueryOutcome)],
    ) -> Result<ExporterObservation, ExporterUnavailable> {
        use ExporterUnavailable::*;
        if outcomes.len() != self.queries.len() {
            return Err(IncompleteResponse);
        }
        let mut evaluations = Vec::with_capacity(outcomes.len());
        let mut ages = Vec::with_capacity(outcomes.len());
        for ((expected, _), (probe, outcome)) in self.queries.iter().zip(outcomes) {
            if expected != probe
                || outcome.status != "success"
                || outcome.http_status != 200
                || outcome.error.is_some()
                || outcome.partial
                || !outcome.warnings.is_empty()
                || !outcome.infos.is_empty()
            {
                return Err(IncompleteResponse);
            }
            let Some(QueryData::Vector { series }) = &outcome.data else {
                return Err(InvalidResponse);
            };
            if series.is_empty() {
                return Err(MissingSeries);
            }
            if series.len() != 1 {
                return Err(AmbiguousTarget);
            }
            let row = &series[0];
            if row.samples.len() != 1
                || row.labels.len() > 64
                || row
                    .labels
                    .iter()
                    .map(|(k, v)| k.len() + v.len())
                    .sum::<usize>()
                    > 16384
            {
                return Err(InvalidResponse);
            }
            let sample = &row.samples[0];
            let value = match &sample.value {
                SampleValue::Finite { value } => *value,
                _ => return Err(InvalidResponse),
            };
            if !sample.timestamp.is_finite()
                || !(0.0..=253_402_300_799.0).contains(&sample.timestamp)
                || !value.is_finite()
            {
                return Err(InvalidResponse);
            }
            evaluations.push(sample.timestamp);
            if *probe == Probe::Up {
                if !self
                    .selector
                    .0
                    .iter()
                    .all(|(key, value)| row.labels.get(key) == Some(value))
                {
                    return Err(InvalidResponse);
                }
                if value != 1.0 {
                    return Err(TargetDown);
                }
            } else {
                if !row.labels.is_empty() {
                    return Err(InvalidResponse);
                }
                if value < 0.0 {
                    return Err(ClockUnknown);
                }
                ages.push((value, sample.timestamp));
            }
        }
        let latest = evaluations.iter().copied().fold(0.0, f64::max);
        let earliest = evaluations.iter().copied().fold(f64::INFINITY, f64::min);
        if latest - earliest > 5.0 {
            return Err(ClockUnknown);
        }
        let max_age = ages
            .iter()
            .map(|(age, time)| age + latest - time)
            .fold(0.0, f64::max);
        if max_age > f64::from(self.max_age) {
            return Err(StaleSeries);
        }
        Ok(ExporterObservation::Observed {
            max_sample_age_seconds: max_age,
            evaluated_at_source_seconds: latest,
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ExporterUnavailable {
    ReadFailed,
    IncompleteResponse,
    InvalidResponse,
    MissingSeries,
    AmbiguousTarget,
    TargetDown,
    StaleSeries,
    ClockUnknown,
}
/// Untrusted source observations, not a host/installed-version attestation.
#[derive(Clone, Debug, Serialize)]
#[serde(
    tag = "state",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum ExporterObservation {
    Observed {
        max_sample_age_seconds: f64,
        evaluated_at_source_seconds: f64,
    },
    Unavailable {
        reason: ExporterUnavailable,
    },
}

#[cfg(test)]
mod tests;
