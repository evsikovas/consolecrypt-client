//! Records stored in the local database.

use cc_models::known_host::KnownHostSource;
use cc_models::KekClass;
use cc_protocol::envelopes::KeyEnvelope;
use cc_protocol::sync::{EncryptedBody, Mutation, MutationOp};
use cc_protocol::vaults::{VaultRole, VaultState};
use cc_protocol::{DeviceId, MutationId, ObjectId, Timestamp, UserId, VaultId};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Sync status of a locally stored object.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LocalState {
    /// Local state equals the latest server state we know of.
    Synced,
    /// Local change waiting in the outbox.
    Pending,
    /// The server rejected our change with a revision conflict and the
    /// conflict is not resolved yet (e.g. the vault was locked).
    Conflict,
    /// Local profile (ADR-0106): the object lives only on this machine.
    /// `revision` is a local revision; nothing is queued for sync.
    LocalOnly,
}

impl LocalState {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            LocalState::Synced => "synced",
            LocalState::Pending => "pending",
            LocalState::Conflict => "conflict",
            LocalState::LocalOnly => "local_only",
        }
    }

    pub(crate) fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "synced" => LocalState::Synced,
            "pending" => LocalState::Pending,
            "conflict" => LocalState::Conflict,
            "local_only" => LocalState::LocalOnly,
            _ => return None,
        })
    }
}

/// A vault object as stored locally (ciphertext only).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredObject {
    pub vault_id: VaultId,
    pub object_id: ObjectId,
    /// Revision the stored ciphertext is bound to (object AAD).
    pub revision: i64,
    /// Latest revision confirmed by the server (0 = never reached the server).
    pub server_revision: i64,
    /// Server sequence of `server_revision`, if known.
    pub sequence: Option<i64>,
    pub deleted: bool,
    /// `None` iff `deleted`.
    pub body: Option<EncryptedBody>,
    /// Cached KEK class (skips trial unwrapping, ADR-0002). Not authoritative.
    pub kek_class_hint: Option<KekClass>,
    pub local_state: LocalState,
    /// Set on conflict copies: the object this copy was made from.
    pub conflict_origin: Option<ObjectId>,
    /// Device that wrote revision `server_revision` (from the server for
    /// pulled states; this device for its own accepted pushes).
    pub writer_device_id: Option<DeviceId>,
    pub updated_at: Timestamp,
}

/// Latest server state of an object that has local unpushed changes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteObject {
    pub vault_id: VaultId,
    pub object_id: ObjectId,
    pub revision: i64,
    pub sequence: i64,
    pub deleted: bool,
    pub body: Option<EncryptedBody>,
    pub kek_class_hint: Option<KekClass>,
    pub writer_device_id: Option<DeviceId>,
    pub updated_at: Timestamp,
}

/// Kind of queued mutation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OutboxOp {
    Put,
    Delete,
}

impl OutboxOp {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            OutboxOp::Put => "put",
            OutboxOp::Delete => "delete",
        }
    }
}

/// State of an outbox entry. Accepted entries are removed from the outbox.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OutboxState {
    /// Waiting to be pushed (or re-pushed with the same `mutation_id`).
    Queued,
    /// Server answered `Conflict`; waiting for client-side resolution.
    Conflict,
    /// Permanently rejected (validation); needs a new local edit or user action.
    Failed,
}

impl OutboxState {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            OutboxState::Queued => "queued",
            OutboxState::Conflict => "conflict",
            OutboxState::Failed => "failed",
        }
    }
}

/// One queued mutation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutboxEntry {
    /// FIFO position.
    pub ordering: i64,
    /// Idempotency key; reused on every retry of this mutation.
    pub mutation_id: MutationId,
    pub vault_id: VaultId,
    pub object_id: ObjectId,
    pub base_revision: i64,
    pub op: OutboxOp,
    /// Ciphertext for `base_revision + 1`; `None` for deletes.
    pub body: Option<EncryptedBody>,
    pub state: OutboxState,
    /// How many times this entry was handed to a push (outcome may be unknown).
    pub attempts: u32,
    pub last_error: Option<String>,
    /// For `Conflict`: server's current revision / tombstone flag.
    pub conflict_revision: Option<i64>,
    pub conflict_sequence: Option<i64>,
    pub conflict_deleted: Option<bool>,
    pub created_at: Timestamp,
    pub updated_at: Timestamp,
}

