//! Sync control (synced profiles), ADR-0106 profile transitions (enable
//! sync / disconnect) and re-authentication (expired session, revoked or
//! conflicting device identity).

use crate::app::{authenticate, pick_envelopes, sign_in, AppCore};
use crate::dto::*;
use crate::error::{AppError, AppResult};
use crate::secrets;
use crate::session::NEEDS_ATTEST_SETTING;
use cc_protocol::devices::RevokeDeviceRequest;
use cc_storage_core::{Profile, ProfileKind};
use cc_sync_core::EnableSync;

impl AppCore {
    /// Sync status of the open profile (`LocalOnly` for local profiles).
    pub async fn sync_status(&self) -> AppResult<SyncStatusDto> {
        self.session().await?.sync_status().await
    }

    /// Run one sync cycle now (push, resolve conflicts, pull).
    pub async fn sync_now(&self) -> AppResult<SyncReportDto> {
        let (s, u) = self.unlocked().await?;
        if s.kind().await? == ProfileKind::Local {
            return Err(AppError::LocalProfile);
        }
        let engine = s.start_engine(&u, false).await?;
        let report = engine.sync_now().await?;
        // Read-after-sync consistency: remote changes reach the working set
        // through events asynchronously; reload now if anything came in.
        if report.pulled > 0 || report.resolved > 0 || report.snapshot_pages > 0 {
            u.writer.reload_all().await?;
        }
        Ok((&report).into())
    }

    /// Start the background engine + WebSocket events (no-op if running).
    pub async fn start_background_sync(&self) -> AppResult<()> {
        let (s, u) = self.unlocked().await?;
        if s.kind().await? == ProfileKind::Local {
            return Err(AppError::LocalProfile);
        }
        u.stop_engine().await;
        s.start_engine(&u, true).await?;
        Ok(())
    }

    /// Stop background syncing (local edits keep queueing in the outbox).
    pub async fn stop_background_sync(&self) -> AppResult<()> {
        let (_, u) = self.unlocked().await?;
        u.stop_engine().await;
        Ok(())
    }

    /// **Enable sync** for a Local profile (ADR-0106): register or log in,
    /// upload the existing vault under its id (or reconnect if it already
    /// exists), push every object, switch the profile to Synced.
    pub async fn enable_sync(
        &self,
        server_url: String,
        email: String,
        account_password: String,
        mode: AccountMode,
    ) -> AppResult<ProfileInfo> {
        let (s, u) = self.unlocked().await?;
        let old = s.profile().await?;
        if old.kind == ProfileKind::Synced {
            return Err(AppError::AlreadySynced);
        }
        let email = crate::validate::email(&email)?;
        let url = crate::validate::server_url(&server_url)?;
        crate::validate::secret_text("account_password", &account_password)?;
        let pid = s.profile_id;
        let api = self.inner.ctx.api_for(pid, &url, &s.identity_cell())?;
        let password = cc_protocol::auth::SecretString::new(account_password);
        let progress = |step: &str, uploaded: u64, total: u64| {
            self.inner.ctx.emit(AppEvent::EnableSyncProgress {
                step: step.to_owned(),
                uploaded,
                total,
            })
        };
        progress("authenticating", 0, 0);
        let (auth, identity) =
            sign_in(self, &api, pid, s.identity(), &email, &password, mode).await?;
        s.set_identity(identity.clone());
        let vault = u.vault()?;
        let rec = s
            .storage
            .get_vault(u.vault_id)
            .await?
            .ok_or(AppError::NoVault)?;
        let pw = rec
            .password_envelope
            .ok_or_else(|| AppError::Storage("no password envelope stored".into()))?;
        let re = rec
            .recovery_envelope
            .ok_or_else(|| AppError::Storage("no recovery envelope stored".into()))?;
        let create_vault = vault.enable_sync_request(&pw, &re, &identity)?;
        let attest = Some(vault.attest_device_request(&identity)?);
        let mut profile = Profile::new_synced(
            pid,
            identity.device_id(),
            url,
            Some(auth.user_id),
            Some(email),
        );
        profile.display_name = old.display_name.clone();
        profile.created_at = old.created_at;
        progress("creating_remote_vault", 0, 0);
        let store = u.store().clone();
        let attach = cc_sync_core::enable_sync(
            u.store(),
            &api,
            EnableSync {
                create_vault,
                attest,
                profile,
            },
            s.engine_config(),
        );
        tokio::pin!(attach);
        // Upload progress: the attach queues every object in the outbox; the
        // first sync cycle drains it.
        let mut tick = tokio::time::interval(std::time::Duration::from_millis(250));
        let mut total = 0u64;
        let mut last = None;
        let (engine, outcome) = loop {
            tokio::select! {
                r = &mut attach => break r?,
                _ = tick.tick() => {
                    if let Ok(c) = store.outbox_counts().await {
                        let pending = c.total();
                        total = total.max(pending);
                        let now = (total - pending, total);
                        if total > 0 && last != Some(now) {
                            last = Some(now);
                            progress("uploading", now.0, now.1);
                        }
                    }
                }
            }
        };
        if outcome.path == cc_sync_core::EnableSyncPath::Reconnected {
            progress("reconnecting", 0, 0);
        }
        let queued = outcome.attach.map(|a| a.queued as u64).unwrap_or(total);
        let pending = store.outbox_counts().await.map(|c| c.total()).unwrap_or(0);
        let total = total.max(queued);
        progress("uploading", total.saturating_sub(pending), total);
        progress("finishing", 0, 0);
        tracing::info!(
            path = ?outcome.path,
            queued = outcome.attach.map(|a| a.queued).unwrap_or(0),
            "sync enabled"
        );
        if let Err(e) = &outcome.upload {
            tracing::warn!(error = %e, "initial upload incomplete; it resumes automatically");
        }
        s.set_api(Some(api.clone()));
        u.writer.reload_all().await?;
        if let Ok(l) = api.list_envelopes(u.vault_id).await {
            let (p, r, d) = pick_envelopes(&l.envelopes, identity.device_id());
            s.cache_envelopes(u.vault_id, p, r, d).await?;
        }
        s.install_engine(&u, engine, self.inner.ctx.config.background_sync)
            .await?;
        self.set_index_kind(pid, ProfileKind::Synced).await?;
        let info = self
            .active_profile()
            .await?
            .ok_or(AppError::NoActiveProfile)?;
        progress("done", 0, 0);
        Ok(info)
    }

