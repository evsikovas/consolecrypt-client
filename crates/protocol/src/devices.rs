//! Devices and device trust (`/v1/devices/*`). See ADR-0004.
//!
//! Two independent notions:
//!
//! * **Account-level status** ([`DeviceStatus`]): a device is `active` from the
//!   moment it logs in, until it is `revoked`. Revoked devices lose all
//!   sessions and all API access immediately.
//! * **Vault-level trust**: a device is *trusted for vault V* iff it holds a
//!   non-revoked `device` key envelope for V. Only trusted devices can pull
//!   objects, push mutations, approve other devices for V or replace V's
//!   envelopes. A device becomes trusted for V by:
//!   1. creating V (its own device envelope is uploaded with the vault);
//!   2. being approved by a device already trusted for V
//!      ([`ApproveDeviceRequest`], Ed25519-signed, with out-of-band
//!      verification-code comparison by the user);
//!   3. proving knowledge of V's root key via the vault access key
//!      ([`AttestDeviceRequest`]) after unlocking with the Vault passphrase or
//!      Recovery Key.

use crate::bytes::Bytes;
use crate::envelopes::NewEnvelope;
use crate::ids::{DeviceId, DeviceRequestId, VaultId};
use crate::version::Platform;
use crate::Timestamp;
use serde::{Deserialize, Serialize};

/// Sent with register/login. Keys are generated on the device; private
/// halves never leave it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeviceRegistration {
    pub device_id: DeviceId,
    /// User-visible name, e.g. "Work MacBook". Not secret, but treat as PII.
    pub name: String,
    pub platform: Platform,
    /// X25519 public key (32 bytes) — recipient key for device envelopes.
    pub encryption_public_key: Bytes,
    /// Ed25519 public key (32 bytes) — verifies approvals signed by this device.
    pub signing_public_key: Bytes,
    /// App version at registration time.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub client_version: Option<String>,
}

/// Proof that the caller holds the Ed25519 signing key of the device it logs
/// in as (protocol 1.4, ADR-0006). `signature` is Ed25519 by the device's
/// signing key over [`crate::canonical::device_login_message`]`(device_id,
/// issued_at, nonce)`.
///
/// Required by the server when the `device_id` is already registered
/// (otherwise `422 invalid_proof` with `details.reason =
/// "device_proof_required"`); verified when present for a new device.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeviceProof {
    /// Unix seconds; must be within `limits::MAX_SIGNATURE_SKEW_SECONDS`.
    pub issued_at: i64,
    /// 32 fresh random bytes; single use per device.
    pub nonce: Bytes,
    /// 64-byte Ed25519 signature.
    pub signature: Bytes,
}

/// Per-request proof of possession of the device key (protocol 1.5), sent in
/// the [`crate::version::HEADER_DEVICE_PROOF`] header of every authenticated
/// request (incl. the WebSocket upgrade and `POST /v1/auth/refresh`):
///
/// ```text
/// x-cc-device-proof: <issued_at>.<base64url(nonce)>.<base64url(signature)>
/// ```
///
/// base64url without padding; `signature` = Ed25519 by the device signing
/// key over [`crate::canonical::request_proof_message`]. The server checks
/// it against the device of the session, `issued_at` within
/// `limits::MAX_REQUEST_PROOF_SKEW_SECONDS`, nonce single-use per device.
/// Errors: `422 invalid_proof` with `details.reason` =
/// `missing` | `malformed` | `stale` | `replayed` | `invalid_signature`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RequestProof {
    pub issued_at: i64,
    pub nonce: [u8; 32],
    pub signature: [u8; 64],
}

impl RequestProof {
    /// Header value.
    pub fn encode(&self) -> String {
        use base64::engine::general_purpose::URL_SAFE_NO_PAD;
        use base64::Engine as _;
        format!(
            "{}.{}.{}",
            self.issued_at,
            URL_SAFE_NO_PAD.encode(self.nonce),
            URL_SAFE_NO_PAD.encode(self.signature)
        )
    }

