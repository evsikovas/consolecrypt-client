//! Actual signed default job and actual source observations. No fabricated
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
    tokio::time::timeout(Duration::from_secs(120), async {
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
    tokio::time::timeout(Duration::from_secs(960), async {
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

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires UUID-owned QEMU systemd target, real pinned Runner and real Prometheus coordinator"]
async fn owned_node_install_signed_job_real_source_binding_and_panel() {
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
        .automation_project_create("Reviewed node exporter".into(), None)
        .await
        .unwrap();
    for (path, filename) in [
        ("playbook.yml", "node-exporter.yml"),
        ("node-exporter.service.j2", "node-exporter.service.j2"),
    ] {
        let text = std::fs::read_to_string(
            workspace
                .join("monitoring-e2/next-exporter-bootstrap/templates")
                .join(filename),
        )
        .unwrap();
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
        "playbook":"playbook.yml","host_ids":[hosts[1].id],"mode":"apply","timeout_seconds":900,"forks":1,
        "cancel_on_lock":true,"parameters":{"extra_vars":{"cc_exporter_mode":"install","cc_node_exporter_version":"1.10.2",
        "cc_node_exporter_sha256":"c46e5b6f53948477ff3a19d97c58307394a29fe64a01905646f026ddc32cb65b",
        "cc_node_exporter_arch":"amd64","cc_node_exporter_port":19100},"tags":[],"skip_tags":[],"diff":false}})).unwrap();
    let review = app.automation_prepare_run(request).await.unwrap();
    assert_eq!(review["validation"]["valid"], true);
    let admitted = app
        .automation_confirm_run(review["review_id"].as_str().unwrap().into())
        .await
        .unwrap();
    let job = admitted["job_id"].as_str().unwrap();
    let completed = terminal(&app, job).await;
    // Observation evidence only: fixed codes, never arbitrary remote text or
    // the approval/parameters/credentials. Preserve an actual unknown result.
    let phase = match completed["state"].as_str() {
        Some("succeeded" | "failed" | "cancelled" | "interrupted" | "unknown") =>
            completed["state"].as_str().unwrap(),
        _ => "unclassified",
    };
    let failure = completed["runner_result"]["control_failure"].as_str();
    let safe_failure = match failure {
        Some("rpc_timeout" | "rpc_closed" | "rpc_queue_full" | "rpc_protocol" |
             "rpc_remote" | "rpc_io" | "rpc_invalid" | "owner_closed" |
             "observer_deadline" | "unexpected_runner_phase" | "runner_binding_mismatch") => failure,
        _ => None,
    };
    publish(&root, "job-observation.json", json!({
        "version":1, "token":token, "state":phase, "controlFailure":safe_failure
    }));
    assert_eq!(completed["state"], "succeeded");
    publish(
        &root,
        "job-complete.json",
        json!({"token":token,"jobId":job}),
    );
    let observation = read(&root, "endpoint-observed.json").await;
    assert_eq!(observation["token"], token);
    assert_eq!(observation["systemdActive"], true);
    assert_eq!(observation["loopbackOnly"], true);
    assert_eq!(observation["actualBinaryChecksum"], true);
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
        "exporter":"nodeExporter","selector":{"job":"cc_owned_node","instance":"127.0.0.1:19100"},"maxAgeSeconds":120}))).await.unwrap();
    assert_eq!(verified["result"]["observation"]["state"], "observed");
    let mut origin = context;
    origin.as_object_mut().unwrap().remove("kind");
    origin.as_object_mut().unwrap().remove("protocol");
    let origin = serde_json::from_value(origin).unwrap();
    let bound = app
        .monitoring_binding_create_with_exporter_origin(
            1,
            "Reviewed owned node".into(),
            hosts[1].id.clone(),
            source.clone(),
            1,
            [
                ("job".into(), "cc_owned_node".into()),
                ("instance".into(), "127.0.0.1:19100".into()),
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
        "bindingRevision":binding.revision,"panel":"hostOverview","start":end-60.0,"end":end,"stepSeconds":10.0}))).await.unwrap();
    assert_eq!(panel["result"]["kind"], "bindingPanel");
    assert_eq!(panel["result"]["state"], "ready");
    publish(
        &root,
        "native-result.json",
        json!({"token":token,"ok":true,"genuineSignedJob":true,
        "freshSourceObserved":true,"originalHostBinding":true,"fixedPanelReady":true}),
    );
    app.shutdown().await.unwrap();
}
