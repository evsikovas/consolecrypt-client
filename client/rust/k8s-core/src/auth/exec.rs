//! Device-local, explicitly approved Kubernetes ExecCredential commands.

use super::{AuthError, Credential};
use crate::session::Authentication;
use cc_models::kubernetes::{ExecInteractiveMode, KubeCluster};
use chrono::DateTime;
use secrecy::{ExposeSecret, SecretString};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fmt,
    io::Read,
    path::{Path, PathBuf},
    process::Stdio,
    sync::Mutex,
    time::{Duration, SystemTime},
};
use tokio::io::{AsyncRead, AsyncReadExt};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;
use zeroize::Zeroizing;

const MAX_BINARY: u64 = 512 * 1024 * 1024;
const MAX_OUTPUT: usize = 4 * 1024 * 1024;
const MAX_ARGUMENT_BYTES: usize = 1024 * 1024;
const TIMEOUT: Duration = Duration::from_secs(30);
const REVIEW_TTL: Duration = Duration::from_secs(600);

#[cfg(windows)]
#[path = "exec_windows.rs"]
mod windows;

/// The complete effective environment must be supplied explicitly. The parent
/// process environment is never inherited implicitly. Resolve any inherited
/// PATH/HOME/cloud settings before presenting this request for approval.
pub struct ExecRequest {
    pub context_id: Uuid,
    pub binary: PathBuf,
    pub args: Vec<SecretString>,
    pub environment: BTreeMap<String, SecretString>,
    pub api_version: String,
    pub interactive_mode: ExecInteractiveMode,
    /// Serialized ExecCredential spec.cluster, constructed by the native
    /// runtime only when provideClusterInfo was explicitly configured.
    pub cluster_info: Option<SecretString>,
}

impl fmt::Debug for ExecRequest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ExecRequest")
            .field("context_id", &self.context_id)
            .finish_non_exhaustive()
    }
}

/// This value is native and cannot be deserialized from inventory or Flutter.
/// The runtime retains it behind a random one-use review ticket.
pub struct PreparedExec {
    request: ExecRequest,
    binary_hash: [u8; 32],
    invocation_hash: [u8; 32],
    prepared_at: std::time::Instant,
}

impl fmt::Debug for PreparedExec {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PreparedExec").finish_non_exhaustive()
    }
}

impl PreparedExec {
    pub fn binary_path(&self) -> &Path {
        &self.request.binary
    }

    pub fn binary_sha256(&self) -> String {
        self.binary_hash
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect()
    }

    pub fn invocation_sha256(&self) -> String {
        self.invocation_hash
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect()
    }

    /// May contain secrets. Only expose in the protected consent dialog, never
    /// in general inventory DTOs, logs, errors or AI context.
    pub fn arguments_for_consent(&self) -> &[SecretString] {
        &self.request.args
    }

    pub fn environment_for_consent(&self) -> &BTreeMap<String, SecretString> {
        &self.request.environment
    }

    pub fn context_id(&self) -> Uuid {
        self.request.context_id
    }
}

#[derive(Default)]
struct ApprovalState {
    closed: bool,
    approved: BTreeMap<Uuid, [u8; 32]>,
}

/// One instance per unlocked profile. There is deliberately no persistence or
/// serialization API. Lock must call close(), cancelling running commands.
pub struct ExecEngine {
    approvals: Mutex<ApprovalState>,
    cancellation: CancellationToken,
}

impl fmt::Debug for ExecEngine {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ExecEngine").finish_non_exhaustive()
    }
}

impl Default for ExecEngine {
    fn default() -> Self {
        Self {
            approvals: Mutex::new(ApprovalState::default()),
            cancellation: CancellationToken::new(),
        }
    }
}

impl Drop for ExecEngine {
    fn drop(&mut self) {
        self.close();
    }
}

impl ExecEngine {
    pub async fn prepare(&self, mut request: ExecRequest) -> Result<PreparedExec, AuthError> {
        desktop_only()?;
        self.ensure_open()?;
        validate(&request)?;
        if request.interactive_mode == ExecInteractiveMode::Always {
            return Err(AuthError::InteractiveRequired);
        }
        request.binary =
            dunce::canonicalize(&request.binary).map_err(|_| AuthError::InvalidConfiguration)?;
        let path = request.binary.clone();
        let binary_hash = tokio::task::spawn_blocking(move || binary_hash(&path))
            .await
            .map_err(|_| AuthError::Process)??;
        self.ensure_open()?;
        let invocation_hash = invocation_hash(&request, &binary_hash);
        Ok(PreparedExec {
            request,
            binary_hash,
            invocation_hash,
            prepared_at: std::time::Instant::now(),
        })
    }

