//! # cc-sftp-core — SFTP over ssh-core sessions (russh-sftp)
//!
//! Browse (list / stat), streaming upload / download with progress and
//! cancellation, mkdir, rename, remove (file / directory, optionally
//! recursive) and chmod. Works over any SSH session (incl. jump chains) via
//! [`SftpClient::open`], or over any byte stream via
//! [`SftpClient::from_stream`]. Atomic replace via
//! `posix-rename@openssh.com` ([`SftpClient::posix_rename`]).
//!
//! [`edit`] — "edit in the default editor" sessions with auto-upload on save
//! (docs/design/SFTP_BROWSER_SPEC.md §2).

use cc_ssh_core::{SshError, SshSession};
use russh_sftp::client::error::Error as SftpProtoError;
use russh_sftp::client::fs::Metadata;
use russh_sftp::client::SftpSession;
use russh_sftp::protocol::{FileAttributes, FileType, OpenFlags, StatusCode};
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::sync::Notify;

/// SFTP errors.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum SftpError {
    #[error("SSH error: {0}")]
    Ssh(#[from] SshError),
    #[error("no such file or directory: {0}")]
    NotFound(String),
    #[error("permission denied: {0}")]
    PermissionDenied(String),
    #[error("already exists: {0}")]
    AlreadyExists(String),
    #[error("transfer cancelled")]
    Cancelled,
    #[error("local I/O error on {path}: {source}")]
    Local {
        path: String,
        #[source]
        source: std::io::Error,
    },
    #[error("SFTP error on {path}: {message}")]
    Remote { path: String, message: String },
    /// The server does not support the requested operation / extension.
    #[error("not supported by the server: {0}")]
    Unsupported(String),
}

fn map_err(path: &str, e: SftpProtoError) -> SftpError {
    match &e {
        SftpProtoError::Status(s) => match s.status_code {
            StatusCode::NoSuchFile => SftpError::NotFound(path.to_string()),
            StatusCode::PermissionDenied => SftpError::PermissionDenied(path.to_string()),
            _ => SftpError::Remote {
                path: path.to_string(),
                message: e.to_string(),
            },
        },
        _ => SftpError::Remote {
            path: path.to_string(),
            message: e.to_string(),
        },
    }
}

fn io_to_sftp(path: &str, e: std::io::Error) -> SftpError {
    match e.kind() {
        std::io::ErrorKind::NotFound => SftpError::NotFound(path.to_string()),
        std::io::ErrorKind::PermissionDenied => SftpError::PermissionDenied(path.to_string()),
        _ => SftpError::Remote {
            path: path.to_string(),
            message: e.to_string(),
        },
    }
}

/// Kind of a remote entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum EntryKind {
    File,
    Dir,
    Symlink,
    Other,
}

/// A remote directory entry / stat result.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RemoteEntry {
    pub name: String,
    pub path: String,
    pub kind: EntryKind,
    pub size: u64,
    /// Permission bits (`0o7777` mask), if reported.
    pub permissions: Option<u32>,
    pub uid: Option<u32>,
    pub gid: Option<u32>,
    pub user: Option<String>,
    pub group: Option<String>,
    pub modified: Option<chrono::DateTime<chrono::Utc>>,
}

impl RemoteEntry {
    pub fn is_dir(&self) -> bool {
        self.kind == EntryKind::Dir
    }

    /// `ls -l` style mode string, e.g. `drwxr-xr-x`.
    pub fn mode_string(&self) -> String {
        let t = match self.kind {
            EntryKind::Dir => 'd',
            EntryKind::Symlink => 'l',
            EntryKind::File => '-',
            EntryKind::Other => '?',
        };
        let mut s = String::with_capacity(10);
        s.push(t);
        let p = self.permissions.unwrap_or(0);
        for shift in [6u32, 3, 0] {
            let bits = (p >> shift) & 7;
            s.push(if bits & 4 != 0 { 'r' } else { '-' });
            s.push(if bits & 2 != 0 { 'w' } else { '-' });
            s.push(if bits & 1 != 0 { 'x' } else { '-' });
        }
        s
    }
}

