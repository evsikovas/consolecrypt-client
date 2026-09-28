//! Recovery Key (ADR-0002 §Recovery envelope): 32 random bytes shown to the
//! user once, as a BIP-39 English 24-word mnemonic (entropy encoding only —
//! *not* the BIP-39 seed derivation) and as a QR payload
//! `consolecrypt-recovery:v1:<vault_id>:<RK base64url, no padding>`.

use crate::error::{CryptoError, MnemonicError, Result};
use crate::keys::{random_secret32, secret32_from, Secret32};
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine as _;
use bip39::Language;
use cc_protocol::limits::KEY_LEN;
use cc_protocol::VaultId;
use secrecy::{ExposeSecret, ExposeSecretMut};
use sha2::{Digest, Sha256};
use std::fmt;
use std::str::FromStr;
use zeroize::Zeroizing;

/// Number of words in a recovery phrase (256-bit entropy + 8-bit checksum).
pub const RECOVERY_WORD_COUNT: usize = 24;
/// QR payload prefix (scheme + version).
pub const RECOVERY_QR_PREFIX: &str = "consolecrypt-recovery:v1:";

/// The 256-bit Recovery Key. Never stored anywhere by the client; only its
/// recovery envelope is uploaded.
pub struct RecoveryKey(Secret32);

impl RecoveryKey {
    /// Generate a fresh Recovery Key from the OS CSPRNG.
    pub fn generate() -> Result<Self> {
        random_secret32().map(Self)
    }

    #[allow(dead_code)]
    pub(crate) fn from_bytes(bytes: &[u8; KEY_LEN]) -> Self {
        Self(secret32_from(bytes))
    }

    pub(crate) fn expose(&self) -> &[u8; KEY_LEN] {
        self.0.expose_secret()
    }

    /// BIP-39 English 24-word encoding of the key.
    pub fn to_mnemonic(&self) -> RecoveryPhrase {
        let words = Language::English.word_list();
        let entropy = self.expose();
        let checksum = Sha256::digest(entropy)[0];
        let mut phrase = Zeroizing::new(String::with_capacity(RECOVERY_WORD_COUNT * 9));
        for i in 0..RECOVERY_WORD_COUNT {
            let idx = word_index_at(entropy, checksum, i);
            if i > 0 {
                phrase.push(' ');
            }
            phrase.push_str(words[idx]);
        }
        RecoveryPhrase(phrase)
    }

    /// Decode a 24-word phrase. Case-insensitive; any whitespace separates
    /// words. Errors name the offending (1-based) word position.
    pub fn from_mnemonic(phrase: &str) -> Result<Self> {
        let count = phrase.split_whitespace().count();
        if count != RECOVERY_WORD_COUNT {
            return Err(MnemonicError::WordCount(count).into());
        }
        let mut indices = Zeroizing::new([0u16; RECOVERY_WORD_COUNT]);
        for (i, word) in phrase.split_whitespace().enumerate() {
            let lower = Zeroizing::new(word.to_lowercase());
            indices[i] = Language::English
                .find_word(&lower)
                .ok_or(MnemonicError::UnknownWord(i + 1))?;
        }
        // 24 × 11 bits = 264 bits = 32 bytes entropy + 1 byte checksum.
        let mut bits = Zeroizing::new([0u8; KEY_LEN + 1]);
        for (i, idx) in indices.iter().enumerate() {
            for b in 0..11 {
                if (idx >> (10 - b)) & 1 == 1 {
                    let pos = i * 11 + b;
                    bits[pos / 8] |= 1 << (7 - (pos % 8));
                }
            }
        }
        let mut key = Secret32::init_with_mut(|_| {});
        key.expose_secret_mut().copy_from_slice(&bits[..KEY_LEN]);
        if Sha256::digest(key.expose_secret())[0] != bits[KEY_LEN] {
            return Err(MnemonicError::Checksum.into());
        }
        Ok(Self(key))
    }

