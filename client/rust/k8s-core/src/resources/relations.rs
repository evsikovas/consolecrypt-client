use super::*;

const MAX_KINDS: usize = 64;
const MAX_SCANNED: usize = 10_000;
const MAX_BYTES: usize = 16 * 1024 * 1024;
const MAX_RELATED: usize = 2000;

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum ResourceRelation {
    Owner,
    Dependent,
    SelectedPod,
    Service,
    Ingress,
    Volume,
    UsesVolume,
    Configuration,
    UsesConfiguration,
}
#[derive(Debug, Clone, Serialize)]
pub struct RelatedObject {
    pub reference: ResourceRef,
    pub uid: String,
    pub status: String,
    pub relation: ResourceRelation,
}
#[derive(Debug, Default, Serialize)]
pub struct ResourceRelations {
    pub items: Vec<RelatedObject>,
    pub partial: bool,
    pub unavailable_kinds: Vec<ResourceKind>,
}

struct Target {
    reference: ResourceRef,
    expected_uid: Option<String>,
    relation: ResourceRelation,
}

impl ResourceApi {
    /// Read relationships without returning object bodies. Owner identities are
    /// verified by UID, so a replacement object with the same name is not an
    /// owner. Arbitrary resources are discovered, never addressed by raw paths.
    pub async fn related_objects(
        &self,
        reference: &ResourceRef,
    ) -> Result<ResourceRelations, ResourceError> {
        let source = self.get(reference, ReadOptions::AI).await?;
        self.related_objects_from_detail(reference, &source, false)
            .await
    }

    /// Selected-object AI context reuses its existing read and uses smaller
    /// scan/time bounds. It never recursively follows related resources.
    pub(crate) async fn related_objects_for_ai(
        &self,
        reference: &ResourceRef,
        source: &ResourceDetail,
    ) -> Result<ResourceRelations, ResourceError> {
        self.related_objects_from_detail(reference, source, true)
            .await
    }

