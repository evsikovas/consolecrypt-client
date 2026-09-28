//! Vault key envelopes: the Vault Root Key (VRK) encrypted for a recipient.
//! The server stores envelopes as opaque ciphertext plus the non-secret
//! metadata below. Byte-level formats are specified in ADR-0002.

use crate::bytes::Bytes;
use crate::ids::{DeviceId, EnvelopeId, VaultId};
use crate::limits;
use crate::Timestamp;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RecipientType {
    /// VRK encrypted under a key derived from the Vault passphrase (Argon2id).
    Password,
    /// VRK encrypted under a key derived from the Recovery Key (HKDF).
    Recovery,
    /// VRK encrypted to a device's X25519 key.
    Device,
    /// Team Vault foundation: VRK encrypted to a user's key. Not used in MVP.
    User,
    /// Reserved for organization recovery. Not used in MVP.
    OrganizationFuture,
}

/// What is inside the envelope.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EnvelopeKind {
    /// 32-byte Vault Root Key, format v1.
    VrkV1,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EnvelopeAlgorithm {
    /// Argon2id(passphrase) → XChaCha20-Poly1305. Password envelopes.
    Argon2idXchacha20poly1305V1,
    /// HKDF-SHA256(recovery key) → XChaCha20-Poly1305. Recovery envelopes.
    HkdfSha256Xchacha20poly1305V1,
    /// Ephemeral X25519 + HKDF-SHA256 → XChaCha20-Poly1305. Device/user envelopes.
    X25519HkdfSha256Xchacha20poly1305V1,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum KdfAlgorithm {
    Argon2id,
}

/// Password-KDF parameters. Not secret; the client needs them (and the salt)
/// before it can attempt to unlock.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KdfParams {
    pub algorithm: KdfAlgorithm,
    pub salt: Bytes,
    pub memory_kib: u32,
    pub iterations: u32,
    pub parallelism: u32,
}

impl KdfParams {
    /// Floor the server enforces so a hostile or buggy client cannot upload a
    /// trivially brute-forceable password envelope (OWASP 2023 minimum-ish).
    pub const MIN_MEMORY_KIB: u32 = 19 * 1024;
    pub const MIN_ITERATIONS: u32 = 2;
    /// Ceiling so a hostile server cannot DoS clients with absurd params.
    pub const MAX_MEMORY_KIB: u32 = 4 * 1024 * 1024;
    pub const MAX_ITERATIONS: u32 = 64;
    pub const MAX_PARALLELISM: u32 = 16;
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EnvelopeMetadata {
    pub algorithm: EnvelopeAlgorithm,
    /// Present iff `algorithm` is Argon2id-based.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kdf: Option<KdfParams>,
    /// Sender's ephemeral X25519 public key; present iff X25519-based.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ephemeral_public_key: Option<Bytes>,
}

/// Envelope as uploaded by a client.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NewEnvelope {
    pub recipient_type: RecipientType,
    /// `DeviceId`/`UserId` UUID for device/user recipients; `None` otherwise.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recipient_id: Option<Uuid>,
    pub kind: EnvelopeKind,
    pub metadata: EnvelopeMetadata,
    pub ciphertext: Bytes,
    pub nonce: Bytes,
}

/// Envelope as stored/returned by the server.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KeyEnvelope {
    pub envelope_id: EnvelopeId,
    pub vault_id: VaultId,
    pub recipient_type: RecipientType,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recipient_id: Option<Uuid>,
    pub kind: EnvelopeKind,
    pub metadata: EnvelopeMetadata,
    pub ciphertext: Bytes,
    pub nonce: Bytes,
    pub created_at: Timestamp,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created_by_device_id: Option<DeviceId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub revoked_at: Option<Timestamp>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum EnvelopeValidationError {
    #[error("algorithm does not match recipient type")]
    AlgorithmMismatch,
    #[error("recipient_id required for device/user recipients and forbidden otherwise")]
    RecipientId,
    #[error("kdf params missing, unexpected, or outside allowed bounds")]
    Kdf,
    #[error("ephemeral public key missing, unexpected, or wrong length")]
    EphemeralKey,
    #[error("nonce must be 24 bytes")]
    Nonce,
    #[error("ciphertext has unexpected length")]
    Ciphertext,
    #[error("recipient type not accepted in this protocol version")]
    UnsupportedRecipient,
}

