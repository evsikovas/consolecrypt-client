//! Named secret blobs in a secure store (OS keychain or an in-memory fake).

use secrecy::SecretSlice;
use std::collections::HashMap;
use std::fmt;
use std::sync::Mutex;
use zeroize::Zeroizing;

/// Maximum secret size. Windows Credential Manager caps blobs at 2560
/// bytes; 2048 keeps every backend portable.
pub const MAX_SECRET_LEN: usize = 2048;
/// Maximum secret name length (Windows target names are ≤ 32767 chars, but
/// keep names short and readable).
pub const MAX_SECRET_NAME_LEN: usize = 128;

/// Errors from a [`SecureStore`]. Never contain secret bytes.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum SecureStoreError {
    /// The name is empty, too long or has characters outside
    /// `[A-Za-z0-9._:/-]`.
    #[error("invalid secret name")]
    InvalidName,
    /// The secret exceeds [`MAX_SECRET_LEN`].
    #[error("secret too large ({0} bytes, max {MAX_SECRET_LEN})")]
    TooLarge(usize),
    /// The OS refused access (locked keychain, user denied the prompt, …).
    #[error("secure store access denied: {0}")]
    AccessDenied(String),
    /// The stored item exists but could not be read back as expected.
    #[error("secure store item is corrupted")]
    Corrupted,
    /// No secure store implementation for this platform / build.
    #[error("secure store unsupported: {0}")]
    Unsupported(&'static str),
    /// Any other platform failure (message from the OS, never secret data).
    #[error("secure store failure: {0}")]
    Backend(String),
}

/// A store of small named secret blobs (device keys, database keys, …).
///
/// Implementations must keep values confidential at rest (OS keychain) and
/// must never log values. Calls may block (OS prompts); async callers should
/// use `spawn_blocking`.
pub trait SecureStore: Send + Sync + fmt::Debug {
    /// Read a secret. `Ok(None)` if it does not exist.
    fn get(&self, name: &str) -> Result<Option<SecretSlice<u8>>, SecureStoreError>;
    /// Create or overwrite a secret.
    fn set(&self, name: &str, secret: &[u8]) -> Result<(), SecureStoreError>;
    /// Delete a secret. Returns whether it existed.
    fn delete(&self, name: &str) -> Result<bool, SecureStoreError>;
}

/// Check a secret name: 1..=128 chars of `[A-Za-z0-9._:/-]`.
pub fn validate_secret_name(name: &str) -> Result<(), SecureStoreError> {
    let ok = !name.is_empty()
        && name.len() <= MAX_SECRET_NAME_LEN
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b':' | b'/' | b'-'));
    if ok {
        Ok(())
    } else {
        Err(SecureStoreError::InvalidName)
    }
}

pub(crate) fn validate_secret(secret: &[u8]) -> Result<(), SecureStoreError> {
    if secret.len() > MAX_SECRET_LEN {
        return Err(SecureStoreError::TooLarge(secret.len()));
    }
    Ok(())
}

/// In-process, non-persistent [`SecureStore`] for tests and headless tools.
/// Values are zeroized when overwritten, deleted or dropped.
#[derive(Default)]
pub struct InMemorySecureStore {
    items: Mutex<HashMap<String, Zeroizing<Vec<u8>>>>,
}

impl InMemorySecureStore {
    /// Empty store.
    pub fn new() -> Self {
        Self::default()
    }

    /// Number of stored secrets.
    pub fn len(&self) -> usize {
        self.lock().len()
    }

    /// True if no secret is stored.
    pub fn is_empty(&self) -> bool {
        self.lock().is_empty()
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<String, Zeroizing<Vec<u8>>>> {
        // A poisoned lock only means another thread panicked mid-operation;
        // the map itself is still consistent (single insert/remove calls).
        self.items.lock().unwrap_or_else(|p| p.into_inner())
    }
}

impl fmt::Debug for InMemorySecureStore {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("InMemorySecureStore")
            .field("items", &self.len())
            .finish()
    }
}

impl SecureStore for InMemorySecureStore {
    fn get(&self, name: &str) -> Result<Option<SecretSlice<u8>>, SecureStoreError> {
        validate_secret_name(name)?;
        Ok(self
            .lock()
            .get(name)
            .map(|v| SecretSlice::from(v.as_slice().to_vec())))
    }

    fn set(&self, name: &str, secret: &[u8]) -> Result<(), SecureStoreError> {
        validate_secret_name(name)?;
        validate_secret(secret)?;
        self.lock()
            .insert(name.to_owned(), Zeroizing::new(secret.to_vec()));
        Ok(())
    }

    fn delete(&self, name: &str) -> Result<bool, SecureStoreError> {
        validate_secret_name(name)?;
        Ok(self.lock().remove(name).is_some())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use secrecy::ExposeSecret;

    #[test]
    fn in_memory_roundtrip() {
        let s = InMemorySecureStore::new();
        assert!(s.is_empty());
        assert!(s.get("a/b").unwrap().is_none());
        s.set("a/b", b"one").unwrap();
        assert_eq!(s.get("a/b").unwrap().unwrap().expose_secret(), b"one");
        s.set("a/b", b"two").unwrap();
        assert_eq!(s.get("a/b").unwrap().unwrap().expose_secret(), b"two");
        assert_eq!(s.len(), 1);
        assert!(s.delete("a/b").unwrap());
        assert!(!s.delete("a/b").unwrap());
        assert!(s.get("a/b").unwrap().is_none());
    }

    #[test]
    fn names_and_sizes_validated() {
        let s = InMemorySecureStore::new();
        for bad in ["", "has space", "ümlaut", "semi;colon", &"x".repeat(129)] {
            assert_eq!(
                s.set(bad, b"v"),
                Err(SecureStoreError::InvalidName),
                "{bad}"
            );
        }
        validate_secret_name("profiles/default/device-identity.v1").unwrap();
        assert_eq!(
            s.set("big", &vec![0u8; MAX_SECRET_LEN + 1]),
            Err(SecureStoreError::TooLarge(MAX_SECRET_LEN + 1))
        );
        s.set("max", &vec![0u8; MAX_SECRET_LEN]).unwrap();
    }

    #[test]
    fn debug_does_not_leak_values() {
        let s = InMemorySecureStore::new();
        s.set("k", b"super-secret-value").unwrap();
        let dbg = format!("{s:?} {:?}", s.get("k").unwrap());
        assert!(!dbg.contains("super-secret-value"), "{dbg}");
    }
}
