//! Profiles (ADR-0106), vault lifecycle and recovery (ADR-0002/0004).
//!
//! JSON results: `ProfileInfo`, `CreatedProfile`, `SyncedAccountDto`,
//! `RemoteVaultDto`, `RecoveryKitDto` (the only secret ever returned — shown
//! once during onboarding / regeneration).

use crate::api::error::BridgeError;
use crate::state::{opt_secret_string, secret_string, to_json, with_core};
use cc_app_core::AccountMode;

// ---- profiles ----------------------------------------------------------------------

/// All profiles (`Vec<ProfileInfo>`; details filled for the open one).
pub async fn profiles_list() -> Result<String, BridgeError> {
    with_core(move |c| async move { to_json(&c.list_profiles().await?) }).await
}

/// The profile remembered as active (to reopen at start-up).
pub async fn profiles_last_active_id() -> Result<Option<String>, BridgeError> {
    with_core(move |c| async move { Ok(c.last_active_profile_id().await?) }).await
}

/// Open (switch to) a profile; its vault starts locked → `ProfileInfo`.
pub async fn profiles_open(profile_id: String) -> Result<String, BridgeError> {
    with_core(move |c| async move { to_json(&c.open_profile(profile_id).await?) }).await
}

/// Close the open profile (locks the vault).
pub async fn profiles_close() -> Result<(), BridgeError> {
    with_core(move |c| async move { Ok(c.close_profile().await?) }).await
}

pub async fn profiles_rename(
    profile_id: String,
    display_name: String,
) -> Result<String, BridgeError> {
    with_core(move |c| async move { to_json(&c.rename_profile(profile_id, display_name).await?) })
        .await
}

/// Delete a profile's database, index entry and keychain items.
pub async fn profiles_remove(profile_id: String) -> Result<(), BridgeError> {
    with_core(move |c| async move { Ok(c.remove_profile(profile_id).await?) }).await
}

/// New Local profile with its vault (active, unlocked) → `CreatedProfile`
/// (incl. the Recovery Kit to show once).
pub async fn profiles_create_local(
    display_name: String,
    passphrase: Vec<u8>,
) -> Result<String, BridgeError> {
    let passphrase = secret_string("passphrase", passphrase)?;
    with_core(
        move |c| async move { to_json(&c.create_local_profile(display_name, passphrase).await?) },
    )
    .await
}

/// New Synced profile: register (`register = true`) or log in →
/// `SyncedAccountDto` (`vaults` empty → create one, else join).
pub async fn profiles_create_synced(
    display_name: String,
    server_url: String,
    email: String,
    password: Vec<u8>,
    register: bool,
) -> Result<String, BridgeError> {
    let password = secret_string("account_password", password)?;
    let mode = if register {
        AccountMode::Register
    } else {
        AccountMode::Login
    };
    with_core(move |c| async move {
        to_json(
            &c.create_synced_profile(display_name, server_url, email, password, mode)
                .await?,
        )
    })
    .await
}

/// Vaults of the signed-in account (`Vec<RemoteVaultDto>`).
pub async fn profiles_remote_vaults() -> Result<String, BridgeError> {
    with_core(move |c| async move { to_json(&c.list_remote_vaults().await?) }).await
}

// ---- vault -------------------------------------------------------------------------

/// Create the vault of a synced profile that has none → `RecoveryKitDto`.
pub async fn vault_create(passphrase: Vec<u8>) -> Result<String, BridgeError> {
    let passphrase = secret_string("passphrase", passphrase)?;
    with_core(move |c| async move { to_json(&c.create_vault(passphrase).await?) }).await
}

/// Join an existing vault on this device with the passphrase → `ProfileInfo`.
pub async fn vault_join_with_passphrase(
    vault_id: String,
    passphrase: Vec<u8>,
) -> Result<String, BridgeError> {
    let passphrase = secret_string("passphrase", passphrase)?;
    with_core(move |c| async move {
        to_json(&c.join_vault_with_passphrase(vault_id, passphrase).await?)
    })
    .await
}

/// Join with the Recovery Key (24 words / QR text) → `ProfileInfo`.
pub async fn vault_join_with_recovery_key(
    vault_id: String,
    recovery_input: Vec<u8>,
    new_passphrase: Option<Vec<u8>>,
) -> Result<String, BridgeError> {
    let recovery_input = secret_string("recovery_input", recovery_input)?;
    let new_passphrase = opt_secret_string("new_passphrase", new_passphrase)?;
    with_core(move |c| async move {
        to_json(
            &c.join_vault_with_recovery_key(vault_id, recovery_input, new_passphrase)
                .await?,
        )
    })
    .await
}

pub async fn vault_unlock_with_passphrase(passphrase: Vec<u8>) -> Result<(), BridgeError> {
    let passphrase = secret_string("passphrase", passphrase)?;
    with_core(move |c| async move { Ok(c.unlock_with_passphrase(passphrase).await?) }).await
}

/// OS / biometric unlock via the device envelope. Needs a fresh grant of
/// the OS authenticator (see [`vault_unlock_with_device_attested`]); without
/// one it fails with `os_auth_failed`.
pub async fn vault_unlock_with_device() -> Result<(), BridgeError> {
    with_core(move |c| async move { Ok(c.unlock_with_device().await?) }).await
}

