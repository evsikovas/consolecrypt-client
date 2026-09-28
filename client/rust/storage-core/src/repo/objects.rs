//! `objects` and `remote_objects` repositories.

use crate::db::Tx;
use crate::error::Result;
use crate::model::{LocalState, RemoteObject, StoredObject};
use crate::sql::{
    body_cols, enum_str, parse_id, parse_opt_enum, parse_opt_id, parse_ts, read_body, ts,
};
use cc_protocol::{ObjectId, VaultId};
use rusqlite::{params, Row};

const OBJ_COLS: &str = "vault_id, object_id, revision, server_revision, sequence, format, \
     ciphertext, nonce, wrapped_dek, wrapped_dek_nonce, deleted, kek_class_hint, local_state, \
     conflict_origin, writer_device_id, updated_at";

pub(crate) const REMOTE_COLS: &str =
    "vault_id, object_id, revision, sequence, deleted, format, ciphertext, \
     nonce, wrapped_dek, wrapped_dek_nonce, kek_class_hint, writer_device_id, updated_at";

fn object_from_row(row: &Row<'_>) -> Result<StoredObject> {
    const T: &str = "objects";
    let local_state: String = row.get(12)?;
    Ok(StoredObject {
        vault_id: parse_id(&row.get::<_, String>(0)?, T, "vault_id")?,
        object_id: parse_id(&row.get::<_, String>(1)?, T, "object_id")?,
        revision: row.get(2)?,
        server_revision: row.get(3)?,
        sequence: row.get(4)?,
        body: read_body(row, 5, T)?,
        deleted: row.get::<_, i64>(10)? != 0,
        kek_class_hint: parse_opt_enum(row.get(11)?, T, "kek_class_hint")?,
        local_state: LocalState::parse(&local_state)
            .ok_or_else(|| crate::StorageError::corrupt(T, "local_state", "unknown value"))?,
        conflict_origin: parse_opt_id(row.get(13)?, T, "conflict_origin")?,
        writer_device_id: parse_opt_id(row.get(14)?, T, "writer_device_id")?,
        updated_at: parse_ts(&row.get::<_, String>(15)?, T, "updated_at")?,
    })
}

fn remote_from_row(row: &Row<'_>) -> Result<RemoteObject> {
    remote_from_row_in(row, "remote_objects")
}

/// Parse a row selected with [`REMOTE_COLS`] from `table`.
pub(crate) fn remote_from_row_in(row: &Row<'_>, table: &'static str) -> Result<RemoteObject> {
    Ok(RemoteObject {
        vault_id: parse_id(&row.get::<_, String>(0)?, table, "vault_id")?,
        object_id: parse_id(&row.get::<_, String>(1)?, table, "object_id")?,
        revision: row.get(2)?,
        sequence: row.get(3)?,
        deleted: row.get::<_, i64>(4)? != 0,
        body: read_body(row, 5, table)?,
        kek_class_hint: parse_opt_enum(row.get(10)?, table, "kek_class_hint")?,
        writer_device_id: parse_opt_id(row.get(11)?, table, "writer_device_id")?,
        updated_at: parse_ts(&row.get::<_, String>(12)?, table, "updated_at")?,
    })
}

