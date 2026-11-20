//! Closed DATA-only historical result projection. No outcome reconciliation,
//! retry, new scope, arbitrary error text or server-log integrity claim.
use cc_models::automation::is_digest;
use serde_json::{Map, Value, json};
use std::collections::BTreeSet;
use uuid::Uuid;

const MAX_INTEGER: u64 = 9_007_199_254_740_991;
fn fields(raw: &Value, names: &[&str], valid: impl Fn(&str, &Value) -> bool) -> Map<String, Value> {
    let mut out = Map::new();
    for name in names {
        if let Some(value) = raw.get(*name) {
            if valid(name, value) {
                out.insert((*name).into(), value.clone());
            }
        }
    }
    out
}
fn number(v: &Value) -> bool {
    v.is_null() || v.as_u64().is_some_and(|x| x <= MAX_INTEGER)
}
fn flag(v: &Value) -> bool {
    v.is_null() || v.is_boolean()
}
fn code(v: &Value) -> bool {
    v.is_null()
        || v.as_i64()
            .is_some_and(|x| (-2_147_483_648..=2_147_483_647).contains(&x))
}
fn utc(v: &Value) -> bool {
    v.is_null()
        || v.as_i64()
            .is_some_and(|n| (1..=253_402_300_799).contains(&n))
        || v.as_str().is_some_and(|s| {
            s.len() <= 64
                && s.ends_with('Z')
                && chrono::DateTime::parse_from_rfc3339(s)
                    .is_ok_and(|t| t.timestamp() > 0 && t.timestamp() <= 253_402_300_799)
        })
}
fn identifier(v: &Value) -> bool {
    v.as_str()
        .is_some_and(|s| Uuid::parse_str(s).is_ok_and(|x| !x.is_nil() && x.to_string() == s))
}
fn version(v: &Value) -> bool {
    v == &json!(1)
}
fn reason(v: &Value) -> bool {
    v.is_null()
        || v.as_str().is_some_and(|s| {
            matches!(
                s,
                "control_channel_or_storage_lost"
                    | "native_operation_not_acknowledged"
                    | "vault_material_unavailable_before_dispatch"
                    | "vault_material_expired_before_dispatch"
                    | "approval_expired_in_queue"
                    | "helper_restarted"
                    | "preflight_changed"
                    | "executor_lost"
                    | "client_restarted_before_dispatch"
            )
        })
}
fn interval(raw: &Value) -> Value {
    if raw.is_null() || raw == &json!({"redacted":true}) {
        return raw.clone();
    }
    let expected = [
        "version",
        "clockSource",
        "scope",
        "spawnObservedUtc",
        "exitObservedUtc",
        "elapsedMonotonicMs",
    ];
    let Some(map) = raw.as_object() else {
        return json!({"redacted":true});
    };
    if map.len() != 6
        || expected.iter().any(|k| !map.contains_key(*k))
        || !version(&raw["version"])
        || raw["clockSource"] != "runner-wall"
        || raw["scope"] != "runner-process"
        || !raw["spawnObservedUtc"].is_string()
        || !utc(&raw["spawnObservedUtc"])
        || !raw["exitObservedUtc"].is_string()
        || !utc(&raw["exitObservedUtc"])
        || raw["elapsedMonotonicMs"]
            .as_u64()
            .is_none_or(|x| x > MAX_INTEGER)
    {
        json!({"redacted":true})
    } else {
        raw.clone()
    }
}
fn safe_version(v: &Value) -> bool {
    v.as_str().is_some_and(|s| {
        let parts = s.split('.').collect::<Vec<_>>();
        s.len() <= 32
            && (2..=4).contains(&parts.len())
            && parts
                .iter()
                .all(|p| !p.is_empty() && p.len() <= 8 && p.bytes().all(|b| b.is_ascii_digit()))
    })
}

