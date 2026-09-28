//! SFTP browser facade (SFTP_BROWSER_SPEC §1): detailed listing with owner
//! names and symlink targets, stat, path resolution, new file, recursive
//! duplicate, bounded in-memory preview and recursive transfer jobs.
//! Sessions come from [`AppCore::sftp_open`]; paths are absolute POSIX
//! paths on the server.

use crate::app::AppCore;
use crate::dto::opt_ms;
use crate::error::{AppError, AppResult};
use crate::transfers::{TransferJobDto, TransferRequestDto};
use cc_sftp_core::{EntryKind, RemoteEntry, SftpClient, SftpError};
use serde::{Deserialize, Serialize};

/// Largest preview the core reads into memory (8 MiB: images).
pub const MAX_PREVIEW_BYTES: u64 = 8 * 1024 * 1024;

/// Nesting limit of [`AppCore::sftp_duplicate`].
const MAX_COPY_DEPTH: usize = 64;

/// A remote entry with full metadata (`lstat`; symlink target filled).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RemoteFileInfoDto {
    pub name: String,
    /// Absolute path as listed (symlinks not resolved).
    pub path: String,
    /// `file` | `dir` | `symlink` | `other` — a symlink is `symlink`
    /// whatever it points to.
    pub kind: String,
    pub size: u64,
    /// Permission bits (`0o7777` mask), if reported.
    pub permissions: Option<u32>,
    pub uid: Option<u32>,
    pub gid: Option<u32>,
    /// User / group names from the server's `ls -l` long name.
    pub owner: Option<String>,
    pub group: Option<String>,
    pub modified_at_ms: Option<i64>,
    /// Symlinks: the link text (`readlink`).
    pub link_target: Option<String>,
    /// Symlinks: kind of the final target (`stat`); `None` = dangling.
    pub link_target_kind: Option<String>,
}

fn kind_name(k: EntryKind) -> String {
    format!("{k:?}").to_ascii_lowercase()
}

impl RemoteFileInfoDto {
    fn from_entry(e: &RemoteEntry) -> Self {
        Self {
            name: e.name.clone(),
            path: e.path.clone(),
            kind: kind_name(e.kind),
            size: e.size,
            permissions: e.permissions,
            uid: e.uid,
            gid: e.gid,
            owner: e.user.clone(),
            group: e.group.clone(),
            modified_at_ms: opt_ms(&e.modified),
            link_target: None,
            link_target_kind: None,
        }
    }
}

/// A bounded prefix of a remote file (never written to disk).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SftpPreviewDto {
    pub path: String,
    pub data: Vec<u8>,
    /// Size of the whole remote file.
    pub total_size: u64,
}

/// Validate a remote path argument (non-empty, no NUL).
pub(crate) fn remote_path_arg(field: &str, path: &str) -> AppResult<String> {
    if path.trim().is_empty() {
        return Err(AppError::invalid(field.to_owned(), "must not be empty"));
    }
    if path.contains('\0') || path.len() > crate::validate::MAX_PATH_LEN {
        return Err(AppError::invalid(field.to_owned(), "not a valid path"));
    }
    Ok(path.to_owned())
}

/// A remote file name that is safe as one local path component (no
/// separators, no `.` / `..`, no NUL).
pub(crate) fn safe_local_name(name: &str) -> AppResult<String> {
    if name.is_empty()
        || name == "."
        || name == ".."
        || name.contains(['/', '\\', '\0'])
        || name.chars().any(char::is_control)
    {
        return Err(AppError::invalid(
            "remote_path",
            "the server returned a file name that cannot be stored locally",
        ));
    }
    Ok(name.to_owned())
}

