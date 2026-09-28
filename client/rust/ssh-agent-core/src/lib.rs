//! # cc-ssh-agent-core — built-in SSH agent and OS agent client
//!
//! * [`AgentService`] / [`start_agent`]: an SSH agent serving keys that
//!   live only in memory (Unix socket 0600 in a private 0700 directory with a
//!   peer-uid check; Windows named pipe, local clients only). Used by the
//!   OpenSSH fallback so decrypted keys never touch the disk.
//! * [`prepare_openssh`]: builds an OpenSSH invocation for a
//!   `ConnectionPlan` backed by a per-session built-in agent.
//! * OS agent client: [`os_agent`] re-exports ssh-core's client
//!   (`SSH_AUTH_SOCK`, `\\.\pipe\openssh-ssh-agent`, Pageant) and lists
//!   identities.

pub mod launcher;
pub mod server;

pub use launcher::{collect_agent_keys, prepare_openssh, LaunchOptions, OpenSshLaunch};
pub use server::{
    start_agent, AgentHandle, AgentKey, AgentListenOptions, AgentService, SignConfirm,
};

/// OS / external agent client (implemented in ssh-core, which needs it for
/// native agent authentication).
pub mod os_agent {
    pub use cc_ssh_core::agent::{
        connect_agent, default_agent_path, describe_identity, list_agent_identities,
        AgentIdentityInfo, DynAgentClient, WINDOWS_OPENSSH_AGENT_PIPE,
    };
}

/// Agent errors (never contain key material).
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum AgentError {
    #[error("key error: {0}")]
    Key(String),
    #[error("credential error: {0}")]
    Credential(String),
    #[error("agent protocol error: {0}")]
    Protocol(String),
    #[error("unsupported: {0}")]
    Unsupported(String),
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
}

#[cfg(test)]
mod tests;
