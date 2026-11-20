//! Private prerequisite tests, not software attestation/admission acceptance.
//! Policy metadata is explicit HTTP fixture data; the existing real engine,
//! atomic storage and codec are exercised, no fake writer capability exists.
mod common;
use cc_models::host::Host;
use cc_models::{ObjectPayload, VaultObject};
use cc_protocol::{Bytes, VaultId, paths};
use cc_sync_core::mock::{FaultAction, MockServer};
use cc_sync_core::*;
use common::*;
use serde_json::json;
use std::sync::Arc;
fn path(vault: VaultId) -> String {
    format!("/v1/vaults/{vault}/compatibility/monitoring-settings")
}
async fn observation(
    server: &MockServer,
    client: &Client,
    vault: VaultId,
) -> SettingsAccountPolicy {
    client.api.set_request_signer(Some(client.keys.signer()));
    let account = client.api.me().await.unwrap().user_id;
    let body = json!({"version":1,"scope":"personalAccount","accountId":account,"revision":1,"state":"enabled",
      "minimumClientVersion":"2.0.0","offerMinimumClientVersion":"2.0.0","rosterDigest":"a".repeat(64),
      "checkpoint":{"vaultId":vault,"vaultEpoch":server.epoch(vault).unwrap(),"vaultSequence":server.latest_sequence(vault)}});
    server.inject(
        &path(vault),
        1,
        FaultAction::RawResponse { status: 200, body },
    );
    client.api.settings_compatibility(vault).await.unwrap()
}
#[tokio::test]
async fn fixed_metadata_and_pull_only_checkpoint_apply_existing_ciphertext_without_push() {
    let server = MockServer::start().await;
    let (a, b, vault) = two_devices(&server).await;
    let host = Host::new("Owned checkpoint fixture", "192.0.2.1");
    a.engine
        .put(ObjectPayload::new(VaultObject::Host(host.clone())))
        .await
        .unwrap();
    a.engine.sync_now().await.unwrap();
    let policy = observation(&server, &b, vault.vault_id).await;
    let before = server.hits(paths::SYNC_PUSH);
    let applied = b.engine.pull_settings_checkpoint(&policy).await.unwrap();
    assert_eq!(applied.vault_id(), vault.vault_id);
    assert_eq!(applied.epoch(), server.epoch(vault.vault_id).unwrap());
    assert_eq!(applied.sequence(), 1);
    assert_eq!(applied.account_id(), policy.account_id());
    assert_eq!(server.hits(paths::SYNC_PUSH), before);
    assert!(
        matches!(b.store.get(host.id).await.unwrap().unwrap().payload.object,VaultObject::Host(h) if h.name==host.name)
    );
}
#[tokio::test]
async fn pending_edits_and_wrong_epoch_never_issue_checkpoint_data() {
    let server = MockServer::start().await;
    let (_a, b, vault) = two_devices(&server).await;
    let policy = observation(&server, &b, vault.vault_id).await;
    b.engine
        .put(ObjectPayload::new(VaultObject::Host(Host::new(
            "Owned pending",
            "192.0.2.2",
        ))))
        .await
        .unwrap();
    let before = server.hits(paths::SYNC_PUSH);
    assert!(b.engine.pull_settings_checkpoint(&policy).await.is_err());
    assert_eq!(server.hits(paths::SYNC_PUSH), before);
    // Separate clean owned device; stale epoch is refused without recovery upload.
    let server = MockServer::start().await;
    let (_a, b, vault) = two_devices(&server).await;
    let policy = observation(&server, &b, vault.vault_id).await;
    server.rotate_epoch(vault.vault_id);
    let before = server.hits(paths::SYNC_PUSH);
    assert!(b.engine.pull_settings_checkpoint(&policy).await.is_err());
    assert_eq!(server.hits(paths::SYNC_PUSH), before);
    let cursor = b
        .storage
        .read(move |tx| tx.get_sync_cursor(vault.vault_id))
        .await
        .unwrap();
    assert!(cursor.recovery.is_none());
    assert!(!cursor.snapshot_complete);
}
#[tokio::test]
async fn integrity_warning_is_not_a_successful_checkpoint_and_page_transaction_is_unchanged() {
    let server = MockServer::start().await;
    let (a, b, vault) = two_devices(&server).await;
    let host = Host::new("Owned integrity fixture", "192.0.2.3");
    a.engine
        .put(ObjectPayload::new(VaultObject::Host(host.clone())))
        .await
        .unwrap();
    a.engine.sync_now().await.unwrap();
    let policy = observation(&server, &b, vault.vault_id).await;
    server.inject(
        paths::SYNC_SNAPSHOT,
        1,
        FaultAction::Rewrite(Arc::new(|body| {
            body["objects"][0]["body"]["nonce"] = json!(Bytes::new(vec![0; 24]));
        })),
    );
    assert!(b.engine.pull_settings_checkpoint(&policy).await.is_err());
    assert!(b.store.get(host.id).await.unwrap().is_none());
    let cursor = b
        .storage
        .read(move |tx| tx.get_sync_cursor(vault.vault_id))
        .await
        .unwrap();
    assert!(!cursor.snapshot_complete);
    assert_eq!(cursor.last_sequence, 0);
}
#[tokio::test]
async fn checkpoint_page_budget_and_stopped_engine_are_fail_closed_without_push() {
    let server = MockServer::start().await;
    let (a, b, vault) = two_devices(&server).await;
    for n in 0..33 {
        a.engine
            .put(ObjectPayload::new(VaultObject::Host(Host::new(
                format!("Owned{n}"),
                "192.0.2.4",
            ))))
            .await
            .unwrap();
    }
    a.engine.sync_now().await.unwrap();
    let mut cfg = fast_config(b.keys.device_id);
    cfg.page_limit = 1;
    let engine = SyncEngine::new(b.store.clone(), b.api.clone(), cfg)
        .await
        .unwrap();
    let policy = observation(&server, &b, vault.vault_id).await;
    let before = server.hits(paths::SYNC_PUSH);
    let snapshots = server.hits(paths::SYNC_SNAPSHOT);
    assert!(engine.pull_settings_checkpoint(&policy).await.is_err());
    assert!(server.hits(paths::SYNC_SNAPSHOT) - snapshots <= 32);
    assert_eq!(server.hits(paths::SYNC_PUSH), before);
    engine.stop().await;
    assert!(matches!(
        engine.pull_settings_checkpoint(&policy).await,
        Err(SyncError::Stopped(_))
    ));
}

