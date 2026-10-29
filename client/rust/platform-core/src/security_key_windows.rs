//! Security keys on Windows through `webauthn.dll` (FEAT-16). Windows owns
//! the USB/NFC transport and shows its own PIN and touch dialogs
//! ([`SecurityKeyPin::System`]); no administrator rights are needed. The
//! PRF mode of `WebAuthNAuthenticatorGetAssertion` hashes the salt as
//! WebAuthn PRF, matching the CTAP path on other platforms.
//!
//! Verified with a YubiKey on Windows 11 (WebAuthn API 9) by the
//! `fido-probe` prototype; not runnable on the development Mac.

#![allow(unsafe_code)]

use crate::security_key::{
    SecurityKeyAuthenticator, SecurityKeyError, SecurityKeyPin, SECURITY_KEY_RP_ID,
};
use windows::core::{BOOL, HSTRING, PCWSTR};
use windows::Win32::Foundation::HWND;
use windows::Win32::Networking::WindowsWebServices::*;
use windows::Win32::UI::WindowsAndMessaging::GetForegroundWindow;
use zeroize::Zeroizing;

const ORIGIN: &str = "https://consolecrypt.dev";
const TIMEOUT_MS: u32 = 120_000;

#[derive(Debug, Default)]
pub struct WindowsSecurityKeys;

/// Option struct versions for the installed `webauthn.dll`; `None` before
/// API 4, which has no hmac-secret salts.
fn versions() -> Option<(u32, u32)> {
    // SAFETY: no arguments; returns a version number.
    let api = unsafe { WebAuthNGetApiVersionNumber() };
    let make = match api {
        7.. => WEBAUTHN_AUTHENTICATOR_MAKE_CREDENTIAL_OPTIONS_VERSION_7,
        6 => WEBAUTHN_AUTHENTICATOR_MAKE_CREDENTIAL_OPTIONS_VERSION_6,
        4 | 5 => WEBAUTHN_AUTHENTICATOR_MAKE_CREDENTIAL_OPTIONS_VERSION_5,
        _ => return None,
    };
    let get = match api {
        7.. => WEBAUTHN_AUTHENTICATOR_GET_ASSERTION_OPTIONS_VERSION_7,
        _ => WEBAUTHN_AUTHENTICATOR_GET_ASSERTION_OPTIONS_VERSION_6,
    };
    Some((make, get))
}

fn window() -> HWND {
    // SAFETY: no arguments; the dialog is parented to the app's window.
    unsafe { GetForegroundWindow() }
}

fn random<const N: usize>() -> [u8; N] {
    let mut b = [0u8; N];
    getrandom::fill(&mut b).expect("system random");
    b
}

fn client_data(kind: &str) -> Vec<u8> {
    let challenge: String = random::<32>().iter().map(|b| format!("{b:02x}")).collect();
    format!(
        r#"{{"type":"{kind}","challenge":"{challenge}","origin":"{ORIGIN}","crossOrigin":false}}"#
    )
    .into_bytes()
}

fn error(e: &windows::core::Error) -> SecurityKeyError {
    match e.code().0 as u32 {
        0x800704C7 => SecurityKeyError::Cancelled,
        0x80090011 | 0x80070490 => SecurityKeyError::WrongKey,
        0x80090027 => SecurityKeyError::Unsupported,
        code => SecurityKeyError::Device(format!("0x{code:08X}")),
    }
}

impl SecurityKeyAuthenticator for WindowsSecurityKeys {
    fn pin(&self) -> Option<SecurityKeyPin> {
        versions().map(|_| SecurityKeyPin::System)
    }

