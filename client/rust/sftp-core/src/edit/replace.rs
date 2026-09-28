//! Replacing the remote file with a saved working copy.
//!
//! Default: atomic replace — upload to `.<name>.cc-upload-<rand>` in the
//! same directory (created exclusively with mode 0600, so the content is never
//! world-readable while uploading), restore the original permission bits,
//! then rename over the original:
//!
//! 1. `posix-rename@openssh.com` when the server announces it (atomic);
//! 2. else a plain SFTP `RENAME` (atomic on servers with POSIX semantics);
//! 3. if that refuses to overwrite (OpenSSH implements `RENAME` with
//!    `link()`+`unlink()`), remove the original and rename. **Race:** between
//!    the remove and the rename the path does not exist (readers get ENOENT,
//!    another writer may create it and our rename then fails). If that final
//!    rename fails, the uploaded temp file is kept on the server (the original
//!    is already gone) and reported; the local working copy is always kept.
//!
//! In-place fallback (optional): truncate + rewrite the original when the
//! temp file cannot be created (directory not writable) or when the replaced
//! file would get a different owner / group. Not atomic, but keeps owner,
//! group, mode, ACLs and hard links.

use super::remote::{EditRemote, ProgressFn};
use super::store::{remote_base_name, remote_parent, BaseVersion};
use crate::{join, RemoteEntry, SftpError};

/// How the file was replaced.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Strategy {
    PosixRename,
    Rename,
    RemoveThenRename,
    InPlace,
}

#[derive(Debug)]
pub(crate) struct Replaced {
    /// Remote metadata of the new file (taken from the temp file before the
    /// rename, which does not change size or mtime).
    pub entry: RemoteEntry,
    pub strategy: Strategy,
}

#[derive(Debug)]
pub(crate) struct ReplaceFailure {
    pub error: Box<SftpError>,
    /// The original was removed by us (non-atomic fallback) and not replaced.
    pub target_removed: bool,
    /// Temp file intentionally left on the server (holds the new content).
    pub leftover_tmp: Option<String>,
}

impl From<SftpError> for ReplaceFailure {
    fn from(error: SftpError) -> Self {
        Self {
            error: Box::new(error),
            target_removed: false,
            leftover_tmp: None,
        }
    }
}

/// Name of the temp upload file for `name` (bounded length).
pub(crate) fn temp_name(name: &str) -> String {
    let mut cut = name.len().min(180);
    while !name.is_char_boundary(cut) {
        cut -= 1;
    }
    let rand = uuid::Uuid::new_v4().simple().to_string();
    format!(".{}.cc-upload-{}", &name[..cut], &rand[..12])
}

async fn discard(remote: &dyn EditRemote, tmp: &str) {
    let _ = remote.remove_file(tmp).await;
}

fn owner_changes(base: &BaseVersion, new: &RemoteEntry) -> bool {
    let differs = |a: Option<u32>, b: Option<u32>| matches!((a, b), (Some(a), Some(b)) if a != b);
    differs(base.uid, new.uid) || differs(base.gid, new.gid)
}

/// Replace `target` with `data` (see module docs).
pub(crate) async fn replace(
    remote: &dyn EditRemote,
    target: &str,
    data: &[u8],
    base: &BaseVersion,
    in_place_fallback: bool,
    progress: ProgressFn<'_>,
) -> Result<Replaced, ReplaceFailure> {
    let name = remote_base_name(target);
    let tmp = join(remote_parent(target), &temp_name(name));

    match remote.write_new_file(&tmp, data, progress).await {
        Ok(()) => {}
        Err(SftpError::PermissionDenied(_)) if in_place_fallback => {
            tracing::debug!(
                remote_path = target,
                "cannot create a temp file next to the target; writing in place"
            );
            return in_place(remote, target, data, progress).await;
        }
        Err(e) => return Err(e.into()),
    }

    if let Some(mode) = base.permissions {
        if let Err(e) = remote.chmod(&tmp, mode).await {
            // setuid/setgid/sticky may be refused: keep at least rwx bits.
            let plain = mode & 0o777;
            if plain == mode || remote.chmod(&tmp, plain).await.is_err() {
                discard(remote, &tmp).await;
                return Err(e.into());
            }
        }
    }

    let mut entry = match remote.stat(&tmp).await {
        Ok(e) => e,
        Err(e) => {
            discard(remote, &tmp).await;
            return Err(e.into());
        }
    };

    if in_place_fallback && owner_changes(base, &entry) {
        tracing::debug!(
            remote_path = target,
            "replace would change owner/group; writing in place"
        );
        discard(remote, &tmp).await;
        return in_place(remote, target, data, progress).await;
    }

    let strategy = if remote.supports_posix_rename() {
        match remote.posix_rename(&tmp, target).await {
            Ok(()) => Strategy::PosixRename,
            Err(SftpError::Unsupported(_)) => rename_fallback(remote, &tmp, target).await?,
            Err(e) => {
                discard(remote, &tmp).await;
                return Err(e.into());
            }
        }
    } else {
        rename_fallback(remote, &tmp, target).await?
    };

    entry.path = target.to_string();
    entry.name = name.to_string();
    Ok(Replaced { entry, strategy })
}

async fn rename_fallback(
    remote: &dyn EditRemote,
    tmp: &str,
    target: &str,
) -> Result<Strategy, ReplaceFailure> {
    match remote.rename(tmp, target).await {
        Ok(()) => return Ok(Strategy::Rename),
        // Generic failure / exists: the server refuses to overwrite.
        Err(SftpError::Remote { .. } | SftpError::AlreadyExists(_)) => {}
        Err(e) => {
            discard(remote, tmp).await;
            return Err(e.into());
        }
    }
    // Non-atomic from here (see module docs for the race).
    match remote.remove_file(target).await {
        Ok(()) | Err(SftpError::NotFound(_)) => {}
        Err(e) => {
            discard(remote, tmp).await;
            return Err(e.into());
        }
    }
    match remote.rename(tmp, target).await {
        Ok(()) => Ok(Strategy::RemoveThenRename),
        Err(error) => Err(ReplaceFailure {
            error: Box::new(error),
            target_removed: true,
            leftover_tmp: Some(tmp.to_string()),
        }),
    }
}

async fn in_place(
    remote: &dyn EditRemote,
    target: &str,
    data: &[u8],
    progress: ProgressFn<'_>,
) -> Result<Replaced, ReplaceFailure> {
    remote.overwrite_file(target, data, progress).await?;
    let entry = remote.stat(target).await?;
    Ok(Replaced {
        entry,
        strategy: Strategy::InPlace,
    })
}