fn join(dir: &str, name: &str) -> String {
    if dir.is_empty() || dir == "." {
        name.to_string()
    } else if dir.ends_with('/') {
        format!("{dir}{name}")
    } else {
        format!("{dir}/{name}")
    }
}

fn entry_from(name: String, path: String, a: &FileAttributes) -> RemoteEntry {
    let kind = match a.file_type() {
        FileType::Dir => EntryKind::Dir,
        FileType::File => EntryKind::File,
        FileType::Symlink => EntryKind::Symlink,
        FileType::Other => EntryKind::Other,
    };
    RemoteEntry {
        name,
        path,
        kind,
        size: a.size.unwrap_or(0),
        permissions: a.permissions.map(|p| p & 0o7777),
        uid: a.uid,
        gid: a.gid,
        user: a.user.clone(),
        group: a.group.clone(),
        modified: a
            .mtime
            .and_then(|t| chrono::DateTime::from_timestamp(i64::from(t), 0)),
    }
}

fn base_name(path: &str) -> String {
    path.trim_end_matches('/')
        .rsplit('/')
        .next()
        .unwrap_or(path)
        .to_string()
}

/// Cooperative cancellation for transfers (cheap to clone).
#[derive(Debug, Clone, Default)]
pub struct CancelToken {
    inner: Arc<(AtomicBool, Notify)>,
}

impl CancelToken {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn cancel(&self) {
        self.inner.0.store(true, Ordering::SeqCst);
        self.inner.1.notify_waiters();
    }
    pub fn is_cancelled(&self) -> bool {
        self.inner.0.load(Ordering::SeqCst)
    }
}

/// Progress of a transfer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct TransferProgress {
    pub transferred: u64,
    /// Total size when known.
    pub total: Option<u64>,
}

/// Transfer tunables.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TransferOptions {
    /// Read/write chunk size (progress granularity).
    pub chunk_size: usize,
    /// Replace an existing destination.
    pub overwrite: bool,
    /// Delete the partially written destination on cancel / error.
    pub remove_partial: bool,
    /// Copy the source permission bits to the destination (Unix).
    pub preserve_permissions: bool,
}

impl Default for TransferOptions {
    fn default() -> Self {
        Self {
            chunk_size: 256 * 1024,
            overwrite: true,
            remove_partial: true,
            preserve_permissions: false,
        }
    }
}

/// Result of a completed transfer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct TransferSummary {
    pub bytes: u64,
    pub elapsed_ms: u64,
}

/// SFTP client bound to one SSH channel.
pub struct SftpClient {
    inner: SftpSession,
    /// Extension requests russh-sftp does not expose (see [`ext`]).
    ext: ext::ExtChannel,
}

impl std::fmt::Debug for SftpClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SftpClient").finish_non_exhaustive()
    }
}

async fn copy_with_progress<R, W>(
    reader: &mut R,
    writer: &mut W,
    total: Option<u64>,
    chunk: usize,
    progress: &mut (dyn FnMut(TransferProgress) + Send),
    cancel: &CancelToken,
) -> Result<u64, std::io::Result<()>>
where
    R: AsyncRead + Unpin,
    W: AsyncWrite + Unpin,
{
    let mut buf = vec![0u8; chunk.max(4096)];
    let mut done = 0u64;
    progress(TransferProgress {
        transferred: 0,
        total,
    });
    loop {
        if cancel.is_cancelled() {
            return Err(Ok(()));
        }
        let n = tokio::select! {
            r = reader.read(&mut buf) => r.map_err(Err)?,
            _ = cancel.inner.1.notified() => return Err(Ok(())),
        };
        if n == 0 {
            break;
        }
        writer.write_all(&buf[..n]).await.map_err(Err)?;
        done += n as u64;
        progress(TransferProgress {
            transferred: done,
            total,
        });
    }
    writer.flush().await.map_err(Err)?;
    Ok(done)
}

