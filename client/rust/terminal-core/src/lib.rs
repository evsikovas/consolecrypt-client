//! # cc-terminal-core — terminal session management
//!
//! [`TerminalManager`] opens interactive PTY shells over ssh-core
//! connection plans, streams output bytes to the UI (with a local-only,
//! in-memory [`Scrollback`] ring buffer for late attach), forwards input and
//! resizes, and reports status changes for reconnect UX. A
//! [`CommandTracker`] per session offers best-effort `last command` /
//! `last error` hooks for the AI layer.

pub mod local;
pub mod scrollback;
pub mod tracker;

pub use scrollback::Scrollback;
pub use tracker::CommandTracker;

use async_trait::async_trait;
use bytes::Bytes;
use cc_models::ObjectId;
use cc_ssh_core::{
    ConnectionPlan, PtyRequest, ShellChannel, ShellEvent, ShellWriter, SshConnector, SshSession,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use tokio::sync::{broadcast, watch};

/// Terminal session id.
pub type TerminalId = uuid::Uuid;

/// Terminal size in character cells.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct TerminalSize {
    pub cols: u32,
    pub rows: u32,
}

impl Default for TerminalSize {
    fn default() -> Self {
        Self { cols: 80, rows: 24 }
    }
}

/// Session status.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum TerminalStatus {
    Connecting,
    Connected,
    /// Remote shell ended or the user closed the session.
    Closed {
        exit_status: Option<u32>,
        reason: Option<String>,
    },
    /// Connection could not be established or was lost. The UI offers
    /// "Reconnect" (a new [`TerminalManager::spawn_open`] with the same plan).
    Failed(String),
}

impl TerminalStatus {
    pub fn is_terminal(&self) -> bool {
        matches!(
            self,
            TerminalStatus::Closed { .. } | TerminalStatus::Failed(_)
        )
    }
}

/// Status change notification.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TerminalEvent {
    pub id: TerminalId,
    pub status: TerminalStatus,
}

/// Errors.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum TerminalError {
    #[error("terminal session {0} not found")]
    NotFound(TerminalId),
    #[error("terminal session {0} is not connected")]
    NotConnected(TerminalId),
    #[error("could not open the terminal: {0}")]
    Open(String),
}

/// An opened shell plus whatever must stay alive with it.
pub struct OpenedShell {
    pub channel: ShellChannel,
    /// SSH session owned by the terminal (disconnected when it closes).
    pub session: Option<Arc<SshSession>>,
    /// Out-of-band close for owned native processes, including cancelled opens.
    pub lifetime: Option<Arc<dyn ShellLifetime>>,
}

pub trait ShellLifetime: Send + Sync {
    fn close(&self);
}

impl std::fmt::Debug for OpenedShell {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OpenedShell")
            .field("owns_session", &self.session.is_some())
            .finish_non_exhaustive()
    }
}

/// Opens shells for a plan (abstracted for tests).
#[async_trait]
pub trait ShellOpener: Send + Sync {
    async fn open_shell(
        &self,
        plan: &ConnectionPlan,
        pty: PtyRequest,
    ) -> Result<OpenedShell, TerminalError>;
}

/// [`ShellOpener`] that connects with the native connector.
#[derive(Debug, Clone)]
pub struct SshShellOpener {
    connector: SshConnector,
}

impl SshShellOpener {
    pub fn new(connector: SshConnector) -> Self {
        Self { connector }
    }
}

#[async_trait]
impl ShellOpener for SshShellOpener {
    async fn open_shell(
        &self,
        plan: &ConnectionPlan,
        pty: PtyRequest,
    ) -> Result<OpenedShell, TerminalError> {
        let session = self
            .connector
            .connect(plan)
            .await
            .map_err(|e| TerminalError::Open(e.to_string()))?;
        let session = Arc::new(session);
        let channel = session
            .open_shell(pty)
            .await
            .map_err(|e| TerminalError::Open(e.to_string()))?;
        Ok(OpenedShell {
            channel,
            session: Some(session),
            lifetime: None,
        })
    }
}

