//! OS integration: secure storage (macOS Keychain, Windows Credential
//! Manager), app data paths, OS/biometric authentication hooks.
//!
//! * [`SecureStore`] — small named secret blobs; [`OsSecureStore`] (feature
//!   `os-keychain`, default) and [`InMemorySecureStore`] for tests.
//! * [`AppDirs`] / [`ProfilePaths`] — per-profile data layout.
//! * [`OsAuthenticator`] — biometric / OS credential prompt hook; the default
//!   [`UnsupportedOsAuthenticator`] always declines.
//! * [`FileOpener`] / [`open_path`] / [`open_with`] / [`choose_app_and_open`] /
//!   [`reveal`] — open local files with the default or a chosen application.

mod auth;
mod dirs;
pub mod open;
#[cfg(feature = "os-keychain")]
mod os_store;
mod secure_store;

pub use auth::{
    ExternalOsAuthenticator, OsAuthAvailability, OsAuthError, OsAuthKind, OsAuthenticator,
    UnsupportedOsAuthenticator,
};
pub use dirs::{
    validate_profile_id, AppDirs, DirsError, ProfilePaths, DATA_DIR_ENV, MAX_PROFILE_ID_LEN,
};
pub use open::{
    choose_app_and_open, open_path, open_with, reveal, AppRef, ChooseOutcome, FileOpener, Launcher,
    OpenError, OsFamily, SystemLauncher,
};
#[cfg(feature = "os-keychain")]
pub use os_store::{OsSecureStore, DEFAULT_SERVICE};
pub use secure_store::{
    validate_secret_name, InMemorySecureStore, SecureStore, SecureStoreError, MAX_SECRET_LEN,
    MAX_SECRET_NAME_LEN,
};

/// Re-exported so callers can read [`SecureStore::get`] results.
pub use secrecy::{ExposeSecret, SecretSlice};
