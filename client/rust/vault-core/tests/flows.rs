//! Full vault flows (synced profile): create → unlock by every method,
//! device approval, attestation, passphrase change, recovery-kit
//! regeneration, and clean failures on wrong inputs.

mod common;

use cc_crypto_core::{verify_device_approval, DeviceApproval};
use cc_models::host::Host;
use cc_models::secret::{Secret, SecretKind, SecretValue};
use cc_models::{KekClass, ObjectPayload, VaultObject};
use cc_protocol::devices::{DeviceRequestStatus, DeviceStatus};
use cc_protocol::version::Platform;
use cc_protocol::{ObjectId, VaultId};
use cc_vault_core::{
    create_vault, unlock_with_device, unlock_with_passphrase, unlock_with_recovery_input,
    CreatedVault, DeviceIdentity, PendingApproval, UnlockedVault, VaultError,
};
use common::*;

const PASSPHRASE: &str = "correct horse battery staple";

fn new_vault(identity: &DeviceIdentity) -> CreatedVault {
    create_vault(
        identity,
        &pass(PASSPHRASE),
        kdf(),
        Some("https://sync.example.org"),
        now(),
    )
    .unwrap()
}

fn host() -> (ObjectId, ObjectPayload) {
    let h = Host::new("prod-db", "10.10.10.20");
    (h.id, ObjectPayload::new(VaultObject::Host(h)))
}

/// Encrypt with `a`, decrypt with `b`: proves both hold the same VRK.
fn assert_same_vault(a: &UnlockedVault, b: &UnlockedVault) {
    assert_eq!(a.vault_id(), b.vault_id());
    assert_eq!(a.vault_access_key(), b.vault_access_key());
    let (id, p) = host();
    let body = a.encrypt_object(id, 1, &p).unwrap();
    let (back, class) = b.decrypt_object(id, 1, &body, None).unwrap();
    assert_eq!(back, p);
    assert_eq!(class, KekClass::Inventory);
}

#[test]
fn create_then_unlock_by_every_method() {
    let device = DeviceIdentity::generate().unwrap();
    let created = new_vault(&device);
    let req = &created.create_request;
    let vault_id = req.vault_id;

    // Request shape.
    assert_eq!(req.vault_access_key.len(), 32);
    for e in [
        &req.password_envelope,
        &req.recovery_envelope,
        &req.device_envelope,
    ] {
        e.validate().unwrap();
    }
    assert_eq!(req.device_envelope.recipient_id, Some(device.device_id().0));
    assert_eq!(created.recovery_kit.vault_id(), vault_id);
    assert_eq!(
        created.recovery_kit.server_url(),
        Some("https://sync.example.org")
    );

    // What the server would hand back.
    let pw = stored(vault_id, &req.password_envelope);
    let rec = stored(vault_id, &req.recovery_envelope);
    let dev = stored(vault_id, &req.device_envelope);

    let by_pass = unlock_with_passphrase(vault_id, &pw, &pass(PASSPHRASE)).unwrap();
    assert_same_vault(&created.unlocked, &by_pass);

    let by_device = unlock_with_device(vault_id, &dev, &device).unwrap();
    assert_same_vault(&created.unlocked, &by_device);

    let words = pass(created.recovery_kit.phrase().expose_phrase());
    let by_words = unlock_with_recovery_input(vault_id, &rec, &words).unwrap();
    assert_same_vault(&created.unlocked, &by_words);

    let qr = pass(created.recovery_kit.expose_qr_payload());
    let by_qr = unlock_with_recovery_input(vault_id, &rec, &qr).unwrap();
    assert_same_vault(&created.unlocked, &by_qr);

    // Locking consumes and zeroizes.
    by_qr.lock();
}

