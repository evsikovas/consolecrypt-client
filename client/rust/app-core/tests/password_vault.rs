mod common;
use cc_app_core::*;
use cc_platform_core::{FakePrompt, InMemoryDeviceKeys, InMemorySecurityKey};
use common::*;
use std::sync::Arc;

#[tokio::test]
async fn android_kdbx_material_is_cryptographically_checked_and_scoped() {
    let tmp = tempfile::tempdir().unwrap();
    let keys = Arc::new(cc_platform_core::ExternalDeviceKeys::new());
    keys.set_status(Some(cc_platform_core::OsAuthKind::Fingerprint), false);
    let app = AppCore::with_platform(config(tmp.path()), memory_store(), keys).unwrap();
    app.create_local_profile("Android KDBX".into(), PASSPHRASE.into())
        .await
        .unwrap();
    let id = "d".repeat(64);
    let method = "hello".to_owned();
    let info = app
        .password_vault_key("info".into(), id.clone(), method.clone(), vec![], None)
        .await
        .unwrap();
    let info: serde_json::Value = serde_json::from_slice(&info).unwrap();
    assert_eq!(info["available"], true);
    assert_eq!(info["external"], true);
    let handle = b"opaque-android-ciphertext-handle";
    let mut payload = vec![77; 32];
    payload.extend_from_slice(&[23; 32]);
    payload.extend_from_slice(handle);
    for size in [0, 32, 64, 4161] {
        assert!(
            app.password_vault_key(
                "enable_external".into(),
                id.clone(),
                method.clone(),
                vec![0; size],
                None
            )
            .await
            .is_err()
        );
    }
    app.password_vault_key(
        "enable_external".into(),
        id.clone(),
        method.clone(),
        payload,
        None,
    )
    .await
    .unwrap();
    assert_eq!(
        app.password_vault_key(
            "external_handle".into(),
            id.clone(),
            method.clone(),
            vec![],
            None
        )
        .await
        .unwrap(),
        handle
    );
    assert!(
        app.password_vault_key(
            "release_external".into(),
            id.clone(),
            method.clone(),
            vec![24; 32],
            None
        )
        .await
        .is_err()
    );
    assert!(
        app.password_vault_key(
            "release_external".into(),
            "e".repeat(64),
            method.clone(),
            vec![23; 32],
            None
        )
        .await
        .is_err()
    );
    assert!(
        app.password_vault_key("release".into(), id.clone(), method.clone(), vec![], None)
            .await
            .is_err()
    );
    app.lock().await.unwrap();
    assert!(
        app.password_vault_key(
            "release_external".into(),
            id.clone(),
            method.clone(),
            vec![23; 32],
            None
        )
        .await
        .is_err()
    );
    app.unlock_with_passphrase(PASSPHRASE.into()).await.unwrap();
    assert_eq!(
        app.password_vault_key(
            "release_external".into(),
            id.clone(),
            method.clone(),
            vec![23; 32],
            None
        )
        .await
        .unwrap(),
        vec![77; 32]
    );
    app.password_vault_key(
        "forget_external_handle".into(),
        id.clone(),
        method.clone(),
        b"old-handle".to_vec(),
        None,
    )
    .await
    .unwrap();
    assert_eq!(
        app.password_vault_key(
            "external_handle".into(),
            id.clone(),
            method.clone(),
            vec![],
            None
        )
        .await
        .unwrap(),
        handle
    );
    app.password_vault_key(
        "forget_external_handle".into(),
        id.clone(),
        method.clone(),
        handle.to_vec(),
        None,
    )
    .await
    .unwrap();
    assert!(
        app.password_vault_key("release_external".into(), id, method, vec![23; 32], None)
            .await
            .is_err()
    );
    app.shutdown().await.unwrap();
}

#[tokio::test]
async fn webdav_settings_roundtrip_and_validate_without_storing_authentication_secret() {
    let tmp = tempfile::tempdir().unwrap();
    let app = app(tmp.path());
    app.create_local_profile("KDBX".into(), PASSPHRASE.into())
        .await
        .unwrap();
    let mut settings = app.get_vault_settings().await.unwrap();
    let connection = cc_models::settings::PasswordVaultConnection {
        id: "connection-1".into(),
        name: "Personal".into(),
        url: "https://dav.example/vault.kdbx".into(),
        username: "user".into(),
        credential_id: None,
    };
    settings.password_vaults.push(connection.clone());
    app.save_vault_settings(settings.clone()).await.unwrap();
    settings.vault_name = "Renamed".into();
    app.save_vault_settings(settings.clone()).await.unwrap();
    app.lock().await.unwrap();
    app.unlock_with_passphrase(PASSPHRASE.into()).await.unwrap();
    assert_eq!(
        app.get_vault_settings().await.unwrap().password_vaults,
        vec![connection]
    );
    for bad in [
        "http://example.org/v.kdbx",
        "https://u:password@example.org/v.kdbx",
        "https://example.org/v.kdbx#fragment",
    ] {
        let mut invalid = settings.clone();
        invalid.password_vaults[0].url = bad.into();
        assert!(app.save_vault_settings(invalid).await.is_err());
    }
    let mut invalid = settings;
    invalid
        .password_vaults
        .push(invalid.password_vaults[0].clone());
    assert!(app.save_vault_settings(invalid).await.is_err());
    app.shutdown().await.unwrap();
}