fn environment(raw: &Value) -> Value {
    if raw.is_null() || raw == &json!({"redacted":true}) {
        return raw.clone();
    }
    if !raw.is_object() {
        return json!({"redacted":true});
    }
    let mut out = fields(
        raw,
        &["version", "environment_digest"],
        |name, v| match name {
            "version" => version(v),
            "environment_digest" => v.as_str().is_some_and(is_digest),
            _ => false,
        },
    );
    if let Some(identity) = raw.get("identity").filter(|v| v.is_object()) {
        let mut value = fields(
            identity,
            &["system", "machine", "python", "helper"],
            |name, v| match name {
                "system" => v
                    .as_str()
                    .is_some_and(|s| matches!(s, "Linux" | "Darwin" | "Windows")),
                "machine" => v
                    .as_str()
                    .is_some_and(|s| matches!(s, "x86_64" | "aarch64" | "arm64" | "AMD64")),
                _ => safe_version(v),
            },
        );
        if let Some(packages) = identity.get("packages").filter(|v| v.is_object()) {
            let mut selected = Map::new();
            for name in ["ansible-core", "ansible-runner", "ansible-lint"] {
                if let Some(package) = packages.get(name) {
                    let one = fields(package, &["actual"], |_, v| safe_version(v));
                    selected.insert(name.into(), Value::Object(one));
                }
            }
            value.insert("packages".into(), Value::Object(selected));
        }
        out.insert("identity".into(), Value::Object(value));
    }
    Value::Object(out)
}
fn result(raw: &Value, aliases: &BTreeSet<String>) -> Value {
    if raw.is_null() || raw == &json!({"redacted":true}) {
        return raw.clone();
    }
    if !raw.is_object() {
        return json!({"redacted":true});
    }
    let mut out = fields(
        raw,
        &[
            "exit_code",
            "timed_out",
            "cancel_requested",
            "remote_changes_may_be_partial",
            "partial",
            "started",
            "elapsed_ms",
            "elapsed",
            "reason",
        ],
        |name, v| match name {
            "exit_code" => code(v),
            "timed_out"
            | "cancel_requested"
            | "remote_changes_may_be_partial"
            | "partial"
            | "started" => flag(v),
            "elapsed_ms" => number(v),
            "elapsed" => {
                v.is_null()
                    || v.as_f64()
                        .is_some_and(|n| n.is_finite() && n >= 0.0 && n <= MAX_INTEGER as f64)
            }
            "reason" => reason(v),
            _ => false,
        },
    );
    if let Some(value) = raw.get("summary") {
        out.insert(
            "summary".into(),
            if value.is_null() {
                Value::Null
            } else {
                cc_automation_core::structural_summary(value, aliases)
            },
        );
    }
    if let Some(value) = raw.get("runner_execution") {
        out.insert("runner_execution".into(), interval(value));
    }
    Value::Object(out)
}
fn retirement(raw: &Value) -> Value {
    if raw.is_null() || raw == &json!({"redacted":true}) {
        return raw.clone();
    }
    Value::Object(fields(
        raw,
        &[
            "version",
            "retired_at",
            "activity_at",
            "last_sequence",
            "output_bytes",
            "events_sha256",
            "cause",
            "original_result_sha256",
            "retained_result_sha256",
        ],
        |name, v| match name {
            "version" => version(v),
            "retired_at" | "activity_at" | "last_sequence" | "output_bytes" => number(v),
            "events_sha256" | "original_result_sha256" | "retained_result_sha256" => {
                v.as_str().is_some_and(is_digest)
            }
            "cause" => v.as_str().is_some_and(|s| {
                matches!(
                    s,
                    "age_limit"
                        | "service_policy_expired"
                        | "service_revoked"
                        | "service_quiescent"
                )
            }),
            _ => false,
        },
    ))
}
fn quiescence(raw: &Value) -> Value {
    if raw.is_null() || raw == &json!({"redacted":true}) {
        return raw.clone();
    }
    if !raw.is_object() {
        return json!({"redacted":true});
    }
    let out = fields(
        raw,
        &[
            "version",
            "original_state",
            "original_updated_at",
            "original_result_sha256",
        ],
        |name, v| match name {
            "version" => version(v),
            "original_state" => v.as_str().is_some_and(|s| {
                matches!(
                    s,
                    "queued"
                        | "running"
                        | "cancelling"
                        | "succeeded"
                        | "failed"
                        | "cancelled"
                        | "interrupted"
                        | "unknown"
                )
            }),
            "original_updated_at" => v.is_string() && utc(v),
            "original_result_sha256" => v.as_str().is_some_and(is_digest),
            _ => false,
        },
    );
    if out.len() == 4 {
        Value::Object(out)
    } else {
        json!({"redacted":true})
    }
}

