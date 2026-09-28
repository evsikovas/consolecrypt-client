//! # cc-terminal-core — terminal session management
//!
//! [`TerminalManager`] opens interactive PTY shells over ssh-core
//! connection plans, streams output bytes to the UI (with a local-only,
//! in-memory [`Scrollback`] ring buffer for late attach), forwards input and
//! resizes, and reports status changes for reconnect UX. A
//! [`CommandTracker`] per session offers best-effort `last command` /
//! `last error` hooks for the AI layer.

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
    pub host_id: ObjectId,
    pub title: String,
    pub status: TerminalStatus,
    pub size: TerminalSize,
}

struct Inner {
    writer: Option<ShellWriter>,
    session: Option<Arc<SshSession>>,
    size: TerminalSize,
    scrollback: Scrollback,
    tracker: CommandTracker,
}

struct Session {
    id: TerminalId,
    host_id: ObjectId,
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
}

impl std::fmt::Debug for TerminalManager {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TerminalManager")
            .field("config", &self.config)
            .field("sessions", &self.map().len())
            .finish_non_exhaustive()
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
            host_id: plan.host_id,
            title: plan.name.clone(),
            status: watch::channel(TerminalStatus::Connecting).0,
            output: broadcast::channel(1024).0,
            inner: Mutex::new(Inner {
                writer: None,
                session: None,
                size,
                scrollback: Scrollback::new(self.config.scrollback_bytes),
                tracker: CommandTracker::new(),
            }),
        });
        self.map().insert(id, session.clone());
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
                    Self::set_status(&events, &session, TerminalStatus::Failed(e.to_string()))
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
            host_id: ssh.host_id(),
            title: title.into(),
            status: watch::channel(TerminalStatus::Connecting).0,
            output: broadcast::channel(1024).0,
            inner: Mutex::new(Inner {
                writer: None,
                session: None,
                size,
                scrollback: Scrollback::new(self.config.scrollback_bytes),
                tracker: CommandTracker::new(),
            }),
        });
        self.map().insert(id, session.clone());
        let events = self.events.clone();
        let (reader, owned) = Self::install(
            &session,
            OpenedShell {
                channel,
                session: None,
            },
            &events,
        );
        tokio::spawn(Self::pump(session, reader, owned, events));
        Ok(id)
    }

    /// Make the session writable and mark it connected.
    fn install(
        session: &Session,
        opened: OpenedShell,
        events: &broadcast::Sender<TerminalEvent>,
    ) -> (cc_ssh_core::ShellReader, Option<Arc<SshSession>>) {
        let (writer, reader) = opened.channel.split();
        let owned = opened.session;
        {
            let mut inner = session.lock();
            inner.writer = Some(writer);
            inner.session = owned.clone();
        }
        Self::set_status(events, session, TerminalStatus::Connected);
        (reader, owned)
    }

    async fn run(
        session: Arc<Session>,
        opened: OpenedShell,
        events: broadcast::Sender<TerminalEvent>,
    ) {
        let (reader, owned) = Self::install(&session, opened, &events);
        Self::pump(session, reader, owned, events).await
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
            host_id: s.host_id,
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
        let (writer, ssh) = {
            let inner = s.lock();
            (inner.writer.clone(), inner.session.clone())
        };
        if let Some(w) = writer {
            let _ = w.close().await;
        }
        if let Some(ssh) = ssh {
            let _ = ssh.disconnect().await;
        }
        Ok(())
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
