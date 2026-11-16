//! Own fixture only: public AppCore -> pinned native SSH -> scan + cleanup_scan.
//! No installation, deletion, jobs or existing user profile/credential material.
use cc_app_core::{AppConfig, AppCore, HostDto};
use cc_automation_core::SetupTarget;
use cc_models::host::HostKeyPolicy;
use cc_ssh_core::keys::{generate_key, KeyGenAlgorithm};
use secrecy::ExposeSecret;
use serde::Deserialize;
use serde_json::json;
use std::io::{self, BufRead, Read, Write};
use uuid::Uuid;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Fixture {
    port: u16,
    public_key: String,
    jump: Option<Jump>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Jump {
    port: u16,
    public_key: String,
    runner_address: String,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let key = generate_key(KeyGenAlgorithm::Ed25519, "own readonly Runner proof", None)?;
    println!("{}", json!({"public_key":key.public_openssh}));
    io::stdout().flush()?;
    let mut input = String::new();
    io::stdin().lock().take(16 * 1024).read_line(&mut input)?;
    let fixture: Fixture = serde_json::from_str(&input)?;
    assert!(fixture.port > 0 && fixture.public_key.len() < 8192);
    let directory = tempfile::tempdir()?;
    let app = AppCore::new(AppConfig::for_tests(
        directory.path().to_string_lossy().into_owned(),
    ))?;
    app.create_local_profile(
        "Own readonly Runner fixture".into(),
        "own readonly fixture passphrase 42".into(),
    )
    .await?;
    let credential = app
        .import_ssh_key(
            "Own ephemeral key".into(),
            Some("runner".into()),
            key.private_openssh.expose_secret().to_owned(),
            None,
            false,
            None,
        )
        .await?;
    let jump = if let Some(jump) = &fixture.jump {
        assert!(jump.port > 0 && jump.public_key.len() < 8192 && jump.runner_address.len() < 254);
        app.import_known_hosts(format!(
            "[127.0.0.1]:{} {}\n",
            jump.port,
            jump.public_key.trim()
        ))
        .await?;
        let mut host = HostDto::new("Own gateway", "127.0.0.1");
        host.port = Some(jump.port);
        host.username = Some("runner".into());
        host.credential_id = Some(credential.id.clone());
        host.host_key_policy = HostKeyPolicy::Strict;
        Some(app.save_host(host).await?.id)
    } else {
        None
    };
    let address = fixture
        .jump
        .as_ref()
        .map_or("127.0.0.1", |jump| jump.runner_address.as_str());
    app.import_known_hosts(format!(
        "[{address}]:{} {}\n",
        fixture.port,
        fixture.public_key.trim()
    ))
    .await?;
    let mut host = HostDto::new("Own readonly Runner", address);
    host.port = Some(fixture.port);
    host.username = Some("runner".into());
    host.credential_id = Some(credential.id);
    host.host_key_policy = HostKeyPolicy::Strict;
    if let Some(jump) = jump {
        host.jump_chain = vec![jump]
    }
    let host = app.save_host(host).await?;
    let target = SetupTarget::Remote { host_id: host.id };
    let scan = app
        .automation_runner_setup(Uuid::new_v4().to_string(), target.clone(), false)
        .await?;
    assert_eq!(scan["version"], 1);
    assert_eq!(scan["system"], "Linux");
    assert_eq!(scan["user"], "runner");
    assert_eq!(scan["non_root"], true);
    assert_eq!(scan["candidates"], json!([]));
    // Missing packages/namespaces are an honest report, not a generic PyPI
    // failure and not proof this particular image can install a Runner.
    println!(
        "{}",
        json!({"phase":"remote_scan","ok":true,"candidates":0,"missing":scan["missing"],"installAttempted":false})
    );
    let cleanup = app
        .automation_runner_cleanup_scan(
            json!({"operationId":Uuid::new_v4(),"target":target,"selectedPython":null}).to_string(),
        )
        .await?;
    assert_eq!(cleanup["version"], 1);
    assert_eq!(cleanup["identity"], serde_json::Value::Null);
    assert_eq!(cleanup["candidates"], json!([]));
    assert_eq!(cleanup["currentProfileChecked"], true);
    assert_eq!(cleanup["otherProfilesChecked"], false);
    println!(
        "{}",
        json!({"phase":"remote_cleanup_scan","ok":true,"candidates":0,"deleteAttempted":false})
    );
    app.shutdown().await?;
    println!(
        "{}",
        json!({"phase":"readonly_complete","ok":true,"strictHostKey":true,"privateEphemeralProfile":true,
        "jump":fixture.jump.is_some(),"userEnvironmentTouched":false})
    );
    Ok(())
}
