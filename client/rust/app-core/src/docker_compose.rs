//! Compose projects of a Docker session: the files named by the containers'
//! labels are read on the engine host (this computer's disk, or SFTP over
//! the session's SSH connection), and edits are validated with
//! `docker compose config -q` against a candidate written next to the
//! original before anything is replaced. `up -d` then runs as a job whose
//! output the UI polls. Editing needs the vault passphrase (a short-lived
//! grant per project); file contents are treated as secrets and shown
//! masked until then.
//!
//! Saving never trusts a name twice: candidates and backups are created
//! exclusively without following links and written through the one open
//! file; the candidate is checked to still be that file before and after
//! validation and before and after it replaces the original (inode on this
//! computer, SHA-256 over SFTP), and a swapped file is restored from the
//! backup.
use crate::{AppCore, AppError, AppResult};
use async_trait::async_trait;
use cc_docker_core::compose::{
    self, backup_path, candidate_path, compose_argv, strip_ansi, ComposeCli, ComposeLocateError,
    ComposeLocation,
};
use cc_sftp_core::{EntryKind, SftpClient, SftpError};
use cc_ssh_core::SshSession;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::{HashMap, VecDeque},
    fmt,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use tokio::sync::Notify;
use tokio_util::sync::CancellationToken;
use zeroize::Zeroize;

/// Largest compose file shown or saved.
pub(crate) const MAX_FILE: u64 = 1 << 20;
/// How long a passphrase check allows editing one project.
const GRANT_TTL: Duration = Duration::from_secs(15 * 60);
const MAX_GRANTS: usize = 64;
const MAX_JOBS: usize = 16;
const DETECT_TIMEOUT: Duration = Duration::from_secs(30);
const VALIDATE_TIMEOUT: Duration = Duration::from_secs(120);
const APPLY_TIMEOUT: Duration = Duration::from_secs(30 * 60);
/// Finished jobs nobody polls any more are dropped after this.
const JOB_RETENTION: Duration = Duration::from_secs(10 * 60);
const MAX_JOB_LINES: usize = 10_000;
/// Collected command output kept for the result (validation errors).
const MAX_OUTPUT: usize = 64 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum HostFileError {
    NotFound,
    PermissionDenied,
    Exists,
    TooLarge,
    NotAFile,
    Other(String),
}

impl HostFileError {
    pub(crate) fn code(&self) -> &'static str {
        match self {
            Self::NotFound => "not_found",
            Self::PermissionDenied => "permission_denied",
            Self::Exists => "exists",
            Self::TooLarge => "too_large",
            Self::NotAFile => "not_a_file",
            Self::Other(_) => "io_error",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum RunError {
    /// The program is not installed (or not on the search path).
    NotFound,
    Failed(String),
}

#[derive(Debug, Clone, Default)]
pub(crate) struct RunOutput {
    pub exit: Option<i32>,
    pub output: String,
}

impl RunOutput {
    fn success(&self) -> bool {
        self.exit == Some(0)
    }
}

pub(crate) type LineSink<'a> = &'a (dyn Fn(String) + Send + Sync);

/// File access and command execution on the engine host.
#[async_trait]
pub(crate) trait ComposeHost: Send + Sync {
    /// The whole file, refusing files larger than `max`.
    async fn read(&self, path: &str, max: u64) -> Result<Vec<u8>, HostFileError>;
    /// The path with symlinks resolved (edits replace the real file).
    async fn real_path(&self, path: &str) -> Result<String, HostFileError>;
    /// Permission bits, when the host reports them.
    async fn mode(&self, path: &str) -> Result<Option<u32>, HostFileError>;
    /// Creates `path` exclusively ([`HostFileError::Exists`] if taken, a
    /// link included) and writes `data` and the final `mode` through the one
    /// open file, never by name. Returns what identifies the new file.
    async fn create_new(
        &self,
        path: &str,
        data: &[u8],
        mode: Option<u32>,
    ) -> Result<FileStamp, HostFileError>;
    /// Whether `path` (not followed if a link) is still the regular file
    /// `stamp` describes.
    async fn verify(&self, path: &str, stamp: &FileStamp) -> Result<bool, HostFileError>;
    /// Whether users other than this one can rename or replace entries of
    /// directory `path`.
    async fn shared_dir(&self, path: &str) -> Result<bool, HostFileError>;
    /// Renames `from` over `to` (atomically where the host can).
    async fn replace(&self, from: &str, to: &str) -> Result<(), HostFileError>;
    async fn remove(&self, path: &str) -> Result<(), HostFileError>;
    /// Runs `argv` (no shell interpretation of any word), passing each
    /// output line to `on_line`.
    async fn run(&self, argv: &[String], on_line: LineSink<'_>) -> Result<RunOutput, RunError>;
}

/// What identifies a file a save created, checked again before it is
/// trusted.
pub(crate) enum FileStamp {
    /// This computer: device and inode of the created file, which stays open
    /// so the inode cannot be reused meanwhile.
    #[cfg(unix)]
    Inode {
        _file: std::fs::File,
        dev: u64,
        ino: u64,
    },
    /// Elsewhere: the SHA-256 of what was written.
    Digest(String),
}

impl fmt::Debug for FileStamp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            #[cfg(unix)]
            Self::Inode { dev, ino, .. } => write!(f, "Inode({dev}:{ino})"),
            Self::Digest(digest) => write!(f, "Digest({digest})"),
        }
    }
}

/// Whether others (root aside) can rename or replace entries of a directory
/// with permission bits `mode` owned by `owner`: group or world writable
/// (unless sticky and ours), or owned by another user. `me` unknown: only
/// the permission bits count.
pub(crate) fn shared_directory(mode: u32, owner: u32, me: Option<u32>) -> bool {
    let others_write = mode & 0o022 != 0;
    let sticky = mode & 0o1000 != 0;
    match me {
        Some(me) => (owner != me && owner != 0) || (others_write && (!sticky || owner != me)),
        None => others_write,
    }
}

/// Splits raw output into clean lines and keeps a bounded copy.
struct Lines<'a> {
    on_line: LineSink<'a>,
    output: Mutex<String>,
}

impl<'a> Lines<'a> {
    fn new(on_line: LineSink<'a>) -> Self {
        Self {
            on_line,
            output: Mutex::new(String::new()),
        }
    }

    fn line(&self, raw: &[u8]) {
        let text = strip_ansi(&String::from_utf8_lossy(raw));
        let text = text.trim_end().to_owned();
        {
            let mut output = self.output.lock().unwrap_or_else(|e| e.into_inner());
            if output.len() + text.len() < MAX_OUTPUT {
                output.push_str(&text);
                output.push('\n');
            }
        }
        (self.on_line)(text);
    }

    /// Feeds a chunk; complete lines leave `partial`.
    fn feed(&self, partial: &mut Vec<u8>, chunk: &[u8]) {
        partial.extend_from_slice(chunk);
        while let Some(index) = partial.iter().position(|b| *b == b'\n') {
            let line: Vec<u8> = partial.drain(..=index).collect();
            self.line(&line[..line.len() - 1]);
        }
        // A runaway line without a newline is flushed in pieces.
        if partial.len() > 16 * 1024 {
            let line = std::mem::take(partial);
            self.line(&line);
        }
    }

    fn finish(&self, partial: &mut Vec<u8>) {
        if !partial.is_empty() {
            let line = std::mem::take(partial);
            self.line(&line);
        }
    }

    fn take(self) -> String {
        self.output.into_inner().unwrap_or_else(|e| e.into_inner())
    }
}

fn io_error(error: &std::io::Error) -> HostFileError {
    match error.kind() {
        std::io::ErrorKind::NotFound => HostFileError::NotFound,
        std::io::ErrorKind::PermissionDenied => HostFileError::PermissionDenied,
        std::io::ErrorKind::AlreadyExists => HostFileError::Exists,
        _ => HostFileError::Other(error.kind().to_string()),
    }
}

/// This computer: files on disk, the `docker` CLI talking to the socket the
/// session uses.
#[derive(Debug)]
pub(crate) struct LocalComposeHost {
    pub socket: Option<std::path::PathBuf>,
}

impl LocalComposeHost {
    /// Extra places GUI apps miss on macOS (their PATH is minimal).
    fn extra_dirs() -> Vec<std::path::PathBuf> {
        let mut dirs = vec![
            std::path::PathBuf::from("/usr/local/bin"),
            std::path::PathBuf::from("/opt/homebrew/bin"),
            std::path::PathBuf::from("/Applications/Docker.app/Contents/Resources/bin"),
            std::path::PathBuf::from("/usr/bin"),
        ];
        if let Some(home) = std::env::var_os("HOME").map(std::path::PathBuf::from) {
            for relative in [".docker/bin", ".orbstack/bin", ".rd/bin", ".local/bin"] {
                dirs.push(home.join(relative));
            }
        }
        dirs
    }

    /// The first executable `name` on PATH or in the usual places. Symlinks
    /// stay unresolved: some runtimes ship one multi-call binary that
    /// dispatches on its invoked name.
    fn program(name: &str) -> Option<std::path::PathBuf> {
        let mut dirs: Vec<std::path::PathBuf> = std::env::var_os("PATH")
            .map(|path| std::env::split_paths(&path).collect())
            .unwrap_or_default();
        dirs.extend(Self::extra_dirs());
        dirs.into_iter()
            .filter(|dir| dir.is_absolute())
            .map(|dir| dir.join(name))
            .find(|path| {
                std::fs::metadata(path).is_ok_and(|meta| {
                    #[cfg(unix)]
                    {
                        use std::os::unix::fs::PermissionsExt;
                        meta.is_file() && meta.permissions().mode() & 0o111 != 0
                    }
                    #[cfg(not(unix))]
                    {
                        meta.is_file()
                    }
                })
            })
    }
}

#[async_trait]
impl ComposeHost for LocalComposeHost {
    async fn read(&self, path: &str, max: u64) -> Result<Vec<u8>, HostFileError> {
        let meta = tokio::fs::metadata(path).await.map_err(|e| io_error(&e))?;
        if !meta.is_file() {
            return Err(HostFileError::NotAFile);
        }
        if meta.len() > max {
            return Err(HostFileError::TooLarge);
        }
        let bytes = tokio::fs::read(path).await.map_err(|e| io_error(&e))?;
        if bytes.len() as u64 > max {
            return Err(HostFileError::TooLarge);
        }
        Ok(bytes)
    }

    async fn real_path(&self, path: &str) -> Result<String, HostFileError> {
        let real = tokio::fs::canonicalize(path)
            .await
            .map_err(|e| io_error(&e))?;
        real.to_str()
            .map(str::to_owned)
            .ok_or_else(|| HostFileError::Other("path".into()))
    }

