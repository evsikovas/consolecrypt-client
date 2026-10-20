use super::*;

#[derive(Debug, Clone, Serialize)]
pub struct DrainCandidate {
    pub reference: ResourceRef,
    pub uid: String,
    pub resource_version: String,
    pub blocker: Option<String>,
}

#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DebugContainer {
    pub name: String,
    pub image: String,
    pub target_container: Option<String>,
    pub command: Vec<String>,
}
redacted_debug!(DebugContainer);

impl ResourceApi {
    pub async fn scale(
        &self,
        reference: &ResourceRef,
        resource_version: &str,
        replicas: u32,
    ) -> Result<(), ResourceError> {
        if reference.api.group != "apps"
            || !matches!(
                reference.api.resource.as_str(),
                "deployments" | "statefulsets" | "replicasets"
            )
            || replicas > 1_000_000
        {
            return Err(ResourceError::InvalidRequest);
        }
        validate_revision(resource_version)?;
        let path = format!("{}/scale", self.object_path(reference, "patch")?);
        let body =
            json!({"metadata":{"resourceVersion":resource_version},"spec":{"replicas":replicas}});
        self.json_request(
            Method::PATCH,
            &path,
            serde_json::to_vec(&body).map_err(|_| ResourceError::InvalidRequest)?,
            Some("application/merge-patch+json"),
        )
        .await?;
        Ok(())
    }

    pub async fn restart(
        &self,
        reference: &ResourceRef,
        resource_version: &str,
    ) -> Result<(), ResourceError> {
        if reference.api.group != "apps"
            || !matches!(
                reference.api.resource.as_str(),
                "deployments" | "statefulsets" | "daemonsets"
            )
        {
            return Err(ResourceError::InvalidRequest);
        }
        validate_revision(resource_version)?;
        self.merge_patch(reference,json!({
            "metadata":{"resourceVersion":resource_version},
            "spec":{"template":{"metadata":{"annotations":{"kubectl.kubernetes.io/restartedAt":chrono::Utc::now().to_rfc3339()}}}}
        })).await?;
        Ok(())
    }

    pub async fn cordon(
        &self,
        node: &str,
        resource_version: &str,
        unschedulable: bool,
    ) -> Result<(), ResourceError> {
        validate_revision(resource_version)?;
        let reference = ResourceRef {
            api: self.resolve("", "v1", "nodes")?,
            namespace: None,
            name: node.into(),
        };
        self.merge_patch(&reference,json!({"metadata":{"resourceVersion":resource_version},"spec":{"unschedulable":unschedulable}})).await?;
        Ok(())
    }

    /// Inspect before an eviction action. DaemonSets/mirror pods are excluded;
    /// unmanaged pods and emptyDir data are explicit blockers, never silently
    /// forced. Eviction still obeys the server's PodDisruptionBudgets.
    pub async fn drain_plan(&self, node: &str) -> Result<Vec<DrainCandidate>, ResourceError> {
        if !dns_name(node, 253) {
            return Err(ResourceError::InvalidRequest);
        }
        let node_ref = ResourceRef {
            api: self.resolve("", "v1", "nodes")?,
            namespace: None,
            name: node.into(),
        };
        let node = self.get(&node_ref, ReadOptions::default()).await?;
        if node.object["spec"]["unschedulable"] != true {
            return Err(ResourceError::InvalidRequest);
        }
        let kind = self.resolve("", "v1", "pods")?;
        let mut query = ListQuery {
            field_selector: Some(format!("spec.nodeName={}", node_ref.name)),
            ..ListQuery::default()
        };
        let mut candidates = Vec::new();
        let mut seen = BTreeSet::new();
        loop {
            let page = self.list(&kind, &query).await?;
            for pod in page.items {
                if pod.object["metadata"]["annotations"]["kubernetes.io/config.mirror"].is_string()
                {
                    continue;
                }
                let owners = pod.object["metadata"]["ownerReferences"].as_array();
                if owners
                    .is_some_and(|owners| owners.iter().any(|owner| owner["kind"] == "DaemonSet"))
                {
                    continue;
                }
                let blocker = if owners.is_none_or(Vec::is_empty) {
                    Some("unmanaged_pod".into())
                } else if pod.object["spec"]["volumes"]
                    .as_array()
                    .is_some_and(|volumes| volumes.iter().any(|v| v.get("emptyDir").is_some()))
                {
                    Some("local_empty_dir_data".into())
                } else {
                    None
                };
                candidates.push(DrainCandidate {
                    reference: ResourceRef {
                        api: kind.clone(),
                        namespace: pod.namespace,
                        name: pod.name,
                    },
                    uid: pod.uid,
                    resource_version: pod.resource_version,
                    blocker,
                });
                if candidates.len() > 20_000 {
                    return Err(ResourceError::TooLarge);
                }
            }
            query.continue_token = page.continue_token;
            if let Some(token) = &query.continue_token {
                if !seen.insert(token.clone()) {
                    return Err(ResourceError::InvalidResponse);
                }
            } else {
                break;
            }
        }
        Ok(candidates)
    }