    async fn related_objects_from_detail(
        &self,
        reference: &ResourceRef,
        source: &ResourceDetail,
        ai: bool,
    ) -> Result<ResourceRelations, ResourceError> {
        let deadline = tokio::time::Instant::now() + Duration::from_secs(if ai { 5 } else { 30 });
        if source.summary.uid.is_empty() {
            return Err(ResourceError::InvalidResponse);
        }
        let mut result = ResourceRelations::default();
        let mut kinds = self.kinds()?;
        // Prefer stable versions; scan a served resource at most once.
        kinds.sort_by_key(|kind| {
            (
                !matches!(
                    kind.group.as_str(),
                    "" | "apps" | "batch" | "networking.k8s.io"
                ),
                kind.version.contains("alpha") || kind.version.contains("beta"),
                kind.key(),
            )
        });
        let mut targets = Vec::new();
        if let Some(owners) = source.object["metadata"]["ownerReferences"].as_array() {
            for owner in owners.iter().take(128) {
                let Some((api, name, uid)) = owner_identity(owner, &kinds) else {
                    result.partial = true;
                    continue;
                };
                targets.push(Target {
                    reference: ResourceRef {
                        namespace: api
                            .namespaced
                            .then(|| reference.namespace.clone())
                            .flatten(),
                        api,
                        name,
                    },
                    expected_uid: Some(uid),
                    relation: ResourceRelation::Owner,
                });
            }
            result.partial |= owners.len() > 128;
        }
        let mut named = named_dependencies(&source.object);
        if named.len() > 128 {
            named.truncate(128);
            result.partial = true;
        }
        for (group, kind_name, name, relation) in named {
            let Some(api) = kinds
                .iter()
                .find(|kind| kind.group == group && kind.kind == kind_name)
                .cloned()
            else {
                result.partial = true;
                continue;
            };
            targets.push(Target {
                reference: ResourceRef {
                    namespace: api
                        .namespaced
                        .then(|| reference.namespace.clone())
                        .flatten(),
                    api,
                    name,
                },
                expected_uid: None,
                relation,
            });
        }
        if ai && targets.len() > 8 {
            targets.truncate(8);
            result.partial = true;
        }
        for target in targets {
            match tokio::time::timeout_at(deadline, self.get(&target.reference, ReadOptions::AI))
                .await
            {
                Ok(Ok(detail)) => {
                    if target
                        .expected_uid
                        .as_ref()
                        .is_some_and(|uid| uid != &detail.summary.uid)
                    {
                        result.partial = true;
                        continue;
                    }
                    // A bound PV may have been released and rebound since the
                    // PVC was read. Do not display that replacement binding.
                    if source.object["kind"] == "PersistentVolumeClaim"
                        && detail.object["kind"] == "PersistentVolume"
                        && detail.object["spec"]["claimRef"]["uid"] != source.summary.uid
                    {
                        result.partial = true;
                        continue;
                    }
                    insert(
                        &mut result,
                        &target.reference.api,
                        detail.summary,
                        target.relation,
                    );
                }
                Ok(Err(ResourceError::Closed)) => return Err(ResourceError::Closed),
                Ok(Err(_)) => unavailable(&mut result, &target.reference.api),
                Err(_) => {
                    result.partial = true;
                    return Ok(result);
                }
            }
        }
        let mut served = BTreeSet::new();
        kinds.retain(|kind| {
            kind.verbs.iter().any(|verb| verb == "list")
                && served.insert((kind.group.clone(), kind.resource.clone()))
        });
        let max_kinds = if ai { 12 } else { MAX_KINDS };
        if ai {
            kinds.sort_by_key(|kind| {
                (
                    !matches!(
                        kind.resource.as_str(),
                        "pods"
                            | "services"
                            | "persistentvolumeclaims"
                            | "persistentvolumes"
                            | "ingresses"
                            | "replicasets"
                            | "deployments"
                            | "statefulsets"
                            | "daemonsets"
                            | "jobs"
                    ),
                    kind.key(),
                )
            });
        }
        result.partial |= kinds.len() > max_kinds;
        kinds.truncate(max_kinds);
        let mut scanned = 0;
        let mut bytes = 0;
        for kind in kinds {
            let mut query = ListQuery {
                namespace: if kind.namespaced {
                    reference.namespace.clone()
                } else {
                    None
                },
                limit: Some(200),
                ..ListQuery::default()
            };
            let mut tokens = BTreeSet::new();
            loop {
                let page = match tokio::time::timeout_at(deadline, self.list(&kind, &query)).await {
                    Ok(Ok(page)) => page,
                    Ok(Err(ResourceError::Closed)) => return Err(ResourceError::Closed),
                    Ok(Err(_)) => {
                        unavailable(&mut result, &kind);
                        break;
                    }
                    Err(_) => {
                        result.partial = true;
                        return Ok(result);
                    }
                };
                for row in page.items {
                    scanned += 1;
                    bytes += serde_json::to_vec(&row.object)
                        .map_err(|_| ResourceError::InvalidResponse)?
                        .len();
                    if scanned > (if ai { 500 } else { MAX_SCANNED })
                        || bytes > (if ai { 2 * 1024 * 1024 } else { MAX_BYTES })
                        || result.items.len() >= (if ai { 50 } else { MAX_RELATED })
                    {
                        result.partial = true;
                        return Ok(result);
                    }
                    if row.uid == source.summary.uid {
                        continue;
                    }
                    for relation in relationships(&source.object, &row.object) {
                        insert(&mut result, &kind, row.clone(), relation);
                    }
                }
                query.continue_token = page.continue_token;
                let Some(token) = &query.continue_token else {
                    break;
                };
                if !tokens.insert(token.clone()) {
                    result.partial = true;
                    break;
                }
            }
        }
        Ok(result)
    }
}

