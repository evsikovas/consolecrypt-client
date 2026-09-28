//! Local-only profiles (ADR-0106): local mutations without an outbox,
//! Local → Synced conversion ("enable sync" / reconnect), Synced → Local
//! ("disconnect"), and the storage side of encrypted backups.

use crate::db::Tx;
use crate::error::{Result, StorageError};
use crate::model::{
    ExportedObject, LocalState, OutboxOp, Profile, ProfileKind, StoredObject, SyncCursor,
    VaultExport, VaultRecord,
};
use crate::sql::now;
use crate::sync_ops::LocalMutationOutcome;
use cc_models::KekClass;
use cc_protocol::sync::EncryptedBody;
use cc_protocol::vaults::VaultRole;
use cc_protocol::{MutationId, ObjectId, VaultId};
use std::collections::HashSet;

/// How a local vault is attached to a server.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AttachMode {
    /// The vault was just created on the server (`POST /v1/vaults` → 201):
    /// every live object is uploaded as a create (revision 1); tombstones
    /// and old server revisions are dropped.
    Fresh,
    /// The vault already exists on the server (re-attach after disconnect or
    /// restore): unmodified objects are kept as synced, local modifications
    /// are queued on top of the last known server revision, never-synced
    /// objects are queued as creates; a fresh snapshot is scheduled and
    /// overlaps are resolved with the ADR-0003 rules.
    Reconnect,
}

/// Result of [`Tx::attach_to_server`].
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct AttachSummary {
    /// Mutations queued for upload.
    pub queued: usize,
    /// Objects kept as synced (reconnect only).
    pub unchanged: usize,
    /// Local tombstones / never-synced deletions dropped.
    pub dropped: usize,
}

/// Result of [`Tx::detach_to_local`].
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct DetachSummary {
    /// Unpushed outbox entries discarded (their data stays in `objects`).
    pub discarded_mutations: usize,
    /// Objects now `LocalOnly`.
    pub objects: usize,
}

/// Result of [`Tx::import_vault`].
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ImportSummary {
    pub objects: usize,
}

