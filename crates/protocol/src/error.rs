//! Uniform API error body.
//!
//! Every non-2xx response carries an [`ApiError`] JSON body. `message` is
//! human-readable and MUST NOT contain secrets, tokens, ciphertext or
//! envelope contents. Clients branch on `code`, never on `message`.

use crate::ids::RequestId;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCode {
    /// 400 — malformed request, failed validation.
    BadRequest,
    /// 401 — missing/invalid/expired access token.
    Unauthorized,
    /// 401 — email/password pair rejected. Deliberately indistinguishable
    /// from "no such user".
    InvalidCredentials,
    /// 401 — refresh token reuse detected; the whole session family was revoked.
    RefreshTokenReused,
    /// 403 — authenticated but not allowed (e.g. device not trusted for vault).
    Forbidden,
    /// 403 — the calling device has been revoked.
    DeviceRevoked,
    /// 403 — device is registered but not yet trusted for this vault.
    DeviceNotTrusted,
    /// 403 — account email not verified and the operation requires it.
    EmailNotVerified,
    /// 404 — resource does not exist or is not visible to the caller
    /// (no distinction, to avoid IDOR probing).
    NotFound,
    /// 409 — optimistic concurrency conflict (see `sync::PushResponse`).
    Conflict,
    /// 409 — unique constraint. `details.field` names what collided:
    /// `"email"` (register), `"device_id"` (device id owned by another account
    /// or registered with different keys), `"vault_id"` (create vault).
    AlreadyExists,
    /// 410 — resource existed but is gone (expired request, deleted vault).
    Gone,
    /// 413 — payload exceeds limits in [`crate::limits`].
    PayloadTooLarge,
    /// 422 — cryptographic proof failed (bad signature, wrong vault access key).
    InvalidProof,
    /// 426 — client protocol too old / incompatible major.
    UpgradeRequired,
    /// 429 — rate limited. See `retry_after_seconds`.
    RateLimited,
    /// 500
    Internal,
    /// 503
    Unavailable,
}

impl ErrorCode {
    pub const fn http_status(&self) -> u16 {
        match self {
            ErrorCode::BadRequest => 400,
            ErrorCode::Unauthorized
            | ErrorCode::InvalidCredentials
            | ErrorCode::RefreshTokenReused => 401,
            ErrorCode::Forbidden
            | ErrorCode::DeviceRevoked
            | ErrorCode::DeviceNotTrusted
            | ErrorCode::EmailNotVerified => 403,
            ErrorCode::NotFound => 404,
            ErrorCode::Conflict | ErrorCode::AlreadyExists => 409,
            ErrorCode::Gone => 410,
            ErrorCode::PayloadTooLarge => 413,
            ErrorCode::InvalidProof => 422,
            ErrorCode::UpgradeRequired => 426,
            ErrorCode::RateLimited => 429,
            ErrorCode::Internal => 500,
            ErrorCode::Unavailable => 503,
        }
    }

    /// Whether a client may retry the same request unchanged later.
    pub const fn is_retryable(&self) -> bool {
        matches!(
            self,
            ErrorCode::RateLimited | ErrorCode::Internal | ErrorCode::Unavailable
        )
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, thiserror::Error)]
#[error("{code:?}: {message}")]
pub struct ApiError {
    pub code: ErrorCode,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub details: Option<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub request_id: Option<RequestId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub retry_after_seconds: Option<u32>,
}

impl ApiError {
    pub fn new(code: ErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            details: None,
            request_id: None,
            retry_after_seconds: None,
        }
    }

    pub fn with_details(mut self, details: serde_json::Value) -> Self {
        self.details = Some(details);
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serializes_snake_case_code() {
        let e = ApiError::new(ErrorCode::DeviceNotTrusted, "device is pending approval");
        let v = serde_json::to_value(&e).unwrap();
        assert_eq!(v["code"], "device_not_trusted");
        assert!(v.get("details").is_none());
        assert_eq!(ErrorCode::DeviceNotTrusted.http_status(), 403);
    }
}
