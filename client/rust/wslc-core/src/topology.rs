//! Relations between containers, images, networks and volumes for the map view.
//! All containers are inspected with one `container inspect id…` call per 100 IDs.
use crate::models::*;
use serde_json::Value;

pub const BATCH: usize = 100;

/// Inspect output for several containers: a JSON array of objects.
pub fn inspect_many(text: &str) -> AppResult<Vec<Value>> {
    let invalid = || {
        AppError::new(
            "invalid_output",
            "topology",
            "CLI вернул повреждённый Inspect",
        )
    };
    let v: Value = serde_json::from_str(text).map_err(|_| invalid())?;
    let items = v.as_array().ok_or_else(invalid)?;
    if items.iter().any(|i| !i.is_object()) {
        return Err(invalid());
    }
    Ok(items.clone())
}

pub fn links(inspect: &Value) -> Option<ContainerLinks> {
    let text = |v: &Value| v.as_str().filter(|s| !s.is_empty()).map(str::to_owned);
    let id = text(&inspect["Id"])?;
    let mut volumes = Vec::new();
    let mut binds = Vec::new();
    for m in inspect["Mounts"].as_array().into_iter().flatten() {
        match m["Type"].as_str() {
            Some("volume") => volumes.extend(text(&m["Name"]).or_else(|| text(&m["Source"]))),
            Some("bind") => binds.extend(text(&m["Source"])),
            _ => {}
        }
    }
    let networks = inspect["NetworkSettings"]["Networks"]
        .as_object()
        .map(|n| n.keys().cloned().collect())
        .unwrap_or_default();
    let raw_state = text(&inspect["State"]["Status"]);
    Some(ContainerLinks {
        state: ContainerState::from_raw(raw_state.as_deref()),
        name: text(&inspect["Name"]).map(|n| n.trim_start_matches('/').to_owned()),
        image_id: text(&inspect["Image"]),
        image_name: text(&inspect["Config"]["Image"]),
        networks,
        volumes,
        binds,
        id,
    })
}

/// Containers whose Inspect lacks an ID are skipped rather than failing the map.
pub fn parse(text: &str) -> AppResult<Vec<ContainerLinks>> {
    Ok(inspect_many(text)?.iter().filter_map(links).collect())
}
