//! "Edit in the default app" sessions (ADR-0108, SFTP_BROWSER_SPEC §2) over
//! sftp-core's [`EditManager`]: one manager per unlocked vault
//! (`<profile cache>/edit`), sessions bound to the SFTP session they were
//! opened through, status changes as [`AppEvent::EditStatus`].
//!
//! Lifecycle: closing an SFTP session stops its edit sessions with
//! `UploadOrKeep`; vault lock / shutdown stops all of them with
//! `UploadOrKeep` *before* SSH is torn down (conflicts / failures keep the
//! working copy as a leftover); after unlock, leftovers of earlier runs are
//! announced once with [`AppEvent::EditLeftovers`].

use crate::app::AppCore;
use crate::dto::{opt_ms, AppEvent};
use crate::error::{AppError, AppResult};
use crate::session::AppCtx;
use cc_platform_core::{AppRef, FileOpener};
use cc_sftp_core::edit::{
    ConflictResolution, EditConfig, EditEvent, EditManager, EditRemote, EditSessionInfo,
    EditStatus, Leftover, OpenWith, RemoteMeta, StopMode, StopOutcome,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use tokio::sync::broadcast;
use uuid::Uuid;

// ---- DTOs ------------------------------------------------------------------------

/// A specific application (`platform_core::AppRef`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AppRefDto {
    /// `path` (bundle / executable) | `name` (macOS) | `bundle_id` (macOS).
    pub kind: String,
    pub value: String,
}

impl AppRefDto {
    fn from_ref(a: &AppRef) -> Self {
        let (kind, value) = match a {
            AppRef::Path(p) => ("path", p.to_string_lossy().into_owned()),
            AppRef::Name(n) => ("name", n.clone()),
            AppRef::BundleId(b) => ("bundle_id", b.clone()),
        };
        Self {
            kind: kind.into(),
            value,
        }
    }

    fn to_ref(&self) -> AppResult<AppRef> {
        let v = self.value.trim();
        if v.is_empty() || v.chars().any(char::is_control) {
            return Err(AppError::invalid("app", "must not be empty"));
        }
        Ok(match self.kind.as_str() {
            "path" => AppRef::Path(PathBuf::from(v)),
            "name" => AppRef::Name(v.to_owned()),
            "bundle_id" => AppRef::BundleId(v.to_owned()),
            _ => return Err(AppError::invalid("app", "unknown application kind")),
        })
    }
}

/// Which application opens a working copy.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum OpenWithDto {
    /// The OS default application ("Open").
    #[default]
    Default,
    /// A specific application.
    App { app: AppRefDto },
    /// The platform chooser ("Open With…"); the pick is reused by reopens.
    Choose,
}

impl OpenWithDto {
    pub(crate) fn to_open_with(&self) -> AppResult<OpenWith> {
        Ok(match self {
            OpenWithDto::Default => OpenWith::Default,
            OpenWithDto::App { app } => OpenWith::App(app.to_ref()?),
            OpenWithDto::Choose => OpenWith::Choose,
        })
    }
}

/// Remote metadata shown with a conflict.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RemoteMetaDto {
    pub size: u64,
    pub modified_at_ms: Option<i64>,
    pub permissions: Option<u32>,
}

impl From<&RemoteMeta> for RemoteMetaDto {
    fn from(m: &RemoteMeta) -> Self {
        Self {
            size: m.size,
            modified_at_ms: opt_ms(&m.modified),
            permissions: m.permissions,
        }
    }
}

/// State of an edit session (`state` tag, snake_case).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum EditStatusDto {
    Opening,
    Synced,
    Modified,
    Uploading {
        transferred: u64,
        total: Option<u64>,
    },
    /// `remote` = `None`: the remote file was deleted.
    Conflict {
        remote: Option<RemoteMetaDto>,
    },
    Error {
        message: String,
        retryable: bool,
    },
    Closed,
}

