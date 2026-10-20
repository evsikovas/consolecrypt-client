//! Workload log discovery and explicit bounded export. No log data is stored
//! in the vault, sync queue, diagnostics, or preference tables.
use crate::{AppCore, AppError, AppResult};
use cc_k8s_core::resources::{ListQuery, ReadOptions, ResourceRef};
use serde::Serialize;
use serde_json::Value;
use std::{
    io::Write,
    path::{Path, PathBuf},
};
use zeroize::Zeroizing;

#[derive(Debug, Serialize)]
pub struct KubeLogTarget {
    pub pod: String,
    pub containers: Vec<String>,
}
#[derive(Debug, Serialize)]
pub struct KubeLogTargets {
    pub targets: Vec<KubeLogTarget>,
    pub truncated: bool,
}
fn invalid(rule: &'static str) -> AppError {
    AppError::invalid("kubernetes", rule)
}
fn part(value: &str, key: bool) -> bool {
    !value.is_empty()
        && value.len() <= 253
        && value
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || b"-_.".contains(&c) || (key && c == b'/'))
}
fn selector(value: &Value) -> AppResult<String> {
    let object = value
        .as_object()
        .ok_or_else(|| invalid("workload_selector_missing"))?;
    let mut parts = Vec::new();
    if let Some(labels) = object.get("matchLabels") {
        for (key, value) in labels
            .as_object()
            .ok_or_else(|| invalid("workload_selector_invalid"))?
        {
            let value = value
                .as_str()
                .ok_or_else(|| invalid("workload_selector_invalid"))?;
            if !part(key, true) || (!value.is_empty() && !part(value, false)) {
                return Err(invalid("workload_selector_invalid"));
            }
            parts.push(format!("{key}={value}"));
        }
    }
    if let Some(expressions) = object.get("matchExpressions") {
        for expression in expressions
            .as_array()
            .ok_or_else(|| invalid("workload_selector_invalid"))?
        {
            let key = expression["key"]
                .as_str()
                .filter(|key| part(key, true))
                .ok_or_else(|| invalid("workload_selector_invalid"))?;
            let values = expression
                .get("values")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default();
            match expression["operator"].as_str() {
                Some("Exists") if values.is_empty() => parts.push(key.into()),
                Some("DoesNotExist") if values.is_empty() => parts.push(format!("!{key}")),
                Some(operator @ ("In" | "NotIn")) if !values.is_empty() => {
                    let values = values
                        .iter()
                        .map(|value| {
                            value
                                .as_str()
                                .filter(|value| part(value, false))
                                .ok_or_else(|| invalid("workload_selector_invalid"))
                        })
                        .collect::<AppResult<Vec<_>>>()?;
                    parts.push(format!(
                        "{key} {} ({})",
                        if operator == "In" { "in" } else { "notin" },
                        values.join(",")
                    ));
                }
                _ => return Err(invalid("workload_selector_invalid")),
            }
        }
    }
    let result = parts.join(",");
    if result.is_empty() || result.len() > 4096 {
        return Err(invalid("workload_selector_missing"));
    }
    Ok(result)
}
fn target(pod: &str, object: &Value) -> KubeLogTarget {
    let mut containers = Vec::new();
    for kind in ["containers", "initContainers", "ephemeralContainers"] {
        if let Some(items) = object["spec"][kind].as_array() {
            for item in items.iter().take(64) {
                if let Some(name) = item["name"].as_str().filter(|name| part(name, false)) {
                    if !containers.iter().any(|v| v == name) {
                        containers.push(name.to_owned());
                    }
                }
            }
        }
    }
    KubeLogTarget {
        pod: pod.into(),
        containers,
    }
}
impl AppCore {
    pub async fn kube_log_targets(
        &self,
        session_id: String,
        reference: ResourceRef,
    ) -> AppResult<KubeLogTargets> {
        let (owner, unlocked) = self.unlocked().await?;
        let live = unlocked.kube.get(&session_id)?;
        let detail = live
            .resources
            .get(&reference, ReadOptions::default())
            .await
            .map_err(super::kubernetes_runtime::resource_error)?;
        let result = if reference.api.group.is_empty() && reference.api.resource == "pods" {
            KubeLogTargets {
                targets: vec![target(&reference.name, &detail.object)],
                truncated: false,
            }
        } else {
            if !matches!(
                (
                    reference.api.group.as_str(),
                    reference.api.resource.as_str()
                ),
                (
                    "apps",
                    "deployments" | "replicasets" | "statefulsets" | "daemonsets"
                ) | ("batch", "jobs")
            ) {
                return Err(invalid("workload_logs_unsupported"));
            }
            let labels = selector(&detail.object["spec"]["selector"])?;
            let pods = live
                .resources
                .resolve("", "v1", "pods")
                .map_err(super::kubernetes_runtime::resource_error)?;
            let page = live
                .resources
                .list(
                    &pods,
                    &ListQuery {
                        namespace: reference.namespace,
                        label_selector: Some(labels),
                        limit: Some(9),
                        ..ListQuery::default()
                    },
                )
                .await
                .map_err(super::kubernetes_runtime::resource_error)?;
            KubeLogTargets {
                truncated: page.items.len() > 8 || page.continue_token.is_some(),
                targets: page
                    .items
                    .iter()
                    .take(8)
                    .map(|row| target(&row.name, &row.object))
                    .collect(),
            }
        };
        let _profile = self.hold_session(&owner).await?;
        let _generation = owner.hold_unlocked(&unlocked).await?;
        if live.cancel.is_cancelled() {
            return Err(AppError::VaultLocked);
        }
        Ok(result)
    }
    pub async fn kube_export_logs(
        &self,
        session_id: String,
        path: String,
        contents: Zeroizing<Vec<u8>>,
        mask: bool,
    ) -> AppResult<()> {
        let (owner, unlocked) = self.unlocked().await?;
        let live = unlocked.kube.get(&session_id)?;
        if contents.len() > 8 * 1024 * 1024 {
            return Err(invalid("logs_too_large"));
        }
        let text = std::str::from_utf8(&contents).map_err(|_| invalid("logs_invalid"))?;
        let masked = if mask {
            Some(Zeroizing::new(
                cc_ai_core::sanitizer::sanitize(cc_models::ai::PrivacyProfile::Standard, text)
                    .into_string(),
            ))
        } else {
            None
        };
        let bytes = masked
            .as_ref()
            .map_or(contents.as_slice(), |value| value.as_bytes());
        let destination = export_destination(Path::new(&path))?;
        let directory = tempfile::Builder::new()
            .prefix(".consolecrypt-logs-")
            .tempdir_in(
                destination
                    .parent()
                    .ok_or_else(|| invalid("invalid_path"))?,
            )
            .map_err(|_| invalid("export_failed"))?;
        crate::kubernetes_proxy::restrict_directory(directory.path()).await?;
        let mut file = tempfile::NamedTempFile::new_in(directory.path())
            .map_err(|_| invalid("export_failed"))?;
        file.write_all(bytes)
            .and_then(|_| file.as_file().sync_all())
            .map_err(|_| invalid("export_failed"))?;
        let _profile = self.hold_session(&owner).await?;
        let _generation = owner.hold_unlocked(&unlocked).await?;
        if live.cancel.is_cancelled() {
            return Err(AppError::VaultLocked);
        }
        file.persist_noclobber(destination)
            .map_err(|_| invalid("destination_exists_or_unwritable"))?;
        Ok(())
    }
}
fn export_destination(path: &Path) -> AppResult<PathBuf> {
    if !path.is_absolute() || path.file_name().is_none() {
        return Err(invalid("invalid_path"));
    }
    let parent = path
        .parent()
        .ok_or_else(|| invalid("invalid_path"))?
        .canonicalize()
        .map_err(|_| invalid("invalid_path"))?;
    let path = parent.join(path.file_name().ok_or_else(|| invalid("invalid_path"))?);
    if path.symlink_metadata().is_ok() {
        return Err(invalid("destination_exists_or_unwritable"));
    }
    Ok(path)
}
#[cfg(test)]
#[path = "kubernetes_logs_tests.rs"]
mod integration_tests;
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn workload_selectors_preserve_expressions_and_never_expand_to_namespace() {
        let value = serde_json::json!({"matchLabels":{"app":"api"},"matchExpressions":[{"key":"tier","operator":"In","values":["back","worker"]},{"key":"debug","operator":"DoesNotExist"}]});
        assert_eq!(
            selector(&value).unwrap(),
            "app=api,tier in (back,worker),!debug"
        );
        for value in [
            serde_json::json!({}),
            serde_json::json!({"matchLabels":{"app":"x,y=z"}}),
            serde_json::json!({"matchExpressions":[{"key":"app","operator":"In","values":[]} ]}),
        ] {
            assert!(selector(&value).is_err());
        }
    }
    #[test]
    fn export_never_overwrites_existing_files_or_symlinks() {
        let temp = tempfile::tempdir().unwrap();
        let existing = temp.path().join("existing.log");
        std::fs::write(&existing, "keep").unwrap();
        assert!(export_destination(&existing).is_err());
        assert!(export_destination(Path::new("relative.log")).is_err());
        #[cfg(unix)]
        {
            let dangling = temp.path().join("dangling.log");
            std::os::unix::fs::symlink(temp.path().join("missing"), &dangling).unwrap();
            assert!(export_destination(&dangling).is_err());
        }
        assert_eq!(std::fs::read_to_string(existing).unwrap(), "keep");
    }
}
