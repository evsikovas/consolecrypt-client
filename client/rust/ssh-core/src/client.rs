//! Native (russh) connector: direct and multi-hop connections, auth, host
//! key verification, keepalive, exec / shell (PTY) / direct-tcpip /
//! remote forwarding / subsystems.
//!
//! Multi-hop (CLIENT_SPEC §6.3):
//!
//! ```text
//! client ─TCP (or SOCKS5/HTTP proxy)→ hop1 ─direct-tcpip→ hop2 ─direct-tcpip→ target
//! ```
//!
//! Each hop is a full SSH session running over the previous hop's
//! `direct-tcpip` channel stream, verified and authenticated with that hop's
//! own credential and host-key policy. The chain length is unlimited.

use crate::agent;
use crate::error::SshError;
use crate::keys::{self, fingerprint_sha256, key_blob, KeyError};
use crate::known_hosts::{
    check_host_key, new_known_host, normalize_key_type, HostKeyCheck, PresentedCertificate,
};
use crate::planner::{ConnectionPlan, Hop};
use crate::proxy::connect_via_proxy;
use crate::traits::{
    CredentialResolver, HostKeyDecision, HostKeyInfo, HostKeyPrompt, KnownHostsStore,
    ResolvedCredential,
};
use bytes::Bytes;
use cc_models::credential::{Credential, CredentialKind};
use cc_models::host::HostKeyPolicy;
use cc_models::known_host::{host_pattern, KnownHost, KnownHostSource};
use cc_models::ObjectId;
use russh::client::{
    self, AuthResult, ChannelOpenHandle, DisconnectReason, Handle, KeyboardInteractiveAuthResponse,
    Msg,
};
use russh::keys::agent::AgentIdentity;
use russh::keys::ssh_encoding::Decode;
use russh::keys::ssh_key::certificate::CertType;
use russh::keys::ssh_key::public::KeyData;
use russh::keys::ssh_key::{Algorithm, Certificate, HashAlg};
use russh::keys::{PrivateKeyWithHashAlg, PublicKeyOrCertificate};
use russh::{
    Channel, ChannelMsg, ChannelOpenFailure, ChannelStream, Disconnect, MethodKind, Preferred,
    SshId,
};
use secrecy::{ExposeSecret, SecretString};
use std::borrow::Cow;
use std::collections::HashMap;
use std::fmt;
use std::future::Future;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;
use tokio::net::TcpStream;
use tokio::sync::{broadcast, mpsc, watch};

/// Bidirectional byte stream over an SSH channel (`AsyncRead + AsyncWrite`).
pub type SshStream = ChannelStream<Msg>;

/// Tunables of the native connector.
#[derive(Debug, Clone)]
pub struct ConnectOptions {
    /// TCP connect + SSH handshake timeout (per hop) when no user prompt is
    /// involved.
    pub connect_timeout: Duration,
    /// Handshake timeout when the host-key policy is `Ask` (includes the
    /// time the user needs to decide).
    pub prompt_timeout: Duration,
    /// Authentication timeout per hop (includes passphrase prompts).
    pub auth_timeout: Duration,
    /// Keepalive interval when the host does not override it
    /// (`Host::keepalive_secs`; `Some(0)` disables).
    pub default_keepalive: Option<Duration>,
    /// Unanswered keepalives before the connection is declared dead.
    pub keepalive_max: usize,
    /// Close the connection after this much inactivity (`None` = never).
    pub inactivity_timeout: Option<Duration>,
}

impl Default for ConnectOptions {
    fn default() -> Self {
        Self {
            connect_timeout: Duration::from_secs(20),
            prompt_timeout: Duration::from_secs(300),
            auth_timeout: Duration::from_secs(300),
            default_keepalive: Some(Duration::from_secs(30)),
            keepalive_max: 3,
            inactivity_timeout: None,
        }
    }
}

/// Lifecycle events of a session (for status UI and reconnect UX).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SessionEvent {
    /// Transport + key exchange with hop `index` done (host key verified).
    HopConnected {
        index: usize,
        host: String,
        port: u16,
    },
    /// Hop `index` authenticated.
    Authenticated { index: usize },
    /// Pre-auth banner from hop `index`.
    Banner { index: usize, message: String },
    /// Hop `index` disconnected; the whole session is unusable afterwards.
    /// `error = true` for abnormal termination (network loss, keepalive
    /// timeout…) — the UI should offer "Reconnect".
    Disconnected {
        index: usize,
        reason: String,
        error: bool,
    },
}

/// Collected result of [`SshSession::exec`].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ExecOutput {
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
    pub exit_status: Option<u32>,
    pub exit_signal: Option<String>,
}

impl ExecOutput {
    pub fn stdout_lossy(&self) -> String {
        String::from_utf8_lossy(&self.stdout).into_owned()
    }
    pub fn stderr_lossy(&self) -> String {
        String::from_utf8_lossy(&self.stderr).into_owned()
    }
}

/// PTY parameters for an interactive shell.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PtyRequest {
    /// `TERM`, default `xterm-256color`.
    pub term: String,
    pub cols: u32,
    pub rows: u32,
    pub pix_width: u32,
    pub pix_height: u32,
    /// Environment variables to request (servers may refuse; ignored then).
    pub env: Vec<(String, String)>,
}

impl Default for PtyRequest {
    fn default() -> Self {
        Self {
            term: "xterm-256color".into(),
            cols: 80,
            rows: 24,
            pix_width: 0,
            pix_height: 0,
            env: Vec::new(),
        }
    }
}

/// Input to an interactive shell channel.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ShellInput {
    Data(Bytes),
    Resize { cols: u32, rows: u32 },
    Eof,
    Close,
}

/// Output of an interactive shell channel.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ShellEvent {
    Data(Bytes),
    /// Extended data (stderr); PTY sessions normally merge it into `Data`.
    Stderr(Bytes),
    ExitStatus(u32),
    ExitSignal {
        signal: String,
        message: String,
    },
    Eof,
    /// Channel closed; no more events follow.
    Closed,
}

