//! Windows device unlock key: a **Windows Hello** key credential
//! (`KeyCredentialManager`), bound to Hello (face, fingerprint or PIN) and
//! kept by the OS (TPM when available).
//!
//! Key material = the credential's signature over a random 32-byte
//! challenge stored in the handle. Hello signs only after its own prompt;
//! RSASSA-PKCS1-v1_5 signatures are deterministic, so the same challenge
//! always yields the same material. Without configured Hello the option is
//! unavailable. Resetting Hello (or deleting the credential) makes
//! `OpenAsync` report `NotFound` → [`OsAuthError::Invalidated`].
//!
//! Unpackaged desktop apps get the Hello dialog behind their window; a
//! helper thread brings the "Windows Security" dialog to the front.
//!
//! Not runnable on the development Mac: covered by the handle tests here,
//! the protector-agnostic core tests, and the manual test plan.

use crate::auth::{OsAuthError, OsAuthKind};
use crate::device_key::{
    handle, DeviceKeyProtector, DeviceKeyStatus, KeyMaterial, ProtectedKey, UnlockProtection,
};
use windows::core::{Array, HSTRING};
use windows::Security::Credentials::{
    KeyCredential, KeyCredentialCreationOption, KeyCredentialManager, KeyCredentialStatus,
};
use windows::Security::Cryptography::CryptographicBuffer;
use zeroize::Zeroizing;

/// Follow the dialog for the lifetime of one Hello operation. The OS may
/// recreate it when a fingerprint is retried or the user switches to PIN;
/// stopping after the first appearance leaves the replacement behind the app.
/// Dropping the guard stops polling on success, cancellation and API errors.
struct HelloDialogFocus(std::sync::Arc<std::sync::atomic::AtomicBool>);

impl HelloDialogFocus {
    #[allow(unsafe_code)]
    fn start() -> Self {
        use std::sync::atomic::{AtomicBool, Ordering};
        use windows::core::{w, PCWSTR};
        use windows::Win32::UI::WindowsAndMessaging::{
            FindWindowW, GetForegroundWindow, IsWindowVisible, SetForegroundWindow,
        };
        let stopped = std::sync::Arc::new(AtomicBool::new(false));
        let worker_stopped = stopped.clone();
        std::thread::spawn(move || {
            while !worker_stopped.load(Ordering::Acquire) {
                std::thread::sleep(std::time::Duration::from_millis(100));
                if worker_stopped.load(Ordering::Acquire) {
                    break;
                }
                // SAFETY: static class name, null title; returns a window handle
                // or an error, no ownership transfer.
                if let Ok(hwnd) =
                    unsafe { FindWindowW(w!("Credential Dialog Xaml Host"), PCWSTR::null()) }
                {
                    // SAFETY: a borrowed top-level HWND; no ownership transfer.
                    if !hwnd.is_invalid()
                        && unsafe { IsWindowVisible(hwnd).as_bool() }
                        && unsafe { GetForegroundWindow() } != hwnd
                    {
                        // SAFETY: a valid top-level window handle.
                        let _ = unsafe { SetForegroundWindow(hwnd) };
                    }
                }
            }
        });
        Self(stopped)
    }
}

impl Drop for HelloDialogFocus {
    fn drop(&mut self) {
        self.0.store(true, std::sync::atomic::Ordering::Release);
    }
}

const HANDLE_V1: u8 = 1;
const NAME_PREFIX: &str = "ConsoleCrypt/device-unlock/";
// Some Passport providers reject path separators in credential names with
// NTE_INVALID_PARAMETER before presenting any UI. Old handles remain readable.
const SAFE_NAME_PREFIX: &str = "ConsoleCrypt.device-unlock.";
const CHALLENGE_LEN: usize = 32;

fn credential_name(label: &str) -> String {
    // Hex is reversible and collision-free for labels containing ':' or '/'.
    let suffix: String = label
        .as_bytes()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    format!("{SAFE_NAME_PREFIX}{suffix}")
}

/// Windows Hello device unlock keys.
#[derive(Debug, Default)]
pub struct WindowsHelloDeviceKeys;

