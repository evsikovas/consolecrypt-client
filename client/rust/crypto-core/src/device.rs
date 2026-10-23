//! Device keys (ADR-0002 §Device keys, ADR-0004): an X25519 keypair for
//! receiving device envelopes and an Ed25519 keypair for signing device
//! approvals, plus the verification code users compare before approving.

use crate::error::{CryptoError, Result};
use crate::keys::{secret32_from, Secret32};
use crate::rng::fill_random;
use cc_protocol::canonical::{
    device_approval_message, device_approval_message_v2, device_envelope_hash_input,
    device_fingerprint_input, device_login_message, device_pairing_code_v2_input,
    request_proof_message, vault_key_commitment_message, ApprovedVaultKey,
};
use cc_protocol::devices::DeviceInfo;
use cc_protocol::envelopes::NewEnvelope;
use cc_protocol::limits::{ED25519_PUBLIC_KEY_LEN, ED25519_SIGNATURE_LEN, X25519_PUBLIC_KEY_LEN};
use cc_protocol::{Bytes, DeviceId, DeviceRequestId, VaultId};
use ed25519_dalek::{Signature, Signer, SigningKey, VerifyingKey};
use sha2::{Digest, Sha256};
use std::fmt;
use x25519_dalek::{PublicKey as X25519PublicKey, StaticSecret};
use zeroize::Zeroizing;

/// Magic + version prefix of the secret-key storage encoding.
const SECRET_MAGIC: &[u8; 4] = b"CCDK";
const SECRET_VERSION: u8 = 1;
/// `magic (4) || version (1) || x25519 secret (32) || ed25519 seed (32)`.
const SECRET_ENCODED_LEN: usize = 4 + 1 + 32 + 32;

/// A device's private keys. Zeroized on drop, redacted `Debug`, never leave
/// the device (persist them only through a secure store).
pub struct DeviceSecretKeys {
    encryption: Box<StaticSecret>,
    signing: Box<SigningKey>,
}

impl DeviceSecretKeys {
    /// Generate fresh X25519 + Ed25519 keypairs from the OS CSPRNG.
    pub fn generate() -> Result<Self> {
        let mut x = Zeroizing::new([0u8; 32]);
        let mut e = Zeroizing::new([0u8; 32]);
        fill_random(x.as_mut())?;
        fill_random(e.as_mut())?;
        Ok(Self::from_seeds(&x, &e))
    }

    fn from_seeds(x25519_secret: &[u8; 32], ed25519_seed: &[u8; 32]) -> Self {
        Self {
            encryption: Box::new(StaticSecret::from(*x25519_secret)),
            signing: Box::new(SigningKey::from_bytes(ed25519_seed)),
        }
    }

    /// Serialize for a secure store:
    /// `"CCDK" || 0x01 || x25519 secret (32) || ed25519 seed (32)`.
    pub fn to_secret_bytes(&self) -> Zeroizing<Vec<u8>> {
        let mut out = Zeroizing::new(Vec::with_capacity(SECRET_ENCODED_LEN));
        out.extend_from_slice(SECRET_MAGIC);
        out.push(SECRET_VERSION);
        out.extend_from_slice(self.encryption.as_bytes());
        out.extend_from_slice(self.signing.as_bytes());
        out
    }

    /// Inverse of [`DeviceSecretKeys::to_secret_bytes`].
    pub fn from_secret_bytes(bytes: &[u8]) -> Result<Self> {
        if bytes.len() != SECRET_ENCODED_LEN {
            return Err(CryptoError::SecretKeyEncoding("wrong length"));
        }
        if &bytes[..4] != SECRET_MAGIC {
            return Err(CryptoError::SecretKeyEncoding("bad magic"));
        }
        if bytes[4] != SECRET_VERSION {
            return Err(CryptoError::SecretKeyEncoding("unsupported version"));
        }
        let mut x = Zeroizing::new([0u8; 32]);
        let mut e = Zeroizing::new([0u8; 32]);
        x.copy_from_slice(&bytes[5..37]);
        e.copy_from_slice(&bytes[37..69]);
        Ok(Self::from_seeds(&x, &e))
    }

