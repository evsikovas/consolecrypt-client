//! Hosts, jump profiles and proxies.

use crate::{ObjectId, Timestamp, ValidationError};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub const DEFAULT_SSH_PORT: u16 = 22;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HostKeyPolicy {
    /// Unknown key → ask the user (TOFU with confirmation). Changed key → hard fail.
    #[default]
    Ask,
    /// Only keys already in known hosts are accepted.
    Strict,
    /// Unknown keys are accepted and recorded; changed key → hard fail.
    AcceptNew,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SshBackend {
    /// Built-in russh backend.
    #[default]
    Native,
    /// System OpenSSH client (compatibility fallback).
    OpenSsh,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Host {
    pub id: ObjectId,
    pub name: String,
    /// Hostname or IP literal.
    pub address: String,
    /// `None` = inherit from group, else 22.
    #[serde(default)]
    pub port: Option<u16>,
    /// `None` = inherit from group.
    #[serde(default)]
    pub username: Option<String>,
    /// `None` = inherit from group.
    #[serde(default)]
    pub credential_id: Option<ObjectId>,
    #[serde(default)]
    pub group_id: Option<ObjectId>,
    /// Ordered hops (Host ids), first = closest to the client. Empty = inherit
    /// group jump profile (if any), else direct.
    #[serde(default)]
    pub jump_chain: Vec<ObjectId>,
    /// Explicit jump profile; overrides the group's.
    #[serde(default)]
    pub jump_profile_id: Option<ObjectId>,
    #[serde(default)]
    pub proxy_id: Option<ObjectId>,
    /// OpenSSH `ProxyCommand` (e.g. imported from `~/.ssh/config`). Only the
    /// OpenSSH backend can honour it; the native backend reports it as
    /// unsupported. Added 2026-09-26.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub proxy_command: Option<String>,
    #[serde(default)]
    pub host_key_policy: HostKeyPolicy,
    #[serde(default)]
    pub backend: SshBackend,
    /// Keepalive interval; `None` = app default.
    #[serde(default)]
    pub keepalive_secs: Option<u32>,
    #[serde(default)]
    pub agent_forwarding: bool,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub notes: String,
    #[serde(default)]
    pub metadata: BTreeMap<String, String>,
    pub created_at: Timestamp,
    pub updated_at: Timestamp,
}

impl Host {
    pub fn new(name: impl Into<String>, address: impl Into<String>) -> Self {
        let now = chrono::Utc::now();
        Self {
            id: ObjectId::new(),
            name: name.into(),
            address: address.into(),
            port: None,
            username: None,
            credential_id: None,
            group_id: None,
            jump_chain: Vec::new(),
            jump_profile_id: None,
            proxy_id: None,
            proxy_command: None,
            host_key_policy: HostKeyPolicy::default(),
            backend: SshBackend::default(),
            keepalive_secs: None,
            agent_forwarding: false,
            tags: Vec::new(),
            notes: String::new(),
            metadata: BTreeMap::new(),
            created_at: now,
            updated_at: now,
        }
    }

    pub fn validate(&self) -> Result<(), ValidationError> {
        if self.name.trim().is_empty() {
            return Err(ValidationError::new("name", "must not be empty"));
        }
        let addr = self.address.trim();
        if addr.is_empty() {
            return Err(ValidationError::new("address", "must not be empty"));
        }
        if addr.chars().any(|c| c.is_whitespace() || c.is_control()) {
            return Err(ValidationError::new(
                "address",
                "must not contain whitespace",
            ));
        }
        if self.port == Some(0) {
            return Err(ValidationError::new("port", "must be 1..=65535"));
        }
        if self.jump_chain.contains(&self.id) {
            return Err(ValidationError::new(
                "jump_chain",
                "host cannot jump through itself",
            ));
        }
        Ok(())
    }
}

/// Reusable ordered chain of jump hosts (referenced by groups and hosts).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct JumpProfile {
    pub id: ObjectId,
    pub name: String,
    /// Host ids, first = closest to the client.
    pub chain: Vec<ObjectId>,
    pub created_at: Timestamp,
    pub updated_at: Timestamp,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProxyKind {
    Socks5,
    HttpConnect,
}

/// Network proxy used to reach the first hop (not an SSH jump host).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Proxy {
    pub id: ObjectId,
    pub name: String,
    pub kind: ProxyKind,
    pub address: String,
    pub port: u16,
    #[serde(default)]
    pub username: Option<String>,
    /// Secret with the proxy password.
    #[serde(default)]
    pub password_secret_id: Option<ObjectId>,
    pub created_at: Timestamp,
    pub updated_at: Timestamp,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validation() {
        let mut h = Host::new("db", "10.0.0.1");
        h.validate().unwrap();
        h.port = Some(0);
        assert!(h.validate().is_err());
        h.port = Some(2222);
        h.jump_chain = vec![h.id];
        assert!(h.validate().is_err());
        let h = Host::new("db", "bad host");
        assert!(h.validate().is_err());
    }

    #[test]
    fn missing_optional_fields_deserialize() {
        let json = serde_json::json!({
            "id": ObjectId::new(),
            "name": "x",
            "address": "example.org",
            "created_at": "2026-01-01T00:00:00Z",
            "updated_at": "2026-01-01T00:00:00Z"
        });
        let h: Host = serde_json::from_value(json).unwrap();
        assert_eq!(h.host_key_policy, HostKeyPolicy::Ask);
        assert!(h.jump_chain.is_empty());
    }
}