#[test]
fn wrong_inputs_fail_cleanly() {
    let device = DeviceIdentity::generate().unwrap();
    let created = new_vault(&device);
    let req = &created.create_request;
    let v = req.vault_id;
    let pw = stored(v, &req.password_envelope);
    let rec = stored(v, &req.recovery_envelope);
    let dev = stored(v, &req.device_envelope);

    assert_eq!(
        unlock_with_passphrase(v, &pw, &pass("correct horse battery stapl")).err(),
        Some(VaultError::WrongPassphrase)
    );
    assert_eq!(
        unlock_with_passphrase(v, &pw, &pass("")).err(),
        Some(VaultError::WrongPassphrase)
    );

    let other_kit = new_vault(&device).recovery_kit;
    assert_eq!(
        unlock_with_recovery_input(v, &rec, &pass(other_kit.phrase().expose_phrase())).err(),
        Some(VaultError::WrongRecoveryKey)
    );
    // QR code of another vault is recognized as such.
    assert_eq!(
        unlock_with_recovery_input(v, &rec, &pass(other_kit.expose_qr_payload())).err(),
        Some(VaultError::RecoveryKeyForOtherVault)
    );
    assert!(matches!(
        unlock_with_recovery_input(v, &rec, &pass("abandon abandon")),
        Err(VaultError::Crypto(_))
    ));

    let stranger = DeviceIdentity::generate().unwrap();
    assert_eq!(
        unlock_with_device(v, &dev, &stranger).err(),
        Some(VaultError::DeviceNotAuthorized)
    );

    // Server returns an envelope labelled with another vault id.
    let mislabelled = stored(VaultId::new(), &req.password_envelope);
    assert_eq!(
        unlock_with_passphrase(v, &mislabelled, &pass(PASSPHRASE)).err(),
        Some(VaultError::VaultMismatch)
    );
    // Password envelope offered for device unlock.
    assert!(matches!(
        unlock_with_device(v, &pw, &device),
        Err(VaultError::Crypto(_))
    ));

    // Weak passphrase at creation.
    assert_eq!(
        create_vault(&device, &pass("short"), kdf(), None, now()).err(),
        Some(VaultError::WeakPassphrase)
    );
}

#[test]
fn approve_new_device_and_it_opens_its_envelope() {
    // Device A (trusted) created the vault.
    let a = DeviceIdentity::generate().unwrap();
    let created = new_vault(&a);
    let v = created.create_request.vault_id;
    let a_reg = a.registration("Laptop A", Platform::Macos, None).unwrap();

    // Device B logs in and asks for trust.
    let b = DeviceIdentity::generate().unwrap();
    let b_reg = b
        .registration("Desktop B", Platform::Windows, None)
        .unwrap();
    let request = trust_request(&b_reg, vec![v]);

    // A validates the request and shows the code; B shows its own code.
    let pending = PendingApproval::new(&request, &a, now()).unwrap();
    assert_eq!(pending.verification_code(), b.verification_code());
    assert_eq!(pending.new_device_name(), "Desktop B");
    let confirmed = pending.confirm_verification_code();
    let approve = confirmed
        .build_request(&a, &[&created.unlocked], now())
        .unwrap();

    // Server-side checks: signature by A's registered key over the canonical message.
    assert_eq!(approve.request_id, request.request_id);
    assert_eq!(approve.envelopes.len(), 1);
    let b_keys = cc_crypto_core::DevicePublicKeys::from_device_info(&request.device).unwrap();
    let a_sign: [u8; 32] = a_reg.signing_public_key.to_array().unwrap();
    let vault_ids = [v];
    verify_device_approval(
        &a_sign,
        &DeviceApproval {
            request_id: request.request_id,
            approver_device_id: a.device_id(),
            new_device_id: b.device_id(),
            new_device_keys: &b_keys,
            issued_at: approve.issued_at,
            vault_ids: &vault_ids,
        },
        approve.signature.as_slice(),
    )
    .unwrap();

    // B opens the envelope it was given (as returned by the server).
    let env = stored(v, &approve.envelopes[0].envelope);
    let b_vault = unlock_with_device(v, &env, &b).unwrap();
    assert_same_vault(&created.unlocked, &b_vault);
    // A cannot open B's envelope with A's keys.
    assert_eq!(
        unlock_with_device(v, &env, &a).err(),
        Some(VaultError::DeviceNotAuthorized)
    );
}

#[test]
fn malicious_server_key_substitution_changes_the_code() {
    let a = DeviceIdentity::generate().unwrap();
    let created = new_vault(&a);
    let v = created.create_request.vault_id;
    let b = DeviceIdentity::generate().unwrap();
    let b_reg = b.registration("B", Platform::Linux, None).unwrap();

    // The server swaps in its own keys for B.
    let evil = DeviceIdentity::generate().unwrap();
    let mut forged = trust_request(&b_reg, vec![v]);
    forged.device.encryption_public_key = evil.public_keys().encryption_bytes();
    forged.device.signing_public_key = evil.public_keys().signing_bytes();

    let pending = PendingApproval::new(&forged, &a, now()).unwrap();
    // The codes differ → the user must reject.
    assert_ne!(pending.verification_code(), b.verification_code());
}

