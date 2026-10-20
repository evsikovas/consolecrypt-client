use super::*;

#[derive(Debug, Clone, Serialize)]
pub struct MetricUsage {
    pub name: String,
    pub namespace: Option<String>,
    pub timestamp: String,
    pub window: String,
    pub cpu_cores: f64,
    pub memory_bytes: u64,
    pub containers: Vec<ContainerUsage>,
    pub requests: Option<ResourceAmounts>,
    pub limits: Option<ResourceAmounts>,
    pub capacity: Option<ResourceAmounts>,
    pub allocatable: Option<ResourceAmounts>,
    pub resources_available: bool,
}
#[derive(Debug, Clone, Serialize)]
pub struct ContainerUsage {
    pub name: String,
    pub cpu_cores: f64,
    pub memory_bytes: u64,
    pub requests: Option<ResourceAmounts>,
    pub limits: Option<ResourceAmounts>,
}

#[derive(Debug, Clone, Default, Serialize, PartialEq)]
pub struct ResourceAmounts {
    pub cpu_cores: Option<f64>,
    pub memory_bytes: Option<u64>,
}

impl ResourceApi {
    pub async fn metric_usage(
        &self,
        pods: bool,
        namespace: Option<&str>,
    ) -> Result<Vec<MetricUsage>, ResourceError> {
        let mut rows = parse_metrics(&self.metrics(pods, namespace).await?)?;
        let api = self.resolve("", "v1", if pods { "pods" } else { "nodes" })?;
        let lookup: BTreeMap<_, _> = rows
            .iter()
            .enumerate()
            .map(|(index, row)| ((row.namespace.clone(), row.name.clone()), index))
            .collect();
        let mut query = ListQuery {
            namespace: namespace.map(str::to_owned),
            limit: Some(500),
            ..ListQuery::default()
        };
        let mut seen = BTreeSet::new();
        let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
        let mut count = 0;
        loop {
            let page = match tokio::time::timeout_at(deadline, self.list(&api, &query)).await {
                Ok(Ok(page)) => page,
                Ok(Err(ResourceError::Closed)) => return Err(ResourceError::Closed),
                _ => break, // Metrics RBAC can differ from core object access.
            };
            for object in page.items {
                count += 1;
                if count > 20_000 {
                    return Ok(rows);
                }
                if let Some(index) = lookup.get(&(object.namespace, object.name)) {
                    enrich(&mut rows[*index], &object.object, pods)?;
                }
            }
            query.continue_token = page.continue_token;
            let Some(token) = &query.continue_token else {
                break;
            };
            if !seen.insert(token.clone()) {
                break;
            }
        }
        Ok(rows)
    }
}

pub fn parse_metrics(value: &Value) -> Result<Vec<MetricUsage>, ResourceError> {
    let items = value["items"]
        .as_array()
        .ok_or(ResourceError::InvalidResponse)?;
    if items.len() > 20_000 {
        return Err(ResourceError::TooLarge);
    }
    let mut result = Vec::with_capacity(items.len());
    for item in items {
        let name = response_string(&item["metadata"]["name"], 253)?
            .ok_or(ResourceError::InvalidResponse)?;
        let namespace = response_string(&item["metadata"]["namespace"], 63)?;
        let timestamp =
            response_string(&item["timestamp"], 64)?.ok_or(ResourceError::InvalidResponse)?;
        chrono::DateTime::parse_from_rfc3339(&timestamp)
            .map_err(|_| ResourceError::InvalidResponse)?;
        let window = response_string(&item["window"], 64)?.unwrap_or_default();
        let (containers, cpu_cores, memory_bytes) =
            if let Some(containers) = item["containers"].as_array() {
                if containers.len() > 1024 {
                    return Err(ResourceError::TooLarge);
                }
                let mut entries = Vec::with_capacity(containers.len());
                let mut cpu = 0.0;
                let mut memory = 0_u64;
                for container in containers {
                    let name = response_string(&container["name"], 63)?
                        .ok_or(ResourceError::InvalidResponse)?;
                    let (cpu_cores, memory_bytes) = usage(&container["usage"])?;
                    cpu += cpu_cores;
                    memory = memory
                        .checked_add(memory_bytes)
                        .ok_or(ResourceError::TooLarge)?;
                    entries.push(ContainerUsage {
                        name,
                        cpu_cores,
                        memory_bytes,
                        requests: None,
                        limits: None,
                    });
                }
                if !cpu.is_finite() {
                    return Err(ResourceError::InvalidResponse);
                }
                (entries, cpu, memory)
            } else {
                let (cpu, memory) = usage(&item["usage"])?;
                (Vec::new(), cpu, memory)
            };
        result.push(MetricUsage {
            name,
            namespace,
            timestamp,
            window,
            cpu_cores,
            memory_bytes,
            containers,
            requests: None,
            limits: None,
            capacity: None,
            allocatable: None,
            resources_available: false,
        });
    }
    Ok(result)
}

