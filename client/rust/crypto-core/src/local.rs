//! Client-local key wrapping (S5, device unlock under OS protection).
//!
//! These formats never leave the installation and are not part of the
//! protocol, so their labels live here rather than in
//! `cc_protocol::canonical`:
//!
//! * **storage wrap** — a local secret (the profile's SQLCipher key) sealed
//!   under `HKDF(VRK, vault_id, "…/local/storage-wrap")`: the local database
//!   opens only after the vault is unlocked (passphrase, Recovery Key or
//!   device unlock);
//! * **unlock envelope** — the VRK sealed under
//!   `HKDF(unlock_key, vault_id, "…/local/device-unlock")`, where the
//!   [`UnlockKey`] is released only by the operating system after its own
//!   user-presence check (Secure Enclave, Android Keystore, Windows Hello).
//!
//! Both are XChaCha20-Poly1305 with a fresh nonce and the associated data
//! `"consolecrypt/v1/aad/local\0" || purpose || vault_id(16) ||
//! u32be(len(context)) || context`, so a blob cannot be replayed for another
//! vault, purpose or context (e.g. another profile).

use crate::aead;
use crate::error::{CryptoError, Result};
use crate::kdf::hkdf_sha256_32;
#[cfg(test)]
use crate::keys::secret32_from;
use crate::keys::{random_secret32, Secret32, Vrk};
use crate::rng::random_array;
use cc_protocol::limits::{KEY_LEN, NONCE_LEN, TAG_LEN};
use cc_protocol::{Bytes, VaultId};
use secrecy::ExposeSecret;
use serde::{Deserialize, Serialize};
use std::fmt;
use zeroize::Zeroizing;

/// HKDF label of the storage-wrap key derived from the VRK.
pub const LOCAL_STORAGE_WRAP_LABEL: &[u8] = b"consolecrypt/v1/local/storage-wrap";
/// HKDF label of the unlock-envelope key derived from an [`UnlockKey`].
pub const LOCAL_DEVICE_UNLOCK_LABEL: &[u8] = b"consolecrypt/v1/local/device-unlock";
/// HKDF label turning OS-released key material into an [`UnlockKey`].
pub const LOCAL_UNLOCK_MATERIAL_LABEL: &[u8] = b"consolecrypt/v1/local/unlock-material";
/// Minimum OS key material length (bytes).
pub const MIN_UNLOCK_MATERIAL_LEN: usize = 32;
/// HKDF label prefix of [`derive_local_subkey`].
pub const LOCAL_SUBKEY_LABEL: &[u8] = b"consolecrypt/v1/local/subkey/";
/// Associated-data prefix of every local blob.
pub const AAD_LOCAL: &[u8] = b"consolecrypt/v1/aad/local\0";

/// Maximum context length (profile ids and short names).
pub const MAX_LOCAL_CONTEXT_LEN: usize = 256;
/// Maximum size of a sealed local secret's plaintext.
pub const MAX_LOCAL_SECRET_LEN: usize = 1024;

const PURPOSE_STORAGE: u8 = 1;
const PURPOSE_UNLOCK: u8 = 2;

/// A sealed local blob: `nonce` (24 bytes) and `ciphertext || tag`.
/// Ciphertext only — safe to keep in a plain file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LocalSealed {
    pub nonce: Bytes,
    pub ciphertext: Bytes,
}

/// The device unlock key: `HKDF(material)` of what the operating system
/// released after its user-presence check (a Secure Enclave ECDH secret, a
/// Windows Hello signature, a Keystore-decrypted random key, …).
/// Zeroized on drop; redacted `Debug`.
pub struct UnlockKey(Secret32);

impl UnlockKey {
    /// A fresh random key (OS CSPRNG).
    pub fn generate() -> Result<Self> {
        random_secret32().map(Self)
    }

    /// Derive the key from OS-released material (at least
    /// [`MIN_UNLOCK_MATERIAL_LEN`] bytes). The caller zeroizes its copy.
    pub fn from_material(material: &[u8]) -> Result<Self> {
        if material.len() < MIN_UNLOCK_MATERIAL_LEN {
            return Err(CryptoError::Decrypt);
        }
        Ok(Self(hkdf_sha256_32(
            material,
            &[],
            &[LOCAL_UNLOCK_MATERIAL_LABEL],
        )))
    }

    #[cfg(test)]
    fn same_as(&self) -> Self {
        Self(secret32_from(self.0.expose_secret()))
    }
}

