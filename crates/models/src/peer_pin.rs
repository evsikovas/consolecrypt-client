//! Pinned keys of colleagues' devices for sharing (audit H3, TR-12).
//!
//! A pin records the keys a device of another account had when it was
//! first chosen as a recipient (or as the owner of an accepted share) and
//! whether a human verified them outside the server (QR code or safety
//! number). The keys never change silently: other keys reported later for
//! the same device turn the pin into `keys_changed` and block new access
//! until the new keys are verified.
//!
//! 0.9.2 keeps pins in the encrypted profile database of each device:
//! clients up to 0.9.1 would report an unknown vault object kind as an
//! integrity warning, and the server does not tell which app version each
//! device runs (TR-20). A later release moves them into a synced vault
//! object of this shape.

use cc_protocol::{Bytes, DeviceId, Timestamp, UserId};
use serde::{Deserialize, Serialize};

/// Trust in a colleague's device keys.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PeerPinStatus {
    /// Chosen by the user, keys pinned on first use, not verified.
    Chosen,
    /// Verified by the user outside the server.
    Verified,
    /// The server reported other keys for this device than the pinned
    /// ones: no new access until the new keys are verified.
    KeysChanged,
    /// A status added by a newer version (treated as not verified).
    #[serde(other)]
    Unknown,
}

/// How a pin was verified.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PeerVerificationMethod {
    /// The colleague's QR code (full keys) was scanned or entered.
    QrCode,
    /// The long safety number was read out and typed in.
    SafetyNumber,
    /// A later method (treated as not verified).
    #[serde(other)]
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PeerPin {
    /// Stable id (the `verification_id` sharing calls refer to).
    pub id: uuid::Uuid,
    /// Sharing instance of the server (pins never cross instances).
    pub instance_id: uuid::Uuid,
    pub user_id: UserId,
    pub device_id: DeviceId,
    /// Pinned X25519 / Ed25519 public keys (32 bytes each).
    pub encryption_public_key: Bytes,
    pub signing_public_key: Bytes,
    pub status: PeerPinStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub method: Option<PeerVerificationMethod>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub verified_at: Option<Timestamp>,
    pub first_seen: Timestamp,
    /// E-mail the user looked the colleague up by (display only).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub email: Option<String>,
    /// While `keys_changed`: the other keys the server reported.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reported_encryption_public_key: Option<Bytes>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reported_signing_public_key: Option<Bytes>,
}

impl PeerPin {
    /// Whether new access may be granted to this device without a
    /// "verified only" policy.
    pub fn usable(&self) -> bool {
        matches!(self.status, PeerPinStatus::Chosen | PeerPinStatus::Verified)
    }

    pub fn is_verified(&self) -> bool {
        self.status == PeerPinStatus::Verified
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn future_status_and_method_decode_as_unknown() {
        let now = chrono::Utc::now();
        let pin = PeerPin {
            id: uuid::Uuid::new_v4(),
            instance_id: uuid::Uuid::new_v4(),
            user_id: UserId::new(),
            device_id: DeviceId::new(),
            encryption_public_key: Bytes::new(vec![1; 32]),
            signing_public_key: Bytes::new(vec![2; 32]),
            status: PeerPinStatus::Verified,
            method: Some(PeerVerificationMethod::QrCode),
            verified_at: Some(now),
            first_seen: now,
            email: None,
            reported_encryption_public_key: None,
            reported_signing_public_key: None,
        };
        let mut v = serde_json::to_value(&pin).unwrap();
        assert_eq!(serde_json::from_value::<PeerPin>(v.clone()).unwrap(), pin);
        v["status"] = "witnessed".into();
        v["method"] = "nfc".into();
        let back: PeerPin = serde_json::from_value(v).unwrap();
        assert_eq!(back.status, PeerPinStatus::Unknown);
        assert_eq!(back.method, Some(PeerVerificationMethod::Unknown));
        assert!(!back.usable());
    }
}