    /// Call only after the user accepted this exact prepared request in the
    /// local protected dialog. Never call as a side effect of import or sync.
    pub fn approve(&self, prepared: &PreparedExec) -> Result<(), AuthError> {
        desktop_only()?;
        if prepared.prepared_at.elapsed() >= REVIEW_TTL {
            return Err(AuthError::ApprovalRequired);
        }
        let mut state = self.approvals.lock().map_err(|_| AuthError::Closed)?;
        if state.closed {
            return Err(AuthError::Closed);
        }
        state
            .approved
            .insert(prepared.request.context_id, prepared.invocation_hash);
        Ok(())
    }

    pub fn is_approved(&self, prepared: &PreparedExec) -> bool {
        self.approvals.lock().is_ok_and(|state| {
            !state.closed
                && state.approved.get(&prepared.request.context_id)
                    == Some(&prepared.invocation_hash)
        })
    }

    pub fn revoke(&self, context_id: Uuid) {
        if let Ok(mut state) = self.approvals.lock() {
            state.approved.remove(&context_id);
        }
    }

    pub fn close(&self) {
        self.cancellation.cancel();
        if let Ok(mut state) = self.approvals.lock() {
            state.closed = true;
            state.approved.clear();
        }
    }

    fn ensure_open(&self) -> Result<(), AuthError> {
        if self.cancellation.is_cancelled() {
            Err(AuthError::Closed)
        } else {
            Ok(())
        }
    }

    pub async fn execute(
        &self,
        prepared: PreparedExec,
        cancellation: CancellationToken,
    ) -> Result<Credential, AuthError> {
        desktop_only()?;
        self.ensure_open()?;
        if cancellation.is_cancelled() {
            return Err(AuthError::Cancelled);
        }
        if !self.is_approved(&prepared) {
            return Err(AuthError::ApprovalRequired);
        }
        let path = prepared.request.binary.clone();
        let hash = tokio::task::spawn_blocking(move || binary_hash(&path))
            .await
            .map_err(|_| AuthError::Process)??;
        if hash != prepared.binary_hash {
            self.revoke(prepared.request.context_id);
            return Err(AuthError::CommandChanged);
        }
        // Recheck after the asynchronous file read: a vault lock/revocation
        // during hashing must not start a process.
        self.ensure_open()?;
        if !self.is_approved(&prepared) {
            return Err(AuthError::ApprovalRequired);
        }
        let work = run(prepared.request);
        tokio::select! {
            biased;
            _ = self.cancellation.cancelled() => Err(AuthError::Closed),
            _ = cancellation.cancelled() => Err(AuthError::Cancelled),
            result = tokio::time::timeout(TIMEOUT, work) => result.map_err(|_| AuthError::Timeout)?,
        }
    }
}

fn desktop_only() -> Result<(), AuthError> {
    if cfg!(any(target_os = "android", target_os = "ios")) {
        Err(AuthError::UnsupportedPlatform)
    } else {
        Ok(())
    }
}

fn validate(request: &ExecRequest) -> Result<(), AuthError> {
    if request.context_id.is_nil()
        || !request.binary.is_absolute()
        || !matches!(
            request.api_version.as_str(),
            "client.authentication.k8s.io/v1" | "client.authentication.k8s.io/v1beta1"
        )
        || request.args.len() > 256
        || request.environment.len() > 256
        || request.environment.contains_key("KUBERNETES_EXEC_INFO")
    {
        return Err(AuthError::InvalidConfiguration);
    }
    #[cfg(windows)]
    if !request
        .binary
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("exe"))
    {
        // Rust/Windows may launch .bat and .cmd through cmd.exe implicitly.
        // Authentication must invoke a native executable with normal argv.
        return Err(AuthError::InvalidConfiguration);
    }
    let mut size = 0usize;
    for value in request.args.iter().map(|s| s.expose_secret()).chain(
        request
            .environment
            .iter()
            .flat_map(|(key, value)| [key.as_str(), value.expose_secret()]),
    ) {
        size = size.saturating_add(value.len());
        if value.contains('\0') || size > MAX_ARGUMENT_BYTES {
            return Err(AuthError::InvalidConfiguration);
        }
    }
    if request.environment.keys().any(|name| {
        name.is_empty()
            || name.len() > 256
            || !name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
            || name.as_bytes()[0].is_ascii_digit()
    }) {
        return Err(AuthError::InvalidConfiguration);
    }
    if request
        .cluster_info
        .as_ref()
        .is_some_and(|info| info.expose_secret().len() > MAX_ARGUMENT_BYTES)
    {
        return Err(AuthError::InvalidConfiguration);
    }
    Ok(())
}

