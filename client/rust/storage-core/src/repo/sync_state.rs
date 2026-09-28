//! `sync_state` and `snapshot_seen` repositories.

use crate::db::Tx;
use crate::error::{Result, StorageError};
use crate::model::{RollbackReason, RollbackRecovery, SyncCursor};
use crate::repo::objects::OptionalStorage;
use crate::sql::{now, parse_id, parse_opt_id, parse_opt_ts, ts};
use cc_protocol::{ObjectId, Timestamp, VaultId};
use rusqlite::params;

impl Tx<'_> {
    /// Sync cursor of a vault (defaults if the vault was never synced).
    pub fn get_sync_cursor(&self, vault_id: VaultId) -> Result<SyncCursor> {
        const T: &str = "sync_state";
        let row = self
            .c()
            .query_row_and_then(
                "SELECT vault_id, last_sequence, snapshot_complete, snapshot_cursor, \
                 snapshot_start_sequence, server_latest_sequence, last_sync_at, last_error, \
                 epoch, recovery_reason, recovery_epoch, recovery_cursor, recovery_listed, \
                 recovery_latest_sequence, recovery_detected_at \
                 FROM sync_state WHERE vault_id = ?1",
                params![vault_id.to_string()],
                |r| -> Result<SyncCursor> {
                    let recovery = match r.get::<_, Option<String>>(9)? {
                        None => None,
                        Some(reason) => Some(RollbackRecovery {
                            reason: RollbackReason::parse(&reason).ok_or_else(|| {
                                StorageError::corrupt(T, "recovery_reason", "unknown value")
                            })?,
                            epoch: parse_opt_id(r.get(10)?, T, "recovery_epoch")?,
                            cursor: r.get(11)?,
                            listed: r.get::<_, i64>(12)? != 0,
                            latest_sequence: r.get(13)?,
                            detected_at: parse_opt_ts(r.get(14)?, T, "recovery_detected_at")?
                                .unwrap_or_else(now),
                        }),
                    };
                    Ok(SyncCursor {
                        vault_id: parse_id(&r.get::<_, String>(0)?, T, "vault_id")?,
                        last_sequence: r.get(1)?,
                        snapshot_complete: r.get::<_, i64>(2)? != 0,
                        snapshot_cursor: r.get(3)?,
                        snapshot_start_sequence: r.get(4)?,
                        server_latest_sequence: r.get(5)?,
                        last_sync_at: parse_opt_ts(r.get(6)?, T, "last_sync_at")?,
                        last_error: r.get(7)?,
                        epoch: parse_opt_id(r.get(8)?, T, "epoch")?,
                        recovery,
                    })
                },
            )
            .optional_storage()?;
        Ok(row.unwrap_or_else(|| SyncCursor::new(vault_id)))
    }

    /// Persist a sync cursor.
    pub fn put_sync_cursor(&self, c: &SyncCursor) -> Result<()> {
        let rec = c.recovery.as_ref();
        self.c().execute(
            "INSERT OR REPLACE INTO sync_state (vault_id, last_sequence, snapshot_complete, \
             snapshot_cursor, snapshot_start_sequence, server_latest_sequence, last_sync_at, \
             last_error, epoch, recovery_reason, recovery_epoch, recovery_cursor, recovery_listed, \
             recovery_latest_sequence, recovery_detected_at) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)",
            params![
                c.vault_id.to_string(),
                c.last_sequence,
                c.snapshot_complete as i64,
                c.snapshot_cursor,
                c.snapshot_start_sequence,
                c.server_latest_sequence,
                c.last_sync_at.as_ref().map(ts),
                c.last_error,
                c.epoch.map(|e| e.to_string()),
                rec.map(|r| r.reason.as_str()),
                rec.and_then(|r| r.epoch).map(|e| e.to_string()),
                rec.map_or(0, |r| r.cursor),
                rec.is_some_and(|r| r.listed) as i64,
                rec.map_or(0, |r| r.latest_sequence),
                rec.map(|r| ts(&r.detected_at)),
            ],
        )?;
        Ok(())
    }

    /// Raise the "latest server sequence we heard of" (never lowers it).
    pub fn note_server_sequence(&self, vault_id: VaultId, latest: i64) -> Result<()> {
        let mut c = self.get_sync_cursor(vault_id)?;
        if latest > c.server_latest_sequence {
            c.server_latest_sequence = latest;
            self.put_sync_cursor(&c)?;
        }
        Ok(())
    }

    /// Record the outcome of a sync attempt.
    pub fn record_sync_attempt(
        &self,
        vault_id: VaultId,
        at: Timestamp,
        error: Option<&str>,
    ) -> Result<()> {
        let mut c = self.get_sync_cursor(vault_id)?;
        match error {
            None => {
                c.last_sync_at = Some(at);
                c.last_error = None;
            }
            Some(e) => c.last_error = Some(e.chars().take(500).collect()),
        }
        self.put_sync_cursor(&c)
    }

    /// Forget snapshot progress so the next sync runs a fresh snapshot
    /// (after `410 gone`). Local objects are kept until the snapshot
    /// completes; then synced objects not seen are removed.
    pub fn reset_for_resnapshot(&self, vault_id: VaultId) -> Result<()> {
        let mut c = self.get_sync_cursor(vault_id)?;
        c.snapshot_complete = false;
        c.snapshot_cursor = None;
        c.snapshot_start_sequence = None;
        self.put_sync_cursor(&c)?;
        self.c().execute(
            "DELETE FROM snapshot_seen WHERE vault_id = ?1",
            params![vault_id.to_string()],
        )?;
        Ok(())
    }

    pub(crate) fn mark_snapshot_seen(&self, vault_id: VaultId, object_id: ObjectId) -> Result<()> {
        self.c().execute(
            "INSERT OR IGNORE INTO snapshot_seen (vault_id, object_id) VALUES (?1, ?2)",
            params![vault_id.to_string(), object_id.to_string()],
        )?;
        Ok(())
    }

    /// Synced, live objects that were not part of the finished snapshot and
    /// whose server state is at or below `start_sequence` — they no longer
    /// exist server-side.
    pub(crate) fn unseen_synced_objects(
        &self,
        vault_id: VaultId,
        start_sequence: i64,
    ) -> Result<Vec<ObjectId>> {
        let mut stmt = self.c().prepare(
            "SELECT object_id FROM objects o WHERE o.vault_id = ?1 AND o.local_state = 'synced' \
             AND o.deleted = 0 AND COALESCE(o.sequence, 0) <= ?2 AND NOT EXISTS \
             (SELECT 1 FROM snapshot_seen s WHERE s.vault_id = o.vault_id AND s.object_id = o.object_id)",
        )?;
        let rows = stmt.query_and_then(params![vault_id.to_string(), start_sequence], |r| {
            parse_id::<ObjectId>(&r.get::<_, String>(0)?, "objects", "object_id")
        })?;
        rows.collect()
    }

    pub(crate) fn clear_snapshot_seen(&self, vault_id: VaultId) -> Result<()> {
        self.c().execute(
            "DELETE FROM snapshot_seen WHERE vault_id = ?1",
            params![vault_id.to_string()],
        )?;
        Ok(())
    }
}