fn unavailable(result: &mut ResourceRelations, kind: &ResourceKind) {
    result.partial = true;
    if !result.unavailable_kinds.iter().any(|old| old == kind) {
        result.unavailable_kinds.push(kind.clone());
    }
}
fn insert(
    result: &mut ResourceRelations,
    kind: &ResourceKind,
    row: ResourceRow,
    relation: ResourceRelation,
) {
    if row.uid.is_empty()
        || result
            .items
            .iter()
            .any(|item| item.uid == row.uid && item.relation == relation)
    {
        return;
    }
    result.items.push(RelatedObject {
        reference: ResourceRef {
            api: kind.clone(),
            namespace: row.namespace,
            name: row.name,
        },
        uid: row.uid,
        status: row.status,
        relation,
    });
}
fn owner_identity(owner: &Value, kinds: &[ResourceKind]) -> Option<(ResourceKind, String, String)> {
    let api_version = owner["apiVersion"].as_str()?;
    let (group, version) = api_version.split_once('/').unwrap_or(("", api_version));
    let api = kinds
        .iter()
        .find(|api| api.group == group && api.version == version && owner["kind"] == api.kind)?
        .clone();
    let name = owner["name"].as_str()?;
    let uid = owner["uid"].as_str()?;
    if name.is_empty() || uid.is_empty() || uid.len() > 128 {
        return None;
    }
    Some((api, name.into(), uid.into()))
}
fn relationships(source: &Value, candidate: &Value) -> Vec<ResourceRelation> {
    let mut result = Vec::new();
    let uid = source["metadata"]["uid"].as_str().unwrap_or_default();
    let same_namespace = source["metadata"]["namespace"] == candidate["metadata"]["namespace"];
    if !uid.is_empty()
        && (source["metadata"]["namespace"].is_null() || same_namespace)
        && candidate["metadata"]["ownerReferences"]
            .as_array()
            .is_some_and(|owners| owners.iter().any(|owner| owner["uid"] == uid))
    {
        result.push(ResourceRelation::Dependent);
    }
    if !same_namespace {
        return result;
    }
    if source["kind"] == "Service" && candidate["kind"] == "Pod" && selected(source, candidate) {
        result.push(ResourceRelation::SelectedPod);
    }
    if source["kind"] == "Pod" && candidate["kind"] == "Service" && selected(candidate, source) {
        result.push(ResourceRelation::Service);
    }
    let source_name = source["metadata"]["name"].as_str().unwrap_or_default();
    for (group, kind, name, relation) in named_dependencies(candidate) {
        let api_version = source["apiVersion"].as_str().unwrap_or_default();
        let source_group = api_version.split_once('/').map_or("", |(group, _)| group);
        if group == source_group && source["kind"] == kind && source_name == name {
            result.push(match relation {
                ResourceRelation::Service => ResourceRelation::Ingress,
                ResourceRelation::Volume => ResourceRelation::UsesVolume,
                _ => ResourceRelation::UsesConfiguration,
            });
        }
    }
    result.sort();
    result.dedup();
    result
}
fn selected(service: &Value, pod: &Value) -> bool {
    service["spec"]["selector"]
        .as_object()
        .is_some_and(|selector| {
            !selector.is_empty()
                && selector.iter().all(|(key, value)| {
                    value.is_string() && pod["metadata"]["labels"].get(key) == Some(value)
                })
        })
}