impl OutboxEntry {
    /// Wire form for `POST /v1/sync/push`.
    pub fn to_mutation(&self) -> Mutation {
        let op = match (&self.op, &self.body) {
            (OutboxOp::Put, Some(body)) => MutationOp::Put { body: body.clone() },
            // The table CHECK guarantees put ⇒ body; delete never has one.
            _ => MutationOp::Delete,
        };
        Mutation {
            mutation_id: self.mutation_id,
            object_id: self.object_id,
            base_revision: self.base_revision,
            op,
        }
    }

    /// Approximate JSON size on the wire (base64 expansion + overhead).
    pub fn approx_wire_size(&self) -> usize {
        let raw = self.body.as_ref().map_or(0, |b| {
            b.ciphertext.len() + b.nonce.len() + b.wrapped_dek.len() + b.wrapped_dek_nonce.len()
        });
        raw.div_ceil(3) * 4 + 256
    }
}

/// Counts of outbox entries by state for one vault.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct OutboxCounts {
    pub queued: u64,
    pub conflict: u64,
    pub failed: u64,
}

impl OutboxCounts {
    /// All entries not yet accepted by the server.
    pub fn total(&self) -> u64 {
        self.queued + self.conflict + self.failed
    }
}

/// Why the client concluded that the server lost state it had confirmed
/// before (restored from a backup / rolled back). ADR-0103 addendum.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RollbackReason {
    /// The vault epoch (protocol 1.3) differs from the one stored locally —
    /// the operator rotated it after a restore.
    EpochChanged,
    /// The server's `latest_sequence` is below what the client already
    /// pulled.
    SequenceRegressed,
    /// A push was accepted with a sequence the client already holds for
    /// another object (sequences are never reused on a healthy server).
    SequenceReused,
}

impl RollbackReason {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            RollbackReason::EpochChanged => "epoch_changed",
            RollbackReason::SequenceRegressed => "sequence_regressed",
            RollbackReason::SequenceReused => "sequence_reused",
        }
    }

    pub(crate) fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "epoch_changed" => RollbackReason::EpochChanged,
            "sequence_regressed" => RollbackReason::SequenceRegressed,
            "sequence_reused" => RollbackReason::SequenceReused,
            _ => return None,
        })
    }
}

impl std::fmt::Display for RollbackReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            RollbackReason::EpochChanged => "vault epoch changed (server restored)",
            RollbackReason::SequenceRegressed => "server sequence went backwards",
            RollbackReason::SequenceReused => "server reused a sequence number",
        })
    }
}

/// Progress of a server-rollback recovery (persisted, resumable).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RollbackRecovery {
    pub reason: RollbackReason,
    pub detected_at: Timestamp,
    /// Epoch the recovery listing runs against (`None` until a page reports
    /// one, or for pre-1.3 servers). Becomes [`SyncCursor::epoch`] when the
    /// recovery completes.
    pub epoch: Option<Uuid>,
    /// Next `changes?after=` of the full listing.
    pub cursor: i64,
    /// The listing is complete; only the reconcile step is left.
    pub listed: bool,
    /// Highest `latest_sequence` reported by the listing.
    pub latest_sequence: i64,
}

