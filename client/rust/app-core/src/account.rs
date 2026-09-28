//! Account operations of synced profiles (logout, password reset e-mail,
//! password change) and the explicit "reveal secret" action.
//!
//! `reveal_credential_secret` is the **only** facade path that returns a
//! stored secret: it runs on an explicit user action (Reveal / Copy), is
//! decrypted on demand from its Secret object (never cached), returned once
//! in a zeroizing wrapper and logged as an audit line without the value.

use crate::app::AppCore;
use crate::dto::{parse_id, AppEvent};
use crate::error::{AppError, AppResult};
use cc_models::credential::CredentialKind;
use cc_protocol::auth::{ChangePasswordRequest, ForgotPasswordRequest, LogoutRequest};
use cc_protocol::limits::MIN_ACCOUNT_PASSWORD_LEN;
use cc_storage_core::ProfileKind;
use std::sync::Arc;
use zeroize::{Zeroize, ZeroizeOnDrop};

/// A secret returned by [`AppCore::reveal_credential_secret`]. Redacted
/// `Debug`, zeroized on drop.
#[derive(Clone, PartialEq, Eq, Zeroize, ZeroizeOnDrop)]
pub struct RevealedSecret {
    /// `password` | `private_key`.
    #[zeroize(skip)]
    pub kind: &'static str,
    /// UTF-8 secret (password text or OpenSSH private key).
    pub value: String,
}

impl std::fmt::Debug for RevealedSecret {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RevealedSecret")
            .field("kind", &self.kind)
            .field("value", &"<redacted>")
            .finish()
    }
}

impl RevealedSecret {
    /// Move the value out as bytes (for FFI); `self` is left empty.
    pub fn take_bytes(&mut self) -> Vec<u8> {
        std::mem::take(&mut self.value).into_bytes()
    }
}

impl AppCore {
    /// Sign out of the account of the open **synced** profile: stop sync,
    /// revoke this session on the server (best effort when offline) and
    /// forget the session tokens. Local data and the vault state are
    /// untouched; [`AppCore::reauthenticate`] signs in again. Emits
    /// [`AppEvent::SignedOut`].
    pub async fn logout(&self) -> AppResult<()> {
        let s = self.session().await?;
        if s.kind().await? != ProfileKind::Synced {
            return Err(AppError::LocalProfile);
        }
        if let Some(u) = s.unlocked_opt().await {
            u.stop_engine().await;
        }
        let api = s.api()?;
        if let Err(e) = api.logout(&LogoutRequest::default()).await {
            tracing::warn!(error = %e, "logout request failed; forgetting the session locally");
        }
        api.clear_tokens().await?;
        tracing::info!(profile_id = %s.profile_id, "signed out");
        self.inner.ctx.emit(AppEvent::SignedOut {
            profile_id: s.profile_id.to_string(),
        });
        Ok(())
    }

    /// Ask `server_url` to send an **account** password-reset e-mail to
    /// `email` (always "accepted", whether or not the address exists). Does
    /// not grant vault access (ADR-0004). No profile needed.
    pub async fn request_password_reset(&self, server_url: String, email: String) -> AppResult<()> {
        let email = crate::validate::email(&email)?;
        let url = crate::validate::server_url(&server_url)?;
        let api = self.inner.ctx.anonymous_api(&url)?;
        api.password_forgot(&ForgotPasswordRequest { email })
            .await?;
        Ok(())
    }

    /// Change the account password of the open synced profile. A wrong
    /// `current` fails with `invalid_credentials` + reason
    /// `current_password_wrong`; a short `new` with `invalid_input` + reason
    /// `account_password_too_short` (at least 12 characters).
    pub async fn change_account_password(
        &self,
        current: String,
        new_password: String,
    ) -> AppResult<()> {
        crate::validate::secret_text("current_password", &current)?;
        crate::validate::secret_text("new_password", &new_password)?;
        if current.is_empty() {
            return Err(AppError::invalid("current_password", "must not be empty"));
        }
        if new_password.len() < MIN_ACCOUNT_PASSWORD_LEN {
            return Err(AppError::invalid(
                "new_password",
                format!("at least {MIN_ACCOUNT_PASSWORD_LEN} characters"),
            ));
        }
        let s = self.session().await?;
        if s.kind().await? != ProfileKind::Synced {
            return Err(AppError::LocalProfile);
        }
        let req = ChangePasswordRequest {
            current_password: cc_protocol::auth::SecretString::new(current),
            new_password: cc_protocol::auth::SecretString::new(new_password),
        };
        match s.api()?.password_change(&req).await.map_err(AppError::from) {
            Ok(()) => {
                tracing::info!(profile_id = %s.profile_id, "account password changed");
                Ok(())
            }
            Err(AppError::InvalidCredentials) => Err(AppError::WrongCurrentPassword),
            Err(e) => Err(e),
        }
    }

    /// **Explicit user action only** ("Reveal" / "Copy password"): decrypt
    /// the secret of a password or key credential and return it once
    /// (password text / OpenSSH private key). An audit line (credential id,
    /// kind — never the value) is logged. Agent credentials and
    /// ask-at-connect hosts have none (`not_found` + `secret_not_stored`).
    pub async fn reveal_credential_secret(
        &self,
        credential_id: String,
    ) -> AppResult<RevealedSecret> {
        let (_, u) = self.unlocked().await?;
        let id = parse_id("credential_id", &credential_id)?;
        let c = u
            .working()
            .credential(id)
            .ok_or_else(|| AppError::not_found("credential", id))?;
        let kind = match c.kind {
            CredentialKind::Password => "password",
            CredentialKind::SshPrivateKey | CredentialKind::SshCertificate => "private_key",
            _ => return Err(AppError::not_found("secret", id)),
        };
        let secret_id = c
            .secret_id
            .ok_or_else(|| AppError::not_found("secret", id))?;
        let secret = u.writer.read_secret(secret_id).await?;
        tracing::info!(
            credential_id = %id,
            kind,
            "credential secret revealed by explicit user action"
        );
        let out = RevealedSecret {
            kind,
            value: secret.value.expose_secret().to_owned(),
        };
        drop(secret);
        Ok(out)
    }
}

impl crate::session::AppCtx {
    /// API client without an account (meta, password-reset e-mail).
    pub(crate) fn anonymous_api(&self, server_url: &str) -> AppResult<cc_sync_core::ApiClient> {
        let cfg = cc_sync_core::ApiConfig::with_http_policy(
            server_url,
            self.config.client_version.clone(),
            self.config.platform,
            self.config.allow_insecure_http,
        )?;
        Ok(cc_sync_core::ApiClient::new(
            cfg,
            Arc::new(cc_sync_core::MemoryTokenStore::new()),
        )?)
    }
}
