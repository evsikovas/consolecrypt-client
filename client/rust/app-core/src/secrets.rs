//! Secrets at rest (CLIENT_ARCHITECTURE §4): per-profile SQLCipher key,
//! device identity (vault-core naming) and session tokens, all in the
//! [`SecureStore`]. Values never leave this module except as the typed
//! wrappers of the consuming crates.

use crate::error::{AppError, AppResult};
use async_trait::async_trait;
use cc_platform_core::{ExposeSecret, SecureStore};
use cc_protocol::auth::TokenPair;
use cc_storage_core::{DatabaseKey, ProfileId};
use cc_sync_core::{TokenStore, TokenStoreError};
use cc_vault_core::DeviceIdentity;
use std::sync::Arc;
use zeroize::Zeroizing;

/// Secure-store item of a profile's database key.
pub(crate) fn db_key_name(profile: ProfileId) -> String {
    format!("profiles/{profile}/db-key")
}

/// Secure-store item of a profile's session tokens.
pub(crate) fn tokens_name(profile: ProfileId) -> String {
    format!("profiles/{profile}/session-tokens")
}

/// Run blocking secure-store work off the async runtime (OS prompts).
pub(crate) async fn blocking<T, F>(f: F) -> AppResult<T>
where
    T: Send + 'static,
    F: FnOnce() -> AppResult<T> + Send + 'static,
{
    tokio::task::spawn_blocking(f)
        .await
        .map_err(|e| AppError::internal(format!("blocking task failed: {e}")))?
}

/// Create and store a fresh random 32-byte database key.
pub(crate) async fn create_db_key(
    store: Arc<dyn SecureStore>,
    profile: ProfileId,
) -> AppResult<DatabaseKey> {
    blocking(move || {
        let mut bytes = Zeroizing::new([0u8; 32]);
        cc_crypto_core::fill_random(bytes.as_mut())?;
        store.set(&db_key_name(profile), bytes.as_ref())?;
        Ok(DatabaseKey::from_bytes(*bytes))
    })
    .await
}

/// Load a profile's database key.
pub(crate) async fn load_db_key(
    store: Arc<dyn SecureStore>,
    profile: ProfileId,
) -> AppResult<DatabaseKey> {
    blocking(move || {
        let v = store.get(&db_key_name(profile))?.ok_or_else(|| {
            AppError::SecureStore(
                "the database key of this profile is missing from the secure store".into(),
            )
        })?;
        DatabaseKey::from_slice(v.expose_secret())
            .ok_or_else(|| AppError::SecureStore("stored database key is corrupted".into()))
    })
    .await
}

/// Load (or create) the device identity of a profile.
pub(crate) async fn load_or_create_identity(
    store: Arc<dyn SecureStore>,
    profile: ProfileId,
) -> AppResult<DeviceIdentity> {
    blocking(move || {
        Ok(DeviceIdentity::load_or_create(
            store.as_ref(),
            &profile.to_string(),
        )?)
    })
    .await
}

/// Replace the device identity of a profile with a fresh one.
pub(crate) async fn regenerate_identity(
    store: Arc<dyn SecureStore>,
    profile: ProfileId,
) -> AppResult<DeviceIdentity> {
    blocking(move || {
        let fresh = DeviceIdentity::generate()?;
        fresh.save(store.as_ref(), &profile.to_string())?;
        Ok(fresh)
    })
    .await
}

/// Remove every secure-store item of a profile (best effort per item).
pub(crate) async fn delete_profile_secrets(
    store: Arc<dyn SecureStore>,
    profile: ProfileId,
) -> AppResult<()> {
    blocking(move || {
        let mut first_err = None;
        for name in [
            db_key_name(profile),
            tokens_name(profile),
            crate::ai::index_key_name(profile),
        ] {
            if let Err(e) = store.delete(&name) {
                first_err.get_or_insert(AppError::from(e));
            }
        }
        if let Err(e) = DeviceIdentity::delete(store.as_ref(), &profile.to_string()) {
            first_err.get_or_insert(AppError::from(e));
        }
        match first_err {
            Some(e) => Err(e),
            None => Ok(()),
        }
    })
    .await
}

