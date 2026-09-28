//! Object encryption through the public API.

use cc_crypto_core::{
    decrypt_object, encrypt_object, padded_len, CryptoError, KeyHierarchy, Vrk,
    MAX_PADDED_PLAINTEXT, PAD_LARGE, PAD_SMALL,
};
use cc_models::host::Host;
use cc_models::note::Note;
use cc_models::secret::{Secret, SecretKind, SecretValue};
use cc_models::{KekClass, ObjectPayload, VaultObject};
use cc_protocol::limits::TAG_LEN;
use cc_protocol::sync::EncryptedBody;
use cc_protocol::{ObjectId, VaultId};

fn keys() -> KeyHierarchy {
    KeyHierarchy::derive(&Vrk::generate().unwrap(), VaultId::new())
}

fn host_payload() -> (ObjectId, ObjectPayload) {
    let h = Host::new("prod-db", "10.10.10.20");
    (h.id, ObjectPayload::new(VaultObject::Host(h)))
}

fn note_payload(body: String) -> (ObjectId, ObjectPayload) {
    let now = chrono::Utc::now();
    let n = Note {
        id: ObjectId::new(),
        title: "t".into(),
        body,
        tags: vec![],
        created_at: now,
        updated_at: now,
    };
    (n.id, ObjectPayload::new(VaultObject::Note(n)))
}

fn secret_payload() -> (ObjectId, ObjectPayload) {
    let s = Secret::new(SecretKind::Password, SecretValue::new("test-only-value"));
    (s.id, ObjectPayload::new(VaultObject::Secret(s)))
}

#[test]
fn roundtrip_reports_class_for_each_kind() {
    let k = keys();
    let v = k.vault_id();
    for ((id, p), class) in [
        (host_payload(), KekClass::Inventory),
        (secret_payload(), KekClass::Secrets),
        (note_payload("hello".into()), KekClass::Snippets),
    ] {
        let body = encrypt_object(&k, v, id, 1, &p).unwrap();
        let (back, got) = decrypt_object(&k, v, id, 1, &body, None).unwrap();
        assert_eq!(back, p);
        assert_eq!(got, class);
        // With the correct hint, and with a wrong hint (falls back to trial).
        assert_eq!(
            decrypt_object(&k, v, id, 1, &body, Some(class)).unwrap().1,
            class
        );
        let wrong_hint = if class == KekClass::Settings {
            KekClass::Inventory
        } else {
            KekClass::Settings
        };
        assert_eq!(
            decrypt_object(&k, v, id, 1, &body, Some(wrong_hint))
                .unwrap()
                .1,
            class
        );
    }
}

#[test]
fn fresh_dek_and_nonces_per_encryption() {
    let k = keys();
    let (id, p) = host_payload();
    let a = encrypt_object(&k, k.vault_id(), id, 2, &p).unwrap();
    let b = encrypt_object(&k, k.vault_id(), id, 2, &p).unwrap();
    assert_ne!(a.nonce, b.nonce);
    assert_ne!(a.wrapped_dek_nonce, b.wrapped_dek_nonce);
    assert_ne!(a.wrapped_dek, b.wrapped_dek);
    assert_ne!(a.ciphertext, b.ciphertext);
}

#[test]
fn revision_is_bound() {
    let k = keys();
    let v = k.vault_id();
    let (id, p) = host_payload();
    let body = encrypt_object(&k, v, id, 3, &p).unwrap();
    // A hostile server replaying revision 3's body as revision 4 (or 2).
    for rev in [2, 4] {
        assert_eq!(
            decrypt_object(&k, v, id, rev, &body, None).err(),
            Some(CryptoError::Decrypt)
        );
    }
    assert_eq!(
        encrypt_object(&k, v, id, 0, &p).err(),
        Some(CryptoError::InvalidRevision(0))
    );
    assert_eq!(
        decrypt_object(&k, v, id, -1, &body, None).err(),
        Some(CryptoError::InvalidRevision(-1))
    );
}

#[test]
fn object_and_vault_are_bound() {
    let k = keys();
    let v = k.vault_id();
    let (id, p) = host_payload();
    let body = encrypt_object(&k, v, id, 1, &p).unwrap();
    // Served under another object id.
    assert_eq!(
        decrypt_object(&k, v, ObjectId::new(), 1, &body, None).err(),
        Some(CryptoError::Decrypt)
    );
    // Keys of another vault (same VRK would still differ via salt).
    let other = keys();
    assert_eq!(
        decrypt_object(&other, other.vault_id(), id, 1, &body, None).err(),
        Some(CryptoError::Decrypt)
    );
    // vault_id argument must match the key hierarchy.
    assert_eq!(
        decrypt_object(&k, VaultId::new(), id, 1, &body, None).err(),
        Some(CryptoError::VaultMismatch)
    );
    assert_eq!(
        encrypt_object(&k, VaultId::new(), id, 1, &p).err(),
        Some(CryptoError::VaultMismatch)
    );
    // Payload id must equal the storage id.
    assert_eq!(
        encrypt_object(&k, v, ObjectId::new(), 1, &p).err(),
        Some(CryptoError::ObjectIdMismatch)
    );
}

