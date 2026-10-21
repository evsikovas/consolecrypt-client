//! Local-only profiles (ADR-0106): create and use a vault with no server,
//! persist envelopes locally, unlock from them, then enable sync.

mod common;

use cc_models::host::Host;
use cc_models::{ObjectPayload, VaultObject};
use cc_platform_core::{DeviceKeyProtector, FakePrompt, InMemoryDeviceKeys, InMemorySecureStore};
use cc_protocol::version::Platform;
use cc_vault_core::{
    create_vault, unlock_with_device, unlock_with_passphrase, unlock_with_recovery_input,
    unlock_with_unlock_key, DeviceIdentity, LocalVaultEnvelopes, UnlockKey, VaultError,
};
use common::*;

const PROFILE: &str = "local-personal";
const PASSPHRASE: &str = "local only passphrase";

#[test]
fn local_vault_lifecycle_and_enable_sync() {
    // First launch in local mode: identity in the secure store, vault created
    // offline; the CreateVaultRequest is simply not sent.
    let store = InMemorySecureStore::new();
    let identity = DeviceIdentity::load_or_create(&store, PROFILE).unwrap();
    let created = create_vault(&identity, &pass(PASSPHRASE), kdf(), None, now()).unwrap();
    assert_eq!(created.recovery_kit.server_url(), None);
    let vault_id = created.unlocked.vault_id();

    // Persist envelopes (e.g. JSON column in the local DB) and an object.
    let persisted = serde_json::to_string(&created.local_envelopes()).unwrap();
    let h = Host::new("nas", "192.168.1.10");
    let object_id = h.id;
    let payload = ObjectPayload::new(VaultObject::Host(h));
    let body = created
        .unlocked
        .encrypt_object(object_id, 1, &payload)
        .unwrap();
    let recovery_words = created.recovery_kit.phrase().expose_phrase().to_owned();
    drop(created);

    // Next app start: everything comes from local storage.
    let envs: LocalVaultEnvelopes = serde_json::from_str(&persisted).unwrap();
    assert_eq!(envs.vault_id, vault_id);
    let identity = DeviceIdentity::load(&store, PROFILE).unwrap().unwrap();

    let by_pass = unlock_with_passphrase(vault_id, &envs.password, &pass(PASSPHRASE)).unwrap();
    assert_eq!(
        by_pass.decrypt_object(object_id, 1, &body, None).unwrap().0,
        payload
    );

    // No device envelope is kept locally any more: the device key alone
    // never opens the vault (S5).
    assert!(envs.device.is_none());
    let by_device = &by_pass;

    // Forgot passphrase → Recovery Key against the local recovery envelope.
    let by_rk =
        unlock_with_recovery_input(vault_id, &envs.recovery, &pass(&recovery_words)).unwrap();
    let new_pw = by_rk
        .change_passphrase(&pass("new local passphrase"), kdf())
        .unwrap()
        .envelope;
    unlock_with_passphrase(vault_id, &new_pw, &pass("new local passphrase")).unwrap();

    // Enable sync: register this installation, then upload the EXISTING vault.
    let reg = identity
        .registration("Home PC", Platform::Windows, None)
        .unwrap();
    assert_eq!(reg.device_id, identity.device_id());
    let req = by_pass
        .enable_sync_request(&new_pw, &envs.recovery, &identity)
        .unwrap();
    assert_eq!(req.vault_id, vault_id);
    assert_eq!(req.password_envelope, new_pw);
    assert_eq!(req.recovery_envelope, envs.recovery);
    assert_eq!(req.vault_access_key, by_pass.vault_access_key());
    assert_eq!(
        req.device_envelope.recipient_id,
        Some(identity.device_id().0)
    );
    // The uploaded device envelope is a trust marker without the VRK.
    let server_dev = stored(vault_id, &req.device_envelope);
    let marker = unlock_with_device(vault_id, &server_dev, &identity).unwrap();
    assert_ne!(marker.vault_access_key(), by_pass.vault_access_key());
    let server_rec = stored(vault_id, &req.recovery_envelope);
    unlock_with_recovery_input(vault_id, &server_rec, &pass(&recovery_words)).unwrap();

    // Local objects are re-encrypted as revision 1 creates.
    let (rev1, _) = by_pass.reencrypt_object(object_id, 1, &body, 1).unwrap();
    assert_eq!(
        by_device
            .decrypt_object(object_id, 1, &rev1, None)
            .unwrap()
            .0,
        payload
    );
}

#[test]
fn enable_sync_rejects_wrong_envelopes() {
    let identity = DeviceIdentity::generate().unwrap();
    let created = create_vault(&identity, &pass(PASSPHRASE), kdf(), None, now()).unwrap();
    let envs = created.local_envelopes();
    // Swapped password/recovery.
    assert!(matches!(
        created
            .unlocked
            .enable_sync_request(&envs.recovery, &envs.password, &identity),
        Err(VaultError::Crypto(_))
    ));
    // Server-labelled envelope of another vault.
    let foreign = stored(cc_protocol::VaultId::new(), &envs.password);
    assert_eq!(
        created
            .unlocked
            .enable_sync_request(&foreign, &envs.recovery, &identity)
            .err(),
        Some(VaultError::VaultMismatch)
    );
    // Structurally invalid (tampered KDF params beyond the ceiling).
    let mut bad = envs.password.clone();
    bad.metadata.kdf.as_mut().unwrap().memory_kib = u32::MAX;
    assert!(created
        .unlocked
        .enable_sync_request(&bad, &envs.recovery, &identity)
        .is_err());
}

#[test]
fn device_unlock_needs_the_os_released_key() {
    let store = InMemorySecureStore::new();
    let identity = DeviceIdentity::load_or_create(&store, PROFILE).unwrap();
    let created = create_vault(&identity, &pass(PASSPHRASE), kdf(), None, now()).unwrap();
    let v = created.unlocked.vault_id();
    let os = InMemoryDeviceKeys::new();
    let protected = os.create(PROFILE, "enable").unwrap();
    let key = UnlockKey::from_material(&protected.material).unwrap();
    let env = created
        .unlocked
        .seal_unlock_envelope(&key, PROFILE.as_bytes())
        .unwrap();

    // Approved prompt → the OS releases the material → vault opens.
    let material = os.release(&protected.handle, "unlock").unwrap();
    let u = unlock_with_unlock_key(
        v,
        &env,
        &UnlockKey::from_material(&material).unwrap(),
        PROFILE.as_bytes(),
    )
    .unwrap();
    assert_eq!(u.vault_access_key(), created.unlocked.vault_access_key());

    // Cancelled prompt → nothing to open the envelope with.
    os.set_next(FakePrompt::Cancel);
    assert!(os.release(&protected.handle, "unlock").is_err());

    // Everything in the plain secret store (device identity) is useless.
    let identity = DeviceIdentity::load(&store, PROFILE).unwrap().unwrap();
    let guess =
        UnlockKey::from_material(identity.device_id().0.as_bytes().repeat(2).as_slice()).unwrap();
    assert_eq!(
        unlock_with_unlock_key(v, &env, &guess, PROFILE.as_bytes()).err(),
        Some(VaultError::DeviceNotAuthorized)
    );
}
