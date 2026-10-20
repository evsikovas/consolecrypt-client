//! Native GitOps views and narrowly typed mutations. No external controller
//! endpoint or executable is needed, and every mutation uses ProdGuard.

use crate::resources::{ResourceApi, ResourceError, ResourceRef};
use serde::Serialize;
use serde_json::{json, Value};
use std::{
    collections::{BTreeMap, BTreeSet},
    time::{SystemTime, UNIX_EPOCH},
};

#[derive(Debug, Clone, Copy)]
pub enum FluxAction {
    Reconcile,
    ReconcileWithSource,
    Suspend,
    Resume,
    ResetFailures,
    ForceUpgrade,
}
#[derive(Debug, Clone, Copy)]
pub enum ArgoAction {
    Refresh,
    HardRefresh,
    Sync,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq, PartialOrd, Ord)]
pub struct ObjectIdentity {
    pub group: String,
    pub kind: String,
    pub namespace: String,
    pub name: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct DependencyEdge {
    pub from: ObjectIdentity,
    pub to: ObjectIdentity,
    pub ready: bool,
    pub missing: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct GitOpsOverview {
    pub ready: usize,
    pub not_ready: usize,
    pub suspended: usize,
    pub stalled: usize,
    pub reconciling: usize,
    pub dependencies: Vec<DependencyEdge>,
    pub cycles: Vec<ObjectIdentity>,
}

#[derive(Debug, Clone, Serialize)]
pub struct FlowNode {
    pub object: ObjectIdentity,
    /// None means the referenced object's status was not included in the view.
    pub ready: Option<bool>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq, PartialOrd, Ord)]
pub struct FlowEdge {
    pub from: ObjectIdentity,
    pub to: ObjectIdentity,
    pub relation: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct GitOpsFlow {
    pub nodes: Vec<FlowNode>,
    pub edges: Vec<FlowEdge>,
    pub truncated: bool,
}

/// Builds the data flow from sources through controllers to their inventory.
/// Missing status is represented explicitly; a graph edge is never proof of
/// ownership or permission to mutate the referenced resource.
pub fn flow_graph(objects: &[Value]) -> GitOpsFlow {
    let mut nodes = BTreeMap::new();
    let mut edges = BTreeSet::new();
    let mut truncated = objects.len() > 20_000;
    let mut observed = BTreeMap::new();
    for object in objects.iter().take(20_000) {
        if let Some(id) = resource_identity(object) {
            // Duplicate snapshots must not fabricate a definitive status.
            observed
                .entry(id)
                .and_modify(|ready| *ready = None)
                .or_insert_with(|| object_ready(object));
        }
    }
    for object in objects.iter().take(20_000) {
        let Some(id) = identity(object) else { continue };
        let ready = observed.get(&id).copied().flatten();
        nodes.insert(id, ready);
    }
    for object in objects.iter().take(20_000) {
        let Some(id) = identity(object) else { continue };
        let mut add = |from: ObjectIdentity, to: ObjectIdentity, relation: &str| {
            if edges.len() >= 50_000 || nodes.len() >= 50_000 {
                truncated = true;
                return;
            }
            nodes
                .entry(from.clone())
                .or_insert_with(|| observed.get(&from).copied().flatten());
            nodes
                .entry(to.clone())
                .or_insert_with(|| observed.get(&to).copied().flatten());
            edges.insert(FlowEdge {
                from,
                to,
                relation: relation.into(),
            });
        };
        for source in [
            &object["spec"]["sourceRef"],
            &object["spec"]["chartRef"],
            &object["spec"]["chart"]["spec"]["sourceRef"],
        ] {
            if let (Some(kind), Some(name)) = (source["kind"].as_str(), source["name"].as_str()) {
                if kind.len() <= 128 && name.len() <= 253 {
                    add(
                        ObjectIdentity {
                            group: "source.toolkit.fluxcd.io".into(),
                            kind: kind.into(),
                            name: name.into(),
                            namespace: source["namespace"]
                                .as_str()
                                .unwrap_or(&id.namespace)
                                .chars()
                                .take(63)
                                .collect(),
                        },
                        id.clone(),
                        "source",
                    );
                }
            }
        }
        for dependency in object["spec"]["dependsOn"]
            .as_array()
            .into_iter()
            .flatten()
            .take(1024)
        {
            if let Some(name) = dependency["name"].as_str().filter(|name| name.len() <= 253) {
                add(
                    ObjectIdentity {
                        group: id.group.clone(),
                        kind: id.kind.clone(),
                        name: name.into(),
                        namespace: dependency["namespace"]
                            .as_str()
                            .unwrap_or(&id.namespace)
                            .chars()
                            .take(63)
                            .collect(),
                    },
                    id.clone(),
                    "depends_on",
                );
            }
        }
        for entry in object["status"]["inventory"]["entries"]
            .as_array()
            .into_iter()
            .flatten()
        {
            if let Some(inventory) = entry["id"].as_str().filter(|id| id.len() <= 1024) {
                // Flux's inventory ID is namespace_name_group_kind. The
                // namespace and object name cannot contain an underscore.
                let parts = inventory.split('_').collect::<Vec<_>>();
                if parts.len() == 4 && !parts[1].is_empty() && !parts[3].is_empty() {
                    add(
                        id.clone(),
                        ObjectIdentity {
                            group: parts[2].into(),
                            namespace: parts[0].into(),
                            name: parts[1].into(),
                            kind: parts[3].into(),
                        },
                        "inventory",
                    );
                }
            }
        }
        if object["apiVersion"]
            .as_str()
            .is_some_and(|version| version.starts_with("argoproj.io/"))
        {
            for resource in object["status"]["resources"]
                .as_array()
                .into_iter()
                .flatten()
            {
                if let (Some(kind), Some(name)) =
                    (resource["kind"].as_str(), resource["name"].as_str())
                {
                    if kind.len() <= 128 && name.len() <= 253 {
                        add(
                            id.clone(),
                            ObjectIdentity {
                                group: resource["group"]
                                    .as_str()
                                    .unwrap_or("")
                                    .chars()
                                    .take(253)
                                    .collect(),
                                kind: kind.into(),
                                name: name.into(),
                                namespace: resource["namespace"]
                                    .as_str()
                                    .unwrap_or("")
                                    .chars()
                                    .take(63)
                                    .collect(),
                            },
                            "inventory",
                        );
                    }
                }
            }
        }
    }
    GitOpsFlow {
        nodes: nodes
            .into_iter()
            .map(|(object, ready)| FlowNode { object, ready })
            .collect(),
        edges: edges.into_iter().collect(),
        truncated,
    }
}

impl ResourceApi {
    /// Queues upstream sources before the selected controller. Read and
    /// validate the complete bounded chain before mutating any object; each
    /// individual PATCH still passes the current native production guard.
    pub async fn flux_reconcile_with_source(
        &self,
        reference: &ResourceRef,
    ) -> Result<(), ResourceError> {
        if !matches!(
            (reference.api.group.as_str(), reference.api.kind.as_str()),
            ("kustomize.toolkit.fluxcd.io", "Kustomization")
                | ("helm.toolkit.fluxcd.io", "HelmRelease")
        ) {
            return Err(ResourceError::InvalidRequest);
        }
        let mut chain = Vec::new();
        let mut seen = BTreeSet::new();
        let mut current = reference.clone();
        loop {
            if chain.len() >= 4
                || !seen.insert((
                    current.api.group.clone(),
                    current.api.resource.clone(),
                    current.namespace.clone(),
                    current.name.clone(),
                ))
            {
                return Err(ResourceError::InvalidRequest);
            }
            let detail = self
                .get(&current, crate::resources::ReadOptions::AI)
                .await?;
            if detail.summary.uid.is_empty() || detail.summary.resource_version.is_empty() {
                return Err(ResourceError::InvalidResponse);
            }
            let next = self.flux_upstream(&current, &detail.object)?;
            chain.push((current, detail.summary.resource_version));
            let Some(next) = next else {
                break;
            };
            current = next;
        }
        let now = uuid::Uuid::new_v4().to_string();
        for (target, version) in chain.into_iter().rev() {
            self.merge_patch(
                &target,
                json!({"metadata":{"resourceVersion":version,
                "annotations":{"reconcile.fluxcd.io/requestedAt":now}}}),
            )
            .await?;
        }
        Ok(())
    }

