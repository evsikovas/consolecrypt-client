//! Real PreparedReview/default validation/SSH, not a reconstructed review.
//! Requires the bounded owned password-SSH/Node-custody coordinator.
use super::*;
use crate::{
    HostDto, automation_prepare::AutomationRunRequest, automation_runners::AutomationRunnerConfig,
};
use cc_automation_core::{RemoteHelperConfig, RunParameters};
use cc_models::{automation::JobMode, host::HostKeyPolicy};
use cc_ssh_core::keys::{KeyGenAlgorithm, generate_key};
use secrecy::{ExposeSecret, SecretString};
use std::{
    io::{Read, Write},
    path::{Path, PathBuf},
    time::Duration,
};
use zeroize::Zeroizing;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Fixture {
    version: u8,
    token: String,
    runner_port: u16,
    runner_key: String,
    target_address: String,
    target_port: u16,
    target_key: String,
    #[serde(deserialize_with = "secret_input")]
    target_password: SecretString,
    service_signing: String,
    service_recipient: String,
    schedule_id: Uuid,
    definition_digest: String,
    egress_blocked: bool,
}
fn secret_input<'de, D: serde::Deserializer<'de>>(d: D) -> Result<SecretString, D::Error> {
    String::deserialize(d).map(SecretString::from)
}
fn publish(root: &Path, name: &str, value: Value) {
    let raw = serde_json::to_vec(&value).unwrap();
    assert!(raw.len() <= 65536);
    let mut f = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(root.join(name))
        .unwrap();
    f.write_all(&raw).unwrap();
    f.sync_all().unwrap();
}
async fn receive(root: &Path, name: &str) -> Value {
    tokio::time::timeout(Duration::from_secs(45), async {
        loop {
            let p = root.join(name);
            if let Ok(metadata) = std::fs::symlink_metadata(&p) {
                assert!(
                    metadata.is_file()
                        && !metadata.file_type().is_symlink()
                        && metadata.len() <= 16384
                );
                return serde_json::from_slice(&std::fs::read(p).unwrap()).unwrap();
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("owned service custody acknowledgement deadline")
}
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires owned pinned password-SSH Runner/target plus real Node custody coordinator"]
async fn actual_prepared_review_explicit_service_consent_and_owner_signature_reach_same_node_custody()
 {
    let token = std::env::var("CC_SERVICE_PROOF_TOKEN").unwrap();
    let id = Uuid::parse_str(&token).unwrap();
    assert!(!id.is_nil() && id.to_string() == token);
    let root = PathBuf::from(std::env::var_os("CC_SERVICE_PROOF_ROOT").unwrap());
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(4)
        .unwrap();
    let allowed = workspace
        .join("Tasks/service-v4/control")
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
    let key = generate_key(KeyGenAlgorithm::Ed25519, "owned service Runner", None).unwrap();
    publish(
        &root,
        "native-public.json",
        json!({"version":1,"token":token,"publicKey":key.public_openssh}),
    );
    let mut raw = Zeroizing::new(Vec::new());
    std::io::stdin().take(65537).read_to_end(&mut raw).unwrap();
    assert!(raw.len() <= 65536);
    let fixture: Fixture = serde_json::from_slice(&raw).unwrap();
    drop(raw);
    assert_eq!(fixture.version, 1);
    assert_eq!(fixture.token, token);
    assert!(fixture.runner_port > 0 && fixture.target_port == 2222);
    assert!(
        fixture
            .target_address
            .parse::<std::net::Ipv4Addr>()
            .unwrap()
            .is_private()
    );
    assert!((32..=128).contains(&fixture.target_password.expose_secret().len()));
    assert!(
        !fixture.schedule_id.is_nil()
            && cc_models::automation::is_digest(&fixture.definition_digest)
    );
    let profile = tempfile::Builder::new()
        .prefix("native-")
        .tempdir_in(&root)
        .unwrap();
    let app = AppCore::new(crate::AppConfig::for_tests(
        profile.path().to_string_lossy().into_owned(),
    ))
    .unwrap();
    app.create_local_profile(
        "Owned service consent".into(),
        "synthetic owner passphrase42".into(),
    )
    .await
    .unwrap();
    let ssh = app
        .import_ssh_key(
            "Owned Runner key".into(),
            Some("runner".into()),
            key.private_openssh.expose_secret().to_owned(),
            None,
            false,
            None,
        )
        .await
        .unwrap();
    let password = app
        .add_password_credential(
            "Explicit owned target password".into(),
            Some("runner".into()),
            fixture.target_password.expose_secret().to_owned(),
        )
        .await
        .unwrap();
    let mut hosts = Vec::new();
    for (address, port, host_key, credential) in [
        (
            "127.0.0.1",
            fixture.runner_port,
            fixture.runner_key.as_str(),
            ssh.id.as_str(),
        ),
        (
            fixture.target_address.as_str(),
            fixture.target_port,
            fixture.target_key.as_str(),
            password.id.as_str(),
        ),
    ] {
        assert!(
            host_key.starts_with("ssh-ed25519 ")
                && host_key.len() < 8192
                && !host_key.chars().any(char::is_control)
        );
        app.import_known_hosts(format!("[{address}]:{port} {host_key}\n"))
            .await
            .unwrap();
        let mut host = HostDto::new("Owned service proof node", address);
        host.port = Some(port);
        host.username = Some("runner".into());
        host.credential_id = Some(credential.into());
        host.host_key_policy = HostKeyPolicy::Strict;
        hosts.push(app.save_host(host).await.unwrap());
    }
    let runner = app
        .automation_runner_create(
            "Owned service Runner".into(),
            AutomationRunnerConfig::Remote {
                host_id: hosts[0].id.clone(),
                helper: RemoteHelperConfig {
                    python: "/opt/cc-runtime/bin/python".into(),
                    module_dir: "/opt/consolecrypt/src".into(),
                    root: format!("/home/runner/proof-{token}/jobs"),
                },
            },
        )
        .await
        .unwrap();
    app.automation_runner_connect(runner.id.clone())
        .await
        .unwrap();
    let project = app
        .automation_project_create("Owned service source".into(), None)
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
    let before = app
        .automation_project_files(project.id.clone())
        .await
        .unwrap();
    let request = AutomationRunRequest {
        project_id: project.id.clone(),
        runner_id: runner.id.clone(),
        playbook: "playbook.yml".into(),
        host_ids: vec![hosts[1].id.clone()],
        mode: JobMode::Check,
        timeout_seconds: 120,
        forks: 1,
        cancel_on_lock: false,
        parameters: RunParameters {
            extra_vars: BTreeMap::new(),
            tags: vec![],
            skip_tags: vec![],
            diff: false,
            vault: None,
        },
        retry_of: None,
        vault_credential_id: None,
        vault_selection: None, deployment_environment:None,
    };
    let prepared = app.automation_prepare_run(request.clone()).await.unwrap();
    assert_eq!(prepared["validation"]["valid"], true);
    let review = prepared["review_id"].as_str().unwrap().to_owned();
    let (owner, unlock) = app.unlocked().await.unwrap();
    let profile_id = owner.profile_id.to_string();
    let owner_id = owner.device_id().to_string();
    let keys = owner.identity().public_keys();
    drop(unlock);
    let actual_targets = {
        let reviews = owner.automation.reviews.lock().unwrap();
        reviews
            .get(&Uuid::parse_str(&review).unwrap())
            .unwrap()
            .targets
            .clone()
    };
    let mut selected = terms();
    selected.service_id = Uuid::parse_str(&runner.id).unwrap();
    selected.schedule_id = fixture.schedule_id;
    selected.definition_digest = fixture.definition_digest.clone();
    selected.signer.public_key = fixture.service_signing.clone();
    selected.recipient.public_key = fixture.service_recipient.clone();
    selected.access_mode = ServiceAccessMode::BoundedOffline;
    let preview = app
        .service_delegation_preview(profile_id.clone(), review.clone(), selected)
        .await
        .unwrap();
    assert_eq!(preview.version, 4);
    assert_eq!(
        preview.access_policy.mode,
        ServiceAccessMode::BoundedOffline
    );
    assert_eq!(
        preview.access_policy.offline_not_after,
        Some(preview.expires_at)
    );
    let confirmation = app
        .service_confirm_delegation(profile_id.clone(), review.clone(), preview.id.clone())
        .await
        .unwrap();
    let verified = SignedServiceDelegation::verify_json(
        &confirmation.delegation.to_json().unwrap(),
        &owner_id,
        &keys,
        &preview.tenant,
        now().unwrap(),
    )
    .unwrap();
    assert!(same(verified.body(), &preview).unwrap());
    assert!(
        app.service_confirm_delegation(profile_id.clone(), review.clone(), preview.id.clone())
            .await
            .is_err()
    );
    assert!(
        app.automation_jobs().await.unwrap()["jobs"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    // A second genuine review proves normal execution cannot consume service
    // consent. Neither policy nor a PreparedReview is fabricated by this test.
    let other = app.automation_prepare_run(request).await.unwrap();
    assert_eq!(other["validation"]["valid"], true);
    let other_id = other["review_id"].as_str().unwrap().to_owned();
    let mut other_terms = terms();
    other_terms.service_id = Uuid::parse_str(&runner.id).unwrap();
    let other_policy = app
        .service_delegation_preview(profile_id.clone(), other_id.clone(), other_terms)
        .await
        .unwrap();
    assert!(app.automation_confirm_run(other_id.clone()).await.is_err());
    assert!(
        app.service_confirm_delegation(profile_id, other_id, other_policy.id)
            .await
            .is_err()
    );
    assert_eq!(
        app.automation_project_files(project.id.clone())
            .await
            .unwrap(),
        before
    );
    let policy_json =
        serde_json::from_slice::<Value>(&confirmation.delegation.to_json().unwrap()).unwrap();
    let initial_json =
        serde_json::from_slice::<Value>(&confirmation.initial_status.to_json().unwrap()).unwrap();
    app.shutdown().await.unwrap();
    publish(
        &root,
        "confirmed-policy.json",
        json!({"version":1,"token":token,"delegation":policy_json,"initialStatus":initial_json,"targets":actual_targets,"ownerId":owner_id,"ownerSigning":hex::encode(keys.signing),"ownerEncryption":hex::encode(keys.encryption),"ownerNotAfter":preview.expires_at,"nativeClosed":true,"normalJobCount":0}),
    );
    let stored = receive(&root, "stored-custody.json").await;
    assert_eq!(stored["token"], token);
    assert_eq!(stored["durable"], true);
    assert_eq!(stored["dispatch"], false);
    assert_eq!(
        stored["delegationDigest"],
        confirmation.delegation.digest().unwrap()
    );
    publish(
        &root,
        "native-result.json",
        json!({"version":1,"token":token,"ok":true,"genuinePreparedReview":true,"explicitServicePreviewAndConfirm":true,"oneUse":true,"normalRunPurposeDenied":true,"originalUnchanged":true,"nativeClosedBeforeCustody":true,"egressBlocked":fixture.egress_blocked,"serviceTasksExecuted":false}),
    );
}