impl SftpClient {
    /// Start the `sftp` subsystem on `session` (works over jump chains).
    pub async fn open(session: &SshSession) -> Result<Self, SftpError> {
        let stream = session.open_subsystem("sftp").await?;
        Self::from_stream(stream).await
    }

    /// Run the SFTP protocol over an arbitrary byte stream.
    pub async fn from_stream<S>(stream: S) -> Result<Self, SftpError>
    where
        S: AsyncRead + AsyncWrite + Unpin + Send + 'static,
    {
        let (pipe, ext) = ext::wrap(stream);
        let inner = SftpSession::new(pipe)
            .await
            .map_err(|e| map_err("<init>", e))?;
        inner.set_timeout(60);
        Ok(Self { inner, ext })
    }

    /// Extensions announced by the server (`name → version`).
    pub fn server_extensions(&self) -> std::collections::HashMap<String, String> {
        self.ext.extensions()
    }

    /// Does the server support `posix-rename@openssh.com`?
    pub fn supports_posix_rename(&self) -> bool {
        self.ext.supports(ext::POSIX_RENAME, "1")
    }

    /// Rename `from` to `to`, atomically replacing an existing `to`
    /// (`posix-rename@openssh.com`; [`SftpError::Unsupported`] if the server
    /// lacks it). Plain [`SftpClient::rename`] fails on OpenSSH when `to`
    /// exists.
    pub async fn posix_rename(&self, from: &str, to: &str) -> Result<(), SftpError> {
        self.ext.posix_rename(from, to).await
    }

    /// Resolve a path on the server (`"."` = home directory).
    pub async fn canonicalize(&self, path: &str) -> Result<String, SftpError> {
        self.inner
            .canonicalize(path)
            .await
            .map_err(|e| map_err(path, e))
    }

    /// Remote home directory.
    pub async fn home_dir(&self) -> Result<String, SftpError> {
        self.canonicalize(".").await
    }

    /// List a directory (without `.`/`..`), directories first, then by name.
    pub async fn list(&self, path: &str) -> Result<Vec<RemoteEntry>, SftpError> {
        let rd = self
            .inner
            .read_dir(path)
            .await
            .map_err(|e| map_err(path, e))?;
        let mut v: Vec<RemoteEntry> = rd
            .filter(|e| e.file_name() != "." && e.file_name() != "..")
            .map(|e| {
                let name = e.file_name();
                let meta: Metadata = e.metadata();
                entry_from(name.clone(), join(path, &name), &meta)
            })
            .collect();
        v.sort_by(|a, b| {
            b.is_dir()
                .cmp(&a.is_dir())
                .then_with(|| a.name.cmp(&b.name))
        });
        Ok(v)
    }

    /// Stat (follows symlinks).
    pub async fn stat(&self, path: &str) -> Result<RemoteEntry, SftpError> {
        let m = self
            .inner
            .metadata(path)
            .await
            .map_err(|e| map_err(path, e))?;
        Ok(entry_from(base_name(path), path.to_string(), &m))
    }

    /// Stat without following symlinks.
    pub async fn lstat(&self, path: &str) -> Result<RemoteEntry, SftpError> {
        let m = self
            .inner
            .symlink_metadata(path)
            .await
            .map_err(|e| map_err(path, e))?;
        Ok(entry_from(base_name(path), path.to_string(), &m))
    }

    /// Does `path` exist?
    pub async fn exists(&self, path: &str) -> Result<bool, SftpError> {
        match self.stat(path).await {
            Ok(_) => Ok(true),
            Err(SftpError::NotFound(_)) => Ok(false),
            Err(e) => Err(e),
        }
    }

    pub async fn mkdir(&self, path: &str) -> Result<(), SftpError> {
        self.inner
            .create_dir(path)
            .await
            .map_err(|e| map_err(path, e))
    }

