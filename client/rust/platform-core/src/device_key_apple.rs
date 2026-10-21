//! macOS / iOS device unlock key: a **Secure Enclave** P-256 key whose use
//! requires `biometryCurrentSet` *or* the device password
//! (`SecAccessControl`, `kSecAttrAccessibleWhenUnlockedThisDeviceOnly`).
//!
//! Why not a data-protection keychain item: that keychain needs a
//! keychain-access-group entitlement (a provisioning profile), which the
//! directly distributed, ad-hoc signed macOS app does not have
//! (`errSecMissingEntitlement`). Secure Enclave keys work without it: the
//! private key never leaves the enclave; the non-secret, device-bound key
//! blob (`toid`) is what the core persists as the handle.
//!
//! Key material = `ECDH(SE private key, ephemeral public key)`:
//! * create — a throwaway software P-256 key computes the shared secret with
//!   the SE *public* key (no prompt); its public half goes into the handle;
//! * release — the enclave computes the same secret with its private key,
//!   which the OS allows only after Touch ID / the password (`LAContext`
//!   prompt with our reason).
//!
//! Enrollment changes: the handle records `LAContext`'s biometric domain
//! state at creation; a different state at release (fingerprint added or
//! removed) → [`OsAuthError::Invalidated`] (the `.or(devicePasscode)` branch
//! would otherwise keep the key usable).
//!
//! FFI module: every `unsafe` block calls Security / LocalAuthentication /
//! CoreFoundation with owned or framework-constant references.
#![allow(unsafe_code)]

use crate::auth::{OsAuthError, OsAuthKind};
use crate::device_key::{
    handle, DeviceKeyProtector, DeviceKeyStatus, KeyMaterial, ProtectedKey, UnlockProtection,
};
use core_foundation::base::{CFOptionFlags, CFRelease, CFType, CFTypeRef, TCFType};
use core_foundation::boolean::CFBoolean;
use core_foundation::data::{CFData, CFDataRef};
use core_foundation::dictionary::{CFDictionary, CFDictionaryRef};
use core_foundation::error::{CFError, CFErrorRef};
use core_foundation::number::CFNumber;
use core_foundation::string::{CFString, CFStringRef};
use objc2::msg_send;
use objc2::rc::Retained;
use objc2::runtime::{AnyClass, AnyObject};
use std::ffi::c_void;
use zeroize::Zeroizing;

type SecKeyRef = *mut c_void;
type SecAccessControlRef = *mut c_void;

#[link(name = "Security", kind = "framework")]
extern "C" {
    static kSecAttrKeyType: CFStringRef;
    static kSecAttrKeyTypeECSECPrimeRandom: CFStringRef;
    static kSecAttrKeySizeInBits: CFStringRef;
    static kSecAttrKeyClass: CFStringRef;
    static kSecAttrKeyClassPrivate: CFStringRef;
    static kSecAttrKeyClassPublic: CFStringRef;
    static kSecAttrTokenID: CFStringRef;
    static kSecAttrTokenIDSecureEnclave: CFStringRef;
    static kSecAttrIsPermanent: CFStringRef;
    static kSecAttrAccessControl: CFStringRef;
    static kSecPrivateKeyAttrs: CFStringRef;
    static kSecUseAuthenticationContext: CFStringRef;
    static kSecAttrAccessibleWhenUnlockedThisDeviceOnly: CFStringRef;
    static kSecKeyAlgorithmECDHKeyExchangeStandard: CFStringRef;

    fn SecAccessControlCreateWithFlags(
        allocator: *const c_void,
        protection: CFTypeRef,
        flags: CFOptionFlags,
        error: *mut CFErrorRef,
    ) -> SecAccessControlRef;
    fn SecKeyCreateRandomKey(parameters: CFDictionaryRef, error: *mut CFErrorRef) -> SecKeyRef;
    fn SecKeyCreateWithData(
        key_data: CFDataRef,
        attributes: CFDictionaryRef,
        error: *mut CFErrorRef,
    ) -> SecKeyRef;
    fn SecKeyCopyAttributes(key: SecKeyRef) -> CFDictionaryRef;
    fn SecKeyCopyPublicKey(key: SecKeyRef) -> SecKeyRef;
    fn SecKeyCopyExternalRepresentation(key: SecKeyRef, error: *mut CFErrorRef) -> CFDataRef;
    fn SecKeyCopyKeyExchangeResult(
        private_key: SecKeyRef,
        algorithm: CFStringRef,
        public_key: SecKeyRef,
        parameters: CFDictionaryRef,
        error: *mut CFErrorRef,
    ) -> CFDataRef;
}

