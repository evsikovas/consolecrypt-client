//! Linux system Secret Service, using an encrypted D-Bus session.
//!
//! Only the user's existing, unlocked default collection is used. We never
//! create/unlock collections, use the transient session collection, or fall
//! back to a file. GNOME Keyring is the tested provider; UTF-8 encoding also
//! accommodates providers whose Secret Service accepts only text (KWallet).

use crate::secure_store::{validate_secret, validate_secret_name, SecureStore, SecureStoreError};
use base64::{engine::general_purpose::STANDARD, Engine};
use secrecy::SecretSlice;
use secret_service::{Collection, EncryptionType, Item, SecretService};
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
    runtime: Option<tokio::runtime::Runtime>,
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
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(1)
            .enable_all()
            .build()
            .map_err(|_| locked_error())?;
        let connection = match run_on(&runtime, async {
            SecretService::connect(EncryptionType::Dh)
                .await
                .map_err(map_error)
        }) {
            Ok(connection) => connection,
            Err(error) => {
                runtime.shutdown_background();
                return Err(error);
            }
        };
        let store = Self {
            service: service.to_owned(),
            connection: Mutex::new(connection),
            runtime: Some(runtime),
        };
        // Validate before AppCore creates profile metadata. Never create or
        // unlock a collection or fall back to a plaintext/session collection.
        {
            let connection = store.lock()?;
            store.run(async { default_collection(&connection).await.map(|_| ()) })?;
        }
        Ok(store)
    }

    fn attributes<'a>(&'a self, name: &'a str) -> HashMap<&'a str, &'a str> {
        HashMap::from([("service", self.service.as_str()), ("username", name)])
    }

    fn lock(&self) -> Result<std::sync::MutexGuard<'_, SecretService<'static>>, SecureStoreError> {
        self.connection
            .lock()
            .map_err(|_| SecureStoreError::Backend("Secret Service connection interrupted".into()))
    }

    fn run<T: Send>(
        &self,
        future: impl std::future::Future<Output = Result<T, SecureStoreError>> + Send,
    ) -> Result<T, SecureStoreError> {
        run_on(
            self.runtime.as_ref().expect("keyring runtime is live"),
            future,
        )
    }
}

impl Drop for OsSecureStore {
    fn drop(&mut self) {
        // Safe even when a profile is dropped by an asynchronous UI callback.
        if let Some(runtime) = self.runtime.take() {
            runtime.shutdown_background();
        }
    }
}

fn run_on<T: Send>(
    runtime: &tokio::runtime::Runtime,
    future: impl std::future::Future<Output = Result<T, SecureStoreError>> + Send,
) -> Result<T, SecureStoreError> {
    run_with_timeout(runtime, future, std::time::Duration::from_secs(10))
}

fn run_with_timeout<T: Send>(
    runtime: &tokio::runtime::Runtime,
    future: impl std::future::Future<Output = Result<T, SecureStoreError>> + Send,
    timeout: std::time::Duration,
) -> Result<T, SecureStoreError> {
    // A scoped helper avoids nesting Runtime::block_on inside AppCore's Tokio
    // runtime. The mutex serializes this connection's operations, and a hard
    // timeout bounds a stalled provider without forwarding remote diagnostics.
    std::thread::scope(|scope| {
        std::thread::Builder::new()
            .name("cc-secret-service".into())
            .spawn_scoped(scope, || {
                runtime.block_on(async {
                    tokio::time::timeout(timeout, future)
                        .await
                        .map_err(|_| locked_error())?
                })
            })
            .map_err(|_| locked_error())?
            .join()
            .map_err(|_| locked_error())?
    })
}

