//! Synchronous database handle (one SQLCipher connection).

use crate::error::{Result, StorageError};
use crate::key::DatabaseKey;
use crate::migrations;
use rusqlite::{Connection, ErrorCode, OpenFlags, TransactionBehavior};
use std::path::{Path, PathBuf};
use std::time::Duration;

/// A keyed, migrated SQLCipher database.
///
/// Blocking: use [`crate::Storage`] from async code — it owns a `Database`
/// on a dedicated thread.
pub struct Database {
    conn: Connection,
    path: Option<PathBuf>,
}

impl std::fmt::Debug for Database {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Database")
            .field("path", &self.path)
            .finish_non_exhaustive()
    }
}

impl Database {
    /// Open (or create) the database at `path`, keyed with `key`, and apply
    /// migrations. Fails with [`StorageError::WrongKeyOrNotADatabase`] if an
    /// existing file cannot be decrypted with `key`.
    pub fn open(path: impl AsRef<Path>, key: &DatabaseKey) -> Result<Self> {
        let path = path.as_ref();
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent)?;
            }
        }
        let conn = Connection::open_with_flags(
            path,
            OpenFlags::SQLITE_OPEN_READ_WRITE
                | OpenFlags::SQLITE_OPEN_CREATE
                | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )?;
        let conn = Self::init(conn, key, true)?;
        Ok(Self {
            conn,
            path: Some(path.to_path_buf()),
        })
    }

    /// Encrypted in-memory database (tests, ephemeral profiles).
    pub fn open_in_memory(key: &DatabaseKey) -> Result<Self> {
        let conn = Connection::open_in_memory()?;
        let conn = Self::init(conn, key, false)?;
        Ok(Self { conn, path: None })
    }

    fn init(mut conn: Connection, key: &DatabaseKey, wal: bool) -> Result<Connection> {
        // Must be the very first statement on the connection.
        conn.execute_batch(&key.pragma_statement())?;
        // Fail closed if the linked SQLite is not SQLCipher.
        let cipher: Option<String> = conn
            .query_row("PRAGMA cipher_version", [], |r| r.get(0))
            .ok();
        if cipher.is_none() {
            return Err(StorageError::Invalid(
                "linked SQLite library has no SQLCipher support".into(),
            ));
        }
        // First real read decrypts page 1 and validates the key.
        match conn.query_row("SELECT count(*) FROM sqlite_master", [], |r| {
            r.get::<_, i64>(0)
        }) {
            Ok(_) => {}
            Err(rusqlite::Error::SqliteFailure(e, _)) if e.code == ErrorCode::NotADatabase => {
                return Err(StorageError::WrongKeyOrNotADatabase)
            }
            Err(e) => return Err(e.into()),
        }
        conn.busy_timeout(Duration::from_secs(5))?;
        conn.pragma_update(None, "foreign_keys", true)?;
        conn.pragma_update(None, "secure_delete", true)?;
        if wal {
            let _mode: String = conn.query_row("PRAGMA journal_mode = WAL", [], |r| r.get(0))?;
            conn.pragma_update(None, "synchronous", "NORMAL")?;
        }
        migrations::migrate(&mut conn)?;
        Ok(conn)
    }

    /// Path of the database file (`None` for in-memory databases).
    pub fn path(&self) -> Option<&Path> {
        self.path.as_deref()
    }

    /// Current schema version.
    pub fn schema_version(&self) -> Result<i64> {
        migrations::current_version(&self.conn)
    }

    /// Run `f` in an IMMEDIATE (write) transaction; commit on `Ok`, roll back
    /// on `Err`.
    pub fn write<T, E>(&mut self, f: impl FnOnce(&Tx<'_>) -> Result<T, E>) -> Result<T, E>
    where
        E: From<StorageError>,
    {
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(StorageError::from)?;
        let tx = Tx { tx };
        let out = f(&tx)?;
        tx.tx.commit().map_err(StorageError::from)?;
        Ok(out)
    }

    /// Run `f` in a read transaction (consistent snapshot); always rolled back.
    pub fn read<T, E>(&mut self, f: impl FnOnce(&Tx<'_>) -> Result<T, E>) -> Result<T, E>
    where
        E: From<StorageError>,
    {
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Deferred)
            .map_err(StorageError::from)?;
        let tx = Tx { tx };
        f(&tx)
    }
}

/// An open transaction. All repository methods live on this type so that
/// composite operations are atomic by construction.
pub struct Tx<'a> {
    tx: rusqlite::Transaction<'a>,
}

impl std::fmt::Debug for Tx<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Tx")
    }
}

impl Tx<'_> {
    /// Raw connection for repository implementations inside this crate.
    pub(crate) fn c(&self) -> &Connection {
        &self.tx
    }
}