#[link(name = "LocalAuthentication", kind = "framework")]
extern "C" {}

// SecAccessControlCreateFlags.
const BIOMETRY_CURRENT_SET: CFOptionFlags = 1 << 3;
const DEVICE_PASSCODE: CFOptionFlags = 1 << 4;
const OR: CFOptionFlags = 1 << 14;
const PRIVATE_KEY_USAGE: CFOptionFlags = 1 << 30;

// LAPolicy.
const POLICY_BIOMETRICS: isize = 1;
const POLICY_OWNER_AUTH: isize = 2;
// LABiometryType.
const BIOMETRY_TOUCH_ID: isize = 1;
const BIOMETRY_FACE_ID: isize = 2;
// LAError codes.
const LA_AUTH_FAILED: isize = -1;
const LA_USER_CANCEL: isize = -2;
const LA_USER_FALLBACK: isize = -3;
const LA_SYSTEM_CANCEL: isize = -4;
const LA_PASSCODE_NOT_SET: isize = -5;
const LA_BIOMETRY_NOT_AVAILABLE: isize = -6;
const LA_BIOMETRY_NOT_ENROLLED: isize = -7;
const LA_BIOMETRY_LOCKOUT: isize = -8;
const LA_APP_CANCEL: isize = -9;
const LA_INVALID_CONTEXT: isize = -10;
const LA_NOT_INTERACTIVE: isize = -1004;
// OSStatus.
const ERR_SEC_USER_CANCELED: isize = -128;
const ERR_SEC_AUTH_FAILED: isize = -25293;
const ERR_SEC_ITEM_NOT_FOUND: isize = -25300;
const ERR_SEC_INTERACTION_NOT_ALLOWED: isize = -25308;

const HANDLE_V1: u8 = 1;
/// Private `kSecAttrTokenOID`: the enclave-encrypted key blob.
const TOKEN_OID: &str = "toid";

/// Secure Enclave device unlock keys (macOS with Touch ID or a password,
/// iOS with Face ID / Touch ID / passcode).
#[derive(Debug, Default)]
pub struct SecureEnclaveDeviceKeys;

impl SecureEnclaveDeviceKeys {
    pub fn new() -> Self {
        Self
    }
}

/// Owned CoreFoundation-style reference released on drop.
struct Owned(*mut c_void);

impl Owned {
    fn new(p: *mut c_void) -> Option<Self> {
        (!p.is_null()).then(|| Self(p))
    }
}

impl Drop for Owned {
    fn drop(&mut self) {
        // SAFETY: non-null, owned (+1) CF object from a Create/Copy call.
        unsafe { CFRelease(self.0 as CFTypeRef) }
    }
}

fn cfstr(s: CFStringRef) -> CFString {
    // SAFETY: framework constants are valid, immortal CFStrings.
    unsafe { CFString::wrap_under_get_rule(s) }
}

fn take_error(err: CFErrorRef) -> Option<CFError> {
    // SAFETY: an out-error from a Security call is owned (+1) when non-null.
    (!err.is_null()).then(|| unsafe { CFError::wrap_under_create_rule(err) })
}

fn map_cf_error(err: Option<CFError>) -> OsAuthError {
    let Some(e) = err else {
        return OsAuthError::Platform("Secure Enclave operation failed".into());
    };
    let code = e.code();
    let domain = e.domain().to_string();
    if domain == "com.apple.LocalAuthentication" {
        return map_la_code(code);
    }
    match code {
        ERR_SEC_USER_CANCELED => OsAuthError::Cancelled,
        ERR_SEC_AUTH_FAILED => OsAuthError::Failed,
        ERR_SEC_ITEM_NOT_FOUND => OsAuthError::Invalidated,
        // Screen locked / keybag not ready (WhenUnlockedThisDeviceOnly).
        ERR_SEC_INTERACTION_NOT_ALLOWED => OsAuthError::Failed,
        _ => OsAuthError::Platform(format!("{domain} {code}")),
    }
}

