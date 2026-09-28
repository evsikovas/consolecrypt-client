//! Envelope behaviour through the public API: round trips and every
//! rejection path (wrong passphrase / recovery key / device, tampering,
//! structural validation, KDF bounds).

use cc_crypto_core::{
    open_device_envelope, open_password_envelope, open_recovery_envelope, password_envelope_params,
    seal_device_envelope, seal_password_envelope, seal_recovery_envelope,
    stored_envelope_for_vault, Argon2Params, CryptoError, DeviceSecretKeys, KeyHierarchy,
    RecoveryKey, SecretString, Vrk,
};
use cc_protocol::envelopes::{
    EnvelopeValidationError, KdfParams, KeyEnvelope, NewEnvelope, RecipientType,
};
use cc_protocol::{Bytes, DeviceId, EnvelopeId, VaultId};

fn pass(s: &str) -> SecretString {
    SecretString::from(s)
}

/// Same VRK ⇔ same derived VAK (the VRK itself is not observable).
fn same_vrk(a: &Vrk, b: &Vrk, vault: VaultId) -> bool {
    KeyHierarchy::derive(a, vault).vault_access_key().verifier()
        == KeyHierarchy::derive(b, vault).vault_access_key().verifier()
}

fn params() -> Argon2Params {
    Argon2Params::for_tests()
}

#[test]
fn password_roundtrip_and_wrong_passphrase() {
    let vault = VaultId::new();
    let vrk = Vrk::generate().unwrap();
    let env = seal_password_envelope(&vrk, vault, &pass("hunter2 hunter2"), params()).unwrap();
    env.validate().unwrap();
    assert_eq!(env.recipient_type, RecipientType::Password);
    assert_eq!(password_envelope_params(&env).unwrap(), params());

    let opened = open_password_envelope(&env, vault, &pass("hunter2 hunter2")).unwrap();
    assert!(same_vrk(&vrk, &opened, vault));

    assert_eq!(
        open_password_envelope(&env, vault, &pass("hunter3 hunter3")).err(),
        Some(CryptoError::Decrypt)
    );
    assert_eq!(
        open_password_envelope(&env, vault, &pass("")).err(),
        Some(CryptoError::EmptyPassphrase)
    );
}

#[test]
fn passphrase_is_nfc_normalized() {
    let vault = VaultId::new();
    let vrk = Vrk::generate().unwrap();
    let env = seal_password_envelope(&vrk, vault, &pass("pa\u{301}ssword-long"), params()).unwrap();
    // Precomposed á opens an envelope sealed with a + combining acute.
    let opened = open_password_envelope(&env, vault, &pass("p\u{e1}ssword-long")).unwrap();
    assert!(same_vrk(&vrk, &opened, vault));
}

#[test]
fn fresh_salt_and_nonce_per_seal() {
    let vault = VaultId::new();
    let vrk = Vrk::generate().unwrap();
    let a = seal_password_envelope(&vrk, vault, &pass("same passphrase"), params()).unwrap();
    let b = seal_password_envelope(&vrk, vault, &pass("same passphrase"), params()).unwrap();
    assert_ne!(a.nonce, b.nonce);
    assert_ne!(a.metadata.kdf.unwrap().salt, b.metadata.kdf.unwrap().salt);
    assert_ne!(a.ciphertext, b.ciphertext);
}

#[test]
fn envelope_bound_to_vault_id() {
    let vault = VaultId::new();
    let vrk = Vrk::generate().unwrap();
    let rk = RecoveryKey::generate().unwrap();
    let env = seal_recovery_envelope(&vrk, vault, &rk).unwrap();
    assert_eq!(
        open_recovery_envelope(&env, VaultId::new(), &rk).err(),
        Some(CryptoError::Decrypt)
    );
    let env = seal_password_envelope(&vrk, vault, &pass("passphrase!"), params()).unwrap();
    assert_eq!(
        open_password_envelope(&env, VaultId::new(), &pass("passphrase!")).err(),
        Some(CryptoError::Decrypt)
    );
}

#[test]
fn recovery_roundtrip_and_wrong_key() {
    let vault = VaultId::new();
    let vrk = Vrk::generate().unwrap();
    let rk = RecoveryKey::generate().unwrap();
    let env = seal_recovery_envelope(&vrk, vault, &rk).unwrap();
    env.validate().unwrap();

    let via_words = RecoveryKey::from_mnemonic(rk.to_mnemonic().expose_phrase()).unwrap();
    assert!(same_vrk(
        &vrk,
        &open_recovery_envelope(&env, vault, &via_words).unwrap(),
        vault
    ));
    let (v, via_qr) = RecoveryKey::from_qr_payload(&rk.to_qr_payload(vault)).unwrap();
    assert_eq!(v, vault);
    assert!(same_vrk(
        &vrk,
        &open_recovery_envelope(&env, vault, &via_qr).unwrap(),
        vault
    ));

    let other = RecoveryKey::generate().unwrap();
    assert_eq!(
        open_recovery_envelope(&env, vault, &other).err(),
        Some(CryptoError::Decrypt)
    );
}

