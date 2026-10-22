//! Mutation path of an unlocked vault: every write goes through the
//! sync-core [`ObjectStore`] (Local profiles: local revisions; Synced: the
//! outbox, via the engine when it runs) and is read back into the working
//! set, so facade reads right after a write see it.
//!
//! Teams: an object of a team vault is written through that team's store
//! and engine (editor+ only); a new object is placed by
//! [`crate::teams::infer_vault`] unless the caller names the vault.

use crate::error::{AppError, AppResult};
use crate::teams::{check_references, infer_vault, TeamHandle, TeamRegistry};
use crate::working_set::{Readback, WorkingSet};
use cc_models::secret::Secret;
use cc_models::{ObjectId, ObjectKind, ObjectPayload, VaultObject};
use cc_protocol::VaultId;
use cc_sync_core::{DecryptedObject, ObjectStore, SyncEngine};
use std::sync::{Arc, RwLock};

/// Shared slot for the (optional) sync engine of the session.
pub(crate) type EngineSlot = Arc<RwLock<Option<SyncEngine>>>;

#[derive(Clone)]
pub(crate) struct VaultWriter {
    pub store: ObjectStore,
    pub engine: EngineSlot,
    pub working: Arc<WorkingSet>,
    pub teams: Arc<TeamRegistry>,
}

/// Where one write goes.
enum Target {
    Personal,
    Team(Arc<TeamHandle>),
}

impl Target {
    fn vault(&self) -> Option<VaultId> {
        match self {
            Target::Personal => None,
            Target::Team(t) => Some(t.vault_id),
        }
    }
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

    fn team_target(&self, vault: VaultId) -> AppResult<Target> {
        let team = self.teams.require(vault)?;
        team.require_writable()?;
        Ok(Target::Team(team))
    }

    /// Vault of an object about to be written (`explicit` for new objects
    /// the caller places; existing objects never change vault here).
    fn target_for(
        &self,
        object: &VaultObject,
        explicit: Option<Option<VaultId>>,
    ) -> AppResult<Target> {
        let id = object.id();
        let vault = if self.working.kind_of(id).is_some() {
            let current = self.working.vault_of(id);
            if explicit.is_some_and(|e| e != current) {
                return Err(AppError::invalid("vault", "object_in_other_vault"));
            }
            current
        } else {
            match explicit {
                Some(v) => v,
                None => infer_vault(&self.working, object)?,
            }
        };
        check_references(&self.working, vault, object)?;
        match vault {
            None => Ok(Target::Personal),
            Some(v) => self.team_target(v),
        }
    }

    fn target_of_existing(&self, id: ObjectId) -> AppResult<Target> {
        match self.working.vault_of(id) {
            None => Ok(Target::Personal),
            Some(v) => self.team_target(v),
        }
    }

    fn parts(&self, target: &Target) -> (ObjectStore, Option<SyncEngine>) {
        match target {
            Target::Personal => (self.store.clone(), self.engine()),
            Target::Team(t) => (t.store.clone(), t.engine()),
        }
    }

    /// Team policy for new objects: production by default (ТР-25).
    fn with_new_object_policies(&self, target: &Target, mut object: VaultObject) -> VaultObject {
        let Target::Team(team) = target else {
            return object;
        };
        if self.working.kind_of(object.id()).is_some() || !team.policies().production_by_default {
            return object;
        }
        match &mut object {
            VaultObject::KubeContext(c) => c.production = true,
            VaultObject::DbConnection(c) => c.production = true,
            _ => {}
        }
        object
    }

    /// Create or update an object.
    pub(crate) async fn put(&self, object: VaultObject) -> AppResult<()> {
        let target = self.target_for(&object, None)?;
        let object = self.with_new_object_policies(&target, object);
        self.put_to(target, object).await
    }

    /// Create an object in `vault` (`None` = personal).
    pub(crate) async fn put_in(
        &self,
        vault: Option<VaultId>,
        object: VaultObject,
    ) -> AppResult<()> {
        let target = self.target_for(&object, Some(vault))?;
        let object = self.with_new_object_policies(&target, object);
        self.put_to(target, object).await
    }

    /// Write a team's `VaultSettings` (admins only; never in the shared
    /// working set, see [`crate::working_set`]).
    pub(crate) async fn put_team_settings(
        &self,
        vault: VaultId,
        settings: cc_models::settings::VaultSettings,
    ) -> AppResult<()> {
        let team = self.teams.require(vault)?;
        team.require_admin()?;
        self.put_to(Target::Team(team), VaultObject::VaultSettings(settings))
            .await
    }