#[test]
fn tampering_is_detected() {
    let k = keys();
    let v = k.vault_id();
    let (id, p) = host_payload();
    let body = encrypt_object(&k, v, id, 1, &p).unwrap();
    type Mutation = Box<dyn Fn(&mut EncryptedBody)>;
    let mutations: Vec<Mutation> = vec![
        Box::new(|b| b.ciphertext.0[0] ^= 1),
        Box::new(|b| {
            let n = b.ciphertext.0.len();
            b.ciphertext.0[n - 1] ^= 1
        }),
        Box::new(|b| b.nonce.0[23] ^= 1),
        Box::new(|b| b.wrapped_dek.0[0] ^= 1),
        Box::new(|b| b.wrapped_dek_nonce.0[0] ^= 1),
    ];
    for m in mutations {
        let mut b = body.clone();
        m(&mut b);
        assert_eq!(
            decrypt_object(&k, v, id, 1, &b, None).err(),
            Some(CryptoError::Decrypt)
        );
    }
    // Swapping in another object's wrapped DEK.
    let (id2, p2) = host_payload();
    let other = encrypt_object(&k, v, id2, 1, &p2).unwrap();
    let mut b = body.clone();
    b.wrapped_dek = other.wrapped_dek;
    b.wrapped_dek_nonce = other.wrapped_dek_nonce;
    assert_eq!(
        decrypt_object(&k, v, id, 1, &b, None).err(),
        Some(CryptoError::Decrypt)
    );
}

#[test]
fn malformed_bodies_rejected() {
    let k = keys();
    let v = k.vault_id();
    let (id, p) = host_payload();
    let body = encrypt_object(&k, v, id, 1, &p).unwrap();
    let mut b = body.clone();
    b.format = 2;
    assert_eq!(
        decrypt_object(&k, v, id, 1, &b, None).err(),
        Some(CryptoError::UnsupportedFormat(2))
    );
    let mut b = body.clone();
    b.nonce.0.truncate(12);
    assert!(matches!(
        decrypt_object(&k, v, id, 1, &b, None),
        Err(CryptoError::MalformedBody(_))
    ));
    let mut b = body.clone();
    b.wrapped_dek.0.push(0);
    assert!(matches!(
        decrypt_object(&k, v, id, 1, &b, None),
        Err(CryptoError::MalformedBody(_))
    ));
    let mut b = body.clone();
    b.ciphertext.0.truncate(10);
    assert!(matches!(
        decrypt_object(&k, v, id, 1, &b, None),
        Err(CryptoError::MalformedBody(_))
    ));
}

#[test]
fn padding_sizes() {
    let k = keys();
    let v = k.vault_id();
    // Small: multiple of 256 (+ tag).
    let (id, p) = host_payload();
    let body = encrypt_object(&k, v, id, 1, &p).unwrap();
    let plain = body.ciphertext.len() - TAG_LEN;
    assert_eq!(plain % PAD_SMALL, 0);
    assert_eq!(plain, padded_len(4 + serde_json::to_vec(&p).unwrap().len()));

    // Two payloads of different small sizes in one bucket are indistinguishable.
    let (i1, p1) = note_payload("a".into());
    let (i2, p2) = note_payload("a".repeat(10));
    assert_eq!(
        encrypt_object(&k, v, i1, 1, &p1).unwrap().ciphertext.len(),
        encrypt_object(&k, v, i2, 1, &p2).unwrap().ciphertext.len()
    );

    // Large: multiple of 4096.
    let (id, p) = note_payload("x".repeat(20_000));
    let body = encrypt_object(&k, v, id, 1, &p).unwrap();
    let plain = body.ciphertext.len() - TAG_LEN;
    assert_eq!(plain % PAD_LARGE, 0);
    assert_eq!(plain, 20 * 1024);
    assert_eq!(decrypt_object(&k, v, id, 1, &body, None).unwrap().0, p);

    // Too large for one object.
    let (id, p) = note_payload("x".repeat(MAX_PADDED_PLAINTEXT));
    assert!(matches!(
        encrypt_object(&k, v, id, 1, &p),
        Err(CryptoError::ObjectTooLarge { .. })
    ));
}

#[test]
fn body_debug_does_not_leak() {
    let k = keys();
    let (id, p) = secret_payload();
    let body = encrypt_object(&k, k.vault_id(), id, 1, &p).unwrap();
    let dbg = format!("{body:?} {k:?}");
    assert!(!dbg.contains("test-only-value"));
    assert!(dbg.contains("bytes>"));
}