    async fn mode(&self, path: &str) -> Result<Option<u32>, HostFileError> {
        let meta = tokio::fs::metadata(path).await.map_err(|e| io_error(&e))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            Ok(Some(meta.permissions().mode() & 0o7777))
        }
        #[cfg(not(unix))]
        {
            let _ = meta;
            Ok(None)
        }
    }

    async fn create_new(
        &self,
        path: &str,
        data: &[u8],
        mode: Option<u32>,
    ) -> Result<FileStamp, HostFileError> {
        use tokio::io::AsyncWriteExt;
        let mut options = tokio::fs::OpenOptions::new();
        // `create_new` is O_EXCL (a link is a taken name); O_NOFOLLOW too.
        options.write(true).create_new(true);
        #[cfg(unix)]
        options.mode(0o600).custom_flags(libc::O_NOFOLLOW);
        let mut file = options.open(path).await.map_err(|e| io_error(&e))?;
        let written = async {
            file.write_all(data).await?;
            #[cfg(unix)]
            if let Some(mode) = mode {
                use std::os::unix::fs::PermissionsExt;
                // fchmod on the open file, not chmod by name.
                file.set_permissions(std::fs::Permissions::from_mode(mode & 0o7777))
                    .await?;
            }
            #[cfg(not(unix))]
            let _ = mode;
            file.sync_all().await?;
            Ok::<_, std::io::Error>(())
        }
        .await;
        if let Err(error) = written {
            drop(file);
            let _ = tokio::fs::remove_file(path).await;
            return Err(io_error(&error));
        }
        let file = file.into_std().await;
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            if let Ok(meta) = file.metadata() {
                return Ok(FileStamp::Inode {
                    dev: meta.dev(),
                    ino: meta.ino(),
                    _file: file,
                });
            }
        }
        drop(file);
        Ok(FileStamp::Digest(digest(data)))
    }

    async fn verify(&self, path: &str, stamp: &FileStamp) -> Result<bool, HostFileError> {
        let meta = match tokio::fs::symlink_metadata(path).await {
            Ok(meta) => meta,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
            Err(error) => return Err(io_error(&error)),
        };
        if !meta.is_file() {
            return Ok(false);
        }
        match stamp {
            #[cfg(unix)]
            FileStamp::Inode { dev, ino, .. } => {
                use std::os::unix::fs::MetadataExt;
                Ok(meta.dev() == *dev && meta.ino() == *ino)
            }
            FileStamp::Digest(expected) => {
                let mut bytes = self.read(path, MAX_FILE).await?;
                let same = digest(&bytes) == *expected;
                bytes.zeroize();
                Ok(same)
            }
        }
    }

    async fn shared_dir(&self, path: &str) -> Result<bool, HostFileError> {
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            let meta = tokio::fs::metadata(path).await.map_err(|e| io_error(&e))?;
            #[allow(unsafe_code)]
            // SAFETY: geteuid has no preconditions and cannot fail.
            let me = unsafe { libc::geteuid() };
            Ok(shared_directory(meta.mode() & 0o7777, meta.uid(), Some(me)))
        }
        #[cfg(not(unix))]
        {
            let _ = path;
            Ok(false)
        }
    }

    async fn replace(&self, from: &str, to: &str) -> Result<(), HostFileError> {
        tokio::fs::rename(from, to).await.map_err(|e| io_error(&e))
    }

    async fn remove(&self, path: &str) -> Result<(), HostFileError> {
        tokio::fs::remove_file(path).await.map_err(|e| io_error(&e))
    }

    async fn run(&self, argv: &[String], on_line: LineSink<'_>) -> Result<RunOutput, RunError> {
        use tokio::io::AsyncReadExt;
        let (name, args) = argv.split_first().ok_or(RunError::NotFound)?;
        let program = Self::program(name).ok_or(RunError::NotFound)?;
        let mut command = tokio::process::Command::new(&program);
        command
            .args(args)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .kill_on_drop(true);
        if let Some(socket) = &self.socket {
            command.env("DOCKER_HOST", format!("unix://{}", socket.display()));
        }
        // Credential helpers and CLI plugins are looked up on PATH.
        let mut path: Vec<std::path::PathBuf> =
            program.parent().map(Into::into).into_iter().collect();
        if let Some(current) = std::env::var_os("PATH") {
            path.extend(std::env::split_paths(&current));
        }
        path.extend(Self::extra_dirs());
        if let Ok(joined) = std::env::join_paths(path) {
            command.env("PATH", joined);
        }
        let mut child = command.spawn().map_err(|e| match e.kind() {
            std::io::ErrorKind::NotFound => RunError::NotFound,
            _ => RunError::Failed(e.kind().to_string()),
        })?;
        let lines = Lines::new(on_line);
        let mut stdout = child.stdout.take();
        let mut stderr = child.stderr.take();
        let pump = |stream: Option<Box<dyn tokio::io::AsyncRead + Unpin + Send>>| {
            let lines = &lines;
            async move {
                let Some(mut stream) = stream else { return };
                let mut partial = Vec::new();
                let mut buffer = vec![0u8; 8192];
                loop {
                    match stream.read(&mut buffer).await {
                        Ok(0) | Err(_) => break,
                        Ok(n) => lines.feed(&mut partial, &buffer[..n]),
                    }
                }
                lines.finish(&mut partial);
            }
        };
        tokio::join!(
            pump(stdout.take().map(|s| Box::new(s) as _)),
            pump(stderr.take().map(|s| Box::new(s) as _)),
        );
        let status = child
            .wait()
            .await
            .map_err(|e| RunError::Failed(e.kind().to_string()))?;
        Ok(RunOutput {
            exit: status.code(),
            output: lines.take(),
        })
    }
}

fn sftp_error(error: SftpError) -> HostFileError {
    match error {
        SftpError::NotFound(_) => HostFileError::NotFound,
        SftpError::PermissionDenied(_) => HostFileError::PermissionDenied,
        SftpError::AlreadyExists(_) => HostFileError::Exists,
        SftpError::Remote { message, .. } => HostFileError::Other(message),
        other => HostFileError::Other(other.to_string()),
    }
}

/// A saved SSH host: SFTP over the Docker session's connection, commands
/// through an exec channel with every word shell-quoted.
pub(crate) struct SshComposeHost {
    session: Arc<SshSession>,
    sftp: tokio::sync::OnceCell<SftpClient>,
    /// `id -u` of the SSH user, asked once.
    uid: tokio::sync::OnceCell<Option<u32>>,
}

impl fmt::Debug for SshComposeHost {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SshComposeHost").finish_non_exhaustive()
    }
}

impl SshComposeHost {
    pub(crate) fn new(session: Arc<SshSession>) -> Self {
        Self {
            session,
            sftp: tokio::sync::OnceCell::new(),
            uid: tokio::sync::OnceCell::new(),
        }
    }

    async fn sftp(&self) -> Result<&SftpClient, HostFileError> {
        self.sftp
            .get_or_try_init(|| SftpClient::open(&self.session))
            .await
            .map_err(|error| match error {
                SftpError::Ssh(_) => HostFileError::Other("sftp_unavailable".into()),
                other => sftp_error(other),
            })
    }
}

impl Drop for SshComposeHost {
    fn drop(&mut self) {
        if let Some(sftp) = self.sftp.take() {
            tokio::spawn(async move {
                let _ = sftp.close().await;
            });
        }
    }
}

/// File operations over SFTP, shared by [`SshComposeHost`] and tests.
mod sftp_files {
    use super::*;

    pub(crate) async fn read(
        sftp: &SftpClient,
        path: &str,
        max: u64,
    ) -> Result<Vec<u8>, HostFileError> {
        let entry = sftp.stat(path).await.map_err(sftp_error)?;
        if entry.kind != EntryKind::File {
            return Err(HostFileError::NotAFile);
        }
        if entry.size > max {
            return Err(HostFileError::TooLarge);
        }
        let (bytes, total) = sftp.read_head(path, max + 1).await.map_err(sftp_error)?;
        if total > max || bytes.len() as u64 > max {
            return Err(HostFileError::TooLarge);
        }
        Ok(bytes)
    }

    /// One open (`CREATE | EXCLUDE | WRITE`), then write, `fsetstat`,
    /// `fsync` and close on that handle.
    pub(crate) async fn create_new(
        sftp: &SftpClient,
        path: &str,
        data: &[u8],
        mode: Option<u32>,
    ) -> Result<FileStamp, HostFileError> {
        sftp.create_exclusive_with(path, mode.unwrap_or(0o600), data)
            .await
            .map_err(sftp_error)?;
        Ok(FileStamp::Digest(digest(data)))
    }

    /// SFTP has no inodes: a regular file (lstat) whose content hashes to
    /// what was written.
    pub(crate) async fn verify(
        sftp: &SftpClient,
        path: &str,
        stamp: &FileStamp,
    ) -> Result<bool, HostFileError> {
        let FileStamp::Digest(expected) = stamp else {
            return Ok(false);
        };
        match sftp.lstat(path).await {
            Ok(entry) if entry.kind == EntryKind::File => {}
            Ok(_) | Err(SftpError::NotFound(_)) => return Ok(false),
            Err(error) => return Err(sftp_error(error)),
        }
        let mut bytes = match read(sftp, path, MAX_FILE).await {
            Ok(bytes) => bytes,
            Err(HostFileError::NotFound | HostFileError::TooLarge | HostFileError::NotAFile) => {
                return Ok(false)
            }
            Err(error) => return Err(error),
        };
        let same = digest(&bytes) == *expected;
        bytes.zeroize();
        Ok(same)
    }

    pub(crate) async fn replace(
        sftp: &SftpClient,
        from: &str,
        to: &str,
    ) -> Result<(), HostFileError> {
        if sftp.supports_posix_rename() {
            return sftp.posix_rename(from, to).await.map_err(sftp_error);
        }
        // SFTP v3 rename refuses an existing target; the backup is already
        // in place and the result is verified, so remove-then-rename is safe
        // enough here.
        sftp.remove_file(to).await.map_err(sftp_error)?;
        sftp.rename(from, to).await.map_err(sftp_error)
    }

    pub(crate) async fn shared_dir(
        sftp: &SftpClient,
        path: &str,
        me: Option<u32>,
    ) -> Result<bool, HostFileError> {
        let entry = sftp.stat(path).await.map_err(sftp_error)?;
        Ok(match (entry.permissions, entry.uid) {
            (Some(mode), Some(owner)) => shared_directory(mode, owner, me),
            (Some(mode), None) => shared_directory(mode, 0, None),
            _ => false,
        })
    }
}

#[async_trait]
impl ComposeHost for SshComposeHost {
    async fn read(&self, path: &str, max: u64) -> Result<Vec<u8>, HostFileError> {
        sftp_files::read(self.sftp().await?, path, max).await
    }

    async fn real_path(&self, path: &str) -> Result<String, HostFileError> {
        self.sftp()
            .await?
            .canonicalize(path)
            .await
            .map_err(sftp_error)
    }

    async fn mode(&self, path: &str) -> Result<Option<u32>, HostFileError> {
        Ok(self
            .sftp()
            .await?
            .stat(path)
            .await
            .map_err(sftp_error)?
            .permissions)
    }