    fn register(&self, user_label: &str, _pin: Option<&str>) -> Result<Vec<u8>, SecurityKeyError> {
        let (make_version, _) = versions().ok_or(SecurityKeyError::Unsupported)?;
        let rp_id = HSTRING::from(SECURITY_KEY_RP_ID);
        let rp_name = HSTRING::from("ConsoleCrypt");
        let rp = WEBAUTHN_RP_ENTITY_INFORMATION {
            dwVersion: WEBAUTHN_RP_ENTITY_INFORMATION_CURRENT_VERSION,
            pwszId: PCWSTR(rp_id.as_ptr()),
            pwszName: PCWSTR(rp_name.as_ptr()),
            pwszIcon: PCWSTR::null(),
        };
        let mut user_id = random::<16>();
        let user_name = HSTRING::from(user_label);
        let user = WEBAUTHN_USER_ENTITY_INFORMATION {
            dwVersion: WEBAUTHN_USER_ENTITY_INFORMATION_CURRENT_VERSION,
            cbId: user_id.len() as u32,
            pbId: user_id.as_mut_ptr(),
            pwszName: PCWSTR(user_name.as_ptr()),
            pwszIcon: PCWSTR::null(),
            pwszDisplayName: PCWSTR(user_name.as_ptr()),
        };
        let mut param = WEBAUTHN_COSE_CREDENTIAL_PARAMETER {
            dwVersion: WEBAUTHN_COSE_CREDENTIAL_PARAMETER_CURRENT_VERSION,
            pwszCredentialType: WEBAUTHN_CREDENTIAL_TYPE_PUBLIC_KEY,
            lAlg: WEBAUTHN_COSE_ALGORITHM_ECDSA_P256_WITH_SHA256,
        };
        let params = WEBAUTHN_COSE_CREDENTIAL_PARAMETERS {
            cCredentialParameters: 1,
            pCredentialParameters: &mut param,
        };
        let mut json = client_data("webauthn.create");
        let client = WEBAUTHN_CLIENT_DATA {
            dwVersion: WEBAUTHN_CLIENT_DATA_CURRENT_VERSION,
            cbClientDataJSON: json.len() as u32,
            pbClientDataJSON: json.as_mut_ptr(),
            pwszHashAlgId: WEBAUTHN_HASH_ALGORITHM_SHA_256,
        };
        let mut hmac_on = BOOL(1);
        let mut extension = WEBAUTHN_EXTENSION {
            pwszExtensionIdentifier: WEBAUTHN_EXTENSIONS_IDENTIFIER_HMAC_SECRET,
            cbExtension: std::mem::size_of::<BOOL>() as u32,
            pvExtension: (&mut hmac_on as *mut BOOL).cast(),
        };
        // SAFETY: a plain C struct; zero is the documented default of every field.
        let mut options: WEBAUTHN_AUTHENTICATOR_MAKE_CREDENTIAL_OPTIONS =
            unsafe { std::mem::zeroed() };
        options.dwVersion = make_version;
        options.dwTimeoutMilliseconds = TIMEOUT_MS;
        options.Extensions = WEBAUTHN_EXTENSIONS {
            cExtensions: 1,
            pExtensions: &mut extension,
        };
        options.dwAuthenticatorAttachment = WEBAUTHN_AUTHENTICATOR_ATTACHMENT_CROSS_PLATFORM;
        options.dwUserVerificationRequirement = WEBAUTHN_USER_VERIFICATION_REQUIREMENT_REQUIRED;
        options.dwAttestationConveyancePreference = WEBAUTHN_ATTESTATION_CONVEYANCE_PREFERENCE_NONE;
        let make = |options: &WEBAUTHN_AUTHENTICATOR_MAKE_CREDENTIAL_OPTIONS| {
            // SAFETY: every pointer refers to a local that outlives the call.
            unsafe {
                WebAuthNAuthenticatorMakeCredential(
                    window(),
                    &rp,
                    &user,
                    &params,
                    &client,
                    Some(options),
                )
            }
        };
        // PRF on top of hmac-secret where the API knows it; some builds
        // reject the pair, so retry with hmac-secret alone.
        let attestation =
            if make_version >= WEBAUTHN_AUTHENTICATOR_MAKE_CREDENTIAL_OPTIONS_VERSION_6 {
                options.bEnablePrf = BOOL(1);
                make(&options).or_else(|e| {
                    if error(&e) == SecurityKeyError::Cancelled {
                        return Err(e);
                    }
                    options.bEnablePrf = BOOL(0);
                    make(&options)
                })
            } else {
                make(&options)
            }
            .map_err(|e| error(&e))?;
        // SAFETY: a valid attestation until it is freed below.
        unsafe {
            let a = &*attestation;
            let id =
                std::slice::from_raw_parts(a.pbCredentialId, a.cbCredentialId as usize).to_vec();
            WebAuthNFreeCredentialAttestation(Some(attestation));
            Ok(id)
        }
    }

