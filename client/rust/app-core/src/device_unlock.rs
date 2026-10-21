//! Device unlock under OS protection (S5): "Unlock with Touch ID / Windows
//! Hello / fingerprint" for the open profile.
//!
//! * Off by default and per installation: the record lives only in the
//!   profile key file (`keyfile.rs`), never in synced objects or backups.
//! * On: the OS protector (`ctx.device_keys`) holds a key that it releases
//!   only after its own biometric / device-password check; the VRK is sealed
//!   under it (local unlock envelope). Nothing the UI says is trusted: no key
//!   from the OS → no unlock (`os_auth_failed`).
//! * Android: the app runs `BiometricPrompt` with a Keystore `CryptoObject`
//!   and passes the decrypted material (`*_with_material`); the core only
//!   checks it cryptographically.
//! * Invalidation (fingerprints changed, Hello reset): the record is
//!   dropped, unlock fails with `os_auth_invalidated`, and the next
//!   passphrase unlock offers to switch it on again.
//!
//! The server copy of this device's envelope is only a trust marker
//! (`UnlockedVault::trust_envelope_for`): toggling device unlock does not
//! touch it.

use crate::app::AppCore;
use crate::dto::DeviceUnlockDto;
use crate::error::{AppError, AppResult};
use crate::keyfile::{self, DeviceUnlockRecord, KeyFile, ReofferReason};
use crate::secrets::blocking;
use crate::session::Session;
use cc_platform_core::{KeyMaterial, OsAuthError, OsAuthKind, UnlockProtection};
use cc_protocol::Bytes;
use cc_vault_core::{unlock_with_unlock_key, UnlockKey, UnlockedVault, VaultError};
use std::sync::Arc;
use zeroize::Zeroizing;

/// Shown in the OS dialog when the UI passes no localized reason.
pub(crate) const DEFAULT_UNLOCK_REASON: &str = "unlock your ConsoleCrypt vault";

pub(crate) fn kind_name(kind: OsAuthKind) -> &'static str {
    match kind {
        OsAuthKind::TouchId => "touch_id",
        OsAuthKind::FaceId => "face_id",
        OsAuthKind::Fingerprint => "fingerprint",
        OsAuthKind::WindowsHello => "windows_hello",
        OsAuthKind::Keyring => "keyring",
        _ => "device_credential",
    }
}

fn protection_name(p: UnlockProtection) -> &'static str {
    match p {
        UnlockProtection::System => "system",
        UnlockProtection::Convenience => "convenience",
    }
}

fn reoffer_name(r: ReofferReason) -> &'static str {
    match r {
        ReofferReason::Migrated => "migrated",
        ReofferReason::Invalidated => "invalidated",
    }
}

fn disabled() -> AppError {
    AppError::Unsupported("Device unlock is disabled for this profile".into())
}

fn reason_or_default(reason: String) -> String {
    let r = reason.trim();
    if r.is_empty() || r.chars().any(char::is_control) {
        DEFAULT_UNLOCK_REASON.to_owned()
    } else {
        r.chars().take(200).collect()
    }
}

fn check_material(material: &[u8]) -> AppResult<()> {
    if !(32..=4096).contains(&material.len()) {
        return Err(AppError::invalid(
            "material",
            "unexpected key material length",
        ));
    }
    Ok(())
}

impl AppCore {
    /// Whether "Unlock with Touch ID / Windows Hello / fingerprint" can be
    /// offered for the open profile, and its state.
    pub async fn device_unlock_info(&self) -> AppResult<DeviceUnlockDto> {
        let s = self.session().await?;
        let status = self.inner.ctx.device_keys.status();
        let kf = s.keyfile();
        let record = kf.as_ref().and_then(|k| k.device_unlock.clone());
        let kind = match &record {
            Some(r) => Some(r.kind.clone()),
            None => status.kind.map(|k| kind_name(k).to_owned()),
        };
        Ok(DeviceUnlockDto {
            enabled: record.is_some(),
            available: record.is_some() && status.available(),
            kind,
            has_vault: s.vault_id().is_some(),
            not_enrolled: status.not_enrolled,
            protection: protection_name(status.protection).to_owned(),
            external: status.external,
            reoffer: kf
                .and_then(|k| k.flags.reoffer_device_unlock)
                .map(|r| reoffer_name(r).to_owned()),
        })
    }