/// Per-vault sync cursor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SyncCursor {
    pub vault_id: VaultId,
    /// `changes?after=` cursor: every change with sequence ≤ this is applied.
    pub last_sequence: i64,
    pub snapshot_complete: bool,
    /// Next `snapshot?cursor=` while a snapshot is in progress.
    pub snapshot_cursor: Option<i64>,
    /// `latest_sequence` reported by the first snapshot page; becomes
    /// `last_sequence` once the snapshot completes.
    pub snapshot_start_sequence: Option<i64>,
    /// Latest server sequence we have heard of (pull/push/WS).
    pub server_latest_sequence: i64,
    pub last_sync_at: Option<Timestamp>,
    pub last_error: Option<String>,
    /// Vault epoch (protocol 1.3) from the first page / `VaultInfo` that
    /// reported one; `None` = unknown.
    pub epoch: Option<Uuid>,
    /// Set while a server-rollback recovery is in progress.
    pub recovery: Option<RollbackRecovery>,
}

impl SyncCursor {
    pub(crate) fn new(vault_id: VaultId) -> Self {
        Self {
            vault_id,
            last_sequence: 0,
            snapshot_complete: false,
            snapshot_cursor: None,
            snapshot_start_sequence: None,
            server_latest_sequence: 0,
            last_sync_at: None,
            last_error: None,
            epoch: None,
            recovery: None,
        }
    }

    /// Whether a server response (page, `VaultInfo`) carrying `epoch` and
    /// `latest_sequence` proves the server lost state this client already
    /// holds. An unknown epoch on either side never triggers.
    pub fn rollback_evidence(
        &self,
        epoch: Option<Uuid>,
        latest_sequence: i64,
    ) -> Option<RollbackReason> {
        if let (Some(known), Some(seen)) = (self.epoch, epoch) {
            if known != seen {
                return Some(RollbackReason::EpochChanged);
            }
        }
        if latest_sequence < self.last_sequence
            || latest_sequence < self.snapshot_start_sequence.unwrap_or(0)
        {
            return Some(RollbackReason::SequenceRegressed);
        }
        None
    }
}

/// Identifier of a local profile (one database file per profile).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ProfileId(pub Uuid);

impl ProfileId {
    /// New random (time-ordered) id.
    pub fn new() -> Self {
        Self(Uuid::now_v7())
    }
}

impl Default for ProfileId {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Display for ProfileId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::Display::fmt(&self.0, f)
    }
}

impl std::str::FromStr for ProfileId {
    type Err = uuid::Error;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Uuid::parse_str(s).map(Self)
    }
}

/// Profile kind (ADR-0106).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProfileKind {
    /// No server, no account; nothing is ever queued for sync.
    Local,
    /// Synced with a self-hosted server.
    Synced,
}

/// The profile this database belongs to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Profile {
    pub profile_id: ProfileId,
    pub kind: ProfileKind,
    /// User-visible name ("Personal", "Work").
    pub display_name: Option<String>,
    /// Self-hosted server base URL (required for `Synced`, `None` for `Local`).
    pub server_url: Option<String>,
    pub user_id: Option<UserId>,
    /// This installation's device id (also used for local OS unlock).
    pub device_id: DeviceId,
    pub email: Option<String>,
    pub created_at: Timestamp,
    pub updated_at: Timestamp,
}

impl Profile {
    /// A new local-only profile.
    pub fn new_local(
        profile_id: ProfileId,
        device_id: DeviceId,
        display_name: Option<String>,
    ) -> Self {
        let now = chrono::Utc::now();
        Self {
            profile_id,
            kind: ProfileKind::Local,
            display_name,
            server_url: None,
            user_id: None,
            device_id,
            email: None,
            created_at: now,
            updated_at: now,
        }
    }

    /// A new synced profile.
    pub fn new_synced(
        profile_id: ProfileId,
        device_id: DeviceId,
        server_url: impl Into<String>,
        user_id: Option<UserId>,
        email: Option<String>,
    ) -> Self {
        let now = chrono::Utc::now();
        Self {
            profile_id,
            kind: ProfileKind::Synced,
            display_name: None,
            server_url: Some(server_url.into()),
            user_id,
            device_id,
            email,
            created_at: now,
            updated_at: now,
        }
    }
}

