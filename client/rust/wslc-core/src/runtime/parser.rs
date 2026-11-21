//! WSLc 3.x output (verified on 3.0.1 and 3.0.3). List = NDJSON; inspect = JSON array.
//! Fields used for decisions are strict; informational fields added after 3.0.1 are lenient.
use crate::models::*;
use serde_json::Value;

fn invalid(operation: &str) -> AppError {
    // Never include the raw document: inspect may contain credentials.
    AppError::new(
        "invalid_output",
        operation,
        "CLI вернул повреждённый или неподдерживаемый формат данных",
    )
}

fn field(value: &Value, name: &str, op: &str) -> AppResult<Option<String>> {
    match value.get(name) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(s)) => Ok(Some(s.clone())),
        _ => Err(invalid(op)),
    }
}

fn optional_display(value: Option<String>) -> Option<String> {
    value.filter(|s| !matches!(s.as_str(), "N/A" | "<none>" | ""))
}

/// Display-only string that never fails the whole document when its type changes.
fn lenient(value: &Value, name: &str) -> Option<String> {
    value.get(name)?.as_str().map(str::to_owned)
}

fn command_line(config: &Value) -> Option<String> {
    let words: Vec<&str> = ["Entrypoint", "Cmd"]
        .iter()
        .filter_map(|key| config.get(*key)?.as_array())
        .flatten()
        .filter_map(Value::as_str)
        .collect();
    (!words.is_empty()).then(|| words.join(" "))
}

fn port_bindings(v: &Value) -> Vec<PortBinding> {
    // 3.0.3 reports a top-level map; Docker-style NetworkSettings.Ports is accepted as well.
    let map = v
        .get("Ports")
        .and_then(Value::as_object)
        .or_else(|| v["NetworkSettings"].get("Ports")?.as_object());
    let Some(map) = map else {
        return Vec::new();
    };
    let mut ports = Vec::new();
    for (container_port, bindings) in map {
        match bindings.as_array() {
            Some(items) if !items.is_empty() => ports.extend(items.iter().map(|b| PortBinding {
                container_port: container_port.clone(),
                host_ip: optional_display(lenient(b, "HostIp")),
                host_port: optional_display(lenient(b, "HostPort")),
            })),
            _ => ports.push(PortBinding {
                container_port: container_port.clone(),
                host_ip: None,
                host_port: None,
            }),
        }
    }
    ports
}

fn network_attachments(v: &Value) -> Vec<NetworkAttachment> {
    let Some(map) = v["NetworkSettings"]
        .get("Networks")
        .and_then(Value::as_object)
    else {
        return Vec::new();
    };
    map.iter()
        .map(|(name, n)| NetworkAttachment {
            name: name.clone(),
            ip_address: optional_display(lenient(n, "IPAddress")),
            gateway: optional_display(lenient(n, "Gateway")),
            mac_address: optional_display(lenient(n, "MacAddress")),
        })
        .collect()
}

fn required(value: &Value, name: &str, op: &str) -> AppResult<String> {
    field(value, name, op)?
        .filter(|s| !s.is_empty())
        .ok_or_else(|| invalid(op))
}

pub fn list_documents(text: &str, op: &str) -> AppResult<Vec<Value>> {
    // Empty successful NDJSON is a real empty list. The executor has checked the exit code.
    text.lines()
        .filter(|l| !l.trim().is_empty())
        .map(|line| {
            let v: Value = serde_json::from_str(line).map_err(|_| invalid(op))?;
            if !v.is_object() {
                return Err(invalid(op));
            }
            Ok(v)
        })
        .collect()
}

pub fn containers(text: &str) -> AppResult<Vec<ContainerSummary>> {
    let op = "list_containers";
    list_documents(text, op)?
        .iter()
        .map(|v| {
            let raw_state = field(v, "State", op)?;
            let state = ContainerState::from_raw(raw_state.as_deref());
            let ports = field(v, "Ports", op)?;
            Ok(ContainerSummary {
                id: required(v, "ID", op)?,
                name: field(v, "Names", op)?,
                image: field(v, "Image", op)?,
                state,
                raw_state,
                ports_display: if state == ContainerState::Running {
                    ports
                } else {
                    optional_display(ports)
                },
                created_at: optional_display(field(v, "CreatedAt", op)?),
                health_status: optional_display(field(v, "HealthStatus", op)?),
                status_display: optional_display(lenient(v, "Status")),
            })
        })
        .collect()
}

pub fn images(text: &str) -> AppResult<Vec<ImageSummary>> {
    let op = "list_images";
    list_documents(text, op)?
        .iter()
        .map(|v| {
            Ok(ImageSummary {
                id: required(v, "ID", op)?,
                repository: optional_display(field(v, "Repository", op)?),
                tag: optional_display(field(v, "Tag", op)?),
                size_display: optional_display(field(v, "Size", op)?),
                created_at: optional_display(field(v, "CreatedAt", op)?),
                containers: lenient(v, "Containers").and_then(|n| n.parse().ok()),
            })
        })
        .collect()
}

pub fn volumes(text: &str) -> AppResult<Vec<VolumeSummary>> {
    let op = "list_volumes";
    list_documents(text, op)?
        .iter()
        .map(|v| {
            Ok(VolumeSummary {
                name: required(v, "Name", op)?,
                driver: field(v, "Driver", op)?,
                size_display: optional_display(field(v, "Size", op)?),
            })
        })
        .collect()
}