#[test]
fn approval_preconditions() {
    let a = DeviceIdentity::generate().unwrap();
    let created = new_vault(&a);
    let v = created.create_request.vault_id;
    let b = DeviceIdentity::generate().unwrap();
    let b_reg = b.registration("B", Platform::Cli, None).unwrap();

    let mut r = trust_request(&b_reg, vec![v]);
    r.status = DeviceRequestStatus::Approved;
    assert!(matches!(
        PendingApproval::new(&r, &a, now()),
        Err(VaultError::Approval(_))
    ));

    let mut r = trust_request(&b_reg, vec![v]);
    r.expires_at = now() - chrono::Duration::seconds(1);
    assert!(matches!(
        PendingApproval::new(&r, &a, now()),
        Err(VaultError::Approval(_))
    ));

    let mut r = trust_request(&b_reg, vec![v]);
    r.device.status = DeviceStatus::Revoked;
    assert!(matches!(
        PendingApproval::new(&r, &a, now()),
        Err(VaultError::Approval(_))
    ));

    let a_reg = a.registration("A", Platform::Cli, None).unwrap();
    let r = trust_request(&a_reg, vec![v]);
    assert!(matches!(
        PendingApproval::new(&r, &a, now()),
        Err(VaultError::Approval(_))
    ));

    let mut r = trust_request(&b_reg, vec![v]);
    r.device.signing_public_key.0.truncate(10);
    assert!(matches!(
        PendingApproval::new(&r, &a, now()),
        Err(VaultError::Crypto(_))
    ));

    // Vault not in the request / no vaults / duplicates / expired at build time.
    let other = new_vault(&a);
    let r = trust_request(&b_reg, vec![v]);
    let c = PendingApproval::new(&r, &a, now())
        .unwrap()
        .confirm_verification_code();
    assert!(matches!(
        c.build_request(&a, &[&other.unlocked], now()),
        Err(VaultError::Approval("vault was not requested"))
    ));
    assert!(matches!(
        c.build_request(&a, &[], now()),
        Err(VaultError::Approval(_))
    ));
    assert!(matches!(
        c.build_request(&a, &[&created.unlocked, &created.unlocked], now()),
        Err(VaultError::Approval(_))
    ));
    assert!(matches!(
        c.build_request(
            &a,
            &[&created.unlocked],
            now() + chrono::Duration::hours(25)
        ),
        Err(VaultError::Approval(_))
    ));

    // Empty vault list in the request = all vaults: several may be approved.
    let r = trust_request(&b_reg, vec![]);
    let c = PendingApproval::new(&r, &a, now())
        .unwrap()
        .confirm_verification_code();
    let req = c
        .build_request(&a, &[&created.unlocked, &other.unlocked], now())
        .unwrap();
    assert_eq!(req.envelopes.len(), 2);
    for ve in &req.envelopes {
        unlock_with_device(ve.vault_id, &ve.envelope, &b).unwrap();
    }
}

#[test]
fn attest_after_passphrase_unlock_on_new_device() {
    let a = DeviceIdentity::generate().unwrap();
    let created = new_vault(&a);
    let v = created.create_request.vault_id;
    let pw = stored(v, &created.create_request.password_envelope);

    // New device B: login → download password envelope → unlock → attest.
    let b = DeviceIdentity::generate().unwrap();
    let b_vault = unlock_with_passphrase(v, &pw, &pass(PASSPHRASE)).unwrap();
    let attest = b_vault.attest_device_request(&b).unwrap();
    assert_eq!(attest.vault_id, v);
    // Server check: SHA-256(VAK) equals the verifier stored at creation.
    assert_eq!(
        cc_crypto_core::vault_access_key_verifier(attest.vault_access_key.as_slice()),
        cc_crypto_core::vault_access_key_verifier(
            created.create_request.vault_access_key.as_slice()
        )
    );
    attest.envelope.validate().unwrap();
    let again = unlock_with_device(v, &attest.envelope, &b).unwrap();
    assert_same_vault(&created.unlocked, &again);
}