#[tokio::test]
async fn locked_codec_cannot_advance_a_strict_checkpoint_as_decrypted_recapture() {
    let server = MockServer::start().await;
    let (a, b, vault) = two_devices(&server).await;
    let host = Host::new("Owned locked fixture", "192.0.2.5");
    a.engine
        .put(ObjectPayload::new(VaultObject::Host(host)))
        .await
        .unwrap();
    a.engine.sync_now().await.unwrap();
    let policy = observation(&server, &b, vault.vault_id).await;
    b.codec.set_locked(true);
    assert!(b.engine.pull_settings_checkpoint(&policy).await.is_err());
    let cursor = b
        .storage
        .read(move |tx| tx.get_sync_cursor(vault.vault_id))
        .await
        .unwrap();
    assert!(!cursor.snapshot_complete);
    assert_eq!(cursor.last_sequence, 0);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn once_policy_get_preserves_actual_me_subject_and_logout_denies_late_response() {
    let server = MockServer::start().await;
    let (_a, b, vault) = two_devices(&server).await;
    b.api.set_request_signer(Some(b.keys.signer()));
    let subject = b.api.capture_reader_auth().await.unwrap();
    let body = json!({"version":1,"scope":"personalAccount","accountId":subject.user_id(),"revision":0,"state":"off",
        "minimumClientVersion":null,"offerMinimumClientVersion":"2.0.0","rosterDigest":"a".repeat(64),
        "checkpoint":{"vaultId":vault.vault_id,"vaultEpoch":server.epoch(vault.vault_id).unwrap(),"vaultSequence":server.latest_sequence(vault.vault_id)}});
    let route = path(vault.vault_id);
    let refreshes = server.hits(paths::AUTH_REFRESH);
    server.inject(
        &route,
        1,
        FaultAction::RawResponse {
            status: 200,
            body: body.clone(),
        },
    );
    let policy = b
        .api
        .settings_compatibility_once(vault.vault_id)
        .await
        .unwrap();
    assert_eq!(policy.account_id(), subject.user_id());
    subject
        .check_current(&b.api)
        .expect("fixed read must not invalidate original authenticated provenance");
    server.inject(
        &route,
        1,
        FaultAction::RawResponse {
            status: 200,
            body: json!({"future":true}),
        },
    );
    assert_eq!(
        b.api
            .settings_compatibility_once(vault.vault_id)
            .await
            .unwrap_err(),
        SettingsCompatibilityError::InvalidResponse
    );
    subject.check_current(&b.api).unwrap();
    let (entered, wait_entered) = tokio::sync::oneshot::channel();
    let entered = Arc::new(std::sync::Mutex::new(Some(entered)));
    let (release, wait_release) = std::sync::mpsc::channel();
    let wait_release = Arc::new(std::sync::Mutex::new(wait_release));
    server.inject(
        &route,
        1,
        FaultAction::Rewrite(Arc::new(move |value| {
            entered.lock().unwrap().take().unwrap().send(()).unwrap();
            wait_release
                .lock()
                .unwrap()
                .recv_timeout(std::time::Duration::from_secs(5))
                .unwrap();
            *value = body.clone();
        })),
    );
    let api = b.api.clone();
    let id = vault.vault_id;
    let worker = tokio::spawn(async move { api.settings_compatibility_once(id).await });
    tokio::time::timeout(std::time::Duration::from_secs(5), wait_entered)
        .await
        .unwrap()
        .unwrap();
    subject.check_current(&b.api).expect("pending read must preserve original subject before logout entry");
    let api=b.api.clone();
    let logout=tokio::spawn(async move {api.logout(&cc_protocol::auth::LogoutRequest::default()).await});
    // Observe the ACTUAL original auth transition before releasing the held
    // response, without waiting for unrelated logout network completion.
    tokio::time::timeout(std::time::Duration::from_secs(5),subject.cancelled()).await.unwrap();
    release.send(()).unwrap();
    logout.await.unwrap().unwrap();
    assert_eq!(worker.await.unwrap().unwrap_err(), SettingsCompatibilityError::Unauthenticated);
    assert!(subject.check_current(&b.api).is_err());
    let before = server.hits(&route);
    assert!(
        b.api
            .settings_compatibility_once(vault.vault_id)
            .await
            .is_err()
    );
    assert_eq!(server.hits(&route), before);
    assert_eq!(server.hits(paths::AUTH_REFRESH), refreshes);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn once_policy_get_rejects_observer_invalidation_while_actual_response_is_held() {
    let server = MockServer::start().await;
    let (_a, b, vault) = two_devices(&server).await;
    b.api.set_request_signer(Some(b.keys.signer()));
    let subject = b.api.capture_reader_auth().await.unwrap();
    let body = json!({"version":1,"scope":"personalAccount","accountId":subject.user_id(),"revision":0,"state":"off",
        "minimumClientVersion":null,"offerMinimumClientVersion":"2.0.0","rosterDigest":"a".repeat(64),
        "checkpoint":{"vaultId":vault.vault_id,"vaultEpoch":server.epoch(vault.vault_id).unwrap(),"vaultSequence":0}});
    let route = path(vault.vault_id);
    let refreshes = server.hits(paths::AUTH_REFRESH);
    let (entered, wait_entered) = tokio::sync::oneshot::channel();
    let entered = Arc::new(std::sync::Mutex::new(Some(entered)));
    let (release, wait_release) = std::sync::mpsc::channel();
    let wait_release = Arc::new(std::sync::Mutex::new(wait_release));
    server.inject(
        &route,
        1,
        FaultAction::Rewrite(Arc::new(move |value| {
            entered.lock().unwrap().take().unwrap().send(()).unwrap();
            wait_release
                .lock()
                .unwrap()
                .recv_timeout(std::time::Duration::from_secs(5))
                .unwrap();
            *value = body.clone();
        })),
    );
    let api = b.api.clone();
    let id = vault.vault_id;
    let worker = tokio::spawn(async move { api.settings_compatibility_once(id).await });
    tokio::time::timeout(std::time::Duration::from_secs(5), wait_entered)
        .await
        .unwrap()
        .unwrap();
    b.api.invalidate_reader_auth();
    release.send(()).unwrap();
    assert_eq!(worker.await.unwrap().unwrap_err(), SettingsCompatibilityError::Unauthenticated);
    assert!(subject.check_current(&b.api).is_err());
    assert_eq!(server.hits(paths::AUTH_REFRESH), refreshes);
}
