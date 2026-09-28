//! `outbox` repository.

use crate::db::Tx;
use crate::error::{Result, StorageError};
use crate::model::{OutboxCounts, OutboxEntry, OutboxOp, OutboxState};
use crate::repo::objects::OptionalStorage;
use crate::sql::{body_cols, now, parse_id, parse_ts, read_body, ts};
use cc_protocol::{MutationId, ObjectId, VaultId};
use rusqlite::{params, Row};

const OUTBOX_COLS: &str = "ordering, mutation_id, vault_id, object_id, base_revision, op, format, \
     ciphertext, nonce, wrapped_dek, wrapped_dek_nonce, state, attempts, last_error, \
     conflict_revision, conflict_sequence, conflict_deleted, created_at, updated_at";

fn entry_from_row(row: &Row<'_>) -> Result<OutboxEntry> {
    const T: &str = "outbox";
    let op: String = row.get(5)?;
    let state: String = row.get(11)?;
    Ok(OutboxEntry {
        ordering: row.get(0)?,
        mutation_id: parse_id(&row.get::<_, String>(1)?, T, "mutation_id")?,
        vault_id: parse_id(&row.get::<_, String>(2)?, T, "vault_id")?,
        object_id: parse_id(&row.get::<_, String>(3)?, T, "object_id")?,
        base_revision: row.get(4)?,
        op: match op.as_str() {
            "put" => OutboxOp::Put,
            "delete" => OutboxOp::Delete,
            _ => return Err(StorageError::corrupt(T, "op", "unknown value")),
        },
        body: read_body(row, 6, T)?,
        state: match state.as_str() {
            "queued" => OutboxState::Queued,
            "conflict" => OutboxState::Conflict,
            "failed" => OutboxState::Failed,
            _ => return Err(StorageError::corrupt(T, "state", "unknown value")),
        },
        attempts: u32::try_from(row.get::<_, i64>(12)?).unwrap_or(u32::MAX),
        last_error: row.get(13)?,
        conflict_revision: row.get(14)?,
        conflict_sequence: row.get(15)?,
        conflict_deleted: row.get::<_, Option<i64>>(16)?.map(|v| v != 0),
        created_at: parse_ts(&row.get::<_, String>(17)?, T, "created_at")?,
        updated_at: parse_ts(&row.get::<_, String>(18)?, T, "updated_at")?,
    })
}

