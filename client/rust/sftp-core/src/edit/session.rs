//! One edit session: a handle ([`EditSession`]) and an actor task that owns
//! the state machine (watch → hash → conflict check → replace → status).

use super::opener::{EditOpener, OpenWith};
use super::remote::EditRemote;
use super::replace;
use super::store::{
    remote_copy_name, secure_remove_dir, sha256, write_private_file, BaseVersion, Manifest,
};
use super::types::{
    ConflictResolution, EditConfig, EditError, EditEvent, EditSessionInfo, EditStatus, RemoteMeta,
    StopMode, StopOutcome,
};
use super::watch::DirWatcher;
use crate::{RemoteEntry, SftpError, TransferProgress};
use cc_platform_core::ChooseOutcome;
use chrono::{DateTime, Utc};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime};
use tokio::sync::{broadcast, mpsc, oneshot, watch, Notify};
use tokio::time::Instant;
use uuid::Uuid;

/// Minimum interval between `Uploading` progress events.
const PROGRESS_INTERVAL: Duration = Duration::from_millis(100);

/// Registry key: one session per (host, remote path).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) struct SessionKey {
    host_id: String,
    path: String,
}

impl SessionKey {
    pub(crate) fn new(host_id: &str, path: &str) -> Self {
        Self {
            host_id: host_id.to_string(),
            path: path.to_string(),
        }
    }
}

#[derive(Debug, Default)]
pub(crate) struct RegistryInner {
    pub by_id: HashMap<Uuid, EditSession>,
    pub by_key: HashMap<SessionKey, Uuid>,
    pub opening: HashSet<SessionKey>,
}

pub(crate) type Registry = Arc<Mutex<RegistryInner>>;

pub(crate) fn lock(r: &Registry) -> std::sync::MutexGuard<'_, RegistryInner> {
    r.lock().unwrap_or_else(|e| e.into_inner())
}

type Reply<T> = oneshot::Sender<Result<T, EditError>>;

pub(crate) enum Cmd {
    SyncNow(Reply<EditStatus>),
    Resolve(ConflictResolution, Reply<Option<PathBuf>>),
    Reopen(Option<OpenWith>, Reply<()>),
    Stop(StopMode, Reply<StopOutcome>),
}

pub(crate) struct Shared {
    id: Uuid,
    host_id: String,
    keys: Vec<SessionKey>,
    info: Mutex<EditSessionInfo>,
    status: watch::Sender<EditStatus>,
    events: broadcast::Sender<EditEvent>,
}

impl Shared {
    fn info(&self) -> std::sync::MutexGuard<'_, EditSessionInfo> {
        self.info.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub(crate) fn set_status(&self, s: EditStatus) {
        let changed = self.status.send_if_modified(|cur| {
            if *cur == s {
                false
            } else {
                *cur = s.clone();
                true
            }
        });
        if changed {
            self.info().status = s.clone();
            let _ = self.events.send(EditEvent {
                session_id: self.id,
                status: s,
            });
        }
    }
}

/// Handle to an edit session (cheap to clone). Dropping handles does not end
/// the session while its manager is alive; use [`EditSession::stop`].
#[derive(Clone)]
pub struct EditSession {
    shared: Arc<Shared>,
    /// Only handles hold senders: when the manager and every handle are
    /// gone the actor stops (keeping the files for recovery).
    cmd: mpsc::Sender<Cmd>,
}

impl std::fmt::Debug for EditSession {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EditSession")
            .field("id", &self.shared.id)
            .finish_non_exhaustive()
    }
}

impl EditSession {
    pub fn id(&self) -> Uuid {
        self.shared.id
    }

    /// Caller-defined host identity given to `open`.
    pub fn host_id(&self) -> &str {
        &self.shared.host_id
    }

    /// Snapshot for the "Editing" list.
    pub fn info(&self) -> EditSessionInfo {
        self.shared.info().clone()
    }

    pub fn status(&self) -> EditStatus {
        self.shared.status.borrow().clone()
    }

    /// Status updates of this session.
    pub fn watch_status(&self) -> watch::Receiver<EditStatus> {
        self.shared.status.subscribe()
    }

