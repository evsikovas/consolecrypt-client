//! Security key unlock (FEAT-16): "Unlock with a security key" (YubiKey and
//! other FIDO2 keys) for the open profile.
//!
//! * Off by default and per installation, like device unlock: the record
//!   lives only in the profile key file, never in synced objects or backups.
//!   The passphrase and the Recovery Key keep working.
//! * On: the key registers a credential for `consolecrypt.dev`; its PRF
//!   answer to a random salt (stored next to it) seals the VRK in a local
//!   unlock envelope. Only that key, with its PIN, opens it again.
//! * Windows asks for the PIN in its own dialog; elsewhere the app asks and
//!   passes it (`pin` in [`SecurityKeyDto`]).

use crate::app::AppCore;
use crate::dto::SecurityKeyDto;
use crate::error::{AppError, AppResult};
use crate::key_trust::KeySource;
use crate::keyfile::{self, SecurityKeyRecord};
use crate::secrets::blocking;
use crate::session::Session;
use cc_platform_core::{SecurityKeyAuthenticator, SecurityKeyError, SecurityKeyPin};
use cc_protocol::Bytes;
use cc_vault_core::{unlock_with_unlock_key, UnlockKey, UnlockedVault, VaultError};
use std::sync::Arc;
use zeroize::Zeroizing;

/// The machine-readable reason of a failed security key step (the UI
/// localizes it): `invalid_input` with field `security_key`.
fn failure(e: SecurityKeyError) -> AppError {
    let rule = match e {
        SecurityKeyError::Unsupported => "unsupported",
        SecurityKeyError::NoDevice => "no_device",
        SecurityKeyError::SeveralDevices => "several_devices",
        SecurityKeyError::PinRequired => "pin_required",
        SecurityKeyError::PinInvalid => "pin_invalid",
        SecurityKeyError::PinBlocked => "pin_blocked",
        SecurityKeyError::Cancelled => "cancelled",
        SecurityKeyError::WrongKey => "wrong_key",
        SecurityKeyError::NoHmacSecret => "no_hmac_secret",
        SecurityKeyError::Device(detail) => {
            tracing::warn!(%detail, "security key error");
            "device_error"
        }
    };
    AppError::invalid("security_key", rule)
}

fn disabled() -> AppError {
    AppError::Unsupported("Security key unlock is disabled for this profile".into())
}

fn pin_name(pin: SecurityKeyPin) -> &'static str {
    match pin {
        SecurityKeyPin::System => "system",
        SecurityKeyPin::App => "app",
    }
}

fn check_pin(pin: Option<&str>) -> AppResult<()> {
    if let Some(p) = pin {
        if p.is_empty() || p.len() > 63 || p.chars().any(char::is_control) {
            return Err(AppError::invalid("security_key", "pin_invalid"));
        }
    }
    Ok(())
}

