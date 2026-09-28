//! [`ObjectCodec`] over `vault_core::UnlockedVault` (CLIENT_ARCHITECTURE §3).
//!
//! The codec owns the only long-lived reference to the unlocked vault of a
//! session. [`VaultCodec::clear`] drops it (zeroizing the VRK and derived
//! keys once in-flight operations finish); afterwards every call reports
//! [`CodecError::Locked`], even through clones of the `Arc<dyn ObjectCodec>`
//! held by sync-core.

use crate::error::{AppError, AppResult};
use cc_crypto_core::CryptoError;
use cc_models::{KekClass, ObjectPayload};
use cc_protocol::sync::EncryptedBody;
use cc_protocol::{ObjectId, VaultId};
use cc_sync_core::{CodecError, ObjectCodec};
use cc_vault_core::{UnlockedVault, VaultError};
use std::sync::{Arc, RwLock};

pub(crate) struct VaultCodec {
    vault_id: VaultId,
    vault: RwLock<Option<Arc<UnlockedVault>>>,
}

impl std::fmt::Debug for VaultCodec {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("VaultCodec")
            .field("vault_id", &self.vault_id)
            .field("unlocked", &self.is_unlocked())
            .finish()
    }
}

fn map_err(e: VaultError) -> CodecError {
    match e {
        VaultError::Crypto(c) => match c {
            CryptoError::Decrypt
            | CryptoError::ObjectIdMismatch
            | CryptoError::VaultMismatch
            | CryptoError::MalformedBody(_)
            | CryptoError::MalformedPlaintext(_) => CodecError::Integrity,
            CryptoError::KekClassMismatch { .. } => CodecError::ClassMismatch,
            CryptoError::UnsupportedFormat(f) => CodecError::UnsupportedFormat(f),
            CryptoError::Serialization => CodecError::Encoding("payload serialization".into()),
            other => CodecError::Other(other.to_string()),
        },
        other => CodecError::Other(other.to_string()),
    }
}

impl VaultCodec {
    pub(crate) fn new(vault: UnlockedVault) -> Arc<Self> {
        Arc::new(Self {
            vault_id: vault.vault_id(),
            vault: RwLock::new(Some(Arc::new(vault))),
        })
    }

    /// The unlocked vault (for envelope / approval / backup operations).
    pub(crate) fn vault(&self) -> AppResult<Arc<UnlockedVault>> {
        self.vault
            .read()
            .unwrap_or_else(|p| p.into_inner())
            .clone()
            .ok_or(AppError::VaultLocked)
    }

    pub(crate) fn is_unlocked(&self) -> bool {
        self.vault
            .read()
            .unwrap_or_else(|p| p.into_inner())
            .is_some()
    }

    /// Lock: drop the key material.
    pub(crate) fn clear(&self) {
        let taken = self.vault.write().unwrap_or_else(|p| p.into_inner()).take();
        drop(taken);
    }

    fn with<T>(
        &self,
        f: impl FnOnce(&UnlockedVault) -> Result<T, VaultError>,
    ) -> Result<T, CodecError> {
        let guard = self.vault.read().unwrap_or_else(|p| p.into_inner());
        let v = guard.as_ref().ok_or(CodecError::Locked)?;
        f(v).map_err(map_err)
    }
}

impl ObjectCodec for VaultCodec {
    fn encrypt(
        &self,
        object_id: ObjectId,
        revision: i64,
        payload: &ObjectPayload,
    ) -> Result<EncryptedBody, CodecError> {
        self.with(|v| v.encrypt_object(object_id, revision, payload))
    }

    fn decrypt(
        &self,
        object_id: ObjectId,
        revision: i64,
        body: &EncryptedBody,
    ) -> Result<ObjectPayload, CodecError> {
        self.decrypt_hinted(object_id, revision, body, None)
    }

    fn decrypt_hinted(
        &self,
        object_id: ObjectId,
        revision: i64,
        body: &EncryptedBody,
        kek_class_hint: Option<KekClass>,
    ) -> Result<ObjectPayload, CodecError> {
        // crypto-core verifies the class after decryption and falls back to
        // trial unwrapping if the (non-authoritative) hint is wrong.
        self.with(|v| v.decrypt_object(object_id, revision, body, kek_class_hint))
            .map(|(payload, _class)| payload)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cc_models::host::Host;
    use cc_models::VaultObject;
    use cc_vault_core::{create_vault, Argon2Params, DeviceIdentity, SecretString};

    #[test]
    fn roundtrip_hint_and_lock() {
        let id = DeviceIdentity::generate().unwrap();
        let created = create_vault(
            &id,
            &SecretString::from("codec test passphrase"),
            Argon2Params::for_tests(),
            None,
            chrono::Utc::now(),
        )
        .unwrap();
        let codec = VaultCodec::new(created.unlocked);
        let h = Host::new("h", "192.0.2.1");
        let oid = h.id;
        let p = ObjectPayload::new(VaultObject::Host(h));
        let body = codec.encrypt(oid, 1, &p).unwrap();
        assert_eq!(codec.decrypt(oid, 1, &body).unwrap(), p);
        // A wrong hint still decrypts (trial fallback), class verified.
        assert_eq!(
            codec
                .decrypt_hinted(oid, 1, &body, Some(KekClass::Secrets))
                .unwrap(),
            p
        );
        // Revision binding.
        assert_eq!(codec.decrypt(oid, 2, &body), Err(CodecError::Integrity));
        codec.clear();
        assert_eq!(codec.decrypt(oid, 1, &body), Err(CodecError::Locked));
        assert!(matches!(codec.vault(), Err(AppError::VaultLocked)));
    }
}
