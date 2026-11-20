//! Explicit owned-container normal-job acceptance; no user profile or fake Runner.
use super::*;
use cc_automation_core::RemoteHelperConfig;
use cc_models::host::HostKeyPolicy;
use cc_ssh_core::keys::{KeyGenAlgorithm, generate_key};
use secrecy::ExposeSecret;
use std::io::{BufRead, Read, Write};
use std::time::Duration;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct OwnedInput {
    port: u16,
    host_public_key: String,
    target_port: u16,
    target_address: String,
    target_public_key: String,
    project_path: PathBuf,
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "explicit owned two-container manager and original compiled runtime required"]
async fn actual_project_environment_override_executes_signed_normal_job()
-> Result<(), Box<dyn std::error::Error>> {
    let runner_key = generate_key(KeyGenAlgorithm::Ed25519, "owned SEC01 Runner", None)?;
    let target_key = generate_key(KeyGenAlgorithm::Ed25519, "owned SEC01 target", None)?;
    println!(
        "\n{}",
        json!({"public_key":runner_key.public_openssh,"target_public_key":target_key.public_openssh})
    );
    std::io::stdout().flush()?;
    let mut line = String::new();
    std::io::stdin().lock().take(16385).read_line(&mut line)?;
    assert!(line.len() <= 16384 && line.ends_with('\n'));
    let input: OwnedInput = serde_json::from_str(&line)?;
    assert!(input.port > 0 && input.target_port == 2222);
    let target: std::net::Ipv4Addr = input.target_address.parse()?;
    assert!(target.is_private());
    for key in [&input.host_public_key, &input.target_public_key] {
        assert!(key.starts_with("ssh-ed25519 ") && key.len() < 8192 && !key.contains(['\n', '\r']));
    }
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(4)
        .unwrap()
        .join("Tasks/runner-fixtures/ssh execution")
        .canonicalize()?;
    assert_eq!(input.project_path.canonicalize()?, root);
    let playbook = std::fs::read_to_string(root.join("playbook.yml"))?;
    assert!(playbook.contains("  hosts: fixture\n"));
    let directory = tempfile::tempdir()?;
    let app = AppCore::new(crate::AppConfig::for_tests(
        directory.path().to_string_lossy().into_owned(),
    ))?;
    app.create_local_profile("Owned SEC01 normal execution".into(), PASSWORD.into())
        .await?;
    let runner_credential = app
        .import_ssh_key(
            "Owned Runner key".into(),
            Some("runner".into()),
            runner_key.private_openssh.expose_secret().to_owned(),
            None,
            false,
            None,
        )
        .await?;
    let selected = app
        .import_ssh_key(
            "Owned selected target key".into(),
            Some("runner".into()),
            target_key.private_openssh.expose_secret().to_owned(),
            None,
            false,
            None,
        )
        .await?;
    let mut runner_host = crate::HostDto::new("Owned SEC01 Runner", "127.0.0.1");
    runner_host.port = Some(input.port);
    runner_host.username = Some("runner".into());
    runner_host.credential_id = Some(runner_credential.id.clone());
    runner_host.host_key_policy = HostKeyPolicy::Strict;
    let runner_host = app.save_host(runner_host).await?;
    let mut target_host = crate::HostDto::new("Owned SEC01 target", &input.target_address);
    target_host.port = Some(input.target_port);
    target_host.username = Some("runner".into());
    // Deliberately WRONG global key. Only the project/environment key can authenticate.
    target_host.credential_id = Some(runner_credential.id.clone());
    target_host.host_key_policy = HostKeyPolicy::Strict;
    let target_host = app.save_host(target_host).await?;
    app.import_known_hosts(format!(
        "[127.0.0.1]:{} {}\n[{}]:{} {}\n",
        input.port,
        input.host_public_key,
        input.target_address,
        input.target_port,
        input.target_public_key
    ))
    .await?;
    let runner = app
        .automation_runner_create(
            "Owned SEC01 actual Python Runner".into(),
            crate::automation_runners::AutomationRunnerConfig::Remote {
                host_id: runner_host.id.clone(),
                helper: RemoteHelperConfig {
                    python: "/opt/cc-runtime/bin/python".into(),
                    module_dir: "/opt/consolecrypt/src".into(),
                    root: "/home/runner/sec01-helper".into(),
                },
            },
        )
        .await?;
    let probe = app.automation_runner_connect(runner.id.clone()).await?;
    assert_eq!(probe["probe"]["dependencies_ready"], true);
    let project = app
        .automation_project_create("Owned SEC01 project".into(), None)
        .await?;
    let scaffold = app
        .automation_project_file(project.id.clone(), "playbook.yml".into())
        .await?;
    app.automation_project_save(
        project.id.clone(),
        "playbook.yml".into(),
        Some(scaffold.sha256),
        playbook.replace("  hosts: fixture\n", "  hosts: test_targets\n"),
    )
    .await?;
    app.automation_project_save_inventory(project.id.clone(),project.revision.unwrap(),json!({"version":2,"selected_hosts":[target_host.id],"groups":{"test_targets":[target_host.id]}}).to_string()).await?;
    let mut request: crate::automation_prepare::AutomationRunRequest = serde_json::from_value(
        json!({
        "project_id":project.id,"runner_id":runner.id,"playbook":"playbook.yml","host_ids":[target_host.id],"mode":"apply",
        "parameters":{"extra_vars":{},"tags":[],"skip_tags":[],"diff":false},"timeout_seconds":120,"forks":1,"cancel_on_lock":true}),
    )?;
    // Baseline fails at real SSH authentication; it cannot dispatch a job.
    let baseline = app
        .automation_prepare_run(request.clone())
        .await
        .unwrap_err();
    assert!(
        baseline.code() == "ssh_auth_failed",
        "baseline must be actual SSH error: {baseline}"
    );
    assert!(
        app.automation_jobs().await?["jobs"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    let context = app
        .automation_project_credentials_context(project.id.clone())
        .await?;
    let environment = Uuid::new_v4().to_string();
    let saved=app.automation_project_credentials_save(json!({"version":1,"frame":context["frame"],"policy":{"version":1,"environments":[{
        "id":environment,"name":"Owned selected environment","production":false,"overrides":[{"host_id":target_host.id,"credential_id":selected.id}]}]}}).to_string()).await?;
    let missing = app
        .automation_prepare_run(request.clone())
        .await
        .unwrap_err();
    assert!(
        missing
            .to_string()
            .contains("project_environment_selection_required")
    );
    request.deployment_environment = Some(serde_json::from_value(
        json!({"version":1,"frame":saved["frame"],"environment_id":environment}),
    )?);
    let mut outside = request.clone();
    outside.host_ids = vec![runner_host.id.clone()];
    assert!(app.automation_prepare_run(outside).await.is_err());
    let review = app.automation_prepare_run(request.clone()).await?;
    assert_eq!(review["validation"]["valid"], true, "{review}");
    assert_eq!(review["deployment_environment"]["id"], environment);
    assert_eq!(review["deployment_environment"]["production"], false);
    assert_eq!(
        review["deployment_environment"]["credential_overrides"],
        json!([{"host_id":target_host.id,"credential_id":selected.id}])
    );
    let review_id = review["review_id"].as_str().unwrap().to_owned();
    let admitted = app.automation_confirm_run(review_id.clone()).await?;
    assert!(app.automation_confirm_run(review_id).await.is_err());
    let end = tokio::time::Instant::now() + Duration::from_secs(150);
    let result = loop {
        let jobs = app.automation_jobs().await?;
        let row = jobs["jobs"]
            .as_array()
            .unwrap()
            .iter()
            .find(|r| r["job_id"] == admitted["job_id"])
            .unwrap()
            .clone();
        if !matches!(
            row["state"].as_str(),
            Some("queued" | "dispatching" | "running" | "cancelling")
        ) {
            break row;
        }
        assert!(
            tokio::time::Instant::now() < end,
            "original job did not finish"
        );
        tokio::time::sleep(Duration::from_millis(100)).await;
    };
    assert_eq!(result["state"], "succeeded", "{result}");
    assert_eq!(
        result["manifest"]["targets"][0]["credential_refs"],
        json!([selected.id])
    );
    assert_eq!(result["manifest"]["policy"]["production"], false);
    assert_eq!(
        app.get_host(target_host.id.clone()).await?.credential_id,
        Some(runner_credential.id)
    );
    let job = admitted["job_id"].as_str().unwrap().to_owned();
    let events = app.automation_job_events(job.clone(), 0, 100).await?;
    assert!(events.events.iter().any(|e| e.kind == "runner_on_ok"
        && e.payload["task"].as_str() == Some("Verify the returned marker")));
    assert!(!events.events.iter().any(|e| matches!(
        e.kind.as_str(),
        "runner_on_failed" | "runner_on_unreachable"
    )));
    app.shutdown().await?;
    println!(
        "{}",
        json!({"status":"PASS","job_id":job,"state":"succeeded","events":events.events.len(),
        "wrong_global_key_authentication_denied":true,"explicit_selection_required":true,"selected_reference_signed":true,
        "global_host_credential_unchanged":true,"outside_target_denied":true,"review_replay_denied":true,"original_shutdown_joined":true})
    );
    std::io::stdout().flush()?;
    Ok(())
}
