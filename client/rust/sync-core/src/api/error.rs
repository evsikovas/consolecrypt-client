//! Client-side API error.

use super::signer::{ProofRejection, SignerError};
use cc_protocol::meta::ServerInfo;
use cc_protocol::ErrorCode;
use std::time::Duration;

/// Error from an [`crate::ApiClient`] call. Never contains tokens, request
/// bodies or ciphertext.
#[derive(Debug, Clone, thiserror::Error)]
pub enum ApiError {
    /// Unusable configuration (server URL, header values).
    #[error("invalid client configuration: {0}")]
    InvalidConfig(String),
    /// Connection failed / reset / DNS — the client is probably offline.
    #[error("network error: {0}")]
    Network(String),
    /// The request timed out.
    #[error("request timed out")]
    Timeout,
    /// No tokens stored; sign in first.
    #[error("not signed in")]
    NotAuthenticated,
    /// Access and refresh token rejected; sign in again.
    #[error("session expired; sign in again")]
    SessionExpired,
    /// The server detected reuse of a refresh token and revoked the whole
    /// session family (possible token theft). Sign in again.
    #[error("refresh token reuse detected; session revoked")]
    RefreshTokenReused,
    /// This device was revoked; all its sessions are gone.
    #[error("this device has been revoked")]
    DeviceRevoked,
    /// Enterprise sign-in (protocol 1.8): the account was disabled by the
    /// administrator or in the IdP/directory (`403 account_disabled`, also
    /// from `POST /v1/auth/refresh` after the periodic re-check). Every
    /// session was revoked; the local tokens are cleared.
    #[error("the account was disabled by the administrator")]
    AccountDisabled,
    /// Login refused because this device id was revoked: generate a new
    /// device identity (id + keys) and log in again.
    #[error("device identity revoked; a new device identity is required")]
    DeviceIdentityRevoked,
    /// Login refused because the device id belongs to another account or
    /// was registered with different keys: generate a new device identity.
    #[error("device identity conflicts with the server; a new device identity is required")]
    DeviceIdentityConflict,
    /// Client protocol too old / incompatible (`426`).
    #[error("client upgrade required")]
    UpgradeRequired {
        /// Server metadata from the error details, if present.
        server: Option<Box<ServerInfo>>,
    },
    /// Rate limited (`429`).
    #[error("rate limited")]
    RateLimited {
        /// From `retry_after_seconds` or the `Retry-After` header.
        retry_after: Option<Duration>,
        /// `details.limit` (protocol 1.11): which server limit was hit
        /// (`push_device`, `recipient_lookup_account`, …). Unknown values
        /// are kept as sent; `None` from older servers.
        limit: Option<String>,
    },
    /// Any other structured error response.
    #[error("server error {status}: {error}")]
    Server {
        status: u16,
        error: Box<cc_protocol::ApiError>,
    },
    /// The response could not be understood (wrong shape, bad JSON).
    #[error("invalid response (status {status}): {reason}")]
    InvalidResponse { status: u16, reason: String },
    /// The [`crate::TokenStore`] failed.
    #[error("{0}")]
    TokenStore(String),
    /// The server refused this request's device proof (protocol 1.5,
    /// `422 invalid_proof`); the request was not executed. Security-relevant
    /// — surface it: `Missing` = this client sends no proofs to a server
    /// that requires them; `Stale` = the device clock is off (already
    /// re-signed and retried once); `InvalidSignature` = key mismatch or an
    /// altered request.
    #[error("request proof rejected: {reason}")]
    RequestProofRejected { reason: ProofRejection },
    /// The configured [`crate::RequestSigner`] failed; the request was not
    /// sent (never falls back to an unsigned request).
    #[error(transparent)]
    RequestSigning(#[from] SignerError),
}

impl ApiError {
    /// Protocol error code, when the server sent one.
    pub fn code(&self) -> Option<ErrorCode> {
        match self {
            ApiError::Server { error, .. } => Some(error.code),
            ApiError::DeviceRevoked | ApiError::DeviceIdentityRevoked => {
                Some(ErrorCode::DeviceRevoked)
            }
            ApiError::DeviceIdentityConflict => Some(ErrorCode::AlreadyExists),
            ApiError::RefreshTokenReused => Some(ErrorCode::RefreshTokenReused),
            ApiError::AccountDisabled => Some(ErrorCode::AccountDisabled),
            ApiError::UpgradeRequired { .. } => Some(ErrorCode::UpgradeRequired),
            ApiError::RateLimited { .. } => Some(ErrorCode::RateLimited),
            ApiError::RequestProofRejected { .. } => Some(ErrorCode::InvalidProof),
            _ => None,
        }
    }

    /// HTTP status, when a response was received.
    pub fn status(&self) -> Option<u16> {
        match self {
            ApiError::Server { status, .. } | ApiError::InvalidResponse { status, .. } => {
                Some(*status)
            }
            ApiError::DeviceRevoked
            | ApiError::DeviceIdentityRevoked
            | ApiError::AccountDisabled => Some(403),
            ApiError::DeviceIdentityConflict => Some(409),
            ApiError::UpgradeRequired { .. } => Some(426),
            ApiError::RateLimited { .. } => Some(429),
            ApiError::SessionExpired | ApiError::RefreshTokenReused => Some(401),
            ApiError::RequestProofRejected { .. } => Some(422),
            _ => None,
        }
    }