    /// QR payload `consolecrypt-recovery:v1:<vault_id>:<base64url(RK)>`.
    pub fn to_qr_payload(&self, vault_id: VaultId) -> Zeroizing<String> {
        let mut s = Zeroizing::new(String::with_capacity(
            RECOVERY_QR_PREFIX.len() + 36 + 1 + 43,
        ));
        s.push_str(RECOVERY_QR_PREFIX);
        s.push_str(&vault_id.to_string());
        s.push(':');
        URL_SAFE_NO_PAD.encode_string(self.expose(), &mut s);
        s
    }

    /// Parse a QR payload into `(vault_id, recovery key)`. Surrounding
    /// whitespace is ignored; everything else must match exactly.
    pub fn from_qr_payload(payload: &str) -> Result<(VaultId, Self)> {
        let rest = payload
            .trim()
            .strip_prefix(RECOVERY_QR_PREFIX)
            .ok_or(CryptoError::RecoveryPayload("unknown prefix or version"))?;
        let (vault, key) = rest
            .split_once(':')
            .ok_or(CryptoError::RecoveryPayload("missing key part"))?;
        let vault_id = VaultId::from_str(vault)
            .map_err(|_| CryptoError::RecoveryPayload("invalid vault id"))?;
        let decoded = Zeroizing::new(
            URL_SAFE_NO_PAD
                .decode(key)
                .map_err(|_| CryptoError::RecoveryPayload("invalid base64url key"))?,
        );
        let bytes: &[u8; KEY_LEN] = decoded
            .as_slice()
            .try_into()
            .map_err(|_| CryptoError::RecoveryPayload("key must be 32 bytes"))?;
        Ok((vault_id, Self(secret32_from(bytes))))
    }
}

/// 11-bit word index `i` of `entropy (256 bits) || checksum (8 bits)`.
fn word_index_at(entropy: &[u8; KEY_LEN], checksum: u8, i: usize) -> usize {
    let bit = |pos: usize| -> usize {
        let byte = if pos / 8 < KEY_LEN {
            entropy[pos / 8]
        } else {
            checksum
        };
        usize::from((byte >> (7 - (pos % 8))) & 1)
    };
    (0..11).fold(0usize, |acc, b| (acc << 1) | bit(i * 11 + b))
}

impl fmt::Debug for RecoveryKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("RecoveryKey(<redacted>)")
    }
}

/// A 24-word recovery phrase (single-space separated, lowercase) in a
/// zeroizing buffer, with redacted `Debug`.
pub struct RecoveryPhrase(Zeroizing<String>);

impl RecoveryPhrase {
    /// The whole phrase. Show it to the user; never log it.
    pub fn expose_phrase(&self) -> &str {
        &self.0
    }

    /// The words, in order.
    pub fn words(&self) -> impl Iterator<Item = &str> + '_ {
        self.0.split(' ')
    }

    /// Word at 1-based `position` (1..=24).
    pub fn word(&self, position: usize) -> Option<&str> {
        position.checked_sub(1).and_then(|i| self.words().nth(i))
    }
}

impl fmt::Debug for RecoveryPhrase {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("RecoveryPhrase(<redacted>)")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mnemonic_roundtrip_and_matches_bip39_crate() {
        for _ in 0..20 {
            let rk = RecoveryKey::generate().unwrap();
            let phrase = rk.to_mnemonic();
            assert_eq!(phrase.words().count(), 24);
            // Cross-check our encoder against the reference implementation.
            let reference =
                bip39::Mnemonic::from_entropy_in(Language::English, rk.expose()).unwrap();
            assert_eq!(phrase.expose_phrase(), reference.to_string());
            let back = RecoveryKey::from_mnemonic(phrase.expose_phrase()).unwrap();
            assert_eq!(back.expose(), rk.expose());
        }
    }

