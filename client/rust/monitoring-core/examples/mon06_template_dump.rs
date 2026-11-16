//! Offline Native-template dump only. No HTTP client/profile/secret/endpoint.
//! Input is the staged native-requests.json; output is compared byte-for-value
//! against the statically extracted fixture expressions before promtool runs.
use cc_host_mapping::{HostBinding, Labels};
use cc_monitoring_core::panels::PanelKind;
use serde::Deserialize;
use serde_json::json;
use std::io::{self, Read};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Request {
    id: String,
    panel: PanelKind,
    instance: String,
    query_id: String,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut raw = Vec::new();
    io::stdin().lock().take(65537).read_to_end(&mut raw)?;
    if raw.len() > 65536 {
        return Err("fixture input limit".into());
    }
    let requests: Vec<Request> = serde_json::from_slice(&raw)?;
    if requests.is_empty() || requests.len() > 32 {
        return Err("fixture case limit".into());
    }
    let mut output = Vec::new();
    for request in requests {
        if request.id.len() > 80 || request.instance.len() > 100 || request.query_id.len() > 80 {
            return Err("fixture field limit".into());
        }
        let binding = HostBinding::new(
            "5e7c5182-19df-48e1-99ed-87d28c9356e9",
            "owned-mon06-fixture",
            1,
            Labels::from([
                ("instance".into(), request.instance),
                ("job".into(), "owned".into()),
            ]),
        )?;
        let query = request
            .panel
            .query_templates(&binding)?
            .into_iter()
            .find(|query| query.id == request.query_id)
            .ok_or("requested Native template does not exist")?;
        output.push(json!({"caseId": request.id, "query": query}));
    }
    println!("{}", serde_json::to_string_pretty(&output)?);
    Ok(())
}
