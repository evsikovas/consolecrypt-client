use super::*;

const MAX_OBJECTS: usize = 128;
const MAX_RESULT: usize = 1024 * 1024;

#[derive(Clone, Serialize)]
pub struct HelmObjectComparison {
    pub reference: ResourceRef,
    /// `equal` compares visible authored fields only, not admission defaults.
    pub state: String,
    pub diff: ManifestDiff,
}
#[derive(Clone, Serialize)]
pub struct HelmLiveComparison {
    pub revision: u64,
    pub items: Vec<HelmObjectComparison>,
    pub partial: bool,
    pub masked_fields: bool,
}
redacted_debug!(HelmObjectComparison, HelmLiveComparison);

impl ResourceApi {
    /// Read-only comparison with the saved release manifest. This does not
    /// perform Flux's server-side dry-run or reproduce admission/defaulting.
    pub async fn helm_compare_live(
        &self,
        namespace: &str,
        secret_name: &str,
    ) -> Result<HelmLiveComparison, ResourceError> {
        tokio::time::timeout(
            Duration::from_secs(60),
            self.helm_compare_inner(namespace, secret_name),
        )
        .await
        .map_err(|_| ResourceError::Timeout)?
    }

    async fn helm_compare_inner(
        &self,
        namespace: &str,
        secret_name: &str,
    ) -> Result<HelmLiveComparison, ResourceError> {
        let release = self.helm_release(namespace, secret_name).await?;
        let documents: Vec<Value> = serde_saphyr::from_multiple(&release.manifest)
            .map_err(|_| ResourceError::InvalidResponse)?;
        let mut documents = ProtectedJson(Value::Array(documents));
        let documents = documents
            .as_array_mut()
            .ok_or(ResourceError::InvalidResponse)?;
        let mut partial = documents.len() > MAX_OBJECTS;
        let masked_fields = release.manifest.contains("[redacted]");
        let kinds = self.kinds()?;
        let mut candidates = Vec::new();
        let mut seen = BTreeSet::new();
        for object in documents.iter_mut().take(MAX_OBJECTS) {
            if object.is_null() {
                continue;
            }
            let api = object["apiVersion"].as_str().unwrap_or("");
            let (group, version) = api.split_once('/').unwrap_or(("", api));
            let kind = object["kind"].as_str().unwrap_or("");
            let Some(descriptor) = kinds.iter().find(|candidate| {
                candidate.group == group && candidate.version == version && candidate.kind == kind
            }) else {
                partial = true;
                continue;
            };
            let name = object["metadata"]["name"].as_str().unwrap_or("");
            let reference = ResourceRef {
                api: descriptor.clone(),
                name: name.into(),
                namespace: descriptor.namespaced.then(|| {
                    object["metadata"]["namespace"]
                        .as_str()
                        .unwrap_or(&release.namespace)
                        .to_owned()
                }),
            };
            // Validate identities and capabilities before any GET. A release
            // cannot use embedded URLs or bypass served discovery.
            if self.object_path(&reference, "get").is_err()
                || !seen.insert((
                    descriptor.key(),
                    reference.namespace.clone(),
                    reference.name.clone(),
                ))
            {
                partial = true;
                continue;
            }
            if descriptor.namespaced && object["metadata"]["namespace"].is_null() {
                object["metadata"]["namespace"] = json!(reference.namespace);
            }
            candidates.push((reference, object.take()));
        }
        let mut requests = stream::iter(candidates)
            .map(|(reference, expected)| async move {
                let current = self.get_json(&self.object_path(&reference, "get")?).await;
                let (state, diff) = match current {
                    Ok(current) => {
                        let mut current = ProtectedJson(current);
                        complete_type_metadata(&mut current, &reference.api)?;
                        redact_object(&mut current, &reference.api, ReadOptions::AI);
                        crate::helm::mask_workload_literals(&mut current);
                        let actual = authored_projection(&expected, &current);
                        let diff = manifest_diff(expected, actual);
                        (
                            if diff.changes.is_empty() {
                                "equal"
                            } else {
                                "changed"
                            },
                            diff,
                        )
                    }
                    Err(ResourceError::NotFound) => ("missing", empty_diff()),
                    Err(ResourceError::Closed) => return Err(ResourceError::Closed),
                    Err(_) => ("unavailable", empty_diff()),
                };
                Ok(HelmObjectComparison {
                    reference,
                    state: state.into(),
                    diff,
                })
            })
            .buffered(4);
        let mut items = Vec::new();
        let mut bytes = 0usize;
        while let Some(item) = requests.next().await {
            let item = item?;
            partial |= item.state == "unavailable" || item.diff.truncated;
            bytes = bytes.saturating_add(
                serde_json::to_vec(&item)
                    .map_err(|_| ResourceError::InvalidResponse)?
                    .len(),
            );
            if bytes > MAX_RESULT {
                partial = true;
                break;
            }
            items.push(item);
        }
        Ok(HelmLiveComparison {
            revision: release.revision,
            items,
            partial,
            masked_fields,
        })
    }
}

fn empty_diff() -> ManifestDiff {
    ManifestDiff {
        changes: Vec::new(),
        truncated: false,
    }
}

/// Inspect authored paths only: server-added metadata/default fields are not
/// actionable drift. Array lengths and values remain significant.
fn authored_projection(expected: &Value, current: &Value) -> Value {
    match (expected, current) {
        (Value::String(value), _) if value == "[redacted]" => expected.clone(),
        (Value::Object(expected), Value::Object(current)) => Value::Object(
            expected
                .iter()
                .filter_map(|(key, value)| {
                    current
                        .get(key)
                        .map(|current| (key.clone(), authored_projection(value, current)))
                })
                .collect(),
        ),
        (Value::Array(expected), Value::Array(current)) => Value::Array(
            current
                .iter()
                .enumerate()
                .map(|(index, current)| {
                    expected.get(index).map_or_else(
                        || current.clone(),
                        |expected| authored_projection(expected, current),
                    )
                })
                .collect(),
        ),
        _ => current.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn projection_ignores_defaults_but_preserves_missing_changed_and_added_array_items() {
        let expected = json!({"metadata":{"name":"web"},"spec":{"replicas":2,"template":{
            "containers":[{"name":"web","image":"image:v1","env":"[redacted]"}]}}});
        let actual = json!({"metadata":{"name":"web","uid":"server"},"spec":{"replicas":3,
            "defaulted":true,"template":{"containers":[{"name":"web","image":"image:v2","env":"secret",
                "serverDefault":true},{"name":"sidecar"}]}},"status":{"availableReplicas":3}});
        let diff = manifest_diff(expected.clone(), authored_projection(&expected, &actual));
        let text = serde_json::to_string(&diff).unwrap();
        assert!(!text.contains("serverDefault") && !text.contains("secret"));
        assert!(diff
            .changes
            .iter()
            .any(|change| change.path == "/spec/replicas"));
        assert!(diff
            .changes
            .iter()
            .any(|change| change.path.ends_with("/image")));
        assert!(diff
            .changes
            .iter()
            .any(|change| change.path.ends_with("/containers/1")));
        let diff = manifest_diff(expected.clone(), authored_projection(&expected, &json!({})));
        assert!(!diff.changes.is_empty());
    }
}