impl From<&EditStatus> for EditStatusDto {
    fn from(s: &EditStatus) -> Self {
        match s {
            EditStatus::Opening => Self::Opening,
            EditStatus::Synced => Self::Synced,
            EditStatus::Modified => Self::Modified,
            EditStatus::Uploading { progress } => Self::Uploading {
                transferred: progress.transferred,
                total: progress.total,
            },
            EditStatus::Conflict { remote_meta } => Self::Conflict {
                remote: remote_meta.as_ref().map(Into::into),
            },
            EditStatus::Error { message, retryable } => Self::Error {
                message: message.clone(),
                retryable: *retryable,
            },
            EditStatus::Closed => Self::Closed,
            // `EditStatus` is non-exhaustive: unknown future states read as
            // "synced" until the facade learns them.
            #[allow(unreachable_patterns)]
            _ => Self::Synced,
        }
    }
}

/// Snapshot of an edit session.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EditSessionDto {
    pub id: String,
    pub host_id: String,
    /// The SFTP session it uploads through (`None` once that closed).
    pub sftp_id: Option<String>,
    pub remote_path: String,
    /// Remote path actually replaced (symlinks resolved).
    pub target_path: String,
    /// Private local working copy.
    pub local_path: String,
    pub status: EditStatusDto,
    pub opened_at_ms: i64,
    pub last_synced_at_ms: Option<i64>,
    pub uploads: u64,
    pub remote_copies: Vec<String>,
    /// Application chosen with "Open With…" (reused by reopen).
    pub app: Option<AppRefDto>,
}

impl EditSessionDto {
    fn from_info(i: &EditSessionInfo, sftp_id: Option<String>) -> Self {
        Self {
            id: i.id.to_string(),
            host_id: i.host_id.clone(),
            sftp_id,
            remote_path: i.remote_path.clone(),
            target_path: i.target_path.clone(),
            local_path: i.local_path.to_string_lossy().into_owned(),
            status: (&i.status).into(),
            opened_at_ms: i.opened_at.timestamp_millis(),
            last_synced_at_ms: opt_ms(&i.last_synced_at),
            uploads: i.uploads,
            remote_copies: i
                .remote_copies
                .iter()
                .map(|p| p.to_string_lossy().into_owned())
                .collect(),
            app: i.app.as_ref().map(AppRefDto::from_ref),
        }
    }
}

/// How to resolve a conflict.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EditConflictResolutionDto {
    OverwriteRemote,
    KeepRemoteCopyLocally,
    DiscardLocal,
}

impl From<EditConflictResolutionDto> for ConflictResolution {
    fn from(r: EditConflictResolutionDto) -> Self {
        match r {
            EditConflictResolutionDto::OverwriteRemote => ConflictResolution::OverwriteRemote,
            EditConflictResolutionDto::KeepRemoteCopyLocally => {
                ConflictResolution::KeepRemoteCopyLocally
            }
            EditConflictResolutionDto::DiscardLocal => ConflictResolution::DiscardLocal,
        }
    }
}

/// How a session ends.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EditStopModeDto {
    Upload,
    UploadOrKeep,
    KeepFiles,
    Discard,
}

impl From<EditStopModeDto> for StopMode {
    fn from(m: EditStopModeDto) -> Self {
        match m {
            EditStopModeDto::Upload => StopMode::Upload,
            EditStopModeDto::UploadOrKeep => StopMode::UploadOrKeep,
            EditStopModeDto::KeepFiles => StopMode::KeepFiles,
            EditStopModeDto::Discard => StopMode::Discard,
        }
    }
}

/// Result of a stop request (`outcome` tag).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum EditStopOutcomeDto {
    /// Closed, working copy removed.
    Closed { uploaded: bool },
    /// Closed, working copy kept (a leftover next time).
    KeptFiles { directory: String },
    /// Not stopped: the remote changed — resolve first.
    Conflict { remote: Option<RemoteMetaDto> },
    /// Not stopped: the final upload failed.
    UploadFailed { message: String },
}

impl From<&StopOutcome> for EditStopOutcomeDto {
    fn from(o: &StopOutcome) -> Self {
        match o {
            StopOutcome::Closed { uploaded } => Self::Closed {
                uploaded: *uploaded,
            },
            StopOutcome::KeptFiles { dir } => Self::KeptFiles {
                directory: dir.to_string_lossy().into_owned(),
            },
            StopOutcome::Conflict { remote_meta } => Self::Conflict {
                remote: remote_meta.as_ref().map(Into::into),
            },
            StopOutcome::UploadFailed { message } => Self::UploadFailed {
                message: message.clone(),
            },
            #[allow(unreachable_patterns)]
            _ => Self::Closed { uploaded: false },
        }
    }
}