/// Manager configuration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TerminalConfig {
    /// Scrollback capacity per session in bytes (local only).
    pub scrollback_bytes: usize,
    /// `TERM` requested for the PTY.
    pub term: String,
}

impl Default for TerminalConfig {
    fn default() -> Self {
        Self {
            scrollback_bytes: 2 * 1024 * 1024,
            term: "xterm-256color".into(),
        }
    }
}

/// Scrollback snapshot + live output subscription (no gap, no duplicate).
#[derive(Debug)]
pub struct Attachment {
    pub snapshot: Vec<u8>,
    /// Live output. On `RecvError::Lagged` re-attach to resynchronize.
    pub output: broadcast::Receiver<Bytes>,
}

/// Public info about a session.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TerminalInfo {
    pub id: TerminalId,
    pub target: TerminalTarget,
    pub title: String,
    pub status: TerminalStatus,
    pub size: TerminalSize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TerminalTarget {
    Ssh {
        host_id: ObjectId,
    },
    Local {
        profile_id: String,
        shell: String,
        display_name: String,
        distribution: Option<String>,
    },
    Kubernetes {
        context_id: ObjectId,
        namespace: String,
        pod: String,
        container: Option<String>,
    },
    /// A shell in a Docker container; `host_id` is None on this computer.
    Docker {
        host_id: Option<ObjectId>,
        container: String,
    },
}

impl TerminalTarget {
    pub fn host_id(&self) -> Option<ObjectId> {
        match self {
            Self::Ssh { host_id } => Some(*host_id),
            Self::Local { .. } | Self::Kubernetes { .. } | Self::Docker { .. } => None,
        }
    }
}

struct Inner {
    writer: Option<ShellWriter>,
    session: Option<Arc<SshSession>>,
    lifetime: Option<Arc<dyn ShellLifetime>>,
    closing: bool,
    size: TerminalSize,
    scrollback: Scrollback,
    tracker: CommandTracker,
}

struct Session {
    id: TerminalId,
    target: TerminalTarget,
    title: String,
    status: watch::Sender<TerminalStatus>,
    output: broadcast::Sender<Bytes>,
    inner: Mutex<Inner>,
}

impl Session {
    fn lock(&self) -> std::sync::MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// Owns all terminal sessions.
pub struct TerminalManager {
    opener: Arc<dyn ShellOpener>,
    config: TerminalConfig,
    sessions: Mutex<HashMap<TerminalId, Arc<Session>>>,
    events: broadcast::Sender<TerminalEvent>,
    stopped: AtomicBool,
}

impl std::fmt::Debug for TerminalManager {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TerminalManager")
            .field("config", &self.config)
            .field("sessions", &self.map().len())
            .finish_non_exhaustive()
    }
}

impl Drop for TerminalManager {
    fn drop(&mut self) {
        self.begin_shutdown();
    }
}

impl TerminalManager {
    pub fn new(opener: Arc<dyn ShellOpener>) -> Self {
        Self::with_config(opener, TerminalConfig::default())
    }

    pub fn with_config(opener: Arc<dyn ShellOpener>, config: TerminalConfig) -> Self {
        Self {
            opener,
            config,
            sessions: Mutex::new(HashMap::new()),
            events: broadcast::channel(256).0,
            stopped: AtomicBool::new(false),
        }
    }

    fn map(&self) -> std::sync::MutexGuard<'_, HashMap<TerminalId, Arc<Session>>> {
        self.sessions.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn get(&self, id: TerminalId) -> Result<Arc<Session>, TerminalError> {
        self.map()
            .get(&id)
            .cloned()
            .ok_or(TerminalError::NotFound(id))
    }

    fn register(&self, session: Arc<Session>) {
        let mut sessions = self.map();
        sessions.insert(session.id, session.clone());
        // A shutdown may have run after this session was prepared but before
        // registration. Holding the map lock pairs with begin_shutdown's walk.
        if self.stopped.load(Ordering::Acquire) {
            session.lock().closing = true;
        }
    }

