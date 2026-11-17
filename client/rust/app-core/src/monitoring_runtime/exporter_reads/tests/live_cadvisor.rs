//! STAGED, UNRUN cAdvisor derivative: actual signed job and actual source observations. No fabricated
//! successful row, approval, manifest, exporter output or metric response.
use crate::{
    AppCore, HostDto, automation_prepare::AutomationRunRequest,
    automation_runners::AutomationRunnerConfig,
};
use cc_automation_core::RemoteHelperConfig;
use cc_models::host::HostKeyPolicy;
use cc_monitoring_core::{AuthRef, Route, SourceConfig, TlsConfig};
use cc_ssh_core::keys::{KeyGenAlgorithm, generate_key};
use secrecy::ExposeSecret;
use serde::Deserialize;
use serde_json::{Value, json};
use std::{
    io::Write,
    path::{Path, PathBuf},
    time::Duration,
};
use uuid::Uuid;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Pins {
    token: String,
    runner_port: u16,
    runner_key: String,
    target_address: String,
    target_port: u16,
    target_key: String,
    prometheus_port: u16,
    cadvisor_image_id: String,
    cadvisor_manifest_sha256: String,
}
fn publish(root: &Path, name: &str, value: Value) {
    let bytes = serde_json::to_vec(&value).unwrap();
    assert!(bytes.len() <= 16384);
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(root.join(name))
        .unwrap();
    file.write_all(&bytes).unwrap();
    file.sync_all().unwrap();
}
async fn read(root: &Path, name: &str) -> Value {
    // Initial case preparation+boot+one genuine pull share the unchanged900s
    // outer budget. This test wait grants no product lease/credential renewal.
    let seconds = if name == "fixture.json" { 600 } else { 120 };
    tokio::time::timeout(Duration::from_secs(seconds), async {
        loop {
            let path = root.join(name);
            if path.exists() {
                let info = std::fs::symlink_metadata(&path).unwrap();
                assert!(info.is_file() && !info.file_type().is_symlink() && info.len() <= 16384);
                return serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    })
    .await
    .expect("owned fixture readiness deadline")
}
async fn terminal(app: &AppCore, id: &str) -> Value {
    tokio::time::timeout(Duration::from_secs(270), async {
        loop {
            let jobs = app.automation_jobs().await.unwrap();
            let row = jobs["jobs"]
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
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    })
    .await
    .expect("actual exporter job terminal deadline")
}
fn envelope(source: &str, action: Value) -> String {
    json!({"operationId":Uuid::new_v4(),"sourceId":source,"sourceRevision":1,"action":action})
        .to_string()
}

#[cfg_attr(test, tokio::test(flavor = "multi_thread", worker_threads = 2))]
#[cfg_attr(test, ignore = "requires UUID-owned QEMU systemd target, real pinned Runner and real Prometheus coordinator")]
pub(crate) async fn owned_cadvisor_install_signed_job_real_source_binding_and_docker_panel() {
    let token = std::env::var("CC_COR03_TOKEN").unwrap();
    assert_eq!(Uuid::parse_str(&token).unwrap().to_string(), token);
    let root = PathBuf::from(std::env::var_os("CC_COR03_ROOT").unwrap());
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(4)
        .unwrap();
    let allowed = workspace
        .join("Tasks/cor03/control")
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
    assert!(
        !std::fs::symlink_metadata(&root)
            .unwrap()
            .file_type()
            .is_symlink()
    );
    let keys = generate_key(KeyGenAlgorithm::Ed25519, "Owned COR03 fixture", None).unwrap();
    publish(
        &root,
        "native-public.json",
        json!({"token":token,"publicKey":keys.public_openssh}),
    );
    let pins: Pins = serde_json::from_value(read(&root, "fixture.json").await).unwrap();
    assert_eq!(pins.token, token);
    assert!(pins.cadvisor_manifest_sha256.len() == 64
        && pins.cadvisor_manifest_sha256.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)));
    assert_eq!(pins.cadvisor_image_id.len(), 71);
    assert!(pins.cadvisor_image_id.starts_with("sha256:")
        && pins.cadvisor_image_id[7..].bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)));
    assert!(pins.runner_port > 0 && pins.target_port == 2223 && pins.prometheus_port > 0);
    assert!(
        pins.target_address
            .parse::<std::net::Ipv4Addr>()
            .unwrap()
            .is_private()
    );
    for key in [&pins.runner_key, &pins.target_key] {
        assert!(
            key.starts_with("ssh-ed25519 ")
                && key.len() < 8192
                && !key.chars().any(char::is_control)
        );
    }
    let profile = tempfile::Builder::new()
        .prefix("profile-")
        .tempdir_in(&root)
        .unwrap();
    let app = AppCore::new(crate::AppConfig::for_tests(
        profile.path().to_string_lossy().into_owned(),
    ))
    .unwrap();
    app.create_local_profile(
        "Owned COR03".into(),
        "owned COR03 profile password42".into(),
    )
    .await
    .unwrap();
    let ssh = app
        .import_ssh_key(
            "Owned fixture key".into(),
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
        ("127.0.0.1", pins.runner_port, pins.runner_key.as_str()),
        (
            pins.target_address.as_str(),
            pins.target_port,
            pins.target_key.as_str(),
        ),
    ] {
        app.import_known_hosts(format!("[{address}]:{port} {key}\n"))
            .await
            .unwrap();
        let mut host = HostDto::new("Owned COR03 node", address);
        host.port = Some(port);
        host.username = Some("runner".into());
        host.credential_id = Some(ssh.id.clone());
        host.host_key_policy = HostKeyPolicy::Strict;
        hosts.push(app.save_host(host).await.unwrap());
    }
    let runner = app
        .automation_runner_create(
            "Owned COR03 Runner".into(),
            AutomationRunnerConfig::Remote {
                host_id: hosts[0].id.clone(),
                helper: RemoteHelperConfig {
                    python: "/opt/cc-runtime/bin/python".into(),
                    module_dir: "/opt/consolecrypt/src".into(),
                    root: format!("/home/runner/cor03-{token}/jobs"),
                },
            },
        )
        .await
        .unwrap();
    app.automation_runner_connect(runner.id.clone())
        .await
        .unwrap();
    let project = app
        .automation_project_create("Reviewed cAdvisor privileged host access".into(), None)
        .await
        .unwrap();
    for (path, filename) in [("playbook.yml", "cadvisor.yml")] {
        assert_eq!(filename, "cadvisor.yml");
        // The exact canonical shipped asset is compiled into this real Native
        // ignored fixture. No mutable prototype path, downloaded YAML or fake
        // approval/JobManifest is substituted at runtime.
        let text = include_str!("../../../../../../flutter/assets/automation/exporters/cadvisor.yml").to_owned();
        let expected = app
            .automation_project_file(project.id.clone(), path.into())
            .await
            .ok()
            .map(|f| f.sha256);
        app.automation_project_save(project.id.clone(), path.into(), expected, text)
            .await
            .unwrap();
    }
    app.automation_project_save_inventory(
        project.id.clone(),
        1,
        json!({"version":2,"groups":{"owned":[hosts[1].id]},"selected_hosts":[hosts[1].id]})
            .to_string(),
    )
    .await
    .unwrap();
    let request:AutomationRunRequest=serde_json::from_value(json!({"project_id":project.id,"runner_id":runner.id,
        "playbook":"playbook.yml","host_ids":[hosts[1].id],"mode":"apply","timeout_seconds":240,"forks":1,
        "cancel_on_lock":true,"parameters":{"extra_vars":{"cc_exporter_mode":"install","cc_cadvisor_version":"0.60.6",
        "cc_cadvisor_image_sha256":pins.cadvisor_manifest_sha256,
        "cc_cadvisor_image_id":pins.cadvisor_image_id,"cc_cadvisor_owner":token,
        "cc_cadvisor_port":19101,"cc_cadvisor_host_access":"privileged-host-filesystems-and-kmsg"},"tags":[],"skip_tags":[],"diff":false}})).unwrap();
    let review = app.automation_prepare_run(request).await.unwrap();
    assert_eq!(review["validation"]["valid"], true, "{}", review["validation"]);
    let admitted = app
        .automation_confirm_run(review["review_id"].as_str().unwrap().into())
        .await
        .unwrap();
    let job = admitted["job_id"].as_str().unwrap();
    let completed = terminal(&app, job).await;
    assert_eq!(completed["state"], "succeeded");
    publish(
        &root,
        "job-complete.json",
        json!({"token":token,"jobId":job}),
    );
    let observation = read(&root, "endpoint-observed.json").await;
    assert_eq!(observation["token"], token);
    assert_eq!(observation["ownedContainerRunning"], true);
    assert_eq!(observation["loopbackOnly"], true);
    assert_eq!(observation["actualImageIdentity"], true);
    assert_eq!(observation["resourceLimitsChecked"], true);
    assert_eq!(observation["realWorkloadSeries"], true);
    assert_eq!(observation["workloadCpuRatePositive"], true);
    let config = SourceConfig {
        id: String::new(),
        name: "Owned real Prometheus".into(),
        endpoint: format!("http://127.0.0.1:{}", pins.prometheus_port),
        route: Route::Direct,
        tls: TlsConfig::default(),
        auth: AuthRef::None,
    };
    let catalog = app.monitoring_source_create(0, config).await.unwrap();
    let source = &catalog.sources[0].id;
    let (owner, u) = app.unlocked().await.unwrap();
    let profile = owner.profile_id.to_string();
    drop(u);
    let context=app.monitoring_read(envelope(source,json!({"kind":"exporterContext","version":1,
        "profileId":profile,"hostId":hosts[1].id,"jobId":job,"jobRevision":completed["notification_revision"]}))).await.unwrap()["result"].clone();
    let verified=app.monitoring_read(envelope(source,json!({"kind":"exporterVerification","version":1,
        "profileId":profile,"notificationScopeId":context["notificationScopeId"],"hostId":hosts[1].id,
        "hostRevision":context["hostRevision"],"jobId":job,"jobRevision":completed["notification_revision"],
        "exporter":"cadvisor","selector":{"job":"cc_owned_cadvisor","instance":"127.0.0.1:19101"},"maxAgeSeconds":120}))).await.unwrap();
    assert_eq!(verified["result"]["observation"]["state"], "observed");
    let mut origin = context;
    origin.as_object_mut().unwrap().remove("kind");
    origin.as_object_mut().unwrap().remove("protocol");
    let origin = serde_json::from_value(origin).unwrap();
    let bound = app
        .monitoring_binding_create_with_exporter_origin(
            1,
            "Reviewed owned container host".into(),
            hosts[1].id.clone(),
            source.clone(),
            1,
            [
                ("job".into(), "cc_owned_cadvisor".into()),
                ("instance".into(), "127.0.0.1:19101".into()),
            ]
            .into(),
            Some(origin),
        )
        .await
        .unwrap();
    assert_eq!(bound.bindings.len(), 1);
    let binding = &bound.bindings[0];
    let end = chrono::Utc::now().timestamp() as f64;
    let panel=app.monitoring_read(envelope(source,json!({"kind":"bindingPanel","bindingId":binding.id,
        "bindingRevision":binding.revision,"panel":"docker","start":end-60.0,"end":end,"stepSeconds":10.0}))).await.unwrap();
    assert_eq!(panel["result"]["kind"], "bindingPanel");
    assert_eq!(panel["result"]["state"], "ready");
    // A second actual signed job cleans ONLY the same disposable guest's
    // captured original containers. No Native key export or host cleanup SSH.
    let cleanup_source = "---\n- name: End only this original owned cAdvisor fixture\n  hosts: all\n  gather_facts: false\n  become: true\n  tasks:\n    - name: Verify and end captured original guest resources\n      ansible.builtin.command:\n        argv: [/usr/bin/python3, /opt/fixture/guest_cleanup.py]\n      changed_when: true\n      no_log: true\n";
    app.automation_project_save(project.id.clone(), "cleanup.yml".into(), None, cleanup_source.into()).await.unwrap();
    let cleanup: AutomationRunRequest = serde_json::from_value(json!({"project_id":project.id,"runner_id":runner.id,
        "playbook":"cleanup.yml","host_ids":[hosts[1].id],"mode":"apply","timeout_seconds":60,"forks":1,
        "cancel_on_lock":true,"parameters":{"extra_vars":{},"tags":[],"skip_tags":[],"diff":false}})).unwrap();
    let cleanup_review = app.automation_prepare_run(cleanup).await.unwrap();
    assert_eq!(cleanup_review["validation"]["valid"], true, "{}", cleanup_review["validation"]);
    let cleanup_admitted = app.automation_confirm_run(cleanup_review["review_id"].as_str().unwrap().into()).await.unwrap();
    let cleanup_job = cleanup_admitted["job_id"].as_str().unwrap();
    let cleanup_end = terminal(&app, cleanup_job).await;
    assert_eq!(cleanup_end["state"], "succeeded");
    publish(
        &root,
        "native-result.json",
        json!({"token":token,"ok":true,"genuineSignedJob":true,
        "freshSourceObserved":true,"originalHostBinding":true,"fixedPanelReady":true,
        "cleanupJobSucceeded":true,"cleanupJobId":cleanup_job}),
    );
    app.shutdown().await.unwrap();
}