    fn flux_upstream(
        &self,
        current: &ResourceRef,
        object: &Value,
    ) -> Result<Option<ResourceRef>, ResourceError> {
        let generated;
        let source = match current.api.kind.as_str() {
            "Kustomization" | "HelmChart" => &object["spec"]["sourceRef"],
            "HelmRelease" if object["spec"]["chartRef"].is_object() => &object["spec"]["chartRef"],
            "HelmRelease" if object["status"]["helmChart"].is_string() => {
                let parts: Vec<_> = object["status"]["helmChart"]
                    .as_str()
                    .unwrap_or_default()
                    .split('/')
                    .collect();
                if parts.len() != 2 || parts.iter().any(|part| part.is_empty()) {
                    return Err(ResourceError::InvalidResponse);
                }
                generated = json!({"kind":"HelmChart","namespace":parts[0],"name":parts[1]});
                &generated
            }
            "HelmRelease" => &object["spec"]["chart"]["spec"]["sourceRef"],
            "GitRepository" | "OCIRepository" | "HelmRepository" | "Bucket" => return Ok(None),
            _ => return Err(ResourceError::InvalidRequest),
        };
        let kind = source["kind"]
            .as_str()
            .ok_or(ResourceError::InvalidResponse)?;
        if !matches!(
            kind,
            "GitRepository" | "OCIRepository" | "HelmRepository" | "HelmChart" | "Bucket"
        ) {
            return Err(ResourceError::InvalidRequest);
        }
        let mut kinds = self
            .kinds()?
            .into_iter()
            .filter(|candidate| {
                candidate.group == "source.toolkit.fluxcd.io"
                    && candidate.kind == kind
                    && candidate.verbs.iter().any(|verb| verb == "patch")
            })
            .collect::<Vec<_>>();
        kinds.sort_by_key(|kind| {
            (
                kind.version.contains("beta") || kind.version.contains("alpha"),
                kind.version.clone(),
            )
        });
        let api = kinds
            .into_iter()
            .next()
            .ok_or(ResourceError::UnknownResource)?;
        Ok(Some(ResourceRef {
            api,
            namespace: source["namespace"]
                .as_str()
                .map(str::to_owned)
                .or_else(|| current.namespace.clone()),
            name: source["name"]
                .as_str()
                .ok_or(ResourceError::InvalidResponse)?
                .into(),
        }))
    }