    /// Local working copy (e.g. for "Reveal in Finder").
    pub fn local_path(&self) -> PathBuf {
        self.shared.info().local_path.clone()
    }

    pub fn is_closed(&self) -> bool {
        *self.shared.status.borrow() == EditStatus::Closed || self.cmd.is_closed()
    }

    pub(crate) fn keys(&self) -> &[SessionKey] {
        &self.shared.keys
    }

    pub(crate) fn set_status(&self, s: EditStatus) {
        self.shared.set_status(s);
    }

    async fn call<T>(&self, make: impl FnOnce(Reply<T>) -> Cmd) -> Result<T, EditError> {
        let (tx, rx) = oneshot::channel();
        self.cmd
            .send(make(tx))
            .await
            .map_err(|_| EditError::Closed)?;
        rx.await.map_err(|_| EditError::Closed)?
    }

    /// Check the working copy now and upload it if it changed ("Retry" /
    /// "Upload now"). Resets the automatic retry budget. Returns the
    /// resulting status.
    pub async fn sync_now(&self) -> Result<EditStatus, EditError> {
        self.call(Cmd::SyncNow).await
    }

    /// Resolve a conflict. Returns the path of the saved remote copy for
    /// [`ConflictResolution::KeepRemoteCopyLocally`].
    pub async fn resolve(&self, r: ConflictResolution) -> Result<Option<PathBuf>, EditError> {
        self.call(|tx| Cmd::Resolve(r, tx)).await
    }

    /// Open the working copy again (with the session's application unless
    /// `with` is given).
    pub async fn reopen(&self, with: Option<OpenWith>) -> Result<(), EditError> {
        self.call(|tx| Cmd::Reopen(with, tx)).await
    }

    /// End the session (see [`StopMode`]).
    pub async fn stop(&self, mode: StopMode) -> Result<StopOutcome, EditError> {
        self.call(|tx| Cmd::Stop(mode, tx)).await
    }
}

/// Everything needed to start a session.
pub(crate) struct NewSession {
    pub id: Uuid,
    pub host_id: String,
    pub remote_path: String,
    pub target: String,
    /// Canonical session directory.
    pub dir: PathBuf,
    pub file_name: String,
    pub base: BaseVersion,
    pub created_at: DateTime<Utc>,
    pub open_with: OpenWith,
}

enum Local {
    Missing,
    /// Changed while being read (an editor is mid-save).
    Unstable,
    TooLarge(u64),
    Ok(Snapshot),
}

struct Snapshot {
    data: Vec<u8>,
    hash: [u8; 32],
}

enum Upload {
    Done,
    Conflict(Option<RemoteMeta>),
    Failed(String),
}

type Seen = Option<(u64, Option<SystemTime>)>;

fn seen(path: &Path) -> Seen {
    std::fs::metadata(path)
        .ok()
        .map(|m| (m.len(), m.modified().ok()))
}

fn read_local_blocking(path: &Path, max: u64) -> std::io::Result<Local> {
    use std::io::ErrorKind::NotFound;
    let m1 = match std::fs::metadata(path) {
        Ok(m) => m,
        Err(e) if e.kind() == NotFound => return Ok(Local::Missing),
        Err(e) => return Err(e),
    };
    if m1.len() > max {
        return Ok(Local::TooLarge(m1.len()));
    }
    let data = match std::fs::read(path) {
        Ok(d) => d,
        Err(e) if e.kind() == NotFound => return Ok(Local::Missing),
        Err(e) => return Err(e),
    };
    let m2 = match std::fs::metadata(path) {
        Ok(m) => m,
        Err(e) if e.kind() == NotFound => return Ok(Local::Missing),
        Err(e) => return Err(e),
    };
    if m1.len() != m2.len()
        || m2.len() != data.len() as u64
        || m1.modified().ok() != m2.modified().ok()
    {
        return Ok(Local::Unstable);
    }
    if data.len() as u64 > max {
        return Ok(Local::TooLarge(data.len() as u64));
    }
    let hash = sha256(&data);
    Ok(Local::Ok(Snapshot { data, hash }))
}

