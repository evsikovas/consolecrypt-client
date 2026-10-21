//! S5: device unlock under OS protection and the sealed profile database.
//! Acceptance criteria of TZ-2026-10-21-S5 §4, with a fake OS protector
//! (`InMemoryDeviceKeys`) standing in for Secure Enclave / Keystore / Hello.

mod common;

use cc_app_core::platform::{
    DeviceKeyProtector, DeviceKeyStatus, ExternalDeviceKeys, FakePrompt, InMemoryDeviceKeys,
    OsAuthKind, SecureStore, UnlockProtection,
};
use cc_app_core::*;
use cc_models::host::Host;
use cc_models::{KekClass, ObjectPayload, VaultObject};
use cc_storage_core::{
    local_key_envelope, DatabaseKey, Profile, ProfileDirectory, ProfileKind, Storage, VaultRecord,
};
use cc_sync_core::mock::MockServer;
use cc_vault_core::{
    create_vault, unlock_with_device, unlock_with_passphrase, Argon2Params, DeviceIdentity,
    SecretString,
};
use common::*;
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::sync::Arc;

fn profile_dir(root: &Path, pid: &str) -> PathBuf {
    root.join("profiles").join(pid)
}

fn keyfile(root: &Path, pid: &str) -> Value {
    let bytes = std::fs::read(profile_dir(root, pid).join("vault-keys.json")).unwrap();
    serde_json::from_slice(&bytes).unwrap()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn opt_in_needs_unlock_and_the_os_key_and_survives_restart() {
    let dir = tempfile::tempdir().unwrap();
    let store = memory_store();
    let keys = Arc::new(InMemoryDeviceKeys::new());
    let app = app_with_keys(dir.path(), store.clone(), keys.clone());
    let profile = app
        .create_local_profile("A".into(), PASSPHRASE.into())
        .await
        .unwrap()
        .profile;
    // Off by default (TR-1): no OS key, no unlock record.
    let info = app.device_unlock_info().await.unwrap();
    assert!(info.has_vault && !info.enabled && !info.available);
    assert_eq!(info.protection, "system");
    assert_eq!(keys.key_count(), 0);
    assert!(keyfile(dir.path(), &profile.id)["device_unlock"].is_null());

    app.lock().await.unwrap();
    assert_eq!(
        app.set_device_unlock_enabled(true, "r".into(), false)
            .await
            .unwrap_err()
            .code(),
        "vault_locked"
    );
    assert_eq!(
        app.unlock_with_device("r".into()).await.unwrap_err().code(),
        "unsupported"
    );

    // A cancelled OS prompt while enabling leaves it off.
    app.unlock_with_passphrase(PASSPHRASE.into()).await.unwrap();
    keys.set_next(FakePrompt::Cancel);
    assert_eq!(
        app.set_device_unlock_enabled(true, "r".into(), false)
            .await
            .unwrap_err()
            .code(),
        "os_auth_failed"
    );
    assert!(!app.device_unlock_info().await.unwrap().enabled);
    keys.set_next(FakePrompt::Succeed);
    app.set_device_unlock_enabled(true, "r".into(), false)
        .await
        .unwrap();
    let info = app.device_unlock_info().await.unwrap();
    assert!(info.enabled && info.available);
    assert_eq!(info.kind.as_deref(), Some("touch_id"));

    // Criterion 3: cancel → vault stays locked, os_auth_failed.
    app.lock().await.unwrap();
    keys.set_next(FakePrompt::Cancel);
    assert_eq!(
        app.unlock_with_device("r".into()).await.unwrap_err().code(),
        "os_auth_failed"
    );
    keys.set_next(FakePrompt::Fail);
    assert_eq!(
        app.unlock_with_device("r".into()).await.unwrap_err().code(),
        "os_auth_failed"
    );
    assert!(!app.is_unlocked().await);
    keys.set_next(FakePrompt::Succeed);
    let prompts = keys.prompts();
    app.unlock_with_device("r".into()).await.unwrap();
    assert!(app.is_unlocked().await);
    assert_eq!(keys.prompts(), prompts + 1, "exactly one OS prompt");
    app.save_host(HostDto::new("h1", "10.0.0.1")).await.unwrap();
    app.shutdown().await.unwrap();

    // Restart: the profile opens sealed; device unlock still works.
    let app = app_with_keys(dir.path(), store.clone(), keys.clone());
    let info = app.open_profile(profile.id.clone()).await.unwrap();
    assert_eq!(info.vault_state, VaultStateDto::Locked);
    assert!(app.device_unlock_info().await.unwrap().available);
    app.unlock_with_device("r".into()).await.unwrap();
    assert_eq!(app.list_hosts().await.unwrap().len(), 1);

    // Off again: the OS key is deleted and the device path is gone.
    app.set_device_unlock_enabled(false, String::new(), false)
        .await
        .unwrap();
    assert_eq!(keys.key_count(), 0);
    app.lock().await.unwrap();
    assert_eq!(
        app.unlock_with_device("r".into()).await.unwrap_err().code(),
        "unsupported"
    );
    assert_eq!(
        app.reset_passphrase_with_device("another long passphrase".into(), "r".into())
            .await
            .unwrap_err()
            .code(),
        "unsupported"
    );
    app.unlock_with_passphrase(PASSPHRASE.into()).await.unwrap();
    app.shutdown().await.unwrap();
}

/// Criterion 1: without device unlock, a copy of the OS secret store and
/// of the profile files does not open the vault or its database.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn os_store_and_files_alone_do_not_open_anything() {
    let dir = tempfile::tempdir().unwrap();
    let store = memory_store();
    let app = app_with(dir.path(), store.clone(), true);
    let pid = app
        .create_local_profile("A".into(), PASSPHRASE.into())
        .await
        .unwrap()
        .profile
        .id;
    let marker = "attacker-should-not-see-this-host";
    app.save_host(HostDto::new(marker, "10.9.8.7"))
        .await
        .unwrap();
    app.shutdown().await.unwrap();

    // The OS store holds the device identity and nothing that opens data.
    assert!(store
        .get(&format!("profiles/{pid}/db-key"))
        .unwrap()
        .is_none());
    assert!(store
        .get(&format!("profiles/{pid}/search-index-key"))
        .unwrap()
        .is_none());
    let identity = DeviceIdentity::load(store.as_ref(), &pid).unwrap().unwrap();

    // The key file holds ciphertext only; no device unlock record.
    let kf = keyfile(dir.path(), &pid);
    assert!(kf["device_unlock"].is_null());
    let text = serde_json::to_string(&kf).unwrap();
    assert!(!text.contains(marker));

    // The database file is SQLCipher ciphertext; keys an attacker could
    // derive from the OS store do not open it.
    let db = profile_dir(dir.path(), &pid).join("vault.db");
    let raw = std::fs::read(&db).unwrap();
    assert!(!raw.windows(marker.len()).any(|w| w == marker.as_bytes()));
    for candidate in [
        [0u8; 32],
        *identity
            .device_id()
            .0
            .as_bytes()
            .repeat(2)
            .first_chunk::<32>()
            .unwrap(),
    ] {
        assert!(Storage::open(&db, DatabaseKey::from_bytes(candidate))
            .await
            .is_err());
    }

    // A process without the passphrase gets a locked vault and no device
    // path (even with an approving OS prompt it has no unlock record).
    let attacker = app_with(dir.path(), store.clone(), true);
    attacker.open_profile(pid.clone()).await.unwrap();
    assert!(!attacker.is_unlocked().await);
    assert_eq!(
        attacker
            .unlock_with_device("x".into())
            .await
            .unwrap_err()
            .code(),
        "unsupported"
    );
    assert_eq!(
        attacker.list_hosts().await.unwrap_err().code(),
        "vault_locked"
    );
    assert_eq!(
        attacker
            .unlock_with_passphrase("wrong passphrase!!".into())
            .await
            .unwrap_err()
            .code(),
        "wrong_passphrase"
    );
    attacker.shutdown().await.unwrap();
}

