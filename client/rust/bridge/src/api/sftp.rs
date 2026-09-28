//! SFTP browser (SFTP_BROWSER_SPEC §1) and "edit in the default app"
//! sessions (ADR-0108) of app-core. Sessions come from `sftp_open`
//! (`api::ssh`); transfer jobs are started with `sftp_transfer`.
//!
//! JSON results: `RemoteFileInfoDto`, `TransferJobDto`, `EditSessionDto`,
//! `EditStatusDto`, `EditStopOutcomeDto`, `EditLeftoverDto`. `open_with_json`
//! parameters take `OpenWithDto` JSON: `{"kind":"default"}`,
//! `{"kind":"choose"}`, `{"kind":"app","app":{"kind":"path","value":"…"}}`.
//! Edit status changes arrive on `core_events` as `edit_status` (re-read
//! `edit_sessions`) and `edit_leftovers` after unlock.

use crate::api::error::BridgeError;
use crate::state::{from_json, to_json, with_core};
use cc_app_core::{EditConflictResolutionDto, EditStopModeDto, OpenWithDto};

/// A bounded prefix of a remote file (Quick Look; never written to disk).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SftpPreview {
    pub path: String,
    pub data: Vec<u8>,
    /// Size of the whole remote file.
    pub total_size: u64,
}

/// How to resolve an edit conflict.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditConflictChoice {
    /// Upload the local copy over the changed remote file.
    OverwriteRemote,
    /// Save the remote version next to the working copy and open it.
    KeepRemoteCopyLocally,
    /// Replace the working copy with the remote version.
    DiscardLocal,
}

/// How an edit session ends.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditStopChoice {
    /// Upload pending changes, then remove the working copy (stays open on
    /// conflict / failure; the outcome says why).
    Upload,
    /// Like `Upload`, but keep the files for recovery instead of asking.
    UploadOrKeep,
    /// Stop watching, keep the working copy (a leftover).
    KeepFiles,
    /// Remove the working copy without uploading.
    Discard,
}

fn open_with(json: Option<String>) -> Result<Option<OpenWithDto>, BridgeError> {
    json.filter(|j| !j.trim().is_empty())
        .map(|j| from_json("open_with", &j))
        .transpose()
}

// ---- browsing ----------------------------------------------------------------------

/// Entries of `path` with full metadata → `Vec<RemoteFileInfoDto>`.
pub async fn sftp_list_detailed(sftp_id: String, path: String) -> Result<String, BridgeError> {
    with_core(move |c| async move { to_json(&c.sftp_list_detailed(sftp_id, path).await?) }).await
}

/// `lstat` of one entry (link target filled) → `RemoteFileInfoDto`.
pub async fn sftp_stat(sftp_id: String, path: String) -> Result<String, BridgeError> {
    with_core(move |c| async move { to_json(&c.sftp_stat(sftp_id, path).await?) }).await
}

/// Canonical absolute directory for a typed path (`~`, relative to `base`,
/// `..`); `not_found` + reason `directory_not_found` if it is not one.
pub async fn sftp_resolve_directory(
    sftp_id: String,
    input: String,
    base: String,
) -> Result<String, BridgeError> {
    with_core(move |c| async move { Ok(c.sftp_resolve_directory(sftp_id, input, base).await?) })
        .await
}

/// `chmod` (`mode & 0o7777`).
pub async fn sftp_chmod(sftp_id: String, path: String, mode: u32) -> Result<(), BridgeError> {
    with_core(move |c| async move { Ok(c.sftp_chmod(sftp_id, path, mode).await?) }).await
}

/// Empty regular file, exclusive (`already_exists` if taken).
pub async fn sftp_create_file(sftp_id: String, path: String) -> Result<(), BridgeError> {
    with_core(move |c| async move { Ok(c.sftp_create_file(sftp_id, path).await?) }).await
}

/// Recursive server-side copy (`already_exists` if `to` exists).
pub async fn sftp_duplicate(sftp_id: String, from: String, to: String) -> Result<(), BridgeError> {
    with_core(move |c| async move { Ok(c.sftp_duplicate(sftp_id, from, to).await?) }).await
}

/// At most `max_bytes` (≤ 8 MiB) from the start of a regular file.
pub async fn sftp_read_preview(
    sftp_id: String,
    path: String,
    max_bytes: u64,
) -> Result<SftpPreview, BridgeError> {
    with_core(move |c| async move {
        let p = c.sftp_read_preview(sftp_id, path, max_bytes).await?;
        Ok(SftpPreview {
            path: p.path,
            data: p.data,
            total_size: p.total_size,
        })
    })
    .await
}

/// All transfer jobs of this unlock (`Vec<TransferJobDto>`, newest first).
pub async fn sftp_transfers() -> Result<String, BridgeError> {
    with_core(move |c| async move {
        match c.sftp_transfers().await {
            Ok(v) => to_json(&v),
            Err(cc_app_core::AppError::VaultLocked)
            | Err(cc_app_core::AppError::NoActiveProfile) => Ok("[]".to_owned()),
            Err(e) => Err(e.into()),
        }
    })
    .await
}

