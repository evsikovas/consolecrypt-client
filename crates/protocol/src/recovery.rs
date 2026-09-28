//! Vault recovery (`/v1/recovery/vault/*`). Account recovery reuses
//! [`crate::auth::ForgotPasswordRequest`] / [`crate::auth::ResetPasswordRequest`]
//! on `/v1/recovery/account/{start,confirm}` and never grants Vault access.
//!
//! Scenarios (ADR-0004):
//! * forgot Vault passphrase, have a trusted device → unlock via device
//!   envelope → `password-envelope/replace`;
//! * no trusted device, have Recovery Key → login on new device →
//!   `GET envelope` → unlock recovery envelope → `/v1/devices/{self}/attest`
//!   → `password-envelope/replace`;
//! * lost everything → account recovery + create a new, empty vault.

use crate::bytes::Bytes;
use crate::envelopes::{KeyEnvelope, NewEnvelope};
use crate::ids::VaultId;
use serde::{Deserialize, Serialize};

/// `GET /v1/recovery/vault/envelope?vault_id=…` — any active device of a
/// vault member.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VaultRecoveryMaterial {
    pub vault_id: VaultId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub password_envelope: Option<KeyEnvelope>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recovery_envelope: Option<KeyEnvelope>,
    /// The calling device's own envelope, if it has one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub device_envelope: Option<KeyEnvelope>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecoveryMaterialQuery {
    pub vault_id: VaultId,
}

/// `POST /v1/recovery/vault/password-envelope/replace` and
/// `POST /v1/recovery/vault/recovery-envelope/replace` → `200` [`KeyEnvelope`].
///
/// Caller must be trusted for the vault and present the vault access key.
/// The old envelope of the same recipient type is revoked atomically.
/// Emits `recovery_changed`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReplaceEnvelopeRequest {
    pub vault_id: VaultId,
    pub vault_access_key: Bytes,
    pub envelope: NewEnvelope,
}
