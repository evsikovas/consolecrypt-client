//! OS / biometric user authentication hook (Touch ID, Windows Hello).
//!
//! Used before unlocking a vault through this installation's device envelope
//! (ADR-0004 recovery scenario "forgot passphrase, have a trusted device";
//! ADR-0106 OS/biometric unlock of local profiles).

use std::fmt;
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// What the platform can offer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OsAuthAvailability {
    /// Biometric or device-credential authentication can be requested.
    Available(OsAuthKind),
    /// Hardware exists but nothing is enrolled (no fingerprint / PIN).
    NotEnrolled,
    /// Not implemented or not available on this platform.
    Unsupported,
}

/// Kind of authentication the OS will perform.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OsAuthKind {
    TouchId,
    FaceId,
    WindowsHello,
    /// OS account password / PIN fallback.
    DeviceCredential,
}

/// Why authentication did not succeed.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum OsAuthError {
    #[error("OS authentication is not supported on this platform")]
    Unsupported,
    #[error("no biometric or device credential is enrolled")]
    NotEnrolled,
    #[error("authentication cancelled by the user")]
    Cancelled,
    #[error("authentication failed")]
    Failed,
    #[error("OS authentication error: {0}")]
    Platform(String),
}

/// Asks the operating system to verify the user's presence.
///
/// Blocking (the OS shows a prompt); async callers use `spawn_blocking`.
pub trait OsAuthenticator: Send + Sync + fmt::Debug {
    /// What is available right now.
    fn availability(&self) -> OsAuthAvailability;
    /// Prompt the user with `reason` (shown in the OS dialog).
    fn authenticate(&self, reason: &str) -> Result<(), OsAuthError>;
}

/// Default authenticator: reports [`OsAuthAvailability::Unsupported`] and
/// fails every request, so callers fall back to the vault passphrase.
///
/// TODO(platform): real implementations — deferred because the Flutter side
/// may own the prompt (`local_auth` plugin) and the stronger design gates the
/// device key itself; next: (1) macOS — store the device identity in the
/// Keychain with a `SecAccessControl` of `.biometryCurrentSet | .or(.devicePasscode)`
/// (security-framework) so the key is released only after Touch ID, and
/// expose `LAContext.evaluatePolicy` via objc2-local-authentication for
/// availability; (2) Windows — `Windows.Security.Credentials.UI.UserConsentVerifier`
/// (windows crate) for Windows Hello, with the device key protected by DPAPI-NG
/// or a Hello-bound KeyCredential.
#[derive(Debug, Clone, Copy, Default)]
pub struct UnsupportedOsAuthenticator;

impl OsAuthenticator for UnsupportedOsAuthenticator {
    fn availability(&self) -> OsAuthAvailability {
        OsAuthAvailability::Unsupported
    }

    fn authenticate(&self, _reason: &str) -> Result<(), OsAuthError> {
        Err(OsAuthError::Unsupported)
    }
}

/// OS authentication performed by the **UI layer** (e.g. Touch ID through
/// a Flutter platform channel calling `LAContext.evaluatePolicy`), handed to
/// the core as a short-lived, single-use grant.
///
/// The UI reports what the OS offers ([`set_availability`](Self::set_availability))
/// and, after a successful prompt, calls [`grant_once`](Self::grant_once)
/// right before the core operation that needs it; the next
/// [`authenticate`](OsAuthenticator::authenticate) consumes the grant
/// (valid for [`GRANT_TTL`](Self::GRANT_TTL)). Without a fresh grant
/// authentication fails, so a stale "yes" is never reused.
///
/// Security note: this is a user-presence gate in the same process, as
/// strong as an in-core `LAContext` call; the vault key stays protected by
/// the device envelope (unlocked by the device identity from the OS
/// keychain), which never leaves the core.
#[derive(Debug)]
pub struct ExternalOsAuthenticator {
    state: Mutex<ExternalState>,
}

#[derive(Debug)]
struct ExternalState {
    availability: OsAuthAvailability,
    grant: Option<Instant>,
}

impl Default for ExternalOsAuthenticator {
    fn default() -> Self {
        Self::new()
    }
}

impl ExternalOsAuthenticator {
    /// How long a grant stays valid.
    pub const GRANT_TTL: Duration = Duration::from_secs(30);

    /// Starts as [`OsAuthAvailability::Unsupported`] without a grant.
    pub fn new() -> Self {
        Self {
            state: Mutex::new(ExternalState {
                availability: OsAuthAvailability::Unsupported,
                grant: None,
            }),
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, ExternalState> {
        self.state.lock().unwrap_or_else(|p| p.into_inner())
    }

    /// What the UI's OS query reported.
    pub fn set_availability(&self, availability: OsAuthAvailability) {
        let mut s = self.lock();
        s.availability = availability;
        if !matches!(availability, OsAuthAvailability::Available(_)) {
            s.grant = None;
        }
    }

    /// The UI's OS prompt just succeeded: allow one authentication.
    pub fn grant_once(&self) {
        self.lock().grant = Some(Instant::now());
    }

    /// Drop an unused grant.
    pub fn revoke(&self) {
        self.lock().grant = None;
    }
}

impl OsAuthenticator for ExternalOsAuthenticator {
    fn availability(&self) -> OsAuthAvailability {
        self.lock().availability
    }

    fn authenticate(&self, _reason: &str) -> Result<(), OsAuthError> {
        let mut s = self.lock();
        // Every attempt consumes the grant, successful or not.
        let grant = s.grant.take();
        match s.availability {
            OsAuthAvailability::Unsupported => return Err(OsAuthError::Unsupported),
            OsAuthAvailability::NotEnrolled => return Err(OsAuthError::NotEnrolled),
            OsAuthAvailability::Available(_) => {}
        }
        match grant {
            Some(t) if t.elapsed() <= Self::GRANT_TTL => Ok(()),
            _ => Err(OsAuthError::Failed),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn external_grants_are_single_use_and_need_availability() {
        let a = ExternalOsAuthenticator::new();
        a.grant_once();
        assert_eq!(a.authenticate("x"), Err(OsAuthError::Unsupported));
        a.set_availability(OsAuthAvailability::Available(OsAuthKind::TouchId));
        assert_eq!(a.authenticate("x"), Err(OsAuthError::Failed), "no grant");
        a.grant_once();
        assert_eq!(a.authenticate("x"), Ok(()));
        assert_eq!(a.authenticate("x"), Err(OsAuthError::Failed), "used up");
        a.grant_once();
        a.revoke();
        assert_eq!(a.authenticate("x"), Err(OsAuthError::Failed));
        a.set_availability(OsAuthAvailability::NotEnrolled);
        assert_eq!(a.authenticate("x"), Err(OsAuthError::NotEnrolled));
    }

    #[test]
    fn unsupported_default() {
        let a: &dyn OsAuthenticator = &UnsupportedOsAuthenticator;
        assert_eq!(a.availability(), OsAuthAvailability::Unsupported);
        assert_eq!(a.authenticate("unlock"), Err(OsAuthError::Unsupported));
    }
}
