//! Composite, transactional operations used by the sync engine (ADR-0003
//! client algorithm). Each method runs inside the caller's [`Tx`], so a
//! whole step (e.g. "update object + enqueue outbox", "apply page + advance
//! cursor") commits or rolls back as one unit.
//!
//! Invariants maintained here:
//! * an object is `Synced` iff it has no outbox entries;
//! * `remote_objects` only holds rows for objects that have outbox entries;
//! * an outbox entry that may have reached the server (attempted, outcome
//!   unknown) is never modified — later edits are chained after it — so a
//!   retry with the same `mutation_id` always carries the original body.

use crate::db::Tx;
use crate::error::{Result, StorageError};
use crate::model::{
    LocalState, OutboxEntry, OutboxOp, OutboxState, ProfileKind, RemoteObject, StoredObject,
    SyncCursor,
};
use crate::sql::now;
use cc_models::KekClass;
use cc_protocol::sync::{EncryptedBody, MutationResult};
use cc_protocol::{DeviceId, MutationId, ObjectId, Timestamp, VaultId};

/// Outcome of recording a local mutation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LocalMutationOutcome {
    /// A mutation is queued (new entry, or an unsent entry was rewritten).
    Queued {
        mutation_id: MutationId,
        /// Revision the change is based on (0 = create).
        base_revision: i64,
        /// Revision the ciphertext is bound to (`base_revision + 1`).
        revision: i64,
    },
    /// A create that never reached the server was deleted: the object and
    /// its outbox entry (if any) were removed locally, nothing will be pushed.
    DroppedUnsent,
    /// Local profile (ADR-0106): stored with a new local revision, nothing
    /// queued.
    LocalOnly {
        /// Local revision the ciphertext is bound to.
        revision: i64,
        deleted: bool,
    },
    /// Nothing to do (delete of a missing or already deleted object).
    NoOp,
}

/// Where the next local mutation of an object goes.
enum Slot {
    /// Append a new entry based on `base`.
    New { base: i64 },
    /// Rewrite `entry` in place (the server provably has not applied it).
    Rewrite {
        entry: EntryRef,
        new_mutation_id: bool,
        state: OutboxState,
        reset_attempts: bool,
    },
}

/// The parts of an outbox entry a rewrite needs.
struct EntryRef {
    ordering: i64,
    mutation_id: MutationId,
    base_revision: i64,
}

impl From<&OutboxEntry> for EntryRef {
    fn from(e: &OutboxEntry) -> Self {
        Self {
            ordering: e.ordering,
            mutation_id: e.mutation_id,
            base_revision: e.base_revision,
        }
    }
}

impl Slot {
    fn base(&self) -> i64 {
        match self {
            Slot::New { base } => *base,
            Slot::Rewrite { entry, .. } => entry.base_revision,
        }
    }

    fn resulting_state(&self) -> LocalState {
        match self {
            Slot::Rewrite {
                state: OutboxState::Conflict,
                ..
            } => LocalState::Conflict,
            _ => LocalState::Pending,
        }
    }
}

/// A validated server change (from `changes` or `snapshot`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteChange {
    pub object_id: ObjectId,
    pub revision: i64,
    pub sequence: i64,
    pub deleted: bool,
    /// `None` iff `deleted`.
    pub body: Option<EncryptedBody>,
    /// KEK class found when the sync engine verified the ciphertext.
    pub kek_class_hint: Option<KekClass>,
    pub writer_device_id: DeviceId,
    pub updated_at: Timestamp,
}

/// Which kind of page is being applied and how the cursor advances.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PageCursor {
    /// `changes` page: `last_sequence := next_after`.
    Changes {
        next_after: i64,
        latest_sequence: i64,
    },
    /// `snapshot` page: `next_cursor = None` marks the last page.
    Snapshot {
        next_cursor: Option<i64>,
        latest_sequence: i64,
    },
}

