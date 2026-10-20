use super::*;
use std::collections::{BTreeMap, BTreeSet};

pub(super) fn exit_code(object: &Value, container: Option<&str>) -> Option<i64> {
    if object["kind"] != "Pod" {
        return None;
    }
    let name = container?;
    let status = ["containerStatuses", "initContainerStatuses"]
        .iter()
        .filter_map(|field| object["status"][field].as_array())
        .flatten()
        .find(|status| status["name"] == name)?;
    status["lastState"]["terminated"]["exitCode"]
        .as_i64()
        .or_else(|| status["state"]["terminated"]["exitCode"].as_i64())
}

fn group(object: &Value) -> &str {
    object["apiVersion"]
        .as_str()
        .unwrap_or("")
        .split_once('/')
        .map_or("", |(group, _)| group)
}
fn key(object: &Value) -> (String, String, String, String) {
    (
        group(object).into(),
        object["kind"].as_str().unwrap_or("").into(),
        object["metadata"]["namespace"]
            .as_str()
            .unwrap_or("")
            .into(),
        object["metadata"]["name"].as_str().unwrap_or("").into(),
    )
}
fn timestamp(value: &Value) -> Option<i64> {
    chrono::DateTime::parse_from_rfc3339(value.as_str()?)
        .ok()
        .map(|time| time.timestamp())
}
fn ready(object: &Value) -> bool {
    object["spec"]["suspend"] != true
        && object["status"]["conditions"]
            .as_array()
            .is_some_and(|conditions| {
                conditions
                    .iter()
                    .any(|condition| condition["type"] == "Ready" && condition["status"] == "True")
            })
}
fn issue(object: &Value, code: &str, severity: Severity, subject: Option<String>) -> Issue {
    Issue {
        code: code.into(),
        severity,
        kind: object["kind"]
            .as_str()
            .unwrap_or("")
            .chars()
            .take(128)
            .collect(),
        namespace: object["metadata"]["namespace"]
            .as_str()
            .map(|value| value.chars().take(63).collect()),
        name: object["metadata"]["name"]
            .as_str()
            .unwrap_or("")
            .chars()
            .take(253)
            .collect(),
        subject,
        actions: vec!["events".into(), "yaml".into(), "dependencies".into()],
        exit_code: None,
        count: 1,
        owner: None,
    }
}

