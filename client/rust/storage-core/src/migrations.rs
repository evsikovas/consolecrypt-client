//! Versioned schema migrations, tracked in `PRAGMA user_version`.
//!
//! Rules: migrations are append-only; never edit a released migration. Each
//! migration runs in its own transaction together with the `user_version`
//! bump. Opening a database with a newer version than this build knows is
//! refused ([`crate::StorageError::SchemaTooNew`]) instead of risking data loss.

use crate::error::{Result, StorageError};
use rusqlite::Connection;

/// Ordered migrations; index `i` upgrades the schema from version `i` to `i + 1`.
const MIGRATIONS: &[&str] = &[V1, V2];

/// Highest schema version this build understands.
pub const SCHEMA_VERSION: i64 = MIGRATIONS.len() as i64;

/// v1 — initial schema (CLIENT_SPEC §9, ADR-0003 client state).
const V1: &str = r#"
-- The profile this database belongs to (ADR-0106): exactly one row.
-- `kind = 'local'` profiles have no server fields and never sync.
CREATE TABLE profile (
    id            INTEGER PRIMARY KEY CHECK (id = 1),
    profile_id    TEXT NOT NULL,
    kind          TEXT NOT NULL CHECK (kind IN ('local', 'synced')),
    display_name  TEXT,
    server_url    TEXT,
    user_id       TEXT,
    device_id     TEXT NOT NULL,
    email         TEXT,
    created_at    TEXT NOT NULL,
    updated_at    TEXT NOT NULL,
    CHECK (kind = 'local' OR server_url IS NOT NULL)
);

-- Vaults known to this profile, with cached envelopes for offline unlock.
CREATE TABLE vaults (
    vault_id                TEXT PRIMARY KEY NOT NULL,
    role                    TEXT NOT NULL,
    state                   TEXT NOT NULL DEFAULT 'active',
    owner_user_id           TEXT,
    caller_trusted          INTEGER NOT NULL DEFAULT 0,
    server_latest_sequence  INTEGER NOT NULL DEFAULT 0,
    password_envelope       TEXT,
    recovery_envelope       TEXT,
    device_envelope         TEXT,
    server_created_at       TEXT,
    last_unlocked_at        TEXT,
    updated_at              TEXT NOT NULL
);

-- Latest local state of every object: per-object E2EE ciphertext only.
-- `revision` is the revision the stored ciphertext is bound to (AAD);
-- `server_revision` is the latest revision confirmed by the server (kept in
-- local-only mode as the base for a later reconnect merge).
CREATE TABLE objects (
    vault_id           TEXT NOT NULL,
    object_id          TEXT NOT NULL,
    revision           INTEGER NOT NULL CHECK (revision >= 1),
    server_revision    INTEGER NOT NULL DEFAULT 0 CHECK (server_revision >= 0),
    sequence           INTEGER,
    format             INTEGER,
    ciphertext         BLOB,
    nonce              BLOB,
    wrapped_dek        BLOB,
    wrapped_dek_nonce  BLOB,
    deleted            INTEGER NOT NULL DEFAULT 0 CHECK (deleted IN (0, 1)),
    kek_class_hint     TEXT,
    local_state        TEXT NOT NULL DEFAULT 'synced'
                       CHECK (local_state IN ('synced', 'pending', 'conflict', 'local_only')),
    conflict_origin    TEXT,
    writer_device_id   TEXT,
    updated_at         TEXT NOT NULL,
    PRIMARY KEY (vault_id, object_id),
    CHECK ((deleted = 1 AND ciphertext IS NULL)
        OR (deleted = 0 AND ciphertext IS NOT NULL AND nonce IS NOT NULL
            AND wrapped_dek IS NOT NULL AND wrapped_dek_nonce IS NOT NULL
            AND format IS NOT NULL))
) WITHOUT ROWID;
CREATE INDEX objects_by_state ON objects (vault_id, local_state);