impl fmt::Debug for UnlockKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("UnlockKey(<redacted>)")
    }
}

fn local_aad(purpose: u8, vault_id: VaultId, context: &[u8]) -> Result<Vec<u8>> {
    if context.len() > MAX_LOCAL_CONTEXT_LEN {
        return Err(CryptoError::Decrypt);
    }
    let mut aad = Vec::with_capacity(AAD_LOCAL.len() + 1 + 16 + 4 + context.len());
    aad.extend_from_slice(AAD_LOCAL);
    aad.push(purpose);
    aad.extend_from_slice(vault_id.as_bytes());
    // Bounded by MAX_LOCAL_CONTEXT_LEN above.
    aad.extend_from_slice(&(context.len() as u32).to_be_bytes());
    aad.extend_from_slice(context);
    Ok(aad)
}

fn nonce_of(sealed: &LocalSealed) -> Result<[u8; NONCE_LEN]> {
    sealed
        .nonce
        .0
        .as_slice()
        .try_into()
        .map_err(|_| CryptoError::Decrypt)
}

fn storage_key(vrk: &Vrk, vault_id: VaultId) -> Secret32 {
    hkdf_sha256_32(
        vrk.expose(),
        vault_id.as_bytes(),
        &[LOCAL_STORAGE_WRAP_LABEL],
    )
}

fn unlock_wrap_key(key: &UnlockKey, vault_id: VaultId) -> Secret32 {
    hkdf_sha256_32(
        key.0.expose_secret(),
        vault_id.as_bytes(),
        &[LOCAL_DEVICE_UNLOCK_LABEL],
    )
}

/// Seal a local secret (≤ [`MAX_LOCAL_SECRET_LEN`] bytes) under the VRK's
/// storage-wrap key, bound to `vault_id` and `context`.
pub fn seal_local_secret(
    vrk: &Vrk,
    vault_id: VaultId,
    context: &[u8],
    secret: &[u8],
) -> Result<LocalSealed> {
    if secret.len() > MAX_LOCAL_SECRET_LEN {
        return Err(CryptoError::Decrypt);
    }
    let aad = local_aad(PURPOSE_STORAGE, vault_id, context)?;
    let nonce = random_array::<NONCE_LEN>()?;
    let key = storage_key(vrk, vault_id);
    let ciphertext = aead::seal(key.expose_secret(), &nonce, &aad, secret)?;
    Ok(LocalSealed {
        nonce: Bytes::new(nonce.to_vec()),
        ciphertext: Bytes::new(ciphertext),
    })
}

/// Open a blob of [`seal_local_secret`]. Wrong VRK, vault, context or
/// tampering → [`CryptoError::Decrypt`].
pub fn open_local_secret(
    vrk: &Vrk,
    vault_id: VaultId,
    context: &[u8],
    sealed: &LocalSealed,
) -> Result<Zeroizing<Vec<u8>>> {
    let aad = local_aad(PURPOSE_STORAGE, vault_id, context)?;
    let key = storage_key(vrk, vault_id);
    aead::open(
        key.expose_secret(),
        &nonce_of(sealed)?,
        &aad,
        &sealed.ciphertext.0,
    )
}

/// Seal the VRK for device unlock under `key` (the OS-protected unlock key).
pub fn seal_unlock_envelope(
    vrk: &Vrk,
    vault_id: VaultId,
    key: &UnlockKey,
    context: &[u8],
) -> Result<LocalSealed> {
    let aad = local_aad(PURPOSE_UNLOCK, vault_id, context)?;
    let nonce = random_array::<NONCE_LEN>()?;
    let wrap = unlock_wrap_key(key, vault_id);
    let ciphertext = aead::seal(wrap.expose_secret(), &nonce, &aad, vrk.expose())?;
    Ok(LocalSealed {
        nonce: Bytes::new(nonce.to_vec()),
        ciphertext: Bytes::new(ciphertext),
    })
}

/// Open an unlock envelope with the key the OS released.
pub fn open_unlock_envelope(
    sealed: &LocalSealed,
    vault_id: VaultId,
    key: &UnlockKey,
    context: &[u8],
) -> Result<Vrk> {
    if sealed.ciphertext.0.len() != KEY_LEN + TAG_LEN {
        return Err(CryptoError::Decrypt);
    }
    let aad = local_aad(PURPOSE_UNLOCK, vault_id, context)?;
    let wrap = unlock_wrap_key(key, vault_id);
    let vrk = aead::open_key32(
        wrap.expose_secret(),
        &nonce_of(sealed)?,
        &aad,
        &sealed.ciphertext.0,
    )?;
    Ok(Vrk::from_secret(vrk))
}

