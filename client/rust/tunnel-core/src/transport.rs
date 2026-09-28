//! What a tunnel needs from an SSH connection, as a trait so the tunnel
//! logic is testable without a server. Implemented for
//! [`cc_ssh_core::SshSession`] (which already carries the whole jump chain).

use async_trait::async_trait;
use cc_ssh_core::{SshError, SshSession};
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::sync::mpsc;

/// Byte stream usable by the tunnel pumps.
pub trait AsyncStream: AsyncRead + AsyncWrite + Send + Unpin {}
impl<T: AsyncRead + AsyncWrite + Send + Unpin> AsyncStream for T {}

/// Boxed stream.
pub type BoxStream = Box<dyn AsyncStream>;

/// A connection accepted on the remote side of a remote forward.
pub struct IncomingConnection {
    pub stream: BoxStream,
    /// Remote peer address/port as reported by the server.
    pub originator: (String, u16),
}

impl std::fmt::Debug for IncomingConnection {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("IncomingConnection")
            .field("originator", &self.originator)
            .finish_non_exhaustive()
    }
}

/// An active remote listener.
#[derive(Debug)]
pub struct RemoteListener {
    pub bound_port: u16,
    pub incoming: mpsc::Receiver<IncomingConnection>,
}

/// Transport errors (stringly: the SSH layer already produced a precise message).
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{0}")]
pub struct TransportError(pub String);

impl From<SshError> for TransportError {
    fn from(e: SshError) -> Self {
        TransportError(e.to_string())
    }
}

/// SSH-side operations used by tunnels.
#[async_trait]
pub trait TunnelTransport: Send + Sync {
    /// `direct-tcpip` to `host:port` (as seen from the SSH target).
    async fn open_direct(
        &self,
        host: &str,
        port: u16,
        originator: SocketAddr,
    ) -> Result<BoxStream, TransportError>;
    /// `tcpip-forward` on the SSH target.
    async fn remote_forward(
        &self,
        bind_host: &str,
        bind_port: u16,
    ) -> Result<RemoteListener, TransportError>;
    /// Cancel a remote forward.
    async fn cancel_remote_forward(
        &self,
        bind_host: &str,
        bound_port: u16,
    ) -> Result<(), TransportError>;
    /// Resolves when the underlying connection is gone.
    async fn closed(&self);
    fn is_closed(&self) -> bool;
}

#[async_trait]
impl TunnelTransport for SshSession {
    async fn open_direct(
        &self,
        host: &str,
        port: u16,
        originator: SocketAddr,
    ) -> Result<BoxStream, TransportError> {
        let s = self.open_direct_tcpip(host, port, Some(originator)).await?;
        Ok(Box::new(s))
    }

    async fn remote_forward(
        &self,
        bind_host: &str,
        bind_port: u16,
    ) -> Result<RemoteListener, TransportError> {
        let mut fwd = self.request_remote_forward(bind_host, bind_port).await?;
        let (tx, rx) = mpsc::channel(64);
        tokio::spawn(async move {
            while let Some(c) = fwd.incoming.recv().await {
                let conn = IncomingConnection {
                    stream: Box::new(c.stream),
                    originator: c.originator,
                };
                if tx.send(conn).await.is_err() {
                    break;
                }
            }
        });
        Ok(RemoteListener {
            bound_port: fwd.bound_port,
            incoming: rx,
        })
    }

    async fn cancel_remote_forward(
        &self,
        bind_host: &str,
        bound_port: u16,
    ) -> Result<(), TransportError> {
        Ok(SshSession::cancel_remote_forward(self, bind_host, bound_port).await?)
    }

    async fn closed(&self) {
        SshSession::closed(self).await
    }

    fn is_closed(&self) -> bool {
        SshSession::is_closed(self)
    }
}

#[async_trait]
impl<T: TunnelTransport + ?Sized> TunnelTransport for Arc<T> {
    async fn open_direct(
        &self,
        host: &str,
        port: u16,
        originator: SocketAddr,
    ) -> Result<BoxStream, TransportError> {
        (**self).open_direct(host, port, originator).await
    }
    async fn remote_forward(
        &self,
        bind_host: &str,
        bind_port: u16,
    ) -> Result<RemoteListener, TransportError> {
        (**self).remote_forward(bind_host, bind_port).await
    }
    async fn cancel_remote_forward(
        &self,
        bind_host: &str,
        bound_port: u16,
    ) -> Result<(), TransportError> {
        (**self).cancel_remote_forward(bind_host, bound_port).await
    }
    async fn closed(&self) {
        (**self).closed().await
    }
    fn is_closed(&self) -> bool {
        (**self).is_closed()
    }
}