-- Latest known server state of objects that have unpushed local changes
-- (kept aside until the conflict is resolved or our push is confirmed).
CREATE TABLE remote_objects (
    vault_id           TEXT NOT NULL,
    object_id          TEXT NOT NULL,
    revision           INTEGER NOT NULL,
    sequence           INTEGER NOT NULL,
    deleted            INTEGER NOT NULL CHECK (deleted IN (0, 1)),
    format             INTEGER,
    ciphertext         BLOB,
    nonce              BLOB,
    wrapped_dek        BLOB,
    wrapped_dek_nonce  BLOB,
    kek_class_hint     TEXT,
    writer_device_id   TEXT,
    updated_at         TEXT NOT NULL,
    PRIMARY KEY (vault_id, object_id)
) WITHOUT ROWID;

-- Offline outbox (ADR-0003 client algorithm). `ordering` is the FIFO order.
CREATE TABLE outbox (
    ordering           INTEGER PRIMARY KEY AUTOINCREMENT,
    mutation_id        TEXT NOT NULL UNIQUE,
    vault_id           TEXT NOT NULL,
    object_id          TEXT NOT NULL,
    base_revision      INTEGER NOT NULL CHECK (base_revision >= 0),
    op                 TEXT NOT NULL CHECK (op IN ('put', 'delete')),
    format             INTEGER,
    ciphertext         BLOB,
    nonce              BLOB,
    wrapped_dek        BLOB,
    wrapped_dek_nonce  BLOB,
    state              TEXT NOT NULL DEFAULT 'queued'
                       CHECK (state IN ('queued', 'conflict', 'failed')),
    attempts           INTEGER NOT NULL DEFAULT 0,
    last_error         TEXT,
    conflict_revision  INTEGER,
    conflict_sequence  INTEGER,
    conflict_deleted   INTEGER,
    created_at         TEXT NOT NULL,
    updated_at         TEXT NOT NULL,
    CHECK ((op = 'delete' AND ciphertext IS NULL)
        OR (op = 'put' AND ciphertext IS NOT NULL AND nonce IS NOT NULL
            AND wrapped_dek IS NOT NULL AND wrapped_dek_nonce IS NOT NULL
            AND format IS NOT NULL))
);
CREATE INDEX outbox_by_vault ON outbox (vault_id, state, ordering);
CREATE INDEX outbox_by_object ON outbox (vault_id, object_id, ordering);

CREATE TABLE sync_state (
    vault_id                 TEXT PRIMARY KEY NOT NULL,
    last_sequence            INTEGER NOT NULL DEFAULT 0,
    snapshot_complete        INTEGER NOT NULL DEFAULT 0,
    snapshot_cursor          INTEGER,
    snapshot_start_sequence  INTEGER,
    server_latest_sequence   INTEGER NOT NULL DEFAULT 0,
    last_sync_at             TEXT,
    last_error               TEXT
);

-- Objects seen during a (re-)snapshot; used to drop locally cached objects
-- that disappeared server-side (tombstone horizon, `410 gone`).
CREATE TABLE snapshot_seen (
    vault_id   TEXT NOT NULL,
    object_id  TEXT NOT NULL,
    PRIMARY KEY (vault_id, object_id)
) WITHOUT ROWID;

-- Local known-hosts cache (host keys are public; the whole DB is encrypted).
CREATE TABLE known_hosts (
    id                  INTEGER PRIMARY KEY AUTOINCREMENT,
    host_pattern        TEXT NOT NULL,
    key_type            TEXT NOT NULL,
    public_key          TEXT NOT NULL,
    fingerprint_sha256  TEXT NOT NULL,
    source              TEXT NOT NULL,
    revoked             INTEGER NOT NULL DEFAULT 0,
    vault_id            TEXT,
    object_id           TEXT,
    added_at            TEXT NOT NULL,
    updated_at          TEXT NOT NULL,
    UNIQUE (host_pattern, key_type, public_key)
);
CREATE INDEX known_hosts_by_pattern ON known_hosts (host_pattern);

-- Terminal session metadata (never synced; no scrollback content here).
CREATE TABLE terminal_sessions (
    session_id   TEXT PRIMARY KEY NOT NULL,
    vault_id     TEXT,
    host_id      TEXT,
    title        TEXT NOT NULL,
    started_at   TEXT NOT NULL,
    ended_at     TEXT,
    exit_status  INTEGER,
    metadata     TEXT NOT NULL DEFAULT '{}'
);
CREATE INDEX terminal_sessions_by_start ON terminal_sessions (started_at);