impl WindowsHelloDeviceKeys {
    pub fn new() -> Self {
        Self
    }
}

fn platform_at(operation: &'static str, e: windows::core::Error) -> OsAuthError {
    OsAuthError::WindowsHello {
        operation,
        diagnostic: format!("HRESULT {:#010x}", e.code().0 as u32),
    }
}

fn status_error(status: KeyCredentialStatus, operation: &'static str) -> OsAuthError {
    if status == KeyCredentialStatus::NotFound {
        OsAuthError::Invalidated
    } else {
        OsAuthError::WindowsHello {
            operation,
            diagnostic: format!("KeyCredentialStatus {}", status.0),
        }
    }
}

fn sign(credential: &KeyCredential, challenge: &[u8]) -> Result<KeyMaterial, OsAuthError> {
    let platform = |e| platform_at("sign", e);
    let data = CryptographicBuffer::CreateFromByteArray(challenge).map_err(platform)?;
    let _focus = HelloDialogFocus::start();
    let result = credential
        .RequestSignAsync(&data)
        .map_err(platform)?
        .join()
        .map_err(platform)?;
    let status = result.Status().map_err(platform)?;
    if status != KeyCredentialStatus::Success {
        return Err(status_error(status, "sign"));
    }
    let signature = result.Result().map_err(platform)?;
    let mut bytes = Array::<u8>::new();
    CryptographicBuffer::CopyToByteArray(&signature, &mut bytes).map_err(platform)?;
    let material = Zeroizing::new(bytes.as_slice().to_vec());
    if material.len() < 32 {
        return Err(OsAuthError::Platform(
            "Windows Hello signature too short".into(),
        ));
    }
    Ok(material)
}

/// Parse a handle: (credential name, challenge).
fn parse(handle_bytes: &[u8]) -> Result<(String, Vec<u8>), OsAuthError> {
    let fields = handle::decode(HANDLE_V1, handle_bytes, 2)?;
    let name = std::str::from_utf8(fields[0]).map_err(|_| OsAuthError::Invalidated)?;
    if !(name.starts_with(NAME_PREFIX) || name.starts_with(SAFE_NAME_PREFIX))
        || fields[1].len() != CHALLENGE_LEN
    {
        return Err(OsAuthError::Invalidated);
    }
    Ok((name.to_owned(), fields[1].to_vec()))
}

impl DeviceKeyProtector for WindowsHelloDeviceKeys {
    fn status(&self) -> DeviceKeyStatus {
        let supported = KeyCredentialManager::IsSupportedAsync()
            .and_then(|op| op.join())
            .unwrap_or(false);
        DeviceKeyStatus {
            kind: supported.then_some(OsAuthKind::WindowsHello),
            protection: UnlockProtection::System,
            // Hello hardware may exist without a PIN; the OS reports both as
            // unsupported, and the UI points to Settings → Sign-in options.
            not_enrolled: false,
            external: false,
        }
    }

    fn create(&self, label: &str, _reason: &str) -> Result<ProtectedKey, OsAuthError> {
        let platform = |e| platform_at("create", e);
        if !self.status().available() {
            return Err(OsAuthError::Unsupported);
        }
        let name = credential_name(label);
        let mut challenge = [0u8; CHALLENGE_LEN];
        getrandom::fill(&mut challenge)
            .map_err(|_| OsAuthError::Platform("OS random generator failed".into()))?;
        // Hello prompt (create).
        let focus = HelloDialogFocus::start();
        let created = KeyCredentialManager::RequestCreateAsync(
            &HSTRING::from(name.as_str()),
            KeyCredentialCreationOption::ReplaceExisting,
        )
        .map_err(platform)?
        .join()
        .map_err(platform)?;
        drop(focus);
        let status = created.Status().map_err(platform)?;
        if status != KeyCredentialStatus::Success {
            return Err(status_error(status, "create"));
        }
        let credential = created.Credential().map_err(platform)?;
        // Second Hello prompt (sign) — proves the key works before enabling.
        let material = sign(&credential, &challenge)?;
        Ok(ProtectedKey {
            material,
            handle: handle::encode(HANDLE_V1, &[name.as_bytes(), &challenge]),
        })
    }

