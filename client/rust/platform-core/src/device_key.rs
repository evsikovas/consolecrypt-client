//! OS-protected device unlock keys (S5 — "unlock with Touch ID / Windows
//! Hello / fingerprint").
//!
//! A [`DeviceKeyProtector`] creates key material that **the operating
//! system** releases only after its own user-presence check; the core turns
//! that material into the key of the profile's local unlock envelope. No
//! "the UI says the user authenticated" grant exists: bypassing the UI (or
//! reading the OS secret store) yields no key.
//!
//! | Platform | Implementation | Protection |
//! |---|---|---|
//! | macOS, iOS | Secure Enclave P-256 key, `biometryCurrentSet \| devicePasscode` (`device_key_apple.rs`) | [`UnlockProtection::System`] |
//! | Windows | Windows Hello `KeyCredentialManager` signature (`device_key_windows.rs`) | [`UnlockProtection::System`] |
//! | Android | Keystore + `BiometricPrompt` `CryptoObject` in the app ([`ExternalDeviceKeys`]) | [`UnlockProtection::System`] |
//! | Linux | random key in the Secret Service ([`StoreBackedDeviceKeys`]) | [`UnlockProtection::Convenience`] |
//!
//! The opaque `handle` returned by [`DeviceKeyProtector::create`] is
//! persisted by the caller (it is not secret: an SE key blob usable only on
//! this device with the user's biometrics, a credential name, …).

use crate::auth::{OsAuthError, OsAuthKind};
use crate::secure_store::{validate_secret_name, SecureStore, SecureStoreError};
use secrecy::ExposeSecret;
use std::collections::HashMap;
use std::fmt;
use std::sync::{Arc, Mutex};
use zeroize::Zeroizing;

/// Key material released by the OS (≥ 32 bytes; the core derives the
/// unlock key from it with HKDF). Zeroized on drop.
pub type KeyMaterial = Zeroizing<Vec<u8>>;

/// How strongly the platform protects the device unlock key.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnlockProtection {
    /// The OS releases the key only after biometrics / the device password
    /// (Secure Enclave, Android Keystore, Windows Hello).
    System,
    /// No user-presence control: any program of the same OS user can read
    /// the key (Linux Secret Service). Convenience, not protection.
    Convenience,
}

/// What the protector offers right now.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DeviceKeyStatus {
    /// The prompt the user will see; `None` = device unlock unavailable.
    pub kind: Option<OsAuthKind>,
    pub protection: UnlockProtection,
    /// Hardware present, nothing enrolled (no fingerprint / PIN / password).
    pub not_enrolled: bool,
    /// The app (not the core) runs the OS step and hands the core the
    /// material (Android: `BiometricPrompt` needs the activity).
    pub external: bool,
}

impl DeviceKeyStatus {
    /// Device unlock unavailable on this platform.
    pub const UNSUPPORTED: Self = Self {
        kind: None,
        protection: UnlockProtection::System,
        not_enrolled: false,
        external: false,
    };

    /// Whether a key can be created / released.
    pub fn available(&self) -> bool {
        self.kind.is_some()
    }
}

/// A freshly created protected key.
pub struct ProtectedKey {
    /// Material to derive the unlock key from (now, without a second prompt).
    pub material: KeyMaterial,
    /// Opaque, non-secret handle to persist and pass to `release`/`destroy`.
    pub handle: Vec<u8>,
}

impl fmt::Debug for ProtectedKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ProtectedKey")
            .field("handle_len", &self.handle.len())
            .finish_non_exhaustive()
    }
}

/// Creates and releases OS-protected device unlock keys.
///
/// Blocking (OS prompts); async callers use `spawn_blocking`.
pub trait DeviceKeyProtector: Send + Sync + fmt::Debug {
    /// What is available right now.
    fn status(&self) -> DeviceKeyStatus;

    /// Create a new protected key named after `label` (unique per profile,
    /// `[A-Za-z0-9._:/-]`). May show the OS prompt with `reason`.
    fn create(&self, label: &str, reason: &str) -> Result<ProtectedKey, OsAuthError>;

