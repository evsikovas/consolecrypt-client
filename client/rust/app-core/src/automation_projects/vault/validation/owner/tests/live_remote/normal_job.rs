//! Genuine public preparation/signing/default execution, never a fabricated
//! approval/report/peer. Requires the owned two-SSH-node coordinator.
use super::*;
use crate::automation_prepare::AutomationRunRequest;
use cc_models::automation::JobMode;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct JobPins {
    version: u8,
    token: String,
    ssh_port: u16,
    host_public_key: String,
    target_address: String,
    target_port: u16,
    target_public_key: String,
    egress_blocked: bool,
}
async fn terminal_job(app: &AppCore, id: &str) -> Value {
    tokio::time::timeout(Duration::from_secs(90), async {
        loop {
            let catalog = app.automation_jobs().await.unwrap();
            let row = catalog["jobs"]
                .as_array()
                .unwrap()
                .iter()
                .find(|r| r["job_id"] == id)
                .unwrap()
                .clone();
            if !matches!(
                row["state"].as_str(),
                Some("queued" | "dispatching" | "running" | "cancelling")
            ) {
                return row;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    })
    .await
    .expect("owned normal job terminal deadline")
}
async fn request(
    app: &AppCore,
    project: &str,
    runner: &str,
    target: &str,
    credential: &str,
    on_lock: bool,
) -> AutomationRunRequest {
    let context: Value = serde_json::from_str(
        &app.automation_project_vault_context_json(project.into())
            .await
            .unwrap(),
    )
    .unwrap();
    serde_json::from_value(
        json!({"project_id":project,"runner_id":runner,"playbook":"playbook.yml",
        "host_ids":[target],"mode":"apply","timeout_seconds":60,"forks":1,"cancel_on_lock":on_lock,
        "parameters":{"extra_vars":{},"tags":[],"skip_tags":[],"diff":false},
        "vault_credential_id":credential,"vault_selection":context["frame"]}),
    )
    .unwrap()
}
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires owned pinned Runner plus separate SSH target and private synthetic stdin"]
async fn normal_signed_vault_job_positive_wrong_password_cancel_and_lock_policies() {
    let token = std::env::var("CC_VAULT_JOB_TOKEN").unwrap();
    let parsed = Uuid::parse_str(&token).unwrap();
    assert!(!parsed.is_nil() && parsed.to_string() == token);
    let root = PathBuf::from(std::env::var_os("CC_VAULT_JOB_ROOT").unwrap());
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(4)
        .unwrap();
    let allowed = workspace
        .join("Tasks/vault-job/control")
        .canonicalize()
        .unwrap();
    assert_eq!(
        root.canonicalize().unwrap().parent(),
        Some(allowed.as_path())
    );
    assert_eq!(
        root.file_name().unwrap().to_str().unwrap(),
        format!("owned-{token}")
    );
    assert!(!std::fs::symlink_metadata(&root)
        .unwrap()
        .file_type()
        .is_symlink());
    let keys = generate_key(
        KeyGenAlgorithm::Ed25519,
        "owned normal Vault job fixture",
        None,
    )
    .unwrap();
    publish(
        &root,
        "native-public.json",
        json!({"version":1,"token":token,"public_key":keys.public_openssh}),
    );
    let mut raw = Zeroizing::new(Vec::new());
    std::io::stdin().take(16385).read_to_end(&mut raw).unwrap();
    assert!(raw.len() <= 16384);
    let setup: Setup = serde_json::from_slice(&raw).expect("invalid private synthetic setup");
    assert!(
        setup.version == 1
            && setup.token == token
            && setup.password.len() >= 32
            && setup.password.len() <= 128
    );
    assert!(
        setup.ciphertext.starts_with("$ANSIBLE_VAULT;1.1;AES256\n")
            && setup.ciphertext.len() < 8192
    );
    let password = SecretString::from(setup.password);
    drop(raw);
    let pins: JobPins = serde_json::from_value(bounded_json(&root, "fixture.json").await).unwrap();
    assert!(
        pins.version == 1 && pins.token == token && pins.ssh_port > 0 && pins.target_port == 2222
    );
    assert!(pins
        .target_address
        .parse::<std::net::Ipv4Addr>()
        .unwrap()
        .is_private());
    for key in [&pins.host_public_key, &pins.target_public_key] {
        assert!(
            key.starts_with("ssh-ed25519 ")
                && key.len() < 8192
                && !key.chars().any(char::is_control)
        );
    }
    let profile = tempfile::Builder::new()
        .prefix("native-profile-")
        .tempdir_in(&root)
        .unwrap();
    let app = AppCore::new(crate::AppConfig::for_tests(
        profile.path().to_string_lossy().into_owned(),
    ))
    .unwrap();
    app.create_local_profile(
        "Owned normal Vault job".into(),
        "synthetic profile password42".into(),
    )
    .await
    .unwrap();
    let ssh = app
        .import_ssh_key(
            "Own fixture ephemeral key".into(),
            Some("runner".into()),
            keys.private_openssh.expose_secret().to_owned(),
            None,
            false,
            None,
        )
        .await
        .unwrap();
    let mut hosts = Vec::new();
    for (address, port, key) in [
        ("127.0.0.1", pins.ssh_port, pins.host_public_key.as_str()),
        (
            pins.target_address.as_str(),
            pins.target_port,
            pins.target_public_key.as_str(),
        ),
    ] {
        app.import_known_hosts(format!("[{address}]:{port} {key}\n"))
            .await
            .unwrap();
        let mut host = HostDto::new("Owned normal Vault node", address);
        host.port = Some(port);
        host.username = Some("runner".into());
        host.credential_id = Some(ssh.id.clone());
        host.host_key_policy = HostKeyPolicy::Strict;
        hosts.push(app.save_host(host).await.unwrap());
    }
    let runner = app
        .automation_runner_create(
            "Owned normal Vault Runner".into(),
            AutomationRunnerConfig::Remote {
                host_id: hosts[0].id.clone(),
                helper: RemoteHelperConfig {
                    python: format!("/home/runner/proof-{token}/python"),
                    module_dir: "/opt/consolecrypt/src".into(),
                    root: format!("/home/runner/proof-{token}/jobs"),
                },
            },
        )
        .await
        .unwrap();
    let probe = app
        .automation_runner_connect(runner.id.clone())
        .await
        .unwrap();
    assert_eq!(probe["probe"]["capabilities"]["vault_job_v1"], true);
    let credential = app
        .add_password_credential(
            "Explicit normal-job Vault password".into(),
            None,
            password.expose_secret().to_owned(),
        )
        .await
        .unwrap();
    let project = app
        .automation_project_create("Owned normal Vault source".into(), None)
        .await
        .unwrap();
    let encrypted = setup
        .ciphertext
        .lines()
        .map(|line| format!("      {line}\n"))
        .collect::<String>();
    let text=format!("---\n- name: Owned signed Vault job\n  hosts: \"{{{{ selected_group }}}}\"\n  gather_facts: false\n  vars:\n    selected_group: !vault |\n{encrypted}  tasks:\n    - name: Execute only on the approved separate target\n      ansible.builtin.command:\n        argv: [/bin/true]\n      changed_when: false\n");
    let current = app
        .automation_project_file(project.id.clone(), "playbook.yml".into())
        .await
        .unwrap();
    app.automation_project_save(
        project.id.clone(),
        "playbook.yml".into(),
        Some(current.sha256),
        text,
    )
    .await
    .unwrap();
    app.automation_project_save_inventory(
        project.id.clone(),
        1,
        json!({"version":2,"groups":{"fixture":[hosts[1].id]},"selected_hosts":[hosts[1].id]})
            .to_string(),
    )
    .await
    .unwrap();
    let context: Value = serde_json::from_str(
        &app.automation_project_vault_context_json(project.id.clone())
            .await
            .unwrap(),
    )
    .unwrap();
    app.automation_project_vault_select_json(
        json!({"frame":context["frame"],"credentialId":credential.id}).to_string(),
    )
    .await
    .unwrap();
    let source = app
        .automation_project_files(project.id.clone())
        .await
        .unwrap();
    // Incorrect secret must run real default checks, produce safe diagnostics,
    // and never create a signed job. A fresh correct selection follows.
    app.set_credential_password(
        credential.id.clone(),
        "synthetic incorrect password unavailable".into(),
    )
    .await
    .unwrap();
    let wrong = app
        .automation_prepare_run(
            request(
                &app,
                &project.id,
                &runner.id,
                &hosts[1].id,
                &credential.id,
                true,
            )
            .await,
        )
        .await
        .unwrap();
    assert_eq!(wrong["validation"]["valid"], false);
    assert!(!serde_json::to_string(&wrong)
        .unwrap()
        .contains(password.expose_secret()));
    assert!(app
        .automation_confirm_run(wrong["review_id"].as_str().unwrap().into())
        .await
        .is_err());
    assert!(app.automation_jobs().await.unwrap()["jobs"]
        .as_array()
        .unwrap()
        .is_empty());
    app.set_credential_password(credential.id.clone(), password.expose_secret().to_owned())
        .await
        .unwrap();
    for case in [
        "positive",
        "cancel_on_lock",
        "continue_on_lock",
        "explicit_cancel",
    ] {
        let prepared = app
            .automation_prepare_run(
                request(
                    &app,
                    &project.id,
                    &runner.id,
                    &hosts[1].id,
                    &credential.id,
                    case != "continue_on_lock",
                )
                .await,
            )
            .await
            .unwrap();
        assert_eq!(prepared["validation"]["valid"], true);
        assert!(prepared["validation"]["checks"]
            .as_array()
            .unwrap()
            .iter()
            .all(|c| c["output"] == "Vault-sensitive diagnostics withheld"));
        let accepted = app
            .automation_confirm_run(prepared["review_id"].as_str().unwrap().into())
            .await
            .unwrap();
        let job = accepted["job_id"].as_str().unwrap().to_owned();
        publish(
            &root,
            &format!("case-{case}.json"),
            json!({"version":1,"token":token,"case":case,"job":job}),
        );
        if case != "positive" {
            let gate = bounded_json(&root, &format!("loader-gate-{case}.json")).await;
            assert_eq!(gate["job"], job);
            if case == "explicit_cancel" {
                app.automation_cancel_job(job.clone()).await.unwrap();
            } else {
                app.lock().await.unwrap();
                assert!(app.unlocked().await.is_err());
            }
            publish(
                &root,
                &format!("release-{case}.json"),
                json!({"version":1,"token":token,"job":job}),
            );
            if case == "continue_on_lock" {
                // Coordinator reads only the exact OWNED Runner job's fixed
                // state. Completion must occur BEFORE general vault unlock.
                let remote = bounded_json(&root, "completed-while-locked.json").await;
                assert_eq!(remote["job"], job);
                assert_eq!(remote["state"], "succeeded");
                assert!(app.unlocked().await.is_err());
            }
            if case != "explicit_cancel" {
                app.unlock_with_passphrase("synthetic profile password42".into())
                    .await
                    .unwrap();
            }
        }
        let done = terminal_job(&app, &job).await;
        if matches!(case, "positive" | "continue_on_lock") {
            assert_eq!(done["state"], "succeeded");
        } else if done["state"] == "unknown" {
            // Existing Runner policy preserves uncertainty after stopping a
            // running process. Require exact cancellation evidence, never an
            // arbitrary failure or timeout.
            let result = &done["runner_result"]["result"];
            assert_eq!(result["cancel_requested"], true);
            assert_eq!(result["timed_out"], false);
            assert_eq!(result["remote_changes_may_be_partial"], true);
        } else {
            assert!(matches!(
                done["state"].as_str(),
                Some("cancelled" | "interrupted")
            ));
        }
        if matches!(case, "cancel_on_lock" | "explicit_cancel") {
            let events = app
                .automation_job_events(job.clone(), 0, 100)
                .await
                .unwrap();
            assert!(
                !events
                    .events
                    .iter()
                    .any(|event| event.kind == "runner_on_ok"),
                "no task success after cancellation before password loader"
            );
        }
        if done["state"] == "unknown" {
            // Explicit fixture-only acknowledgement AFTER the observed result
            // and no-success checks. Never a successful job or auto-retry.
            // Wait for the actual task to relinquish its ticket before the
            // existing acknowledgement API releases target custody.
            let (owner, unlocked) = app.unlocked().await.unwrap();
            drop(unlocked);
            let job_id = Uuid::parse_str(&job).unwrap();
            tokio::time::timeout(Duration::from_secs(10), async {
                loop {
                    let active = owner
                        .automation
                        .live_jobs
                        .lock()
                        .unwrap_or_else(|error| error.into_inner())
                        .contains_key(&job_id);
                    if !active {
                        break;
                    }
                    tokio::time::sleep(Duration::from_millis(10)).await;
                }
            })
            .await
            .expect("cancelled job task releases its original ticket");
            assert!(owner
                .automation
                .coordinator
                .unknown_jobs()
                .contains(&job_id));
            app.automation_acknowledge_unknown(job.clone(), true)
                .await
                .unwrap();
            assert_eq!(terminal_job(&app, &job).await["state"], "acknowledged");
        }
        assert_eq!(
            app.automation_project_files(project.id.clone())
                .await
                .unwrap(),
            source
        );
        publish(
            &root,
            &format!("done-{case}.json"),
            json!({"version":1,"token":token,"job":job,"state":done["state"]}),
        );
    }
    let (owner, u) = app.unlocked().await.unwrap();
    drop(u);
    owner.automation.vault_validation.close_and_wait().await;
    assert_eq!(
        app.automation_project_files(project.id.clone())
            .await
            .unwrap(),
        source
    );
    publish(
        &root,
        "native-result.json",
        json!({"version":1,"token":token,"ok":true,"normalSignedJobs":true,
        "actualSeparateSshTarget":true,"defaultValidationAndExecution":true,"wrongPasswordDenied":true,
        "positive":true,"cancelOnLock":true,"continueOnLockPreissuedOnly":true,"explicitCancel":true,
        "originalUnchanged":true,"nativeOwnedCloseAwaited":true,"egressBlocked":pins.egress_blocked}),
    );
    app.shutdown().await.unwrap();
}