    /// Public halves, as registered with the server.
    pub fn public_keys(&self) -> DevicePublicKeys {
        DevicePublicKeys {
            encryption: X25519PublicKey::from(self.encryption.as_ref()).to_bytes(),
            signing: self.signing.verifying_key().to_bytes(),
        }
    }

    /// X25519 with `their_public`; rejects the all-zero (non-contributory)
    /// shared secret.
    pub(crate) fn diffie_hellman(&self, their_public: &[u8; 32]) -> Result<Secret32> {
        x25519(&self.encryption, their_public)
    }

    /// Sign a crate-owned canonical message. Public entry points must bind a
    /// specific domain and validate the operation before reaching this helper.
    pub(crate) fn sign_message(&self, message: &[u8]) -> [u8; ED25519_SIGNATURE_LEN] {
        self.signing.sign(message).to_bytes()
    }

    /// Ed25519-sign a device approval (ADR-0004 step 4).
    ///
    /// Call only after the user confirmed that the verification code of
    /// `approval.new_device_id` matches on both screens.
    pub fn sign_device_approval(
        &self,
        approval: &DeviceApproval<'_>,
    ) -> [u8; ED25519_SIGNATURE_LEN] {
        self.signing.sign(&approval.message()).to_bytes()
    }

    /// Ed25519-sign a v2 device approval (protocol 1.11): covers the
    /// approver's own keys and, per vault, the key commitment and the
    /// envelope sealed for the new device. Call only after the user
    /// confirmed the pairing code v2 on both screens.
    pub fn sign_device_approval_v2(
        &self,
        approval: &DeviceApprovalV2<'_>,
    ) -> [u8; ED25519_SIGNATURE_LEN] {
        self.signing.sign(&approval.message()).to_bytes()
    }

    /// Ed25519-sign this device's key-commitment record (protocol 1.11,
    /// [`vault_key_commitment_message`]).
    pub fn sign_key_commitment(
        &self,
        vault_id: VaultId,
        commitment: &[u8; 32],
        device_id: DeviceId,
        issued_at: i64,
    ) -> [u8; ED25519_SIGNATURE_LEN] {
        self.signing
            .sign(&vault_key_commitment_message(
                vault_id, commitment, device_id, issued_at,
            ))
            .to_bytes()
    }

    /// Proof of possession for login/register of `device_id` (protocol 1.4,
    /// ADR-0006): Ed25519 over [`device_login_message`]. The nonce must be
    /// fresh random bytes; the server rejects replays and stale `issued_at`.
    pub fn sign_device_login(
        &self,
        device_id: DeviceId,
        issued_at: i64,
        nonce: &[u8; 32],
    ) -> [u8; ED25519_SIGNATURE_LEN] {
        self.signing
            .sign(&device_login_message(device_id, issued_at, nonce))
            .to_bytes()
    }
}

impl DeviceSecretKeys {
    /// Per-request proof of possession (protocol 1.5, sender-constrained
    /// tokens): Ed25519 over [`request_proof_message`]. `path_and_query` is
    /// the request target exactly as sent; `body_sha256` is SHA-256 of the
    /// exact body bytes (of `b""` when empty).
    pub fn sign_request(
        &self,
        device_id: DeviceId,
        method: &str,
        path_and_query: &str,
        body_sha256: &[u8; 32],
        issued_at: i64,
        nonce: &[u8; 32],
    ) -> [u8; ED25519_SIGNATURE_LEN] {
        let msg = request_proof_message(
            device_id,
            method,
            path_and_query,
            body_sha256,
            issued_at,
            nonce,
        );
        self.signing.sign(&msg).to_bytes()
    }
}

/// SHA-256 of a request body, as bound into request proofs.
pub fn request_body_sha256(body: &[u8]) -> [u8; 32] {
    Sha256::digest(body).into()
}