/// A working directory left by a crash / quit / disconnect.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EditLeftoverDto {
    pub id: String,
    pub host_id: Option<String>,
    pub remote_path: Option<String>,
    pub target_path: Option<String>,
    pub created_at_ms: Option<i64>,
    pub working_file: Option<String>,
    /// The working copy differs from the last downloaded / uploaded version.
    pub locally_modified: Option<bool>,
}

impl From<&Leftover> for EditLeftoverDto {
    fn from(l: &Leftover) -> Self {
        Self {
            id: l.session_id.to_string(),
            host_id: l.host_id.clone(),
            remote_path: l.remote_path.clone(),
            target_path: l.target_path.clone(),
            created_at_ms: opt_ms(&l.created_at),
            working_file: l
                .working_file
                .as_ref()
                .map(|p| p.to_string_lossy().into_owned()),
            locally_modified: l.locally_modified,
        }
    }
}

// ---- runtime ---------------------------------------------------------------------

/// Edit sessions of one unlocked vault.
pub(crate) struct EditRuntime {
    manager: Result<EditManager, String>,
    opener: FileOpener,
    /// Edit session → SFTP session it was opened / resumed through.
    owners: Mutex<HashMap<Uuid, String>>,
}

impl std::fmt::Debug for EditRuntime {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EditRuntime").finish_non_exhaustive()
    }
}

fn parse_session(id: &str) -> AppResult<Uuid> {
    Uuid::parse_str(id.trim()).map_err(|_| AppError::invalid("edit_session_id", "not a valid id"))
}

impl EditRuntime {
    /// Create the manager for `root` (`<profile cache>/edit`).
    pub(crate) fn new(ctx: &AppCtx, root: Option<PathBuf>) -> Arc<Self> {
        let opener = ctx.file_opener();
        let manager = match root {
            None => Err("no profile directory".to_owned()),
            Some(root) => {
                let mut cfg = EditConfig::new(root);
                cfg.exclude_from_backup = ctx.config.edit_exclude_from_backup;
                EditManager::new(cfg, Arc::new(opener.clone())).map_err(|e| e.to_string())
            }
        };
        if let Err(e) = &manager {
            tracing::warn!(error = %e, "edit sessions unavailable");
        }
        Arc::new(Self {
            manager,
            opener,
            owners: Mutex::new(HashMap::new()),
        })
    }

    pub(crate) fn manager(&self) -> AppResult<&EditManager> {
        self.manager
            .as_ref()
            .map_err(|e| AppError::Io(format!("edit sessions unavailable: {e}")))
    }

