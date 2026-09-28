//! Error type of the crypto core.
//!
//! Errors never carry key material, plaintext or ciphertext — only static
//! descriptions and non-secret identifiers/lengths.

use cc_models::KekClass;
use cc_protocol::envelopes::{EnvelopeValidationError, RecipientType};

/// Everything that can go wrong in `cc-crypto-core`.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum CryptoError {
    /// AEAD authentication failed: wrong key (passphrase, recovery key,
    /// device key, KEK) *or* tampered ciphertext / nonce / associated data.
    /// The two cases are deliberately indistinguishable.
    #[error("decryption failed (wrong key or tampered data)")]
    Decrypt,

    /// The operating system's random number generator failed.
    #[error("OS random number generator failed")]
    Rng,

    /// A received envelope failed structural validation
    /// ([`cc_protocol::envelopes::NewEnvelope::validate`]).
    #[error("invalid envelope: {0}")]
    InvalidEnvelope(#[from] EnvelopeValidationError),

    /// The envelope is for a different recipient type than the operation.
    #[error("envelope recipient type {actual:?} does not match expected {expected:?}")]
    WrongRecipientType {
        expected: RecipientType,
        actual: RecipientType,
    },

    /// A device envelope addressed to a different device.
    #[error("envelope is addressed to a different device")]
    WrongRecipientDevice,

    /// The envelope, object or recovery payload belongs to another vault.
    #[error("vault id mismatch")]
    VaultMismatch,

    /// Argon2id parameters outside the client-accepted range
    /// (below the server floor or above the client ceiling).
    #[error("password KDF parameters out of range: {0}")]
    KdfParams(&'static str),

    /// Argon2id itself failed (e.g. could not allocate its memory).
    #[error("password KDF failed")]
    Kdf,

    /// Passphrase is empty after normalization.
    #[error("passphrase must not be empty")]
    EmptyPassphrase,

    /// The X25519 exchange produced the all-zero shared secret (low-order
    /// public key). Rejected per ADR-0002.
    #[error("key agreement produced a non-contributory (all-zero) shared secret")]
    WeakKeyAgreement,

    /// A key, nonce, signature or public key has the wrong length or is not
    /// a valid curve point.
    #[error("invalid {what}")]
    InvalidKey { what: &'static str },

    /// Encrypted body is structurally malformed (lengths, nonce sizes).
    #[error("malformed encrypted body: {0}")]
    MalformedBody(&'static str),

    /// The object body uses a format this client does not implement.
    #[error("unsupported object format {0}")]
    UnsupportedFormat(u16),

    /// Decrypted plaintext is not a well-formed padded payload.
    #[error("malformed plaintext: {0}")]
    MalformedPlaintext(&'static str),

    /// Payload (de)serialization failed. The message never contains payload
    /// content.
    #[error("payload serialization failed")]
    Serialization,

    /// Padded object would exceed the protocol's object size limit.
    #[error("object too large ({size} bytes, max {max})")]
    ObjectTooLarge { size: usize, max: usize },

    /// Revision must be ≥ 1 (the revision the server will assign).
    #[error("invalid revision {0}")]
    InvalidRevision(i64),

    /// The object id inside the payload differs from the object id it is
    /// stored under.
    #[error("payload object id does not match the storage object id")]
    ObjectIdMismatch,

    /// Class-confusion defence (ADR-0002): the DEK was unwrapped by a KEK of
    /// a different class than the decrypted payload's kind requires.
    #[error(
        "KEK class mismatch: unwrapped with {unwrapped_with:?}, payload requires {required:?}"
    )]
    KekClassMismatch {
        unwrapped_with: KekClass,
        required: KekClass,
    },

    /// Ed25519 signature verification failed.
    #[error("invalid signature")]
    InvalidSignature,

    /// Recovery mnemonic could not be decoded.
    #[error("invalid recovery phrase: {0}")]
    Mnemonic(MnemonicError),

    /// Recovery QR payload could not be parsed.
    #[error("invalid recovery QR payload: {0}")]
    RecoveryPayload(&'static str),

    /// Encrypted backup container is malformed or was modified.
    #[error("invalid backup: {0}")]
    Backup(&'static str),

    /// Stored secret-key blob could not be decoded.
    #[error("invalid secret key encoding: {0}")]
    SecretKeyEncoding(&'static str),
}

/// Why a recovery mnemonic was rejected. Positions are 1-based so they can be
/// shown to the user directly ("word 7 is not in the word list").
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum MnemonicError {
    #[error("expected 24 words, got {0}")]
    WordCount(usize),
    #[error("word {0} is not in the BIP-39 English word list")]
    UnknownWord(usize),
    #[error("checksum mismatch (a word is wrong or words are out of order)")]
    Checksum,
}

impl From<MnemonicError> for CryptoError {
    fn from(e: MnemonicError) -> Self {
        CryptoError::Mnemonic(e)
    }
}

/// Result alias for this crate.
pub type Result<T, E = CryptoError> = std::result::Result<T, E>;
