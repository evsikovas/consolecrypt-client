//! Private policy observation/checkpoint recapture. NEVER admission/Ready.
//! The candidate floor alone grants no writes: original owner consent and
//! current server admission remain mandatory.
use super::*;
use crate::session::{Session, Unlocked};
use cc_protocol::{UserId, VaultId};
use cc_sync_core::{
    ApiClient, SettingsAccountPolicy, SettingsCompatibilityError, SettingsPolicyState,
};
use cc_vault_core::DeviceIdentity;
use std::sync::{Arc, Weak};
use std::time::Instant;
fn changed() -> crate::AppError {
    crate::AppError::invalid("settings_sync", "context_changed")
}
fn unavailable() -> crate::AppError {
    crate::AppError::invalid("settings_sync", "policy_unavailable")
}
fn policy_error(error: SettingsCompatibilityError) -> crate::AppError {
    use SettingsCompatibilityError as E;
    let rule = match error {
        E::UpgradeRequired => "client_upgrade_required",
        E::Quarantined => "account_quarantined",
        E::ContextChanged => "context_changed",
        E::Unauthenticated => "reauthentication_required",
        E::Revoked => "device_revoked",
        E::SignerRequired | E::ProofRefused => "proof_required",
        E::InvalidResponse | E::Refused => "policy_invalid_or_refused",
        E::Unsupported | E::Timeout | E::Unavailable => "policy_unavailable",
    };
    crate::AppError::invalid("settings_sync", rule)
}

