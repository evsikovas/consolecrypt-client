//! Pending vault deletion (protocol 1.13, TODO-01) against the in-process
//! mock server: a deletion scheduled from an untrusted device of the owner
//! is shown on a trusted device, which keeps the vault by cancelling it.

mod common;

use cc_app_core::*;
use cc_protocol::vaults::VaultState;
use cc_protocol::VaultId;
use cc_sync_core::mock::MockServer;
use common::*;
use std::str::FromStr;

#[tokio::test]
async fn a_trusted_device_sees_and_cancels_a_scheduled_deletion() {
    let server = MockServer::start().await;
    let tmp = tempfile::tempdir().unwrap();
    let app = common::app(tmp.path());
    app.create_synced_profile(
        "Work".into(),
        server.url().to_string(),
        "deletion@example.org".into(),
        ACCOUNT_PASSWORD.into(),
        AccountMode::Register,
    )
    .await
    .unwrap();
    let kit = app.create_vault(PASSPHRASE.into()).await.unwrap();
    let vault = VaultId::from_str(&kit.vault_id).unwrap();
    assert_eq!(app.vault_deletion_scheduled().await.unwrap(), None);

    // Another device of the owner, not yet trusted, asked for the deletion.
    assert!(server.schedule_vault_deletion(vault));
    let at = app
        .vault_deletion_scheduled()
        .await
        .unwrap()
        .expect("scheduled");
    let days = (at - chrono::Utc::now().timestamp_millis()) / 86_400_000;
    assert!((6..=7).contains(&days), "{days}");

    app.cancel_vault_deletion().await.unwrap();
    assert_eq!(server.vault_state(vault), Some(VaultState::Active));
    assert_eq!(app.vault_deletion_scheduled().await.unwrap(), None);
    // Nothing to cancel any more.
    assert!(app.cancel_vault_deletion().await.is_err());

    // A locked vault cannot prove its key.
    assert!(server.schedule_vault_deletion(vault));
    app.lock().await.unwrap();
    assert!(app.vault_deletion_scheduled().await.unwrap().is_some());
    assert!(app.cancel_vault_deletion().await.is_err());
}