/// Transient errors are retried automatically; the rest wait for the user.
fn is_transient(e: &SftpError) -> bool {
    matches!(
        e,
        SftpError::Ssh(_) | SftpError::Remote { .. } | SftpError::Local { .. }
    )
}

pub(crate) async fn open_in_editor(
    opener: Arc<dyn EditOpener>,
    path: PathBuf,
    with: OpenWith,
) -> Result<ChooseOutcome, EditError> {
    tokio::task::spawn_blocking(move || opener.open(&path, &with))
        .await
        .map_err(|e| EditError::Opener(e.to_string()))?
        .map_err(|e| EditError::Opener(e.to_string()))
}

async fn sleep_opt(at: Option<Instant>) {
    match at {
        Some(t) => tokio::time::sleep_until(t).await,
        None => std::future::pending().await,
    }
}

async fn tick_opt(i: &mut Option<tokio::time::Interval>) {
    match i {
        Some(i) => {
            i.tick().await;
        }
        None => std::future::pending().await,
    }
}

pub(crate) struct Actor {
    shared: Arc<Shared>,
    /// Weak: dropping the manager (and every handle) ends the actor, which
    /// then keeps its files for recovery.
    registry: std::sync::Weak<Mutex<RegistryInner>>,
    remote: Arc<dyn EditRemote>,
    opener: Arc<dyn EditOpener>,
    cfg: Arc<EditConfig>,
    id: Uuid,
    host_id: String,
    remote_path: String,
    target: String,
    dir: PathBuf,
    work: PathBuf,
    file_name: String,
    created_at: DateTime<Utc>,
    base: BaseVersion,
    /// Hash of the working copy content last acted upon.
    ack: Option<[u8; 32]>,
    /// Our non-atomic replace removed the remote file and failed afterwards.
    remote_removed_by_us: bool,
    /// Temp files kept on the server after such a failure (removed later).
    orphan_tmps: Vec<String>,
    open_with: OpenWith,
    attempt: u32,
    retry_at: Option<Instant>,
    watcher: Option<DirWatcher>,
    changed: Arc<Notify>,
    last_seen: Seen,
}

impl Actor {
    pub(crate) fn new(
        n: NewSession,
        keys: Vec<SessionKey>,
        registry: std::sync::Weak<Mutex<RegistryInner>>,
        remote: Arc<dyn EditRemote>,
        opener: Arc<dyn EditOpener>,
        cfg: Arc<EditConfig>,
        events: broadcast::Sender<EditEvent>,
    ) -> (Self, EditSession, mpsc::Receiver<Cmd>) {
        let work = n.dir.join(&n.file_name);
        let (cmd_tx, cmd_rx) = mpsc::channel(16);
        let (status_tx, _) = watch::channel(EditStatus::Opening);
        let info = EditSessionInfo {
            id: n.id,
            host_id: n.host_id.clone(),
            remote_path: n.remote_path.clone(),
            target_path: n.target.clone(),
            local_path: work.clone(),
            status: EditStatus::Opening,
            opened_at: Utc::now(),
            last_synced_at: None,
            uploads: 0,
            remote_copies: Vec::new(),
            app: match &n.open_with {
                OpenWith::App(a) => Some(a.clone()),
                _ => None,
            },
        };
        let shared = Arc::new(Shared {
            id: n.id,
            host_id: n.host_id.clone(),
            keys,
            info: Mutex::new(info),
            status: status_tx,
            events,
        });
        let session = EditSession {
            shared: shared.clone(),
            cmd: cmd_tx,
        };
        let last_seen = seen(&work);
        let actor = Self {
            shared,
            registry,
            remote,
            opener,
            cfg,
            id: n.id,
            host_id: n.host_id,
            remote_path: n.remote_path,
            target: n.target,
            dir: n.dir,
            work,
            file_name: n.file_name,
            created_at: n.created_at,
            base: n.base,
            ack: None,
            remote_removed_by_us: false,
            orphan_tmps: Vec::new(),
            open_with: n.open_with,
            attempt: 0,
            retry_at: None,
            watcher: None,
            changed: Arc::new(Notify::new()),
            last_seen,
        };
        (actor, session, cmd_rx)
    }

