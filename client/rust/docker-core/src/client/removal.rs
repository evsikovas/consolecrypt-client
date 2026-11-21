//! Preview and remove resources attached to one exact container. No pruning,
//! force deletion, anonymous-volume cascade, host paths or image operations.
use super::{checked, DockerClient};
use crate::{ContainerAction, DockerError};
use hyper::Method;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use tokio_util::sync::CancellationToken;

#[cfg(test)]
mod tests;

const MAX_RESOURCES: usize = 256;
const PROJECT: &str = "com.docker.compose.project";

#[derive(Debug, Clone, Serialize)]
pub struct DockerRemovalResource {
    pub kind: &'static str,
    pub id: String,
    pub name: String,
    pub eligible: bool,
    pub reason: &'static str,
    #[serde(skip)]
    fingerprint: Option<[u8; 32]>,
}
#[derive(Debug, Clone, Serialize)]
pub struct DockerRemovalPlan {
    pub plan_id: String,
    pub container_id: String,
    pub name: String,
    pub running: bool,
    pub resources: Vec<DockerRemovalResource>,
    #[serde(skip)]
    project: Option<String>,
    #[serde(skip)]
    managed: bool,
    #[serde(skip)]
    auto_remove: Option<bool>,
}
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DockerRemovalSelection {
    pub plan_id: String,
    pub volumes: Vec<String>,
    pub networks: Vec<String>,
    pub stop_running: bool,
    pub confirm_name: String,
}
#[derive(Debug, Serialize)]
pub struct DockerRemovedResource {
    pub kind: &'static str,
    pub id: String,
    pub name: String,
    pub status: &'static str,
    pub reason: &'static str,
}
#[derive(Debug, Default, Serialize)]
pub struct DockerRemovalResult {
    pub container_removed: bool,
    pub container_stopped: bool,
    pub resources: Vec<DockerRemovedResource>,
    pub error: Option<&'static str>,
}

fn full_id(id: &str) -> bool {
    id.len() == 64
        && id
            .bytes()
            .all(|v| v.is_ascii_hexdigit() && !v.is_ascii_uppercase())
}

fn reference(
    kind: &'static str,
    id: &str,
    name: &str,
    reason: &'static str,
) -> DockerRemovalResource {
    DockerRemovalResource {
        kind,
        id: id.into(),
        name: name.into(),
        eligible: false,
        reason,
        fingerprint: None,
    }
}

fn code(error: DockerError) -> &'static str {
    match error {
        DockerError::Api { status: 409, .. } => "in_use",
        DockerError::Api { status: 403, .. } => "permission_denied",
        other => other.code(),
    }
}

fn external_label(labels: &Value) -> bool {
    labels.as_object().is_some_and(|labels| {
        labels.keys().any(|key| {
            key.starts_with("com.docker.stack.")
                || key.starts_with("com.docker.swarm.")
                || key == "com.docker.compose.external"
                || key == "com.docker.network.generic"
                || key.starts_with("io.kubernetes.")
                || key.starts_with("io.podman.compose.")
        })
    })
}

fn default_network(name: &str) -> bool {
    matches!(
        name,
        "bridge" | "host" | "none" | "nat" | "null" | "ingress" | "docker_gwbridge"
    )
}

fn fingerprint(value: &Value) -> [u8; 32] {
    Sha256::digest(value.to_string().as_bytes()).into()
}

