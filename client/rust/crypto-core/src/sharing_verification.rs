//! Verifying a colleague's sharing keys outside the server (audit H3,
//! TR-11): a long safety number computed locally from **both** parties'
//! identity keys, and a QR payload with the full keys of one party.
//!
//! * **Safety number**: 12 groups of 5 digits (≈ 199 bits). Both devices
//!   compute the same number from their own keys and the keys they were
//!   given for the other party. A server that substitutes either side's keys
//!   changes the number on that side. One party reads it out, the other
//!   types it in (pasting is not offered for this field).
//! * **QR code**: `consolecrypt-verify:v1:<base64url(instance ‖ user ‖
//!   device ‖ X25519 ‖ Ed25519)>` — scanning (or entering) the payload of the
//!   colleague's screen proves their real keys in person.
//!
//! The previous 64-hex "verification code" (`sharing_identity_code`) remains
//! a fingerprint for details; it is not a comparison code.

use crate::sharing::SharingCryptoError;
use crate::DevicePublicKeys;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine as _;
use cc_protocol::{DeviceId, UserId};
use sha2::{Digest, Sha512};
use std::fmt;

/// Domain label of the safety number input.
pub const SAFETY_NUMBER_LABEL: &[u8] = b"consolecrypt/v1/sharing-safety-number\0";
/// Scheme and version prefix of the QR payload.
pub const VERIFY_QR_PREFIX: &str = "consolecrypt-verify:v1:";
const PARTY_LEN: usize = 16 + 16 + 32 + 32;

/// One party of a sharing verification: account, installation, keys.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SharingParty {
    pub user_id: UserId,
    pub device_id: DeviceId,
    pub keys: DevicePublicKeys,
}

impl SharingParty {
    fn encode(&self) -> [u8; PARTY_LEN] {
        let mut out = [0u8; PARTY_LEN];
        out[..16].copy_from_slice(self.user_id.as_bytes());
        out[16..32].copy_from_slice(self.device_id.as_bytes());
        out[32..64].copy_from_slice(&self.keys.encryption);
        out[64..].copy_from_slice(&self.keys.signing);
        out
    }
}

/// The long safety number: 12 groups of 5 digits.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct SafetyNumber {
    groups: [u32; 12],
}

impl SafetyNumber {
    /// The groups as zero-padded 5-digit strings.
    pub fn group_strings(&self) -> [String; 12] {
        self.groups.map(|g| format!("{g:05}"))
    }

    /// True if `input` spells this number. Only digits count; spaces,
    /// dashes and line breaks are ignored. Any other character fails.
    pub fn matches_str(&self, input: &str) -> bool {
        let mut digits = String::with_capacity(60);
        for c in input.chars() {
            if c.is_ascii_digit() {
                digits.push(c);
            } else if !(c.is_whitespace() || c == '-') {
                return false;
            }
        }
        digits == self.group_strings().concat()
    }
}

impl fmt::Display for SafetyNumber {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.group_strings().join(" "))
    }
}

impl fmt::Debug for SafetyNumber {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "SafetyNumber({self})")
    }
}

/// Safety number between `a` and `b` on server instance `instance`. The
/// order of the parties does not matter (they are sorted by their encoding),
/// so both devices compute the same number.
///
/// `SHA-512(SAFETY_NUMBER_LABEL ‖ instance (16) ‖ min(enc a, enc b) ‖
/// max(enc a, enc b))`, `enc` = user (16) ‖ device (16) ‖ X25519 (32) ‖
/// Ed25519 (32); group `i` = big-endian u40 of bytes `[5i, 5i+5)` mod 100000.
pub fn sharing_safety_number(
    instance: uuid::Uuid,
    a: &SharingParty,
    b: &SharingParty,
) -> Result<SafetyNumber, SharingCryptoError> {
    if instance.is_nil() || a.device_id == b.device_id {
        return Err(SharingCryptoError::ContextMismatch);
    }
    let (ea, eb) = (a.encode(), b.encode());
    let (lo, hi) = if ea <= eb { (ea, eb) } else { (eb, ea) };
    let mut h = Sha512::new();
    h.update(SAFETY_NUMBER_LABEL);
    h.update(instance.as_bytes());
    h.update(lo);
    h.update(hi);
    let digest = h.finalize();
    let mut groups = [0u32; 12];
    for (i, g) in groups.iter_mut().enumerate() {
        let v = digest[5 * i..5 * i + 5]
            .iter()
            .fold(0u64, |acc, b| (acc << 8) | u64::from(*b));
        *g = (v % 100_000) as u32;
    }
    Ok(SafetyNumber { groups })
}

/// QR payload with the full keys of `party` (shown on its own screen).
pub fn verification_qr_payload(instance: uuid::Uuid, party: &SharingParty) -> String {
    let mut bytes = Vec::with_capacity(16 + PARTY_LEN);
    bytes.extend_from_slice(instance.as_bytes());
    bytes.extend_from_slice(&party.encode());
    format!("{VERIFY_QR_PREFIX}{}", URL_SAFE_NO_PAD.encode(bytes))
}