/// `HKDF-SHA256(ikm = secret, salt = "", info = LOCAL_SUBKEY_LABEL || name)`:
/// an independent 32-byte key for another local file (e.g. the search index
/// key from the database key).
pub fn derive_local_subkey(secret: &[u8; KEY_LEN], name: &[u8]) -> Zeroizing<[u8; KEY_LEN]> {
    let k = hkdf_sha256_32(secret, &[], &[LOCAL_SUBKEY_LABEL, name]);
    Zeroizing::new(*k.expose_secret())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vid() -> VaultId {
        VaultId::new()
    }

    #[test]
    fn storage_wrap_roundtrip_and_binding() {
        let vrk = Vrk::generate().unwrap();
        let v = vid();
        let sealed = seal_local_secret(&vrk, v, b"db:p1", &[7u8; 32]).unwrap();
        assert_eq!(
            open_local_secret(&vrk, v, b"db:p1", &sealed)
                .unwrap()
                .as_slice(),
            &[7u8; 32]
        );
        let other = Vrk::generate().unwrap();
        assert_eq!(
            open_local_secret(&other, v, b"db:p1", &sealed).err(),
            Some(CryptoError::Decrypt)
        );
        assert!(open_local_secret(&vrk, vid(), b"db:p1", &sealed).is_err());
        assert!(open_local_secret(&vrk, v, b"db:p2", &sealed).is_err());
        let mut tampered = sealed.clone();
        tampered.ciphertext.0[0] ^= 1;
        assert!(open_local_secret(&vrk, v, b"db:p1", &tampered).is_err());
        let mut short_nonce = sealed;
        short_nonce.nonce.0.pop();
        assert!(open_local_secret(&vrk, v, b"db:p1", &short_nonce).is_err());
    }

    #[test]
    fn unlock_envelope_needs_the_unlock_key() {
        let vrk = Vrk::generate().unwrap();
        let v = vid();
        let key = UnlockKey::generate().unwrap();
        let sealed = seal_unlock_envelope(&vrk, v, &key, b"p1").unwrap();
        let back = open_unlock_envelope(&sealed, v, &key, b"p1").unwrap();
        assert_eq!(back.expose(), vrk.expose());
        assert!(open_unlock_envelope(&sealed, v, &key.same_as(), b"p1").is_ok());
        let wrong = UnlockKey::generate().unwrap();
        assert!(open_unlock_envelope(&sealed, v, &wrong, b"p1").is_err());
        assert!(open_unlock_envelope(&sealed, v, &key, b"p2").is_err());
        assert!(open_unlock_envelope(&sealed, vid(), &key, b"p1").is_err());
        // A storage-wrap blob is not an unlock envelope (purpose separation).
        let storage = seal_local_secret(&vrk, v, b"p1", &[1u8; 32]).unwrap();
        assert!(open_unlock_envelope(&storage, v, &key, b"p1").is_err());
        assert!(UnlockKey::from_material(&[0u8; 31]).is_err());
        let m1 = UnlockKey::from_material(&[5u8; 256]).unwrap();
        let m2 = UnlockKey::from_material(&[5u8; 256]).unwrap();
        let m3 = UnlockKey::from_material(&[6u8; 256]).unwrap();
        let env = seal_unlock_envelope(&vrk, v, &m1, b"p1").unwrap();
        assert!(open_unlock_envelope(&env, v, &m2, b"p1").is_ok());
        assert!(open_unlock_envelope(&env, v, &m3, b"p1").is_err());
    }

    #[test]
    fn subkeys_are_independent_and_stable() {
        let a = derive_local_subkey(&[1u8; 32], b"search-index");
        let b = derive_local_subkey(&[1u8; 32], b"search-index");
        let c = derive_local_subkey(&[1u8; 32], b"other");
        assert_eq!(*a, *b);
        assert_ne!(*a, *c);
        assert_ne!(*a, [1u8; 32]);
    }

    #[test]
    fn limits_and_debug() {
        let vrk = Vrk::generate().unwrap();
        assert!(seal_local_secret(&vrk, vid(), &[0; 257], b"x").is_err());
        assert!(seal_local_secret(&vrk, vid(), b"c", &[0; 1025]).is_err());
        let key = UnlockKey::generate().unwrap();
        assert_eq!(format!("{key:?}"), "UnlockKey(<redacted>)");
    }
}