// Docker's Volume API has no immutable volume ID. Bind the plan to the observed
// creation timestamp and driver metadata, then inspect again immediately before
// issuing the non-force DELETE. A 409 always preserves the resource.
fn classify(resource: &mut DockerRemovalResource, obj: &Value, project: Option<&str>) {
    let name = obj["Name"].as_str();
    if name != Some(&resource.name) {
        return;
    }
    let Some(labels) = obj.get("Labels") else {
        return;
    };
    if !labels.is_object() && !labels.is_null() {
        return;
    }
    if labels.get(PROJECT).is_some() && !labels[PROJECT].as_str().is_some_and(|s| !s.is_empty()) {
        return;
    }
    let owner = labels[PROJECT].as_str();
    let local = obj["Scope"] == "local";
    resource.reason = if resource.kind == "network"
        && (default_network(&resource.name)
            || obj["Ingress"] == true
            || obj["Options"]["com.docker.network.bridge.default_bridge"] == "true")
    {
        "default"
    } else if !local || external_label(labels) || owner.is_some() && owner != project {
        "external"
    } else if resource.kind == "volume" {
        if checked(&resource.id).is_err() || obj["CreatedAt"].as_str().is_none_or(str::is_empty) {
            return;
        }
        let Some(options) = obj.get("Options") else {
            return;
        };
        if obj["Driver"] != "local"
            || !obj["ClusterVolume"].is_null()
            || !(options.is_null() || options.as_object().is_some_and(|m| m.is_empty()))
        {
            "external"
        } else if project.is_some()
            && !(owner == project && labels["com.docker.compose.volume"].is_string()
                || full_id(&resource.name) && labels.get("com.docker.volume.anonymous").is_some())
        {
            "external"
        } else {
            resource.fingerprint = Some(fingerprint(&json!([
                name,
                obj["CreatedAt"],
                obj["Driver"],
                obj["Scope"],
                labels,
                options
            ])));
            "eligible"
        }
    } else {
        if !full_id(&resource.id) || obj["Id"] != resource.id {
            return;
        }
        if obj["Driver"] != "bridge"
            || obj["ConfigOnly"] == true
            || obj["ConfigFrom"]["Network"]
                .as_str()
                .is_some_and(|n| !n.is_empty())
            || project.is_some()
                && !(owner == project && labels["com.docker.compose.network"].is_string())
        {
            "external"
        } else {
            resource.fingerprint = Some(fingerprint(&json!([
                obj["Id"],
                name,
                obj["Driver"],
                obj["Scope"],
                labels
            ])));
            "eligible"
        }
    };
    resource.eligible = resource.reason == "eligible";
}

// Stopped containers can have no active network endpoint in network inspect.
// Include all container summaries, matching by both network name and ID.
fn usage(rows: &Value, target: &str, resource: &DockerRemovalResource) -> &'static str {
    let Some(rows) = rows.as_array() else {
        return "unknown";
    };
    for row in rows {
        let Some(id) = row["Id"].as_str().filter(|id| full_id(id)) else {
            return "unknown";
        };
        if id == target {
            continue;
        }
        if resource.kind == "volume" {
            if row.get("Mounts") == Some(&Value::Null) {
                continue;
            }
            let Some(mounts) = row["Mounts"].as_array() else {
                return "unknown";
            };
            for mount in mounts {
                let Some(kind) = mount["Type"].as_str() else {
                    return "unknown";
                };
                if kind == "volume" {
                    let Some(name) = mount["Name"].as_str() else {
                        return "unknown";
                    };
                    if name == resource.name {
                        return "shared";
                    }
                }
            }
        } else {
            let Some(networks) = row["NetworkSettings"]["Networks"].as_object() else {
                return "unknown";
            };
            if networks.iter().any(|(name, network)| {
                name == &resource.name || network["NetworkID"] == resource.id
            }) {
                return "shared";
            }
            if row["HostConfig"]["NetworkMode"]
                .as_str()
                .is_some_and(|v| v == resource.id || v == resource.name)
            {
                return "shared";
            }
        }
    }
    "eligible"
}

fn apply_usage(resource: &mut DockerRemovalResource, rows: &Value, target: &str) {
    if !resource.eligible {
        return;
    }
    resource.reason = usage(rows, target, resource);
    resource.eligible = resource.reason == "eligible";
}

fn chosen<'a>(
    plan: &'a DockerRemovalPlan,
    selection: &DockerRemovalSelection,
) -> Result<Vec<&'a DockerRemovalResource>, DockerError> {
    if selection.confirm_name != plan.name
        || selection.plan_id != plan.plan_id
        || selection.volumes.len() + selection.networks.len() > MAX_RESOURCES
    {
        return Err(DockerError::InvalidRequest);
    }
    let mut seen = BTreeSet::new();
    let mut result = Vec::new();
    for (kind, ids) in [
        ("volume", &selection.volumes),
        ("network", &selection.networks),
    ] {
        for id in ids {
            let resource = plan
                .resources
                .iter()
                .find(|r| r.eligible && r.kind == kind && &r.id == id)
                .ok_or(DockerError::InvalidRequest)?;
            if !seen.insert((kind, id)) {
                return Err(DockerError::InvalidRequest);
            }
            result.push(resource);
        }
    }
    Ok(result)
}