impl Tx<'_> {
    /// Fetch one object (including tombstones).
    pub fn get_object(
        &self,
        vault_id: VaultId,
        object_id: ObjectId,
    ) -> Result<Option<StoredObject>> {
        self.c()
            .query_row_and_then(
                &format!("SELECT {OBJ_COLS} FROM objects WHERE vault_id = ?1 AND object_id = ?2"),
                params![vault_id.to_string(), object_id.to_string()],
                object_from_row,
            )
            .optional_storage()
    }

    /// All objects of a vault, ordered by object id. Tombstones only if asked.
    pub fn list_objects(
        &self,
        vault_id: VaultId,
        include_deleted: bool,
    ) -> Result<Vec<StoredObject>> {
        let sql = if include_deleted {
            format!("SELECT {OBJ_COLS} FROM objects WHERE vault_id = ?1 ORDER BY object_id")
        } else {
            format!(
                "SELECT {OBJ_COLS} FROM objects WHERE vault_id = ?1 AND deleted = 0 ORDER BY object_id"
            )
        };
        let mut stmt = self.c().prepare(&sql)?;
        let rows = stmt.query_and_then(params![vault_id.to_string()], object_from_row)?;
        rows.collect()
    }

    /// Objects of a vault in the given local state.
    pub fn list_objects_in_state(
        &self,
        vault_id: VaultId,
        state: LocalState,
    ) -> Result<Vec<StoredObject>> {
        let mut stmt = self.c().prepare(&format!(
            "SELECT {OBJ_COLS} FROM objects WHERE vault_id = ?1 AND local_state = ?2 ORDER BY object_id"
        ))?;
        let rows = stmt.query_and_then(
            params![vault_id.to_string(), state.as_str()],
            object_from_row,
        )?;
        rows.collect()
    }

    /// Number of live (non-deleted) objects in a vault.
    pub fn count_live_objects(&self, vault_id: VaultId) -> Result<u64> {
        let n: i64 = self.c().query_row(
            "SELECT count(*) FROM objects WHERE vault_id = ?1 AND deleted = 0",
            params![vault_id.to_string()],
            |r| r.get(0),
        )?;
        Ok(n.max(0) as u64)
    }

    /// Insert or replace an object row.
    pub fn upsert_object(&self, o: &StoredObject) -> Result<()> {
        let (format, ct, nonce, wd, wdn) = body_cols(o.body.as_ref());
        let hint = o.kek_class_hint.as_ref().map(enum_str).transpose()?;
        self.c().execute(
            &format!(
                "INSERT OR REPLACE INTO objects ({OBJ_COLS}) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16)"
            ),
            params![
                o.vault_id.to_string(),
                o.object_id.to_string(),
                o.revision,
                o.server_revision,
                o.sequence,
                format,
                ct,
                nonce,
                wd,
                wdn,
                o.deleted as i64,
                hint,
                o.local_state.as_str(),
                o.conflict_origin.map(|id| id.to_string()),
                o.writer_device_id.map(|id| id.to_string()),
                ts(&o.updated_at),
            ],
        )?;
        Ok(())
    }

    /// Set the local state of an object.
    pub fn set_object_state(
        &self,
        vault_id: VaultId,
        object_id: ObjectId,
        state: LocalState,
    ) -> Result<()> {
        self.c().execute(
            "UPDATE objects SET local_state = ?3 WHERE vault_id = ?1 AND object_id = ?2",
            params![vault_id.to_string(), object_id.to_string(), state.as_str()],
        )?;
        Ok(())
    }

    /// Update the cached KEK class of an object.
    pub fn set_kek_class_hint(
        &self,
        vault_id: VaultId,
        object_id: ObjectId,
        hint: Option<cc_models::KekClass>,
    ) -> Result<()> {
        let hint = hint.as_ref().map(enum_str).transpose()?;
        self.c().execute(
            "UPDATE objects SET kek_class_hint = ?3 WHERE vault_id = ?1 AND object_id = ?2",
            params![vault_id.to_string(), object_id.to_string(), hint],
        )?;
        Ok(())
    }

    /// Remove an object row entirely (local purge, not a tombstone).
    pub fn delete_object_row(&self, vault_id: VaultId, object_id: ObjectId) -> Result<()> {
        self.c().execute(
            "DELETE FROM objects WHERE vault_id = ?1 AND object_id = ?2",
            params![vault_id.to_string(), object_id.to_string()],
        )?;
        Ok(())
    }

    /// Stashed server state for an object with local changes.
    pub fn get_remote_object(
        &self,
        vault_id: VaultId,
        object_id: ObjectId,
    ) -> Result<Option<RemoteObject>> {
        self.c()
            .query_row_and_then(
                &format!(
                    "SELECT {REMOTE_COLS} FROM remote_objects WHERE vault_id = ?1 AND object_id = ?2"
                ),
                params![vault_id.to_string(), object_id.to_string()],
                remote_from_row,
            )
            .optional_storage()
    }

    /// Insert or replace the stashed server state of an object.
    pub fn upsert_remote_object(&self, r: &RemoteObject) -> Result<()> {
        let (format, ct, nonce, wd, wdn) = body_cols(r.body.as_ref());
        let hint = r.kek_class_hint.as_ref().map(enum_str).transpose()?;
        self.c().execute(
            &format!(
                "INSERT OR REPLACE INTO remote_objects ({REMOTE_COLS}) \
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

    /// Drop the stashed server state of an object.
    pub fn delete_remote_object(&self, vault_id: VaultId, object_id: ObjectId) -> Result<()> {
        self.c().execute(
            "DELETE FROM remote_objects WHERE vault_id = ?1 AND object_id = ?2",
            params![vault_id.to_string(), object_id.to_string()],
        )?;
        Ok(())
    }
}

/// `optional()` for `query_row_and_then` results carrying [`crate::StorageError`].
pub(crate) trait OptionalStorage<T> {
    fn optional_storage(self) -> Result<Option<T>>;
}

impl<T> OptionalStorage<T> for Result<T> {
    fn optional_storage(self) -> Result<Option<T>> {
        match self {
            Ok(v) => Ok(Some(v)),
            Err(crate::StorageError::Sqlite(rusqlite::Error::QueryReturnedNoRows)) => Ok(None),
            Err(e) => Err(e),
        }
    }
}