impl Tx<'_> {
    pub(crate) fn record_local_only_put<E, F>(
        &self,
        vault_id: VaultId,
        object_id: ObjectId,
        kek_class_hint: Option<KekClass>,
        encrypt: F,
    ) -> Result<LocalMutationOutcome, E>
    where
        E: From<StorageError>,
        F: FnOnce(i64) -> Result<EncryptedBody, E>,
    {
        let obj = self.get_object(vault_id, object_id)?;
        let revision = obj.as_ref().map_or(1, |o| o.revision + 1);
        let body = encrypt(revision)?;
        self.upsert_object(&StoredObject {
            vault_id,
            object_id,
            revision,
            server_revision: obj.as_ref().map_or(0, |o| o.server_revision),
            sequence: obj.as_ref().and_then(|o| o.sequence),
            deleted: false,
            body: Some(body),
            kek_class_hint: kek_class_hint.or(obj.as_ref().and_then(|o| o.kek_class_hint)),
            local_state: LocalState::LocalOnly,
            conflict_origin: obj.as_ref().and_then(|o| o.conflict_origin),
            writer_device_id: obj.as_ref().and_then(|o| o.writer_device_id),
            updated_at: now(),
        })?;
        Ok(LocalMutationOutcome::LocalOnly {
            revision,
            deleted: false,
        })
    }

    pub(crate) fn record_local_only_delete(
        &self,
        vault_id: VaultId,
        object_id: ObjectId,
    ) -> Result<LocalMutationOutcome> {
        let Some(obj) = self.get_object(vault_id, object_id)? else {
            return Ok(LocalMutationOutcome::NoOp);
        };
        if obj.deleted {
            return Ok(LocalMutationOutcome::NoOp);
        }
        if obj.server_revision == 0 {
            // Never reached a server: no tombstone needed.
            self.delete_object_row(vault_id, object_id)?;
            return Ok(LocalMutationOutcome::DroppedUnsent);
        }
        let revision = obj.revision + 1;
        self.upsert_object(&StoredObject {
            revision,
            deleted: true,
            body: None,
            local_state: LocalState::LocalOnly,
            updated_at: now(),
            ..obj
        })?;
        Ok(LocalMutationOutcome::LocalOnly {
            revision,
            deleted: true,
        })
    }

    /// Convert a local vault into a synced one (ADR-0106 "enable sync"),
    /// atomically: switches the profile row to `profile` (must be `Synced`),
    /// queues uploads with fresh, persisted mutation ids and resets the sync
    /// cursor. `reencrypt(object, revision)` must return the object's current
    /// payload encrypted for `revision` (fresh DEK).
    ///
    /// Resumable by construction: once committed, the upload is ordinary
    /// outbox content that any later sync run pushes idempotently.
    pub fn attach_to_server<E, F>(
        &self,
        vault_id: VaultId,
        profile: &Profile,
        mode: AttachMode,
        mut reencrypt: F,
    ) -> Result<AttachSummary, E>
    where
        E: From<StorageError>,
        F: FnMut(&StoredObject, i64) -> Result<EncryptedBody, E>,
    {
        if profile.kind != ProfileKind::Synced {
            return Err(StorageError::Invalid("attach needs a synced profile".into()).into());
        }
        let mut summary = AttachSummary::default();
        self.outbox_clear(vault_id)?;
        self.c()
            .execute(
                "DELETE FROM remote_objects WHERE vault_id = ?1",
                rusqlite::params![vault_id.to_string()],
            )
            .map_err(StorageError::from)?;
        for obj in self.list_objects(vault_id, true)? {
            let (vault, id) = (obj.vault_id, obj.object_id);
            let never_synced = mode == AttachMode::Fresh || obj.server_revision == 0;
            if never_synced {
                if obj.deleted {
                    self.delete_object_row(vault, id)?;
                    summary.dropped += 1;
                    continue;
                }
                let body = reencrypt(&obj, 1)?;
                self.upsert_object(&StoredObject {
                    revision: 1,
                    server_revision: 0,
                    sequence: None,
                    body: Some(body),
                    local_state: LocalState::Pending,
                    writer_device_id: None,
                    updated_at: now(),
                    ..obj
                })?;
                let row = self.get_object(vault, id)?;
                self.outbox_insert(
                    MutationId::new(),
                    vault,
                    id,
                    0,
                    OutboxOp::Put,
                    row.as_ref().and_then(|r| r.body.as_ref()),
                )?;
                summary.queued += 1;
                continue;
            }
            if obj.revision <= obj.server_revision {
                self.set_object_state(vault, id, LocalState::Synced)?;
                summary.unchanged += 1;
                continue;
            }
            // Modified since the last sync: queue on top of the known base.
            let base = obj.server_revision;
            if obj.deleted {
                self.upsert_object(&StoredObject {
                    revision: base + 1,
                    local_state: LocalState::Pending,
                    updated_at: now(),
                    ..obj
                })?;
                self.outbox_insert(MutationId::new(), vault, id, base, OutboxOp::Delete, None)?;
            } else {
                let body = if obj.revision == base + 1 {
                    obj.body
                        .clone()
                        .ok_or_else(|| StorageError::Invalid("live object without body".into()))?
                } else {
                    reencrypt(&obj, base + 1)?
                };
                self.upsert_object(&StoredObject {
                    revision: base + 1,
                    body: Some(body.clone()),
                    local_state: LocalState::Pending,
                    updated_at: now(),
                    ..obj
                })?;
                self.outbox_insert(
                    MutationId::new(),
                    vault,
                    id,
                    base,
                    OutboxOp::Put,
                    Some(&body),
                )?;
            }
            summary.queued += 1;
        }
        self.clear_recovery_remote(vault_id)?;
        let mut cursor = SyncCursor::new(vault_id);
        match mode {
            // Empty vault server-side: nothing to snapshot.
            AttachMode::Fresh => cursor.snapshot_complete = true,
            AttachMode::Reconnect => self.clear_snapshot_seen(vault_id)?,
        }
        self.put_sync_cursor(&cursor)?;
        let mut vault = self
            .get_vault(vault_id)?
            .unwrap_or_else(|| VaultRecord::new(vault_id, VaultRole::Owner));
        vault.caller_trusted = true;
        vault.updated_at = now();
        self.put_vault(&vault)?;
        self.put_profile(profile)?;
        Ok(summary)
    }

    /// Convert a synced vault into a local one (ADR-0106 "disconnect"),
    /// atomically: discards the outbox and sync cursor, marks every object
    /// `LocalOnly` (keeping revisions and the last known server revision for
    /// a later reconnect) and switches the profile row to `profile` (must be
    /// `Local`). Unpushed edits are kept as local modifications.
    pub fn detach_to_local(&self, vault_id: VaultId, profile: &Profile) -> Result<DetachSummary> {
        if profile.kind != ProfileKind::Local {
            return Err(StorageError::Invalid("detach needs a local profile".into()));
        }
        let v = vault_id.to_string();
        let discarded = self.outbox_clear(vault_id)?;
        for table in [
            "remote_objects",
            "sync_state",
            "snapshot_seen",
            "recovery_remote",
        ] {
            self.c().execute(
                &format!("DELETE FROM {table} WHERE vault_id = ?1"),
                rusqlite::params![v],
            )?;
        }
        let objects = self.c().execute(
            "UPDATE objects SET local_state = 'local_only' WHERE vault_id = ?1",
            rusqlite::params![v],
        )?;
        if let Some(mut vault) = self.get_vault(vault_id)? {
            vault.caller_trusted = false;
            vault.updated_at = now();
            self.put_vault(&vault)?;
        }
        self.put_profile(profile)?;
        Ok(DetachSummary {
            discarded_mutations: discarded,
            objects,
        })
    }

    /// Visit every live object of a vault (ciphertext) in object-id order.
    pub fn for_each_live_object<E, F>(&self, vault_id: VaultId, mut f: F) -> Result<(), E>
    where
        E: From<StorageError>,
        F: FnMut(StoredObject) -> Result<(), E>,
    {
        for obj in self.list_objects(vault_id, false)? {
            f(obj)?;
        }
        Ok(())
    }

    /// Storage-side backup content of a vault: password + recovery envelopes
    /// and every live object's ciphertext with its revision.
    pub fn export_vault(&self, vault_id: VaultId) -> Result<VaultExport> {
        let vault = self
            .get_vault(vault_id)?
            .ok_or_else(|| StorageError::Invalid("unknown vault".into()))?;
        let mut objects = Vec::new();
        self.for_each_live_object::<StorageError, _>(vault_id, |o| {
            let body = o
                .body
                .ok_or_else(|| StorageError::Invalid("live object without body".into()))?;
            objects.push(ExportedObject {
                object_id: o.object_id,
                revision: o.revision,
                body,
                kek_class_hint: None,
            });
            Ok(())
        })?;
        Ok(VaultExport {
            vault_id,
            password_envelope: vault.password_envelope,
            recovery_envelope: vault.recovery_envelope,
            objects,
            exported_at: now(),
        })
    }

    /// Restore a backup into this (local, empty) profile, atomically. The
    /// vault record is created if missing (role owner, untrusted) and its
    /// password/recovery envelopes are replaced by the backup's.
    pub fn import_vault(&self, export: &VaultExport) -> Result<ImportSummary> {
        if self.profile_kind()? != ProfileKind::Local {
            return Err(StorageError::Invalid(
                "backups are imported into local profiles (enable sync afterwards)".into(),
            ));
        }
        let vault_id = export.vault_id;
        if !self.list_objects(vault_id, true)?.is_empty() {
            return Err(StorageError::Invalid(
                "vault already has local objects".into(),
            ));
        }
        let mut vault = self
            .get_vault(vault_id)?
            .unwrap_or_else(|| VaultRecord::new(vault_id, VaultRole::Owner));
        vault.password_envelope = export.password_envelope.clone();
        vault.recovery_envelope = export.recovery_envelope.clone();
        vault.device_envelope = None;
        vault.caller_trusted = false;
        vault.updated_at = now();
        self.put_vault(&vault)?;
        let mut seen = HashSet::new();
        for o in &export.objects {
            if !seen.insert(o.object_id) || o.revision < 1 {
                return Err(StorageError::Invalid(
                    "duplicate object or invalid revision".into(),
                ));
            }
            self.upsert_object(&StoredObject {
                vault_id,
                object_id: o.object_id,
                revision: o.revision,
                server_revision: 0,
                sequence: None,
                deleted: false,
                body: Some(o.body.clone()),
                kek_class_hint: o.kek_class_hint,
                local_state: LocalState::LocalOnly,
                conflict_origin: None,
                writer_device_id: None,
                updated_at: now(),
            })?;
        }
        Ok(ImportSummary {
            objects: export.objects.len(),
        })
    }
}
