//! 0.9.2: step-up confirmations with the OS check instead of the vault
//! passphrase, with a fake OS protector (`InMemoryDeviceKeys`) standing in
//! for Secure Enclave / Windows Hello.

mod common;

use cc_app_core::platform::{
    DeviceKeyStatus, ExternalDeviceKeys, FakePrompt, InMemoryDeviceKeys, OsAuthKind,
    UnlockProtection,
};
use cc_app_core::*;
use common::*;
use std::sync::Arc;

fn code<T: std::fmt::Debug>(r: AppResult<T>) -> &'static str {
    r.unwrap_err().code()
}

async fn confirm(app: &AppCore, action: &str, object: &str, token: &str) -> AppResult<()> {
    app.confirm_step_up(action.into(), object.into(), StepUp::Presence(token.into()))
        .await
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn presence_tokens_are_minted_by_the_os_check_and_redeemed_once() {
    let dir = tempfile::tempdir().unwrap();
    let keys = Arc::new(InMemoryDeviceKeys::new());
    let app = app_with_keys(dir.path(), memory_store(), keys.clone());
    app.create_local_profile("A".into(), PASSPHRASE.into())
        .await
        .unwrap();

    // Device unlock off: passphrase only.
    let info = app.device_unlock_info().await.unwrap();
    assert!(!info.presence_available && !info.confirm_actions);
    assert_eq!(
        code(
            app.verify_user_presence("confirm".into(), String::new(), "r".into())
                .await
        ),
        "passphrase_required"
    );
    assert_eq!(
        code(app.set_confirm_actions_with_presence(true).await),
        "unsupported"
    );

    // On: confirming actions with the OS check is on by default.
    app.set_device_unlock_enabled(true, "r".into(), false)
        .await
        .unwrap();
    let info = app.device_unlock_info().await.unwrap();
    assert!(info.confirm_actions && info.presence_available);

    // One OS prompt per check; the token is bound to action and object.
    let prompts = keys.prompts();
    let token = app
        .verify_user_presence("confirm".into(), "kube/s1".into(), "confirm".into())
        .await
        .unwrap();
    assert_eq!(keys.prompts(), prompts + 1);
    assert_eq!(
        code(confirm(&app, "confirm", "kube/s2", &token).await),
        "passphrase_required"
    );
    // A mismatching attempt used it up.
    assert_eq!(
        code(confirm(&app, "confirm", "kube/s1", &token).await),
        "passphrase_required"
    );
    let token = app
        .verify_user_presence("confirm".into(), "kube/s1".into(), "confirm".into())
        .await
        .unwrap();
    assert_eq!(
        code(confirm(&app, "docker_reveal", "kube/s1", &token).await),
        "passphrase_required"
    );
    let token = app
        .verify_user_presence("confirm".into(), "kube/s1".into(), "confirm".into())
        .await
        .unwrap();
    confirm(&app, "confirm", "kube/s1", &token).await.unwrap();
    // Single use.
    assert_eq!(
        code(confirm(&app, "confirm", "kube/s1", &token).await),
        "passphrase_required"
    );
    // Unknown / forged tokens.
    assert_eq!(
        code(confirm(&app, "confirm", "kube/s1", "forged").await),
        "passphrase_required"
    );
    // No proof at all.
    assert_eq!(
        code(StepUp::from_parts(None, None).map(|_| ())),
        "passphrase_required"
    );

    // The passphrase keeps working for every step-up.
    app.confirm_step_up(
        "confirm".into(),
        "kube/s1".into(),
        StepUp::Passphrase(PASSPHRASE.into()),
    )
    .await
    .unwrap();
    assert_eq!(
        code(
            app.confirm_step_up(
                "confirm".into(),
                String::new(),
                StepUp::Passphrase("wrong".into()),
            )
            .await
        ),
        "wrong_passphrase"
    );

    // A cancelled or failed OS prompt mints nothing.
    keys.set_next(FakePrompt::Cancel);
    assert_eq!(
        code(
            app.verify_user_presence("confirm".into(), String::new(), "r".into())
                .await
        ),
        "os_auth_failed"
    );
    keys.set_next(FakePrompt::Fail);
    assert_eq!(
        code(
            app.verify_user_presence("confirm".into(), String::new(), "r".into())
                .await
        ),
        "os_auth_failed"
    );
    keys.set_next(FakePrompt::Succeed);
    app.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn passphrase_only_actions_never_take_a_token() {
    let dir = tempfile::tempdir().unwrap();
    let keys = Arc::new(InMemoryDeviceKeys::new());
    let app = app_with_keys(dir.path(), memory_store(), keys.clone());
    app.create_local_profile("A".into(), PASSPHRASE.into())
        .await
        .unwrap();
    app.set_device_unlock_enabled(true, "r".into(), false)
        .await
        .unwrap();
    let prompts = keys.prompts();
    for action in PASSPHRASE_ONLY {
        // No OS prompt is even shown for them.
        assert_eq!(
            code(
                app.verify_user_presence(action.wire().into(), String::new(), "r".into())
                    .await
            ),
            "passphrase_required",
            "{action:?}"
        );
        let token = app
            .verify_user_presence("confirm".into(), String::new(), "r".into())
            .await
            .unwrap();
        assert_eq!(
            code(confirm(&app, action.wire(), "", &token).await),
            "passphrase_required",
            "{action:?}"
        );
        app.confirm_step_up(
            action.wire().into(),
            String::new(),
            StepUp::Passphrase(PASSPHRASE.into()),
        )
        .await
        .unwrap();
    }
    assert_eq!(keys.prompts(), prompts + PASSPHRASE_ONLY.len());
    assert_eq!(
        code(
            app.verify_user_presence("unlock_everything".into(), String::new(), "r".into())
                .await
        ),
        "invalid_input"
    );
    app.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn preference_lock_and_enrollment_changes_end_presence() {
    let dir = tempfile::tempdir().unwrap();
    let keys = Arc::new(InMemoryDeviceKeys::new());
    let app = app_with_keys(dir.path(), memory_store(), keys.clone());
    app.create_local_profile("A".into(), PASSPHRASE.into())
        .await
        .unwrap();
    app.set_device_unlock_enabled(true, "r".into(), false)
        .await
        .unwrap();

    // Preference off: nothing offered, and earlier tokens are refused.
    let token = app
        .verify_user_presence("confirm".into(), String::new(), "r".into())
        .await
        .unwrap();
    app.set_confirm_actions_with_presence(false).await.unwrap();
    let info = app.device_unlock_info().await.unwrap();
    assert!(info.enabled && !info.confirm_actions && !info.presence_available);
    assert_eq!(
        code(confirm(&app, "confirm", "", &token).await),
        "passphrase_required"
    );
    assert_eq!(
        code(
            app.verify_user_presence("confirm".into(), String::new(), "r".into())
                .await
        ),
        "passphrase_required"
    );
    app.set_confirm_actions_with_presence(true).await.unwrap();
    assert!(app.device_unlock_info().await.unwrap().presence_available);

    // Locking drops every token.
    let token = app
        .verify_user_presence("confirm".into(), String::new(), "r".into())
        .await
        .unwrap();
    app.lock().await.unwrap();
    assert_eq!(
        code(
            app.verify_user_presence("confirm".into(), String::new(), "r".into())
                .await
        ),
        "vault_locked"
    );
    app.unlock_with_passphrase(PASSPHRASE.into()).await.unwrap();
    assert_eq!(
        code(confirm(&app, "confirm", "", &token).await),
        "passphrase_required"
    );

    // The preference survives a lock; re-enabling device unlock turns it on.
    app.set_confirm_actions_with_presence(false).await.unwrap();
    app.lock().await.unwrap();
    app.unlock_with_passphrase(PASSPHRASE.into()).await.unwrap();
    assert!(!app.device_unlock_info().await.unwrap().confirm_actions);
    app.set_device_unlock_enabled(false, String::new(), false)
        .await
        .unwrap();
    app.set_device_unlock_enabled(true, "r".into(), false)
        .await
        .unwrap();
    assert!(app.device_unlock_info().await.unwrap().confirm_actions);

    // New fingerprint: the key is gone, device unlock is dropped.
    keys.change_enrollment();
    assert_eq!(
        code(
            app.verify_user_presence("confirm".into(), String::new(), "r".into())
                .await
        ),
        "os_auth_invalidated"
    );
    let info = app.device_unlock_info().await.unwrap();
    assert!(!info.enabled && !info.presence_available);
    app.shutdown().await.unwrap();
}

/// Linux keyring: device unlock is a convenience, never a step-up.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn convenience_protection_is_passphrase_only() {
    let dir = tempfile::tempdir().unwrap();
    let keys = Arc::new(InMemoryDeviceKeys::new());
    keys.set_status(DeviceKeyStatus {
        kind: Some(OsAuthKind::Keyring),
        protection: UnlockProtection::Convenience,
        not_enrolled: false,
        external: false,
    });
    let app = app_with_keys(dir.path(), memory_store(), keys.clone());
    app.create_local_profile("A".into(), PASSPHRASE.into())
        .await
        .unwrap();
    app.set_device_unlock_enabled(true, "r".into(), true)
        .await
        .unwrap();
    let info = app.device_unlock_info().await.unwrap();
    assert!(info.enabled && !info.presence_available);
    let prompts = keys.prompts();
    assert_eq!(
        code(
            app.verify_user_presence("confirm".into(), String::new(), "r".into())
                .await
        ),
        "passphrase_required"
    );
    assert_eq!(keys.prompts(), prompts);
    app.shutdown().await.unwrap();
}

/// Android: the app's `BiometricPrompt` decrypts the material; the core
/// checks it like for unlock.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn external_material_mints_a_token() {
    let dir = tempfile::tempdir().unwrap();
    let external = Arc::new(ExternalDeviceKeys::new());
    let app = AppCore::with_platform(config(dir.path()), memory_store(), external.clone()).unwrap();
    app.create_local_profile("A".into(), PASSPHRASE.into())
        .await
        .unwrap();
    external.set_status(Some(OsAuthKind::Fingerprint), false);
    let material = vec![7u8; 32];
    app.enable_device_unlock_with_material(
        material.clone(),
        b"keystore:p1:iv".to_vec(),
        "fingerprint".into(),
    )
    .await
    .unwrap();
    assert!(app.device_unlock_info().await.unwrap().presence_available);
    // The in-core path refuses: the app must do the OS step.
    assert_eq!(
        code(
            app.verify_user_presence("db_production_write".into(), "s1".into(), "r".into())
                .await
        ),
        "os_auth_failed"
    );
    assert_eq!(
        code(
            app.verify_user_presence_with_material(
                "db_production_write".into(),
                "s1".into(),
                vec![1u8; 8]
            )
            .await
        ),
        "invalid_input"
    );
    let token = app
        .verify_user_presence_with_material("confirm".into(), "s1".into(), material.clone())
        .await
        .unwrap();
    confirm(&app, "confirm", "s1", &token).await.unwrap();
    // A different key than the sealed one proves nothing (and drops the
    // stale device unlock record, as for unlock).
    assert_eq!(
        code(
            app.verify_user_presence_with_material("confirm".into(), "s1".into(), vec![9u8; 32])
                .await
        ),
        "os_auth_invalidated"
    );
    assert!(!app.device_unlock_info().await.unwrap().enabled);
    app.shutdown().await.unwrap();
}