    /// Switch device unlock on (creates the OS-protected key; the OS may
    /// prompt with `reason`) or off (deletes it). Needs an unlocked vault.
    /// On a [`UnlockProtection::Convenience`] platform (Linux) enabling
    /// requires `allow_unprotected` — the user confirmed the warning.
    pub async fn set_device_unlock_enabled(
        &self,
        enabled: bool,
        reason: String,
        allow_unprotected: bool,
    ) -> AppResult<()> {
        let s = self.session().await?;
        let u = s.unlocked().await?;
        if !s.has_keyfile() {
            return Err(AppError::Unsupported(
                "Device unlock needs the profile to be unlocked with the passphrase first".into(),
            ));
        }
        if !enabled {
            return self.forget_device_unlock(&s, None).await;
        }
        let keys = self.inner.ctx.device_keys.clone();
        let status = keys.status();
        if !status.available() {
            return Err(AppError::OsAuth(
                if status.not_enrolled {
                    OsAuthError::NotEnrolled
                } else {
                    OsAuthError::Unsupported
                }
                .to_string(),
            ));
        }
        if status.external {
            return Err(AppError::from(OsAuthError::ExternalRequired));
        }
        if status.protection == UnlockProtection::Convenience && !allow_unprotected {
            return Err(AppError::invalid(
                "allow_unprotected",
                "device unlock on this platform is not protected by the OS; confirm first",
            ));
        }
        let label = keyfile::unlock_label(s.profile_id);
        let reason = reason_or_default(reason);
        let k = keys.clone();
        let protected = blocking(move || Ok(k.create(&label, &reason)?)).await?;
        let vault = u.vault()?;
        let kind = status
            .kind
            .map(kind_name)
            .unwrap_or("device_credential")
            .to_owned();
        let record = seal_record(
            &s,
            &vault,
            &protected.material,
            protected.handle,
            kind,
            protection_name(status.protection),
            false,
        )?;
        self.store_record(&s, record).await
    }

    /// Android: the app created a Keystore-protected key inside a successful
    /// `BiometricPrompt` and passes its material and handle.
    pub async fn enable_device_unlock_with_material(
        &self,
        material: Vec<u8>,
        handle: Vec<u8>,
        kind: String,
    ) -> AppResult<()> {
        let material = Zeroizing::new(material);
        check_material(&material)?;
        if handle.is_empty() || handle.len() > 4096 {
            return Err(AppError::invalid("handle", "unexpected handle length"));
        }
        let s = self.session().await?;
        let u = s.unlocked().await?;
        if !s.has_keyfile() {
            return Err(AppError::Unsupported(
                "Device unlock needs the profile to be unlocked with the passphrase first".into(),
            ));
        }
        let kind = match kind.as_str() {
            "fingerprint" | "face_id" | "device_credential" => kind,
            _ => "fingerprint".to_owned(),
        };
        let vault = u.vault()?;
        let record = seal_record(&s, &vault, &material, handle, kind, "system", true)?;
        self.store_record(&s, record).await
    }

    async fn store_record(&self, s: &Arc<Session>, record: DeviceUnlockRecord) -> AppResult<()> {
        let previous = s.keyfile().and_then(|k| k.device_unlock);
        s.update_keyfile(|k| {
            k.device_unlock = Some(record);
            k.flags.reoffer_device_unlock = None;
        })
        .await?;
        if let Some(old) = previous {
            self.destroy_os_key(&old).await;
        }
        tracing::info!(profile_id = %s.profile_id, "device unlock enabled");
        Ok(())
    }

    /// Drop the device unlock record (and its OS key); with `reoffer`, ask
    /// again after the next passphrase unlock.
    pub(crate) async fn forget_device_unlock(
        &self,
        s: &Arc<Session>,
        reoffer: Option<ReofferReason>,
    ) -> AppResult<()> {
        let previous = s.keyfile().and_then(|k| k.device_unlock);
        s.update_keyfile(|k| {
            k.device_unlock = None;
            k.flags.reoffer_device_unlock = reoffer;
        })
        .await?;
        if let Some(old) = previous {
            self.destroy_os_key(&old).await;
            tracing::info!(profile_id = %s.profile_id, "device unlock disabled");
        }
        Ok(())
    }

    async fn destroy_os_key(&self, record: &DeviceUnlockRecord) {
        if record.external {
            return; // The app deletes its Keystore alias.
        }
        let keys = self.inner.ctx.device_keys.clone();
        let handle = record.handle.0.clone();
        if let Err(e) = blocking(move || Ok(keys.destroy(&handle)?)).await {
            tracing::warn!(error = %e, "OS device unlock key not deleted");
        }
    }

    /// The user declined the offer to switch device unlock on again.
    pub async fn dismiss_device_unlock_offer(&self) -> AppResult<()> {
        let s = self.session().await?;
        s.update_keyfile(|k| k.flags.reoffer_device_unlock = None)
            .await
    }

