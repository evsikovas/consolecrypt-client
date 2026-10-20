//! Explicit authentication flows. None of these APIs are invoked by kubeconfig
//! parsing, synchronized inventory or session construction. Approval and token
//! caches belong to an unlocked device session, never to synchronized models.

pub mod exec;
pub mod oidc;

use crate::session::Authentication;
use std::{fmt, time::SystemTime};

/// Native memory only: callers must discard this value when the vault locks,
/// the source credential changes, or `expires_at` is reached.
pub struct Credential {
    pub authentication: Authentication,
    pub expires_at: SystemTime,
}

impl fmt::Debug for Credential {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Credential")
            .field("expires_at", &self.expires_at)
            .finish_non_exhaustive()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum AuthError {
    #[error("External authentication commands are not available on this device")]
    UnsupportedPlatform,
    #[error("The authentication configuration is invalid")]
    InvalidConfiguration,
    #[error("This authentication command requires local approval")]
    ApprovalRequired,
    #[error("The authentication command changed and requires approval again")]
    CommandChanged,
    #[error("The authentication command requires interactive input")]
    InteractiveRequired,
    #[error("The authentication command could not be started")]
    Process,
    #[error("Authentication was cancelled")]
    Cancelled,
    #[error("Authentication timed out")]
    Timeout,
    #[error("The authentication response is too large")]
    ResponseTooLarge,
    #[error("The authentication provider returned an invalid response")]
    InvalidResponse,
    #[error("The authentication provider could not be reached securely")]
    Transport,
    #[error("The authentication provider rejected the request")]
    Rejected,
    #[error("The browser authentication callback did not match this request")]
    InvalidCallback,
    #[error("The identity token could not be verified")]
    InvalidToken,
    #[error("This authentication session is closed")]
    Closed,
}
