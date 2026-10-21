//! OS integration: secure storage (macOS Keychain, Windows Credential
//! Manager, Linux Secret Service), app data paths, OS/biometric authentication hooks.
//!
//! * [`SecureStore`] — small named secret blobs; [`OsSecureStore`] (feature
//!   `os-keychain`, default) and [`InMemorySecureStore`] for tests.
//! * [`AppDirs`] / [`ProfilePaths`] — per-profile data layout.
//! * [`DeviceKeyProtector`] — device unlock keys released only by the OS
//!   after biometrics / the device password ([`platform_device_keys`]);
//!   [`UnsupportedDeviceKeys`] declines, [`InMemoryDeviceKeys`] for tests.
//! * [`FileOpener`] / [`open_path`] / [`open_with`] / [`choose_app_and_open`] /
//!   [`reveal`] — open local files with the default or a chosen application.
//! * [`strip_verbatim_prefix`] — the user-facing form of Windows `\\?\` paths.

mod auth;
mod device_key;
#[cfg(all(feature = "os-keychain", any(target_os = "macos", target_os = "ios")))]
mod device_key_apple;
#[cfg(all(feature = "os-keychain", target_os = "windows"))]
mod device_key_windows;
mod dirs;
#[cfg(all(feature = "os-keychain", target_os = "linux"))]
mod linux_secret_service;
pub mod open;
#[cfg(all(feature = "os-keychain", not(target_os = "linux")))]
mod os_store;
mod paths;
mod secure_store;

pub use auth::{OsAuthError, OsAuthKind};
pub use device_key::{
    DeviceKeyProtector, DeviceKeyStatus, ExternalDeviceKeys, FakePrompt, InMemoryDeviceKeys,
    KeyMaterial, ProtectedKey, StoreBackedDeviceKeys, UnlockProtection, UnsupportedDeviceKeys,
};
#[cfg(all(feature = "os-keychain", any(target_os = "macos", target_os = "ios")))]
pub use device_key_apple::SecureEnclaveDeviceKeys;
#[cfg(all(feature = "os-keychain", target_os = "windows"))]
pub use device_key_windows::WindowsHelloDeviceKeys;
pub use dirs::{
    validate_profile_id, AppDirs, DirsError, ProfilePaths, DATA_DIR_ENV, MAX_PROFILE_ID_LEN,
};
#[cfg(all(feature = "os-keychain", target_os = "linux"))]
pub use linux_secret_service::{OsSecureStore, DEFAULT_SERVICE};
pub use open::{
    choose_app_and_open, open_path, open_with, reveal, AppRef, ChooseOutcome, FileOpener, Launcher,
    OpenError, OsFamily, SystemLauncher,
};
#[cfg(all(feature = "os-keychain", not(target_os = "linux")))]
pub use os_store::{OsSecureStore, DEFAULT_SERVICE};
pub use paths::strip_verbatim_prefix;
pub use secure_store::{
    validate_secret_name, InMemorySecureStore, SecureStore, SecureStoreError, MAX_SECRET_LEN,
    MAX_SECRET_NAME_LEN,
};

/// The device unlock protector of this platform: Secure Enclave (macOS,
/// iOS), Windows Hello, the Secret Service as convenience only (Linux,
/// `secure` is that store), app-performed (Android); unsupported otherwise
/// or without the `os-keychain` feature.
#[allow(unused_variables)]
pub fn platform_device_keys(
    secure: std::sync::Arc<dyn SecureStore>,
) -> std::sync::Arc<dyn DeviceKeyProtector> {
    #[cfg(all(feature = "os-keychain", any(target_os = "macos", target_os = "ios")))]
    return std::sync::Arc::new(SecureEnclaveDeviceKeys::new());
    #[cfg(all(feature = "os-keychain", target_os = "windows"))]
    return std::sync::Arc::new(WindowsHelloDeviceKeys::new());
    #[cfg(all(feature = "os-keychain", target_os = "linux"))]
    return std::sync::Arc::new(StoreBackedDeviceKeys::new(secure));
    #[cfg(target_os = "android")]
    return std::sync::Arc::new(ExternalDeviceKeys::new());
    #[allow(unreachable_code)]
    std::sync::Arc::new(UnsupportedDeviceKeys)
}

/// Re-exported so callers can read [`SecureStore::get`] results.
pub use secrecy::{ExposeSecret, SecretSlice};
