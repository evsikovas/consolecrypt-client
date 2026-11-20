//! Client-only encrypted FILE container, not a VaultObject or sync protocol.
//! Existing Argon2id/NFC + HKDF + XChaCha20Poly1305 primitives only. A separate
//! explicit package passphrase is never a ConsoleCrypt/Ansible Vault lookup.
use crate::{
    aead,
    kdf::{derive_password_key, hkdf_sha256_32, Argon2Params},
    rng::random_array,
    CryptoError, Result, SecretString,
};
use secrecy::ExposeSecret;
use zeroize::Zeroizing;

const MAGIC: &[u8] = b"consolecrypt/project-package/v1\0";
const DOMAIN: &[u8] = b"consolecrypt/client/project-package/key/v1";
const SALT: usize = 16;
const NONCE: usize = 24;
const TAG: usize = 16;
const HEADER: usize = MAGIC.len() + SALT + NONCE + 4;
const MAX_PASSPHRASE_BYTES: usize = 4096;
///16MiB source bytes +512KiB bounded manifest + u32 manifest length.
pub const MAX_PORTABLE_PROJECT_PLAIN: usize = 16 * 1024 * 1024 + 512 * 1024 + 4;
pub const MAX_PORTABLE_PROJECT_BYTES: usize = HEADER + MAX_PORTABLE_PROJECT_PLAIN + TAG;
fn v1_params() -> Result<Argon2Params> {
    // Freeze FILE v1 costs, not a moving DEFAULT. A future default increase
    // must not silently make old v1 files decrypt under different parameters.
    Argon2Params::new(64 * 1024, 3, 1)
}

/// Native caller admits at most two operations before invoking KDF. The v1
/// header cannot select costs: fixed existing DEFAULT64MiB/t3/p1 only. A future
/// choice needs a different version and explicit UX/compatibility review.
pub fn seal_portable_project(passphrase: &SecretString, plaintext: &[u8]) -> Result<Vec<u8>> {
    if plaintext.len() > MAX_PORTABLE_PROJECT_PLAIN
        || passphrase.expose_secret().len() > MAX_PASSPHRASE_BYTES
    {
        return Err(CryptoError::Decrypt);
    }
    let salt = random_array::<SALT>()?;
    let nonce = random_array::<NONCE>()?;
    let mut header = Vec::with_capacity(HEADER);
    header.extend_from_slice(MAGIC);
    header.extend_from_slice(&salt);
    header.extend_from_slice(&nonce);
    header.extend_from_slice(&(plaintext.len() as u32).to_be_bytes());
    let params = v1_params()?;
    if !params.meets_new_minimum() {
        return Err(CryptoError::KdfParams(
            "portable package version requires newer parameters",
        ));
    }
    let password_key = derive_password_key(passphrase, &salt, params)?;
    let key = hkdf_sha256_32(password_key.expose_secret(), &salt, &[DOMAIN]);
    let ciphertext = aead::seal(key.expose_secret(), &nonce, &header, plaintext)?;
    header.extend_from_slice(&ciphertext);
    Ok(header)
}

/// Strict size/version before KDF, complete AEAD authentication before any
/// plaintext parser or destination writer is called. No compression/expansion.
pub fn open_portable_project(
    passphrase: &SecretString,
    package: &[u8],
) -> Result<Zeroizing<Vec<u8>>> {
    if package.len() < HEADER + TAG
        || package.len() > MAX_PORTABLE_PROJECT_BYTES
        || !package.starts_with(MAGIC)
        || passphrase.expose_secret().len() > MAX_PASSPHRASE_BYTES
    {
        return Err(CryptoError::Decrypt);
    }
    let salt: &[u8; SALT] = package[MAGIC.len()..MAGIC.len() + SALT]
        .try_into()
        .map_err(|_| CryptoError::Decrypt)?;
    let nonce: &[u8; NONCE] = package[MAGIC.len() + SALT..HEADER - 4]
        .try_into()
        .map_err(|_| CryptoError::Decrypt)?;
    let length = u32::from_be_bytes(
        package[HEADER - 4..HEADER]
            .try_into()
            .map_err(|_| CryptoError::Decrypt)?,
    ) as usize;
    if length > MAX_PORTABLE_PROJECT_PLAIN || package.len() != HEADER + length + TAG {
        return Err(CryptoError::Decrypt);
    }
    let password_key = derive_password_key(passphrase, salt, v1_params()?)?;
    let key = hkdf_sha256_32(password_key.expose_secret(), salt, &[DOMAIN]);
    aead::open(
        key.expose_secret(),
        nonce,
        &package[..HEADER],
        &package[HEADER..],
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn actual_roundtrip_wrong_password_tamper_and_fixed_header() {
        let pass = SecretString::from("owned package passphrase distinct from vault");
        let plain = b"synthetic source marker not a credential";
        let sealed = seal_portable_project(&pass, plain).unwrap();
        assert!(!sealed.windows(plain.len()).any(|w| w == plain));
        assert_eq!(&**open_portable_project(&pass, &sealed).unwrap(), plain);
        assert!(open_portable_project(
            &SecretString::from("wrong owned package password"),
            &sealed
        )
        .is_err());
        let mut changed = sealed.clone();
        *changed.last_mut().unwrap() ^= 1;
        assert!(open_portable_project(&pass, &changed).is_err());
        for offset in [0, HEADER - 1, MAGIC.len(), MAGIC.len() + SALT] {
            let mut changed = sealed.clone();
            changed[offset] ^= 1;
            assert!(open_portable_project(&pass, &changed).is_err());
        }
        assert!(open_portable_project(&pass, &sealed[..sealed.len() - 1]).is_err());
    }
    #[test]
    fn version_lengths_and_plaintext_budget_reject_before_kdf_or_parser() {
        assert_eq!(
            v1_params().unwrap(),
            Argon2Params::DEFAULT,
            "current v1 write policy must be explicitly revisited when defaults change"
        );
        let pass = SecretString::from("");
        assert!(open_portable_project(&pass, b"unknown portable version").is_err());
        let mut raw = vec![0; HEADER + TAG];
        raw[..MAGIC.len()].copy_from_slice(MAGIC);
        raw[HEADER - 4..HEADER].copy_from_slice(&u32::MAX.to_be_bytes());
        assert_eq!(
            open_portable_project(&pass, &raw).unwrap_err(),
            CryptoError::Decrypt
        );
        assert_eq!(
            seal_portable_project(&pass, &vec![0; MAX_PORTABLE_PROJECT_PLAIN + 1]).unwrap_err(),
            CryptoError::Decrypt
        );
    }
}
