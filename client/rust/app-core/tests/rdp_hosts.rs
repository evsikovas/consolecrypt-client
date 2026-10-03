mod common;
use cc_app_core::*;
use common::*;

fn rdp(name: &str) -> HostDto {
    let mut h = HostDto::new(name, "windows.example.test");
    h.protocol = HostProtocol::Rdp;
    h.username = Some("operator".into());
    h.rdp_domain = Some("LAB".into());
    h
}

#[tokio::test]
async fn rdp_inventory_survives_lock_and_owns_its_password_credential() {
    let dir = tempfile::tempdir().unwrap();
    let app = app(dir.path());
    app.create_local_profile("rdp".into(), PASSPHRASE.into())
        .await
        .unwrap();
    let password = uuid::Uuid::new_v4().to_string();
    let saved = app
        .save_host_with_auth(
            rdp("Windows"),
            HostAuth::InlinePassword {
                password: Some(password.clone()),
            },
        )
        .await
        .unwrap();
    assert_eq!(saved.protocol, HostProtocol::Rdp);
    assert_eq!(saved.rdp_domain.as_deref(), Some("LAB"));
    assert!(!serde_json::to_string(&saved).unwrap().contains(&password));
    assert!(
        app.plan_preview_host(saved.id.clone()).await.is_err(),
        "RDP must not become an SSH target"
    );
    app.lock().await.unwrap();
    app.unlock_with_passphrase(PASSPHRASE.into()).await.unwrap();
    let hosts = app.list_hosts().await.unwrap();
    assert_eq!(hosts.len(), 1);
    assert_eq!(hosts[0].protocol, HostProtocol::Rdp);
    assert_eq!(hosts[0].rdp_width, 1280);
    assert_eq!(hosts[0].port, None);
    assert!(
        matches!(
            app.delete_credential(saved.credential_id.clone().unwrap())
                .await,
            Err(AppError::InUse { .. })
        ),
        "a saved RDP host must keep its password credential"
    );
    app.delete_host(saved.id).await.unwrap();
    assert!(app.list_hosts().await.unwrap().is_empty());
    assert!(app.list_credentials().await.unwrap().is_empty());
    app.shutdown().await.unwrap();
}

#[tokio::test]
async fn rdp_disallows_ssh_auth_and_protocol_reinterpretation() {
    let dir = tempfile::tempdir().unwrap();
    let app = app(dir.path());
    app.create_local_profile("rdp".into(), PASSPHRASE.into())
        .await
        .unwrap();
    assert!(app
        .save_host_with_auth(rdp("Windows"), HostAuth::Inherit)
        .await
        .is_err());
    let ssh = app
        .save_host(HostDto::new("SSH", "ssh.example.test"))
        .await
        .unwrap();
    let mut changed = ssh.clone();
    changed.protocol = HostProtocol::Rdp;
    assert!(app
        .save_host_with_auth(
            changed,
            HostAuth::InlinePassword {
                password: Some(uuid::Uuid::new_v4().to_string())
            }
        )
        .await
        .is_err());
    assert!(app.list_credentials().await.unwrap().is_empty());
    let saved = app
        .save_host_with_auth(rdp("RDP"), HostAuth::PasswordPrompt)
        .await
        .unwrap();
    let mut changed = saved;
    changed.protocol = HostProtocol::Ssh;
    assert!(app.save_host(changed).await.is_err());
    assert_eq!(app.list_hosts().await.unwrap().len(), 2);
    app.shutdown().await.unwrap();
}

#[tokio::test]
async fn saved_connection_rejects_unconfirmed_or_stale_snapshot_without_connecting() {
    let dir = tempfile::tempdir().unwrap();
    let app = app(dir.path());
    app.create_local_profile("rdp".into(), PASSPHRASE.into())
        .await
        .unwrap();
    let h = app
        .save_host_with_auth(rdp("Windows"), HostAuth::PasswordPrompt)
        .await
        .unwrap();
    let ticket = RdpSavedHostTicket {
        host_id: h.id,
        name: h.name,
        address: h.address,
        port: 3389,
        username: "operator".into(),
        domain: "LAB".into(),
        width: 1280,
        height: 720,
        fingerprint: "00".repeat(32),
        snapshot_stamp: "stale".into(),
        has_saved_password: false,
    };
    let result = app
        .rdp_saved_host_connect(ticket, None, RdpSessionPermissions::default())
        .await;
    assert!(matches!(result,Err(AppError::InvalidInput{field,..}) if field=="host"));
    app.shutdown().await.unwrap();
}
