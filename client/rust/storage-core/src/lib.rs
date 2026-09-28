//! # cc-storage-core — local encrypted storage
//!
//! One SQLCipher database per profile (CLIENT_ARCHITECTURE §4):
//!
//! * `objects` — vault objects exactly as synced: per-object E2EE ciphertext
//!   (never decrypted payloads), revision/sequence, tombstones, local sync
//!   state (`synced | pending | conflict`);
//! * `remote_objects` — server state stashed for objects with unpushed edits;
//! * `outbox` — offline mutation queue with stable `mutation_id`s;
//! * `sync_state` — per-vault cursors (`last_sequence`, snapshot progress,
//!   vault epoch, rollback-recovery progress);
//! * `recovery_remote` — server listing held while recovering from a server
//!   rollback;
//! * `profile`, `vaults` — profile kind (`local` | `synced`, ADR-0106),
//!   account/vault metadata and envelopes (the only copy in local profiles,
//!   a cache for offline unlock in synced ones);
//! * `known_hosts`, `terminal_sessions`, `local_settings` — local-only data.
//!
//! The database key is supplied by the caller (32 random bytes kept in the OS
//! secure store); this crate never generates or persists it.
//!
//! ## Usage
//!
//! Blocking code uses [`Database`]; async code uses [`Storage`], which runs
//! every operation on a dedicated thread. All repository methods live on
//! [`Tx`], so multi-step operations are atomic:
//!
//! ```no_run
//! # async fn demo() -> Result<(), cc_storage_core::StorageError> {
//! use cc_storage_core::{DatabaseKey, Storage};
//! let storage = Storage::open("/tmp/vault.db", DatabaseKey::from_bytes([7; 32])).await?;
//! let vaults = storage.read(|tx| tx.list_vaults()).await?;
//! # let _ = vaults; Ok(()) }
//! ```
//!
//! The composite operations in [`sync_ops`] implement the storage side of the
//! ADR-0003 client algorithm (local mutation → objects + outbox in one
//! transaction; apply pulled page + advance cursor in one transaction; push
//! results; conflict resolution); [`Tx::begin_rollback_recovery`] /
//! [`Tx::record_recovery_page`] / [`Tx::reconcile_rollback`] recover from a
//! server restored from an older backup (ADR-0103 addendum).
//!
//! ## Profiles (ADR-0106)
//!
//! One database per profile; [`ProfileDirectory`] manages the layout and a
//! non-secret index. In a `Local` profile local mutations get local
//! revisions and `LocalOnly` state and the outbox is never written;
//! [`Tx::attach_to_server`] / [`Tx::detach_to_local`] convert between the
//! kinds, [`Tx::export_vault`] / [`Tx::import_vault`] are the storage side of
//! `.ccbackup` files.

mod db;
mod error;
mod key;
mod local_mode;
mod migrations;
mod model;
mod profiles;
mod repo;
mod rollback;
mod sql;
pub mod sync_ops;
mod worker;

pub use db::{Database, Tx};
pub use error::{Result, StorageError};
pub use key::DatabaseKey;
pub use local_mode::{AttachMode, AttachSummary, DetachSummary, ImportSummary};
pub use migrations::SCHEMA_VERSION;
pub use model::{
    local_key_envelope, ExportedObject, KnownHostRecord, LocalState, OutboxCounts, OutboxEntry,
    OutboxOp, OutboxState, Profile, ProfileId, ProfileKind, RemoteObject, RollbackReason,
    RollbackRecovery, StoredObject, SyncCursor, TerminalSessionRecord, VaultExport, VaultRecord,
};
pub use profiles::{ProfileDirectory, ProfileEntry, PROFILE_DB_FILE};
pub use rollback::{ReconcileCodec, ReconcileOutcome, RecoveryPage};
pub use sync_ops::{
    ConflictContext, ConflictCopy, LocalMutationOutcome, PageCursor, PageOutcome, PushOutcome,
    RemoteApplyOutcome, RemoteChange, Resolution, ResolutionOutcome,
};
pub use worker::Storage;