    /// Subscribe to status changes of all sessions.
    pub fn events(&self) -> broadcast::Receiver<TerminalEvent> {
        self.events.subscribe()
    }

    fn set_status(events: &broadcast::Sender<TerminalEvent>, s: &Session, status: TerminalStatus) {
        s.status.send_replace(status.clone());
        let _ = events.send(TerminalEvent { id: s.id, status });
    }

    /// Start connecting in the background; returns immediately with status
    /// `Connecting`. Progress is reported via [`Self::events`].
    pub fn spawn_open(&self, plan: ConnectionPlan, size: TerminalSize) -> TerminalId {
        let id = uuid::Uuid::new_v4();
        let session = Arc::new(Session {
            id,
            target: TerminalTarget::Ssh {
                host_id: plan.host_id,
            },
            title: plan.name.clone(),
            status: watch::channel(TerminalStatus::Connecting).0,
            output: broadcast::channel(1024).0,
            inner: Mutex::new(Inner {
                writer: None,
                session: None,
                lifetime: None,
                closing: false,
                size,
                scrollback: Scrollback::new(self.config.scrollback_bytes),
                tracker: CommandTracker::new(),
            }),
        });
        self.register(session.clone());
        if self.stopped.load(Ordering::Acquire) {
            session.lock().closing = true;
            Self::set_status(
                &self.events,
                &session,
                TerminalStatus::Closed {
                    exit_status: None,
                    reason: None,
                },
            );
            return id;
        }
        let _ = self.events.send(TerminalEvent {
            id,
            status: TerminalStatus::Connecting,
        });
        let opener = self.opener.clone();
        let events = self.events.clone();
        let pty = PtyRequest {
            term: self.config.term.clone(),
            cols: size.cols,
            rows: size.rows,
            ..Default::default()
        };
        tokio::spawn(async move {
            match opener.open_shell(&plan, pty).await {
                Ok(opened) => Self::run(session, opened, events).await,
                Err(e) => {
                    let inner = session.lock();
                    if !inner.closing {
                        Self::set_status(&events, &session, TerminalStatus::Failed(e.to_string()));
                    }
                }
            }
        });
        id
    }

    /// Connect and open a shell; resolves once connected (or failed).
    pub async fn open(
        &self,
        plan: ConnectionPlan,
        size: TerminalSize,
    ) -> Result<TerminalId, TerminalError> {
        let id = self.spawn_open(plan, size);
        let mut rx = self.get(id)?.status.subscribe();
        let status = rx
            .wait_for(|s| *s != TerminalStatus::Connecting)
            .await
            .map(|s| s.clone())
            .unwrap_or(TerminalStatus::Failed("session dropped".into()));
        match status {
            TerminalStatus::Failed(e) => {
                self.map().remove(&id);
                Err(TerminalError::Open(e))
            }
            _ => Ok(id),
        }
    }

    /// Open a shell on an existing (shared) SSH session. The session is not
    /// disconnected when the terminal closes.
    pub async fn open_on_session(
        &self,
        ssh: Arc<SshSession>,
        title: impl Into<String>,
        size: TerminalSize,
    ) -> Result<TerminalId, TerminalError> {
        let channel = ssh
            .open_shell(PtyRequest {
                term: self.config.term.clone(),
                cols: size.cols,
                rows: size.rows,
                ..Default::default()
            })
            .await
            .map_err(|e| TerminalError::Open(e.to_string()))?;
        let id = uuid::Uuid::new_v4();
        let session = Arc::new(Session {
            id,
            target: TerminalTarget::Ssh {
                host_id: ssh.host_id(),
            },
            title: title.into(),
            status: watch::channel(TerminalStatus::Connecting).0,
            output: broadcast::channel(1024).0,
            inner: Mutex::new(Inner {
                writer: None,
                session: None,
                lifetime: None,
                closing: self.stopped.load(Ordering::Acquire),
                size,
                scrollback: Scrollback::new(self.config.scrollback_bytes),
                tracker: CommandTracker::new(),
            }),
        });
        self.register(session.clone());
        let events = self.events.clone();
        let installed = Self::install(
            &session,
            OpenedShell {
                channel,
                session: None,
                lifetime: None,
            },
            &events,
        )
        .await;
        let Some((reader, owned)) = installed else {
            self.map().remove(&id);
            return Err(TerminalError::Open("terminal manager is closed".into()));
        };
        tokio::spawn(Self::pump(session, reader, owned, events));
        Ok(id)
    }