/// Write side of a shell channel (cheap to clone).
#[derive(Debug, Clone)]
pub struct ShellWriter {
    tx: mpsc::Sender<ShellInput>,
}

impl ShellWriter {
    pub async fn send(&self, input: ShellInput) -> Result<(), SshError> {
        self.tx
            .send(input)
            .await
            .map_err(|_| SshError::Disconnected)
    }
    pub async fn write(&self, data: impl Into<Bytes>) -> Result<(), SshError> {
        self.send(ShellInput::Data(data.into())).await
    }
    pub async fn resize(&self, cols: u32, rows: u32) -> Result<(), SshError> {
        self.send(ShellInput::Resize { cols, rows }).await
    }
    pub async fn eof(&self) -> Result<(), SshError> {
        self.send(ShellInput::Eof).await
    }
    pub async fn close(&self) -> Result<(), SshError> {
        self.send(ShellInput::Close).await
    }
}

/// Read side of a shell channel.
#[derive(Debug)]
pub struct ShellReader {
    rx: mpsc::Receiver<ShellEvent>,
}

impl ShellReader {
    /// Next event; `None` after the channel is gone.
    pub async fn recv(&mut self) -> Option<ShellEvent> {
        self.rx.recv().await
    }
}

/// An interactive shell: writer + reader halves.
#[derive(Debug)]
pub struct ShellChannel {
    pub writer: ShellWriter,
    pub reader: ShellReader,
}

/// The "remote" end of a [`ShellChannel::pair`] (tests and adapters).
#[derive(Debug)]
pub struct ShellRemote {
    pub inputs: mpsc::Receiver<ShellInput>,
    pub events: mpsc::Sender<ShellEvent>,
}

impl ShellChannel {
    pub fn split(self) -> (ShellWriter, ShellReader) {
        (self.writer, self.reader)
    }

    /// In-memory channel pair: whatever is written to the returned channel
    /// arrives at `ShellRemote::inputs`; events sent to
    /// `ShellRemote::events` are read from the channel.
    pub fn pair(buffer: usize) -> (ShellChannel, ShellRemote) {
        let (in_tx, in_rx) = mpsc::channel(buffer);
        let (ev_tx, ev_rx) = mpsc::channel(buffer);
        (
            ShellChannel {
                writer: ShellWriter { tx: in_tx },
                reader: ShellReader { rx: ev_rx },
            },
            ShellRemote {
                inputs: in_rx,
                events: ev_tx,
            },
        )
    }
}

/// A connection opened by the server for one of our remote forwards.
pub struct ForwardedTcpip {
    pub stream: SshStream,
    /// Address/port the remote side accepted the connection on.
    pub connected: (String, u16),
    /// Remote peer that connected.
    pub originator: (String, u16),
}

impl fmt::Debug for ForwardedTcpip {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ForwardedTcpip")
            .field("connected", &self.connected)
            .field("originator", &self.originator)
            .finish_non_exhaustive()
    }
}

/// An active `tcpip-forward` request.
#[derive(Debug)]
pub struct RemoteForward {
    pub bind_host: String,
    /// Port bound on the server (differs from the request when it was 0).
    pub bound_port: u16,
    pub incoming: mpsc::Receiver<ForwardedTcpip>,
}

#[derive(Default)]
struct ForwardRegistry {
    by_port: Mutex<HashMap<u32, mpsc::Sender<ForwardedTcpip>>>,
}

impl ForwardRegistry {
    fn insert(&self, port: u32, tx: mpsc::Sender<ForwardedTcpip>) {
        self.by_port
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(port, tx);
    }
    fn remove(&self, port: u32) {
        self.by_port
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .remove(&port);
    }
    fn get(&self, port: u32) -> Option<mpsc::Sender<ForwardedTcpip>> {
        self.by_port
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get(&port)
            .cloned()
    }
}

#[derive(Debug, Clone)]
struct HopContext {
    index: usize,
    count: usize,
    host_id: ObjectId,
    name: String,
    host: String,
    port: u16,
    policy: HostKeyPolicy,
}

/// russh client handler: host-key verification, events, forwarded channels.
struct ClientHandler {
    ctx: HopContext,
    known_hosts: Arc<dyn KnownHostsStore>,
    prompt: Arc<dyn HostKeyPrompt>,
    events: broadcast::Sender<SessionEvent>,
    forwards: Arc<ForwardRegistry>,
    closed: Arc<watch::Sender<bool>>,
    /// Blob of the key accepted during the first key exchange (re-keys
    /// present the same key and must not prompt again).
    accepted_key: Option<Vec<u8>>,
}

impl ClientHandler {
    fn host_cert_valid(&self, cert: &Certificate, entries: &[KnownHost]) -> bool {
        use base64::Engine as _;
        let b64 = base64::engine::general_purpose::STANDARD;
        let ca_fps: Vec<_> = entries
            .iter()
            .filter(|e| e.is_cert_authority() && !e.revoked)
            .filter_map(|e| b64.decode(e.public_key.as_bytes()).ok())
            .filter_map(|blob| KeyData::decode(&mut blob.as_slice()).ok())
            .map(|k| k.fingerprint(HashAlg::Sha256))
            .collect();
        if ca_fps.is_empty() {
            return false;
        }
        let now = chrono::Utc::now().timestamp().max(0) as u64;
        if cert.validate_at(now, ca_fps.iter()).is_err() {
            return false;
        }
        if cert.cert_type() != CertType::Host || !cert.critical_options().is_empty() {
            return false;
        }
        // Empty principals would be a "golden ticket": refuse it.
        cert.valid_principals()
            .iter()
            .any(|p| crate::known_hosts::glob_match(p, &self.ctx.host))
    }

