//! Local-only tables: `known_hosts`, `terminal_sessions`, `local_settings`.

use crate::db::Tx;
use crate::error::Result;
use crate::model::{KnownHostRecord, TerminalSessionRecord};
use crate::repo::objects::OptionalStorage;
use crate::sql::{enum_str, now, parse_enum, parse_id, parse_opt_id, parse_opt_ts, parse_ts, ts};
use cc_protocol::Timestamp;
use rusqlite::{params, Row};
use serde::de::DeserializeOwned;
use serde::Serialize;
use uuid::Uuid;

const KH_COLS: &str =
    "id, host_pattern, key_type, public_key, fingerprint_sha256, source, revoked, \
     vault_id, object_id, added_at, updated_at";

fn known_host_from_row(r: &Row<'_>) -> Result<KnownHostRecord> {
    const T: &str = "known_hosts";
    Ok(KnownHostRecord {
        id: Some(r.get(0)?),
        host_pattern: r.get(1)?,
        key_type: r.get(2)?,
        public_key: r.get(3)?,
        fingerprint_sha256: r.get(4)?,
        source: parse_enum(&r.get::<_, String>(5)?, T, "source")?,
        revoked: r.get::<_, i64>(6)? != 0,
        vault_id: parse_opt_id(r.get(7)?, T, "vault_id")?,
        object_id: parse_opt_id(r.get(8)?, T, "object_id")?,
        added_at: parse_ts(&r.get::<_, String>(9)?, T, "added_at")?,
        updated_at: parse_ts(&r.get::<_, String>(10)?, T, "updated_at")?,
    })
}

const TS_COLS: &str =
    "session_id, vault_id, host_id, title, started_at, ended_at, exit_status, metadata";

fn session_from_row(r: &Row<'_>) -> Result<TerminalSessionRecord> {
    const T: &str = "terminal_sessions";
    let metadata: String = r.get(7)?;
    Ok(TerminalSessionRecord {
        session_id: parse_id::<Uuid>(&r.get::<_, String>(0)?, T, "session_id")?,
        vault_id: parse_opt_id(r.get(1)?, T, "vault_id")?,
        host_id: parse_opt_id(r.get(2)?, T, "host_id")?,
        title: r.get(3)?,
        started_at: parse_ts(&r.get::<_, String>(4)?, T, "started_at")?,
        ended_at: parse_opt_ts(r.get(5)?, T, "ended_at")?,
        exit_status: r.get(6)?,
        metadata: serde_json::from_str(&metadata)
            .map_err(|e| crate::StorageError::corrupt(T, "metadata", e))?,
    })
}

