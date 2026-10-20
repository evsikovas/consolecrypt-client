use super::*;

const MAX_CHANGES: usize = 256;

#[derive(Clone, Serialize)]
pub struct ManifestChange {
    /// RFC 6901 JSON pointer. Empty means the entire document.
    pub path: String,
    pub before: Option<Value>,
    pub after: Option<Value>,
}
#[derive(Clone, Serialize)]
pub struct ManifestDiff {
    pub changes: Vec<ManifestChange>,
    pub truncated: bool,
}
redacted_debug!(ManifestChange, ManifestDiff);

impl ResourceApi {
    /// Compares the edited object with the current revision. Secret values
    /// are masked before any change is put into a serializable result.
    pub async fn compare_manifest(
        &self,
        reference: &ResourceRef,
        manifest: Zeroizing<String>,
    ) -> Result<ManifestDiff, ResourceError> {
        if manifest.len() > MAX_MANIFEST {
            return Err(ResourceError::TooLarge);
        }
        let mut candidate = ProtectedJson(
            serde_saphyr::from_str(&manifest).map_err(|_| ResourceError::InvalidRequest)?,
        );
        validate_manifest(reference, &candidate)?;
        let current = self.get(reference, ReadOptions::default()).await?;
        if current.object["metadata"]["resourceVersion"] != candidate["metadata"]["resourceVersion"]
        {
            return Err(ResourceError::Conflict);
        }
        redact_object(&mut candidate, &reference.api, ReadOptions::default());
        Ok(manifest_diff(current.object, candidate.into_inner()))
    }
}

/// Inputs must already have the appropriate secret projection. Transport
/// callers use compare_manifest, which performs that projection itself.
pub fn manifest_diff(mut before: Value, mut after: Value) -> ManifestDiff {
    strip_server_fields(&mut before);
    strip_server_fields(&mut after);
    let mut result = ManifestDiff {
        changes: Vec::new(),
        truncated: false,
    };
    difference(Some(&before), Some(&after), String::new(), 0, &mut result);
    result
}

fn strip_server_fields(value: &mut Value) {
    if let Some(object) = value.as_object_mut() {
        object.remove("status");
    }
    if let Some(metadata) = value.get_mut("metadata").and_then(Value::as_object_mut) {
        for key in [
            "resourceVersion",
            "managedFields",
            "uid",
            "creationTimestamp",
            "generation",
        ] {
            metadata.remove(key);
        }
    }
}

fn difference(
    before: Option<&Value>,
    after: Option<&Value>,
    path: String,
    depth: usize,
    result: &mut ManifestDiff,
) {
    if before == after {
        return;
    }
    if result.changes.len() >= MAX_CHANGES {
        result.truncated = true;
        return;
    }
    if depth < 64 {
        match (before, after) {
            (Some(Value::Object(before)), Some(Value::Object(after))) => {
                let keys: BTreeSet<_> = before.keys().chain(after.keys()).collect();
                for key in keys {
                    let key_path =
                        format!("{}/{}", path, key.replace('~', "~0").replace('/', "~1"));
                    difference(before.get(key), after.get(key), key_path, depth + 1, result);
                    if result.truncated {
                        break;
                    }
                }
                return;
            }
            (Some(Value::Array(before)), Some(Value::Array(after))) => {
                for index in 0..before.len().max(after.len()) {
                    difference(
                        before.get(index),
                        after.get(index),
                        format!("{path}/{index}"),
                        depth + 1,
                        result,
                    );
                    if result.truncated {
                        break;
                    }
                }
                return;
            }
            _ => {}
        }
    }
    result.changes.push(ManifestChange {
        path,
        before: before.cloned(),
        after: after.cloned(),
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn semantic_diff_ignores_server_fields_and_bounds_changes() {
        let before = json!({"metadata":{"name":"app","resourceVersion":"1","labels":{"a/b":"before"}},"status":{"phase":"Pending"},"spec":{"replicas":1}});
        let after = json!({"metadata":{"name":"app","resourceVersion":"2","labels":{"a/b":"after"}},"status":{"phase":"Running"},"spec":{"replicas":2}});
        let diff = manifest_diff(before, after);
        assert_eq!(diff.changes.len(), 2);
        assert_eq!(diff.changes[0].path, "/metadata/labels/a~1b");
        assert!(!diff.truncated);
        let before = json!((0..1000).collect::<Vec<_>>());
        let after = json!((1000..2000).collect::<Vec<_>>());
        let diff = manifest_diff(before, after);
        assert_eq!(diff.changes.len(), MAX_CHANGES);
        assert!(diff.truncated);
    }
}