pub(super) fn diagnose(objects: &[Value], now: i64) -> Vec<Issue> {
    let objects = &objects[..objects.len().min(20_000)];
    let index: BTreeMap<_, _> = objects.iter().map(|object| (key(object), object)).collect();
    let mut issues = Vec::new();
    for object in objects {
        if object["kind"] == "Pod" && object["status"]["phase"] == "Pending" {
            for condition in object["status"]["conditions"]
                .as_array()
                .into_iter()
                .flatten()
            {
                if condition["type"] != "PodScheduled" || condition["status"] != "False" {
                    continue;
                }
                // Classify known scheduler phrases; never copy their text,
                // node names, resource values or embedded credentials.
                let text = condition["message"]
                    .as_str()
                    .unwrap_or("")
                    .to_ascii_lowercase();
                for (matches, code) in [
                    (
                        text.contains("insufficient cpu")
                            || text.contains("insufficient memory")
                            || text.contains("insufficient ephemeral-storage"),
                        "pod_pending_resources",
                    ),
                    (
                        text.contains("untolerated taint") || text.contains("had taint"),
                        "pod_pending_taints",
                    ),
                    (
                        text.contains("unbound immediate persistentvolumeclaims")
                            || text.contains("volume node affinity conflict")
                            || text.contains("persistentvolumeclaim") && text.contains("not found"),
                        "pod_pending_volume",
                    ),
                ] {
                    if matches {
                        issues.push(issue(object, code, Severity::Error, None));
                    }
                }
            }
        }
        if !group(object).ends_with(".toolkit.fluxcd.io") {
            continue;
        }
        let namespace = object["metadata"]["namespace"].as_str().unwrap_or("");
        let kind = object["kind"].as_str().unwrap_or("");
        if object["spec"]["suspend"] == true {
            // Flux has no standard suspension timestamp. Use the timestamp of
            // this explicit action, or an actual Suspended condition only.
            let since =
                timestamp(&object["metadata"]["annotations"]["consolecrypt.dev/suspended-at"])
                    .or_else(|| {
                        object["status"]["conditions"]
                            .as_array()?
                            .iter()
                            .find(|condition| {
                                condition["type"] == "Suspended" && condition["status"] == "True"
                            })
                            .and_then(|condition| timestamp(&condition["lastTransitionTime"]))
                    });
            if since.is_some_and(|since| now.saturating_sub(since) > 86400) {
                issues.push(issue(
                    object,
                    "flux_suspended_long",
                    Severity::Warning,
                    None,
                ));
            }
        }
        if matches!(kind, "Kustomization" | "HelmRelease") {
            for dependency in object["spec"]["dependsOn"]
                .as_array()
                .into_iter()
                .flatten()
                .take(1000)
            {
                let Some(name) = dependency["name"].as_str() else {
                    continue;
                };
                let ns = dependency["namespace"].as_str().unwrap_or(namespace);
                let dependency_key = (group(object).into(), kind.into(), ns.into(), name.into());
                let dependency = index.get(&dependency_key);
                let code = match dependency {
                    None => Some("flux_dependency_missing"),
                    Some(dependency) if !ready(dependency) => Some("flux_dependency_not_ready"),
                    _ => None,
                };
                if let Some(code) = code {
                    issues.push(issue(
                        object,
                        code,
                        Severity::Warning,
                        Some(format!("{ns}/{name}").chars().take(128).collect()),
                    ));
                }
            }
        }
        if kind == "HelmRelease"
            && object["status"]["conditions"]
                .as_array()
                .is_some_and(|conditions| {
                    conditions.iter().any(|condition| {
                        condition["type"] == "Drifted" && condition["status"] == "True"
                            || condition["reason"] == "DriftDetected"
                                && condition["status"] == "True"
                    })
                })
        {
            issues.push(issue(object, "flux_helm_drift", Severity::Warning, None));
        }
        // Source artifact revisions and lastAppliedRevision have compatible
        // semantics for Kustomization. Helm chart versions and Git commits do
        // not; comparing those would create spurious stale-source warnings.
        if kind == "Kustomization" && object["spec"]["suspend"] != true {
            let source = &object["spec"]["sourceRef"];
            let source_key = (
                "source.toolkit.fluxcd.io".into(),
                source["kind"].as_str().unwrap_or("").into(),
                source["namespace"].as_str().unwrap_or(namespace).into(),
                source["name"].as_str().unwrap_or("").into(),
            );
            if let Some(source) = index.get(&source_key) {
                let revision = source["status"]["artifact"]["revision"]
                    .as_str()
                    .unwrap_or("");
                let applied = object["status"]["lastAppliedRevision"]
                    .as_str()
                    .unwrap_or("");
                let changed = timestamp(&source["status"]["artifact"]["lastUpdateTime"]);
                let created = timestamp(&object["metadata"]["creationTimestamp"]);
                let since = changed.map(|changed| changed.max(created.unwrap_or(changed)));
                let interval = duration_seconds(object["spec"]["interval"].as_str().unwrap_or(""));
                if !revision.is_empty()
                    && revision != applied
                    && since.zip(interval).is_some_and(|(since, interval)| {
                        now.saturating_sub(since) > interval.saturating_mul(2)
                    })
                {
                    issues.push(issue(object, "flux_source_stale", Severity::Warning, None));
                }
            }
        }
        if kind == "HelmRelease" && object["spec"]["suspend"] != true {
            if let Some(source_key) = helm_source_key(object, namespace) {
                if let Some(source) = index.get(&source_key) {
                    if helm_source_changed(object, source)
                        && source_older_than_two_intervals(object, source, now)
                    {
                        issues.push(issue(object, "flux_source_stale", Severity::Warning, None));
                    }
                }
            }
        }
    }
    issues
}

