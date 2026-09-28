//! Key derivation: HKDF-SHA256 for high-entropy keys, Argon2id for
//! passphrases (ADR-0002 §Primitives).

use crate::error::{CryptoError, Result};
use crate::keys::Secret32;
use argon2::{Algorithm, Argon2, Params, Version};
use cc_protocol::envelopes::{KdfAlgorithm, KdfParams};
use cc_protocol::limits::{ARGON2_SALT_LEN, KEY_LEN};
use cc_protocol::Bytes;
use hkdf::Hkdf;
use secrecy::{ExposeSecret, ExposeSecretMut, SecretString};
use sha2::Sha256;
use unicode_normalization::UnicodeNormalization;
use zeroize::Zeroizing;

/// `HKDF-SHA256(ikm, salt, info = info_parts[0] || info_parts[1] || …)`,
/// 32-byte output written straight into a zeroizing box.
pub(crate) fn hkdf_sha256_32(ikm: &[u8], salt: &[u8], info_parts: &[&[u8]]) -> Secret32 {
    let hk = Hkdf::<Sha256>::new(Some(salt), ikm);
    let mut out = Secret32::init_with_mut(|_| {});
    // Infallible: 32 bytes is far below HKDF-SHA256's 255 * 32 output limit.
    hk.expand_multi_info(info_parts, out.expose_secret_mut())
        .expect("HKDF-SHA256 can always produce 32 bytes");
    out
}

/// Argon2id cost parameters of a password envelope (the salt is generated
/// per envelope and stored next to them in `metadata.kdf`).
///
/// Every constructor enforces `KdfParams::MIN_*` (the server floor) and
/// `KdfParams::MAX_*` (the client ceiling), so an out-of-range value can
/// never be produced locally.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Argon2Params {
    memory_kib: u32,
    iterations: u32,
    parallelism: u32,
}

impl Argon2Params {
    /// Production default (ADR-0002): m = 65536 KiB (64 MiB), t = 3, p = 1.
    pub const DEFAULT: Self = Self {
        memory_kib: 64 * 1024,
        iterations: 3,
        parallelism: 1,
    };

    /// The server floor (`KdfParams::MIN_*`, p = 1): the cheapest parameters
    /// any valid envelope may carry. Never go below this; calibration on slow
    /// machines bottoms out here.
    pub const FLOOR: Self = Self {
        memory_kib: KdfParams::MIN_MEMORY_KIB,
        iterations: KdfParams::MIN_ITERATIONS,
        parallelism: 1,
    };

    /// Validated constructor.
    pub fn new(memory_kib: u32, iterations: u32, parallelism: u32) -> Result<Self> {
        let p = Self {
            memory_kib,
            iterations,
            parallelism,
        };
        p.check()?;
        Ok(p)
    }

    /// Cheapest valid parameters, for **tests only** — keeps test suites fast
    /// while still producing envelopes that pass `validate()`. Production
    /// code uses [`Argon2Params::DEFAULT`] or a calibrated value.
    pub const fn for_tests() -> Self {
        Self::FLOOR
    }

    /// Memory cost in KiB.
    pub const fn memory_kib(&self) -> u32 {
        self.memory_kib
    }

    /// Time cost (passes).
    pub const fn iterations(&self) -> u32 {
        self.iterations
    }

    /// Lanes.
    pub const fn parallelism(&self) -> u32 {
        self.parallelism
    }

    fn check(&self) -> Result<()> {
        if self.memory_kib < KdfParams::MIN_MEMORY_KIB {
            return Err(CryptoError::KdfParams("memory below server floor"));
        }
        if self.memory_kib > KdfParams::MAX_MEMORY_KIB {
            return Err(CryptoError::KdfParams("memory above client ceiling"));
        }
        if self.iterations < KdfParams::MIN_ITERATIONS {
            return Err(CryptoError::KdfParams("iterations below server floor"));
        }
        if self.iterations > KdfParams::MAX_ITERATIONS {
            return Err(CryptoError::KdfParams("iterations above client ceiling"));
        }
        if self.parallelism == 0 || self.parallelism > KdfParams::MAX_PARALLELISM {
            return Err(CryptoError::KdfParams("parallelism out of range"));
        }
        // Argon2 needs at least 8 KiB per lane; implied by the floor, checked
        // anyway so the invariant is local.
        if self.memory_kib < 8 * self.parallelism {
            return Err(CryptoError::KdfParams("memory too small for parallelism"));
        }
        Ok(())
    }

