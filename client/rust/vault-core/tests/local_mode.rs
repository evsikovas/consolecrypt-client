//! Local-only profiles (ADR-0106): create and use a vault with no server,
//! persist envelopes locally, unlock from them, then enable sync.

mod common;

use cc_models::host::Host;
use cc_models::{ObjectPayload, VaultObject};
use cc_platform_core::{
    InMemorySecureStore, OsAuthAvailability, OsAuthError, OsAuthKind, OsAuthenticator,
    UnsupportedOsAuthenticator,
};
use cc_protocol::version::Platform;
use cc_vault_core::{
    create_vault, unlock_with_device, unlock_with_os_auth, unlock_with_passphrase,
    unlock_with_recovery_input, DeviceIdentity, LocalVaultEnvelopes, VaultError,
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

    // OS/biometric unlock path: device envelope + device keys from the store.
    let device_env = envs.device.as_ref().unwrap();
    let by_device = unlock_with_device(vault_id, device_env, &identity).unwrap();
    assert_eq!(
        by_device
            .decrypt_object(object_id, 1, &body, None)
            .unwrap()
            .0,
        payload
    );

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
    assert_ne!(&req.device_envelope, device_env, "device envelope is fresh");
    // The uploaded envelopes still work from the server's copy.
    let server_dev = stored(vault_id, &req.device_envelope);
    unlock_with_device(vault_id, &server_dev, &identity).unwrap();
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

/// Test double standing in for Touch ID / Windows Hello.
#[derive(Debug)]
struct FakeBiometrics {
    approve: bool,
}

impl OsAuthenticator for FakeBiometrics {
    fn availability(&self) -> OsAuthAvailability {
        OsAuthAvailability::Available(OsAuthKind::TouchId)
    }
    fn authenticate(&self, _reason: &str) -> Result<(), OsAuthError> {
        if self.approve {
            Ok(())
        } else {
            Err(OsAuthError::Cancelled)
        }
    }
}

#[test]
fn os_auth_unlock_uses_device_envelope_and_secure_store() {
    let store = InMemorySecureStore::new();
    let identity = DeviceIdentity::load_or_create(&store, PROFILE).unwrap();
    let created = create_vault(&identity, &pass(PASSPHRASE), kdf(), None, now()).unwrap();
    let v = created.unlocked.vault_id();
    let env = created.local_envelopes().device.unwrap();
    let reason = "Unlock your ConsoleCrypt vault";

    let u = unlock_with_os_auth(
        v,
        &env,
        &store,
        PROFILE,
        &FakeBiometrics { approve: true },
        reason,
    )
    .unwrap();
    assert_eq!(u.vault_access_key(), created.unlocked.vault_access_key());

    assert_eq!(
        unlock_with_os_auth(
            v,
            &env,
            &store,
            PROFILE,
            &FakeBiometrics { approve: false },
            reason
        )
        .err(),
        Some(VaultError::OsAuth(OsAuthError::Cancelled))
    );
    assert_eq!(
        unlock_with_os_auth(
            v,
            &env,
            &store,
            PROFILE,
            &UnsupportedOsAuthenticator,
            reason
        )
        .err(),
        Some(VaultError::OsAuth(OsAuthError::Unsupported))
    );
    assert_eq!(
        unlock_with_os_auth(
            v,
            &env,
            &store,
            "other-profile",
            &FakeBiometrics { approve: true },
            reason
        )
        .err(),
        Some(VaultError::NoDeviceIdentity)
    );
}
