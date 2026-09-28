//! Events emitted by [`crate::ObjectStore`] / [`crate::SyncEngine`] so the
//! app can refresh decrypted caches, search indexes and the UI. Events carry
//! metadata only — re-read (decrypt) the object through the store.
//!
//! The channel is bounded (1024); a subscriber that gets
//! `RecvError::Lagged` (e.g. during a large snapshot) should reload its
//! cache with [`crate::ObjectStore::list`].

use crate::error::StopReason;
use cc_models::KekClass;
use cc_protocol::{ObjectId, VaultId};
use cc_storage_core::RollbackReason;

/// Who changed an object.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ChangeOrigin {
    /// A local mutation on this device.
    Local,
    /// Applied from the server (pull, snapshot, or a newer state found
    /// while confirming our push).
    Remote,
    /// Written by automatic conflict resolution.
    ConflictResolution,
}

/// How a conflict was resolved (ADR-0003 policy table).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ConflictResolution {
    /// Remote kept in place; local version saved as a new conflict-copy
    /// object (secrets and inventory/snippets/notes/tunnels).
    RemoteKeptLocalCopied,
    /// Remote kept; local change discarded (LWW remote newer, or local
    /// delete vs remote edit).
    RemoteKept,
    /// Local version re-applied on top of remote (LWW local newer).
    LocalKept,
    /// Remote tombstone vs local edit: the edit resurrects the object.
    LocalEditResurrected,
    /// Deleted on both sides.
    BothDeleted,
    /// Local and remote payloads were identical.
    Identical,
    /// The server does not know the object; the local deletion is final.
    LocalDropped,
}

/// Something happened in a vault.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SyncEvent {
    /// The stored state of an object changed; re-read it.
    ObjectChanged {
        vault_id: VaultId,
        object_id: ObjectId,
        revision: i64,
        deleted: bool,
        origin: ChangeOrigin,
    },
    /// The object no longer exists locally (unsent create deleted, or
    /// removed by a re-snapshot).
    ObjectRemoved {
        vault_id: VaultId,
        object_id: ObjectId,
    },
    /// A conflict was resolved automatically; notify the user for secrets
    /// and conflict copies.
    ConflictResolved {
        vault_id: VaultId,
        object_id: ObjectId,
        resolution: ConflictResolution,
        /// New object holding the local version, if one was created.
        conflict_copy: Option<ObjectId>,
        /// Class of the local version (e.g. `Secrets` → warn the user).
        kek_class: Option<KekClass>,
    },
    /// Server ciphertext failed verification; stored as received but it
    /// cannot be decrypted with this vault's keys.
    IntegrityWarning {
        vault_id: VaultId,
        object_id: ObjectId,
        revision: i64,
    },
    /// Initial (or re-) snapshot finished — also emitted when a server
    /// rollback recovery completed. Reload caches from the store.
    SnapshotCompleted { vault_id: VaultId },
    /// The server lost state this device had seen (restored from an older
    /// backup, protocol 1.3 epoch change, or sequences going backwards). The
    /// engine re-downloads the vault and re-uploads local objects the server
    /// lacks; [`crate::SyncStatus::rollback_recovery`] stays set until that
    /// is done (then `SnapshotCompleted` follows). Tell the user: edits made
    /// on other devices after the backup may reappear as conflict copies.
    ServerRollbackDetected {
        vault_id: VaultId,
        reason: RollbackReason,
    },
    /// The engine stopped for good.
    Stopped {
        vault_id: VaultId,
        reason: StopReason,
    },
}
