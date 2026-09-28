//! Object encryption (ADR-0002 §Objects).
//!
//! ```text
//! plain   = u32_be(len(json)) || json(ObjectPayload) || zero padding
//!           (total rounded up to 256 B below 16 KiB, to 4 KiB from 16 KiB)
//! DEK     = 32 random bytes, fresh per revision
//! ct      = XChaCha20-Poly1305(DEK, nonce, plain, object_aad(vault, object, revision, 1))
//! wrapped = XChaCha20-Poly1305(KEK_class, nonce2, DEK, wrapped_dek_aad(vault, object, revision))
//! ```
//!
//! The object kind is not sent to the server; on decryption the KEK classes
//! are tried in a fixed order (optionally starting with a cached hint) and the
//! decrypted payload's kind MUST map to the class that unwrapped the DEK.

use crate::aead;
use crate::error::{CryptoError, Result};
use crate::keys::{Dek, KeyHierarchy};
use crate::rng::random_array;
use cc_models::{KekClass, ObjectPayload};
use cc_protocol::canonical::{object_aad, wrapped_dek_aad};
use cc_protocol::limits::{MAX_OBJECT_CIPHERTEXT_BYTES, NONCE_LEN, TAG_LEN, WRAPPED_DEK_LEN};
use cc_protocol::sync::{EncryptedBody, OBJECT_FORMAT_V1};
use cc_protocol::{Bytes, ObjectId, VaultId};
use std::io;
use zeroize::Zeroizing;

/// Padding bucket below [`PAD_LARGE_THRESHOLD`].
pub const PAD_SMALL: usize = 256;
/// Padding bucket from [`PAD_LARGE_THRESHOLD`] on.
pub const PAD_LARGE: usize = 4096;
/// Unpadded size (length prefix + JSON) from which the large bucket applies.
pub const PAD_LARGE_THRESHOLD: usize = 16 * 1024;
/// Largest padded plaintext whose ciphertext fits the protocol object limit.
pub const MAX_PADDED_PLAINTEXT: usize = MAX_OBJECT_CIPHERTEXT_BYTES - TAG_LEN;

/// Fixed trial order for KEK unwrapping (ADR-0002).
pub const KEK_TRIAL_ORDER: [KekClass; 5] = [
    KekClass::Inventory,
    KekClass::Secrets,
    KekClass::Snippets,
    KekClass::History,
    KekClass::Settings,
];

const LEN_PREFIX: usize = 4;

/// Padded plaintext size for an unpadded size (`4 + len(json)`).
pub const fn padded_len(unpadded: usize) -> usize {
    let bucket = if unpadded < PAD_LARGE_THRESHOLD {
        PAD_SMALL
    } else {
        PAD_LARGE
    };
    unpadded.div_ceil(bucket) * bucket
}

/// Encrypt `payload` as object `object_id` at `revision` (= base_revision + 1,
/// the revision the server will assign). A fresh DEK and nonces are used.
///
/// `vault_id` must equal `keys.vault_id()` and `payload`'s object id must
/// equal `object_id`.
pub fn encrypt_object(
    keys: &KeyHierarchy,
    vault_id: VaultId,
    object_id: ObjectId,
    revision: i64,
    payload: &ObjectPayload,
) -> Result<EncryptedBody> {
    check_context(keys, vault_id, revision)?;
    if payload.object.id() != object_id {
        return Err(CryptoError::ObjectIdMismatch);
    }
    let class = payload.object.kind().kek_class();
    let plain = serialize_padded(payload)?;
    let dek = Dek::generate()?;
    let nonce = random_array::<NONCE_LEN>()?;
    let wrap_nonce = random_array::<NONCE_LEN>()?;
    encrypt_padded_with(
        keys,
        object_id,
        revision,
        class,
        &plain,
        &dek,
        &nonce,
        &wrap_nonce,
    )
}