impl Tx<'_> {
    /// Append a new entry; returns it with its assigned `ordering`.
    #[allow(clippy::too_many_arguments)]
    pub fn outbox_insert(
        &self,
        mutation_id: MutationId,
        vault_id: VaultId,
        object_id: ObjectId,
        base_revision: i64,
        op: OutboxOp,
        body: Option<&cc_protocol::sync::EncryptedBody>,
    ) -> Result<OutboxEntry> {
        if (op == OutboxOp::Put) != body.is_some() {
            return Err(StorageError::Invalid(
                "put needs a body, delete must not have one".into(),
            ));
        }
        let (format, ct, nonce, wd, wdn) = body_cols(body);
        let t = ts(&now());
        self.c().execute(
            "INSERT INTO outbox (mutation_id, vault_id, object_id, base_revision, op, format, \
             ciphertext, nonce, wrapped_dek, wrapped_dek_nonce, state, attempts, created_at, updated_at) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, 'queued', 0, ?11, ?11)",
            params![
                mutation_id.to_string(),
                vault_id.to_string(),
                object_id.to_string(),
                base_revision,
                op.as_str(),
                format,
                ct,
                nonce,
                wd,
                wdn,
                t,
            ],
        )?;
        self.outbox_get(mutation_id)?
            .ok_or_else(|| StorageError::Invalid("inserted outbox entry vanished".into()))
    }

    /// Rewrite an entry's content in place (only for entries the server has
    /// provably not applied: never attempted, conflicted or failed).
    #[allow(clippy::too_many_arguments)]
    pub fn outbox_rewrite(
        &self,
        ordering: i64,
        new_mutation_id: MutationId,
        base_revision: i64,
        op: OutboxOp,
        body: Option<&cc_protocol::sync::EncryptedBody>,
        state: OutboxState,
        reset_attempts: bool,
    ) -> Result<()> {
        if (op == OutboxOp::Put) != body.is_some() {
            return Err(StorageError::Invalid(
                "put needs a body, delete must not have one".into(),
            ));
        }
        let (format, ct, nonce, wd, wdn) = body_cols(body);
        self.c().execute(
            "UPDATE outbox SET mutation_id = ?2, base_revision = ?3, op = ?4, format = ?5, \
             ciphertext = ?6, nonce = ?7, wrapped_dek = ?8, wrapped_dek_nonce = ?9, state = ?10, \
             attempts = CASE WHEN ?11 THEN 0 ELSE attempts END, last_error = NULL, updated_at = ?12 \
             WHERE ordering = ?1",
            params![
                ordering,
                new_mutation_id.to_string(),
                base_revision,
                op.as_str(),
                format,
                ct,
                nonce,
                wd,
                wdn,
                state.as_str(),
                reset_attempts,
                ts(&now()),
            ],
        )?;
        Ok(())
    }

    /// Entry by mutation id.
    pub fn outbox_get(&self, mutation_id: MutationId) -> Result<Option<OutboxEntry>> {
        self.c()
            .query_row_and_then(
                &format!("SELECT {OUTBOX_COLS} FROM outbox WHERE mutation_id = ?1"),
                params![mutation_id.to_string()],
                entry_from_row,
            )
            .optional_storage()
    }

    /// All entries of a vault in FIFO order.
    pub fn outbox_list(&self, vault_id: VaultId) -> Result<Vec<OutboxEntry>> {
        let mut stmt = self.c().prepare(&format!(
            "SELECT {OUTBOX_COLS} FROM outbox WHERE vault_id = ?1 ORDER BY ordering"
        ))?;
        let rows = stmt.query_and_then(params![vault_id.to_string()], entry_from_row)?;
        rows.collect()
    }

    /// Entries of one object in FIFO order.
    pub fn outbox_for_object(
        &self,
        vault_id: VaultId,
        object_id: ObjectId,
    ) -> Result<Vec<OutboxEntry>> {
        let mut stmt = self.c().prepare(&format!(
            "SELECT {OUTBOX_COLS} FROM outbox WHERE vault_id = ?1 AND object_id = ?2 ORDER BY ordering"
        ))?;
        let rows = stmt.query_and_then(
            params![vault_id.to_string(), object_id.to_string()],
            entry_from_row,
        )?;
        rows.collect()
    }

    /// Whether the object has any entry not yet accepted by the server.
    pub fn outbox_has_object(&self, vault_id: VaultId, object_id: ObjectId) -> Result<bool> {
        let n: i64 = self.c().query_row(
            "SELECT count(*) FROM outbox WHERE vault_id = ?1 AND object_id = ?2",
            params![vault_id.to_string(), object_id.to_string()],
            |r| r.get(0),
        )?;
        Ok(n > 0)
    }

    /// Delete one entry.
    pub fn outbox_delete(&self, mutation_id: MutationId) -> Result<()> {
        self.c().execute(
            "DELETE FROM outbox WHERE mutation_id = ?1",
            params![mutation_id.to_string()],
        )?;
        Ok(())
    }

    /// Delete every entry of a vault.
    pub fn outbox_clear(&self, vault_id: VaultId) -> Result<usize> {
        Ok(self.c().execute(
            "DELETE FROM outbox WHERE vault_id = ?1",
            params![vault_id.to_string()],
        )?)
    }

    /// Delete every entry of an object.
    pub fn outbox_delete_object(&self, vault_id: VaultId, object_id: ObjectId) -> Result<()> {
        self.c().execute(
            "DELETE FROM outbox WHERE vault_id = ?1 AND object_id = ?2",
            params![vault_id.to_string(), object_id.to_string()],
        )?;
        Ok(())
    }

    /// Move an entry to `Conflict` with the server's current state.
    pub fn outbox_mark_conflict(
        &self,
        mutation_id: MutationId,
        current_revision: i64,
        current_sequence: i64,
        current_deleted: bool,
    ) -> Result<()> {
        self.c().execute(
            "UPDATE outbox SET state = 'conflict', conflict_revision = ?2, conflict_sequence = ?3, \
             conflict_deleted = ?4, last_error = NULL, updated_at = ?5 WHERE mutation_id = ?1",
            params![
                mutation_id.to_string(),
                current_revision,
                current_sequence,
                current_deleted as i64,
                ts(&now())
            ],
        )?;
        Ok(())
    }

    /// Record an error for an entry; `permanent` moves it to `Failed`.
    pub fn outbox_record_error(
        &self,
        mutation_id: MutationId,
        error: &str,
        permanent: bool,
    ) -> Result<()> {
        let error: String = error.chars().take(500).collect();
        self.c().execute(
            "UPDATE outbox SET last_error = ?2, \
             state = CASE WHEN ?3 THEN 'failed' ELSE state END, updated_at = ?4 \
             WHERE mutation_id = ?1",
            params![mutation_id.to_string(), error, permanent, ts(&now())],
        )?;
        Ok(())
    }

    /// Put `Failed` entries of a vault back into the queue (user "retry").
    pub fn outbox_requeue_failed(&self, vault_id: VaultId) -> Result<usize> {
        Ok(self.c().execute(
            "UPDATE outbox SET state = 'queued', last_error = NULL, updated_at = ?2 \
             WHERE vault_id = ?1 AND state = 'failed'",
            params![vault_id.to_string(), ts(&now())],
        )?)
    }

    /// Counts by state.
    pub fn outbox_counts(&self, vault_id: VaultId) -> Result<OutboxCounts> {
        let mut stmt = self
            .c()
            .prepare("SELECT state, count(*) FROM outbox WHERE vault_id = ?1 GROUP BY state")?;
        let mut rows = stmt.query(params![vault_id.to_string()])?;
        let mut counts = OutboxCounts::default();
        while let Some(row) = rows.next()? {
            let state: String = row.get(0)?;
            let n = row.get::<_, i64>(1)?.max(0) as u64;
            match state.as_str() {
                "queued" => counts.queued = n,
                "conflict" => counts.conflict = n,
                "failed" => counts.failed = n,
                _ => {}
            }
        }
        Ok(counts)
    }

    /// Select the next push batch and mark every selected entry as attempted
    /// (`attempts += 1`) in the same transaction.
    ///
    /// Rules: FIFO; at most one entry per object (the oldest); objects whose
    /// oldest entry is not `Queued`, or that have a `Conflict` entry, are
    /// skipped entirely (later entries depend on the earlier one); stops at
    /// `max_count` entries or `max_bytes` estimated wire size (always at
    /// least one entry if any is eligible).
    pub fn outbox_begin_batch(
        &self,
        vault_id: VaultId,
        max_count: usize,
        max_bytes: usize,
    ) -> Result<Vec<OutboxEntry>> {
        let all = self.outbox_list(vault_id)?;
        let blocked: std::collections::HashSet<ObjectId> = all
            .iter()
            .filter(|e| e.state == OutboxState::Conflict)
            .map(|e| e.object_id)
            .collect();
        let mut seen = std::collections::HashSet::new();
        let mut batch = Vec::new();
        let mut bytes = 0usize;
        for e in all {
            if !seen.insert(e.object_id) {
                continue;
            }
            if e.state != OutboxState::Queued || blocked.contains(&e.object_id) {
                continue;
            }
            let size = e.approx_wire_size();
            if !batch.is_empty() && (batch.len() >= max_count || bytes + size > max_bytes) {
                break;
            }
            bytes += size;
            batch.push(e);
        }
        let t = ts(&now());
        for e in batch.iter_mut() {
            self.c().execute(
                "UPDATE outbox SET attempts = attempts + 1, updated_at = ?2 WHERE ordering = ?1",
                params![e.ordering, t],
            )?;
            e.attempts = e.attempts.saturating_add(1);
        }
        Ok(batch)
    }
}
