//! [`EditManager`]: opens, tracks and stops edit sessions.

use super::opener::{EditOpener, OpenWith};
use super::remote::EditRemote;
use super::session::{lock, open_in_editor, Actor, NewSession, Registry, SessionKey};
use super::store::{
    create_session_dir, list_leftovers, prepare_root, remote_base_name, remove_leftover,
    sanitize_file_name, secure_remove_dir, sha256, write_private_file, BaseVersion, Leftover,
    Manifest,
};
use super::types::{
    EditConfig, EditError, EditEvent, EditSessionInfo, EditStatus, StopMode, StopOutcome,
};
use super::EditSession;
use crate::EntryKind;
use cc_platform_core::ChooseOutcome;
use chrono::Utc;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::{broadcast, watch};
use uuid::Uuid;

const EVENT_CAPACITY: usize = 256;

struct Inner {
    cfg: Arc<EditConfig>,
    opener: Arc<dyn EditOpener>,
    registry: Registry,
    events: broadcast::Sender<EditEvent>,
    shutdown: watch::Sender<bool>,
}

/// Manages the edit sessions of one profile. Cheap to clone.
///
/// Lifecycle hooks for the facade: "Stop editing" → [`EditSession::stop`]
/// with [`StopMode::Upload`]; disconnect of a host → [`EditManager::stop_host`];
/// vault lock / app quit → [`EditManager::shutdown`] with
/// [`StopMode::UploadOrKeep`]; app start → [`EditManager::leftovers`]
/// ("Recover unsaved edits?" → [`EditManager::resume`] or
/// [`EditManager::discard_leftover`]).
#[derive(Clone)]
pub struct EditManager {
    inner: Arc<Inner>,
}

impl std::fmt::Debug for EditManager {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EditManager")
            .field("root", &self.inner.cfg.root)
            .field("sessions", &lock(&self.inner.registry).by_id.len())
            .finish_non_exhaustive()
    }
}

/// Removes in-flight "opening" reservations when an open ends.
struct OpeningGuard {
    registry: Registry,
    keys: Vec<SessionKey>,
}

impl OpeningGuard {
    fn acquire(registry: &Registry, keys: Vec<SessionKey>, path: &str) -> Result<Self, EditError> {
        let mut r = lock(registry);
        if r.closed {
            return Err(EditError::Closed);
        }
        if keys.iter().any(|k| r.opening.contains(k)) {
            return Err(EditError::Busy(path.to_string()));
        }
        r.opening.extend(keys.iter().cloned());
        Ok(Self {
            registry: registry.clone(),
            keys,
        })
    }
}

/// Cancellation must clean a newly downloaded plaintext working copy too,
/// including while an OS chooser is still running on its blocking thread.
struct PendingWorkingCopy {
    dir: PathBuf,
    overwrite_limit: u64,
    committed: bool,
}

impl Drop for PendingWorkingCopy {
    fn drop(&mut self) {
        if !self.committed {
            let _ = secure_remove_dir(&self.dir, self.overwrite_limit);
        }
    }
}

impl Drop for OpeningGuard {
    fn drop(&mut self) {
        let mut r = lock(&self.registry);
        for k in &self.keys {
            r.opening.remove(k);
        }
    }
}

fn normalize(path: &str) -> String {
    let t = path.trim_end_matches('/');
    if t.is_empty() && path.starts_with('/') {
        "/".into()
    } else {
        t.to_string()
    }
}

impl EditManager {
    /// Create the manager; prepares the edit root (0700, Spotlight marker).
    pub fn new(cfg: EditConfig, opener: Arc<dyn EditOpener>) -> Result<Self, EditError> {
        prepare_root(&cfg.root, cfg.exclude_from_backup)
            .map_err(|e| EditError::local(&cfg.root, e))?;
        let (events, _) = broadcast::channel(EVENT_CAPACITY);
        Ok(Self {
            inner: Arc::new(Inner {
                cfg: Arc::new(cfg),
                opener,
                registry: Registry::default(),
                events,
                shutdown: watch::channel(false).0,
            }),
        })
    }

    pub fn config(&self) -> &EditConfig {
        &self.inner.cfg
    }

    /// Status changes of all sessions (lagging receivers skip old events;
    /// [`EditManager::sessions`] gives the current state).
    pub fn subscribe(&self) -> broadcast::Receiver<EditEvent> {
        self.inner.events.subscribe()
    }