/// sync-core [`TokenStore`] persisting the rotating [`TokenPair`] in the
/// secure store (JSON in a zeroized buffer).
pub(crate) struct SecureTokenStore {
    store: Arc<dyn SecureStore>,
    name: String,
}

impl SecureTokenStore {
    pub(crate) fn new(store: Arc<dyn SecureStore>, profile: ProfileId) -> Self {
        Self {
            store,
            name: tokens_name(profile),
        }
    }
}

impl std::fmt::Debug for SecureTokenStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SecureTokenStore")
            .field("name", &self.name)
            .finish_non_exhaustive()
    }
}

fn token_err(e: impl std::fmt::Display) -> TokenStoreError {
    TokenStoreError(e.to_string())
}

#[async_trait]
impl TokenStore for SecureTokenStore {
    async fn load(&self) -> Result<Option<TokenPair>, TokenStoreError> {
        let store = self.store.clone();
        let name = self.name.clone();
        tokio::task::spawn_blocking(move || {
            let Some(v) = store.get(&name).map_err(token_err)? else {
                return Ok(None);
            };
            serde_json::from_slice::<TokenPair>(v.expose_secret())
                .map(Some)
                .map_err(|_| TokenStoreError("stored session tokens are corrupted".into()))
        })
        .await
        .map_err(token_err)?
    }

    async fn save(&self, tokens: &TokenPair) -> Result<(), TokenStoreError> {
        let json = Zeroizing::new(serde_json::to_vec(tokens).map_err(token_err)?);
        let store = self.store.clone();
        let name = self.name.clone();
        tokio::task::spawn_blocking(move || store.set(&name, &json).map_err(token_err))
            .await
            .map_err(token_err)?
    }

    async fn clear(&self) -> Result<(), TokenStoreError> {
        let store = self.store.clone();
        let name = self.name.clone();
        tokio::task::spawn_blocking(move || store.delete(&name).map(|_| ()).map_err(token_err))
            .await
            .map_err(token_err)?
    }
}

#[cfg(feature = "insecure-file-store")]
pub(crate) use file_store::InsecureFileSecureStore;

#[cfg(feature = "insecure-file-store")]
mod file_store {
    //! **Insecure** plaintext file store for headless/CI CLI use (feature
    //! `insecure-file-store`, explicit runtime opt-in). Protects values only
    //! by file permissions (0600 in a 0700 directory).
    use base64::Engine as _;
    use cc_platform_core::{validate_secret_name, SecretSlice, SecureStore, SecureStoreError};
    use std::collections::BTreeMap;
    use std::path::PathBuf;
    use std::sync::Mutex;
    use zeroize::Zeroizing;

    const B64: base64::engine::GeneralPurpose = base64::engine::general_purpose::STANDARD;

    pub(crate) struct InsecureFileSecureStore {
        path: PathBuf,
        lock: Mutex<()>,
    }

