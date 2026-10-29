mod common;

use cc_app_core::*;
use common::*;

fn options() -> RdpConnectConfig {
    RdpConnectConfig {
        address: "127.0.0.1".into(),
        port: 9,
        username: "test".into(),
        domain: None,
        width: 800,
        height: 600,
        accepted_certificate_sha256: [1; 32],
    }
}

#[tokio::test]
async fn rdp_cannot_access_network_or_input_without_an_unlocked_profile() {
    let dir = tempfile::tempdir().unwrap();
    let app = app_with(dir.path(), memory_store(), false);
    assert_eq!(
        app.rdp_probe_certificate("127.0.0.1".into(), 9)
            .await
            .unwrap_err()
            .code(),
        "no_active_profile"
    );
    app.create_local_profile("RDP test".into(), PASSPHRASE.into())
        .await
        .unwrap();
    app.lock().await.unwrap();
    assert_eq!(
        app.rdp_probe_certificate("127.0.0.1".into(), 9)
            .await
            .unwrap_err()
            .code(),
        "vault_locked"
    );
    let password = uuid::Uuid::new_v4().to_string();
    assert_eq!(
        app.rdp_connect(options(), password.into())
            .await
            .unwrap_err()
            .code(),
        "vault_locked"
    );
    assert_eq!(
        app.rdp_poll("missing".into()).await.unwrap_err().code(),
        "vault_locked"
    );
    assert_eq!(
        app.rdp_send_input("missing".into(), vec![RdpInput::ReleaseAll])
            .await
            .unwrap_err()
            .code(),
        "vault_locked"
    );
    app.rdp_disconnect("missing".into()).await.unwrap();
    app.shutdown().await.unwrap();
}

#[tokio::test]
async fn a_lock_keeps_rdp_sessions_and_closing_the_profile_ends_them() {
    let dir = tempfile::tempdir().unwrap();
    let app = app_with(dir.path(), memory_store(), false);
    app.create_local_profile("RDP test".into(), PASSPHRASE.into())
        .await
        .unwrap();
    // Connection refusal is immaterial: this tests session ownership, not a
    // reachable RDP server, and credentials are generated per invocation.
    let id = app
        .rdp_connect(options(), uuid::Uuid::new_v4().to_string().into())
        .await
        .unwrap();
    // An idle auto-lock must not drop the remote desktop (BUG-01).
    app.lock().await.unwrap();
    app.unlock_with_passphrase(PASSPHRASE.into()).await.unwrap();
    app.rdp_poll(id.clone()).await.unwrap();

    // Closing the profile ends its sessions.
    let profile = app.active_profile().await.unwrap().unwrap();
    app.close_profile().await.unwrap();
    app.open_profile(profile.id.clone()).await.unwrap();
    app.unlock_with_passphrase(PASSPHRASE.into()).await.unwrap();
    assert!(matches!(
        app.rdp_poll(id).await,
        Err(AppError::Rdp(RdpError::SessionNotFound))
    ));
    app.shutdown().await.unwrap();
}
