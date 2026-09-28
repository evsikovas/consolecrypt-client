//! Async facade: a dedicated OS thread owns the [`Database`]; async callers
//! send closures to it and await the result, so blocking SQLite work never
//! runs on a tokio worker thread.

use crate::db::{Database, Tx};
use crate::error::{Result, StorageError};
use crate::key::DatabaseKey;
use crate::model::{KnownHostRecord, Profile, VaultRecord};
use cc_protocol::envelopes::KeyEnvelope;
use cc_protocol::vaults::VaultInfo;
use cc_protocol::VaultId;
use serde::de::DeserializeOwned;
use serde::Serialize;
use std::panic::AssertUnwindSafe;
use std::path::PathBuf;
use std::sync::mpsc;
use tokio::sync::oneshot;

type Job = Box<dyn FnOnce(&mut Database) + Send + 'static>;

/// Message to the storage thread.
enum Msg {
    Job(Job),
    /// Close the connection now and stop the thread (acknowledged once the
    /// database is closed).
    Close(oneshot::Sender<()>),
}

/// Cloneable async handle to the local database.
///
/// All operations are serialized on one connection (SQLite writes are
/// serialized anyway), which also makes read-modify-write closures atomic
/// with respect to each other. The thread exits when the last handle drops
/// or after [`Storage::close`].
#[derive(Clone)]
pub struct Storage {
    jobs: mpsc::Sender<Msg>,
    path: Option<PathBuf>,
}

impl std::fmt::Debug for Storage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Storage")
            .field("path", &self.path)
            .finish_non_exhaustive()
    }
}

impl Storage {
    /// Open (or create) the database file on a new storage thread.
    pub async fn open(path: impl Into<PathBuf>, key: DatabaseKey) -> Result<Self> {
        let path = path.into();
        let p = path.clone();
        Self::spawn(move || Database::open(&p, &key), Some(path)).await
    }

    /// Encrypted in-memory database on a new storage thread.
    pub async fn open_in_memory(key: DatabaseKey) -> Result<Self> {
        Self::spawn(move || Database::open_in_memory(&key), None).await
    }

    async fn spawn(
        open: impl FnOnce() -> Result<Database> + Send + 'static,
        path: Option<PathBuf>,
    ) -> Result<Self> {
        let (jobs, rx) = mpsc::channel::<Msg>();
        let (ready_tx, ready_rx) = oneshot::channel::<Result<()>>();
        std::thread::Builder::new()
            .name("cc-storage".into())
            .spawn(move || {
                let mut db = match open() {
                    Ok(db) => {
                        let _ = ready_tx.send(Ok(()));
                        db
                    }
                    Err(e) => {
                        let _ = ready_tx.send(Err(e));
                        return;
                    }
                };
                while let Ok(msg) = rx.recv() {
                    match msg {
                        Msg::Job(job) => {
                            // A panicking closure must not take the storage
                            // thread down; its open transaction is rolled
                            // back on unwind.
                            if std::panic::catch_unwind(AssertUnwindSafe(|| job(&mut db))).is_err()
                            {
                                tracing::error!("storage job panicked");
                            }
                        }
                        Msg::Close(ack) => {
                            drop(db);
                            let _ = ack.send(());
                            return;
                        }
                    }
                }
            })?;
        ready_rx.await.map_err(|_| StorageError::Closed)??;
        Ok(Self { jobs, path })
    }

    /// Path of the database file (`None` for in-memory).
    pub fn path(&self) -> Option<&std::path::Path> {
        self.path.as_deref()
    }

    /// Close the database now (after already queued operations) and stop
    /// the storage thread; waits until the connection is closed. Later calls
    /// through any clone of this handle fail with [`StorageError::Closed`].
    ///
    /// Call this before process exit: closing a SQLCipher connection on a
    /// background thread while the process runs its exit handlers (OpenSSL
    /// cleanup) can crash.
    pub async fn close(&self) {
        let (tx, rx) = oneshot::channel();
        if self.jobs.send(Msg::Close(tx)).is_ok() {
            let _ = rx.await;
        }
    }