    /// Lock screen: the OS prompts with `reason` and releases the key, the
    /// vault opens (in-core platforms: macOS, iOS, Windows, Linux).
    pub async fn unlock_with_device(&self, reason: String) -> AppResult<()> {
        let s = self.session().await?;
        if s.unlocked_opt().await.is_some() {
            return Ok(());
        }
        let vault = self.device_unlock(&s, reason).await?;
        self.finish_unlock(&s, vault).await
    }

    /// Android: the handle the app's `BiometricPrompt` decrypts with
    /// (`None` = device unlock off).
    pub async fn device_unlock_handle(&self) -> AppResult<Option<Vec<u8>>> {
        let s = self.session().await?;
        Ok(s.keyfile()
            .and_then(|k| k.device_unlock)
            .map(|r| r.handle.0))
    }

    /// Android: unlock with the material the app's `BiometricPrompt`
    /// `CryptoObject` decrypted.
    pub async fn unlock_with_device_material(&self, material: Vec<u8>) -> AppResult<()> {
        let material = Zeroizing::new(material);
        check_material(&material)?;
        let s = self.session().await?;
        if s.unlocked_opt().await.is_some() {
            return Ok(());
        }
        let (kf, record) = enabled_record(&s)?;
        let vault = self.open_with_material(&s, &kf, &record, &material).await?;
        self.finish_unlock(&s, vault).await
    }

    /// Android: the Keystore key was permanently invalidated (new
    /// fingerprint): forget it and offer it again after the passphrase.
    pub async fn device_unlock_invalidated(&self) -> AppResult<()> {
        let s = self.session().await?;
        self.forget_device_unlock(&s, Some(ReofferReason::Invalidated))
            .await
    }

    /// The vault of `s` from device unlock (in-core protector).
    pub(crate) async fn device_unlock(
        &self,
        s: &Arc<Session>,
        reason: String,
    ) -> AppResult<UnlockedVault> {
        let (kf, record) = enabled_record(s)?;
        if record.external {
            return Err(AppError::from(OsAuthError::ExternalRequired));
        }
        let keys = self.inner.ctx.device_keys.clone();
        let handle = record.handle.0.clone();
        let reason = reason_or_default(reason);
        let released = blocking(move || Ok(keys.release(&handle, &reason))).await?;
        let material = match released {
            Ok(m) => m,
            Err(OsAuthError::Invalidated) => {
                self.invalidated(s).await;
                return Err(AppError::OsAuthInvalidated);
            }
            Err(e) => return Err(e.into()),
        };
        self.open_with_material(s, &kf, &record, &material).await
    }

    /// Android variant of [`AppCore::reset_passphrase_with_device`].
    pub async fn reset_passphrase_with_device_material(
        &self,
        material: Vec<u8>,
        new_passphrase: String,
    ) -> AppResult<()> {
        self.unlock_with_device_material(material).await?;
        self.set_new_passphrase(new_passphrase).await
    }

    async fn open_with_material(
        &self,
        s: &Arc<Session>,
        kf: &KeyFile,
        record: &DeviceUnlockRecord,
        material: &KeyMaterial,
    ) -> AppResult<UnlockedVault> {
        let key = UnlockKey::from_material(material)?;
        match unlock_with_unlock_key(
            kf.vault_id,
            &record.envelope,
            &key,
            &keyfile::unlock_context(s.profile_id),
        ) {
            Ok(v) => Ok(v),
            // The OS released a different key than the one sealed: the
            // protected key was replaced underneath us.
            Err(VaultError::DeviceNotAuthorized) => {
                self.invalidated(s).await;
                Err(AppError::OsAuthInvalidated)
            }
            Err(e) => Err(e.into()),
        }
    }

    async fn invalidated(&self, s: &Arc<Session>) {
        tracing::warn!(profile_id = %s.profile_id, "device unlock key invalidated");
        if let Err(e) = self
            .forget_device_unlock(s, Some(ReofferReason::Invalidated))
            .await
        {
            tracing::warn!(error = %e, "invalidated device unlock not cleared");
        }
    }
}

fn enabled_record(s: &Session) -> AppResult<(KeyFile, DeviceUnlockRecord)> {
    let kf = s.keyfile().ok_or_else(disabled)?;
    let record = kf.device_unlock.clone().ok_or_else(disabled)?;
    Ok((kf, record))
}

fn seal_record(
    s: &Session,
    vault: &UnlockedVault,
    material: &[u8],
    handle: Vec<u8>,
    kind: String,
    protection: &str,
    external: bool,
) -> AppResult<DeviceUnlockRecord> {
    let key = UnlockKey::from_material(material)?;
    let envelope = vault.seal_unlock_envelope(&key, &keyfile::unlock_context(s.profile_id))?;
    Ok(DeviceUnlockRecord {
        kind,
        protection: protection.to_owned(),
        external,
        handle: Bytes::new(handle),
        envelope,
        created_at: chrono::Utc::now(),
    })
}
