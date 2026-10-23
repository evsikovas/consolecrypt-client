//! Credential secrets of device-local connections (`database_local`): the
//! password or TLS client key of a Cassandra / ScyllaDB connection never
//! becomes a vault object either. Each secret is sealed exactly like a vault
//! object payload (the vault's keys, its own data key, bound to its id and
//! revision) and kept in this profile's local store; it is decrypted only
//! when a session opens.
use crate::{AppCore, AppError, AppResult};
use cc_models::{secret::Secret, ObjectId, ObjectPayload, VaultObject};
use cc_protocol::sync::EncryptedBody;
use cc_sync_core::ObjectCodec;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

/// Local store key: sealed secrets by id.
const LOCAL_SECRETS: &str = "database.local_secrets.v1";

/// One sealed secret at its revision (bound into the ciphertext).
#[derive(Clone, Serialize, Deserialize)]
struct Sealed {
    revision: i64,
    body: EncryptedBody,
}

type Store = BTreeMap<ObjectId, Sealed>;

impl AppCore {
    /// A credential secret of a device-local connection, if it is one.
    pub(crate) async fn db_local_secret(&self, id: ObjectId) -> AppResult<Option<Secret>> {
        let (owner, unlocked) = self.unlocked().await?;
        let store: Store = owner
            .storage()?
            .setting_get(LOCAL_SECRETS)
            .await?
            .unwrap_or_default();
        let Some(sealed) = store.get(&id) else {
            return Ok(None);
        };
        match unlocked
            .codec
            .decrypt(id, sealed.revision, &sealed.body)?
            .object
        {
            VaultObject::Secret(secret) if secret.id == id => Ok(Some(secret)),
            _ => Err(AppError::invalid("secret_id", "not a secret object")),
        }
    }

    /// Whether `id` names a sealed device-local secret.
    pub(crate) async fn db_has_local_secret(&self, id: ObjectId) -> AppResult<bool> {
        let (owner, _) = self.unlocked().await?;
        let store: Store = owner
            .storage()?
            .setting_get(LOCAL_SECRETS)
            .await?
            .unwrap_or_default();
        Ok(store.contains_key(&id))
    }

    /// Seals and stores new secrets of a device-local connection.
    pub(crate) async fn db_save_local_secrets(&self, secrets: Vec<Secret>) -> AppResult<()> {
        if secrets.is_empty() {
            return Ok(());
        }
        let (owner, unlocked) = self.unlocked().await?;
        let mut sealed = Vec::with_capacity(secrets.len());
        for secret in secrets {
            let id = secret.id;
            let body =
                unlocked
                    .codec
                    .encrypt(id, 1, &ObjectPayload::new(VaultObject::Secret(secret)))?;
            sealed.push((id, Sealed { revision: 1, body }));
        }
        let _profile = self.hold_session(&owner).await?;
        let _generation = owner.hold_unlocked(&unlocked).await?;
        owner
            .storage()?
            .write(move |tx| {
                let mut store: Store = tx.setting_get(LOCAL_SECRETS)?.unwrap_or_default();
                store.extend(sealed);
                tx.setting_set(LOCAL_SECRETS, &store)
            })
            .await?;
        Ok(())
    }

    /// Drops sealed secrets no device-local connection refers to (after a
    /// replaced password or a deleted connection).
    pub(crate) async fn db_prune_local_secrets(&self) -> AppResult<()> {
        let referenced: BTreeSet<ObjectId> = self
            .db_local_connections()
            .await?
            .iter()
            .flat_map(|connection| connection.auth.secret_ids())
            .collect();
        let (owner, unlocked) = self.unlocked().await?;
        let _profile = self.hold_session(&owner).await?;
        let _generation = owner.hold_unlocked(&unlocked).await?;
        owner
            .storage()?
            .write(move |tx| {
                let mut store: Store = tx.setting_get(LOCAL_SECRETS)?.unwrap_or_default();
                let before = store.len();
                store.retain(|id, _| referenced.contains(id));
                if store.len() != before {
                    tx.setting_set(LOCAL_SECRETS, &store)?;
                }
                Ok::<_, cc_storage_core::StorageError>(())
            })
            .await?;
        Ok(())
    }
}
