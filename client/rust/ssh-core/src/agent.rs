//! Client side of the SSH agent protocol: connect to the OS agent
//! (`SSH_AUTH_SOCK` on Unix, `\\.\pipe\openssh-ssh-agent` / Pageant on
//! Windows) or to an explicit socket / pipe (external agents such as
//! 1Password or Secretive, or our own built-in agent).

use crate::error::SshError;
use crate::keys::{fingerprint_sha256, key_blob};
use russh::keys::agent::client::{AgentClient, AgentStream};
use russh::keys::agent::AgentIdentity;
use std::path::{Path, PathBuf};

/// Type-erased agent client usable with any transport.
pub type DynAgentClient = AgentClient<Box<dyn AgentStream + Send + Unpin + 'static>>;

/// Default Windows OpenSSH agent pipe.
pub const WINDOWS_OPENSSH_AGENT_PIPE: &str = r"\\.\pipe\openssh-ssh-agent";

/// Path of the default OS agent, if one is configured.
pub fn default_agent_path() -> Option<PathBuf> {
    if let Some(v) = std::env::var_os("SSH_AUTH_SOCK").filter(|v| !v.is_empty()) {
        return Some(PathBuf::from(v));
    }
    if cfg!(windows) {
        return Some(PathBuf::from(WINDOWS_OPENSSH_AGENT_PIPE));
    }
    None
}

/// Connect to an agent. `socket = None` → the OS default agent.
pub async fn connect_agent(socket: Option<&Path>) -> Result<DynAgentClient, SshError> {
    let path = match socket {
        Some(p) => p.to_path_buf(),
        None => default_agent_path().ok_or_else(|| {
            SshError::Agent("no SSH agent configured (SSH_AUTH_SOCK unset)".into())
        })?,
    };
    connect_path(&path).await
}

#[cfg(unix)]
async fn connect_path(path: &Path) -> Result<DynAgentClient, SshError> {
    let client = AgentClient::connect_uds(path).await.map_err(|e| {
        SshError::Agent(format!(
            "cannot connect to agent at {}: {e}",
            path.display()
        ))
    })?;
    Ok(client.dynamic())
}

#[cfg(windows)]
async fn connect_path(path: &Path) -> Result<DynAgentClient, SshError> {
    let s = path.as_os_str();
    if s.to_string_lossy().eq_ignore_ascii_case("pageant") {
        let client = AgentClient::connect_pageant()
            .await
            .map_err(|e| SshError::Agent(format!("cannot connect to Pageant: {e}")))?;
        return Ok(client.dynamic());
    }
    let client = AgentClient::connect_named_pipe(s).await.map_err(|e| {
        SshError::Agent(format!(
            "cannot connect to agent pipe {}: {e}",
            path.display()
        ))
    })?;
    Ok(client.dynamic())
}

#[cfg(not(any(unix, windows)))]
async fn connect_path(path: &Path) -> Result<DynAgentClient, SshError> {
    Err(SshError::Unsupported(format!(
        "SSH agents are not supported on this platform ({})",
        path.display()
    )))
}

/// Non-secret description of an identity held by an agent.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct AgentIdentityInfo {
    pub algorithm_name: String,
    pub fingerprint_sha256: String,
    pub comment: String,
    /// OpenSSH public key (or certificate) line.
    pub openssh: String,
    pub is_certificate: bool,
}

/// Describe an agent identity.
pub fn describe_identity(id: &AgentIdentity) -> AgentIdentityInfo {
    match id {
        AgentIdentity::PublicKey { key, comment } => AgentIdentityInfo {
            algorithm_name: key.algorithm().as_str().to_string(),
            fingerprint_sha256: fingerprint_sha256(&key_blob(key.key_data())),
            comment: comment.clone(),
            openssh: key.to_openssh().unwrap_or_default(),
            is_certificate: false,
        },
        AgentIdentity::Certificate {
            certificate,
            comment,
        } => AgentIdentityInfo {
            algorithm_name: certificate.algorithm().to_certificate_type(),
            fingerprint_sha256: fingerprint_sha256(&key_blob(certificate.public_key())),
            comment: comment.clone(),
            openssh: certificate.to_openssh().unwrap_or_default(),
            is_certificate: true,
        },
    }
}

/// List the identities of an agent (`None` = OS default agent).
pub async fn list_agent_identities(
    socket: Option<&Path>,
) -> Result<Vec<AgentIdentityInfo>, SshError> {
    let mut client = connect_agent(socket).await?;
    let ids = client
        .request_identities()
        .await
        .map_err(|e| SshError::Agent(e.to_string()))?;
    Ok(ids.iter().map(describe_identity).collect())
}