    /// A local shell belongs to the same unlocked session as SSH tabs. An
    /// in-flight native spawn after lock is closed before it can be published.
    pub async fn open_local(
        &self,
        profile_id: &str,
        size: TerminalSize,
    ) -> Result<TerminalId, TerminalError> {
        if self.stopped.load(Ordering::Acquire) {
            return Err(TerminalError::Open("terminal manager is closed".into()));
        }
        let (profile, opened) = local::open(profile_id, size).await?;
        let id = uuid::Uuid::new_v4();
        let session = Arc::new(Session {
            id,
            target: TerminalTarget::Local {
                profile_id: profile.id,
                shell: profile.shell,
                display_name: profile.label.clone(),
                distribution: profile.distribution,
            },
            title: profile.label,
            status: watch::channel(TerminalStatus::Connecting).0,
            output: broadcast::channel(1024).0,
            inner: Mutex::new(Inner {
                writer: None,
                session: None,
                lifetime: None,
                closing: self.stopped.load(Ordering::Acquire),
                size,
                scrollback: Scrollback::new(self.config.scrollback_bytes),
                tracker: CommandTracker::new(),
            }),
        });
        self.register(session.clone());
        let mut status = session.status.subscribe();
        tokio::spawn(Self::run(session, opened, self.events.clone()));
        let current = status
            .wait_for(|s| *s != TerminalStatus::Connecting)
            .await
            .map_err(|_| TerminalError::Open("terminal closed during start".into()))?
            .clone();
        if current != TerminalStatus::Connected {
            self.map().remove(&id);
            return Err(TerminalError::Open("terminal closed during start".into()));
        }
        Ok(id)
    }

    /// Register a native transport adapter in the same bounded scrollback,
    /// attach, resize and lock lifecycle used by SSH and local shells.
    pub async fn open_channel(
        &self,
        target: TerminalTarget,
        title: String,
        size: TerminalSize,
        opened: OpenedShell,
    ) -> Result<TerminalId, TerminalError> {
        let id = uuid::Uuid::new_v4();
        let session = Arc::new(Session {
            id,
            target,
            title,
            status: watch::channel(TerminalStatus::Connecting).0,
            output: broadcast::channel(1024).0,
            inner: Mutex::new(Inner {
                writer: None,
                session: None,
                lifetime: None,
                closing: self.stopped.load(Ordering::Acquire),
                size,
                scrollback: Scrollback::new(self.config.scrollback_bytes),
                tracker: CommandTracker::new(),
            }),
        });
        self.register(session.clone());
        let events = self.events.clone();
        let Some((reader, owned)) = Self::install(&session, opened, &events).await else {
            self.map().remove(&id);
            return Err(TerminalError::Open("terminal manager is closed".into()));
        };
        tokio::spawn(Self::pump(session, reader, owned, events));
        Ok(id)
    }

    /// Make the session writable and mark it connected.
    async fn install(
        session: &Session,
        opened: OpenedShell,
        events: &broadcast::Sender<TerminalEvent>,
    ) -> Option<(cc_ssh_core::ShellReader, Option<Arc<SshSession>>)> {
        let (writer, reader) = opened.channel.split();
        let owned = opened.session;
        let lifetime = opened.lifetime;
        loop {
            let (size, closed) = {
                let inner = session.lock();
                (inner.size, inner.closing)
            };
            if closed {
                if let Some(lifetime) = lifetime {
                    lifetime.close();
                }
                let _ = writer.close().await;
                if let Some(ssh) = owned {
                    let _ = ssh.disconnect().await;
                }
                Self::set_status(
                    events,
                    session,
                    TerminalStatus::Closed {
                        exit_status: None,
                        reason: None,
                    },
                );
                return None;
            }
            // Resize before publishing Connected, retrying if the UI resized
            // during the await. No stale initial size can overwrite a later one.
            let _ = writer.resize(size.cols, size.rows).await;
            let mut inner = session.lock();
            if inner.closing || inner.size != size {
                continue;
            }
            inner.writer = Some(writer.clone());
            inner.session = owned.clone();
            inner.lifetime = lifetime;
            Self::set_status(events, session, TerminalStatus::Connected);
            return Some((reader, owned));
        }
    }