/// What happened to one remote change.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RemoteApplyOutcome {
    /// Written to `objects` (the object had no local changes).
    Applied {
        object_id: ObjectId,
        revision: i64,
        deleted: bool,
    },
    /// Kept aside because the object has unpushed local changes.
    Stashed { object_id: ObjectId, revision: i64 },
    /// Already known (e.g. echo of our own write) or older than local state.
    Skipped { object_id: ObjectId },
}

/// Result of applying a page.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PageOutcome {
    pub changes: Vec<RemoteApplyOutcome>,
    /// Set when a snapshot's last page was applied.
    pub snapshot_completed: bool,
    /// Objects removed because a completed re-snapshot no longer lists them.
    pub removed: Vec<ObjectId>,
    /// Cursor after the page.
    pub cursor: Option<SyncCursor>,
}

/// What happened to one push result.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PushOutcome {
    /// Mutation committed server-side.
    Committed {
        object_id: ObjectId,
        mutation_id: MutationId,
        revision: i64,
        sequence: i64,
        /// No further local changes of this object are pending.
        synced: bool,
        /// A newer stashed server state was applied on top (object changed).
        applied_remote: Option<(i64, bool)>,
    },
    /// Server reported a revision conflict; entry moved to `Conflict`.
    Conflicted {
        object_id: ObjectId,
        mutation_id: MutationId,
        current_revision: i64,
        current_deleted: bool,
    },
    /// Result for a mutation that is no longer in the outbox (ignored).
    Unknown { mutation_id: MutationId },
}

/// Everything needed to resolve one conflicted object.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConflictContext {
    /// Latest local state (ciphertext bound to `local.revision`).
    pub local: StoredObject,
    /// Latest known server state, if pulled.
    pub remote: Option<RemoteObject>,
    /// Outbox entries of the object in FIFO order.
    pub entries: Vec<OutboxEntry>,
    /// Server-reported current revision from the conflicting push result.
    pub conflict_revision: i64,
    /// Server-reported tombstone flag from the conflicting push result.
    pub conflict_deleted: bool,
}

impl ConflictContext {
    /// Whether the local side is a deletion.
    pub fn local_is_delete(&self) -> bool {
        self.local.deleted
    }
}

/// A conflict copy: the local version saved under a new object id.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConflictCopy {
    pub object_id: ObjectId,
    /// Ciphertext for revision 1 of the new object.
    pub body: EncryptedBody,
    pub kek_class_hint: Option<KekClass>,
}

/// Decision for a conflicted object.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Resolution {
    /// Keep the server state in place, drop local entries; optionally save
    /// the local version as a new object.
    AcceptRemote { copy: Option<ConflictCopy> },
    /// Re-apply the local version on top of server revision `base_revision`
    /// (`body` is encrypted for `base_revision + 1`).
    Rebase {
        base_revision: i64,
        body: EncryptedBody,
        kek_class_hint: Option<KekClass>,
    },
    /// The server does not know the object and the local side is a
    /// deletion: forget the object locally.
    DropLocal,
}

/// Result of [`Tx::apply_resolution`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolutionOutcome {
    pub object_id: ObjectId,
    /// Revision/tombstone now stored for the original object.
    pub revision: i64,
    pub deleted: bool,
    /// New mutation queued for the original object (rebase).
    pub requeued: Option<MutationId>,
    /// Conflict copy created (object id, its queued mutation).
    pub copy: Option<(ObjectId, MutationId)>,
}

