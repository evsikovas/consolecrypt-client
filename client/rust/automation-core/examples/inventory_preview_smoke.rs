//! Real Windows project upload -> WSL namespace normalization -> native proof.
use cc_automation_core::{
    CapturedInventory, HelperLaunch, InventoryArtifact, InventoryFormat, InventorySource,
    ProjectRoot,
};
use serde::Deserialize;
use serde_json::json;
use sha2::{Digest, Sha256};
use std::{
    io::{self, BufRead},
    time::Duration,
};
use tokio_util::sync::CancellationToken;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Fixture {
    launch: HelperLaunch,
}
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut line = String::new();
    io::stdin().lock().read_line(&mut line)?;
    if line.len() > 128 * 1024 {
        return Err("fixture exceeds limit".into());
    }
    let fixture: Fixture = serde_json::from_str(&line)?;
    let folder = tempfile::tempdir()?;
    let defaults = b"[all:vars]\nansible_user=deployer\nansible_port=2222\n";
    let hosts = b"\xef\xbb\xbfweb:\n  hosts:\n    service_a: {ansible_host: 192.0.2.10}\n";
    std::fs::write(folder.path().join("defaults.ini"), defaults)?;
    std::fs::create_dir_all(folder.path().join("inventory/group_vars"))?;
    std::fs::create_dir_all(folder.path().join("inventory/host_vars"))?;
    std::fs::write(folder.path().join("inventory/hosts.yml"), hosts)?;
    std::fs::write(
        folder.path().join("inventory/group_vars/web.yml"),
        "ansible_user: deployer\nansible_port: 2222\nenabled: true\n",
    )?;
    std::fs::write(
        folder.path().join("inventory/host_vars/service_a.yml"),
        "threshold: 0.0000001\n",
    )?;
    let root = ProjectRoot::open(folder.path())?;
    let revision = root.revision()?;
    let helper = fixture.launch.spawn().await?;
    let result = async {
        let uploaded = root.upload(&revision, &helper.peer).await?;
        let sources = vec![
            InventorySource {
                path: "defaults.ini".into(),
                format: InventoryFormat::Ini,
            },
            InventorySource {
                path: "inventory".into(),
                format: InventoryFormat::Directory,
            },
        ];
        let inventory = CapturedInventory::fetch(
            &helper.peer,
            &uploaded,
            &revision,
            &sources,
            &CancellationToken::new(),
        )
        .await?;
        assert_eq!(inventory.inventory.hosts.len(), 1);
        let host = &inventory.inventory.hosts[0];
        assert_eq!(host.alias, "service_a");
        assert_eq!(host.address, "192.0.2.10");
        assert_eq!(host.port, 2222);
        assert_eq!(host.username.as_deref(), Some("deployer"));
        assert!(host.host_id.is_none());
        assert!(host.credential_refs.is_empty());
        assert_eq!(
            inventory.inventory.source_digests["inventory/hosts.yml"],
            hex::encode(Sha256::digest(hosts))
        );
        assert_eq!(inventory.sources.len(), 4);
        assert_eq!(
            inventory.inventory.groups["web"].vars["enabled"],
            json!(true)
        );
        assert_eq!(host.variables["threshold"], json!(1e-7));
        let mut expected = serde_json::to_value(&inventory.inventory)?;
        expected["hosts"][0]["variables"] =
            json!({"large_value":"x".repeat(1200000),"threshold":1e-7});
        let artifact = InventoryArtifact::store(
            &helper.peer,
            &uploaded,
            &expected,
            &CancellationToken::new(),
        )
        .await?;
        assert!(artifact.witness.canonical_json.len() > 1024 * 1024);
        assert!(
            artifact.witness.verify(&expected).is_err(),
            "the general frame limit must remain unchanged"
        );
        artifact.witness.verify_inventory(&expected)?;
        assert!(!format!("{artifact:?}").contains("large_value"));
        let cancel = CancellationToken::new();
        cancel.cancel();
        assert!(
            CapturedInventory::fetch(&helper.peer, &uploaded, &revision, &sources, &cancel)
                .await
                .is_err()
        );
        assert!(
            InventoryArtifact::store(&helper.peer, &uploaded, &expected, &cancel)
                .await
                .is_err()
        );
        helper
            .peer
            .request(
                "revision_release",
                json!({"revision_id":uploaded.revision_id}),
                Duration::from_secs(5),
            )
            .await?;
        Ok::<_, Box<dyn std::error::Error>>(())
    }
    .await;
    helper.close().await;
    result?;
    println!(
        "{}",
        json!({"status":"PASS","native_windows_inventory_preview":true,"independent_digest":true,
        "captured_source_hashes":true,"resolved_endpoints":true,"source_has_no_authority":true,"cancelled_request_denied":true})
    );
    Ok(())
}
