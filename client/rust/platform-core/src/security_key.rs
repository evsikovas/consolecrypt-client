//! FIDO2 security key (YubiKey and others) as a vault unlock factor
//! (FEAT-16).
//!
//! The key never hands out a stored secret: for a credential it registered
//! it answers a 32-byte salt with an HMAC (the `hmac-secret` extension).
//! ConsoleCrypt uses WebAuthn PRF semantics so one key gives the same value
//! on every OS: Windows asks `webauthn.dll` in PRF mode, the CTAP path hashes
//! the salt the same way (`SHA-256("WebAuthn PRF" || 0x00 || salt)`) before
//! sending it as the hmac-secret salt. The PIN is always required.
//!
//! Calls block until the user touches the key; run them off the async
//! runtime.

use std::sync::{Arc, Mutex};
use zeroize::Zeroizing;

/// Relying party of every ConsoleCrypt credential, on every platform.
pub const SECURITY_KEY_RP_ID: &str = "consolecrypt.dev";

/// Who asks for the security key PIN.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SecurityKeyPin {
    /// The OS dialog (Windows): the app passes no PIN.
    System,
    /// The app asks and passes it (CTAP over HID: macOS).
    App,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SecurityKeyError {
    #[error("security keys are not supported on this platform")]
    Unsupported,
    #[error("no security key is connected")]
    NoDevice,
    #[error("more than one security key is connected")]
    SeveralDevices,
    #[error("the security key PIN is required")]
    PinRequired,
    #[error("the security key PIN is wrong")]
    PinInvalid,
    #[error("the security key PIN is blocked")]
    PinBlocked,
    #[error("the request was cancelled or timed out")]
    Cancelled,
    #[error("this is not the registered security key")]
    WrongKey,
    #[error("the security key does not support hmac-secret")]
    NoHmacSecret,
    #[error("security key error: {0}")]
    Device(String),
}

/// A FIDO2 authenticator reachable from this platform.
pub trait SecurityKeyAuthenticator: Send + Sync + std::fmt::Debug {
    /// `None`: security keys are not supported here.
    fn pin(&self) -> Option<SecurityKeyPin>;

    /// Register a new non-resident credential with hmac-secret; returns its
    /// id. `pin` is used only with [`SecurityKeyPin::App`].
    fn register(&self, user_label: &str, pin: Option<&str>) -> Result<Vec<u8>, SecurityKeyError>;

    /// The PRF output of `salt` for `credential`.
    fn prf(
        &self,
        credential: &[u8],
        salt: &[u8; 32],
        pin: Option<&str>,
    ) -> Result<Zeroizing<[u8; 32]>, SecurityKeyError>;
}

/// The salt sent as hmac-secret input for a PRF `salt` (WebAuthn L3 §10.1.4).
pub fn prf_salt(salt: &[u8; 32]) -> [u8; 32] {
    use sha2::Digest;
    let mut h = sha2::Sha256::new();
    h.update(b"WebAuthn PRF");
    h.update([0u8]);
    h.update(salt);
    h.finalize().into()
}

/// No security key support (Linux, mobile, builds without `os-keychain`).
#[derive(Debug, Default)]
pub struct UnsupportedSecurityKeys;

impl SecurityKeyAuthenticator for UnsupportedSecurityKeys {
    fn pin(&self) -> Option<SecurityKeyPin> {
        None
    }
    fn register(&self, _: &str, _: Option<&str>) -> Result<Vec<u8>, SecurityKeyError> {
        Err(SecurityKeyError::Unsupported)
    }
    fn prf(
        &self,
        _: &[u8],
        _: &[u8; 32],
        _: Option<&str>,
    ) -> Result<Zeroizing<[u8; 32]>, SecurityKeyError> {
        Err(SecurityKeyError::Unsupported)
    }
}

/// Test authenticator: one simulated key with a random secret and PIN
/// `1234` (asked by the app). [`InMemorySecurityKey::replace`] simulates
/// plugging in a different key.
#[derive(Debug)]
pub struct InMemorySecurityKey {
    secret: Mutex<[u8; 32]>,
    credentials: Mutex<Vec<Vec<u8>>>,
}

