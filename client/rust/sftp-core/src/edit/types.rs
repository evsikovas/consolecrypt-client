//! Public value types of edit sessions.

use crate::{RemoteEntry, SftpError, TransferProgress};
use cc_platform_core::ProfilePaths;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::time::Duration;
use uuid::Uuid;

/// Default maximum size of a file opened for editing (50 MiB).
pub const DEFAULT_MAX_FILE_SIZE: u64 = 50 * 1024 * 1024;
/// Default debounce of file-system events.
pub const DEFAULT_DEBOUNCE: Duration = Duration::from_millis(500);
/// Files up to this size are overwritten with zeros before deletion.
pub const DEFAULT_SECURE_OVERWRITE_LIMIT: u64 = 64 * 1024 * 1024;

/// `<profile cache>/edit` — the edit root of a profile.
pub fn edit_root(profile: &ProfilePaths) -> PathBuf {
    profile.cache_dir().join("edit")
}

/// Engine settings.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EditConfig {
    /// Edit root; sessions live in `<root>/<session-uuid>/<file name>`.
    pub root: PathBuf,
    /// Larger remote files are refused; larger local copies are not uploaded.
    pub max_file_size: u64,
    /// Quiet period after the last file-system event before a save is
    /// processed (coalesces editors' multi-step saves).
    pub debounce: Duration,
    /// Additionally compare the working copy's size / mtime at this interval
    /// (safety net for missed file-system events); `None` = events only.
    pub poll_interval: Option<Duration>,
    /// Automatic retries of a failed upload (transient errors only) before
    /// the session stays in `Error` waiting for a manual retry.
    pub auto_retries: u32,
    /// Delay before the first automatic retry (doubles each attempt).
    pub retry_backoff: Duration,
    /// Write the remote file in place (truncate + write, not atomic) when the
    /// atomic replace is impossible (directory not writable) or would change
    /// the file's owner / group.
    pub in_place_fallback: bool,
    /// Files up to this size are overwritten with zeros before deletion
    /// (best effort; copy-on-write file systems / SSDs may keep old blocks).
    pub secure_overwrite_limit: u64,
    /// macOS: exclude the edit root from Time Machine (`tmutil addexclusion`,
    /// best effort).
    pub exclude_from_backup: bool,
}

impl EditConfig {
    /// Defaults with an explicit edit root.
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self {
            root: root.into(),
            max_file_size: DEFAULT_MAX_FILE_SIZE,
            debounce: DEFAULT_DEBOUNCE,
            poll_interval: Some(Duration::from_secs(3)),
            auto_retries: 3,
            retry_backoff: Duration::from_secs(2),
            in_place_fallback: true,
            secure_overwrite_limit: DEFAULT_SECURE_OVERWRITE_LIMIT,
            exclude_from_backup: true,
        }
    }

    /// Defaults for a profile (`<profile cache>/edit`).
    pub fn for_profile(profile: &ProfilePaths) -> Self {
        Self::new(edit_root(profile))
    }
}

/// Remote metadata shown with a conflict.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RemoteMeta {
    pub size: u64,
    pub modified: Option<DateTime<Utc>>,
    pub permissions: Option<u32>,
}

impl From<&RemoteEntry> for RemoteMeta {
    fn from(e: &RemoteEntry) -> Self {
        Self {
            size: e.size,
            modified: e.modified,
            permissions: e.permissions,
        }
    }
}

/// State of an edit session (UI: status bar / "Editing" list).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
#[non_exhaustive]
pub enum EditStatus {
    /// Downloading and opening the editor.
    Opening,
    /// The remote file equals the local working copy.
    Synced,
    /// The working copy has changes that are not uploaded yet and will be
    /// uploaded on the next save or on stop (after "keep remote copy").
    Modified,
    /// Uploading a save.
    Uploading { progress: TransferProgress },
    /// The remote file changed since it was downloaded / last uploaded
    /// (`None`: it was deleted). Nothing is uploaded until resolved.
    Conflict { remote_meta: Option<RemoteMeta> },
    /// The last upload failed; the local copy is kept. `retryable`: a Retry
    /// action makes sense (automatic retries may already be scheduled).
    Error { message: String, retryable: bool },
    /// Session ended.
    Closed,
}