-- Device-local settings (JSON values), never synced.
CREATE TABLE local_settings (
    key         TEXT PRIMARY KEY NOT NULL,
    value       TEXT NOT NULL,
    updated_at  TEXT NOT NULL
);
"#;

/// v2 — server rollback / restore detection (protocol 1.3 vault epoch,
/// ADR-0103 addendum "Server rollback recovery"). Additive only.
const V2: &str = r#"
-- Vault epoch last seen from the server (NULL = unknown / pre-1.3 server).
ALTER TABLE sync_state ADD COLUMN epoch TEXT;
-- Rollback recovery in progress (NULL reason = none). The full listing
-- (`changes?after=0`) is paged into `recovery_remote`; `recovery_cursor` is
-- the next `after`, `recovery_listed = 1` once the last page is stored.
ALTER TABLE sync_state ADD COLUMN recovery_reason TEXT;
ALTER TABLE sync_state ADD COLUMN recovery_epoch TEXT;
ALTER TABLE sync_state ADD COLUMN recovery_cursor INTEGER NOT NULL DEFAULT 0;
ALTER TABLE sync_state ADD COLUMN recovery_listed INTEGER NOT NULL DEFAULT 0;
ALTER TABLE sync_state ADD COLUMN recovery_latest_sequence INTEGER NOT NULL DEFAULT 0;
ALTER TABLE sync_state ADD COLUMN recovery_detected_at TEXT;

-- Server state (latest revision incl. tombstones) of every object listed
-- during a rollback recovery; consumed and cleared by the reconcile step.
CREATE TABLE recovery_remote (
    vault_id           TEXT NOT NULL,
    object_id          TEXT NOT NULL,
    revision           INTEGER NOT NULL,
    sequence           INTEGER NOT NULL,
    deleted            INTEGER NOT NULL CHECK (deleted IN (0, 1)),
    format             INTEGER,
    ciphertext         BLOB,
    nonce              BLOB,
    wrapped_dek        BLOB,
    wrapped_dek_nonce  BLOB,
    kek_class_hint     TEXT,
    writer_device_id   TEXT,
    updated_at         TEXT NOT NULL,
    PRIMARY KEY (vault_id, object_id)
) WITHOUT ROWID;
"#;

/// Current `user_version` of the database.
pub fn current_version(conn: &Connection) -> Result<i64> {
    Ok(conn.query_row("PRAGMA user_version", [], |r| r.get(0))?)
}

/// Apply all pending migrations. Returns the resulting version.
pub fn migrate(conn: &mut Connection) -> Result<i64> {
    let mut version = current_version(conn)?;
    if version > SCHEMA_VERSION {
        return Err(StorageError::SchemaTooNew {
            found: version,
            supported: SCHEMA_VERSION,
        });
    }
    while version < SCHEMA_VERSION {
        let sql = MIGRATIONS[version as usize];
        let tx = conn.transaction()?;
        tx.execute_batch(sql)?;
        tx.pragma_update(None, "user_version", version + 1)?;
        tx.commit()?;
        version += 1;
        tracing::debug!(version, "applied storage migration");
    }
    Ok(version)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn v1_database_upgrades_to_v2_keeping_sync_state() {
        let mut conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(V1).unwrap();
        conn.pragma_update(None, "user_version", 1).unwrap();
        conn.execute(
            "INSERT INTO sync_state (vault_id, last_sequence, snapshot_complete) VALUES ('v', 42, 1)",
            [],
        )
        .unwrap();
        assert_eq!(migrate(&mut conn).unwrap(), SCHEMA_VERSION);
        let (last, epoch, reason, cursor): (i64, Option<String>, Option<String>, i64) = conn
            .query_row(
                "SELECT last_sequence, epoch, recovery_reason, recovery_cursor FROM sync_state",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
            )
            .unwrap();
        assert_eq!((last, epoch, reason, cursor), (42, None, None, 0));
        let n: i64 = conn
            .query_row("SELECT count(*) FROM recovery_remote", [], |r| r.get(0))
            .unwrap();
        assert_eq!(n, 0);
    }
}