    /// **Disconnect** a Synced profile (ADR-0106): stop syncing, log out
    /// (or revoke this device), keep all local data; the profile becomes
    /// Local. The server copy is left untouched.
    pub async fn disconnect(&self, revoke_device: bool) -> AppResult<ProfileInfo> {
        let s = self.session().await?;
        let old = s.profile().await?;
        if old.kind != ProfileKind::Synced {
            return Err(AppError::LocalProfile);
        }
        if let Some(u) = s.unlocked_opt().await {
            u.stop_engine().await;
        }
        if let Some(api) = s.api_opt() {
            let r = if revoke_device {
                api.revoke_device(
                    s.device_id(),
                    &RevokeDeviceRequest {
                        reason: Some("disconnected by the user".into()),
                    },
                )
                .await
            } else {
                api.logout(&Default::default()).await
            };
            if let Err(e) = r {
                tracing::warn!(error = %e, "server call during disconnect failed");
            }
            let _ = api.clear_tokens().await;
        }
        let mut local = Profile::new_local(s.profile_id, s.device_id(), old.display_name.clone());
        local.created_at = old.created_at;
        match s.vault_id() {
            Some(vault_id) => {
                let p = local.clone();
                s.storage
                    .write(move |tx| tx.detach_to_local(vault_id, &p))
                    .await?;
            }
            None => s.storage.put_profile(local).await?,
        }
        s.set_api(None);
        self.set_index_kind(s.profile_id, ProfileKind::Local)
            .await?;
        self.active_profile()
            .await?
            .ok_or(AppError::NoActiveProfile)
    }

    /// Sign in again after [`AppError::ReauthRequired`] /
    /// [`AppEvent::ReauthRequired`]. With `allow_new_device_identity`, a
    /// revoked/conflicting device id is replaced by a fresh identity (the
    /// device then attests with the vault access key — immediately if the
    /// vault is unlocked, else on the next unlock).
    pub async fn reauthenticate(
        &self,
        account_password: String,
        allow_new_device_identity: bool,
    ) -> AppResult<AuthStateDto> {
        crate::validate::secret_text("account_password", &account_password)?;
        let s = self.session().await?;
        let api = s.api()?;
        let mut profile = s.profile().await?;
        let email = profile
            .email
            .clone()
            .ok_or_else(|| AppError::Storage("synced profile without email".into()))?;
        let password = cc_protocol::auth::SecretString::new(account_password);
        let mut new_identity = false;
        let auth = match authenticate(
            self,
            &api,
            &s.identity(),
            &email,
            &password,
            AccountMode::Login,
        )
        .await
        {
            Ok(a) => a,
            Err(AppError::NewDeviceIdentityRequired) if allow_new_device_identity => {
                let fresh = std::sync::Arc::new(
                    secrets::regenerate_identity(self.inner.ctx.secure.clone(), s.profile_id)
                        .await?,
                );
                s.set_identity(fresh.clone());
                new_identity = true;
                s.storage.setting_set(NEEDS_ATTEST_SETTING, true).await?;
                authenticate(self, &api, &fresh, &email, &password, AccountMode::Login).await?
            }
            Err(e) => return Err(e),
        };
        profile.device_id = s.device_id();
        profile.user_id = Some(auth.user_id);
        profile.updated_at = chrono::Utc::now();
        s.storage.put_profile(profile).await?;
        let mut trusted = false;
        if let Some(vault_id) = s.vault_id() {
            let info = api.get_vault(vault_id).await.ok();
            trusted = info.as_ref().is_some_and(|v| v.caller_trusted);
            if let Some(u) = s.unlocked_opt().await {
                if !trusted {
                    let vault = u.vault()?;
                    let req = vault.attest_device_request(&s.identity())?;
                    let env = api.attest_device(s.device_id(), &req).await?;
                    s.cache_envelopes(vault_id, None, None, Some(env)).await?;
                    s.storage.setting_set(NEEDS_ATTEST_SETTING, false).await?;
                    trusted = true;
                }
                u.stop_engine().await;
                s.start_engine(&u, self.inner.ctx.config.background_sync)
                    .await?;
                if let Some(info) = &info {
                    u.observe_vault_info(std::slice::from_ref(info)).await;
                }
            } else if !trusted {
                s.storage.setting_set(NEEDS_ATTEST_SETTING, true).await?;
            }
        }
        Ok(AuthStateDto {
            device_id: s.device_id().to_string(),
            new_device_identity: new_identity,
            trusted,
        })
    }

    pub(crate) async fn set_index_kind(
        &self,
        pid: cc_storage_core::ProfileId,
        kind: ProfileKind,
    ) -> AppResult<()> {
        let profiles = self.inner.profiles.clone();
        secrets::blocking(move || {
            if let Some(mut e) = profiles.get(pid)? {
                e.kind = kind;
                profiles.update(&e)?;
            }
            Ok(())
        })
        .await?;
        self.inner.ctx.emit(AppEvent::ProfilesChanged);
        Ok(())
    }
}
