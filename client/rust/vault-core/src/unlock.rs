//! Unlocking a vault: passphrase, device envelope, or Recovery Key — from
//! server-stored [`KeyEnvelope`]s (synced profiles) or locally stored
//! [`NewEnvelope`]s (local-only profiles, backups; ADR-0106).

use crate::error::{Result, VaultError};
use crate::identity::DeviceIdentity;
use crate::unlocked::UnlockedVault;
use cc_crypto_core::{
    open_device_envelope, open_password_envelope, open_recovery_envelope, CryptoError, RecoveryKey,
};
use cc_platform_core::{OsAuthenticator, SecureStore};
use cc_protocol::envelopes::{KeyEnvelope, NewEnvelope};
use cc_protocol::VaultId;
use secrecy::{ExposeSecret, SecretString};
use std::borrow::Cow;

/// Scheme part of the recovery QR payload (any version); the full prefix
/// and version are checked by `RecoveryKey::from_qr_payload`.
const QR_SCHEME: &str = "consolecrypt-recovery:";

/// Anything an envelope can be read from: the server's [`KeyEnvelope`] or a
/// locally persisted [`NewEnvelope`].
pub trait EnvelopeSource {
    /// The envelope in its uploadable shape (what the crypto operates on).
    fn envelope(&self) -> Cow<'_, NewEnvelope>;
    /// Vault the storage claims the envelope belongs to, if it records one.
    fn stored_vault_id(&self) -> Option<VaultId> {
        None
    }
}

impl EnvelopeSource for NewEnvelope {
    fn envelope(&self) -> Cow<'_, NewEnvelope> {
        Cow::Borrowed(self)
    }
}

impl EnvelopeSource for KeyEnvelope {
    fn envelope(&self) -> Cow<'_, NewEnvelope> {
        Cow::Owned(self.to_new())
    }
    fn stored_vault_id(&self) -> Option<VaultId> {
        Some(self.vault_id)
    }
}

/// The envelope, after checking any vault id recorded next to it.
pub(crate) fn envelope_for<'a>(
    source: &'a (impl EnvelopeSource + ?Sized),
    vault_id: VaultId,
) -> Result<Cow<'a, NewEnvelope>> {
    match source.stored_vault_id() {
        Some(v) if v != vault_id => Err(VaultError::VaultMismatch),
        _ => Ok(source.envelope()),
    }
}

/// Unlock with the vault passphrase (Argon2id; takes ~0.1–1 s — call from a
/// blocking context).
pub fn unlock_with_passphrase(
    vault_id: VaultId,
    password_envelope: &(impl EnvelopeSource + ?Sized),
    passphrase: &SecretString,
) -> Result<UnlockedVault> {
    let env = envelope_for(password_envelope, vault_id)?;
    let vrk = open_password_envelope(&env, vault_id, passphrase).map_err(|e| match e {
        CryptoError::Decrypt | CryptoError::EmptyPassphrase => VaultError::WrongPassphrase,
        other => VaultError::Crypto(other),
    })?;
    Ok(UnlockedVault::from_vrk(vault_id, vrk))
}

/// Unlock with this installation's device envelope (trusted-device / OS
/// biometric unlock). The caller performs any OS authentication first.
pub fn unlock_with_device(
    vault_id: VaultId,
    device_envelope: &(impl EnvelopeSource + ?Sized),
    identity: &DeviceIdentity,
) -> Result<UnlockedVault> {
    let env = envelope_for(device_envelope, vault_id)?;
    let vrk = open_device_envelope(&env, vault_id, identity.device_id(), identity.secret_keys())
        .map_err(|e| match e {
            CryptoError::Decrypt | CryptoError::WrongRecipientDevice => {
                VaultError::DeviceNotAuthorized
            }
            other => VaultError::Crypto(other),
        })?;
    Ok(UnlockedVault::from_vrk(vault_id, vrk))
}

/// OS / biometric unlock of this installation (ADR-0004 "forgot passphrase,
/// have a trusted device"; ADR-0106 local profiles): ask `authenticator`
/// with `reason`, then load the profile's device identity from `store` and
/// open the device envelope. With the default
/// `UnsupportedOsAuthenticator` this fails with [`VaultError::OsAuth`] and
/// the UI falls back to the passphrase.
pub fn unlock_with_os_auth(
    vault_id: VaultId,
    device_envelope: &(impl EnvelopeSource + ?Sized),
    store: &dyn SecureStore,
    profile_id: &str,
    authenticator: &dyn OsAuthenticator,
    reason: &str,
) -> Result<UnlockedVault> {
    authenticator.authenticate(reason)?;
    let identity = DeviceIdentity::load(store, profile_id)?.ok_or(VaultError::NoDeviceIdentity)?;
    unlock_with_device(vault_id, device_envelope, &identity)
}

/// Unlock with a Recovery Key.
pub fn unlock_with_recovery(
    vault_id: VaultId,
    recovery_envelope: &(impl EnvelopeSource + ?Sized),
    recovery_key: &RecoveryKey,
) -> Result<UnlockedVault> {
    let env = envelope_for(recovery_envelope, vault_id)?;
    let vrk = open_recovery_envelope(&env, vault_id, recovery_key).map_err(|e| match e {
        CryptoError::Decrypt => VaultError::WrongRecoveryKey,
        other => VaultError::Crypto(other),
    })?;
    Ok(UnlockedVault::from_vrk(vault_id, vrk))
}

/// Unlock with what the user entered: the 24 words or a scanned QR payload.
pub fn unlock_with_recovery_input(
    vault_id: VaultId,
    recovery_envelope: &(impl EnvelopeSource + ?Sized),
    input: &SecretString,
) -> Result<UnlockedVault> {
    let rk = parse_recovery_input(input, vault_id)?;
    unlock_with_recovery(vault_id, recovery_envelope, &rk)
}

/// Parse user recovery input for `vault_id`: a QR payload
/// (`consolecrypt-recovery:v1:…`, whose vault id must match) or a 24-word
/// mnemonic (case/whitespace-insensitive).
pub fn parse_recovery_input(input: &SecretString, vault_id: VaultId) -> Result<RecoveryKey> {
    let text = input.expose_secret().trim();
    if text.starts_with(QR_SCHEME) {
        let (qr_vault, rk) = RecoveryKey::from_qr_payload(text)?;
        if qr_vault != vault_id {
            return Err(VaultError::RecoveryKeyForOtherVault);
        }
        Ok(rk)
    } else {
        Ok(RecoveryKey::from_mnemonic(text)?)
    }
}