    /// Active sessions, oldest first.
    pub fn sessions(&self) -> Vec<EditSessionInfo> {
        let mut v: Vec<_> = lock(&self.inner.registry)
            .by_id
            .values()
            .map(EditSession::info)
            .collect();
        v.sort_by_key(|i| i.opened_at);
        v
    }

    pub fn session(&self, id: Uuid) -> Option<EditSession> {
        lock(&self.inner.registry).by_id.get(&id).cloned()
    }

    /// The session editing `remote_path` on `host_id` (requested or resolved
    /// path).
    pub fn find(&self, host_id: &str, remote_path: &str) -> Option<EditSession> {
        self.find_key(&SessionKey::new(host_id, &normalize(remote_path)))
    }

    fn find_key(&self, key: &SessionKey) -> Option<EditSession> {
        let r = lock(&self.inner.registry);
        r.by_key.get(key).and_then(|id| r.by_id.get(id)).cloned()
    }

    /// Bring an existing session's working copy up again. `None` if there is
    /// none or it closed meanwhile (then a new session is opened).
    async fn reuse(
        &self,
        existing: Option<EditSession>,
        with: &OpenWith,
    ) -> Result<Option<EditSession>, EditError> {
        let Some(s) = existing else { return Ok(None) };
        match s.reopen(Some(with.clone())).await {
            Ok(()) => Ok(Some(s)),
            Err(EditError::Closed) => Ok(None),
            Err(e) => Err(e),
        }
    }

    fn register(&self, session: &EditSession) -> Result<(), EditError> {
        let mut r = lock(&self.inner.registry);
        if r.closed {
            return Err(EditError::Closed);
        }
        for k in session.keys() {
            r.by_key.insert(k.clone(), session.id());
        }
        r.by_id.insert(session.id(), session.clone());
        Ok(())
    }

    fn emit(&self, session_id: Uuid, status: EditStatus) {
        let _ = self.inner.events.send(EditEvent { session_id, status });
    }

    /// Download `remote_path` into a private working directory, open it with
    /// `with` and upload every save. If the file is already being edited the
    /// existing session is returned and its working copy opened again.
    pub async fn open(
        &self,
        remote: Arc<dyn EditRemote>,
        host_id: &str,
        remote_path: &str,
        with: OpenWith,
    ) -> Result<EditSession, EditError> {
        let mut shutdown = self.inner.shutdown.subscribe();
        if *shutdown.borrow() {
            return Err(EditError::Closed);
        }
        tokio::select! {
            biased;
            _ = shutdown.changed() => Err(EditError::Closed),
            result = self.open_active(remote, host_id, remote_path, with) => result,
        }
    }

    async fn open_active(
        &self,
        remote: Arc<dyn EditRemote>,
        host_id: &str,
        remote_path: &str,
        with: OpenWith,
    ) -> Result<EditSession, EditError> {
        let requested = normalize(remote_path);
        if let Some(s) = self.reuse(self.find(host_id, &requested), &with).await? {
            return Ok(s);
        }
        // Replace the real file, never the symlink pointing to it.
        let link = remote.lstat(&requested).await?;
        let target = if link.kind == EntryKind::Symlink {
            normalize(&remote.canonicalize(&requested).await?)
        } else {
            requested.clone()
        };
        let key = SessionKey::new(host_id, &target);
        if let Some(s) = self.reuse(self.find_key(&key), &with).await? {
            return Ok(s);
        }
        let mut keys = vec![SessionKey::new(host_id, &requested)];
        if target != requested {
            keys.push(key);
        }
        let _guard = OpeningGuard::acquire(&self.inner.registry, keys.clone(), &requested)?;

        let entry = remote.stat(&target).await?;
        if entry.kind != EntryKind::File {
            return Err(EditError::NotAFile(requested));
        }
        let cfg = &self.inner.cfg;
        if entry.size > cfg.max_file_size {
            return Err(EditError::TooLarge {
                size: entry.size,
                limit: cfg.max_file_size,
            });
        }

        let id = Uuid::new_v4();
        self.emit(id, EditStatus::Opening);
        prepare_root(&cfg.root, cfg.exclude_from_backup)
            .map_err(|e| EditError::local(&cfg.root, e))?;
        let dir = cfg.root.join(id.to_string());
        create_session_dir(&dir).map_err(|e| EditError::local(&dir, e))?;
        let mut working_copy = PendingWorkingCopy {
            dir: dir.clone(),
            overwrite_limit: cfg.secure_overwrite_limit,
            committed: false,
        };
        let result = match std::fs::canonicalize(&dir) {
            Ok(canonical) => {
                self.open_in(
                    canonical, id, remote, host_id, &requested, &target, keys, with,
                )
                .await
            }
            Err(e) => Err(EditError::local(&dir, e)),
        };
        if result.is_err() {
            self.emit(id, EditStatus::Closed);
        } else {
            working_copy.committed = true;
        }
        result
    }

