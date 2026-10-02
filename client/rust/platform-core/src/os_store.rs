//! OS secure store: macOS Keychain, Windows Credential Manager and Android Keystore, via the
//! keyring 4.x ecosystem (`keyring-core` + native store crates).

use crate::secure_store::{validate_secret, validate_secret_name, SecureStore, SecureStoreError};
use keyring_core::{CredentialStore, Entry, Error as KeyringError};
use secrecy::SecretSlice;
use std::fmt;
use std::sync::Arc;
use zeroize::Zeroize;

/// Default keychain service / credential target prefix.
pub const DEFAULT_SERVICE: &str = "io.consolecrypt.ConsoleCrypt";

/// [`SecureStore`] backed by the operating system.
///
/// Every secret is a generic-password item with `service` = the store's
/// service name and `account` = the secret name.
pub struct OsSecureStore {
    service: String,
    store: Arc<CredentialStore>,
}

impl OsSecureStore {
    /// Open the platform store with the [`DEFAULT_SERVICE`] name.
    pub fn new() -> Result<Self, SecureStoreError> {
        Self::with_service(DEFAULT_SERVICE)
    }

    /// Open the platform store with a custom service name (e.g. to isolate
    /// test runs or a portable installation).
    pub fn with_service(service: &str) -> Result<Self, SecureStoreError> {
        validate_secret_name(service)?;
        Ok(Self {
            service: service.to_owned(),
            store: platform_store()?,
        })
    }

    fn entry(&self, name: &str) -> Result<Entry, SecureStoreError> {
        validate_secret_name(name)?;
        #[cfg(target_os = "ios")]
        let modifiers = Some(std::collections::HashMap::from([(
            "access-policy",
            "when-unlocked-this-device-only",
        )]));
        #[cfg(not(target_os = "ios"))]
        let modifiers: Option<std::collections::HashMap<&str, &str>> = None;
        self.store
            .build(&self.service, name, modifiers.as_ref())
            .map_err(map_error)
    }
}

#[cfg(target_os = "macos")]
fn platform_store() -> Result<Arc<CredentialStore>, SecureStoreError> {
    let store: Arc<CredentialStore> =
        apple_native_keyring_store::keychain::Store::new().map_err(map_error)?;
    Ok(store)
}

#[cfg(target_os = "windows")]
fn platform_store() -> Result<Arc<CredentialStore>, SecureStoreError> {
    let store: Arc<CredentialStore> =
        windows_native_keyring_store::Store::new().map_err(map_error)?;
    Ok(store)
}

#[cfg(target_os = "ios")]
fn platform_store() -> Result<Arc<CredentialStore>, SecureStoreError> {
    // Default app access group, explicitly local: device identity and local
    // SQLCipher keys must never migrate through iCloud Keychain or backups.
    let configuration = std::collections::HashMap::from([("cloud-sync", "false")]);
    let store: Arc<CredentialStore> =
        apple_native_keyring_store::protected::Store::new_with_configuration(&configuration)
            .map_err(map_error)?;
    Ok(store)
}

#[cfg(target_os = "android")]
fn platform_store() -> Result<Arc<CredentialStore>, SecureStoreError> {
    // AES-GCM ciphertext in private SharedPreferences; the wrapping key
    // remains in Android Keystore. The runner initializes NDK context first.
    let store: Arc<CredentialStore> =
        android_native_keyring_store::Store::new().map_err(map_error)?;
    Ok(store)
}

// Linux uses the strict Secret Service backend in linux_secret_service.rs.
// Other unsupported targets must fail closed; no file-based fallback.
#[cfg(not(any(
    target_os = "macos",
    target_os = "windows",
    target_os = "android",
    target_os = "ios"
)))]
fn platform_store() -> Result<Arc<CredentialStore>, SecureStoreError> {
    Err(SecureStoreError::Unsupported(
        "no OS secure store for this platform yet",
    ))
}

/// Map keyring errors without ever formatting payloads that may contain
/// secret bytes (`BadEncoding`, `BadDataFormat`).
fn map_error(e: KeyringError) -> SecureStoreError {
    match e {
        KeyringError::NoStorageAccess(p) => SecureStoreError::AccessDenied(p.to_string()),
        KeyringError::PlatformFailure(p) => SecureStoreError::Backend(p.to_string()),
        KeyringError::BadEncoding(mut bytes) | KeyringError::BadDataFormat(mut bytes, _) => {
            bytes.zeroize();
            SecureStoreError::Corrupted
        }
        KeyringError::Invalid(_, _) | KeyringError::TooLong(_, _) => SecureStoreError::InvalidName,
        KeyringError::NoEntry => SecureStoreError::Backend("no entry".into()),
        KeyringError::Ambiguous(_) => SecureStoreError::Backend("ambiguous entry".into()),
        KeyringError::BadStoreFormat(m) => SecureStoreError::Backend(m),
        KeyringError::NoDefaultStore => SecureStoreError::Unsupported("no credential store"),
        KeyringError::NotSupportedByStore(m) => SecureStoreError::Backend(m),
        _ => SecureStoreError::Backend("unknown keyring error".into()),
    }
}

impl fmt::Debug for OsSecureStore {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("OsSecureStore")
            .field("service", &self.service)
            .finish_non_exhaustive()
    }
}

impl SecureStore for OsSecureStore {
    fn get(&self, name: &str) -> Result<Option<SecretSlice<u8>>, SecureStoreError> {
        match self.entry(name)?.get_secret() {
            Ok(bytes) => Ok(Some(SecretSlice::from(bytes))),
            Err(KeyringError::NoEntry) => Ok(None),
            Err(e) => Err(map_error(e)),
        }
    }

    fn set(&self, name: &str, secret: &[u8]) -> Result<(), SecureStoreError> {
        validate_secret(secret)?;
        self.entry(name)?.set_secret(secret).map_err(map_error)
    }

    fn delete(&self, name: &str) -> Result<bool, SecureStoreError> {
        match self.entry(name)?.delete_credential() {
            Ok(()) => Ok(true),
            Err(KeyringError::NoEntry) => Ok(false),
            Err(e) => Err(map_error(e)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use secrecy::ExposeSecret;

    /// Touches the real OS keychain (may prompt on macOS). Run manually:
    /// `cargo test -p cc-platform-core -- --ignored os_store`.
    #[test]
    #[ignore = "uses the real OS keychain"]
    fn os_store_roundtrip() {
        let store = OsSecureStore::with_service("io.consolecrypt.test").unwrap();
        let name = "cc-platform-core/test-roundtrip";
        let _ = store.delete(name);
        assert!(store.get(name).unwrap().is_none());
        store.set(name, &[1, 2, 3, 0, 255]).unwrap();
        assert_eq!(
            store.get(name).unwrap().unwrap().expose_secret(),
            &[1, 2, 3, 0, 255]
        );
        assert!(store.delete(name).unwrap());
        assert!(!store.delete(name).unwrap());
    }

    #[test]
    fn invalid_names_rejected_before_os_call() {
        if let Ok(store) = OsSecureStore::new() {
            assert_eq!(
                store.get("bad name").err(),
                Some(SecureStoreError::InvalidName)
            );
        }
        assert!(OsSecureStore::with_service("bad service").is_err());
    }
}
