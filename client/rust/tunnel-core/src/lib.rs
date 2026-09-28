//! # cc-tunnel-core — port forwarding over ssh-core connections
//!
//! * **Local**: `bind_host:bind_port` (here) → `direct-tcpip` →
//!   `target_host:target_port` (as seen from the SSH target).
//! * **Remote**: `tcpip-forward` on the SSH target (`bind_host:bind_port`
//!   there) → `forwarded-tcpip` channels → `target_host:target_port` (here).
//! * **Dynamic**: local SOCKS5 server (no-auth, CONNECT, IPv4/IPv6/domain)
//!   → `direct-tcpip`.
//!
//! Tunnels run over any [`TunnelTransport`] — normally an
//! `Arc<cc_ssh_core::SshSession>` (so jump chains are transparent).
//! [`TunnelManager`] tracks start/stop, status, per-tunnel counters and
//! warns when a tunnel binds to a non-loopback address.

pub mod socks5;
pub mod transport;

pub use transport::{
    AsyncStream, BoxStream, IncomingConnection, RemoteListener, TransportError, TunnelTransport,
};

use cc_models::tunnel::{Tunnel, TunnelKind};
use cc_models::ObjectId;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::net::{IpAddr, SocketAddr};
use std::pin::Pin;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::task::{Context, Poll};
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{broadcast, watch};
use tokio::task::JoinSet;

/// Tunnel errors.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum TunnelError {
    #[error("invalid tunnel: {0}")]
    Invalid(String),
    #[error("tunnel {0} is already running")]
    AlreadyRunning(ObjectId),
    #[error("tunnel {0} is not running")]
    NotRunning(ObjectId),
    #[error("cannot bind {addr}: {reason}")]
    Bind { addr: String, reason: String },
    #[error("SSH transport error: {0}")]
    Transport(#[from] TransportError),
}

/// Lifecycle state.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum TunnelState {
    Running,
    /// Stopped because of an error (e.g. the SSH session closed).
    Failed(String),
    Stopped,
}

/// Snapshot of a tunnel for the UI.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TunnelStatus {
    pub id: ObjectId,
    pub name: String,
    pub kind: TunnelKind,
    pub state: TunnelState,
    /// Actual listening address (local/dynamic: local socket; remote:
    /// `bind_host:bound_port` on the server).
    pub listen: String,
    /// Bytes from local clients towards the SSH side.
    pub bytes_sent: u64,
    /// Bytes from the SSH side towards local clients.
    pub bytes_received: u64,
    pub active_connections: u64,
    pub total_connections: u64,
    pub failed_connections: u64,
    pub warnings: Vec<String>,
    pub started_at: chrono::DateTime<chrono::Utc>,
}

/// Status change notifications.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TunnelEvent {
    Started(ObjectId),
    Stopped(ObjectId),
    Failed { id: ObjectId, reason: String },
}

#[derive(Debug, Default)]
struct Stats {
    sent: AtomicU64,
    received: AtomicU64,
    active: AtomicU64,
    total: AtomicU64,
    failed: AtomicU64,
}

struct Running {
    spec: Tunnel,
    listen: String,
    warnings: Vec<String>,
    started_at: chrono::DateTime<chrono::Utc>,
    stats: Arc<Stats>,
    state: Arc<Mutex<TunnelState>>,
    shutdown: watch::Sender<bool>,
    task: tokio::task::JoinHandle<()>,
    remote_cancel: Option<(Arc<dyn TunnelTransport>, String, u16)>,
}

/// Is `host` a loopback name/address?
pub fn is_loopback_host(host: &str) -> bool {
    let h = host.trim().trim_start_matches('[').trim_end_matches(']');
    if h.eq_ignore_ascii_case("localhost") {
        return true;
    }
    h.parse::<IpAddr>().is_ok_and(|ip| ip.is_loopback())
}

/// Warnings for a tunnel definition (non-loopback binds).
pub fn tunnel_warnings(t: &Tunnel) -> Vec<String> {
    let mut w = Vec::new();
    if !is_loopback_host(&t.bind_host) {
        match t.kind {
            TunnelKind::Remote => w.push(format!(
                "remote tunnel '{}' binds to {} on the server: it may be reachable from the server's network (depends on GatewayPorts)",
                t.name, t.bind_host
            )),
            _ => w.push(format!(
                "tunnel '{}' binds to {} (not loopback): it is reachable from the network",
                t.name, t.bind_host
            )),
        }
    }
    w
}