/// Hash a regular executable through a bounded, no-follow file handle.
pub fn binary_hash(path: &Path) -> Result<[u8; 32], AuthError> {
    let mut options = std::fs::OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_CLOEXEC);
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        // FILE_FLAG_OPEN_REPARSE_POINT: reject a final symlink swapped in
        // after canonicalization instead of following it during hashing.
        options.custom_flags(0x0020_0000);
    }
    let mut file = options
        .open(path)
        .map_err(|_| AuthError::InvalidConfiguration)?;
    let before = file
        .metadata()
        .map_err(|_| AuthError::InvalidConfiguration)?;
    if !before.is_file() || before.len() == 0 || before.len() > MAX_BINARY {
        return Err(AuthError::InvalidConfiguration);
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if before.permissions().mode() & 0o111 == 0 {
            return Err(AuthError::InvalidConfiguration);
        }
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if before.file_attributes() & 0x0000_0400 != 0 {
            return Err(AuthError::InvalidConfiguration);
        }
    }
    let mut digest = Sha256::new();
    let mut buffer = [0u8; 64 * 1024];
    let mut total = 0u64;
    loop {
        let read = file.read(&mut buffer).map_err(|_| AuthError::Process)?;
        if read == 0 {
            break;
        }
        total += read as u64;
        if total > MAX_BINARY {
            return Err(AuthError::InvalidConfiguration);
        }
        digest.update(&buffer[..read]);
    }
    let after = file.metadata().map_err(|_| AuthError::Process)?;
    if total != before.len()
        || before.len() != after.len()
        || before.modified().ok() != after.modified().ok()
    {
        return Err(AuthError::CommandChanged);
    }
    Ok(digest.finalize().into())
}

fn invocation_hash(request: &ExecRequest, binary: &[u8; 32]) -> [u8; 32] {
    fn field(hash: &mut Sha256, bytes: &[u8]) {
        hash.update((bytes.len() as u64).to_le_bytes());
        hash.update(bytes);
    }
    let mut hash = Sha256::new();
    field(&mut hash, b"ConsoleCrypt Kubernetes exec approval v1");
    field(&mut hash, request.context_id.as_bytes());
    field(&mut hash, binary);
    field(&mut hash, request.binary.as_os_str().as_encoded_bytes());
    field(&mut hash, request.api_version.as_bytes());
    field(
        &mut hash,
        match request.interactive_mode {
            ExecInteractiveMode::Never => b"never",
            ExecInteractiveMode::IfAvailable => b"if_available",
            ExecInteractiveMode::Always => b"always",
        },
    );
    field(&mut hash, &(request.args.len() as u64).to_le_bytes());
    for arg in &request.args {
        field(&mut hash, arg.expose_secret().as_bytes());
    }
    field(&mut hash, &(request.environment.len() as u64).to_le_bytes());
    for (name, value) in &request.environment {
        field(&mut hash, name.as_bytes());
        field(&mut hash, value.expose_secret().as_bytes());
    }
    field(
        &mut hash,
        request
            .cluster_info
            .as_ref()
            .map_or(b"", |value| value.expose_secret().as_bytes()),
    );
    hash.finalize().into()
}

