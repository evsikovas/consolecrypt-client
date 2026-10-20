//! Deterministic local explanations. Only stable codes leave this module;
//! controller messages and log fragments are not copied into diagnostics.

use serde::Serialize;
use serde_json::Value;
use std::collections::BTreeSet;

mod advanced;

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum Severity {
    Info,
    Warning,
    Error,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct Issue {
    pub code: String,
    pub severity: Severity,
    pub kind: String,
    pub namespace: Option<String>,
    pub name: String,
    /// Container/condition identifiers, never arbitrary response text.
    pub subject: Option<String>,
    pub actions: Vec<String>,
    pub exit_code: Option<i64>,
    pub count: u32,
    pub owner: Option<IssueOwner>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct IssueOwner {
    pub kind: String,
    pub namespace: Option<String>,
    pub name: String,
    pub uid: String,
}

pub fn diagnose(objects: &[Value]) -> Vec<Issue> {
    diagnose_at(objects, chrono::Utc::now().timestamp())
}

pub fn diagnose_at(objects: &[Value], now: i64) -> Vec<Issue> {
    let mut issues = Vec::new();
    for object in objects.iter().take(20_000) {
        let kind = object["kind"].as_str().unwrap_or("");
        let mut add = |code: &str, severity: Severity, subject: Option<&str>, actions: &[&str]| {
            issues.push(Issue {
                code: code.into(),
                severity,
                kind: kind.chars().take(128).collect(),
                namespace: object["metadata"]["namespace"]
                    .as_str()
                    .map(|v| v.chars().take(63).collect()),
                name: object["metadata"]["name"]
                    .as_str()
                    .unwrap_or("")
                    .chars()
                    .take(253)
                    .collect(),
                subject: subject.map(|value| {
                    value
                        .chars()
                        .filter(|c| !c.is_control())
                        .take(128)
                        .collect()
                }),
                actions: actions.iter().map(|action| (*action).to_owned()).collect(),
                exit_code: advanced::exit_code(object, subject),
                count: 1,
                owner: None,
            });
        };
        match kind {
            "Pod" => {
                if object["status"]["reason"] == "Evicted" {
                    add(
                        "pod_evicted",
                        Severity::Error,
                        None,
                        &["events", "nodes", "yaml"],
                    );
                }
                for container in ["initContainerStatuses", "containerStatuses"]
                    .into_iter()
                    .filter_map(|key| object["status"][key].as_array())
                    .flatten()
                {
                    let name = container["name"].as_str();
                    match container["state"]["waiting"]["reason"].as_str() {
                        Some("CrashLoopBackOff") => add(
                            "pod_crash_loop",
                            Severity::Error,
                            name,
                            &["previous_logs", "events", "yaml"],
                        ),
                        Some("ImagePullBackOff" | "ErrImagePull" | "InvalidImageName") => {
                            add("pod_image_pull", Severity::Error, name, &["events", "yaml"])
                        }
                        Some("CreateContainerConfigError" | "CreateContainerError") => add(
                            "pod_container_config",
                            Severity::Error,
                            name,
                            &["events", "yaml"],
                        ),
                        _ => {}
                    }
                    if container["lastState"]["terminated"]["reason"] == "OOMKilled"
                        || container["state"]["terminated"]["reason"] == "OOMKilled"
                    {
                        add(
                            "pod_oom_killed",
                            Severity::Error,
                            name,
                            &["previous_logs", "metrics", "yaml"],
                        );
                    }
                    if container["restartCount"].as_u64().unwrap_or(0) >= 5 {
                        add(
                            "pod_restarts",
                            Severity::Warning,
                            name,
                            &["previous_logs", "events"],
                        );
                    }
                }
                if let Some(conditions) = object["status"]["conditions"].as_array() {
                    for condition in conditions {
                        if condition["type"] == "PodScheduled" && condition["status"] == "False" {
                            add(
                                "pod_unschedulable",
                                Severity::Error,
                                None,
                                &["events", "nodes", "yaml"],
                            );
                        }
                        if condition["type"] == "Ready"
                            && condition["status"] == "False"
                            && object["status"]["phase"] == "Running"
                        {
                            add(
                                "pod_not_ready",
                                Severity::Warning,
                                None,
                                &["logs", "events"],
                            );
                        }
                    }
                }
            }
            "Node" => {
                for condition in object["status"]["conditions"]
                    .as_array()
                    .into_iter()
                    .flatten()
                {
                    match (condition["type"].as_str(), condition["status"].as_str()) {
                        (Some("Ready"), Some("False" | "Unknown")) => add(
                            "node_not_ready",
                            Severity::Error,
                            None,
                            &["events", "metrics"],
                        ),
                        (Some("MemoryPressure" | "DiskPressure" | "PIDPressure"), Some("True")) => {
                            add(
                                "node_pressure",
                                Severity::Error,
                                condition["type"].as_str(),
                                &["events", "metrics"],
                            );
                        }
                        _ => {}
                    }
                }
            }
            "Deployment" | "StatefulSet" | "DaemonSet" | "ReplicaSet" => {
                for condition in object["status"]["conditions"]
                    .as_array()
                    .into_iter()
                    .flatten()
                {
                    if condition["reason"] == "ProgressDeadlineExceeded" {
                        add(
                            "rollout_deadline",
                            Severity::Error,
                            None,
                            &["pods", "events", "yaml"],
                        );
                    }
                    if condition["type"] == "ReplicaFailure" && condition["status"] == "True" {
                        add(
                            "workload_replica_failure",
                            Severity::Error,
                            None,
                            &["pods", "events"],
                        );
                    }
                }
                let desired = object["spec"]["replicas"]
                    .as_u64()
                    .or_else(|| object["status"]["desiredNumberScheduled"].as_u64())
                    .unwrap_or(1);
                let ready = object["status"]["readyReplicas"]
                    .as_u64()
                    .or_else(|| object["status"]["numberReady"].as_u64())
                    .unwrap_or(0);
                if ready < desired {
                    add(
                        "workload_replicas_unavailable",
                        Severity::Warning,
                        None,
                        &["pods", "events"],
                    );
                }
            }
            "PersistentVolumeClaim" => {
                if object["status"]["phase"] == "Pending" {
                    add(
                        "volume_claim_pending",
                        Severity::Warning,
                        None,
                        &["events", "storage_classes", "yaml"],
                    );
                }
                if object["status"]["phase"] == "Lost" {
                    add(
                        "volume_claim_lost",
                        Severity::Error,
                        None,
                        &["events", "volumes"],
                    );
                }
            }
            "Job" => {
                if object["status"]["conditions"]
                    .as_array()
                    .is_some_and(|conditions| {
                        conditions
                            .iter()
                            .any(|c| c["type"] == "Failed" && c["status"] == "True")
                    })
                {
                    add(
                        "job_failed",
                        Severity::Error,
                        None,
                        &["pods", "logs", "events"],
                    );
                }
            }
            "Application"
                if object["apiVersion"]
                    .as_str()
                    .is_some_and(|v| v.starts_with("argoproj.io/")) =>
            {
                if object["status"]["health"]["status"] == "Degraded" {
                    add(
                        "argo_degraded",
                        Severity::Error,
                        None,
                        &["resources", "events"],
                    );
                }
                if object["status"]["sync"]["status"] == "OutOfSync" {
                    add(
                        "argo_out_of_sync",
                        Severity::Warning,
                        None,
                        &["diff", "resources"],
                    );
                }
            }
            _ => {}
        }
        if object["apiVersion"].as_str().is_some_and(|v| {
            v.split('/')
                .next()
                .is_some_and(|g| g.ends_with(".toolkit.fluxcd.io"))
        }) {
            if object["spec"]["suspend"] == true {
                add("flux_suspended", Severity::Info, None, &["yaml"]);
            }
            for condition in object["status"]["conditions"]
                .as_array()
                .into_iter()
                .flatten()
            {
                if condition["type"] == "Stalled" && condition["status"] == "True" {
                    add(
                        "flux_stalled",
                        Severity::Error,
                        None,
                        &["controller_logs", "events", "dependencies"],
                    );
                }
                if condition["type"] == "Ready" && condition["status"] == "False" {
                    add(
                        "flux_not_ready",
                        Severity::Warning,
                        None,
                        &["controller_logs", "events", "dependencies"],
                    );
                }
            }
            let current = object["metadata"]["generation"].as_u64();
            let observed = object["status"]["observedGeneration"].as_u64();
            if current.zip(observed).is_some_and(|(a, b)| a > b) {
                add("flux_generation_pending", Severity::Info, None, &["events"]);
            }
        }
    }
    issues.extend(advanced::diagnose(objects, now));
    let mut seen = BTreeSet::new();
    issues.retain(|issue| {
        seen.insert((
            issue.kind.clone(),
            issue.namespace.clone(),
            issue.name.clone(),
            issue.code.clone(),
            issue.subject.clone(),
        ))
    });
    issues.sort_by(|a, b| {
        b.severity
            .cmp(&a.severity)
            .then_with(|| a.namespace.cmp(&b.namespace))
            .then_with(|| a.name.cmp(&b.name))
            .then_with(|| a.code.cmp(&b.code))
    });
    issues.truncate(10_000);
    advanced::group_by_owner(issues, objects)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn golden_pod_flux_node_and_volume_problems_do_not_copy_messages() {
        let issues = diagnose(&[
            json!({"apiVersion":"v1","kind":"Pod","metadata":{"name":"app","namespace":"shop"},"status":{"phase":"Running","containerStatuses":[{"name":"web","restartCount":9,"state":{"waiting":{"reason":"CrashLoopBackOff","message":"secret=fixture-password"}},"lastState":{"terminated":{"reason":"OOMKilled"}}}]}}),
            json!({"apiVersion":"kustomize.toolkit.fluxcd.io/v1","kind":"Kustomization","metadata":{"name":"shop","generation":3},"status":{"observedGeneration":2,"conditions":[{"type":"Stalled","status":"True","message":"token=fixture-token"}]}}),
            json!({"kind":"Node","metadata":{"name":"worker"},"status":{"conditions":[{"type":"Ready","status":"Unknown"},{"type":"MemoryPressure","status":"True"}]}}),
            json!({"kind":"PersistentVolumeClaim","metadata":{"name":"data"},"status":{"phase":"Pending"}}),
        ]);
        let codes: BTreeSet<_> = issues.iter().map(|i| i.code.as_str()).collect();
        assert_eq!(
            codes,
            BTreeSet::from([
                "pod_crash_loop",
                "pod_oom_killed",
                "pod_restarts",
                "flux_stalled",
                "flux_generation_pending",
                "node_not_ready",
                "node_pressure",
                "volume_claim_pending"
            ])
        );
        let output = serde_json::to_string(&issues).unwrap();
        assert!(!output.contains("fixture-password"));
        assert!(!output.contains("fixture-token"));
        assert_eq!(issues[0].severity, Severity::Error);
    }
}
