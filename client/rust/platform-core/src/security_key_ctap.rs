//! Security keys over CTAP2/HID (`ctap-hid-fido2`), used on macOS: the app
//! is not sandboxed and talks to the FIDO HID interface directly, so it asks
//! for the PIN itself ([`SecurityKeyPin::App`]). The salt is hashed like
//! WebAuthn PRF so the value matches Windows for the same key.
//!
//! Not runnable without a key: covered by the core tests with
//! [`crate::InMemorySecurityKey`] and the manual test plan.

use crate::security_key::{
    prf_salt, SecurityKeyAuthenticator, SecurityKeyError, SecurityKeyPin, SECURITY_KEY_RP_ID,
};
use ctap_hid_fido2::fidokey::{
    AssertionExtension, CredentialExtension, GetAssertionArgsBuilder, MakeCredentialArgsBuilder,
};
use ctap_hid_fido2::{FidoKeyHid, FidoKeyHidFactory, LibCfg};
use zeroize::Zeroizing;

#[derive(Debug, Default)]
pub struct CtapSecurityKeys;

fn open() -> Result<FidoKeyHid, SecurityKeyError> {
    match ctap_hid_fido2::get_fidokey_devices().len() {
        0 => return Err(SecurityKeyError::NoDevice),
        1 => {}
        _ => return Err(SecurityKeyError::SeveralDevices),
    }
    // The library would print its own "touch the sensor" line to stdout;
    // the app shows the touch prompt itself.
    let mut cfg = LibCfg::init();
    cfg.enable_keep_alive_msg = false;
    FidoKeyHidFactory::create(&cfg).map_err(|e| error(&e))
}

/// CTAP status names appear in the library's error text.
fn error(e: &anyhow::Error) -> SecurityKeyError {
    let text = format!("{e:#}");
    let has = |name: &str| text.contains(name);
    if has("PIN_INVALID") {
        SecurityKeyError::PinInvalid
    } else if has("PIN_BLOCKED") || has("PIN_AUTH_BLOCKED") {
        SecurityKeyError::PinBlocked
    } else if has("PIN_REQUIRED") || has("PIN_NOT_SET") {
        SecurityKeyError::PinRequired
    } else if has("NO_CREDENTIALS") || has("INVALID_CREDENTIAL") {
        SecurityKeyError::WrongKey
    } else if has("OPERATION_DENIED")
        || has("KEEPALIVE_CANCEL")
        || has("ACTION_TIMEOUT")
        || has("USER_ACTION_TIMEOUT")
    {
        SecurityKeyError::Cancelled
    } else if has("UNSUPPORTED_EXTENSION") {
        SecurityKeyError::NoHmacSecret
    } else if has("not found") {
        SecurityKeyError::NoDevice
    } else {
        SecurityKeyError::Device(text.chars().take(200).collect())
    }
}

fn challenge() -> [u8; 32] {
    let mut c = [0u8; 32];
    getrandom::fill(&mut c).expect("system random");
    c
}

impl SecurityKeyAuthenticator for CtapSecurityKeys {
    fn pin(&self) -> Option<SecurityKeyPin> {
        Some(SecurityKeyPin::App)
    }

    fn register(&self, _user_label: &str, pin: Option<&str>) -> Result<Vec<u8>, SecurityKeyError> {
        let pin = pin.ok_or(SecurityKeyError::PinRequired)?;
        let key = open()?;
        let challenge = challenge();
        let args = MakeCredentialArgsBuilder::new(SECURITY_KEY_RP_ID, &challenge)
            .pin(pin)
            .extensions(&[CredentialExtension::HmacSecret(Some(true))])
            .build();
        let attestation = key
            .make_credential_with_args(&args)
            .map_err(|e| error(&e))?;
        if !attestation
            .extensions
            .iter()
            .any(|e| matches!(e, CredentialExtension::HmacSecret(Some(true))))
        {
            return Err(SecurityKeyError::NoHmacSecret);
        }
        Ok(attestation.credential_descriptor.id)
    }

    fn prf(
        &self,
        credential: &[u8],
        salt: &[u8; 32],
        pin: Option<&str>,
    ) -> Result<Zeroizing<[u8; 32]>, SecurityKeyError> {
        let pin = pin.ok_or(SecurityKeyError::PinRequired)?;
        let key = open()?;
        let challenge = challenge();
        let args = GetAssertionArgsBuilder::new(SECURITY_KEY_RP_ID, &challenge)
            .pin(pin)
            .credential_id(credential)
            .extensions(&[AssertionExtension::HmacSecret(Some(prf_salt(salt)))])
            .build();
        let assertions = key.get_assertion_with_args(&args).map_err(|e| error(&e))?;
        assertions
            .into_iter()
            .flat_map(|a| a.extensions)
            .find_map(|e| match e {
                AssertionExtension::HmacSecret(Some(output)) => Some(Zeroizing::new(output)),
                _ => None,
            })
            .ok_or(SecurityKeyError::NoHmacSecret)
    }
}
