//! Approving a new device from a trusted one (ADR-0004 §Device approval).
//!
//! Two explicit steps, so the mandatory verification-code comparison cannot
//! be skipped by accident:
//!
//! 1. [`PendingApproval::new`] validates the server's trust request and
//!    derives the [`VerificationCode`] from the keys *the server reports*.
//!    The UI shows it; the user compares it with the code on the new device.
//! 2. Only after the user confirmed the match, the UI calls
//!    [`PendingApproval::confirm_verification_code`] and then
//!    [`ConfirmedApproval::build_request`], which encrypts the VRK of every
//!    approved vault to the new device and signs the approval.

use crate::error::{Result, VaultError};
use crate::identity::DeviceIdentity;
use crate::unlocked::UnlockedVault;
use cc_crypto_core::{DeviceApproval, DevicePublicKeys, VerificationCode};
use cc_protocol::devices::{
    ApproveDeviceRequest, DeviceRequestStatus, DeviceStatus, DeviceTrustRequest, VaultEnvelope,
};
use cc_protocol::{Bytes, DeviceId, DeviceRequestId, Timestamp, VaultId};
use std::collections::HashSet;

/// A validated pending trust request, before the user compared codes.
#[derive(Debug, Clone)]
pub struct PendingApproval {
    request_id: DeviceRequestId,
    new_device_id: DeviceId,
    new_device_name: String,
    new_device_keys: DevicePublicKeys,
    requested_vaults: Vec<VaultId>,
    expires_at: Timestamp,
    code: VerificationCode,
}

impl PendingApproval {
    /// Validate `request` (pending, unexpired at `now`, device active, not
    /// the approver itself, well-formed keys) and compute its verification
    /// code.
    pub fn new(
        request: &DeviceTrustRequest,
        approver: &DeviceIdentity,
        now: Timestamp,
    ) -> Result<Self> {
        if request.status != DeviceRequestStatus::Pending {
            return Err(VaultError::Approval("request is not pending"));
        }
        if request.expires_at <= now {
            return Err(VaultError::Approval("request has expired"));
        }
        if request.device.status != DeviceStatus::Active {
            return Err(VaultError::Approval("device is revoked"));
        }
        if request.device.device_id == approver.device_id() {
            return Err(VaultError::Approval("a device cannot approve itself"));
        }
        let keys = DevicePublicKeys::from_device_info(&request.device)?;
        Ok(Self {
            request_id: request.request_id,
            new_device_id: request.device.device_id,
            new_device_name: request.device.name.clone(),
            new_device_keys: keys,
            requested_vaults: request.vault_ids.clone(),
            expires_at: request.expires_at,
            code: keys.verification_code(request.device.device_id),
        })
    }

    /// The code to show next to "Does this match the code on
    /// `<new_device_name>`?".
    pub fn verification_code(&self) -> VerificationCode {
        self.code
    }

    /// The device asking to be trusted.
    pub fn new_device_id(&self) -> DeviceId {
        self.new_device_id
    }

    /// Its user-visible name (as reported by the server).
    pub fn new_device_name(&self) -> &str {
        &self.new_device_name
    }

    /// Vaults requested (empty = every vault of the account).
    pub fn requested_vault_ids(&self) -> &[VaultId] {
        &self.requested_vaults
    }

    /// Record that the user confirmed both codes match. Call this only from
    /// the UI action where the user explicitly confirmed the comparison.
    pub fn confirm_verification_code(self) -> ConfirmedApproval {
        ConfirmedApproval { pending: self }
    }
}

/// A trust request whose verification code the user confirmed.
#[derive(Debug, Clone)]
pub struct ConfirmedApproval {
    pending: PendingApproval,
}

impl ConfirmedApproval {
    /// Build `POST /v1/devices/{new_device_id}/approve` for exactly `vaults`
    /// (each must be unlocked on this device and, if the request names
    /// vaults, be one of them). `now` becomes `issued_at`.
    pub fn build_request(
        &self,
        approver: &DeviceIdentity,
        vaults: &[&UnlockedVault],
        now: Timestamp,
    ) -> Result<ApproveDeviceRequest> {
        let p = &self.pending;
        if p.expires_at <= now {
            return Err(VaultError::Approval("request has expired"));
        }
        if vaults.is_empty() {
            return Err(VaultError::Approval("no vaults to approve"));
        }
        let mut seen = HashSet::new();
        for v in vaults {
            if !seen.insert(v.vault_id()) {
                return Err(VaultError::Approval("duplicate vault"));
            }
            if !p.requested_vaults.is_empty() && !p.requested_vaults.contains(&v.vault_id()) {
                return Err(VaultError::Approval("vault was not requested"));
            }
        }
        let envelopes = vaults
            .iter()
            .map(|v| {
                Ok(VaultEnvelope {
                    vault_id: v.vault_id(),
                    envelope: v.device_envelope(p.new_device_id, &p.new_device_keys.encryption)?,
                })
            })
            .collect::<Result<Vec<_>>>()?;
        let vault_ids: Vec<VaultId> = vaults.iter().map(|v| v.vault_id()).collect();
        let issued_at = now.timestamp();
        let signature = approver.sign_approval(&DeviceApproval {
            request_id: p.request_id,
            approver_device_id: approver.device_id(),
            new_device_id: p.new_device_id,
            new_device_keys: &p.new_device_keys,
            issued_at,
            vault_ids: &vault_ids,
        });
        Ok(ApproveDeviceRequest {
            request_id: p.request_id,
            issued_at,
            signature: Bytes::new(signature.to_vec()),
            envelopes,
        })
    }

    /// The verification code the user confirmed.
    pub fn verification_code(&self) -> VerificationCode {
        self.pending.code
    }
}