/// Unlock with the device envelope after the UI's OS prompt (Touch ID)
/// succeeded **just now**: grants the core's authenticator once, then
/// unlocks (the device key never leaves the core).
pub async fn vault_unlock_with_device_attested() -> Result<(), BridgeError> {
    let auth = crate::api::app::os_auth();
    auth.grant_once();
    let r = with_core(move |c| async move { Ok(c.unlock_with_device().await?) }).await;
    auth.revoke();
    r
}

/// Forgot passphrase on a trusted device after the UI's OS prompt
/// succeeded just now (see [`vault_unlock_with_device_attested`]).
pub async fn vault_reset_passphrase_with_device_attested(next: Vec<u8>) -> Result<(), BridgeError> {
    let next = secret_string("new_passphrase", next)?;
    let auth = crate::api::app::os_auth();
    auth.grant_once();
    let r =
        with_core(move |c| async move { Ok(c.reset_passphrase_with_device(next).await?) }).await;
    auth.revoke();
    r
}

/// Whether "Unlock with Touch ID / Windows Hello" can be offered →
/// `DeviceUnlockDto` (`available`, `kind`, `has_device_envelope`,
/// `not_enrolled`).
pub async fn vault_device_unlock_info() -> Result<String, BridgeError> {
    with_core(move |c| async move { to_json(&c.device_unlock_info().await?) }).await
}

/// Change this profile's local opt-in. Enabling must follow a successful
/// native OS prompt just now; the core consumes the grant and verifies the
/// device envelope. Disabling needs an unlocked vault but no OS prompt.
pub async fn vault_set_device_unlock_enabled_attested(enabled: bool) -> Result<(), BridgeError> {
    let auth = crate::api::app::os_auth();
    if enabled {
        auth.grant_once();
    }
    let r =
        with_core(move |c| async move { Ok(c.set_device_unlock_enabled(enabled).await?) }).await;
    auth.revoke();
    r
}

pub async fn vault_unlock_with_recovery_key(recovery_input: Vec<u8>) -> Result<(), BridgeError> {
    let recovery_input = secret_string("recovery_input", recovery_input)?;
    with_core(move |c| async move { Ok(c.unlock_with_recovery_key(recovery_input).await?) }).await
}

pub async fn vault_verify_passphrase(passphrase: Vec<u8>) -> Result<bool, BridgeError> {
    let passphrase = secret_string("passphrase", passphrase)?;
    with_core(move |c| async move { Ok(c.verify_passphrase(passphrase).await?) }).await
}

pub async fn vault_lock() -> Result<(), BridgeError> {
    with_core(move |c| async move { Ok(c.lock().await?) }).await
}

pub async fn vault_is_unlocked() -> Result<bool, BridgeError> {
    with_core(move |c| async move { Ok(c.is_unlocked().await) }).await
}

/// Whether the mandatory Recovery Kit check of the open profile is still
/// pending — persisted by the core, so it survives restarts (also reported
/// as `ProfileInfo.recovery_kit_pending`).
pub async fn vault_has_pending_recovery_kit() -> Result<bool, BridgeError> {
    with_core(move |c| async move {
        match c.recovery_kit_check_pending().await {
            Ok(p) => Ok(p),
            Err(cc_app_core::AppError::NoActiveProfile) => Ok(false),
            Err(e) => Err(e.into()),
        }
    })
    .await
}

/// Whether the kit itself is still in memory (the 3-word check can run);
/// `false` after a restart — offer to regenerate it then.
pub async fn vault_recovery_kit_available() -> Result<bool, BridgeError> {
    with_core(move |c| async move {
        match c.pending_recovery_kit().await {
            Ok(kit) => Ok(kit.is_some()),
            Err(cc_app_core::AppError::NoActiveProfile) => Ok(false),
            Err(e) => Err(e.into()),
        }
    })
    .await
}

/// The user passed the 3-word check: forget the pending kit.
pub async fn vault_acknowledge_recovery_kit() -> Result<(), BridgeError> {
    with_core(move |c| async move { Ok(c.recovery_kit_acknowledge().await?) }).await
}

/// New Recovery Key (the old one stops working) → `RecoveryKitDto`.
pub async fn vault_regenerate_recovery_kit() -> Result<String, BridgeError> {
    with_core(move |c| async move { to_json(&c.regenerate_recovery_kit().await?) }).await
}

pub async fn vault_change_passphrase(current: Vec<u8>, next: Vec<u8>) -> Result<(), BridgeError> {
    let current = secret_string("current_passphrase", current)?;
    let next = secret_string("new_passphrase", next)?;
    with_core(move |c| async move { Ok(c.change_passphrase(current, next).await?) }).await
}

/// Forgot passphrase on a trusted device (OS authentication).
pub async fn vault_reset_passphrase_with_device(next: Vec<u8>) -> Result<(), BridgeError> {
    let next = secret_string("new_passphrase", next)?;
    with_core(move |c| async move { Ok(c.reset_passphrase_with_device(next).await?) }).await
}

/// Forgot passphrase, Recovery Key: unlock + set a new passphrase.
pub async fn vault_reset_passphrase_with_recovery_key(
    recovery_input: Vec<u8>,
    next: Vec<u8>,
) -> Result<(), BridgeError> {
    let recovery_input = secret_string("recovery_input", recovery_input)?;
    let next = secret_string("new_passphrase", next)?;
    with_core(move |c| async move {
        Ok(c.reset_passphrase_with_recovery_key(recovery_input, next)
            .await?)
    })
    .await
}
