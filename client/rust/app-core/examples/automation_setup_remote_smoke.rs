//! Owned disposable fixture only. No user profile, SSH keys or system changes.
use cc_app_core::automation_runners::AutomationRunnerConfig;
use cc_app_core::{AppConfig, AppCore, HostDto};
use cc_automation_core::{RemoteHelperConfig, SetupTarget};
use cc_models::host::HostKeyPolicy;
use cc_models::{
    credential::{Credential, CredentialKind},
    host::Host,
};
use cc_ssh_core::keys::{KeyGenAlgorithm, generate_key};
use secrecy::ExposeSecret;
use serde::Deserialize;
use serde_json::json;
use std::io::{self, BufRead, Write};
use std::{sync::Arc, time::Duration};
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
    let key = generate_key(KeyGenAlgorithm::Ed25519, "disposable remote setup", None)?;
    println!("{}", json!({"public_key":key.public_openssh}));
    io::stdout().flush()?;
    let mut line = String::new();
    io::stdin().lock().read_line(&mut line)?;
    let fixture: Fixture = serde_json::from_str(&line)?;
    let temporary = tempfile::tempdir()?;
    let app = AppCore::new(AppConfig::for_tests(
        temporary.path().to_string_lossy().into_owned(),
    ))?;
    app.create_local_profile(
        "Disposable remote setup".into(),
        "synthetic fixture passphrase 77".into(),
    )
    .await?;
    let credential = app
        .import_ssh_key(
            "Fixture setup".into(),
            Some("runner".into()),
            key.private_openssh.expose_secret().to_owned(),
            None,
            false,
            None,
        )
        .await?;
    let jump_id = if let Some(jump) = &fixture.jump {
        let mut host = HostDto::new("Fixture setup gateway", "127.0.0.1");
        host.port = Some(jump.port);
        host.username = Some("runner".into());
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
    let mut host = HostDto::new("Fixture setup Runner", address);
    host.port = Some(fixture.port);
    host.username = Some("runner".into());
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
    let full = !std::env::args().any(|arg| arg == "--transport-only");
    if full {
        let target = SetupTarget::Remote {
            host_id: host.id.clone(),
        };
        let report = app
            .automation_runner_setup(Uuid::new_v4().to_string(), target.clone(), false)
            .await?;
        assert_eq!(report["system"], "Linux");
        assert_eq!(report["user"], "runner");
        assert_eq!(report["non_root"], true);
        assert_eq!(report["candidates"], json!([]));
        assert_eq!(report["install_supported"], true, "{report}");
        println!("{}", json!({"phase":"scan_complete"}));
        io::stdout().flush()?;
        let unchanged = app
            .exec(
                host.id.clone(),
                "/usr/bin/test ! -e /home/runner/.local/share/consolecrypt/runner-environments"
                    .into(),
            )
            .await?;
        assert_eq!(
            unchanged.exit_status,
            Some(0),
            "scan must not create an environment"
        );
        let installed = app
            .automation_runner_setup(Uuid::new_v4().to_string(), target.clone(), true)
            .await?;
        let helper: RemoteHelperConfig = serde_json::from_value(installed["paths"].clone())?;
        println!("{}", json!({"phase":"install_complete"}));
        io::stdout().flush()?;
        assert!(
            helper
                .python
                .starts_with("/home/runner/.local/share/consolecrypt/runner-environments/")
        );
        let found = app
            .automation_runner_setup(Uuid::new_v4().to_string(), target.clone(), false)
            .await?;
        assert_eq!(found["candidates"].as_array().unwrap().len(), 1);
        assert_eq!(found["candidates"][0], installed["paths"]);
        println!("{}", json!({"phase":"rediscovery_complete"}));
        io::stdout().flush()?;
        let runner = app
            .automation_runner_create(
                "Installed SSH Runner".into(),
                AutomationRunnerConfig::Remote {
                    host_id: host.id.clone(),
                    helper,
                },
            )
            .await?;
        let checked = app.automation_runner_connect(runner.id.clone()).await?;
        assert_eq!(checked["probe"]["dependencies_ready"], true);
        assert_eq!(checked["probe"]["sandbox"]["network"], true);
        println!("{}", json!({"phase":"helper_connection_complete"}));
        io::stdout().flush()?;
        app.automation_runner_disconnect(runner.id).await?;
        let cancelled = Uuid::new_v4().to_string();
        app.automation_runner_cancel_setup(cancelled.clone())
            .await?;
        assert!(
            app.automation_runner_setup(cancelled, target, false)
                .await
                .is_err()
        );
        let operation = Uuid::new_v4().to_string();
        let target = SetupTarget::Remote {
            host_id: host.id.clone(),
        };
        let (cancelled_result, partial) = tokio::join!(
            app.automation_runner_setup(operation.clone(), target.clone(), true),
            async {
                let mut started = None;
                for _ in 0..40 {
                    let listing=app.exec(host.id.clone(),
                    "/usr/bin/find /home/runner/.local/share/consolecrypt/runner-environments -mindepth 1 -maxdepth 1 -type d".into()).await?;
                    let text = String::from_utf8(listing.stdout)?;
                    let first =
                        std::path::Path::new(installed["paths"]["python"].as_str().unwrap())
                            .parent()
                            .unwrap()
                            .parent()
                            .unwrap()
                            .parent()
                            .unwrap()
                            .to_string_lossy()
                            .into_owned();
                    if let Some(extra) = text.lines().find(|path| *path != first) {
                        started = Some(extra.to_owned());
                        break;
                    }
                    tokio::time::sleep(Duration::from_millis(100)).await;
                }
                app.automation_runner_cancel_setup(operation.clone())
                    .await?;
                Ok::<_, Box<dyn std::error::Error>>(started.expect("second install never started"))
            }
        );
        assert!(matches!(
            cancelled_result,
            Err(cc_app_core::AppError::Cancelled)
        ));
        let partial = partial?;
        tokio::time::sleep(Duration::from_secs(2)).await;
        let processes = app
            .exec(host.id.clone(), "/usr/bin/ps -u runner -o args=".into())
            .await?;
        assert!(
            !String::from_utf8(processes.stdout)?.contains(&partial),
            "cancel left an installation child running"
        );
        let after_cancel = app
            .automation_runner_setup(Uuid::new_v4().to_string(), target, false)
            .await?;
        assert_eq!(
            after_cancel["candidates"].as_array().unwrap().len(),
            1,
            "partial setup became a ready environment"
        );
        assert_eq!(
            after_cancel["candidates"][0], installed["paths"],
            "the previous Runner was replaced"
        );
    }
    // Exercise private stdin and the PTY launcher without apt/sudo or user data.
    let (port, pin) = fixture
        .jump
        .as_ref()
        .map_or((fixture.port, fixture.public_key.as_str()), |jump| {
            (jump.port, jump.public_key.as_str())
        });
    let inventory = Arc::new(cc_ssh_core::MemoryInventory::new());
    let credential = Credential::new("Synthetic transport", CredentialKind::SshPrivateKey);
    let resolver = Arc::new(cc_ssh_core::MemoryCredentialResolver::new());
    resolver.insert(
        credential.id,
        cc_ssh_core::ResolvedCredential::PrivateKey {
            openssh: key.private_openssh,
            passphrase: None,
            certificate: None,
        },
    );
    let mut host = Host::new("Owned transport proof", "127.0.0.1");
    host.port = Some(port);
    host.username = Some("runner".into());
    host.credential_id = Some(credential.id);
    host.host_key_policy = HostKeyPolicy::Strict;
    inventory.add_credential(credential);
    let id = inventory.add_host(host);
    let plan = cc_ssh_core::ConnectionPlanner::new(inventory)
        .plan(id)
        .await?;
    let (entries, errors) = cc_ssh_core::known_hosts::import_known_hosts(&format!(
        "[127.0.0.1]:{port} {}\n",
        pin.trim()
    ));
    assert!(errors.is_empty());
    let connector = cc_ssh_core::SshConnector::new(
        resolver,
        Arc::new(cc_ssh_core::MemoryKnownHosts::with_entries(entries)),
        Arc::new(cc_ssh_core::FixedHostKeyPrompt::new(
            cc_ssh_core::HostKeyDecision::Reject,
        )),
    );
    let remote = connector.connect(&plan).await?;
    let output=remote.exec_with_private_input(
        "exec /usr/bin/python3.12 -I -c 'import sys;print(\"CC_STDIN_\"+str(len(sys.stdin.buffer.readline())))'",
        b"fixture\n",65536).await?;
    assert_eq!(output.exit_status, Some(0));
    assert_eq!(output.stdout, b"CC_STDIN_8\n");
    assert!(
        remote
            .exec_with_private_input(
                "exec /usr/bin/python3.12 -I -c 'print(\"x\"*65537)'",
                b"",
                65536
            )
            .await
            .is_err()
    );
    let mut pty=remote.open_command_pty(cc_ssh_core::PtyRequest::default(),
        r#"exec /bin/sh -c 'stty -echo; printf "CC_PROMPT\n"; IFS= read -r cc_fixture; printf "CC_LENGTH_%s\n" "${#cc_fixture}"'"#).await?;
    tokio::time::timeout(Duration::from_secs(20), async {
        let mut output = String::new();
        let mut sent = false;
        let mut status = None;
        while let Some(event) = pty.reader.recv().await {
            match event {
                cc_ssh_core::ShellEvent::Data(bytes) => {
                    output.push_str(&String::from_utf8_lossy(&bytes));
                    if output.contains("CC_PROMPT") && !sent {
                        pty.writer.write(b"fixture-hidden\r".to_vec()).await?;
                        sent = true;
                    }
                }
                cc_ssh_core::ShellEvent::ExitStatus(code) => status = Some(code),
                cc_ssh_core::ShellEvent::Closed => break,
                _ => {}
            }
        }
        assert!(sent && output.contains("CC_LENGTH_14") && !output.contains("fixture-hidden"));
        assert_eq!(status, Some(0));
        Ok::<_, cc_ssh_core::SshError>(())
    })
    .await??;
    remote.disconnect().await?;
    println!(
        "{}",
        json!({"status":"PASS","read_only_scan":full,"managed_install":full,
        "discovery_and_connection":full,"host_key_policy":"strict","ssh_jump":fixture.jump.is_some(),
        "cancel_tombstone":full,"bounded_private_input":true,"private_ssh_pty":true,"user_servers_modified":false})
    );
    Ok(())
}
