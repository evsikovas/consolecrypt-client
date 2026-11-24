use super::{collect, DockerClient, DockerError, REQUEST_TIMEOUT};
use hyper::{header::DATE, Method};
use serde_json::{json, Value};
use std::time::{Duration, UNIX_EPOCH};

impl DockerClient {
    pub async fn disk_usage(&self) -> Result<Value, DockerError> {
        // Some engines need to walk volume data before sending the response headers.
        let snapshot = tokio::time::timeout(Duration::from_secs(120), async {
            let response = self
                .send(Method::GET, "/system/df?verbose=0", None, false)
                .await?;
            let bytes = collect(response).await?;
            serde_json::from_slice::<Value>(&bytes).map_err(|_| DockerError::Protocol)
        })
        .await
        .map_err(|_| DockerError::Connection)??;
        disk_summary(&snapshot)
    }

    pub async fn recent_events(&self) -> Result<Vec<Value>, DockerError> {
        tokio::time::timeout(REQUEST_TIMEOUT, async {
            // 'until' makes /events a finite history request. Use the daemon's clock:
            // the workstation and its SSH host can have different clocks.
            let ping = self.send(Method::GET, "/_ping", None, false).await?;
            let now = ping
                .headers()
                .get(DATE)
                .and_then(|d| d.to_str().ok())
                .and_then(|d| httpdate::parse_http_date(d).ok())
                .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                .ok_or(DockerError::Protocol)?
                .as_secs();
            drop(ping);
            let filters = "%7B%22type%22%3A%5B%22container%22%5D%7D";
            let path = format!(
                "/events?since={}&until={now}&filters={filters}",
                now.saturating_sub(86_400)
            );
            let response = self.send(Method::GET, &path, None, false).await?;
            event_rows(&collect(response).await?)
        })
        .await
        .map_err(|_| DockerError::Connection)?
    }
}

fn disk_summary(snapshot: &Value) -> Result<Value, DockerError> {
    if !snapshot.is_object()
        || !(snapshot.get("ImagesUsage").is_some() || snapshot.get("LayersSize").is_some())
    {
        return Err(DockerError::Protocol);
    }
    let mut summary = serde_json::Map::new();
    for (key, modern, legacy) in [
        ("images", "ImagesUsage", "Images"),
        ("containers", "ContainersUsage", "Containers"),
        ("volumes", "VolumesUsage", "Volumes"),
        ("cache", "BuildCacheUsage", "BuildCache"),
    ] {
        let stats = if let Some(usage) = snapshot.get(modern).filter(|value| value.is_object()) {
            json!({"bytes":usage["TotalSize"].as_u64(),"count":usage["TotalCount"].as_u64(),"reclaimable":usage["Reclaimable"].as_u64()})
        } else {
            let items: Option<&[Value]> = match snapshot.get(legacy) {
                Some(Value::Null) => Some(&[]),
                Some(Value::Array(values)) => Some(values.as_slice()),
                _ => None,
            };
            let bytes = match key {
                // LayersSize already accounts for layers shared by images.
                "images" => snapshot["LayersSize"].as_u64(),
                "containers" => items.and_then(|rows| sum_bytes(rows, "/SizeRw")),
                "volumes" => items.and_then(|rows| sum_bytes(rows, "/UsageData/Size")),
                _ => items.and_then(|rows| {
                    rows.iter()
                        .filter(|item| item["Shared"] != true)
                        .try_fold(0u64, |n, item| n.checked_add(item["Size"].as_u64()?))
                }),
            };
            json!({"bytes":bytes,"count":items.map(<[Value]>::len),"reclaimable":null})
        };
        summary.insert(key.to_owned(), stats);
    }
    Ok(Value::Object(summary))
}

fn sum_bytes(items: &[Value], pointer: &str) -> Option<u64> {
    items.iter().try_fold(0u64, |n, item| {
        n.checked_add(item.pointer(pointer)?.as_u64()?)
    })
}

fn event_rows(body: &[u8]) -> Result<Vec<Value>, DockerError> {
    let mut events = Vec::new();
    for value in serde_json::Deserializer::from_slice(body).into_iter::<Value>() {
        let value = value.map_err(|_| DockerError::Protocol)?;
        let action = value["Action"].as_str().unwrap_or("");
        // exec events include the command line, and arbitrary actor attributes may be secrets.
        if value["Type"] != "container"
            || !matches!(
                action,
                "create"
                    | "start"
                    | "restart"
                    | "stop"
                    | "die"
                    | "kill"
                    | "oom"
                    | "destroy"
                    | "pause"
                    | "unpause"
                    | "rename"
                    | "health_status: healthy"
                    | "health_status: unhealthy"
                    | "health_status: starting"
            )
        {
            continue;
        }
        let attrs = &value["Actor"]["Attributes"];
        let Some(nanos) = value["timeNano"]
            .as_u64()
            .or_else(|| value["time"].as_u64()?.checked_mul(1_000_000_000))
        else {
            continue;
        };
        let id = value["Actor"]["ID"].as_str().unwrap_or("");
        events.push((nanos, json!({"timeMs":nanos / 1_000_000, "action":action, "id":id,
            "name":attrs["name"].as_str(), "exitCode":attrs["exitCode"].as_str().and_then(|s|s.parse::<i32>().ok())})));
    }
    // Reverse chronological daemon order also resolves events with identical timestamps.
    events.reverse();
    events.sort_by_key(|(nanos, _)| std::cmp::Reverse(*nanos));
    Ok(events
        .into_iter()
        .take(256)
        .map(|(_, event)| event)
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn disk_usage_handles_old_and_new_engines_without_double_counting() {
        let old = json!({"Images":[{"Size":100},{"Size":100}],"LayersSize":130,"Containers":null,
            "Volumes":[{"UsageData":{"Size":-1}}],"BuildCache":[{"Size":20,"Shared":true},{"Size":7,"Shared":false}]});
        let usage = disk_summary(&old).unwrap();
        assert_eq!(usage["images"]["bytes"], 130);
        assert_eq!(usage["containers"]["bytes"], 0);
        assert!(usage["volumes"]["bytes"].is_null());
        assert_eq!(usage["cache"]["bytes"], 7);
        let new = json!({"ImagesUsage":{"TotalSize":64,"TotalCount":2,"Reclaimable":15}, "LayersSize":999});
        assert_eq!(
            disk_summary(&new).unwrap()["images"],
            json!({"bytes":64,"count":2,"reclaimable":15})
        );
        assert!(disk_summary(&Value::Null).is_err());
    }
    #[test]
    fn journal_orders_lifecycle_events_and_omits_arbitrary_attributes() {
        let records = [
            json!({"Type":"container","Action":"die","timeNano":1000001,"Actor":{"ID":"a","Attributes":{"name":"db","exitCode":"3","label":"sensitive"}}}),
            json!({"Type":"container","Action":"exec_start: sensitive","time":3}),
            json!({"Type":"network","Action":"create","time":4}),
            json!({"Type":"container","Action":"restart","timeNano":1000002,"Actor":{"ID":"a"}}),
        ];
        let wire = records
            .iter()
            .map(Value::to_string)
            .collect::<Vec<_>>()
            .join("\n");
        let events = event_rows(wire.as_bytes()).unwrap();
        assert_eq!(events.len(), 2);
        assert_eq!(events[0]["action"], "restart");
        assert_eq!(events[1]["exitCode"], 3);
        assert!(!json!(events).to_string().contains("sensitive"));
        assert!(event_rows(b"").unwrap().is_empty());
    }
}
