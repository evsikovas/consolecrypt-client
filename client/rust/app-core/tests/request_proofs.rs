//! Protocol 1.5 per-request device proofs and rollback detection through
//! the facade, against the sync-core mock server with proofs **required**:
//! every API client app-core creates (new synced profile, reopened
//! profile, enable-sync, re-authentication) signs with the profile's device
//! key; vault info from the server is handed to the sync engine, which
//! reports a server restored from an older backup.

mod common;

use cc_app_core::*;
use cc_protocol::VaultId;
use cc_sync_core::mock::{MockServer, MockServerConfig};
use common::*;
use std::str::FromStr;
use std::time::Duration;

async fn server() -> MockServer {
    MockServer::start_with(MockServerConfig {
        require_request_proof: true,
        ..Default::default()
    })
    .await
}

fn assert_all_signed(server: &MockServer) {
    let s = server.request_proof_stats();
    assert!(s.valid > 0, "{s:?}");
    assert_eq!((s.missing, s.rejected), (0, 0), "{s:?}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn every_client_signs_requests_and_rollbacks_are_reported() {
    let server = server().await;
    let url = server.url().to_string();
    let tmp = tempfile::tempdir().unwrap();
    let store = memory_store();
    let dir = tmp.path().join("a");

    // New synced profile + vault + sync.
    let a = app_with(&dir, store.clone(), false);
    let acct = a
        .create_synced_profile(
            "Work".into(),
            url.clone(),
            "alice@example.org".into(),
            ACCOUNT_PASSWORD.into(),
            AccountMode::Register,
        )
        .await
        .unwrap();
    let kit = a.create_vault(PASSPHRASE.into()).await.unwrap();
    let vid = VaultId::from_str(&kit.vault_id).unwrap();
    a.save_host(host("web", "192.0.2.10", None)).await.unwrap();
    a.sync_now().await.unwrap();
    assert_eq!(a.list_remote_vaults().await.unwrap().len(), 1);
    // Token refresh is signed too.
    server.expire_access_tokens();
    a.sync_now().await.unwrap();
    assert_all_signed(&server);

    // Restart: the reopened profile's client signs as well.
    a.shutdown().await.unwrap();
    let a = app_with(&dir, store.clone(), false);
    a.open_profile(acct.profile.id.clone()).await.unwrap();
    a.unlock_with_passphrase(PASSPHRASE.into()).await.unwrap();
    a.sync_now().await.unwrap();
    assert_all_signed(&server);

    // Re-authentication keeps signing with the (same) device key.
    let st = a
        .reauthenticate(ACCOUNT_PASSWORD.into(), false)
        .await
        .unwrap();
    assert!(st.trusted, "{st:?}");
    a.sync_now().await.unwrap();
    assert_all_signed(&server);

    // The operator restores the server from a backup (epoch rotated):
    // listing vaults hands the info to the engine, which reports it.
    let mut events = a.subscribe_events();
    server.rotate_epoch(vid);
    a.list_remote_vaults().await.unwrap();
    let reason = tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            if let Ok(AppEvent::ServerRollbackDetected { vault_id, reason }) = events.recv().await {
                assert_eq!(vault_id, kit.vault_id);
                return reason;
            }
        }
    })
    .await
    .expect("rollback event");
    assert_eq!(reason, "epoch_changed");
    a.shutdown().await.unwrap();

    // Enable sync for a Local profile: that client signs from the start.
    let b = common::app(&tmp.path().join("b"));
    b.create_local_profile("Local".into(), PASSPHRASE.into())
        .await
        .unwrap();
    b.save_host(host("db", "192.0.2.20", None)).await.unwrap();
    b.enable_sync(
        url,
        "bob@example.org".into(),
        ACCOUNT_PASSWORD.into(),
        AccountMode::Register,
    )
    .await
    .unwrap();
    b.sync_now().await.unwrap();
    assert_all_signed(&server);
    b.shutdown().await.unwrap();
}