/// Verify a request proof signature (test doubles; the server does the same).
#[allow(clippy::too_many_arguments)]
pub fn verify_request_proof(
    device_signing_key: &[u8; ED25519_PUBLIC_KEY_LEN],
    device_id: DeviceId,
    method: &str,
    path_and_query: &str,
    body_sha256: &[u8; 32],
    issued_at: i64,
    nonce: &[u8; 32],
    signature: &[u8],
) -> Result<()> {
    let vk = VerifyingKey::from_bytes(device_signing_key).map_err(|_| CryptoError::InvalidKey {
        what: "Ed25519 public key",
    })?;
    let sig = Signature::from_slice(signature).map_err(|_| CryptoError::InvalidSignature)?;
    let msg = request_proof_message(
        device_id,
        method,
        path_and_query,
        body_sha256,
        issued_at,
        nonce,
    );
    vk.verify_strict(&msg, &sig)
        .map_err(|_| CryptoError::InvalidSignature)
}

/// Verify a device login proof (used by test doubles; the real server does
/// the same with `verify_strict`).
pub fn verify_device_login(
    device_signing_key: &[u8; ED25519_PUBLIC_KEY_LEN],
    device_id: DeviceId,
    issued_at: i64,
    nonce: &[u8; 32],
    signature: &[u8],
) -> Result<()> {
    let vk = VerifyingKey::from_bytes(device_signing_key).map_err(|_| CryptoError::InvalidKey {
        what: "Ed25519 public key",
    })?;
    let sig = Signature::from_slice(signature).map_err(|_| CryptoError::InvalidSignature)?;
    vk.verify_strict(&device_login_message(device_id, issued_at, nonce), &sig)
        .map_err(|_| CryptoError::InvalidSignature)
}

impl fmt::Debug for DeviceSecretKeys {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("DeviceSecretKeys(<redacted>)")
    }
}

/// X25519 with an arbitrary static secret (also used for ephemeral keys).
pub(crate) fn x25519(secret: &StaticSecret, their_public: &[u8; 32]) -> Result<Secret32> {
    let shared = secret.diffie_hellman(&X25519PublicKey::from(*their_public));
    if !shared.was_contributory() {
        return Err(CryptoError::WeakKeyAgreement);
    }
    Ok(secret32_from(shared.as_bytes()))
}

/// Public X25519 key for an X25519 secret.
pub(crate) fn x25519_public(secret: &StaticSecret) -> [u8; 32] {
    X25519PublicKey::from(secret).to_bytes()
}

/// Fresh ephemeral X25519 secret (for device envelopes).
pub(crate) fn random_x25519_secret() -> Result<StaticSecret> {
    let mut x = Zeroizing::new([0u8; 32]);
    fill_random(x.as_mut())?;
    Ok(StaticSecret::from(*x))
}

/// X25519 secret from explicit bytes (deterministic test vectors).
#[cfg(test)]
pub(crate) fn x25519_secret_from(bytes: &[u8; 32]) -> StaticSecret {
    StaticSecret::from(*bytes)
}

#[cfg(test)]
impl DeviceSecretKeys {
    /// Deterministic keys for test vectors.
    pub(crate) fn from_test_seeds(x25519_secret: &[u8; 32], ed25519_seed: &[u8; 32]) -> Self {
        Self::from_seeds(x25519_secret, ed25519_seed)
    }
}

/// A device's public keys (32-byte X25519 and Ed25519).
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct DevicePublicKeys {
    /// X25519 public key — recipient key for device envelopes.
    pub encryption: [u8; X25519_PUBLIC_KEY_LEN],
    /// Ed25519 public key — verifies approvals signed by the device.
    pub signing: [u8; ED25519_PUBLIC_KEY_LEN],
}

impl DevicePublicKeys {
    /// Parse keys received from the server (e.g. [`DeviceInfo`]): checks
    /// lengths and that the Ed25519 key is a valid, non-weak curve point.
    pub fn from_slices(encryption: &[u8], signing: &[u8]) -> Result<Self> {
        let encryption: [u8; 32] = encryption.try_into().map_err(|_| CryptoError::InvalidKey {
            what: "X25519 public key",
        })?;
        let signing: [u8; 32] = signing.try_into().map_err(|_| CryptoError::InvalidKey {
            what: "Ed25519 public key",
        })?;
        let vk = VerifyingKey::from_bytes(&signing).map_err(|_| CryptoError::InvalidKey {
            what: "Ed25519 public key",
        })?;
        if vk.is_weak() {
            return Err(CryptoError::InvalidKey {
                what: "Ed25519 public key",
            });
        }
        Ok(Self {
            encryption,
            signing,
        })
    }

