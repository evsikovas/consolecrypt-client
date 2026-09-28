//! Consumer-side contracts of ssh-core (CLIENT_ARCHITECTURE §3).
//!
//! `app-core` implements these on top of the unlocked vault / local storage.
//! ssh-core never sees the vault itself: plaintext inventory objects come
//! through [`InventoryLookup`] and secret material *only* through
//! [`CredentialResolver`].

use async_trait::async_trait;
use cc_models::credential::{Credential, CredentialKind};
use cc_models::group::Group;
use cc_models::host::{Host, JumpProfile, Proxy};
use cc_models::known_host::KnownHost;
use cc_models::tunnel::Tunnel;
use cc_models::ObjectId;
use secrecy::SecretString;
use std::fmt;
use std::path::PathBuf;

/// Read-only access to plaintext inventory objects (no secrets).
#[async_trait]
pub trait InventoryLookup: Send + Sync {
    async fn host(&self, id: ObjectId) -> Option<Host>;
    async fn group(&self, id: ObjectId) -> Option<Group>;
    async fn jump_profile(&self, id: ObjectId) -> Option<JumpProfile>;
    async fn proxy(&self, id: ObjectId) -> Option<Proxy>;
    async fn credential(&self, id: ObjectId) -> Option<Credential>;
    /// Tunnel profiles attached to a host (become `ConnectionPlan::forwards`).
    async fn tunnels_for_host(&self, _host_id: ObjectId) -> Vec<Tunnel> {
        Vec::new()
    }
}

/// Secret material for one credential. Every secret is wrapped in
/// [`SecretString`] (zeroized on drop, redacted `Debug`).
#[derive(Clone)]
pub enum ResolvedCredential {
    /// Password authentication (also used to answer keyboard-interactive
    /// password prompts).
    Password(SecretString),
    /// OpenSSH (or legacy PEM/PKCS#8) private key text, exactly as stored —
    /// possibly still passphrase-protected. It is decrypted in memory only.
    PrivateKey {
        openssh: SecretString,
        /// Remembered passphrase ("Remember SSH key passphrase"), if any.
        passphrase: Option<SecretString>,
        /// OpenSSH certificate line (not secret). Overrides
        /// `Credential::certificate` when set.
        certificate: Option<String>,
    },
    /// Keys held by an SSH agent. `None` = the OS default agent
    /// (`SSH_AUTH_SOCK`, Windows `\\.\pipe\openssh-ssh-agent`).
    Agent { socket: Option<PathBuf> },
}

impl ResolvedCredential {
    /// Agent credential derived from the credential model alone (agent kinds
    /// carry no secret). Resolvers may use it for `OsSshAgent` /
    /// `ExternalAgent`.
    pub fn agent_for(credential: &Credential) -> Option<Self> {
        match credential.kind {
            CredentialKind::OsSshAgent => Some(ResolvedCredential::Agent {
                socket: credential.agent_path.as_ref().map(PathBuf::from),
            }),
            CredentialKind::ExternalAgent => Some(ResolvedCredential::Agent {
                socket: credential.agent_path.as_ref().map(PathBuf::from),
            }),
            _ => None,
        }
    }
}

impl fmt::Debug for ResolvedCredential {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ResolvedCredential::Password(_) => {
                f.write_str("ResolvedCredential::Password(<redacted>)")
            }
            ResolvedCredential::PrivateKey {
                passphrase,
                certificate,
                ..
            } => f
                .debug_struct("ResolvedCredential::PrivateKey")
                .field("openssh", &"<redacted>")
                .field("passphrase", &passphrase.as_ref().map(|_| "<redacted>"))
                .field("certificate", &certificate.is_some())
                .finish(),
            ResolvedCredential::Agent { socket } => f
                .debug_struct("ResolvedCredential::Agent")
                .field("socket", socket)
                .finish(),
        }
    }
}

