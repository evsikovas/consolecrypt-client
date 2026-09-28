//! Error types of the SSH core.
//!
//! Error messages never contain secret material (passwords, passphrases,
//! private keys). Host names, ports, user names and public-key fingerprints
//! are considered non-secret and are included to make failures actionable.

use crate::planner::PlanError;
use crate::traits::{KnownHostsError, ResolveError};
use cc_models::ObjectId;

/// Errors produced while connecting, authenticating or using a session.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum SshError {
    /// The connection plan could not be built.
    #[error("connection plan error: {0}")]
    Plan(#[from] PlanError),

    /// TCP connection to the first hop (or proxy) failed.
    #[error("could not connect to {host}:{port}: {source}")]
    Connect {
        host: String,
        port: u16,
        #[source]
        source: std::io::Error,
    },

    /// A step of connection establishment exceeded the configured timeout.
    #[error("timed out while {stage} ({host}:{port})")]
    Timeout {
        stage: &'static str,
        host: String,
        port: u16,
    },

    /// SOCKS5 / HTTP CONNECT proxy failure.
    #[error("proxy error: {0}")]
    Proxy(String),

    /// A jump host refused or failed to open `direct-tcpip` to the next hop.
    #[error("jump host {via} could not open a channel to {host}:{port}: {reason}")]
    JumpChannel {
        via: String,
        host: String,
        port: u16,
        reason: String,
    },

    /// The server presented a key different from the one recorded for it.
    /// This is always a hard failure (possible man-in-the-middle attack).
    #[error(
        "REMOTE HOST IDENTIFICATION HAS CHANGED for {host}:{port}: server presented {key_type} \
         {actual_fingerprint}, known keys are [{}]. Someone could be eavesdropping on you \
         (man-in-the-middle attack), or the host key has just been changed. Connection refused; \
         remove the old key from Known Hosts only if you have verified the new fingerprint.",
        expected_fingerprints.join(", ")
    )]
    HostKeyChanged {
        host: String,
        port: u16,
        key_type: String,
        actual_fingerprint: String,
        expected_fingerprints: Vec<String>,
    },

    /// Unknown host key while the policy is `Strict`.
    #[error(
        "host key for {host}:{port} is not in Known Hosts and the host key policy is strict \
         ({key_type} {fingerprint})"
    )]
    HostKeyUnknown {
        host: String,
        port: u16,
        key_type: String,
        fingerprint: String,
    },

    /// The user rejected an unknown host key.
    #[error("host key for {host}:{port} was rejected ({fingerprint})")]
    HostKeyRejected {
        host: String,
        port: u16,
        fingerprint: String,
    },

    /// The host key (or its signing CA) is marked `@revoked`.
    #[error("host key for {host}:{port} is REVOKED ({fingerprint}); connection refused")]
    HostKeyRevoked {
        host: String,
        port: u16,
        fingerprint: String,
    },

    /// Authentication was rejected by the server.
    #[error("authentication failed for {user}@{host}: {detail}")]
    AuthFailed {
        user: String,
        host: String,
        detail: String,
        /// Methods the server said can continue (e.g. `publickey`, `password`).
        remaining_methods: Vec<String>,
    },

    /// The private key is passphrase-protected and no passphrase is available.
    #[error("the private key of credential {credential_id:?} is passphrase-protected; a passphrase is required")]
    PassphraseRequired { credential_id: Option<ObjectId> },

    /// The supplied passphrase does not decrypt the private key.
    #[error("wrong passphrase for the private key of credential {credential_id:?}")]
    WrongPassphrase { credential_id: Option<ObjectId> },

    /// Key parsing / generation error.
    #[error("key error: {0}")]
    Key(#[from] crate::keys::KeyError),

    /// Secret resolution failed (vault locked, secret missing, user cancelled…).
    #[error("credential error: {0}")]
    Credential(#[from] ResolveError),

    /// Known-hosts store failure.
    #[error("known hosts store error: {0}")]
    KnownHosts(#[from] KnownHostsError),

    /// The requested feature/credential kind is not supported by this backend.
    #[error("unsupported: {0}")]
    Unsupported(String),

    /// OS / external SSH agent failure.
    #[error("SSH agent error: {0}")]
    Agent(String),

    /// Channel open / request failure.
    #[error("channel error: {0}")]
    Channel(String),

    /// The session is closed.
    #[error("the SSH session is closed")]
    Disconnected,

    /// Low-level SSH protocol error from russh.
    #[error("SSH protocol error: {0}")]
    Protocol(#[from] russh::Error),

    /// I/O error.
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
}

impl SshError {
    /// `true` for host-key verification failures that must never be retried
    /// automatically (changed / revoked / rejected / unknown under strict).
    pub fn is_host_key_error(&self) -> bool {
        matches!(
            self,
            SshError::HostKeyChanged { .. }
                | SshError::HostKeyUnknown { .. }
                | SshError::HostKeyRejected { .. }
                | SshError::HostKeyRevoked { .. }
        )
    }

    /// `true` when a reconnect attempt may succeed (network-level failures).
    pub fn is_transient(&self) -> bool {
        matches!(
            self,
            SshError::Connect { .. }
                | SshError::Timeout { .. }
                | SshError::Disconnected
                | SshError::Io(_)
                | SshError::Proxy(_)
        ) || matches!(
            self,
            SshError::Protocol(
                russh::Error::Disconnect
                    | russh::Error::HUP
                    | russh::Error::ConnectionTimeout
                    | russh::Error::KeepaliveTimeout
                    | russh::Error::InactivityTimeout
                    | russh::Error::IO(_)
            )
        )
    }
}

impl From<russh::keys::Error> for SshError {
    fn from(e: russh::keys::Error) -> Self {
        SshError::Protocol(russh::Error::Keys(e))
    }
}
