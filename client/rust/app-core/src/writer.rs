//! Mutation path of an unlocked vault: every write goes through the
//! sync-core [`ObjectStore`] (Local profiles: local revisions; Synced: the
//! outbox, via the engine when it runs) and is read back into the working
//! set, so facade reads right after a write see it.

use crate::error::{AppError, AppResult};
use crate::working_set::{Readback, WorkingSet};
use cc_models::secret::Secret;
use cc_models::{ObjectId, ObjectKind, ObjectPayload, VaultObject};
use cc_sync_core::{DecryptedObject, ObjectStore, SyncEngine};
use std::sync::{Arc, RwLock};

/// Shared slot for the (optional) sync engine of the session.
pub(crate) type EngineSlot = Arc<RwLock<Option<SyncEngine>>>;

#[derive(Clone)]
pub(crate) struct VaultWriter {
    pub store: ObjectStore,
    pub engine: EngineSlot,
    pub working: Arc<WorkingSet>,
}

impl std::fmt::Debug for VaultWriter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("VaultWriter")
            .field("vault_id", &self.store.vault_id())
            .finish_non_exhaustive()
    }
}

impl VaultWriter {
    pub(crate) fn engine(&self) -> Option<SyncEngine> {
        self.engine
            .read()
            .unwrap_or_else(|p| p.into_inner())
            .clone()
    }

    /// Create or update an object.
    pub(crate) async fn put(&self, object: VaultObject) -> AppResult<()> {
        let id = object.id();
        let payload = ObjectPayload::new(object);
        match self.engine() {
            Some(e) => {
                e.put(payload).await?;
            }
            None => {
                self.store.put(payload).await?;
            }
        }
        self.refresh(id).await?;
        Ok(())
    }

    /// Delete an object (tombstone, or removal if it never left the device).
    pub(crate) async fn delete(&self, id: ObjectId) -> AppResult<()> {
        match self.engine() {
            Some(e) => {
                e.delete(id).await?;
            }
            None => {
                self.store.delete(id).await?;
            }
        }
        self.refresh(id).await?;
        Ok(())
    }

    /// Read one object back from storage (decrypting it).
    pub(crate) async fn readback(&self, id: ObjectId) -> AppResult<Readback> {
        let Some(stored) = self.store.stored(id).await? else {
            return Ok(Readback::Missing);
        };
        if stored.deleted {
            return Ok(Readback::Deleted {
                revision: stored.revision,
            });
        }
        let body = stored
            .body
            .as_ref()
            .ok_or_else(|| AppError::Storage("live object without body".into()))?;
        let payload =
            self.store
                .codec()
                .decrypt_hinted(id, stored.revision, body, stored.kek_class_hint)?;
        Ok(Readback::Live(Box::new(DecryptedObject {
            object_id: id,
            revision: stored.revision,
            local_state: stored.local_state,
            conflict_origin: stored.conflict_origin,
            payload,
        })))
    }

    /// Refresh one working-set entry; returns the affected kind.
    pub(crate) async fn refresh(&self, id: ObjectId) -> AppResult<Option<ObjectKind>> {
        let before = self.working.kind_of(id);
        let r = self.readback(id).await?;
        Ok(self.working.apply(id, r).or(before))
    }

    /// Reload the whole working set from storage.
    pub(crate) async fn reload_all(&self) -> AppResult<usize> {
        let (objects, failed) = self.store.list().await?;
        for (id, e) in &failed {
            tracing::warn!(object_id = %id, error = %e, "object failed to decrypt; skipped");
        }
        let n = objects.len();
        self.working.replace_all(objects);
        Ok(n)
    }

    /// Decrypt a Secret object on demand (never cached).
    pub(crate) async fn read_secret(&self, id: ObjectId) -> AppResult<Secret> {
        match self.readback(id).await? {
            Readback::Live(d) => match d.payload.object {
                VaultObject::Secret(s) => Ok(s),
                _ => Err(AppError::invalid("secret_id", "not a secret object")),
            },
            _ => Err(AppError::not_found("secret", id)),
        }
    }
}
