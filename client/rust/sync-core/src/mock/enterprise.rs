//! Enterprise sign-in of the mock server (protocol 1.8): a tiny directory
//! for `POST /v1/auth/ldap/login` and an "IdP" that signs one configured
//! user in at `GET /v1/auth/oidc/start` (redirecting straight back to the
//! app with a single-use `login_code`), plus account disabling (sessions
//! revoked, refresh → `403 account_disabled`).

use super::state::{err, MResult, State};
use base64::Engine as _;
use cc_protocol::auth::{AuthResponse, LoginRequest, SecretString};
use cc_protocol::enterprise_auth::{
    is_allowed_app_redirect_uri, is_valid_code_challenge, is_valid_code_verifier, percent_encode,
    LdapLoginRequest, OidcExchangeRequest, OidcStartQuery,
};
use cc_protocol::meta::AuthProvider;
use cc_protocol::{DeviceId, ErrorCode};
use chrono::Utc;
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};
use uuid::Uuid;

struct LoginCode {
    challenge: String,
    device_id: DeviceId,
    email: String,
    expires_at: chrono::DateTime<Utc>,
}

/// Enterprise state of the mock server.
#[derive(Default)]
pub(crate) struct Enterprise {
    /// LDAP: lower-case login → (password, email).
    pub(crate) directory: HashMap<String, (String, String)>,
    /// OIDC: the user the mock IdP signs in (`None` → `access_denied`).
    pub(crate) oidc_user: Option<String>,
    /// Disabled account e-mails.
    pub(crate) disabled: HashSet<String>,
    codes: HashMap<String, LoginCode>,
}

impl State {
    fn require_provider(&self, provider: AuthProvider) -> MResult<()> {
        let auth = &self.cfg.auth;
        if auth.is_enterprise() && auth.provider == Some(provider) {
            Ok(())
        } else {
            Err(err(
                ErrorCode::EnterpriseAuthRequired,
                "this sign-in method is not enabled on this server",
            ))
        }
    }

    /// Password register / login are refused in enterprise mode.
    pub(crate) fn check_password_auth_allowed(&self) -> MResult<()> {
        if self.cfg.auth.is_enterprise() {
            return Err(err(
                ErrorCode::EnterpriseAuthRequired,
                "sign in with your organisation's account; update the app",
            ));
        }
        Ok(())
    }

    /// Log the directory/IdP user in (creating the account on first use).
    fn enterprise_sign_in(
        &mut self,
        email: &str,
        device: cc_protocol::devices::DeviceRegistration,
        device_proof: Option<cc_protocol::devices::DeviceProof>,
    ) -> MResult<AuthResponse> {
        let email = email.trim().to_lowercase();
        if self.enterprise.disabled.contains(&email) {
            return Err(err(ErrorCode::AccountDisabled, "account disabled"));
        }
        let password = self.ensure_enterprise_user(&email);
        self.login(LoginRequest {
            email,
            password: SecretString::new(password),
            device,
            device_proof,
        })
    }

    pub(crate) fn ldap_login(&mut self, req: LdapLoginRequest) -> MResult<AuthResponse> {
        self.require_provider(AuthProvider::Ldap)?;
        let invalid = || err(ErrorCode::InvalidCredentials, "invalid login or password");
        let (password, email) = self
            .enterprise
            .directory
            .get(&req.login.trim().to_lowercase())
            .cloned()
            .ok_or_else(invalid)?;
        if password != req.password.expose_secret() {
            return Err(invalid());
        }
        self.enterprise_sign_in(&email, req.device, req.device_proof)
    }

    /// The redirect back to the app (`Location`).
    pub(crate) fn oidc_start(&mut self, q: OidcStartQuery) -> MResult<String> {
        self.require_provider(AuthProvider::Oidc)?;
        if !is_allowed_app_redirect_uri(&q.redirect_uri)
            || !is_valid_code_challenge(&q.code_challenge)
            || q.state.is_empty()
        {
            return Err(err(ErrorCode::BadRequest, "invalid start request"));
        }
        let back = |params: String| {
            format!(
                "{}?{params}&state={}",
                q.redirect_uri,
                percent_encode(&q.state)
            )
        };
        let Some(email) = self.enterprise.oidc_user.clone() else {
            return Ok(back("error=access_denied".into()));
        };
        if self.enterprise.disabled.contains(&email) {
            return Ok(back(
                "error=account_disabled&error_description=disabled".into(),
            ));
        }
        let code = format!("ccl_{}", Uuid::new_v4().simple());
        self.enterprise.codes.insert(
            code.clone(),
            LoginCode {
                challenge: q.code_challenge.clone(),
                device_id: q.device_id,
                email,
                expires_at: Utc::now() + chrono::Duration::seconds(60),
            },
        );
        Ok(back(format!("login_code={code}")))
    }

    pub(crate) fn oidc_exchange(&mut self, req: OidcExchangeRequest) -> MResult<AuthResponse> {
        self.require_provider(AuthProvider::Oidc)?;
        let invalid = || err(ErrorCode::InvalidCredentials, "invalid login code");
        // Single use: removed before any check.
        let code = self
            .enterprise
            .codes
            .remove(req.login_code.expose_secret())
            .ok_or_else(invalid)?;
        let verifier = req.code_verifier.expose_secret();
        let challenge = base64::engine::general_purpose::URL_SAFE_NO_PAD
            .encode(Sha256::digest(verifier.as_bytes()));
        if code.expires_at <= Utc::now()
            || !is_valid_code_verifier(verifier)
            || challenge != code.challenge
            || req.device.device_id != code.device_id
        {
            return Err(invalid());
        }
        self.enterprise_sign_in(&code.email, req.device, req.device_proof)
    }

    /// Disable an account: every session revoked, refresh answers
    /// `account_disabled`.
    pub(crate) fn disable_account(&mut self, email: &str) {
        let email = email.trim().to_lowercase();
        self.enterprise.disabled.insert(email.clone());
        if let Some(user) = self.user_id(&email) {
            let ids = self.revoke_user_sessions(user);
            self.close_user_sessions(user, ids);
        }
    }

    pub(crate) fn is_disabled_user(&self, user: cc_protocol::UserId) -> bool {
        self.email_of(user)
            .is_some_and(|e| self.enterprise.disabled.contains(&e))
    }
}