    fn prf(
        &self,
        credential: &[u8],
        salt: &[u8; 32],
        _pin: Option<&str>,
    ) -> Result<Zeroizing<[u8; 32]>, SecurityKeyError> {
        let (_, get_version) = versions().ok_or(SecurityKeyError::Unsupported)?;
        let rp_id = HSTRING::from(SECURITY_KEY_RP_ID);
        let mut id = credential.to_vec();
        let mut allowed = WEBAUTHN_CREDENTIAL_EX {
            dwVersion: WEBAUTHN_CREDENTIAL_EX_CURRENT_VERSION,
            cbId: id.len() as u32,
            pbId: id.as_mut_ptr(),
            pwszCredentialType: WEBAUTHN_CREDENTIAL_TYPE_PUBLIC_KEY,
            dwTransports: 0,
        };
        let mut allowed_ptr = &mut allowed as *mut WEBAUTHN_CREDENTIAL_EX;
        let mut allow_list = WEBAUTHN_CREDENTIAL_LIST {
            cCredentials: 1,
            ppCredentials: &mut allowed_ptr,
        };
        let mut first = Zeroizing::new(*salt);
        let mut salt_value = WEBAUTHN_HMAC_SECRET_SALT {
            cbFirst: 32,
            pbFirst: first.as_mut_ptr(),
            cbSecond: 0,
            pbSecond: std::ptr::null_mut(),
        };
        let mut salt_values = WEBAUTHN_HMAC_SECRET_SALT_VALUES {
            pGlobalHmacSalt: &mut salt_value,
            cCredWithHmacSecretSaltList: 0,
            pCredWithHmacSecretSaltList: std::ptr::null_mut(),
        };
        let mut json = client_data("webauthn.get");
        let client = WEBAUTHN_CLIENT_DATA {
            dwVersion: WEBAUTHN_CLIENT_DATA_CURRENT_VERSION,
            cbClientDataJSON: json.len() as u32,
            pbClientDataJSON: json.as_mut_ptr(),
            pwszHashAlgId: WEBAUTHN_HASH_ALGORITHM_SHA_256,
        };
        // SAFETY: a plain C struct; zero is the documented default of every field.
        let mut options: WEBAUTHN_AUTHENTICATOR_GET_ASSERTION_OPTIONS =
            unsafe { std::mem::zeroed() };
        options.dwVersion = get_version;
        options.dwTimeoutMilliseconds = TIMEOUT_MS;
        options.dwAuthenticatorAttachment = WEBAUTHN_AUTHENTICATOR_ATTACHMENT_CROSS_PLATFORM;
        options.dwUserVerificationRequirement = WEBAUTHN_USER_VERIFICATION_REQUIREMENT_REQUIRED;
        // dwFlags 0: PRF mode (Windows hashes the salt as WebAuthn PRF).
        options.pAllowCredentialList = &mut allow_list;
        options.pHmacSecretSaltValues = &mut salt_values;
        // SAFETY: every pointer refers to a local that outlives the call.
        let assertion =
            unsafe { WebAuthNAuthenticatorGetAssertion(window(), &rp_id, &client, Some(&options)) }
                .map_err(|e| error(&e))?;
        // SAFETY: a valid assertion until it is freed below.
        unsafe {
            let a = &*assertion;
            let output = if a.pHmacSecret.is_null() {
                Err(SecurityKeyError::NoHmacSecret)
            } else {
                let s = &*a.pHmacSecret;
                if s.cbFirst != 32 || s.pbFirst.is_null() {
                    Err(SecurityKeyError::NoHmacSecret)
                } else {
                    let mut out = Zeroizing::new([0u8; 32]);
                    out.copy_from_slice(std::slice::from_raw_parts(s.pbFirst, 32));
                    Ok(out)
                }
            };
            WebAuthNFreeAssertion(assertion);
            output
        }
    }
}