/// A live object as exported for an encrypted backup (`.ccbackup`, ADR-0106).
/// Ciphertext only; the KEK class is deliberately not exported.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExportedObject {
    pub object_id: ObjectId,
    /// Revision the ciphertext is bound to (needed to decrypt).
    pub revision: i64,
    pub body: EncryptedBody,
    /// Import only: KEK class found by the restore flow's trial decryption
    /// (seeds the local cache). Never serialized/exported.
    #[serde(skip)]
    pub kek_class_hint: Option<cc_models::KekClass>,
}

/// Storage-side content of a vault backup: everything needed to restore on
/// another machine with the passphrase or Recovery Key. No device envelope,
/// no device keys, no plaintext. The container format (`.ccbackup` header,
/// versioning, app version, file encoding) is owned by vault-core.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VaultExport {
    pub vault_id: VaultId,
    pub password_envelope: Option<KeyEnvelope>,
    pub recovery_envelope: Option<KeyEnvelope>,
    pub objects: Vec<ExportedObject>,
    pub exported_at: Timestamp,
}

/// A vault known locally, with envelopes cached for offline unlock.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VaultRecord {
    pub vault_id: VaultId,
    pub role: VaultRole,
    pub state: VaultState,
    pub owner_user_id: Option<UserId>,
    /// Whether this device holds a device envelope (trusted, ADR-0004).
    pub caller_trusted: bool,
    pub server_latest_sequence: i64,
    pub password_envelope: Option<KeyEnvelope>,
    pub recovery_envelope: Option<KeyEnvelope>,
    pub device_envelope: Option<KeyEnvelope>,
    pub server_created_at: Option<Timestamp>,
    pub last_unlocked_at: Option<Timestamp>,
    pub updated_at: Timestamp,
}

/// Store a locally created envelope (local profiles, restored backups)
/// in the server-side [`KeyEnvelope`] shape with a locally generated id.
pub fn local_key_envelope(
    vault_id: VaultId,
    envelope: cc_protocol::envelopes::NewEnvelope,
    created_by_device_id: Option<DeviceId>,
) -> KeyEnvelope {
    KeyEnvelope {
        envelope_id: cc_protocol::EnvelopeId::new(),
        vault_id,
        recipient_type: envelope.recipient_type,
        recipient_id: envelope.recipient_id,
        kind: envelope.kind,
        metadata: envelope.metadata,
        ciphertext: envelope.ciphertext,
        nonce: envelope.nonce,
        created_at: chrono::Utc::now(),
        created_by_device_id,
        revoked_at: None,
    }
}

impl VaultRecord {
    /// Minimal record for a vault we only know the id and role of.
    pub fn new(vault_id: VaultId, role: VaultRole) -> Self {
        Self {
            vault_id,
            role,
            state: VaultState::Active,
            owner_user_id: None,
            caller_trusted: false,
            server_latest_sequence: 0,
            password_envelope: None,
            recovery_envelope: None,
            device_envelope: None,
            server_created_at: None,
            last_unlocked_at: None,
            updated_at: chrono::Utc::now(),
        }
    }
}

/// Local known-hosts cache entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KnownHostRecord {
    /// Local row id (`None` before insert).
    pub id: Option<i64>,
    pub host_pattern: String,
    pub key_type: String,
    pub public_key: String,
    pub fingerprint_sha256: String,
    pub source: KnownHostSource,
    pub revoked: bool,
    /// Link to the synced `KnownHost` vault object, if any.
    pub vault_id: Option<VaultId>,
    pub object_id: Option<ObjectId>,
    pub added_at: Timestamp,
    pub updated_at: Timestamp,
}

/// Terminal session metadata (local only).
#[derive(Debug, Clone, PartialEq)]
pub struct TerminalSessionRecord {
    pub session_id: Uuid,
    pub vault_id: Option<VaultId>,
    pub host_id: Option<ObjectId>,
    pub title: String,
    pub started_at: Timestamp,
    pub ended_at: Option<Timestamp>,
    pub exit_status: Option<i32>,
    /// Free-form JSON metadata (no secrets, no terminal content).
    pub metadata: serde_json::Value,
}