    pub(crate) fn work_path(&self) -> &Path {
        &self.work
    }

    /// Start the directory watcher. Without a poll fallback a watcher
    /// failure is fatal.
    pub(crate) fn start_watcher(&mut self) -> Result<(), EditError> {
        match DirWatcher::start(
            &self.dir,
            &self.file_name,
            self.cfg.debounce,
            self.changed.clone(),
        ) {
            Ok(w) => {
                self.watcher = Some(w);
                Ok(())
            }
            Err(e) if self.cfg.poll_interval.is_some() => {
                tracing::warn!(error = %e, "edit watcher unavailable; polling only");
                Ok(())
            }
            Err(e) => Err(EditError::Watch(e)),
        }
    }

    /// Remember how the working copy was opened (a chooser's pick is reused
    /// for re-opens and remote copies).
    pub(crate) fn remember_choice(&mut self, with: OpenWith, outcome: &ChooseOutcome) {
        self.open_with = match (with, outcome) {
            (OpenWith::Choose, ChooseOutcome::Opened { app: Some(app) }) => {
                OpenWith::App(app.clone())
            }
            (OpenWith::Choose, _) => OpenWith::Default,
            (w, _) => w,
        };
        self.shared.info().app = match &self.open_with {
            OpenWith::App(a) => Some(a.clone()),
            _ => None,
        };
    }

    /// Ask for an immediate check of the working copy (resume).
    pub(crate) fn request_check(&self) {
        self.changed.notify_one();
    }

    pub(crate) async fn run(mut self, mut rx: mpsc::Receiver<Cmd>) {
        let changed = self.changed.clone();
        let mut poll = self.cfg.poll_interval.map(|d| {
            let mut i = tokio::time::interval_at(Instant::now() + d, d);
            i.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            i
        });
        loop {
            let retry_at = self.retry_at;
            tokio::select! {
                cmd = rx.recv() => match cmd {
                    Some(cmd) => {
                        if self.handle(cmd).await {
                            break;
                        }
                    }
                    None => {
                        // Every handle is gone (manager dropped): keep the files.
                        self.close_keep();
                        break;
                    }
                },
                _ = changed.notified() => self.on_local_change(false).await,
                _ = sleep_opt(retry_at) => {
                    self.retry_at = None;
                    self.on_local_change(true).await;
                }
                _ = tick_opt(&mut poll) => {
                    let now = seen(&self.work);
                    if now != self.last_seen {
                        self.last_seen = now;
                        self.on_local_change(false).await;
                    }
                }
            }
        }
    }

    async fn handle(&mut self, cmd: Cmd) -> bool {
        match cmd {
            Cmd::SyncNow(tx) => {
                let r = self.sync_now().await;
                let _ = tx.send(r);
                false
            }
            Cmd::Resolve(r, tx) => {
                let res = self.resolve(r).await;
                let _ = tx.send(res);
                false
            }
            Cmd::Reopen(with, tx) => {
                let res = self.reopen(with).await;
                let _ = tx.send(res);
                false
            }
            Cmd::Stop(mode, tx) => {
                let (res, exit) = self.stop(mode).await;
                let _ = tx.send(res);
                exit
            }
        }
    }

    fn status(&self) -> EditStatus {
        self.shared.status.borrow().clone()
    }

    fn in_conflict(&self) -> bool {
        matches!(self.status(), EditStatus::Conflict { .. })
    }

    fn set(&self, s: EditStatus) {
        self.shared.set_status(s);
    }

    async fn read_local(&self) -> std::io::Result<Local> {
        let path = self.work.clone();
        let max = self.cfg.max_file_size;
        tokio::task::spawn_blocking(move || read_local_blocking(&path, max))
            .await
            .map_err(std::io::Error::other)?
    }

    fn too_large_message(&self, size: u64) -> String {
        format!(
            "local copy is {size} bytes, above the edit limit of {} bytes; not uploaded",
            self.cfg.max_file_size
        )
    }