    /// `mkdir -p`.
    pub async fn mkdir_all(&self, path: &str) -> Result<(), SftpError> {
        let mut cur = if path.starts_with('/') {
            "/".to_string()
        } else {
            String::new()
        };
        for part in path.split('/').filter(|p| !p.is_empty()) {
            cur = join(&cur, part);
            match self.stat(&cur).await {
                Ok(e) if e.is_dir() => continue,
                Ok(_) => return Err(SftpError::AlreadyExists(cur)),
                Err(SftpError::NotFound(_)) => self.mkdir(&cur).await?,
                Err(e) => return Err(e),
            }
        }
        Ok(())
    }

    pub async fn rename(&self, from: &str, to: &str) -> Result<(), SftpError> {
        self.inner
            .rename(from, to)
            .await
            .map_err(|e| map_err(from, e))
    }

    pub async fn remove_file(&self, path: &str) -> Result<(), SftpError> {
        self.inner
            .remove_file(path)
            .await
            .map_err(|e| map_err(path, e))
    }

    /// Remove a directory; with `recursive` its whole content first.
    pub async fn remove_dir(&self, path: &str, recursive: bool) -> Result<(), SftpError> {
        if recursive {
            let mut stack = vec![(path.to_string(), false)];
            while let Some((dir, visited)) = stack.pop() {
                if visited {
                    self.inner
                        .remove_dir(dir.as_str())
                        .await
                        .map_err(|e| map_err(&dir, e))?;
                    continue;
                }
                stack.push((dir.clone(), true));
                for e in self.list(&dir).await? {
                    if e.kind == EntryKind::Dir {
                        stack.push((e.path, false));
                    } else {
                        self.remove_file(&e.path).await?;
                    }
                }
            }
            Ok(())
        } else {
            self.inner
                .remove_dir(path)
                .await
                .map_err(|e| map_err(path, e))
        }
    }

    /// Change permission bits (`mode & 0o7777`).
    pub async fn chmod(&self, path: &str, mode: u32) -> Result<(), SftpError> {
        let attrs = FileAttributes {
            permissions: Some(mode & 0o7777),
            ..FileAttributes::empty()
        };
        self.inner
            .set_metadata(path, attrs)
            .await
            .map_err(|e| map_err(path, e))
    }

    /// Read a whole (small) remote file.
    pub async fn read(&self, path: &str) -> Result<Vec<u8>, SftpError> {
        self.inner.read(path).await.map_err(|e| map_err(path, e))
    }

    /// Write a whole (small) remote file (create / truncate).
    pub async fn write(&self, path: &str, data: &[u8]) -> Result<(), SftpError> {
        let mut f = self
            .inner
            .open_with_flags(
                path,
                OpenFlags::CREATE | OpenFlags::TRUNCATE | OpenFlags::WRITE,
            )
            .await
            .map_err(|e| map_err(path, e))?;
        f.write_all(data).await.map_err(|e| io_to_sftp(path, e))?;
        f.shutdown().await.map_err(|e| io_to_sftp(path, e))
    }

    /// Stream any reader to a remote file.
    pub async fn upload_from<R>(
        &self,
        reader: &mut R,
        total: Option<u64>,
        remote: &str,
        opts: &TransferOptions,
        progress: &mut (dyn FnMut(TransferProgress) + Send),
        cancel: &CancelToken,
    ) -> Result<TransferSummary, SftpError>
    where
        R: AsyncRead + Unpin + Send,
    {
        let started = std::time::Instant::now();
        if !opts.overwrite && self.exists(remote).await? {
            return Err(SftpError::AlreadyExists(remote.to_string()));
        }
        let mut f = self
            .inner
            .open_with_flags(
                remote,
                OpenFlags::CREATE | OpenFlags::TRUNCATE | OpenFlags::WRITE,
            )
            .await
            .map_err(|e| map_err(remote, e))?;
        let r = copy_with_progress(reader, &mut f, total, opts.chunk_size, progress, cancel).await;
        let close = f.shutdown().await;
        match r {
            Ok(bytes) => {
                close.map_err(|e| io_to_sftp(remote, e))?;
                Ok(TransferSummary {
                    bytes,
                    elapsed_ms: started.elapsed().as_millis() as u64,
                })
            }
            Err(outcome) => {
                if opts.remove_partial {
                    let _ = self.inner.remove_file(remote).await;
                }
                Err(match outcome {
                    Ok(()) => SftpError::Cancelled,
                    Err(e) => io_to_sftp(remote, e),
                })
            }
        }
    }