impl Tx<'_> {
    fn plan_slot(
        &self,
        vault_id: VaultId,
        object_id: ObjectId,
    ) -> Result<(Option<StoredObject>, Vec<OutboxEntry>, Slot)> {
        let obj = self.get_object(vault_id, object_id)?;
        let entries = self.outbox_for_object(vault_id, object_id)?;
        let slot = match entries.last() {
            None => Slot::New {
                base: obj.as_ref().map_or(0, |o| o.server_revision),
            },
            Some(e) => match e.state {
                OutboxState::Queued if e.attempts == 0 => Slot::Rewrite {
                    entry: e.into(),
                    new_mutation_id: false,
                    state: OutboxState::Queued,
                    reset_attempts: false,
                },
                // In flight or outcome unknown: chain after it.
                OutboxState::Queued => Slot::New {
                    base: e.base_revision + 1,
                },
                // Conflicted mutations are not recorded server-side.
                OutboxState::Conflict => Slot::Rewrite {
                    entry: e.into(),
                    new_mutation_id: true,
                    state: OutboxState::Conflict,
                    reset_attempts: true,
                },
                // Rejected as a whole request, never applied.
                OutboxState::Failed => Slot::Rewrite {
                    entry: e.into(),
                    new_mutation_id: true,
                    state: OutboxState::Queued,
                    reset_attempts: true,
                },
            },
        };
        Ok((obj, entries, slot))
    }

    fn write_slot(
        &self,
        vault_id: VaultId,
        object_id: ObjectId,
        slot: Slot,
        op: OutboxOp,
        body: Option<&EncryptedBody>,
    ) -> Result<MutationId> {
        match slot {
            Slot::New { base } => {
                let e =
                    self.outbox_insert(MutationId::new(), vault_id, object_id, base, op, body)?;
                Ok(e.mutation_id)
            }
            Slot::Rewrite {
                entry,
                new_mutation_id,
                state,
                reset_attempts,
            } => {
                let id = if new_mutation_id {
                    MutationId::new()
                } else {
                    entry.mutation_id
                };
                self.outbox_rewrite(
                    entry.ordering,
                    id,
                    entry.base_revision,
                    op,
                    body,
                    state,
                    reset_attempts,
                )?;
                Ok(id)
            }
        }
    }

    /// Local create/update: encrypts via `encrypt(revision)` for the right
    /// revision, stores the ciphertext in `objects` and queues it — in this
    /// transaction. In a `Local` profile nothing is queued: the object gets
    /// the next local revision and `LocalOnly` state.
    pub fn record_local_put<E, F>(
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
        if self.profile_kind()? == ProfileKind::Local {
            return self.record_local_only_put(vault_id, object_id, kek_class_hint, encrypt);
        }
        let (obj, _entries, slot) = self.plan_slot(vault_id, object_id)?;
        let base = slot.base();
        let revision = base + 1;
        let body = encrypt(revision)?;
        let state = slot.resulting_state();
        let row = StoredObject {
            vault_id,
            object_id,
            revision,
            server_revision: obj.as_ref().map_or(0, |o| o.server_revision),
            sequence: obj.as_ref().and_then(|o| o.sequence),
            deleted: false,
            body: Some(body),
            kek_class_hint: kek_class_hint.or(obj.as_ref().and_then(|o| o.kek_class_hint)),
            local_state: state,
            conflict_origin: obj.as_ref().and_then(|o| o.conflict_origin),
            writer_device_id: obj.as_ref().and_then(|o| o.writer_device_id),
            updated_at: now(),
        };
        self.upsert_object(&row)?;
        let mutation_id =
            self.write_slot(vault_id, object_id, slot, OutboxOp::Put, row.body.as_ref())?;
        Ok(LocalMutationOutcome::Queued {
            mutation_id,
            base_revision: base,
            revision,
        })
    }

    /// Local delete: tombstones the object locally and queues the deletion
    /// (`Local` profiles: local tombstone only, or removal if the object
    /// never reached a server).
    pub fn record_local_delete(
        &self,
        vault_id: VaultId,
        object_id: ObjectId,
    ) -> Result<LocalMutationOutcome> {
        if self.profile_kind()? == ProfileKind::Local {
            return self.record_local_only_delete(vault_id, object_id);
        }
        let (obj, entries, slot) = self.plan_slot(vault_id, object_id)?;
        let Some(obj) = obj else {
            return Ok(LocalMutationOutcome::NoOp);
        };
        if obj.deleted
            && (entries.is_empty() || entries.last().is_some_and(|e| e.op == OutboxOp::Delete))
        {
            return Ok(LocalMutationOutcome::NoOp);
        }
        if let [only] = entries.as_slice() {
            let unsent = match only.state {
                OutboxState::Queued => only.attempts == 0,
                OutboxState::Failed => true,
                OutboxState::Conflict => false,
            };
            if unsent && only.base_revision == 0 {
                self.outbox_delete(only.mutation_id)?;
                self.delete_remote_object(vault_id, object_id)?;
                self.delete_object_row(vault_id, object_id)?;
                return Ok(LocalMutationOutcome::DroppedUnsent);
            }
        }
        let base = slot.base();
        let revision = base + 1;
        let row = StoredObject {
            revision,
            deleted: true,
            body: None,
            local_state: slot.resulting_state(),
            updated_at: now(),
            ..obj
        };
        self.upsert_object(&row)?;
        let mutation_id = self.write_slot(vault_id, object_id, slot, OutboxOp::Delete, None)?;
        Ok(LocalMutationOutcome::Queued {
            mutation_id,
            base_revision: base,
            revision,
        })
    }

    /// Apply the per-mutation results of one push (200 or 409) atomically.
    ///
    /// Fails without changing anything if a result is inconsistent with the
    /// outbox (e.g. an accepted revision other than `base_revision + 1`) —
    /// a malformed or hostile response must not corrupt local state.
    pub fn apply_push_results(
        &self,
        vault_id: VaultId,
        latest_sequence: i64,
        results: &[MutationResult],
    ) -> Result<Vec<PushOutcome>> {
        let mut out = Vec::with_capacity(results.len());
        let own_device = self.get_profile()?.map(|p| p.device_id);
        for r in results {
            let Some(entry) = self.outbox_get(r.mutation_id())? else {
                out.push(PushOutcome::Unknown {
                    mutation_id: r.mutation_id(),
                });
                continue;
            };
            if entry.vault_id != vault_id {
                return Err(StorageError::Invalid(
                    "push result for another vault".into(),
                ));
            }
            match *r {
                MutationResult::Accepted {
                    object_id,
                    revision,
                    sequence,
                    ..
                } => {
                    if object_id != entry.object_id
                        || revision != entry.base_revision + 1
                        || sequence < 1
                        || sequence > latest_sequence
                    {
                        return Err(StorageError::Invalid(
                            "accepted push result is inconsistent with the queued mutation".into(),
                        ));
                    }
                    out.push(self.commit_accepted(&entry, revision, sequence, own_device)?);
                }
                MutationResult::Conflict {
                    mutation_id,
                    object_id,
                    current_revision,
                    current_sequence,
                    current_deleted,
                } => {
                    if object_id != entry.object_id
                        || current_revision < 0
                        || current_sequence < 0
                        || current_sequence > latest_sequence
                        || current_revision == entry.base_revision
                    {
                        return Err(StorageError::Invalid(
                            "conflict push result is inconsistent with the queued mutation".into(),
                        ));
                    }
                    self.outbox_mark_conflict(
                        mutation_id,
                        current_revision,
                        current_sequence,
                        current_deleted,
                    )?;
                    self.set_object_state(vault_id, object_id, LocalState::Conflict)?;
                    out.push(PushOutcome::Conflicted {
                        object_id,
                        mutation_id,
                        current_revision,
                        current_deleted,
                    });
                }
            }
        }
        self.note_server_sequence(vault_id, latest_sequence)?;
        Ok(out)
    }

    fn commit_accepted(
        &self,
        entry: &OutboxEntry,
        revision: i64,
        sequence: i64,
        own_device: Option<DeviceId>,
    ) -> Result<PushOutcome> {
        let (vault_id, object_id) = (entry.vault_id, entry.object_id);
        self.outbox_delete(entry.mutation_id)?;
        let Some(mut obj) = self.get_object(vault_id, object_id)? else {
            return Ok(PushOutcome::Unknown {
                mutation_id: entry.mutation_id,
            });
        };
        if revision > obj.server_revision {
            obj.server_revision = revision;
            obj.sequence = Some(sequence);
            // Who wrote `server_revision` — lets a rollback recovery tell our
            // write from another device's write at the same revision.
            obj.writer_device_id = own_device.or(obj.writer_device_id);
        }
        let remaining = self.outbox_has_object(vault_id, object_id)?;
        let mut applied_remote = None;
        if !remaining {
            match self.get_remote_object(vault_id, object_id)? {
                Some(remote) if remote.revision > revision => {
                    // Someone wrote on top of our change meanwhile: take it.
                    obj = stored_from_remote(&remote, obj.conflict_origin);
                    applied_remote = Some((remote.revision, remote.deleted));
                }
                _ => {
                    obj.local_state = LocalState::Synced;
                }
            }
            self.delete_remote_object(vault_id, object_id)?;
        }
        self.upsert_object(&obj)?;
        Ok(PushOutcome::Committed {
            object_id,
            mutation_id: entry.mutation_id,
            revision,
            sequence,
            synced: !remaining,
            applied_remote,
        })
    }

    /// Apply one validated page of server changes and advance the cursor, in
    /// this transaction.
    pub fn apply_remote_page(
        &self,
        vault_id: VaultId,
        changes: &[RemoteChange],
        page: PageCursor,
    ) -> Result<PageOutcome> {
        let mut out = PageOutcome::default();
        let snapshot = matches!(page, PageCursor::Snapshot { .. });
        for c in changes {
            if (c.body.is_some() == c.deleted) || c.revision < 1 {
                return Err(StorageError::Invalid("inconsistent remote change".into()));
            }
            out.changes.push(self.apply_remote_change(vault_id, c)?);
            if snapshot {
                self.mark_snapshot_seen(vault_id, c.object_id)?;
            }
        }
        let mut cursor = self.get_sync_cursor(vault_id)?;
        match page {
            PageCursor::Changes {
                next_after,
                latest_sequence,
            } => {
                cursor.last_sequence = cursor.last_sequence.max(next_after);
                cursor.server_latest_sequence = cursor.server_latest_sequence.max(latest_sequence);
            }
            PageCursor::Snapshot {
                next_cursor,
                latest_sequence,
            } => {
                let start = *cursor
                    .snapshot_start_sequence
                    .get_or_insert(latest_sequence);
                cursor.server_latest_sequence = cursor.server_latest_sequence.max(latest_sequence);
                match next_cursor {
                    Some(n) => cursor.snapshot_cursor = Some(n),
                    None => {
                        for object_id in self.unseen_synced_objects(vault_id, start)? {
                            self.delete_object_row(vault_id, object_id)?;
                            out.removed.push(object_id);
                        }
                        self.clear_snapshot_seen(vault_id)?;
                        cursor.snapshot_complete = true;
                        cursor.snapshot_cursor = None;
                        cursor.snapshot_start_sequence = None;
                        cursor.last_sequence = cursor.last_sequence.max(start);
                        out.snapshot_completed = true;
                    }
                }
            }
        }
        self.put_sync_cursor(&cursor)?;
        out.cursor = Some(cursor);
        Ok(out)
    }

    fn apply_remote_change(
        &self,
        vault_id: VaultId,
        c: &RemoteChange,
    ) -> Result<RemoteApplyOutcome> {
        let obj = self.get_object(vault_id, c.object_id)?;
        let known_revision = obj.as_ref().map_or(0, |o| o.server_revision);
        if self.outbox_has_object(vault_id, c.object_id)? {
            let stash_rev = self
                .get_remote_object(vault_id, c.object_id)?
                .map_or(0, |r| r.revision);
            if c.revision > known_revision && c.revision > stash_rev {
                self.upsert_remote_object(&RemoteObject {
                    vault_id,
                    object_id: c.object_id,
                    revision: c.revision,
                    sequence: c.sequence,
                    deleted: c.deleted,
                    body: c.body.clone(),
                    kek_class_hint: c.kek_class_hint,
                    writer_device_id: Some(c.writer_device_id),
                    updated_at: c.updated_at,
                })?;
                return Ok(RemoteApplyOutcome::Stashed {
                    object_id: c.object_id,
                    revision: c.revision,
                });
            }
            return Ok(RemoteApplyOutcome::Skipped {
                object_id: c.object_id,
            });
        }
        if obj.is_some() && c.revision <= known_revision {
            return Ok(RemoteApplyOutcome::Skipped {
                object_id: c.object_id,
            });
        }
        let row = StoredObject {
            vault_id,
            object_id: c.object_id,
            revision: c.revision,
            server_revision: c.revision,
            sequence: Some(c.sequence),
            deleted: c.deleted,
            body: c.body.clone(),
            kek_class_hint: c
                .kek_class_hint
                .or(obj.as_ref().and_then(|o| o.kek_class_hint)),
            local_state: LocalState::Synced,
            conflict_origin: obj.as_ref().and_then(|o| o.conflict_origin),
            writer_device_id: Some(c.writer_device_id),
            updated_at: c.updated_at,
        };
        self.upsert_object(&row)?;
        Ok(RemoteApplyOutcome::Applied {
            object_id: c.object_id,
            revision: c.revision,
            deleted: c.deleted,
        })
    }

    /// Stashed server state, or — when none was pulled — a tombstone
    /// synthesized from a conflict result that reported the object deleted
    /// (a snapshot never lists tombstones, so after a re-snapshot the
    /// tombstone may be below the `changes` cursor).
    fn effective_remote(
        &self,
        vault_id: VaultId,
        object_id: ObjectId,
        entries: &[OutboxEntry],
    ) -> Result<Option<RemoteObject>> {
        if let Some(r) = self.get_remote_object(vault_id, object_id)? {
            return Ok(Some(r));
        }
        let synthesized = entries
            .iter()
            .find(|e| e.state == OutboxState::Conflict)
            .and_then(|e| match (e.conflict_deleted, e.conflict_revision) {
                (Some(true), Some(rev)) if rev > 0 => Some(RemoteObject {
                    vault_id,
                    object_id,
                    revision: rev,
                    sequence: e.conflict_sequence.unwrap_or(0),
                    deleted: true,
                    body: None,
                    kek_class_hint: None,
                    writer_device_id: None,
                    updated_at: e.updated_at,
                }),
                _ => None,
            });
        Ok(synthesized)
    }

    /// Objects of a vault that have an entry in `Conflict` state.
    pub fn conflicted_objects(&self, vault_id: VaultId) -> Result<Vec<ObjectId>> {
        let mut ids: Vec<ObjectId> = self
            .outbox_list(vault_id)?
            .into_iter()
            .filter(|e| e.state == OutboxState::Conflict)
            .map(|e| e.object_id)
            .collect();
        ids.dedup();
        Ok(ids)
    }

    /// Load everything needed to resolve a conflicted object (`None` if the
    /// object is not in conflict).
    pub fn conflict_context(
        &self,
        vault_id: VaultId,
        object_id: ObjectId,
    ) -> Result<Option<ConflictContext>> {
        let entries = self.outbox_for_object(vault_id, object_id)?;
        let Some(conflicted) = entries.iter().find(|e| e.state == OutboxState::Conflict) else {
            return Ok(None);
        };
        let conflict_revision = conflicted.conflict_revision.unwrap_or(0);
        let conflict_deleted = conflicted.conflict_deleted.unwrap_or(false);
        let Some(local) = self.get_object(vault_id, object_id)? else {
            return Err(StorageError::Invalid(
                "conflicted object has no local row".into(),
            ));
        };
        let remote = self.effective_remote(vault_id, object_id, &entries)?;
        Ok(Some(ConflictContext {
            local,
            remote,
            entries,
            conflict_revision,
            conflict_deleted,
        }))
    }

    /// Apply a conflict decision atomically: replaces the object's outbox
    /// entries, updates `objects`, drops the stashed server state and (for
    /// conflict copies) inserts + queues the copy.
    pub fn apply_resolution(
        &self,
        vault_id: VaultId,
        object_id: ObjectId,
        resolution: Resolution,
    ) -> Result<ResolutionOutcome> {
        let Some(local) = self.get_object(vault_id, object_id)? else {
            return Err(StorageError::Invalid(
                "conflicted object has no local row".into(),
            ));
        };
        let entries = self.outbox_for_object(vault_id, object_id)?;
        let remote = self.effective_remote(vault_id, object_id, &entries)?;
        self.outbox_delete_object(vault_id, object_id)?;
        self.delete_remote_object(vault_id, object_id)?;
        match resolution {
            Resolution::DropLocal => {
                self.delete_object_row(vault_id, object_id)?;
                Ok(ResolutionOutcome {
                    object_id,
                    revision: local.revision,
                    deleted: true,
                    requeued: None,
                    copy: None,
                })
            }
            Resolution::AcceptRemote { copy } => {
                let Some(remote) = remote else {
                    return Err(StorageError::Invalid(
                        "cannot accept remote state that was never pulled".into(),
                    ));
                };
                let row = stored_from_remote(&remote, local.conflict_origin);
                self.upsert_object(&row)?;
                let copy = match copy {
                    Some(c) => {
                        if self.get_object(vault_id, c.object_id)?.is_some() {
                            return Err(StorageError::Invalid(
                                "conflict copy id already exists".into(),
                            ));
                        }
                        self.upsert_object(&StoredObject {
                            vault_id,
                            object_id: c.object_id,
                            revision: 1,
                            server_revision: 0,
                            sequence: None,
                            deleted: false,
                            body: Some(c.body.clone()),
                            kek_class_hint: c.kek_class_hint,
                            local_state: LocalState::Pending,
                            conflict_origin: Some(object_id),
                            writer_device_id: None,
                            updated_at: now(),
                        })?;
                        let e = self.outbox_insert(
                            MutationId::new(),
                            vault_id,
                            c.object_id,
                            0,
                            OutboxOp::Put,
                            Some(&c.body),
                        )?;
                        Some((c.object_id, e.mutation_id))
                    }
                    None => None,
                };
                Ok(ResolutionOutcome {
                    object_id,
                    revision: row.revision,
                    deleted: row.deleted,
                    requeued: None,
                    copy,
                })
            }
            Resolution::Rebase {
                base_revision,
                body,
                kek_class_hint,
            } => {
                if let Some(r) = &remote {
                    if r.revision != base_revision {
                        return Err(StorageError::Invalid(
                            "rebase base does not match the pulled server state".into(),
                        ));
                    }
                }
                let row = StoredObject {
                    vault_id,
                    object_id,
                    revision: base_revision + 1,
                    server_revision: base_revision,
                    sequence: remote.as_ref().map(|r| r.sequence).or(local.sequence),
                    deleted: false,
                    body: Some(body),
                    kek_class_hint: kek_class_hint.or(local.kek_class_hint),
                    local_state: LocalState::Pending,
                    conflict_origin: local.conflict_origin,
                    writer_device_id: remote
                        .as_ref()
                        .and_then(|r| r.writer_device_id)
                        .or(local.writer_device_id),
                    updated_at: now(),
                };
                self.upsert_object(&row)?;
                let e = self.outbox_insert(
                    MutationId::new(),
                    vault_id,
                    object_id,
                    base_revision,
                    OutboxOp::Put,
                    row.body.as_ref(),
                )?;
                Ok(ResolutionOutcome {
                    object_id,
                    revision: row.revision,
                    deleted: false,
                    requeued: Some(e.mutation_id),
                    copy: None,
                })
            }
        }
    }
}

pub(crate) fn stored_from_remote(
    remote: &RemoteObject,
    conflict_origin: Option<ObjectId>,
) -> StoredObject {
    StoredObject {
        vault_id: remote.vault_id,
        object_id: remote.object_id,
        revision: remote.revision,
        server_revision: remote.revision,
        sequence: Some(remote.sequence),
        deleted: remote.deleted,
        body: remote.body.clone(),
        kek_class_hint: remote.kek_class_hint,
        local_state: LocalState::Synced,
        conflict_origin,
        writer_device_id: remote.writer_device_id,
        updated_at: remote.updated_at,
    }
}
