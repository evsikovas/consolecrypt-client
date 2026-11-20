//! Native Reader authority transactions. Existing ordinary write policy stays
//! unchanged. No public raw connection, SQL text or private values are exposed.
use super::{Database, Tx};
use crate::sql::{now, ts};
use crate::{Result as StorageResult, StorageError};
use rusqlite::{params, OptionalExtension, TransactionBehavior};

/// Channel/commit/rollback/restore uncertainty must never publish authority.
pub enum ReaderAuthorityWriteError<E> {
    BeforeWrite(StorageError),
    Rejected(E),
    Unknown,
}
impl<E> std::fmt::Debug for ReaderAuthorityWriteError<E> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::BeforeWrite(_) => "ReaderAuthorityWriteError::BeforeWrite",
            Self::Rejected(_) => "ReaderAuthorityWriteError::Rejected",
            Self::Unknown => "ReaderAuthorityWriteError::Unknown",
        })
    }
}
impl<E> std::fmt::Display for ReaderAuthorityWriteError<E> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::Debug::fmt(self, f)
    }
}
impl<E> std::error::Error for ReaderAuthorityWriteError<E> {}
impl<E> From<StorageError> for ReaderAuthorityWriteError<E> {
    fn from(_: StorageError) -> Self {
        Self::Unknown
    }
}