/// Stream wrapper counting bytes in both directions.
struct Counted<S> {
    inner: S,
    stats: Arc<Stats>,
}

impl<S: AsyncRead + Unpin> AsyncRead for Counted<S> {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        let before = buf.filled().len();
        let r = Pin::new(&mut self.inner).poll_read(cx, buf);
        let n = buf.filled().len() - before;
        if n > 0 {
            self.stats.sent.fetch_add(n as u64, Ordering::Relaxed);
        }
        r
    }
}

impl<S: AsyncWrite + Unpin> AsyncWrite for Counted<S> {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<std::io::Result<usize>> {
        let r = Pin::new(&mut self.inner).poll_write(cx, buf);
        if let Poll::Ready(Ok(n)) = &r {
            self.stats.received.fetch_add(*n as u64, Ordering::Relaxed);
        }
        r
    }
    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.inner).poll_flush(cx)
    }
    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.inner).poll_shutdown(cx)
    }
}

/// Pump bytes between a local stream (counted) and the SSH side.
async fn pipe<L, R>(local: L, mut remote: R, stats: Arc<Stats>)
where
    L: AsyncRead + AsyncWrite + Unpin,
    R: AsyncRead + AsyncWrite + Unpin,
{
    stats.active.fetch_add(1, Ordering::Relaxed);
    let mut local = Counted {
        inner: local,
        stats: stats.clone(),
    };
    let _ = tokio::io::copy_bidirectional(&mut local, &mut remote).await;
    stats.active.fetch_sub(1, Ordering::Relaxed);
}

/// Starts, stops and reports tunnels.
pub struct TunnelManager {
    tunnels: Mutex<HashMap<ObjectId, Running>>,
    events: broadcast::Sender<TunnelEvent>,
}

impl std::fmt::Debug for TunnelManager {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TunnelManager")
            .field("running", &self.lock().len())
            .finish()
    }
}

impl Default for TunnelManager {
    fn default() -> Self {
        Self::new()
    }
}

fn bind_addr(host: &str, port: u16) -> String {
    let h = host.trim().trim_start_matches('[').trim_end_matches(']');
    if h.contains(':') {
        format!("[{h}]:{port}")
    } else {
        format!("{h}:{port}")
    }
}

