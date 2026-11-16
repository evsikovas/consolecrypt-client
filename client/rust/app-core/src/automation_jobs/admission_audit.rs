//! Read-only admission DATA projection, never a grant or server-log guarantee.
//! The original Native catalog is the comparison source. Remote metadata is
//! removed from result evidence before persistence; no caller identity fallback.
use base64::{Engine, engine::general_purpose::STANDARD};
use cc_models::automation::{JOB_SIGNATURE_DOMAIN, JobManifest};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use uuid::Uuid;

const LIMIT: usize = 64 * 1024;
const TARGETS: usize = 512;

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Original {
    version: u8,
    job_id: Uuid,
    profile_id: Uuid,
    device_id: Uuid,
    project_id: Uuid,
    project_revision: u64,
    runner_id: Uuid,
    revision_object_id: Uuid,
    revision_digest: String,
}

enum Expected {
    Legacy,
    Invalid,
    Original { remote: Value, public: Value },
}
fn missing(reason: &str) -> Value {
    json!({"version":1,"status":"unrecorded","reason":reason})
}
fn unavailable() -> Value {
    json!({"version":1,"status":"unavailable","reason":"binding_mismatch"})
}
fn canonical(value: Value) -> Value {
    match value {
        Value::Object(map) => Value::Object(
            map.into_iter()
                .collect::<BTreeMap<_, _>>()
                .into_iter()
                .map(|(k, v)| (k, canonical(v)))
                .collect(),
        ),
        Value::Array(values) => Value::Array(values.into_iter().map(canonical).collect()),
        other => other,
    }
}
fn hash(raw: &[u8]) -> String {
    Sha256::digest(raw)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}
fn expected(row: &Value) -> Expected {
    let Some(raw) = row.get("correlation_provenance") else {
        return Expected::Legacy;
    };
    let build = || -> Option<(Value, Value)> {
        let original: Original = serde_json::from_value(raw.clone()).ok()?;
        if original.version != 1
            || serde_json::to_value(&original).ok()?.ne(raw)
            || [
                original.job_id,
                original.profile_id,
                original.device_id,
                original.project_id,
                original.runner_id,
                original.revision_object_id,
            ]
            .iter()
            .any(Uuid::is_nil)
            || !(1..=9_007_199_254_740_991).contains(&original.project_revision)
        {
            return None;
        }
        let manifest: JobManifest = serde_json::from_value(row.get("manifest")?.clone()).ok()?;
        let raw_manifest = manifest.canonical_bytes().ok()?;
        let mut signed = JOB_SIGNATURE_DOMAIN.to_vec();
        signed.extend(raw_manifest);
        let digest = hash(&signed);
        if row["version"] != 1
            || row["job_id"] != json!(original.job_id)
            || row["runner_id"] != json!(original.runner_id)
            || row["project_id"] != json!(original.project_id)
            || row["manifest_digest"] != digest
            || manifest.job_id != original.job_id
            || manifest.tenant_id != original.profile_id
            || manifest.runner_id != original.runner_id
            || manifest.project_id != original.project_id
            || manifest.revision_digest != original.revision_digest
            || !(1..=253_402_300_799).contains(&manifest.issued_at)
            || manifest.targets.is_empty()
            || manifest.targets.len() > TARGETS
            || row["host_ids"]
                != json!(
                    manifest
                        .targets
                        .iter()
                        .map(|t| t.host_id)
                        .collect::<Vec<_>>()
                )
        {
            return None;
        }
        let owner_digest = if manifest.policy.production {
            let proof = row.get("owner_confirmation")?;
            let _: cc_crypto_core::OwnerConfirmation =
                serde_json::from_value(proof.clone()).ok()?;
            if proof["version"] != 1
                || proof["policy"] != "ownerConfirmationVaultPassphraseV1"
                || proof["signer_id"] != json!(original.device_id)
                || proof["manifest_digest"] != digest
                || proof["signature"].as_str().is_none_or(|s| {
                    s.len() != 88 || !STANDARD.decode(s).is_ok_and(|raw| raw.len() == 64)
                })
            {
                return None;
            }
            Some(hash(&serde_json::to_vec(&canonical(proof.clone())).ok()?))
        } else {
            if row.get("owner_confirmation").is_some_and(|v| !v.is_null()) {
                return None;
            }
            None
        };
        let targets = manifest
            .targets
            .iter()
            .map(|t| json!({"host_id":t.host_id,"route_digest":t.route_digest}))
            .collect::<Vec<_>>();
        let remote = json!({"version":1,"job_id":manifest.job_id,"manifest_digest":digest,
            "signer_id":original.device_id,"scope":"jobs.execute","tenant_id":manifest.tenant_id,
            "runner_id":manifest.runner_id,"project_id":manifest.project_id,
            "revision_digest":manifest.revision_digest,"issued_at":manifest.issued_at,
            "production":manifest.policy.production,"owner_evidence_digest":owner_digest,"targets":targets});
        let public = json!({"version":1,"status":"recorded","jobId":manifest.job_id,"manifestDigest":digest,
            "signerId":original.device_id,"scope":"jobs.execute","projectId":manifest.project_id,
            "runnerId":manifest.runner_id,"revisionDigest":manifest.revision_digest,
            "issuedAt":manifest.issued_at,"production":manifest.policy.production,"ownerEvidenceDigest":owner_digest,
            "targets":manifest.targets.iter().map(|t|json!({"hostId":t.host_id,"routeDigest":t.route_digest})).collect::<Vec<_>>()});
        if serde_json::to_vec(&remote).ok()?.len() > LIMIT
            || serde_json::to_vec(&public).ok()?.len() > LIMIT
        {
            return None;
        }
        Some((remote, public))
    };
    match build() {
        Some((remote, public)) => Expected::Original { remote, public },
        None => Expected::Invalid,
    }
}