/// Remove completed, failed and cancelled jobs.
pub async fn sftp_clear_finished_transfers() -> Result<(), BridgeError> {
    with_core(move |c| async move {
        match c.sftp_clear_finished_transfers().await {
            Ok(()) | Err(cc_app_core::AppError::VaultLocked) => Ok(()),
            Err(e) => Err(e.into()),
        }
    })
    .await
}

// ---- edit sessions -----------------------------------------------------------------

/// Open a remote file in an external application; saves are uploaded
/// (`EditSessionDto`). `payload_too_large` above 50 MiB, `invalid_input`
/// for anything but a regular file.
pub async fn edit_open(
    sftp_id: String,
    remote_path: String,
    open_with_json: Option<String>,
) -> Result<String, BridgeError> {
    let with = open_with(open_with_json)?.unwrap_or_default();
    with_core(move |c| async move { to_json(&c.edit_open(sftp_id, remote_path, with).await?) })
        .await
}

/// Active edit sessions of the profile → `Vec<EditSessionDto>` (empty
/// while locked).
pub async fn edit_sessions() -> Result<String, BridgeError> {
    with_core(move |c| async move {
        match c.edit_sessions().await {
            Ok(v) => to_json(&v),
            Err(cc_app_core::AppError::VaultLocked)
            | Err(cc_app_core::AppError::NoActiveProfile)
            | Err(cc_app_core::AppError::NoVault) => Ok("[]".to_owned()),
            Err(e) => Err(e.into()),
        }
    })
    .await
}

/// "Sync now" / "Retry" → resulting `EditStatusDto`.
pub async fn edit_sync_now(session_id: String) -> Result<String, BridgeError> {
    with_core(move |c| async move { to_json(&c.edit_sync_now(session_id).await?) }).await
}

/// Open the working copy again (`None` = the session's application).
pub async fn edit_reopen(
    session_id: String,
    open_with_json: Option<String>,
) -> Result<(), BridgeError> {
    let with = open_with(open_with_json)?;
    with_core(move |c| async move { Ok(c.edit_reopen(session_id, with).await?) }).await
}

/// Show the working copy in Finder / Explorer.
pub async fn edit_reveal(session_id: String) -> Result<(), BridgeError> {
    with_core(move |c| async move { Ok(c.edit_reveal(session_id).await?) }).await
}

/// Resolve a conflict; the saved remote copy's path for
/// `KeepRemoteCopyLocally`.
pub async fn edit_resolve(
    session_id: String,
    resolution: EditConflictChoice,
) -> Result<Option<String>, BridgeError> {
    let r = match resolution {
        EditConflictChoice::OverwriteRemote => EditConflictResolutionDto::OverwriteRemote,
        EditConflictChoice::KeepRemoteCopyLocally => {
            EditConflictResolutionDto::KeepRemoteCopyLocally
        }
        EditConflictChoice::DiscardLocal => EditConflictResolutionDto::DiscardLocal,
    };
    with_core(move |c| async move { Ok(c.edit_resolve(session_id, r).await?) }).await
}

/// End a session → `EditStopOutcomeDto`.
pub async fn edit_stop(session_id: String, mode: EditStopChoice) -> Result<String, BridgeError> {
    let m = match mode {
        EditStopChoice::Upload => EditStopModeDto::Upload,
        EditStopChoice::UploadOrKeep => EditStopModeDto::UploadOrKeep,
        EditStopChoice::KeepFiles => EditStopModeDto::KeepFiles,
        EditStopChoice::Discard => EditStopModeDto::Discard,
    };
    with_core(move |c| async move { to_json(&c.edit_stop(session_id, m).await?) }).await
}

/// Working copies left by earlier runs → `Vec<EditLeftoverDto>`.
pub async fn edit_leftovers() -> Result<String, BridgeError> {
    with_core(move |c| async move { to_json(&c.edit_leftovers().await?) }).await
}

/// Continue a leftover through `sftp_id` → `EditSessionDto`.
pub async fn edit_resume(
    session_id: String,
    sftp_id: String,
    open_with_json: Option<String>,
) -> Result<String, BridgeError> {
    let with = open_with(open_with_json)?;
    with_core(move |c| async move { to_json(&c.edit_resume(session_id, sftp_id, with).await?) })
        .await
}

/// Securely remove one leftover.
pub async fn edit_discard_leftover(session_id: String) -> Result<(), BridgeError> {
    with_core(move |c| async move { Ok(c.edit_discard_leftover(session_id).await?) }).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn open_with_json_is_parsed_or_refused() {
        assert_eq!(open_with(None).unwrap(), None);
        assert_eq!(open_with(Some(" ".into())).unwrap(), None);
        assert_eq!(
            open_with(Some(r#"{"kind":"choose"}"#.into())).unwrap(),
            Some(OpenWithDto::Choose)
        );
        let app = open_with(Some(
            r#"{"kind":"app","app":{"kind":"bundle_id","value":"com.apple.TextEdit"}}"#.into(),
        ))
        .unwrap()
        .unwrap();
        assert!(matches!(app, OpenWithDto::App { ref app } if app.kind == "bundle_id"));
        let e = open_with(Some(r#"{"kind":"telepathy"}"#.into())).unwrap_err();
        assert_eq!(e.code, "invalid_input");
    }
}