pub fn networks(text: &str) -> AppResult<Vec<NetworkSummary>> {
    let op = "list_networks";
    fn boolean(v: &Value, key: &str) -> AppResult<Option<bool>> {
        match v.get(key) {
            None | Some(Value::Null) => Ok(None),
            Some(Value::String(s)) if s == "true" => Ok(Some(true)),
            Some(Value::String(s)) if s == "false" => Ok(Some(false)),
            _ => Err(invalid("list_networks")),
        }
    }
    list_documents(text, op)?
        .iter()
        .map(|v| {
            Ok(NetworkSummary {
                id: required(v, "ID", op)?,
                name: field(v, "Name", op)?,
                driver: field(v, "Driver", op)?,
                scope: field(v, "Scope", op)?,
                ipv4: boolean(v, "IPv4")?,
                ipv6: boolean(v, "IPv6")?,
                internal: boolean(v, "Internal")?,
            })
        })
        .collect()
}

pub fn stats(text: &str) -> AppResult<Vec<ContainerStats>> {
    let op = "container_stats";
    fn percent(v: &Value, key: &str) -> Option<f64> {
        lenient(v, key)?.trim().trim_end_matches('%').parse().ok()
    }
    list_documents(text, op)?
        .iter()
        .map(|v| {
            Ok(ContainerStats {
                id: required(v, "ID", op)?,
                cpu_percent: percent(v, "CPUPerc"),
                memory_percent: percent(v, "MemPerc"),
                memory_usage: optional_display(lenient(v, "MemUsage")),
                net_io: optional_display(lenient(v, "NetIO")),
                block_io: optional_display(lenient(v, "BlockIO")),
                pids: v.get("PIDs").and_then(|p| {
                    p.as_u64()
                        .or_else(|| p.as_str().and_then(|s| s.parse().ok()))
                }),
            })
        })
        .collect()
}

/// Output of [`crate::runtime::cli::LIST_FILES_SCRIPT`]: `<d|f><l|-><TAB><name>` per entry.
pub fn file_entries(text: &str) -> Vec<FileEntry> {
    let mut entries: Vec<FileEntry> = text
        .lines()
        .filter_map(|line| {
            let (kind, name) = line.split_once('\t')?;
            let kind = kind.as_bytes();
            (kind.len() == 2 && !name.is_empty()).then(|| FileEntry {
                name: name.to_owned(),
                is_dir: kind[0] == b'd',
                is_link: kind[1] == b'l',
            })
        })
        .collect();
    entries.sort_by(|a, b| {
        b.is_dir
            .cmp(&a.is_dir)
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
    entries
}

pub fn inspect_document(text: &str, op: &str) -> AppResult<Value> {
    let v: Value = serde_json::from_str(text).map_err(|_| invalid(op))?;
    let values = v.as_array().ok_or_else(|| invalid(op))?;
    if values.len() != 1 || !values[0].is_object() {
        return Err(invalid(op));
    }
    Ok(values[0].clone())
}

pub fn redact_environment(v: &mut Value) {
    match v {
        Value::Object(map) => {
            for (key, value) in map {
                if key.eq_ignore_ascii_case("env") && !value.is_null() {
                    *value = Value::String("[значения окружения скрыты]".into());
                } else {
                    redact_environment(value);
                }
            }
        }
        Value::Array(items) => items.iter_mut().for_each(redact_environment),
        _ => {}
    }
}

pub fn inspect_container(text: &str, reveal_environment: bool) -> AppResult<ContainerDetails> {
    let op = "inspect_container";
    let mut v = inspect_document(text, op)?;
    let raw_state = field(&v["State"], "Status", op)?;
    let mounts = match v.get("Mounts") {
        None | Some(Value::Null) => None,
        Some(Value::Array(items)) => Some(
            items
                .iter()
                .map(|m| {
                    let read_write = match m.get("ReadWrite") {
                        None | Some(Value::Null) => None,
                        Some(Value::Bool(b)) => Some(*b),
                        _ => return Err(invalid(op)),
                    };
                    Ok(MountInfo {
                        kind: field(m, "Type", op)?,
                        name: optional_display(field(m, "Name", op)?),
                        source: field(m, "Source", op)?,
                        destination: field(m, "Destination", op)?,
                        read_write,
                    })
                })
                .collect::<AppResult<Vec<_>>>()?,
        ),
        _ => return Err(invalid(op)),
    };
    let mut result = ContainerDetails {
        id: required(&v, "Id", op)?,
        name: field(&v, "Name", op)?.map(|n| n.trim_start_matches('/').to_owned()),
        image_id: optional_display(field(&v, "Image", op)?),
        image_name: field(&v["Config"], "Image", op)?,
        state: ContainerState::from_raw(raw_state.as_deref()),
        raw_state,
        // 3.0.1 schema does NOT expose AutoRemove. Do not infer false from absence.
        auto_remove: v["HostConfig"]["AutoRemove"].as_bool(),
        stop_timeout_seconds: v["Config"]["StopTimeout"].as_u64(),
        exit_code: v["State"]["ExitCode"].as_i64(),
        created_at: field(&v, "Created", op)?,
        started_at: optional_display(field(&v["State"], "StartedAt", op)?),
        finished_at: optional_display(lenient(&v["State"], "FinishedAt"))
            .filter(|t| !t.starts_with("0001-")),
        health_status: field(&v["State"]["Health"], "Status", op)?,
        command: command_line(&v["Config"]),
        mounts,
        ports: port_bindings(&v),
        networks: network_attachments(&v),
        inspect_json: String::new(),
    };
    if !reveal_environment {
        redact_environment(&mut v);
    }
    result.inspect_json = serde_json::to_string_pretty(&v).map_err(|_| invalid(op))?;
    Ok(result)
}