/// Status change of one session (manager-wide stream).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct EditEvent {
    pub session_id: Uuid,
    pub status: EditStatus,
}

/// How to resolve [`EditStatus::Conflict`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ConflictResolution {
    /// Upload the local copy over the changed remote file.
    OverwriteRemote,
    /// Download the remote version next to the working copy as
    /// `<stem>.remote-<timestamp>[.<ext>]`, open it for comparison and make it
    /// the new base; the next save of the working copy uploads normally.
    KeepRemoteCopyLocally,
    /// Replace the working copy with the remote version.
    DiscardLocal,
}

/// How [`EditSession::stop`](super::EditSession::stop) ends a session.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum StopMode {
    /// "Stop editing": upload pending changes, then remove the working
    /// directory. On conflict / upload failure the session stays open and the
    /// outcome says why (ask the user).
    Upload,
    /// Unattended end (vault lock, app quit): like `Upload`, but on conflict /
    /// failure the working directory is kept for recovery on next start.
    UploadOrKeep,
    /// Stop watching, keep the working directory (recoverable leftover).
    KeepFiles,
    /// Remove the working directory without uploading.
    Discard,
}

/// Result of a stop request.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[non_exhaustive]
pub enum StopOutcome {
    /// Session closed and its directory removed.
    Closed { uploaded: bool },
    /// Session closed, directory kept (see [`list_leftovers`](super::list_leftovers)).
    KeptFiles { dir: PathBuf },
    /// Not stopped: the remote changed meanwhile — resolve first.
    Conflict { remote_meta: Option<RemoteMeta> },
    /// Not stopped: the final upload failed.
    UploadFailed { message: String },
}

/// Snapshot of a session for the UI.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct EditSessionInfo {
    pub id: Uuid,
    /// Caller-defined host identity (e.g. the host object id).
    pub host_id: String,
    /// Remote path as requested.
    pub remote_path: String,
    /// Remote path that is actually replaced (symlinks resolved).
    pub target_path: String,
    /// Local working copy.
    pub local_path: PathBuf,
    pub status: EditStatus,
    pub opened_at: DateTime<Utc>,
    pub last_synced_at: Option<DateTime<Utc>>,
    /// Successful uploads so far.
    pub uploads: u64,
    /// Remote copies saved by [`ConflictResolution::KeepRemoteCopyLocally`].
    pub remote_copies: Vec<PathBuf>,
    /// The application reopens use when one was chosen ("Open With…" /
    /// [`OpenWith::App`](super::OpenWith::App)); `None` = the OS default.
    #[serde(skip)]
    pub app: Option<cc_platform_core::AppRef>,
}

/// Errors of the edit engine.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum EditError {
    #[error("file too large to edit: {size} bytes (limit {limit})")]
    TooLarge { size: u64, limit: u64 },
    #[error("not a regular file: {0}")]
    NotAFile(String),
    #[error(transparent)]
    Sftp(#[from] SftpError),
    #[error("local I/O error on {path}: {source}")]
    Local {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("could not open the file: {0}")]
    Opener(String),
    #[error("file watcher failed: {0}")]
    Watch(String),
    #[error("upload failed: {0}")]
    Upload(String),
    #[error("cancelled")]
    Cancelled,
    #[error("already being opened: {0}")]
    Busy(String),
    #[error("no such edit session or leftover")]
    UnknownSession,
    #[error("edit session is closed")]
    Closed,
    #[error("invalid in the current state: {0}")]
    InvalidState(&'static str),
}

impl EditError {
    pub(crate) fn local(path: &Path, source: std::io::Error) -> Self {
        Self::Local {
            path: path.to_path_buf(),
            source,
        }
    }
}