    /// Upload a local file.
    pub async fn upload(
        &self,
        local: &Path,
        remote: &str,
        opts: &TransferOptions,
        progress: &mut (dyn FnMut(TransferProgress) + Send),
        cancel: &CancelToken,
    ) -> Result<TransferSummary, SftpError> {
        let lp = local.display().to_string();
        let mut file = tokio::fs::File::open(local)
            .await
            .map_err(|source| SftpError::Local {
                path: lp.clone(),
                source,
            })?;
        let meta = file.metadata().await.map_err(|source| SftpError::Local {
            path: lp.clone(),
            source,
        })?;
        let summary = self
            .upload_from(&mut file, Some(meta.len()), remote, opts, progress, cancel)
            .await?;
        #[cfg(unix)]
        if opts.preserve_permissions {
            use std::os::unix::fs::PermissionsExt;
            self.chmod(remote, meta.permissions().mode()).await?;
        }
        Ok(summary)
    }

    /// Stream a remote file into any writer.
    pub async fn download_to<W>(
        &self,
        remote: &str,
        writer: &mut W,
        opts: &TransferOptions,
        progress: &mut (dyn FnMut(TransferProgress) + Send),
        cancel: &CancelToken,
    ) -> Result<TransferSummary, SftpError>
    where
        W: AsyncWrite + Unpin + Send,
    {
        let started = std::time::Instant::now();
        let mut f = self
            .inner
            .open(remote)
            .await
            .map_err(|e| map_err(remote, e))?;
        let total = f.metadata().await.ok().and_then(|m| m.size);
        let r = copy_with_progress(&mut f, writer, total, opts.chunk_size, progress, cancel).await;
        let _ = f.shutdown().await;
        match r {
            Ok(bytes) => Ok(TransferSummary {
                bytes,
                elapsed_ms: started.elapsed().as_millis() as u64,
            }),
            Err(Ok(())) => Err(SftpError::Cancelled),
            Err(Err(e)) => Err(SftpError::Local {
                path: "<writer>".into(),
                source: e,
            }),
        }
    }

    /// Download a remote file to a local path.
    pub async fn download(
        &self,
        remote: &str,
        local: &Path,
        opts: &TransferOptions,
        progress: &mut (dyn FnMut(TransferProgress) + Send),
        cancel: &CancelToken,
    ) -> Result<TransferSummary, SftpError> {
        let lp = local.display().to_string();
        if !opts.overwrite && tokio::fs::try_exists(local).await.unwrap_or(false) {
            return Err(SftpError::AlreadyExists(lp));
        }
        // Fail early (before creating the local file) if the source is missing.
        let src = self.stat(remote).await?;
        let mut file = tokio::fs::File::create(local)
            .await
            .map_err(|source| SftpError::Local {
                path: lp.clone(),
                source,
            })?;
        let r = self
            .download_to(remote, &mut file, opts, progress, cancel)
            .await;
        let synced = file.sync_all().await;
        drop(file);
        match r {
            Ok(s) => {
                synced.map_err(|source| SftpError::Local {
                    path: lp.clone(),
                    source,
                })?;
                #[cfg(unix)]
                if opts.preserve_permissions {
                    if let Some(mode) = src.permissions {
                        use std::os::unix::fs::PermissionsExt;
                        let _ =
                            std::fs::set_permissions(local, std::fs::Permissions::from_mode(mode));
                    }
                }
                let _ = &src;
                Ok(s)
            }
            Err(e) => {
                if opts.remove_partial {
                    let _ = tokio::fs::remove_file(local).await;
                }
                Err(e)
            }
        }
    }