impl DockerClient {
    async fn removal_candidates(
        &self,
        id: &str,
    ) -> Result<(DockerRemovalPlan, Vec<DockerRemovalResource>), DockerError> {
        if !full_id(id) {
            return Err(DockerError::InvalidRequest);
        }
        let info = self.inspect_unmasked(id).await?;
        let name = info["Name"]
            .as_str()
            .ok_or(DockerError::Protocol)?
            .trim_start_matches('/');
        if info["Id"] != id || checked(name).is_err() {
            return Err(DockerError::Protocol);
        }
        let running = info["State"]["Running"]
            .as_bool()
            .ok_or(DockerError::Protocol)?;
        let mut resources = BTreeMap::new();
        for m in info["Mounts"].as_array().into_iter().flatten() {
            let r = match (m["Type"].as_str(), m["Destination"].as_str()) {
                (Some("volume"), _) => m["Name"]
                    .as_str()
                    .filter(|n| checked(n).is_ok())
                    .map(|n| reference("volume", n, n, "unknown")),
                (Some("bind"), Some(path)) => Some(reference("bind", path, path, "bind")),
                _ => None,
            };
            if let Some(r) = r {
                resources.insert((r.kind, r.id.clone()), r);
            }
        }
        for (name, net) in info["NetworkSettings"]["Networks"]
            .as_object()
            .into_iter()
            .flatten()
        {
            let id = net["NetworkID"]
                .as_str()
                .filter(|id| full_id(id))
                .unwrap_or("");
            let reason = if default_network(name) {
                "default"
            } else {
                "unknown"
            };
            resources.insert(
                ("network", name.clone()),
                reference("network", id, name, reason),
            );
        }
        if resources.len() > MAX_RESOURCES {
            return Err(DockerError::InvalidRequest);
        }
        let labels = &info["Config"]["Labels"];
        let managed = info["Config"].get("Labels").is_none()
            || !(labels.is_null() || labels.is_object())
            || external_label(labels)
            || labels.get(PROJECT).is_some()
                && !labels[PROJECT].as_str().is_some_and(|s| !s.is_empty());
        let plan = DockerRemovalPlan {
            plan_id: String::new(),
            container_id: id.into(),
            name: name.into(),
            running,
            resources: vec![],
            managed,
            project: labels[PROJECT].as_str().map(str::to_owned),
            auto_remove: info["HostConfig"]["AutoRemove"].as_bool(),
        };
        Ok((plan, resources.into_values().collect()))
    }

    pub async fn removal_plan(&self, id: &str) -> Result<DockerRemovalPlan, DockerError> {
        let (mut plan, mut resources) = self.removal_candidates(id).await?;
        let (users, volumes, networks) = tokio::join!(
            self.json(Method::GET, "/containers/json?all=1", None),
            self.json(Method::GET, "/volumes", None),
            self.json(Method::GET, "/networks", None)
        );
        let users = users
            .ok()
            .filter(|rows| {
                rows.as_array()
                    .is_some_and(|entries| entries.iter().any(|row| row["Id"] == id))
            })
            .unwrap_or(Value::Null);
        let volumes = volumes.unwrap_or(Value::Null);
        let networks = networks.unwrap_or(Value::Null);
        for r in &mut resources {
            if r.kind == "bind" || r.reason == "default" {
                continue;
            }
            if plan.managed {
                r.reason = "external";
                continue;
            }
            let obj = if r.kind == "volume" {
                volumes["Volumes"]
                    .as_array()
                    .and_then(|vs| vs.iter().find(|v| v["Name"] == r.name))
            } else {
                networks.as_array().and_then(|ns| {
                    if !r.id.is_empty() {
                        return ns.iter().find(|v| v["Id"] == r.id);
                    }
                    // Docker can omit the endpoint ID while the container is stopped.
                    let mut by_name = ns.iter().filter(|v| v["Name"] == r.name);
                    let network = by_name.next()?;
                    if by_name.next().is_some() {
                        return None;
                    }
                    r.id = network["Id"].as_str().filter(|id| full_id(id))?.to_owned();
                    Some(network)
                })
            };
            if let Some(obj) = obj {
                classify(r, obj, plan.project.as_deref());
                apply_usage(r, &users, id);
            }
        }
        plan.resources = resources;
        Ok(plan)
    }