    async fn verify(&mut self, presented: &PublicKeyOrCertificate) -> Result<bool, SshError> {
        let (key_data, cert) = match presented {
            PublicKeyOrCertificate::PublicKey { key, .. } => (key.key_data().clone(), None),
            PublicKeyOrCertificate::Certificate(c) => (c.public_key().clone(), Some(c)),
        };
        let blob = key_blob(&key_data);
        if self.accepted_key.as_deref() == Some(blob.as_slice()) {
            return Ok(true);
        }
        let key_type = key_data.algorithm().as_str().to_string();
        let fingerprint = fingerprint_sha256(&blob);
        let (host, port) = (self.ctx.host.clone(), self.ctx.port);
        let entries = self.known_hosts.lookup(&host, port).await?;
        let presented_cert = cert.map(|c| PresentedCertificate {
            ca_key_blob: key_blob(c.signature_key()),
            valid_for_host: self.host_cert_valid(c, &entries),
        });

        match check_host_key(&entries, &key_type, &blob, presented_cert.as_ref()) {
            HostKeyCheck::Trusted { via_cert_authority } => {
                tracing::debug!(host = %host, port, via_cert_authority, "host key trusted");
            }
            HostKeyCheck::Revoked => {
                return Err(SshError::HostKeyRevoked {
                    host,
                    port,
                    fingerprint,
                })
            }
            HostKeyCheck::Changed {
                expected_fingerprints,
            } => {
                tracing::warn!(host = %host, port, "host key CHANGED; refusing connection");
                return Err(SshError::HostKeyChanged {
                    host,
                    port,
                    key_type,
                    actual_fingerprint: fingerprint,
                    expected_fingerprints,
                });
            }
            HostKeyCheck::Unknown { other_key_types } => match self.ctx.policy {
                HostKeyPolicy::Strict => {
                    return Err(SshError::HostKeyUnknown {
                        host,
                        port,
                        key_type,
                        fingerprint,
                    })
                }
                HostKeyPolicy::AcceptNew => {
                    self.known_hosts
                        .add(new_known_host(
                            &host,
                            port,
                            &key_type,
                            &blob,
                            KnownHostSource::Tofu,
                        ))
                        .await?;
                }
                HostKeyPolicy::Ask => {
                    use base64::Engine as _;
                    let info = HostKeyInfo {
                        host: host.clone(),
                        port,
                        host_pattern: host_pattern(&host, port),
                        host_id: Some(self.ctx.host_id),
                        host_name: Some(self.ctx.name.clone()),
                        hop_index: self.ctx.index,
                        hop_count: self.ctx.count,
                        key_type: key_type.clone(),
                        fingerprint_sha256: fingerprint.clone(),
                        public_key: base64::engine::general_purpose::STANDARD.encode(&blob),
                        other_known_key_types: other_key_types,
                    };
                    match self.prompt.confirm_unknown(info).await {
                        HostKeyDecision::AcceptAndSave => {
                            self.known_hosts
                                .add(new_known_host(
                                    &host,
                                    port,
                                    &key_type,
                                    &blob,
                                    KnownHostSource::Tofu,
                                ))
                                .await?;
                        }
                        HostKeyDecision::AcceptOnce => {}
                        HostKeyDecision::Reject => {
                            return Err(SshError::HostKeyRejected {
                                host,
                                port,
                                fingerprint,
                            })
                        }
                    }
                }
            },
        }
        self.accepted_key = Some(blob);
        let _ = self.events.send(SessionEvent::HopConnected {
            index: self.ctx.index,
            host,
            port,
        });
        Ok(true)
    }
}

impl client::Handler for ClientHandler {
    type Error = SshError;

    async fn check_server_key(
        &mut self,
        server_public_key: &PublicKeyOrCertificate,
    ) -> Result<bool, Self::Error> {
        self.verify(server_public_key).await
    }

    async fn auth_banner(
        &mut self,
        banner: &str,
        _session: &mut client::Session,
    ) -> Result<(), Self::Error> {
        let _ = self.events.send(SessionEvent::Banner {
            index: self.ctx.index,
            message: banner.to_string(),
        });
        Ok(())
    }

    async fn server_channel_open_forwarded_tcpip(
        &mut self,
        channel: Channel<Msg>,
        connected_address: &str,
        connected_port: u32,
        originator_address: &str,
        originator_port: u32,
        reply: ChannelOpenHandle,
        _session: &mut client::Session,
    ) -> Result<(), Self::Error> {
        match self.forwards.get(connected_port) {
            Some(tx) => {
                reply.accept().await;
                let fwd = ForwardedTcpip {
                    stream: channel.into_stream(),
                    connected: (connected_address.to_string(), connected_port as u16),
                    originator: (originator_address.to_string(), originator_port as u16),
                };
                // Never block the session loop: drop the connection if the
                // consumer is saturated.
                if tx.try_send(fwd).is_err() {
                    tracing::warn!(
                        port = connected_port,
                        "remote forward backlog full; dropping connection"
                    );
                }
            }
            None => {
                reply
                    .reject(ChannelOpenFailure::AdministrativelyProhibited)
                    .await
            }
        }
        Ok(())
    }

    // Channels we never asked for are refused (russh accepts by default).
    async fn server_channel_open_agent_forward(
        &mut self,
        _channel: Channel<Msg>,
        reply: ChannelOpenHandle,
        _session: &mut client::Session,
    ) -> Result<(), Self::Error> {
        reply
            .reject(ChannelOpenFailure::AdministrativelyProhibited)
            .await;
        Ok(())
    }

    async fn server_channel_open_session(
        &mut self,
        _channel: Channel<Msg>,
        reply: ChannelOpenHandle,
        _session: &mut client::Session,
    ) -> Result<(), Self::Error> {
        reply
            .reject(ChannelOpenFailure::AdministrativelyProhibited)
            .await;
        Ok(())
    }

    async fn server_channel_open_direct_tcpip(
        &mut self,
        _channel: Channel<Msg>,
        _host_to_connect: &str,
        _port_to_connect: u32,
        _originator_address: &str,
        _originator_port: u32,
        reply: ChannelOpenHandle,
        _session: &mut client::Session,
    ) -> Result<(), Self::Error> {
        reply
            .reject(ChannelOpenFailure::AdministrativelyProhibited)
            .await;
        Ok(())
    }

