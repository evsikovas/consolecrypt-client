//! Local object access for one vault: encrypted local mutations and
//! decrypting reads. Works in both profile kinds (ADR-0106): in a `Local`
//! profile mutations get local revisions and nothing is queued; in a
//! `Synced` profile they are queued in the outbox for the [`crate::SyncEngine`].

use crate::codec::{CodecError, ObjectCodec};
use crate::error::SyncError;
use crate::events::{ChangeOrigin, SyncEvent};
use cc_models::ObjectPayload;
use cc_protocol::limits::MAX_OBJECT_CIPHERTEXT_BYTES;
use cc_protocol::sync::EncryptedBody;
use cc_protocol::{ObjectId, VaultId};
use cc_storage_core::{LocalMutationOutcome, LocalState, OutboxCounts, Storage, StoredObject};
use std::sync::Arc;
use tokio::sync::{broadcast, Notify};

/// A decrypted live object.
#[derive(Debug, Clone, PartialEq)]
pub struct DecryptedObject {
    pub object_id: ObjectId,
    pub revision: i64,
    pub local_state: LocalState,
    /// Set on conflict copies: the object the copy was made from.
    pub conflict_origin: Option<ObjectId>,
    pub payload: ObjectPayload,
}

pub(crate) struct StoreInner {
    pub(crate) vault_id: VaultId,
    pub(crate) storage: Storage,
    pub(crate) codec: Arc<dyn ObjectCodec>,
    pub(crate) events: broadcast::Sender<SyncEvent>,
    pub(crate) local_changes: Notify,
}

/// Cloneable handle for reading and mutating one vault's objects.
#[derive(Clone)]
pub struct ObjectStore {
    pub(crate) inner: Arc<StoreInner>,
}

impl std::fmt::Debug for ObjectStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ObjectStore")
            .field("vault_id", &self.inner.vault_id)
            .finish_non_exhaustive()
    }
}

/// Reject ciphertexts the server would refuse (`413`).
pub(crate) fn check_size(body: &EncryptedBody) -> Result<(), SyncError> {
    if body.ciphertext.len() > MAX_OBJECT_CIPHERTEXT_BYTES {
        return Err(SyncError::ObjectTooLarge {
            size: body.ciphertext.len(),
            max: MAX_OBJECT_CIPHERTEXT_BYTES,
        });
    }
    Ok(())
}

impl ObjectStore {
    /// Handle for `vault_id` in `storage`, encrypting with `codec`.
    pub fn new(vault_id: VaultId, storage: Storage, codec: Arc<dyn ObjectCodec>) -> Self {
        let (events, _) = broadcast::channel(1024);
        Self {
            inner: Arc::new(StoreInner {
                vault_id,
                storage,
                codec,
                events,
                local_changes: Notify::new(),
            }),
        }
    }

    /// The vault this store serves.
    pub fn vault_id(&self) -> VaultId {
        self.inner.vault_id
    }

    /// Underlying storage.
    pub fn storage(&self) -> &Storage {
        &self.inner.storage
    }

    /// Codec in use.
    pub fn codec(&self) -> &Arc<dyn ObjectCodec> {
        &self.inner.codec
    }

    /// Subscribe to object/sync events of this vault.
    pub fn subscribe(&self) -> broadcast::Receiver<SyncEvent> {
        self.inner.events.subscribe()
    }

    pub(crate) fn emit(&self, event: SyncEvent) {
        // No receivers is fine.
        let _ = self.inner.events.send(event);
    }

    /// Create or update an object (id = `payload.object.id()`): encrypt for
    /// the next revision and store + queue it in one transaction.
    pub async fn put(&self, payload: ObjectPayload) -> Result<LocalMutationOutcome, SyncError> {
        let object_id = payload.object.id();
        let kek = payload.object.kind().kek_class();
        let codec = self.inner.codec.clone();
        let vault_id = self.inner.vault_id;
        let outcome = self
            .inner
            .storage
            .write(move |tx| {
                tx.record_local_put::<SyncError, _>(vault_id, object_id, Some(kek), |revision| {
                    let body = codec.encrypt(object_id, revision, &payload)?;
                    check_size(&body)?;
                    Ok(body)
                })
            })
            .await?;
        self.after_local(object_id, &outcome, false);
        Ok(outcome)
    }

