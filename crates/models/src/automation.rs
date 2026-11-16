//! Protocol DTOs, not new vault object kinds. Sync capability negotiation is
//! required separately before these objects can be added to the encrypted vault.
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, HashSet};
use uuid::Uuid;

pub const JOB_SIGNATURE_DOMAIN: &[u8] = b"ConsoleCrypt JobManifest v1\0";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct JobPolicy {
    pub strict_isolation: bool,
    pub cancel_on_lock: bool,
    pub timeout_seconds: u32,
    pub forks: u8,
    pub production: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ApprovedTarget {
    pub host_id: Uuid,
    pub alias: String,
    pub address: String,
    pub port: u16,
    pub username: String,
    pub route_digest: String,
    pub credential_refs: Vec<Uuid>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct JobManifest {
    pub version: u8,
    pub job_id: Uuid,
    pub tenant_id: Uuid,
    pub runner_id: Uuid,
    pub project_id: Uuid,
    pub revision_digest: String,
    pub inventory_digest: String,
    pub environment_digest: String,
    pub parameters_digest: String,
    pub playbook: String,
    pub policy: JobPolicy,
    pub mode: JobMode,
    pub targets: Vec<ApprovedTarget>,
    pub issued_at: i64,
    pub expires_at: i64,
    pub access_expires_at: i64,
    pub nonce: Uuid,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JobMode {
    Check,
    Apply,
}

pub fn is_digest(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

fn canonical(value: Value) -> Value {
    match value {
        Value::Object(object) => Value::Object(
            object
                .into_iter()
                .collect::<BTreeMap<_, _>>()
                .into_iter()
                .map(|(key, value)| (key, canonical(value)))
                .collect(),
        ),
        Value::Array(array) => Value::Array(array.into_iter().map(canonical).collect()),
        other => other,
    }
}

/// Shared selected-playbook syntax only; this validates no job or permission.
pub fn validate_selected_playbook(playbook: &str) -> Result<(), &'static str> {
    let parts: Vec<_> = playbook.split('/').collect();
    if playbook.is_empty()
        || playbook.len() > 1024
        || parts.len() > 32
        || parts
            .iter()
            .any(|part| part.is_empty() || *part == "." || *part == ".." || part.len() > 255)
        || parts[0] == ".git"
        || playbook
            .chars()
            .any(|c| c.is_control() || c == '\\' || c == ':')
        || !(playbook.ends_with(".yml") || playbook.ends_with(".yaml"))
    {
        return Err("invalid selected playbook");
    }
    Ok(())
}

impl JobManifest {
    pub fn canonical_bytes(&self) -> Result<Vec<u8>, &'static str> {
        if self.version != 1
            || ![
                &self.revision_digest,
                &self.inventory_digest,
                &self.environment_digest,
                &self.parameters_digest,
            ]
            .into_iter()
            .all(|value| is_digest(value))
        {
            return Err("invalid manifest version or digest");
        }
        validate_selected_playbook(&self.playbook)?;
        if !(1..=5).contains(&self.policy.forks)
            || !(1..=86400).contains(&self.policy.timeout_seconds)
            || self.expires_at <= self.issued_at
            || self
                .expires_at
                .checked_sub(self.issued_at)
                .is_none_or(|age| age > 600)
            || self.targets.is_empty()
            || self.targets.len() > 10000
        {
            return Err("invalid job limits");
        }
        if self.access_expires_at <= self.issued_at
            || self
                .issued_at
                .checked_add(i64::from(self.policy.timeout_seconds))
                .is_none_or(|maximum| self.access_expires_at > maximum)
        {
            return Err("invalid job access lease");
        }
        let mut hosts = HashSet::new();
        let mut aliases = HashSet::new();
        for target in &self.targets {
            if !hosts.insert(target.host_id)
                || !aliases.insert(&target.alias)
                || target.alias.is_empty()
                || target.alias.len() > 96
                || !(target.alias.as_bytes()[0].is_ascii_alphabetic() || target.alias.starts_with('_'))
                || !target
                    .alias
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
                || target.address.is_empty()
                || target.address.len() > 253
                || target.address.starts_with('-')
                || target
                    .address
                    .chars()
                    .any(|c| c.is_whitespace() || c.is_control() || c == '/' || c == '@')
                || target.port == 0
                || target.username.is_empty()
                || target.username.len() > 128
                || target.username.starts_with('-')
                || !target
                    .username
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"_.-@\\".contains(&b))
                || !is_digest(&target.route_digest)
                || target.credential_refs.len() > 64
                || target.credential_refs.iter().collect::<HashSet<_>>().len()
                    != target.credential_refs.len()
            {
                return Err("invalid or duplicate approved target");
            }
        }
        let raw = serde_json::to_vec(&canonical(
            serde_json::to_value(self).map_err(|_| "manifest serialization")?,
        ))
        .map_err(|_| "manifest serialization")?;
        if raw.len() > 1024 * 1024 - 1024 {
            return Err("manifest exceeds wire budget");
        }
        Ok(raw)
    }
}
