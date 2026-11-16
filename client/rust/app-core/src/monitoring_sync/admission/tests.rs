use super::*;
use crate::{AccountMode, AppConfig, AppCore};
use cc_models::VaultObject;
use cc_sync_core::mock::MockServer;
use serde_json::json;
const ACCOUNT: &str = "owned staged admission account password42";
const PASS: &str = "owned staged admission vault passphrase42";
fn app(path: &std::path::Path) -> AppCore {
    let mut config = AppConfig::for_tests(path.to_string_lossy().into_owned());
    config.client_version = "2.0.0".into();
    config.background_sync = false;
    AppCore::new(config).unwrap()
}
async fn owned(app: &AppCore, server: &MockServer) {
    app.create_synced_profile(
        "Owned admission fixture".into(),
        server.url().to_string(),
        format!("{}@example.test", uuid::Uuid::new_v4().simple()),
        ACCOUNT.into(),
        AccountMode::Register,
    )
    .await
    .unwrap();
    app.create_vault(PASS.into()).await.unwrap();
}
#[tokio::test]
async fn original_profile_lock_and_new_unlock_cannot_reuse_context() {
    let server = MockServer::start().await;
    let directory = tempfile::tempdir().unwrap();
    let app = app(directory.path());
    owned(&app, &server).await;
    let context = Context::capture(&app).await.unwrap();
    app.lock().await.unwrap();
    assert!(*context.shutdown.borrow());
    if let Some(old) = context.unlock.upgrade() {
        assert!(context.owner.hold_unlocked(&old).await.is_err());
    }
    app.unlock_with_passphrase(PASS.into()).await.unwrap();
    let (_, u) = app.unlocked().await.unwrap();
    let _profile = app.hold_session(&context.owner).await.unwrap();
    assert!(context.owner.hold_unlocked(&u).await.is_ok());
    assert!(context.check_held(&u, true).await.is_err());
}
#[tokio::test]
async fn future_settings_changes_invalidate_original_review_without_rewriting_opaque_raw() {
    let server = MockServer::start().await;
    let directory = tempfile::tempdir().unwrap();
    let app = app(directory.path());
    owned(&app, &server).await;
    let context = Context::capture(&app).await.unwrap();
    let (owner, u) = app.unlocked().await.unwrap();
    let _profile = app.hold_session(&owner).await.unwrap();
    let _unlock = owner.hold_unlocked(&u).await.unwrap();
    let mut settings = u
        .working()
        .all_vault_settings()
        .into_iter()
        .max_by_key(|s| s.updated_at)
        .unwrap();
    settings.monitoring =
        Some(json!({"version":999,"revision":7,"opaqueFuture":"preserve-exact-owned-marker"}));
    u.writer
        .put(VaultObject::VaultSettings(settings.clone()))
        .await
        .unwrap();
    assert!(context.check_held(&u, true).await.is_err());
    assert!(catalog_checkpoint(&u).is_err());
    let after = u
        .working()
        .all_vault_settings()
        .into_iter()
        .find(|s| s.id == settings.id)
        .unwrap();
    assert_eq!(after.monitoring, settings.monitoring);
}
#[tokio::test]
async fn device_identity_and_expiry_are_native_context_gates_not_id_authority() {
    let server = MockServer::start().await;
    let directory = tempfile::tempdir().unwrap();
    let app = app(directory.path());
    owned(&app, &server).await;
    let mut context = Context::capture(&app).await.unwrap();
    let (owner, u) = app.unlocked().await.unwrap();
    let _profile = app.hold_session(&owner).await.unwrap();
    let _unlock = owner.hold_unlocked(&u).await.unwrap();
    context.expires = Instant::now() - Duration::from_secs(1);
    assert!(context.check_held(&u, true).await.is_err());
    context.expires = Instant::now() + Duration::from_secs(30);
    owner.set_identity(Arc::new(DeviceIdentity::generate().unwrap()));
    assert!(context.check_held(&u, true).await.is_err());
}
#[tokio::test]
async fn profile_close_and_local_reopen_never_retarget_an_original_account_context() {
    let server = MockServer::start().await;
    let directory = tempfile::tempdir().unwrap();
    let app = app(directory.path());
    owned(&app, &server).await;
    let context = Context::capture(&app).await.unwrap();
    app.close_profile().await.unwrap();
    app.create_local_profile("Different owned local".into(), PASS.into())
        .await
        .unwrap();
    assert!(app.hold_session(&context.owner).await.is_err());
    assert!(matches!(
        Context::capture(&app).await,
        Err(crate::AppError::LocalProfile)
    ));
}
#[tokio::test]
async fn generic_settings_save_invalidates_review_even_when_monitoring_raw_is_unchanged() {
    let server = MockServer::start().await;
    let directory = tempfile::tempdir().unwrap();
    let app = app(directory.path());
    owned(&app, &server).await;
    let context = Context::capture(&app).await.unwrap();
    let mut generic = app.get_vault_settings().await.unwrap();
    generic.vault_name = "Owned fresh metadata".into();
    app.save_vault_settings(generic).await.unwrap();
    let (owner, u) = app.unlocked().await.unwrap();
    let _profile = app.hold_session(&owner).await.unwrap();
    let _unlock = owner.hold_unlocked(&u).await.unwrap();
    assert_eq!(
        catalog_checkpoint(&u).unwrap().revision,
        context.catalog.revision
    );
    assert!(context.check_held(&u, true).await.is_err());
}

mod http;
