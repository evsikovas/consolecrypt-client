//! Recovery (ADR-0004 / ADR-0106): change passphrase, regenerate the
//! Recovery Kit, and forgot-passphrase flows (OS/device auth or Recovery
//! Key → new passphrase). Synced profiles replace the envelope on the
//! server (trusted device + vault access key); Local profiles replace the
//! locally stored envelope.

use crate::app::AppCore;
use crate::dto::RecoveryKitDto;
use crate::error::{AppError, AppResult};
use crate::secrets::blocking;
use crate::session::{PendingKit, Session, Unlocked};
use cc_storage_core::{local_key_envelope, ProfileKind};
use cc_vault_core::SecretString;
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
                let env = s.api()?.replace_password_envelope(&req).await?;
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

    /// Forgot passphrase, trusted device: OS/biometric authentication →
    /// device envelope → set a new passphrase. Leaves the vault unlocked.
    pub async fn reset_passphrase_with_device(&self, new_passphrase: String) -> AppResult<()> {
        let s = self.session().await?;
        if s.unlocked_opt().await.is_none() {
            let vault = self.device_unlock(&s).await?;
            s.install_unlocked(vault).await?;
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
            s.install_unlocked(vault).await?;
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
            let env = s.api()?.replace_recovery_envelope(&req).await?;
            s.cache_envelopes(u.vault_id, None, Some(env), None).await
        }
        ProfileKind::Local => {
            let env = local_key_envelope(u.vault_id, req.envelope, Some(s.device_id()));
            s.cache_envelopes(u.vault_id, None, Some(env), None).await
        }
    }
}