    async fn on_local_change(&mut self, force: bool) {
        if self.in_conflict() {
            return;
        }
        let snap = match self.read_local().await {
            Ok(Local::Ok(s)) => s,
            Ok(Local::Missing | Local::Unstable) => return,
            Ok(Local::TooLarge(size)) => {
                self.retry_at = None;
                self.set(EditStatus::Error {
                    message: self.too_large_message(size),
                    retryable: false,
                });
                return;
            }
            Err(e) => {
                self.set(EditStatus::Error {
                    message: format!("cannot read the local copy: {e}"),
                    retryable: true,
                });
                return;
            }
        };
        self.last_seen = seen(&self.work);
        if snap.hash == self.base.sha256 && !self.remote_removed_by_us {
            self.ack = Some(snap.hash);
            self.retry_at = None;
            self.attempt = 0;
            self.set(EditStatus::Synced);
            return;
        }
        if !force && self.ack == Some(snap.hash) {
            return;
        }
        if self.ack != Some(snap.hash) {
            self.attempt = 0;
        }
        self.ack = Some(snap.hash);
        self.upload(snap, true).await;
    }

    async fn upload(&mut self, snap: Snapshot, check_conflict: bool) -> Upload {
        if check_conflict {
            match self.remote.stat(&self.target).await {
                Ok(e) if self.base.matches(&e) && !self.remote_removed_by_us => {}
                Ok(e) => return self.conflict(Some(RemoteMeta::from(&e))),
                Err(SftpError::NotFound(_)) if self.remote_removed_by_us => {}
                Err(SftpError::NotFound(_)) => return self.conflict(None),
                Err(e) => return self.failed(e),
            }
        }
        let total = snap.data.len() as u64;
        self.set(EditStatus::Uploading {
            progress: TransferProgress {
                transferred: 0,
                total: Some(total),
            },
        });
        let shared = self.shared.clone();
        let mut last = std::time::Instant::now();
        let mut progress = move |p: TransferProgress| {
            if p.total == Some(p.transferred) || last.elapsed() >= PROGRESS_INTERVAL {
                last = std::time::Instant::now();
                shared.set_status(EditStatus::Uploading { progress: p });
            }
        };
        let result = replace::replace(
            &*self.remote,
            &self.target,
            &snap.data,
            &self.base,
            self.cfg.in_place_fallback,
            &mut progress,
        )
        .await;
        match result {
            Ok(done) => {
                tracing::debug!(
                    session = %self.id,
                    remote_path = %self.target,
                    strategy = ?done.strategy,
                    bytes = total,
                    "edit session uploaded"
                );
                self.rebase(&done.entry, snap.hash);
                self.remote_removed_by_us = false;
                self.attempt = 0;
                self.retry_at = None;
                for tmp in std::mem::take(&mut self.orphan_tmps) {
                    let _ = self.remote.remove_file(&tmp).await;
                }
                self.save_manifest();
                {
                    let mut info = self.shared.info();
                    info.uploads += 1;
                    info.last_synced_at = Some(Utc::now());
                }
                self.set(EditStatus::Synced);
                Upload::Done
            }
            Err(f) => {
                if f.target_removed {
                    self.remote_removed_by_us = true;
                }
                self.orphan_tmps.extend(f.leftover_tmp);
                self.failed(*f.error)
            }
        }
    }

    /// New base version from remote metadata + content hash (keeping known
    /// permission / owner values the server did not report this time).
    fn rebase(&mut self, e: &RemoteEntry, hash: [u8; 32]) {
        let mut b = BaseVersion::new(e, hash);
        b.permissions = b.permissions.or(self.base.permissions);
        b.uid = b.uid.or(self.base.uid);
        b.gid = b.gid.or(self.base.gid);
        self.base = b;
    }

    fn conflict(&mut self, meta: Option<RemoteMeta>) -> Upload {
        self.retry_at = None;
        tracing::debug!(session = %self.id, remote_path = %self.target, "edit session conflict");
        self.set(EditStatus::Conflict {
            remote_meta: meta.clone(),
        });
        Upload::Conflict(meta)
    }

