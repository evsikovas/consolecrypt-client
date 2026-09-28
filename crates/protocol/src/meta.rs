//! Server metadata: `GET /v1/meta` (unauthenticated).

use crate::version::ProtocolVersion;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ServerInfo {
    /// Server build version (semver).
    pub server_version: String,
    /// Protocol version the server speaks.
    pub protocol_version: ProtocolVersion,
    /// Oldest client protocol still accepted.
    pub minimum_supported_protocol: ProtocolVersion,
    /// True when the requesting client (per its headers) must upgrade.
    #[serde(default)]
    pub upgrade_required: bool,
    /// Whether new accounts can self-register on this instance
    /// (self-hosters may run invite-only / single-user instances).
    #[serde(default = "default_true")]
    pub registration_open: bool,
    /// Whether login requires a verified email.
    #[serde(default)]
    pub email_verification_required: bool,
    /// Where users of this instance can obtain the exact source code it runs.
    /// The server is AGPL-3.0 (ADR-0005): operators of modified builds must
    /// point this at their modified source. Clients show it in "About server".
    /// Added in protocol 1.1.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_code_url: Option<String>,
}

fn default_true() -> bool {
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn server_info_from_1_0_server_still_parses() {
        let v = serde_json::json!({
            "server_version": "0.1.0",
            "protocol_version": "1.0",
            "minimum_supported_protocol": "1.0"
        });
        let info: ServerInfo = serde_json::from_value(v).unwrap();
        assert!(info.registration_open);
        assert!(info.source_code_url.is_none());
    }
}