/// An explicit search path is resolved once; the approved command thereafter
/// always uses the absolute canonical path. Empty/relative PATH entries and
/// implicit current-directory lookup are excluded.
pub fn resolve_binary(command: &str, search_path: &str) -> Result<PathBuf, AuthError> {
    desktop_only()?;
    let path = Path::new(command);
    if path.is_absolute() {
        return dunce::canonicalize(path).map_err(|_| AuthError::InvalidConfiguration);
    }
    if path.components().count() != 1 || command.is_empty() {
        return Err(AuthError::InvalidConfiguration);
    }
    for directory in std::env::split_paths(search_path) {
        if !directory.is_absolute() {
            continue;
        }
        let candidate = directory.join(command);
        #[cfg(windows)]
        let candidate = if candidate.extension().is_none() {
            candidate.with_extension("exe")
        } else {
            candidate
        };
        if candidate.is_file() {
            return dunce::canonicalize(candidate).map_err(|_| AuthError::InvalidConfiguration);
        }
    }
    Err(AuthError::InvalidConfiguration)
}

/// Build only the public connection metadata permitted by provideClusterInfo;
/// vault secret references and route/proxy credentials never enter this value.
pub fn cluster_info(cluster: &KubeCluster) -> SecretString {
    use base64::{engine::general_purpose::STANDARD, Engine};
    let mut info = serde_json::json!({
        "server":cluster.server,
        "insecure-skip-tls-verify":cluster.insecure_skip_tls_verify,
    });
    if let Some(name) = &cluster.tls_server_name {
        info["tls-server-name"] = name.clone().into();
    }
    if let Some(ca) = &cluster.certificate_authority_pem {
        info["certificate-authority-data"] = STANDARD.encode(ca.as_bytes()).into();
    }
    SecretString::from(info.to_string())
}

async fn read_bounded(mut pipe: impl AsyncRead + Unpin) -> Result<Zeroizing<Vec<u8>>, AuthError> {
    let mut result = Zeroizing::new(Vec::new());
    let mut buffer = Zeroizing::new(vec![0u8; 8192]);
    loop {
        let size = pipe
            .read(&mut buffer)
            .await
            .map_err(|_| AuthError::Process)?;
        if size == 0 {
            return Ok(result);
        }
        if result.len() + size > MAX_OUTPUT {
            return Err(AuthError::ResponseTooLarge);
        }
        result.extend_from_slice(&buffer[..size]);
    }
}

#[cfg(unix)]
#[derive(Debug)]
struct ProcessGroup(Option<u32>);

#[cfg(unix)]
impl Drop for ProcessGroup {
    fn drop(&mut self) {
        #[cfg(unix)]
        if let Some(id) = self.0 {
            terminate_group(id);
        }
    }
}

#[cfg(unix)]
#[allow(unsafe_code)]
fn terminate_group(id: u32) {
    if let Ok(group) = i32::try_from(id) {
        // SAFETY: the child was started in its own process group. No pointers
        // are passed, and a vanished group merely produces ESRCH.
        unsafe { libc::kill(-group, libc::SIGKILL) };
    }
}

/// Owns the whole native child process tree. Dropping this guard stops all
/// descendants as well as the child. Callers must keep it beside the child.
#[derive(Debug)]
pub struct ProcessTree {
    #[cfg(unix)]
    _group: ProcessGroup,
    #[cfg(windows)]
    _job: windows::Job,
}

/// Starts an explicitly authorized native command without a shell wrapper.
/// This primitive does not grant authorization or populate an environment.
pub fn spawn_scoped(
    command: &mut tokio::process::Command,
) -> Result<(tokio::process::Child, ProcessTree), AuthError> {
    if cfg!(any(target_os = "android", target_os = "ios")) {
        return Err(AuthError::UnsupportedPlatform);
    }
    command.kill_on_drop(true);
    #[cfg(unix)]
    command.process_group(0);
    #[cfg(windows)]
    command.creation_flags(
        windows_sys::Win32::System::Threading::CREATE_SUSPENDED
            | windows_sys::Win32::System::Threading::CREATE_NO_WINDOW,
    );
    let child = command.spawn().map_err(|_| AuthError::Process)?;
    let tree = ProcessTree {
        #[cfg(unix)]
        _group: ProcessGroup(child.id()),
        #[cfg(windows)]
        _job: windows::Job::attach_and_resume(&child)?,
    };
    Ok((child, tree))
}