/// Normalize a typed remote path: `~` / `~/x` → `home`, relative → under
/// `base`, `.` / `..` / duplicate slashes collapsed. Pure.
pub fn normalize_remote_path(input: &str, base: &str, home: &str) -> String {
    let text = input.trim();
    let joined = if text.is_empty() {
        base.to_owned()
    } else if text == "~" {
        home.to_owned()
    } else if let Some(rest) = text.strip_prefix("~/") {
        format!("{}/{rest}", home.trim_end_matches('/'))
    } else if text.starts_with('/') {
        text.to_owned()
    } else {
        format!("{}/{text}", base.trim_end_matches('/'))
    };
    let mut parts: Vec<&str> = Vec::new();
    for p in joined.split('/') {
        match p {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            other => parts.push(other),
        }
    }
    format!("/{}", parts.join("/"))
}

/// `true` if `path` is `root` or below it (normalized POSIX paths).
fn is_within(path: &str, root: &str) -> bool {
    let root = root.trim_end_matches('/');
    path == root || path.starts_with(&format!("{root}/")) || root.is_empty()
}

/// Fill link text / target kind of a symlink entry.
async fn with_link(c: &SftpClient, mut d: RemoteFileInfoDto) -> RemoteFileInfoDto {
    if d.kind == "symlink" {
        d.link_target = c.read_link(&d.path).await.ok();
        d.link_target_kind = match c.stat(&d.path).await {
            Ok(t) => Some(kind_name(t.kind)),
            Err(_) => None,
        };
    }
    d
}

fn not_found_dir(e: SftpError, path: &str) -> AppError {
    match e {
        SftpError::NotFound(_) => AppError::RemoteNotFound {
            path: path.to_owned(),
            directory: true,
        },
        other => other.into(),
    }
}

impl AppCore {
    async fn sftp_client(&self, sftp_id: &str) -> AppResult<std::sync::Arc<SftpClient>> {
        let (_, u) = self.unlocked().await?;
        u.ssh.sftp(sftp_id).await
    }

    /// Entries of `path` with full metadata (`.`/`..` excluded; directories
    /// first, then by name). Symlinks carry their link text and target kind.
    pub async fn sftp_list_detailed(
        &self,
        sftp_id: String,
        path: String,
    ) -> AppResult<Vec<RemoteFileInfoDto>> {
        let path = remote_path_arg("path", &path)?;
        let c = self.sftp_client(&sftp_id).await?;
        let entries = match c.list_detailed(&path).await {
            Ok(v) => v,
            // Servers that reject the raw request: plain listing.
            Err(SftpError::Unsupported(_)) => {
                c.list(&path).await.map_err(|e| not_found_dir(e, &path))?
            }
            Err(e) => return Err(not_found_dir(e, &path)),
        };
        let mut out = Vec::with_capacity(entries.len());
        for e in &entries {
            out.push(with_link(&c, RemoteFileInfoDto::from_entry(e)).await);
        }
        Ok(out)
    }

    /// Metadata of one entry (`lstat`, link target filled).
    pub async fn sftp_stat(&self, sftp_id: String, path: String) -> AppResult<RemoteFileInfoDto> {
        let path = remote_path_arg("path", &path)?;
        let c = self.sftp_client(&sftp_id).await?;
        let e = c.lstat(&path).await?;
        Ok(with_link(&c, RemoteFileInfoDto::from_entry(&e)).await)
    }

    /// Absolute, canonical form of a typed directory path (`~`, `~/x`,
    /// relative to `base`, `..`). Must be an existing directory, else
    /// `not_found` + `directory_not_found` (`args.path`).
    pub async fn sftp_resolve_directory(
        &self,
        sftp_id: String,
        input: String,
        base: String,
    ) -> AppResult<String> {
        if input.contains('\0') || base.contains('\0') {
            return Err(AppError::invalid("path", "not a valid path"));
        }
        let c = self.sftp_client(&sftp_id).await?;
        let t = input.trim();
        let home = if t == "~" || t.starts_with("~/") {
            c.home_dir().await?
        } else {
            String::new()
        };
        let base = if base.trim().is_empty() {
            c.home_dir().await?
        } else {
            base
        };
        let path = normalize_remote_path(&input, &base, &home);
        let real = c
            .canonicalize(&path)
            .await
            .map_err(|e| not_found_dir(e, &path))?;
        match c.stat(&real).await {
            Ok(e) if e.kind == EntryKind::Dir => Ok(real),
            Ok(_) => Err(AppError::RemoteNotFound {
                path,
                directory: true,
            }),
            Err(e) => Err(not_found_dir(e, &path)),
        }
    }

