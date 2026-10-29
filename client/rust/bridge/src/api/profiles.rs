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

/// Device unlock (S5): the core asks the OS (Touch ID / Windows Hello /
/// keyring) to release the profile's unlock key, showing `reason`. Cancel
/// or failure → `os_auth_failed`; a changed enrollment →
/// `os_auth_invalidated` (unlock with the passphrase, enable again).
/// Android uses [`vault_unlock_with_device_material`].
pub async fn vault_unlock_with_device(reason: String) -> Result<(), BridgeError> {
    with_core(move |c| async move { Ok(c.unlock_with_device(reason).await?) }).await
}

/// Forgot passphrase with device unlock on: OS prompt (`reason`) → set
/// `next` as the new passphrase.
pub async fn vault_reset_passphrase_with_device(
    next: Vec<u8>,
    reason: String,
) -> Result<(), BridgeError> {
    let next = secret_string("new_passphrase", next)?;
    with_core(move |c| async move { Ok(c.reset_passphrase_with_device(next, reason).await?) }).await
}

/// Device unlock state of the open profile → `DeviceUnlockDto`
/// (`enabled`, `available`, `kind`, `has_vault`, `not_enrolled`,
/// `protection`, `external`, `reoffer`, `confirm_actions`,
/// `presence_available`).
pub async fn vault_device_unlock_info() -> Result<String, BridgeError> {
    with_core(move |c| async move { to_json(&c.device_unlock_info().await?) }).await
}

/// Switch device unlock on (the OS may prompt with `reason`) or off. Needs
/// an unlocked vault. `allow_unprotected`: the user confirmed the warning
/// for a platform without OS protection (`protection == "convenience"`).
pub async fn vault_set_device_unlock_enabled(
    enabled: bool,
    reason: String,
    allow_unprotected: bool,
) -> Result<(), BridgeError> {
    with_core(move |c| async move {
        Ok(
            c.set_device_unlock_enabled(enabled, reason, allow_unprotected)
                .await?,
        )
    })
    .await
}

/// The user declined switching device unlock on again (after migration or
/// a fingerprint change).
pub async fn vault_dismiss_device_unlock_offer() -> Result<(), BridgeError> {
    with_core(move |c| async move { Ok(c.dismiss_device_unlock_offer().await?) }).await
}

// ---- Security key unlock (FEAT-16) ----

/// Security key unlock state of the open profile → `SecurityKeyDto`
/// (`supported`, `enabled`, `pin`: `system` | `app` | null, `has_vault`,
/// `created_at_ms`).
pub async fn vault_security_key_info() -> Result<String, BridgeError> {
    with_core(move |c| async move { to_json(&c.security_key_info().await?) }).await
}

/// Switch security key unlock on (the key asks for its PIN and two
/// touches) or off. `pin`: the key PIN where `pin == "app"`. Failures are
/// `invalid_input` with field `security_key` and a `rule` such as
/// `no_device`, `pin_invalid`, `cancelled`.
pub async fn vault_set_security_key_enabled(
    enabled: bool,
    pin: Option<Vec<u8>>,
) -> Result<(), BridgeError> {
    let pin = opt_secret_string("pin", pin)?;
    with_core(move |c| async move { Ok(c.set_security_key_enabled(enabled, pin).await?) }).await
}

/// Lock screen: unlock with the security key (PIN and touch).
pub async fn vault_unlock_with_security_key(pin: Option<Vec<u8>>) -> Result<(), BridgeError> {
    let pin = opt_secret_string("pin", pin)?;
    with_core(move |c| async move { Ok(c.unlock_with_security_key(pin).await?) }).await
}

// ---- Android: the app performs the OS step (BiometricPrompt + Keystore) ----

/// The Keystore blob the app decrypts inside `BiometricPrompt` (`None` =
/// device unlock off).
pub async fn vault_device_unlock_handle() -> Result<Option<Vec<u8>>, BridgeError> {
    with_core(move |c| async move { Ok(c.device_unlock_handle().await?) }).await
}