type NamedDependency = (&'static str, &'static str, String, ResourceRelation);
fn named_dependencies(object: &Value) -> Vec<NamedDependency> {
    let mut references = Vec::new();
    let mut add = |kind, name: &Value, relation| {
        if let Some(name) = name
            .as_str()
            .filter(|name| !name.is_empty() && name.len() <= 253)
        {
            references.push(("", kind, name.to_owned(), relation));
        }
    };
    let kind = object["kind"].as_str().unwrap_or_default();
    if kind == "PersistentVolumeClaim" {
        add(
            "PersistentVolume",
            &object["spec"]["volumeName"],
            ResourceRelation::Volume,
        );
    }
    if kind == "Ingress" {
        add(
            "Service",
            &object["spec"]["defaultBackend"]["service"]["name"],
            ResourceRelation::Service,
        );
        if let Some(rules) = object["spec"]["rules"].as_array() {
            for path in rules
                .iter()
                .filter_map(|rule| rule["http"]["paths"].as_array())
                .flatten()
            {
                add(
                    "Service",
                    &path["backend"]["service"]["name"],
                    ResourceRelation::Service,
                );
            }
        }
        if let Some(tls) = object["spec"]["tls"].as_array() {
            for entry in tls {
                add(
                    "Secret",
                    &entry["secretName"],
                    ResourceRelation::Configuration,
                );
            }
        }
    }
    let spec = match kind {
        "Pod" => &object["spec"],
        "CronJob" => &object["spec"]["jobTemplate"]["spec"]["template"]["spec"],
        "Deployment"
        | "ReplicaSet"
        | "StatefulSet"
        | "DaemonSet"
        | "Job"
        | "ReplicationController" => &object["spec"]["template"]["spec"],
        _ => return references,
    };
    for key in ["containers", "initContainers", "ephemeralContainers"] {
        if let Some(containers) = spec[key].as_array() {
            for container in containers {
                if let Some(env) = container["envFrom"].as_array() {
                    for entry in env {
                        add(
                            "ConfigMap",
                            &entry["configMapRef"]["name"],
                            ResourceRelation::Configuration,
                        );
                        add(
                            "Secret",
                            &entry["secretRef"]["name"],
                            ResourceRelation::Configuration,
                        );
                    }
                }
                if let Some(env) = container["env"].as_array() {
                    for entry in env {
                        add(
                            "ConfigMap",
                            &entry["valueFrom"]["configMapKeyRef"]["name"],
                            ResourceRelation::Configuration,
                        );
                        add(
                            "Secret",
                            &entry["valueFrom"]["secretKeyRef"]["name"],
                            ResourceRelation::Configuration,
                        );
                    }
                }
            }
        }
    }
    if let Some(volumes) = spec["volumes"].as_array() {
        for volume in volumes {
            add(
                "ConfigMap",
                &volume["configMap"]["name"],
                ResourceRelation::Configuration,
            );
            add(
                "Secret",
                &volume["secret"]["secretName"],
                ResourceRelation::Configuration,
            );
            add(
                "PersistentVolumeClaim",
                &volume["persistentVolumeClaim"]["claimName"],
                ResourceRelation::Volume,
            );
            if let Some(sources) = volume["projected"]["sources"].as_array() {
                for source in sources {
                    add(
                        "ConfigMap",
                        &source["configMap"]["name"],
                        ResourceRelation::Configuration,
                    );
                    add(
                        "Secret",
                        &source["secret"]["name"],
                        ResourceRelation::Configuration,
                    );
                }
            }
        }
    }
    if let Some(secrets) = spec["imagePullSecrets"].as_array() {
        for secret in secrets {
            add("Secret", &secret["name"], ResourceRelation::Configuration);
        }
    }
    references.sort();
    references.dedup();
    references
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn owners_are_uid_bound_and_service_selectors_never_select_all() {
        let owner =
            json!({"kind":"Deployment","metadata":{"uid":"a","name":"web","namespace":"apps"}});
        let mut pod = json!({"kind":"Pod","metadata":{"name":"pod","namespace":"apps","labels":{"app":"web"},"ownerReferences":[{"uid":"old-a"}]}});
        assert!(relationships(&owner, &pod).is_empty());
        pod["metadata"]["ownerReferences"][0]["uid"] = json!("a");
        assert_eq!(relationships(&owner, &pod), [ResourceRelation::Dependent]);
        pod["metadata"]["namespace"] = json!("other");
        assert!(relationships(&owner, &pod).is_empty());
        let mut service =
            json!({"kind":"Service","metadata":{"namespace":"other"},"spec":{"selector":{}}});
        assert!(!selected(&service, &pod));
        service["spec"]["selector"] = json!({"app":"web"});
        assert!(selected(&service, &pod));
    }
    #[test]
    fn configuration_usages_scan_references_not_values_and_respect_namespace() {
        let secret = json!({"apiVersion":"v1","kind":"Secret","metadata":{"name":"pull","namespace":"apps","uid":"secret"}});
        let mut workload = json!({"kind":"Deployment","metadata":{"namespace":"apps"},"spec":{"template":{"spec":{
            "imagePullSecrets":[{"name":"pull"}],"containers":[{"name":"web","env":[{"name":"X","value":"pull"}]}]
        }}}});
        assert_eq!(
            relationships(&secret, &workload),
            [ResourceRelation::UsesConfiguration]
        );
        workload["spec"]["template"]["spec"]["imagePullSecrets"] = json!([]);
        assert!(relationships(&secret, &workload).is_empty());
        workload["spec"]["template"]["spec"]["volumes"] =
            json!([{"projected":{"sources":[{"secret":{"name":"pull"}}]}}]);
        assert_eq!(
            relationships(&secret, &workload),
            [ResourceRelation::UsesConfiguration]
        );
        workload["metadata"]["namespace"] = json!("other");
        assert!(relationships(&secret, &workload).is_empty());
    }
}