// Check on EVERY operation: locking the desktop keyring after initialization
// must not look like a missing item or cause replacement device keys.
async fn default_collection<'a>(
    ss: &'a SecretService<'_>,
) -> Result<Collection<'a>, SecureStoreError> {
    let collection = ss.get_default_collection().await.map_err(|e| match e {
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
    match ss.get_collection_by_alias("session").await {
        Ok(session) if session.collection_path == collection.collection_path => {
            return Err(SecureStoreError::AccessDenied(
                "The transient Secret Service session collection cannot store device keys".into(),
            ));
        }
        Ok(_) | Err(secret_service::Error::NoResult) => {}
        Err(e) => return Err(map_error(e)),
    }
    if collection.is_locked().await.map_err(map_error)? {
        return Err(locked_error());
    }
    Ok(collection)
}

async fn one_item<'a>(mut items: Vec<Item<'a>>) -> Result<Option<Item<'a>>, SecureStoreError> {
    if items.len() > 1 {
        return Err(SecureStoreError::Backend(
            "Ambiguous Secret Service item".into(),
        ));
    }
    let item = items.pop();
    if let Some(i) = &item {
        if i.is_locked().await.map_err(map_error)? {
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
        self.run(async {
            let collection = default_collection(&ss).await?;
            let item = one_item(
                collection
                    .search_items(self.attributes(name))
                    .await
                    .map_err(map_error)?,
            )
            .await?;
            match item {
                Some(item) => item
                    .get_secret()
                    .await
                    .map_err(map_error)
                    .and_then(decode_secret)
                    .map(Some),
                None => Ok(None),
            }
        })
    }

    fn set(&self, name: &str, secret: &[u8]) -> Result<(), SecureStoreError> {
        validate_secret_name(name)?;
        validate_secret(secret)?;
        let ss = self.lock()?;
        self.run(async {
            let collection = default_collection(&ss).await?;
            let item = one_item(
                collection
                    .search_items(self.attributes(name))
                    .await
                    .map_err(map_error)?,
            )
            .await?;
            let encoded = encode_secret(secret);
            match item {
                Some(item) => item
                    .set_secret(encoded.as_bytes(), "text/plain; charset=utf-8")
                    .await
                    .map_err(map_error),
                None => collection
                    .create_item(
                        "ConsoleCrypt device-local key",
                        self.attributes(name),
                        encoded.as_bytes(),
                        false,
                        "text/plain; charset=utf-8",
                    )
                    .await
                    .map(|_| ())
                    .map_err(map_error),
            }
        })
    }

    fn delete(&self, name: &str) -> Result<bool, SecureStoreError> {
        validate_secret_name(name)?;
        let ss = self.lock()?;
        self.run(async {
            let collection = default_collection(&ss).await?;
            let item = one_item(
                collection
                    .search_items(self.attributes(name))
                    .await
                    .map_err(map_error)?,
            )
            .await?;
            match item {
                Some(item) => item.delete().await.map(|_| true).map_err(map_error),
                None => Ok(false),
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use secrecy::ExposeSecret;

    #[tokio::test]
    async fn synchronous_adapter_works_from_an_existing_async_runtime() {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(1)
            .enable_all()
            .build()
            .unwrap();
        let result = run_on(&runtime, async { Ok(7u8) });
        runtime.shutdown_background();
        assert_eq!(result.unwrap(), 7);
    }

    #[test]
    fn stalled_keyring_request_times_out_without_falling_back() {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(1)
            .enable_all()
            .build()
            .unwrap();
        let result: Result<(), _> = run_with_timeout(
            &runtime,
            std::future::pending(),
            std::time::Duration::from_millis(10),
        );
        runtime.shutdown_background();
        assert!(matches!(result, Err(SecureStoreError::AccessDenied(_))));
    }

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

    #[tokio::test]
    async fn malformed_or_oversized_items_fail_without_disclosing_payload() {
        for bad in [
            b"not-a-consolecrypt-value".to_vec(),
            b"consolecrypt:v1:%%%".to_vec(),
            vec![b'A'; MAX_ENCODED_LEN + 1],
        ] {
            assert_eq!(decode_secret(bad).unwrap_err(), SecureStoreError::Corrupted);
        }
        assert_eq!(one_item(Vec::new()).await.unwrap().map(|_| ()), None);
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