    #[test]
    fn bip39_reference_vector() {
        // BIP-39 official vector (entropy 0x00 * 32).
        let rk = RecoveryKey::from_bytes(&[0u8; 32]);
        let expected = "abandon abandon abandon abandon abandon abandon abandon abandon \
                        abandon abandon abandon abandon abandon abandon abandon abandon \
                        abandon abandon abandon abandon abandon abandon abandon art";
        assert_eq!(rk.to_mnemonic().expose_phrase(), expected);
        // BIP-39 official vector (entropy 0xff * 32).
        let rk = RecoveryKey::from_bytes(&[0xffu8; 32]);
        assert_eq!(
            rk.to_mnemonic().expose_phrase(),
            "zoo zoo zoo zoo zoo zoo zoo zoo zoo zoo zoo zoo zoo zoo zoo zoo zoo zoo zoo zoo zoo zoo zoo vote"
        );
    }

    #[test]
    fn mnemonic_input_is_lenient_on_case_and_whitespace() {
        let rk = RecoveryKey::generate().unwrap();
        let phrase = rk.to_mnemonic();
        let messy = format!(
            "  {}\n",
            phrase.expose_phrase().to_uppercase().replace(' ', " \t ")
        );
        assert_eq!(
            RecoveryKey::from_mnemonic(&messy).unwrap().expose(),
            rk.expose()
        );
    }

    #[test]
    fn mnemonic_errors() {
        let rk = RecoveryKey::generate().unwrap();
        let phrase = rk.to_mnemonic();
        let words: Vec<&str> = phrase.words().collect();
        assert_eq!(
            RecoveryKey::from_mnemonic(&words[..23].join(" ")).err(),
            Some(CryptoError::Mnemonic(MnemonicError::WordCount(23)))
        );
        let mut w = words.clone();
        w[6] = "notaword";
        assert_eq!(
            RecoveryKey::from_mnemonic(&w.join(" ")).err(),
            Some(CryptoError::Mnemonic(MnemonicError::UnknownWord(7)))
        );
        let mut w = words.clone();
        w.swap(0, 1);
        if w[0] != w[1] {
            assert_eq!(
                RecoveryKey::from_mnemonic(&w.join(" ")).err(),
                Some(CryptoError::Mnemonic(MnemonicError::Checksum))
            );
        }
    }

    #[test]
    fn qr_roundtrip_and_errors() {
        let rk = RecoveryKey::generate().unwrap();
        let v = VaultId::new();
        let qr = rk.to_qr_payload(v);
        assert!(qr.starts_with("consolecrypt-recovery:v1:"));
        assert!(!qr.contains('='));
        let (v2, rk2) = RecoveryKey::from_qr_payload(&format!(" {} ", qr.as_str())).unwrap();
        assert_eq!(v2, v);
        assert_eq!(rk2.expose(), rk.expose());

        for bad in [
            qr.replace("v1", "v2"),
            qr.replace("consolecrypt-recovery", "x"),
            format!("{}A", qr.as_str()),
            qr[..qr.len() - 2].to_string(),
            format!(
                "consolecrypt-recovery:v1:not-a-uuid:{}",
                &qr[qr.len() - 43..]
            ),
            format!("consolecrypt-recovery:v1:{v}"),
        ] {
            assert!(
                matches!(
                    RecoveryKey::from_qr_payload(&bad),
                    Err(CryptoError::RecoveryPayload(_))
                ),
                "{bad}"
            );
        }
    }

    #[test]
    fn phrase_accessors_and_debug() {
        let rk = RecoveryKey::generate().unwrap();
        let p = rk.to_mnemonic();
        assert_eq!(p.word(1), p.words().next());
        assert_eq!(p.word(24), p.words().last());
        assert_eq!(p.word(0), None);
        assert_eq!(p.word(25), None);
        assert_eq!(format!("{p:?}"), "RecoveryPhrase(<redacted>)");
        assert_eq!(format!("{rk:?}"), "RecoveryKey(<redacted>)");
    }
}
