//! Docker Engine API over any byte stream: the local Unix socket or the
//! remote `/var/run/docker.sock` reached through an SSH `direct-streamlocal`
//! channel. Every request opens its own stream, so a slow log follow never
//! blocks listing or actions.
use async_trait::async_trait;
use serde::Serialize;
use std::{fmt, sync::Arc};
use tokio::io::{AsyncRead, AsyncWrite};

mod client;
mod logs;
pub use client::{ContainerSummary, DockerClient, EngineInfo, ExecSession, LogLine, PortMapping};

pub trait DockerStream: AsyncRead + AsyncWrite + Unpin + Send {}
impl<T: AsyncRead + AsyncWrite + Unpin + Send> DockerStream for T {}
pub type DockerIo = Box<dyn DockerStream>;

#[derive(Debug, thiserror::Error)]
pub enum DockerError {
    /// The socket exists but this user may not open it.
    #[error("permission denied for the Docker socket")]
    PermissionDenied,
    /// No socket or daemon.
    #[error("Docker is not running")]
    NotRunning,
    #[error("connection failed")]
    Connection,
    #[error("not found")]
    NotFound,
    /// The engine refused the request (status and its short message).
    #[error("Docker API error {status}")]
    Api { status: u16, message: String },
    #[error("invalid request")]
    InvalidRequest,
    #[error("protocol error")]
    Protocol,
    #[error("cancelled")]
    Cancelled,
}

impl DockerError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::PermissionDenied => "permission_denied",
            Self::NotRunning => "not_running",
            Self::Connection => "connection_failed",
            Self::NotFound => "not_found",
            Self::Api { .. } => "api_error",
            Self::InvalidRequest => "invalid_request",
            Self::Protocol => "protocol_error",
            Self::Cancelled => "cancelled",
        }
    }
}

/// Opens one fresh stream to the engine socket per call.
#[async_trait]
pub trait DockerConnector: Send + Sync + fmt::Debug {
    async fn connect(&self) -> Result<DockerIo, DockerError>;
}

/// The local engine socket.
#[cfg(unix)]
#[derive(Debug, Clone)]
pub struct UnixConnector {
    pub path: std::path::PathBuf,
}

#[cfg(unix)]
#[async_trait]
impl DockerConnector for UnixConnector {
    async fn connect(&self) -> Result<DockerIo, DockerError> {
        match tokio::net::UnixStream::connect(&self.path).await {
            Ok(stream) => Ok(Box::new(stream)),
            Err(error) => Err(match error.kind() {
                std::io::ErrorKind::PermissionDenied => DockerError::PermissionDenied,
                std::io::ErrorKind::NotFound | std::io::ErrorKind::ConnectionRefused => {
                    DockerError::NotRunning
                }
                _ => DockerError::Connection,
            }),
        }
    }
}

/// The local socket from `DOCKER_HOST` (unix only), the user's runtime
/// sockets (Docker Desktop, OrbStack, Colima, Rancher, Podman) or
/// `/var/run/docker.sock`; the first that exists.
#[cfg(unix)]
pub fn local_socket() -> Option<std::path::PathBuf> {
    use std::path::PathBuf;
    let mut candidates = Vec::new();
    if let Ok(host) = std::env::var("DOCKER_HOST") {
        if let Some(path) = host.strip_prefix("unix://") {
            candidates.push(PathBuf::from(path));
        }
    }
    if let Some(home) = std::env::var_os("HOME").map(PathBuf::from) {
        for relative in [
            ".docker/run/docker.sock",
            ".orbstack/run/docker.sock",
            ".colima/default/docker.sock",
            ".rd/docker.sock",
            ".local/share/containers/podman/machine/podman.sock",
        ] {
            candidates.push(home.join(relative));
        }
    }
    if let Some(runtime) = std::env::var_os("XDG_RUNTIME_DIR").map(PathBuf::from) {
        candidates.push(runtime.join("docker.sock"));
        candidates.push(runtime.join("podman/podman.sock"));
    }
    candidates.push(PathBuf::from("/var/run/docker.sock"));
    candidates.into_iter().find(|path| path.exists())
}

/// Environment entries whose name looks secret have their value hidden.
pub fn sensitive_key(name: &str) -> bool {
    let name = name.to_ascii_lowercase();
    [
        "password",
        "passwd",
        "pwd",
        "token",
        "secret",
        "api_key",
        "apikey",
        "private_key",
        "credential",
        "access_key",
        "auth",
    ]
    .iter()
    .any(|marker| name.contains(marker))
}

/// Shared handle used by the runtime.
pub type SharedConnector = Arc<dyn DockerConnector>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ContainerAction {
    Start,
    Stop,
    Restart,
    Pause,
    Unpause,
    Kill,
    Remove,
}

impl ContainerAction {
    pub fn parse(value: &str) -> Option<Self> {
        Some(match value {
            "start" => Self::Start,
            "stop" => Self::Stop,
            "restart" => Self::Restart,
            "pause" => Self::Pause,
            "unpause" => Self::Unpause,
            "kill" => Self::Kill,
            "remove" => Self::Remove,
            _ => return None,
        })
    }
}
