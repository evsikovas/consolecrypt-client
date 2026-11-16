//! Agent dataset decoder, not a grant, Prometheus client or UI transport.
use super::{Failure, Result};
use cc_crypto_core::agent_reader::*;
use serde::{de, Deserialize, Deserializer};
use serde_json::{Map, Value};

pub(super) struct PrivateDataset {
    pub target: AgentReaderHash,
    pub value: Value,
}
impl std::fmt::Debug for PrivateDataset {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("PrivateDataset([redacted])")
    }
}
fn exact(v: &Value, keys: &[&str]) -> Result<()> {
    let m = v.as_object().ok_or(Failure::ResponseInvalid)?;
    if m.len() != keys.len() || keys.iter().any(|k| !m.contains_key(*k)) {
        return Err(Failure::ResponseInvalid);
    }
    Ok(())
}
fn integer(v: &Value) -> Result<u64> {
    v.as_u64().ok_or(Failure::ResponseInvalid)
}
fn number(v: &Value) -> Result<f64> {
    let n = v.as_f64().ok_or(Failure::ResponseInvalid)?;
    if !n.is_finite() || !(0.0..=9007199254740992.0).contains(&n) {
        return Err(Failure::ResponseInvalid);
    }
    Ok(n)
}
fn text<'a>(v: &'a Value, allowed: &[&str]) -> Result<&'a str> {
    let s = v.as_str().ok_or(Failure::ResponseInvalid)?;
    if !allowed.contains(&s) {
        return Err(Failure::ResponseInvalid);
    }
    Ok(s)
}
fn flag(v: &Value) -> Result<bool> {
    v.as_bool().ok_or(Failure::ResponseInvalid)
}
fn sha(v: &Value) -> Result<()> {
    let s = v.as_str().ok_or(Failure::ResponseInvalid)?;
    if s.len() != 64
        || !s
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return Err(Failure::ResponseInvalid);
    }
    Ok(())
}