impl TunnelManager {
    pub fn new() -> Self {
        Self {
            tunnels: Mutex::new(HashMap::new()),
            events: broadcast::channel(64).0,
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<ObjectId, Running>> {
        self.tunnels.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Subscribe to start/stop/failure events.
    pub fn events(&self) -> broadcast::Receiver<TunnelEvent> {
        self.events.subscribe()
    }

    /// Start `tunnel` over `transport` (e.g. an `Arc<SshSession>`).
    pub async fn start(
        &self,
        tunnel: &Tunnel,
        transport: Arc<dyn TunnelTransport>,
    ) -> Result<TunnelStatus, TunnelError> {
        tunnel
            .validate()
            .map_err(|e| TunnelError::Invalid(e.to_string()))?;
        if self.lock().get(&tunnel.id).is_some_and(|r| {
            *r.state.lock().unwrap_or_else(PoisonError::into_inner) == TunnelState::Running
        }) {
            return Err(TunnelError::AlreadyRunning(tunnel.id));
        }
        let stats = Arc::new(Stats::default());
        let state = Arc::new(Mutex::new(TunnelState::Running));
        let (shutdown, shutdown_rx) = watch::channel(false);
        let warnings = tunnel_warnings(tunnel);
        for w in &warnings {
            tracing::warn!("{w}");
        }

        let (listen, task, remote_cancel) = match tunnel.kind {
            TunnelKind::Local | TunnelKind::Dynamic => {
                let addr = bind_addr(&tunnel.bind_host, tunnel.bind_port);
                let listener = TcpListener::bind(&addr)
                    .await
                    .map_err(|e| TunnelError::Bind {
                        addr: addr.clone(),
                        reason: e.to_string(),
                    })?;
                let local = listener.local_addr().map(|a| a.to_string()).unwrap_or(addr);
                let task = tokio::spawn(run_listener(
                    tunnel.clone(),
                    listener,
                    transport.clone(),
                    stats.clone(),
                    state.clone(),
                    shutdown_rx,
                    self.events.clone(),
                ));
                (local, task, None)
            }
            TunnelKind::Remote => {
                let rl = transport
                    .remote_forward(&tunnel.bind_host, tunnel.bind_port)
                    .await?;
                let listen = bind_addr(&tunnel.bind_host, rl.bound_port);
                let cancel = Some((transport.clone(), tunnel.bind_host.clone(), rl.bound_port));
                let task = tokio::spawn(run_remote(
                    tunnel.clone(),
                    rl,
                    transport.clone(),
                    stats.clone(),
                    state.clone(),
                    shutdown_rx,
                    self.events.clone(),
                ));
                (listen, task, cancel)
            }
        };

        let running = Running {
            spec: tunnel.clone(),
            listen,
            warnings,
            started_at: chrono::Utc::now(),
            stats,
            state,
            shutdown,
            task,
            remote_cancel,
        };
        let status = snapshot(&running);
        if let Some(old) = self.lock().insert(tunnel.id, running) {
            old.task.abort();
        }
        let _ = self.events.send(TunnelEvent::Started(tunnel.id));
        Ok(status)
    }

    /// Stop a tunnel: closes the listener / cancels the remote forward and
    /// drops its active connections.
    pub async fn stop(&self, id: ObjectId) -> Result<(), TunnelError> {
        let running = self.lock().remove(&id).ok_or(TunnelError::NotRunning(id))?;
        let _ = running.shutdown.send(true);
        if let Some((t, host, port)) = &running.remote_cancel {
            if !t.is_closed() {
                let _ = t.cancel_remote_forward(host, *port).await;
            }
        }
        running.task.abort();
        let _ = running.task.await;
        let _ = self.events.send(TunnelEvent::Stopped(id));
        Ok(())
    }

    /// Stop every tunnel.
    pub async fn stop_all(&self) {
        let ids: Vec<ObjectId> = self.lock().keys().copied().collect();
        for id in ids {
            let _ = self.stop(id).await;
        }
    }

    /// Status of one tunnel (also for failed ones until stopped).
    pub fn status(&self, id: ObjectId) -> Option<TunnelStatus> {
        self.lock().get(&id).map(snapshot)
    }

    /// Status of all tunnels, sorted by name.
    pub fn list(&self) -> Vec<TunnelStatus> {
        let mut v: Vec<TunnelStatus> = self.lock().values().map(snapshot).collect();
        v.sort_by(|a, b| a.name.cmp(&b.name));
        v
    }
}

fn snapshot(r: &Running) -> TunnelStatus {
    TunnelStatus {
        id: r.spec.id,
        name: r.spec.name.clone(),
        kind: r.spec.kind,
        state: r
            .state
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone(),
        listen: r.listen.clone(),
        bytes_sent: r.stats.sent.load(Ordering::Relaxed),
        bytes_received: r.stats.received.load(Ordering::Relaxed),
        active_connections: r.stats.active.load(Ordering::Relaxed),
        total_connections: r.stats.total.load(Ordering::Relaxed),
        failed_connections: r.stats.failed.load(Ordering::Relaxed),
        warnings: r.warnings.clone(),
        started_at: r.started_at,
    }
}

fn fail(
    state: &Mutex<TunnelState>,
    events: &broadcast::Sender<TunnelEvent>,
    id: ObjectId,
    reason: &str,
) {
    *state.lock().unwrap_or_else(PoisonError::into_inner) = TunnelState::Failed(reason.to_string());
    let _ = events.send(TunnelEvent::Failed {
        id,
        reason: reason.to_string(),
    });
}

async fn run_listener(
    tunnel: Tunnel,
    listener: TcpListener,
    transport: Arc<dyn TunnelTransport>,
    stats: Arc<Stats>,
    state: Arc<Mutex<TunnelState>>,
    mut shutdown: watch::Receiver<bool>,
    events: broadcast::Sender<TunnelEvent>,
) {
    let mut conns = JoinSet::new();
    loop {
        tokio::select! {
            _ = shutdown.changed() => break,
            _ = transport.closed() => {
                fail(&state, &events, tunnel.id, "SSH session closed");
                break;
            }
            Some(_) = conns.join_next(), if !conns.is_empty() => {}
            accepted = listener.accept() => {
                let (sock, peer) = match accepted {
                    Ok(v) => v,
                    Err(e) => {
                        tracing::debug!("accept failed: {e}");
                        continue;
                    }
                };
                let _ = sock.set_nodelay(true);
                stats.total.fetch_add(1, Ordering::Relaxed);
                let t = tunnel.clone();
                let tr = transport.clone();
                let st = stats.clone();
                conns.spawn(async move {
                    match t.kind {
                        TunnelKind::Local => handle_local(t, sock, peer, tr, st).await,
                        _ => handle_socks(sock, peer, tr, st).await,
                    }
                });
            }
        }
    }
    conns.shutdown().await;
}

async fn handle_local(
    t: Tunnel,
    sock: TcpStream,
    peer: SocketAddr,
    tr: Arc<dyn TunnelTransport>,
    st: Arc<Stats>,
) {
    let (host, port) = match (t.target_host.as_deref(), t.target_port) {
        (Some(h), Some(p)) => (h.to_string(), p),
        _ => return,
    };
    match tr.open_direct(&host, port, peer).await {
        Ok(remote) => pipe(sock, remote, st).await,
        Err(e) => {
            st.failed.fetch_add(1, Ordering::Relaxed);
            tracing::debug!(
                "local tunnel '{}': direct-tcpip to {host}:{port} failed: {e}",
                t.name
            );
        }
    }
}

async fn handle_socks(
    mut sock: TcpStream,
    peer: SocketAddr,
    tr: Arc<dyn TunnelTransport>,
    st: Arc<Stats>,
) {
    let target = match socks5::accept(&mut sock).await {
        Ok(t) => t,
        Err(e) => {
            st.failed.fetch_add(1, Ordering::Relaxed);
            tracing::debug!("SOCKS5 handshake failed: {e}");
            return;
        }
    };
    match tr.open_direct(&target.host, target.port, peer).await {
        Ok(remote) => {
            if socks5::send_reply(&mut sock, socks5::reply::SUCCEEDED)
                .await
                .is_ok()
            {
                pipe(sock, remote, st).await;
            }
        }
        Err(e) => {
            st.failed.fetch_add(1, Ordering::Relaxed);
            let lower = e.0.to_ascii_lowercase();
            let code = if lower.contains("connectfailed") || lower.contains("connect failed") {
                socks5::reply::CONNECTION_REFUSED
            } else if lower.contains("prohibited") {
                socks5::reply::NOT_ALLOWED
            } else {
                socks5::reply::HOST_UNREACHABLE
            };
            let _ = socks5::send_reply(&mut sock, code).await;
        }
    }
}

async fn run_remote(
    tunnel: Tunnel,
    mut rl: RemoteListener,
    transport: Arc<dyn TunnelTransport>,
    stats: Arc<Stats>,
    state: Arc<Mutex<TunnelState>>,
    mut shutdown: watch::Receiver<bool>,
    events: broadcast::Sender<TunnelEvent>,
) {
    let (host, port) = match (tunnel.target_host.clone(), tunnel.target_port) {
        (Some(h), Some(p)) => (h, p),
        _ => return,
    };
    let mut conns = JoinSet::new();
    loop {
        tokio::select! {
            _ = shutdown.changed() => break,
            _ = transport.closed() => {
                fail(&state, &events, tunnel.id, "SSH session closed");
                break;
            }
            Some(_) = conns.join_next(), if !conns.is_empty() => {}
            incoming = rl.incoming.recv() => {
                let Some(conn) = incoming else {
                    fail(&state, &events, tunnel.id, "remote forward closed");
                    break;
                };
                stats.total.fetch_add(1, Ordering::Relaxed);
                let st = stats.clone();
                let target = bind_addr(&host, port);
                conns.spawn(async move {
                    match TcpStream::connect(&target).await {
                        Ok(local) => {
                            let _ = local.set_nodelay(true);
                            // Counted side = local target (received = towards it).
                            pipe(local, conn.stream, st).await;
                        }
                        Err(e) => {
                            st.failed.fetch_add(1, Ordering::Relaxed);
                            tracing::debug!("remote tunnel: cannot reach local target {target}: {e}");
                        }
                    }
                });
            }
        }
    }
    conns.shutdown().await;
}

#[cfg(test)]
mod tests;