    /// Like [`SftpClient::list`], plus owner / group **names** parsed from
    /// the server's `ls -l` style `longname` (SFTP v3 attributes carry only
    /// uid / gid). Entries are `lstat`-like: a symlink is reported as
    /// [`EntryKind::Symlink`].
    pub async fn list_detailed(&self, path: &str) -> Result<Vec<RemoteEntry>, SftpError> {
        let names = self.ext.read_dir_long(path).await?;
        let mut v: Vec<RemoteEntry> = names
            .into_iter()
            .filter(|n| n.filename != "." && n.filename != "..")
            .map(|n| {
                let mut e = entry_from(n.filename.clone(), join(path, &n.filename), &n.attrs);
                if let Some((user, group)) = ext::longname_owner(&n.longname) {
                    e.user.get_or_insert(user);
                    e.group.get_or_insert(group);
                }
                e
            })
            .collect();
        v.sort_by(|a, b| {
            b.is_dir()
                .cmp(&a.is_dir())
                .then_with(|| a.name.cmp(&b.name))
        });
        Ok(v)
    }

    /// The text of a symbolic link (`readlink`, not resolved).
    pub async fn read_link(&self, path: &str) -> Result<String, SftpError> {
        self.inner
            .read_link(path)
            .await
            .map_err(|e| map_err(path, e))
    }

    /// Create the symbolic link `link` pointing to `target`. OpenSSH's
    /// `sftp-server` reads the two `SSH_FXP_SYMLINK` arguments in reverse
    /// order compared to the draft; servers announcing `@openssh.com`
    /// extensions get the OpenSSH order.
    pub async fn symlink(&self, link: &str, target: &str) -> Result<(), SftpError> {
        let openssh = self
            .ext
            .extensions()
            .keys()
            .any(|k| k.ends_with("@openssh.com"));
        let r = if openssh {
            self.inner.symlink(target, link).await
        } else {
            self.inner.symlink(link, target).await
        };
        r.map_err(|e| map_err(link, e))
    }

    /// Create an empty regular file exclusively (`O_EXCL`) with `mode`
    /// (subject to the server's umask). [`SftpError::AlreadyExists`] if the
    /// name is taken (checked before and, for servers reporting a generic
    /// failure on `O_EXCL`, after the attempt).
    pub async fn create_file(&self, path: &str, mode: u32) -> Result<(), SftpError> {
        match self.lstat(path).await {
            Ok(_) => return Err(SftpError::AlreadyExists(path.to_string())),
            Err(SftpError::NotFound(_)) => {}
            Err(e) => return Err(e),
        }
        let attrs = FileAttributes {
            permissions: Some(mode & 0o7777),
            ..FileAttributes::empty()
        };
        let opened = self
            .inner
            .open_with_flags_and_attributes(
                path,
                OpenFlags::CREATE | OpenFlags::EXCLUDE | OpenFlags::WRITE,
                attrs,
            )
            .await;
        match opened {
            Ok(mut f) => f.shutdown().await.map_err(|e| io_to_sftp(path, e)),
            Err(e) => {
                let err = map_err(path, e);
                if !matches!(err, SftpError::PermissionDenied(_)) && self.lstat(path).await.is_ok()
                {
                    return Err(SftpError::AlreadyExists(path.to_string()));
                }
                Err(err)
            }
        }
    }

