//! Explicit disposable-fixture acceptance. No user profile, account or keys.
use cc_app_core::automation_prepare::AutomationRunRequest;
use cc_app_core::automation_runners::AutomationRunnerConfig;
use cc_app_core::{AppConfig, AppCore, AppEvent, AutomationJobStateDto, HostDto};
use cc_automation_core::{HelperLaunch, RemoteHelperConfig, RunParameters};
use cc_models::{automation::JobMode, host::HostKeyPolicy};
use cc_ssh_core::keys::{generate_key, KeyGenAlgorithm};
use secrecy::ExposeSecret;
use serde::Deserialize;
use serde_json::{json, Value};
use std::{
    collections::BTreeMap,
    io::{self, BufRead, Write},
    path::PathBuf,
    time::Duration,
};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Fixture {
    port: u16,
    host_public_key: String,
    target_port: u16,
    target_address: String,
    target_public_key: String,
    project_path: PathBuf,
    #[serde(default)]
    wsl_launch: Option<HelperLaunch>,
    #[serde(default)]
    git_source: Option<GitFixture>,
    #[serde(default)]
    legacy_probe_python: Option<String>,
    #[serde(default)]
    legacy_dependencies_python: Option<String>,
    #[serde(default)]
    jump: Option<JumpFixture>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct JumpFixture {
    port: u16,
    public_key: String,
    runner_address: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct GitFixture {
    url: String,
    ca_pem: String,
    commit: String,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let runner_key = generate_key(KeyGenAlgorithm::Ed25519, "disposable AppCore Runner", None)?;
    let target_key = generate_key(KeyGenAlgorithm::Ed25519, "disposable AppCore target", None)?;
    println!(
        "{}",
        json!({"public_key":runner_key.public_openssh,"target_public_key":target_key.public_openssh})
    );
    io::stdout().flush()?;
    let mut line = String::new();
    io::stdin().lock().read_line(&mut line)?;
    let fixture: Fixture = serde_json::from_str(&line)?;
    let directory = tempfile::tempdir()?;
    let mut project_directory = directory.path().join("source");
    std::fs::create_dir(&project_directory)?;
    let playbook = std::fs::read_to_string(fixture.project_path.join("playbook.yml"))?;
    assert!(playbook.contains("  hosts: fixture\n"));
    std::fs::write(
        project_directory.join("playbook.yml"),
        playbook.replacen("  hosts: fixture\n", "  hosts: test_targets\n", 1),
    )?;
    let app = AppCore::new(AppConfig::for_tests(
        directory.path().to_string_lossy().into_owned(),
    ))?;
    let created_profile = app
        .create_local_profile(
            "Disposable Automation Acceptance".into(),
            "correct horse fixture passphrase 42".into(),
        )
        .await?;
    let notice_profile = created_profile.profile.id.clone();
    drop(created_profile);
    let runner_credential = app
        .import_ssh_key(
            "Fixture Runner".into(),
            Some("runner".into()),
            runner_key.private_openssh.expose_secret().to_owned(),
            None,
            false,
            None,
        )
        .await?;
    let target_credential = app
        .import_ssh_key(
            "Fixture target".into(),
            Some("runner".into()),
            target_key.private_openssh.expose_secret().to_owned(),
            None,
            false,
            None,
        )
        .await?;
    let jump_host = if let Some(jump) = &fixture.jump {
        let mut host = HostDto::new("Fixture SSH gateway", "127.0.0.1");
        host.port = Some(jump.port);
        host.username = Some("runner".into());
        host.credential_id = Some(runner_credential.id.clone());
        host.host_key_policy = HostKeyPolicy::Strict;
        app.import_known_hosts(format!(
            "[127.0.0.1]:{} {}\n",
            jump.port,
            jump.public_key.trim()
        ))
        .await?;
        Some(app.save_host(host).await?)
    } else {
        None
    };
    let runner_address = fixture
        .jump
        .as_ref()
        .map_or("127.0.0.1", |jump| jump.runner_address.as_str());
    let mut hosts = Vec::new();
    for (name, address, port, credential) in [
        (
            "Fixture Runner",
            runner_address,
            fixture.port,
            &runner_credential.id,
        ),
        (
            "Fixture target",
            fixture.target_address.as_str(),
            fixture.target_port,
            &target_credential.id,
        ),
    ] {
        let mut host = HostDto::new(name, address);
        host.port = Some(port);
        host.username = Some("runner".into());
        host.credential_id = Some(credential.clone());
        host.host_key_policy = HostKeyPolicy::Strict;
        if name == "Fixture Runner" {
            if let Some(jump) = &jump_host {
                host.jump_chain = vec![jump.id.clone()];
            }
        }
        // A saved agent-forwarding preference must not enable it for automation.
        host.agent_forwarding = true;
        hosts.push(app.save_host(host).await?);
    }
    let imported = app
        .import_known_hosts(format!(
            "[{}]:{} {}\n[{}]:{} {}\n",
            runner_address,
            fixture.port,
            fixture.host_public_key.trim(),
            fixture.target_address,
            fixture.target_port,
            fixture.target_public_key.trim()
        ))
        .await?;
    let _ = imported;
    let runner = app
        .automation_runner_create(
            "Actual isolated Linux Runner".into(),
            if let Some(launch) = fixture.wsl_launch {
                AutomationRunnerConfig::Local { launch }
            } else {
                AutomationRunnerConfig::Remote {
                    host_id: hosts[0].id.clone(),
                    helper: RemoteHelperConfig {
                        python: "/opt/cc-runtime/bin/python".into(),
                        module_dir: "/opt/consolecrypt/src".into(),
                        root: "/home/runner/appcore execution".into(),
                    },
                }
            },
        )
        .await?;
    let probe = app.automation_runner_connect(runner.id.clone()).await?;
    assert_eq!(probe["probe"]["dependencies_ready"], true);
    println!("phase: AppCore private Runner ready");
    io::stdout().flush()?;
    let project = if let Some(source) = &fixture.git_source {
        let started = app
            .automation_project_git_start(
                "Actual Git fixture".into(),
                runner.id.clone(),
                source.url.clone(),
                "main".into(),
                Some(source.ca_pem.clone()),
            )
            .await?;
        let deadline = tokio::time::Instant::now() + Duration::from_secs(90);
        let project = loop {
            let catalog = app.automation_projects().await?;
            let row = catalog
                .projects
                .into_iter()
                .find(|p| p.id == started.id)
                .ok_or("missing Git import")?;
            match row.import_state.as_deref() {
                Some("ready") => break row,
                Some("importing") => {}
                _ => return Err("Git import failed".into()),
            }
            assert!(tokio::time::Instant::now() < deadline);
            tokio::time::sleep(Duration::from_millis(50)).await;
        };
        assert_eq!(
            project.git_source.as_ref().unwrap()["commit"],
            source.commit
        );
        project_directory = PathBuf::from(project.path.as_ref().unwrap());
        let before = app
            .automation_project_file(project.id.clone(), "playbook.yml".into())
            .await?;
        assert!(before.text.contains("ConsoleCrypt Git baseline"));
        let after = before
            .text
            .replace("ConsoleCrypt Git baseline", "ConsoleCrypt scoped execution");
        app.automation_project_save(
            project.id.clone(),
            "playbook.yml".into(),
            Some(before.sha256),
            after.clone(),
        )
        .await?;
        let current = app.automation_project_files(project.id.clone()).await?;
        let diff = app
            .automation_project_diff(project.id.clone(), Some(current.digest.clone()), 0, 50)
            .await?;
        assert_eq!(diff["total"], 1);
        assert_eq!(diff["changes"][0]["path"], "playbook.yml");
        let file = app
            .automation_project_file_diff(project.id.clone(), "playbook.yml".into(), current.digest)
            .await?;
        assert_eq!(file["before"], before.text);
        assert_eq!(file["after"], after);
        project
    } else {
        app.automation_project_create(
            "Actual fixture source".into(),
            Some(project_directory.to_string_lossy().into_owned()),
        )
        .await?
    };
    let project_revision = project.revision.unwrap();
    app.automation_project_save_inventory(
        project.id.clone(),project_revision,
        json!({"version":1,"groups":{"test_targets":[hosts[1].id],"second_membership":[hosts[1].id,hosts[0].id]}}).to_string(),
    ).await?;
    app.automation_inventory_select(json!({"version":1,"project_id":project.id,
        "expected_revision":project_revision+1,"saved_hosts":[hosts[1].id],"configuration":null,"preview_id":null}).to_string()).await?;
    let request = AutomationRunRequest {
        project_id: project.id,
        runner_id: runner.id.clone(),
        playbook: "playbook.yml".into(),
        host_ids: vec![hosts[1].id.clone()],
        mode: JobMode::Apply,
        parameters: RunParameters {
            extra_vars: BTreeMap::from([("threshold".into(), json!(1e-7))]),
            tags: vec![],
            skip_tags: vec![],
            diff: false,
            vault: None,
        },
        timeout_seconds: 120,
        forks: 2,
        cancel_on_lock: true,
        retry_of: None,
        vault_credential_id: None,
        vault_selection: None, deployment_environment:None,
    };
    let mut outside_inventory = request.clone();
    outside_inventory.host_ids = vec![hosts[0].id.clone()];
    let outside_error = app
        .automation_prepare_run(outside_inventory)
        .await
        .unwrap_err();
    assert!(outside_error
        .to_string()
        .contains("not selected in project inventory"));
    app.automation_project_save(
        request.project_id.clone(),
        ".consolecrypt-inputs.json".into(),
        None,
        json!({"version":1,"playbooks":{"playbook.yml":{"required_extra_vars":["threshold"]}}})
            .to_string(),
    )
    .await?;
    let main = app
        .automation_project_file(request.project_id.clone(), "playbook.yml".into())
        .await?;
    assert!(main.text.contains("  gather_facts: false\n"));
    app.automation_project_create_role(request.project_id.clone(), "scaffold_fixture".into())
        .await?;
    app.automation_project_save(
        request.project_id.clone(),
        "playbook.yml".into(),
        Some(main.sha256),
        main.text.replace(
            "  gather_facts: false\n",
            "  gather_facts: false\n  roles:\n    - role: scaffold_fixture\n    - role: locked_fixture\n",
        ).replace("  tasks:\n", "  tasks:\n    - name: Hold the owned fixture for transition review\n      ansible.builtin.wait_for:\n        timeout: 1\n"),
    )
    .await?;
    let role_path = "roles/locked_fixture/tasks/main.yml";
    let role_text = "---\n- name: Locked dependency role proof\n  ansible.builtin.assert:\n    that:\n      - threshold == 0.0000001\n";
    let role = app
        .automation_project_save(
            request.project_id.clone(),
            role_path.into(),
            None,
            role_text.into(),
        )
        .await?;
    let role_tree =
        cc_automation_core::ProjectRoot::open(&project_directory.join("roles/locked_fixture"))?
            .revision()?;
    app.automation_project_save(request.project_id.clone(), ".consolecrypt-dependencies.json".into(), None,
        json!({"version":1,"dependencies":[{"kind":"role","name":"locked_fixture","version":"1.2.3",
            "source":{"url":"https://example.test/declared-vendored-role.tar.gz","sha256":"a".repeat(64)},
            "tree_digest":role_tree.digest}]}).to_string()).await?;
    let requirements = app
        .automation_project_save(
            request.project_id.clone(),
            ".consolecrypt-requirements.json".into(),
            None,
            json!({"version":1,"playbooks":{"playbook.yml":{
                "required_extra_vars":["threshold"],
                "required_dependencies":[{"kind":"role","name":"locked_fixture","version":"2.0.0"}]
            }}})
            .to_string(),
        )
        .await?;
    let missing_pin = app.automation_prepare_run(request.clone()).await?;
    assert_eq!(missing_pin["validation"]["valid"], false);
    assert!(missing_pin["validation"]["checks"][0]["output"]
        .as_str()
        .unwrap()
        .contains("pinned version differs"));
    assert!(app
        .automation_confirm_run(missing_pin["review_id"].as_str().unwrap().into())
        .await
        .is_err());
    app.automation_project_save(
        request.project_id.clone(),
        requirements.path,
        Some(requirements.sha256),
        requirements.text.replace("2.0.0", "1.2.3"),
    )
    .await?;
    for (python, capability) in [
        fixture
            .legacy_probe_python
            .as_ref()
            .map(|p| (p, "declared_inputs_v1")),
        fixture
            .legacy_dependencies_python
            .as_ref()
            .map(|p| (p, "locked_dependencies_v1")),
    ]
    .into_iter()
    .flatten()
    {
        let legacy = app
            .automation_runner_create(
                "Older helper capability fixture".into(),
                AutomationRunnerConfig::Remote {
                    host_id: hosts[0].id.clone(),
                    helper: RemoteHelperConfig {
                        python: python.clone(),
                        module_dir: "/opt/consolecrypt/src".into(),
                        root: format!("/home/runner/old capability fixture-{capability}"),
                    },
                },
            )
            .await?;
        let report = app.automation_runner_connect(legacy.id.clone()).await?;
        assert!(report["probe"]["capabilities"].get(capability).is_none());
        let mut old_request = request.clone();
        old_request.runner_id = legacy.id.clone();
        let error = app.automation_prepare_run(old_request).await.unwrap_err();
        assert_eq!(
            error.args().get("field").map(String::as_str),
            Some("automation_runner_update"),
            "{error}"
        );
        assert!(app.automation_jobs().await?["jobs"]
            .as_array()
            .unwrap()
            .is_empty());
        app.automation_runner_disconnect(legacy.id).await?;
    }
    let changed_role = app
        .automation_project_save(
            request.project_id.clone(),
            role_path.into(),
            Some(role.sha256),
            role_text.replace("threshold == 0.0000001", "threshold == 0.0000002"),
        )
        .await?;
    let denied = app.automation_prepare_run(request.clone()).await?;
    assert_eq!(denied["validation"]["valid"], false);
    assert!(denied["validation"]["checks"][0]["output"]
        .as_str()
        .unwrap()
        .contains("pinned tree digest"));
    assert!(app
        .automation_confirm_run(denied["review_id"].as_str().unwrap().into())
        .await
        .is_err());
    assert!(app.automation_jobs().await?["jobs"]
        .as_array()
        .unwrap()
        .is_empty());
    app.automation_project_save(
        request.project_id.clone(),
        role_path.into(),
        Some(changed_role.sha256),
        role_text.into(),
    )
    .await?;
    let mut missing = request.clone();
    missing.parameters.extra_vars.clear();
    let denied = app.automation_prepare_run(missing).await?;
    assert_eq!(denied["validation"]["valid"], false);
    assert!(denied["validation"]["checks"][0]["output"]
        .as_str()
        .unwrap()
        .contains("Missing required execution inputs: threshold"));
    assert!(app
        .automation_confirm_run(denied["review_id"].as_str().unwrap().into())
        .await
        .is_err());
    assert!(app.automation_jobs().await?["jobs"]
        .as_array()
        .unwrap()
        .is_empty());
    let fenced = app.automation_prepare_run(request.clone()).await?;
    assert_eq!(fenced["validation"]["valid"], true, "{fenced}");
    let exit_permit = app.automation_prepare_exit().await?;
    assert!(app
        .automation_confirm_run(fenced["review_id"].as_str().unwrap().into())
        .await
        .is_err());
    assert!(app.automation_jobs().await?["jobs"]
        .as_array()
        .unwrap()
        .is_empty());
    app.automation_abort_exit(exit_permit).await?;
    let review = app.automation_prepare_run(request.clone()).await?;
    assert_eq!(review["validation"]["valid"], true, "{review}");
    if fixture.jump.is_some() {
        assert_eq!(review["origin"]["kind"], "remote");
        let hops = review["origin"]["route"]["hops"].as_array().unwrap();
        assert_eq!(hops.len(), 2, "Runner must retain its reviewed SSH gateway");
        assert_eq!(hops[0]["name"], "Fixture SSH gateway");
        assert_eq!(hops[1]["name"], "Fixture Runner");
    }
    if let Some(source) = &fixture.git_source {
        assert_eq!(review["git_source"]["commit"], source.commit);
        assert_ne!(
            review["git_source"]["snapshot_digest"],
            review["revision"]["digest"]
        );
        let diff = app
            .automation_project_diff(
                request.project_id.clone(),
                Some(review["revision"]["digest"].as_str().unwrap().into()),
                0,
                50,
            )
            .await?;
        let paths: Vec<_> = diff["changes"]
            .as_array()
            .unwrap()
            .iter()
            .map(|entry| entry["path"].as_str().unwrap())
            .collect();
        for path in [
            "playbook.yml",
            ".consolecrypt-inputs.json",
            ".consolecrypt-dependencies.json",
            role_path,
        ] {
            assert!(
                paths.contains(&path),
                "missing reviewed dependency change: {path}"
            );
        }
    }
    let alias = &review["targets"][0]["alias"];
    assert_eq!(
        review["inventory_groups"]["test_targets"]["hosts"],
        json!([alias])
    );
    assert_eq!(
        review["inventory_groups"]["second_membership"]["hosts"],
        json!([alias])
    );
    let current_project_revision = app
        .automation_project_inventory(request.project_id.clone())
        .await?["project_revision"]
        .as_u64()
        .unwrap();
    app.automation_project_save_inventory(
        request.project_id.clone(),
        current_project_revision,
        json!({"version":2,"selected_hosts":[hosts[1].id],"groups":{
        "test_targets":[hosts[1].id]}})
        .to_string(),
    )
    .await?;
    assert!(
        app.automation_confirm_run(review["review_id"].as_str().unwrap().into())
            .await
            .is_err(),
        "changed project groups must revoke the previous review"
    );
    let review = app.automation_prepare_run(request.clone()).await?;
    let route = &review["routes"][0];
    assert_eq!(route["host_id"], hosts[1].id);
    assert_eq!(
        route["hops"][0]["endpoint"],
        format!("runner@{}:{}", fixture.target_address, fixture.target_port)
    );
    assert!(route.get("credential").is_none());
    assert!(route.get("credential_refs").is_none());
    println!("phase: exact AppCore review ready");
    io::stdout().flush()?;
    // A changed native host revokes the review before any signing or job dispatch.
    let mut changed = hosts[1].clone();
    changed.name = "Renamed fixture target".into();
    app.save_host(changed).await?;
    assert!(app
        .automation_confirm_run(review["review_id"].as_str().unwrap().into())
        .await
        .is_err());
    let before_replace = app.automation_prepare_run(request.clone()).await?;
    let source_parent = project_directory.parent().unwrap().canonicalize()?;
    let parked = source_parent.join("source_before_editor_replace");
    let replaced = source_parent.join("source_editor_replacement");
    let fixture_root = directory.path().canonicalize()?;
    assert!(source_parent.starts_with(&fixture_root));
    assert_eq!(
        project_directory.canonicalize()?.parent(),
        Some(source_parent.as_path())
    );
    for path in [&parked, &replaced] {
        assert_eq!(path.parent().unwrap().canonicalize()?, source_parent);
        assert!(!path.exists());
    }
    std::fs::rename(&project_directory, &parked)?;
    std::fs::create_dir(&project_directory)?;
    std::fs::write(
        project_directory.join("playbook.yml"),
        "---\n# editor replaced the whole directory\n- hosts: all\n  tasks: []\n",
    )?;
    assert!(
        app.automation_confirm_run(before_replace["review_id"].as_str().unwrap().into())
            .await
            .is_err(),
        "a replaced current source directory must invalidate the old review"
    );
    assert!(
        app.automation_jobs().await?["jobs"]
            .as_array()
            .unwrap()
            .is_empty(),
        "stale source must not create a queued job"
    );
    std::fs::rename(&project_directory, &replaced)?;
    std::fs::rename(&parked, &project_directory)?;
    let review = app.automation_prepare_run(request.clone()).await?;
    let review_id = review["review_id"].as_str().unwrap().to_owned();
    let mut transition_prompts = app.subscribe_prompts();
    let admitted = app.automation_confirm_run(review_id.clone()).await?;
    assert!(app.automation_confirm_run(review_id).await.is_err());
    let preparing_exit = tokio::spawn({
        let app = app.clone();
        async move { app.automation_prepare_exit().await }
    });
    let cc_app_core::PromptRequest::AutomationTransition(question) =
        tokio::time::timeout(Duration::from_secs(5), transition_prompts.recv()).await??
    else {
        return Err("expected actual running-job transition question".into());
    };
    assert_eq!(question.unknown_count, 0);
    let answering_wait = tokio::spawn({
        let app = app.clone();
        async move {
            app.automation_answer_transition(question.request_id, "wait".into())
                .await
        }
    });
    let prepared = tokio::time::timeout(Duration::from_secs(120), preparing_exit).await?;
    let exit_permit = prepared??;
    answering_wait.await??;
    app.automation_abort_exit(exit_permit).await?;
    drop(transition_prompts);
    let deadline = tokio::time::Instant::now() + Duration::from_secs(180);
    let result: Value = loop {
        let jobs = app.automation_jobs().await?;
        let job = jobs["jobs"]
            .as_array()
            .unwrap()
            .iter()
            .find(|row| row["job_id"] == admitted["job_id"])
            .unwrap()
            .clone();
        if !matches!(
            job["state"].as_str(),
            Some("queued" | "dispatching" | "running")
        ) {
            break job;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "AppCore job timed out"
        );
        tokio::time::sleep(Duration::from_millis(350)).await;
    };
    assert_eq!(result["state"], "succeeded", "{result}");
    let environment_receipt = result["runner_result"]["environment"].clone();
    assert_eq!(
        environment_receipt["environment_digest"],
        result["manifest"]["environment_digest"]
    );
    assert_eq!(
        environment_receipt["identity"]["packages"]["ansible-core"]["actual"],
        "2.20.11"
    );
    if let Some(source) = &fixture.git_source {
        assert_eq!(result["git_source"]["commit"], source.commit);
        assert_eq!(
            result["manifest"]["revision_digest"],
            review["revision"]["digest"]
        );
    }
    let job_id = admitted["job_id"].as_str().unwrap().to_owned();
    let events = app.automation_job_events(job_id.clone(), 0, 100).await?;
    assert!(events
        .events
        .iter()
        .any(|event| event.kind == "runner_on_ok"
            && event.payload["task"]
                .as_str()
                .is_some_and(|name| name.contains("Describe this role"))));
    assert!(!events.events.is_empty());
    assert!(events.events.iter().any(|event| {
        event.kind == "runner_on_ok"
            && event.payload["task"]
                .as_str()
                .is_some_and(|task| task.contains("Locked dependency role proof"))
    }));
    assert!(app
        .automation_job_events(job_id.clone(), events.cursor, 100)
        .await?
        .events
        .is_empty());
    // The approved playbook already slurps/asserts the marker through the
    // selected Runner. A generic app.exec would originate from Windows and
    // cannot reach a deliberately unpublished Docker bridge address.
    // The outer fixture also independently reads the exact target file.
    assert!(
        app.get_host(hosts[1].id.clone()).await?.agent_forwarding,
        "stored host preference changed"
    );
    app.automation_runner_disconnect(runner.id.clone()).await?;
    app.automation_runner_connect(runner.id.clone()).await?;
    let replayed = app.automation_job_events(job_id.clone(), 0, 100).await?;
    let retained = app.automation_jobs().await?;
    let retained = retained["jobs"]
        .as_array()
        .unwrap()
        .iter()
        .find(|job| job["job_id"] == admitted["job_id"])
        .unwrap();
    assert_eq!(
        retained["runner_result"]["environment"],
        environment_receipt
    );
    assert_eq!(
        replayed.cursor, events.cursor,
        "journal changed across private helper reconnect"
    );
    // The second real job exercises external aliases and a private inventory
    // larger than an IPC frame. Only this disposable project is written.
    let external = json!({"test_targets":{"hosts":{"service_a":{
        "ansible_host":fixture.target_address,"ansible_port":fixture.target_port,"ansible_user":"runner"}}}});
    std::fs::create_dir_all(project_directory.join("inventory/group_vars"))?;
    std::fs::create_dir_all(project_directory.join("inventory/host_vars"))?;
    std::fs::write(
        project_directory.join("inventory/hosts.yml"),
        external.to_string(),
    )?;
    std::fs::write(
        project_directory.join("inventory/group_vars/test_targets.yml"),
        "external_group_fixture_marker: must-not-appear-in-native-review\n",
    )?;
    std::fs::write(
        project_directory.join("inventory/host_vars/service_a.yml"),
        format!(
            "large_private_fixture_value: {}\nthreshold: 0.0000001\n",
            "x".repeat(1200000)
        ),
    )?;
    let sources = json!([{"path":"inventory","format":"directory"}]);
    let started = app
        .automation_inventory_start(
            request.project_id.clone(),
            runner.id.clone(),
            sources.to_string(),
        )
        .await?;
    let preview_id = started["preview_id"].as_str().unwrap().to_owned();
    let deadline = tokio::time::Instant::now() + Duration::from_secs(90);
    loop {
        let page = app
            .automation_inventory_page(preview_id.clone(), String::new(), 0, 50)
            .await?;
        match page["state"].as_str() {
            Some("ready") => break,
            Some("preparing") => {}
            _ => return Err("external inventory preview failed".into()),
        }
        assert!(tokio::time::Instant::now() < deadline);
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    let saved_revision = app
        .automation_inventory_configuration(request.project_id.clone())
        .await?["project_revision"]
        .as_u64()
        .unwrap();
    app.automation_inventory_select(
        json!({"version":1,"project_id":request.project_id,"expected_revision":saved_revision,
        "saved_hosts":[hosts[1].id],"preview_id":preview_id,"configuration":{"version":1,"sources":sources,"mode":"external_only",
        "bindings":{"service_a":{"host_id":hosts[1].id,"resolution":"match","source":{
            "address":fixture.target_address,"port":fixture.target_port,"username":"runner"}}}}})
        .to_string(),
    )
    .await?;
    let mut retry = request;
    retry.retry_of = Some(job_id.clone());
    // This is independently validated against a new snapshot/environment and
    // requires a new one-use approval, even after a successful previous run.
    let fresh = app.automation_prepare_run(retry).await?;
    assert_eq!(fresh["retry_origin"]["job_id"], job_id);
    assert_eq!(fresh["targets"][0]["alias"], "service_a");
    assert_eq!(
        fresh["inventory_groups"]["test_targets"]["selected_count"],
        1
    );
    assert!(fresh["inventory_groups"]["test_targets"]
        .get("vars")
        .is_none());
    assert!(!fresh
        .to_string()
        .contains("must-not-appear-in-native-review"));
    assert_ne!(fresh["review_id"], review["review_id"]);
    assert_eq!(
        app.automation_jobs().await?["jobs"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    // A real dispatched job must report completion even when no Jobs UI/API
    // is polling it. Use the same metadata stream the desktop already owns.
    let mut job_notices = app.subscribe_events();
    let repeated = app
        .automation_confirm_run(fresh["review_id"].as_str().unwrap().into())
        .await?;
    assert_ne!(repeated["job_id"], admitted["job_id"]);
    assert_eq!(repeated["retry_origin"]["job_id"], job_id);
    let deadline = tokio::time::Instant::now() + Duration::from_secs(180);
    let mut last_notice_revision = 0;
    let mut saw_queued = false;
    let mut saw_running = false;
    loop {
        let event = tokio::time::timeout_at(deadline, job_notices.recv()).await??;
        if let AppEvent::AutomationJobChanged {
            profile_id,
            job_id,
            revision,
            state,
        } = &event
        {
            if repeated["job_id"] != *job_id {
                continue;
            }
            assert_eq!(profile_id, &notice_profile);
            let encoded = serde_json::to_value(&event)?;
            assert_eq!(encoded.as_object().unwrap().len(), 5);
            if *revision <= last_notice_revision {
                continue;
            }
            last_notice_revision = *revision;
            match state {
                AutomationJobStateDto::Queued => saw_queued = true,
                AutomationJobStateDto::Running => saw_running = true,
                AutomationJobStateDto::Succeeded => break,
                AutomationJobStateDto::Dispatching | AutomationJobStateDto::Cancelling => {}
                _ => return Err("actual job failed before a successful completion notice".into()),
            }
        }
    }
    assert!(saw_queued && saw_running);
    // One explicit reconciliation AFTER the event, never a completion poll.
    let reconciled = app.automation_jobs().await?;
    let completed = reconciled["jobs"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["job_id"] == repeated["job_id"])
        .unwrap();
    assert_eq!(completed["state"], "succeeded");
    assert_eq!(completed["notification_revision"], last_notice_revision);
    app.automation_runner_disconnect(runner.id).await?;
    app.shutdown().await?;
    println!(
        "{}",
        json!({"status":"PASS","native_appcore":true,"sqlcipher_admission":true,"real_ansible_job":"succeeded",
        "stale_review_denied":true,"replaced_source_directory_denied":true,"one_use_confirmation":true,"saved_preferences_preserved":true,"target_marker_verified":true,
        "project_groups_bound_to_selected_targets":true,"new_review_and_job_for_retry":true,
        "native_job_state_events":true,"hidden_job_completion_without_jobs_poll":true,
        "external_inventory_real_job":true,"large_inventory_chunked":true,"private_variables_absent_from_review":true,
        "git_import_diff_signed_execution":fixture.git_source.is_some(),"git_commit_and_snapshot_preserved":fixture.git_source.is_some(),
        "required_input_preflight_blocks_dispatch":true,"old_helper_cannot_ignore_input_contract":fixture.legacy_probe_python.is_some(),
        "environment_receipt_bound_and_retained":true,"old_helper_cannot_ignore_dependency_contract":fixture.legacy_dependencies_python.is_some(),
        "changed_dependency_blocks_dispatch":true,"locked_dependency_role_executed":true,
        "reviewed_requirements_bound_to_signed_revision":true,"required_dependency_version_blocks_dispatch":true,
        "prepared_exit_blocks_new_job_admission":true,"wait_for_exit_keeps_actual_signed_job_running":true,
        "saved_inventory_rejects_unselected_host_before_connect":true,"unified_inventory_selection_atomic":true,
        "native_role_scaffold_validated_and_executed":true,"runner_over_saved_ssh_jump":fixture.jump.is_some()})
    );
    Ok(())
}
