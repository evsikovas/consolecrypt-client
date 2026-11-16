//! Owned actual SSH + current existing-peer probe -> exact preview -> one model send.
//! No fake OS/probe, job, execution, package install or filesystem proposal apply.
use crate::{AppCore, HostDto, automation_runners::AutomationRunnerConfig, config::AppConfig};
use cc_automation_core::RemoteHelperConfig;
use cc_models::host::HostKeyPolicy;
use cc_ssh_core::keys::{KeyGenAlgorithm, generate_key};
use secrecy::ExposeSecret;
use serde_json::{Value, json};
use std::{
    io::Write,
    path::{Path, PathBuf},
    time::Duration,
};
use uuid::Uuid;
#[path = "../../../tests/common/ai_mock.rs"]
mod ai_mock;
fn publish(root: &Path, name: &str, value: Value) {
    let raw = serde_json::to_vec(&value).unwrap();
    assert!(raw.len() <= 16384);
    let mut file = std::fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(root.join(name))
        .unwrap();
    file.write_all(&raw).unwrap();
    file.sync_all().unwrap();
}
async fn fixture(root: &Path) -> Value {
    tokio::time::timeout(Duration::from_secs(30), async {
        loop {
            let p = root.join("fixture.json");
            if p.exists() {
                assert!(!p.is_symlink() && p.metadata().unwrap().len() <= 16384);
                return serde_json::from_slice(&std::fs::read(p).unwrap()).unwrap();
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    })
    .await
    .unwrap()
}
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires exact UUID-owned pinned SSH Runner coordinator; no public or user endpoints"]
async fn owned_actual_os_fresh_runner_probe_exact_oneuse_selected_ai_preview() {
    let token = std::env::var("CC_AI03_TOKEN").unwrap();
    assert_eq!(Uuid::parse_str(&token).unwrap().to_string(), token);
    let root = PathBuf::from(std::env::var_os("CC_AI03_ROOT").unwrap());
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(4)
        .unwrap();
    assert_eq!(
        root.canonicalize().unwrap().parent(),
        Some(
            workspace
                .join("Tasks/ai03/control")
                .canonicalize()
                .unwrap()
                .as_path()
        )
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
    let key = generate_key(KeyGenAlgorithm::Ed25519, "Owned AI03", None).unwrap();
    publish(
        &root,
        "native-public.json",
        json!({"token":token,"publicKey":key.public_openssh}),
    );
    let fixture = fixture(&root).await;
    assert_eq!(fixture["token"], token);
    let port = u16::try_from(fixture["port"].as_u64().unwrap()).unwrap();
    assert!(port > 0);
    let host_key = fixture["hostKey"].as_str().unwrap();
    assert!(host_key.starts_with("ssh-ed25519 ") && !host_key.chars().any(char::is_control));
    let data = tempfile::Builder::new()
        .prefix("profile-")
        .tempdir_in(&root)
        .unwrap();
    let app = AppCore::new(AppConfig::for_tests(
        data.path().to_string_lossy().into_owned(),
    ))
    .unwrap();
    app.create_local_profile("Owned AI03".into(), "owned AI03 profile password42".into())
        .await
        .unwrap();
    let credential = app
        .import_ssh_key(
            "Owned private key".into(),
            Some("runner".into()),
            key.private_openssh.expose_secret().to_owned(),
            None,
            false,
            None,
        )
        .await
        .unwrap();
    app.import_known_hosts(format!("[127.0.0.1]:{port} {host_key}\n"))
        .await
        .unwrap();
    let mut host = HostDto::new("Explicit selected context", "127.0.0.1");
    host.port = Some(port);
    host.username = Some("runner".into());
    host.credential_id = Some(credential.id);
    host.host_key_policy = HostKeyPolicy::Strict;
    let host = app.save_host(host).await.unwrap();
    let runner = app
        .automation_runner_create(
            "Owned connected Runner".into(),
            AutomationRunnerConfig::Remote {
                host_id: host.id.clone(),
                helper: RemoteHelperConfig {
                    python: "/opt/cc-runtime/bin/python".into(),
                    module_dir: "/opt/consolecrypt/src".into(),
                    root: format!("/home/runner/ai03-{token}/jobs"),
                },
            },
        )
        .await
        .unwrap();
    app.automation_runner_connect(runner.id.clone())
        .await
        .unwrap();
    let project = app
        .automation_project_create("Owned context pins".into(), None)
        .await
        .unwrap();
    let before = app
        .automation_project_files(project.id.clone())
        .await
        .unwrap();
    let mock = ai_mock::MockLlm::start().await;
    let provider = app
        .save_ai_provider(crate::dto::AiProviderDto {
            id: String::new(),
            name: "Explicit owned provider".into(),
            provider: cc_models::ai::AiProviderKind::OpenaiCompatible,
            base_url: mock.url(),
            has_api_key: false,
            chat_model: "mock-chat".into(),
            embedding_model: None,
            timeout_secs: 10,
            streaming: true,
            tool_support: false,
            privacy_profile: cc_ai_core::PrivacyProfile::Strict,
            is_default: true,
            created_at_ms: 0,
            updated_at_ms: 0,
        })
        .await
        .unwrap();
    let (owner, u) = app.unlocked().await.unwrap();
    let profile = owner.profile_id.to_string();
    let nonce = u.notices.scope_id().unwrap();
    drop(u);
    let mut captures = Vec::new();
    for (kind, subject) in [
        ("hostOs", host.id.as_str()),
        ("runnerVersions", runner.id.as_str()),
    ] {
        let request = json!({"operationId":Uuid::new_v4(),"profileId":profile,"notificationScopeId":nonce,"projectId":project.id,"expectedProjectRevision":1,"kind":kind,"subjectId":subject});
        let value: Value = serde_json::from_str(
            &app.automation_ai_context_capture_json(request.to_string())
                .await
                .unwrap(),
        )
        .unwrap();
        assert_eq!(value.as_object().unwrap().len(), 13);
        assert_eq!(value["notificationScopeId"], nonce);
        assert_eq!(value["projectRevision"], 1);
        assert!(value["subjectRevision"].as_u64().unwrap() > 0);
        if kind == "hostOs" {
            assert_eq!(value["source"], "sshFixedOsV1");
            assert!(!value["fields"]["architecture"].as_str().unwrap().is_empty());
        } else {
            assert_eq!(value["source"], "runnerProbeProjectPinsV1");
            assert!(!value["fields"]["python"].as_str().unwrap().is_empty());
            assert!(!value["fields"]["ansibleCore"].as_str().unwrap().is_empty());
            assert_eq!(value["fields"]["collections"], json!([]));
        }
        captures.push(value);
    }
    assert_eq!(mock.count("/v1/chat/completions"), 0);
    let request = json!({"profileId":profile,"notificationScopeId":nonce,"projectId":project.id,"expectedProjectRevision":1,"providerId":provider.id,"task":"Propose a read-only debug task","path":"playbook.yml","selectedText":null,"selectedGroups":[],"selectedTags":[],"selectedHostId":null,
        "selections":[{"captureId":captures[0]["captureId"],"fieldIds":["architecture"]},{"captureId":captures[1]["captureId"],"fieldIds":["ansibleCore","python","collections"]}]});
    let preview: Value = serde_json::from_str(
        &app.automation_ai_prepare_context_json(request.to_string())
            .await
            .unwrap(),
    )
    .unwrap();
    let exact = preview["preview"].as_str().unwrap();
    assert!(exact.contains("Selected observations"));
    assert!(!exact.contains("127.0.0.1"));
    assert_eq!(mock.count("/v1/chat/completions"), 0);
    assert!(
        app.automation_ai_prepare_context_json(request.to_string())
            .await
            .is_err()
    );
    mock.push_chat(r#"{"version":1,"description":"Read-only proposal","files":[{"path":"playbook.yml","content":"---\n- hosts: all\n  gather_facts: false\n  tasks: []\n"}],"variables":[],"dependencies":[],"expected_changes":[],"checks":[],"warnings":[]}"#);
    let review = preview["review_id"].as_str().unwrap().to_owned();
    app.automation_ai_generate(review.clone()).await.unwrap();
    assert!(app.automation_ai_generate(review).await.is_err());
    assert_eq!(mock.count("/v1/chat/completions"), 1);
    let sent: Value = serde_json::from_str(&mock.requests().last().unwrap().body).unwrap();
    let actual = sent["messages"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v["content"].as_str().unwrap())
        .collect::<Vec<_>>()
        .join("\n\n");
    assert_eq!(actual, exact);
    assert_eq!(
        app.automation_project_files(project.id.clone())
            .await
            .unwrap(),
        before
    );
    assert!(
        app.automation_jobs().await.unwrap()["jobs"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    publish(
        &root,
        "native-result.json",
        json!({"token":token,"ok":true,"actualFixedOs":true,"freshConnectedRunnerProbe":true,"projectPins":true,"previewBeforeModel":true,"oneUse":true,"exactPreviewSent":true,"jobs":0,"sourceUnchanged":true}),
    );
    app.shutdown().await.unwrap();
}