pub(super) fn decode(
    bytes: &[u8],
    target: &AgentReaderTargetDescriptorV1,
) -> Result<PrivateDataset> {
    target.validate().map_err(|_| Failure::Rejected)?;
    if bytes.is_empty() || bytes.len() > target.max_response_bytes as usize {
        return Err(Failure::ResponseInvalid);
    }
    let mut d = serde_json::Deserializer::from_slice(bytes);
    let Strict(value) = Strict::deserialize(&mut d).map_err(|_| Failure::ResponseInvalid)?;
    d.end().map_err(|_| Failure::ResponseInvalid)?;
    bounded(&value, 0)?;
    match target.dataset.kind {
        AgentReaderDatasetKindV1::HeartbeatV1 => heartbeat(&value, target)?,
        AgentReaderDatasetKindV1::LocalAgentHistoryV1 => history(&value, target, false)?,
        AgentReaderDatasetKindV1::FixedOsHistoryV1 => history(&value, target, true)?,
    }
    Ok(PrivateDataset {
        target: target.descriptor_hash().map_err(|_| Failure::Rejected)?,
        value,
    })
}
fn bounded(v: &Value, depth: usize) -> Result<()> {
    if depth > 16 {
        return Err(Failure::ResponseInvalid);
    }
    match v {
        Value::Number(n) if n.as_f64().is_none_or(|x| !x.is_finite()) => {
            return Err(Failure::ResponseInvalid)
        }
        Value::String(s) if s.len() > 4096 => return Err(Failure::ResponseInvalid),
        Value::Object(m) => {
            for v in m.values() {
                bounded(v, depth + 1)?;
            }
        }
        Value::Array(a) => {
            for v in a {
                bounded(v, depth + 1)?;
            }
        }
        _ => {}
    }
    Ok(())
}
fn heartbeat(v: &Value, t: &AgentReaderTargetDescriptorV1) -> Result<()> {
    exact(
        v,
        &[
            "schema_version",
            "agent_id",
            "host_id",
            "host_binding",
            "version",
            "uptime_seconds",
            "timestamp",
            "collection",
            "capabilities",
            "audit",
            "history_available_while_online_only",
        ],
    )?;
    if integer(&v["schema_version"])? != 1
        || v["agent_id"] != serde_json::json!(t.agent_id)
        || v["host_id"] != serde_json::json!(t.configured_host_id)
        || v["host_binding"] != "configured_not_identity_attested"
        || v["history_available_while_online_only"] != true
    {
        return Err(Failure::ResponseInvalid);
    }
    let version = v["version"].as_str().ok_or(Failure::ResponseInvalid)?;
    if version.is_empty()
        || version.len() > 64
        || !version
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b".+-".contains(&b))
    {
        return Err(Failure::ResponseInvalid);
    }
    number(&v["uptime_seconds"])?;
    number(&v["timestamp"])?;
    exact(
        &v["capabilities"],
        &["protocol", "telemetry.read", "local_history"],
    )?;
    if v["capabilities"]["protocol"] != "cc-agent-local/1"
        || integer(&v["capabilities"]["telemetry.read"])? != 1
        || integer(&v["capabilities"]["local_history"])? != 1
    {
        return Err(Failure::ResponseInvalid);
    }
    let c = v["collection"]
        .as_object()
        .ok_or(Failure::ResponseInvalid)?;
    if c.contains_key("backend") {
        native_collection(&v["collection"])?;
    } else {
        text(
            &v["collection"]["state"],
            &[
                "disabled", "starting", "running", "degraded", "stopped", "blocked", "unknown",
            ],
        )?;
        for (k, val) in c {
            match k.as_str() {
                "state" => {}
                "remote_write_production_allowed"
                | "remote_write_receiver_response_bounded"
                | "remote_write_receiver_destination_pinned" => {
                    if val != false {
                        return Err(Failure::ResponseInvalid);
                    }
                }
                "remote_write_block_reason" => {
                    text(val, &["pinned_collector_buffer_contract_incompatible"])?;
                }
                "remote_write_receiver_response_block_reason" => {
                    text(val, &["pinned_receiver_error_body_unbounded"])?;
                }
                "remote_write_receiver_destination_block_reason" => {
                    text(
                        val,
                        &["remote_write_receiver_redirect_destination_not_pinned"],
                    )?;
                }
                "last_scrape_success" => {
                    if !val.is_null() {
                        flag(val)?;
                    }
                }
                "lag_seconds"
                | "dropped_samples"
                | "remote_write_samples_dropped"
                | "remote_write_packets_dropped"
                | "pending_samples"
                | "buffer_bytes" => {
                    number(val)?;
                }
                _ => return Err(Failure::ResponseInvalid),
            }
        }
    }
    let audit = &v["audit"];
    if audit.get("state") == Some(&Value::String("external_sink".into())) {
        exact(audit, &["state"])?;
    } else {
        exact(
            audit,
            &[
                "state",
                "events",
                "evicted_events",
                "bytes",
                "budget_bytes",
                "tamper_proof",
            ],
        )?;
        text(
            &audit["state"],
            &[
                "memory_only",
                "ready",
                "corrupt_audit_refused",
                "write_failed_memory_only",
            ],
        )?;
        for k in ["events", "evicted_events", "bytes", "budget_bytes"] {
            integer(&audit[k])?;
        }
        if audit["tamper_proof"] != false {
            return Err(Failure::ResponseInvalid);
        }
    }
    Ok(())
}
// Diagnostic observations only; send attempts are never delivery acknowledgements.
fn native_collection(v: &Value) -> Result<()> {
    exact(v, &["state", "backend", "native_queue"])?;
    text(&v["backend"], &["prom-agent-hardmode-v1"])?;
    let state = text(&v["state"], &["unknown", "running", "degraded", "stopped", "blocked"])?;
    let queue = &v["native_queue"];
    const INTEGERS: &[&str] = &[
        "sample_send_attempts_total", "sample_retry_attempts_total",
        "sample_nonrecoverable_failures_total", "sample_pre_send_drops_total",
        "physical_evicted_segments_total", "charged_evicted_wal_bytes_total",
        "byte_evicted_segments_total", "age_evicted_segments_total",
        "shard_pending_samples", "last_validated_root_charge_bytes",
        "system_disk_low_available_rejections_total",
        "system_disk_observation_invalid_rejections_total",
    ];
    const FLOATS: &[&str] = &[
        "source_max_enqueued_timestamp_seconds", "source_max_reported_sent_timestamp_seconds",
        "oldest_retained_segment_residence_seconds",
    ];
    const FLAGS: &[&str] = &["global_unacknowledged_loss_unknown", "sticky_unavailable"];
    const UNKNOWN: &[&str] = &[
        "confirmed_accepted_requests_total", "confirmed_accepted_samples_total",
        "receiver_permanent_rejected_samples_total", "unacknowledged_samples_lost_total",
        "unacknowledged_delivery_lag_seconds",
    ];
    let fields: Vec<_> = INTEGERS.iter().chain(FLOATS).chain(FLAGS).chain(UNKNOWN).copied().collect();
    exact(queue, &fields)?;
    for &name in INTEGERS {
        if !queue[name].is_null() && integer(&queue[name])? >= (1_u64 << 53) {
            return Err(Failure::ResponseInvalid);
        }
    }
    for &name in FLOATS {
        if !queue[name].is_null() && number(&queue[name])? >= 9007199254740992.0 {
            return Err(Failure::ResponseInvalid);
        }
    }
    for &name in FLAGS {
        if !queue[name].is_null() {
            flag(&queue[name])?;
        }
    }
    if UNKNOWN.iter().any(|&name| !queue[name].is_null()) {
        return Err(Failure::ResponseInvalid);
    }
    if queue["sticky_unavailable"] == true && !["degraded", "stopped", "blocked"].contains(&state)
        || state == "running" && queue["sticky_unavailable"] != false
        || queue["last_validated_root_charge_bytes"].as_u64().is_some_and(|n| n > 512 * 1024 * 1024)
            && !["blocked", "stopped"].contains(&state)
    {
        return Err(Failure::ResponseInvalid);
    }
    Ok(())
}