    async fn server_channel_open_x11(
        &mut self,
        _channel: Channel<Msg>,
        _originator_address: &str,
        _originator_port: u32,
        reply: ChannelOpenHandle,
        _session: &mut client::Session,
    ) -> Result<(), Self::Error> {
        reply
            .reject(ChannelOpenFailure::AdministrativelyProhibited)
            .await;
        Ok(())
    }

    async fn server_channel_open_forwarded_streamlocal(
        &mut self,
        _channel: Channel<Msg>,
        _socket_path: &str,
        reply: ChannelOpenHandle,
        _session: &mut client::Session,
    ) -> Result<(), Self::Error> {
        reply
            .reject(ChannelOpenFailure::AdministrativelyProhibited)
            .await;
        Ok(())
    }

    async fn server_channel_open_direct_streamlocal(
        &mut self,
        _channel: Channel<Msg>,
        _socket_path: &str,
        reply: ChannelOpenHandle,
        _session: &mut client::Session,
    ) -> Result<(), Self::Error> {
        reply
            .reject(ChannelOpenFailure::AdministrativelyProhibited)
            .await;
        Ok(())
    }

    async fn disconnected(
        &mut self,
        reason: DisconnectReason<Self::Error>,
    ) -> Result<(), Self::Error> {
        let (text, error) = match &reason {
            DisconnectReason::ReceivedDisconnect(info) => (
                format!(
                    "server disconnected ({:?}) {}",
                    info.reason_code, info.message
                )
                .trim()
                .to_string(),
                false,
            ),
            DisconnectReason::Error(e) => (e.to_string(), true),
        };
        tracing::debug!(hop = self.ctx.index, error, "ssh session ended: {text}");
        let _ = self.events.send(SessionEvent::Disconnected {
            index: self.ctx.index,
            reason: text,
            error,
        });
        self.closed.send_replace(true);
        match reason {
            DisconnectReason::ReceivedDisconnect(_) => Ok(()),
            DisconnectReason::Error(e) => Err(e),
        }
    }
}

/// Native SSH connector. Cheap to clone.
#[derive(Clone)]
pub struct SshConnector {
    resolver: Arc<dyn CredentialResolver>,
    known_hosts: Arc<dyn KnownHostsStore>,
    prompt: Arc<dyn HostKeyPrompt>,
    options: ConnectOptions,
}

impl fmt::Debug for SshConnector {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SshConnector")
            .field("options", &self.options)
            .finish_non_exhaustive()
    }
}

async fn with_timeout<T>(
    dur: Duration,
    stage: &'static str,
    hop: &Hop,
    fut: impl Future<Output = Result<T, SshError>>,
) -> Result<T, SshError> {
    tokio::time::timeout(dur, fut)
        .await
        .map_err(|_| SshError::Timeout {
            stage,
            host: hop.endpoint.host.clone(),
            port: hop.endpoint.port,
        })?
}

fn methods_to_strings(m: &russh::MethodSet) -> Vec<String> {
    m.iter().map(|k| <&str>::from(k).to_string()).collect()
}

fn algorithm_matches(alg: &Algorithm, key_type: &str) -> bool {
    normalize_key_type(alg.as_str()) == normalize_key_type(key_type)
}

/// Preferred host-key algorithms: key types already known for the host
/// first (avoids spurious "unknown key" prompts when e.g. only the RSA key
/// was imported), certificate variants when a CA is trusted.
fn preferred_for(entries: &[KnownHost]) -> Preferred {
    let defaults: Vec<Algorithm> = Preferred::DEFAULT.key.to_vec();
    let mut ordered: Vec<Algorithm> = Vec::with_capacity(defaults.len());
    for e in entries
        .iter()
        .filter(|e| !e.revoked && !e.is_cert_authority())
    {
        for a in &defaults {
            if algorithm_matches(a, &e.key_type) && !ordered.contains(a) {
                ordered.push(a.clone());
            }
        }
    }
    for a in defaults {
        if !ordered.contains(&a) {
            ordered.push(a);
        }
    }
    let mut p = Preferred::default();
    if entries.iter().any(|e| e.is_cert_authority() && !e.revoked) {
        p.host_key_certificates = Cow::Owned(ordered.clone());
    }
    p.key = Cow::Owned(ordered);
    p
}

impl SshConnector {
    pub fn new(
        resolver: Arc<dyn CredentialResolver>,
        known_hosts: Arc<dyn KnownHostsStore>,
        prompt: Arc<dyn HostKeyPrompt>,
    ) -> Self {
        Self {
            resolver,
            known_hosts,
            prompt,
            options: ConnectOptions::default(),
        }
    }

    pub fn with_options(mut self, options: ConnectOptions) -> Self {
        self.options = options;
        self
    }

    pub fn options(&self) -> &ConnectOptions {
        &self.options
    }

    pub fn resolver(&self) -> &Arc<dyn CredentialResolver> {
        &self.resolver
    }

    async fn config_for(&self, hop: &Hop) -> client::Config {
        let entries = self
            .known_hosts
            .lookup(&hop.endpoint.host, hop.endpoint.port)
            .await
            .unwrap_or_default();
        let keepalive = match hop.keepalive_secs {
            Some(0) => None,
            Some(s) => Some(Duration::from_secs(u64::from(s))),
            None => self.options.default_keepalive,
        };
        client::Config {
            client_id: SshId::Standard(Cow::Owned(format!(
                "SSH-2.0-ConsoleCrypt_{}",
                env!("CARGO_PKG_VERSION")
            ))),
            keepalive_interval: keepalive,
            keepalive_max: self.options.keepalive_max,
            inactivity_timeout: self.options.inactivity_timeout,
            nodelay: true,
            preferred: preferred_for(&entries),
            ..Default::default()
        }
    }

    fn handshake_timeout(&self, hop: &Hop) -> Duration {
        if hop.host_key_policy == HostKeyPolicy::Ask {
            self.options.prompt_timeout
        } else {
            self.options.connect_timeout
        }
    }