    fn owners(&self) -> std::sync::MutexGuard<'_, HashMap<Uuid, String>> {
        self.owners.lock().unwrap_or_else(|p| p.into_inner())
    }

    fn dto(&self, info: &EditSessionInfo) -> EditSessionDto {
        let owner = self.owners().get(&info.id).cloned();
        EditSessionDto::from_info(info, owner)
    }

    /// Forward the manager's status changes as app events.
    pub(crate) fn spawn_forwarder(
        self: &Arc<Self>,
        ctx: Arc<AppCtx>,
    ) -> Option<tokio::task::JoinHandle<()>> {
        let mut rx = self.manager.as_ref().ok()?.subscribe();
        let me = self.clone();
        Some(tokio::spawn(async move {
            loop {
                match rx.recv().await {
                    Ok(EditEvent { session_id, status }) => {
                        if status == EditStatus::Closed {
                            me.owners().remove(&session_id);
                        }
                        ctx.emit(AppEvent::EditStatus {
                            session_id: session_id.to_string(),
                            status: (&status).into(),
                        });
                    }
                    Err(broadcast::error::RecvError::Lagged(_)) => {
                        // The UI re-reads the list on any event.
                        ctx.emit(AppEvent::EditStatus {
                            session_id: String::new(),
                            status: EditStatusDto::Synced,
                        });
                    }
                    Err(broadcast::error::RecvError::Closed) => break,
                }
            }
        }))
    }

    /// Stop the sessions opened through `sftp_id` (it is being closed).
    pub(crate) async fn stop_for_sftp(&self, sftp_id: &str) {
        let Ok(m) = self.manager() else { return };
        let ids: Vec<Uuid> = self
            .owners()
            .iter()
            .filter(|(_, s)| s.as_str() == sftp_id)
            .map(|(id, _)| *id)
            .collect();
        for id in ids {
            if let Some(s) = m.session(id) {
                match s.stop(StopMode::UploadOrKeep).await {
                    Ok(o) => {
                        tracing::info!(session = %id, outcome = ?EditStopOutcomeDto::from(&o), "edit session stopped (sftp closed)")
                    }
                    Err(e) => tracing::warn!(session = %id, error = %e, "edit session stop failed"),
                }
            }
            self.owners().remove(&id);
        }
    }

    /// Stop every session (lock / shutdown), keeping files on failure.
    pub(crate) async fn stop_all(&self) {
        let Ok(m) = self.manager() else { return };
        for (id, r) in m.shutdown(StopMode::UploadOrKeep).await {
            match r {
                Ok(o) => {
                    tracing::info!(session = %id, outcome = ?EditStopOutcomeDto::from(&o), "edit session stopped (lock)")
                }
                Err(e) => tracing::warn!(session = %id, error = %e, "edit session stop failed"),
            }
        }
        self.owners().clear();
    }

    pub(crate) fn begin_shutdown(&self) {
        if let Ok(m) = self.manager() {
            m.begin_shutdown();
        }
    }

    /// Number of leftovers (announced after unlock).
    pub(crate) fn leftover_count(&self) -> usize {
        self.manager()
            .ok()
            .and_then(|m| m.leftovers().ok())
            .map(|l| l.len())
            .unwrap_or(0)
    }
}

// ---- facade ----------------------------------------------------------------------

impl AppCore {
    async fn edit_runtime(&self) -> AppResult<Arc<EditRuntime>> {
        let (_, u) = self.unlocked().await?;
        Ok(u.edit.clone())
    }

    /// Download `remote_path` into a private working directory, open it with
    /// `open_with` and upload every save (conflict check + atomic replace).
    /// A file already being edited returns the existing session (opened
    /// again). The session ends when `sftp_id` is closed.
    pub async fn edit_open(
        &self,
        sftp_id: String,
        remote_path: String,
        open_with: OpenWithDto,
    ) -> AppResult<EditSessionDto> {
        let (_, u) = self.unlocked().await?;
        let client = u.ssh.sftp(&sftp_id).await?;
        let host_id = u.ssh.sftp_host(&sftp_id).await?;
        let path = crate::sftp_browser::remote_path_arg("remote_path", &remote_path)?;
        let with = open_with.to_open_with()?;
        let remote: Arc<dyn EditRemote> = client;
        let session = u
            .edit
            .manager()?
            .open(remote, &host_id.to_string(), &path, with)
            .await?;
        u.edit.owners().insert(session.id(), sftp_id);
        Ok(u.edit.dto(&session.info()))
    }

    /// Active edit sessions of the profile (every host), oldest first.
    pub async fn edit_sessions(&self) -> AppResult<Vec<EditSessionDto>> {
        let e = self.edit_runtime().await?;
        let Ok(m) = e.manager() else {
            return Ok(Vec::new());
        };
        Ok(m.sessions().iter().map(|i| e.dto(i)).collect())
    }

    fn edit_session_of(e: &EditRuntime, id: &str) -> AppResult<cc_sftp_core::edit::EditSession> {
        let uuid = parse_session(id)?;
        e.manager()?
            .session(uuid)
            .ok_or_else(|| AppError::not_found("edit session", id))
    }

    /// "Sync now" / "Retry": check the working copy, upload if changed.
    pub async fn edit_sync_now(&self, session_id: String) -> AppResult<EditStatusDto> {
        let e = self.edit_runtime().await?;
        let s = Self::edit_session_of(&e, &session_id)?;
        Ok((&s.sync_now().await?).into())
    }