    /// Read at most `max_bytes` from the start of a remote file into memory
    /// (previews). Returns the bytes and the whole file's size.
    pub async fn read_head(&self, path: &str, max_bytes: u64) -> Result<(Vec<u8>, u64), SftpError> {
        let mut f = self.inner.open(path).await.map_err(|e| map_err(path, e))?;
        let total = f.metadata().await.ok().and_then(|m| m.size).unwrap_or(0);
        let cap = usize::try_from(max_bytes.min(total))
            .unwrap_or(0)
            .min(8 << 20);
        let mut buf = Vec::with_capacity(cap);
        let mut chunk = vec![0u8; 64 * 1024];
        let r = async {
            while (buf.len() as u64) < max_bytes {
                let room = usize::try_from(max_bytes - buf.len() as u64)
                    .unwrap_or(usize::MAX)
                    .min(chunk.len());
                let n = f.read(&mut chunk[..room]).await?;
                if n == 0 {
                    break;
                }
                buf.extend_from_slice(&chunk[..n]);
            }
            Ok::<_, std::io::Error>(())
        }
        .await;
        let _ = f.shutdown().await;
        r.map_err(|e| io_to_sftp(path, e))?;
        let total = total.max(buf.len() as u64);
        Ok((buf, total))
    }

    /// Server-side copy of one regular file through the client (SFTP v3 has
    /// no copy): `to` is created exclusively (owner-only while written),
    /// then gets `from`'s permission bits. The partial copy is removed on
    /// error / cancel.
    pub async fn copy_file(
        &self,
        from: &str,
        to: &str,
        progress: &mut (dyn FnMut(TransferProgress) + Send),
        cancel: &CancelToken,
    ) -> Result<TransferSummary, SftpError> {
        let started = std::time::Instant::now();
        let src = self.stat(from).await?;
        if src.kind != EntryKind::File {
            return Err(SftpError::Remote {
                path: from.to_string(),
                message: "not a regular file".into(),
            });
        }
        let mut reader = self.inner.open(from).await.map_err(|e| map_err(from, e))?;
        let attrs = FileAttributes {
            permissions: Some(0o600),
            ..FileAttributes::empty()
        };
        let mut writer = match self
            .inner
            .open_with_flags_and_attributes(
                to,
                OpenFlags::CREATE | OpenFlags::EXCLUDE | OpenFlags::WRITE,
                attrs,
            )
            .await
        {
            Ok(w) => w,
            Err(e) => {
                let _ = reader.shutdown().await;
                let err = map_err(to, e);
                if !matches!(err, SftpError::PermissionDenied(_)) && self.lstat(to).await.is_ok() {
                    return Err(SftpError::AlreadyExists(to.to_string()));
                }
                return Err(err);
            }
        };
        let chunk = TransferOptions::default().chunk_size;
        let r = copy_with_progress(
            &mut reader,
            &mut writer,
            Some(src.size),
            chunk,
            progress,
            cancel,
        )
        .await;
        let _ = reader.shutdown().await;
        let close = writer.shutdown().await;
        let result = match r {
            Ok(bytes) => close.map_err(|e| io_to_sftp(to, e)).map(|()| bytes),
            Err(Ok(())) => Err(SftpError::Cancelled),
            Err(Err(e)) => Err(io_to_sftp(to, e)),
        };
        match result {
            Ok(bytes) => {
                if let Some(mode) = src.permissions {
                    self.chmod(to, mode).await?;
                }
                Ok(TransferSummary {
                    bytes,
                    elapsed_ms: started.elapsed().as_millis() as u64,
                })
            }
            Err(e) => {
                let _ = self.inner.remove_file(to).await;
                Err(e)
            }
        }
    }

    /// Close the SFTP session (the SSH session stays open).
    pub async fn close(&self) -> Result<(), SftpError> {
        self.inner.close().await.map_err(|e| map_err("<close>", e))
    }
}

pub mod edit;
mod ext;
/// In-process SFTP server over a local directory (tests of this crate and,
/// with the `test-harness` feature, of its consumers).
#[cfg(any(test, feature = "test-harness"))]
pub mod test_server;
#[cfg(test)]
mod tests;