    pub async fn flux_action(
        &self,
        reference: &ResourceRef,
        action: FluxAction,
        reason: Option<&str>,
    ) -> Result<(), ResourceError> {
        if matches!(action, FluxAction::ReconcileWithSource) {
            return self.flux_reconcile_with_source(reference).await;
        }
        let group = &reference.api.group;
        if !matches!(
            group.as_str(),
            "source.toolkit.fluxcd.io"
                | "kustomize.toolkit.fluxcd.io"
                | "helm.toolkit.fluxcd.io"
                | "notification.toolkit.fluxcd.io"
                | "image.toolkit.fluxcd.io"
        ) {
            return Err(ResourceError::InvalidRequest);
        }
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|_| ResourceError::InvalidRequest)?
            .as_millis()
            .to_string();
        let patch = match action {
            FluxAction::ReconcileWithSource => return Err(ResourceError::InvalidRequest),
            FluxAction::Reconcile => {
                json!({"metadata":{"annotations":{"reconcile.fluxcd.io/requestedAt":now}}})
            }
            FluxAction::Suspend | FluxAction::Resume => {
                let reason = reason.unwrap_or("");
                if reason.len() > 1024 || reason.chars().any(|c| c.is_control()) {
                    return Err(ResourceError::InvalidRequest);
                }
                let suspended = matches!(action, FluxAction::Suspend);
                let since = suspended.then(|| chrono::Utc::now().to_rfc3339());
                json!({"spec":{"suspend":suspended},"metadata":{"annotations":{
                    "consolecrypt.dev/suspend-reason":reason,
                    "consolecrypt.dev/suspended-at":since
                }}})
            }
            FluxAction::ResetFailures | FluxAction::ForceUpgrade => {
                if group != "helm.toolkit.fluxcd.io" || reference.api.kind != "HelmRelease" {
                    return Err(ResourceError::InvalidRequest);
                }
                let key = if matches!(action, FluxAction::ResetFailures) {
                    "reconcile.fluxcd.io/resetAt"
                } else {
                    "reconcile.fluxcd.io/forceAt"
                };
                json!({"metadata":{"annotations":{key:now,"reconcile.fluxcd.io/requestedAt":now}}})
            }
        };
        self.merge_patch(reference, patch).await?;
        Ok(())
    }