    /// Show the OS prompt with `reason`; on success return the key material
    /// behind `handle`. [`OsAuthError::Invalidated`] when the key no longer
    /// exists or the biometric enrollment changed.
    fn release(&self, handle: &[u8], reason: &str) -> Result<KeyMaterial, OsAuthError>;

    /// Delete the OS-side key (`Ok` if it is already gone).
    fn destroy(&self, handle: &[u8]) -> Result<(), OsAuthError>;
}

// ---- unsupported -------------------------------------------------------------------

/// No device unlock (CLI, headless, platforms without an implementation).
#[derive(Debug, Clone, Copy, Default)]
pub struct UnsupportedDeviceKeys;

impl DeviceKeyProtector for UnsupportedDeviceKeys {
    fn status(&self) -> DeviceKeyStatus {
        DeviceKeyStatus::UNSUPPORTED
    }

    fn create(&self, _label: &str, _reason: &str) -> Result<ProtectedKey, OsAuthError> {
        Err(OsAuthError::Unsupported)
    }

    fn release(&self, _handle: &[u8], _reason: &str) -> Result<KeyMaterial, OsAuthError> {
        Err(OsAuthError::Unsupported)
    }

    fn destroy(&self, _handle: &[u8]) -> Result<(), OsAuthError> {
        Ok(())
    }
}

// ---- external (Android) ------------------------------------------------------------

/// The app performs the OS step itself (Android Keystore key with
/// `setUserAuthenticationRequired` + `BiometricPrompt` `CryptoObject`) and
/// passes the decrypted material to the core, which only verifies it
/// cryptographically (a wrong key cannot open the unlock envelope). The app
/// reports what the OS offers with [`ExternalDeviceKeys::set_status`]; that
/// report affects only what the UI offers, never what the core accepts.
#[derive(Debug)]
pub struct ExternalDeviceKeys {
    status: Mutex<DeviceKeyStatus>,
}

impl Default for ExternalDeviceKeys {
    fn default() -> Self {
        Self::new()
    }
}

impl ExternalDeviceKeys {
    /// Starts unavailable until the app reports.
    pub fn new() -> Self {
        Self {
            status: Mutex::new(DeviceKeyStatus {
                external: true,
                ..DeviceKeyStatus::UNSUPPORTED
            }),
        }
    }

    /// What the app's OS query reported.
    pub fn set_status(&self, kind: Option<OsAuthKind>, not_enrolled: bool) {
        *self.status.lock().unwrap_or_else(|p| p.into_inner()) = DeviceKeyStatus {
            kind,
            protection: UnlockProtection::System,
            not_enrolled,
            external: true,
        };
    }
}

impl DeviceKeyProtector for ExternalDeviceKeys {
    fn status(&self) -> DeviceKeyStatus {
        *self.status.lock().unwrap_or_else(|p| p.into_inner())
    }

    fn create(&self, _label: &str, _reason: &str) -> Result<ProtectedKey, OsAuthError> {
        Err(OsAuthError::ExternalRequired)
    }

    fn release(&self, _handle: &[u8], _reason: &str) -> Result<KeyMaterial, OsAuthError> {
        Err(OsAuthError::ExternalRequired)
    }

    fn destroy(&self, _handle: &[u8]) -> Result<(), OsAuthError> {
        // The app deletes its Keystore alias.
        Ok(())
    }
}

// ---- secure-store backed (Linux) ---------------------------------------------------

/// Convenience-only protector: a random key in the OS secret store, under
/// `device-unlock/<label>`. Readable by any program of the same OS user
/// while the keyring is unlocked — the UI must say so (Linux).
#[derive(Debug)]
pub struct StoreBackedDeviceKeys {
    store: Arc<dyn SecureStore>,
}

impl StoreBackedDeviceKeys {
    pub fn new(store: Arc<dyn SecureStore>) -> Self {
        Self { store }
    }
}

