//! Public AppCore prerequisite terminal proof on an owned, synthetic SSH alias.
//! The fixture's /usr/bin/sudo only prints a sentinel; apt never executes.
use cc_app_core::{AppConfig, AppCore, HostDto, TerminalChunk};
use cc_automation_core::SetupTarget;
use cc_models::host::HostKeyPolicy;
use cc_ssh_core::keys::{generate_key, KeyGenAlgorithm};
use secrecy::ExposeSecret;
use serde::Deserialize;
use serde_json::json;
use std::io::{self, BufRead, Write};
use std::time::Duration;
use uuid::Uuid;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Fixture {
    port: u16,
    public_key: String,
    login_alias: String,
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
    let key = generate_key(
        KeyGenAlgorithm::Ed25519,
        "own prerequisite alias fixture",
        None,
    )?;
    println!("{}", json!({"public_key":key.public_openssh}));
    io::stdout().flush()?;
    let mut line = String::new();
    io::stdin().lock().read_line(&mut line)?;
    let fixture: Fixture = serde_json::from_str(&line)?;
    assert_eq!(fixture.login_alias, "cc_login_alias");
    let directory = tempfile::tempdir()?;
    let app = AppCore::new(AppConfig::for_tests(
        directory.path().to_string_lossy().into_owned(),
    ))?;
    app.create_local_profile(
        "Own prerequisite terminal".into(),
        "synthetic prerequisite profile password 74".into(),
    )
    .await?;
    let credential = app
        .import_ssh_key(
            "Own SSH alias".into(),
            Some(fixture.login_alias.clone()),
            key.private_openssh.expose_secret().to_owned(),
            None,
            false,
            None,
        )
        .await?;
    let jump_id = if let Some(jump) = &fixture.jump {
        let mut host = HostDto::new("Own alias gateway", "127.0.0.1");
        host.port = Some(jump.port);
        host.username = Some(fixture.login_alias.clone());
        host.credential_id = Some(credential.id.clone());
        host.host_key_policy = HostKeyPolicy::Strict;
        app.import_known_hosts(format!(
            "[127.0.0.1]:{} {}\n",
            jump.port,
            jump.public_key.trim()
        ))
        .await?;
        Some(app.save_host(host).await?.id)
    } else {
        None
    };
    let address = fixture
        .jump
        .as_ref()
        .map_or("127.0.0.1", |jump| jump.runner_address.as_str());
    let mut host = HostDto::new("Own alias prerequisite target", address);
    host.port = Some(fixture.port);
    host.username = Some(fixture.login_alias.clone());
    host.credential_id = Some(credential.id);
    host.host_key_policy = HostKeyPolicy::Strict;
    if let Some(id) = jump_id {
        host.jump_chain = vec![id];
    }
    app.import_known_hosts(format!(
        "[{address}]:{} {}\n",
        fixture.port,
        fixture.public_key.trim()
    ))
    .await?;
    let host = app.save_host(host).await?;
    let target = SetupTarget::Remote {
        host_id: host.id.clone(),
    };
    let report = app
        .automation_runner_setup(Uuid::new_v4().to_string(), target.clone(), false)
        .await?;
    assert_eq!(report["user"], "runner");
    assert_ne!(report["user"], fixture.login_alias);
    assert_eq!(report["non_root"], true);
    assert!(report["missing"]
        .as_array()
        .unwrap()
        .iter()
        .any(|v| v == "bwrap"));
    let command = report["prerequisite_command"]
        .as_str()
        .expect("fixture must require prerequisites")
        .to_owned();
    assert!(command.contains("bubblewrap"));
    let id = app
        .automation_runner_prerequisite_terminal(
            Uuid::new_v4().to_string(),
            target.clone(),
            command,
        )
        .await?;
    Uuid::parse_str(&id)?;
    let info = app.terminal_info(id.clone()).await?;
    assert_eq!(info.host_id.as_deref(), Some(host.id.as_str()));
    let mut attached = app.attach_terminal(id.clone()).await?;
    let mut output = String::from_utf8_lossy(&attached.snapshot).into_owned();
    tokio::time::timeout(Duration::from_secs(20), async {
        loop {
            if output.contains("CC_PREREQUISITE_SUDO_MARKER /usr/bin/apt-get update")
                && output.contains(
                    "CC_PREREQUISITE_SUDO_MARKER /usr/bin/apt-get install --no-install-recommends",
                )
                && output.contains("Runner prerequisites: exit 0")
            {
                break;
            }
            match attached.output.recv().await {
                Some(TerminalChunk::Data(bytes)) => {
                    output.push_str(&String::from_utf8_lossy(&bytes))
                }
                other => panic!("fixture prerequisite terminal ended before sentinel: {other:?}"),
            }
            assert!(output.len() <= 64 * 1024);
        }
    })
    .await?;
    assert!(app.terminal_last_command(id.clone()).await?.is_none());
    // The typed text has no plaintext sudo password and cannot install anything.
    // Escapes ensure the expected marker is output by the shell, not its echo.
    app.terminal_write(
        id.clone(),
        b"printf '\\103\\103_INTERACTIVE_OK\\n'\r".to_vec(),
    )
    .await?;
    tokio::time::timeout(Duration::from_secs(10), async {
        while !output.contains("CC_INTERACTIVE_OK") {
            match attached.output.recv().await {
                Some(TerminalChunk::Data(bytes)) => {
                    output.push_str(&String::from_utf8_lossy(&bytes))
                }
                other => panic!("fixture interactive terminal ended: {other:?}"),
            }
            assert!(output.len() <= 64 * 1024);
        }
    })
    .await?;
    assert!(app.terminal_last_command(id.clone()).await?.is_none());
    let unchanged = app
        .automation_runner_setup(Uuid::new_v4().to_string(), target.clone(), false)
        .await?;
    assert!(
        unchanged["missing"]
            .as_array()
            .unwrap()
            .iter()
            .any(|v| v == "bwrap"),
        "fake sudo must never install bubblewrap"
    );
    let wrong = "sudo apt-get update && sudo apt-get install --no-install-recommends git";
    assert!(app
        .automation_runner_prerequisite_terminal(Uuid::new_v4().to_string(), target, wrong.into())
        .await
        .is_err());
    assert_eq!(app.list_terminals().await?.len(), 1);
    app.close_terminal(id).await?;
    app.shutdown().await?;
    println!(
        "{}",
        json!({"status":"PASS","public_prerequisite_terminal":true,"strict_host_pins":true,"ssh_jump":fixture.jump.is_some(),"login_alias_differs_from_effective_pwd_user":true,"marker_captured":true,"command_tracking":null,"system_packages_installed":false,"user_servers_modified":false})
    );
    Ok(())
}
