//! Known host keys. Synced (E2EE) so a second device verifies hosts the
//! same way the first one did.

use crate::{ObjectId, Timestamp};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum KnownHostSource {
    /// Accepted by the user on first connect.
    Tofu,
    Manual,
    /// Imported from `~/.ssh/known_hosts`.
    Imported,
    /// Trusted via an `@cert-authority` line / host certificate.
    CertAuthority,
}

/// OpenSSH known_hosts line marker.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum KnownHostMarker {
    /// `@cert-authority`: `public_key` is a host CA key.
    CertAuthority,
    /// `@revoked`: the key must never be accepted.
    Revoked,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct KnownHost {
    pub id: ObjectId,
    /// `host` for port 22, otherwise `[host]:port` (OpenSSH convention).
    pub host_pattern: String,
    /// e.g. `ssh-ed25519`.
    pub key_type: String,
    /// Base64 public key blob (as in known_hosts).
    pub public_key: String,
    /// `SHA256:…`
    pub fingerprint_sha256: String,
    pub source: KnownHostSource,
    /// Explicitly distrusted (`@revoked`).
    #[serde(default)]
    pub revoked: bool,
    /// Line marker, independent of `source` (so an imported `@cert-authority`
    /// line keeps `source = Imported`). Added 2026-09-26; absent = no marker.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub marker: Option<KnownHostMarker>,
    pub added_at: Timestamp,
    pub updated_at: Timestamp,
}

impl KnownHost {
    /// Host CA entry (`@cert-authority`). Also honours the pre-`marker`
    /// representation `source = CertAuthority`.
    pub fn is_cert_authority(&self) -> bool {
        self.marker == Some(KnownHostMarker::CertAuthority)
            || self.source == KnownHostSource::CertAuthority
    }

    /// Key must never be accepted (`@revoked`).
    pub fn is_revoked(&self) -> bool {
        self.revoked || self.marker == Some(KnownHostMarker::Revoked)
    }
}

/// Canonical known-hosts pattern for a host/port pair.
pub fn host_pattern(host: &str, port: u16) -> String {
    if port == 22 {
        host.to_ascii_lowercase()
    } else {
        format!("[{}]:{}", host.to_ascii_lowercase(), port)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn patterns_follow_openssh() {
        assert_eq!(host_pattern("Example.org", 22), "example.org");
        assert_eq!(host_pattern("10.0.0.1", 2222), "[10.0.0.1]:2222");
    }
}
