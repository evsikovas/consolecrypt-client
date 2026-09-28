//! Debounced watching of a session directory.
//!
//! The *directory* is watched (not the file) because editors often save via
//! "write temp file + rename over" or "delete + recreate", which replaces the
//! inode a file watch would be attached to. Events are matched by file name;
//! editor artefacts and other files in the directory are ignored.

use notify_debouncer_full::notify::event::{AccessKind, AccessMode};
use notify_debouncer_full::notify::{Event, EventKind, RecommendedWatcher, RecursiveMode};
use notify_debouncer_full::{new_debouncer, DebounceEventResult, Debouncer, RecommendedCache};
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Notify;

/// Editor artefacts that never trigger an upload: vim swap files
/// (`.swp`/`.swo`/`.swx`) and its `4913` write probe, `~` backups, Emacs
/// lock files `.#*`, Finder's `.DS_Store`.
pub fn is_editor_artifact(name: &str) -> bool {
    name.ends_with(".swp")
        || name.ends_with(".swo")
        || name.ends_with(".swx")
        || name.ends_with('~')
        || name.starts_with(".#")
        || name == "4913"
        || name == ".DS_Store"
}

/// Does `event` (in the watched directory `dir`) possibly change the working
/// copy `name`? The working copy itself always counts, even if its name looks
/// like an artefact.
pub(crate) fn is_relevant(event: &Event, dir: &Path, name: &str) -> bool {
    if event.need_rescan() {
        return true;
    }
    match event.kind {
        EventKind::Access(AccessKind::Close(AccessMode::Write)) => {}
        EventKind::Access(_) => return false,
        _ => {}
    }
    if event.paths.is_empty() {
        return true;
    }
    event.paths.iter().any(|p| {
        match p.file_name().and_then(|n| n.to_str()) {
            Some(n) if n == name => true,
            Some(n) if is_editor_artifact(n) => false,
            // The directory itself (coalesced / rescan-style events).
            _ => p == dir,
        }
    })
}

/// Running watcher; dropping it stops watching.
pub(crate) struct DirWatcher {
    _debouncer: Debouncer<RecommendedWatcher, RecommendedCache>,
}

impl std::fmt::Debug for DirWatcher {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DirWatcher").finish_non_exhaustive()
    }
}

impl DirWatcher {
    /// Watch `dir` (canonical path) and signal `changed` after `debounce` of
    /// quiet following relevant events for `name`.
    pub(crate) fn start(
        dir: &Path,
        name: &str,
        debounce: Duration,
        changed: Arc<Notify>,
    ) -> Result<Self, String> {
        let watched: PathBuf = dir.to_path_buf();
        let name = OsString::from(name).to_string_lossy().into_owned();
        let mut debouncer =
            new_debouncer(debounce, None, move |res: DebounceEventResult| match res {
                Ok(events) => {
                    if events
                        .iter()
                        .any(|e| is_relevant(&e.event, &watched, &name))
                    {
                        changed.notify_one();
                    }
                }
                Err(errors) => {
                    tracing::debug!(count = errors.len(), "edit watcher reported errors");
                    changed.notify_one();
                }
            })
            .map_err(|e| e.to_string())?;
        debouncer
            .watch(dir, RecursiveMode::NonRecursive)
            .map_err(|e| e.to_string())?;
        Ok(Self {
            _debouncer: debouncer,
        })
    }
}
