//! MON-08 read-only backend status. Rules execute on Prometheus, never here.
//! Backend labels, annotations, query/file names and states are untrusted text.
use crate::*;
use serde::Serialize;
use serde_json::{Map, Value};
use std::collections::BTreeMap;

pub const MAX_RULE_GROUPS: usize = 100;
pub const MAX_ALERT_RESPONSE_BYTES: usize = 2 * 1024 * 1024;
pub const MAX_ALERT_MESSAGES: usize = 100;
pub const MAX_ALERT_RULES: usize = 1000;
pub const MAX_ACTIVE_ALERTS: usize = 1000;
pub const MAX_LABELS_PER_OBJECT: usize = 64;
pub const MAX_ALERT_LABEL_BYTES: usize = 1024 * 1024;
const MAX_TEXT_BYTES: usize = 4096;

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum AlertState {
    Inactive,
    Pending,
    Firing,
    Unknown { value: Option<String> },
}
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum RuleHealth {
    Ok,
    Error,
    Unknown { value: Option<String> },
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ActiveAlert {
    pub labels: BTreeMap<String, String>,
    pub annotations: BTreeMap<String, String>,
    pub state: AlertState,
    pub value: SampleValue,
    pub active_at: Option<String>,
    pub keep_firing_since: Option<String>,
    pub unsupported_fields: Vec<String>,
}
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AlertRule {
    pub name: String,
    /// Display-only PromQL; never executed or treated as a URL by this API.
    pub query: String,
    pub state: AlertState,
    pub health: RuleHealth,
    pub duration_seconds: Option<f64>,
    pub keep_firing_for_seconds: Option<f64>,
    pub labels: BTreeMap<String, String>,
    pub annotations: BTreeMap<String, String>,
    pub alerts: Vec<ActiveAlert>,
    pub last_error: Option<String>,
    pub evaluation_time_seconds: Option<f64>,
    pub last_evaluation: Option<String>,
    pub unsupported_fields: Vec<String>,
}
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AlertRuleGroup {
    pub name: String,
    /// Opaque backend path for display; never opened on the client filesystem.
    pub file: String,
    pub rules: Vec<AlertRule>,
    pub interval_seconds: Option<f64>,
    pub limit: Option<u64>,
    pub evaluation_time_seconds: Option<f64>,
    pub last_evaluation: Option<String>,
    pub unsupported_fields: Vec<String>,
}
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ActiveAlerts {
    pub alerts: Vec<ActiveAlert>,
    pub unsupported_fields: Vec<String>,
}
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AlertRuleGroups {
    pub groups: Vec<AlertRuleGroup>,
    /// Opaque continuation marker only. No automatic follow-up fetch occurs.
    pub group_next_token: Option<String>,
    pub unsupported_fields: Vec<String>,
}
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AlertReadResponse<T> {
    pub status: String,
    pub http_status: u16,
    pub data: Option<T>,
    pub warnings: Vec<String>,
    pub infos: Vec<String>,
    pub partial: bool,
    pub error: Option<ApiError>,
}
pub type AlertResponse = AlertReadResponse<ActiveAlerts>;
pub type AlertRulesResponse = AlertReadResponse<AlertRuleGroups>;

impl MonitoringClient {
    pub async fn alerts(&self) -> Result<AlertResponse, Error> {
        let (value, status) = self
            .request_with_response_budget("api/v1/alerts", vec![], false, MAX_ALERT_RESPONSE_BYTES)
            .await?;
        let result = envelope(value, status, |data| {
            let object = object(data)?;
            let mut budget = ParseBudget::default();
            Ok(ActiveAlerts {
                alerts: parse_alerts(required(object, "alerts")?, &mut budget)?,
                unsupported_fields: unknown_fields(object, &["alerts"])?,
            })
        })
        .map_err(|e| e.with_http(status))?;
        self.ensure_authorization()?;
        Ok(result)
    }
    pub async fn alert_rules(&self) -> Result<AlertRulesResponse, Error> {
        let (value, status) = self
            .request_with_response_budget(
                "api/v1/rules",
                vec![
                    ("type", "alert".into()),
                    ("group_limit", (MAX_RULE_GROUPS + 1).to_string()),
                ],
                false,
                MAX_ALERT_RESPONSE_BYTES,
            )
            .await?;
        let mut result = envelope(value, status, parse_groups).map_err(|e| e.with_http(status))?;
        if result
            .data
            .as_ref()
            .is_some_and(|d| d.group_next_token.is_some())
        {
            result.partial = true;
        }
        self.ensure_authorization()?;
        Ok(result)
    }
}

fn invalid() -> Error {
    Error::new(
        ErrorCode::InvalidResponse,
        "Invalid alert/rule API response",
    )
}
fn exceeded() -> Error {
    Error::new(
        ErrorCode::BudgetExceeded,
        "Alert/rule response exceeds configured budget",
    )
}
fn object(value: &Value) -> Result<&Map<String, Value>, Error> {
    value.as_object().ok_or_else(invalid)
}
fn required<'a>(value: &'a Map<String, Value>, key: &str) -> Result<&'a Value, Error> {
    value.get(key).ok_or_else(invalid)
}
fn text(value: &Value, max: usize) -> Result<String, Error> {
    let value = value.as_str().ok_or_else(invalid)?;
    if value.len() > max {
        return Err(exceeded());
    }
    Ok(value.into())
}
fn optional_text(
    object: &Map<String, Value>,
    key: &str,
    max: usize,
) -> Result<Option<String>, Error> {
    object.get(key).map(|value| text(value, max)).transpose()
}
fn timestamp(object: &Map<String, Value>, key: &str) -> Result<Option<String>, Error> {
    let value = optional_text(object, key, 128)?;
    if let Some(value) = &value {
        chrono::DateTime::parse_from_rfc3339(value).map_err(|_| invalid())?;
    }
    Ok(value)
}
fn duration(object: &Map<String, Value>, key: &str) -> Result<Option<f64>, Error> {
    object
        .get(key)
        .map(|value| {
            value
                .as_f64()
                .filter(|v| v.is_finite() && *v >= 0.0)
                .ok_or_else(invalid)
        })
        .transpose()
}
fn state(object: &Map<String, Value>) -> Result<AlertState, Error> {
    Ok(match optional_text(object, "state", 64)? {
        Some(v) if v == "inactive" => AlertState::Inactive,
        Some(v) if v == "pending" => AlertState::Pending,
        Some(v) if v == "firing" => AlertState::Firing,
        value => AlertState::Unknown { value },
    })
}
fn health(object: &Map<String, Value>) -> Result<RuleHealth, Error> {
    Ok(match optional_text(object, "health", 64)? {
        Some(v) if v == "ok" => RuleHealth::Ok,
        Some(v) if v == "err" => RuleHealth::Error,
        value => RuleHealth::Unknown { value },
    })
}
fn unknown_fields(object: &Map<String, Value>, known: &[&str]) -> Result<Vec<String>, Error> {
    let fields = object
        .keys()
        .filter(|key| !known.contains(&key.as_str()))
        .collect::<Vec<_>>();
    if fields.len() > 32 || fields.iter().any(|key| key.len() > 128) {
        return Err(exceeded());
    }
    Ok(fields.into_iter().cloned().collect())
}
#[derive(Default)]
struct ParseBudget {
    rules: usize,
    alerts: usize,
    label_bytes: usize,
}
fn labels(
    value: Option<&Value>,
    budget: &mut ParseBudget,
) -> Result<BTreeMap<String, String>, Error> {
    let Some(value) = value else {
        return Ok(BTreeMap::new());
    };
    let object = object(value)?;
    if object.len() > MAX_LABELS_PER_OBJECT {
        return Err(exceeded());
    }
    let mut labels = BTreeMap::new();
    for (key, value) in object {
        if key.is_empty() || key.len() > 1024 {
            return Err(exceeded());
        }
        let value = text(value, MAX_TEXT_BYTES)?;
        budget.label_bytes = budget
            .label_bytes
            .saturating_add(key.len())
            .saturating_add(value.len());
        if budget.label_bytes > MAX_ALERT_LABEL_BYTES {
            return Err(exceeded());
        }
        labels.insert(key.clone(), value);
    }
    Ok(labels)
}
fn alert_value(value: &Value) -> Result<SampleValue, Error> {
    Ok(match value.as_str().ok_or_else(invalid)? {
        "NaN" => SampleValue::NaN,
        "Inf" | "+Inf" => SampleValue::PositiveInfinity,
        "-Inf" => SampleValue::NegativeInfinity,
        value => SampleValue::Finite {
            value: value
                .parse::<f64>()
                .ok()
                .filter(|v| v.is_finite())
                .ok_or_else(invalid)?,
        },
    })
}
fn parse_alerts(value: &Value, budget: &mut ParseBudget) -> Result<Vec<ActiveAlert>, Error> {
    let values = value.as_array().ok_or_else(invalid)?;
    budget.alerts = budget.alerts.saturating_add(values.len());
    if budget.alerts > MAX_ACTIVE_ALERTS {
        return Err(exceeded());
    }
    values
        .iter()
        .map(|value| {
            let value = object(value)?;
            Ok(ActiveAlert {
                labels: labels(Some(required(value, "labels")?), budget)?,
                annotations: labels(value.get("annotations"), budget)?,
                state: state(value)?,
                value: alert_value(required(value, "value")?)?,
                active_at: timestamp(value, "activeAt")?,
                keep_firing_since: timestamp(value, "keepFiringSince")?,
                unsupported_fields: unknown_fields(
                    value,
                    &[
                        "labels",
                        "annotations",
                        "state",
                        "value",
                        "activeAt",
                        "keepFiringSince",
                    ],
                )?,
            })
        })
        .collect()
}
fn parse_rule(value: &Value, budget: &mut ParseBudget) -> Result<AlertRule, Error> {
    budget.rules += 1;
    if budget.rules > MAX_ALERT_RULES {
        return Err(exceeded());
    }
    let value = object(value)?;
    if required(value, "type")?.as_str() != Some("alerting") {
        return Err(Error::new(
            ErrorCode::UnsupportedResult,
            "Rule response contains a non-alerting rule",
        ));
    }
    Ok(AlertRule {
        name: text(required(value, "name")?, MAX_TEXT_BYTES)?,
        query: text(required(value, "query")?, 16 * 1024)?,
        state: state(value)?,
        health: health(value)?,
        duration_seconds: duration(value, "duration")?,
        keep_firing_for_seconds: duration(value, "keepFiringFor")?,
        labels: labels(value.get("labels"), budget)?,
        annotations: labels(value.get("annotations"), budget)?,
        alerts: match value.get("alerts") {
            Some(value) => parse_alerts(value, budget)?,
            None => vec![],
        },
        last_error: optional_text(value, "lastError", MAX_TEXT_BYTES)?,
        evaluation_time_seconds: duration(value, "evaluationTime")?,
        last_evaluation: timestamp(value, "lastEvaluation")?,
        unsupported_fields: unknown_fields(
            value,
            &[
                "name",
                "query",
                "type",
                "state",
                "health",
                "duration",
                "keepFiringFor",
                "labels",
                "annotations",
                "alerts",
                "lastError",
                "evaluationTime",
                "lastEvaluation",
            ],
        )?,
    })
}
fn parse_groups(value: &Value) -> Result<AlertRuleGroups, Error> {
    let value = object(value)?;
    let groups = required(value, "groups")?.as_array().ok_or_else(invalid)?;
    if groups.len() > MAX_RULE_GROUPS {
        return Err(exceeded());
    }
    let mut budget = ParseBudget::default();
    let groups = groups
        .iter()
        .map(|value| {
            let value = object(value)?;
            let rules = required(value, "rules")?.as_array().ok_or_else(invalid)?;
            if rules.len() > MAX_ALERT_RULES.saturating_sub(budget.rules) {
                return Err(exceeded());
            }
            Ok(AlertRuleGroup {
                name: text(required(value, "name")?, MAX_TEXT_BYTES)?,
                file: text(required(value, "file")?, MAX_TEXT_BYTES)?,
                rules: rules
                    .iter()
                    .map(|value| parse_rule(value, &mut budget))
                    .collect::<Result<Vec<_>, _>>()?,
                interval_seconds: duration(value, "interval")?,
                limit: value
                    .get("limit")
                    .map(|value| value.as_u64().ok_or_else(invalid))
                    .transpose()?,
                evaluation_time_seconds: duration(value, "evaluationTime")?,
                last_evaluation: timestamp(value, "lastEvaluation")?,
                unsupported_fields: unknown_fields(
                    value,
                    &[
                        "name",
                        "file",
                        "rules",
                        "interval",
                        "limit",
                        "evaluationTime",
                        "lastEvaluation",
                    ],
                )?,
            })
        })
        .collect::<Result<Vec<_>, Error>>()?;
    Ok(AlertRuleGroups {
        groups,
        group_next_token: optional_text(value, "groupNextToken", MAX_TEXT_BYTES)?
            .filter(|v| !v.is_empty()),
        unsupported_fields: unknown_fields(value, &["groups", "groupNextToken"])?,
    })
}
fn envelope<T>(
    value: Value,
    http_status: u16,
    decode: impl FnOnce(&Value) -> Result<T, Error>,
) -> Result<AlertReadResponse<T>, Error> {
    let value = object(&value)?;
    let status = required(value, "status")?.as_str().ok_or_else(invalid)?;
    if !matches!(status, "success" | "error")
        || (status == "success" && !(200..300).contains(&http_status))
    {
        return Err(invalid());
    }
    let warnings = messages(value.get("warnings"))?;
    let infos = messages(value.get("infos"))?;
    let error = if status == "error" {
        Some(ApiError {
            r#type: text(required(value, "errorType")?, MAX_TEXT_BYTES)?,
            message: text(required(value, "error")?, MAX_TEXT_BYTES)?,
        })
    } else {
        None
    };
    let data = value
        .get("data")
        .filter(|v| !v.is_null())
        .map(decode)
        .transpose()?;
    if status == "success" && data.is_none() {
        return Err(invalid());
    }
    let partial = !warnings.is_empty() || (error.is_some() && data.is_some());
    Ok(AlertReadResponse {
        status: status.into(),
        http_status,
        data,
        warnings,
        infos,
        partial,
        error,
    })
}

fn messages(value: Option<&Value>) -> Result<Vec<String>, Error> {
    let Some(value) = value else {
        return Ok(vec![]);
    };
    let values = value.as_array().ok_or_else(invalid)?;
    if values.len() > MAX_ALERT_MESSAGES {
        return Err(exceeded());
    }
    let result = values
        .iter()
        .map(|value| text(value, MAX_TEXT_BYTES))
        .collect::<Result<Vec<_>, _>>()?;
    if result.iter().map(String::len).sum::<usize>() > 256 * 1024 {
        return Err(exceeded());
    }
    Ok(result)
}

#[cfg(test)]
mod tests;
