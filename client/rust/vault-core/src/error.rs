//! Error type of the vault core. Messages are safe to show to users and to
//! log: they never contain passphrases, keys, words or ciphertext.

use cc_crypto_core::CryptoError;
use cc_platform_core::{OsAuthError, SecureStoreError};
use cc_protocol::ObjectId;

/// Minimum vault passphrase length in Unicode scalar values (after NFC),
/// enforced when a passphrase is *set* (create / change), not on unlock.
pub const MIN_PASSPHRASE_CHARS: usize = 8;

/// Everything that can go wrong in `cc-vault-core`.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum VaultError {
    /// The passphrase does not open the password envelope (or the envelope
    /// was tampered with — indistinguishable by design).
    #[error("wrong vault passphrase")]
    WrongPassphrase,

    /// The Recovery Key does not open the recovery envelope.
    #[error("wrong recovery key")]
    WrongRecoveryKey,

    /// This device's keys cannot open the device envelope (not trusted for
    /// the vault, envelope for another device, or keys were replaced).
    #[error("this device is not authorized for the vault")]
    DeviceNotAuthorized,

    /// New passphrase shorter than [`MIN_PASSPHRASE_CHARS`].
    #[error("passphrase must have at least {MIN_PASSPHRASE_CHARS} characters")]
    WeakPassphrase,

    /// The recovery QR code belongs to a different vault.
    #[error("the recovery code belongs to a different vault")]
    RecoveryKeyForOtherVault,

    /// An envelope / object / request refers to another vault.
    #[error("vault id mismatch")]
    VaultMismatch,

    /// A device-approval precondition failed (request not pending, expired,
    /// device revoked, vault not requested, …).
    #[error("device approval: {0}")]
    Approval(&'static str),

    /// Device name empty, too long or containing control characters.
    #[error("invalid device name")]
    InvalidDeviceName,

    /// The device identity blob in the secure store cannot be decoded.
    #[error("stored device identity is corrupted")]
    CorruptedIdentity,

    /// A backup object failed its trial decryption.
    #[error("backup object {object_id} failed verification: {source}")]
    BackupObject {
        object_id: ObjectId,
        source: CryptoError,
    },

    /// Backup (de)serialization failed.
    #[error("backup is not valid JSON of the expected shape")]
    BackupEncoding,

    /// Lower-level cryptographic failure.
    #[error(transparent)]
    Crypto(#[from] CryptoError),

    /// Secure store failure.
    #[error(transparent)]
    SecureStore(#[from] SecureStoreError),

    /// OS / biometric authentication did not succeed (or is unsupported —
    /// fall back to the passphrase).
    #[error("OS authentication: {0}")]
    OsAuth(#[from] OsAuthError),

    /// No device identity is stored for the profile.
    #[error("no device identity for this profile")]
    NoDeviceIdentity,

    /// Invalid profile id for secure-store naming.
    #[error("invalid profile id")]
    InvalidProfileId,
}

/// Result alias for this crate.
pub type Result<T, E = VaultError> = std::result::Result<T, E>;