fn helm_source_key(object: &Value, namespace: &str) -> Option<(String, String, String, String)> {
    let reference = &object["spec"]["chartRef"];
    let (kind, namespace, name) = if let Some(kind) = reference["kind"].as_str() {
        (
            kind,
            reference["namespace"].as_str().unwrap_or(namespace),
            reference["name"].as_str()?,
        )
    } else {
        let (namespace, name) = object["status"]["helmChart"].as_str()?.split_once('/')?;
        ("HelmChart", namespace, name)
    };
    if !matches!(kind, "HelmChart" | "OCIRepository") || namespace.is_empty() || name.is_empty() {
        return None;
    }
    Some((
        "source.toolkit.fluxcd.io".into(),
        kind.into(),
        namespace.into(),
        name.into(),
    ))
}

fn helm_source_changed(object: &Value, source: &Value) -> bool {
    // Compare the actually deployed chart, not lastAttemptedRevision (which
    // can describe a failed upgrade). OCI artifact.digest is the local archive
    // digest; the remote OCI digest is in artifact.revision after '@'.
    let deployed = object["status"]["history"].as_array().and_then(|entries| {
        entries
            .iter()
            .take(100)
            .find(|entry| entry["status"] == "deployed")
    });
    let source_revision = source["status"]["artifact"]["revision"]
        .as_str()
        .unwrap_or("");
    let (current, applied) = match source["kind"].as_str() {
        Some("HelmChart") => (
            Some(source_revision),
            deployed
                .and_then(|entry| entry["chartVersion"].as_str())
                .or_else(|| object["status"]["lastAppliedRevision"].as_str()),
        ),
        Some("OCIRepository") => (
            source_revision.rsplit_once('@').map(|(_, digest)| digest),
            deployed.and_then(|entry| entry["ociDigest"].as_str()),
        ),
        _ => return false,
    };
    current.zip(applied).is_some_and(|(current, applied)| {
        !current.is_empty() && !applied.is_empty() && current != applied
    })
}

fn source_older_than_two_intervals(object: &Value, source: &Value, now: i64) -> bool {
    let changed = timestamp(&source["status"]["artifact"]["lastUpdateTime"]);
    let created = timestamp(&object["metadata"]["creationTimestamp"]);
    let since = changed.map(|changed| changed.max(created.unwrap_or(changed)));
    let interval = duration_seconds(object["spec"]["interval"].as_str().unwrap_or(""));
    since
        .zip(interval)
        .is_some_and(|(since, interval)| now.saturating_sub(since) > interval.saturating_mul(2))
}

fn duration_seconds(input: &str) -> Option<i64> {
    if input.is_empty() || input.len() > 64 {
        return None;
    }
    let mut tail = input;
    let mut sum = 0_f64;
    while !tail.is_empty() {
        let length = tail
            .bytes()
            .take_while(|byte| byte.is_ascii_digit() || *byte == b'.')
            .count();
        if length == 0 {
            return None;
        }
        let amount = tail[..length].parse::<f64>().ok()?;
        tail = &tail[length..];
        let (unit, scale) = [("ms", 0.001), ("h", 3600.0), ("m", 60.0), ("s", 1.0)]
            .into_iter()
            .find(|(unit, _)| tail.starts_with(unit))?;
        tail = &tail[unit.len()..];
        sum += amount * scale;
    }
    (sum.is_finite() && sum > 0.0 && sum <= 604800.0).then_some(sum.ceil() as i64)
}

