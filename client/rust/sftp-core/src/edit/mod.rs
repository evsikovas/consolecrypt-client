//! # Edit remote files in the OS default editor, upload on save
//!
//! Normative behaviour: docs/design/SFTP_BROWSER_SPEC.md §2; design notes:
//! docs/adr/ADR-0108-sftp-edit-sessions.md; security: THREAT_MODEL "Local
//! plaintext by design".
//!
//! Flow of one session ([`EditManager::open`]):
//!
//! 1. stat (symlinks resolved to their target, size limit) and download into
//!    `<profile cache>/edit/<session-uuid>/<file name>` (dir 0700, file 0600,
//!    macOS Spotlight marker in the edit root); the base version is the remote
//!    size + mtime + SHA-256 of the content;
//! 2. open it with an [`EditOpener`] (default app, a chosen app or the
//!    platform chooser — [`cc_platform_core::FileOpener`] in production);
//! 3. watch the directory (debounced; temp-file+rename and delete+recreate
//!    saves work, editor artefacts are ignored, optional polling safety net);
//! 4. on a save whose hash differs: stat the remote — changed size/mtime →
//!    [`EditStatus::Conflict`] (nothing is overwritten; resolve with
//!    [`ConflictResolution`]) — else atomic replace (temp file in the same
//!    directory, original permissions, `posix-rename@openssh.com` or a
//!    documented remove+rename fallback), then the base version moves on;
//!    failures → [`EditStatus::Error`] with automatic + manual retry;
//! 5. [`EditSession::stop`] attempts the final upload and securely removes the
//!    directory; [`list_leftovers`] / [`EditManager::leftovers`] find
//!    directories left after a crash.
//!
//! File contents are never logged; paths are logged at debug level.

mod manager;
mod opener;
mod remote;
mod replace;
mod session;
mod store;
mod types;
mod watch;

#[cfg(test)]
mod tests;

pub use manager::EditManager;
pub use opener::{EditOpener, OpenWith};
pub use remote::{EditRemote, ProgressFn};
pub use session::EditSession;
pub use store::{list_leftovers, remove_leftover, Leftover};
pub use types::{
    edit_root, ConflictResolution, EditConfig, EditError, EditEvent, EditSessionInfo, EditStatus,
    RemoteMeta, StopMode, StopOutcome, DEFAULT_DEBOUNCE, DEFAULT_MAX_FILE_SIZE,
    DEFAULT_SECURE_OVERWRITE_LIMIT,
};
pub use watch::is_editor_artifact;

/// Re-exported for [`OpenWith::App`] / [`EditOpener`] implementations.
pub use cc_platform_core::{AppRef, ChooseOutcome, FileOpener, OpenError};
