//! Realtime events over `GET /v1/events/ws` (WebSocket, text frames, JSON).
//!
//! Authentication: `Authorization: Bearer <access token>` on the upgrade
//! request. Events carry metadata only — never ciphertext — and are hints:
//! after `vault_changed` the client runs a normal `changes` pull. Clients MUST
//! ignore unknown event types (they deserialize as [`ServerEvent::Unknown`]).
//!
//! Close codes (server → client):
//! * [`CLOSE_REVOKED`] `4001` — session or device revoked: stop, do not reconnect,
//!   surface to the user (re-login or new device identity);
//! * [`CLOSE_LAGGED`] `4002` — client fell behind the event buffer: reconnect
//!   immediately and run a normal `changes` pull;
//! * [`CLOSE_TOKEN_EXPIRED`] `4003` — the access token the socket was opened
//!   with expired or was rotated: refresh if needed and reconnect (1.4).

use crate::envelopes::RecipientType;
use crate::ids::{DeviceId, DeviceRequestId, SessionId, VaultId};
use crate::version::ProtocolVersion;
use crate::Timestamp;
use serde::{Deserialize, Serialize};

/// WebSocket close code: session or device revoked.
pub const CLOSE_REVOKED: u16 = 4001;
/// WebSocket close code: event buffer overflow, reconnect and pull.
pub const CLOSE_LAGGED: u16 = 4002;
/// WebSocket close code: the connecting access token expired or was rotated;
/// reconnect with the current token (protocol 1.4).
pub const CLOSE_TOKEN_EXPIRED: u16 = 4003;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ServerEvent {
    /// First frame after the upgrade.
    Hello {
        protocol_version: ProtocolVersion,
        server_time: Timestamp,
        session_id: SessionId,
    },
    VaultChanged {
        vault_id: VaultId,
        latest_sequence: i64,
    },
    DeviceAdded {
        device_id: DeviceId,
    },
    DeviceApprovalRequested {
        request_id: DeviceRequestId,
        device_id: DeviceId,
    },
    DeviceApproved {
        device_id: DeviceId,
        vault_ids: Vec<VaultId>,
    },
    DeviceRevoked {
        device_id: DeviceId,
    },
    RecoveryChanged {
        vault_id: VaultId,
        recipient_type: RecipientType,
    },
    SessionRevoked {
        session_id: SessionId,
    },
    TeamMembershipChangedFuture {
        vault_id: VaultId,
    },
    /// Forward compatibility: any event type this build does not know.
    #[serde(other)]
    Unknown,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vault_changed_matches_spec_shape() {
        let vault_id = VaultId::new();
        let e = ServerEvent::VaultChanged {
            vault_id,
            latest_sequence: 1042,
        };
        let v = serde_json::to_value(&e).unwrap();
        assert_eq!(
            v,
            serde_json::json!({
                "type": "vault_changed",
                "vault_id": vault_id.to_string(),
                "latest_sequence": 1042
            })
        );
    }

    #[test]
    fn unknown_events_are_tolerated() {
        let e: ServerEvent = serde_json::from_str(r#"{"type":"something_new","x":1}"#).unwrap();
        assert_eq!(e, ServerEvent::Unknown);
    }
}