impl AppCore {
    pub(crate) fn security_keys(&self) -> Arc<dyn SecurityKeyAuthenticator> {
        self.inner
            .ctx
            .security_keys
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    /// Replace the security key authenticator (tests). Rust-only.
    pub fn set_security_keys(&self, keys: Arc<dyn SecurityKeyAuthenticator>) {
        *self
            .inner
            .ctx
            .security_keys
            .write()
            .unwrap_or_else(|e| e.into_inner()) = keys;
    }

    /// Whether "Unlock with a security key" can be offered for the open
    /// profile, and its state.
    pub async fn security_key_info(&self) -> AppResult<SecurityKeyDto> {
        let s = self.session().await?;
        let pin = self.security_keys().pin();
        let record = s.keyfile().and_then(|k| k.security_key);
        Ok(SecurityKeyDto {
            supported: pin.is_some(),
            enabled: record.is_some(),
            pin: pin.map(|p| pin_name(p).to_owned()),
            has_vault: s.vault_id().is_some(),
            created_at_ms: record.map(|r| r.created_at.timestamp_millis()),
        })
    }

    /// Switch security key unlock on (registers a credential on the key and
    /// asks it once more for the sealing secret: two touches) or off. Needs
    /// an unlocked vault. `pin`: the key PIN where the app asks for it.
    pub async fn set_security_key_enabled(
        &self,
        enabled: bool,
        pin: Option<String>,
    ) -> AppResult<()> {
        let pin = pin.map(Zeroizing::new);
        let s = self.session().await?;
        let u = s.unlocked().await?;
        if !s.has_keyfile() {
            return Err(AppError::Unsupported(
                "Security key unlock needs the profile to be unlocked with the passphrase first"
                    .into(),
            ));
        }
        if !enabled {
            s.update_keyfile(|k| k.security_key = None).await?;
            tracing::info!(profile_id = %s.profile_id, "security key unlock disabled");
            return Ok(());
        }
        check_pin(pin.as_deref().map(String::as_str))?;
        let keys = self.security_keys();
        if keys.pin().is_none() {
            return Err(failure(SecurityKeyError::Unsupported));
        }
        let mut salt = [0u8; 32];
        cc_crypto_core::fill_random(&mut salt).map_err(|e| AppError::Crypto(e.to_string()))?;
        let label = "ConsoleCrypt".to_owned();
        let (credential, secret) = blocking(move || {
            let pin = pin.as_deref().map(String::as_str);
            let credential = keys.register(&label, pin).map_err(failure)?;
            let secret = keys.prf(&credential, &salt, pin).map_err(failure)?;
            Ok((credential, secret))
        })
        .await?;
        let vault = u.vault()?;
        let key = UnlockKey::from_material(secret.as_slice())?;
        let envelope =
            vault.seal_unlock_envelope(&key, &keyfile::security_key_context(s.profile_id))?;
        let record = SecurityKeyRecord {
            credential_id: Bytes::new(credential),
            salt: Bytes::new(salt.to_vec()),
            envelope,
            created_at: chrono::Utc::now(),
        };
        s.update_keyfile(|k| k.security_key = Some(record)).await?;
        tracing::info!(profile_id = %s.profile_id, "security key unlock enabled");
        Ok(())
    }

    /// Lock screen: the key answers (after its PIN and a touch), the vault
    /// opens.
    pub async fn unlock_with_security_key(&self, pin: Option<String>) -> AppResult<()> {
        let pin = pin.map(Zeroizing::new);
        let s = self.session().await?;
        if s.unlocked_opt().await.is_some() {
            return Ok(());
        }
        check_pin(pin.as_deref().map(String::as_str))?;
        let vault = self.security_key_vault(&s, pin).await?;
        self.finish_unlock(&s, vault, KeySource::SecurityKey).await
    }

    async fn security_key_vault(
        &self,
        s: &Arc<Session>,
        pin: Option<Zeroizing<String>>,
    ) -> AppResult<UnlockedVault> {
        let kf = s.keyfile().ok_or_else(disabled)?;
        let record = kf.security_key.clone().ok_or_else(disabled)?;
        let salt: [u8; 32] = record
            .salt
            .0
            .as_slice()
            .try_into()
            .map_err(|_| AppError::internal("security key salt length"))?;
        let keys = self.security_keys();
        let credential = record.credential_id.0.clone();
        let secret = blocking(move || {
            keys.prf(&credential, &salt, pin.as_deref().map(String::as_str))
                .map_err(failure)
        })
        .await?;
        let key = UnlockKey::from_material(secret.as_slice())?;
        match unlock_with_unlock_key(
            kf.vault_id,
            &record.envelope,
            &key,
            &keyfile::security_key_context(s.profile_id),
        ) {
            Ok(v) => Ok(v),
            // Another key that happens to know a credential of this id.
            Err(VaultError::DeviceNotAuthorized) => Err(failure(SecurityKeyError::WrongKey)),
            Err(e) => Err(e.into()),
        }
    }
}