    /// Protocol representation with `salt`.
    pub(crate) fn to_protocol(self, salt: &[u8; ARGON2_SALT_LEN]) -> KdfParams {
        KdfParams {
            algorithm: KdfAlgorithm::Argon2id,
            salt: Bytes::new(salt.to_vec()),
            memory_kib: self.memory_kib,
            iterations: self.iterations,
            parallelism: self.parallelism,
        }
    }

    /// Parse and bounds-check received parameters (client ceiling check —
    /// a hostile server must not be able to make us allocate 4 TiB).
    pub(crate) fn from_protocol(k: &KdfParams) -> Result<(Self, [u8; ARGON2_SALT_LEN])> {
        let KdfAlgorithm::Argon2id = k.algorithm;
        let salt = k
            .salt
            .to_array::<ARGON2_SALT_LEN>()
            .ok_or(CryptoError::KdfParams("salt must be 16 bytes"))?;
        let p = Self::new(k.memory_kib, k.iterations, k.parallelism)?;
        Ok((p, salt))
    }
}

impl Default for Argon2Params {
    fn default() -> Self {
        Self::DEFAULT
    }
}

/// Unicode NFC normalization of a passphrase (ADR-0002: `UTF-8(NFC(pass))`),
/// into a zeroizing buffer. The same passphrase typed on macOS (which tends
/// to produce NFD) and Windows (NFC) thus yields the same key.
pub fn normalize_passphrase(passphrase: &SecretString) -> Zeroizing<String> {
    let src = passphrase.expose_secret();
    // Pre-size generously so the buffer (and thus copies of the passphrase)
    // is not reallocated while pushing characters.
    let mut out = Zeroizing::new(String::with_capacity(src.len() * 3 + 16));
    out.extend(src.nfc());
    out
}

/// `PEK = Argon2id(UTF-8(NFC(passphrase)), salt, params)`, 32 bytes.
pub(crate) fn derive_password_key(
    passphrase: &SecretString,
    salt: &[u8; ARGON2_SALT_LEN],
    params: Argon2Params,
) -> Result<Secret32> {
    params.check()?;
    let normalized = normalize_passphrase(passphrase);
    if normalized.is_empty() {
        return Err(CryptoError::EmptyPassphrase);
    }
    let argon_params = Params::new(
        params.memory_kib,
        params.iterations,
        params.parallelism,
        Some(KEY_LEN),
    )
    .map_err(|_| CryptoError::KdfParams("rejected by Argon2"))?;
    let argon = Argon2::new(Algorithm::Argon2id, Version::V0x13, argon_params);
    let mut out = Secret32::init_with_mut(|_| {});
    argon
        .hash_password_into(normalized.as_bytes(), salt, out.expose_secret_mut())
        .map_err(|_| CryptoError::Kdf)?;
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nfc_normalization_unifies_forms() {
        let nfd = SecretString::from("Cafe\u{301} ☕");
        let nfc = SecretString::from("Caf\u{e9} ☕");
        assert_eq!(*normalize_passphrase(&nfd), *normalize_passphrase(&nfc));
        assert_eq!(normalize_passphrase(&nfd).as_str(), "Caf\u{e9} ☕");
    }

    #[test]
    fn params_bounds() {
        assert!(Argon2Params::new(1024, 3, 1).is_err());
        assert!(Argon2Params::new(KdfParams::MAX_MEMORY_KIB + 1, 3, 1).is_err());
        assert!(Argon2Params::new(65536, 1, 1).is_err());
        assert!(Argon2Params::new(65536, KdfParams::MAX_ITERATIONS + 1, 1).is_err());
        assert!(Argon2Params::new(65536, 3, 0).is_err());
        assert!(Argon2Params::new(65536, 3, KdfParams::MAX_PARALLELISM + 1).is_err());
        assert_eq!(
            Argon2Params::new(65536, 3, 1).unwrap(),
            Argon2Params::DEFAULT
        );
        assert_eq!(Argon2Params::default(), Argon2Params::DEFAULT);
        Argon2Params::FLOOR.check().unwrap();
    }

    #[test]
    fn ceiling_enforced_on_received_params() {
        let mut k = Argon2Params::DEFAULT.to_protocol(&[1; 16]);
        k.memory_kib = u32::MAX;
        assert!(matches!(
            Argon2Params::from_protocol(&k),
            Err(CryptoError::KdfParams(_))
        ));
        let mut k = Argon2Params::DEFAULT.to_protocol(&[1; 16]);
        k.salt = Bytes::new(vec![1; 8]);
        assert!(Argon2Params::from_protocol(&k).is_err());
    }

    #[test]
    fn empty_passphrase_rejected() {
        let r = derive_password_key(&SecretString::from(""), &[0; 16], Argon2Params::FLOOR);
        assert_eq!(r.err(), Some(CryptoError::EmptyPassphrase));
    }
}
