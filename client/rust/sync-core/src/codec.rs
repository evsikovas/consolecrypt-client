//! Encryption boundary. sync-core never performs cryptography itself; the
//! caller (app-core, on top of `vault_core::UnlockedVault` + crypto-core)
//! implements [`ObjectCodec`] per vault.

use cc_models::{KekClass, ObjectPayload};
use cc_protocol::sync::EncryptedBody;
use cc_protocol::ObjectId;

/// Per-vault object encryption (ADR-0002 §Objects).
///
/// Called synchronously, often from the storage thread inside a database
/// transaction (so encryption and the write are atomic): implementations
/// must be fast and must not block on the async runtime.
pub trait ObjectCodec: Send + Sync {
    /// Encrypt for `revision` (= base_revision + 1). vault_id is bound inside.
    fn encrypt(
        &self,
        object_id: ObjectId,
        revision: i64,
        payload: &ObjectPayload,
    ) -> Result<EncryptedBody, CodecError>;
    /// Decrypt and verify KEK-class consistency (ADR-0002).
    fn decrypt(
        &self,
        object_id: ObjectId,
        revision: i64,
        body: &EncryptedBody,
    ) -> Result<ObjectPayload, CodecError>;

    /// Decrypt with the locally cached KEK class as a hint to skip trial
    /// unwrapping (ADR-0002). Optional extension; the default ignores the
    /// hint. Implementations must still verify the class after decryption.
    fn decrypt_hinted(
        &self,
        object_id: ObjectId,
        revision: i64,
        body: &EncryptedBody,
        _kek_class_hint: Option<KekClass>,
    ) -> Result<ObjectPayload, CodecError> {
        self.decrypt(object_id, revision, body)
    }
}

/// Errors reported by an [`ObjectCodec`]. Messages must never contain key
/// material or plaintext.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum CodecError {
    /// The vault is locked; keys are not available right now. Sync keeps
    /// moving ciphertext; operations that need plaintext wait.
    #[error("vault is locked")]
    Locked,
    /// AEAD verification failed (wrong key, tampered or mis-bound ciphertext).
    #[error("ciphertext failed authentication")]
    Integrity,
    /// Decrypted payload kind does not match the KEK class that unwrapped it.
    #[error("object kind does not match its key class")]
    ClassMismatch,
    /// Object format this build cannot handle.
    #[error("unsupported object format {0}")]
    UnsupportedFormat(u16),
    /// Payload (de)serialization failed.
    #[error("payload encoding: {0}")]
    Encoding(String),
    /// Anything else.
    #[error("codec: {0}")]
    Other(String),
}