    /// Keys of a device as reported by the server.
    pub fn from_device_info(info: &DeviceInfo) -> Result<Self> {
        Self::from_slices(
            info.encryption_public_key.as_slice(),
            info.signing_public_key.as_slice(),
        )
    }

    /// X25519 key as protocol bytes.
    pub fn encryption_bytes(&self) -> Bytes {
        Bytes::new(self.encryption.to_vec())
    }

    /// Ed25519 key as protocol bytes.
    pub fn signing_bytes(&self) -> Bytes {
        Bytes::new(self.signing.to_vec())
    }

    /// Verification code of the device `device_id` owning these keys.
    pub fn verification_code(&self, device_id: DeviceId) -> VerificationCode {
        verification_code(device_id, &self.encryption, &self.signing)
    }
}

impl fmt::Debug for DevicePublicKeys {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("DevicePublicKeys")
            .field("encryption", &hex_prefix(&self.encryption))
            .field("signing", &hex_prefix(&self.signing))
            .finish()
    }
}

fn hex_prefix(b: &[u8; 32]) -> String {
    let mut s: String = b[..4].iter().map(|x| format!("{x:02x}")).collect();
    s.push('…');
    s
}

/// Fields of a device approval; [`DeviceApproval::message`] is the canonical
/// signed message (`cc_protocol::canonical::device_approval_message`).
#[derive(Debug, Clone, Copy)]
pub struct DeviceApproval<'a> {
    pub request_id: DeviceRequestId,
    pub approver_device_id: DeviceId,
    pub new_device_id: DeviceId,
    pub new_device_keys: &'a DevicePublicKeys,
    /// Unix seconds.
    pub issued_at: i64,
    /// Approved vaults (order and duplicates do not matter).
    pub vault_ids: &'a [VaultId],
}

impl DeviceApproval<'_> {
    /// Canonical bytes to sign/verify.
    pub fn message(&self) -> Vec<u8> {
        device_approval_message(
            self.request_id,
            self.approver_device_id,
            self.new_device_id,
            &self.new_device_keys.encryption,
            &self.new_device_keys.signing,
            self.issued_at,
            self.vault_ids,
        )
    }
}

/// Verify an approval signature (as the server does) with the approver's
/// registered Ed25519 key. Uses strict verification (rejects malleable
/// signatures and weak keys).
pub fn verify_device_approval(
    approver_signing_key: &[u8; ED25519_PUBLIC_KEY_LEN],
    approval: &DeviceApproval<'_>,
    signature: &[u8],
) -> Result<()> {
    let vk =
        VerifyingKey::from_bytes(approver_signing_key).map_err(|_| CryptoError::InvalidKey {
            what: "Ed25519 public key",
        })?;
    let sig = Signature::from_slice(signature).map_err(|_| CryptoError::InvalidSignature)?;
    vk.verify_strict(&approval.message(), &sig)
        .map_err(|_| CryptoError::InvalidSignature)
}

/// Fields of a v2 device approval (protocol 1.11);
/// [`DeviceApprovalV2::message`] is
/// `cc_protocol::canonical::device_approval_message_v2`.
#[derive(Debug, Clone, Copy)]
pub struct DeviceApprovalV2<'a> {
    pub request_id: DeviceRequestId,
    pub approver_device_id: DeviceId,
    pub approver_keys: &'a DevicePublicKeys,
    pub new_device_id: DeviceId,
    pub new_device_keys: &'a DevicePublicKeys,
    /// Unix seconds (the same as the v1 signature's).
    pub issued_at: i64,
    /// One entry per approved vault (sorted by the message).
    pub vaults: &'a [ApprovedVaultKey],
}

impl DeviceApprovalV2<'_> {
    /// Canonical bytes to sign/verify.
    pub fn message(&self) -> Vec<u8> {
        device_approval_message_v2(
            self.request_id,
            self.approver_device_id,
            &self.approver_keys.encryption,
            &self.approver_keys.signing,
            self.new_device_id,
            &self.new_device_keys.encryption,
            &self.new_device_keys.signing,
            self.issued_at,
            self.vaults,
        )
    }
}