impl Tx<'_> {
    // ---- known hosts -------------------------------------------------------

    /// Entries for a host pattern (`host` or `[host]:port`), including revoked ones.
    pub fn known_hosts_for(&self, host_pattern: &str) -> Result<Vec<KnownHostRecord>> {
        let mut stmt = self.c().prepare(&format!(
            "SELECT {KH_COLS} FROM known_hosts WHERE host_pattern = ?1 ORDER BY id"
        ))?;
        let rows = stmt.query_and_then(params![host_pattern], known_host_from_row)?;
        rows.collect()
    }

    /// All entries.
    pub fn known_hosts_list(&self) -> Result<Vec<KnownHostRecord>> {
        let mut stmt = self.c().prepare(&format!(
            "SELECT {KH_COLS} FROM known_hosts ORDER BY host_pattern, id"
        ))?;
        let rows = stmt.query_and_then([], known_host_from_row)?;
        rows.collect()
    }

    /// Insert or update (by pattern + key type + key); returns the row id.
    pub fn known_host_upsert(&self, rec: &KnownHostRecord) -> Result<i64> {
        self.c().execute(
            "INSERT INTO known_hosts (host_pattern, key_type, public_key, fingerprint_sha256, source, \
             revoked, vault_id, object_id, added_at, updated_at) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10) \
             ON CONFLICT (host_pattern, key_type, public_key) DO UPDATE SET \
             fingerprint_sha256 = excluded.fingerprint_sha256, source = excluded.source, \
             revoked = excluded.revoked, vault_id = excluded.vault_id, \
             object_id = excluded.object_id, updated_at = excluded.updated_at",
            params![
                rec.host_pattern,
                rec.key_type,
                rec.public_key,
                rec.fingerprint_sha256,
                enum_str(&rec.source)?,
                rec.revoked as i64,
                rec.vault_id.map(|v| v.to_string()),
                rec.object_id.map(|o| o.to_string()),
                ts(&rec.added_at),
                ts(&rec.updated_at),
            ],
        )?;
        Ok(self.c().query_row(
            "SELECT id FROM known_hosts WHERE host_pattern = ?1 AND key_type = ?2 AND public_key = ?3",
            params![rec.host_pattern, rec.key_type, rec.public_key],
            |r| r.get(0),
        )?)
    }

    /// Mark an entry as revoked (`@revoked`).
    pub fn known_host_revoke(&self, id: i64) -> Result<bool> {
        Ok(self.c().execute(
            "UPDATE known_hosts SET revoked = 1, updated_at = ?2 WHERE id = ?1",
            params![id, ts(&now())],
        )? > 0)
    }

    /// Delete an entry.
    pub fn known_host_remove(&self, id: i64) -> Result<bool> {
        Ok(self
            .c()
            .execute("DELETE FROM known_hosts WHERE id = ?1", params![id])?
            > 0)
    }

    // ---- terminal sessions -------------------------------------------------

    /// Insert or replace session metadata.
    pub fn terminal_session_upsert(&self, s: &TerminalSessionRecord) -> Result<()> {
        self.c().execute(
            &format!(
                "INSERT OR REPLACE INTO terminal_sessions ({TS_COLS}) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)"
            ),
            params![
                s.session_id.to_string(),
                s.vault_id.map(|v| v.to_string()),
                s.host_id.map(|h| h.to_string()),
                s.title,
                ts(&s.started_at),
                s.ended_at.as_ref().map(ts),
                s.exit_status,
                serde_json::to_string(&s.metadata)?,
            ],
        )?;
        Ok(())
    }

    /// Mark a session as ended.
    pub fn terminal_session_end(
        &self,
        session_id: Uuid,
        ended_at: Timestamp,
        exit_status: Option<i32>,
    ) -> Result<bool> {
        Ok(self.c().execute(
            "UPDATE terminal_sessions SET ended_at = ?2, exit_status = ?3 WHERE session_id = ?1",
            params![session_id.to_string(), ts(&ended_at), exit_status],
        )? > 0)
    }

    /// Most recent sessions first.
    pub fn terminal_sessions_recent(&self, limit: u32) -> Result<Vec<TerminalSessionRecord>> {
        let mut stmt = self.c().prepare(&format!(
            "SELECT {TS_COLS} FROM terminal_sessions ORDER BY started_at DESC LIMIT ?1"
        ))?;
        let rows = stmt.query_and_then(params![limit], session_from_row)?;
        rows.collect()
    }

    /// One session.
    pub fn terminal_session_get(&self, session_id: Uuid) -> Result<Option<TerminalSessionRecord>> {
        self.c()
            .query_row_and_then(
                &format!("SELECT {TS_COLS} FROM terminal_sessions WHERE session_id = ?1"),
                params![session_id.to_string()],
                session_from_row,
            )
            .optional_storage()
    }

    /// Delete sessions that started before `before`; returns how many.
    pub fn terminal_sessions_prune(&self, before: Timestamp) -> Result<usize> {
        Ok(self.c().execute(
            "DELETE FROM terminal_sessions WHERE started_at < ?1",
            params![ts(&before)],
        )?)
    }

    // ---- local settings ----------------------------------------------------

    /// Read a JSON setting.
    pub fn setting_get<T: DeserializeOwned>(&self, key: &str) -> Result<Option<T>> {
        let raw: Option<String> = self
            .c()
            .query_row_and_then(
                "SELECT value FROM local_settings WHERE key = ?1",
                params![key],
                |r| -> Result<String> { Ok(r.get(0)?) },
            )
            .optional_storage()?;
        raw.map(|s| {
            serde_json::from_str(&s)
                .map_err(|e| crate::StorageError::corrupt("local_settings", "value", e))
        })
        .transpose()
    }

    /// Write a JSON setting.
    pub fn setting_set<T: Serialize + ?Sized>(&self, key: &str, value: &T) -> Result<()> {
        self.c().execute(
            "INSERT OR REPLACE INTO local_settings (key, value, updated_at) VALUES (?1, ?2, ?3)",
            params![key, serde_json::to_string(value)?, ts(&now())],
        )?;
        Ok(())
    }

    /// Delete a setting; returns whether it existed.
    pub fn setting_delete(&self, key: &str) -> Result<bool> {
        Ok(self
            .c()
            .execute("DELETE FROM local_settings WHERE key = ?1", params![key])?
            > 0)
    }

    /// All setting keys.
    pub fn setting_keys(&self) -> Result<Vec<String>> {
        let mut stmt = self
            .c()
            .prepare("SELECT key FROM local_settings ORDER BY key")?;
        let rows = stmt.query_map([], |r| r.get::<_, String>(0))?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }
}
