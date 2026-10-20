use super::*;

#[derive(Debug, Clone, Serialize, PartialEq, Eq, PartialOrd, Ord)]
pub struct ForwardPort {
    pub name: Option<String>,
    pub port: u16,
}
#[derive(Debug, Clone, Serialize)]
pub struct ForwardTarget {
    pub pod: ResourceRef,
    pub uid: String,
    pub ready: bool,
    pub ports: Vec<ForwardPort>,
}
#[derive(Debug, Default, Serialize)]
pub struct ForwardTargets {
    pub items: Vec<ForwardTarget>,
    pub partial: bool,
}

impl ResourceApi {
    /// Resolves candidates only. The caller must display and confirm a selected
    /// Pod/port before opening the ordinary guarded forward capability.
    pub async fn forward_targets(
        &self,
        reference: &ResourceRef,
    ) -> Result<ForwardTargets, ResourceError> {
        let direct = reference.api.group.is_empty() && reference.api.resource == "pods";
        let service = reference.api.group.is_empty() && reference.api.resource == "services";
        if !direct
            && !service
            && !(reference.api.group == "apps"
                && matches!(
                    reference.api.resource.as_str(),
                    "deployments" | "replicasets" | "statefulsets" | "daemonsets"
                ))
            && !(reference.api.group == "batch" && reference.api.resource == "jobs")
        {
            return Err(ResourceError::InvalidRequest);
        }
        let source = self.get(reference, ReadOptions::AI).await?;
        let kind = self.resolve("", "v1", "pods")?;
        if direct {
            return Ok(ForwardTargets {
                items: vec![target(&kind, source.summary, None)?],
                partial: false,
            });
        }
        let selector = if service {
            operations::selector_from_labels(&source.object["spec"]["selector"])?
        } else {
            operations::selector(&source.object["spec"]["selector"])?
        };
        let mut query = ListQuery {
            namespace: reference.namespace.clone(),
            label_selector: Some(selector),
            limit: Some(200),
            ..ListQuery::default()
        };
        let deadline = tokio::time::Instant::now() + Duration::from_secs(15);
        let mut result = ForwardTargets::default();
        let mut tokens = BTreeSet::new();
        let mut seen = BTreeSet::new();
        loop {
            let page = match tokio::time::timeout_at(deadline, self.list(&kind, &query)).await {
                Ok(result) => result?,
                Err(_) => {
                    result.partial = true;
                    break;
                }
            };
            for row in page.items {
                if !seen.insert(row.uid.clone()) {
                    continue;
                }
                if result.items.len() >= 1000 {
                    result.partial = true;
                    return Ok(result);
                }
                result
                    .items
                    .push(target(&kind, row, service.then_some(&source.object))?);
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
        result.items.sort_by(|a, b| {
            b.ready
                .cmp(&a.ready)
                .then_with(|| a.pod.name.cmp(&b.pod.name))
        });
        Ok(result)
    }
}
fn target(
    api: &ResourceKind,
    row: ResourceRow,
    service: Option<&Value>,
) -> Result<ForwardTarget, ResourceError> {
    if row.uid.is_empty() {
        return Err(ResourceError::InvalidResponse);
    }
    let ports = forward_ports(&row.object, service);
    let ready = row.object["status"]["phase"] == "Running"
        && row.object["status"]["conditions"]
            .as_array()
            .is_some_and(|conditions| {
                conditions
                    .iter()
                    .any(|condition| condition["type"] == "Ready" && condition["status"] == "True")
            });
    Ok(ForwardTarget {
        pod: ResourceRef {
            api: api.clone(),
            namespace: row.namespace,
            name: row.name,
        },
        uid: row.uid,
        ready,
        ports,
    })
}
fn forward_ports(pod: &Value, service: Option<&Value>) -> Vec<ForwardPort> {
    let ports = pod["spec"]["containers"]
        .as_array()
        .into_iter()
        .flatten()
        .chain(
            pod["spec"]["initContainers"]
                .as_array()
                .into_iter()
                .flatten()
                .filter(|container| container["restartPolicy"] == "Always"),
        )
        .filter_map(|container| container["ports"].as_array())
        .flatten()
        .filter(|port| port["protocol"].is_null() || port["protocol"] == "TCP")
        .filter_map(|port| {
            Some(ForwardPort {
                name: port["name"]
                    .as_str()
                    .filter(|name| name.len() <= 63)
                    .map(str::to_owned),
                port: u16::try_from(port["containerPort"].as_u64()?)
                    .ok()
                    .filter(|port| *port > 0)?,
            })
        })
        .collect::<BTreeSet<_>>();
    let Some(service) = service else {
        return ports.into_iter().take(1024).collect();
    };
    let mut result = BTreeSet::new();
    for service_port in service["spec"]["ports"]
        .as_array()
        .into_iter()
        .flatten()
        .take(1024)
    {
        if !service_port["protocol"].is_null() && service_port["protocol"] != "TCP" {
            continue;
        }
        let target = &service_port["targetPort"];
        let number = if let Some(name) = target.as_str() {
            let matches: BTreeSet<_> = ports
                .iter()
                .filter(|port| port.name.as_deref() == Some(name))
                .map(|port| port.port)
                .collect();
            if matches.len() != 1 {
                continue;
            }
            matches.into_iter().next()
        } else {
            let number = if target.is_null() {
                service_port["port"].as_u64()
            } else {
                target.as_u64()
            };
            number
                .and_then(|port| u16::try_from(port).ok())
                .filter(|port| *port > 0)
        };
        if let Some(port) = number {
            result.insert(ForwardPort {
                name: service_port["name"]
                    .as_str()
                    .filter(|name| name.len() <= 63)
                    .map(str::to_owned),
                port,
            });
        }
    }
    result.into_iter().collect()
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn service_named_target_ports_resolve_per_pod_and_never_forward_udp_or_ambiguous_ports() {
        let mut pod = json!({"spec":{"containers":[{"ports":[{"name":"http","containerPort":8080},{"name":"dns","containerPort":53,"protocol":"UDP"}]}]}});
        let service = json!({"spec":{"ports":[{"name":"web","port":80,"targetPort":"http"},{"name":"dns","port":53,"protocol":"UDP"},{"name":"raw","port":4444}]}});
        let ports = forward_ports(&pod, Some(&service));
        assert!(ports
            .iter()
            .any(|port| port.port == 8080 && port.name.as_deref() == Some("web")));
        assert!(ports.iter().any(|port| port.port == 4444));
        assert!(!ports.iter().any(|port| port.port == 53 || port.port == 80));
        pod["spec"]["containers"]
            .as_array_mut()
            .unwrap()
            .push(json!({"ports":[{"name":"http","containerPort":9090}]}));
        assert!(!forward_ports(&pod, Some(&service))
            .iter()
            .any(|port| port.name.as_deref() == Some("web")));
    }
}