/// Verify a v2 approval with the approver key **the verifier pinned**
/// (never one taken from the same server response). Strict verification;
/// duplicate vault ids are rejected.
pub fn verify_device_approval_v2(
    approver_signing_key: &[u8; ED25519_PUBLIC_KEY_LEN],
    approval: &DeviceApprovalV2<'_>,
    signature: &[u8],
) -> Result<()> {
    let mut ids: Vec<VaultId> = approval.vaults.iter().map(|v| v.vault_id).collect();
    ids.sort();
    ids.dedup();
    if ids.len() != approval.vaults.len() || approval.vaults.is_empty() {
        return Err(CryptoError::InvalidSignature);
    }
    if approval.approver_keys.signing != *approver_signing_key {
        return Err(CryptoError::InvalidSignature);
    }
    let vk =
        VerifyingKey::from_bytes(approver_signing_key).map_err(|_| CryptoError::InvalidKey {
            what: "Ed25519 public key",
        })?;
    let sig = Signature::from_slice(signature).map_err(|_| CryptoError::InvalidSignature)?;
    vk.verify_strict(&approval.message(), &sig)
        .map_err(|_| CryptoError::InvalidSignature)
}

/// Verify a key-commitment record signature (protocol 1.11).
pub fn verify_key_commitment(
    device_signing_key: &[u8; ED25519_PUBLIC_KEY_LEN],
    vault_id: VaultId,
    commitment: &[u8; 32],
    device_id: DeviceId,
    issued_at: i64,
    signature: &[u8],
) -> Result<()> {
    let vk = VerifyingKey::from_bytes(device_signing_key).map_err(|_| CryptoError::InvalidKey {
        what: "Ed25519 public key",
    })?;
    let sig = Signature::from_slice(signature).map_err(|_| CryptoError::InvalidSignature)?;
    vk.verify_strict(
        &vault_key_commitment_message(vault_id, commitment, device_id, issued_at),
        &sig,
    )
    .map_err(|_| CryptoError::InvalidSignature)
}

/// `SHA-256(ephemeral_pub ‖ nonce ‖ ciphertext)` of a device envelope, as
/// bound into v2 approvals (protocol 1.11).
pub fn device_envelope_sha256(envelope: &NewEnvelope) -> Result<[u8; 32]> {
    let eph = envelope
        .metadata
        .ephemeral_public_key
        .as_ref()
        .ok_or(CryptoError::InvalidKey {
            what: "ephemeral X25519 public key",
        })?;
    Ok(Sha256::digest(device_envelope_hash_input(
        eph.as_slice(),
        envelope.nonce.as_slice(),
        envelope.ciphertext.as_slice(),
    ))
    .into())
}

/// Mutual pairing code v2 (protocol 1.11): 6 groups of 5 digits from
/// `SHA-256(device_pairing_code_v2_input(..))` over the keys of **both**
/// devices. The new and the approving device must show the same code.
pub fn pairing_code_v2(
    request_id: DeviceRequestId,
    new_device_id: DeviceId,
    new_device_keys: &DevicePublicKeys,
    approver_device_id: DeviceId,
    approver_keys: &DevicePublicKeys,
) -> VerificationCode {
    let h: [u8; 32] = Sha256::digest(device_pairing_code_v2_input(
        request_id,
        new_device_id,
        &new_device_keys.encryption,
        &new_device_keys.signing,
        approver_device_id,
        &approver_keys.encryption,
        &approver_keys.signing,
    ))
    .into();
    VerificationCode::from_hash(&h)
}

/// Device verification code ("safety number", ADR-0004): 6 groups of 5
/// decimal digits. Not secret; both devices display it and the user must
/// confirm they match before an approval is signed.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct VerificationCode {
    groups: [u32; 6],
}

/// `SHA-256(device_fingerprint_input(device_id, X25519_pub, Ed25519_pub))`
/// → group `i` = big-endian u40 of bytes `[5i, 5i+5)` mod 100000.
pub fn verification_code(
    device_id: DeviceId,
    encryption_public_key: &[u8; X25519_PUBLIC_KEY_LEN],
    signing_public_key: &[u8; ED25519_PUBLIC_KEY_LEN],
) -> VerificationCode {
    let h: [u8; 32] = Sha256::digest(device_fingerprint_input(
        device_id,
        encryption_public_key,
        signing_public_key,
    ))
    .into();
    VerificationCode::from_hash(&h)
}