    async fn run(
        session: Arc<Session>,
        opened: OpenedShell,
        events: broadcast::Sender<TerminalEvent>,
    ) {
        if let Some((reader, owned)) = Self::install(&session, opened, &events).await {
            Self::pump(session, reader, owned, events).await;
        }
    }

    async fn pump(
        session: Arc<Session>,
        mut reader: cc_ssh_core::ShellReader,
        owned: Option<Arc<SshSession>>,
        events: broadcast::Sender<TerminalEvent>,
    ) {
        let mut exit_status = None;
        let mut reason = None;
        loop {
            let ev = match &owned {
                Some(ssh) => tokio::select! {
                    ev = reader.recv() => ev,
                    _ = ssh.closed() => {
                        reason = Some("connection lost".to_string());
                        None
                    }
                },
                None => reader.recv().await,
            };
            match ev {
                Some(ShellEvent::Data(d)) | Some(ShellEvent::Stderr(d)) => {
                    // Append + broadcast under one lock: attach() sees no gap.
                    let mut inner = session.lock();
                    if inner.closing {
                        continue;
                    }
                    inner.scrollback.push(&d);
                    inner.tracker.on_output(&d);
                    let _ = session.output.send(d);
                }
                Some(ShellEvent::ExitStatus(c)) => exit_status = Some(c),
                Some(ShellEvent::ExitSignal { signal, message }) => {
                    reason = Some(
                        format!("killed by signal {signal} {message}")
                            .trim()
                            .to_string(),
                    )
                }
                Some(ShellEvent::Eof) => {}
                Some(ShellEvent::Closed) | None => break,
            }
        }
        let lost = reason.as_deref() == Some("connection lost");
        {
            let mut inner = session.lock();
            inner.writer = None;
            inner.session = None;
            inner.lifetime = None;
        }
        if let Some(ssh) = owned {
            let _ = ssh.disconnect().await;
        }
        let status = if lost && exit_status.is_none() {
            TerminalStatus::Failed("connection lost".into())
        } else {
            TerminalStatus::Closed {
                exit_status,
                reason,
            }
        };
        Self::set_status(&events, &session, status);
    }

    /// Send input bytes (keystrokes / paste).
    pub async fn write(&self, id: TerminalId, data: &[u8]) -> Result<(), TerminalError> {
        let s = self.get(id)?;
        let writer = {
            let mut inner = s.lock();
            if inner.closing {
                return Err(TerminalError::NotConnected(id));
            }
            let w = inner
                .writer
                .clone()
                .ok_or(TerminalError::NotConnected(id))?;
            inner.tracker.on_input(data);
            w
        };
        writer
            .write(Bytes::copy_from_slice(data))
            .await
            .map_err(|_| TerminalError::NotConnected(id))
    }

    /// Resize the PTY.
    pub async fn resize(&self, id: TerminalId, size: TerminalSize) -> Result<(), TerminalError> {
        let s = self.get(id)?;
        let writer = {
            let mut inner = s.lock();
            inner.size = size;
            inner.writer.clone()
        };
        if let Some(w) = writer {
            w.resize(size.cols, size.rows)
                .await
                .map_err(|_| TerminalError::NotConnected(id))?;
        }
        Ok(())
    }

    /// Scrollback snapshot + live output stream.
    pub fn attach(&self, id: TerminalId) -> Result<Attachment, TerminalError> {
        let s = self.get(id)?;
        let inner = s.lock();
        Ok(Attachment {
            snapshot: inner.scrollback.snapshot(),
            output: s.output.subscribe(),
        })
    }