fn map_la_code(code: isize) -> OsAuthError {
    match code {
        LA_USER_CANCEL | LA_SYSTEM_CANCEL | LA_APP_CANCEL | LA_USER_FALLBACK => {
            OsAuthError::Cancelled
        }
        LA_AUTH_FAILED | LA_BIOMETRY_LOCKOUT | LA_NOT_INTERACTIVE => OsAuthError::Failed,
        LA_PASSCODE_NOT_SET | LA_BIOMETRY_NOT_ENROLLED => OsAuthError::NotEnrolled,
        LA_BIOMETRY_NOT_AVAILABLE => OsAuthError::Unsupported,
        LA_INVALID_CONTEXT => OsAuthError::Failed,
        other => OsAuthError::Platform(format!("LocalAuthentication {other}")),
    }
}

// ---- LAContext ---------------------------------------------------------------------

struct LaContext(Retained<AnyObject>);

impl LaContext {
    fn new() -> Result<Self, OsAuthError> {
        let cls = AnyClass::get(c"LAContext").ok_or(OsAuthError::Unsupported)?;
        // SAFETY: `+[LAContext new]` returns a retained instance.
        let obj: Option<Retained<AnyObject>> = unsafe { msg_send![cls, new] };
        obj.map(Self).ok_or(OsAuthError::Unsupported)
    }

    /// `canEvaluatePolicy:error:`; the LAError code on failure.
    fn can_evaluate(&self, policy: isize) -> Result<(), isize> {
        let mut err: *mut AnyObject = std::ptr::null_mut();
        // SAFETY: valid receiver and selector; `err` is an autoreleased
        // NSError out-pointer (not owned).
        let ok: bool = unsafe { msg_send![&*self.0, canEvaluatePolicy: policy, error: &mut err] };
        if ok {
            return Ok(());
        }
        if err.is_null() {
            return Err(LA_BIOMETRY_NOT_AVAILABLE);
        }
        // SAFETY: NSError responds to `code`.
        let code: isize = unsafe { msg_send![err, code] };
        Err(code)
    }

    fn biometry_type(&self) -> isize {
        // SAFETY: `biometryType` is a property of LAContext (NSInteger).
        unsafe { msg_send![&*self.0, biometryType] }
    }

    /// Opaque biometric enrollment state (after a biometrics
    /// `can_evaluate`); empty without biometrics.
    fn domain_state(&self) -> Vec<u8> {
        // SAFETY: returns an autoreleased NSData or nil; NSData is
        // toll-free bridged to CFData.
        let data: *mut AnyObject = unsafe { msg_send![&*self.0, evaluatedPolicyDomainState] };
        if data.is_null() {
            return Vec::new();
        }
        // SAFETY: CFData view of the bridged NSData (get rule: not owned).
        let d = unsafe { CFData::wrap_under_get_rule(data as CFDataRef) };
        d.bytes().to_vec()
    }

    fn set_reason(&self, reason: &str) {
        let s = CFString::new(reason);
        // SAFETY: CFString is toll-free bridged to NSString; the setter
        // copies it.
        let ns = s.as_concrete_TypeRef() as *mut AnyObject;
        let _: () = unsafe { msg_send![&*self.0, setLocalizedReason: ns] };
    }

    fn as_cftype(&self) -> CFType {
        let p = Retained::as_ptr(&self.0) as CFTypeRef;
        // SAFETY: an NSObject is a valid CFTypeRef; get rule retains it.
        unsafe { CFType::wrap_under_get_rule(p) }
    }
}

/// Biometric domain state now (empty when biometrics are unavailable).
fn current_domain_state() -> Vec<u8> {
    match LaContext::new() {
        Ok(ctx) if ctx.can_evaluate(POLICY_BIOMETRICS).is_ok() => ctx.domain_state(),
        _ => Vec::new(),
    }
}

// ---- keys --------------------------------------------------------------------------

