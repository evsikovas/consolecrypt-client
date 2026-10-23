//! Recovery (ADR-0004 / ADR-0106): change passphrase, regenerate the
//! Recovery Kit, and forgot-passphrase flows (OS/device auth or Recovery
//! Key → new passphrase). Synced profiles replace the envelope on the
//! server (trusted device + vault access key); Local profiles replace the
//! locally stored envelope.

use crate::app::AppCore;
use crate::dto::RecoveryKitDto;
use crate::error::{AppError, AppResult};
use crate::key_trust::{adopt_own, KeySource};
use crate::secrets::blocking;
use crate::session::{PendingKit, Session, Unlocked};
use cc_protocol::envelopes::KeyEnvelope;
use cc_storage_core::{local_key_envelope, ProfileKind};
use cc_vault_core::{password_envelope_needs_rewrap, SecretString};
use std::sync::Arc;

impl AppCore {
    /// Change the vault passphrase (the current one is verified first).
    pub async fn change_passphrase(
        &self,
        current_passphrase: String,
        new_passphrase: String,
    ) -> AppResult<()> {
        self.unlocked().await?;
        if !self.verify_passphrase(current_passphrase).await? {
            return Err(AppError::WrongCurrentPassphrase);
        }
        self.set_new_passphrase(new_passphrase).await
    }

    /// Replace the password envelope of the unlocked vault.
    pub(crate) async fn set_new_passphrase(&self, new_passphrase: String) -> AppResult<()> {
        crate::validate::secret_text("new_passphrase", &new_passphrase)?;
        let (s, u) = self.unlocked().await?;
        let vault = u.vault()?;
        let kdf = self.inner.ctx.kdf().await?;
        let pass = SecretString::from(new_passphrase);
        let req = blocking(move || Ok(vault.change_passphrase(&pass, kdf)?)).await?;
        match s.kind().await? {
            ProfileKind::Synced => {
                let echo = s.api()?.replace_password_envelope(&req).await?;
                // TR-4: cache our own envelope; the echo only lends its id.
                let env = adopt_own(u.vault_id, &req.envelope, Some(echo), s.device_id());
                s.cache_envelopes(u.vault_id, Some(env), None, None).await?;
            }
            ProfileKind::Local => {
                let env = local_key_envelope(u.vault_id, req.envelope, Some(s.device_id()));
                s.cache_envelopes(u.vault_id, Some(env), None, None).await?;
            }
        }
        tracing::info!(vault_id = %u.vault_id, "vault passphrase changed");
        Ok(())
    }

    /// After a passphrase unlock with `opened`: an envelope sealed below the
    /// minimum KDF cost for new envelopes (pre-0.8.7 calibration on a slow
    /// machine) is re-wrapped with current parameters — same passphrase, same
    /// vault key, objects untouched. Best effort: the old envelope stays in
    /// place until the new one has been verified and stored (atomically),
    /// and a failure is retried at the next unlock.
    pub(crate) async fn rewrap_password_envelope(
        &self,
        s: &Arc<Session>,
        opened: KeyEnvelope,
        passphrase: &Arc<SecretString>,
    ) {
        if !password_envelope_needs_rewrap(&opened) {
            return;
        }
        match self.try_rewrap(s, opened, passphrase.clone()).await {
            Ok(true) => {
                tracing::info!("vault password envelope re-wrapped with current KDF parameters")
            }
            Ok(false) => {}
            Err(error) => {
                tracing::warn!(code = error.code(), "password envelope re-wrap postponed")
            }
        }
    }

