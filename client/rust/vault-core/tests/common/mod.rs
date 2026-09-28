//! Minimal "server side" helpers for flow tests: turn uploaded DTOs into
//! what a server would store/return. No crypto happens here.

#![allow(dead_code)]

use cc_protocol::devices::{
    DeviceInfo, DeviceRegistration, DeviceRequestStatus, DeviceStatus, DeviceTrustRequest,
};
use cc_protocol::envelopes::{KeyEnvelope, NewEnvelope};
use cc_protocol::{DeviceRequestId, EnvelopeId, Timestamp, VaultId};
use cc_vault_core::{Argon2Params, SecretString};

pub fn now() -> Timestamp {
    chrono::Utc::now()
}

pub fn kdf() -> Argon2Params {
    Argon2Params::for_tests()
}

pub fn pass(s: &str) -> SecretString {
    SecretString::from(s)
}

/// What `GET /v1/vaults/{id}/envelopes` would return for an uploaded envelope.
pub fn stored(vault_id: VaultId, e: &NewEnvelope) -> KeyEnvelope {
    KeyEnvelope {
        envelope_id: EnvelopeId::new(),
        vault_id,
        recipient_type: e.recipient_type,
        recipient_id: e.recipient_id,
        kind: e.kind,
        metadata: e.metadata.clone(),
        ciphertext: e.ciphertext.clone(),
        nonce: e.nonce.clone(),
        created_at: now(),
        created_by_device_id: None,
        revoked_at: None,
    }
}

/// Device as listed by the server after registration.
pub fn device_info(reg: &DeviceRegistration) -> DeviceInfo {
    DeviceInfo {
        device_id: reg.device_id,
        name: reg.name.clone(),
        platform: reg.platform,
        encryption_public_key: reg.encryption_public_key.clone(),
        signing_public_key: reg.signing_public_key.clone(),
        status: DeviceStatus::Active,
        trusted_vaults: vec![],
        created_at: now(),
        last_seen_at: None,
        revoked_at: None,
        is_current: false,
    }
}

/// Pending trust request for `reg`, expiring in 24 h.
pub fn trust_request(reg: &DeviceRegistration, vault_ids: Vec<VaultId>) -> DeviceTrustRequest {
    DeviceTrustRequest {
        request_id: DeviceRequestId::new(),
        device: device_info(reg),
        vault_ids,
        status: DeviceRequestStatus::Pending,
        created_at: now(),
        expires_at: now() + chrono::Duration::hours(24),
        approved_by_device_id: None,
    }
}