    async fn create_new(
        &self,
        path: &str,
        data: &[u8],
        mode: Option<u32>,
    ) -> Result<FileStamp, HostFileError> {
        sftp_files::create_new(self.sftp().await?, path, data, mode).await
    }

    async fn verify(&self, path: &str, stamp: &FileStamp) -> Result<bool, HostFileError> {
        sftp_files::verify(self.sftp().await?, path, stamp).await
    }

    async fn shared_dir(&self, path: &str) -> Result<bool, HostFileError> {
        let me = *self
            .uid
            .get_or_init(|| async {
                let output = self.run(&["id".into(), "-u".into()], &|_| {}).await.ok()?;
                output.output.trim().parse().ok()
            })
            .await;
        sftp_files::shared_dir(self.sftp().await?, path, me).await
    }

    async fn replace(&self, from: &str, to: &str) -> Result<(), HostFileError> {
        sftp_files::replace(self.sftp().await?, from, to).await
    }

    async fn remove(&self, path: &str) -> Result<(), HostFileError> {
        self.sftp()
            .await?
            .remove_file(path)
            .await
            .map_err(sftp_error)
    }

    async fn run(&self, argv: &[String], on_line: LineSink<'_>) -> Result<RunOutput, RunError> {
        use cc_ssh_core::russh::ChannelMsg;
        if argv.is_empty() {
            return Err(RunError::NotFound);
        }
        let command = compose::shell_command(argv);
        let mut channel = self
            .session
            .open_exec(&command)
            .await
            .map_err(|e| RunError::Failed(e.to_string()))?;
        let lines = Lines::new(on_line);
        let (mut out, mut err) = (Vec::new(), Vec::new());
        let mut exit = None;
        while let Some(message) = channel.wait().await {
            match message {
                ChannelMsg::Data { data } => lines.feed(&mut out, &data),
                ChannelMsg::ExtendedData { data, .. } => lines.feed(&mut err, &data),
                ChannelMsg::ExitStatus { exit_status } => {
                    exit = Some(i32::try_from(exit_status).unwrap_or(i32::MAX))
                }
                ChannelMsg::Failure => return Err(RunError::Failed("exec refused".into())),
                ChannelMsg::Close => break,
                _ => {}
            }
        }
        lines.finish(&mut out);
        lines.finish(&mut err);
        // The shell's "command not found".
        if exit == Some(127) {
            return Err(RunError::NotFound);
        }
        Ok(RunOutput {
            exit,
            output: lines.take(),
        })
    }
}

/// One compose file as read on the host.
#[derive(Clone, Serialize)]
pub struct ComposeFileDto {
    pub path: String,
    /// UTF-8 text; secret (may hold passwords).
    pub content: Option<String>,
    /// SHA-256 of the bytes read; a save refuses when the file changed.
    pub digest: Option<String>,
    pub size: u64,
    /// `not_found`, `permission_denied`, `too_large`, `not_a_file`,
    /// `not_text` or `io_error`.
    pub error: Option<&'static str>,
    /// `content` has secrets hidden (the full text needs the passphrase).
    pub masked: bool,
}

impl fmt::Debug for ComposeFileDto {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ComposeFileDto")
            .field("path", &self.path)
            .field("size", &self.size)
            .field("error", &self.error)
            .field("masked", &self.masked)
            .finish_non_exhaustive()
    }
}