    /// Parse a header value; `None` if malformed.
    pub fn decode(value: &str) -> Option<Self> {
        use base64::engine::general_purpose::URL_SAFE_NO_PAD;
        use base64::Engine as _;
        let mut parts = value.trim().split('.');
        let issued_at = parts.next()?.parse().ok()?;
        let nonce = URL_SAFE_NO_PAD
            .decode(parts.next()?)
            .ok()?
            .try_into()
            .ok()?;
        let signature = URL_SAFE_NO_PAD
            .decode(parts.next()?)
            .ok()?
            .try_into()
            .ok()?;
        if parts.next().is_some() {
            return None;
        }
        Some(Self {
            issued_at,
            nonce,
            signature,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeviceStatus {
    Active,
    Revoked,
}

/// `GET /v1/devices` item.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceInfo {
    pub device_id: DeviceId,
    pub name: String,
    pub platform: Platform,
    pub encryption_public_key: Bytes,
    pub signing_public_key: Bytes,
    pub status: DeviceStatus,
    /// Vaults this device currently holds a device envelope for.
    pub trusted_vaults: Vec<VaultId>,
    pub created_at: Timestamp,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_seen_at: Option<Timestamp>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub revoked_at: Option<Timestamp>,
    /// True for the device that made this request.
    #[serde(default)]
    pub is_current: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListDevicesResponse {
    pub devices: Vec<DeviceInfo>,
    /// Pending trust requests of this account.
    pub pending_requests: Vec<DeviceTrustRequest>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeviceRequestStatus {
    Pending,
    Approved,
    Rejected,
    Expired,
}

/// `POST /v1/devices` — the calling device asks to be trusted for some (or
/// all) of the account's vaults. Emits `device_approval_requested`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CreateDeviceTrustRequest {
    /// Empty = all vaults the account can access.
    #[serde(default)]
    pub vault_ids: Vec<VaultId>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceTrustRequest {
    pub request_id: DeviceRequestId,
    pub device: DeviceInfo,
    /// Always explicit: the server expands an empty
    /// [`CreateDeviceTrustRequest::vault_ids`] to every vault the account
    /// could access at request time. Approvers may approve any non-empty
    /// subset they are trusted for.
    pub vault_ids: Vec<VaultId>,
    pub status: DeviceRequestStatus,
    pub created_at: Timestamp,
    pub expires_at: Timestamp,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub approved_by_device_id: Option<DeviceId>,
}

/// `POST /v1/devices/{new_device_id}/approve` — called by a device trusted
/// for every vault in `envelopes`.
///
/// `signature` is Ed25519 by the approver's signing key over
/// [`crate::canonical::device_approval_message`]. Before signing, the approver
/// UI MUST show the verification code derived from the new device's public
/// keys ([`crate::canonical::device_fingerprint_input`]) and the user MUST
/// confirm it matches the code shown on the new device. This is what stops a
/// malicious server from substituting its own key.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApproveDeviceRequest {
    pub request_id: DeviceRequestId,
    /// Unix seconds; must be within `limits::MAX_SIGNATURE_SKEW_SECONDS`.
    pub issued_at: i64,
    pub signature: Bytes,
    /// One `device` envelope (recipient = new device) per approved vault.
    pub envelopes: Vec<VaultEnvelope>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VaultEnvelope {
    pub vault_id: VaultId,
    pub envelope: NewEnvelope,
}

/// `POST /v1/devices/{pending_device_id}/reject` → `204`. Any device trusted
/// for at least one of the requested vaults may reject a pending request.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RejectDeviceRequest {
    pub request_id: DeviceRequestId,
}

/// `POST /v1/devices/{self_device_id}/attest` — self-trust after unlocking the
/// vault with the passphrase or Recovery Key. Only for the calling device.
///
/// `vault_access_key` = HKDF(VRK, …) per ADR-0002; the server compares
/// `SHA-256(vault_access_key)` with the verifier stored at vault creation in
/// constant time. The server never learns VRK.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AttestDeviceRequest {
    pub vault_id: VaultId,
    pub vault_access_key: Bytes,
    /// The caller's own `device` envelope for this vault.
    pub envelope: NewEnvelope,
}

/// `POST /v1/devices/{id}/revoke` → `204`. Emits `device_revoked`.
///
/// Allowed for any active device of the account (a stolen laptop must be
/// revocable even if every other trusted device is lost). Revocation kills
/// the device's sessions and revokes its device envelopes. It cannot erase
/// what the device already decrypted — see the threat model on key rotation.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RevokeDeviceRequest {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

/// `PATCH /v1/devices/{id}` — rename.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateDeviceRequest {
    pub name: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_proof_header_roundtrip() {
        let p = RequestProof {
            issued_at: 1_700_000_000,
            nonce: [7; 32],
            signature: [9; 64],
        };
        let h = p.encode();
        assert!(h.starts_with("1700000000."));
        assert_eq!(RequestProof::decode(&h), Some(p));
        for bad in [
            "",
            "1.2",
            "x.AAAA.BBBB",
            &format!("{h}.extra"),
            "1.AAAA.BBBB",
        ] {
            assert_eq!(RequestProof::decode(bad), None, "{bad}");
        }
    }
}
