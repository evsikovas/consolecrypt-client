//! XChaCha20-Poly1305 with explicit 24-byte nonces (ADR-0002). Ciphertexts
//! are `ct || tag(16)`. Decrypted plaintext only ever lands in zeroizing
//! buffers.

use crate::error::{CryptoError, Result};
use crate::keys::Secret32;
use cc_protocol::limits::{KEY_LEN, NONCE_LEN, TAG_LEN};
use chacha20poly1305::aead::{AeadInOut, KeyInit};
use chacha20poly1305::{Tag, XChaCha20Poly1305, XNonce};
use secrecy::ExposeSecretMut;
use zeroize::Zeroizing;

fn cipher(key: &[u8; KEY_LEN]) -> XChaCha20Poly1305 {
    // Infallible: the slice is exactly KEY_LEN bytes.
    XChaCha20Poly1305::new_from_slice(key).expect("key is 32 bytes")
}

/// Encrypt `plaintext` → `ciphertext || tag`.
pub(crate) fn seal(
    key: &[u8; KEY_LEN],
    nonce: &[u8; NONCE_LEN],
    aad: &[u8],
    plaintext: &[u8],
) -> Result<Vec<u8>> {
    let mut buf = Vec::with_capacity(plaintext.len() + TAG_LEN);
    buf.extend_from_slice(plaintext);
    let tag = cipher(key)
        .encrypt_inout_detached(&XNonce::from(*nonce), aad, buf.as_mut_slice().into())
        .map_err(|_| CryptoError::Decrypt)?;
    buf.extend_from_slice(&tag);
    Ok(buf)
}

/// Decrypt `ciphertext || tag` into a zeroizing buffer.
pub(crate) fn open(
    key: &[u8; KEY_LEN],
    nonce: &[u8; NONCE_LEN],
    aad: &[u8],
    ciphertext: &[u8],
) -> Result<Zeroizing<Vec<u8>>> {
    let body_len = ciphertext
        .len()
        .checked_sub(TAG_LEN)
        .ok_or(CryptoError::Decrypt)?;
    let (body, tag) = ciphertext.split_at(body_len);
    let tag = Tag::try_from(tag).map_err(|_| CryptoError::Decrypt)?;
    let mut buf = Zeroizing::new(body.to_vec());
    cipher(key)
        .decrypt_inout_detached(&XNonce::from(*nonce), aad, buf.as_mut_slice().into(), &tag)
        .map_err(|_| CryptoError::Decrypt)?;
    Ok(buf)
}

/// Decrypt a wrapped 32-byte key (VRK in an envelope, DEK under a KEK)
/// directly into a secret box.
pub(crate) fn open_key32(
    key: &[u8; KEY_LEN],
    nonce: &[u8; NONCE_LEN],
    aad: &[u8],
    ciphertext: &[u8],
) -> Result<Secret32> {
    if ciphertext.len() != KEY_LEN + TAG_LEN {
        return Err(CryptoError::Decrypt);
    }
    let plain = open(key, nonce, aad, ciphertext)?;
    let mut out = Secret32::init_with_mut(|_| {});
    out.expose_secret_mut().copy_from_slice(&plain);
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_and_tamper() {
        let key = [7u8; 32];
        let nonce = [9u8; 24];
        let ct = seal(&key, &nonce, b"aad", b"hello").unwrap();
        assert_eq!(ct.len(), 5 + TAG_LEN);
        assert_eq!(&**open(&key, &nonce, b"aad", &ct).unwrap(), b"hello");
        assert!(open(&key, &nonce, b"aaD", &ct).is_err());
        assert!(open(&[8u8; 32], &nonce, b"aad", &ct).is_err());
        let mut n2 = nonce;
        n2[0] ^= 1;
        assert!(open(&key, &n2, b"aad", &ct).is_err());
        let mut bad = ct.clone();
        bad[0] ^= 1;
        assert!(open(&key, &nonce, b"aad", &bad).is_err());
        assert!(open(&key, &nonce, b"aad", &ct[..10]).is_err());
    }
}
