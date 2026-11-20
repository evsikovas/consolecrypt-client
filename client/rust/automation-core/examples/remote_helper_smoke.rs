//! Real SSH acceptance fixture. Private fixture key stays in this process.
use cc_automation_core::{ProjectRoot, RemoteHelperConfig};
use cc_models::{
    credential::{Credential, CredentialKind},
    host::{Host, HostKeyPolicy},
};
use cc_ssh_core::keys::{generate_key, KeyGenAlgorithm};
use cc_ssh_core::{
    ConnectionPlanner, FixedHostKeyPrompt, HostKeyDecision, MemoryCredentialResolver,
    MemoryInventory, MemoryKnownHosts, ResolvedCredential, SshConnector,
};
use serde::Deserialize;
use serde_json::json;
use std::{
    io::{self, BufRead, Write},
    sync::Arc,
    time::Duration,
};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Fixture {
    port: u16,
    host_public_key: String,
    project_path: std::path::PathBuf,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let key = generate_key(
        KeyGenAlgorithm::Ed25519,
        "cc-automation-disposable-fixture",
        None,
    )?;
    println!("{}", json!({"public_key": key.public_openssh}));
    io::stdout().flush()?;
    let mut line = String::new();
    io::stdin().lock().read_line(&mut line)?;
    let fixture: Fixture = serde_json::from_str(&line)?;
    let inventory = Arc::new(MemoryInventory::new());
    let credential = Credential::new("disposable fixture", CredentialKind::SshPrivateKey);
    let resolver = Arc::new(MemoryCredentialResolver::new());
    resolver.insert(
        credential.id,
        ResolvedCredential::PrivateKey {
            openssh: key.private_openssh,
            passphrase: None,
            certificate: None,
        },
    );
    let mut host = Host::new("disposable Ubuntu24 Runner", "127.0.0.1");
    host.username = Some("runner".into());
    host.port = Some(fixture.port);
    host.credential_id = Some(credential.id);
    host.host_key_policy = HostKeyPolicy::Strict;
    inventory.add_credential(credential);
    let host_id = inventory.add_host(host);
    let plan = ConnectionPlanner::new(inventory).plan(host_id).await?;
    let prompt = Arc::new(FixedHostKeyPrompt::new(HostKeyDecision::Reject));
    let untrusted = SshConnector::new(
        resolver.clone(),
        Arc::new(MemoryKnownHosts::new()),
        prompt.clone(),
    );
    assert!(
        untrusted.connect(&plan).await.is_err(),
        "unknown host key must fail"
    );
    let (entries, errors) = cc_ssh_core::known_hosts::import_known_hosts(&format!(
        "[127.0.0.1]:{} {}\n",
        fixture.port,
        fixture.host_public_key.trim()
    ));
    assert!(errors.is_empty() && entries.len() == 1);
    let connector = SshConnector::new(
        resolver,
        Arc::new(MemoryKnownHosts::with_entries(entries)),
        prompt,
    );
    let session = connector.connect(&plan).await?;
    let peer = RemoteHelperConfig {
        python: "/opt/cc-runtime/bin/python".into(),
        module_dir: "/opt/consolecrypt/src".into(),
        root: "/home/runner/helper state".into(),
    }
    .connect(&session)
    .await?;
    let probe = peer
        .request("probe", json!({}), Duration::from_secs(30))
        .await?;
    assert_eq!(probe["non_root"], true);
    assert_eq!(probe["dependencies_ready"], true, "{probe}");
    assert_eq!(probe["sandbox"]["network"], true, "{probe}");
    let project = ProjectRoot::open(&fixture.project_path)?;
    let revision = project.revision()?;
    let capture = project.upload(&revision, &peer).await?;
    let validation = peer
        .request(
            "validate",
            json!({"revision_id": capture.revision_id,
        "playbook": "playbook.yml"}),
            Duration::from_secs(120),
        )
        .await?;
    assert_eq!(validation["valid"], true, "{validation}");
    assert_eq!(validation["revision_digest"], capture.digest);
    peer.close().await;
    session.disconnect().await?;
    println!(
        "{}",
        json!({"status": "PASS", "transport": "native cc-ssh-core, strict pinned host key",
        "system": probe["system"], "python": probe["python"], "revision": capture.digest,
        "validation": validation["valid"]})
    );
    Ok(())
}
