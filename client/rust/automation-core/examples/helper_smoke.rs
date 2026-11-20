//! Explicit opt-in real helper check; never skipped or run against user hosts.
use cc_automation_core::HelperLaunch;
use serde_json::{json, Value};
use std::time::Duration;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = std::env::args().nth(1).ok_or("pass a launch JSON file")?;
    let source = std::env::args()
        .nth(2)
        .ok_or("pass an isolated source project as seen by helper")?;
    let launch: HelperLaunch = serde_json::from_slice(&std::fs::read(path)?)?;
    let process = launch.spawn().await?;
    let result = async {
        let info = process
            .peer
            .request("probe", json!({}), Duration::from_secs(15))
            .await?;
        if info["capabilities"]["validate"] != true || info["non_root"] != true {
            return Err::<(), Box<dyn std::error::Error>>(
                "helper cannot validate in isolation".into(),
            );
        }
        let snapshot = process
            .peer
            .request(
                "capture",
                json!({"source": source}),
                Duration::from_secs(30),
            )
            .await?;
        let revision = snapshot["revision_id"].as_str().ok_or("missing revision")?;
        let report: Value = process
            .peer
            .request(
                "validate",
                json!({"revision_id": revision, "playbook": "playbook.yml"}),
                Duration::from_secs(150),
            )
            .await?;
        if report["valid"] != true || report["revision_digest"] != snapshot["digest"] {
            return Err("real helper validation failed".into());
        }
        println!(
            "{}",
            json!({"result": "PASS", "private_native_stdio": true,
            "snapshot_digest_bound": true, "isolated_real_ansible": true})
        );
        Ok(())
    }
    .await;
    process.close().await;
    result
}
