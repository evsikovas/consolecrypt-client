//! Server rollback / restore recovery (ADR-0103 addendum "Server rollback
//! recovery", protocol 1.3 vault epoch).
//!
//! A server restored from a backup has forgotten mutations it once
//! confirmed. The client notices (epoch change, `latest_sequence` below its
//! cursor, or a push accepted with an already-used sequence) and runs:
//!
//! 1. [`Tx::begin_rollback_recovery`] — persist "recovery pending";
//! 2. a full listing `changes?after=0` (latest state of every object,
//!    tombstones included — ADR-0202 never compacts them), one page per
//!    [`Tx::record_recovery_page`] into `recovery_remote`;
//! 3. [`Tx::reconcile_rollback`] — one transaction that compares every
//!    local object with the listed server state: server-newer states are
//!    applied (or stashed for pending edits), objects the server lacks or
//!    holds at an older revision are re-queued from the local copy
//!    (re-encrypted for the server's revision + 1; creates on base 0), local
//!    tombstones are re-applied, same-revision forks (different body, or a
//!    pending edit whose base revision another device rewrote) go to normal
//!    conflict resolution; then the cursor jumps to the end of the listing
//!    and the new epoch is stored.
//!
//! Every step commits atomically with its progress, so a crash resumes
//! where it stopped; local data is never dropped.

use crate::db::Tx;
use crate::error::{Result, StorageError};
use crate::model::{
    LocalState, OutboxOp, OutboxState, RemoteObject, RollbackReason, RollbackRecovery,
    StoredObject, SyncCursor,
};
use crate::repo::objects::{remote_from_row_in, REMOTE_COLS};
use crate::sql::{body_cols, enum_str, now, ts};
use crate::sync_ops::{stored_from_remote, RemoteChange};
use cc_protocol::sync::{EncryptedBody, MutationResult};
use cc_protocol::{MutationId, ObjectId, VaultId};
use rusqlite::params;
use std::collections::{BTreeSet, HashMap};
use uuid::Uuid;

/// Paging information of one recovery listing page.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RecoveryPage {
    /// `next_after` of the page.
    pub next_after: i64,
    pub has_more: bool,
    pub latest_sequence: i64,
    /// Epoch reported by the page.
    pub epoch: Option<Uuid>,
}

/// What [`Tx::reconcile_rollback`] did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReconcileOutcome {
    pub reason: RollbackReason,
    /// Server states written to `objects` (object, revision, deleted).
    pub applied: Vec<(ObjectId, i64, bool)>,
    /// Live local objects re-queued because the server lacks them or holds
    /// an older revision (object, new local revision).
    pub repushed: Vec<(ObjectId, i64)>,
    /// Local tombstones re-queued (object, new local revision).
    pub tombstones_reapplied: Vec<(ObjectId, i64)>,
    /// Same-revision forks handed to conflict resolution.
    pub conflicts: Vec<ObjectId>,
    /// Objects the server lacks that could not be re-encrypted (local
    /// ciphertext unreadable). Kept locally as they are, not pushed.
    pub unrecoverable: Vec<ObjectId>,
    /// Cursor after the recovery.
    pub cursor: SyncCursor,
}

/// Whether local and server state are the same write.
fn same_state(l: &StoredObject, s: &RemoteObject) -> bool {
    l.deleted == s.deleted && (l.deleted || l.body == s.body)
}

/// Codec access needed by [`Tx::reconcile_rollback`] (called inside the
/// transaction; must not block on the async runtime). An `Err` (e.g. vault
/// locked) aborts the reconcile, which can simply be retried later.
pub trait ReconcileCodec<E> {
    /// `local`'s current payload encrypted for `revision` (fresh DEK), or
    /// `Ok(None)` if the local ciphertext cannot be read.
    fn reencrypt(
        &mut self,
        local: &StoredObject,
        revision: i64,
    ) -> Result<Option<EncryptedBody>, E>;
    /// Whether two live versions (bound to different revisions or written
    /// by different devices) carry the same payload; `Ok(false)` if either
    /// cannot be read.
    fn same_payload(&mut self, local: &StoredObject, server: &RemoteObject) -> Result<bool, E>;
}

