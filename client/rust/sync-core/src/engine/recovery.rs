//! Server rollback recovery (ADR-0103 addendum "Server rollback recovery").
//!
//! ```text
//! detect:    page / VaultInfo epoch ≠ stored epoch (both known)
//!            | latest_sequence < our cursor
//!            | push accepted with a sequence we already hold for another object
//!            → persist "recovery pending", emit ServerRollbackDetected
//! list:      changes?after=0 (paged; latest state incl. tombstones) → recovery_remote
//!            (epoch moves again / sequence below the listing cursor → restart listing)
//! reconcile: one transaction — server newer: apply (or stash for pending edits);
//!            server lacks / older: re-queue the local copy (re-encrypted for
//!            server revision + 1; lost objects as creates on base 0; local
//!            tombstones re-applied); same revision, different write: conflict
//!            policy; cursor := end of listing, epoch := new epoch
//! then:      the normal cycle pushes the re-queued objects and resolves conflicts
//! ```
//!
//! Each step commits with its progress, so a crash or a locked vault only
//! delays the recovery; it resumes on the next cycle.

use super::{malformed_if_invalid, validate, verify_changes, SyncEngine, SyncReport};
use crate::codec::CodecError;
use crate::error::SyncError;
use crate::events::{ChangeOrigin, SyncEvent};
use crate::store::check_size;
use cc_models::ObjectPayload;
use cc_protocol::sync::ChangesQuery;
use cc_protocol::vaults::VaultInfo;
use cc_protocol::{ErrorCode, ObjectId};
use cc_storage_core::{
    ReconcileCodec, RecoveryPage, RemoteObject, RollbackReason, StorageError, StoredObject,
};

/// Listing restarts tolerated within one recovery attempt.
const MAX_LISTING_RESTARTS: usize = 3;

/// The vault codec as seen by the storage-side reconcile.
struct ReconcileWith<'a>(&'a dyn crate::codec::ObjectCodec);

impl ReconcileWith<'_> {
    /// Decrypt; `Ok(None)` for anything but a locked vault.
    fn open(
        &self,
        object_id: ObjectId,
        revision: i64,
        body: Option<&cc_protocol::sync::EncryptedBody>,
        hint: Option<cc_models::KekClass>,
    ) -> Result<Option<ObjectPayload>, SyncError> {
        let Some(body) = body else { return Ok(None) };
        match self.0.decrypt_hinted(object_id, revision, body, hint) {
            Ok(p) => Ok(Some(p)),
            Err(CodecError::Locked) => Err(CodecError::Locked.into()),
            Err(e) => {
                tracing::warn!(%object_id, revision, error = %e, "object undecryptable during rollback reconcile");
                Ok(None)
            }
        }
    }
}

impl ReconcileCodec<SyncError> for ReconcileWith<'_> {
    fn reencrypt(
        &mut self,
        local: &StoredObject,
        revision: i64,
    ) -> Result<Option<cc_protocol::sync::EncryptedBody>, SyncError> {
        let Some(payload) = self.open(
            local.object_id,
            local.revision,
            local.body.as_ref(),
            local.kek_class_hint,
        )?
        else {
            return Ok(None);
        };
        let body = self.0.encrypt(local.object_id, revision, &payload)?;
        Ok(check_size(&body).is_ok().then_some(body))
    }

    fn same_payload(
        &mut self,
        local: &StoredObject,
        server: &RemoteObject,
    ) -> Result<bool, SyncError> {
        let l = self.open(
            local.object_id,
            local.revision,
            local.body.as_ref(),
            local.kek_class_hint,
        )?;
        let s = self.open(
            server.object_id,
            server.revision,
            server.body.as_ref(),
            server.kek_class_hint,
        )?;
        Ok(matches!((l, s), (Some(l), Some(s)) if l == s))
    }
}

impl SyncEngine {
    /// Compare a `VaultInfo` (e.g. from `GET /v1/vaults`) with what this
    /// device holds. Stores the vault epoch if none is known yet; if the
    /// epoch changed or `latest_sequence` is below the local cursor, starts a
    /// rollback recovery (emits [`SyncEvent::ServerRollbackDetected`]) and
    /// wakes the worker. Returns the reason while a recovery is pending.
    pub async fn observe_vault_info(
        &self,
        info: &VaultInfo,
    ) -> Result<Option<RollbackReason>, SyncError> {
        let vault_id = self.vault_id();
        if info.vault_id != vault_id {
            return Err(SyncError::Invalid("VaultInfo is for another vault".into()));
        }
        let (epoch, latest) = (info.epoch, info.latest_sequence);
        let (detected, pending) = self
            .inner
            .store
            .storage()
            .write(move |tx| {
                let c = tx.get_sync_cursor(vault_id)?;
                if let Some(reason) = c.rollback_evidence(epoch, latest) {
                    let started = tx.begin_rollback_recovery(vault_id, reason, epoch)?;
                    return Ok::<_, StorageError>((Some((reason, started)), Some(reason)));
                }
                if c.recovery.is_none() {
                    tx.note_server_epoch(vault_id, epoch)?;
                }
                Ok((None, c.recovery.map(|r| r.reason)))
            })
            .await?;
        if let Some((reason, started)) = detected {
            self.rollback_detected(reason, started);
            self.trigger();
        }
        Ok(pending)
    }

    /// Persist a detected rollback and report it.
    pub(super) async fn begin_recovery(
        &self,
        reason: RollbackReason,
        epoch: Option<uuid::Uuid>,
    ) -> Result<(), SyncError> {
        let vault_id = self.vault_id();
        let started = self
            .inner
            .store
            .storage()
            .write(move |tx| tx.begin_rollback_recovery(vault_id, reason, epoch))
            .await?;
        self.rollback_detected(reason, started);
        Ok(())
    }