fn state(raw: &Value, aliases: &BTreeSet<String>) -> Value {
    let mut out = fields(
        raw,
        &[
            "version",
            "job_id",
            "manifest_digest",
            "state",
            "sequence",
            "output_bytes",
            "output_truncated",
            "accepted_at",
            "updated_at",
            "history_activity_at",
            "cancel_requested",
            "timed_out",
            "remote_changes_may_be_partial",
            "reason",
        ],
        |name, v| match name {
            "version" => version(v),
            "job_id" => identifier(v),
            "manifest_digest" => v.as_str().is_some_and(is_digest),
            "state" => {
                v.is_null()
                    || v.as_str().is_some_and(|s| {
                        matches!(
                            s,
                            "queued"
                                | "running"
                                | "cancelling"
                                | "succeeded"
                                | "failed"
                                | "cancelled"
                                | "interrupted"
                                | "unknown"
                                | "acknowledged"
                        )
                    })
            }
            "sequence" | "output_bytes" => number(v),
            "output_truncated"
            | "cancel_requested"
            | "timed_out"
            | "remote_changes_may_be_partial" => flag(v),
            "accepted_at" | "updated_at" | "history_activity_at" => utc(v),
            "reason" => reason(v),
            _ => false,
        },
    );
    if let Some(value) = raw.get("environment") {
        out.insert("environment".into(), environment(value));
    }
    if let Some(value) = raw.get("result") {
        out.insert("result".into(), result(value, aliases));
    }
    if let Some(value) = raw.get("local_quiescence") {
        out.insert("local_quiescence".into(), quiescence(value));
    }
    if let Some(value) = raw.get("artifact_retirement") {
        out.insert("artifact_retirement".into(), retirement(value));
    }
    Value::Object(out)
}

pub(super) fn project(raw: &Value, aliases: &BTreeSet<String>) -> Value {
    if raw.is_null() || raw == &json!({"redacted":true}) {
        return raw.clone();
    }
    if raw.get("job_id").is_some() {
        return state(raw, aliases);
    }
    if raw["reason"] == "native_operation_not_acknowledged" {
        // Exactly these two keys are also the existing COR01 wrapper grammar.
        return json!({"reason":"native_operation_not_acknowledged","runner_result":state(&raw["runner_result"],aliases)});
    }
    if raw.is_object() {
        return Value::Object(fields(
            raw,
            &[
                "reason",
                "control_failure",
                "remote_changes_may_be_partial",
                "user_acknowledged_partial_changes",
            ],
            |name, v| match name {
                "reason" => reason(v),
                "remote_changes_may_be_partial" | "user_acknowledged_partial_changes" => flag(v),
                "control_failure" => {
                    v.is_null()
                        || v.as_str().is_some_and(|s| {
                            matches!(
                                s,
                                "owner_closed"
                                    | "observer_deadline"
                                    | "unexpected_runner_phase"
                                    | "runner_binding_mismatch"
                                    | "rpc_timeout"
                                    | "rpc_closed"
                                    | "rpc_queue_full"
                                    | "rpc_protocol"
                                    | "rpc_remote"
                                    | "rpc_io"
                                    | "rpc_invalid"
                            )
                        })
                }
                _ => false,
            },
        ));
    }
    json!({"redacted":true})
}

#[cfg(test)]
mod tests;
