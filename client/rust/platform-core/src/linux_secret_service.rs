//! Linux system Secret Service, using an encrypted D-Bus session.
//!
//! Only the user's existing, unlocked default collection is used. We never
//! create/unlock collections, use the transient session collection, or fall
//! back to a file. GNOME Keyring is the tested provider; UTF-8 encoding also
//! accommodates providers whose Secret Service accepts only text (KWallet).

use crate::secure_store::{validate_secret, validate_secret_name, SecureStore, SecureStoreError};
use base64::{engine::general_purpose::STANDARD, Engine};
use secrecy::SecretSlice;
use secret_service::{
    blocking::Collection, blocking::Item, blocking::SecretService, EncryptionType,
};
use std::{collections::HashMap, fmt, sync::Mutex};
use zeroize::Zeroizing;

/// Same application namespace on every platform.
pub const DEFAULT_SERVICE: &str = "io.consolecrypt.ConsoleCrypt";
const ENCODING_PREFIX: &[u8] = b"consolecrypt:v1:";
const MAX_ENCODED_LEN: usize = ENCODING_PREFIX.len() + 4 * crate::MAX_SECRET_LEN.div_ceil(3);

/// Small device-local keys backed exclusively by the system Secret Service.
pub struct OsSecureStore {
    service: String,
    connection: Mutex<SecretService<'static>>,
}

impl fmt::Debug for OsSecureStore {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("OsSecureStore")
            .field("service", &self.service)
            .finish_non_exhaustive()
    }
}

impl OsSecureStore {
    /// Connect to the desktop user's existing unlocked keyring.
    pub fn new() -> Result<Self, SecureStoreError> {
        Self::with_service(DEFAULT_SERVICE)
    }

    /// Isolated application namespace (use a disposable desktop for tests).
    pub fn with_service(service: &str) -> Result<Self, SecureStoreError> {
        validate_secret_name(service)?;
        let connection = SecretService::connect(EncryptionType::Dh).map_err(map_error)?;
        // Fail at initialization too, before AppCore creates profile metadata.
        default_collection(&connection)?;
        Ok(Self {
            service: service.to_owned(),
            connection: Mutex::new(connection),
        })
    }

    fn attributes<'a>(&'a self, name: &'a str) -> HashMap<&'a str, &'a str> {
        HashMap::from([("service", self.service.as_str()), ("username", name)])
    }

    fn lock(&self) -> Result<std::sync::MutexGuard<'_, SecretService<'static>>, SecureStoreError> {
        self.connection
            .lock()
            .map_err(|_| SecureStoreError::Backend("Secret Service connection interrupted".into()))
    }
}

// Check on EVERY operation: locking the desktop keyring after initialization
// must not look like a missing item or cause replacement device keys.
fn default_collection<'a>(ss: &'a SecretService<'_>) -> Result<Collection<'a>, SecureStoreError> {
    let collection = ss.get_default_collection().map_err(|e| match e {
        secret_service::Error::NoResult => SecureStoreError::AccessDenied(
            "No default Secret Service keyring; configure and unlock your desktop login keyring"
                .into(),
        ),
        e => map_error(e),
    })?;
    if collection.collection_path.as_str() == "/org/freedesktop/secrets/collection/session" {
        return Err(SecureStoreError::AccessDenied(
            "The transient Secret Service session collection cannot store device keys".into(),
        ));
    }
    // Providers may use a different object path for the session collection.
    match ss.get_collection_by_alias("session") {
        Ok(session) if session.collection_path == collection.collection_path => {
            return Err(SecureStoreError::AccessDenied(
                "The transient Secret Service session collection cannot store device keys".into(),
            ));
        }
        Ok(_) | Err(secret_service::Error::NoResult) => {}
        Err(e) => return Err(map_error(e)),
    }
    if collection.is_locked().map_err(map_error)? {
        return Err(locked_error());
    }
    Ok(collection)
}

fn one_item<'a>(mut items: Vec<Item<'a>>) -> Result<Option<Item<'a>>, SecureStoreError> {
    if items.len() > 1 {
        return Err(SecureStoreError::Backend(
            "Ambiguous Secret Service item".into(),
        ));
    }
    let item = items.pop();
    if let Some(i) = &item {
        if i.is_locked().map_err(map_error)? {
            return Err(locked_error());
        }
    }
    Ok(item)
}

fn locked_error() -> SecureStoreError {
    SecureStoreError::AccessDenied(
        "Secret Service keyring is locked; unlock your desktop login keyring and retry".into(),
    )
}

fn map_error(error: secret_service::Error) -> SecureStoreError {
    // D-Bus errors can carry remote/provider-controlled messages. Do not copy
    // their payloads into logs or UI. A lock race must fail closed too.
    match error {
        secret_service::Error::Locked => locked_error(),
        _ => SecureStoreError::AccessDenied(
            "Secret Service unavailable or access denied; start and unlock your desktop login keyring".into(),
        ),
    }
}

