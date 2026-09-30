//! # cc-crypto-core
//!
//! Cryptographic primitives and the v1 encrypted formats of
//! `docs/adr/ADR-0002-cryptography-and-key-hierarchy.md` (normative):
//!
//! * key hierarchy — [`Vrk`] → per-class [`Kek`]s + [`Vak`] ([`KeyHierarchy`]);
//! * VRK envelopes — password (Argon2id), recovery (HKDF from the
//!   [`RecoveryKey`]), device (ephemeral X25519 + HKDF) — produced as
//!   [`cc_protocol::envelopes::NewEnvelope`] and validated on receipt;
//! * object encryption — [`encrypt_object`] / [`decrypt_object`] with length
//!   prefix, 256 B / 4 KiB padding, fresh DEK per revision, trial KEK
//!   unwrapping and the mandatory class-confusion check;
//! * recovery key encodings — BIP-39 24 words and the QR payload;
//! * device keys — X25519 + Ed25519 ([`DeviceSecretKeys`]), approval
//!   signatures and the [`VerificationCode`] of ADR-0004;
//! * the encrypted backup container [`VaultBackup`] (`.ccbackup`, ADR-0106)
//!   with its manifest MAC.
//!
//! Pure: no I/O, no clocks, no global state. Byte layouts (labels, AAD,
//! signed messages) come exclusively from [`cc_protocol::canonical`].
//! Randomness comes exclusively from the OS CSPRNG. Fixed-input test vectors
//! for every construction live in `tests/vectors/*.json`.
//!
//! Secret-bearing types zeroize on drop and have redacted `Debug`.

mod aead;
mod backup;
mod device;
mod envelope;
mod error;
mod kdf;
mod keys;
mod object;
mod recovery;
mod rng;
pub mod sharing;
pub mod sharing_enrollment;

#[cfg(test)]
mod vectors;

pub use backup::{
    BackupObject, VaultBackup, BACKUP_FORMAT_V1, BACKUP_MANIFEST_KEY_LABEL, BACKUP_MANIFEST_PREFIX,
    MAX_BACKUP_APP_VERSION_LEN,
};
pub use device::{
    request_body_sha256, verification_code, verify_device_approval, verify_device_login,
    verify_request_proof, DeviceApproval, DevicePublicKeys, DeviceSecretKeys, VerificationCode,
};
pub use envelope::{
    open_device_envelope, open_password_envelope, open_recovery_envelope, password_envelope_params,
    seal_device_envelope, seal_password_envelope, seal_recovery_envelope,
    stored_envelope_for_vault, RECIPIENT_CODE_DEVICE, RECIPIENT_CODE_PASSWORD,
    RECIPIENT_CODE_RECOVERY, RECIPIENT_CODE_USER,
};
pub use error::{CryptoError, MnemonicError, Result};
pub use kdf::{normalize_passphrase, Argon2Params};
pub use keys::{generate_vrk, vault_access_key_verifier, Dek, Kek, KeyHierarchy, Vak, Vrk};
pub use object::{
    decrypt_object, encrypt_object, padded_len, KEK_TRIAL_ORDER, MAX_PADDED_PLAINTEXT, PAD_LARGE,
    PAD_LARGE_THRESHOLD, PAD_SMALL,
};
pub use recovery::{RecoveryKey, RecoveryPhrase, RECOVERY_QR_PREFIX, RECOVERY_WORD_COUNT};
pub use rng::{fill_random, random_below};

/// Re-exported so callers need not depend on `secrecy` just to pass a
/// passphrase.
pub use secrecy::{ExposeSecret, SecretString};
