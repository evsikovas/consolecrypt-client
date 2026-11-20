//! Draft actual typed API/MockServer acceptance, not a fabricated Reader subject.
mod common;
use cc_protocol::auth::LogoutRequest;
use cc_protocol::version::Platform;
use cc_sync_core::mock::MockServer;
use cc_sync_core::{ApiClient, ApiConfig, ApiError, MemoryTokenStore};
use common::*;
use std::sync::Arc;

#[tokio::test]
async fn real_me_subject_matches_registered_user_device_session_and_exact_original_client() {
    let server = MockServer::start().await;
    let keys = DeviceKeys::generate();
    let (api, account) = register(
        &server,
        &format!("reader-{}@example.test", uuid::Uuid::new_v4()),
        &keys,
    )
    .await;
    let tokens = api.refresh().await.unwrap();
    let subject = api.capture_reader_auth().await.unwrap();
    assert_eq!(subject.user_id(), account.user_id);
    assert_eq!(subject.device_id(), keys.device_id);
    assert_eq!(subject.session_id(), tokens.session_id);
    assert_eq!(subject.server_url(), api.config().server_url.as_str());
    assert!(subject.check_current(&api.clone()).is_ok());
    let other = ApiClient::new(
        ApiConfig::new(server.url().as_str(), "t", Platform::Cli).unwrap(),
        Arc::new(MemoryTokenStore::new()),
    )
    .unwrap();
    other.set_tokens(&tokens).await.unwrap();
    assert!(matches!(
        subject.check_current(&other),
        Err(ApiError::NotAuthenticated)
    ));
}

#[tokio::test]
async fn same_subject_real_refresh_cancels_old_observation_new_explicit_capture_preserves_user() {
    let server = MockServer::start().await;
    let keys = DeviceKeys::generate();
    let (api, _) = register(
        &server,
        &format!("reader-{}@example.test", uuid::Uuid::new_v4()),
        &keys,
    )
    .await;
    let first = api.capture_reader_auth().await.unwrap();
    api.refresh().await.unwrap();
    assert!(first.check_current(&api).is_err());
    tokio::time::timeout(std::time::Duration::from_secs(1), first.cancelled())
        .await
        .unwrap();
    let next = api.capture_reader_auth().await.unwrap();
    assert_eq!(next.user_id(), first.user_id());
    assert_eq!(next.device_id(), first.device_id());
    next.check_current(&api).unwrap();
}

#[tokio::test]
async fn explicit_invalidation_and_real_logout_do_not_rely_on_app_event_listener() {
    let server = MockServer::start().await;
    let keys = DeviceKeys::generate();
    let (api, _) = register(
        &server,
        &format!("reader-{}@example.test", uuid::Uuid::new_v4()),
        &keys,
    )
    .await;
    let subject = api.capture_reader_auth().await.unwrap();
    api.invalidate_reader_auth();
    assert!(subject
        .with_current(&api, || panic!("stale subject signed"))
        .is_err());
    let fresh = api.capture_reader_auth().await.unwrap();
    fresh.check_current(&api).unwrap();
    assert!(subject.check_current(&api).is_err()); // Fresh observation never revives old epoch.
    api.logout(&LogoutRequest::default()).await.unwrap();
    assert!(api.capture_reader_auth().await.is_err());
}

#[tokio::test]
async fn nested_readonly_check_inside_authoritative_fence_does_not_relock_or_deadlock() {
    let server = MockServer::start().await;
    let keys = DeviceKeys::generate();
    let (api, _) = register(
        &server,
        &format!("reader-{}@example.test", uuid::Uuid::new_v4()),
        &keys,
    )
    .await;
    let subject = api.capture_reader_auth().await.unwrap();
    let (sent, received) = std::sync::mpsc::channel();
    let child = std::thread::spawn(move || {
        let result = subject.with_current(&api, || {
            subject.check_current(&api)?;
            subject.check_current(&api)?;
            Ok::<_, ApiError>(subject.user_id())
        });
        let _ = sent.send(result);
    });
    received
        .recv_timeout(std::time::Duration::from_secs(2))
        .expect("nested auth observer deadlocked")
        .unwrap()
        .unwrap();
    child.join().unwrap();
}

#[tokio::test]
async fn concurrent_real_refreshes_serialize_without_destroying_legitimate_rotated_tokens() {
    let server = MockServer::start().await;
    let keys = DeviceKeys::generate();
    let (api, _) = register(
        &server,
        &format!("reader-{}@example.test", uuid::Uuid::new_v4()),
        &keys,
    )
    .await;
    let first_subject = api.capture_reader_auth().await.unwrap();
    let (first, second) = tokio::join!(api.refresh(), api.refresh());
    assert!(first.is_ok());
    assert!(second.is_ok());
    assert!(first_subject.check_current(&api).is_err());
    api.capture_reader_auth()
        .await
        .unwrap()
        .check_current(&api)
        .unwrap();
}

#[tokio::test]
async fn real_me_session_must_match_cached_pair_and_subject_debug_contains_no_identity_or_token() {
    let server = MockServer::start().await;
    let keys = DeviceKeys::generate();
    let (api, account) = register(
        &server,
        &format!("reader-{}@example.test", uuid::Uuid::new_v4()),
        &keys,
    )
    .await;
    server.inject(
        cc_protocol::paths::AUTH_ME,
        1,
        cc_sync_core::mock::FaultAction::Rewrite(Arc::new(|body| {
            body["current_session_id"] = serde_json::json!(cc_protocol::SessionId::new());
        })),
    );
    assert!(api.capture_reader_auth().await.is_err());
    let subject = api.capture_reader_auth().await.unwrap();
    let debug = format!("{subject:?}");
    assert!(!debug.contains(&account.user_id.to_string()));
    assert!(!debug.contains(&subject.session_id().to_string()));
    assert_eq!(debug, "ReaderAuthenticatedSubject([redacted])");
}

#[tokio::test]
async fn local_refresh_expiry_cannot_be_hidden_by_a_still_valid_access_token_or_real_me() {
    let server = MockServer::start().await;
    let keys = DeviceKeys::generate();
    let (api, _) = register(
        &server,
        &format!("reader-{}@example.test", uuid::Uuid::new_v4()),
        &keys,
    )
    .await;
    let mut tokens = api.refresh().await.unwrap();
    tokens.refresh_expires_at = chrono::Utc::now() - chrono::Duration::seconds(1);
    api.set_tokens(&tokens).await.unwrap();
    assert!(api.me().await.is_ok()); // Actual server access is still valid.
    assert!(api.capture_reader_auth().await.is_err()); // Reader uses min pair expiry.
}