    #[allow(clippy::too_many_arguments)]
    async fn open_in(
        &self,
        dir: PathBuf,
        id: Uuid,
        remote: Arc<dyn EditRemote>,
        host_id: &str,
        requested: &str,
        target: &str,
        keys: Vec<SessionKey>,
        with: OpenWith,
    ) -> Result<EditSession, EditError> {
        let cfg = self.inner.cfg.clone();
        let file_name = sanitize_file_name(remote_base_name(requested));
        let work = dir.join(&file_name);

        // Download; re-stat afterwards so the base version describes exactly
        // the downloaded content (retry if the file changed meanwhile).
        let mut before = remote.stat(target).await?;
        let mut attempts = 0;
        let (entry, data) = loop {
            let data = remote
                .read_file(target, cfg.max_file_size, &mut |_| {})
                .await?;
            let after = remote.stat(target).await?;
            let stable = after.size == data.len() as u64
                && before.size == after.size
                && before.modified == after.modified;
            attempts += 1;
            if stable || attempts >= 3 {
                break (after, data);
            }
            before = after;
        };
        let hash = sha256(&data);
        write_private_file(&work, &data, true).map_err(|e| EditError::local(&work, e))?;
        drop(data);
        let base = BaseVersion::new(&entry, hash);
        Manifest::new(id, host_id, requested, target, &file_name, base.clone()).save(&dir)?;

        let (mut actor, session, rx) = Actor::new(
            NewSession {
                id,
                host_id: host_id.to_string(),
                remote_path: requested.to_string(),
                target: target.to_string(),
                dir,
                file_name,
                base,
                created_at: Utc::now(),
                open_with: with.clone(),
                shutdown: self.inner.shutdown.subscribe(),
            },
            keys,
            Arc::downgrade(&self.inner.registry),
            remote,
            self.inner.opener.clone(),
            cfg,
            self.inner.events.clone(),
        );
        actor.start_watcher()?;
        tracing::debug!(
            session = %id,
            remote_path = target,
            local = %work.display(),
            "edit session opening editor"
        );
        let outcome = open_in_editor(self.inner.opener.clone(), work, with.clone()).await?;
        if outcome == ChooseOutcome::Cancelled {
            return Err(EditError::Cancelled);
        }
        actor.remember_choice(with, &outcome);
        self.register(&session)?;
        session_set_synced(&session);
        tokio::spawn(actor.run(rx));
        Ok(session)
    }

    /// Stop registered sessions, allowing later opens. Vault lock / app quit
    /// must use [`Self::shutdown`] to cancel pending editor UI too.
    pub async fn stop_all(&self, mode: StopMode) -> Vec<(Uuid, Result<StopOutcome, EditError>)> {
        let sessions: Vec<_> = lock(&self.inner.registry).by_id.values().cloned().collect();
        let mut out = Vec::with_capacity(sessions.len());
        for s in sessions {
            out.push((s.id(), s.stop(mode).await));
        }
        out
    }

    /// Permanently cancel pending opens/resumes before stopping registered
    /// sessions. A fresh unlocked vault constructs a fresh manager.
    pub fn begin_shutdown(&self) {
        lock(&self.inner.registry).closed = true;
        self.inner.shutdown.send_replace(true);
    }

    pub async fn shutdown(&self, mode: StopMode) -> Vec<(Uuid, Result<StopOutcome, EditError>)> {
        self.begin_shutdown();
        self.stop_all(mode).await
    }

    /// Stop the sessions of one host (before / after disconnecting it).
    pub async fn stop_host(
        &self,
        host_id: &str,
        mode: StopMode,
    ) -> Vec<(Uuid, Result<StopOutcome, EditError>)> {
        let sessions: Vec<_> = lock(&self.inner.registry)
            .by_id
            .values()
            .filter(|s| s.host_id() == host_id)
            .cloned()
            .collect();
        let mut out = Vec::with_capacity(sessions.len());
        for s in sessions {
            out.push((s.id(), s.stop(mode).await));
        }
        out
    }

