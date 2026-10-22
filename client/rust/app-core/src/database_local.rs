//! Device-local database connections (0.9.2): connections of engines that
//! clients up to 0.9.1 cannot decode ([`DbEngine::device_local`]) never
//! become vault objects, so they are never synced, shared, backed up or seen
//! half-decoded by an older device. They live in this profile's local
//! SQLCipher store, each object (the connection and its credential secrets)
//! still encrypted as a vault object payload under the vault's own keys, and
//! are decrypted into a per-unlock cache. A later release can move them into
//! the vault unchanged once every client decodes the engine.
//!
//! [`DbEngine::device_local`]: cc_models::database::DbEngine::device_local
use crate::session::{Session, Unlocked};
use crate::{AppError, AppResult};
use cc_models::{database::DbConnection, secret::Secret, ObjectId, ObjectPayload, VaultObject};
use cc_protocol::sync::EncryptedBody;
use cc_sync_core::ObjectCodec;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use tokio::sync::MutexGuard;

const KEY: &str = "database.local_connections.v1";
/// Most device-local objects (connections and their secrets) per profile.
const MAX_OBJECTS: usize = 3000;

fn invalid(reason: &'static str) -> AppError {
    AppError::invalid("database", reason)
}

/// One encrypted payload at its revision (bound into the ciphertext).
#[derive(Clone, Serialize, Deserialize)]
struct Sealed {
    revision: i64,
    body: EncryptedBody,
    /// A credential secret of a connection (collected when unreferenced).
    #[serde(default)]
    secret: bool,
}

#[derive(Default, Clone, Serialize, Deserialize)]
struct Stored {
    objects: BTreeMap<ObjectId, Sealed>,
}

/// Decrypted connections of this unlock; secrets stay sealed until used.
#[derive(Default)]
pub(crate) struct Loaded {
    stored: Stored,
    connections: BTreeMap<ObjectId, DbConnection>,
}

impl Loaded {
    pub(crate) fn connection(&self, id: ObjectId) -> Option<DbConnection> {
        self.connections.get(&id).cloned()
    }

    pub(crate) fn connections(&self) -> Vec<DbConnection> {
        self.connections.values().cloned().collect()
    }

    pub(crate) fn contains(&self, id: ObjectId) -> bool {
        self.stored.objects.contains_key(&id)
    }
}

/// Per-unlock cache of the device-local store.
#[derive(Default)]
pub(crate) struct LocalDb {
    cache: tokio::sync::Mutex<Option<Loaded>>,
}

impl std::fmt::Debug for LocalDb {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("LocalDb([REDACTED])")
    }
}

/// The device-local store of the unlocked vault, loaded on first use.
pub(crate) async fn load<'a>(
    owner: &Session,
    unlocked: &'a Unlocked,
) -> AppResult<MutexGuard<'a, Option<Loaded>>> {
    let mut cache = unlocked.db_local.cache.lock().await;
    if cache.is_none() {
        let stored: Stored = owner.storage()?.setting_get(KEY).await?.unwrap_or_default();
        let mut connections = BTreeMap::new();
        for (id, sealed) in &stored.objects {
            // An object this build cannot read is kept untouched.
            if let Ok(payload) = unlocked.codec.decrypt(*id, sealed.revision, &sealed.body) {
                if let VaultObject::DbConnection(connection) = payload.object {
                    if connection.id == *id {
                        connections.insert(*id, connection);
                    }
                }
            }
        }
        *cache = Some(Loaded {
            stored,
            connections,
        });
    }
    Ok(cache)
}

fn loaded<'a>(cache: &'a mut MutexGuard<'_, Option<Loaded>>) -> AppResult<&'a mut Loaded> {
    cache
        .as_mut()
        .ok_or_else(|| AppError::internal("device-local store not loaded"))
}

/// A connection from the vault or, failing that, from this device.
pub(crate) async fn connection(
    owner: &Session,
    unlocked: &Unlocked,
    id: ObjectId,
) -> AppResult<Option<DbConnection>> {
    if let Some(connection) = unlocked.working().db_connection(id) {
        return Ok(Some(connection));
    }
    let mut cache = load(owner, unlocked).await?;
    Ok(loaded(&mut cache)?.connection(id))
}