    /// Establish every hop of `plan` and return the session to the target.
    pub async fn connect(&self, plan: &ConnectionPlan) -> Result<SshSession, SshError> {
        let hops = plan.all_hops();
        let (events, _) = broadcast::channel(256);
        let (closed_tx, closed_rx) = watch::channel(false);
        let closed_tx = Arc::new(closed_tx);
        let forwards = Arc::new(ForwardRegistry::default());
        let mut handles: Vec<Arc<Handle<ClientHandler>>> = Vec::with_capacity(hops.len());

        for (index, hop) in hops.iter().enumerate() {
            let handler = ClientHandler {
                ctx: HopContext {
                    index,
                    count: hops.len(),
                    host_id: hop.host_id,
                    name: hop.name.clone(),
                    host: hop.endpoint.host.clone(),
                    port: hop.endpoint.port,
                    policy: hop.host_key_policy,
                },
                known_hosts: self.known_hosts.clone(),
                prompt: self.prompt.clone(),
                events: events.clone(),
                forwards: forwards.clone(),
                closed: closed_tx.clone(),
                accepted_key: None,
            };
            let config = Arc::new(self.config_for(hop).await);
            let handshake = self.handshake_timeout(hop);
            let mut handle = if index == 0 {
                let stream = self.open_first_stream(plan, hop).await?;
                with_timeout(
                    handshake,
                    "SSH handshake",
                    hop,
                    client::connect_stream(config, stream, handler),
                )
                .await?
            } else {
                let prev = &handles[index - 1];
                let prev_hop = &hops[index - 1];
                let channel = with_timeout(
                    self.options.connect_timeout,
                    "opening jump channel",
                    hop,
                    async {
                        prev.channel_open_direct_tcpip(
                            hop.endpoint.host.clone(),
                            u32::from(hop.endpoint.port),
                            "127.0.0.1",
                            0,
                        )
                        .await
                        .map_err(|e| SshError::JumpChannel {
                            via: format!("{} ({})", prev_hop.name, prev_hop.endpoint),
                            host: hop.endpoint.host.clone(),
                            port: hop.endpoint.port,
                            reason: e.to_string(),
                        })
                    },
                )
                .await?;
                with_timeout(
                    handshake,
                    "SSH handshake",
                    hop,
                    client::connect_stream(config, channel.into_stream(), handler),
                )
                .await?
            };
            with_timeout(
                self.options.auth_timeout,
                "authenticating",
                hop,
                self.authenticate(&mut handle, hop),
            )
            .await?;
            let _ = events.send(SessionEvent::Authenticated { index });
            tracing::debug!(hop = index, host = %hop.endpoint, "hop authenticated");
            handles.push(Arc::new(handle));
        }

        let target = handles
            .pop()
            .ok_or_else(|| SshError::Channel("empty route".into()))?;
        Ok(SshSession {
            host_id: plan.host_id,
            description: plan.describe(),
            target,
            hops: handles,
            events,
            closed: closed_rx,
            forwards,
        })
    }

    async fn open_first_stream(
        &self,
        plan: &ConnectionPlan,
        hop: &Hop,
    ) -> Result<TcpStream, SshError> {
        let (host, port) = (hop.endpoint.host.as_str(), hop.endpoint.port);
        let fut = async {
            match &plan.proxy {
                Some(proxy) => {
                    let password = if proxy.username.is_some() || proxy.password_secret_id.is_some()
                    {
                        self.resolver.resolve_proxy_password(proxy).await?
                    } else {
                        None
                    };
                    connect_via_proxy(proxy, password.as_ref(), host, port).await
                }
                None => {
                    let s = TcpStream::connect((host, port)).await.map_err(|source| {
                        SshError::Connect {
                            host: host.to_string(),
                            port,
                            source,
                        }
                    })?;
                    let _ = s.set_nodelay(true);
                    Ok(s)
                }
            }
        };
        with_timeout(self.options.connect_timeout, "connecting", hop, fut).await
    }

    async fn authenticate(
        &self,
        handle: &mut Handle<ClientHandler>,
        hop: &Hop,
    ) -> Result<(), SshError> {
        let user = hop.username.as_str();
        let result = match &hop.credential {
            None => self.auth_default(handle, user).await?,
            Some(cred) => {
                if cred.kind == CredentialKind::Fido2 {
                    return Err(SshError::Unsupported(
                        "FIDO2 security keys are not supported by the native backend yet".into(),
                    ));
                }
                let resolved = self.resolver.resolve(cred).await?;
                self.auth_with(handle, user, cred, resolved).await?
            }
        };
        match result {
            AuthResult::Success => Ok(()),
            AuthResult::Failure {
                remaining_methods,
                partial_success,
            } => Err(SshError::AuthFailed {
                user: user.to_string(),
                host: hop.endpoint.to_string(),
                detail: if partial_success {
                    format!(
                        "partial success: the server requires an additional method ({})",
                        methods_to_strings(&remaining_methods).join(", ")
                    )
                } else {
                    format!(
                        "credential rejected; server accepts: {}",
                        methods_to_strings(&remaining_methods).join(", ")
                    )
                },
                remaining_methods: methods_to_strings(&remaining_methods),
            }),
        }
    }

    /// No credential: OS agent if configured, else `none`.
    async fn auth_default(
        &self,
        handle: &mut Handle<ClientHandler>,
        user: &str,
    ) -> Result<AuthResult, SshError> {
        if agent::default_agent_path().is_some() {
            match self.auth_agent(handle, user, None).await {
                Ok(r) if r.success() => return Ok(r),
                Ok(_) => {}
                Err(e) => tracing::debug!("default agent unavailable: {e}"),
            }
        }
        Ok(handle.authenticate_none(user).await?)
    }