    /// Edit directories left behind by earlier runs (not active sessions).
    pub fn leftovers(&self) -> Result<Vec<Leftover>, EditError> {
        let active: Vec<Uuid> = lock(&self.inner.registry).by_id.keys().copied().collect();
        Ok(list_leftovers(&self.inner.cfg.root)?
            .into_iter()
            .filter(|l| !active.contains(&l.session_id))
            .collect())
    }

    /// Securely delete one leftover.
    pub fn discard_leftover(&self, session_id: Uuid) -> Result<(), EditError> {
        if lock(&self.inner.registry).by_id.contains_key(&session_id) {
            return Err(EditError::InvalidState("session is active"));
        }
        remove_leftover(
            &self.inner.cfg.root,
            session_id,
            self.inner.cfg.secure_overwrite_limit,
        )
    }

    /// Securely delete every leftover; returns how many were removed.
    pub fn discard_all_leftovers(&self) -> Result<usize, EditError> {
        let mut n = 0;
        for l in self.leftovers()? {
            self.discard_leftover(l.session_id)?;
            n += 1;
        }
        Ok(n)
    }

    /// Continue a leftover session: its working copy is checked right away
    /// and uploaded if it differs from the recorded base version (conflict
    /// check included). `with`: also open it in an editor.
    pub async fn resume(
        &self,
        remote: Arc<dyn EditRemote>,
        session_id: Uuid,
        with: Option<OpenWith>,
    ) -> Result<EditSession, EditError> {
        let mut shutdown = self.inner.shutdown.subscribe();
        if *shutdown.borrow() {
            return Err(EditError::Closed);
        }
        tokio::select! {
            biased;
            _ = shutdown.changed() => Err(EditError::Closed),
            result = self.resume_active(remote, session_id, with) => result,
        }
    }

    async fn resume_active(
        &self,
        remote: Arc<dyn EditRemote>,
        session_id: Uuid,
        with: Option<OpenWith>,
    ) -> Result<EditSession, EditError> {
        if lock(&self.inner.registry).by_id.contains_key(&session_id) {
            return Err(EditError::InvalidState("session is active"));
        }
        let cfg = self.inner.cfg.clone();
        let dir = cfg.root.join(session_id.to_string());
        let m = Manifest::load(&dir).ok_or(EditError::UnknownSession)?;
        if m.session_id != session_id || !dir.join(&m.file_name).is_file() {
            return Err(EditError::UnknownSession);
        }
        let mut keys = vec![SessionKey::new(&m.host_id, &m.remote_path)];
        if m.target_path != m.remote_path {
            keys.push(SessionKey::new(&m.host_id, &m.target_path));
        }
        if keys.iter().any(|k| self.find_key(k).is_some()) {
            return Err(EditError::Busy(m.remote_path));
        }
        let _guard = OpeningGuard::acquire(&self.inner.registry, keys.clone(), &m.remote_path)?;
        let dir = std::fs::canonicalize(&dir).map_err(|e| EditError::local(&dir, e))?;
        let open_with = with.clone().unwrap_or_default();
        let (mut actor, session, rx) = Actor::new(
            NewSession {
                id: session_id,
                host_id: m.host_id,
                remote_path: m.remote_path,
                target: m.target_path,
                dir,
                file_name: m.file_name,
                base: m.base,
                created_at: m.created_at,
                open_with: open_with.clone(),
                shutdown: self.inner.shutdown.subscribe(),
            },
            keys,
            Arc::downgrade(&self.inner.registry),
            remote,
            self.inner.opener.clone(),
            cfg,
            self.inner.events.clone(),
        );
        actor.start_watcher()?;
        if let Some(with) = with {
            let work = actor.work_path().to_path_buf();
            let outcome = open_in_editor(self.inner.opener.clone(), work, with.clone()).await?;
            if outcome == ChooseOutcome::Cancelled {
                return Err(EditError::Cancelled);
            }
            actor.remember_choice(with, &outcome);
        }
        self.register(&session)?;
        session_set_synced(&session);
        actor.request_check();
        tokio::spawn(actor.run(rx));
        Ok(session)
    }

    /// Root directory of the working copies.
    pub fn root(&self) -> &Path {
        &self.inner.cfg.root
    }
}

fn session_set_synced(session: &EditSession) {
    session.set_status(EditStatus::Synced);
}