const LOCAL: &[&str] = &["agent_uptime_seconds", "agent_collector_running"];
const OS: &[&str] = &[
    "node_memory_MemTotal_bytes",
    "node_memory_MemAvailable_bytes",
    "node_load1",
    "node_boot_time_seconds",
];
fn history(v: &Value, t: &AgentReaderTargetDescriptorV1, os: bool) -> Result<()> {
    let mut keys = vec![
        "metrics",
        "points",
        "truncated",
        "evicted_points",
        "stored_bytes",
        "budget_bytes",
        "effective_data_budget_bytes",
        "max_age_seconds",
        "actual_start",
        "actual_end",
        "storage_state",
        "available_while_agent_online_only",
    ];
    if os {
        keys.extend([
            "history_kind",
            "schema_version",
            "agent_id",
            "host_id",
            "resource_id",
            "restore_complete",
            "clock_state",
            "read_grants_collection",
            "production_append_available",
        ]);
    }
    exact(v, &keys)?;
    let descriptor = &t.dataset.descriptor;
    let expected = if os { OS } else { LOCAL };
    if v["metrics"] != serde_json::json!(expected) || v["available_while_agent_online_only"] != true
    {
        return Err(Failure::ResponseInvalid);
    }
    flag(&v["truncated"])?;
    integer(&v["evicted_points"])?;
    let budget = integer(&v["budget_bytes"])?;
    let effective = integer(&v["effective_data_budget_bytes"])?;
    if budget
        != integer(
            &descriptor[if os {
                "snapshot_reserved_bytes"
            } else {
                "max_bytes"
            }],
        )?
        || effective > budget / 2
        || integer(&v["stored_bytes"])? > effective
        || integer(&v["max_age_seconds"])? != integer(&descriptor["max_age_seconds"])?
    {
        return Err(Failure::ResponseInvalid);
    }
    if os {
        if v["history_kind"] != "fixed_os_snapshot"
            || integer(&v["schema_version"])? != 1
            || v["agent_id"] != serde_json::json!(t.agent_id)
            || v["host_id"] != serde_json::json!(t.configured_host_id)
            || v["resource_id"] != serde_json::json!(t.resource_id)
            || v["restore_complete"] != true
            || v["clock_state"] != "ok"
            || v["read_grants_collection"] != false
            || v["production_append_available"] != false
            || v["storage_state"] != "ready"
        {
            return Err(Failure::ResponseInvalid);
        }
    } else {
        text(
            &v["storage_state"],
            &[
                "memory",
                "ready",
                "oversize_history_refused",
                "corrupt_history_refused",
                "clock_rollback_paused",
                "write_failed_memory_only",
            ],
        )?;
        if effective != budget / 2 {
            return Err(Failure::ResponseInvalid);
        }
    }
    let points = v["points"].as_array().ok_or(Failure::ResponseInvalid)?;
    if points.len() > t.max_query_points as usize
        || points.len() > integer(&descriptor["max_points"])? as usize
    {
        return Err(Failure::ResponseInvalid);
    }
    let mut last = None;
    for p in points {
        let mut fields = vec!["timestamp"];
        fields.extend(expected.iter().copied());
        if os {
            fields.extend([
                "source_timestamps",
                "timestamp_kinds",
                "unknown_reasons",
                "provenance",
                "gap",
            ]);
        }
        // Optional diagnostics describe the SAME two Agent gauges. OS schema,
        // target/resource identity and numeric metric set remain unchanged.
        let collection_state = !os && (p.get("collection_state").is_some()
            || p.get("collection_state_version").is_some());
        if collection_state {
            fields.extend(["collection_state", "collection_state_version"]);
        }
        exact(p, &fields)?;
        let at = number(&p["timestamp"])?;
        if last.is_some_and(|v| at < v) {
            return Err(Failure::ResponseInvalid);
        }
        last = Some(at);
        if os {
            os_point(p, descriptor)?;
        } else {
            number(&p[LOCAL[0]])?;
            if !p[LOCAL[1]].is_null() && !matches!(p[LOCAL[1]].as_u64(), Some(0 | 1)) {
                return Err(Failure::ResponseInvalid);
            }
            if collection_state {
                if p["collection_state_version"].as_u64() != Some(1) {
                    return Err(Failure::ResponseInvalid);
                }
                let running = match p["collection_state"].as_str() {
                    Some("running") => Some(1_u64),
                    Some("disabled" | "stopped" | "not_running") => Some(0_u64),
                    Some("starting" | "degraded" | "blocked" | "unknown") => None,
                    _ => return Err(Failure::ResponseInvalid),
                };
                if p[LOCAL[1]] != serde_json::json!(running) {
                    return Err(Failure::ResponseInvalid);
                }
            }
        }
    }
    if let (Some(first), Some(last)) = (points.first(), points.last()) {
        if v["actual_start"] != first["timestamp"] || v["actual_end"] != last["timestamp"] {
            return Err(Failure::ResponseInvalid);
        }
    } else if !v["actual_start"].is_null() || !v["actual_end"].is_null() {
        return Err(Failure::ResponseInvalid);
    }
    Ok(())
}
fn os_point(p: &Value, descriptor: &Value) -> Result<()> {
    for key in ["source_timestamps", "timestamp_kinds", "unknown_reasons"] {
        exact(&p[key], OS)?;
    }
    let mut unknown = 0;
    for name in OS {
        let v = &p[*name];
        let at = &p["source_timestamps"][*name];
        let kind = &p["timestamp_kinds"][*name];
        let reason = &p["unknown_reasons"][*name];
        text(
            kind,
            &["unknown", "exporter_timestamp", "scrape_completed_at"],
        )?;
        if !at.is_null() {
            number(at)?;
        } else if kind != "unknown" {
            return Err(Failure::ResponseInvalid);
        }
        if v.is_null() {
            unknown += 1;
            text(
                reason,
                &[
                    "ok",
                    "partial",
                    "invalid_scrape_frame",
                    "scrape_binding_denied",
                    "failed_scrape",
                    "scrape_time_unknown_or_stale",
                    "response_budget_exceeded",
                    "unsupported_exposition_type",
                    "line_budget_exceeded",
                    "cardinality_budget_exceeded",
                    "label_budget_exceeded",
                    "cross_host_claim_denied",
                    "ambiguous_gauge_series",
                    "invalid_exposition",
                    "disabled",
                    "production_metrics_admission_unavailable",
                    "busy",
                    "collection_admission_denied",
                    "rate_limited",
                    "ready_scrape_transport_unavailable",
                    "collection_admission_expired",
                    "collection_unavailable",
                    "not_exposed",
                    "unsupported_metric_type",
                    "invalid_source_timestamp",
                    "stale_source_timestamp",
                    "invalid_gauge_value",
                ],
            )?;
        } else {
            number(v)?;
            if at.is_null() || kind == "unknown" || !reason.is_null() {
                return Err(Failure::ResponseInvalid);
            }
        }
    }
    if flag(&p["gap"])? != (unknown == 4) {
        return Err(Failure::ResponseInvalid);
    }
    let v = &p["provenance"];
    exact(
        v,
        &[
            "source",
            "endpoint_sha256",
            "plan_sha256",
            "parser",
            "parser_version",
            "parser_wheel_sha256",
            "host_binding",
        ],
    )?;
    if v["source"] != "ready_node_exporter_scrape"
        || v["plan_sha256"] != descriptor["plan_sha256"]
        || v["parser"] != "prometheus/client_python"
        || v["host_binding"] != "caller_mapping_not_exporter_attestation"
    {
        return Err(Failure::ResponseInvalid);
    }
    sha(&v["endpoint_sha256"])?;
    sha(&v["parser_wheel_sha256"])?;
    let version = v["parser_version"]
        .as_str()
        .ok_or(Failure::ResponseInvalid)?;
    if version.is_empty()
        || version.len() > 32
        || !version.bytes().all(|b| b.is_ascii_digit() || b == b'.')
    {
        return Err(Failure::ResponseInvalid);
    }
    Ok(())
}