/// Whether `id` names a device-local connection.
pub(crate) async fn is_local(
    owner: &Session,
    unlocked: &Unlocked,
    id: ObjectId,
) -> AppResult<bool> {
    let mut cache = load(owner, unlocked).await?;
    Ok(loaded(&mut cache)?.connections.contains_key(&id))
}

/// A credential secret of a device-local connection (None: not local).
pub(crate) async fn secret(
    owner: &Session,
    unlocked: &Unlocked,
    id: ObjectId,
) -> AppResult<Option<Secret>> {
    let mut cache = load(owner, unlocked).await?;
    let Some(sealed) = loaded(&mut cache)?
        .stored
        .objects
        .get(&id)
        .filter(|sealed| sealed.secret)
        .cloned()
    else {
        return Ok(None);
    };
    drop(cache);
    match unlocked
        .codec
        .decrypt(id, sealed.revision, &sealed.body)?
        .object
    {
        VaultObject::Secret(secret) if secret.id == id => Ok(Some(secret)),
        _ => Err(AppError::invalid("secret_id", "not a secret object")),
    }
}

async fn persist(owner: &Session, stored: Stored) -> AppResult<()> {
    owner.storage()?.setting_set(KEY, stored).await?;
    Ok(())
}

/// Saves `connection` and new credential `secrets` on this device. Secrets
/// no local connection references any more are removed.
pub(crate) async fn save(
    owner: &Session,
    unlocked: &Unlocked,
    connection: DbConnection,
    secrets: Vec<Secret>,
) -> AppResult<()> {
    let mut cache = load(owner, unlocked).await?;
    let current = loaded(&mut cache)?;
    let mut stored = current.stored.clone();
    let mut seal = |object: VaultObject| -> AppResult<()> {
        let id = object.id();
        let secret = matches!(object, VaultObject::Secret(_));
        let revision = stored
            .objects
            .get(&id)
            .map_or(1, |sealed| sealed.revision.saturating_add(1));
        let body = unlocked
            .codec
            .encrypt(id, revision, &ObjectPayload::new(object))?;
        stored.objects.insert(
            id,
            Sealed {
                revision,
                body,
                secret,
            },
        );
        Ok(())
    };
    for secret in secrets {
        seal(VaultObject::Secret(secret))?;
    }
    seal(VaultObject::DbConnection(connection.clone()))?;
    let mut connections = current.connections.clone();
    connections.insert(connection.id, connection);
    collect_garbage(&mut stored, &connections);
    if stored.objects.len() > MAX_OBJECTS {
        return Err(invalid("too_many_local_connections"));
    }
    persist(owner, stored.clone()).await?;
    *current = Loaded {
        stored,
        connections,
    };
    Ok(())
}

/// Removes a device-local connection and the secrets only it used.
pub(crate) async fn delete(owner: &Session, unlocked: &Unlocked, id: ObjectId) -> AppResult<()> {
    let mut cache = load(owner, unlocked).await?;
    let current = loaded(&mut cache)?;
    if !current.connections.contains_key(&id) {
        return Err(invalid("connection_unavailable"));
    }
    let mut stored = current.stored.clone();
    let mut connections = current.connections.clone();
    stored.objects.remove(&id);
    connections.remove(&id);
    collect_garbage(&mut stored, &connections);
    persist(owner, stored.clone()).await?;
    *current = Loaded {
        stored,
        connections,
    };
    Ok(())
}

/// Drops sealed secrets no connection references; other objects (also
/// those this build could not decrypt) are kept.
fn collect_garbage(stored: &mut Stored, connections: &BTreeMap<ObjectId, DbConnection>) {
    let referenced: BTreeSet<ObjectId> = connections
        .values()
        .flat_map(|connection| connection.auth.secret_ids())
        .collect();
    stored
        .objects
        .retain(|id, sealed| !sealed.secret || referenced.contains(id));
}

/// Closes the live sessions of a connection that changed or was removed
/// (vault connections are revoked by working-set revision guards).
pub(crate) fn close_sessions(unlocked: &Unlocked, id: ObjectId) {
    unlocked.db.close_connection(&id.to_string());
}