const STORE_HANDLE_PREFIX: &str = "device-unlock/";

fn store_item(handle: &[u8]) -> Result<&str, OsAuthError> {
    let name = std::str::from_utf8(handle).map_err(|_| OsAuthError::Invalidated)?;
    if !name.starts_with(STORE_HANDLE_PREFIX) || validate_secret_name(name).is_err() {
        return Err(OsAuthError::Invalidated);
    }
    Ok(name)
}

fn store_err(e: SecureStoreError) -> OsAuthError {
    match e {
        SecureStoreError::AccessDenied(_) => OsAuthError::Failed,
        SecureStoreError::Unsupported(_) => OsAuthError::Unsupported,
        other => OsAuthError::Platform(other.to_string()),
    }
}

fn random_material() -> Result<KeyMaterial, OsAuthError> {
    let mut m = Zeroizing::new(vec![0u8; 32]);
    getrandom_fill(&mut m)?;
    Ok(m)
}

fn getrandom_fill(buf: &mut [u8]) -> Result<(), OsAuthError> {
    getrandom::fill(buf).map_err(|_| OsAuthError::Platform("OS random generator failed".into()))
}

impl DeviceKeyProtector for StoreBackedDeviceKeys {
    fn status(&self) -> DeviceKeyStatus {
        DeviceKeyStatus {
            kind: Some(OsAuthKind::Keyring),
            protection: UnlockProtection::Convenience,
            not_enrolled: false,
            external: false,
        }
    }

    fn create(&self, label: &str, _reason: &str) -> Result<ProtectedKey, OsAuthError> {
        let name = format!("{STORE_HANDLE_PREFIX}{label}");
        validate_secret_name(&name).map_err(|_| OsAuthError::Platform("invalid label".into()))?;
        let material = random_material()?;
        self.store.set(&name, &material).map_err(store_err)?;
        Ok(ProtectedKey {
            material,
            handle: name.into_bytes(),
        })
    }

    fn release(&self, handle: &[u8], _reason: &str) -> Result<KeyMaterial, OsAuthError> {
        let name = store_item(handle)?;
        match self.store.get(name).map_err(store_err)? {
            Some(v) => Ok(Zeroizing::new(v.expose_secret().to_vec())),
            None => Err(OsAuthError::Invalidated),
        }
    }

    fn destroy(&self, handle: &[u8]) -> Result<(), OsAuthError> {
        let name = store_item(handle)?;
        self.store.delete(name).map(|_| ()).map_err(store_err)
    }
}

// ---- in-memory fake (tests) --------------------------------------------------------

/// Scripted outcome of the next prompt of [`InMemoryDeviceKeys`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FakePrompt {
    /// The user passes the check.
    Succeed,
    /// The user cancels.
    Cancel,
    /// The check fails (wrong finger, lockout).
    Fail,
}

/// System-protected fake for tests: keys live in memory, every create /
/// release "prompts" (counted) and follows the scripted [`FakePrompt`];
/// [`InMemoryDeviceKeys::change_enrollment`] invalidates every key like a
/// new fingerprint does.
#[derive(Debug)]
pub struct InMemoryDeviceKeys {
    state: Mutex<FakeState>,
}

#[derive(Debug)]
struct FakeState {
    status: DeviceKeyStatus,
    next: FakePrompt,
    prompts: usize,
    keys: HashMap<String, Zeroizing<Vec<u8>>>,
}

impl Default for InMemoryDeviceKeys {
    fn default() -> Self {
        Self::new()
    }
}