    async fn cleanup_resource(
        &self,
        old: &DockerRemovalResource,
        project: Option<&str>,
    ) -> (&'static str, &'static str) {
        let path = format!("/{}s/{}", old.kind, old.id);
        let users = match self.json(Method::GET, "/containers/json?all=1", None).await {
            Ok(users) => users,
            Err(err) => return ("skipped", code(err)),
        };
        let data = match self.json(Method::GET, &path, None).await {
            Ok(data) => data,
            Err(err) => return ("skipped", code(err)),
        };
        let mut now = reference(old.kind, &old.id, &old.name, "unknown");
        classify(&mut now, &data, project);
        if !now.eligible {
            return ("skipped", now.reason);
        }
        if old.fingerprint != now.fingerprint {
            return ("skipped", "changed");
        }
        if now.kind == "network" && !data["Containers"].as_object().is_some_and(|c| c.is_empty()) {
            return (
                "skipped",
                if data["Containers"].is_object() {
                    "shared"
                } else {
                    "unknown"
                },
            );
        }
        apply_usage(&mut now, &users, "");
        if !now.eligible {
            return ("skipped", now.reason);
        }
        match self.json(Method::DELETE, &path, None).await {
            Ok(_) => ("removed", "removed"),
            Err(DockerError::Api { status: 409, .. }) => ("skipped", "in_use"),
            Err(err) => ("failed", code(err)),
        }
    }

    pub async fn remove_with_cleanup(
        &self,
        plan: &DockerRemovalPlan,
        selection: &DockerRemovalSelection,
        cancel: &CancellationToken,
    ) -> Result<DockerRemovalResult, DockerError> {
        let requested = chosen(plan, selection)?;
        let mut result = DockerRemovalResult::default();
        let fresh = tokio::select! {
            biased;
            _ = cancel.cancelled() => return Err(DockerError::Cancelled),
            plan = self.removal_plan(&plan.container_id) => plan?,
        };
        if fresh.name != plan.name || fresh.project != plan.project || fresh.managed != plan.managed
        {
            result.error = Some("changed");
            return Ok(result);
        }
        if fresh.running {
            if !plan.running || !selection.stop_running {
                result.error = Some("running");
                return Ok(result);
            }
            if fresh.auto_remove != Some(false) {
                result.error = Some("auto_remove");
                return Ok(result);
            }
            let stop = tokio::select! {
                biased;
                _ = cancel.cancelled() => Err(DockerError::Cancelled),
                stop = self.action(&plan.container_id, ContainerAction::Stop) => stop,
            };
            if let Err(error) = stop {
                result.error = Some(code(error));
                return Ok(result);
            }
            result.container_stopped = true;
        }
        let removed = tokio::select! {
            biased;
            _ = cancel.cancelled() => Err(DockerError::Cancelled),
            remove = self.action(&plan.container_id, ContainerAction::Remove) => remove,
        };
        if let Err(error) = removed {
            result.error = Some(code(error));
            return Ok(result);
        }
        result.container_removed = true;
        for r in requested {
            let observed = fresh
                .resources
                .iter()
                .find(|v| v.kind == r.kind && v.id == r.id);
            let (status, reason) = match observed {
                None => ("skipped", "changed"),
                Some(now) if !now.eligible => ("skipped", now.reason),
                Some(now) if now.fingerprint != r.fingerprint => ("skipped", "changed"),
                Some(_) => {
                    tokio::select! {
                        biased;
                        _ = cancel.cancelled() => ("skipped", "cancelled"),
                        cleanup = self.cleanup_resource(r, plan.project.as_deref()) => cleanup,
                    }
                }
            };
            result.resources.push(DockerRemovedResource {
                kind: r.kind,
                id: r.id.clone(),
                name: r.name.clone(),
                status,
                reason,
            });
        }
        Ok(result)
    }
}