    fn failed(&mut self, e: SftpError) -> Upload {
        let message = e.to_string();
        if is_transient(&e) && self.attempt < self.cfg.auto_retries {
            let delay = self
                .cfg
                .retry_backoff
                .saturating_mul(1u32 << self.attempt.min(16));
            self.attempt += 1;
            self.retry_at = Some(Instant::now() + delay);
        } else {
            self.retry_at = None;
        }
        tracing::debug!(session = %self.id, error = %message, "edit upload failed");
        self.set(EditStatus::Error {
            message: message.clone(),
            retryable: true,
        });
        Upload::Failed(message)
    }

    fn save_manifest(&self) {
        let m = Manifest {
            created_at: self.created_at,
            ..Manifest::new(
                self.id,
                &self.host_id,
                &self.remote_path,
                &self.target,
                &self.file_name,
                self.base.clone(),
            )
        };
        if let Err(e) = m.save(&self.dir) {
            tracing::debug!(session = %self.id, error = %e, "cannot save edit manifest");
        }
    }

    async fn sync_now(&mut self) -> Result<EditStatus, EditError> {
        if self.in_conflict() {
            return Err(EditError::InvalidState("resolve the conflict first"));
        }
        self.retry_at = None;
        self.attempt = 0;
        self.on_local_change(true).await;
        Ok(self.status())
    }

    async fn read_local_required(&self) -> Result<Snapshot, EditError> {
        for _ in 0..10 {
            match self
                .read_local()
                .await
                .map_err(|e| EditError::local(&self.work, e))?
            {
                Local::Ok(s) => return Ok(s),
                Local::Unstable => tokio::time::sleep(Duration::from_millis(100)).await,
                Local::Missing => {
                    return Err(EditError::local(
                        &self.work,
                        std::io::ErrorKind::NotFound.into(),
                    ))
                }
                Local::TooLarge(size) => {
                    return Err(EditError::TooLarge {
                        size,
                        limit: self.cfg.max_file_size,
                    })
                }
            }
        }
        Err(EditError::InvalidState("the local copy keeps changing"))
    }

    async fn fetch_remote(&self) -> Result<(RemoteEntry, Vec<u8>), EditError> {
        let entry = self.remote.stat(&self.target).await?;
        if entry.size > self.cfg.max_file_size {
            return Err(EditError::TooLarge {
                size: entry.size,
                limit: self.cfg.max_file_size,
            });
        }
        let data = self
            .remote
            .read_file(&self.target, self.cfg.max_file_size, &mut |_| {})
            .await?;
        Ok((entry, data))
    }

    async fn resolve(&mut self, r: ConflictResolution) -> Result<Option<PathBuf>, EditError> {
        if !self.in_conflict() {
            return Err(EditError::InvalidState("no conflict to resolve"));
        }
        match r {
            ConflictResolution::OverwriteRemote => {
                let snap = self.read_local_required().await?;
                self.ack = Some(snap.hash);
                self.attempt = 0;
                match self.upload(snap, false).await {
                    Upload::Done => Ok(None),
                    Upload::Failed(m) => Err(EditError::Upload(m)),
                    Upload::Conflict(_) => Err(EditError::InvalidState("unexpected conflict")),
                }
            }
            ConflictResolution::KeepRemoteCopyLocally => {
                let (entry, data) = self.fetch_remote().await?;
                let now = Utc::now();
                let mut copy = None;
                for n in 0..100 {
                    let p = self.dir.join(remote_copy_name(&self.file_name, now, n));
                    match write_private_file(&p, &data, true) {
                        Ok(()) => {
                            copy = Some(p);
                            break;
                        }
                        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
                        Err(e) => return Err(EditError::local(&p, e)),
                    }
                }
                let copy = copy.ok_or(EditError::InvalidState("too many remote copies"))?;
                let hash = sha256(&data);
                drop(data);
                self.rebase(&entry, hash);
                self.remote_removed_by_us = false;
                self.save_manifest();
                self.shared.info().remote_copies.push(copy.clone());
                if let Err(e) =
                    open_in_editor(self.opener.clone(), copy.clone(), self.open_with.clone()).await
                {
                    tracing::debug!(session = %self.id, error = %e, "cannot open remote copy");
                }
                // The working copy stays; its next save uploads against the
                // new base (no conflict).
                let status = match self.read_local().await {
                    Ok(Local::Ok(s)) => {
                        self.ack = Some(s.hash);
                        if s.hash == self.base.sha256 {
                            EditStatus::Synced
                        } else {
                            EditStatus::Modified
                        }
                    }
                    _ => {
                        self.ack = None;
                        EditStatus::Modified
                    }
                };
                self.set(status);
                Ok(Some(copy))
            }
            ConflictResolution::DiscardLocal => {
                let (entry, data) = self.fetch_remote().await?;
                write_private_file(&self.work, &data, false)
                    .map_err(|e| EditError::local(&self.work, e))?;
                let hash = sha256(&data);
                drop(data);
                self.rebase(&entry, hash);
                self.ack = Some(hash);
                self.remote_removed_by_us = false;
                self.last_seen = seen(&self.work);
                self.save_manifest();
                self.set(EditStatus::Synced);
                Ok(None)
            }
        }
    }