impl NewEnvelope {
    /// Structural validation. Performed by the server on upload and by the
    /// client on download (defence against a hostile server).
    pub fn validate(&self) -> Result<(), EnvelopeValidationError> {
        use EnvelopeAlgorithm as A;
        use EnvelopeValidationError as E;
        use RecipientType as R;

        let expected_alg = match self.recipient_type {
            R::Password => A::Argon2idXchacha20poly1305V1,
            R::Recovery => A::HkdfSha256Xchacha20poly1305V1,
            R::Device | R::User => A::X25519HkdfSha256Xchacha20poly1305V1,
            R::OrganizationFuture => return Err(E::UnsupportedRecipient),
        };
        if self.metadata.algorithm != expected_alg {
            return Err(E::AlgorithmMismatch);
        }
        let needs_id = matches!(self.recipient_type, R::Device | R::User);
        if needs_id != self.recipient_id.is_some() {
            return Err(E::RecipientId);
        }
        match (&self.metadata.kdf, expected_alg) {
            (Some(k), A::Argon2idXchacha20poly1305V1) => {
                if k.salt.len() != limits::ARGON2_SALT_LEN
                    || k.memory_kib < KdfParams::MIN_MEMORY_KIB
                    || k.memory_kib > KdfParams::MAX_MEMORY_KIB
                    || k.iterations < KdfParams::MIN_ITERATIONS
                    || k.iterations > KdfParams::MAX_ITERATIONS
                    || k.parallelism == 0
                    || k.parallelism > KdfParams::MAX_PARALLELISM
                {
                    return Err(E::Kdf);
                }
            }
            (None, A::Argon2idXchacha20poly1305V1) | (Some(_), _) => return Err(E::Kdf),
            (None, _) => {}
        }
        match (&self.metadata.ephemeral_public_key, expected_alg) {
            (Some(k), A::X25519HkdfSha256Xchacha20poly1305V1)
                if k.len() == limits::X25519_PUBLIC_KEY_LEN => {}
            (None, A::Argon2idXchacha20poly1305V1 | A::HkdfSha256Xchacha20poly1305V1) => {}
            _ => return Err(E::EphemeralKey),
        }
        if self.nonce.len() != limits::NONCE_LEN {
            return Err(E::Nonce);
        }
        if self.ciphertext.len() != limits::ENVELOPE_CIPHERTEXT_LEN {
            return Err(E::Ciphertext);
        }
        Ok(())
    }
}

impl KeyEnvelope {
    /// View as the uploaded shape, e.g. to re-run [`NewEnvelope::validate`].
    pub fn to_new(&self) -> NewEnvelope {
        NewEnvelope {
            recipient_type: self.recipient_type,
            recipient_id: self.recipient_id,
            kind: self.kind,
            metadata: self.metadata.clone(),
            ciphertext: self.ciphertext.clone(),
            nonce: self.nonce.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn password_env() -> NewEnvelope {
        NewEnvelope {
            recipient_type: RecipientType::Password,
            recipient_id: None,
            kind: EnvelopeKind::VrkV1,
            metadata: EnvelopeMetadata {
                algorithm: EnvelopeAlgorithm::Argon2idXchacha20poly1305V1,
                kdf: Some(KdfParams {
                    algorithm: KdfAlgorithm::Argon2id,
                    salt: Bytes::new(vec![7; 16]),
                    memory_kib: 64 * 1024,
                    iterations: 3,
                    parallelism: 1,
                }),
                ephemeral_public_key: None,
            },
            ciphertext: Bytes::new(vec![0; 48]),
            nonce: Bytes::new(vec![0; 24]),
        }
    }

    fn device_env() -> NewEnvelope {
        NewEnvelope {
            recipient_type: RecipientType::Device,
            recipient_id: Some(DeviceId::new().0),
            kind: EnvelopeKind::VrkV1,
            metadata: EnvelopeMetadata {
                algorithm: EnvelopeAlgorithm::X25519HkdfSha256Xchacha20poly1305V1,
                kdf: None,
                ephemeral_public_key: Some(Bytes::new(vec![9; 32])),
            },
            ciphertext: Bytes::new(vec![0; 48]),
            nonce: Bytes::new(vec![0; 24]),
        }
    }

    #[test]
    fn valid_envelopes_pass() {
        password_env().validate().unwrap();
        device_env().validate().unwrap();
    }

    #[test]
    fn weak_kdf_rejected() {
        let mut e = password_env();
        e.metadata.kdf.as_mut().unwrap().memory_kib = 1024;
        assert_eq!(e.validate(), Err(EnvelopeValidationError::Kdf));
    }

    #[test]
    fn device_envelope_needs_recipient_and_epk() {
        let mut e = device_env();
        e.recipient_id = None;
        assert_eq!(e.validate(), Err(EnvelopeValidationError::RecipientId));
        let mut e = device_env();
        e.metadata.ephemeral_public_key = None;
        assert_eq!(e.validate(), Err(EnvelopeValidationError::EphemeralKey));
    }

    #[test]
    fn algorithm_must_match_recipient() {
        let mut e = password_env();
        e.metadata.algorithm = EnvelopeAlgorithm::HkdfSha256Xchacha20poly1305V1;
        assert_eq!(
            e.validate(),
            Err(EnvelopeValidationError::AlgorithmMismatch)
        );
    }

    #[test]
    fn json_shape_is_stable() {
        let v = serde_json::to_value(device_env()).unwrap();
        assert_eq!(v["recipient_type"], "device");
        assert_eq!(v["kind"], "vrk_v1");
        assert_eq!(
            v["metadata"]["algorithm"],
            "x25519_hkdf_sha256_xchacha20poly1305_v1"
        );
        assert!(v["metadata"].get("kdf").is_none());
    }
}