impl VerificationCode {
    /// Render a SHA-256 hash: group `i` = big-endian u40 of bytes
    /// `[5i, 5i+5)` mod 100000 (device and team member codes).
    pub(crate) fn from_hash(h: &[u8; 32]) -> Self {
        let mut groups = [0u32; 6];
        for (i, g) in groups.iter_mut().enumerate() {
            let v = h[5 * i..5 * i + 5]
                .iter()
                .fold(0u64, |acc, b| (acc << 8) | u64::from(*b));
            // < 100000, always fits in u32.
            *g = (v % 100_000) as u32;
        }
        Self { groups }
    }

    /// The six groups as numbers (each < 100000).
    pub fn groups(&self) -> [u32; 6] {
        self.groups
    }

    /// The six groups as zero-padded 5-digit strings.
    pub fn group_strings(&self) -> [String; 6] {
        self.groups.map(|g| format!("{g:05}"))
    }

    /// True if `input` spells this code (whitespace and `-` ignored), for
    /// UIs that let the user type the other device's code.
    pub fn matches_str(&self, input: &str) -> bool {
        let digits: String = input
            .chars()
            .filter(|c| !c.is_whitespace() && *c != '-')
            .collect();
        digits == self.group_strings().concat()
    }
}

impl fmt::Display for VerificationCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let g = self.group_strings();
        write!(f, "{} {} {} {} {} {}", g[0], g[1], g[2], g[3], g[4], g[5])
    }
}

impl fmt::Debug for VerificationCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "VerificationCode({self})")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn secret_bytes_roundtrip() {
        let k = DeviceSecretKeys::generate().unwrap();
        let bytes = k.to_secret_bytes();
        assert_eq!(bytes.len(), SECRET_ENCODED_LEN);
        let back = DeviceSecretKeys::from_secret_bytes(&bytes).unwrap();
        assert_eq!(k.public_keys(), back.public_keys());
        assert!(DeviceSecretKeys::from_secret_bytes(&bytes[1..]).is_err());
        let mut bad = bytes.to_vec();
        bad[4] = 9;
        assert!(DeviceSecretKeys::from_secret_bytes(&bad).is_err());
        let mut bad = bytes.to_vec();
        bad[0] = b'X';
        assert!(DeviceSecretKeys::from_secret_bytes(&bad).is_err());
    }

    #[test]
    fn debug_redacted() {
        let k = DeviceSecretKeys::generate().unwrap();
        assert_eq!(format!("{k:?}"), "DeviceSecretKeys(<redacted>)");
    }

    #[test]
    fn approval_sign_verify() {
        let approver = DeviceSecretKeys::generate().unwrap();
        let newdev = DeviceSecretKeys::generate().unwrap().public_keys();
        let vaults = [VaultId::new(), VaultId::new()];
        let a = DeviceApproval {
            request_id: DeviceRequestId::new(),
            approver_device_id: DeviceId::new(),
            new_device_id: DeviceId::new(),
            new_device_keys: &newdev,
            issued_at: 1_800_000_000,
            vault_ids: &vaults,
        };
        let sig = approver.sign_device_approval(&a);
        let pk = approver.public_keys().signing;
        verify_device_approval(&pk, &a, &sig).unwrap();
        let reordered = [vaults[1], vaults[0]];
        verify_device_approval(
            &pk,
            &DeviceApproval {
                vault_ids: &reordered,
                ..a
            },
            &sig,
        )
        .unwrap();
        let other = DeviceApproval {
            issued_at: a.issued_at + 1,
            ..a
        };
        assert_eq!(
            verify_device_approval(&pk, &other, &sig),
            Err(CryptoError::InvalidSignature)
        );
        assert!(verify_device_approval(&pk, &a, &sig[..63]).is_err());
        let wrong = DeviceSecretKeys::generate().unwrap().public_keys().signing;
        assert!(verify_device_approval(&wrong, &a, &sig).is_err());
    }

    #[test]
    fn verification_code_shape() {
        let k = DeviceSecretKeys::generate().unwrap().public_keys();
        let id = DeviceId::new();
        let c = k.verification_code(id);
        let s = c.to_string();
        assert_eq!(s.len(), 6 * 5 + 5);
        assert!(s
            .split(' ')
            .all(|g| g.len() == 5 && g.bytes().all(|b| b.is_ascii_digit())));
        assert!(c.matches_str(&s));
        assert!(c.matches_str(&s.replace(' ', "-")));
        assert!(!c.matches_str("00000 00000"));
        assert_ne!(c, k.verification_code(DeviceId::new()));
    }

    #[test]
    fn public_key_parsing() {
        let k = DeviceSecretKeys::generate().unwrap().public_keys();
        assert_eq!(
            DevicePublicKeys::from_slices(&k.encryption, &k.signing).unwrap(),
            k
        );
        assert!(DevicePublicKeys::from_slices(&k.encryption[..31], &k.signing).is_err());
        assert!(DevicePublicKeys::from_slices(&k.encryption, &[0u8; 31]).is_err());
        // Identity point (weak key) is rejected.
        let mut identity = [0u8; 32];
        identity[0] = 1;
        assert!(DevicePublicKeys::from_slices(&k.encryption, &identity).is_err());
    }

    #[test]
    fn all_zero_shared_secret_rejected() {
        let k = DeviceSecretKeys::generate().unwrap();
        assert_eq!(
            k.diffie_hellman(&[0u8; 32]).err(),
            Some(CryptoError::WeakKeyAgreement)
        );
        // Another low-order point (order 8 / 4 points also yield zero).
        let mut one = [0u8; 32];
        one[0] = 1;
        assert_eq!(
            k.diffie_hellman(&one).err(),
            Some(CryptoError::WeakKeyAgreement)
        );
    }
}