/// Criterion 4: a fingerprint added → device unlock stops until switched
/// on again; the next passphrase unlock offers it.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn enrollment_change_invalidates_and_is_reoffered() {
    let dir = tempfile::tempdir().unwrap();
    let keys = Arc::new(InMemoryDeviceKeys::new());
    let app = app_with_keys(dir.path(), memory_store(), keys.clone());
    app.create_local_profile("A".into(), PASSPHRASE.into())
        .await
        .unwrap();
    app.set_device_unlock_enabled(true, "r".into(), false)
        .await
        .unwrap();
    app.lock().await.unwrap();
    keys.change_enrollment();
    assert_eq!(
        app.unlock_with_device("r".into()).await.unwrap_err().code(),
        "os_auth_invalidated"
    );
    let info = app.device_unlock_info().await.unwrap();
    assert!(!info.enabled && !info.available);
    assert_eq!(info.reoffer.as_deref(), Some("invalidated"));
    app.unlock_with_passphrase(PASSPHRASE.into()).await.unwrap();
    assert_eq!(
        app.device_unlock_info().await.unwrap().reoffer.as_deref(),
        Some("invalidated")
    );
    app.set_device_unlock_enabled(true, "r".into(), false)
        .await
        .unwrap();
    let info = app.device_unlock_info().await.unwrap();
    assert!(info.enabled && info.reoffer.is_none());
    app.lock().await.unwrap();
    app.unlock_with_device("r".into()).await.unwrap();

    // Forgot the passphrase: device unlock resets it.
    app.lock().await.unwrap();
    app.reset_passphrase_with_device("a fresh vault passphrase".into(), "r".into())
        .await
        .unwrap();
    app.lock().await.unwrap();
    app.unlock_with_passphrase("a fresh vault passphrase".into())
        .await
        .unwrap();
    app.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn convenience_protection_needs_confirmation() {
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
    let info = app.device_unlock_info().await.unwrap();
    assert_eq!(info.protection, "convenience");
    assert_eq!(info.kind.as_deref(), Some("keyring"));
    assert_eq!(
        app.set_device_unlock_enabled(true, "r".into(), false)
            .await
            .unwrap_err()
            .code(),
        "invalid_input"
    );
    app.set_device_unlock_enabled(true, "r".into(), true)
        .await
        .unwrap();
    assert!(app.device_unlock_info().await.unwrap().enabled);
    app.shutdown().await.unwrap();
}