    /// Create an empty regular file (exclusive, mode 0644 before the umask).
    /// `already_exists` if the name is taken.
    pub async fn sftp_create_file(&self, sftp_id: String, path: String) -> AppResult<()> {
        let path = remote_path_arg("path", &path)?;
        let c = self.sftp_client(&sftp_id).await?;
        Ok(c.create_file(&path, 0o644).await?)
    }

    /// Server-side copy of a file or (recursively) a folder to `to` in the
    /// same session, keeping permission bits; symlinks are recreated.
    /// Streams through the client (SFTP v3 has no copy). `already_exists`
    /// if `to` exists.
    pub async fn sftp_duplicate(&self, sftp_id: String, from: String, to: String) -> AppResult<()> {
        let from = remote_path_arg("from", &from)?;
        let to = remote_path_arg("to", &to)?;
        let c = self.sftp_client(&sftp_id).await?;
        match c.lstat(&to).await {
            Ok(_) => return Err(AppError::RemoteExists { path: to }),
            Err(SftpError::NotFound(_)) => {}
            Err(e) => return Err(e.into()),
        }
        let root = c.lstat(&from).await?;
        if root.kind == EntryKind::Dir && is_within(&to, from.trim_end_matches('/')) {
            return Err(AppError::invalid("to", "cannot copy a folder into itself"));
        }
        let cancel = cc_sftp_core::CancelToken::new();
        // Iterative: (source, destination, depth); folder modes applied last
        // so read-only folders can still be filled.
        let mut stack = vec![(from.clone(), to.clone(), 0usize)];
        let mut dir_modes: Vec<(String, u32)> = Vec::new();
        while let Some((src, dst, depth)) = stack.pop() {
            if depth > MAX_COPY_DEPTH {
                return Err(AppError::invalid("from", "folder nesting too deep"));
            }
            let e = c.lstat(&src).await?;
            match e.kind {
                EntryKind::File => {
                    c.copy_file(&src, &dst, &mut |_| {}, &cancel).await?;
                }
                EntryKind::Symlink => {
                    let target = c.read_link(&src).await?;
                    c.symlink(&dst, &target).await?;
                }
                EntryKind::Dir => {
                    c.mkdir(&dst).await?;
                    if let Some(mode) = e.permissions {
                        dir_modes.push((dst.clone(), mode));
                    }
                    for child in c.list(&src).await? {
                        let name = safe_local_name(&child.name)?;
                        let d = format!("{}/{}", dst.trim_end_matches('/'), name);
                        stack.push((child.path, d, depth + 1));
                    }
                }
                EntryKind::Other => {
                    tracing::debug!(path = %src, "duplicate: special file skipped");
                }
            }
        }
        for (dir, mode) in dir_modes.into_iter().rev() {
            c.chmod(&dir, mode).await?;
        }
        Ok(())
    }

    /// Read at most `max_bytes` (≤ [`MAX_PREVIEW_BYTES`]) from the start of
    /// a regular file into memory (Quick Look). Nothing touches the disk.
    pub async fn sftp_read_preview(
        &self,
        sftp_id: String,
        path: String,
        max_bytes: u64,
    ) -> AppResult<SftpPreviewDto> {
        let path = remote_path_arg("path", &path)?;
        let c = self.sftp_client(&sftp_id).await?;
        let st = c.stat(&path).await?;
        if st.kind != EntryKind::File {
            return Err(AppError::invalid("path", "not a regular file"));
        }
        let limit = max_bytes.clamp(1, MAX_PREVIEW_BYTES);
        let (data, total) = c.read_head(&path, limit).await?;
        Ok(SftpPreviewDto {
            path,
            data,
            total_size: total.max(st.size),
        })
    }