fn amounts(value: &Value) -> Result<ResourceAmounts, ResourceError> {
    let parse = |key: &str| {
        value
            .get(key)
            .map(|value| quantity(value.as_str().ok_or(ResourceError::InvalidResponse)?))
            .transpose()
    };
    let memory = parse("memory")?;
    if memory.is_some_and(|memory| memory > u64::MAX as f64) {
        return Err(ResourceError::TooLarge);
    }
    Ok(ResourceAmounts {
        cpu_cores: parse("cpu")?,
        memory_bytes: memory.map(|memory| memory.ceil() as u64),
    })
}
fn enrich(row: &mut MetricUsage, object: &Value, pods: bool) -> Result<(), ResourceError> {
    if !pods {
        row.capacity = Some(amounts(&object["status"]["capacity"])?);
        row.allocatable = Some(amounts(&object["status"]["allocatable"])?);
    } else {
        for container in &mut row.containers {
            if let Some(definition) = ["containers", "initContainers"]
                .into_iter()
                .filter_map(|field| object["spec"][field].as_array())
                .flatten()
                .find(|definition| definition["name"] == container.name)
            {
                container.requests = Some(amounts(&definition["resources"]["requests"])?);
                container.limits = Some(amounts(&definition["resources"]["limits"])?);
            }
        }
        row.requests = Some(pod_amounts(object, false)?);
        row.limits = Some(pod_amounts(object, true)?);
    }
    row.resources_available = true;
    Ok(())
}
fn zero() -> ResourceAmounts {
    ResourceAmounts {
        cpu_cores: Some(0.0),
        memory_bytes: Some(0),
    }
}
fn add(a: &ResourceAmounts, b: &ResourceAmounts) -> Result<ResourceAmounts, ResourceError> {
    let cpu = a.cpu_cores.zip(b.cpu_cores).map(|(a, b)| a + b);
    if cpu.is_some_and(|cpu| !cpu.is_finite()) {
        return Err(ResourceError::TooLarge);
    }
    let memory = a
        .memory_bytes
        .zip(b.memory_bytes)
        .map(|(a, b)| a.checked_add(b).ok_or(ResourceError::TooLarge))
        .transpose()?;
    Ok(ResourceAmounts {
        cpu_cores: cpu,
        memory_bytes: memory,
    })
}
fn maximum(a: &ResourceAmounts, b: &ResourceAmounts) -> ResourceAmounts {
    ResourceAmounts {
        cpu_cores: a.cpu_cores.zip(b.cpu_cores).map(|(a, b)| a.max(b)),
        memory_bytes: a.memory_bytes.zip(b.memory_bytes).map(|(a, b)| a.max(b)),
    }
}
fn pod_amounts(pod: &Value, limits: bool) -> Result<ResourceAmounts, ResourceError> {
    let field = if limits { "limits" } else { "requests" };
    let container = |value: &Value| -> Result<ResourceAmounts, ResourceError> {
        let mut amount = amounts(&value["resources"][field])?;
        if !limits {
            amount.cpu_cores.get_or_insert(0.0);
            amount.memory_bytes.get_or_insert(0);
        }
        Ok(amount)
    };
    let mut app = zero();
    for value in pod["spec"]["containers"].as_array().into_iter().flatten() {
        app = add(&app, &container(value)?)?;
    }
    let mut sidecars = zero();
    let mut init = zero();
    for value in pod["spec"]["initContainers"]
        .as_array()
        .into_iter()
        .flatten()
    {
        let amount = container(value)?;
        if value["restartPolicy"] == "Always" {
            sidecars = add(&sidecars, &amount)?;
            init = maximum(&init, &sidecars);
        } else {
            init = maximum(&init, &add(&sidecars, &amount)?);
        }
    }
    let mut effective = maximum(&add(&app, &sidecars)?, &init);
    let pod_level = amounts(&pod["spec"]["resources"][field])?;
    if pod_level.cpu_cores.is_some() {
        effective.cpu_cores = pod_level.cpu_cores;
    }
    if pod_level.memory_bytes.is_some() {
        effective.memory_bytes = pod_level.memory_bytes;
    }
    let mut overhead = amounts(&pod["spec"]["overhead"])?;
    overhead.cpu_cores.get_or_insert(0.0);
    overhead.memory_bytes.get_or_insert(0);
    add(&effective, &overhead)
}