    pub async fn argo_action(
        &self,
        reference: &ResourceRef,
        action: ArgoAction,
    ) -> Result<(), ResourceError> {
        if reference.api.group != "argoproj.io" || reference.api.kind != "Application" {
            return Err(ResourceError::InvalidRequest);
        }
        let patch = match action {
            ArgoAction::Refresh => {
                json!({"metadata":{"annotations":{"argocd.argoproj.io/refresh":"normal"}}})
            }
            ArgoAction::HardRefresh => {
                json!({"metadata":{"annotations":{"argocd.argoproj.io/refresh":"hard"}}})
            }
            ArgoAction::Sync => {
                json!({"operation":{"initiatedBy":{"username":"consolecrypt"},"sync":{"prune":false,"syncStrategy":{"apply":{}}}}})
            }
        };
        self.merge_patch(reference, patch).await?;
        Ok(())
    }
}

pub fn overview(objects: &[Value]) -> GitOpsOverview {
    let mut result = GitOpsOverview {
        ready: 0,
        not_ready: 0,
        suspended: 0,
        stalled: 0,
        reconciling: 0,
        dependencies: Vec::new(),
        cycles: Vec::new(),
    };
    let mut known = BTreeMap::new();
    for object in objects.iter().take(20_000) {
        let Some(identity) = identity(object) else {
            continue;
        };
        let conditions = object["status"]["conditions"].as_array();
        let has = |kind: &str| {
            conditions.is_some_and(|c| c.iter().any(|c| c["type"] == kind && c["status"] == "True"))
        };
        let argo = object["apiVersion"]
            .as_str()
            .is_some_and(|api| api.starts_with("argoproj.io/"));
        let ready = if argo {
            object["status"]["health"]["status"] == "Healthy"
                && object["status"]["sync"]["status"] == "Synced"
        } else {
            has("Ready")
        };
        if object["spec"]["suspend"] == true {
            result.suspended += 1;
        } else if has("Stalled") {
            result.stalled += 1;
        } else if has("Reconciling")
            || (argo && object["status"]["operationState"]["phase"] == "Running")
        {
            result.reconciling += 1;
        } else if ready {
            result.ready += 1;
        } else {
            result.not_ready += 1;
        }
        known.insert(identity, (object, ready));
    }
    let mut edges: BTreeMap<ObjectIdentity, Vec<ObjectIdentity>> = BTreeMap::new();
    for (from, (object, _)) in &known {
        for dependency in object["spec"]["dependsOn"]
            .as_array()
            .into_iter()
            .flatten()
            .take(1024)
        {
            let Some(name) = dependency["name"].as_str().filter(|v| v.len() <= 253) else {
                continue;
            };
            let to = ObjectIdentity {
                group: from.group.clone(),
                kind: from.kind.clone(),
                namespace: dependency["namespace"]
                    .as_str()
                    .unwrap_or(&from.namespace)
                    .chars()
                    .take(63)
                    .collect(),
                name: name.into(),
            };
            let ready = known.get(&to).is_some_and(|(_, ready)| *ready);
            result.dependencies.push(DependencyEdge {
                from: from.clone(),
                to: to.clone(),
                ready,
                missing: !known.contains_key(&to),
            });
            edges.entry(from.clone()).or_default().push(to);
        }
    }
    // Iterative DFS marks actual cycle members without treating healthy
    // dependencies reachable from a cycle as cyclic.
    let mut visited = BTreeSet::new();
    let mut cyclic = BTreeSet::new();
    for start in known.keys() {
        if visited.contains(start) {
            continue;
        }
        let mut stack = vec![(start.clone(), 0_usize)];
        let mut active = BTreeMap::from([(start.clone(), 0_usize)]);
        while let Some((node, index)) = stack.last_mut() {
            let targets = edges.get(node).map(Vec::as_slice).unwrap_or(&[]);
            if *index >= targets.len() {
                if let Some((node, _)) = stack.pop() {
                    active.remove(&node);
                    visited.insert(node);
                }
                continue;
            }
            let target = targets[*index].clone();
            *index += 1;
            if let Some(start) = active.get(&target) {
                cyclic.extend(stack[*start..].iter().map(|(node, _)| node.clone()));
            } else if !visited.contains(&target) && known.contains_key(&target) {
                active.insert(target.clone(), stack.len());
                stack.push((target, 0));
            }
        }
    }
    result.cycles = cyclic.into_iter().collect();
    result
}

fn identity(object: &Value) -> Option<ObjectIdentity> {
    let api = object["apiVersion"].as_str()?;
    if !api.split('/').next()?.ends_with(".toolkit.fluxcd.io")
        && !(api.starts_with("argoproj.io/") && object["kind"] == "Application")
    {
        return None;
    }
    resource_identity(object)
}

fn resource_identity(object: &Value) -> Option<ObjectIdentity> {
    let api = object["apiVersion"].as_str()?;
    let group = api.split_once('/').map_or("", |(group, _)| group);
    let kind = object["kind"].as_str()?;
    let name = object["metadata"]["name"].as_str()?;
    if name.len() > 253 || kind.len() > 128 || group.len() > 253 {
        return None;
    }
    Some(ObjectIdentity {
        group: group.into(),
        kind: kind.into(),
        namespace: object["metadata"]["namespace"]
            .as_str()
            .unwrap_or("")
            .chars()
            .take(63)
            .collect(),
        name: name.into(),
    })
}

fn object_ready(object: &Value) -> Option<bool> {
    if object["metadata"]["deletionTimestamp"].is_string() || object["spec"]["suspend"] == true {
        return Some(false);
    }
    let api = object["apiVersion"].as_str().unwrap_or("");
    let kind = object["kind"].as_str().unwrap_or("");
    if api.starts_with("argoproj.io/") && kind == "Application" {
        return object["status"].is_object().then(|| {
            object["status"]["health"]["status"] == "Healthy"
                && object["status"]["sync"]["status"] == "Synced"
        });
    }
    let conditions = object["status"]["conditions"].as_array();
    if conditions.is_some_and(|conditions| {
        conditions.iter().any(|condition| {
            matches!(
                condition["type"].as_str(),
                Some("Stalled" | "Reconciling" | "Failed")
            ) && condition["status"] == "True"
        })
    }) {
        return Some(false);
    }
    if let Some(ready) = conditions.and_then(|conditions| {
        conditions
            .iter()
            .find(|condition| matches!(condition["type"].as_str(), Some("Ready" | "Complete")))
    }) {
        return Some(ready["status"] == "True");
    }
    if api == "apps/v1" {
        if object["metadata"]["generation"]
            .as_i64()
            .is_some_and(|generation| {
                object["status"]["observedGeneration"]
                    .as_i64()
                    .unwrap_or_default()
                    < generation
            })
        {
            return Some(false);
        }
        let replicas = object["spec"]["replicas"].as_u64().unwrap_or(1);
        return match kind {
            "Deployment" => object["status"].is_object().then(|| {
                object["status"]["availableReplicas"].as_u64().unwrap_or(0) >= replicas
                    && object["status"]["updatedReplicas"].as_u64().unwrap_or(0) >= replicas
            }),
            "StatefulSet" => object["status"].is_object().then(|| {
                object["status"]["readyReplicas"].as_u64().unwrap_or(0) >= replicas
                    && (!object["status"]["updateRevision"].is_string()
                        || object["status"]["currentRevision"]
                            == object["status"]["updateRevision"])
            }),
            "DaemonSet" => object["status"]["desiredNumberScheduled"]
                .as_u64()
                .map(|desired| {
                    object["status"]["numberReady"].as_u64().unwrap_or(0) >= desired
                        && object["status"]["updatedNumberScheduled"]
                            .as_u64()
                            .unwrap_or(0)
                            >= desired
                }),
            _ => None,
        };
    }
    match (api, kind, object["status"]["phase"].as_str()) {
        ("v1", "Pod", Some("Succeeded")) => Some(true),
        ("v1", "Pod", Some("Pending" | "Failed")) => Some(false),
        ("v1", "PersistentVolumeClaim", Some(phase)) => Some(phase == "Bound"),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn flow_uses_observed_workload_and_argo_health_without_inventing_missing_status() {
        let app = json!({"apiVersion":"argoproj.io/v1alpha1","kind":"Application",
            "metadata":{"name":"app","namespace":"apps"},"status":{"health":{"status":"Healthy"},
                "sync":{"status":"Synced"},"resources":[{"group":"apps","kind":"Deployment","name":"web","namespace":"apps"}]}});
        let mut workload = json!({"apiVersion":"apps/v1","kind":"Deployment",
            "metadata":{"name":"web","namespace":"apps","generation":2},"spec":{"replicas":3},
            "status":{"observedGeneration":2,"updatedReplicas":3,"availableReplicas":3}});
        let graph = flow_graph(&[app.clone(), workload.clone()]);
        assert_eq!(
            graph
                .nodes
                .iter()
                .find(|n| n.object.name == "web")
                .unwrap()
                .ready,
            Some(true)
        );
        assert_eq!(
            graph
                .nodes
                .iter()
                .find(|n| n.object.name == "app")
                .unwrap()
                .ready,
            Some(true)
        );
        workload["status"]["observedGeneration"] = json!(1);
        assert_eq!(
            flow_graph(&[app, workload])
                .nodes
                .iter()
                .find(|n| n.object.name == "web")
                .unwrap()
                .ready,
            Some(false)
        );
    }
    #[test]
    fn flow_identifies_same_kind_and_name_by_api_group() {
        let controller = json!({"apiVersion":"kustomize.toolkit.fluxcd.io/v1","kind":"Kustomization",
            "metadata":{"name":"root","namespace":"apps"},"status":{"inventory":{"entries":[
                {"id":"apps_web_apps_Deployment"},{"id":"apps_web_custom.example_Deployment"}]}}});
        let a = json!({"apiVersion":"apps/v1","kind":"Deployment","metadata":{"name":"web","namespace":"apps"},
            "spec":{"replicas":1},"status":{"availableReplicas":1,"updatedReplicas":1}});
        let b = json!({"apiVersion":"custom.example/v1","kind":"Deployment","metadata":{"name":"web","namespace":"apps"},
            "status":{"conditions":[{"type":"Ready","status":"False"}]}});
        let graph = flow_graph(&[controller, a, b]);
        let web = graph
            .nodes
            .iter()
            .filter(|node| node.object.name == "web")
            .collect::<Vec<_>>();
        assert_eq!(web.len(), 2);
        assert_eq!(
            web.iter()
                .find(|node| node.object.group == "apps")
                .unwrap()
                .ready,
            Some(true)
        );
        assert_eq!(
            web.iter()
                .find(|node| node.object.group == "custom.example")
                .unwrap()
                .ready,
            Some(false)
        );
        assert_eq!(graph.edges.len(), 2);
    }
    #[test]
    fn flow_links_source_to_controller_to_inventory_and_keeps_unknown_status() {
        let source = json!({"apiVersion":"source.toolkit.fluxcd.io/v1","kind":"GitRepository","metadata":{"namespace":"apps","name":"source"},"status":{"conditions":[{"type":"Ready","status":"True"}]}});
        let controller = json!({"apiVersion":"kustomize.toolkit.fluxcd.io/v1","kind":"Kustomization",
            "metadata":{"namespace":"apps","name":"delivery"},
            "spec":{"sourceRef":{"kind":"GitRepository","name":"source"},"dependsOn":[{"name":"prerequisite"}]},
            "status":{"inventory":{"entries":[{"id":"apps_web_apps_Deployment","v":"v1"}]}}});
        let graph = flow_graph(&[source, controller]);
        assert!(!graph.truncated);
        assert_eq!(graph.edges.len(), 3);
        assert!(graph
            .edges
            .iter()
            .any(|edge| edge.from.kind == "GitRepository"
                && edge.to.name == "delivery"
                && edge.relation == "source"));
        assert!(graph.edges.iter().any(|edge| edge.to.kind == "Deployment"
            && edge.to.name == "web"
            && edge.relation == "inventory"));
        assert_eq!(
            graph
                .nodes
                .iter()
                .find(|node| node.object.name == "source")
                .unwrap()
                .ready,
            Some(true)
        );
        assert_eq!(
            graph
                .nodes
                .iter()
                .find(|node| node.object.name == "web")
                .unwrap()
                .ready,
            None
        );
    }
    #[test]
    fn dependencies_missing_statuses_and_cycles_are_bounded() {
        let object = |name: &str, dependency: &str, ready: bool| {
            json!({
                "apiVersion":"kustomize.toolkit.fluxcd.io/v1","kind":"Kustomization",
                "metadata":{"name":name,"namespace":"apps"},"spec":{"dependsOn":[{"name":dependency}]},
                "status":{"conditions":[{"type":"Ready","status":if ready{"True"}else{"False"}}]}
            })
        };
        let view = overview(&[
            object("a", "b", true),
            object("b", "a", false),
            object("c", "absent", false),
        ]);
        assert_eq!(view.ready, 1);
        assert_eq!(view.not_ready, 2);
        assert_eq!(view.cycles.len(), 2);
        assert_eq!(view.dependencies.iter().filter(|d| d.missing).count(), 1);
        let healthy = json!({"apiVersion":"argoproj.io/v1alpha1","kind":"Application","metadata":{"name":"web"},"status":{"health":{"status":"Healthy"},"sync":{"status":"Synced"}}});
        assert_eq!(overview(&[healthy]).ready, 1);
        let mut a = object("a", "b", false);
        a["spec"]["dependsOn"] = json!([{"name":"b"},{"name":"common"}]);
        let mut common = object("common", "unused", true);
        common["spec"]["dependsOn"] = json!([]);
        assert_eq!(
            overview(&[a, object("b", "a", false), common]).cycles.len(),
            2
        );
    }
}