// Only the two existing genuine result shapes; never recurse through arbitrary
// payloads or infer an audit from result text. Missing old-helper field is legacy.
fn take_remote(row: &mut Value) -> Option<(Value, Value, Option<Value>)> {
    let evidence = row.get_mut("runner_result")?;
    let state = if evidence.get("job_id").is_some() {
        evidence
    } else {
        evidence.get_mut("runner_result")?
    };
    let job = state.get("job_id")?.clone();
    let digest = state.get("manifest_digest")?.clone();
    let audit = state.as_object_mut()?.remove("admission_audit");
    Some((job, digest, audit))
}

pub(super) fn project_row(row: &mut Value) {
    let observed = take_remote(row);
    let previous = row.get("admission_audit").cloned();
    let value = match expected(row) {
        Expected::Legacy => missing("legacy_native_provenance"),
        Expected::Invalid => unavailable(),
        Expected::Original { remote, public } => {
            // An already checked original admission fact does not expire or
            // become a new permission when a later status omits old metadata.
            let prior_recorded = previous.as_ref() == Some(&public);
            match observed {
                Some((job, digest, Some(wire))) => {
                    let matching = job == row["job_id"] && digest == row["manifest_digest"];
                    if matching && wire == json!({"version":1,"status":"recorded","record":remote})
                    {
                        public
                    } else if prior_recorded {
                        public
                    } else if matching && wire == json!({"version":1,"status":"unrecorded"}) {
                        missing("runner_unrecorded")
                    } else {
                        unavailable()
                    }
                }
                Some((job, digest, None))
                    if job == row["job_id"] && digest == row["manifest_digest"] =>
                {
                    if prior_recorded {
                        public
                    } else {
                        missing("runner_unrecorded")
                    }
                }
                Some(_) => {
                    if prior_recorded {
                        public
                    } else {
                        unavailable()
                    }
                }
                None => {
                    if prior_recorded {
                        public
                    } else if previous == Some(unavailable()) {
                        unavailable()
                    } else if previous == Some(missing("runner_unrecorded")) {
                        missing("runner_unrecorded")
                    } else {
                        missing("not_observed")
                    }
                }
            }
        }
    };
    row["admission_audit"] = value;
}

#[cfg(any(test, feature = "test-harness"))]
pub(crate) mod tests;