async fn run(request: ExecRequest) -> Result<Credential, AuthError> {
    let mut command = tokio::process::Command::new(&request.binary);
    command
        .args(request.args.iter().map(ExposeSecret::expose_secret))
        .env_clear()
        .envs(
            request
                .environment
                .iter()
                .map(|(key, value)| (key, value.expose_secret())),
        )
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    // Work in the executable's directory rather than inheriting a terminal or
    // file-picker directory that could change how a plugin resolves files.
    command.current_dir(
        request
            .binary
            .parent()
            .ok_or(AuthError::InvalidConfiguration)?,
    );
    let cluster: Option<serde_json::Value> = request
        .cluster_info
        .as_ref()
        .map(|value| serde_json::from_str(value.expose_secret()))
        .transpose()
        .map_err(|_| AuthError::InvalidConfiguration)?;
    let mut exec_info = serde_json::json!({
        "apiVersion": request.api_version,
        "kind": "ExecCredential",
        "spec": { "interactive": false }
    });
    if let Some(cluster) = cluster {
        if !cluster.is_object() {
            return Err(AuthError::InvalidConfiguration);
        }
        exec_info["spec"]["cluster"] = cluster;
    }
    let info = Zeroizing::new(exec_info.to_string());
    command.env("KUBERNETES_EXEC_INFO", info.as_str());
    let (mut child, _process_tree) = spawn_scoped(&mut command)?;
    let stdout = child.stdout.take().ok_or(AuthError::Process)?;
    let stderr = child.stderr.take().ok_or(AuthError::Process)?;
    let (status, stdout, _stderr) = tokio::try_join!(
        async { child.wait().await.map_err(|_| AuthError::Process) },
        read_bounded(stdout),
        read_bounded(stderr)
    )?;
    if !status.success() {
        return Err(AuthError::Rejected);
    }
    parse_credential(&stdout, &request.api_version)
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ExecResponse {
    api_version: String,
    kind: String,
    status: ExecStatus,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ExecStatus {
    #[serde(default, deserialize_with = "deserialize_secret")]
    token: Option<SecretString>,
    #[serde(default, deserialize_with = "deserialize_secret")]
    client_key_data: Option<SecretString>,
    #[serde(default)]
    client_certificate_data: Option<String>,
    expiration_timestamp: Option<String>,
}

fn deserialize_secret<'de, D>(deserializer: D) -> Result<Option<SecretString>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    Option::<String>::deserialize(deserializer).map(|value| value.map(SecretString::from))
}

fn parse_credential(bytes: &[u8], version: &str) -> Result<Credential, AuthError> {
    let value: ExecResponse =
        serde_json::from_slice(bytes).map_err(|_| AuthError::InvalidResponse)?;
    if value.api_version != version || value.kind != "ExecCredential" {
        return Err(AuthError::InvalidResponse);
    }
    let now = SystemTime::now();
    // Providers without expiration may be used for this connection, but are
    // not granted an unbounded cache lifetime.
    let expires_at = value
        .status
        .expiration_timestamp
        .map(|text| {
            let timestamp =
                DateTime::parse_from_rfc3339(&text).map_err(|_| AuthError::InvalidResponse)?;
            let seconds =
                u64::try_from(timestamp.timestamp()).map_err(|_| AuthError::InvalidResponse)?;
            SystemTime::UNIX_EPOCH
                .checked_add(Duration::new(seconds, timestamp.timestamp_subsec_nanos()))
                .ok_or(AuthError::InvalidResponse)
        })
        .transpose()?
        .unwrap_or(now + Duration::from_secs(60));
    if expires_at <= now {
        return Err(AuthError::InvalidResponse);
    }
    let expires_at = expires_at.min(now + Duration::from_secs(24 * 60 * 60));
    let authentication = match (
        value.status.token,
        value.status.client_certificate_data,
        value.status.client_key_data,
    ) {
        (Some(token), None, None)
            if !token.expose_secret().is_empty()
                && !token.expose_secret().bytes().any(|b| b.is_ascii_control()) =>
        {
            Authentication::Bearer(token)
        }
        (None, Some(certificate_pem), Some(private_key_pem))
            if !certificate_pem.is_empty() && !private_key_pem.expose_secret().is_empty() =>
        {
            Authentication::ClientCertificate {
                certificate_pem,
                private_key_pem,
            }
        }
        _ => return Err(AuthError::InvalidResponse),
    };
    Ok(Credential {
        authentication,
        expires_at,
    })
}

#[cfg(all(test, unix, not(any(target_os = "android", target_os = "ios"))))]
#[path = "exec_tests.rs"]
mod tests;
