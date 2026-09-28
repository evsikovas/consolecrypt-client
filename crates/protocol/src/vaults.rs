//! Vault lifecycle (`/v1/vaults/*`). The server stores no VRK and no
//! plaintext vault metadata — even the vault's display name lives inside an
//! encrypted object in the vault.

use crate::bytes::Bytes;
use crate::envelopes::{KeyEnvelope, NewEnvelope};
use crate::ids::{UserId, VaultId};
use crate::Timestamp;
use serde::{Deserialize, Serialize};

/// `POST /v1/vaults` → `201` [`VaultInfo`]. The creating device becomes
/// trusted for the vault.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateVaultRequest {
    /// Client-generated (bound into AEAD associated data).
    pub vault_id: VaultId,
    /// 32-byte vault access key (ADR-0002 §VAK). The server stores only
    /// `SHA-256(vault_access_key)` and uses it to authorize attestation and
    /// envelope replacement. It cannot be used to derive VRK.
    pub vault_access_key: Bytes,
    pub password_envelope: NewEnvelope,
    pub recovery_envelope: NewEnvelope,
    /// Device envelope for the calling device.
    pub device_envelope: NewEnvelope,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VaultRole {
    Owner,
    /// Team Vault foundation — not used by MVP UI.
    Admin,
    Editor,
    Viewer,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VaultState {
    Active,
    /// Deletion requested by an untrusted device of the owner account; purged
    /// after the grace period unless a trusted device cancels.
    PendingDeletion,
    Deleted,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VaultInfo {
    pub vault_id: VaultId,
    pub owner_user_id: UserId,
    /// Caller's role.
    pub role: VaultRole,
    pub state: VaultState,
    pub created_at: Timestamp,
    pub updated_at: Timestamp,
    /// Highest sequence allocated in this vault (0 = empty).
    pub latest_sequence: i64,
    /// Whether the calling device holds a device envelope for this vault.
    pub caller_trusted: bool,
    /// Vault epoch: random id set when the vault is created and rotated by
    /// the operator after restoring the server from a backup
    /// (`consolecrypt-server admin rotate-epoch`). A client that sees the
    /// epoch change — or `latest_sequence` go below what it already holds —
    /// must assume a rollback: re-snapshot and re-push local objects the
    /// server lacks. `None` from servers older than protocol 1.3.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub epoch: Option<uuid::Uuid>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deletion_scheduled_at: Option<Timestamp>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListVaultsResponse {
    pub vaults: Vec<VaultInfo>,
}

/// `DELETE /v1/vaults/{id}` body (optional). With a valid access key from a
/// trusted device the vault is deleted immediately (soft delete, purged by
/// retention job); otherwise it enters `pending_deletion`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DeleteVaultRequest {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vault_access_key: Option<Bytes>,
}

/// `GET /v1/vaults/{id}/envelopes`
///
/// A trusted device sees all non-revoked envelopes. An untrusted (but active)
/// device of a member sees only the password envelope, the recovery envelope
/// and its own device envelope — exactly what it needs to unlock.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListEnvelopesResponse {
    pub envelopes: Vec<KeyEnvelope>,
}

/// `DELETE /v1/vaults/{id}/envelopes/{envelope_id}` body → `204`.
///
/// Trusted device + vault access key. Only `device` and `user` envelopes can
/// be deleted (e.g. to stop trusting a device for this vault without revoking
/// it); the password and recovery envelopes can only be replaced via
/// `/v1/recovery/vault/*-envelope/replace`. Added in protocol 1.2.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeleteEnvelopeRequest {
    pub vault_access_key: Bytes,
}

/// `POST /v1/vaults/{id}/envelopes` → `201` [`KeyEnvelope`]. Trusted device +
/// access key. For `user` recipients (Team Vault) and for re-keying the
/// caller's own device envelope. Other devices are added via
/// `/v1/devices/{id}/approve`; password/recovery via `/v1/recovery/vault/*`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateEnvelopeRequest {
    pub vault_access_key: Bytes,
    pub envelope: NewEnvelope,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn delete_envelope_request_shape() {
        let r = DeleteEnvelopeRequest {
            vault_access_key: Bytes::new(vec![7; 32]),
        };
        let v = serde_json::to_value(&r).unwrap();
        assert!(v["vault_access_key"].is_string());
        let back: DeleteEnvelopeRequest = serde_json::from_value(v).unwrap();
        assert_eq!(back.vault_access_key.len(), 32);
        assert!(!format!("{back:?}").contains("7, 7"));
    }
}