/// Android: the app runs `BiometricPrompt` and passes key material.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn external_material_flow() {
    let dir = tempfile::tempdir().unwrap();
    let external = Arc::new(ExternalDeviceKeys::new());
    let app = AppCore::with_platform(config(dir.path()), memory_store(), external.clone()).unwrap();
    app.create_local_profile("A".into(), PASSPHRASE.into())
        .await
        .unwrap();
    assert!(!app.device_unlock_info().await.unwrap().available);
    external.set_status(Some(OsAuthKind::Fingerprint), false);
    let info = app.device_unlock_info().await.unwrap();
    assert!(info.external && info.kind.as_deref() == Some("fingerprint"));
    // The in-core path refuses: the app must do the OS step.
    assert_eq!(
        app.set_device_unlock_enabled(true, "r".into(), false)
            .await
            .unwrap_err()
            .code(),
        "os_auth_failed"
    );
    let material = vec![7u8; 32];
    app.enable_device_unlock_with_material(
        material.clone(),
        b"keystore:p1:iv".to_vec(),
        "fingerprint".into(),
    )
    .await
    .unwrap();
    app.lock().await.unwrap();
    assert_eq!(
        app.device_unlock_handle().await.unwrap().as_deref(),
        Some(&b"keystore:p1:iv"[..])
    );
    assert_eq!(
        app.unlock_with_device("r".into()).await.unwrap_err().code(),
        "os_auth_failed"
    );
    assert_eq!(
        app.unlock_with_device_material(vec![1u8; 8])
            .await
            .unwrap_err()
            .code(),
        "invalid_input"
    );
    app.unlock_with_device_material(material.clone())
        .await
        .unwrap();
    assert!(app.is_unlocked().await);
    app.lock().await.unwrap();
    // KeyPermanentlyInvalidatedException in the app.
    app.device_unlock_invalidated().await.unwrap();
    let info = app.device_unlock_info().await.unwrap();
    assert!(!info.enabled);
    assert_eq!(info.reoffer.as_deref(), Some("invalidated"));
    assert!(app.device_unlock_handle().await.unwrap().is_none());
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
    app.set_device_unlock_enabled(true, "r".into(), false)
        .await
        .unwrap();
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