fn encode_secret(secret: &[u8]) -> Zeroizing<String> {
    let mut encoded = Zeroizing::new(String::with_capacity(MAX_ENCODED_LEN));
    // This constant prefix is ASCII; the encoded value never leaves the
    // encrypted item or process memory (and is not an additional encryption).
    encoded.push_str("consolecrypt:v1:");
    STANDARD.encode_string(secret, &mut encoded);
    encoded
}

fn decode_secret(encoded: Vec<u8>) -> Result<SecretSlice<u8>, SecureStoreError> {
    let encoded = Zeroizing::new(encoded);
    if encoded.len() > MAX_ENCODED_LEN {
        return Err(SecureStoreError::Corrupted);
    }
    let payload = encoded
        .strip_prefix(ENCODING_PREFIX)
        .ok_or(SecureStoreError::Corrupted)?;
    let mut decoded = Zeroizing::new(Vec::new());
    STANDARD
        .decode_vec(payload, &mut decoded)
        .map_err(|_| SecureStoreError::Corrupted)?;
    if decoded.len() > crate::MAX_SECRET_LEN {
        return Err(SecureStoreError::Corrupted);
    }
    Ok(SecretSlice::from(std::mem::take(&mut *decoded)))
}

impl SecureStore for OsSecureStore {
    fn get(&self, name: &str) -> Result<Option<SecretSlice<u8>>, SecureStoreError> {
        validate_secret_name(name)?;
        let ss = self.lock()?;
        let collection = default_collection(&ss)?;
        let item = one_item(
            collection
                .search_items(self.attributes(name))
                .map_err(map_error)?,
        )?;
        item.map(|i| i.get_secret().map_err(map_error).and_then(decode_secret))
            .transpose()
    }

    fn set(&self, name: &str, secret: &[u8]) -> Result<(), SecureStoreError> {
        validate_secret_name(name)?;
        validate_secret(secret)?;
        let ss = self.lock()?;
        let collection = default_collection(&ss)?;
        let item = one_item(
            collection
                .search_items(self.attributes(name))
                .map_err(map_error)?,
        )?;
        let encoded = encode_secret(secret);
        match item {
            Some(item) => item
                .set_secret(encoded.as_bytes(), "text/plain; charset=utf-8")
                .map_err(map_error),
            None => collection
                .create_item(
                    "ConsoleCrypt device-local key",
                    self.attributes(name),
                    encoded.as_bytes(),
                    false,
                    "text/plain; charset=utf-8",
                )
                .map(|_| ())
                .map_err(map_error),
        }
    }

    fn delete(&self, name: &str) -> Result<bool, SecureStoreError> {
        validate_secret_name(name)?;
        let ss = self.lock()?;
        let collection = default_collection(&ss)?;
        let item = one_item(
            collection
                .search_items(self.attributes(name))
                .map_err(map_error)?,
        )?;
        match item {
            Some(item) => item.delete().map(|_| true).map_err(map_error),
            None => Ok(false),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use secrecy::ExposeSecret;

    #[test]
    fn production_secret_service_preserves_existing_item_namespace() {
        assert_eq!(DEFAULT_SERVICE, "io.consolecrypt.ConsoleCrypt");
        assert_ne!(DEFAULT_SERVICE, "io.consolecrypt.ConsoleCrypt.Dev");
        assert!(validate_secret_name(DEFAULT_SERVICE).is_ok());
        // No D-Bus connection, collection unlock or user item lookup.
    }

    #[test]
    fn text_encoding_roundtrips_all_binary_values_and_portable_limit() {
        for value in [
            Vec::new(),
            (0..=255).collect(),
            vec![0xff; crate::MAX_SECRET_LEN],
        ] {
            let encoded = encode_secret(&value);
            assert!(encoded.is_ascii());
            let result = decode_secret(encoded.as_bytes().to_vec()).unwrap();
            assert_eq!(result.expose_secret(), value);
        }
    }

    #[test]
    fn malformed_or_oversized_items_fail_without_disclosing_payload() {
        for bad in [
            b"not-a-consolecrypt-value".to_vec(),
            b"consolecrypt:v1:%%%".to_vec(),
            vec![b'A'; MAX_ENCODED_LEN + 1],
        ] {
            assert_eq!(decode_secret(bad).unwrap_err(), SecureStoreError::Corrupted);
        }
        assert_eq!(one_item(Vec::new()).unwrap().map(|_| ()), None);
    }

    #[test]
    fn invalid_service_is_rejected_before_dbus() {
        assert_eq!(
            OsSecureStore::with_service("bad service").unwrap_err(),
            SecureStoreError::InvalidName
        );
    }

    #[test]
    fn provider_diagnostics_are_never_forwarded() {
        let error = map_error(secret_service::Error::Crypto("untrusted-provider-payload"));
        assert!(matches!(error, SecureStoreError::AccessDenied(_)));
        assert!(!error.to_string().contains("untrusted-provider-payload"));
        assert_eq!(map_error(secret_service::Error::Locked), locked_error());
    }
}