impl Default for InMemorySecurityKey {
    fn default() -> Self {
        Self {
            secret: Mutex::new(random()),
            credentials: Mutex::new(Vec::new()),
        }
    }
}

impl InMemorySecurityKey {
    pub const PIN: &'static str = "1234";

    pub fn replace(&self) {
        *self.secret.lock().unwrap_or_else(|e| e.into_inner()) = random();
        self.credentials
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clear();
    }

    fn check_pin(pin: Option<&str>) -> Result<(), SecurityKeyError> {
        match pin {
            None => Err(SecurityKeyError::PinRequired),
            Some(Self::PIN) => Ok(()),
            Some(_) => Err(SecurityKeyError::PinInvalid),
        }
    }
}

impl SecurityKeyAuthenticator for InMemorySecurityKey {
    fn pin(&self) -> Option<SecurityKeyPin> {
        Some(SecurityKeyPin::App)
    }

    fn register(&self, _: &str, pin: Option<&str>) -> Result<Vec<u8>, SecurityKeyError> {
        Self::check_pin(pin)?;
        let id = random::<32>().to_vec();
        self.credentials
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .push(id.clone());
        Ok(id)
    }

    fn prf(
        &self,
        credential: &[u8],
        salt: &[u8; 32],
        pin: Option<&str>,
    ) -> Result<Zeroizing<[u8; 32]>, SecurityKeyError> {
        Self::check_pin(pin)?;
        if !self
            .credentials
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .iter()
            .any(|c| c == credential)
        {
            return Err(SecurityKeyError::WrongKey);
        }
        use sha2::Digest;
        let mut h = sha2::Sha256::new();
        h.update(*self.secret.lock().unwrap_or_else(|e| e.into_inner()));
        h.update(credential);
        h.update(prf_salt(salt));
        Ok(Zeroizing::new(h.finalize().into()))
    }
}

fn random<const N: usize>() -> [u8; N] {
    let mut b = [0u8; N];
    getrandom::fill(&mut b).expect("system random");
    b
}

/// The security key authenticator of this platform.
pub fn platform_security_keys() -> Arc<dyn SecurityKeyAuthenticator> {
    #[cfg(all(feature = "os-keychain", target_os = "windows"))]
    return Arc::new(crate::security_key_windows::WindowsSecurityKeys);
    #[cfg(all(feature = "os-keychain", target_os = "macos"))]
    return Arc::new(crate::security_key_ctap::CtapSecurityKeys);
    #[allow(unreachable_code)]
    Arc::new(UnsupportedSecurityKeys)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prf_salt_matches_the_webauthn_definition() {
        use sha2::Digest;
        let salt = [7u8; 32];
        let mut input = b"WebAuthn PRF\0".to_vec();
        input.extend_from_slice(&salt);
        let expected: [u8; 32] = sha2::Sha256::digest(&input).into();
        assert_eq!(prf_salt(&salt), expected);
    }

    #[test]
    fn in_memory_key_is_stable_per_salt_and_checks_pin_and_key() {
        let key = InMemorySecurityKey::default();
        assert_eq!(
            key.register("x", None).unwrap_err(),
            SecurityKeyError::PinRequired
        );
        let id = key.register("x", Some("1234")).unwrap();
        let a = key.prf(&id, &[1; 32], Some("1234")).unwrap();
        assert_eq!(*a, *key.prf(&id, &[1; 32], Some("1234")).unwrap());
        assert_ne!(*a, *key.prf(&id, &[2; 32], Some("1234")).unwrap());
        assert_eq!(
            key.prf(&id, &[1; 32], Some("0000")).unwrap_err(),
            SecurityKeyError::PinInvalid
        );
        key.replace();
        assert_eq!(
            key.prf(&id, &[1; 32], Some("1234")).unwrap_err(),
            SecurityKeyError::WrongKey
        );
    }
}