// ---- migration of a 0.8.x profile (criterion 5) ----------------------------------------

/// Write a profile the way 0.8.x left it: database key and search-index
/// key in the OS store, a local device envelope with the real VRK, the
/// device-unlock opt-in on, no key file.
async fn legacy_profile(
    root: &Path,
    store: &Arc<dyn SecureStore>,
    host_name: &str,
) -> (String, DatabaseKey) {
    let profiles = ProfileDirectory::new(root);
    let entry = profiles.create("Old", ProfileKind::Local).unwrap();
    let pid = entry.profile_id;
    let key = DatabaseKey::from_bytes([0x3c; 32]);
    store
        .set(&format!("profiles/{pid}/db-key"), key.expose_bytes())
        .unwrap();
    store
        .set(&format!("profiles/{pid}/search-index-key"), &[0x11; 32])
        .unwrap();
    let identity = DeviceIdentity::load_or_create(store.as_ref(), &pid.to_string()).unwrap();
    let created = create_vault(
        &identity,
        &SecretString::from(PASSPHRASE.to_owned()),
        Argon2Params::for_tests(),
        None,
        chrono::Utc::now(),
    )
    .unwrap();
    let vault = created.unlocked;
    let vault_id = vault.vault_id();
    let did = identity.device_id();
    let envs = created.create_request;
    let legacy_device_env = vault
        .device_envelope(did, &identity.public_keys().encryption)
        .unwrap();
    let storage = Storage::open(profiles.db_path(pid), key.clone())
        .await
        .unwrap();
    storage
        .put_profile(Profile::new_local(pid, did, Some("Old".into())))
        .await
        .unwrap();
    let host = Host::new(host_name, "192.0.2.1");
    let object_id = host.id;
    let payload = ObjectPayload::new(VaultObject::Host(host));
    let body = vault.encrypt_object(object_id, 1, &payload).unwrap();
    storage
        .write(move |tx| {
            let mut rec = VaultRecord::new(vault_id, cc_protocol::vaults::VaultRole::Owner);
            rec.password_envelope = Some(local_key_envelope(
                vault_id,
                envs.password_envelope,
                Some(did),
            ));
            rec.recovery_envelope = Some(local_key_envelope(
                vault_id,
                envs.recovery_envelope,
                Some(did),
            ));
            rec.device_envelope = Some(local_key_envelope(vault_id, legacy_device_env, Some(did)));
            tx.put_vault(&rec)?;
            tx.setting_set("app_core.vault_id", &vault_id)?;
            tx.setting_set("app_core.device_unlock_enabled", &true)?;
            tx.record_local_put::<cc_storage_core::StorageError, _>(
                vault_id,
                object_id,
                Some(KekClass::Inventory),
                |_rev| Ok(body),
            )?;
            Ok::<_, cc_storage_core::StorageError>(())
        })
        .await
        .unwrap();
    storage.close().await;
    (pid.to_string(), key)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn profile_from_0_8_is_migrated_on_passphrase_unlock() {
    let dir = tempfile::tempdir().unwrap();
    let store = memory_store();
    let (pid, old_key) = legacy_profile(dir.path(), &store, "legacy-host").await;
    let keys = Arc::new(InMemoryDeviceKeys::new());
    let app = app_with_keys(dir.path(), store.clone(), keys.clone());
    app.open_profile(pid.clone()).await.unwrap();
    // Device unlock is off until the passphrase unlock.
    let info = app.device_unlock_info().await.unwrap();
    assert!(!info.enabled && info.reoffer.is_none());
    assert_eq!(
        app.unlock_with_device("r".into()).await.unwrap_err().code(),
        "unsupported"
    );
    app.unlock_with_passphrase(PASSPHRASE.into()).await.unwrap();
    // Data survived the re-key.
    let hosts = app.list_hosts().await.unwrap();
    assert_eq!(hosts.len(), 1);
    assert_eq!(hosts[0].name, "legacy-host");
    // Old OS items deleted, key file written, the offer is pending.
    assert!(store
        .get(&format!("profiles/{pid}/db-key"))
        .unwrap()
        .is_none());
    assert!(store
        .get(&format!("profiles/{pid}/search-index-key"))
        .unwrap()
        .is_none());
    let kf = keyfile(dir.path(), &pid);
    assert!(kf["db_key_next"].is_null());
    assert_eq!(kf["flags"]["reoffer_device_unlock"], "migrated");
    let info = app.device_unlock_info().await.unwrap();
    assert_eq!(info.reoffer.as_deref(), Some("migrated"));
    // One tap + biometrics.
    app.set_device_unlock_enabled(true, "r".into(), false)
        .await
        .unwrap();
    assert!(app.device_unlock_info().await.unwrap().reoffer.is_none());
    app.shutdown().await.unwrap();

    // The old key no longer opens the database; the legacy device envelope
    // is gone from it.
    let db = profile_dir(dir.path(), &pid).join("vault.db");
    assert!(Storage::open(&db, old_key).await.is_err());

    let app = app_with_keys(dir.path(), store.clone(), keys.clone());
    app.open_profile(pid.clone()).await.unwrap();
    app.unlock_with_device("r".into()).await.unwrap();
    assert_eq!(app.list_hosts().await.unwrap().len(), 1);
    app.shutdown().await.unwrap();
}

/// TR-5: the re-key is transactional — a crash before or after
/// `PRAGMA rekey` leaves a profile that opens and finishes the move.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn interrupted_rekey_is_completed_on_next_unlock() {
    let dir = tempfile::tempdir().unwrap();
    let store = memory_store();
    let app = app_with(dir.path(), store.clone(), false);
    let pid = app
        .create_local_profile("A".into(), PASSPHRASE.into())
        .await
        .unwrap()
        .profile
        .id;
    app.save_host(HostDto::new("kept", "10.0.0.2"))
        .await
        .unwrap();
    app.shutdown().await.unwrap();

    let kf_path = profile_dir(dir.path(), &pid).join("vault-keys.json");
    let mut kf: Value = keyfile(dir.path(), &pid);
    let vault_id: cc_protocol::VaultId = serde_json::from_value(kf["vault_id"].clone()).unwrap();
    let env: cc_protocol::envelopes::KeyEnvelope =
        serde_json::from_value(kf["password_envelope"].clone()).unwrap();
    let vault =
        unlock_with_passphrase(vault_id, &env, &SecretString::from(PASSPHRASE.to_owned())).unwrap();
    let ctx = format!("db-key:{pid}").into_bytes();
    let next_key = [0x55u8; 32];
    let next = vault.seal_local_secret(&ctx, &next_key).unwrap();

    // Crash before the re-key: db_key fits, db_key_next pending.
    kf["db_key_next"] = serde_json::to_value(&next).unwrap();
    std::fs::write(&kf_path, serde_json::to_vec(&kf).unwrap()).unwrap();
    let app = app_with(dir.path(), store.clone(), false);
    app.open_profile(pid.clone()).await.unwrap();
    app.unlock_with_passphrase(PASSPHRASE.into()).await.unwrap();
    assert_eq!(app.list_hosts().await.unwrap().len(), 1);
    app.shutdown().await.unwrap();
    let kf2 = keyfile(dir.path(), &pid);
    assert!(kf2["db_key_next"].is_null());
    let db = profile_dir(dir.path(), &pid).join("vault.db");
    let st = Storage::open(&db, DatabaseKey::from_bytes(next_key))
        .await
        .expect("finished re-key to the pending key");
    st.close().await;

    // Crash after the re-key: the file is under db_key_next already, the
    // key file still names the old key first.
    let newer = [0x66u8; 32];
    let st = Storage::open(&db, DatabaseKey::from_bytes(next_key))
        .await
        .unwrap();
    st.rekey(DatabaseKey::from_bytes(newer)).await.unwrap();
    st.close().await;
    let mut kf3 = kf2.clone();
    kf3["db_key_next"] =
        serde_json::to_value(vault.seal_local_secret(&ctx, &newer).unwrap()).unwrap();
    std::fs::write(&kf_path, serde_json::to_vec(&kf3).unwrap()).unwrap();
    let app = app_with(dir.path(), store.clone(), false);
    app.open_profile(pid.clone()).await.unwrap();
    app.unlock_with_passphrase(PASSPHRASE.into()).await.unwrap();
    assert_eq!(app.list_hosts().await.unwrap()[0].name, "kept");
    app.shutdown().await.unwrap();
    assert!(keyfile(dir.path(), &pid)["db_key_next"].is_null());
}