    /// Run `f` with exclusive access to the database on the storage thread.
    pub async fn call<T, E, F>(&self, f: F) -> Result<T, E>
    where
        F: FnOnce(&mut Database) -> Result<T, E> + Send + 'static,
        T: Send + 'static,
        E: From<StorageError> + Send + 'static,
    {
        let (tx, rx) = oneshot::channel();
        let job: Job = Box::new(move |db| {
            let _ = tx.send(f(db));
        });
        self.jobs
            .send(Msg::Job(job))
            .map_err(|_| E::from(StorageError::Closed))?;
        rx.await.map_err(|_| E::from(StorageError::Closed))?
    }

    /// Run `f` in a write transaction (commit on `Ok`).
    pub async fn write<T, E, F>(&self, f: F) -> Result<T, E>
    where
        F: FnOnce(&Tx<'_>) -> Result<T, E> + Send + 'static,
        T: Send + 'static,
        E: From<StorageError> + Send + 'static,
    {
        self.call(move |db| db.write(f)).await
    }

    /// Run `f` in a read transaction.
    pub async fn read<T, E, F>(&self, f: F) -> Result<T, E>
    where
        F: FnOnce(&Tx<'_>) -> Result<T, E> + Send + 'static,
        T: Send + 'static,
        E: From<StorageError> + Send + 'static,
    {
        self.call(move |db| db.read(f)).await
    }

    // ---- convenience wrappers ------------------------------------------------

    /// See [`Tx::get_profile`].
    pub async fn get_profile(&self) -> Result<Option<Profile>> {
        self.read(|tx| tx.get_profile()).await
    }

    /// See [`Tx::put_profile`].
    pub async fn put_profile(&self, profile: Profile) -> Result<()> {
        self.write(move |tx| tx.put_profile(&profile)).await
    }

    /// See [`Tx::list_vaults`].
    pub async fn list_vaults(&self) -> Result<Vec<VaultRecord>> {
        self.read(|tx| tx.list_vaults()).await
    }

    /// See [`Tx::get_vault`].
    pub async fn get_vault(&self, vault_id: VaultId) -> Result<Option<VaultRecord>> {
        self.read(move |tx| tx.get_vault(vault_id)).await
    }

    /// See [`Tx::upsert_vault_info`].
    pub async fn upsert_vault_info(&self, info: VaultInfo) -> Result<VaultRecord> {
        self.write(move |tx| tx.upsert_vault_info(&info)).await
    }

    /// See [`Tx::cache_vault_envelopes`].
    pub async fn cache_vault_envelopes(
        &self,
        vault_id: VaultId,
        password: Option<KeyEnvelope>,
        recovery: Option<KeyEnvelope>,
        device: Option<KeyEnvelope>,
    ) -> Result<()> {
        self.write(move |tx| {
            tx.cache_vault_envelopes(
                vault_id,
                password.as_ref(),
                recovery.as_ref(),
                device.as_ref(),
            )
        })
        .await
    }

    /// See [`Tx::setting_get`].
    pub async fn setting_get<T: DeserializeOwned + Send + 'static>(
        &self,
        key: impl Into<String>,
    ) -> Result<Option<T>> {
        let key = key.into();
        self.read(move |tx| tx.setting_get(&key)).await
    }

    /// See [`Tx::setting_set`].
    pub async fn setting_set<T: Serialize + Send + 'static>(
        &self,
        key: impl Into<String>,
        value: T,
    ) -> Result<()> {
        let key = key.into();
        self.write(move |tx| tx.setting_set(&key, &value)).await
    }

    /// See [`Tx::known_hosts_for`].
    pub async fn known_hosts_for(
        &self,
        host_pattern: impl Into<String>,
    ) -> Result<Vec<KnownHostRecord>> {
        let p = host_pattern.into();
        self.read(move |tx| tx.known_hosts_for(&p)).await
    }

    /// See [`Tx::known_host_upsert`].
    pub async fn known_host_upsert(&self, rec: KnownHostRecord) -> Result<i64> {
        self.write(move |tx| tx.known_host_upsert(&rec)).await
    }
}