fn access_control() -> Result<Owned, OsAuthError> {
    let mut err: CFErrorRef = std::ptr::null_mut();
    // SAFETY: valid protection constant and flags; owned result.
    let ac = unsafe {
        SecAccessControlCreateWithFlags(
            std::ptr::null(),
            kSecAttrAccessibleWhenUnlockedThisDeviceOnly as CFTypeRef,
            PRIVATE_KEY_USAGE | BIOMETRY_CURRENT_SET | OR | DEVICE_PASSCODE,
            &mut err,
        )
    };
    Owned::new(ac).ok_or_else(|| map_cf_error(take_error(err)))
}

fn dict(pairs: &[(CFStringRef, CFType)]) -> CFDictionary<CFString, CFType> {
    let pairs: Vec<(CFString, CFType)> =
        pairs.iter().map(|(k, v)| (cfstr(*k), v.clone())).collect();
    CFDictionary::from_CFType_pairs(&pairs)
}

fn p256_type() -> CFType {
    // SAFETY: framework constant.
    unsafe { cfstr(kSecAttrKeyTypeECSECPrimeRandom) }.as_CFType()
}

fn generate(secure_enclave: bool) -> Result<Owned, OsAuthError> {
    let private_attrs = if secure_enclave {
        let ac = access_control()?;
        // SAFETY: `ac` is a live SecAccessControl (a CFType).
        let ac = unsafe { CFType::wrap_under_get_rule(ac.0 as CFTypeRef) };
        // SAFETY: framework constants.
        unsafe {
            dict(&[
                (kSecAttrIsPermanent, CFBoolean::false_value().as_CFType()),
                (kSecAttrAccessControl, ac),
            ])
        }
    } else {
        // SAFETY: framework constant.
        unsafe { dict(&[(kSecAttrIsPermanent, CFBoolean::false_value().as_CFType())]) }
    };
    // SAFETY: framework constants.
    let mut pairs = unsafe {
        vec![
            (kSecAttrKeyType, p256_type()),
            (kSecAttrKeySizeInBits, CFNumber::from(256i32).as_CFType()),
            (kSecPrivateKeyAttrs, private_attrs.as_CFType()),
        ]
    };
    if secure_enclave {
        // SAFETY: framework constants.
        pairs.push(unsafe {
            (
                kSecAttrTokenID,
                cfstr(kSecAttrTokenIDSecureEnclave).as_CFType(),
            )
        });
    }
    let params = dict(&pairs);
    let mut err: CFErrorRef = std::ptr::null_mut();
    // SAFETY: valid parameter dictionary; owned result.
    let key = unsafe { SecKeyCreateRandomKey(params.as_concrete_TypeRef(), &mut err) };
    Owned::new(key).ok_or_else(|| match take_error(err) {
        // No Secure Enclave (old Intel Macs, VMs).
        Some(e) if e.code() == -4 || e.code() == -50 => OsAuthError::Unsupported,
        e => map_cf_error(e),
    })
}

fn public_bytes(key: &Owned) -> Result<Vec<u8>, OsAuthError> {
    // SAFETY: live key; owned results.
    let public = Owned::new(unsafe { SecKeyCopyPublicKey(key.0) })
        .ok_or_else(|| OsAuthError::Platform("no public key".into()))?;
    let mut err: CFErrorRef = std::ptr::null_mut();
    let data = unsafe { SecKeyCopyExternalRepresentation(public.0, &mut err) };
    if data.is_null() {
        return Err(map_cf_error(take_error(err)));
    }
    // SAFETY: owned CFData.
    let data = unsafe { CFData::wrap_under_create_rule(data) };
    Ok(data.bytes().to_vec())
}

fn token_oid(key: &Owned) -> Result<Vec<u8>, OsAuthError> {
    // SAFETY: live key; owned dictionary.
    let attrs = unsafe { SecKeyCopyAttributes(key.0) };
    if attrs.is_null() {
        return Err(OsAuthError::Platform("no key attributes".into()));
    }
    // SAFETY: owned dictionary of CFString → CFType.
    let attrs: CFDictionary<CFString, CFType> =
        unsafe { CFDictionary::wrap_under_create_rule(attrs) };
    let oid = attrs
        .find(CFString::new(TOKEN_OID))
        .and_then(|v| v.downcast::<CFData>())
        .ok_or_else(|| OsAuthError::Platform("Secure Enclave key blob missing".into()))?;
    Ok(oid.bytes().to_vec())
}