impl Database {
    /// Set and verify at least FULL before the IMMEDIATE transaction on this exclusively
    /// borrowed connection. Native state read/compare/write belongs in ONE
    /// closure. A successful disk commit uses SQLite's FULL acknowledgement;
    /// in-memory databases provide process atomicity only, never power durability.
    ///
    /// Panics propagate to the existing Storage worker catch boundary. The
    /// transaction rolls back on unwind; no success can escape. FULL may remain
    /// enabled until close after a panic, which is a stronger failure policy.
    pub fn reader_authority_write<T, E>(
        &mut self,
        f: impl FnOnce(&Tx<'_>) -> Result<T, E>,
    ) -> Result<T, ReaderAuthorityWriteError<E>>
    where
        E: From<StorageError>,
    {
        self.durable_write_kernel(false, f)
    }
    // SAME at-least-FULL/journal/readback/restore kernel. require_file is a
    // fixed internal purpose choice, never a frontend setting or SQL string.
    pub(super) fn durable_write_kernel<T, E>(
        &mut self,
        require_file: bool,
        f: impl FnOnce(&Tx<'_>) -> Result<T, E>,
    ) -> Result<T, ReaderAuthorityWriteError<E>>
    where
        E: From<StorageError>,
    {
        if require_file && self.path.as_ref().is_none_or(|path| !path.is_absolute()) {
            return Err(ReaderAuthorityWriteError::BeforeWrite(
                StorageError::Invalid("project publication requires file-backed storage".into()),
            ));
        }
        let journal: String = self
            .conn
            .query_row("PRAGMA journal_mode", [], |r| r.get(0))
            .map_err(|e| ReaderAuthorityWriteError::BeforeWrite(e.into()))?;
        let durable_journal = matches!(journal.as_str(), "wal" | "delete" | "truncate" | "persist");
        // OFF cannot roll back even in memory. MEMORY is process-only and is
        // permitted solely for an explicitly in-memory Database, never a file.
        if !durable_journal && !(self.path.is_none() && journal == "memory") {
            return Err(ReaderAuthorityWriteError::BeforeWrite(
                StorageError::Invalid("reader authority requires durable journal".into()),
            ));
        }
        let previous: i64 = self
            .conn
            .query_row("PRAGMA synchronous", [], |r| r.get(0))
            .map_err(|e| ReaderAuthorityWriteError::BeforeWrite(e.into()))?;
        if !matches!(previous, 1..=3) {
            return Err(ReaderAuthorityWriteError::BeforeWrite(
                StorageError::Invalid("reader authority requires synchronous storage".into()),
            ));
        }
        let required = previous.max(2);
        self.conn
            .pragma_update(None, "synchronous", required)
            .map_err(|e| ReaderAuthorityWriteError::BeforeWrite(e.into()))?;
        let full = self
            .conn
            .query_row("PRAGMA synchronous", [], |r| r.get::<_, i64>(0));
        if full.ok() != Some(required) {
            let restored = self.conn.pragma_update(None, "synchronous", previous);
            return if restored.is_err() {
                Err(ReaderAuthorityWriteError::Unknown)
            } else {
                Err(ReaderAuthorityWriteError::BeforeWrite(
                    StorageError::Invalid("reader authority FULL unavailable".into()),
                ))
            };
        }
        let outcome = match self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
        {
            Ok(transaction) => {
                let tx = Tx { tx: transaction };
                match f(&tx) {
                    Ok(value) => match tx.tx.commit() {
                        Ok(()) => Ok(value),
                        Err(_) => Err(ReaderAuthorityWriteError::Unknown),
                    },
                    Err(error) => match tx.tx.rollback() {
                        Ok(()) => Err(ReaderAuthorityWriteError::Rejected(error)),
                        Err(_) => Err(ReaderAuthorityWriteError::Unknown),
                    },
                }
            }
            Err(error) => Err(ReaderAuthorityWriteError::BeforeWrite(error.into())),
        };
        // A restore failure can follow a committed write, so never return its
        // value as success or describe that operation as known rejected.
        if self
            .conn
            .pragma_update(None, "synchronous", previous)
            .is_err()
        {
            return Err(ReaderAuthorityWriteError::Unknown);
        }
        let restored = self
            .conn
            .query_row("PRAGMA synchronous", [], |r| r.get::<_, i64>(0));
        if restored.ok() != Some(previous) {
            return Err(ReaderAuthorityWriteError::Unknown);
        }
        outcome
    }
}

fn key_and_limit(key: &str, max: usize) -> StorageResult<()> {
    if !(1..=8 * 1024 * 1024).contains(&max)
        || key.len() > 200
        || !(key.starts_with("cc.native.reader.root.v1/")
            || key.starts_with("cc.native.reader.peer.v1/"))
        || !key
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b"./-".contains(&b))
        || key.ends_with('/')
        || key.contains("//")
        || key.contains("..")
    {
        return Err(StorageError::Invalid(
            "reader authority namespace or budget".into(),
        ));
    }
    Ok(())
}
impl Tx<'_> {
    /// SQL CASE guards the value before it is materialized in Rust. Oversized
    /// corrupt rows return only their length; the raw value never becomes a
    /// String/Vec. This is a read bound, not a whole database/page-cache limit.
    pub fn reader_authority_get_bounded(
        &self,
        key: &str,
        max: usize,
    ) -> StorageResult<Option<Vec<u8>>> {
        key_and_limit(key, max)?;
        let row: Option<(i64, Option<Vec<u8>>)> = self.c().query_row(
            "SELECT length(CAST(value AS BLOB)), CASE WHEN length(CAST(value AS BLOB)) <= ?2 THEN CAST(value AS BLOB) ELSE NULL END FROM local_settings WHERE key = ?1",
            params![key, max as i64], |r| Ok((r.get(0)?, r.get(1)?)),
        ).optional()?;
        match row {
            None => Ok(None),
            Some((length, Some(value)))
                if length >= 0 && length as usize <= max && value.len() == length as usize =>
            {
                Ok(Some(value))
            }
            _ => Err(StorageError::Invalid(
                "reader authority stored value budget or encoding".into(),
            )),
        }
    }
    /// Bounded UTF-8 JSON storage, without parsing or establishing approval.
    /// The authority owner validates its closed schema before calling this.
    pub fn reader_authority_put_bounded(
        &self,
        key: &str,
        value: &[u8],
        max: usize,
    ) -> StorageResult<()> {
        key_and_limit(key, max)?;
        if value.is_empty() || value.len() > max {
            return Err(StorageError::Invalid(
                "reader authority value budget".into(),
            ));
        }
        let value = std::str::from_utf8(value)
            .map_err(|_| StorageError::Invalid("reader authority value encoding".into()))?;
        self.c().execute(
            "INSERT OR REPLACE INTO local_settings (key, value, updated_at) VALUES (?1, ?2, ?3)",
            params![key, value, ts(&now())],
        )?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::DatabaseKey;
    const KEY: &str = "cc.native.reader.root.v1/11111111-1111-4111-8111-111111111111";

    #[test]
    fn reader_authority_full_commit_restores_policy_and_survives_reopen() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("reader.db");
        let key = DatabaseKey::from_bytes([7; 32]);
        {
            let mut db = Database::open(&path, &key).unwrap();
            let before: i64 = db
                .conn
                .query_row("PRAGMA synchronous", [], |r| r.get(0))
                .unwrap();
            assert_eq!(before, 1);
            db.reader_authority_write::<_, StorageError>(|tx| {
                let actual: i64 = tx.c().query_row("PRAGMA synchronous", [], |r| r.get(0))?;
                assert_eq!(actual, 2);
                assert!(tx.reader_authority_get_bounded(KEY, 64)?.is_none());
                tx.reader_authority_put_bounded(KEY, br#"{"revision":1}"#, 64)
            })
            .unwrap();
            let after: i64 = db
                .conn
                .query_row("PRAGMA synchronous", [], |r| r.get(0))
                .unwrap();
            assert_eq!(after, before);
        }
        let mut db = Database::open(&path, &key).unwrap();
        assert_eq!(
            db.read(|tx| tx.reader_authority_get_bounded(KEY, 64))
                .unwrap()
                .unwrap(),
            br#"{"revision":1}"#
        );
    }

    #[test]
    fn reader_authority_rejection_rolls_back_and_disabled_sync_never_calls_closure() {
        let mut db = Database::open_in_memory(&DatabaseKey::from_bytes([8; 32])).unwrap();
        let result = db.reader_authority_write::<(), StorageError>(|tx| {
            tx.reader_authority_put_bounded(KEY, b"1", 16)?;
            Err(StorageError::Invalid("owned stale revision".into()))
        });
        assert!(matches!(
            result,
            Err(ReaderAuthorityWriteError::Rejected(_))
        ));
        assert!(db
            .read(|tx| tx.reader_authority_get_bounded(KEY, 16))
            .unwrap()
            .is_none());
        db.conn.pragma_update(None, "synchronous", "OFF").unwrap();
        let mut called = false;
        let result = db.reader_authority_write::<(), StorageError>(|_| {
            called = true;
            Ok(())
        });
        assert!(!called);
        assert!(matches!(
            result,
            Err(ReaderAuthorityWriteError::BeforeWrite(_))
        ));
    }

    #[test]
    fn reader_authority_file_off_and_memory_journals_never_call_closure() {
        for mode in ["OFF", "MEMORY"] {
            let temp = tempfile::tempdir().unwrap();
            let mut db = Database::open(
                temp.path().join("reader.db"),
                &DatabaseKey::from_bytes([11; 32]),
            )
            .unwrap();
            db.conn.pragma_update(None, "journal_mode", mode).unwrap();
            let mut called = false;
            let result = db.reader_authority_write::<(), StorageError>(|_| {
                called = true;
                Ok(())
            });
            assert!(!called, "unsafe journal must reject before closure");
            assert!(matches!(
                result,
                Err(ReaderAuthorityWriteError::BeforeWrite(_))
            ));
        }
        let mut db = Database::open_in_memory(&DatabaseKey::from_bytes([12; 32])).unwrap();
        db.conn.pragma_update(None, "journal_mode", "OFF").unwrap();
        let result = db.reader_authority_write::<(), StorageError>(|_| panic!("OFF closure ran"));
        assert!(matches!(
            result,
            Err(ReaderAuthorityWriteError::BeforeWrite(_))
        ));
    }

    #[test]
    fn reader_authority_preserves_extra_inside_commit_and_afterwards() {
        let temp = tempfile::tempdir().unwrap();
        let mut db = Database::open(
            temp.path().join("reader.db"),
            &DatabaseKey::from_bytes([13; 32]),
        )
        .unwrap();
        db.conn.pragma_update(None, "synchronous", "EXTRA").unwrap();
        db.reader_authority_write::<(), StorageError>(|tx| {
            let actual: i64 = tx.c().query_row("PRAGMA synchronous", [], |r| r.get(0))?;
            assert_eq!(actual, 3);
            tx.reader_authority_put_bounded(KEY, b"1", 16)
        })
        .unwrap();
        let restored: i64 = db
            .conn
            .query_row("PRAGMA synchronous", [], |r| r.get(0))
            .unwrap();
        assert_eq!(restored, 3);
    }

    #[test]
    fn reader_authority_bounded_corrupt_row_and_namespace_reject_without_public_value() {
        let mut db = Database::open_in_memory(&DatabaseKey::from_bytes([9; 32])).unwrap();
        db.conn
            .execute(
                "INSERT INTO local_settings (key,value,updated_at) VALUES (?1,?2,?3)",
                params![KEY, "x".repeat(1024 * 1024), ts(&now())],
            )
            .unwrap();
        assert!(db
            .read(|tx| tx.reader_authority_get_bounded(KEY, 32))
            .is_err());
        db.reader_authority_write::<_, StorageError>(|tx| {
            assert!(tx.reader_authority_get_bounded("foreign", 32).is_err());
            assert!(tx.reader_authority_put_bounded(KEY, &[0xff], 32).is_err());
            assert!(tx.reader_authority_put_bounded(KEY, &[0; 33], 32).is_err());
            assert!(tx
                .reader_authority_get_bounded(KEY, 8 * 1024 * 1024 + 1)
                .is_err());
            Ok(())
        })
        .unwrap();
    }

    #[test]
    fn reader_authority_two_connections_have_one_compare_and_swap_winner() {
        use std::sync::{Arc, Barrier};
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("reader.db");
        {
            let mut db = Database::open(&path, &DatabaseKey::from_bytes([10; 32])).unwrap();
            db.reader_authority_write::<_, StorageError>(|tx| {
                tx.reader_authority_put_bounded(KEY, b"0", 16)
            })
            .unwrap();
        }
        let ready = Arc::new(Barrier::new(2));
        let workers: Vec<_> = (0..2)
            .map(|_| {
                let path = path.clone();
                let ready = ready.clone();
                std::thread::spawn(move || {
                    let mut db = Database::open(path, &DatabaseKey::from_bytes([10; 32])).unwrap();
                    ready.wait();
                    db.reader_authority_write::<_, StorageError>(|tx| {
                        if tx.reader_authority_get_bounded(KEY, 16)?.as_deref() != Some(b"0") {
                            return Err(StorageError::Invalid("owned stale revision".into()));
                        }
                        tx.reader_authority_put_bounded(KEY, b"1", 16)
                    })
                })
            })
            .collect();
        let outcomes: Vec<_> = workers.into_iter().map(|w| w.join().unwrap()).collect();
        assert_eq!(outcomes.iter().filter(|r| r.is_ok()).count(), 1);
        assert_eq!(
            outcomes
                .iter()
                .filter(|r| matches!(r, Err(ReaderAuthorityWriteError::Rejected(_))))
                .count(),
            1
        );
    }
}