#[tokio::test]
async fn kdbx_keys_require_independent_auth_and_survive_profile_lock() {
    let tmp = tempfile::tempdir().unwrap();
    let os = Arc::new(InMemoryDeviceKeys::new());
    let store = memory_store();
    let app = app_with_keys(tmp.path(), store.clone(), os.clone());
    let profile = app
        .create_local_profile("KDBX".into(), PASSPHRASE.into())
        .await
        .unwrap()
        .profile;
    let id = "a".repeat(64);
    let other = "b".repeat(64);
    let info = app
        .password_vault_key("info".into(), id.clone(), "hello".into(), vec![], None)
        .await
        .unwrap();
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&info).unwrap()["enabled"],
        false
    );
    app.password_vault_key(
        "enable".into(),
        id.clone(),
        "hello".into(),
        vec![77; 32],
        None,
    )
    .await
    .unwrap();
    assert_eq!(os.key_count(), 1);
    os.set_next(FakePrompt::Cancel);
    assert!(
        app.password_vault_key("release".into(), id.clone(), "hello".into(), vec![], None)
            .await
            .is_err()
    );
    assert!(
        app.password_vault_key("release".into(), other, "hello".into(), vec![], None)
            .await
            .is_err()
    );
    os.set_next(FakePrompt::Succeed);
    // Force a local key-file commit failure, inside this test's own directory.
    // The existing authenticator remains usable and the new OS key is removed.
    let key_path = tmp
        .path()
        .join("profiles")
        .join(&profile.id)
        .join("vault-keys.json");
    let backup = key_path.with_extension("test-backup");
    assert!(key_path.starts_with(tmp.path()) && backup.starts_with(tmp.path()));
    std::fs::rename(&key_path, &backup).unwrap();
    std::fs::create_dir(&key_path).unwrap();
    let failed = app
        .password_vault_key(
            "enable".into(),
            id.clone(),
            "hello".into(),
            vec![88; 32],
            None,
        )
        .await;
    std::fs::remove_dir(&key_path).unwrap();
    std::fs::rename(&backup, &key_path).unwrap();
    assert!(failed.is_err());
    assert_eq!(os.key_count(), 1);
    assert_eq!(
        app.password_vault_key("release".into(), id.clone(), "hello".into(), vec![], None)
            .await
            .unwrap(),
        vec![77; 32]
    );
    app.lock().await.unwrap();
    assert_eq!(
        app.password_vault_key("release".into(), id.clone(), "hello".into(), vec![], None)
            .await
            .unwrap_err()
            .code(),
        "vault_locked"
    );
    app.unlock_with_passphrase(PASSPHRASE.into()).await.unwrap();
    assert_eq!(
        app.password_vault_key("release".into(), id.clone(), "hello".into(), vec![], None)
            .await
            .unwrap(),
        vec![77; 32]
    );
    // Protected KDBX keys are ciphertext-only and independent of profile unlock.
    let raw = std::fs::read(
        tmp.path()
            .join("profiles")
            .join(&profile.id)
            .join("vault-keys.json"),
    )
    .unwrap();
    assert!(!raw.windows(32).any(|w| w == [77; 32]));
    app.shutdown().await.unwrap();
    let again = app_with_keys(tmp.path(), store, os.clone());
    again.open_profile(profile.id).await.unwrap();
    again
        .unlock_with_passphrase(PASSPHRASE.into())
        .await
        .unwrap();
    assert_eq!(
        again
            .password_vault_key("release".into(), id.clone(), "hello".into(), vec![], None)
            .await
            .unwrap(),
        vec![77; 32]
    );
    again
        .password_vault_key("forget".into(), id.clone(), "hello".into(), vec![], None)
        .await
        .unwrap();
    assert_eq!(os.key_count(), 0);
    assert!(
        again
            .password_vault_key("release".into(), id, "hello".into(), vec![], None)
            .await
            .is_err()
    );
    again.shutdown().await.unwrap();
}