    /// Status + event for a (possibly already pending) recovery.
    pub(super) fn rollback_detected(&self, reason: RollbackReason, started: bool) {
        let vault_id = self.vault_id();
        self.inner
            .status
            .send_modify(|s| s.rollback_recovery = Some(reason));
        if started {
            tracing::warn!(%vault_id, %reason, "server rollback detected; recovering");
            self.inner
                .store
                .emit(SyncEvent::ServerRollbackDetected { vault_id, reason });
        }
    }

    pub(super) fn emit_integrity_warnings(&self, warnings: Vec<(ObjectId, i64)>) {
        let vault_id = self.vault_id();
        for (object_id, revision) in warnings {
            tracing::warn!(%object_id, revision, "server ciphertext failed verification");
            self.inner.store.emit(SyncEvent::IntegrityWarning {
                vault_id,
                object_id,
                revision,
            });
        }
    }

    /// Run (or resume) a pending recovery to completion.
    pub(super) async fn recover(&self, report: &mut SyncReport) -> Result<(), SyncError> {
        let vault_id = self.vault_id();
        let limit = self.inner.cfg.page_limit;
        let mut restarts = 0;
        for _ in 0..1_000_000 {
            let Some(rec) = self.cursor().await?.recovery else {
                return Ok(());
            };
            self.inner
                .status
                .send_modify(|s| s.rollback_recovery = Some(rec.reason));
            if rec.listed {
                return self.reconcile(report).await;
            }
            let q = ChangesQuery {
                vault_id,
                after: rec.cursor,
                limit: Some(limit),
            };
            let resp = match self.inner.api.changes(&q).await {
                Ok(r) => r,
                // TODO(server): a tombstone horizon (410 on `changes`) would
                // make the full listing impossible — ADR-0202 rules it out in
                // v1; next: if compaction is ever introduced, fall back to a
                // snapshot listing and probe unlisted objects with creates.
                Err(e) if e.is_code(ErrorCode::Gone) => {
                    return Err(SyncError::MalformedResponse(
                        "server refused the full listing needed for rollback recovery (410)".into(),
                    ))
                }
                Err(e) => return Err(e.into()),
            };
            let epoch_moved = matches!((rec.epoch, resp.epoch), (Some(a), Some(b)) if a != b);
            if epoch_moved || resp.latest_sequence < rec.cursor {
                restarts += 1;
                if restarts > MAX_LISTING_RESTARTS {
                    return Err(SyncError::MalformedResponse(
                        "server state keeps changing during rollback recovery".into(),
                    ));
                }
                tracing::warn!(%vault_id, "server rolled back again during recovery; restarting listing");
                let epoch = resp.epoch;
                self.inner
                    .store
                    .storage()
                    .write(move |tx| tx.restart_recovery_listing(vault_id, epoch))
                    .await?;
                continue;
            }
            validate::changes_page(&resp, rec.cursor, limit)?;
            let page = RecoveryPage {
                next_after: resp.next_after,
                has_more: resp.has_more,
                latest_sequence: resp.latest_sequence,
                epoch: resp.epoch,
            };
            let codec = self.inner.store.codec().clone();
            let changes = resp.changes;
            let warnings = self
                .inner
                .store
                .storage()
                .write(move |tx| -> Result<_, SyncError> {
                    let (remote, warnings) = verify_changes(codec.as_ref(), changes);
                    tx.record_recovery_page(vault_id, &remote, page)
                        .map_err(malformed_if_invalid)?;
                    Ok(warnings)
                })
                .await?;
            self.emit_integrity_warnings(warnings);
            report.change_pages += 1;
        }
        Ok(())
    }

    /// Reconcile the completed listing with local state (one transaction;
    /// needs the codec to re-encrypt objects for their new revision).
    async fn reconcile(&self, report: &mut SyncReport) -> Result<(), SyncError> {
        let vault_id = self.vault_id();
        let codec = self.inner.store.codec().clone();
        let out = self
            .inner
            .store
            .storage()
            .write(move |tx| {
                tx.reconcile_rollback::<SyncError, _>(vault_id, &mut ReconcileWith(codec.as_ref()))
            })
            .await?;
        tracing::info!(
            %vault_id,
            reason = %out.reason,
            applied = out.applied.len(),
            repushed = out.repushed.len(),
            tombstones = out.tombstones_reapplied.len(),
            conflicts = out.conflicts.len(),
            unrecoverable = out.unrecoverable.len(),
            "server rollback reconciled"
        );
        for &(object_id, revision, deleted) in &out.applied {
            self.inner.store.emit(SyncEvent::ObjectChanged {
                vault_id,
                object_id,
                revision,
                deleted,
                origin: ChangeOrigin::Remote,
            });
        }
        let requeued = out
            .repushed
            .iter()
            .map(|&(id, rev)| (id, rev, false))
            .chain(
                out.tombstones_reapplied
                    .iter()
                    .map(|&(id, rev)| (id, rev, true)),
            );
        for (object_id, revision, deleted) in requeued {
            self.inner.store.emit(SyncEvent::ObjectChanged {
                vault_id,
                object_id,
                revision,
                deleted,
                origin: ChangeOrigin::Local,
            });
        }
        report.rollback_recoveries += 1;
        report.pulled += out.applied.len();
        report.repushed += out.repushed.len();
        report.tombstones_reapplied += out.tombstones_reapplied.len();
        let (last, latest) = (out.cursor.last_sequence, out.cursor.server_latest_sequence);
        self.inner.status.send_modify(|s| {
            s.rollback_recovery = None;
            s.last_sequence = last;
            s.server_latest_sequence = latest;
        });
        self.inner
            .store
            .emit(SyncEvent::SnapshotCompleted { vault_id });
        Ok(())
    }
}