impl Tx<'_> {
    // ---- detection ---------------------------------------------------------------------

    /// Store the server's vault epoch if none is known yet. A different
    /// known epoch is an error: callers check [`SyncCursor::rollback_evidence`]
    /// first and start a recovery instead.
    pub fn note_server_epoch(&self, vault_id: VaultId, epoch: Option<Uuid>) -> Result<()> {
        let Some(epoch) = epoch else { return Ok(()) };
        let mut c = self.get_sync_cursor(vault_id)?;
        match c.epoch {
            None => {
                c.epoch = Some(epoch);
                self.put_sync_cursor(&c)
            }
            Some(known) if known == epoch => Ok(()),
            Some(_) => Err(StorageError::Invalid("vault epoch changed".into())),
        }
    }

    /// Evidence of a server rollback in a push response (before the results
    /// are applied): `latest_sequence` below the cursor, or a fresh
    /// (non-replayed) acceptance with a sequence ≤ one this client already
    /// holds for another object or has pulled past.
    pub fn push_rollback_evidence(
        &self,
        vault_id: VaultId,
        latest_sequence: i64,
        results: &[MutationResult],
    ) -> Result<Option<RollbackReason>> {
        let c = self.get_sync_cursor(vault_id)?;
        if c.rollback_evidence(None, latest_sequence).is_some() {
            return Ok(Some(RollbackReason::SequenceRegressed));
        }
        for r in results {
            if let MutationResult::Accepted {
                object_id,
                sequence,
                replayed: false,
                ..
            } = *r
            {
                let held = c
                    .last_sequence
                    .max(self.max_sequence_except(vault_id, object_id)?);
                if sequence <= held {
                    return Ok(Some(RollbackReason::SequenceReused));
                }
            }
        }
        Ok(None)
    }

    /// Highest server sequence stored for any object of the vault other than
    /// `object_id` (objects and stashed server states).
    fn max_sequence_except(&self, vault_id: VaultId, object_id: ObjectId) -> Result<i64> {
        let n: Option<i64> = self.c().query_row(
            "SELECT MAX(s) FROM (\
               SELECT sequence AS s FROM objects WHERE vault_id = ?1 AND object_id <> ?2 \
               UNION ALL \
               SELECT sequence AS s FROM remote_objects WHERE vault_id = ?1 AND object_id <> ?2)",
            params![vault_id.to_string(), object_id.to_string()],
            |r| r.get(0),
        )?;
        Ok(n.unwrap_or(0))
    }

    /// Mark a rollback recovery pending (idempotent). Returns `true` if a new
    /// recovery started, `false` if one was already in progress. A pending
    /// recovery whose listing ran against another epoch restarts its listing.
    pub fn begin_rollback_recovery(
        &self,
        vault_id: VaultId,
        reason: RollbackReason,
        epoch: Option<Uuid>,
    ) -> Result<bool> {
        let mut c = self.get_sync_cursor(vault_id)?;
        match &c.recovery {
            None => {
                c.recovery = Some(RollbackRecovery {
                    reason,
                    detected_at: now(),
                    epoch,
                    cursor: 0,
                    listed: false,
                    latest_sequence: 0,
                });
                self.clear_recovery_remote(vault_id)?;
                self.put_sync_cursor(&c)?;
                Ok(true)
            }
            Some(rec) => {
                let moved = match (rec.epoch, epoch) {
                    (Some(a), Some(b)) => a != b,
                    // Pages listed so far may predate the new epoch.
                    (None, Some(_)) => rec.cursor > 0 || rec.listed,
                    _ => false,
                };
                if moved {
                    self.restart_recovery_listing(vault_id, epoch)?;
                }
                Ok(false)
            }
        }
    }

    /// Restart the recovery listing from scratch (the server changed its
    /// epoch again or its sequence went below the listing cursor).
    pub fn restart_recovery_listing(&self, vault_id: VaultId, epoch: Option<Uuid>) -> Result<()> {
        let mut c = self.get_sync_cursor(vault_id)?;
        let Some(rec) = c.recovery.as_mut() else {
            return Err(StorageError::Invalid(
                "no rollback recovery in progress".into(),
            ));
        };
        rec.cursor = 0;
        rec.listed = false;
        rec.latest_sequence = 0;
        if epoch.is_some() {
            rec.epoch = epoch;
        }
        self.clear_recovery_remote(vault_id)?;
        self.put_sync_cursor(&c)
    }

    // ---- listing -----------------------------------------------------------------------

    /// Store one validated page of the recovery listing and advance its
    /// cursor, in this transaction.
    pub fn record_recovery_page(
        &self,
        vault_id: VaultId,
        changes: &[RemoteChange],
        page: RecoveryPage,
    ) -> Result<RollbackRecovery> {
        let mut c = self.get_sync_cursor(vault_id)?;
        let Some(rec) = c.recovery.as_mut() else {
            return Err(StorageError::Invalid(
                "no rollback recovery in progress".into(),
            ));
        };
        if rec.listed {
            return Err(StorageError::Invalid(
                "recovery listing already complete".into(),
            ));
        }
        if let (Some(a), Some(b)) = (rec.epoch, page.epoch) {
            if a != b {
                return Err(StorageError::Invalid(
                    "epoch changed during recovery".into(),
                ));
            }
        }
        if page.next_after < rec.cursor {
            return Err(StorageError::Invalid(
                "recovery cursor went backwards".into(),
            ));
        }
        for ch in changes {
            if (ch.body.is_some() == ch.deleted) || ch.revision < 1 || ch.sequence <= rec.cursor {
                return Err(StorageError::Invalid("inconsistent remote change".into()));
            }
            let newer = self
                .get_recovery_remote(vault_id, ch.object_id)?
                .is_none_or(|r| ch.sequence > r.sequence);
            if newer {
                self.upsert_recovery_remote(&RemoteObject {
                    vault_id,
                    object_id: ch.object_id,
                    revision: ch.revision,
                    sequence: ch.sequence,
                    deleted: ch.deleted,
                    body: ch.body.clone(),
                    kek_class_hint: ch.kek_class_hint,
                    writer_device_id: Some(ch.writer_device_id),
                    updated_at: ch.updated_at,
                })?;
            }
        }
        if rec.epoch.is_none() {
            rec.epoch = page.epoch;
        }
        rec.cursor = page.next_after;
        rec.latest_sequence = rec.latest_sequence.max(page.latest_sequence);
        rec.listed = !page.has_more;
        let out = rec.clone();
        self.put_sync_cursor(&c)?;
        Ok(out)
    }

    // ---- reconcile ---------------------------------------------------------------------

    /// Reconcile local state with the completed recovery listing, atomically
    /// (see the module docs for the rules). Objects whose local ciphertext
    /// cannot be re-encrypted are kept locally, untouched, and reported as
    /// unrecoverable. An `Err` from `codec` (e.g. vault locked) rolls
    /// everything back; the listing stays stored and the reconcile can
    /// simply be retried.
    pub fn reconcile_rollback<E, C>(
        &self,
        vault_id: VaultId,
        codec: &mut C,
    ) -> Result<ReconcileOutcome, E>
    where
        E: From<StorageError>,
        C: ReconcileCodec<E>,
    {
        let mut cursor = self.get_sync_cursor(vault_id)?;
        let rec = match &cursor.recovery {
            Some(r) if r.listed => r.clone(),
            Some(_) => {
                return Err(StorageError::Invalid("recovery listing incomplete".into()).into())
            }
            None => {
                return Err(StorageError::Invalid("no rollback recovery in progress".into()).into())
            }
        };
        let server: HashMap<ObjectId, RemoteObject> = self
            .list_recovery_remote(vault_id)?
            .into_iter()
            .map(|r| (r.object_id, r))
            .collect();
        let locals: HashMap<ObjectId, StoredObject> = self
            .list_objects(vault_id, true)?
            .into_iter()
            .map(|o| (o.object_id, o))
            .collect();
        // Stashed server states are from the old timeline.
        self.c()
            .execute(
                "DELETE FROM remote_objects WHERE vault_id = ?1",
                params![vault_id.to_string()],
            )
            .map_err(StorageError::from)?;

        let mut out = ReconcileOutcome {
            reason: rec.reason,
            applied: Vec::new(),
            repushed: Vec::new(),
            tombstones_reapplied: Vec::new(),
            conflicts: Vec::new(),
            unrecoverable: Vec::new(),
            cursor: cursor.clone(),
        };
        let ids: BTreeSet<ObjectId> = locals.keys().chain(server.keys()).copied().collect();
        for id in ids {
            let s = server.get(&id);
            let Some(l) = locals.get(&id) else {
                if let Some(s) = s {
                    self.upsert_object(&stored_from_remote(s, None))?;
                    out.applied.push((id, s.revision, s.deleted));
                }
                continue;
            };
            // Conflict results came from the old timeline: push again and
            // let the restored server answer.
            let mut entries = self.outbox_for_object(vault_id, id)?;
            for e in entries
                .iter_mut()
                .filter(|e| e.state == OutboxState::Conflict)
            {
                let fresh = MutationId::new();
                self.outbox_requeue_conflict(e.ordering, fresh)?;
                e.mutation_id = fresh;
                e.state = OutboxState::Queued;
            }
            if !entries.is_empty() && l.local_state != LocalState::Pending {
                self.set_object_state(vault_id, id, LocalState::Pending)?;
            }
            let Some(s) = s else {
                // The server does not know the object at all.
                if entries.first().is_some_and(|e| e.base_revision == 0) {
                    continue; // a pending create — pushes as is
                }
                if l.deleted && l.server_revision == 0 && entries.is_empty() {
                    continue; // never reached any server
                }
                self.requeue_local(l, 0, None, codec, &mut out)?;
                continue;
            };
            if entries.is_empty() {
                // Same content under another revision / ciphertext (e.g. an
                // object another device already restored): take the
                // server's copy, nothing to push or resolve.
                let same_content = |codec: &mut C| -> Result<bool, E> {
                    Ok(same_state(l, s)
                        || (l.deleted && s.deleted)
                        || (!l.deleted && !s.deleted && codec.same_payload(l, s)?))
                };
                if s.revision > l.server_revision {
                    self.adopt_remote(l, s)?;
                    out.applied.push((id, s.revision, s.deleted));
                } else if same_content(codec)? {
                    self.adopt_remote(l, s)?;
                } else if s.revision == l.server_revision {
                    self.synthesize_conflict(l, s)?;
                    out.conflicts.push(id);
                } else {
                    self.requeue_local(l, s.revision, Some(s.sequence), codec, &mut out)?;
                }
                continue;
            }
            let believed = entries[0].base_revision;
            let forked = s.revision == believed
                && l.server_revision == believed
                && matches!((l.writer_device_id, s.writer_device_id), (Some(a), Some(b)) if a != b);
            if forked {
                // Our edit is based on a revision another device rewrote
                // after the restore: decide with the conflict policy instead
                // of pushing over it.
                self.upsert_remote_object(s)?;
                self.outbox_mark_conflict(
                    entries[0].mutation_id,
                    s.revision,
                    s.sequence,
                    s.deleted,
                )?;
                self.set_object_state(vault_id, id, LocalState::Conflict)?;
                out.conflicts.push(id);
            } else if s.revision > believed {
                // Server moved past our base: the push conflicts and the
                // normal policy decides against this stashed state.
                self.upsert_remote_object(s)?;
            } else if s.revision < believed {
                if l.deleted && s.deleted {
                    self.outbox_delete_object(vault_id, id)?;
                    self.adopt_remote(l, s)?;
                } else {
                    self.requeue_local(l, s.revision, Some(s.sequence), codec, &mut out)?;
                }
            }
        }

        self.clear_recovery_remote(vault_id)?;
        self.clear_snapshot_seen(vault_id)?;
        cursor.last_sequence = rec.cursor;
        cursor.server_latest_sequence = rec.latest_sequence.max(rec.cursor);
        cursor.snapshot_complete = true;
        cursor.snapshot_cursor = None;
        cursor.snapshot_start_sequence = None;
        if rec.epoch.is_some() {
            cursor.epoch = rec.epoch;
        }
        cursor.recovery = None;
        self.put_sync_cursor(&cursor)?;
        out.cursor = cursor;
        Ok(out)
    }

    /// Replace the local row with the server state (keeping local-only
    /// metadata such as the conflict origin and a known KEK class).
    fn adopt_remote(&self, l: &StoredObject, s: &RemoteObject) -> Result<()> {
        let mut row = stored_from_remote(s, l.conflict_origin);
        row.kek_class_hint = row.kek_class_hint.or(l.kek_class_hint);
        self.upsert_object(&row)
    }

    /// Queue the local version of `l` on top of server revision `base`
    /// (replacing any queued entries of the object).
    fn requeue_local<E, C>(
        &self,
        l: &StoredObject,
        base: i64,
        sequence: Option<i64>,
        codec: &mut C,
        out: &mut ReconcileOutcome,
    ) -> Result<(), E>
    where
        E: From<StorageError>,
        C: ReconcileCodec<E>,
    {
        let (vault_id, id) = (l.vault_id, l.object_id);
        let revision = base + 1;
        let body = if l.deleted {
            None
        } else if l.revision == revision && l.body.is_some() {
            l.body.clone()
        } else {
            match codec.reencrypt(l, revision)? {
                Some(b) => Some(b),
                None => {
                    tracing::warn!(object_id = %id, "local object unreadable; not re-pushed after server rollback");
                    out.unrecoverable.push(id);
                    return Ok(());
                }
            }
        };
        self.outbox_delete_object(vault_id, id)?;
        let row = StoredObject {
            revision,
            server_revision: base,
            sequence,
            body,
            local_state: LocalState::Pending,
            updated_at: now(),
            ..l.clone()
        };
        self.upsert_object(&row)?;
        let op = if l.deleted {
            OutboxOp::Delete
        } else {
            OutboxOp::Put
        };
        self.outbox_insert(MutationId::new(), vault_id, id, base, op, row.body.as_ref())?;
        if l.deleted {
            out.tombstones_reapplied.push((id, revision));
        } else {
            out.repushed.push((id, revision));
        }
        Ok(())
    }

    /// Same revision, different write on each side: hand the pair to the
    /// normal conflict policy (stash the server state, park the local
    /// version as a conflicted entry).
    fn synthesize_conflict(&self, l: &StoredObject, s: &RemoteObject) -> Result<()> {
        let (vault_id, id) = (l.vault_id, l.object_id);
        self.upsert_remote_object(s)?;
        let op = if l.deleted {
            OutboxOp::Delete
        } else {
            OutboxOp::Put
        };
        let e = self.outbox_insert(
            MutationId::new(),
            vault_id,
            id,
            s.revision,
            op,
            l.body.as_ref(),
        )?;
        self.outbox_mark_conflict(e.mutation_id, s.revision, s.sequence, s.deleted)?;
        self.set_object_state(vault_id, id, LocalState::Conflict)
    }

    // ---- repositories ------------------------------------------------------------------

    fn outbox_requeue_conflict(&self, ordering: i64, mutation_id: MutationId) -> Result<()> {
        self.c().execute(
            "UPDATE outbox SET state = 'queued', mutation_id = ?2, attempts = 0, \
             conflict_revision = NULL, conflict_sequence = NULL, conflict_deleted = NULL, \
             last_error = NULL, updated_at = ?3 WHERE ordering = ?1",
            params![ordering, mutation_id.to_string(), ts(&now())],
        )?;
        Ok(())
    }

    /// Server state listed for an object during the current recovery.
    pub fn get_recovery_remote(
        &self,
        vault_id: VaultId,
        object_id: ObjectId,
    ) -> Result<Option<RemoteObject>> {
        use crate::repo::objects::OptionalStorage;
        self.c()
            .query_row_and_then(
                &format!(
                    "SELECT {REMOTE_COLS} FROM recovery_remote WHERE vault_id = ?1 AND object_id = ?2"
                ),
                params![vault_id.to_string(), object_id.to_string()],
                |r| remote_from_row_in(r, "recovery_remote"),
            )
            .optional_storage()
    }

    /// Every server state listed during the current recovery.
    pub fn list_recovery_remote(&self, vault_id: VaultId) -> Result<Vec<RemoteObject>> {
        let mut stmt = self.c().prepare(&format!(
            "SELECT {REMOTE_COLS} FROM recovery_remote WHERE vault_id = ?1 ORDER BY sequence"
        ))?;
        let rows = stmt.query_and_then(params![vault_id.to_string()], |r| {
            remote_from_row_in(r, "recovery_remote")
        })?;
        rows.collect()
    }

    fn upsert_recovery_remote(&self, r: &RemoteObject) -> Result<()> {
        let (format, ct, nonce, wd, wdn) = body_cols(r.body.as_ref());
        let hint = r.kek_class_hint.as_ref().map(enum_str).transpose()?;
        self.c().execute(
            &format!(
                "INSERT OR REPLACE INTO recovery_remote ({REMOTE_COLS}) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)"
            ),
            params![
                r.vault_id.to_string(),
                r.object_id.to_string(),
                r.revision,
                r.sequence,
                r.deleted as i64,
                format,
                ct,
                nonce,
                wd,
                wdn,
                hint,
                r.writer_device_id.map(|id| id.to_string()),
                ts(&r.updated_at),
            ],
        )?;
        Ok(())
    }

    pub(crate) fn clear_recovery_remote(&self, vault_id: VaultId) -> Result<()> {
        self.c().execute(
            "DELETE FROM recovery_remote WHERE vault_id = ?1",
            params![vault_id.to_string()],
        )?;
        Ok(())
    }
}