impl Drop for ComposeFileDto {
    fn drop(&mut self) {
        if let Some(content) = &mut self.content {
            content.zeroize();
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct ComposeProjectDto {
    pub project: String,
    pub working_dir: String,
    pub env_files: Vec<String>,
    /// `local` or `ssh`.
    pub host: &'static str,
    pub files: Vec<ComposeFileDto>,
}

/// A changed file to save: the new text and the digest of the text it was
/// edited from.
#[derive(Deserialize)]
pub struct ComposeEdit {
    pub path: String,
    pub content: String,
    pub digest: String,
}

impl fmt::Debug for ComposeEdit {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ComposeEdit")
            .field("path", &self.path)
            .finish_non_exhaustive()
    }
}

impl Drop for ComposeEdit {
    fn drop(&mut self) {
        self.content.zeroize();
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum ComposeSaveOutcome {
    /// Written (originals kept as `backups`); `job_id` runs `up -d`.
    Saved {
        backups: Vec<String>,
        job_id: Option<String>,
    },
    /// `config -q` refused the candidate; nothing was written.
    Invalid { output: String },
    /// The file changed on the host since it was opened.
    Conflict { path: String },
    /// The file, a backup or the candidate could not be written.
    WriteFailed { path: String, reason: &'static str },
    /// Neither `docker compose` nor `docker-compose` runs on the host.
    ComposeMissing,
    /// The validation command could not run (timeout, channel refused).
    RunFailed { detail: String },
    /// The edit permission expired: ask for the passphrase again.
    Locked,
    /// A file this save created was replaced or changed on the host before
    /// it was used (`compose_tampered`). With `backup` the original had
    /// already been replaced; `restored` says whether it was put back.
    Tampered {
        path: String,
        backup: Option<String>,
        restored: bool,
    },
    /// Other users can write to these directories (temporary files and
    /// backups go there); save again only with the user's consent.
    UnsafeDirectory { dirs: Vec<String> },
}

#[derive(Debug, Clone, Serialize)]
pub struct ComposeJobUpdate {
    pub lines: Vec<String>,
    pub done: bool,
    pub exit_code: Option<i32>,
    /// `timeout`, `cancelled`, `compose_missing` or `run_failed`.
    pub error: Option<&'static str>,
}

fn digest(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// Reads every file of the project; failures are per file.
pub(crate) async fn read_project(
    host: &dyn ComposeHost,
    location: &ComposeLocation,
    kind: &'static str,
) -> ComposeProjectDto {
    let mut files = Vec::new();
    for path in &location.config_files {
        let mut file = ComposeFileDto {
            path: path.clone(),
            content: None,
            digest: None,
            size: 0,
            error: None,
            masked: false,
        };
        match host.read(path, MAX_FILE).await {
            Ok(bytes) => {
                file.size = bytes.len() as u64;
                file.digest = Some(digest(&bytes));
                match String::from_utf8(bytes) {
                    Ok(text) => file.content = Some(text),
                    Err(error) => {
                        error.into_bytes().zeroize();
                        file.digest = None;
                        file.error = Some("not_text");
                    }
                }
            }
            Err(HostFileError::Other(_)) => file.error = Some("io_error"),
            Err(error) => file.error = Some(error.code()),
        }
        files.push(file);
    }
    ComposeProjectDto {
        project: location.project.clone(),
        working_dir: location.working_dir.clone(),
        env_files: location.env_files.clone(),
        host: kind,
        files,
    }
}

/// Hides secrets in every file's text (`cc_docker_core::mask_compose`);
/// digests stay those of the files as read.
pub(crate) fn mask_project(project: &mut ComposeProjectDto) {
    for file in &mut project.files {
        if let Some(content) = &mut file.content {
            let (masked, found) = cc_docker_core::mask_compose(content);
            if found {
                content.zeroize();
                *content = masked;
                file.masked = true;
            }
        }
    }
}

/// 128 random bits as hex: candidate names nobody can guess.
pub(crate) fn random_tag() -> AppResult<String> {
    let mut bytes = [0u8; 16];
    cc_crypto_core::fill_random(&mut bytes)?;
    Ok(bytes.iter().map(|byte| format!("{byte:02x}")).collect())
}

/// `docker compose` when the plugin answers, else `docker-compose`.
pub(crate) async fn detect_cli(host: &dyn ComposeHost) -> Option<ComposeCli> {
    for cli in [ComposeCli::Plugin, ComposeCli::Standalone] {
        let mut argv = cli.program();
        argv.push("version".into());
        if let Ok(Ok(output)) = tokio::time::timeout(DETECT_TIMEOUT, host.run(&argv, &|_| {})).await
        {
            if output.success() {
                return Some(cli);
            }
        }
    }
    None
}

/// What [`save`] did.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Saved {
    Written {
        backups: Vec<String>,
        cli: ComposeCli,
    },
    Rejected(ComposeSaveOutcome),
}

async fn remove_all(host: &dyn ComposeHost, paths: &[String]) {
    for path in paths {
        let _ = host.remove(path).await;
    }
}

/// One edited file: the original as it is now (unchanged since opened,
/// resolved through symlinks, with its permission bits) and where its
/// candidate goes.
struct Target {
    real: String,
    original: Vec<u8>,
    mode: Option<u32>,
    candidate: String,
}

impl Drop for Target {
    fn drop(&mut self) {
        self.original.zeroize();
    }
}

/// Puts the original text (what the backup holds) back at `target.real`
/// after a foreign file took its place. Another swap can win this rename
/// too, so it is tried a few times, each with a fresh candidate checked
/// after the rename.
async fn restore(host: &dyn ComposeHost, target: &Target, tag: &str) -> bool {
    for attempt in 0..3 {
        let candidate = candidate_path(&target.real, &format!("{tag}-restore{attempt}"));
        let Ok(stamp) = host
            .create_new(&candidate, &target.original, target.mode)
            .await
        else {
            continue;
        };
        if host.replace(&candidate, &target.real).await.is_err() {
            let _ = host.remove(&candidate).await;
            continue;
        }
        if host.verify(&target.real, &stamp).await.unwrap_or(false) {
            return true;
        }
    }
    false
}

/// The first edit whose candidate is no longer the file this save wrote.
async fn swapped<'a>(
    host: &dyn ComposeHost,
    edits: &'a [ComposeEdit],
    targets: &[Target],
    stamps: &[FileStamp],
) -> Option<&'a ComposeEdit> {
    for ((edit, target), stamp) in edits.iter().zip(targets).zip(stamps) {
        if !host.verify(&target.candidate, stamp).await.unwrap_or(false) {
            return Some(edit);
        }
    }
    None
}

/// Validate-then-write: candidates next to the originals, `config -q` with
/// them standing in, then a timestamped backup of each original and a
/// rename over it. Nothing is replaced unless validation passed, and a
/// candidate is used only while it is still the file this save wrote
/// (checked before and after validation and around the rename; a foreign
/// file renamed in is replaced by the original again). Directories other
/// users can write to need `allow_shared_dirs`.
pub(crate) async fn save(
    host: &dyn ComposeHost,
    location: &ComposeLocation,
    edits: &[ComposeEdit],
    stamp: &str,
    tag: &str,
    allow_shared_dirs: bool,
) -> AppResult<Saved> {
    if edits.is_empty() {
        return Err(AppError::invalid("docker", "compose_nothing_to_save"));
    }
    for (index, edit) in edits.iter().enumerate() {
        if !location.config_files.contains(&edit.path)
            || edits[..index].iter().any(|other| other.path == edit.path)
        {
            return Err(AppError::invalid("docker", "compose_file_unknown"));
        }
        if edit.content.len() as u64 > MAX_FILE {
            return Err(AppError::invalid("docker", "compose_file_too_large"));
        }
    }
    let rejected = |outcome| Ok(Saved::Rejected(outcome));
    let Some(cli) = detect_cli(host).await else {
        return rejected(ComposeSaveOutcome::ComposeMissing);
    };
    let mut targets = Vec::new();
    for edit in edits {
        let failed = |error: HostFileError| {
            rejected(ComposeSaveOutcome::WriteFailed {
                path: edit.path.clone(),
                reason: error.code(),
            })
        };
        let real = match host.real_path(&edit.path).await {
            Ok(real) if compose::valid_host_path(&real) => real,
            Ok(_) => return failed(HostFileError::NotAFile),
            Err(error) => return failed(error),
        };
        let original = match host.read(&real, MAX_FILE).await {
            Ok(bytes) => bytes,
            Err(error) => return failed(error),
        };
        if digest(&original) != edit.digest {
            return rejected(ComposeSaveOutcome::Conflict {
                path: edit.path.clone(),
            });
        }
        let mode = host.mode(&real).await.ok().flatten();
        let candidate = candidate_path(&real, tag);
        targets.push(Target {
            real,
            original,
            mode,
            candidate,
        });
    }
    if !allow_shared_dirs {
        let dirs = targets
            .iter()
            .map(|target| compose::parent(&target.real).to_owned())
            .chain([location.working_dir.clone()]);
        let mut checked: Vec<String> = Vec::new();
        let mut shared = Vec::new();
        for dir in dirs {
            if checked.contains(&dir) {
                continue;
            }
            if host.shared_dir(&dir).await.unwrap_or(false) {
                shared.push(dir.clone());
            }
            checked.push(dir);
        }
        if !shared.is_empty() {
            return rejected(ComposeSaveOutcome::UnsafeDirectory { dirs: shared });
        }
    }
    let tampered = |path: &str, backup: Option<String>, restored: bool| {
        rejected(ComposeSaveOutcome::Tampered {
            path: path.to_owned(),
            backup,
            restored,
        })
    };
    let mut written = Vec::new();
    let mut stamps = Vec::new();
    for (edit, target) in edits.iter().zip(&targets) {
        match host
            .create_new(&target.candidate, edit.content.as_bytes(), target.mode)
            .await
        {
            Ok(stamp) => {
                written.push(target.candidate.clone());
                stamps.push(stamp);
            }
            Err(error) => {
                remove_all(host, &written).await;
                return rejected(ComposeSaveOutcome::WriteFailed {
                    path: edit.path.clone(),
                    reason: error.code(),
                });
            }
        }
    }
    // Validation must see exactly what this save wrote.
    if let Some(edit) = swapped(host, edits, &targets, &stamps).await {
        remove_all(host, &written).await;
        return tampered(&edit.path, None, false);
    }
    let files: Vec<String> = location
        .config_files
        .iter()
        .map(|file| {
            edits
                .iter()
                .zip(&targets)
                .find(|(edit, _)| &edit.path == file)
                .map_or_else(|| file.clone(), |(_, target)| target.candidate.clone())
        })
        .collect();
    let argv = compose_argv(cli, location, &files, &["config", "-q"]);
    let validated = tokio::time::timeout(VALIDATE_TIMEOUT, host.run(&argv, &|_| {})).await;
    let failure = match validated {
        Ok(Ok(output)) if output.success() => None,
        Ok(Ok(output)) => {
            // Name the user's files, not the hidden candidates.
            let mut text = output.output;
            for (edit, target) in edits.iter().zip(&targets) {
                text = text.replace(&target.candidate, &edit.path);
            }
            Some(ComposeSaveOutcome::Invalid {
                output: text.trim().to_owned(),
            })
        }
        Ok(Err(RunError::NotFound)) => Some(ComposeSaveOutcome::ComposeMissing),
        Ok(Err(RunError::Failed(detail))) => Some(ComposeSaveOutcome::RunFailed { detail }),
        Err(_) => Some(ComposeSaveOutcome::RunFailed {
            detail: "timeout".into(),
        }),
    };
    if let Some(outcome) = failure {
        remove_all(host, &written).await;
        return rejected(outcome);
    }
    if let Some(edit) = swapped(host, edits, &targets, &stamps).await {
        remove_all(host, &written).await;
        return tampered(&edit.path, None, false);
    }
    let mut backups = Vec::new();
    for (edit, target) in edits.iter().zip(&targets) {
        let mut saved = None;
        for attempt in 0..20 {
            let backup = backup_path(&target.real, stamp, attempt);
            match host
                .create_new(&backup, &target.original, target.mode)
                .await
            {
                Ok(_) => {
                    saved = Some(Ok(backup));
                    break;
                }
                Err(HostFileError::Exists) => continue,
                Err(error) => {
                    saved = Some(Err(error));
                    break;
                }
            }
        }
        match saved {
            Some(Ok(backup)) => backups.push(backup),
            Some(Err(error)) => {
                remove_all(host, &written).await;
                return rejected(ComposeSaveOutcome::WriteFailed {
                    path: edit.path.clone(),
                    reason: error.code(),
                });
            }
            None => {
                remove_all(host, &written).await;
                return rejected(ComposeSaveOutcome::WriteFailed {
                    path: edit.path.clone(),
                    reason: HostFileError::Exists.code(),
                });
            }
        }
    }
    for (index, ((edit, target), stamp)) in edits.iter().zip(&targets).zip(&stamps).enumerate() {
        if !host.verify(&target.candidate, stamp).await.unwrap_or(false) {
            remove_all(host, &written[index..]).await;
            return tampered(&edit.path, None, false);
        }
        if let Err(error) = host.replace(&target.candidate, &target.real).await {
            remove_all(host, &written[index..]).await;
            return rejected(ComposeSaveOutcome::WriteFailed {
                path: edit.path.clone(),
                reason: error.code(),
            });
        }
        // The rename moved whatever had the candidate's name by then.
        if !host.verify(&target.real, stamp).await.unwrap_or(false) {
            let restored = restore(host, target, tag).await;
            remove_all(host, &written[index + 1..]).await;
            return tampered(&edit.path, Some(backups[index].clone()), restored);
        }
    }
    Ok(Saved::Written { backups, cli })
}

struct Grant {
    session_id: String,
    project: String,
    expires: Instant,
}

struct JobEnd {
    exit_code: Option<i32>,
    error: Option<&'static str>,
    at: Instant,
}

pub(crate) struct ComposeJob {
    session_id: String,
    lines: Mutex<VecDeque<String>>,
    end: Mutex<Option<JobEnd>>,
    notify: Notify,
}

impl ComposeJob {
    fn push(&self, line: String) {
        let mut lines = self.lines.lock().unwrap_or_else(|e| e.into_inner());
        if lines.len() >= MAX_JOB_LINES {
            lines.pop_front();
        }
        lines.push_back(line);
        drop(lines);
        self.notify.notify_waiters();
    }

    fn finish(&self, exit_code: Option<i32>, error: Option<&'static str>) {
        *self.end.lock().unwrap_or_else(|e| e.into_inner()) = Some(JobEnd {
            exit_code,
            error,
            at: Instant::now(),
        });
        self.notify.notify_waiters();
    }
}

/// Edit grants and running `up` jobs; dropped on lock.
#[derive(Default)]
pub(crate) struct ComposeState {
    grants: Mutex<HashMap<String, Grant>>,
    jobs: Mutex<HashMap<String, Arc<ComposeJob>>>,
}

impl ComposeState {
    pub(crate) fn close(&self) {
        self.grants
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clear();
        self.jobs.lock().unwrap_or_else(|e| e.into_inner()).clear();
    }

    /// Forgets grants and finished jobs of a closed session.
    pub(crate) fn forget_session(&self, session_id: &str) {
        self.grants
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .retain(|_, grant| grant.session_id != session_id);
        self.jobs
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .retain(|_, job| job.session_id != session_id);
    }

    fn grant(&self, session_id: &str, project: &str) -> String {
        let mut grants = self.grants.lock().unwrap_or_else(|e| e.into_inner());
        let now = Instant::now();
        grants.retain(|_, grant| grant.expires > now);
        if grants.len() >= MAX_GRANTS {
            if let Some(oldest) = grants
                .iter()
                .min_by_key(|(_, grant)| grant.expires)
                .map(|(id, _)| id.clone())
            {
                grants.remove(&oldest);
            }
        }
        let id = uuid::Uuid::new_v4().to_string();
        grants.insert(
            id.clone(),
            Grant {
                session_id: session_id.to_owned(),
                project: project.to_owned(),
                expires: now + GRANT_TTL,
            },
        );
        id
    }

    /// Whether `grant` allows editing `project` of `session_id`; renews it.
    fn check(&self, grant: &str, session_id: &str, project: &str) -> bool {
        let mut grants = self.grants.lock().unwrap_or_else(|e| e.into_inner());
        let now = Instant::now();
        match grants.get_mut(grant) {
            Some(entry)
                if entry.expires > now
                    && entry.session_id == session_id
                    && entry.project == project =>
            {
                entry.expires = now + GRANT_TTL;
                true
            }
            _ => false,
        }
    }

    fn start_job(&self, session_id: &str) -> AppResult<(String, Arc<ComposeJob>)> {
        let mut jobs = self.jobs.lock().unwrap_or_else(|e| e.into_inner());
        jobs.retain(|_, job| {
            job.end
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .as_ref()
                .is_none_or(|end| end.at.elapsed() < JOB_RETENTION)
        });
        if jobs.len() >= MAX_JOBS {
            return Err(AppError::invalid("docker", "too_many_compose_jobs"));
        }
        let id = uuid::Uuid::new_v4().to_string();
        let job = Arc::new(ComposeJob {
            session_id: session_id.to_owned(),
            lines: Mutex::new(VecDeque::new()),
            end: Mutex::new(None),
            notify: Notify::new(),
        });
        jobs.insert(id.clone(), job.clone());
        Ok((id, job))
    }

    fn job(&self, id: &str) -> AppResult<Arc<ComposeJob>> {
        self.jobs
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(id)
            .cloned()
            .ok_or_else(|| AppError::invalid("docker", "compose_job_closed"))
    }
}

/// Runs `up -d` in the background; output lands in the job.
fn spawn_apply(
    host: Arc<dyn ComposeHost>,
    argv: Vec<String>,
    job: Arc<ComposeJob>,
    cancel: CancellationToken,
) {
    tokio::spawn(async move {
        let sink = {
            let job = job.clone();
            move |line: String| job.push(line)
        };
        let run = tokio::time::timeout(APPLY_TIMEOUT, host.run(&argv, &sink));
        let result = tokio::select! {
            _ = cancel.cancelled() => None,
            result = run => Some(result),
        };
        match result {
            None => job.finish(None, Some("cancelled")),
            Some(Err(_)) => job.finish(None, Some("timeout")),
            Some(Ok(Err(RunError::NotFound))) => job.finish(None, Some("compose_missing")),
            Some(Ok(Err(RunError::Failed(_)))) => job.finish(None, Some("run_failed")),
            Some(Ok(Ok(output))) => job.finish(output.exit, None),
        }
    });
}

fn locate_error(error: ComposeLocateError) -> AppError {
    AppError::invalid(
        "docker",
        match error {
            ComposeLocateError::NoContainers => "compose_project_not_found",
            ComposeLocateError::FilesUnknown => "compose_files_unknown",
            ComposeLocateError::InvalidPath => "compose_files_invalid",
        },
    )
}

impl AppCore {
    async fn compose_context(
        &self,
        session_id: &str,
        project: &str,
    ) -> AppResult<(
        ComposeLocation,
        Arc<dyn ComposeHost>,
        &'static str,
        CancellationToken,
    )> {
        if !compose::valid_project_name(project) {
            return Err(AppError::invalid("docker", "compose_project_invalid"));
        }
        let (_, unlocked) = self.unlocked().await?;
        let target = unlocked.docker.compose_target(session_id)?;
        let labels = target
            .client
            .compose_labels(project)
            .await
            .map_err(|e| AppError::invalid("docker", e.code()))?;
        let location = compose::locate(project, &labels).map_err(locate_error)?;
        let (host, kind): (Arc<dyn ComposeHost>, _) = match target.ssh {
            Some(session) => (Arc::new(SshComposeHost::new(session)), "ssh"),
            None => (
                Arc::new(LocalComposeHost {
                    socket: target.local_socket,
                }),
                "local",
            ),
        };
        Ok((location, host, kind, target.cancel))
    }

    /// The project's compose files, read on the engine host, with secrets
    /// hidden (see [`Self::docker_compose_reveal`]).
    pub async fn docker_compose_project(
        &self,
        session_id: String,
        project: String,
    ) -> AppResult<ComposeProjectDto> {
        let (location, host, kind, _) = self.compose_context(&session_id, &project).await?;
        let mut files = read_project(host.as_ref(), &location, kind).await;
        mask_project(&mut files);
        Ok(files)
    }

    /// The full texts under a grant from [`Self::docker_compose_unlock`];
    /// `None` once the grant expired (ask for the passphrase again).
    pub async fn docker_compose_reveal(
        &self,
        session_id: String,
        project: String,
        grant: String,
    ) -> AppResult<Option<ComposeProjectDto>> {
        let (_, unlocked) = self.unlocked().await?;
        if !unlocked.docker.compose.check(&grant, &session_id, &project) {
            return Ok(None);
        }
        let (location, host, kind, _) = self.compose_context(&session_id, &project).await?;
        Ok(Some(read_project(host.as_ref(), &location, kind).await))
    }

    /// Checks the vault passphrase and returns a grant to edit the project.
    pub async fn docker_compose_unlock(
        &self,
        session_id: String,
        project: String,
        passphrase: String,
    ) -> AppResult<String> {
        if !compose::valid_project_name(&project) {
            return Err(AppError::invalid("docker", "compose_project_invalid"));
        }
        let (_, unlocked) = self.unlocked().await?;
        unlocked.docker.compose_target(&session_id)?;
        if !self.verify_passphrase(passphrase).await? {
            return Err(AppError::WrongPassphrase);
        }
        Ok(unlocked.docker.compose.grant(&session_id, &project))
    }

    /// Validates and writes the edited files; with `apply` starts `up -d`.
    /// `allow_shared_dirs` is the user's consent to save in directories
    /// other users can write to ([`ComposeSaveOutcome::UnsafeDirectory`]).
    pub async fn docker_compose_save(
        &self,
        session_id: String,
        project: String,
        grant: String,
        edits: Vec<ComposeEdit>,
        apply: bool,
        allow_shared_dirs: bool,
    ) -> AppResult<ComposeSaveOutcome> {
        let (_, unlocked) = self.unlocked().await?;
        if !unlocked.docker.compose.check(&grant, &session_id, &project) {
            return Ok(ComposeSaveOutcome::Locked);
        }
        let (location, host, _, cancel) = self.compose_context(&session_id, &project).await?;
        let stamp = chrono::Local::now().format("%Y%m%d-%H%M%S").to_string();
        let tag = random_tag()?;
        let saved = save(
            host.as_ref(),
            &location,
            &edits,
            &stamp,
            &tag,
            allow_shared_dirs,
        )
        .await?;
        let (backups, cli) = match saved {
            Saved::Written { backups, cli } => (backups, cli),
            Saved::Rejected(outcome) => return Ok(outcome),
        };
        drop(edits);
        let job_id = if apply {
            let (id, job) = unlocked.docker.compose.start_job(&session_id)?;
            let argv = compose_argv(cli, &location, &location.config_files, &["up", "-d"]);
            spawn_apply(host, argv, job, cancel);
            Some(id)
        } else {
            None
        };
        Ok(ComposeSaveOutcome::Saved { backups, job_id })
    }

    /// New output of an `up` job (waits up to `wait_ms` for some).
    pub async fn docker_compose_job_poll(
        &self,
        job_id: String,
        wait_ms: u64,
    ) -> AppResult<ComposeJobUpdate> {
        let (_, unlocked) = self.unlocked().await?;
        let job = unlocked.docker.compose.job(&job_id)?;
        let notified = job.notify.notified();
        let ready = !job
            .lines
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .is_empty()
            || job.end.lock().unwrap_or_else(|e| e.into_inner()).is_some();
        if !ready {
            let _ = tokio::time::timeout(Duration::from_millis(wait_ms.min(2000)), notified).await;
        }
        let lines: Vec<String> = job
            .lines
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .drain(..)
            .collect();
        let end = job.end.lock().unwrap_or_else(|e| e.into_inner());
        let update = match end.as_ref() {
            // Report the end only once every line was delivered.
            Some(end) => ComposeJobUpdate {
                lines,
                done: true,
                exit_code: end.exit_code,
                error: end.error,
            },
            None => ComposeJobUpdate {
                lines,
                done: false,
                exit_code: None,
                error: None,
            },
        };
        drop(end);
        if update.done {
            unlocked
                .docker
                .compose
                .jobs
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .remove(&job_id);
        }
        Ok(update)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    /// In-memory host: files, a log of every operation, and a scripted
    /// `config -q` verdict.
    #[derive(Default)]
    struct FakeHost {
        files: Mutex<BTreeMap<String, (Vec<u8>, u32)>>,
        log: Mutex<Vec<String>>,
        plugin: bool,
        standalone: bool,
        /// `config -q` output when the candidate contains this marker.
        invalid_marker: Option<&'static str>,
        deny_writes_in: Option<&'static str>,
        runs: Mutex<Vec<Vec<String>>>,
        /// Directories reported as writable by other users.
        shared: Vec<&'static str>,
        /// Overwrites every candidate while `config -q` runs.
        swap_during_validation: bool,
        /// Puts foreign text in place of the original right after a rename.
        swap_after_replace: bool,
    }

    impl FakeHost {
        fn with(files: &[(&str, &str)]) -> Self {
            let host = Self {
                plugin: true,
                ..Self::default()
            };
            for (path, text) in files {
                host.files
                    .lock()
                    .unwrap()
                    .insert((*path).to_owned(), (text.as_bytes().to_vec(), 0o640));
            }
            host
        }
        fn text(&self, path: &str) -> Option<String> {
            self.files
                .lock()
                .unwrap()
                .get(path)
                .map(|(bytes, _)| String::from_utf8(bytes.clone()).unwrap())
        }
        fn log(&self) -> Vec<String> {
            self.log.lock().unwrap().clone()
        }
        fn note(&self, entry: String) {
            self.log.lock().unwrap().push(entry);
        }
    }

    #[async_trait]
    impl ComposeHost for FakeHost {
        async fn read(&self, path: &str, max: u64) -> Result<Vec<u8>, HostFileError> {
            let files = self.files.lock().unwrap();
            let (bytes, _) = files.get(path).ok_or(HostFileError::NotFound)?;
            if bytes.len() as u64 > max {
                return Err(HostFileError::TooLarge);
            }
            Ok(bytes.clone())
        }
        async fn real_path(&self, path: &str) -> Result<String, HostFileError> {
            Ok(path.to_owned())
        }
        async fn mode(&self, path: &str) -> Result<Option<u32>, HostFileError> {
            Ok(self.files.lock().unwrap().get(path).map(|(_, mode)| *mode))
        }
        async fn create_new(
            &self,
            path: &str,
            data: &[u8],
            mode: Option<u32>,
        ) -> Result<FileStamp, HostFileError> {
            if self.deny_writes_in.is_some_and(|dir| path.starts_with(dir)) {
                return Err(HostFileError::PermissionDenied);
            }
            let mut files = self.files.lock().unwrap();
            if files.contains_key(path) {
                return Err(HostFileError::Exists);
            }
            files.insert(path.to_owned(), (data.to_vec(), mode.unwrap_or(0o600)));
            drop(files);
            self.note(format!("create {path}"));
            Ok(FileStamp::Digest(digest(data)))
        }
        async fn verify(&self, path: &str, stamp: &FileStamp) -> Result<bool, HostFileError> {
            let FileStamp::Digest(expected) = stamp else {
                return Ok(false);
            };
            Ok(self
                .files
                .lock()
                .unwrap()
                .get(path)
                .is_some_and(|(bytes, _)| digest(bytes) == *expected))
        }
        async fn shared_dir(&self, path: &str) -> Result<bool, HostFileError> {
            Ok(self.shared.contains(&path))
        }
        async fn replace(&self, from: &str, to: &str) -> Result<(), HostFileError> {
            let mut files = self.files.lock().unwrap();
            let entry = files.remove(from).ok_or(HostFileError::NotFound)?;
            files.insert(to.to_owned(), entry);
            if self.swap_after_replace
                && from.contains(".consolecrypt-")
                && !from.contains("-restore")
            {
                files.insert(to.to_owned(), (b"image: evil\n".to_vec(), 0o666));
            }
            drop(files);
            self.note(format!("replace {from} -> {to}"));
            Ok(())
        }
        async fn remove(&self, path: &str) -> Result<(), HostFileError> {
            self.files.lock().unwrap().remove(path);
            self.note(format!("remove {path}"));
            Ok(())
        }
        async fn run(&self, argv: &[String], on_line: LineSink<'_>) -> Result<RunOutput, RunError> {
            self.runs.lock().unwrap().push(argv.to_vec());
            let available = match argv[0].as_str() {
                "docker" => self.plugin,
                "docker-compose" => self.standalone,
                _ => false,
            };
            if !available {
                return Err(RunError::NotFound);
            }
            if argv.last().map(String::as_str) == Some("version") {
                return Ok(RunOutput {
                    exit: Some(0),
                    output: String::new(),
                });
            }
            if argv.ends_with(&["config".to_owned(), "-q".to_owned()]) {
                self.note("validate".into());
                let mut files = self.files.lock().unwrap();
                if self.swap_during_validation {
                    for (path, (bytes, _)) in files.iter_mut() {
                        if path.contains(".consolecrypt-") {
                            *bytes = b"image: evil\n".to_vec();
                        }
                    }
                }
                for (index, word) in argv.iter().enumerate() {
                    if index > 0 && argv[index - 1] == "-f" {
                        let (bytes, _) = files.get(word).expect("validated file exists");
                        if let Some(marker) = self.invalid_marker {
                            if String::from_utf8_lossy(bytes).contains(marker) {
                                return Ok(RunOutput {
                                    exit: Some(15),
                                    output: format!(
                                        "{word}: yaml: line 3: mapping values are not allowed\n"
                                    ),
                                });
                            }
                        }
                    }
                }
                return Ok(RunOutput {
                    exit: Some(0),
                    output: String::new(),
                });
            }
            on_line("Container web  Recreated".into());
            Ok(RunOutput {
                exit: Some(0),
                output: String::new(),
            })
        }
    }

    fn location() -> ComposeLocation {
        ComposeLocation {
            project: "shop".into(),
            working_dir: "/srv/shop".into(),
            config_files: vec![
                "/srv/shop/compose.yaml".into(),
                "/srv/shop/compose.override.yaml".into(),
            ],
            env_files: vec![],
        }
    }

    const BASE: &str = "services:\n  web:\n    image: nginx\n";
    const OVERRIDE: &str = "services:\n  web:\n    ports: [\"8080:80\"]\n";

    fn edit(path: &str, content: &str, original: &str) -> ComposeEdit {
        ComposeEdit {
            path: path.into(),
            content: content.into(),
            digest: digest(original.as_bytes()),
        }
    }

    #[tokio::test]
    async fn reads_every_file_with_per_file_errors() {
        let host = FakeHost::with(&[("/srv/shop/compose.yaml", BASE)]);
        let project = read_project(&host, &location(), "ssh").await;
        assert_eq!(project.files.len(), 2);
        assert_eq!(project.files[0].content.as_deref(), Some(BASE));
        assert_eq!(project.files[0].digest, Some(digest(BASE.as_bytes())));
        assert_eq!(project.files[1].error, Some("not_found"));
        host.files.lock().unwrap().insert(
            "/srv/shop/compose.override.yaml".into(),
            (vec![0xff, 0xfe], 0o600),
        );
        let project = read_project(&host, &location(), "ssh").await;
        assert_eq!(project.files[1].error, Some("not_text"));
        assert!(project.files[1].content.is_none());
        let big = "x".repeat(MAX_FILE as usize + 1);
        host.files.lock().unwrap().insert(
            "/srv/shop/compose.override.yaml".into(),
            (big.into_bytes(), 0o600),
        );
        let project = read_project(&host, &location(), "ssh").await;
        assert_eq!(project.files[1].error, Some("too_large"));
    }

    #[tokio::test]
    async fn validates_with_the_candidate_then_backs_up_and_replaces() {
        let host = FakeHost::with(&[
            ("/srv/shop/compose.yaml", BASE),
            ("/srv/shop/compose.override.yaml", OVERRIDE),
        ]);
        let changed = "services:\n  web:\n    image: nginx:1.27\n";
        let saved = save(
            &host,
            &location(),
            &[edit("/srv/shop/compose.yaml", changed, BASE)],
            "20261021-091500",
            "t1",
            false,
        )
        .await
        .unwrap();
        assert_eq!(
            saved,
            Saved::Written {
                backups: vec!["/srv/shop/compose.yaml.bak-20261021-091500".into()],
                cli: ComposeCli::Plugin,
            }
        );
        let candidate = "/srv/shop/.compose.yaml.consolecrypt-t1.tmp";
        assert_eq!(
            host.log(),
            [
                format!("create {candidate}"),
                "validate".into(),
                "create /srv/shop/compose.yaml.bak-20261021-091500".into(),
                format!("replace {candidate} -> /srv/shop/compose.yaml"),
            ]
        );
        assert_eq!(
            host.text("/srv/shop/compose.yaml").as_deref(),
            Some(changed)
        );
        assert_eq!(
            host.text("/srv/shop/compose.yaml.bak-20261021-091500")
                .as_deref(),
            Some(BASE)
        );
        // Mode of the original is kept on the new file and the backup.
        assert_eq!(
            host.files.lock().unwrap()["/srv/shop/compose.yaml"].1,
            0o640
        );
        // Validation ran on the candidate in the original's place, in order.
        let runs = host.runs.lock().unwrap();
        let validate = runs
            .iter()
            .find(|argv| argv.ends_with(&["-q".to_owned()]))
            .unwrap();
        assert_eq!(
            validate[2..],
            [
                "-p",
                "shop",
                "-f",
                candidate,
                "-f",
                "/srv/shop/compose.override.yaml",
                "--project-directory",
                "/srv/shop",
                "config",
                "-q"
            ]
        );
    }

    #[tokio::test]
    async fn invalid_yaml_writes_nothing_and_names_the_users_file() {
        let host = FakeHost {
            invalid_marker: Some("BROKEN"),
            ..FakeHost::with(&[
                ("/srv/shop/compose.yaml", BASE),
                ("/srv/shop/compose.override.yaml", OVERRIDE),
            ])
        };
        let saved = save(
            &host,
            &location(),
            &[edit("/srv/shop/compose.yaml", "BROKEN: [", BASE)],
            "s",
            "t2",
            false,
        )
        .await
        .unwrap();
        match saved {
            Saved::Rejected(ComposeSaveOutcome::Invalid { output }) => {
                assert!(
                    output.starts_with("/srv/shop/compose.yaml: yaml: line 3"),
                    "{output}"
                );
                assert!(!output.contains("consolecrypt-t2"), "{output}");
            }
            other => panic!("{other:?}"),
        }
        assert_eq!(host.text("/srv/shop/compose.yaml").as_deref(), Some(BASE));
        let files = host.files.lock().unwrap();
        assert_eq!(files.len(), 2, "candidate removed, no backup: {files:?}");
    }

    #[tokio::test]
    async fn changed_on_host_is_a_conflict() {
        let host = FakeHost::with(&[
            ("/srv/shop/compose.yaml", BASE),
            ("/srv/shop/compose.override.yaml", OVERRIDE),
        ]);
        let saved = save(
            &host,
            &location(),
            &[edit("/srv/shop/compose.yaml", "x: 1\n", "services: {}\n")],
            "s",
            "t3",
            false,
        )
        .await
        .unwrap();
        assert_eq!(
            saved,
            Saved::Rejected(ComposeSaveOutcome::Conflict {
                path: "/srv/shop/compose.yaml".into()
            })
        );
        assert!(host.log().is_empty());
    }

    #[tokio::test]
    async fn missing_compose_and_unwritable_directories_are_reported() {
        let host = FakeHost {
            plugin: false,
            ..FakeHost::with(&[("/srv/shop/compose.yaml", BASE)])
        };
        let edits = [edit("/srv/shop/compose.yaml", "a: 1\n", BASE)];
        assert_eq!(
            save(&host, &location(), &edits, "s", "t", false)
                .await
                .unwrap(),
            Saved::Rejected(ComposeSaveOutcome::ComposeMissing)
        );
        // The standalone binary is used when the plugin is missing.
        let host = FakeHost {
            plugin: false,
            standalone: true,
            ..FakeHost::with(&[
                ("/srv/shop/compose.yaml", BASE),
                ("/srv/shop/compose.override.yaml", OVERRIDE),
            ])
        };
        assert!(matches!(
            save(&host, &location(), &edits, "s", "t", false)
                .await
                .unwrap(),
            Saved::Written {
                cli: ComposeCli::Standalone,
                ..
            }
        ));
        let host = FakeHost {
            deny_writes_in: Some("/srv/shop/"),
            ..FakeHost::with(&[("/srv/shop/compose.yaml", BASE)])
        };
        assert_eq!(
            save(&host, &location(), &edits, "s", "t", false)
                .await
                .unwrap(),
            Saved::Rejected(ComposeSaveOutcome::WriteFailed {
                path: "/srv/shop/compose.yaml".into(),
                reason: "permission_denied"
            })
        );
        assert_eq!(host.text("/srv/shop/compose.yaml").as_deref(), Some(BASE));
    }

    #[tokio::test]
    async fn existing_backup_names_get_a_counter() {
        let host = FakeHost::with(&[
            ("/srv/shop/compose.yaml", BASE),
            ("/srv/shop/compose.override.yaml", OVERRIDE),
            ("/srv/shop/compose.yaml.bak-s", "older"),
        ]);
        let saved = save(
            &host,
            &location(),
            &[edit("/srv/shop/compose.yaml", "a: 1\n", BASE)],
            "s",
            "t",
            false,
        )
        .await
        .unwrap();
        assert!(
            matches!(saved, Saved::Written { ref backups, .. } if backups == &["/srv/shop/compose.yaml.bak-s-1".to_owned()])
        );
        assert_eq!(
            host.text("/srv/shop/compose.yaml.bak-s").as_deref(),
            Some("older")
        );
    }

    #[tokio::test]
    async fn only_files_of_the_project_can_be_saved() {
        let host = FakeHost::with(&[("/srv/shop/compose.yaml", BASE)]);
        for edits in [
            vec![edit("/etc/passwd", "x", "")],
            vec![
                edit("/srv/shop/compose.yaml", "a", BASE),
                edit("/srv/shop/compose.yaml", "b", BASE),
            ],
            vec![],
        ] {
            assert!(save(&host, &location(), &edits, "s", "t", false)
                .await
                .is_err());
        }
        assert!(host.log().is_empty());
    }

    #[tokio::test]
    async fn apply_job_collects_lines_then_ends() {
        let state = ComposeState::default();
        let (_, job) = state.start_job("session").unwrap();
        let host: Arc<dyn ComposeHost> = Arc::new(FakeHost::with(&[]));
        spawn_apply(
            host,
            vec!["docker".into(), "compose".into(), "up".into(), "-d".into()],
            job.clone(),
            CancellationToken::new(),
        );
        for _ in 0..100 {
            if job.end.lock().unwrap().is_some() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        assert_eq!(
            job.lines
                .lock()
                .unwrap()
                .iter()
                .cloned()
                .collect::<Vec<_>>(),
            ["Container web  Recreated"]
        );
        let end = job.end.lock().unwrap();
        assert_eq!(end.as_ref().unwrap().exit_code, Some(0));
    }

    #[test]
    fn grants_are_bound_to_session_and_project() {
        let state = ComposeState::default();
        let grant = state.grant("s1", "shop");
        assert!(state.check(&grant, "s1", "shop"));
        assert!(!state.check(&grant, "s2", "shop"));
        assert!(!state.check(&grant, "s1", "other"));
        assert!(!state.check("forged", "s1", "shop"));
        state.forget_session("s1");
        assert!(!state.check(&grant, "s1", "shop"));
    }

    #[test]
    fn lines_are_split_across_chunks_and_cleaned() {
        let seen = Mutex::new(Vec::new());
        let sink = |line: String| seen.lock().unwrap().push(line);
        let lines = Lines::new(&sink);
        let mut partial = Vec::new();
        lines.feed(&mut partial, b"one\r\n\x1b[32mtw");
        lines.feed(&mut partial, b"o\x1b[0m\nthree");
        lines.finish(&mut partial);
        assert_eq!(*seen.lock().unwrap(), ["one", "two", "three"]);
        assert_eq!(lines.take(), "one\ntwo\nthree\n");
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn local_host_writes_exclusively_and_keeps_the_mode() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("compose.yaml");
        std::fs::write(&path, BASE).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o640)).unwrap();
        let host = LocalComposeHost { socket: None };
        let path = path.to_str().unwrap().to_owned();
        assert_eq!(host.read(&path, MAX_FILE).await.unwrap(), BASE.as_bytes());
        assert_eq!(host.read(&path, 3).await, Err(HostFileError::TooLarge));
        assert_eq!(host.mode(&path).await.unwrap(), Some(0o640));
        let other = format!("{path}.new");
        let stamp = host
            .create_new(&other, b"a: 1\n", Some(0o640))
            .await
            .unwrap();
        assert!(matches!(
            host.create_new(&other, b"b", None).await,
            Err(HostFileError::Exists)
        ));
        assert!(host.verify(&other, &stamp).await.unwrap());
        assert!(!host.verify(&path, &stamp).await.unwrap());
        assert_eq!(
            std::fs::metadata(&other).unwrap().permissions().mode() & 0o7777,
            0o640
        );
        host.replace(&other, &path).await.unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "a: 1\n");
        assert!(
            host.verify(&path, &stamp).await.unwrap(),
            "rename keeps the inode"
        );
        assert_eq!(
            host.read(&format!("{path}.missing"), MAX_FILE).await,
            Err(HostFileError::NotFound)
        );
        assert_eq!(
            host.read(dir.path().to_str().unwrap(), MAX_FILE).await,
            Err(HostFileError::NotAFile)
        );
        let seen = Mutex::new(Vec::new());
        let sink = |line: String| seen.lock().unwrap().push(line);
        assert_eq!(
            host.run(&["cc-no-such-program-xyz".into()], &sink)
                .await
                .unwrap_err(),
            RunError::NotFound
        );
    }

    #[tokio::test]
    async fn a_candidate_changed_during_validation_is_never_used() {
        let host = FakeHost {
            swap_during_validation: true,
            ..FakeHost::with(&[
                ("/srv/shop/compose.yaml", BASE),
                ("/srv/shop/compose.override.yaml", OVERRIDE),
            ])
        };
        let saved = save(
            &host,
            &location(),
            &[edit("/srv/shop/compose.yaml", "a: 1\n", BASE)],
            "s",
            "t",
            false,
        )
        .await
        .unwrap();
        assert_eq!(
            saved,
            Saved::Rejected(ComposeSaveOutcome::Tampered {
                path: "/srv/shop/compose.yaml".into(),
                backup: None,
                restored: false,
            })
        );
        assert_eq!(host.text("/srv/shop/compose.yaml").as_deref(), Some(BASE));
        assert_eq!(host.files.lock().unwrap().len(), 2, "candidate removed");
    }

    #[tokio::test]
    async fn a_foreign_file_renamed_in_is_replaced_by_the_backup() {
        let host = FakeHost {
            swap_after_replace: true,
            ..FakeHost::with(&[
                ("/srv/shop/compose.yaml", BASE),
                ("/srv/shop/compose.override.yaml", OVERRIDE),
            ])
        };
        let saved = save(
            &host,
            &location(),
            &[edit("/srv/shop/compose.yaml", "a: 1\n", BASE)],
            "s",
            "t",
            false,
        )
        .await
        .unwrap();
        assert_eq!(
            saved,
            Saved::Rejected(ComposeSaveOutcome::Tampered {
                path: "/srv/shop/compose.yaml".into(),
                backup: Some("/srv/shop/compose.yaml.bak-s".into()),
                restored: true,
            })
        );
        assert_eq!(host.text("/srv/shop/compose.yaml").as_deref(), Some(BASE));
        assert_eq!(
            host.text("/srv/shop/compose.yaml.bak-s").as_deref(),
            Some(BASE)
        );
    }

    #[tokio::test]
    async fn shared_directories_need_consent() {
        let host = FakeHost {
            shared: vec!["/srv/shop"],
            ..FakeHost::with(&[
                ("/srv/shop/compose.yaml", BASE),
                ("/srv/shop/compose.override.yaml", OVERRIDE),
            ])
        };
        let edits = [edit("/srv/shop/compose.yaml", "a: 1\n", BASE)];
        assert_eq!(
            save(&host, &location(), &edits, "s", "t", false)
                .await
                .unwrap(),
            Saved::Rejected(ComposeSaveOutcome::UnsafeDirectory {
                dirs: vec!["/srv/shop".into()]
            })
        );
        assert!(host.log().is_empty(), "nothing written before consent");
        assert!(matches!(
            save(&host, &location(), &edits, "s", "t", true)
                .await
                .unwrap(),
            Saved::Written { .. }
        ));
    }

    #[test]
    fn directories_others_can_write_to() {
        const ME: u32 = 1000;
        const OTHER: u32 = 1001;
        assert!(!shared_directory(0o755, ME, Some(ME)));
        assert!(!shared_directory(0o755, 0, Some(ME)), "root-owned is fine");
        assert!(!shared_directory(0o1777, ME, Some(ME)), "sticky and ours");
        assert!(shared_directory(0o777, ME, Some(ME)));
        assert!(shared_directory(0o775, ME, Some(ME)));
        assert!(
            shared_directory(0o1777, 0, Some(ME)),
            "sticky, others' files"
        );
        assert!(shared_directory(0o755, OTHER, Some(ME)));
        assert!(shared_directory(0o757, 0, None));
        assert!(!shared_directory(0o755, OTHER, None));
    }

    #[test]
    fn temporary_names_carry_128_random_bits() {
        let a = random_tag().unwrap();
        let b = random_tag().unwrap();
        assert_eq!(a.len(), 32);
        assert!(a.bytes().all(|c| c.is_ascii_hexdigit()));
        assert_ne!(a, b);
    }

    #[test]
    fn project_view_is_masked_with_the_original_digest() {
        let secret = "services:\n  db:\n    environment:\n      POSTGRES_PASSWORD: hunter2\n";
        let mut project = ComposeProjectDto {
            project: "shop".into(),
            working_dir: "/srv/shop".into(),
            env_files: vec![],
            host: "local",
            files: vec![
                ComposeFileDto {
                    path: "/srv/shop/compose.yaml".into(),
                    content: Some(secret.into()),
                    digest: Some(digest(secret.as_bytes())),
                    size: secret.len() as u64,
                    error: None,
                    masked: false,
                },
                ComposeFileDto {
                    path: "/srv/shop/compose.override.yaml".into(),
                    content: Some(OVERRIDE.into()),
                    digest: Some(digest(OVERRIDE.as_bytes())),
                    size: 0,
                    error: None,
                    masked: false,
                },
            ],
        };
        mask_project(&mut project);
        let file = &project.files[0];
        assert!(file.masked);
        assert_eq!(
            file.content.as_deref(),
            Some("services:\n  db:\n    environment:\n      POSTGRES_PASSWORD: ••••••\n")
        );
        assert_eq!(file.digest, Some(digest(secret.as_bytes())));
        assert!(!project.files[1].masked);
        assert_eq!(project.files[1].content.as_deref(), Some(OVERRIDE));
    }

    /// Commands for hosts whose files are real: `version` and `config -q`
    /// succeed; `on_validate` runs while the candidate is being checked.
    async fn scripted_run(argv: &[String], on_validate: &(dyn Fn() + Send + Sync)) -> RunOutput {
        if argv.ends_with(&["config".to_owned(), "-q".to_owned()]) {
            on_validate();
        }
        RunOutput {
            exit: Some(0),
            output: String::new(),
        }
    }

    /// The SFTP file operations of the SSH host against the in-process
    /// server over a temporary directory.
    struct SftpHost {
        sftp: SftpClient,
        root: std::path::PathBuf,
        on_validate: Box<dyn Fn() + Send + Sync>,
        on_replace: Box<dyn Fn() + Send + Sync>,
    }

    #[async_trait]
    impl ComposeHost for SftpHost {
        async fn read(&self, path: &str, max: u64) -> Result<Vec<u8>, HostFileError> {
            sftp_files::read(&self.sftp, path, max).await
        }
        async fn real_path(&self, path: &str) -> Result<String, HostFileError> {
            self.sftp.canonicalize(path).await.map_err(sftp_error)
        }
        async fn mode(&self, path: &str) -> Result<Option<u32>, HostFileError> {
            Ok(self.sftp.stat(path).await.map_err(sftp_error)?.permissions)
        }
        async fn create_new(
            &self,
            path: &str,
            data: &[u8],
            mode: Option<u32>,
        ) -> Result<FileStamp, HostFileError> {
            sftp_files::create_new(&self.sftp, path, data, mode).await
        }
        async fn verify(&self, path: &str, stamp: &FileStamp) -> Result<bool, HostFileError> {
            sftp_files::verify(&self.sftp, path, stamp).await
        }
        async fn shared_dir(&self, path: &str) -> Result<bool, HostFileError> {
            sftp_files::shared_dir(&self.sftp, path, None).await
        }
        async fn replace(&self, from: &str, to: &str) -> Result<(), HostFileError> {
            sftp_files::replace(&self.sftp, from, to).await?;
            if !from.contains("-restore") {
                (self.on_replace)();
            }
            Ok(())
        }
        async fn remove(&self, path: &str) -> Result<(), HostFileError> {
            self.sftp.remove_file(path).await.map_err(sftp_error)
        }
        async fn run(&self, argv: &[String], _: LineSink<'_>) -> Result<RunOutput, RunError> {
            Ok(scripted_run(argv, &*self.on_validate).await)
        }
    }

    impl fmt::Debug for SftpHost {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.debug_struct("SftpHost")
                .field("root", &self.root)
                .finish()
        }
    }

    #[cfg(unix)]
    async fn sftp_host(
        posix_rename: bool,
        on_validate: Box<dyn Fn() + Send + Sync>,
        on_replace: Box<dyn Fn() + Send + Sync>,
    ) -> (tempfile::TempDir, SftpHost) {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let shop = dir.path().join("srv/shop");
        std::fs::create_dir_all(&shop).unwrap();
        std::fs::set_permissions(&shop, std::fs::Permissions::from_mode(0o755)).unwrap();
        std::fs::write(shop.join("compose.yaml"), BASE).unwrap();
        std::fs::set_permissions(
            shop.join("compose.yaml"),
            std::fs::Permissions::from_mode(0o640),
        )
        .unwrap();
        std::fs::write(shop.join("compose.override.yaml"), OVERRIDE).unwrap();
        let mut server = cc_sftp_core::test_server::LocalFsServer::new(dir.path().to_path_buf());
        if !posix_rename {
            server = server.without_posix_rename();
        }
        let sftp = cc_sftp_core::test_server::connect(server).await.unwrap();
        let root = dir.path().to_path_buf();
        (
            dir,
            SftpHost {
                sftp,
                root,
                on_validate,
                on_replace,
            },
        )
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn sftp_save_writes_candidates_and_backups_through_handles() {
        use std::os::unix::fs::PermissionsExt;
        for posix_rename in [true, false] {
            let (dir, host) = sftp_host(posix_rename, Box::new(|| {}), Box::new(|| {})).await;
            let changed = "services:\n  web:\n    image: nginx:1.27\n";
            let saved = save(
                &host,
                &location(),
                &[edit("/srv/shop/compose.yaml", changed, BASE)],
                "s",
                "t",
                false,
            )
            .await
            .unwrap();
            assert!(matches!(saved, Saved::Written { .. }), "{saved:?}");
            let shop = dir.path().join("srv/shop");
            assert_eq!(
                std::fs::read_to_string(shop.join("compose.yaml")).unwrap(),
                changed
            );
            assert_eq!(
                std::fs::read_to_string(shop.join("compose.yaml.bak-s")).unwrap(),
                BASE
            );
            for name in ["compose.yaml", "compose.yaml.bak-s"] {
                assert_eq!(
                    std::fs::metadata(shop.join(name))
                        .unwrap()
                        .permissions()
                        .mode()
                        & 0o7777,
                    0o640,
                    "{name}"
                );
            }
            assert!(!shop.join(".compose.yaml.consolecrypt-t.tmp").exists());
        }
    }

    /// The candidate becomes a link to the owner's file while `config -q`
    /// runs: nothing is renamed, the link target is untouched.
    #[cfg(unix)]
    #[tokio::test]
    async fn sftp_candidate_swapped_for_a_link_is_tampering() {
        let victim_dir = tempfile::tempdir().unwrap();
        let victim = victim_dir.path().join("authorized_keys");
        std::fs::write(&victim, "ssh-ed25519 AAAA\n").unwrap();
        let holder: Arc<Mutex<Option<std::path::PathBuf>>> = Arc::default();
        let swap = {
            let holder = holder.clone();
            let victim = victim.clone();
            Box::new(move || {
                let root = holder.lock().unwrap().clone().unwrap();
                let candidate = root.join("srv/shop/.compose.yaml.consolecrypt-t.tmp");
                std::fs::remove_file(&candidate).unwrap();
                std::os::unix::fs::symlink(&victim, &candidate).unwrap();
            })
        };
        let (dir, host) = sftp_host(true, swap, Box::new(|| {})).await;
        *holder.lock().unwrap() = Some(dir.path().to_path_buf());
        let saved = save(
            &host,
            &location(),
            &[edit("/srv/shop/compose.yaml", "a: 1\n", BASE)],
            "s",
            "t",
            false,
        )
        .await
        .unwrap();
        assert_eq!(
            saved,
            Saved::Rejected(ComposeSaveOutcome::Tampered {
                path: "/srv/shop/compose.yaml".into(),
                backup: None,
                restored: false,
            })
        );
        let shop = dir.path().join("srv/shop");
        assert_eq!(
            std::fs::read_to_string(shop.join("compose.yaml")).unwrap(),
            BASE
        );
        assert_eq!(
            std::fs::read_to_string(&victim).unwrap(),
            "ssh-ed25519 AAAA\n"
        );
        assert!(!shop.join(".compose.yaml.consolecrypt-t.tmp").exists());
    }

    /// Something else is renamed into the original's place: the original
    /// comes back from the backup.
    #[cfg(unix)]
    #[tokio::test]
    async fn sftp_foreign_file_after_rename_is_restored_from_the_backup() {
        let holder: Arc<Mutex<Option<std::path::PathBuf>>> = Arc::default();
        let swap = {
            let holder = holder.clone();
            Box::new(move || {
                let root = holder.lock().unwrap().clone().unwrap();
                std::fs::write(root.join("srv/shop/compose.yaml"), "image: evil\n").unwrap();
            })
        };
        let (dir, host) = sftp_host(true, Box::new(|| {}), swap).await;
        *holder.lock().unwrap() = Some(dir.path().to_path_buf());
        let saved = save(
            &host,
            &location(),
            &[edit("/srv/shop/compose.yaml", "a: 1\n", BASE)],
            "s",
            "t",
            false,
        )
        .await
        .unwrap();
        assert_eq!(
            saved,
            Saved::Rejected(ComposeSaveOutcome::Tampered {
                path: "/srv/shop/compose.yaml".into(),
                backup: Some("/srv/shop/compose.yaml.bak-s".into()),
                restored: true,
            })
        );
        let shop = dir.path().join("srv/shop");
        assert_eq!(
            std::fs::read_to_string(shop.join("compose.yaml")).unwrap(),
            BASE
        );
    }

    /// This computer's file operations with scripted commands.
    #[derive(Debug)]
    struct LocalFiles {
        files: LocalComposeHost,
    }

    #[async_trait]
    impl ComposeHost for LocalFiles {
        async fn read(&self, path: &str, max: u64) -> Result<Vec<u8>, HostFileError> {
            self.files.read(path, max).await
        }
        async fn real_path(&self, path: &str) -> Result<String, HostFileError> {
            self.files.real_path(path).await
        }
        async fn mode(&self, path: &str) -> Result<Option<u32>, HostFileError> {
            self.files.mode(path).await
        }
        async fn create_new(
            &self,
            path: &str,
            data: &[u8],
            mode: Option<u32>,
        ) -> Result<FileStamp, HostFileError> {
            self.files.create_new(path, data, mode).await
        }
        async fn verify(&self, path: &str, stamp: &FileStamp) -> Result<bool, HostFileError> {
            self.files.verify(path, stamp).await
        }
        async fn shared_dir(&self, path: &str) -> Result<bool, HostFileError> {
            self.files.shared_dir(path).await
        }
        async fn replace(&self, from: &str, to: &str) -> Result<(), HostFileError> {
            self.files.replace(from, to).await
        }
        async fn remove(&self, path: &str) -> Result<(), HostFileError> {
            self.files.remove(path).await
        }
        async fn run(&self, argv: &[String], _: LineSink<'_>) -> Result<RunOutput, RunError> {
            Ok(scripted_run(argv, &|| {}).await)
        }
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn local_shared_directory_is_reported() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let host = LocalComposeHost { socket: None };
        let path = dir.path().to_str().unwrap();
        std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o755)).unwrap();
        assert!(!host.shared_dir(path).await.unwrap());
        std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o777)).unwrap();
        assert!(host.shared_dir(path).await.unwrap());
        std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    }

    /// Another process swaps every candidate it sees for a link to the
    /// owner's file and plants links at the backup names, as fast as it
    /// can, during 1000 saves: the linked file is never written or
    /// re-moded, and the compose file only ever holds text this save wrote.
    #[cfg(unix)]
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn local_race_never_writes_through_a_link() {
        use std::os::unix::fs::PermissionsExt;
        use std::sync::atomic::{AtomicBool, Ordering};
        const ROUNDS: usize = 1000;
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        let shop = root.join("shop");
        std::fs::create_dir(&shop).unwrap();
        let victim = root.join("authorized_keys");
        std::fs::write(&victim, "ssh-ed25519 AAAA\n").unwrap();
        std::fs::set_permissions(&victim, std::fs::Permissions::from_mode(0o600)).unwrap();
        let compose = shop.join("compose.yaml");
        std::fs::write(&compose, "v: 0\n").unwrap();
        std::fs::set_permissions(&compose, std::fs::Permissions::from_mode(0o644)).unwrap();
        for round in 0..ROUNDS {
            let _ = std::os::unix::fs::symlink(
                &victim,
                shop.join(format!("compose.yaml.bak-r{round}")),
            );
        }
        let stop = Arc::new(AtomicBool::new(false));
        let attacker = {
            let stop = stop.clone();
            let shop = shop.clone();
            let victim = victim.clone();
            std::thread::spawn(move || {
                let mut swaps = 0u32;
                while !stop.load(Ordering::Relaxed) {
                    let Ok(entries) = std::fs::read_dir(&shop) else {
                        continue;
                    };
                    for entry in entries.flatten() {
                        let name = entry.file_name().to_string_lossy().into_owned();
                        if name.contains(".consolecrypt-") && !name.ends_with(".stolen") {
                            let path = entry.path();
                            if std::fs::rename(&path, shop.join(format!("{name}.stolen"))).is_ok()
                                && std::os::unix::fs::symlink(&victim, &path).is_ok()
                            {
                                swaps += 1;
                            }
                        }
                    }
                }
                swaps
            })
        };
        let location = ComposeLocation {
            project: "race".into(),
            working_dir: shop.to_str().unwrap().into(),
            config_files: vec![compose.to_str().unwrap().into()],
            env_files: vec![],
        };
        let host = LocalFiles {
            files: LocalComposeHost { socket: None },
        };
        let mut texts = vec!["v: 0\n".to_owned()];
        let (mut written, mut tampered, mut unrestored_rounds) = (0, 0, 0);
        for round in 0..ROUNDS {
            let current = std::fs::read(&compose).unwrap();
            let text = format!("v: {}\n", round + 1);
            texts.push(text.clone());
            let edits = [ComposeEdit {
                path: location.config_files[0].clone(),
                content: text,
                digest: digest(&current),
            }];
            let tag = random_tag().unwrap();
            let outcome = save(&host, &location, &edits, &format!("r{round}"), &tag, true)
                .await
                .unwrap();
            let unrestored = matches!(
                outcome,
                Saved::Rejected(ComposeSaveOutcome::Tampered {
                    restored: false,
                    backup: Some(_),
                    ..
                })
            );
            match outcome {
                Saved::Written { .. } => written += 1,
                Saved::Rejected(ComposeSaveOutcome::Tampered { .. }) => tampered += 1,
                Saved::Rejected(ComposeSaveOutcome::WriteFailed { .. }) => {}
                other => panic!("round {round}: {other:?}"),
            }
            let meta = std::fs::symlink_metadata(&compose).unwrap();
            if unrestored {
                // Reported as such: the user restores from the backup.
                unrestored_rounds += 1;
                std::fs::remove_file(&compose).unwrap();
                std::fs::write(&compose, &current).unwrap();
            } else {
                assert!(
                    meta.is_file(),
                    "round {round}: the compose file is a link only when reported"
                );
            }
            let now = std::fs::read_to_string(&compose).unwrap();
            assert!(texts.contains(&now), "round {round}: {now:?}");
        }
        stop.store(true, Ordering::Relaxed);
        let swaps = attacker.join().unwrap();
        assert_eq!(
            std::fs::read_to_string(&victim).unwrap(),
            "ssh-ed25519 AAAA\n"
        );
        assert_eq!(
            std::fs::metadata(&victim).unwrap().permissions().mode() & 0o7777,
            0o600
        );
        eprintln!(
            "{written} written, {tampered} tampered ({unrestored_rounds} not restored), {swaps} swaps"
        );
        assert!(written + tampered > 0);
    }
}