    pub async fn evict_pod(
        &self,
        reference: &ResourceRef,
        uid: &str,
        resource_version: &str,
    ) -> Result<(), ResourceError> {
        if !reference.api.group.is_empty()
            || reference.api.resource != "pods"
            || uid.is_empty()
            || !safe_query_value(uid, 128)
        {
            return Err(ResourceError::InvalidRequest);
        }
        validate_revision(resource_version)?;
        // Create permission applies to the eviction subresource, not the pod
        // collection. Verify the discovered Pod identity through its get verb.
        let path = format!("{}/eviction", self.object_path(reference, "get")?);
        let body = json!({"apiVersion":"policy/v1","kind":"Eviction",
            "metadata":{"name":reference.name,"namespace":reference.namespace},
            "deleteOptions":{"preconditions":{"uid":uid,"resourceVersion":resource_version}}});
        self.json_request(
            Method::POST,
            &path,
            serde_json::to_vec(&body).map_err(|_| ResourceError::InvalidRequest)?,
            Some("application/json"),
        )
        .await?;
        Ok(())
    }

    pub async fn debug_container(
        &self,
        reference: &ResourceRef,
        resource_version: &str,
        container: DebugContainer,
    ) -> Result<(), ResourceError> {
        if !reference.api.group.is_empty()
            || reference.api.resource != "pods"
            || !dns_name(&container.name, 63)
            || container.image.is_empty()
            || container.image.len() > 1024
            || container.image.chars().any(char::is_whitespace)
            || container.command.is_empty()
            || container.command.len() > 64
            || container.command.iter().map(String::len).sum::<usize>() > 64 * 1024
            || container.command.iter().any(|arg| arg.contains('\0'))
        {
            return Err(ResourceError::InvalidRequest);
        }
        validate_revision(resource_version)?;
        let mut pod = self.get_json(&self.object_path(reference, "get")?).await?;
        if pod["metadata"]["resourceVersion"] != resource_version {
            return Err(ResourceError::Conflict);
        }
        if let Some(target) = &container.target_container {
            if !dns_name(target, 63)
                || !pod["spec"]["containers"]
                    .as_array()
                    .is_some_and(|containers| containers.iter().any(|c| c["name"] == *target))
            {
                return Err(ResourceError::InvalidRequest);
            }
        }
        let mut containers = pod["spec"]["ephemeralContainers"]
            .as_array_mut()
            .map(std::mem::take)
            .unwrap_or_default();
        if containers.len() >= 64 || containers.iter().any(|c| c["name"] == container.name) {
            return Err(ResourceError::InvalidRequest);
        }
        let mut entry = json!({"name":container.name,"image":container.image,"command":container.command,"stdin":true,"tty":true,
            "securityContext":{"privileged":false,"allowPrivilegeEscalation":false}});
        if let Some(target) = container.target_container {
            entry["targetContainerName"] = Value::String(target);
        }
        containers.push(entry);
        let body = json!({"metadata":{"resourceVersion":resource_version},"spec":{"ephemeralContainers":containers}});
        let path = format!(
            "{}/ephemeralcontainers",
            self.object_path(reference, "patch")?
        );
        self.json_request(
            Method::PATCH,
            &path,
            serde_json::to_vec(&body).map_err(|_| ResourceError::InvalidRequest)?,
            Some("application/merge-patch+json"),
        )
        .await?;
        Ok(())
    }

    /// Selectors come from the saved workload object, including match
    /// expressions, rather than assuming an app=<name> convention.
    pub async fn workload_pods(
        &self,
        reference: &ResourceRef,
    ) -> Result<ResourcePage, ResourceError> {
        if !((reference.api.group == "apps"
            && matches!(
                reference.api.resource.as_str(),
                "deployments" | "replicasets" | "statefulsets" | "daemonsets"
            ))
            || (reference.api.group == "batch" && reference.api.resource == "jobs")
            || (reference.api.group.is_empty() && reference.api.resource == "services"))
        {
            return Err(ResourceError::InvalidRequest);
        }
        let object = self.get(reference, ReadOptions::default()).await?;
        let selector = if reference.api.resource == "services" {
            selector_from_labels(&object.object["spec"]["selector"])?
        } else {
            selector(&object.object["spec"]["selector"])?
        };
        self.list(
            &self.resolve("", "v1", "pods")?,
            &ListQuery {
                namespace: reference.namespace.clone(),
                label_selector: Some(selector),
                ..ListQuery::default()
            },
        )
        .await
    }