    async fn reopen(&mut self, with: Option<OpenWith>) -> Result<(), EditError> {
        let with = with.unwrap_or_else(|| self.open_with.clone());
        let outcome = open_in_editor(self.opener.clone(), self.work.clone(), with.clone()).await?;
        if outcome == ChooseOutcome::Cancelled {
            return Err(EditError::Cancelled);
        }
        self.remember_choice(with, &outcome);
        Ok(())
    }

    async fn stop(&mut self, mode: StopMode) -> (Result<StopOutcome, EditError>, bool) {
        let keep_on_failure = mode == StopMode::UploadOrKeep;
        match mode {
            StopMode::Discard => (Ok(self.close_remove(false)), true),
            StopMode::KeepFiles => (Ok(self.close_keep()), true),
            StopMode::Upload | StopMode::UploadOrKeep => match self.flush().await {
                Ok(uploaded) => (Ok(self.close_remove(uploaded)), true),
                Err(_) if keep_on_failure => (Ok(self.close_keep()), true),
                Err(outcome) => (Ok(outcome), false),
            },
        }
    }

    /// Final upload. `Ok(uploaded)` when nothing is pending any more;
    /// `Err(outcome)` when the session must stay open.
    async fn flush(&mut self) -> Result<bool, StopOutcome> {
        if let EditStatus::Conflict { remote_meta } = self.status() {
            return Err(StopOutcome::Conflict { remote_meta });
        }
        let snap = match self.read_local_required().await {
            Ok(s) => s,
            // Never delete the remote file because the local one is gone.
            Err(EditError::Local { source, .. })
                if source.kind() == std::io::ErrorKind::NotFound =>
            {
                return Ok(false)
            }
            Err(e) => {
                return Err(StopOutcome::UploadFailed {
                    message: e.to_string(),
                })
            }
        };
        if snap.hash == self.base.sha256 && !self.remote_removed_by_us {
            return Ok(false);
        }
        self.ack = Some(snap.hash);
        match self.upload(snap, true).await {
            Upload::Done => Ok(true),
            Upload::Conflict(remote_meta) => Err(StopOutcome::Conflict { remote_meta }),
            Upload::Failed(message) => Err(StopOutcome::UploadFailed { message }),
        }
    }

    fn unregister(&self) {
        if let Some(registry) = self.registry.upgrade() {
            let mut r = lock(&registry);
            r.by_id.remove(&self.id);
            r.by_key.retain(|_, v| *v != self.id);
        }
    }

    fn close_remove(&mut self, uploaded: bool) -> StopOutcome {
        self.watcher = None;
        self.retry_at = None;
        self.unregister();
        let outcome = match secure_remove_dir(&self.dir, self.cfg.secure_overwrite_limit) {
            Ok(()) => StopOutcome::Closed { uploaded },
            Err(e) => {
                tracing::debug!(session = %self.id, error = %e, "cannot remove edit directory");
                StopOutcome::KeptFiles {
                    dir: self.dir.clone(),
                }
            }
        };
        self.set(EditStatus::Closed);
        outcome
    }

    fn close_keep(&mut self) -> StopOutcome {
        self.watcher = None;
        self.retry_at = None;
        self.save_manifest();
        self.unregister();
        self.set(EditStatus::Closed);
        StopOutcome::KeptFiles {
            dir: self.dir.clone(),
        }
    }
}