impl InMemoryDeviceKeys {
    /// Available (Touch ID), every prompt succeeds.
    pub fn new() -> Self {
        Self {
            state: Mutex::new(FakeState {
                status: DeviceKeyStatus {
                    kind: Some(OsAuthKind::TouchId),
                    protection: UnlockProtection::System,
                    not_enrolled: false,
                    external: false,
                },
                next: FakePrompt::Succeed,
                prompts: 0,
                keys: HashMap::new(),
            }),
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, FakeState> {
        self.state.lock().unwrap_or_else(|p| p.into_inner())
    }

    /// Override the reported status.
    pub fn set_status(&self, status: DeviceKeyStatus) {
        self.lock().status = status;
    }

    /// Outcome of every following prompt.
    pub fn set_next(&self, next: FakePrompt) {
        self.lock().next = next;
    }

    /// Prompts shown so far.
    pub fn prompts(&self) -> usize {
        self.lock().prompts
    }

    /// Keys currently held.
    pub fn key_count(&self) -> usize {
        self.lock().keys.len()
    }

    /// A fingerprint was added: every existing key becomes invalid.
    pub fn change_enrollment(&self) {
        self.lock().keys.clear();
    }

    fn prompt(s: &mut FakeState) -> Result<(), OsAuthError> {
        if s.status.kind.is_none() {
            return Err(if s.status.not_enrolled {
                OsAuthError::NotEnrolled
            } else {
                OsAuthError::Unsupported
            });
        }
        s.prompts += 1;
        match s.next {
            FakePrompt::Succeed => Ok(()),
            FakePrompt::Cancel => Err(OsAuthError::Cancelled),
            FakePrompt::Fail => Err(OsAuthError::Failed),
        }
    }
}

impl DeviceKeyProtector for InMemoryDeviceKeys {
    fn status(&self) -> DeviceKeyStatus {
        self.lock().status
    }

    fn create(&self, label: &str, _reason: &str) -> Result<ProtectedKey, OsAuthError> {
        let mut s = self.lock();
        Self::prompt(&mut s)?;
        let material = random_material()?;
        let handle = format!("fake/{label}/{}", s.keys.len());
        s.keys.insert(handle.clone(), material.clone());
        Ok(ProtectedKey {
            material,
            handle: handle.into_bytes(),
        })
    }

    fn release(&self, handle: &[u8], _reason: &str) -> Result<KeyMaterial, OsAuthError> {
        let mut s = self.lock();
        let name = String::from_utf8_lossy(handle).into_owned();
        if !s.keys.contains_key(&name) {
            return Err(OsAuthError::Invalidated);
        }
        Self::prompt(&mut s)?;
        Ok(s.keys[&name].clone())
    }

    fn destroy(&self, handle: &[u8]) -> Result<(), OsAuthError> {
        self.lock().keys.remove(&*String::from_utf8_lossy(handle));
        Ok(())
    }
}

// ---- handle codec ------------------------------------------------------------------

/// Tiny versioned, length-prefixed codec for platform handles
/// (`version || (u16be len || bytes)*`).
#[allow(dead_code)]
pub(crate) mod handle {
    use crate::auth::OsAuthError;

    pub(crate) fn encode(version: u8, fields: &[&[u8]]) -> Vec<u8> {
        let mut out = vec![version];
        for f in fields {
            // Fields are SE blobs, public keys and names: far below 64 KiB.
            let len = u16::try_from(f.len()).unwrap_or(u16::MAX);
            out.extend_from_slice(&len.to_be_bytes());
            out.extend_from_slice(&f[..usize::from(len)]);
        }
        out
    }

    /// Fields of a handle written with `version`; malformed or foreign
    /// handles are treated as an invalidated key.
    pub(crate) fn decode(
        version: u8,
        bytes: &[u8],
        count: usize,
    ) -> Result<Vec<&[u8]>, OsAuthError> {
        let (&v, mut rest) = bytes.split_first().ok_or(OsAuthError::Invalidated)?;
        if v != version {
            return Err(OsAuthError::Invalidated);
        }
        let mut fields = Vec::with_capacity(count);
        while !rest.is_empty() {
            if rest.len() < 2 {
                return Err(OsAuthError::Invalidated);
            }
            let len = usize::from(u16::from_be_bytes([rest[0], rest[1]]));
            rest = &rest[2..];
            if rest.len() < len {
                return Err(OsAuthError::Invalidated);
            }
            fields.push(&rest[..len]);
            rest = &rest[len..];
        }
        if fields.len() != count {
            return Err(OsAuthError::Invalidated);
        }
        Ok(fields)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::InMemorySecureStore;

    #[test]
    fn fake_prompts_cancel_and_invalidation() {
        let k = InMemoryDeviceKeys::new();
        let created = k.create("p1", "r").unwrap();
        assert_eq!(created.material.len(), 32);
        assert_eq!(k.prompts(), 1);
        let again = k.release(&created.handle, "r").unwrap();
        assert_eq!(*again, *created.material);
        k.set_next(FakePrompt::Cancel);
        assert_eq!(k.release(&created.handle, "r"), Err(OsAuthError::Cancelled));
        k.set_next(FakePrompt::Fail);
        assert_eq!(k.release(&created.handle, "r"), Err(OsAuthError::Failed));
        k.set_next(FakePrompt::Succeed);
        k.change_enrollment();
        assert_eq!(
            k.release(&created.handle, "r"),
            Err(OsAuthError::Invalidated)
        );
        k.set_status(DeviceKeyStatus {
            not_enrolled: true,
            ..DeviceKeyStatus::UNSUPPORTED
        });
        assert_eq!(k.create("p1", "r").err(), Some(OsAuthError::NotEnrolled));
    }

    #[test]
    fn store_backed_is_convenience_and_roundtrips() {
        let store = Arc::new(InMemorySecureStore::new());
        let k = StoreBackedDeviceKeys::new(store.clone());
        assert_eq!(k.status().protection, UnlockProtection::Convenience);
        assert!(k.status().available());
        let c = k.create("p1", "r").unwrap();
        assert_eq!(c.handle, b"device-unlock/p1");
        assert_eq!(*k.release(&c.handle, "r").unwrap(), *c.material);
        // Not a store item of ours: never read arbitrary secrets by handle.
        store.set("profiles/x/db-key", b"secret").unwrap();
        assert_eq!(
            k.release(b"profiles/x/db-key", "r"),
            Err(OsAuthError::Invalidated)
        );
        k.destroy(&c.handle).unwrap();
        assert_eq!(k.release(&c.handle, "r"), Err(OsAuthError::Invalidated));
        assert!(k.create("bad label", "r").is_err());
    }

    #[test]
    fn external_reports_only() {
        let k = ExternalDeviceKeys::new();
        assert!(!k.status().available());
        assert!(k.status().external);
        k.set_status(Some(OsAuthKind::Fingerprint), false);
        assert!(k.status().available());
        assert_eq!(
            k.create("p", "r").err(),
            Some(OsAuthError::ExternalRequired)
        );
        assert_eq!(k.release(b"x", "r"), Err(OsAuthError::ExternalRequired));
    }

    #[test]
    fn unsupported_declines() {
        let k = UnsupportedDeviceKeys;
        assert!(!k.status().available());
        assert_eq!(k.create("p", "r").err(), Some(OsAuthError::Unsupported));
        assert_eq!(k.release(b"x", "r"), Err(OsAuthError::Unsupported));
    }

    #[test]
    fn handle_codec_rejects_foreign_data() {
        let h = handle::encode(1, &[b"abc", b"", &[9u8; 65]]);
        let f = handle::decode(1, &h, 3).unwrap();
        assert_eq!(f[0], b"abc");
        assert!(f[1].is_empty());
        assert_eq!(f[2].len(), 65);
        assert_eq!(handle::decode(2, &h, 3), Err(OsAuthError::Invalidated));
        assert_eq!(handle::decode(1, &h, 2), Err(OsAuthError::Invalidated));
        assert_eq!(
            handle::decode(1, &h[..h.len() - 1], 3),
            Err(OsAuthError::Invalidated)
        );
        assert_eq!(handle::decode(1, &[], 0), Err(OsAuthError::Invalidated));
    }
}