#[test]
fn device_roundtrip_wrong_device_and_wrong_id() {
    let vault = VaultId::new();
    let vrk = Vrk::generate().unwrap();
    let dev_id = DeviceId::new();
    let dev = DeviceSecretKeys::generate().unwrap();
    let env = seal_device_envelope(&vrk, vault, dev_id, &dev.public_keys().encryption).unwrap();
    env.validate().unwrap();
    assert_eq!(env.recipient_id, Some(dev_id.0));

    assert!(same_vrk(
        &vrk,
        &open_device_envelope(&env, vault, dev_id, &dev).unwrap(),
        vault
    ));

    // Right id, wrong keys.
    let thief = DeviceSecretKeys::generate().unwrap();
    assert_eq!(
        open_device_envelope(&env, vault, dev_id, &thief).err(),
        Some(CryptoError::Decrypt)
    );
    // Envelope addressed to someone else.
    assert_eq!(
        open_device_envelope(&env, vault, DeviceId::new(), &dev).err(),
        Some(CryptoError::WrongRecipientDevice)
    );
    // Re-addressing the envelope does not help: recipient id is in the AAD.
    let other_id = DeviceId::new();
    let mut readdressed = env.clone();
    readdressed.recipient_id = Some(other_id.0);
    assert_eq!(
        open_device_envelope(&readdressed, vault, other_id, &dev).err(),
        Some(CryptoError::Decrypt)
    );
    // Two seals for the same device use different ephemeral keys.
    let env2 = seal_device_envelope(&vrk, vault, dev_id, &dev.public_keys().encryption).unwrap();
    assert_ne!(
        env.metadata.ephemeral_public_key,
        env2.metadata.ephemeral_public_key
    );
}

#[test]
fn low_order_keys_rejected() {
    let vault = VaultId::new();
    let vrk = Vrk::generate().unwrap();
    // Sealing to an all-zero (low-order) recipient key.
    assert_eq!(
        seal_device_envelope(&vrk, vault, DeviceId::new(), &[0u8; 32]).err(),
        Some(CryptoError::WeakKeyAgreement)
    );
    // Opening an envelope whose ephemeral key is low order.
    let id = DeviceId::new();
    let dev = DeviceSecretKeys::generate().unwrap();
    let mut env = seal_device_envelope(&vrk, vault, id, &dev.public_keys().encryption).unwrap();
    env.metadata.ephemeral_public_key = Some(Bytes::new(vec![0u8; 32]));
    assert_eq!(
        open_device_envelope(&env, vault, id, &dev).err(),
        Some(CryptoError::WeakKeyAgreement)
    );
}

fn flip(b: &mut Bytes, i: usize) {
    b.0[i] ^= 0x01;
}

#[test]
fn tampering_is_detected() {
    let vault = VaultId::new();
    let vrk = Vrk::generate().unwrap();
    let rk = RecoveryKey::generate().unwrap();
    let env = seal_recovery_envelope(&vrk, vault, &rk).unwrap();

    for i in [0, 20, 47] {
        let mut e = env.clone();
        flip(&mut e.ciphertext, i);
        assert_eq!(
            open_recovery_envelope(&e, vault, &rk).err(),
            Some(CryptoError::Decrypt)
        );
    }
    let mut e = env.clone();
    flip(&mut e.nonce, 5);
    assert_eq!(
        open_recovery_envelope(&e, vault, &rk).err(),
        Some(CryptoError::Decrypt)
    );

    let dev_id = DeviceId::new();
    let dev = DeviceSecretKeys::generate().unwrap();
    let denv = seal_device_envelope(&vrk, vault, dev_id, &dev.public_keys().encryption).unwrap();
    let mut e = denv.clone();
    flip(e.metadata.ephemeral_public_key.as_mut().unwrap(), 3);
    assert!(open_device_envelope(&e, vault, dev_id, &dev).is_err());

    let penv = seal_password_envelope(&vrk, vault, &pass("passphrase!"), params()).unwrap();
    let mut e = penv.clone();
    flip(&mut e.metadata.kdf.as_mut().unwrap().salt, 0);
    assert_eq!(
        open_password_envelope(&e, vault, &pass("passphrase!")).err(),
        Some(CryptoError::Decrypt)
    );
    let mut e = penv.clone();
    e.metadata.kdf.as_mut().unwrap().iterations += 1;
    assert_eq!(
        open_password_envelope(&e, vault, &pass("passphrase!")).err(),
        Some(CryptoError::Decrypt)
    );
}