/// Unlock with the material the app's `CryptoObject` decrypted. A wrong
/// value cannot open the vault.
pub async fn vault_unlock_with_device_material(material: Vec<u8>) -> Result<(), BridgeError> {
    with_core(move |c| async move { Ok(c.unlock_with_device_material(material).await?) }).await
}

/// Switch device unlock on with material the app generated and encrypted
/// with its Keystore key inside a successful `BiometricPrompt` (`handle`:
/// that ciphertext, `kind`: `fingerprint` | `face_id` | `device_credential`).
pub async fn vault_enable_device_unlock_with_material(
    material: Vec<u8>,
    handle: Vec<u8>,
    kind: String,
) -> Result<(), BridgeError> {
    with_core(move |c| async move {
        Ok(c.enable_device_unlock_with_material(material, handle, kind)
            .await?)
    })
    .await
}

/// The app's Keystore key was permanently invalidated (biometrics changed).
pub async fn vault_device_unlock_invalidated() -> Result<(), BridgeError> {
    with_core(move |c| async move { Ok(c.device_unlock_invalidated().await?) }).await
}

/// Android variant of [`vault_reset_passphrase_with_device`].
pub async fn vault_reset_passphrase_with_device_material(
    material: Vec<u8>,
    next: Vec<u8>,
) -> Result<(), BridgeError> {
    let next = secret_string("new_passphrase", next)?;
    with_core(move |c| async move {
        Ok(c.reset_passphrase_with_device_material(material, next)
            .await?)
    })
    .await
}

pub async fn vault_unlock_with_recovery_key(recovery_input: Vec<u8>) -> Result<(), BridgeError> {
    let recovery_input = secret_string("recovery_input", recovery_input)?;
    with_core(move |c| async move { Ok(c.unlock_with_recovery_key(recovery_input).await?) }).await
}

// ---- step-up confirmations with the OS check (0.9.2) ---------------------------

/// The OS (Touch ID / Windows Hello) releases the device unlock key after
/// its prompt with `reason`; the core checks it and returns a single-use
/// presence token for `action` on `object` (30 s). The key never leaves the
/// core. `passphrase_required` when the OS check is not offered (device
/// unlock off or unprotected, preference off, passphrase-only action).
/// Android uses [`vault_verify_user_presence_with_material`].
pub async fn vault_verify_user_presence(
    action: String,
    object: String,
    reason: String,
) -> Result<String, BridgeError> {
    with_core(move |c| async move { Ok(c.verify_user_presence(action, object, reason).await?) })
        .await
}

/// Android: [`vault_verify_user_presence`] with the material the app's
/// `BiometricPrompt` `CryptoObject` decrypted.
pub async fn vault_verify_user_presence_with_material(
    action: String,
    object: String,
    material: Vec<u8>,
) -> Result<String, BridgeError> {
    with_core(move |c| async move {
        Ok(
            c.verify_user_presence_with_material(action, object, material)
                .await?,
        )
    })
    .await
}

/// A step-up the app enforces itself: the passphrase (bytes) or a presence
/// token for `action` on `object`, redeemed now. `wrong_passphrase` /
/// `passphrase_required` when it does not check out.
pub async fn vault_confirm_step_up(
    action: String,
    object: String,
    passphrase: Option<Vec<u8>>,
    presence_token: Option<String>,
) -> Result<(), BridgeError> {
    let proof = crate::state::step_up(passphrase, presence_token)?;
    with_core(move |c| async move { Ok(c.confirm_step_up(action, object, proof).await?) }).await
}

/// "Confirm actions with biometrics" on this device for the open profile.
pub async fn vault_set_confirm_actions_with_presence(enabled: bool) -> Result<(), BridgeError> {
    with_core(move |c| async move { Ok(c.set_confirm_actions_with_presence(enabled).await?) }).await
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
