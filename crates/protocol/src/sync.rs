//! Sync protocol (`/v1/sync/*`). See ADR-0003.
//!
//! * Each vault has a monotonic `sequence`, allocated by the server per
//!   accepted mutation. Pulls are ordered by sequence.
//! * Each object has a `revision` (1 on create, +1 per accepted mutation).
//!   Mutations carry `base_revision`; mismatch → conflict (optimistic
//!   concurrency). `base_revision = 0` means "create; must not exist".
//! * The server keeps only the latest version of each object (no history in
//!   MVP), so `changes?after=N` returns at most one entry per object: its
//!   latest state, if that state's sequence is > N.
//! * Deletions are tombstones (`deleted = true`, no body).
//! * Mutations are idempotent by `mutation_id`.

use crate::bytes::Bytes;
use crate::ids::{DeviceId, MutationId, ObjectId, VaultId};
use crate::Timestamp;
use serde::{Deserialize, Serialize};

/// Current object payload format (ADR-0002 §Objects).
pub const OBJECT_FORMAT_V1: u16 = 1;

/// Client-side encrypted object. Opaque to the server except for sizes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EncryptedBody {
    /// Payload format version, currently [`OBJECT_FORMAT_V1`].
    pub format: u16,
    /// XChaCha20-Poly1305(DEK, padded payload, AAD = object AAD).
    pub ciphertext: Bytes,
    /// 24-byte nonce for `ciphertext`.
    pub nonce: Bytes,
    /// XChaCha20-Poly1305(KEK, DEK) — 48 bytes.
    pub wrapped_dek: Bytes,
    /// 24-byte nonce for `wrapped_dek`.
    pub wrapped_dek_nonce: Bytes,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum MutationOp {
    /// Create or replace the object body.
    Put { body: EncryptedBody },
    /// Tombstone the object.
    Delete,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Mutation {
    pub mutation_id: MutationId,
    pub object_id: ObjectId,
    /// Revision the client based this change on; 0 = create.
    pub base_revision: i64,
    #[serde(flatten)]
    pub op: MutationOp,
}

/// `POST /v1/sync/push`
///
/// Mutations are applied independently, in order. Response status:
/// * `200` — every mutation accepted (or replayed);
/// * `409` — at least one conflict. The body is still a [`PushResponse`];
///   accepted mutations in the same batch ARE committed.
///
/// The request is rejected as a whole (`400`/`413`) before any mutation is
/// applied if validation fails (limits, duplicate object ids in one batch,
/// malformed body).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PushRequest {
    pub vault_id: VaultId,
    /// Must equal the device bound to the access token.
    pub device_id: DeviceId,
    pub mutations: Vec<Mutation>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum MutationResult {
    Accepted {
        mutation_id: MutationId,
        object_id: ObjectId,
        revision: i64,
        sequence: i64,
        /// True when this `mutation_id` had already been applied earlier and
        /// the stored result is returned (idempotent retry).
        #[serde(default)]
        replayed: bool,
    },
    Conflict {
        mutation_id: MutationId,
        object_id: ObjectId,
        /// 0 if the object does not exist (update of a never-created id).
        current_revision: i64,
        current_sequence: i64,
        #[serde(default)]
        current_deleted: bool,
    },
}

impl MutationResult {
    pub fn mutation_id(&self) -> MutationId {
        match self {
            MutationResult::Accepted { mutation_id, .. }
            | MutationResult::Conflict { mutation_id, .. } => *mutation_id,
        }
    }

    pub fn is_conflict(&self) -> bool {
        matches!(self, MutationResult::Conflict { .. })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PushResponse {
    /// One result per request mutation, same order.
    pub results: Vec<MutationResult>,
    /// Vault sequence after this push.
    pub latest_sequence: i64,
}

/// `GET /v1/sync/changes?vault_id=…&after=…&limit=…`
///
/// If `after` is older than the server's tombstone horizon the server answers
/// `410 gone` and the client must re-run a snapshot.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChangesQuery {
    pub vault_id: VaultId,
    pub after: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<u32>,
}

/// Latest state of one object.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Change {
    pub object_id: ObjectId,
    pub revision: i64,
    pub sequence: i64,
    pub deleted: bool,
    /// `None` iff `deleted`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body: Option<EncryptedBody>,
    pub writer_device_id: DeviceId,
    pub updated_at: Timestamp,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChangesResponse {
    /// Ordered by `sequence` ascending.
    pub changes: Vec<Change>,
    /// Cursor for the next page (= last returned sequence, or `after` if empty).
    pub next_after: i64,
    pub has_more: bool,
    pub latest_sequence: i64,
    /// Vault epoch at the time of this page (see [`crate::vaults::VaultInfo::epoch`]).
    /// Added in protocol 1.3.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub epoch: Option<uuid::Uuid>,
}

/// `GET /v1/sync/snapshot?vault_id=…&cursor=…&limit=…`
///
/// Initial sync: all live (non-deleted) objects, paged by sequence. Objects
/// modified while paging move to a higher sequence and are therefore still
/// returned on a later page. After the last page the client continues with
/// `changes?after=<max sequence seen>`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SnapshotQuery {
    pub vault_id: VaultId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cursor: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SnapshotResponse {
    /// Live objects only (`deleted = false`), ordered by sequence.
    pub objects: Vec<Change>,
    /// `None` on the last page.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<i64>,
    pub latest_sequence: i64,
    /// Vault epoch at the time of this page (see [`crate::vaults::VaultInfo::epoch`]).
    /// Added in protocol 1.3.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub epoch: Option<uuid::Uuid>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn body() -> EncryptedBody {
        EncryptedBody {
            format: OBJECT_FORMAT_V1,
            ciphertext: Bytes::new(vec![1; 64]),
            nonce: Bytes::new(vec![2; 24]),
            wrapped_dek: Bytes::new(vec![3; 48]),
            wrapped_dek_nonce: Bytes::new(vec![4; 24]),
        }
    }

    #[test]
    fn mutation_json_is_flat_and_tagged() {
        let m = Mutation {
            mutation_id: MutationId::new(),
            object_id: ObjectId::new(),
            base_revision: 0,
            op: MutationOp::Put { body: body() },
        };
        let v = serde_json::to_value(&m).unwrap();
        assert_eq!(v["op"], "put");
        assert!(v["body"]["ciphertext"].is_string());
        let back: Mutation = serde_json::from_value(v).unwrap();
        assert_eq!(back, m);

        let d = Mutation {
            op: MutationOp::Delete,
            ..m
        };
        let v = serde_json::to_value(&d).unwrap();
        assert_eq!(v["op"], "delete");
        assert!(v.get("body").is_none());
    }

    #[test]
    fn epoch_is_optional_for_1_2_peers() {
        let v = serde_json::json!({
            "changes": [], "next_after": 0, "has_more": false, "latest_sequence": 0
        });
        let r: ChangesResponse = serde_json::from_value(v).unwrap();
        assert!(r.epoch.is_none());
        let back = serde_json::to_value(&r).unwrap();
        assert!(back.get("epoch").is_none());
    }

    #[test]
    fn mutation_result_tagging() {
        let r = MutationResult::Conflict {
            mutation_id: MutationId::new(),
            object_id: ObjectId::new(),
            current_revision: 4,
            current_sequence: 1042,
            current_deleted: false,
        };
        let v = serde_json::to_value(&r).unwrap();
        assert_eq!(v["status"], "conflict");
        assert_eq!(v["current_sequence"], 1042);
        assert!(r.is_conflict());
    }
}