#[test]
fn structural_validation_runs_first() {
    let vault = VaultId::new();
    let vrk = Vrk::generate().unwrap();
    let rk = RecoveryKey::generate().unwrap();
    let env = seal_recovery_envelope(&vrk, vault, &rk).unwrap();

    let mut e = env.clone();
    e.nonce = Bytes::new(vec![0; 12]);
    assert_eq!(
        open_recovery_envelope(&e, vault, &rk).err(),
        Some(CryptoError::InvalidEnvelope(EnvelopeValidationError::Nonce))
    );
    let mut e = env.clone();
    e.ciphertext.0.push(0);
    assert_eq!(
        open_recovery_envelope(&e, vault, &rk).err(),
        Some(CryptoError::InvalidEnvelope(
            EnvelopeValidationError::Ciphertext
        ))
    );
    // Recovery envelope presented as a password envelope.
    assert!(matches!(
        open_password_envelope(&env, vault, &pass("x")),
        Err(CryptoError::WrongRecipientType { .. })
    ));
    // Algorithm/recipient mismatch.
    let mut e = env.clone();
    e.recipient_type = RecipientType::Password;
    assert_eq!(
        open_password_envelope(&e, vault, &pass("x")).err(),
        Some(CryptoError::InvalidEnvelope(
            EnvelopeValidationError::AlgorithmMismatch
        ))
    );
}

#[test]
fn kdf_ceiling_and_floor_enforced_before_argon2() {
    let vault = VaultId::new();
    let vrk = Vrk::generate().unwrap();
    let env = seal_password_envelope(&vrk, vault, &pass("passphrase!"), params()).unwrap();

    let with = |f: &dyn Fn(&mut KdfParams)| -> NewEnvelope {
        let mut e = env.clone();
        f(e.metadata.kdf.as_mut().unwrap());
        e
    };
    // A hostile server asking for 4 TiB / huge iteration counts must fail
    // fast (this test would take forever or OOM otherwise).
    for e in [
        with(&|k| k.memory_kib = u32::MAX),
        with(&|k| k.memory_kib = KdfParams::MAX_MEMORY_KIB + 1),
        with(&|k| k.iterations = 1_000_000),
        with(&|k| k.parallelism = 1_000),
        with(&|k| k.memory_kib = 1024),
        with(&|k| k.iterations = 1),
    ] {
        assert_eq!(
            open_password_envelope(&e, vault, &pass("passphrase!")).err(),
            Some(CryptoError::InvalidEnvelope(EnvelopeValidationError::Kdf))
        );
    }
    // Local construction below the floor is impossible.
    assert!(Argon2Params::new(1024, 1, 1).is_err());
}

#[test]
fn stored_envelopes_are_checked_for_vault() {
    let vault = VaultId::new();
    let vrk = Vrk::generate().unwrap();
    let rk = RecoveryKey::generate().unwrap();
    let new = seal_recovery_envelope(&vrk, vault, &rk).unwrap();
    let stored = KeyEnvelope {
        envelope_id: EnvelopeId::new(),
        vault_id: vault,
        recipient_type: new.recipient_type,
        recipient_id: new.recipient_id,
        kind: new.kind,
        metadata: new.metadata.clone(),
        ciphertext: new.ciphertext.clone(),
        nonce: new.nonce.clone(),
        created_at: chrono::Utc::now(),
        created_by_device_id: None,
        revoked_at: None,
    };
    let back = stored_envelope_for_vault(&stored, vault).unwrap();
    assert!(same_vrk(
        &vrk,
        &open_recovery_envelope(&back, vault, &rk).unwrap(),
        vault
    ));
    assert_eq!(
        stored_envelope_for_vault(&stored, VaultId::new()).err(),
        Some(CryptoError::VaultMismatch)
    );
}

#[test]
fn default_params_are_the_adr_values() {
    let p = Argon2Params::DEFAULT;
    assert_eq!(
        (p.memory_kib(), p.iterations(), p.parallelism()),
        (65536, 3, 1)
    );
    assert!(p.memory_kib() >= KdfParams::MIN_MEMORY_KIB);
}