#[tokio::test]
async fn kdbx_fido_prf_requires_the_enrolled_key_and_pin() {
    let tmp = tempfile::tempdir().unwrap();
    let app = app(tmp.path());
    let key = Arc::new(InMemorySecurityKey::default());
    app.set_security_keys(key.clone());
    app.create_local_profile("KDBX".into(), PASSPHRASE.into())
        .await
        .unwrap();
    let id = "c".repeat(64);
    let pin = Some(InMemorySecurityKey::PIN.into());
    app.password_vault_key(
        "enable".into(),
        id.clone(),
        "security_key".into(),
        vec![45; 32],
        pin.clone(),
    )
    .await
    .unwrap();
    assert!(
        app.password_vault_key(
            "release".into(),
            id.clone(),
            "security_key".into(),
            vec![],
            Some("0000".into())
        )
        .await
        .is_err()
    );
    assert_eq!(
        app.password_vault_key(
            "release".into(),
            id.clone(),
            "security_key".into(),
            vec![],
            pin.clone()
        )
        .await
        .unwrap(),
        vec![45; 32]
    );
    key.replace();
    assert!(
        app.password_vault_key("release".into(), id, "security_key".into(), vec![], pin)
            .await
            .is_err()
    );
    app.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn external_vaults_and_saved_login_roundtrip_between_devices_without_being_erased_by_legacy_settings(
) {
    let tmp = tempfile::tempdir().unwrap();
    let server = cc_sync_core::mock::MockServer::start().await;
    let a = app(&tmp.path().join("a"));
    let b = app(&tmp.path().join("b"));
    let email = "vault-config-sync@example.invalid";
    a.create_synced_profile(
        "desktop".into(),
        server.url().to_string(),
        email.into(),
        ACCOUNT_PASSWORD.into(),
        AccountMode::Register,
    )
    .await
    .unwrap();
    let kit = a.create_vault(PASSPHRASE.into()).await.unwrap();
    let original = a.get_vault_settings().await.unwrap();
    let password = "only-a-synthetic-vault-credential";
    let cred = a
        .add_password_credential("Vault login".into(), None, password.into())
        .await
        .unwrap();
    let password_connection = ExternalVaultConfig {
        id: "fixture-vault".into(),
        name: "Vault".into(),
        endpoint: "https://vault.example".into(),
        auth: "userpass".into(),
        username: "alice".into(),
        auth_mount: "userpass".into(),
        mount: "kv".into(),
        namespace: "team".into(),
        versioned: true,
        credential_id: Some(cred.id.parse().unwrap()),
    };
    let mut first = original.clone();
    first.original_external_vaults = first.external_vaults.clone();
    first.external_vaults = Some(vec![password_connection.clone()]);
    a.save_vault_settings(first.clone()).await.unwrap();
    // Retrying a successful write must not duplicate the registration.
    a.save_vault_settings(first).await.unwrap();
    assert_eq!(
        a.delete_credential(cred.id.clone())
            .await
            .unwrap_err()
            .code(),
        "in_use"
    );

    // A second independent edit from the same snapshot retains the first connection.
    let token = "synthetic-openbao-token";
    let token_cred = a
        .add_password_credential("OpenBao token".into(), None, token.into())
        .await
        .unwrap();
    let token_connection = ExternalVaultConfig {
        id: "fixture-bao".into(),
        name: "OpenBao".into(),
        endpoint: "http://127.0.0.1:8200".into(),
        auth: "token".into(),
        username: String::new(),
        auth_mount: String::new(),
        credential_id: Some(token_cred.id.parse().unwrap()),
        ..password_connection.clone()
    };
    let mut second = original.clone();
    second.original_external_vaults = second.external_vaults.clone();
    second.external_vaults = Some(vec![token_connection.clone()]);
    a.save_vault_settings(second).await.unwrap();
    a.sync_now().await.unwrap();
    b.create_synced_profile(
        "phone".into(),
        server.url().to_string(),
        email.into(),
        ACCOUNT_PASSWORD.into(),
        AccountMode::Login,
    )
    .await
    .unwrap();
    b.join_vault_with_passphrase(kit.vault_id.clone(), PASSPHRASE.into())
        .await
        .unwrap();
    b.sync_now().await.unwrap();
    let expected = vec![token_connection.clone(), password_connection.clone()];
    wait_until!(
        20,
        "second-device connections",
        b.get_vault_settings()
            .await
            .unwrap()
            .external_vaults
            .as_ref()
            == Some(&expected)
    );
    for (id, wanted) in [(cred.id.clone(), password), (token_cred.id.clone(), token)] {
        assert_eq!(b.reveal_credential_secret(id).await.unwrap().value, wanted);
    }
    let read = b.get_vault_settings().await.unwrap();
    let json = serde_json::to_string(&read).unwrap();
    assert!(!json.contains(password) && !json.contains(token) && !json.contains("wrapped"));
    let mut legacy = read.clone();
    legacy.external_vaults = None;
    legacy.vault_name = "Renamed on old device".into();
    b.save_vault_settings(legacy).await.unwrap();
    let mut malicious = read.clone();
    malicious.original_external_vaults = read.external_vaults.clone();
    malicious.external_vaults.as_mut().unwrap()[0].endpoint = "https://different.example".into();
    assert!(b.save_vault_settings(malicious).await.is_err());
    let mut remove = read;
    remove.original_external_vaults = remove.external_vaults.clone();
    remove.external_vaults = Some(vec![password_connection.clone()]);
    b.save_vault_settings(remove).await.unwrap();
    b.delete_credential(token_cred.id).await.unwrap();
    b.sync_now().await.unwrap();
    a.sync_now().await.unwrap();
    wait_until!(
        20,
        "synced deletion",
        a.get_vault_settings().await.unwrap().external_vaults
            == Some(vec![password_connection.clone()])
    );
    assert_eq!(
        a.get_vault_settings().await.unwrap().vault_name,
        "Renamed on old device"
    );
    a.shutdown().await.unwrap();
    b.shutdown().await.unwrap();
}