    impl std::fmt::Debug for InsecureFileSecureStore {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.debug_struct("InsecureFileSecureStore")
                .field("path", &self.path)
                .finish_non_exhaustive()
        }
    }

    type Map = BTreeMap<String, Zeroizing<String>>;

    fn io(e: std::io::Error) -> SecureStoreError {
        SecureStoreError::Backend(e.to_string())
    }

    impl InsecureFileSecureStore {
        pub(crate) fn new(path: PathBuf) -> Self {
            Self {
                path,
                lock: Mutex::new(()),
            }
        }

        fn read(&self) -> Result<Map, SecureStoreError> {
            match std::fs::read(&self.path) {
                Ok(bytes) => {
                    let bytes = Zeroizing::new(bytes);
                    let raw: BTreeMap<String, String> =
                        serde_json::from_slice(&bytes).map_err(|_| SecureStoreError::Corrupted)?;
                    Ok(raw
                        .into_iter()
                        .map(|(k, v)| (k, Zeroizing::new(v)))
                        .collect())
                }
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Map::new()),
                Err(e) => Err(io(e)),
            }
        }

        fn write(&self, map: &Map) -> Result<(), SecureStoreError> {
            let plain: BTreeMap<&str, &str> =
                map.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect();
            let json = Zeroizing::new(
                serde_json::to_vec(&plain).map_err(|_| SecureStoreError::Corrupted)?,
            );
            if let Some(dir) = self.path.parent() {
                std::fs::create_dir_all(dir).map_err(io)?;
            }
            let tmp = self.path.with_extension("json.tmp");
            {
                use std::io::Write;
                let mut o = std::fs::OpenOptions::new();
                o.write(true).create(true).truncate(true);
                #[cfg(unix)]
                {
                    use std::os::unix::fs::OpenOptionsExt;
                    o.mode(0o600);
                }
                let mut f = o.open(&tmp).map_err(io)?;
                f.write_all(&json).map_err(io)?;
                f.sync_all().map_err(io)?;
            }
            std::fs::rename(&tmp, &self.path).map_err(io)
        }

        fn guard(&self) -> std::sync::MutexGuard<'_, ()> {
            self.lock.lock().unwrap_or_else(|p| p.into_inner())
        }
    }

    impl SecureStore for InsecureFileSecureStore {
        fn get(&self, name: &str) -> Result<Option<SecretSlice<u8>>, SecureStoreError> {
            validate_secret_name(name)?;
            let _g = self.guard();
            let map = self.read()?;
            match map.get(name) {
                Some(v) => {
                    let bytes = B64
                        .decode(v.as_bytes())
                        .map_err(|_| SecureStoreError::Corrupted)?;
                    Ok(Some(SecretSlice::from(bytes)))
                }
                None => Ok(None),
            }
        }

        fn set(&self, name: &str, secret: &[u8]) -> Result<(), SecureStoreError> {
            validate_secret_name(name)?;
            if secret.len() > cc_platform_core::MAX_SECRET_LEN {
                return Err(SecureStoreError::TooLarge(secret.len()));
            }
            let _g = self.guard();
            let mut map = self.read()?;
            map.insert(name.to_owned(), Zeroizing::new(B64.encode(secret)));
            self.write(&map)
        }

        fn delete(&self, name: &str) -> Result<bool, SecureStoreError> {
            validate_secret_name(name)?;
            let _g = self.guard();
            let mut map = self.read()?;
            let existed = map.remove(name).is_some();
            if existed {
                self.write(&map)?;
            }
            Ok(existed)
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use cc_platform_core::ExposeSecret;

        #[test]
        fn roundtrip_and_permissions() {
            let dir = tempfile::tempdir().unwrap();
            let s = InsecureFileSecureStore::new(dir.path().join("secure-store.json"));
            assert!(s.get("a/b").unwrap().is_none());
            s.set("a/b", &[1, 2, 3]).unwrap();
            let s2 = InsecureFileSecureStore::new(dir.path().join("secure-store.json"));
            assert_eq!(s2.get("a/b").unwrap().unwrap().expose_secret(), &[1, 2, 3]);
            assert!(s2.delete("a/b").unwrap());
            assert!(!s2.delete("a/b").unwrap());
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let mode = std::fs::metadata(dir.path().join("secure-store.json"))
                    .unwrap()
                    .permissions()
                    .mode();
                assert_eq!(mode & 0o777, 0o600);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cc_platform_core::InMemorySecureStore;

    #[tokio::test]
    async fn db_key_and_tokens_roundtrip() {
        let store: Arc<dyn SecureStore> = Arc::new(InMemorySecureStore::new());
        let p = ProfileId::new();
        let k = create_db_key(store.clone(), p).await.unwrap();
        let k2 = load_db_key(store.clone(), p).await.unwrap();
        assert_eq!(k.expose_bytes(), k2.expose_bytes());
        let ts = SecureTokenStore::new(store.clone(), p);
        assert!(ts.load().await.unwrap().is_none());
        delete_profile_secrets(store.clone(), p).await.unwrap();
        assert!(load_db_key(store, p).await.is_err());
    }
}
