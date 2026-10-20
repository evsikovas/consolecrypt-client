use super::*;

#[derive(Clone, Serialize)]
pub struct WorkloadRevision {
    pub revision: u64,
    pub name: String,
    pub created_at: Option<String>,
    pub replicas: Option<u64>,
    pub template: Value,
}
redacted_debug!(WorkloadRevision);

impl ResourceApi {
    /// Controller-owned history only. Labels alone are insufficient because
    /// another workload can deliberately reuse them.
    pub async fn rollout_history(
        &self,
        reference: &ResourceRef,
    ) -> Result<Vec<WorkloadRevision>, ResourceError> {
        if reference.api.group != "apps"
            || !matches!(
                reference.api.kind.as_str(),
                "Deployment" | "StatefulSet" | "DaemonSet"
            )
        {
            return Err(ResourceError::InvalidRequest);
        }
        let owner = self.get(reference, ReadOptions::default()).await?;
        let resource = if reference.api.kind == "Deployment" {
            "replicasets"
        } else {
            "controllerrevisions"
        };
        let kind = self.resolve("apps", "v1", resource)?;
        let mut query = ListQuery {
            namespace: reference.namespace.clone(),
            limit: Some(200),
            ..ListQuery::default()
        };
        let mut continuations = BTreeSet::new();
        let mut scanned = 0_usize;
        let mut bytes = 0_usize;
        let mut revisions = Vec::new();
        loop {
            let page = self.list(&kind, &query).await?;
            scanned += page.items.len();
            if scanned > 20_000 {
                return Err(ResourceError::TooLarge);
            }
            for row in page.items {
                if let Some(revision) = owned_revision(&row.object, &owner.summary.uid) {
                    bytes = bytes.saturating_add(
                        serde_json::to_vec(&revision.template)
                            .map_err(|_| ResourceError::InvalidResponse)?
                            .len(),
                    );
                    if bytes > 8 * 1024 * 1024 || revisions.len() >= 1000 {
                        return Err(ResourceError::TooLarge);
                    }
                    revisions.push(revision);
                }
            }
            query.continue_token = page.continue_token;
            let Some(token) = &query.continue_token else {
                break;
            };
            if !continuations.insert(token.clone()) {
                return Err(ResourceError::InvalidResponse);
            }
        }
        revisions.sort_by_key(|row| row.revision);
        Ok(revisions)
    }

    pub async fn diff_revisions(
        &self,
        reference: &ResourceRef,
        before: u64,
        after: u64,
    ) -> Result<ManifestDiff, ResourceError> {
        if before == 0 || after == 0 {
            return Err(ResourceError::InvalidRequest);
        }
        let revisions = self.rollout_history(reference).await?;
        let before = revisions
            .iter()
            .find(|row| row.revision == before)
            .ok_or(ResourceError::NotFound)?;
        let after = revisions
            .iter()
            .find(|row| row.revision == after)
            .ok_or(ResourceError::NotFound)?;
        Ok(manifest_diff(
            before.template.clone(),
            after.template.clone(),
        ))
    }
}

fn owned_revision(object: &Value, owner_uid: &str) -> Option<WorkloadRevision> {
    if owner_uid.is_empty()
        || !object["metadata"]["ownerReferences"]
            .as_array()?
            .iter()
            .any(|owner| owner["uid"] == owner_uid && owner["controller"] == true)
    {
        return None;
    }
    let revision = if object["kind"] == "ReplicaSet" {
        object["metadata"]["annotations"]["deployment.kubernetes.io/revision"]
            .as_str()?
            .parse::<u64>()
            .ok()?
    } else {
        object["revision"].as_u64()?
    };
    let mut template = if object["kind"] == "ReplicaSet" {
        object["spec"]["template"].clone()
    } else {
        object["data"]["spec"]["template"].clone()
    };
    if !template.is_object() || revision == 0 {
        return None;
    }
    if let Some(metadata) = template["metadata"].as_object_mut() {
        metadata.remove("creationTimestamp");
        if let Some(labels) = metadata.get_mut("labels").and_then(Value::as_object_mut) {
            labels.remove("pod-template-hash");
        }
    }
    if let Some(object) = template.as_object_mut() {
        object.remove("$patch");
    }
    Some(WorkloadRevision {
        revision,
        name: object["metadata"]["name"].as_str()?.to_owned(),
        created_at: object["metadata"]["creationTimestamp"]
            .as_str()
            .map(str::to_owned),
        replicas: object["spec"]["replicas"].as_u64(),
        template,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn history_checks_controller_uid_and_removes_generated_template_fields() {
        let object = json!({"kind":"ReplicaSet","metadata":{"name":"web-abc",
            "ownerReferences":[{"uid":"owner","controller":true}],
            "annotations":{"deployment.kubernetes.io/revision":"2"}},
            "spec":{"replicas":3,"template":{"metadata":{"labels":{"pod-template-hash":"abc","app":"web"},"creationTimestamp":null},
            "spec":{"containers":[{"name":"web","image":"example:v2"}]}}}});
        assert!(owned_revision(&object, "another-owner").is_none());
        let revision = owned_revision(&object, "owner").unwrap();
        assert_eq!(revision.revision, 2);
        assert_eq!(revision.replicas, Some(3));
        assert!(revision.template["metadata"]["labels"]
            .get("pod-template-hash")
            .is_none());
        assert_eq!(revision.template["metadata"]["labels"]["app"], "web");
    }
}