#[test]
fn change_passphrase() {
    let a = DeviceIdentity::generate().unwrap();
    let created = new_vault(&a);
    let v = created.create_request.vault_id;
    let replace = created
        .unlocked
        .change_passphrase(&pass("a much better passphrase"), kdf())
        .unwrap();
    assert_eq!(replace.vault_id, v);
    assert_eq!(
        replace.vault_access_key,
        created.create_request.vault_access_key
    );
    let new_env = stored(v, &replace.envelope);
    let unlocked = unlock_with_passphrase(v, &new_env, &pass("a much better passphrase")).unwrap();
    assert_same_vault(&created.unlocked, &unlocked);
    assert_eq!(
        unlock_with_passphrase(v, &new_env, &pass(PASSPHRASE)).err(),
        Some(VaultError::WrongPassphrase)
    );
    assert_eq!(
        created
            .unlocked
            .change_passphrase(&pass("1234567"), kdf())
            .err(),
        Some(VaultError::WeakPassphrase)
    );
}

#[test]
fn forgot_passphrase_with_trusted_device_then_set_new() {
    // ADR-0004 recovery scenario 1: device envelope → VRK → new passphrase.
    let a = DeviceIdentity::generate().unwrap();
    let created = new_vault(&a);
    let v = created.create_request.vault_id;
    let dev = stored(v, &created.create_request.device_envelope);
    let unlocked = unlock_with_device(v, &dev, &a).unwrap();
    let replace = unlocked
        .change_passphrase(&pass("brand new passphrase"), kdf())
        .unwrap();
    unlock_with_passphrase(v, &replace.envelope, &pass("brand new passphrase")).unwrap();
}

#[test]
fn regenerate_recovery_kit() {
    let a = DeviceIdentity::generate().unwrap();
    let created = new_vault(&a);
    let v = created.create_request.vault_id;
    let (kit, replace) = created
        .unlocked
        .regenerate_recovery_kit(Some("https://sync.example.org"), now())
        .unwrap();
    assert_eq!(kit.vault_id(), v);
    assert_eq!(
        replace.envelope.recipient_type,
        cc_protocol::envelopes::RecipientType::Recovery
    );
    let new_env = stored(v, &replace.envelope);
    let unlocked =
        unlock_with_recovery_input(v, &new_env, &pass(kit.phrase().expose_phrase())).unwrap();
    assert_same_vault(&created.unlocked, &unlocked);
    // The old kit no longer opens the new envelope.
    assert_eq!(
        unlock_with_recovery_input(
            v,
            &new_env,
            &pass(created.recovery_kit.phrase().expose_phrase())
        )
        .err(),
        Some(VaultError::WrongRecoveryKey)
    );
    // Onboarding check on the new kit.
    let check = kit.start_check().unwrap();
    let answers = check
        .positions()
        .map(|p| kit.phrase().word(p).unwrap().to_owned());
    assert!(check.verify([&answers[0], &answers[1], &answers[2]]));
}

#[test]
fn secret_objects_use_the_secrets_kek() {
    let a = DeviceIdentity::generate().unwrap();
    let created = new_vault(&a);
    let s = Secret::new(SecretKind::Password, SecretValue::new("test-only-value"));
    let id = s.id;
    let p = ObjectPayload::new(VaultObject::Secret(s));
    let body = created.unlocked.encrypt_object(id, 1, &p).unwrap();
    let (_, class) = created
        .unlocked
        .decrypt_object(id, 1, &body, Some(KekClass::Secrets))
        .unwrap();
    assert_eq!(class, KekClass::Secrets);
    // Re-encryption for a new revision uses a fresh DEK.
    let (re, class2) = created.unlocked.reencrypt_object(id, 1, &body, 2).unwrap();
    assert_eq!(class2, KekClass::Secrets);
    assert_ne!(re.wrapped_dek, body.wrapped_dek);
    assert!(created.unlocked.decrypt_object(id, 1, &re, None).is_err());
    assert_eq!(
        created.unlocked.decrypt_object(id, 2, &re, None).unwrap().0,
        p
    );
}

#[test]
fn nothing_secret_in_debug_output() {
    let a = DeviceIdentity::generate().unwrap();
    let created = new_vault(&a);
    let check = created.recovery_kit.start_check().unwrap();
    let dbg = format!(
        "{created:?} {:?} {a:?} {check:?} {:?}",
        created.recovery_kit, created.create_request
    );
    let phrase = created.recovery_kit.phrase().expose_phrase().to_owned();
    for word in phrase.split(' ') {
        assert!(!dbg.contains(&format!(" {word} ")), "leaked word {word}");
    }
    assert!(!dbg.contains(&phrase));
    assert!(!dbg.contains(created.recovery_kit.expose_qr_payload()));
    assert!(!dbg.contains(PASSPHRASE));
    assert!(!dbg.contains(&created.create_request.vault_access_key.to_base64()));
}