    async fn auth_with(
        &self,
        handle: &mut Handle<ClientHandler>,
        user: &str,
        cred: &Credential,
        resolved: ResolvedCredential,
    ) -> Result<AuthResult, SshError> {
        match resolved {
            ResolvedCredential::Password(pw) => auth_password(handle, user, &pw).await,
            ResolvedCredential::Agent { socket } => {
                self.auth_agent(handle, user, socket.as_deref()).await
            }
            ResolvedCredential::PrivateKey {
                openssh,
                passphrase,
                certificate,
            } => {
                let key = self.decrypt_key(cred, &openssh, passphrase).await?;
                let key = Arc::new(key);
                let cert_line = certificate.or_else(|| cred.certificate.clone());
                match cert_line {
                    Some(line) => {
                        let cert = Certificate::from_openssh(line.trim())
                            .map_err(|e| KeyError::InvalidFormat(e.to_string()))?;
                        if cert.public_key() != key.public_key().key_data() {
                            return Err(KeyError::CertificateMismatch.into());
                        }
                        Ok(handle.authenticate_openssh_cert(user, key, cert).await?)
                    }
                    None => {
                        let hash = if key.algorithm().is_rsa() {
                            handle.best_supported_rsa_hash().await?.flatten()
                        } else {
                            None
                        };
                        Ok(handle
                            .authenticate_publickey(user, PrivateKeyWithHashAlg::new(key, hash))
                            .await?)
                    }
                }
            }
        }
    }

    /// Decrypt a private key in memory, prompting for the passphrase via the
    /// resolver when needed (up to 3 attempts).
    async fn decrypt_key(
        &self,
        cred: &Credential,
        openssh: &SecretString,
        remembered: Option<SecretString>,
    ) -> Result<russh::keys::PrivateKey, SshError> {
        match keys::load_private_key(openssh, remembered.as_ref()) {
            Ok(k) => return Ok(k),
            Err(KeyError::PassphraseRequired | KeyError::WrongPassphrase) => {}
            Err(e) => return Err(e.into()),
        }
        let mut attempt = 0u32;
        loop {
            let Some(p) = self.resolver.prompt_passphrase(cred, attempt).await? else {
                return Err(if attempt == 0 && remembered.is_none() {
                    SshError::PassphraseRequired {
                        credential_id: Some(cred.id),
                    }
                } else {
                    SshError::WrongPassphrase {
                        credential_id: Some(cred.id),
                    }
                });
            };
            match keys::load_private_key(openssh, Some(&p)) {
                Ok(k) => return Ok(k),
                Err(KeyError::WrongPassphrase) if attempt < 2 => attempt += 1,
                Err(KeyError::WrongPassphrase) => {
                    return Err(SshError::WrongPassphrase {
                        credential_id: Some(cred.id),
                    })
                }
                Err(e) => return Err(e.into()),
            }
        }
    }

    async fn auth_agent(
        &self,
        handle: &mut Handle<ClientHandler>,
        user: &str,
        socket: Option<&std::path::Path>,
    ) -> Result<AuthResult, SshError> {
        let mut client = agent::connect_agent(socket).await?;
        let identities = client
            .request_identities()
            .await
            .map_err(|e| SshError::Agent(e.to_string()))?;
        if identities.is_empty() {
            return Err(SshError::Agent("the SSH agent holds no identities".into()));
        }
        let mut rsa_hash: Option<Option<HashAlg>> = None;
        let mut last = AuthResult::Failure {
            remaining_methods: russh::MethodSet::empty(),
            partial_success: false,
        };
        for id in identities {
            let is_rsa = id.public_key().algorithm().is_rsa();
            let hash = if is_rsa {
                if rsa_hash.is_none() {
                    rsa_hash = Some(handle.best_supported_rsa_hash().await?.flatten());
                }
                rsa_hash.flatten()
            } else {
                None
            };
            let r = match id {
                AgentIdentity::PublicKey { key, .. } => handle
                    .authenticate_publickey_with(user, key, hash, &mut client)
                    .await
                    .map_err(|e| SshError::Agent(e.to_string()))?,
                AgentIdentity::Certificate { certificate, .. } => handle
                    .authenticate_certificate_with(user, certificate, hash, &mut client)
                    .await
                    .map_err(|e| SshError::Agent(e.to_string()))?,
            };
            if r.success() {
                return Ok(r);
            }
            last = r;
        }
        Ok(last)
    }
}

async fn auth_password(
    handle: &mut Handle<ClientHandler>,
    user: &str,
    password: &SecretString,
) -> Result<AuthResult, SshError> {
    // NOTE: russh copies the password into an internal `String` for the
    // wire message; that copy is outside our zeroization control.
    let first = handle
        .authenticate_password(user, password.expose_secret())
        .await?;
    let AuthResult::Failure {
        remaining_methods, ..
    } = &first
    else {
        return Ok(first);
    };
    if !remaining_methods.contains(&MethodKind::KeyboardInteractive) {
        return Ok(first);
    }
    // Servers with PAM often only offer keyboard-interactive: answer every
    // hidden prompt with the password.
    let mut resp = handle
        .authenticate_keyboard_interactive_start(user, None)
        .await?;
    for _ in 0..5 {
        match resp {
            KeyboardInteractiveAuthResponse::Success => return Ok(AuthResult::Success),
            KeyboardInteractiveAuthResponse::Failure {
                remaining_methods,
                partial_success,
            } => {
                return Ok(AuthResult::Failure {
                    remaining_methods,
                    partial_success,
                })
            }
            KeyboardInteractiveAuthResponse::InfoRequest { prompts, .. } => {
                let answers = prompts
                    .iter()
                    .map(|p| {
                        if p.echo {
                            String::new()
                        } else {
                            password.expose_secret().to_string()
                        }
                    })
                    .collect();
                resp = handle
                    .authenticate_keyboard_interactive_respond(answers)
                    .await?;
            }
        }
    }
    Ok(first)
}

/// An authenticated SSH session to the target (holding every jump hop
/// alive). Share it with `Arc`.
pub struct SshSession {
    host_id: ObjectId,
    description: String,
    target: Arc<Handle<ClientHandler>>,
    /// Jump hops, first = closest to the client (kept alive).
    hops: Vec<Arc<Handle<ClientHandler>>>,
    events: broadcast::Sender<SessionEvent>,
    closed: watch::Receiver<bool>,
    forwards: Arc<ForwardRegistry>,
}

impl fmt::Debug for SshSession {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SshSession")
            .field("host_id", &self.host_id)
            .field("route", &self.description)
            .field("closed", &self.is_closed())
            .finish()
    }
}

