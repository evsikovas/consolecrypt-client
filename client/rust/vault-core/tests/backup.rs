//! Encrypted backup export / import (`.ccbackup`, ADR-0106).

mod common;

use cc_models::host::Host;
use cc_models::note::Note;
use cc_models::secret::{Secret, SecretKind, SecretValue};
use cc_models::{KekClass, ObjectPayload, VaultObject};
use cc_protocol::ObjectId;
use cc_vault_core::{
    create_vault, decode_backup, encode_backup, restore_backup_with_passphrase,
    restore_backup_with_recovery, BackupObject, CreatedVault, DeviceIdentity, VaultBackup,
    VaultError,
};
use common::*;

const PASSPHRASE: &str = "backup test passphrase";
const SECRET_MARKER: &str = "test-only-secret-marker";

struct Fixture {
    created: CreatedVault,
    backup: VaultBackup,
    ids: Vec<(ObjectId, KekClass)>,
}

fn fixture() -> Fixture {
    let identity = DeviceIdentity::generate().unwrap();
    let created = create_vault(&identity, &pass(PASSPHRASE), kdf(), None, now()).unwrap();
    let u = &created.unlocked;
    let mut objects = Vec::new();
    let mut ids = Vec::new();

    let h = Host::new("db", "10.0.0.5");
    let (hid, hp) = (h.id, ObjectPayload::new(VaultObject::Host(h)));
    objects.push(BackupObject {
        object_id: hid,
        revision: 3,
        deleted: false,
        body: Some(u.encrypt_object(hid, 3, &hp).unwrap()),
    });
    ids.push((hid, KekClass::Inventory));

    let s = Secret::new(SecretKind::Password, SecretValue::new(SECRET_MARKER));
    let (sid, sp) = (s.id, ObjectPayload::new(VaultObject::Secret(s)));
    objects.push(BackupObject {
        object_id: sid,
        revision: 1,
        deleted: false,
        body: Some(u.encrypt_object(sid, 1, &sp).unwrap()),
    });
    ids.push((sid, KekClass::Secrets));

    let t = now();
    let n = Note {
        id: ObjectId::new(),
        title: "runbook".into(),
        body: SECRET_MARKER.into(),
        tags: vec![],
        created_at: t,
        updated_at: t,
    };
    let (nid, np) = (n.id, ObjectPayload::new(VaultObject::Note(n)));
    objects.push(BackupObject {
        object_id: nid,
        revision: 2,
        deleted: false,
        body: Some(u.encrypt_object(nid, 2, &np).unwrap()),
    });
    ids.push((nid, KekClass::Snippets));

    // A tombstone.
    objects.push(BackupObject {
        object_id: ObjectId::new(),
        revision: 4,
        deleted: true,
        body: None,
    });

    let envs = created.local_envelopes();
    let backup = u
        .create_backup(&envs.password, &envs.recovery, objects, now(), "0.1.0-test")
        .unwrap();
    Fixture {
        created,
        backup,
        ids,
    }
}

#[test]
fn export_import_roundtrip_with_passphrase_and_recovery() {
    let f = fixture();
    let file = encode_backup(&f.backup).unwrap();

    // No plaintext and no device material in the file.
    let text = String::from_utf8(file.clone()).unwrap();
    assert!(!text.contains(SECRET_MARKER));
    assert!(!text.contains("10.0.0.5"));
    assert!(!text.contains("\"device\""));
    assert!(text.contains("consolecrypt-backup/v1"));

    let decoded = decode_backup(&file).unwrap();
    assert_eq!(decoded, f.backup);

    let restored = restore_backup_with_passphrase(decoded.clone(), &pass(PASSPHRASE)).unwrap();
    assert_eq!(restored.unlocked.vault_id(), f.created.unlocked.vault_id());
    assert_eq!(
        restored.unlocked.vault_access_key(),
        f.created.unlocked.vault_access_key()
    );
    let mut classes = restored.classes.clone();
    classes.sort_by_key(|(id, _)| *id);
    let mut expected = f.ids.clone();
    expected.sort_by_key(|(id, _)| *id);
    assert_eq!(classes, expected);

    let words = pass(f.created.recovery_kit.phrase().expose_phrase());
    let restored = restore_backup_with_recovery(decoded.clone(), &words).unwrap();
    assert_eq!(restored.backup.objects.len(), 4);
    let qr = pass(f.created.recovery_kit.expose_qr_payload());
    restore_backup_with_recovery(decoded, &qr).unwrap();
}

#[test]
fn wrong_passphrase_or_recovery_key() {
    let f = fixture();
    assert_eq!(
        restore_backup_with_passphrase(f.backup.clone(), &pass("not the passphrase")).err(),
        Some(VaultError::WrongPassphrase)
    );
    let other = fixture();
    assert_eq!(
        restore_backup_with_recovery(
            f.backup.clone(),
            &pass(other.created.recovery_kit.phrase().expose_phrase())
        )
        .err(),
        Some(VaultError::WrongRecoveryKey)
    );
    assert_eq!(
        restore_backup_with_recovery(
            f.backup.clone(),
            &pass(other.created.recovery_kit.expose_qr_payload())
        )
        .err(),
        Some(VaultError::RecoveryKeyForOtherVault)
    );
}

#[test]
fn tampered_backups_are_rejected() {
    let f = fixture();
    let restore = |b: VaultBackup| restore_backup_with_passphrase(b, &pass(PASSPHRASE));

    // Flipped ciphertext byte.
    let mut b = f.backup.clone();
    b.objects[0].body.as_mut().unwrap().ciphertext.0[5] ^= 1;
    assert!(matches!(
        restore(b),
        Err(VaultError::Crypto(cc_crypto_core::CryptoError::Backup(_)))
    ));

    // Dropped object.
    let mut b = f.backup.clone();
    b.objects.remove(1);
    assert!(matches!(
        restore(b),
        Err(VaultError::Crypto(cc_crypto_core::CryptoError::Backup(_)))
    ));

    // Rolled-back revision.
    let mut b = f.backup.clone();
    b.objects[0].revision = 2;
    assert!(restore(b).is_err());

    // Garbage / wrong format never reaches the unlock step.
    assert_eq!(
        decode_backup(b"not json").err(),
        Some(VaultError::BackupEncoding)
    );
    let mut b = f.backup.clone();
    b.format = "consolecrypt-backup/v9".into();
    let bytes = encode_backup(&b).unwrap();
    assert!(matches!(decode_backup(&bytes), Err(VaultError::Crypto(_))));
}

#[test]
fn backup_rejects_foreign_envelopes() {
    let f = fixture();
    let other = fixture();
    let o = other.created.local_envelopes();
    // Envelopes swapped.
    assert!(f
        .created
        .unlocked
        .create_backup(&o.recovery, &o.password, vec![], now(), "t")
        .is_err());
}