/// Original Native context, no secret resolver/material, ID authority flag or
/// transferable serde object. Expiry only bounds this local observation.
#[derive(Clone)]
struct Context {
    owner: Arc<Session>,
    unlock: Weak<Unlocked>,
    identity: Weak<DeviceIdentity>,
    device: DeviceId,
    account: UserId,
    vault: VaultId,
    server: String,
    api: ApiClient,
    working_version: u64,
    catalog: SettingsCheckpoint,
    shutdown: tokio::sync::watch::Receiver<bool>,
    expires: Instant,
}
pub(crate) struct SettingsPolicyReview {
    context: Context,
    policy: SettingsAccountPolicy,
}
impl std::fmt::Debug for SettingsPolicyReview {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SettingsPolicyReview")
            .finish_non_exhaustive()
    }
}
/// Current data after real ciphertext pull/reload, not writer permission.
#[derive(Debug)]
pub(crate) struct SettingsRecaptured {
    policy: SettingsAccountPolicy,
    catalog: SettingsCheckpoint,
    working_version: u64,
}
impl Context {
    async fn capture(app: &AppCore) -> AppResult<Self> {
        let (owner, u) = app.unlocked().await?;
        Self::capture_original(app, owner, u).await
    }
    async fn capture_original(
        app: &AppCore,
        owner: Arc<Session>,
        u: Arc<Unlocked>,
    ) -> AppResult<Self> {
        let _profile = app.hold_session(&owner).await?;
        let _unlock = owner.hold_unlocked(&u).await?;
        let profile = owner.profile().await?;
        if profile.kind != ProfileKind::Synced {
            return Err(crate::AppError::LocalProfile);
        }
        let account = profile
            .user_id
            .filter(|id| *id != UserId::NIL)
            .ok_or_else(changed)?;
        if profile.device_id != owner.device_id() {
            return Err(changed());
        }
        let api = owner.api()?;
        let server = profile.server_url.ok_or_else(changed)?;
        if url::Url::parse(&server).ok().as_ref() != Some(&api.config().server_url) {
            return Err(changed());
        }
        let vault = owner.require_vault_id()?;
        if u.vault_id != vault {
            return Err(changed());
        }
        let working_version = u.working().version();
        let catalog = settings_checkpoint(&u)?;
        if u.working().version() != working_version {
            return Err(changed());
        }
        let value = Self {
            owner: owner.clone(),
            unlock: Arc::downgrade(&u),
            identity: Arc::downgrade(&owner.identity()),
            device: owner.device_id(),
            account,
            vault,
            server,
            api,
            working_version,
            catalog,
            shutdown: u.sharing_shutdown.subscribe(),
            expires: Instant::now() + Duration::from_secs(300),
        };
        if *value.shutdown.borrow() {
            return Err(crate::AppError::VaultLocked);
        }
        Ok(value)
    }
    async fn check_held(&self, u: &Arc<Unlocked>, catalog_too: bool) -> AppResult<()> {
        if Instant::now() >= self.expires
            || *self.shutdown.borrow()
            || u.vault_id != self.vault
            || self.owner.device_id() != self.device
            || self.owner.require_vault_id()? != self.vault
            || !Weak::ptr_eq(&self.identity, &Arc::downgrade(&self.owner.identity()))
        {
            return Err(changed());
        }
        let profile = self.owner.profile().await?;
        if profile.kind != ProfileKind::Synced
            || profile.user_id != Some(self.account)
            || profile.device_id != self.device
            || profile.server_url.as_deref() != Some(self.server.as_str())
            || self.owner.api()?.config().server_url != self.api.config().server_url
        {
            return Err(changed());
        }
        if catalog_too
            && (u.working().version() != self.working_version
                || settings_checkpoint(u)? != self.catalog)
        {
            return Err(changed());
        }
        // A queued profile read must not extend the ORIGINAL deadline.
        if Instant::now() >= self.expires || *self.shutdown.borrow() {
            return Err(changed());
        }
        Ok(())
    }
}
impl AppCore {
    /// Observation only; no account opt-in, key proof, state persistence or Ready.
    pub(crate) async fn settings_policy_observe(
        &self,
    ) -> AppResult<SettingsPolicyReview> {
        let mut context = Context::capture(self).await?;
        let response = tokio::select! {
            biased;
            _=context.shutdown.changed()=>return Err(crate::AppError::VaultLocked),
            response=tokio::time::timeout(Duration::from_secs(10),context.api.settings_compatibility(context.vault))=>response,
        };
        let u = context
            .unlock
            .upgrade()
            .ok_or(crate::AppError::VaultLocked)?;
        let _profile = self.hold_session(&context.owner).await?;
        let _unlock = context.owner.hold_unlocked(&u).await?;
        context.check_held(&u, true).await?;
        let policy = response.map_err(|_| unavailable())?.map_err(policy_error)?;
        if policy.account_id() != context.account || policy.checkpoint().vault_id() != context.vault
        {
            return Err(changed());
        }
        drop(_unlock);
        drop(_profile);
        Ok(SettingsPolicyReview { context, policy })
    }
    /// Consume an existing observation and recapture current encrypted settings
    /// through the original engine. No owner PUT/automatic opt-in is performed.
    /// This is explicitly not a replacement for post-clear key commitment review
    /// or the release-owned floor, and cannot bypass current settings writer policy.
    pub(crate) async fn settings_checkpoint_recapture(
        &self,
        mut review: SettingsPolicyReview,
    ) -> AppResult<SettingsRecaptured> {
        if review.policy.state() != SettingsPolicyState::Enabled {
            return Err(crate::AppError::invalid(
                "settings_sync",
                if review.policy.state() == SettingsPolicyState::Quarantined {
                    "account_quarantined"
                } else {
                    "admission_not_enabled"
                },
            ));
        }
        let engine = {
            let u = review
                .context
                .unlock
                .upgrade()
                .ok_or(crate::AppError::VaultLocked)?;
            let _profile = self.hold_session(&review.context.owner).await?;
            let _unlock = review.context.owner.hold_unlocked(&u).await?;
            review.context.check_held(&u, true).await?;
            u.engine()
                .filter(|engine| engine.stop_reason().is_none())
                .ok_or_else(unavailable)?
        };
        let response = tokio::select! {
            biased;
            _=review.context.shutdown.changed()=>return Err(crate::AppError::VaultLocked),
            response=engine.pull_settings_checkpoint(&review.policy)=>response,
        };
        let u = review
            .context
            .unlock
            .upgrade()
            .ok_or(crate::AppError::VaultLocked)?;
        let _profile = self.hold_session(&review.context.owner).await?;
        let _unlock = review.context.owner.hold_unlocked(&u).await?;
        review.context.check_held(&u, false).await?;
        let applied = response.map_err(|_| unavailable())?;
        if applied.account_id() != review.context.account
            || applied.vault_id() != review.context.vault
            || applied.epoch() != review.policy.checkpoint().epoch()
            || applied.sequence() < review.policy.checkpoint().sequence()
        {
            return Err(changed());
        }
        // Only bounded local storage work under guards; no network/key resolver.
        u.writer.reload_all().await?;
        review.context.check_held(&u, false).await?;
        let working_version = u.working().version();
        let catalog = settings_checkpoint(&u)?;
        if u.working().version() != working_version {
            return Err(changed());
        }
        Ok(SettingsRecaptured {
            policy: review.policy,
            catalog,
            working_version,
        })
    }
}
#[cfg(test)]
mod tests;

mod consent;
pub use consent::SettingsSyncConsent;

mod writer;
pub(crate) use writer::{SettingsWritePolicy, SettingsWriteAdmission};