// ---- synced profiles: trust marker (TR-2, criterion 6) -------------------------------

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn synced_device_envelope_is_a_trust_marker_and_sync_works() {
    let server = MockServer::start().await;
    let url = server.url().to_string();
    let tmp = tempfile::tempdir().unwrap();
    let store_a = memory_store();
    let a = app_with(&tmp.path().join("a"), store_a.clone(), true);
    let acct = a
        .create_synced_profile(
            "Work".into(),
            url.clone(),
            "trust@example.org".into(),
            ACCOUNT_PASSWORD.into(),
            AccountMode::Register,
        )
        .await
        .unwrap();
    let kit = a.create_vault(PASSPHRASE.into()).await.unwrap();
    let vault_id: cc_protocol::VaultId = kit.vault_id.parse().unwrap();
    a.save_host(HostDto::new("shared", "10.1.1.1"))
        .await
        .unwrap();
    a.sync_now().await.unwrap();

    // Trusted, but the server copy opens to no VRK with the device key.
    let a_id = DeviceIdentity::load(store_a.as_ref(), &acct.profile.id)
        .unwrap()
        .unwrap();
    assert!(server.device_trusted(a_id.device_id(), vault_id));
    let env = server.device_envelope(a_id.device_id(), vault_id).unwrap();
    let marker = unlock_with_device(vault_id, &env, &a_id).unwrap();
    let real = unlock_with_passphrase(
        vault_id,
        &kit_password_envelope(&tmp.path().join("a"), &acct.profile.id),
        &SecretString::from(PASSPHRASE.to_owned()),
    )
    .unwrap();
    assert_ne!(marker.vault_access_key(), real.vault_access_key());

    // A second device: approval delivers the real VRK once, then its own
    // server envelope is replaced by a trust marker too; sync works.
    let store_b = memory_store();
    let b = app_with(&tmp.path().join("b"), store_b.clone(), true);
    let acct_b = b
        .create_synced_profile(
            "Work".into(),
            url.clone(),
            "trust@example.org".into(),
            ACCOUNT_PASSWORD.into(),
            AccountMode::Login,
        )
        .await
        .unwrap();
    let req = b
        .request_device_approval(Some(kit.vault_id.clone()))
        .await
        .unwrap();
    a.start_device_approval(req.request_id.clone())
        .await
        .unwrap();
    a.confirm_device_approval(req.request_id).await.unwrap();
    assert!(b.finish_device_approval(None).await.unwrap());
    b.sync_now().await.unwrap();
    assert_eq!(b.list_hosts().await.unwrap().len(), 1);
    let b_id = DeviceIdentity::load(store_b.as_ref(), &acct_b.profile.id)
        .unwrap()
        .unwrap();
    assert!(server.device_trusted(b_id.device_id(), vault_id));
    let env_b = server.device_envelope(b_id.device_id(), vault_id).unwrap();
    let marker_b = unlock_with_device(vault_id, &env_b, &b_id).unwrap();
    assert_ne!(marker_b.vault_access_key(), real.vault_access_key());
    assert!(store_b
        .get(&format!("profiles/{}/db-key", acct_b.profile.id))
        .unwrap()
        .is_none());

    // B locks and unlocks with device unlock; edits still sync to A.
    b.set_device_unlock_enabled(true, "r".into(), false)
        .await
        .unwrap();
    b.lock().await.unwrap();
    b.unlock_with_device("r".into()).await.unwrap();
    b.save_host(HostDto::new("from-b", "10.1.1.2"))
        .await
        .unwrap();
    b.sync_now().await.unwrap();
    a.sync_now().await.unwrap();
    assert_eq!(a.list_hosts().await.unwrap().len(), 2);
    a.shutdown().await.unwrap();
    b.shutdown().await.unwrap();
}

fn kit_password_envelope(root: &Path, pid: &str) -> cc_protocol::envelopes::KeyEnvelope {
    serde_json::from_value(keyfile(root, pid)["password_envelope"].clone()).unwrap()
}

#[allow(dead_code)]
fn assert_send_sync<T: DeviceKeyProtector>() {}