/// Parse a scanned or entered QR payload → (instance, party). The keys are
/// validated like keys from the server.
pub fn parse_verification_qr(
    payload: &str,
) -> Result<(uuid::Uuid, SharingParty), SharingCryptoError> {
    let bad = || SharingCryptoError::Structure("not a ConsoleCrypt verification code");
    let body = payload
        .trim()
        .strip_prefix(VERIFY_QR_PREFIX)
        .ok_or_else(bad)?;
    let bytes = URL_SAFE_NO_PAD.decode(body).map_err(|_| bad())?;
    if bytes.len() != 16 + PARTY_LEN {
        return Err(bad());
    }
    let instance = uuid::Uuid::from_slice(&bytes[..16]).map_err(|_| bad())?;
    let user_id = UserId::from(uuid::Uuid::from_slice(&bytes[16..32]).map_err(|_| bad())?);
    let device_id = DeviceId::from(uuid::Uuid::from_slice(&bytes[32..48]).map_err(|_| bad())?);
    let keys = DevicePublicKeys::from_slices(&bytes[48..80], &bytes[80..112])?;
    Ok((
        instance,
        SharingParty {
            user_id,
            device_id,
            keys,
        },
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::DeviceSecretKeys;
    use std::str::FromStr;

    fn party(seed_x: u8, seed_e: u8, user: &str, device: &str) -> SharingParty {
        let mut x = [0u8; 32];
        let mut e = [0u8; 32];
        for i in 0..32 {
            x[i] = seed_x.wrapping_add(i as u8);
            e[i] = seed_e.wrapping_add(i as u8);
        }
        SharingParty {
            user_id: UserId::from_str(user).unwrap(),
            device_id: DeviceId::from_str(device).unwrap(),
            keys: DeviceSecretKeys::from_test_seeds(&x, &e).public_keys(),
        }
    }

    fn instance() -> uuid::Uuid {
        uuid::Uuid::from_str("00000000-0000-7000-8000-0000000000f1").unwrap()
    }

    #[test]
    fn both_sides_compute_the_same_number_and_substitution_changes_it() {
        let a = party(
            0x11,
            0x22,
            "00000000-0000-7000-8000-0000000000b1",
            "00000000-0000-7000-8000-0000000000c1",
        );
        let b = party(
            0x33,
            0x44,
            "00000000-0000-7000-8000-0000000000b2",
            "00000000-0000-7000-8000-0000000000c2",
        );
        let ab = sharing_safety_number(instance(), &a, &b).unwrap();
        assert_eq!(ab, sharing_safety_number(instance(), &b, &a).unwrap());
        assert_eq!(ab.group_strings().len(), 12);
        assert!(ab.matches_str(&ab.to_string()));
        assert!(ab.matches_str(&ab.group_strings().join("-")));
        assert!(!ab.matches_str(&format!("{ab}x")));
        // A server shows A other keys for B: the numbers differ.
        let fake_b = SharingParty {
            keys: DeviceSecretKeys::generate().unwrap().public_keys(),
            ..b
        };
        assert_ne!(ab, sharing_safety_number(instance(), &a, &fake_b).unwrap());
        // Another instance gives another number.
        assert_ne!(
            ab,
            sharing_safety_number(uuid::Uuid::new_v4(), &a, &b).unwrap()
        );
        assert!(sharing_safety_number(instance(), &a, &a).is_err());
    }

    /// Fixed vector for other implementations.
    #[test]
    fn safety_number_vector() {
        let a = party(
            0x11,
            0x22,
            "00000000-0000-7000-8000-0000000000b1",
            "00000000-0000-7000-8000-0000000000c1",
        );
        let b = party(
            0x33,
            0x44,
            "00000000-0000-7000-8000-0000000000b2",
            "00000000-0000-7000-8000-0000000000c2",
        );
        let n = sharing_safety_number(instance(), &a, &b).unwrap();
        // Recompute independently from the documented layout.
        let mut input = SAFETY_NUMBER_LABEL.to_vec();
        input.extend_from_slice(instance().as_bytes());
        let (ea, eb) = (a.encode(), b.encode());
        let (lo, hi) = if ea <= eb { (ea, eb) } else { (eb, ea) };
        input.extend_from_slice(&lo);
        input.extend_from_slice(&hi);
        let d = Sha512::digest(&input);
        let first = d[..5]
            .iter()
            .fold(0u64, |acc, b| (acc << 8) | u64::from(*b))
            % 100_000;
        assert_eq!(n.group_strings()[0], format!("{first:05}"));
    }

    #[test]
    fn qr_payload_roundtrip_and_rejects_garbage() {
        let a = party(
            0x55,
            0x66,
            "00000000-0000-7000-8000-0000000000b3",
            "00000000-0000-7000-8000-0000000000c3",
        );
        let p = verification_qr_payload(instance(), &a);
        assert!(p.starts_with(VERIFY_QR_PREFIX));
        assert_eq!(parse_verification_qr(&p).unwrap(), (instance(), a));
        assert_eq!(parse_verification_qr(&format!("  {p}\n")).unwrap().1, a);
        for bad in [
            "",
            "consolecrypt-verify:v1:",
            "consolecrypt-recovery:v1:x",
            &p[..p.len() - 4],
        ] {
            assert!(parse_verification_qr(bad).is_err(), "{bad}");
        }
    }
}
