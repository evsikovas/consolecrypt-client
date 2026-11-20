//! Project catalog acknowledgement over the SAME validated FULL kernel.
//! No new connection/DB/outbox, public raw SQL, journal toggle or authority flag.
use super::{Database, ReaderAuthorityWriteError, Tx};
use crate::StorageError;
pub enum ProjectPublicationWriteError<E> {
    BeforeWrite(StorageError),
    Rejected(E),
    Unknown,
}
impl<E> std::fmt::Debug for ProjectPublicationWriteError<E> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::BeforeWrite(_) => "ProjectPublicationWriteError::BeforeWrite",
            Self::Rejected(_) => "ProjectPublicationWriteError::Rejected",
            Self::Unknown => "ProjectPublicationWriteError::Unknown",
        })
    }
}
impl<E> std::fmt::Display for ProjectPublicationWriteError<E> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::Debug::fmt(self, f)
    }
}
impl<E> std::error::Error for ProjectPublicationWriteError<E> {}
impl<E> From<StorageError> for ProjectPublicationWriteError<E> {
    fn from(_: StorageError) -> Self {
        Self::Unknown
    }
}
impl Database {
    /// Native publication accepts only a real file-backed supported journal.
    /// FULL is SQLite's acknowledgement under OS/storage flush assumptions,
    /// not a claim that unit tests simulate a real machine power cut.
    pub fn project_publication_write<T, E>(
        &mut self,
        f: impl FnOnce(&Tx<'_>) -> Result<T, E>,
    ) -> Result<T, ProjectPublicationWriteError<E>>
    where
        E: From<StorageError>,
    {
        self.durable_write_kernel(true, f).map_err(|e| match e {
            ReaderAuthorityWriteError::BeforeWrite(e) => {
                ProjectPublicationWriteError::BeforeWrite(e)
            }
            ReaderAuthorityWriteError::Rejected(e) => ProjectPublicationWriteError::Rejected(e),
            ReaderAuthorityWriteError::Unknown => ProjectPublicationWriteError::Unknown,
        })
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::{DatabaseKey, Storage};
    const KEY: &str = "automation.projects.v1";
    #[test]
    fn file_backed_project_ack_is_at_least_full_restores_stronger_policy_and_survives_reopen() {
        let owned = tempfile::tempdir().unwrap();
        let key = DatabaseKey::from_bytes([23; 32]);
        for previous in [1i64, 2, 3] {
            let path = owned.path().join(format!("owned-project-{previous}.db"));
            {
                let mut db = Database::open(&path, &key).unwrap();
                db.conn
                    .pragma_update(None, "synchronous", previous)
                    .unwrap();
                db.project_publication_write::<_, StorageError>(|tx| {
                    let actual: i64 = tx.c().query_row("PRAGMA synchronous", [], |r| r.get(0))?;
                    assert_eq!(actual, previous.max(2));
                    tx.setting_set(
                        KEY,
                        &serde_json::json!({"version":1,"records":{},"nativeAck":previous}),
                    )
                })
                .unwrap();
                let restored: i64 = db
                    .conn
                    .query_row("PRAGMA synchronous", [], |r| r.get(0))
                    .unwrap();
                assert_eq!(restored, previous);
            }
            let mut db = Database::open(&path, &key).unwrap();
            let row: serde_json::Value = db.read(|tx| tx.setting_get(KEY)).unwrap().unwrap();
            assert_eq!(row["nativeAck"], previous);
        }
    }
    #[test]
    fn project_memory_refusal_keeps_reader_process_atomic_contract_unchanged() {
        let mut db = Database::open_in_memory(&DatabaseKey::from_bytes([24; 32])).unwrap();
        let mut called = false;
        let outcome = db.project_publication_write::<(), StorageError>(|_| {
            called = true;
            Ok(())
        });
        assert!(!called);
        assert!(matches!(
            outcome,
            Err(ProjectPublicationWriteError::BeforeWrite(_))
        ));
        db.reader_authority_write::<_, StorageError>(|tx| {
            tx.setting_set("owned-reader-regression", &7)
        })
        .unwrap();
        let value: Option<i32> = db
            .read(|tx| tx.setting_get("owned-reader-regression"))
            .unwrap();
        assert_eq!(value, Some(7));
    }
    #[test]
    fn unsafe_journal_and_disabled_sync_deny_before_callback_and_rejection_rolls_back() {
        let owned = tempfile::tempdir().unwrap();
        let key = DatabaseKey::from_bytes([25; 32]);
        for mode in ["OFF", "MEMORY"] {
            let mut db =
                Database::open(&owned.path().join(format!("owned-{mode}.db")), &key).unwrap();
            db.conn.pragma_update(None, "journal_mode", mode).unwrap();
            let mut called = false;
            let outcome = db.project_publication_write::<(), StorageError>(|_| {
                called = true;
                Ok(())
            });
            assert!(!called);
            assert!(matches!(
                outcome,
                Err(ProjectPublicationWriteError::BeforeWrite(_))
            ));
        }
        let mut db = Database::open(&owned.path().join("owned-rejected.db"), &key).unwrap();
        let outcome = db.project_publication_write::<(), StorageError>(|tx| {
            tx.setting_set(KEY, &"uncommitted")?;
            Err(StorageError::Invalid("synthetic stale CAS".into()))
        });
        assert!(matches!(
            outcome,
            Err(ProjectPublicationWriteError::Rejected(_))
        ));
        let row: Option<String> = db.read(|tx| tx.setting_get(KEY)).unwrap();
        assert!(row.is_none());
        db.conn.pragma_update(None, "synchronous", 0).unwrap();
        let mut called = false;
        let outcome = db.project_publication_write::<(), StorageError>(|_| {
            called = true;
            Ok(())
        });
        assert!(!called);
        assert!(matches!(
            outcome,
            Err(ProjectPublicationWriteError::BeforeWrite(_))
        ));
    }
    #[tokio::test]
    async fn project_storage_wrapper_uses_existing_worker_and_actual_full_commit_ack() {
        let owned = tempfile::tempdir().unwrap();
        let storage = Storage::open(
            owned.path().join("owned-project-worker.db"),
            DatabaseKey::from_bytes([26; 32]),
        )
        .await
        .unwrap();
        let result = storage
            .write_project_publication::<_, StorageError, _>(|tx| {
                let synchronous: i64 = tx.c().query_row("PRAGMA synchronous", [], |r| r.get(0))?;
                assert!(synchronous >= 2);
                tx.setting_set(KEY, &42)?;
                Ok(42)
            })
            .await
            .unwrap();
        assert_eq!(result, 42);
        let row: Option<i32> = storage.read(|tx| tx.setting_get(KEY)).await.unwrap();
        assert_eq!(row, Some(42));
        storage.close().await;
    }
}