    /// No response at all (offline, DNS, reset, timeout).
    pub fn is_offline(&self) -> bool {
        matches!(self, ApiError::Network(_) | ApiError::Timeout)
    }

    /// Worth retrying the same request later.
    pub fn is_retryable(&self) -> bool {
        match self {
            ApiError::Network(_) | ApiError::Timeout | ApiError::RateLimited { .. } => true,
            ApiError::Server { status, error } => *status >= 500 || error.code.is_retryable(),
            _ => false,
        }
    }

    /// The user must sign in again.
    pub fn requires_reauth(&self) -> bool {
        matches!(
            self,
            ApiError::NotAuthenticated | ApiError::SessionExpired | ApiError::RefreshTokenReused
        )
    }

    /// The installation must create a new device identity (login only).
    pub fn requires_new_device_identity(&self) -> bool {
        matches!(
            self,
            ApiError::DeviceIdentityRevoked | ApiError::DeviceIdentityConflict
        )
    }

    /// Server-requested delay before retrying.
    pub fn retry_after(&self) -> Option<Duration> {
        match self {
            ApiError::RateLimited { retry_after, .. } => *retry_after,
            ApiError::Server { error, .. } => error
                .retry_after_seconds
                .map(|s| Duration::from_secs(u64::from(s))),
            _ => None,
        }
    }

    /// Whether this is a structured error with `code`.
    pub fn is_code(&self, code: ErrorCode) -> bool {
        self.code() == Some(code)
    }

    /// `details` of a structured error response.
    pub fn details(&self) -> Option<&serde_json::Value> {
        match self {
            ApiError::Server { error, .. } => error.details.as_ref(),
            _ => None,
        }
    }

    /// `details.limit` of a `429 rate_limited` (protocol 1.11).
    pub fn rate_limit_name(&self) -> Option<&str> {
        match self {
            ApiError::RateLimited { limit, .. } => limit.as_deref(),
            _ => None,
        }
    }

    /// A quota refusal (protocol 1.11): `413 payload_too_large` with
    /// `details.reason` = `object_limit` / `vault_quota`, or `403 forbidden`
    /// at sign-in with `device_limit` / `device_daily_limit`.
    pub fn quota(&self) -> Option<QuotaRefusal> {
        let ApiError::Server { error, .. } = self else {
            return None;
        };
        let details = error.details.as_ref()?;
        let reason = details.get(quota::DETAILS_REASON)?.as_str()?;
        let known = match error.code {
            ErrorCode::PayloadTooLarge => {
                matches!(reason, quota::OBJECT_LIMIT | quota::VAULT_QUOTA)
            }
            ErrorCode::Forbidden => {
                matches!(reason, quota::DEVICE_LIMIT | quota::DEVICE_DAILY_LIMIT)
            }
            _ => false,
        };
        if !known {
            return None;
        }
        let number = |key: &str| details.get(key).and_then(serde_json::Value::as_u64);
        Some(QuotaRefusal {
            reason: reason.to_owned(),
            limit: number("limit").or_else(|| number("limit_bytes")),
            until: details
                .get("until")
                .and_then(|v| v.as_str())
                .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
                .map(|t| t.with_timezone(&chrono::Utc)),
        })
    }
}

/// Wire names of quota refusals (protocol 1.11, `cc_protocol::quotas` of
/// server 0.2.2; spelled out here so the client builds against older
/// protocol crates).
pub mod quota {
    /// Key of the quota reason in `details`.
    pub const DETAILS_REASON: &str = "reason";
    /// `413`: the vault would hold more than `details.limit` objects.
    pub const OBJECT_LIMIT: &str = "object_limit";
    /// `413`: the vault's bytes would exceed `details.limit_bytes`.
    pub const VAULT_QUOTA: &str = "vault_quota";
    /// `403` at sign-in: the account already has `details.limit` devices.
    pub const DEVICE_LIMIT: &str = "device_limit";
    /// `403` at sign-in: too many new devices in 24 hours (`details.until`).
    pub const DEVICE_DAILY_LIMIT: &str = "device_daily_limit";
}

/// A quota the server refused a request for (see [`ApiError::quota`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QuotaRefusal {
    /// One of the [`quota`] reasons.
    pub reason: String,
    /// `details.limit` (objects, devices) or `details.limit_bytes`.
    pub limit: Option<u64>,
    /// `details.until` (`device_daily_limit`): when the next device is
    /// accepted.
    pub until: Option<chrono::DateTime<chrono::Utc>>,
}

impl std::fmt::Display for QuotaRefusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let what = match self.reason.as_str() {
            quota::OBJECT_LIMIT => "vault object limit reached",
            quota::VAULT_QUOTA => "vault storage quota exceeded",
            quota::DEVICE_LIMIT => "device limit reached",
            quota::DEVICE_DAILY_LIMIT => "too many new devices today",
            other => other,
        };
        f.write_str(what)?;
        match (self.reason.as_str(), self.limit) {
            (quota::VAULT_QUOTA, Some(n)) => write!(f, " (limit {n} bytes)"),
            (quota::OBJECT_LIMIT, Some(n)) => write!(f, " (limit {n} objects)"),
            (_, Some(n)) => write!(f, " (limit {n})"),
            (_, None) => Ok(()),
        }
    }
}