    pub async fn object_events(
        &self,
        reference: &ResourceRef,
        uid: &str,
    ) -> Result<ResourcePage, ResourceError> {
        if uid.is_empty() || !safe_query_value(uid, 128) || uid.contains([',', '=', '!']) {
            return Err(ResourceError::InvalidRequest);
        }
        self.object_path(reference, "get")?;
        self.list(
            &self.resolve("", "v1", "events")?,
            &ListQuery {
                namespace: reference.namespace.clone(),
                field_selector: Some(format!("involvedObject.uid={uid}")),
                ..ListQuery::default()
            },
        )
        .await
    }
}

fn validate_revision(value: &str) -> Result<(), ResourceError> {
    if value.is_empty() || !safe_query_value(value, 128) {
        Err(ResourceError::InvalidRequest)
    } else {
        Ok(())
    }
}

fn label_component(value: &str, allow_empty: bool) -> bool {
    (allow_empty || !value.is_empty())
        && value.len() <= 317
        && value
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'.' | b'-' | b'_' | b'/'))
}

pub(super) fn selector_from_labels(value: &Value) -> Result<String, ResourceError> {
    let labels = value.as_object().ok_or(ResourceError::InvalidResponse)?;
    if labels.is_empty() || labels.len() > 128 {
        return Err(ResourceError::InvalidResponse);
    }
    let mut parts = Vec::new();
    for (key, value) in labels {
        let value = value.as_str().ok_or(ResourceError::InvalidResponse)?;
        if !label_component(key, false) || !label_component(value, true) {
            return Err(ResourceError::InvalidResponse);
        }
        parts.push(format!("{key}={value}"));
    }
    Ok(parts.join(","))
}

pub(super) fn selector(value: &Value) -> Result<String, ResourceError> {
    let mut parts = Vec::new();
    if let Some(labels) = value.get("matchLabels") {
        if labels.as_object().is_some_and(|labels| !labels.is_empty()) {
            parts.push(selector_from_labels(labels)?);
        }
    }
    if let Some(expressions) = value["matchExpressions"].as_array() {
        if expressions.len() > 128 {
            return Err(ResourceError::InvalidResponse);
        }
        for expression in expressions {
            let key = expression["key"]
                .as_str()
                .ok_or(ResourceError::InvalidResponse)?;
            if !label_component(key, false) {
                return Err(ResourceError::InvalidResponse);
            }
            let operation = expression["operator"]
                .as_str()
                .ok_or(ResourceError::InvalidResponse)?;
            let values = expression["values"].as_array().cloned().unwrap_or_default();
            match operation {
                "Exists" if values.is_empty() => parts.push(key.into()),
                "DoesNotExist" if values.is_empty() => parts.push(format!("!{key}")),
                "In" | "NotIn" if !values.is_empty() && values.len() <= 128 => {
                    let values = values
                        .iter()
                        .map(|value| {
                            value
                                .as_str()
                                .filter(|v| label_component(v, true))
                                .ok_or(ResourceError::InvalidResponse)
                        })
                        .collect::<Result<Vec<_>, _>>()?;
                    parts.push(format!(
                        "{key} {} ({})",
                        if operation == "In" { "in" } else { "notin" },
                        values.join(",")
                    ));
                }
                _ => return Err(ResourceError::InvalidResponse),
            }
        }
    }
    let result = parts.join(",");
    if result.is_empty() || result.len() > 4096 {
        return Err(ResourceError::InvalidResponse);
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn workload_selector_supports_expressions_without_selector_injection() {
        assert_eq!(selector(&json!({"matchLabels":{"app":"shop"},"matchExpressions":[
            {"key":"tier","operator":"In","values":["web","worker"]},{"key":"disabled","operator":"DoesNotExist"}
        ]})).unwrap(),"app=shop,tier in (web,worker),!disabled");
        assert!(selector(&json!({"matchLabels":{"app":"shop,other=evil"}})).is_err());
        assert!(selector(&json!({})).is_err());
    }
}