/// Decrypt object `object_id` at `revision`. Returns the payload and the KEK
/// class that unwrapped its DEK (cache it and pass it as `class_hint` next
/// time to skip trial unwrapping).
///
/// Fails with [`CryptoError::Decrypt`] on any tampering (ciphertext, nonces,
/// wrapped DEK, a replayed older revision, another object's body) and with
/// [`CryptoError::KekClassMismatch`] on class confusion.
pub fn decrypt_object(
    keys: &KeyHierarchy,
    vault_id: VaultId,
    object_id: ObjectId,
    revision: i64,
    body: &EncryptedBody,
    class_hint: Option<KekClass>,
) -> Result<(ObjectPayload, KekClass)> {
    check_context(keys, vault_id, revision)?;
    let (plain, class) = decrypt_padded(keys, object_id, revision, body, class_hint)?;
    let json = unpad(&plain)?;
    let payload: ObjectPayload =
        serde_json::from_slice(json).map_err(|_| CryptoError::Serialization)?;
    let required = payload.object.kind().kek_class();
    if required != class {
        return Err(CryptoError::KekClassMismatch {
            unwrapped_with: class,
            required,
        });
    }
    if payload.object.id() != object_id {
        return Err(CryptoError::ObjectIdMismatch);
    }
    Ok((payload, class))
}

fn check_context(keys: &KeyHierarchy, vault_id: VaultId, revision: i64) -> Result<()> {
    if keys.vault_id() != vault_id {
        return Err(CryptoError::VaultMismatch);
    }
    if revision < 1 {
        return Err(CryptoError::InvalidRevision(revision));
    }
    Ok(())
}

/// `io::Write` sink that only counts bytes (sizing pass, so the real buffer
/// is allocated once and never reallocated with plaintext inside).
struct CountingWriter(usize);