fn usage(value: &Value) -> Result<(f64, u64), ResourceError> {
    let cpu = quantity(
        value["cpu"]
            .as_str()
            .ok_or(ResourceError::InvalidResponse)?,
    )?;
    let memory = quantity(
        value["memory"]
            .as_str()
            .ok_or(ResourceError::InvalidResponse)?,
    )?;
    if memory > u64::MAX as f64 {
        return Err(ResourceError::TooLarge);
    }
    Ok((cpu, memory.ceil() as u64))
}

fn quantity(value: &str) -> Result<f64, ResourceError> {
    if value.len() > 64 {
        return Err(ResourceError::InvalidResponse);
    }
    let units = [
        ("Ki", 1024_f64),
        ("Mi", 1024_f64.powi(2)),
        ("Gi", 1024_f64.powi(3)),
        ("Ti", 1024_f64.powi(4)),
        ("Pi", 1024_f64.powi(5)),
        ("Ei", 1024_f64.powi(6)),
        ("n", 1e-9),
        ("u", 1e-6),
        ("m", 1e-3),
        ("k", 1e3),
        ("K", 1e3),
        ("M", 1e6),
        ("G", 1e9),
        ("T", 1e12),
        ("P", 1e15),
        ("E", 1e18),
    ];
    let (number, multiplier) = units
        .iter()
        .find_map(|(suffix, multiplier)| {
            value
                .strip_suffix(suffix)
                .map(|number| (number, *multiplier))
        })
        .unwrap_or((value, 1.0));
    let result = number
        .parse::<f64>()
        .map_err(|_| ResourceError::InvalidResponse)?
        * multiplier;
    if !result.is_finite() || result < 0.0 {
        return Err(ResourceError::InvalidResponse);
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn metrics_use_correct_cpu_and_binary_memory_units() {
        let value = json!({"items":[{"metadata":{"name":"web","namespace":"shop"},"timestamp":"2026-10-20T12:00:00Z","window":"30s","containers":[
            {"name":"app","usage":{"cpu":"125000000n","memory":"128Mi"}},
            {"name":"sidecar","usage":{"cpu":"25m","memory":"4Ki"}}
        ]}]});
        let rows = parse_metrics(&value).unwrap();
        assert!((rows[0].cpu_cores - 0.15).abs() < 0.000_001);
        assert_eq!(rows[0].memory_bytes, 128 * 1024 * 1024 + 4 * 1024);
        for value in ["NaN", "inf", "-1", "1e999", "fixture"] {
            assert!(quantity(value).is_err());
        }
    }
    #[test]
    fn pod_budget_accounts_for_init_peak_sidecars_overhead_and_unbounded_limits() {
        let pod = json!({"spec":{"containers":[{"name":"app","resources":{"requests":{"cpu":"200m","memory":"100Mi"},"limits":{"cpu":"1"}}}],
            "initContainers":[{"name":"sidecar","restartPolicy":"Always","resources":{"requests":{"cpu":"100m","memory":"10Mi"}}},
                {"name":"setup","resources":{"requests":{"cpu":"2","memory":"200Mi"}}}],
            "overhead":{"cpu":"50m","memory":"5Mi"}}});
        let requests = pod_amounts(&pod, false).unwrap();
        assert!((requests.cpu_cores.unwrap() - 2.15).abs() < 0.000001);
        assert_eq!(requests.memory_bytes, Some(215 * 1024 * 1024));
        assert_eq!(pod_amounts(&pod, true).unwrap(), ResourceAmounts::default());
        let node = json!({"status":{"capacity":{"cpu":"8","memory":"32Gi"},"allocatable":{"cpu":"7500m","memory":"30Gi"}}});
        let mut metric = parse_metrics(&json!({"items":[{"metadata":{"name":"worker"},"timestamp":"2026-10-20T12:00:00Z","usage":{"cpu":"1","memory":"1Gi"}}]})).unwrap().remove(0);
        enrich(&mut metric, &node, false).unwrap();
        assert_eq!(metric.allocatable.unwrap().cpu_cores, Some(7.5));
        assert!(metric.resources_available);
    }
}