struct Strict(Value);
impl<'de> Deserialize<'de> for Strict {
    fn deserialize<D: Deserializer<'de>>(d: D) -> std::result::Result<Self, D::Error> {
        struct V;
        impl<'de> de::Visitor<'de> for V {
            type Value = Strict;
            fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                f.write_str("bounded reader dataset")
            }
            fn visit_bool<E: de::Error>(self, v: bool) -> std::result::Result<Strict, E> {
                Ok(Strict(v.into()))
            }
            fn visit_u64<E: de::Error>(self, v: u64) -> std::result::Result<Strict, E> {
                Ok(Strict(v.into()))
            }
            fn visit_i64<E: de::Error>(self, v: i64) -> std::result::Result<Strict, E> {
                Ok(Strict(v.into()))
            }
            fn visit_f64<E: de::Error>(self, v: f64) -> std::result::Result<Strict, E> {
                Ok(Strict(
                    serde_json::Number::from_f64(v)
                        .ok_or_else(|| E::custom("invalid number"))?
                        .into(),
                ))
            }
            fn visit_str<E: de::Error>(self, v: &str) -> std::result::Result<Strict, E> {
                Ok(Strict(v.into()))
            }
            fn visit_string<E: de::Error>(self, v: String) -> std::result::Result<Strict, E> {
                Ok(Strict(v.into()))
            }
            fn visit_unit<E: de::Error>(self) -> std::result::Result<Strict, E> {
                Ok(Strict(Value::Null))
            }
            fn visit_none<E: de::Error>(self) -> std::result::Result<Strict, E> {
                Ok(Strict(Value::Null))
            }
            fn visit_seq<A: de::SeqAccess<'de>>(
                self,
                mut a: A,
            ) -> std::result::Result<Strict, A::Error> {
                let mut v = Vec::new();
                while let Some(x) = a.next_element::<Strict>()? {
                    v.push(x.0);
                }
                Ok(Strict(v.into()))
            }
            fn visit_map<A: de::MapAccess<'de>>(
                self,
                mut a: A,
            ) -> std::result::Result<Strict, A::Error> {
                let mut v = Map::new();
                while let Some(k) = a.next_key::<String>()? {
                    if v.contains_key(&k) {
                        return Err(de::Error::custom("duplicate field"));
                    }
                    v.insert(k, a.next_value::<Strict>()?.0);
                }
                Ok(Strict(v.into()))
            }
        }
        d.deserialize_any(V)
    }
}
