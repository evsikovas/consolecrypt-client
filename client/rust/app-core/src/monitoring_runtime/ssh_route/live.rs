//! Actual owned SSH direct/jump + HTTPS transport. A fixed HTTP fixture proves
//! wire/authority/fences, not Prometheus metric freshness or endpoint identity.
use super::*;
use crate::{HostDto, config::AppConfig};
use cc_models::host::HostKeyPolicy;
use cc_monitoring_core::{AuthRef, TlsConfig};
use cc_ssh_core::keys::{KeyGenAlgorithm, generate_key};
use secrecy::ExposeSecret;
use std::path::{Path, PathBuf};

const PASSWORD: &str = "owned monitoring SSH profile password42";
fn publish(root: &Path, name: &str, value: Value) {
    let bytes = serde_json::to_vec(&value).unwrap();
    assert!(bytes.len() <= 16384);
    let next = root.join(format!("{name}.next"));
    std::fs::write(&next, bytes).unwrap();
    std::fs::rename(next, root.join(name)).unwrap();
}
async fn observed(root: &Path, name: &str) -> Value {
    tokio::time::timeout(Duration::from_secs(25), async {
        loop {
            let path = root.join(name);
            if path.exists() {
                assert!(!path.is_symlink() && path.metadata().unwrap().len() <= 16384);
                return serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
            }
            tokio::time::sleep(Duration::from_millis(30)).await;
        }
    })
    .await
    .unwrap()
}
fn request(source: &str, operation: Uuid) -> String {
    json!({"operationId":operation,"sourceId":source,"sourceRevision":1,
        "action":{"kind":"query","expression":"up","time":null}})
    .to_string()
}
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires exact owned SSH+HTTPS coordinator; no production/user endpoints"]
async fn owned_saved_host_direct_jump_https_credentials_and_original_fences() {
    let token = std::env::var("CC_MON_SSH_TOKEN").unwrap();
    assert_eq!(Uuid::parse_str(&token).unwrap().to_string(), token);
    let root = PathBuf::from(std::env::var_os("CC_MON_SSH_ROOT").unwrap());
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(4)
        .unwrap();
    assert_eq!(
        root.canonicalize().unwrap().parent(),
        Some(
            workspace
                .join("Tasks/mon-ssh/control")
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
    let key = generate_key(KeyGenAlgorithm::Ed25519, "Owned monitoring SSH", None).unwrap();
    let rcgen::CertifiedKey { cert, signing_key } =
        rcgen::generate_simple_self_signed(vec!["monitoring-original.invalid".into()]).unwrap();
    // Synthetic endpoint TLS key only. The Native SSH key/profile are NEVER
    // written here or mounted into the guest.
    let server = root.join("server");
    std::fs::create_dir(&server).unwrap();
    std::fs::write(server.join("tls.pem"), cert.pem()).unwrap();
    std::fs::write(server.join("tls.key"), signing_key.serialize_pem()).unwrap();
    publish(
        &root,
        "native-public.json",
        json!({"token":token,"publicKey":key.public_openssh}),
    );
    let fixture = observed(&root, "fixture.json").await;
    assert_eq!(fixture["token"], token);
    let port = u16::try_from(fixture["port"].as_u64().unwrap()).unwrap();
    assert!(port > 0);
    let host_key = fixture["hostKey"].as_str().unwrap();
    assert!(host_key.starts_with("ssh-ed25519 ") && !host_key.chars().any(char::is_control));
    let data = tempfile::Builder::new()
        .prefix("data-")
        .tempdir_in(&root)
        .unwrap();
    let app = Arc::new(
        AppCore::new(AppConfig::for_tests(
            data.path().to_string_lossy().into_owned(),
        ))
        .unwrap(),
    );
    app.create_local_profile("Owned monitoring transport".into(), PASSWORD.into())
        .await
        .unwrap();
    let credential = app
        .import_ssh_key(
            "Owned SSH".into(),
            Some("runner".into()),
            key.private_openssh.expose_secret().to_owned(),
            None,
            false,
            None,
        )
        .await
        .unwrap();
    app.import_known_hosts(format!(
        "[127.0.0.1]:{port} {host_key}\n[127.0.0.1]:2222 {host_key}\n"
    ))
    .await
    .unwrap();
    let mut direct = HostDto::new("Explicit direct saved host", "127.0.0.1");
    direct.port = Some(port);
    direct.username = Some("runner".into());
    direct.credential_id = Some(credential.id.clone());
    direct.host_key_policy = HostKeyPolicy::Strict;
    let direct = app.save_host(direct).await.unwrap();
    let mut jump = HostDto::new("Explicit target through saved jump", "127.0.0.1");
    jump.port = Some(2222);
    jump.username = Some("runner".into());
    jump.credential_id = Some(credential.id);
    jump.host_key_policy = HostKeyPolicy::Strict;
    jump.jump_chain = vec![direct.id.clone()];
    let jump = app.save_host(jump).await.unwrap();
    let basic = app
        .add_password_credential(
            "Explicit monitoring Basic".into(),
            Some("scoped-user".into()),
            "owned-http-password42".into(),
        )
        .await
        .unwrap();
    let source = |host_id: &str, auth: AuthRef| SourceConfig {
        id: String::new(),
        name: "Explicit original TLS source".into(),
        endpoint: "https://monitoring-original.invalid:19091/prefix".into(),
        route: Route::SshHost {
            host_id: host_id.into(),
        },
        tls: TlsConfig {
            custom_ca_pem: Some(cert.pem()),
        },
        auth,
    };
    let first = app
        .monitoring_source_create(0, source(&direct.id, AuthRef::None))
        .await
        .unwrap();
    let direct_id = first.sources[0].id.clone();
    let second = app
        .monitoring_source_create(
            1,
            source(
                &jump.id,
                AuthRef::BasicRef {
                    credential_id: basic.id,
                },
            ),
        )
        .await
        .unwrap();
    let jump_id = second
        .sources
        .iter()
        .find(|s| s.id != direct_id)
        .unwrap()
        .id
        .clone();
    let original_nonce = app.unlocked().await.unwrap().1.notices.scope_id().unwrap();
    for id in [&direct_id, &jump_id] {
        let value = app
            .monitoring_read(request(id, Uuid::new_v4()))
            .await
            .unwrap();
        assert_eq!(value["result"]["status"], "success");
        assert!(!value.to_string().contains("owned-http-password42"));
    }
    // Real host-key verification must deny a different key before any HTTP.
    for known in app.list_known_hosts().await.unwrap() {
        app.remove_known_host(known.id).await.unwrap();
    }
    let wrong = generate_key(KeyGenAlgorithm::Ed25519, "Wrong owned key", None).unwrap();
    app.import_known_hosts(format!("[127.0.0.1]:{port} {}\n", wrong.public_openssh))
        .await
        .unwrap();
    assert!(
        app.monitoring_read(request(&direct_id, Uuid::new_v4()))
            .await
            .is_err()
    );
    publish(
        &root,
        "phase.json",
        json!({"token":token,"phase":"trustDenied"}),
    );
    assert_eq!(
        observed(&root, "trustDenied-observed.json").await["requests"],
        2
    );
    for known in app.list_known_hosts().await.unwrap() {
        app.remove_known_host(known.id).await.unwrap();
    }
    app.import_known_hosts(format!(
        "[127.0.0.1]:{port} {host_key}\n[127.0.0.1]:2222 {host_key}\n"
    ))
    .await
    .unwrap();
    // Actual source CAS during an HTTP response must retire the old operation.
    let reader = app.clone();
    let input = request(&direct_id, Uuid::new_v4());
    let pending = tokio::spawn(async move { reader.monitoring_read(input).await });
    publish(
        &root,
        "phase.json",
        json!({"token":token,"phase":"pendingSource"}),
    );
    assert_eq!(
        observed(&root, "pendingSource-observed.json").await["requests"],
        3
    );
    let mut changed = source(&direct.id, AuthRef::None);
    changed.id = direct_id.clone();
    changed.name = "Explicit changed source".into();
    app.monitoring_source_update(direct_id.clone(), 2, 1, changed)
        .await
        .unwrap();
    assert!(pending.await.unwrap().is_err());
    assert!(
        app.monitoring_read(request(&direct_id, Uuid::new_v4()))
            .await
            .is_err()
    );
    // Actual lock/reunlock closes the original scope; no late HTTP result is
    // adopted by the new unlock even with identical profile/source IDs.
    let reader = app.clone();
    let input = request(&jump_id, Uuid::new_v4());
    let pending = tokio::spawn(async move { reader.monitoring_read(input).await });
    publish(
        &root,
        "phase.json",
        json!({"token":token,"phase":"pendingLock"}),
    );
    assert_eq!(
        observed(&root, "pendingLock-observed.json").await["requests"],
        4
    );
    app.lock().await.unwrap();
    assert!(pending.await.unwrap().is_err());
    app.unlock_with_passphrase(PASSWORD.into()).await.unwrap();
    assert_ne!(
        app.unlocked().await.unwrap().1.notices.scope_id().unwrap(),
        original_nonce
    );
    assert!(
        app.automation_jobs().await.unwrap()["jobs"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    app.shutdown().await.unwrap();
    publish(
        &root,
        "native-result.json",
        json!({"token":token,"ok":true,"direct":true,"jump":true,
        "originalHttpsIdentity":true,"explicitCredentialOnly":true,"wrongHostKeyDenied":true,
        "sourceCasFence":true,"lockFence":true,"jobs":0}),
    );
}
