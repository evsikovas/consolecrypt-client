//! Sync-level errors.

use crate::api::ApiError;
use crate::codec::CodecError;
use cc_protocol::ErrorCode;
use cc_storage_core::StorageError;
use std::time::Duration;

/// Why a sync engine stopped for good (until the app intervenes).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum StopReason {
    /// This device was revoked (`403 device_revoked`).
    DeviceRevoked,
    /// Session gone (expired refresh token, reuse detected, not signed in).
    ReauthRequired,
    /// The device is not (or no longer) trusted for the vault.
    NotTrusted,
    /// The vault is not visible on the server (deleted / not a member).
    VaultGone,
    /// The server requires a newer client (`426`).
    UpgradeRequired,
    /// Stopped by the application.
    Shutdown,
}

impl std::fmt::Display for StopReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            StopReason::DeviceRevoked => "device revoked",
            StopReason::ReauthRequired => "sign-in required",
            StopReason::NotTrusted => "device not trusted for this vault",
            StopReason::VaultGone => "vault not found on server",
            StopReason::UpgradeRequired => "client upgrade required",
            StopReason::Shutdown => "stopped",
        })
    }
}

/// Error of a sync-core operation.
#[derive(Debug, thiserror::Error)]
pub enum SyncError {
    /// Server/API failure.
    #[error(transparent)]
    Api(#[from] ApiError),
    /// Local database failure.
    #[error(transparent)]
    Storage(#[from] StorageError),
    /// Encryption boundary failure (e.g. vault locked).
    #[error(transparent)]
    Codec(#[from] CodecError),
    /// The server sent something inconsistent; nothing was applied.
    #[error("malformed server response: {0}")]
    MalformedResponse(String),
    /// Sync operations are not available in a local-only profile.
    #[error("profile is local-only; sync is disabled")]
    LocalProfile,
    /// The engine has stopped.
    #[error("sync stopped: {0}")]
    Stopped(StopReason),
    /// The encrypted object exceeds `MAX_OBJECT_CIPHERTEXT_BYTES`.
    #[error("object too large: {size} bytes (max {max})")]
    ObjectTooLarge { size: usize, max: usize },
    /// `POST /v1/vaults` said the vault exists and no attestation request
    /// was provided for the reconnect path.
    #[error("vault already exists on the server; attestation required to reconnect")]
    VaultExists,
    /// Invalid argument or state.
    #[error("{0}")]
    Invalid(String),
}

/// What the background worker should do after an error.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Disposition {
    Stop(StopReason),
    Retry {
        offline: bool,
        after: Option<Duration>,
    },
}

impl SyncError {
    /// Stop reason if this error ends syncing.
    pub fn stop_reason(&self) -> Option<StopReason> {
        match self.disposition() {
            Disposition::Stop(r) => Some(r),
            Disposition::Retry { .. } => None,
        }
    }

    /// No connectivity.
    pub fn is_offline(&self) -> bool {
        matches!(self, SyncError::Api(e) if e.is_offline())
    }

    pub(crate) fn disposition(&self) -> Disposition {
        match self {
            SyncError::Stopped(r) => Disposition::Stop(*r),
            SyncError::LocalProfile => Disposition::Stop(StopReason::Shutdown),
            SyncError::Api(e) => match e {
                ApiError::DeviceRevoked => Disposition::Stop(StopReason::DeviceRevoked),
                ApiError::NotAuthenticated
                | ApiError::SessionExpired
                | ApiError::RefreshTokenReused => Disposition::Stop(StopReason::ReauthRequired),
                ApiError::UpgradeRequired { .. } => Disposition::Stop(StopReason::UpgradeRequired),
                ApiError::Server { error, .. } => match error.code {
                    ErrorCode::DeviceNotTrusted | ErrorCode::Forbidden => {
                        Disposition::Stop(StopReason::NotTrusted)
                    }
                    ErrorCode::NotFound => Disposition::Stop(StopReason::VaultGone),
                    _ => Disposition::Retry {
                        offline: false,
                        after: e.retry_after(),
                    },
                },
                _ => Disposition::Retry {
                    offline: e.is_offline(),
                    after: e.retry_after(),
                },
            },
            _ => Disposition::Retry {
                offline: false,
                after: None,
            },
        }
    }
}