pub(super) fn group_by_owner(issues: Vec<Issue>, objects: &[Value]) -> Vec<Issue> {
    let by_uid: BTreeMap<_, _> = objects
        .iter()
        .take(20_000)
        .filter_map(|object| Some((object["metadata"]["uid"].as_str()?, object)))
        .collect();
    let pods: BTreeMap<_, _> = objects
        .iter()
        .take(20_000)
        .filter(|object| object["kind"] == "Pod")
        .map(|object| {
            (
                (
                    object["metadata"]["namespace"].as_str().unwrap_or(""),
                    object["metadata"]["name"].as_str().unwrap_or(""),
                ),
                object,
            )
        })
        .collect();
    let mut grouped: BTreeMap<(String, String, Option<String>), usize> = BTreeMap::new();
    let mut result: Vec<Issue> = Vec::new();
    for mut issue in issues {
        if issue.kind == "Pod" {
            if let Some(pod) = pods.get(&(
                issue.namespace.as_deref().unwrap_or(""),
                issue.name.as_str(),
            )) {
                issue.owner = owner(pod, &by_uid);
            }
        }
        if let Some(owner) = &issue.owner {
            let key = (owner.uid.clone(), issue.code.clone(), issue.subject.clone());
            if let Some(index) = grouped.get(&key) {
                result[*index].count = result[*index].count.saturating_add(1);
                continue;
            }
            grouped.insert(key, result.len());
        }
        result.push(issue);
    }
    result
}
fn owner(object: &Value, index: &BTreeMap<&str, &Value>) -> Option<IssueOwner> {
    let mut current = object;
    let mut owner = None;
    let mut seen = BTreeSet::new();
    for _ in 0..32 {
        let Some(reference) = current["metadata"]["ownerReferences"]
            .as_array()
            .and_then(|owners| owners.iter().find(|owner| owner["controller"] == true))
        else {
            break;
        };
        let uid = reference["uid"].as_str()?;
        let kind = reference["kind"].as_str()?;
        let name = reference["name"].as_str()?;
        if uid.is_empty()
            || uid.len() > 128
            || kind.len() > 128
            || name.len() > 253
            || !seen.insert(uid)
        {
            break;
        }
        owner = Some(IssueOwner {
            uid: uid.into(),
            kind: kind.into(),
            name: name.into(),
            namespace: current["metadata"]["namespace"].as_str().map(str::to_owned),
        });
        let Some(next) = index.get(uid) else { break };
        if next["metadata"]["name"] != name
            || next["kind"] != kind
            || next["metadata"]["namespace"] != current["metadata"]["namespace"]
        {
            break;
        }
        current = next;
    }
    owner
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn durations_and_staleness_require_actual_timestamps_and_comparable_revisions() {
        assert_eq!(duration_seconds("1h30m5s"), Some(5405));
        assert_eq!(duration_seconds("0s"), None);
        assert_eq!(duration_seconds("2days"), None);
        let now = 1_700_000_000;
        let stamp = chrono::DateTime::from_timestamp(now - 601, 0)
            .unwrap()
            .to_rfc3339();
        let source = json!({"apiVersion":"source.toolkit.fluxcd.io/v1","kind":"GitRepository","metadata":{"name":"git","namespace":"apps"},
            "status":{"artifact":{"revision":"main@new","lastUpdateTime":stamp}}});
        let mut app = json!({"apiVersion":"kustomize.toolkit.fluxcd.io/v1","kind":"Kustomization","metadata":{"name":"app","namespace":"apps"},
            "spec":{"interval":"5m0s","sourceRef":{"kind":"GitRepository","name":"git"}},"status":{"lastAppliedRevision":"main@old"}});
        assert!(diagnose(&[source.clone(), app.clone()], now)
            .iter()
            .any(|issue| issue.code == "flux_source_stale"));
        app["status"]["lastAppliedRevision"] = json!("main@new");
        assert!(!diagnose(&[source, app], now)
            .iter()
            .any(|issue| issue.code == "flux_source_stale"));
    }
    #[test]
    fn missing_dependencies_drift_and_old_explicit_suspension_are_distinct() {
        let now = 1_700_000_000;
        let stamp = chrono::DateTime::from_timestamp(now - 86401, 0)
            .unwrap()
            .to_rfc3339();
        let hr = json!({"apiVersion":"helm.toolkit.fluxcd.io/v2","kind":"HelmRelease","metadata":{"name":"app","namespace":"apps",
            "annotations":{"consolecrypt.dev/suspended-at":stamp}},"spec":{"suspend":true,"dependsOn":[{"name":"missing"}]},
            "status":{"conditions":[{"type":"Drifted","status":"True","message":"password=do-not-copy"}]}});
        let issues = diagnose(&[hr], now);
        assert_eq!(issues.len(), 3);
        assert!(!serde_json::to_string(&issues)
            .unwrap()
            .contains("do-not-copy"));
    }
    #[test]
    fn helm_staleness_uses_deployed_chart_and_remote_oci_digest_not_failed_attempts() {
        let now = 1_700_000_000;
        let stamp = chrono::DateTime::from_timestamp(now - 601, 0)
            .unwrap()
            .to_rfc3339();
        let source = json!({"apiVersion":"source.toolkit.fluxcd.io/v1","kind":"HelmChart",
            "metadata":{"name":"chart","namespace":"sources"},
            "status":{"artifact":{"revision":"1.2.0+abcd","lastUpdateTime":stamp}}});
        let mut app = json!({"apiVersion":"helm.toolkit.fluxcd.io/v2","kind":"HelmRelease",
            "metadata":{"name":"app","namespace":"apps"},"spec":{"interval":"5m"},
            "status":{"helmChart":"sources/chart","lastAttemptedRevision":"1.2.0+abcd",
                "history":[{"status":"failed","chartVersion":"1.2.0+abcd"},
                    {"status":"deployed","chartVersion":"1.1.0"}]}});
        assert!(diagnose(&[source.clone(), app.clone()], now)
            .iter()
            .any(|issue| issue.code == "flux_source_stale"));
        app["status"]["history"][1]["chartVersion"] = json!("1.2.0+abcd");
        assert!(!diagnose(&[source, app.clone()], now)
            .iter()
            .any(|issue| issue.code == "flux_source_stale"));
        app["spec"]["chartRef"] =
            json!({"kind":"OCIRepository","name":"oci","namespace":"sources"});
        app["status"]["history"][1]["ociDigest"] = json!("sha256:old");
        let mut oci = json!({"apiVersion":"source.toolkit.fluxcd.io/v1","kind":"OCIRepository",
            "metadata":{"name":"oci","namespace":"sources"},
            "status":{"artifact":{"revision":"1.2.0@sha256:new","digest":"sha256:local-archive","lastUpdateTime":stamp}}});
        assert!(diagnose(&[oci.clone(), app.clone()], now)
            .iter()
            .any(|issue| issue.code == "flux_source_stale"));
        app["status"]["history"][1]["ociDigest"] = json!("sha256:new");
        assert!(!diagnose(&[oci.clone(), app.clone()], now)
            .iter()
            .any(|issue| issue.code == "flux_source_stale"));
        oci["status"]["artifact"]["revision"] = json!("1.3.0@sha256:newer");
        oci["status"]["artifact"]["lastUpdateTime"] =
            json!(chrono::DateTime::from_timestamp(now - 60, 0)
                .unwrap()
                .to_rfc3339());
        assert!(!diagnose(&[oci, app], now)
            .iter()
            .any(|issue| issue.code == "flux_source_stale"));
    }
    #[test]
    fn pod_issues_group_by_owner_uid_through_replica_sets_and_keep_exit_code() {
        let make = |name: &str, uid: &str| {
            json!({"apiVersion":"v1","kind":"Pod","metadata":{"name":name,"namespace":"apps","uid":uid,
            "ownerReferences":[{"controller":true,"kind":"ReplicaSet","name":"rs","uid":"rs-uid"}]},
            "status":{"containerStatuses":[{"name":"web","state":{"waiting":{"reason":"CrashLoopBackOff"}},"lastState":{"terminated":{"exitCode":137}}}]}})
        };
        let objects = [
            make("one", "pod-1"),
            make("two", "pod-2"),
            json!({"apiVersion":"apps/v1","kind":"ReplicaSet","metadata":{"uid":"rs-uid","name":"rs","namespace":"apps",
                "ownerReferences":[{"controller":true,"kind":"Deployment","name":"app","uid":"deployment-uid"}]},"spec":{"replicas":0}}),
        ];
        let issues = super::super::diagnose_at(&objects, 1_700_000_000);
        let issue = issues
            .iter()
            .find(|issue| issue.code == "pod_crash_loop")
            .unwrap();
        assert_eq!(issue.count, 2);
        assert_eq!(issue.exit_code, Some(137));
        assert_eq!(issue.owner.as_ref().unwrap().kind, "Deployment");
    }
}