/// Why a credential could not be resolved.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum ResolveError {
    #[error("the vault is locked")]
    Locked,
    #[error("secret {0} not found")]
    SecretNotFound(ObjectId),
    #[error("credential {0} has no secret attached")]
    NoSecret(ObjectId),
    #[error("cancelled by the user")]
    Cancelled,
    #[error("credential kind {0:?} is not supported")]
    Unsupported(CredentialKind),
    #[error("{0}")]
    Other(String),
}

/// The ONLY path from ssh-core to secret material.
#[async_trait]
pub trait CredentialResolver: Send + Sync {
    /// Resolve the secret material of `credential`. Implementations may
    /// prompt the user (e.g. for a password that is not stored).
    async fn resolve(&self, credential: &Credential) -> Result<ResolvedCredential, ResolveError>;

    /// Ask the user for the passphrase of an encrypted private key when none
    /// is remembered. `Ok(None)` = not available (the connect fails with
    /// [`crate::SshError::PassphraseRequired`]). `attempt` starts at 0 and is
    /// incremented after a wrong passphrase.
    async fn prompt_passphrase(
        &self,
        _credential: &Credential,
        _attempt: u32,
    ) -> Result<Option<SecretString>, ResolveError> {
        Ok(None)
    }

    /// Password of a SOCKS5 / HTTP proxy (`Proxy::password_secret_id`).
    async fn resolve_proxy_password(
        &self,
        _proxy: &Proxy,
    ) -> Result<Option<SecretString>, ResolveError> {
        Ok(None)
    }
}

/// Known-hosts store failure.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{0}")]
pub struct KnownHostsError(pub String);

/// Persistent known-hosts database (synced E2EE by app-core).
#[async_trait]
pub trait KnownHostsStore: Send + Sync {
    /// Every entry whose `host_pattern` matches `host:port`, including
    /// `@cert-authority` (source `CertAuthority`) and `@revoked` entries.
    /// Use [`crate::known_hosts::pattern_matches`] for OpenSSH semantics.
    async fn lookup(&self, host: &str, port: u16) -> Result<Vec<KnownHost>, KnownHostsError>;
    /// Record a key (TOFU acceptance, manual add or import).
    async fn add(&self, entry: KnownHost) -> Result<(), KnownHostsError>;
    /// Mark every entry with this `SHA256:` fingerprint as revoked.
    async fn revoke(&self, fingerprint_sha256: &str) -> Result<(), KnownHostsError>;
    /// Delete an entry (e.g. after the user verified a legitimately changed key).
    async fn remove(&self, id: ObjectId) -> Result<(), KnownHostsError>;
}

/// Information shown to the user when a host key is not yet known.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct HostKeyInfo {
    /// Host as dialed (address of the hop).
    pub host: String,
    pub port: u16,
    /// Canonical known-hosts pattern (`host` or `[host]:port`).
    pub host_pattern: String,
    /// Inventory host this hop belongs to.
    pub host_id: Option<ObjectId>,
    pub host_name: Option<String>,
    /// 0-based position in the route (target = `hop_count - 1`).
    pub hop_index: usize,
    pub hop_count: usize,
    /// e.g. `ssh-ed25519`.
    pub key_type: String,
    /// `SHA256:…`
    pub fingerprint_sha256: String,
    /// Base64 key blob as written in known_hosts.
    pub public_key: String,
    /// Key types already known for this host (the server offered another
    /// type). Non-empty = be extra careful.
    pub other_known_key_types: Vec<String>,
}

/// The user's decision about an unknown host key.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum HostKeyDecision {
    /// Trust and record in Known Hosts.
    AcceptAndSave,
    /// Trust for this connection only.
    AcceptOnce,
    /// Abort the connection.
    Reject,
}

/// UI decision for unknown host keys (policy `Ask`).
#[async_trait]
pub trait HostKeyPrompt: Send + Sync {
    async fn confirm_unknown(&self, info: HostKeyInfo) -> HostKeyDecision;
}
