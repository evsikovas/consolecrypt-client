//! Account authentication DTOs (`/v1/auth/*`).
//!
//! The account password authenticates the *account* only. It is unrelated to
//! the Vault passphrase and never decrypts anything (ADR-0004). Changing or
//! resetting it never touches Vault ciphertext.
//!
//! Token model:
//! * `access_token` — short-lived bearer token (`Authorization: Bearer …`).
//! * `refresh_token` — long-lived, single-use, rotated on every refresh,
//!   stored server-side only as a hash. Presenting an already-used refresh
//!   token revokes the whole session family (replay detection).
//!
//! Every session is bound to exactly one device.

use crate::devices::{DeviceProof, DeviceRegistration, DeviceStatus};
use crate::ids::{DeviceId, SessionId, UserId};
use crate::Timestamp;
use serde::{Deserialize, Serialize};
use std::fmt;

/// Wrapper for account passwords and one-time tokens inside request DTOs.
/// `Debug` is redacted so request structs can be traced safely.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct SecretString(String);

impl SecretString {
    pub fn new(s: impl Into<String>) -> Self {
        Self(s.into())
    }
    /// Explicit access; grep-able so reviewers can audit every use.
    pub fn expose_secret(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for SecretString {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("SecretString(<redacted>)")
    }
}

impl From<String> for SecretString {
    fn from(s: String) -> Self {
        Self(s)
    }
}

impl From<&str> for SecretString {
    fn from(s: &str) -> Self {
        Self(s.to_owned())
    }
}

/// `POST /v1/auth/register`
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegisterRequest {
    pub email: String,
    pub password: SecretString,
    pub device: DeviceRegistration,
    /// Proof of possession of `device.signing_public_key` (protocol 1.4,
    /// ADR-0006). Optional for a new device; verified when present.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub device_proof: Option<DeviceProof>,
}

/// `POST /v1/auth/login`
///
/// If `device.device_id` is already registered for this account the stored
/// public keys MUST match, otherwise `409 already_exists`, and
/// `device_proof` is REQUIRED (protocol 1.4, ADR-0006): without it anyone
/// with the password could log in as an existing trusted device. An unknown
/// `device_id` registers a new, untrusted device.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoginRequest {
    pub email: String,
    pub password: SecretString,
    pub device: DeviceRegistration,
    /// Proof of possession of the device's Ed25519 signing key.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub device_proof: Option<DeviceProof>,
}

/// Response to register, login and email-verify (when it logs in).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthResponse {
    pub user_id: UserId,
    pub device_id: DeviceId,
    pub device_status: DeviceStatus,
    pub email_verified: bool,
    pub tokens: TokenPair,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct TokenPair {
    pub session_id: SessionId,
    pub access_token: SecretString,
    pub access_expires_at: Timestamp,
    pub refresh_token: SecretString,
    pub refresh_expires_at: Timestamp,
}

impl fmt::Debug for TokenPair {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TokenPair")
            .field("session_id", &self.session_id)
            .field("access_token", &"<redacted>")
            .field("access_expires_at", &self.access_expires_at)
            .field("refresh_token", &"<redacted>")
            .field("refresh_expires_at", &self.refresh_expires_at)
            .finish()
    }
}

/// `POST /v1/auth/refresh` → [`TokenPair`]
///
/// Errors: `401 refresh_token_reused` (family revoked), `401 unauthorized`
/// (unknown/expired token), `403 device_revoked` (the session's device was
/// revoked — the client must create a new device identity).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RefreshRequest {
    pub refresh_token: SecretString,
}

/// `POST /v1/auth/logout` (authenticated) → `204`
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct LogoutRequest {
    /// Revoke every session of the account, not just the current one.
    #[serde(default)]
    pub all_sessions: bool,
}

/// `POST /v1/auth/password/forgot` and `POST /v1/recovery/account/start` → `202`
/// (always, regardless of whether the email exists).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ForgotPasswordRequest {
    pub email: String,
}

/// `POST /v1/auth/password/reset` and `POST /v1/recovery/account/confirm` → `204`
///
/// Resets the *account* password and revokes all sessions. Vault ciphertext
/// and envelopes are untouched; trusted devices stay trusted.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResetPasswordRequest {
    pub token: SecretString,
    pub new_password: SecretString,
}

/// `POST /v1/auth/password/change` (authenticated) → `204`
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChangePasswordRequest {
    pub current_password: SecretString,
    pub new_password: SecretString,
}

/// `POST /v1/auth/email/verify` → `204`
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerifyEmailRequest {
    pub token: SecretString,
}

/// `GET /v1/auth/me`
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccountInfo {
    pub user_id: UserId,
    pub email: String,
    pub email_verified: bool,
    pub created_at: Timestamp,
    pub current_device_id: DeviceId,
    pub current_session_id: SessionId,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn secret_string_is_redacted_in_debug() {
        let s = SecretString::new("hunter2hunter2");
        assert!(!format!("{s:?}").contains("hunter2"));
        // …but serializes normally so it can be sent over TLS.
        assert_eq!(serde_json::to_string(&s).unwrap(), "\"hunter2hunter2\"");
    }
}