    /// Delete an object (tombstone; removal if it never reached a server).
    pub async fn delete(&self, object_id: ObjectId) -> Result<LocalMutationOutcome, SyncError> {
        let vault_id = self.inner.vault_id;
        let outcome = self
            .inner
            .storage
            .write(move |tx| tx.record_local_delete(vault_id, object_id))
            .await?;
        self.after_local(object_id, &outcome, true);
        Ok(outcome)
    }

    fn after_local(&self, object_id: ObjectId, outcome: &LocalMutationOutcome, deleted: bool) {
        let vault_id = self.inner.vault_id;
        match *outcome {
            LocalMutationOutcome::Queued { revision, .. } => {
                self.emit(SyncEvent::ObjectChanged {
                    vault_id,
                    object_id,
                    revision,
                    deleted,
                    origin: ChangeOrigin::Local,
                });
                self.inner.local_changes.notify_one();
            }
            LocalMutationOutcome::LocalOnly { revision, deleted } => {
                self.emit(SyncEvent::ObjectChanged {
                    vault_id,
                    object_id,
                    revision,
                    deleted,
                    origin: ChangeOrigin::Local,
                });
            }
            LocalMutationOutcome::DroppedUnsent => {
                self.emit(SyncEvent::ObjectRemoved {
                    vault_id,
                    object_id,
                });
            }
            LocalMutationOutcome::NoOp => {}
        }
    }

    /// Stored (encrypted) state of an object, including tombstones.
    pub async fn stored(&self, object_id: ObjectId) -> Result<Option<StoredObject>, SyncError> {
        let vault_id = self.inner.vault_id;
        Ok(self
            .inner
            .storage
            .read(move |tx| tx.get_object(vault_id, object_id))
            .await?)
    }

    /// Decrypt one live object (`None` if missing or deleted).
    pub async fn get(&self, object_id: ObjectId) -> Result<Option<DecryptedObject>, SyncError> {
        let vault_id = self.inner.vault_id;
        let codec = self.inner.codec.clone();
        self.inner
            .storage
            .read(move |tx| -> Result<_, SyncError> {
                let Some(obj) = tx.get_object(vault_id, object_id)? else {
                    return Ok(None);
                };
                Ok(decrypt_stored(codec.as_ref(), obj).transpose()?)
            })
            .await
    }

    /// Decrypt every live object. Objects that fail to decrypt are returned
    /// separately (e.g. tampered ciphertext) instead of failing the call.
    #[allow(clippy::type_complexity)]
    pub async fn list(
        &self,
    ) -> Result<(Vec<DecryptedObject>, Vec<(ObjectId, CodecError)>), SyncError> {
        let vault_id = self.inner.vault_id;
        let codec = self.inner.codec.clone();
        self.inner
            .storage
            .read(move |tx| -> Result<_, SyncError> {
                let mut ok = Vec::new();
                let mut failed = Vec::new();
                for obj in tx.list_objects(vault_id, false)? {
                    let id = obj.object_id;
                    match decrypt_stored(codec.as_ref(), obj) {
                        Some(Ok(d)) => ok.push(d),
                        Some(Err(e)) => failed.push((id, e)),
                        None => {}
                    }
                }
                Ok((ok, failed))
            })
            .await
    }

    /// Outbox counts of this vault.
    pub async fn outbox_counts(&self) -> Result<OutboxCounts, SyncError> {
        let vault_id = self.inner.vault_id;
        Ok(self
            .inner
            .storage
            .read(move |tx| tx.outbox_counts(vault_id))
            .await?)
    }
}

fn decrypt_stored(
    codec: &dyn ObjectCodec,
    obj: StoredObject,
) -> Option<Result<DecryptedObject, CodecError>> {
    let body = obj.body.as_ref()?;
    if obj.deleted {
        return None;
    }
    Some(
        codec
            .decrypt_hinted(obj.object_id, obj.revision, body, obj.kek_class_hint)
            .map(|payload| DecryptedObject {
                object_id: obj.object_id,
                revision: obj.revision,
                local_state: obj.local_state,
                conflict_origin: obj.conflict_origin,
                payload,
            }),
    )
}