    /// Current status.
    pub fn status(&self, id: TerminalId) -> Result<TerminalStatus, TerminalError> {
        Ok(self.get(id)?.status.borrow().clone())
    }

    /// Wait until the session reaches a final status.
    pub async fn wait_closed(&self, id: TerminalId) -> Result<TerminalStatus, TerminalError> {
        let mut rx = self.get(id)?.status.subscribe();
        let s = rx
            .wait_for(TerminalStatus::is_terminal)
            .await
            .map(|s| s.clone())
            .unwrap_or(TerminalStatus::Failed("session dropped".into()));
        Ok(s)
    }

    pub fn info(&self, id: TerminalId) -> Result<TerminalInfo, TerminalError> {
        let s = self.get(id)?;
        let size = s.lock().size;
        let status = s.status.borrow().clone();
        Ok(TerminalInfo {
            id,
            target: s.target.clone(),
            title: s.title.clone(),
            status,
            size,
        })
    }

    pub fn list(&self) -> Vec<TerminalInfo> {
        let ids: Vec<TerminalId> = self.map().keys().copied().collect();
        ids.into_iter()
            .filter_map(|id| self.info(id).ok())
            .collect()
    }

    /// Close the shell (and the SSH session if the terminal owns it). The
    /// session stays listed with status `Closed` until [`Self::remove`].
    pub async fn close(&self, id: TerminalId) -> Result<(), TerminalError> {
        let s = self.get(id)?;
        let (writer, ssh, lifetime) = {
            let mut inner = s.lock();
            inner.closing = true;
            Self::set_status(
                &self.events,
                &s,
                TerminalStatus::Closed {
                    exit_status: None,
                    reason: None,
                },
            );
            (
                inner.writer.take(),
                inner.session.take(),
                inner.lifetime.take(),
            )
        };
        if let Some(lifetime) = lifetime {
            lifetime.close();
        }
        if let Some(w) = writer {
            let _ = w.close().await;
        }
        if let Some(ssh) = ssh {
            let _ = ssh.disconnect().await;
        }
        Ok(())
    }

    /// Permanently fence this unlocked runtime before closing its sessions.
    pub async fn shutdown(&self) {
        self.begin_shutdown();
        for info in self.list() {
            let _ = self.remove(info.id).await;
        }
    }

    pub fn begin_shutdown(&self) {
        self.stopped.store(true, Ordering::Release);
        for session in self.map().values() {
            let mut inner = session.lock();
            inner.closing = true;
            if let Some(lifetime) = &inner.lifetime {
                lifetime.close();
            }
            Self::set_status(
                &self.events,
                session,
                TerminalStatus::Closed {
                    exit_status: None,
                    reason: None,
                },
            );
        }
    }

    /// Forget a session and wipe its scrollback / tracker.
    pub async fn remove(&self, id: TerminalId) -> Result<(), TerminalError> {
        self.close(id).await?;
        if let Some(s) = self.map().remove(&id) {
            let mut inner = s.lock();
            inner.scrollback.clear();
            inner.tracker.clear();
        }
        Ok(())
    }

    /// Last command typed in this session (best effort; AI hook).
    pub fn last_command(&self, id: TerminalId) -> Option<String> {
        let s = self.get(id).ok()?;
        let inner = s.lock();
        inner.tracker.last_command().map(str::to_string)
    }

    /// Last error-looking output of this session (best effort; AI hook).
    pub fn last_error(&self, id: TerminalId) -> Option<String> {
        let s = self.get(id).ok()?;
        let inner = s.lock();
        inner.tracker.last_error().map(str::to_string)
    }

    /// Last `n` bytes of scrollback (e.g. "explain this output").
    pub fn scrollback_tail(&self, id: TerminalId, n: usize) -> Result<Vec<u8>, TerminalError> {
        Ok(self.get(id)?.lock().scrollback.tail(n))
    }
}

#[cfg(test)]
mod tests;