#[cfg(test)]
mod login_proof_tests {
    use super::*;

    #[test]
    fn login_proof_roundtrip_and_binding() {
        let keys = DeviceSecretKeys::generate().unwrap();
        let pk = keys.public_keys().signing_bytes();
        let pk: [u8; 32] = pk.as_slice().try_into().unwrap();
        let id = DeviceId::new();
        let nonce = [7u8; 32];
        let sig = keys.sign_device_login(id, 1_700_000_000, &nonce);
        verify_device_login(&pk, id, 1_700_000_000, &nonce, &sig).unwrap();
        // Bound to device id, time and nonce.
        assert!(verify_device_login(&pk, DeviceId::new(), 1_700_000_000, &nonce, &sig).is_err());
        assert!(verify_device_login(&pk, id, 1_700_000_001, &nonce, &sig).is_err());
        assert!(verify_device_login(&pk, id, 1_700_000_000, &[8u8; 32], &sig).is_err());
        // Another device's key cannot produce it.
        let other = DeviceSecretKeys::generate().unwrap();
        let sig2 = other.sign_device_login(id, 1_700_000_000, &nonce);
        assert!(verify_device_login(&pk, id, 1_700_000_000, &nonce, &sig2).is_err());
    }
}

#[cfg(test)]
mod request_proof_tests {
    use super::*;

    #[test]
    fn request_proof_binds_every_field() {
        let keys = DeviceSecretKeys::generate().unwrap();
        let pk: [u8; 32] = keys
            .public_keys()
            .signing_bytes()
            .as_slice()
            .try_into()
            .unwrap();
        let id = DeviceId::new();
        let body = request_body_sha256(b"{\"x\":1}");
        let nonce = [3u8; 32];
        let path = "/v1/sync/changes?vault_id=abc&after=0";
        let sig = keys.sign_request(id, "GET", path, &body, 1_700_000_000, &nonce);
        verify_request_proof(&pk, id, "GET", path, &body, 1_700_000_000, &nonce, &sig).unwrap();
        for (m, p, b, t) in [
            ("POST", path, body, 1_700_000_000),
            (
                "GET",
                "/v1/sync/changes?vault_id=abc&after=1",
                body,
                1_700_000_000,
            ),
            ("GET", path, request_body_sha256(b""), 1_700_000_000),
            ("GET", path, body, 1_700_000_001),
        ] {
            assert!(verify_request_proof(&pk, id, m, p, &b, t, &nonce, &sig).is_err());
        }
        assert!(verify_request_proof(
            &pk,
            DeviceId::new(),
            "GET",
            path,
            &body,
            1_700_000_000,
            &nonce,
            &sig
        )
        .is_err());
    }
}