impl io::Write for CountingWriter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.0 += buf.len();
        Ok(buf.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

/// `u32_be(len) || json || zero padding` in a zeroizing buffer.
pub(crate) fn serialize_padded(payload: &ObjectPayload) -> Result<Zeroizing<Vec<u8>>> {
    let mut counter = CountingWriter(0);
    serde_json::to_writer(&mut counter, payload).map_err(|_| CryptoError::Serialization)?;
    let json_len = counter.0;
    let total = checked_padded_len(json_len)?;
    let mut buf = Zeroizing::new(Vec::with_capacity(total));
    buf.extend_from_slice(&len_prefix(json_len)?);
    serde_json::to_writer(&mut *buf, payload).map_err(|_| CryptoError::Serialization)?;
    if buf.len() != LEN_PREFIX + json_len {
        // Serialization must be deterministic between the two passes.
        return Err(CryptoError::Serialization);
    }
    buf.resize(total, 0);
    Ok(buf)
}

/// Pad already-serialized JSON (test vectors use exact JSON bytes so they do
/// not depend on model serialization details).
#[cfg(test)]
pub(crate) fn pad_serialized(json: &[u8]) -> Result<Zeroizing<Vec<u8>>> {
    let total = checked_padded_len(json.len())?;
    let mut buf = Zeroizing::new(Vec::with_capacity(total));
    buf.extend_from_slice(&len_prefix(json.len())?);
    buf.extend_from_slice(json);
    buf.resize(total, 0);
    Ok(buf)
}

fn checked_padded_len(json_len: usize) -> Result<usize> {
    let total = padded_len(LEN_PREFIX + json_len);
    if total > MAX_PADDED_PLAINTEXT {
        return Err(CryptoError::ObjectTooLarge {
            size: total,
            max: MAX_PADDED_PLAINTEXT,
        });
    }
    Ok(total)
}

fn len_prefix(json_len: usize) -> Result<[u8; LEN_PREFIX]> {
    u32::try_from(json_len)
        .map(u32::to_be_bytes)
        .map_err(|_| CryptoError::ObjectTooLarge {
            size: json_len,
            max: MAX_PADDED_PLAINTEXT,
        })
}

/// Strip the length prefix and verify the padding is all zeros.
pub(crate) fn unpad(plain: &[u8]) -> Result<&[u8]> {
    if plain.len() < LEN_PREFIX {
        return Err(CryptoError::MalformedPlaintext(
            "shorter than length prefix",
        ));
    }
    let (prefix, rest) = plain.split_at(LEN_PREFIX);
    let mut len_bytes = [0u8; LEN_PREFIX];
    len_bytes.copy_from_slice(prefix);
    let json_len = usize::try_from(u32::from_be_bytes(len_bytes))
        .map_err(|_| CryptoError::MalformedPlaintext("length prefix overflow"))?;
    if json_len > rest.len() {
        return Err(CryptoError::MalformedPlaintext(
            "length prefix exceeds plaintext",
        ));
    }
    let (json, padding) = rest.split_at(json_len);
    if padding.iter().any(|b| *b != 0) {
        return Err(CryptoError::MalformedPlaintext("non-zero padding"));
    }
    Ok(json)
}

/// Encrypt an already padded plaintext with explicit DEK and nonces.
#[allow(clippy::too_many_arguments)]
pub(crate) fn encrypt_padded_with(
    keys: &KeyHierarchy,
    object_id: ObjectId,
    revision: i64,
    class: KekClass,
    plain: &[u8],
    dek: &Dek,
    nonce: &[u8; NONCE_LEN],
    wrap_nonce: &[u8; NONCE_LEN],
) -> Result<EncryptedBody> {
    let vault_id = keys.vault_id();
    let aad = object_aad(vault_id, object_id, revision, OBJECT_FORMAT_V1);
    let ciphertext = aead::seal(dek.expose(), nonce, &aad, plain)?;
    let wrap_aad = wrapped_dek_aad(vault_id, object_id, revision);
    let wrapped = aead::seal(
        keys.kek(class).expose(),
        wrap_nonce,
        &wrap_aad,
        dek.expose(),
    )?;
    Ok(EncryptedBody {
        format: OBJECT_FORMAT_V1,
        ciphertext: Bytes::new(ciphertext),
        nonce: Bytes::new(nonce.to_vec()),
        wrapped_dek: Bytes::new(wrapped),
        wrapped_dek_nonce: Bytes::new(wrap_nonce.to_vec()),
    })
}

/// Unwrap the DEK by trial (hint first, then [`KEK_TRIAL_ORDER`]) and decrypt
/// the padded plaintext.
pub(crate) fn decrypt_padded(
    keys: &KeyHierarchy,
    object_id: ObjectId,
    revision: i64,
    body: &EncryptedBody,
    class_hint: Option<KekClass>,
) -> Result<(Zeroizing<Vec<u8>>, KekClass)> {
    if body.format != OBJECT_FORMAT_V1 {
        return Err(CryptoError::UnsupportedFormat(body.format));
    }
    let nonce = body
        .nonce
        .to_array::<NONCE_LEN>()
        .ok_or(CryptoError::MalformedBody("nonce must be 24 bytes"))?;
    let wrap_nonce =
        body.wrapped_dek_nonce
            .to_array::<NONCE_LEN>()
            .ok_or(CryptoError::MalformedBody(
                "wrapped DEK nonce must be 24 bytes",
            ))?;
    if body.wrapped_dek.len() != WRAPPED_DEK_LEN {
        return Err(CryptoError::MalformedBody("wrapped DEK must be 48 bytes"));
    }
    let ct_len = body.ciphertext.len();
    if !(TAG_LEN + LEN_PREFIX..=MAX_OBJECT_CIPHERTEXT_BYTES).contains(&ct_len) {
        return Err(CryptoError::MalformedBody("ciphertext length out of range"));
    }

    let vault_id = keys.vault_id();
    let wrap_aad = wrapped_dek_aad(vault_id, object_id, revision);
    let order = class_hint.into_iter().chain(
        KEK_TRIAL_ORDER
            .into_iter()
            .filter(|c| Some(*c) != class_hint),
    );
    let mut unwrapped = None;
    for class in order {
        match aead::open_key32(
            keys.kek(class).expose(),
            &wrap_nonce,
            &wrap_aad,
            body.wrapped_dek.as_slice(),
        ) {
            Ok(dek) => {
                unwrapped = Some((Dek::from_secret(dek), class));
                break;
            }
            Err(CryptoError::Decrypt) => continue,
            Err(e) => return Err(e),
        }
    }
    let (dek, class) = unwrapped.ok_or(CryptoError::Decrypt)?;
    let aad = object_aad(vault_id, object_id, revision, OBJECT_FORMAT_V1);
    let plain = aead::open(dek.expose(), &nonce, &aad, body.ciphertext.as_slice())?;
    Ok((plain, class))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn padding_buckets() {
        assert_eq!(padded_len(1), 256);
        assert_eq!(padded_len(256), 256);
        assert_eq!(padded_len(257), 512);
        assert_eq!(padded_len(16 * 1024 - 1), 16 * 1024);
        assert_eq!(padded_len(16 * 1024), 16 * 1024);
        assert_eq!(padded_len(16 * 1024 + 1), 20 * 1024);
        assert_eq!(padded_len(100_000), 102_400);
    }

    #[test]
    fn unpad_rejects_malformed() {
        assert!(unpad(&[0, 0]).is_err());
        assert!(unpad(&[0, 0, 0, 5, b'a']).is_err());
        assert!(unpad(&[0, 0, 0, 1, b'a', 0, 1]).is_err());
        assert_eq!(unpad(&[0, 0, 0, 1, b'a', 0, 0]).unwrap(), b"a");
    }

    #[test]
    fn serialized_padding_is_zero_and_bucketed() {
        let host = cc_models::host::Host::new("h", "10.0.0.1");
        let p = ObjectPayload::new(cc_models::VaultObject::Host(host));
        let plain = serialize_padded(&p).unwrap();
        assert_eq!(plain.len() % PAD_SMALL, 0);
        let json = unpad(&plain).unwrap();
        assert_eq!(json, serde_json::to_vec(&p).unwrap().as_slice());
        assert_eq!(&*pad_serialized(json).unwrap(), &*plain);
    }

    /// A key holder (or buggy client) wrapping a Host's DEK under the Secrets
    /// KEK must be rejected: otherwise code holding only some KEK classes
    /// could be tricked into treating data of one class as another.
    #[test]
    fn class_confusion_is_rejected() {
        use crate::keys::Vrk;
        let keys = KeyHierarchy::derive(&Vrk::generate().unwrap(), VaultId::new());
        let host = cc_models::host::Host::new("h", "10.0.0.1");
        let id = host.id;
        let p = ObjectPayload::new(cc_models::VaultObject::Host(host));
        let plain = serialize_padded(&p).unwrap();
        for wrong in [
            KekClass::Secrets,
            KekClass::Snippets,
            KekClass::History,
            KekClass::Settings,
        ] {
            let body = encrypt_padded_with(
                &keys,
                id,
                1,
                wrong,
                &plain,
                &Dek::generate().unwrap(),
                &[1; 24],
                &[2; 24],
            )
            .unwrap();
            for hint in [None, Some(wrong), Some(KekClass::Inventory)] {
                assert_eq!(
                    decrypt_object(&keys, keys.vault_id(), id, 1, &body, hint).err(),
                    Some(CryptoError::KekClassMismatch {
                        unwrapped_with: wrong,
                        required: KekClass::Inventory,
                    })
                );
            }
        }
    }

    #[test]
    fn malformed_plaintext_rejected_after_decrypt() {
        use crate::keys::Vrk;
        let keys = KeyHierarchy::derive(&Vrk::generate().unwrap(), VaultId::new());
        let id = ObjectId::new();
        let dek = Dek::generate().unwrap();
        let enc = |plain: &[u8]| {
            encrypt_padded_with(
                &keys,
                id,
                1,
                KekClass::Inventory,
                plain,
                &dek,
                &[3; 24],
                &[4; 24],
            )
            .unwrap()
        };
        let dec = |b: &EncryptedBody| decrypt_object(&keys, keys.vault_id(), id, 1, b, None);
        // Length prefix beyond the plaintext.
        assert!(matches!(
            dec(&enc(&[0, 0, 1, 0, b'{', b'}'])),
            Err(CryptoError::MalformedPlaintext(_))
        ));
        // Non-zero padding.
        assert!(matches!(
            dec(&enc(&[0, 0, 0, 2, b'{', b'}', 7])),
            Err(CryptoError::MalformedPlaintext(_))
        ));
        // Not an ObjectPayload.
        assert_eq!(
            dec(&enc(&pad_serialized(b"{}").unwrap())).err(),
            Some(CryptoError::Serialization)
        );
    }
}
