//! Enterprise sign-in endpoints (protocol 1.8): the account is authenticated
//! by the organisation's IdP (OIDC, system browser + PKCE) or directory
//! (LDAP / Active Directory login and password) instead of an account
//! password. The wire types live in [`cc_protocol::enterprise_auth`]; this
//! module is the one place the client touches them, so a later contract
//! change stays local.
//!
//! Nothing here can decrypt a vault: after an enterprise sign-in the device
//! is an ordinary new device (vault passphrase / Recovery Key / approval).

use super::{to_json, ApiClient, ApiError};
use cc_protocol::auth::AuthResponse;
use cc_protocol::paths;
use reqwest::Method;

pub use cc_protocol::enterprise_auth::{
    callback_params, is_allowed_app_redirect_uri, is_valid_code_challenge, is_valid_code_verifier,
    LdapLoginRequest, OidcAppCallback, OidcExchangeRequest, OidcStartQuery, APP_REDIRECT_PATH,
    APP_REDIRECT_URI_MOBILE, CODE_CHALLENGE_METHOD_S256,
};
pub use cc_protocol::limits::{
    MAX_ENTERPRISE_LOGIN_LEN, MAX_OIDC_STATE_LEN, OIDC_FLOW_TTL_SECONDS,
    OIDC_LOGIN_CODE_TTL_SECONDS,
};
pub use cc_protocol::meta::{AuthCapability, AuthMode, AuthProvider};

impl ApiClient {
    /// `POST /v1/auth/oidc/exchange`: redeem the single-use `login_code`
    /// from the browser redirect; stores the returned tokens.
    ///
    /// The code is bound to the `device_id` given to `start`, so a refused
    /// device identity cannot be retried with the same code: the caller
    /// regenerates the identity and starts a new browser sign-in.
    pub async fn oidc_exchange(&self, req: &OidcExchangeRequest) -> Result<AuthResponse, ApiError> {
        self.enterprise_login(paths::AUTH_OIDC_EXCHANGE, to_json(req)?)
            .await
    }

    /// `POST /v1/auth/ldap/login`: directory login + password; stores the
    /// returned tokens. The password is only sent over the configured
    /// (TLS) connection, never logged.
    pub async fn ldap_login(&self, req: &LdapLoginRequest) -> Result<AuthResponse, ApiError> {
        self.enterprise_login(paths::AUTH_LDAP_LOGIN, to_json(req)?)
            .await
    }

    async fn enterprise_login(&self, path: &str, body: Vec<u8>) -> Result<AuthResponse, ApiError> {
        let resp: AuthResponse =
            self.public(Method::POST, path, Some(body))
                .await
                .map_err(|e| match e {
                    // Same device-identity rules as the password login.
                    ApiError::DeviceRevoked => ApiError::DeviceIdentityRevoked,
                    e if e.is_code(cc_protocol::ErrorCode::AlreadyExists) => {
                        ApiError::DeviceIdentityConflict
                    }
                    e => e,
                })?;
        self.store_tokens(&resp.tokens).await?;
        Ok(resp)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn enterprise_error_codes_map() {
        use crate::api::map_error;
        use crate::api::Raw;
        let raw = |status: u16, code: &str| Raw {
            status: reqwest::StatusCode::from_u16(status).unwrap(),
            headers: Default::default(),
            body: bytes::Bytes::from(format!(r#"{{"code":"{code}","message":"m"}}"#)),
        };
        assert!(matches!(
            map_error(&raw(403, "account_disabled")),
            ApiError::AccountDisabled
        ));
        let e = map_error(&raw(503, "idp_unavailable"));
        assert!(e.is_code(cc_protocol::ErrorCode::IdpUnavailable));
        assert!(e.is_retryable());
        let e = map_error(&raw(403, "group_required"));
        assert!(e.is_code(cc_protocol::ErrorCode::GroupRequired));
        assert!(!e.is_retryable());
    }
}