    // ---- transfer jobs ---------------------------------------------------------

    /// Queue an upload (`local_path` → `remote_path`) or download
    /// (`remote_path` → `local_path`) of a file or folder (recursive) under
    /// the caller's `transfer_id`. Progress / state: [`AppEvent::TransferUpdate`](crate::AppEvent::TransferUpdate).
    pub async fn sftp_start_transfer(
        &self,
        request: TransferRequestDto,
    ) -> AppResult<TransferJobDto> {
        let (_, u) = self.unlocked().await?;
        let client = u.ssh.sftp(&request.sftp_id).await?;
        u.transfers.start(request, client)
    }

    /// Cancel a queued / running job (partial files are removed). `false`
    /// if it already finished or is unknown.
    pub async fn sftp_cancel_transfer(&self, transfer_id: String) -> AppResult<bool> {
        let (_, u) = self.unlocked().await?;
        Ok(u.transfers.cancel(&transfer_id))
    }

    /// Re-queue a failed / cancelled (or completed) job with the same source
    /// and destination under `new_transfer_id`; the old job is removed.
    pub async fn sftp_retry_transfer(
        &self,
        transfer_id: String,
        new_transfer_id: String,
    ) -> AppResult<TransferJobDto> {
        let (_, u) = self.unlocked().await?;
        let mut request = u.transfers.take_for_retry(&transfer_id)?;
        let client = u.ssh.sftp(&request.sftp_id).await?;
        request.transfer_id = new_transfer_id;
        u.transfers.start(request, client)
    }

    /// All transfer jobs of this unlock, most recent first.
    pub async fn sftp_transfers(&self) -> AppResult<Vec<TransferJobDto>> {
        let (_, u) = self.unlocked().await?;
        Ok(u.transfers.list())
    }

    /// One job's snapshot.
    pub async fn sftp_transfer(&self, transfer_id: String) -> AppResult<TransferJobDto> {
        let (_, u) = self.unlocked().await?;
        u.transfers
            .get(&transfer_id)
            .ok_or_else(|| AppError::not_found("transfer", transfer_id))
    }

    /// Remove completed, failed and cancelled jobs.
    pub async fn sftp_clear_finished_transfers(&self) -> AppResult<()> {
        let (_, u) = self.unlocked().await?;
        u.transfers.clear_finished();
        Ok(())
    }

    /// Rust-only (feature `test-harness`): register an SFTP client that is
    /// not backed by SSH (e.g. `cc_sftp_core::test_server::connect_local`)
    /// as a session of `host_id`.
    #[cfg(any(test, feature = "test-harness"))]
    pub async fn sftp_attach_client_for_tests(
        &self,
        host_id: String,
        client: SftpClient,
    ) -> AppResult<String> {
        let (_, u) = self.unlocked().await?;
        let id = crate::dto::parse_id("host_id", &host_id)?;
        Ok(u.ssh.sftp_register(id, client, None).await)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn typed_paths_are_normalized() {
        let n = |i: &str| normalize_remote_path(i, "/var/www", "/home/alice");
        assert_eq!(n("~"), "/home/alice");
        assert_eq!(n("~/logs/../x"), "/home/alice/x");
        assert_eq!(n("html"), "/var/www/html");
        assert_eq!(n(".."), "/var");
        assert_eq!(n("../../../.."), "/");
        assert_eq!(n("/etc//nginx/."), "/etc/nginx");
        assert_eq!(n("  "), "/var/www");
        assert!(is_within("/a/b/c", "/a/b"));
        assert!(is_within("/a/b", "/a/b"));
        assert!(!is_within("/a/bc", "/a/b"));
    }

    #[test]
    fn unsafe_remote_names_are_refused() {
        for bad in ["", ".", "..", "a/b", "a\\b", "x\0", "tab\t"] {
            assert!(safe_local_name(bad).is_err(), "{bad:?}");
        }
        assert_eq!(safe_local_name("ok name.txt").unwrap(), "ok name.txt");
    }
}
