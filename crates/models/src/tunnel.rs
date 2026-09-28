//! Port forwarding profiles.

use crate::{ObjectId, Timestamp, ValidationError};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TunnelKind {
    /// `bind_host:bind_port` (local) → SSH → `target_host:target_port`.
    Local,
    /// `bind_host:bind_port` (remote side) → SSH → `target_host:target_port` (local side).
    Remote,
    /// Local SOCKS5 listener → `direct-tcpip` through SSH.
    Dynamic,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Tunnel {
    pub id: ObjectId,
    pub name: String,
    pub kind: TunnelKind,
    /// Host whose SSH connection (incl. its jump chain) carries the tunnel.
    pub host_id: ObjectId,
    pub bind_host: String,
    pub bind_port: u16,
    /// Required for Local/Remote, ignored for Dynamic.
    #[serde(default)]
    pub target_host: Option<String>,
    #[serde(default)]
    pub target_port: Option<u16>,
    #[serde(default)]
    pub auto_start: bool,
    pub created_at: Timestamp,
    pub updated_at: Timestamp,
}

impl Tunnel {
    pub fn validate(&self) -> Result<(), ValidationError> {
        if self.bind_host.trim().is_empty() {
            return Err(ValidationError::new("bind_host", "must not be empty"));
        }
        match self.kind {
            TunnelKind::Local | TunnelKind::Remote => {
                if self
                    .target_host
                    .as_deref()
                    .is_none_or(|h| h.trim().is_empty())
                {
                    return Err(ValidationError::new("target_host", "required"));
                }
                if matches!(self.target_port, None | Some(0)) {
                    return Err(ValidationError::new("target_port", "must be 1..=65535"));
                }
            }
            TunnelKind::Dynamic => {}
        }
        Ok(())
    }

    /// Binding to anything but loopback exposes the tunnel to the network.
    /// `localhost` and any loopback IP literal (`127.0.0.0/8`, `::1`) count as
    /// loopback; everything else (incl. `0.0.0.0`, hostnames) is public.
    pub fn binds_publicly(&self) -> bool {
        let host = self
            .bind_host
            .trim()
            .trim_start_matches('[')
            .trim_end_matches(']');
        if host.eq_ignore_ascii_case("localhost") {
            return false;
        }
        !host
            .parse::<std::net::IpAddr>()
            .is_ok_and(|ip| ip.is_loopback())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tunnel(bind: &str) -> Tunnel {
        let now = chrono::Utc::now();
        Tunnel {
            id: ObjectId::new(),
            name: "t".into(),
            kind: TunnelKind::Dynamic,
            host_id: ObjectId::new(),
            bind_host: bind.into(),
            bind_port: 1080,
            target_host: None,
            target_port: None,
            auto_start: false,
            created_at: now,
            updated_at: now,
        }
    }

    #[test]
    fn loopback_detection() {
        for b in [
            "127.0.0.1",
            "127.1.2.3",
            "::1",
            "[::1]",
            "localhost",
            "LOCALHOST",
        ] {
            assert!(!tunnel(b).binds_publicly(), "{b}");
        }
        for b in ["0.0.0.0", "::", "192.168.1.5", "example.org"] {
            assert!(tunnel(b).binds_publicly(), "{b}");
        }
    }
}