fn public_key_from(bytes: &[u8]) -> Result<Owned, OsAuthError> {
    // SAFETY: framework constants.
    let attrs = unsafe {
        dict(&[
            (kSecAttrKeyType, p256_type()),
            (kSecAttrKeyClass, cfstr(kSecAttrKeyClassPublic).as_CFType()),
        ])
    };
    let data = CFData::from_buffer(bytes);
    let mut err: CFErrorRef = std::ptr::null_mut();
    // SAFETY: valid data and attributes; owned result.
    let key = unsafe {
        SecKeyCreateWithData(
            data.as_concrete_TypeRef(),
            attrs.as_concrete_TypeRef(),
            &mut err,
        )
    };
    Owned::new(key).ok_or(OsAuthError::Invalidated)
}

/// Re-open the enclave key from its blob, bound to `ctx` (reason/prompt).
fn enclave_key_from(oid: &[u8], ctx: &LaContext) -> Result<Owned, OsAuthError> {
    // The blob goes in as the private `toid` attribute (passing it as the
    // data argument instead yields a different key).
    // SAFETY: framework constants.
    let pairs = unsafe {
        vec![
            (cfstr(kSecAttrKeyType), p256_type()),
            (
                cfstr(kSecAttrKeyClass),
                cfstr(kSecAttrKeyClassPrivate).as_CFType(),
            ),
            (
                cfstr(kSecAttrTokenID),
                cfstr(kSecAttrTokenIDSecureEnclave).as_CFType(),
            ),
            (cfstr(kSecUseAuthenticationContext), ctx.as_cftype()),
            (
                CFString::new(TOKEN_OID),
                CFData::from_buffer(oid).as_CFType(),
            ),
        ]
    };
    let attrs = CFDictionary::from_CFType_pairs(&pairs);
    let empty = CFData::from_buffer(&[]);
    let mut err: CFErrorRef = std::ptr::null_mut();
    // SAFETY: valid arguments; owned result.
    let key = unsafe {
        SecKeyCreateWithData(
            empty.as_concrete_TypeRef(),
            attrs.as_concrete_TypeRef(),
            &mut err,
        )
    };
    Owned::new(key).ok_or_else(|| {
        let _ = take_error(err);
        OsAuthError::Invalidated
    })
}

fn ecdh(private: &Owned, public: &Owned) -> Result<KeyMaterial, OsAuthError> {
    let params: CFDictionary<CFString, CFType> = CFDictionary::from_CFType_pairs(&[]);
    let mut err: CFErrorRef = std::ptr::null_mut();
    // SAFETY: live keys of the same curve; owned result.
    let shared = unsafe {
        SecKeyCopyKeyExchangeResult(
            private.0,
            kSecKeyAlgorithmECDHKeyExchangeStandard,
            public.0,
            params.as_concrete_TypeRef(),
            &mut err,
        )
    };
    if shared.is_null() {
        return Err(map_cf_error(take_error(err)));
    }
    // SAFETY: owned CFData.
    let shared = unsafe { CFData::wrap_under_create_rule(shared) };
    Ok(Zeroizing::new(shared.bytes().to_vec()))
}

impl DeviceKeyProtector for SecureEnclaveDeviceKeys {
    fn status(&self) -> DeviceKeyStatus {
        let Ok(ctx) = LaContext::new() else {
            return DeviceKeyStatus::UNSUPPORTED;
        };
        let kind = match ctx.can_evaluate(POLICY_BIOMETRICS) {
            Ok(()) => match ctx.biometry_type() {
                BIOMETRY_FACE_ID => Some(OsAuthKind::FaceId),
                BIOMETRY_TOUCH_ID => Some(OsAuthKind::TouchId),
                _ => Some(OsAuthKind::DeviceCredential),
            },
            Err(_) => None,
        };
        let (kind, not_enrolled) = match kind {
            Some(k) => (Some(k), false),
            None => match ctx.can_evaluate(POLICY_OWNER_AUTH) {
                Ok(()) => (Some(OsAuthKind::DeviceCredential), false),
                Err(code) => (None, code == LA_PASSCODE_NOT_SET),
            },
        };
        DeviceKeyStatus {
            kind,
            protection: UnlockProtection::System,
            not_enrolled,
            external: false,
        }
    }