    /// Open the working copy again (`None` = the session's application).
    pub async fn edit_reopen(
        &self,
        session_id: String,
        open_with: Option<OpenWithDto>,
    ) -> AppResult<()> {
        let e = self.edit_runtime().await?;
        let s = Self::edit_session_of(&e, &session_id)?;
        let with = open_with.map(|w| w.to_open_with()).transpose()?;
        Ok(s.reopen(with).await?)
    }

    /// Show the working copy in Finder / Explorer.
    pub async fn edit_reveal(&self, session_id: String) -> AppResult<()> {
        let e = self.edit_runtime().await?;
        let s = Self::edit_session_of(&e, &session_id)?;
        let path = s.local_path();
        let opener = e.opener.clone();
        crate::secrets::blocking(move || {
            opener
                .reveal(&path)
                .map_err(|e| AppError::Io(format!("cannot reveal the file: {e}")))
        })
        .await
    }

    /// Resolve a conflict; returns the saved remote copy's path for
    /// `keep_remote_copy_locally`.
    pub async fn edit_resolve(
        &self,
        session_id: String,
        resolution: EditConflictResolutionDto,
    ) -> AppResult<Option<String>> {
        let e = self.edit_runtime().await?;
        let s = Self::edit_session_of(&e, &session_id)?;
        Ok(s.resolve(resolution.into())
            .await?
            .map(|p| p.to_string_lossy().into_owned()))
    }

    /// End a session (see [`EditStopModeDto`]).
    pub async fn edit_stop(
        &self,
        session_id: String,
        mode: EditStopModeDto,
    ) -> AppResult<EditStopOutcomeDto> {
        let e = self.edit_runtime().await?;
        let s = Self::edit_session_of(&e, &session_id)?;
        let outcome = s.stop(mode.into()).await?;
        if matches!(
            outcome,
            StopOutcome::Closed { .. } | StopOutcome::KeptFiles { .. }
        ) {
            e.owners().remove(&s.id());
        }
        Ok((&outcome).into())
    }

    /// Working directories left by earlier runs (not active sessions).
    pub async fn edit_leftovers(&self) -> AppResult<Vec<EditLeftoverDto>> {
        let e = self.edit_runtime().await?;
        let Ok(m) = e.manager() else {
            return Ok(Vec::new());
        };
        Ok(m.leftovers()?.iter().map(Into::into).collect())
    }

    /// Continue a leftover through `sftp_id` (connected to its host): the
    /// working copy is checked and uploaded if it differs from its base
    /// (conflict check included); `open_with` also opens it.
    pub async fn edit_resume(
        &self,
        session_id: String,
        sftp_id: String,
        open_with: Option<OpenWithDto>,
    ) -> AppResult<EditSessionDto> {
        let (_, u) = self.unlocked().await?;
        let uuid = parse_session(&session_id)?;
        let client = u.ssh.sftp(&sftp_id).await?;
        let host_id = u.ssh.sftp_host(&sftp_id).await?.to_string();
        let m = u.edit.manager()?;
        let leftover = m
            .leftovers()?
            .into_iter()
            .find(|l| l.session_id == uuid)
            .ok_or_else(|| AppError::not_found("edit session", &session_id))?;
        if leftover.host_id.as_deref() != Some(host_id.as_str()) {
            return Err(AppError::invalid(
                "sftp_id",
                "connected to another host than the leftover's",
            ));
        }
        let with = open_with.map(|w| w.to_open_with()).transpose()?;
        let remote: Arc<dyn EditRemote> = client;
        let session = m.resume(remote, uuid, with).await?;
        u.edit.owners().insert(session.id(), sftp_id);
        Ok(u.edit.dto(&session.info()))
    }

    /// Securely remove one leftover.
    pub async fn edit_discard_leftover(&self, session_id: String) -> AppResult<()> {
        let e = self.edit_runtime().await?;
        let uuid = parse_session(&session_id)?;
        Ok(e.manager()?.discard_leftover(uuid)?)
    }

    /// Rust-only: replace the opener used for new edit managers and reveal
    /// (tests inject a recording launcher so no application is started).
    pub fn set_file_opener(&self, opener: FileOpener) {
        *self
            .inner
            .ctx
            .file_opener
            .write()
            .unwrap_or_else(|p| p.into_inner()) = opener;
    }
}
