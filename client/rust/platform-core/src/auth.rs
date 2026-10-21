//! OS user-presence vocabulary shared by the device unlock protectors
//! ([`crate::DeviceKeyProtector`]).
//!
//! There is deliberately no "the UI authenticated the user" grant: the
//! device unlock key is released by the operating system itself (S5).

/// Kind of authentication the OS will perform.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum OsAuthKind {
    TouchId,
    FaceId,
    /// Android fingerprint / class-3 biometrics.
    Fingerprint,
    WindowsHello,
    /// OS account password / PIN / pattern.
    DeviceCredential,
    /// Linux desktop keyring: no prompt, no user-presence control.
    Keyring,
}

/// Why the OS did not release the key.
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
    /// The protected key is gone or the biometric enrollment changed (a
    /// fingerprint was added): unlock with the passphrase and enable again.
    #[error("the device unlock key is no longer valid")]
    Invalidated,
    /// The app performs this OS step itself (Android).
    #[error("the app must perform the OS authentication")]
    ExternalRequired,
    #[error("OS authentication error: {0}")]
    Platform(String),
}