#[cfg(test)]
mod key_trust_tests {
    use super::*;

    fn entry(vault: VaultId, c: u8, e: u8) -> ApprovedVaultKey {
        ApprovedVaultKey {
            vault_id: vault,
            commitment: [c; 32],
            envelope_sha256: [e; 32],
        }
    }

    #[test]
    fn approval_v2_verifies_only_with_the_pinned_key_and_exact_fields() {
        let approver = DeviceSecretKeys::generate().unwrap();
        let new = DeviceSecretKeys::generate().unwrap();
        let a_keys = approver.public_keys();
        let n_keys = new.public_keys();
        let (r, a, n) = (DeviceRequestId::new(), DeviceId::new(), DeviceId::new());
        let vaults = [entry(VaultId::new(), 1, 2)];
        let approval = DeviceApprovalV2 {
            request_id: r,
            approver_device_id: a,
            approver_keys: &a_keys,
            new_device_id: n,
            new_device_keys: &n_keys,
            issued_at: 5,
            vaults: &vaults,
        };
        let sig = approver.sign_device_approval_v2(&approval);
        verify_device_approval_v2(&a_keys.signing, &approval, &sig).unwrap();
        // Another key (e.g. one a server substituted) does not verify.
        let other = DeviceSecretKeys::generate().unwrap().public_keys();
        assert!(verify_device_approval_v2(&other.signing, &approval, &sig).is_err());
        // A different commitment, envelope hash or vault does not verify.
        for changed in [
            entry(vaults[0].vault_id, 9, 2),
            entry(vaults[0].vault_id, 1, 9),
            entry(VaultId::new(), 1, 2),
        ] {
            let v = [changed];
            let tampered = DeviceApprovalV2 {
                vaults: &v,
                ..approval
            };
            assert!(verify_device_approval_v2(&a_keys.signing, &tampered, &sig).is_err());
        }
        // Duplicate or empty vault lists are refused.
        let dup = [vaults[0], vaults[0]];
        assert!(verify_device_approval_v2(
            &a_keys.signing,
            &DeviceApprovalV2 {
                vaults: &dup,
                ..approval
            },
            &sig
        )
        .is_err());
        assert!(verify_device_approval_v2(
            &a_keys.signing,
            &DeviceApprovalV2 {
                vaults: &[],
                ..approval
            },
            &sig
        )
        .is_err());
    }

    #[test]
    fn pairing_code_v2_depends_on_both_devices() {
        let a = DeviceSecretKeys::generate().unwrap().public_keys();
        let n = DeviceSecretKeys::generate().unwrap().public_keys();
        let x = DeviceSecretKeys::generate().unwrap().public_keys();
        let (r, ai, ni) = (DeviceRequestId::new(), DeviceId::new(), DeviceId::new());
        let code = pairing_code_v2(r, ni, &n, ai, &a);
        assert_eq!(code, pairing_code_v2(r, ni, &n, ai, &a));
        // A substituted approver key or new-device key changes the code.
        assert_ne!(code, pairing_code_v2(r, ni, &n, ai, &x));
        assert_ne!(code, pairing_code_v2(r, ni, &x, ai, &a));
        assert_ne!(
            code,
            pairing_code_v2(DeviceRequestId::new(), ni, &n, ai, &a)
        );
        // It is not the plain v1 device code.
        assert_ne!(code, n.verification_code(ni));
    }

    #[test]
    fn key_commitment_record_roundtrip() {
        let keys = DeviceSecretKeys::generate().unwrap();
        let pk = keys.public_keys().signing;
        let (v, d) = (VaultId::new(), DeviceId::new());
        let sig = keys.sign_key_commitment(v, &[4; 32], d, 9);
        verify_key_commitment(&pk, v, &[4; 32], d, 9, &sig).unwrap();
        assert!(verify_key_commitment(&pk, v, &[5; 32], d, 9, &sig).is_err());
        assert!(verify_key_commitment(&pk, v, &[4; 32], d, 10, &sig).is_err());
    }
}