    async fn try_rewrap(
        &self,
        s: &Arc<Session>,
        opened: KeyEnvelope,
        passphrase: Arc<SecretString>,
    ) -> AppResult<bool> {
        let kdf = self.inner.ctx.kdf().await?;
        // Test and `--fast-kdf` policies seal at the floor: nothing to gain.
        if !kdf.meets_new_minimum() {
            return Ok(false);
        }
        let (current, u) = self.unlocked().await?;
        if !Arc::ptr_eq(&current, s) {
            return Ok(false);
        }
        let vault = u.vault()?;
        let req = blocking(move || Ok(vault.rewrap_password_envelope(&passphrase, kdf)?)).await?;
        // Only over the envelope that was opened: a passphrase changed
        // meanwhile (elsewhere, for a synced vault) is never overwritten.
        let (_, stored, _) = s.unlock_envelopes().await?;
        if stored.map(|env| env.envelope_id) != Some(opened.envelope_id) {
            return Ok(false);
        }
        match s.kind().await? {
            ProfileKind::Synced => {
                let api = s.api()?;
                let server = api.recovery_vault_envelope(u.vault_id).await?;
                if server.password_envelope.map(|env| env.envelope_id) != Some(opened.envelope_id) {
                    return Ok(false);
                }
                let echo = api.replace_password_envelope(&req).await?;
                let env = adopt_own(u.vault_id, &req.envelope, Some(echo), s.device_id());
                s.cache_envelopes(u.vault_id, Some(env), None, None).await?;
            }
            ProfileKind::Local => {
                let env = local_key_envelope(u.vault_id, req.envelope, Some(s.device_id()));
                s.cache_envelopes(u.vault_id, Some(env), None, None).await?;
            }
        }
        Ok(true)
    }

    /// Generate a new Recovery Key (the old one stops working). Show the
    /// kit once and run the onboarding check again.
    pub async fn regenerate_recovery_kit(&self) -> AppResult<RecoveryKitDto> {
        let (s, u) = self.unlocked().await?;
        let vault = u.vault()?;
        let profile = s.profile().await?;
        let url = profile.server_url.clone();
        let (kit, req) =
            blocking(
                move || Ok(vault.regenerate_recovery_kit(url.as_deref(), chrono::Utc::now())?),
            )
            .await?;
        replace_recovery(&s, &u, req).await?;
        let dto = RecoveryKitDto::from_kit(&kit);
        s.set_pending_kit(Some(PendingKit { kit, check: None }))
            .await?;
        tracing::info!(vault_id = %u.vault_id, "recovery kit regenerated");
        Ok(dto)
    }

    /// Forgot passphrase with device unlock switched on: the OS releases
    /// the unlock key (prompt with `reason`) → VRK → set a new passphrase.
    /// Leaves the vault unlocked.
    pub async fn reset_passphrase_with_device(
        &self,
        new_passphrase: String,
        reason: String,
    ) -> AppResult<()> {
        let s = self.session().await?;
        if s.unlocked_opt().await.is_none() {
            let vault = self.device_unlock(&s, reason).await?;
            self.finish_unlock(&s, vault, KeySource::DeviceUnlock)
                .await?;
        }
        self.set_new_passphrase(new_passphrase).await
    }

    /// Forgot passphrase, Recovery Key: unlock the recovery envelope → set
    /// a new passphrase. Leaves the vault unlocked.
    pub async fn reset_passphrase_with_recovery_key(
        &self,
        recovery_input: String,
        new_passphrase: String,
    ) -> AppResult<()> {
        let s = self.session().await?;
        if s.unlocked_opt().await.is_none() {
            let vault = self.recovery_unlock(&s, recovery_input).await?;
            self.finish_unlock(&s, vault, KeySource::RecoveryKey)
                .await?;
        }
        self.set_new_passphrase(new_passphrase).await
    }
}

async fn replace_recovery(
    s: &Arc<Session>,
    u: &Unlocked,
    req: cc_protocol::recovery::ReplaceEnvelopeRequest,
) -> AppResult<()> {
    match s.kind().await? {
        ProfileKind::Synced => {
            let echo = s.api()?.replace_recovery_envelope(&req).await?;
            let env = adopt_own(u.vault_id, &req.envelope, Some(echo), s.device_id());
            s.cache_envelopes(u.vault_id, None, Some(env), None).await
        }
        ProfileKind::Local => {
            let env = local_key_envelope(u.vault_id, req.envelope, Some(s.device_id()));
            s.cache_envelopes(u.vault_id, None, Some(env), None).await
        }
    }
}
