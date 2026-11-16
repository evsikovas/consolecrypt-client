//! Owner-provisioned DATA, never an admission. Existing Lease commits its hash.
use crate::*;
use serde::{Deserialize, Serialize};

pub const TUNNEL_POLICY_DOMAIN: &[u8] = b"consolecrypt.agent.tunnel.route-policy.v1";

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub enum TunnelClientSubject {
    LocalProfile { id: String },
    PersonalAccount { id: String },
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TunnelDestination {
    pub protocol: String,
    pub ip: String,
    pub port: u16,
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TunnelClientBind {
    pub ip: String,
    pub port: u16,
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TunnelLimits {
    pub max_ttl_seconds: u64,
    pub idle_timeout_seconds: u64,
    pub max_channels: u64,
    pub max_frame_bytes: u64,
    pub max_pending_frames_per_channel: u64,
    pub max_pending_frames_total: u64,
    pub max_queued_bytes_per_channel: u64,
    pub max_queued_bytes_total: u64,
    pub max_bytes_per_channel: u64,
    pub max_bytes_total: u64,
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TunnelRoutePolicy {
    pub format: String,
    pub context: Context,
    pub agent_id: AgentId,
    pub agent_certificate_hash: Hash,
    pub key_epoch: u64,
    pub route_id: ResourceId,
    pub client_subject: TunnelClientSubject,
    pub client_device_id: String,
    pub client_tls_certificate_sha256: Hash,
    pub agent_tls_certificate_sha256: Hash,
    pub destination: TunnelDestination,
    pub client_bind: TunnelClientBind,
    pub limits: TunnelLimits,
}

impl std::fmt::Debug for TunnelRoutePolicy {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("TunnelRoutePolicy([redacted])")
    }
}

fn uuid(value: &str) -> Result<()> {
    let parsed = uuid::Uuid::parse_str(value).map_err(|_| Error::InvalidWire)?;
    if parsed.is_nil() || parsed.to_string() != value {
        return Err(Error::InvalidWire);
    }
    Ok(())
}

impl TunnelRoutePolicy {
    pub fn validate(&self) -> Result<()> {
        self.context.personal()?;
        let subject = match &self.client_subject {
            TunnelClientSubject::LocalProfile { id }
            | TunnelClientSubject::PersonalAccount { id } => id,
        };
        uuid(subject)?;
        uuid(&self.client_device_id)?;
        if self.format != "cc-agent-tunnel-route-policy/v1"
            || self.key_epoch == 0
            || [
                self.agent_certificate_hash,
                self.client_tls_certificate_sha256,
                self.agent_tls_certificate_sha256,
            ]
            .iter()
            .any(|h| h.as_bytes() == &[0; 32])
            || self.destination.protocol != "tcp"
            || !matches!(self.destination.ip.as_str(), "127.0.0.1" | "::1")
            || self.destination.port == 0
            || !matches!(self.client_bind.ip.as_str(), "127.0.0.1" | "::1")
        {
            return Err(Error::InvalidWire);
        }
        let l = &self.limits;
        for (value, maximum) in [
            (l.max_ttl_seconds, 300),
            (l.idle_timeout_seconds, 30),
            (l.max_channels, 4),
            (l.max_frame_bytes, 16384),
            (l.max_pending_frames_per_channel, 4),
            (l.max_pending_frames_total, 16),
            (l.max_queued_bytes_per_channel, 65536),
            (l.max_queued_bytes_total, 262144),
            (l.max_bytes_per_channel, 8388608),
            (l.max_bytes_total, 33554432),
        ] {
            if value == 0 || value > maximum {
                return Err(Error::Quota);
            }
        }
        if l.idle_timeout_seconds > l.max_ttl_seconds
            || l.max_frame_bytes > l.max_queued_bytes_per_channel
            || l.max_queued_bytes_per_channel > l.max_queued_bytes_total
            || l.max_pending_frames_per_channel > l.max_pending_frames_total
            || l.max_bytes_per_channel > l.max_bytes_total
        {
            return Err(Error::Quota);
        }
        Ok(())
    }

    pub fn digest(&self) -> Result<Hash> {
        self.validate()?;
        let value = serde_json::to_value(self).map_err(|_| Error::InvalidWire)?;
        let mut bytes = Vec::new();
        crate::wire::canonical_json(&value, &mut bytes)?;
        Ok(crate::wire::hash(TUNNEL_POLICY_DOMAIN, &bytes))
    }

    pub fn operation(&self) -> Result<Operation> {
        Ok(Operation::TunnelOpenBound {
            route: self.route_id,
            policy_hash: self.digest()?,
        })
    }

    pub fn check_binding(&self, binding: &NodeRequestBinding) -> Result<()> {
        self.validate()?;
        let cert = &binding.certificate.body;
        let lease = &binding.lease.body;
        if binding.operation != self.operation()? || lease.operation != binding.operation {
            return Err(Error::ScopeDenied);
        }
        if cert.header.context != self.context
            || lease.header.context != self.context
            || binding.freshness.body.header.context != self.context
        {
            return Err(Error::WrongContext);
        }
        if cert.agent_id != self.agent_id
            || lease.agent_id != self.agent_id
            || cert.digest()? != self.agent_certificate_hash
            || lease.certificate_hash != self.agent_certificate_hash
        {
            return Err(Error::WrongClaim);
        }
        if cert.key_epoch != self.key_epoch || lease.key_epoch != self.key_epoch {
            return Err(Error::WrongEpoch);
        }
        if cert.capabilities.version != 1
            || cert.capabilities.scopes.0.as_slice() != [Scope::TunnelOpen]
            || cert.scopes.0.as_slice() != [Scope::TunnelOpen]
        {
            return Err(Error::CapabilityDenied);
        }
        if lease
            .expires_at
            .seconds()
            .checked_sub(lease.issued_at.seconds())
            .filter(|seconds| *seconds > 0 && *seconds <= self.limits.max_ttl_seconds)
            .is_none()
        {
            return Err(Error::Expired);
        }
        Ok(())
    }
}

#[cfg(test)]
#[path = "tunnel_tests.rs"]
mod tests;
