//! In-memory Helm v3 release decoding with decompression and projection caps.
//! Helm release payloads are Secrets: values/manifests are masked before a
//! serializable result is returned.

use crate::resources::protected_json::{wipe_json, ProtectedJson};
use crate::resources::{redact_object, ReadOptions, ResourceError, ResourceKind};
use base64::{engine::general_purpose::STANDARD, Engine};
use flate2::read::GzDecoder;
use serde::Serialize;
use serde_json::Value;
use std::io::Read;
use zeroize::{Zeroize, Zeroizing};

const MAX_ENCODED: usize = 8 * 1024 * 1024;
const MAX_RELEASE: usize = 16 * 1024 * 1024;

#[derive(Clone, Serialize)]
pub struct HelmRelease {
    pub name: String,
    pub namespace: String,
    pub revision: u64,
    pub status: String,
    pub chart: String,
    pub chart_version: String,
    pub app_version: String,
    pub updated_at: Option<String>,
    pub values: Value,
    pub chart_values: Value,
    pub computed_values: Value,
    pub computed_values_partial: bool,
    pub manifest: String,
}
impl std::fmt::Debug for HelmRelease {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("HelmRelease([redacted])")
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct HelmSummary {
    pub name: String,
    pub namespace: String,
    pub revision: u64,
    pub status: String,
    pub chart: String,
    pub chart_version: String,
    pub app_version: String,
    pub updated_at: Option<String>,
    pub secret_name: String,
}
impl From<&HelmRelease> for HelmSummary {
    fn from(release: &HelmRelease) -> Self {
        Self {
            name: release.name.clone(),
            namespace: release.namespace.clone(),
            revision: release.revision,
            status: release.status.clone(),
            chart: release.chart.clone(),
            chart_version: release.chart_version.clone(),
            app_version: release.app_version.clone(),
            updated_at: release.updated_at.clone(),
            secret_name: format!("sh.helm.release.v1.{}.v{}", release.name, release.revision),
        }
    }
}

pub fn decode_release_secret(secret: &Value) -> Result<HelmRelease, ResourceError> {
    if secret["type"] != "helm.sh/release.v1" || secret["kind"] != "Secret" {
        return Err(ResourceError::InvalidResponse);
    }
    let encoded = secret["data"]["release"]
        .as_str()
        .ok_or(ResourceError::InvalidResponse)?;
    if encoded.len() > MAX_ENCODED {
        return Err(ResourceError::TooLarge);
    }
    // Kubernetes JSON encodes Secret data once; Helm's storage driver adds a
    // second base64 layer around its gzip-compressed JSON release.
    let kubernetes = Zeroizing::new(
        STANDARD
            .decode(encoded)
            .map_err(|_| ResourceError::InvalidResponse)?,
    );
    let helm = Zeroizing::new(
        STANDARD
            .decode(&*kubernetes)
            .map_err(|_| ResourceError::InvalidResponse)?,
    );
    let mut decoded = Zeroizing::new(Vec::new());
    if helm.starts_with(&[0x1f, 0x8b]) {
        GzDecoder::new(&helm[..])
            .take((MAX_RELEASE + 1) as u64)
            .read_to_end(&mut decoded)
            .map_err(|_| ResourceError::InvalidResponse)?;
    } else {
        decoded.extend_from_slice(&helm);
    }
    if decoded.len() > MAX_RELEASE {
        return Err(ResourceError::TooLarge);
    }
    let mut release = ProtectedJson(
        serde_json::from_slice(&decoded).map_err(|_| ResourceError::InvalidResponse)?,
    );
    let text = |key: &str| -> Result<String, ResourceError> {
        release[key]
            .as_str()
            .filter(|s| s.len() <= 253 && !s.chars().any(char::is_control))
            .map(str::to_owned)
            .ok_or(ResourceError::InvalidResponse)
    };
    let name = text("name")?;
    let namespace = text("namespace")?;
    let revision = release["version"]
        .as_u64()
        .ok_or(ResourceError::InvalidResponse)?;
    let status = release["info"]["status"]
        .as_str()
        .unwrap_or("unknown")
        .chars()
        .take(64)
        .collect();
    let chart = release["chart"]["metadata"]["name"]
        .as_str()
        .unwrap_or("")
        .chars()
        .take(253)
        .collect();
    let chart_version = release["chart"]["metadata"]["version"]
        .as_str()
        .unwrap_or("")
        .chars()
        .take(128)
        .collect();
    let app_version = release["chart"]["metadata"]["appVersion"]
        .as_str()
        .unwrap_or("")
        .chars()
        .take(128)
        .collect();
    let updated_at = release["info"]["last_deployed"]
        .as_str()
        .map(|v| v.chars().take(64).collect());
    let manifest = mask_manifest(release["manifest"].as_str().unwrap_or(""))?;
    // Helm's persisted Chart does not serialize its private dependency graph.
    // Root defaults plus overrides are exact only without dependent charts.
    let computed_values_partial = release["chart"]["metadata"]["dependencies"]
        .as_array()
        .is_some_and(|dependencies| !dependencies.is_empty())
        || release["chart"]["dependencies"]
            .as_array()
            .is_some_and(|dependencies| !dependencies.is_empty());
    let mut values = release["config"].take();
    let mut chart_values = release["chart"]["values"].take();
    let mut computed_values = chart_values.clone();
    coalesce_values(&mut computed_values, &values);
    mask_values(&mut values);
    mask_values(&mut chart_values);
    mask_values(&mut computed_values);
    Ok(HelmRelease {
        name,
        namespace,
        revision,
        status,
        chart,
        chart_version,
        app_version,
        updated_at,
        values,
        chart_values,
        computed_values,
        computed_values_partial,
        manifest,
    })
}

fn coalesce_values(defaults: &mut Value, overrides: &Value) {
    match (defaults, overrides) {
        (Value::Object(defaults), Value::Object(overrides)) => {
            for (key, value) in overrides {
                if value.is_null() {
                    if let Some(mut removed) = defaults.remove(key) {
                        wipe_json(&mut removed);
                    }
                } else if let Some(default) = defaults.get_mut(key) {
                    coalesce_values(default, value);
                } else {
                    defaults.insert(key.clone(), value.clone());
                }
            }
        }
        (_, Value::Null) => {}
        (default, value) => {
            wipe_json(default);
            *default = value.clone();
        }
    }
}

pub fn mask_values(value: &mut Value) {
    // Helm values can contain arbitrary credentials under arbitrary keys.
    // Keep the structure and booleans/numbers, but mask all string leaves;
    // revealing a value is a separate explicit action owned by the UI.
    match value {
        Value::Object(values) => {
            for (key, value) in values {
                let key = key.to_ascii_lowercase().replace(['-', '_'], "");
                if matches!(
                    key.as_str(),
                    "password"
                        | "passwd"
                        | "token"
                        | "accesstoken"
                        | "refreshtoken"
                        | "clientsecret"
                        | "secret"
                        | "apikey"
                        | "secretkey"
                        | "privatekey"
                        | "pin"
                ) {
                    wipe_json(value);
                    *value = Value::String("[redacted]".into());
                } else {
                    mask_values(value);
                }
            }
        }
        Value::Array(values) => {
            for value in values {
                mask_values(value);
            }
        }
        Value::String(value) => {
            value.zeroize();
            *value = "[redacted]".into();
        }
        _ => {}
    }
}

fn mask_manifest(manifest: &str) -> Result<String, ResourceError> {
    if manifest.len() > MAX_RELEASE {
        return Err(ResourceError::TooLarge);
    }
    let documents: Vec<Value> =
        serde_saphyr::from_multiple(manifest).map_err(|_| ResourceError::InvalidResponse)?;
    let mut documents = ProtectedJson(Value::Array(documents));
    let documents = documents
        .as_array_mut()
        .ok_or(ResourceError::InvalidResponse)?;
    if documents.len() > 4096 {
        return Err(ResourceError::TooLarge);
    }
    let mut output = Vec::new();
    for object in documents {
        if object.is_null() {
            continue;
        }
        let kind = object["kind"].as_str().unwrap_or("").to_owned();
        let api = object["apiVersion"].as_str().unwrap_or("");
        let (group, version) = api.split_once('/').unwrap_or(("", api));
        let resource = match kind.as_str() {
            "Secret" => "secrets",
            "ConfigMap" => "configmaps",
            _ => "",
        };
        let descriptor = ResourceKind {
            group: group.into(),
            version: version.into(),
            resource: resource.into(),
            kind: kind.clone(),
            namespaced: true,
            verbs: Vec::new(),
        };
        redact_object(object, &descriptor, ReadOptions::AI);
        // Environment literals and command arguments in workload specs can
        // contain credentials just as a Secret can.
        mask_workload_literals(object);
        output.push(serde_saphyr::to_string(&object).map_err(|_| ResourceError::InvalidResponse)?);
    }
    Ok(output.join("---\n"))
}

pub(crate) fn mask_workload_literals(value: &mut Value) {
    match value {
        Value::Object(values) => {
            for (key, value) in values {
                if matches!(key.as_str(), "env" | "envFrom" | "command" | "args") {
                    mask_values(value);
                } else {
                    mask_workload_literals(value);
                }
            }
        }
        Value::Array(values) => {
            for value in values {
                mask_workload_literals(value);
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use flate2::{write::GzEncoder, Compression};
    use serde_json::json;
    use std::io::Write;
    fn secret(release: Value) -> Value {
        let mut gzip = GzEncoder::new(Vec::new(), Compression::default());
        gzip.write_all(release.to_string().as_bytes()).unwrap();
        json!({"kind":"Secret","type":"helm.sh/release.v1","data":{"release":STANDARD.encode(STANDARD.encode(gzip.finish().unwrap()))}})
    }
    #[test]
    fn real_helm_storage_layers_are_decoded_and_secrets_never_return() {
        let release=decode_release_secret(&secret(json!({
            "name":"shop","namespace":"apps","version":3,"info":{"status":"deployed"},
            "chart":{"metadata":{"name":"shop","version":"1.2.3"},"values":{"unusual":"fixture-chart-token"}},
            "config":{"password":"fixture-password"},
            "manifest":"apiVersion: v1\nkind: Secret\nmetadata:\n  name: app\ndata:\n  token: Zml4dHVyZS10b2tlbg==\n---\napiVersion: apps/v1\nkind: Deployment\nmetadata:\n  name: app\nspec:\n  template:\n    spec:\n      containers:\n      - name: web\n        env:\n        - name: PASSWORD\n          value: fixture-env-password\n"
        }))).unwrap();
        assert_eq!(release.revision, 3);
        let serialized = serde_json::to_string(&release).unwrap();
        for forbidden in [
            "fixture-password",
            "fixture-chart-token",
            "Zml4dHVyZS10b2tlbg==",
            "fixture-env-password",
        ] {
            assert!(!serialized.contains(forbidden));
        }
        assert!(release.manifest.contains("Deployment"));
    }
    #[test]
    fn invalid_or_excessive_release_encoding_is_rejected() {
        assert!(decode_release_secret(
            &json!({"kind":"Secret","type":"helm.sh/release.v1","data":{"release":"!"}})
        )
        .is_err());
        assert_eq!(decode_release_secret(&json!({"kind":"Secret","type":"helm.sh/release.v1","data":{"release":"A".repeat(MAX_ENCODED+1)}})).err(),Some(ResourceError::TooLarge));
    }

    #[test]
    fn computed_values_mark_unavailable_subchart_defaults_as_partial() {
        let release = decode_release_secret(&secret(json!({
            "name":"parent","namespace":"apps","version":1,"info":{"status":"deployed"},
            "chart":{"metadata":{"name":"parent","version":"1.0.0","dependencies":[{"name":"database","version":"1.0.0"}]},
                "values":{"replicas":1,"database":{"port":5432}}},
            "config":{"replicas":3,"database":{"password":"fixture-private"}}
        }))).unwrap();
        assert!(release.computed_values_partial);
        assert_eq!(release.computed_values["replicas"], 3);
        assert_eq!(release.computed_values["database"]["port"], 5432);
        assert_eq!(
            release.computed_values["database"]["password"],
            "[redacted]"
        );
    }
}
