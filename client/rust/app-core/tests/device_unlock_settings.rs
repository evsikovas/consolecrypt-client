mod common;

use cc_app_core::platform::{ExternalOsAuthenticator, OsAuthAvailability, OsAuthKind};
use cc_app_core::*;
use common::*;
use std::sync::Arc;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn opt_in_requires_unlock_and_fresh_auth_and_survives_restart() {
    let dir = tempfile::tempdir().unwrap();
    let store = memory_store();
    let auth = Arc::new(ExternalOsAuthenticator::new());
    auth.set_availability(OsAuthAvailability::Available(OsAuthKind::TouchId));
    let app = AppCore::with_platform(config(dir.path()), store.clone(), auth.clone()).unwrap();
    let profile = app
        .create_local_profile("A".into(), PASSPHRASE.into())
        .await
        .unwrap()
        .profile;
    let info = app.device_unlock_info().await.unwrap();
    assert!(info.has_device_envelope && !info.enabled && !info.available);
    assert_eq!(
        app.set_device_unlock_enabled(true)
            .await
            .unwrap_err()
            .code(),
        "os_auth_failed"
    );
    assert!(!app.device_unlock_info().await.unwrap().enabled);
    app.lock().await.unwrap();
    auth.grant_once();
    assert_eq!(
        app.set_device_unlock_enabled(true)
            .await
            .unwrap_err()
            .code(),
        "vault_locked"
    );
    auth.revoke();
    assert_eq!(
        app.unlock_with_device().await.unwrap_err().code(),
        "unsupported"
    );
    app.unlock_with_passphrase(PASSPHRASE.into()).await.unwrap();
    auth.grant_once();
    app.set_device_unlock_enabled(true).await.unwrap();
    assert!(app.device_unlock_info().await.unwrap().available);
    app.lock().await.unwrap();
    assert_eq!(
        app.unlock_with_device().await.unwrap_err().code(),
        "os_auth_failed",
        "enable consumes grant"
    );
    app.shutdown().await.unwrap();

    let app = AppCore::with_platform(config(dir.path()), store, auth.clone()).unwrap();
    app.open_profile(profile.id).await.unwrap();
    assert!(app.device_unlock_info().await.unwrap().enabled);
    auth.grant_once();
    app.unlock_with_device().await.unwrap();
    assert!(app.is_unlocked().await);
    // Disabling still works after hardware becomes unavailable; no OS prompt.
    auth.set_availability(OsAuthAvailability::Unsupported);
    app.set_device_unlock_enabled(false).await.unwrap();
    app.lock().await.unwrap();
    auth.set_availability(OsAuthAvailability::Available(OsAuthKind::TouchId));
    auth.grant_once();
    assert_eq!(
        app.unlock_with_device().await.unwrap_err().code(),
        "unsupported"
    );
    assert_eq!(
        app.reset_passphrase_with_device("another long passphrase".into())
            .await
            .unwrap_err()
            .code(),
        "unsupported"
    );
    auth.revoke();
    // Password and trust remain intact.
    app.unlock_with_passphrase(PASSPHRASE.into()).await.unwrap();
    let info = app.device_unlock_info().await.unwrap();
    assert!(info.has_device_envelope && !info.enabled);
    app.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn device_preference_does_not_cross_profiles_or_backup_imports() {
    let dir = tempfile::tempdir().unwrap();
    let app = app_with(dir.path(), memory_store(), true);
    let a = app
        .create_local_profile("A".into(), PASSPHRASE.into())
        .await
        .unwrap()
        .profile;
    app.set_device_unlock_enabled(true).await.unwrap();
    let backup = dir
        .path()
        .join("vault.ccbackup")
        .to_string_lossy()
        .to_string();
    app.export_backup(backup.clone()).await.unwrap();
    app.close_profile().await.unwrap();
    app.create_local_profile("B".into(), PASSPHRASE.into())
        .await
        .unwrap();
    assert!(!app.device_unlock_info().await.unwrap().enabled);
    app.close_profile().await.unwrap();
    app.open_profile(a.id).await.unwrap();
    assert!(app.device_unlock_info().await.unwrap().enabled);
    app.close_profile().await.unwrap();
    app.import_backup(
        backup,
        "Restored".into(),
        BackupUnlock::Passphrase(PASSPHRASE.into()),
        None,
    )
    .await
    .unwrap();
    assert!(!app.device_unlock_info().await.unwrap().enabled);
    app.shutdown().await.unwrap();
}
