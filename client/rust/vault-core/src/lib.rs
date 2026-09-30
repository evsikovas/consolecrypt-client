//! Vault lifecycle on top of `cc-crypto-core`: create vault, unlock
//! (passphrase / device envelope / recovery key), change passphrase,
//! regenerate recovery kit, device approval and attestation payloads,
//! enable-sync for local vaults, encrypted backups, and the in-memory
//! [`UnlockedVault`] state (zeroized on lock).
//!
//! Works identically for synced and local-only profiles (ADR-0106): nothing
//! here performs network I/O or depends on a server response. Synced
//! profiles send the returned protocol DTOs; local profiles persist the
//! envelopes they contain.
//!
//! | Flow | Entry point |
//! |---|---|
//! | device identity | [`DeviceIdentity::load_or_create`], [`DeviceIdentity::registration`] |
//! | create vault | [`create_vault`] → [`CreatedVault`] (+ [`RecoveryKit::start_check`]) |
//! | unlock | [`unlock_with_passphrase`], [`unlock_with_device`], [`unlock_with_os_auth`], [`unlock_with_recovery_input`] |
//! | objects | [`UnlockedVault::encrypt_object`], [`UnlockedVault::decrypt_object`] |
//! | change passphrase | [`UnlockedVault::change_passphrase`] |
//! | new recovery kit | [`UnlockedVault::regenerate_recovery_kit`] |
//! | attest this device | [`UnlockedVault::attest_device_request`] |
//! | approve a device | [`PendingApproval::new`] → code → [`ConfirmedApproval::build_request`] |
//! | enable sync | [`UnlockedVault::enable_sync_request`] |
//! | backup | [`UnlockedVault::create_backup`], [`decode_backup`], [`restore_backup_with_passphrase`] |
//! | KDF cost | [`calibrate_argon2`] |

mod approval;
mod backup;
mod calibrate;
mod create;
mod device_sharing;
mod error;
mod identity;
mod recovery_kit;
mod unlock;
mod unlocked;

pub use approval::{ConfirmedApproval, PendingApproval};
pub use backup::{
    decode_backup, encode_backup, restore_backup_with_passphrase, restore_backup_with_recovery,
    RestoredBackup, MAX_BACKUP_BYTES,
};
pub use calibrate::{
    calibrate_argon2, calibrate_argon2_with, DEFAULT_CALIBRATION_TARGET, MAX_CALIBRATED_ITERATIONS,
};
pub use create::{create_vault, CreatedVault, LocalVaultEnvelopes};
pub use error::{Result, VaultError, MIN_PASSPHRASE_CHARS};
pub use identity::DeviceIdentity;
pub use recovery_kit::{RecoveryKit, RecoveryKitCheck, RECOVERY_CHECK_WORDS};
pub use unlock::{
    parse_recovery_input, unlock_with_device, unlock_with_os_auth, unlock_with_passphrase,
    unlock_with_recovery, unlock_with_recovery_input, EnvelopeSource,
};
pub use unlocked::UnlockedVault;

/// Re-exports callers commonly need alongside this crate.
pub use cc_crypto_core::{
    Argon2Params, BackupObject, RecoveryKey, RecoveryPhrase, SecretString, VaultBackup,
    VerificationCode,
};

#[cfg(test)]
mod tests {
    /// app-core keeps these in shared state / FRB opaque handles.
    #[test]
    fn public_state_types_are_send_and_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<super::UnlockedVault>();
        assert_send_sync::<super::DeviceIdentity>();
        assert_send_sync::<super::CreatedVault>();
        assert_send_sync::<super::RecoveryKit>();
        assert_send_sync::<super::RecoveryKitCheck>();
        assert_send_sync::<super::PendingApproval>();
        assert_send_sync::<super::ConfirmedApproval>();
        assert_send_sync::<super::RestoredBackup>();
        assert_send_sync::<super::VaultError>();
    }
}