fn map_channel_msg(msg: ChannelMsg) -> Option<ShellEvent> {
    match msg {
        ChannelMsg::Data { data } => Some(ShellEvent::Data(data)),
        ChannelMsg::ExtendedData { data, .. } => Some(ShellEvent::Stderr(data)),
        ChannelMsg::ExitStatus { exit_status } => Some(ShellEvent::ExitStatus(exit_status)),
        ChannelMsg::ExitSignal {
            signal_name,
            error_message,
            ..
        } => Some(ShellEvent::ExitSignal {
            signal: format!("{signal_name:?}"),
            message: error_message,
        }),
        ChannelMsg::Eof => Some(ShellEvent::Eof),
        ChannelMsg::Close => Some(ShellEvent::Closed),
        _ => None,
    }
}

impl SshSession {
    /// Inventory host this session belongs to.
    pub fn host_id(&self) -> ObjectId {
        self.host_id
    }

    /// Route description (`user@hop1:22 -> user@target:22`).
    pub fn description(&self) -> &str {
        &self.description
    }

    /// Number of SSH sessions in the chain (jump hops + target).
    pub fn hop_count(&self) -> usize {
        self.hops.len() + 1
    }

    /// Subscribe to lifecycle events (disconnects for reconnect UX…).
    pub fn events(&self) -> broadcast::Receiver<SessionEvent> {
        self.events.subscribe()
    }

    /// `true` once any hop of the chain is gone.
    pub fn is_closed(&self) -> bool {
        *self.closed.borrow() || self.target.is_closed() || self.hops.iter().any(|h| h.is_closed())
    }

    /// Resolves when the session (any hop) disconnects.
    pub async fn closed(&self) {
        let mut rx = self.closed.clone();
        let _ = rx.wait_for(|c| *c).await;
    }

    fn ensure_open(&self) -> Result<(), SshError> {
        if self.is_closed() {
            Err(SshError::Disconnected)
        } else {
            Ok(())
        }
    }

    /// Send a keepalive (`keepalive@openssh.com`) to the target.
    pub async fn keepalive(&self) -> Result<(), SshError> {
        self.ensure_open()?;
        Ok(self.target.send_keepalive(true).await?)
    }

    /// Run a command and collect its output.
    pub async fn exec(&self, command: &str) -> Result<ExecOutput, SshError> {
        self.ensure_open()?;
        let mut ch = self.target.channel_open_session().await?;
        ch.exec(true, command).await?;
        let mut out = ExecOutput::default();
        while let Some(msg) = ch.wait().await {
            match msg {
                ChannelMsg::Data { data } => out.stdout.extend_from_slice(&data),
                ChannelMsg::ExtendedData { data, .. } => out.stderr.extend_from_slice(&data),
                ChannelMsg::ExitStatus { exit_status } => out.exit_status = Some(exit_status),
                ChannelMsg::ExitSignal { signal_name, .. } => {
                    out.exit_signal = Some(format!("{signal_name:?}"))
                }
                ChannelMsg::Failure => {
                    return Err(SshError::Channel(
                        "the server refused to execute the command".into(),
                    ))
                }
                ChannelMsg::Close => break,
                _ => {}
            }
        }
        Ok(out)
    }

    /// Open a raw session channel running `command` (streaming use).
    pub async fn open_exec(&self, command: &str) -> Result<Channel<Msg>, SshError> {
        self.ensure_open()?;
        let ch = self.target.channel_open_session().await?;
        ch.exec(true, command).await?;
        Ok(ch)
    }

    /// Open an interactive shell with a PTY.
    pub async fn open_shell(&self, pty: PtyRequest) -> Result<ShellChannel, SshError> {
        self.ensure_open()?;
        let mut ch = self.target.channel_open_session().await?;
        for (k, v) in &pty.env {
            let _ = ch.set_env(false, k.clone(), v.clone()).await;
        }
        ch.request_pty(
            false,
            &pty.term,
            pty.cols,
            pty.rows,
            pty.pix_width,
            pty.pix_height,
            &[],
        )
        .await?;
        ch.request_shell(true).await?;
        // Wait for the shell reply; keep anything else that arrives first.
        let mut early = Vec::new();
        loop {
            match ch.wait().await {
                Some(ChannelMsg::Success) => break,
                Some(ChannelMsg::Failure) => {
                    return Err(SshError::Channel(
                        "the server refused the shell/PTY request".into(),
                    ))
                }
                Some(ChannelMsg::WindowAdjusted { .. }) => {}
                Some(other) => early.push(other),
                None => {
                    return Err(SshError::Channel(
                        "channel closed before the shell started".into(),
                    ))
                }
            }
        }
        let (in_tx, mut in_rx) = mpsc::channel::<ShellInput>(256);
        let (ev_tx, ev_rx) = mpsc::channel::<ShellEvent>(256);
        let (mut read, write) = ch.split();

        tokio::spawn(async move {
            for m in early {
                if let Some(ev) = map_channel_msg(m) {
                    if ev_tx.send(ev).await.is_err() {
                        return;
                    }
                }
            }
            loop {
                match read.wait().await {
                    Some(m) => {
                        let closed = matches!(m, ChannelMsg::Close);
                        if let Some(ev) = map_channel_msg(m) {
                            if ev_tx.send(ev).await.is_err() {
                                return;
                            }
                        }
                        if closed {
                            return;
                        }
                    }
                    None => {
                        let _ = ev_tx.send(ShellEvent::Closed).await;
                        return;
                    }
                }
            }
        });
        tokio::spawn(async move {
            while let Some(input) = in_rx.recv().await {
                let r = match input {
                    ShellInput::Data(d) => write.data_bytes(d).await,
                    ShellInput::Resize { cols, rows } => {
                        write.window_change(cols, rows, 0, 0).await
                    }
                    ShellInput::Eof => write.eof().await,
                    ShellInput::Close => {
                        let _ = write.close().await;
                        return;
                    }
                };
                if r.is_err() {
                    return;
                }
            }
            // All writers dropped: close the channel.
            let _ = write.close().await;
        });
        Ok(ShellChannel {
            writer: ShellWriter { tx: in_tx },
            reader: ShellReader { rx: ev_rx },
        })
    }