    async fn put_to(&self, target: Target, object: VaultObject) -> AppResult<()> {
        let id = object.id();
        let payload = ObjectPayload::new(object);
        let (store, engine) = self.parts(&target);
        match engine {
            Some(e) => {
                e.put(payload).await?;
            }
            None => {
                store.put(payload).await?;
            }
        }
        self.refresh_in(target.vault(), id).await?;
        Ok(())
    }

    /// Commit related objects atomically, then refresh their inventory
    /// entries. All of them must belong to one vault.
    pub(crate) async fn put_batch(&self, objects: Vec<VaultObject>) -> AppResult<()> {
        let new_team = objects
            .iter()
            .filter(|o| self.working.kind_of(o.id()).is_none())
            .map(|o| infer_vault(&self.working, o))
            .collect::<AppResult<Vec<_>>>()?
            .into_iter()
            .flatten()
            .next();
        let existing = objects
            .iter()
            .find(|o| self.working.kind_of(o.id()).is_some())
            .map(|o| self.working.vault_of(o.id()));
        let vault = existing.unwrap_or(new_team);
        self.put_batch_in(vault, objects).await
    }

    /// [`VaultWriter::put_batch`] into `vault` (`None` = personal).
    pub(crate) async fn put_batch_in(
        &self,
        vault: Option<VaultId>,
        objects: Vec<VaultObject>,
    ) -> AppResult<()> {
        let mut target = Target::Personal;
        let mut checked = Vec::with_capacity(objects.len());
        for o in objects {
            target = self.target_for(&o, Some(vault))?;
            checked.push(self.with_new_object_policies(&target, o));
        }
        let objects = checked;
        let ids: Vec<_> = objects.iter().map(VaultObject::id).collect();
        let payloads = objects.into_iter().map(ObjectPayload::new).collect();
        let (store, engine) = self.parts(&target);
        match engine {
            Some(engine) => {
                engine.put_batch(payloads).await?;
            }
            None => {
                store.put_batch(payloads).await?;
            }
        }
        for id in ids {
            self.refresh_in(vault, id).await?;
        }
        Ok(())
    }

    /// Delete an object (tombstone, or removal if it never left the device).
    pub(crate) async fn delete(&self, id: ObjectId) -> AppResult<()> {
        let target = self.target_of_existing(id)?;
        let (store, engine) = self.parts(&target);
        match engine {
            Some(e) => {
                e.delete(id).await?;
            }
            None => {
                store.delete(id).await?;
            }
        }
        self.refresh_in(target.vault(), id).await?;
        Ok(())
    }

    fn store_of(&self, vault: Option<VaultId>) -> AppResult<ObjectStore> {
        match vault {
            None => Ok(self.store.clone()),
            Some(v) => Ok(self.teams.require(v)?.store.clone()),
        }
    }

    /// Read one object back from storage (decrypting it).
    pub(crate) async fn readback(&self, id: ObjectId) -> AppResult<Readback> {
        let store = self.store_of(self.working.vault_of(id))?;
        readback_from(&store, id).await
    }

    /// Refresh one working-set entry; returns the affected kind.
    pub(crate) async fn refresh(&self, id: ObjectId) -> AppResult<Option<ObjectKind>> {
        self.refresh_in(self.working.vault_of(id), id).await
    }

    /// Refresh one entry of `vault` (`None` = personal).
    pub(crate) async fn refresh_in(
        &self,
        vault: Option<VaultId>,
        id: ObjectId,
    ) -> AppResult<Option<ObjectKind>> {
        let store = self.store_of(vault)?;
        let before = self.working.kind_of(id);
        let r = readback_from(&store, id).await?;
        if let (Some(v), Readback::Live(d)) = (vault, &r) {
            if let VaultObject::VaultSettings(s) = &d.payload.object {
                if let Some(team) = self.teams.get(v) {
                    team.set_settings(&self.working, Some(s.clone()));
                }
                return Ok(Some(ObjectKind::VaultSettings));
            }
        }
        Ok(self.working.apply_in(vault, id, r).or(before))
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

/// Read one object back from `store` (decrypting it).
pub(crate) async fn readback_from(store: &ObjectStore, id: ObjectId) -> AppResult<Readback> {
    let Some(stored) = store.stored(id).await? else {
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
    let payload = store
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