    fn create(&self, _label: &str, _reason: &str) -> Result<ProtectedKey, OsAuthError> {
        if !self.status().available() {
            return Err(OsAuthError::Unsupported);
        }
        let enclave = generate(true)?;
        let oid = token_oid(&enclave)?;
        let enclave_public = public_key_from(&public_bytes(&enclave)?)?;
        let ephemeral = generate(false)?;
        let ephemeral_public = public_bytes(&ephemeral)?;
        // Software side of the exchange: no enclave use, no prompt.
        let material = ecdh(&ephemeral, &enclave_public)?;
        let state = current_domain_state();
        Ok(ProtectedKey {
            material,
            handle: handle::encode(HANDLE_V1, &[&oid, &ephemeral_public, &state]),
        })
    }

    fn release(&self, handle_bytes: &[u8], reason: &str) -> Result<KeyMaterial, OsAuthError> {
        let fields = handle::decode(HANDLE_V1, handle_bytes, 3)?;
        let (oid, ephemeral_public, state) = (fields[0], fields[1], fields[2]);
        let ctx = LaContext::new()?;
        // Fingerprints added / removed since enabling: the enclave would
        // still accept the password branch, so refuse here. A temporary
        // biometric lockout keeps the key (the password branch applies).
        match ctx.can_evaluate(POLICY_BIOMETRICS) {
            Ok(()) if ctx.domain_state() != state => return Err(OsAuthError::Invalidated),
            Err(LA_BIOMETRY_NOT_ENROLLED) if !state.is_empty() => {
                return Err(OsAuthError::Invalidated)
            }
            _ => {}
        }
        ctx.set_reason(reason);
        let private = enclave_key_from(oid, &ctx)?;
        let public = public_key_from(ephemeral_public)?;
        // The enclave asks for Touch ID / the password here.
        ecdh(&private, &public)
    }

    fn destroy(&self, _handle: &[u8]) -> Result<(), OsAuthError> {
        // Non-permanent enclave keys exist only as the persisted blob:
        // forgetting the handle destroys them.
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn handle_and_status_without_prompt() {
        let k = SecureEnclaveDeviceKeys::new();
        let status = k.status();
        if !status.available() {
            eprintln!("no LocalAuthentication policy available: skipped");
            return;
        }
        assert_eq!(status.protection, UnlockProtection::System);
        // Creating an enclave key and the software half needs no prompt.
        match k.create("test", "test") {
            Ok(created) => {
                assert!(created.material.len() >= 32);
                let fields = handle::decode(HANDLE_V1, &created.handle, 3).unwrap();
                assert!(fields[0].len() > 100, "enclave blob");
                assert_eq!(fields[1].len(), 65, "uncompressed P-256 point");
                // Garbage handles are rejected before any prompt.
                assert_eq!(
                    k.release(b"\x01junk", "test"),
                    Err(OsAuthError::Invalidated)
                );
            }
            Err(OsAuthError::Unsupported) => eprintln!("no Secure Enclave: skipped"),
            // The login session is locked (keybag not ready): CI / screen lock.
            Err(OsAuthError::Failed) => eprintln!("device locked: skipped"),
            Err(e) => panic!("create failed: {e}"),
        }
    }

    /// Shows the real Touch ID / password prompt. Run manually:
    /// `cargo test -p cc-platform-core -- --ignored secure_enclave_prompt --nocapture`
    /// (approve → material matches; cancel → `Cancelled`).
    #[test]
    #[ignore = "shows the OS authentication prompt"]
    fn secure_enclave_prompt() {
        let k = SecureEnclaveDeviceKeys::new();
        let created = k.create("manual", "create").unwrap();
        let r = k.release(&created.handle, "ConsoleCrypt test: approve or cancel");
        match r {
            Ok(m) => {
                assert_eq!(*m, *created.material, "same ECDH secret");
                eprintln!("released: material matches");
            }
            Err(e) => eprintln!("release refused: {e:?}"),
        }
    }
}
