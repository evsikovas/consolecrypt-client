//! Compose projects of a Docker session: the files named by the containers'
//! labels are read on the engine host (this computer's disk, or SFTP over
//! the session's SSH connection), and edits are validated with
//! `docker compose config -q` against a candidate written next to the
//! original before anything is replaced. `up -d` then runs as a job whose
//! output the UI polls. Editing needs the vault passphrase (a short-lived
//! grant per project); file contents are treated as secrets.
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
    /// Creates `path` exclusively ([`HostFileError::Exists`] if taken).
    async fn create_new(
        &self,
        path: &str,
        data: &[u8],
        mode: Option<u32>,
    ) -> Result<(), HostFileError>;
    /// Renames `from` over `to` (atomically where the host can).
    async fn replace(&self, from: &str, to: &str) -> Result<(), HostFileError>;
    async fn remove(&self, path: &str) -> Result<(), HostFileError>;
    /// Runs `argv` (no shell interpretation of any word), passing each
    /// output line to `on_line`.
    async fn run(&self, argv: &[String], on_line: LineSink<'_>) -> Result<RunOutput, RunError>;
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
    ) -> Result<(), HostFileError> {
        use tokio::io::AsyncWriteExt;
        let mut options = tokio::fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        options.mode(0o600);
        let mut file = options.open(path).await.map_err(|e| io_error(&e))?;
        let written = async {
            file.write_all(data).await?;
            file.sync_all().await?;
            #[cfg(unix)]
            if let Some(mode) = mode {
                use std::os::unix::fs::PermissionsExt;
                tokio::fs::set_permissions(path, std::fs::Permissions::from_mode(mode)).await?;
            }
            #[cfg(not(unix))]
            let _ = mode;
            Ok::<_, std::io::Error>(())
        }
        .await;
        if let Err(error) = written {
            let _ = tokio::fs::remove_file(path).await;
            return Err(io_error(&error));
        }
        Ok(())
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

#[async_trait]
impl ComposeHost for SshComposeHost {
    async fn read(&self, path: &str, max: u64) -> Result<Vec<u8>, HostFileError> {
        let sftp = self.sftp().await?;
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
    ) -> Result<(), HostFileError> {
        let sftp = self.sftp().await?;
        sftp.create_file(path, 0o600).await.map_err(sftp_error)?;
        let written = async {
            sftp.write(path, data).await?;
            if let Some(mode) = mode {
                sftp.chmod(path, mode).await?;
            }
            Ok::<_, SftpError>(())
        }
        .await;
        if let Err(error) = written {
            let _ = sftp.remove_file(path).await;
            return Err(sftp_error(error));
        }
        Ok(())
    }

    async fn replace(&self, from: &str, to: &str) -> Result<(), HostFileError> {
        let sftp = self.sftp().await?;
        if sftp.supports_posix_rename() {
            return sftp.posix_rename(from, to).await.map_err(sftp_error);
        }
        // SFTP v3 rename refuses an existing target; the backup is already
        // in place, so remove-then-rename is safe enough here.
        sftp.remove_file(to).await.map_err(sftp_error)?;
        sftp.rename(from, to).await.map_err(sftp_error)
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
}

impl fmt::Debug for ComposeFileDto {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ComposeFileDto")
            .field("path", &self.path)
            .field("size", &self.size)
            .field("error", &self.error)
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

/// Validate-then-write: candidates next to the originals, `config -q` with
/// them standing in, then a timestamped backup of each original and a
/// rename over it. Nothing is replaced unless validation passed.
pub(crate) async fn save(
    host: &dyn ComposeHost,
    location: &ComposeLocation,
    edits: &[ComposeEdit],
    stamp: &str,
    tag: &str,
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
    // The originals as they are now: unchanged since opened, resolved
    // through symlinks, with their permission bits.
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
    let mut written = Vec::new();
    for (edit, target) in edits.iter().zip(&targets) {
        if let Err(error) = host
            .create_new(&target.candidate, edit.content.as_bytes(), target.mode)
            .await
        {
            remove_all(host, &written).await;
            return rejected(ComposeSaveOutcome::WriteFailed {
                path: edit.path.clone(),
                reason: error.code(),
            });
        }
        written.push(target.candidate.clone());
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
    let mut backups = Vec::new();
    for (edit, target) in edits.iter().zip(&targets) {
        let mut saved = None;
        for attempt in 0..20 {
            let backup = backup_path(&target.real, stamp, attempt);
            match host
                .create_new(&backup, &target.original, target.mode)
                .await
            {
                Ok(()) => {
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
    for (index, (edit, target)) in edits.iter().zip(&targets).enumerate() {
        if let Err(error) = host.replace(&target.candidate, &target.real).await {
            remove_all(host, &written[index..]).await;
            return rejected(ComposeSaveOutcome::WriteFailed {
                path: edit.path.clone(),
                reason: error.code(),
            });
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

    /// The project's compose files, read on the engine host.
    pub async fn docker_compose_project(
        &self,
        session_id: String,
        project: String,
    ) -> AppResult<ComposeProjectDto> {
        let (location, host, kind, _) = self.compose_context(&session_id, &project).await?;
        Ok(read_project(host.as_ref(), &location, kind).await)
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
    pub async fn docker_compose_save(
        &self,
        session_id: String,
        project: String,
        grant: String,
        edits: Vec<ComposeEdit>,
        apply: bool,
    ) -> AppResult<ComposeSaveOutcome> {
        let (_, unlocked) = self.unlocked().await?;
        if !unlocked.docker.compose.check(&grant, &session_id, &project) {
            return Ok(ComposeSaveOutcome::Locked);
        }
        let (location, host, _, cancel) = self.compose_context(&session_id, &project).await?;
        let stamp = chrono::Local::now().format("%Y%m%d-%H%M%S").to_string();
        let tag = uuid::Uuid::new_v4().simple().to_string()[..12].to_owned();
        let (backups, cli) = match save(host.as_ref(), &location, &edits, &stamp, &tag).await? {
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
        ) -> Result<(), HostFileError> {
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
            Ok(())
        }
        async fn replace(&self, from: &str, to: &str) -> Result<(), HostFileError> {
            let mut files = self.files.lock().unwrap();
            let entry = files.remove(from).ok_or(HostFileError::NotFound)?;
            files.insert(to.to_owned(), entry);
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
                let files = self.files.lock().unwrap();
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
            save(&host, &location(), &edits, "s", "t").await.unwrap(),
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
            save(&host, &location(), &edits, "s", "t").await.unwrap(),
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
            save(&host, &location(), &edits, "s", "t").await.unwrap(),
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
            assert!(save(&host, &location(), &edits, "s", "t").await.is_err());
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
        host.create_new(&other, b"a: 1\n", Some(0o640))
            .await
            .unwrap();
        assert_eq!(
            host.create_new(&other, b"b", None).await,
            Err(HostFileError::Exists)
        );
        assert_eq!(
            std::fs::metadata(&other).unwrap().permissions().mode() & 0o7777,
            0o640
        );
        host.replace(&other, &path).await.unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "a: 1\n");
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
}