    fn release(&self, handle_bytes: &[u8], _reason: &str) -> Result<KeyMaterial, OsAuthError> {
        let platform = |e| platform_at("open", e);
        let (name, challenge) = parse(handle_bytes)?;
        let opened = KeyCredentialManager::OpenAsync(&HSTRING::from(name.as_str()))
            .map_err(platform)?
            .join()
            .map_err(platform)?;
        let status = opened.Status().map_err(platform)?;
        if status != KeyCredentialStatus::Success {
            return Err(status_error(status, "open"));
        }
        let credential = opened.Credential().map_err(platform)?;
        sign(&credential, &challenge)
    }

    fn destroy(&self, handle_bytes: &[u8]) -> Result<(), OsAuthError> {
        let platform = |e| platform_at("delete", e);
        let (name, _) = parse(handle_bytes)?;
        match KeyCredentialManager::DeleteAsync(&HSTRING::from(name.as_str()))
            .and_then(|op| op.join())
        {
            Ok(()) => Ok(()),
            // Already gone.
            Err(e) if e.code().0 as u32 == 0x8009_0011 => Ok(()),
            Err(e) => Err(platform(e)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_names_are_provider_safe_and_old_handles_still_decode() {
        let name = credential_name("profile-1234/test:one");
        assert!(name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-'));
        assert_ne!(name, credential_name("profile-1234_test:one"));
        let h = handle::encode(HANDLE_V1, &[name.as_bytes(), &[1u8; 32]]);
        assert_eq!(parse(&h).unwrap().0, name);
        let old = handle::encode(
            HANDLE_V1,
            &[b"ConsoleCrypt/device-unlock/profile-1234", &[1u8; 32]],
        );
        assert_eq!(
            parse(&old).unwrap().0,
            "ConsoleCrypt/device-unlock/profile-1234"
        );
    }

    #[test]
    fn handles_are_validated() {
        let h = handle::encode(HANDLE_V1, &[b"ConsoleCrypt/device-unlock/p1", &[1u8; 32]]);
        let (name, challenge) = parse(&h).unwrap();
        assert_eq!(name, "ConsoleCrypt/device-unlock/p1");
        assert_eq!(challenge, vec![1u8; 32]);
        let foreign = handle::encode(HANDLE_V1, &[b"Other/app-key", &[1u8; 32]]);
        assert_eq!(parse(&foreign).err(), Some(OsAuthError::Invalidated));
        let short = handle::encode(HANDLE_V1, &[b"ConsoleCrypt/device-unlock/p1", &[1u8; 8]]);
        assert_eq!(parse(&short).err(), Some(OsAuthError::Invalidated));
    }

    #[test]
    fn statuses_map_to_errors() {
        assert_eq!(
            status_error(KeyCredentialStatus::UserCanceled, "create"),
            OsAuthError::WindowsHello {
                operation: "create",
                diagnostic: "KeyCredentialStatus 3".into()
            }
        );
        assert_eq!(
            status_error(KeyCredentialStatus::NotFound, "open"),
            OsAuthError::Invalidated
        );
        assert_eq!(
            status_error(KeyCredentialStatus::SecurityDeviceLocked, "sign"),
            OsAuthError::WindowsHello {
                operation: "sign",
                diagnostic: "KeyCredentialStatus 6".into()
            }
        );
    }

    /// Shows the Windows Hello prompts. Run manually on Windows:
    /// `cargo test -p cc-platform-core -- --ignored windows_hello_prompt --nocapture`.
    #[test]
    #[ignore = "shows the Windows Hello prompt"]
    fn windows_hello_prompt() {
        let k = WindowsHelloDeviceKeys::new();
        eprintln!("status: {:?}", k.status());
        let created = k.create("manual-test", "create").unwrap();
        let again = k.release(&created.handle, "release").unwrap();
        assert_eq!(*again, *created.material, "deterministic signature");
        k.destroy(&created.handle).unwrap();
        assert_eq!(
            k.release(&created.handle, "after delete").err(),
            Some(OsAuthError::Invalidated)
        );
    }
}