    /// Open `direct-tcpip` to `host:port` as seen from the target.
    pub async fn open_direct_tcpip(
        &self,
        host: &str,
        port: u16,
        originator: Option<SocketAddr>,
    ) -> Result<SshStream, SshError> {
        self.ensure_open()?;
        let (oa, op) = match originator {
            Some(a) => (a.ip().to_string(), u32::from(a.port())),
            None => ("127.0.0.1".to_string(), 0),
        };
        let ch = self
            .target
            .channel_open_direct_tcpip(host.to_string(), u32::from(port), oa, op)
            .await
            .map_err(|e| match e {
                russh::Error::ChannelOpenFailure(f) => {
                    SshError::Channel(format!("direct-tcpip to {host}:{port} refused: {f:?}"))
                }
                other => SshError::Protocol(other),
            })?;
        Ok(ch.into_stream())
    }

    /// Start a subsystem (e.g. `sftp`) and return its byte stream.
    pub async fn open_subsystem(&self, name: &str) -> Result<SshStream, SshError> {
        self.ensure_open()?;
        let mut ch = self.target.channel_open_session().await?;
        ch.request_subsystem(true, name).await?;
        loop {
            match ch.wait().await {
                Some(ChannelMsg::Success) => break,
                Some(ChannelMsg::Failure) => {
                    return Err(SshError::Channel(format!(
                        "subsystem '{name}' refused by the server"
                    )))
                }
                Some(ChannelMsg::WindowAdjusted { .. }) => {}
                Some(ChannelMsg::Close | ChannelMsg::Eof) | None => {
                    return Err(SshError::Channel(format!(
                        "channel closed while starting '{name}'"
                    )))
                }
                Some(_) => {}
            }
        }
        Ok(ch.into_stream())
    }

    /// Request `tcpip-forward` on the target (`bind_port = 0` lets the server
    /// choose). Incoming connections arrive on [`RemoteForward::incoming`].
    pub async fn request_remote_forward(
        &self,
        bind_host: &str,
        bind_port: u16,
    ) -> Result<RemoteForward, SshError> {
        self.ensure_open()?;
        let (tx, rx) = mpsc::channel(64);
        if bind_port != 0 {
            self.forwards.insert(u32::from(bind_port), tx.clone());
        }
        let bound = match self
            .target
            .tcpip_forward(bind_host.to_string(), u32::from(bind_port))
            .await
        {
            Ok(p) => p,
            Err(e) => {
                if bind_port != 0 {
                    self.forwards.remove(u32::from(bind_port));
                }
                return Err(match e {
                    russh::Error::RequestDenied => SshError::Channel(format!(
                        "the server refused remote forwarding of {bind_host}:{bind_port}"
                    )),
                    other => SshError::Protocol(other),
                });
            }
        };
        let bound = if bound == 0 {
            u32::from(bind_port)
        } else {
            bound
        };
        self.forwards.insert(bound, tx);
        Ok(RemoteForward {
            bind_host: bind_host.to_string(),
            bound_port: bound as u16,
            incoming: rx,
        })
    }

    /// Cancel a remote forward previously requested.
    pub async fn cancel_remote_forward(
        &self,
        bind_host: &str,
        bound_port: u16,
    ) -> Result<(), SshError> {
        self.forwards.remove(u32::from(bound_port));
        if self.is_closed() {
            return Ok(());
        }
        self.target
            .cancel_tcpip_forward(bind_host.to_string(), u32::from(bound_port))
            .await
            .map_err(SshError::Protocol)
    }

    /// Disconnect the target and every jump hop (target first).
    pub async fn disconnect(&self) -> Result<(), SshError> {
        let _ = self
            .target
            .disconnect(Disconnect::ByApplication, "bye", "en")
            .await;
        for h in self.hops.iter().rev() {
            let _ = h.disconnect(Disconnect::ByApplication, "bye", "en").await;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::known_hosts::new_known_host;

    #[test]
    fn preferred_puts_known_types_first() {
        let rsa = new_known_host("h", 22, "ssh-rsa", b"x", KnownHostSource::Imported);
        let p = preferred_for(&[rsa]);
        assert!(p.key[0].clone().is_rsa(), "{:?}", p.key);
        assert!(p.host_key_certificates.is_empty());
        let len = Preferred::DEFAULT.key.len();
        assert_eq!(p.key.len(), len);

        let ca = new_known_host("*", 22, "ssh-ed25519", b"y", KnownHostSource::CertAuthority);
        let p = preferred_for(&[ca]);
        assert_eq!(p.key[0], Algorithm::Ed25519);
        assert!(!p.host_key_certificates.is_empty());
    }

    #[tokio::test]
    async fn shell_pair_roundtrip() {
        let (ch, mut remote) = ShellChannel::pair(8);
        let (w, mut r) = ch.split();
        w.write(&b"ls\r"[..]).await.unwrap();
        assert_eq!(
            remote.inputs.recv().await,
            Some(ShellInput::Data(Bytes::from_static(b"ls\r")))
        );
        remote
            .events
            .send(ShellEvent::Data(Bytes::from_static(b"out")))
            .await
            .unwrap();
        assert_eq!(
            r.recv().await,
            Some(ShellEvent::Data(Bytes::from_static(b"out")))
        );
        w.resize(100, 40).await.unwrap();
        assert_eq!(
            remote.inputs.recv().await,
            Some(ShellInput::Resize {
                cols: 100,
                rows: 40
            })
        );
    }

    #[test]
    fn exec_output_lossy() {
        let o = ExecOutput {
            stdout: b"hi\n".to_vec(),
            stderr: vec![0xff],
            exit_status: Some(0),
            exit_signal: None,
        };
        assert_eq!(o.stdout_lossy(), "hi\n");
        assert_eq!(o.stderr_lossy(), "\u{fffd}");
    }
}
