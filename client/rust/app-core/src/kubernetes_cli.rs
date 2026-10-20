//! Explicit desktop Flux CLI operations. Discovery never executes a candidate;
//! every executable action originates from a user-confirmed native plan.
mod kubectl;
use base64::{engine::general_purpose::STANDARD, Engine as _};
use futures::StreamExt;
pub use kubectl::KubectlDiscovery;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, HashMap},
    ffi::OsString,
    io::{Read, Write},
    path::{Path, PathBuf},
    process::Stdio,
    sync::Mutex,
    time::{Duration, Instant},
};
use tokio::io::AsyncReadExt;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;
use zeroize::Zeroizing;

const MAX_ARCHIVE: usize = 128 * 1024 * 1024;
const MAX_BINARY: usize = 256 * 1024 * 1024;
const MAX_OUTPUT: usize = 4 * 1024 * 1024;
const PLAN_LIFETIME: Duration = Duration::from_secs(600);

#[derive(Debug, thiserror::Error)]
pub enum CliError {
    #[error("Desktop CLI is unavailable on this platform")]
    UnsupportedPlatform,
    #[error("Invalid CLI configuration")]
    Invalid,
    #[error("CLI executable was not found")]
    NotFound,
    #[error("CLI executable changed after review")]
    Changed,
    #[error("CLI request expired or was cancelled")]
    Cancelled,
    #[error("CLI operation timed out")]
    Timeout,
    #[error("CLI output exceeded the limit")]
    TooLarge,
    #[error("CLI operation failed")]
    Process,
    #[error("Download failed")]
    Download,
    #[error("Cosign verification is required")]
    VerificationRequired,
    #[error("Release verification failed")]
    Verification,
    #[error("Installation failed")]
    Install,
}
type Result<T> = std::result::Result<T, CliError>;

#[derive(Clone, Debug, Serialize)]
pub struct CliCandidate {
    pub path: PathBuf,
}
#[derive(Clone, Debug, Serialize)]
pub struct FluxDiscovery {
    pub supported: bool,
    pub candidates: Vec<CliCandidate>,
    pub package_managers: Vec<InstallMethod>,
    pub cosign: Option<PathBuf>,
    pub destination: Option<PathBuf>,
}
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum InstallMethod {
    Homebrew,
    Winget,
    Scoop,
    Chocolatey,
    OfficialRelease,
}
#[derive(Clone, Debug, Serialize)]
pub struct CliVersion {
    pub path: PathBuf,
    pub version: String,
    pub sha256: String,
}
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum InstallOperation {
    Install,
    Update,
    Remove,
}
#[derive(Clone, Debug, Serialize)]
pub struct InstallReview {
    pub tool: String,
    pub ticket: String,
    pub method: InstallMethod,
    pub operation: InstallOperation,
    pub version: String,
    pub executable: Option<PathBuf>,
    pub arguments: Vec<String>,
    pub source: String,
    pub destination: PathBuf,
    pub verifier: Option<PathBuf>,
    pub executable_sha256: Option<String>,
}
#[derive(Clone)]
pub(crate) struct Binary {
    pub(crate) path: PathBuf,
    pub(crate) hash: String,
}
struct Plan {
    created: Instant,
    review: InstallReview,
    binary: Binary,
    previous: Option<String>,
}
/// Unforgeable one-use confirmation state, owned by the unlocked runtime.
#[derive(Default)]
pub struct FluxCliRuntime {
    plans: Mutex<HashMap<String, Plan>>,
    pub(crate) helm_plans: Mutex<HashMap<String, crate::kubernetes_helm_cli::HelmPlan>>,
    closed: CancellationToken,
    active: Mutex<HashMap<String, CancellationToken>>,
}
impl std::fmt::Debug for FluxCliRuntime {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FluxCliRuntime").finish_non_exhaustive()
    }
}
impl FluxCliRuntime {
    pub fn cancellation(&self) -> CancellationToken {
        self.closed.child_token()
    }
    pub fn review(&self, ticket: &str) -> Result<InstallReview> {
        self.ensure_open()?;
        let plans = self.plans.lock().map_err(|_| CliError::Cancelled)?;
        let plan = plans.get(ticket).ok_or(CliError::Cancelled)?;
        if plan.created.elapsed() >= PLAN_LIFETIME {
            return Err(CliError::Cancelled);
        }
        Ok(plan.review.clone())
    }
    pub(crate) fn track_operation(
        &self,
        ticket: &str,
        parent: &CancellationToken,
    ) -> Result<TrackedOperation<'_>> {
        self.ensure_open()?;
        let cancellation = parent.child_token();
        let mut active = self.active.lock().map_err(|_| CliError::Cancelled)?;
        if active.len() >= 8 || active.contains_key(ticket) {
            return Err(CliError::Invalid);
        }
        active.insert(ticket.into(), cancellation.clone());
        Ok(TrackedOperation {
            runtime: self,
            ticket: ticket.into(),
            cancellation,
        })
    }
    pub fn close(&self) {
        self.closed.cancel();
        if let Ok(mut plans) = self.helm_plans.lock() {
            plans.clear();
        }
        if let Ok(mut plans) = self.plans.lock() {
            plans.clear();
        }
        if let Ok(mut active) = self.active.lock() {
            for (_, cancel) in active.drain() {
                cancel.cancel();
            }
        }
    }
    pub fn cancel(&self, ticket: &str) {
        if let Ok(mut plans) = self.helm_plans.lock() {
            plans.remove(ticket);
        }
        if let Ok(mut plans) = self.plans.lock() {
            plans.remove(ticket);
        }
        if let Ok(mut active) = self.active.lock() {
            if let Some(cancel) = active.remove(ticket) {
                cancel.cancel();
            }
        }
    }
    pub fn discover(&self, manual: Option<&Path>) -> Result<FluxDiscovery> {
        if mobile() {
            return Ok(FluxDiscovery {
                supported: false,
                candidates: vec![],
                package_managers: vec![],
                cosign: None,
                destination: None,
            });
        }
        self.ensure_open()?;
        let mut candidates = Vec::new();
        if let Some(path) = manual {
            if let Ok(path) = executable(path) {
                candidates.push(CliCandidate { path });
            }
        }
        for dir in search_directories() {
            if let Ok(path) = executable(&dir.join(binary_name("flux"))) {
                if !candidates.iter().any(|candidate| candidate.path == path) {
                    candidates.push(CliCandidate { path });
                }
            }
        }
        let package_managers = [
            InstallMethod::Homebrew,
            InstallMethod::Winget,
            InstallMethod::Scoop,
            InstallMethod::Chocolatey,
        ]
        .into_iter()
        .filter(|method| manager(*method).is_some())
        .collect();
        Ok(FluxDiscovery {
            supported: true,
            candidates,
            package_managers,
            cosign: find("cosign"),
            destination: destination().ok(),
        })
    }
    /// Only call in response to the user's CLI check/use action.
    pub async fn probe(&self, path: &Path, cancel: CancellationToken) -> Result<CliVersion> {
        self.ensure_open()?;
        let binary = inspect(path)?;
        let output = self
            .run(
                &binary,
                &[
                    "version".into(),
                    "--client".into(),
                    "-o".into(),
                    "json".into(),
                ],
                None,
                Duration::from_secs(10),
                cancel,
            )
            .await?;
        if output.exit_code != 0 {
            return Err(CliError::Process);
        }
        let data: serde_json::Value =
            serde_json::from_slice(&output.stdout).map_err(|_| CliError::Invalid)?;
        let version = data
            .get("flux")
            .and_then(|value| value.get("version").or(Some(value)))
            .and_then(|v| v.as_str())
            .or_else(|| data.get("version").and_then(|v| v.as_str()))
            .ok_or(CliError::Invalid)?;
        let version = version_number(version)?;
        Ok(CliVersion {
            path: binary.path,
            version,
            sha256: binary.hash,
        })
    }
    pub fn prepare_install(&self, version: &str, method: InstallMethod) -> Result<InstallReview> {
        self.prepare_maintenance(version, method, InstallOperation::Install)
    }
    pub fn prepare_update(&self, version: &str, method: InstallMethod) -> Result<InstallReview> {
        self.prepare_maintenance(version, method, InstallOperation::Update)
    }
    pub fn prepare_remove(&self, version: &str, method: InstallMethod) -> Result<InstallReview> {
        self.prepare_maintenance(version, method, InstallOperation::Remove)
    }
    fn prepare_maintenance(
        &self,
        version: &str,
        method: InstallMethod,
        operation: InstallOperation,
    ) -> Result<InstallReview> {
        self.ensure_open()?;
        let version = version_number(version)?;
        let destination = destination()?;
        let previous = if destination.exists() {
            Some(inspect(&destination)?.hash)
        } else {
            None
        };
        let (binary, arguments, source, verifier) =
            if method == InstallMethod::OfficialRelease && operation == InstallOperation::Remove {
                (
                    inspect(&destination)?,
                    vec![],
                    format!("https://github.com/fluxcd/flux2/releases/tag/v{version}"),
                    None,
                )
            } else if method == InstallMethod::OfficialRelease {
                let binary = inspect(&find("cosign").ok_or(CliError::VerificationRequired)?)?;
                (
                    binary.clone(),
                    vec![],
                    format!("https://github.com/fluxcd/flux2/releases/tag/v{version}"),
                    Some(binary.path),
                )
            } else {
                let path = manager(method).ok_or(CliError::NotFound)?;
                let args = manager_arguments(method, &version, operation)?;
                (
                    inspect(&path)?,
                    args,
                    "https://fluxcd.io/flux/installation/".into(),
                    None,
                )
            };
        let ticket = Uuid::new_v4().to_string();
        let review = InstallReview {
            tool: "flux".into(),
            ticket: ticket.clone(),
            method,
            operation,
            version,
            executable: if method == InstallMethod::OfficialRelease {
                None
            } else {
                Some(binary.path.clone())
            },
            arguments,
            source,
            destination,
            verifier,
            executable_sha256: Some(binary.hash.clone()),
        };
        let mut plans = self.plans.lock().map_err(|_| CliError::Cancelled)?;
        plans.retain(|_, plan| plan.created.elapsed() < PLAN_LIFETIME);
        if plans.len() >= 4 {
            return Err(CliError::Invalid);
        }
        plans.insert(
            ticket,
            Plan {
                created: Instant::now(),
                review: review.clone(),
                binary,
                previous,
            },
        );
        Ok(review)
    }
    /// Consume once, then run the returned command in a visible native terminal.
    /// Package-manager installs may ask for an administrator password there.
    pub fn approve_package_install(&self, ticket: &str) -> Result<PackageCommand> {
        let plan = self.take(ticket)?;
        if plan.review.method == InstallMethod::OfficialRelease {
            return Err(CliError::Invalid);
        }
        unchanged(&plan.binary)?;
        Ok(PackageCommand {
            executable: plan.binary.path,
            arguments: plan.review.arguments,
            environment: clean_environment(),
            cancellation: self.closed.child_token(),
        })
    }
    pub async fn install_release(
        &self,
        ticket: &str,
        cancel: CancellationToken,
    ) -> Result<CliVersion> {
        let plan = self.take(ticket)?;
        if plan.review.method != InstallMethod::OfficialRelease
            || plan.review.operation == InstallOperation::Remove
        {
            return Err(CliError::Invalid);
        }
        let operation = self.closed.child_token();
        {
            let mut active = self.active.lock().map_err(|_| CliError::Cancelled)?;
            if active.len() >= 4 {
                return Err(CliError::Invalid);
            }
            active.insert(ticket.into(), operation.clone());
        }
        let _guard = InstallGuard {
            runtime: self,
            ticket: ticket.into(),
        };
        tokio::select! {biased;
            _=operation.cancelled()=>Err(CliError::Cancelled),
            _=cancel.cancelled()=>Err(CliError::Cancelled),
            result=tokio::time::timeout(Duration::from_secs(600),install(&plan,operation.clone()))=>result.map_err(|_|CliError::Timeout)?,
        }
    }
    /// Remove only the exact explicitly reviewed direct-release executable.
    pub fn remove_release(&self, ticket: &str) -> Result<()> {
        let plan = self.take(ticket)?;
        if plan.review.method != InstallMethod::OfficialRelease
            || plan.review.operation != InstallOperation::Remove
        {
            return Err(CliError::Invalid);
        }
        let metadata =
            std::fs::symlink_metadata(&plan.review.destination).map_err(|_| CliError::NotFound)?;
        if metadata.file_type().is_symlink() || !metadata.is_file() {
            return Err(CliError::Changed);
        }
        unchanged(&plan.binary)?;
        std::fs::remove_file(&plan.review.destination).map_err(|_| CliError::Install)
    }
    pub async fn execute(
        &self,
        binary: &Path,
        action: FluxAction,
        proxy_config: &Path,
        cancel: CancellationToken,
    ) -> Result<CliOutput> {
        self.ensure_open()?;
        if !proxy_config.is_absolute() || !proxy_config.is_file() {
            return Err(CliError::Invalid);
        }
        let binary = inspect(binary)?;
        let mut arguments = action.arguments()?;
        arguments.extend([
            "--kubeconfig".into(),
            proxy_config.to_string_lossy().into_owned(),
            "--context".into(),
            "consolecrypt".into(),
        ]);
        self.run(
            &binary,
            &arguments,
            Some(proxy_config),
            Duration::from_secs(120),
            cancel,
        )
        .await
    }
    async fn run(
        &self,
        binary: &Binary,
        args: &[String],
        config: Option<&Path>,
        limit: Duration,
        cancel: CancellationToken,
    ) -> Result<CliOutput> {
        tokio::select! {biased;
            _=self.closed.cancelled()=>Err(CliError::Cancelled),
            result=run(binary,args,config,limit,cancel)=>result,
        }
    }
    fn ensure_open(&self) -> Result<()> {
        if mobile() {
            Err(CliError::UnsupportedPlatform)
        } else if self.closed.is_cancelled() {
            Err(CliError::Cancelled)
        } else {
            Ok(())
        }
    }
    fn take(&self, ticket: &str) -> Result<Plan> {
        self.ensure_open()?;
        let plan = self
            .plans
            .lock()
            .map_err(|_| CliError::Cancelled)?
            .remove(ticket)
            .ok_or(CliError::Cancelled)?;
        if plan.created.elapsed() >= PLAN_LIFETIME {
            return Err(CliError::Cancelled);
        }
        Ok(plan)
    }
}

pub struct PackageCommand {
    pub executable: PathBuf,
    pub arguments: Vec<String>,
    pub environment: BTreeMap<OsString, OsString>,
    pub cancellation: CancellationToken,
}
impl std::fmt::Debug for PackageCommand {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PackageCommand")
            .field("executable", &self.executable)
            .finish_non_exhaustive()
    }
}
/// Protected output: never log or serialize automatically. The caller chooses
/// an explicit output view and drops the zeroizing buffers when it closes.
pub struct CliOutput {
    pub exit_code: i32,
    pub stdout: Zeroizing<Vec<u8>>,
    pub stderr: Zeroizing<Vec<u8>>,
}
impl std::fmt::Debug for CliOutput {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CliOutput")
            .field("exit_code", &self.exit_code)
            .finish_non_exhaustive()
    }
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum FluxAction {
    Check,
    Trace {
        kind: String,
        name: String,
        namespace: String,
    },
    Tree {
        name: String,
        namespace: String,
    },
    Events {
        namespace: String,
    },
    Logs {
        namespace: String,
    },
    Diff {
        name: String,
        namespace: String,
        path: PathBuf,
    },
}
impl FluxAction {
    pub fn arguments(&self) -> Result<Vec<String>> {
        let (mut args, namespace) = match self {
            Self::Check => (vec!["check".into()], None),
            Self::Trace {
                kind,
                name,
                namespace,
            } => {
                identifier(kind)?;
                identifier(name)?;
                (
                    vec!["trace".into(), kind.clone(), name.clone()],
                    Some(namespace),
                )
            }
            Self::Tree { name, namespace } => {
                identifier(name)?;
                (
                    vec!["tree".into(), "kustomization".into(), name.clone()],
                    Some(namespace),
                )
            }
            Self::Events { namespace } => (vec!["events".into()], Some(namespace)),
            Self::Logs { namespace } => (
                vec!["logs".into(), "--follow=false".into(), "--tail=500".into()],
                Some(namespace),
            ),
            Self::Diff {
                name,
                namespace,
                path,
            } => {
                identifier(name)?;
                if !path.is_absolute() || !path.is_dir() {
                    return Err(CliError::Invalid);
                }
                (
                    vec![
                        "diff".into(),
                        "kustomization".into(),
                        name.clone(),
                        "--path".into(),
                        path.to_string_lossy().into_owned(),
                    ],
                    Some(namespace),
                )
            }
        };
        if let Some(namespace) = namespace {
            identifier(namespace)?;
            args.extend(["--namespace".into(), namespace.clone()]);
        }
        args.push("--timeout=90s".into());
        Ok(args)
    }
}
pub(crate) fn identifier(value: &str) -> Result<()> {
    if value.is_empty()
        || value.len() > 253
        || !value
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || b".-_".contains(&c))
        || value.starts_with('-')
    {
        Err(CliError::Invalid)
    } else {
        Ok(())
    }
}
fn version_number(value: &str) -> Result<String> {
    let version = semver::Version::parse(value.strip_prefix('v').unwrap_or(value))
        .map_err(|_| CliError::Invalid)?;
    if !version.pre.is_empty() || !version.build.is_empty() || version.major != 2 {
        return Err(CliError::Invalid);
    }
    Ok(version.to_string())
}
fn mobile() -> bool {
    cfg!(any(target_os = "android", target_os = "ios"))
}
fn binary_name(name: &str) -> String {
    if cfg!(windows) {
        format!("{name}.exe")
    } else {
        name.into()
    }
}
fn search_directories() -> Vec<PathBuf> {
    let mut dirs = std::env::var_os("PATH")
        .map(|path| {
            std::env::split_paths(&path)
                .filter(|path| path.is_absolute())
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    if cfg!(target_os = "macos") {
        // Read the system login-shell path sources without executing .zprofile
        // or any shell startup file while opening the GitOps page.
        if let Ok(contents) = std::fs::read_to_string("/etc/paths") {
            dirs.extend(
                contents
                    .lines()
                    .map(PathBuf::from)
                    .filter(|path| path.is_absolute()),
            );
        }
        if let Ok(entries) = std::fs::read_dir("/etc/paths.d") {
            for entry in entries.flatten().take(64) {
                if let Ok(metadata) = entry.metadata() {
                    if metadata.is_file() && metadata.len() < 65536 {
                        if let Ok(contents) = std::fs::read_to_string(entry.path()) {
                            dirs.extend(
                                contents
                                    .lines()
                                    .map(PathBuf::from)
                                    .filter(|path| path.is_absolute()),
                            );
                        }
                    }
                }
            }
        }
    }
    dirs.extend([
        PathBuf::from("/opt/homebrew/bin"),
        PathBuf::from("/usr/local/bin"),
        PathBuf::from("/home/linuxbrew/.linuxbrew/bin"),
    ]);
    if let Some(home) = std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" }) {
        dirs.push(PathBuf::from(&home).join(".local/bin"));
        dirs.push(PathBuf::from(home).join("scoop/shims"));
    }
    if let Some(local) = std::env::var_os("LOCALAPPDATA") {
        dirs.push(PathBuf::from(local).join("Programs/flux"));
    }
    if let Some(choco) = std::env::var_os("ChocolateyInstall") {
        dirs.push(PathBuf::from(choco).join("bin"));
    }
    dirs.retain(|path| path.is_absolute());
    dirs.dedup();
    dirs.truncate(256);
    dirs
}
pub(crate) fn find(name: &str) -> Option<PathBuf> {
    search_directories()
        .into_iter()
        .find_map(|dir| executable(&dir.join(binary_name(name))).ok())
}
fn manager(method: InstallMethod) -> Option<PathBuf> {
    match method {
        InstallMethod::Homebrew if !cfg!(windows) => find("brew"),
        InstallMethod::Winget if cfg!(windows) => find("winget"),
        InstallMethod::Scoop if cfg!(windows) => find("scoop"),
        InstallMethod::Chocolatey if cfg!(windows) => find("choco"),
        _ => None,
    }
}
fn manager_arguments(
    method: InstallMethod,
    version: &str,
    operation: InstallOperation,
) -> Result<Vec<String>> {
    let action = match operation {
        InstallOperation::Install => "install",
        InstallOperation::Update => "upgrade",
        InstallOperation::Remove => "uninstall",
    };
    Ok(match method {
        InstallMethod::Homebrew => vec![action.into(), "fluxcd/tap/flux".into()],
        InstallMethod::Winget => {
            let mut args = vec![
                action.into(),
                "--id".into(),
                "FluxCD.Flux".into(),
                "--exact".into(),
            ];
            if operation != InstallOperation::Remove {
                args.extend(["--version".into(), version.into()]);
            }
            args
        }
        InstallMethod::Scoop => vec![
            if operation == InstallOperation::Update {
                "update"
            } else {
                action
            }
            .into(),
            if operation == InstallOperation::Install {
                format!("flux@{version}")
            } else {
                "flux".into()
            },
        ],
        InstallMethod::Chocolatey => {
            let mut args = vec![action.into(), "flux".into()];
            if operation != InstallOperation::Remove {
                args.push(format!("--version={version}"));
            }
            args
        }
        _ => return Err(CliError::Invalid),
    })
}
pub(crate) fn destination() -> Result<PathBuf> {
    let key = if cfg!(windows) {
        "LOCALAPPDATA"
    } else {
        "HOME"
    };
    let base = PathBuf::from(std::env::var_os(key).ok_or(CliError::Invalid)?);
    if !base.is_absolute() {
        return Err(CliError::Invalid);
    }
    Ok(base.join(if cfg!(windows) {
        "Programs/flux/flux.exe"
    } else {
        ".local/bin/flux"
    }))
}
fn executable(path: &Path) -> Result<PathBuf> {
    if !path.is_absolute() {
        return Err(CliError::Invalid);
    }
    let path = path.canonicalize().map_err(|_| CliError::NotFound)?;
    let metadata = path.metadata().map_err(|_| CliError::NotFound)?;
    if !metadata.is_file() || metadata.len() == 0 || metadata.len() > MAX_BINARY as u64 {
        return Err(CliError::Invalid);
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if metadata.permissions().mode() & 0o111 == 0 {
            return Err(CliError::Invalid);
        }
    }
    #[cfg(windows)]
    if !path
        .extension()
        .and_then(|v| v.to_str())
        .is_some_and(|v| v.eq_ignore_ascii_case("exe"))
    {
        return Err(CliError::Invalid);
    }
    Ok(path)
}
pub(crate) fn inspect(path: &Path) -> Result<Binary> {
    let path = executable(path)?;
    let hash = cc_k8s_core::auth::exec::binary_hash(&path).map_err(|_| CliError::Changed)?;
    Ok(Binary {
        path,
        hash: hash.iter().map(|byte| format!("{byte:02x}")).collect(),
    })
}
pub(crate) fn unchanged(binary: &Binary) -> Result<()> {
    let current = inspect(&binary.path)?;
    if current.hash != binary.hash || current.path != binary.path {
        Err(CliError::Changed)
    } else {
        Ok(())
    }
}
pub(crate) fn clean_environment() -> BTreeMap<OsString, OsString> {
    // No cloud credentials, KUBECONFIG, loader overrides or proxy environment.
    [
        "PATH",
        "HOME",
        "USER",
        "LOGNAME",
        "USERPROFILE",
        "SYSTEMROOT",
        "WINDIR",
        "LOCALAPPDATA",
        "APPDATA",
        "TEMP",
        "TMP",
        "LANG",
        "LC_ALL",
    ]
    .into_iter()
    .filter_map(|name| std::env::var_os(name).map(|value| (OsString::from(name), value)))
    .collect()
}
async fn read_output(mut reader: impl tokio::io::AsyncRead + Unpin) -> Result<Zeroizing<Vec<u8>>> {
    let mut output = Zeroizing::new(Vec::new());
    let mut buffer = Zeroizing::new([0u8; 8192]);
    loop {
        let read = reader
            .read(&mut buffer[..])
            .await
            .map_err(|_| CliError::Process)?;
        if read == 0 {
            return Ok(output);
        }
        if output.len() + read > MAX_OUTPUT {
            return Err(CliError::TooLarge);
        }
        output.extend_from_slice(&buffer[..read]);
    }
}
pub(crate) async fn run(
    binary: &Binary,
    args: &[String],
    config: Option<&Path>,
    limit: Duration,
    cancel: CancellationToken,
) -> Result<CliOutput> {
    if cancel.is_cancelled() {
        return Err(CliError::Cancelled);
    }
    unchanged(binary)?;
    let mut command = tokio::process::Command::new(&binary.path);
    command
        .args(args)
        .env_clear()
        .envs(clean_environment())
        .current_dir(binary.path.parent().ok_or(CliError::Invalid)?)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    // CLI never discovers ~/.kube/config even for a version-only request.
    command.env(
        "KUBECONFIG",
        config.unwrap_or_else(|| Path::new(if cfg!(windows) { "NUL" } else { "/dev/null" })),
    );
    let (mut child, _tree) =
        cc_k8s_core::auth::exec::spawn_scoped(&mut command).map_err(|_| CliError::Process)?;
    let stdout = child.stdout.take().ok_or(CliError::Process)?;
    let stderr = child.stderr.take().ok_or(CliError::Process)?;
    let work = async {
        let (status, stdout, stderr) = tokio::try_join!(
            async { child.wait().await.map_err(|_| CliError::Process) },
            read_output(stdout),
            read_output(stderr)
        )?;
        Ok(CliOutput {
            exit_code: status.code().unwrap_or(-1),
            stdout,
            stderr,
        })
    };
    tokio::select! {biased;_=cancel.cancelled()=>Err(CliError::Cancelled),result=tokio::time::timeout(limit,work)=>result.map_err(|_|CliError::Timeout)?,}
}

fn platform_asset(version: &str) -> Result<String> {
    let os = if cfg!(target_os = "macos") {
        "darwin"
    } else if cfg!(target_os = "windows") {
        "windows"
    } else if cfg!(target_os = "linux") {
        "linux"
    } else {
        return Err(CliError::UnsupportedPlatform);
    };
    let arch = if cfg!(target_arch = "aarch64") {
        "arm64"
    } else if cfg!(target_arch = "x86_64") {
        "amd64"
    } else {
        return Err(CliError::UnsupportedPlatform);
    };
    Ok(format!(
        "flux_{version}_{os}_{arch}.{}",
        if cfg!(windows) { "zip" } else { "tar.gz" }
    ))
}
fn release_client() -> Result<reqwest::Client> {
    reqwest::Client::builder()
        .no_proxy()
        .https_only(true)
        .retry(reqwest::retry::never())
        .connect_timeout(Duration::from_secs(15))
        .timeout(Duration::from_secs(180))
        .redirect(reqwest::redirect::Policy::custom(|attempt| {
            let url = attempt.url();
            if attempt.previous().len() >= 5
                || url.scheme() != "https"
                || !url.username().is_empty()
                || url.password().is_some()
                || !matches!(
                    url.host_str(),
                    Some(
                        "github.com"
                            | "release-assets.githubusercontent.com"
                            | "objects.githubusercontent.com"
                    )
                )
            {
                attempt.error("Release redirect rejected")
            } else {
                attempt.follow()
            }
        }))
        .user_agent("ConsoleCrypt Flux installer")
        .build()
        .map_err(|_| CliError::Download)
}
async fn download(client: &reqwest::Client, url: &str, limit: usize) -> Result<Vec<u8>> {
    let response = client
        .get(url)
        .send()
        .await
        .map_err(|_| CliError::Download)?;
    if !response.status().is_success() {
        return Err(CliError::Download);
    }
    if response
        .content_length()
        .is_some_and(|size| size > limit as u64)
    {
        return Err(CliError::TooLarge);
    }
    let mut stream = response.bytes_stream();
    let mut bytes = Vec::new();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|_| CliError::Download)?;
        if bytes.len() + chunk.len() > limit {
            return Err(CliError::TooLarge);
        }
        bytes.extend_from_slice(&chunk);
    }
    Ok(bytes)
}
fn checksum(manifest: &[u8], asset: &str) -> Result<String> {
    let text = std::str::from_utf8(manifest).map_err(|_| CliError::Verification)?;
    let mut found = None;
    for line in text.lines() {
        let fields = line.split_whitespace().collect::<Vec<_>>();
        if fields.len() != 2 {
            return Err(CliError::Verification);
        }
        if fields[1].trim_start_matches('*') == asset {
            if found.is_some()
                || fields[0].len() != 64
                || !fields[0].bytes().all(|b| b.is_ascii_hexdigit())
            {
                return Err(CliError::Verification);
            }
            found = Some(fields[0].to_ascii_lowercase());
        }
    }
    found.ok_or(CliError::Verification)
}
fn certificate_pem(bytes: Vec<u8>) -> Result<Vec<u8>> {
    let decoded = if bytes.starts_with(b"-----BEGIN CERTIFICATE-----") {
        bytes
    } else {
        STANDARD
            .decode(
                bytes
                    .iter()
                    .copied()
                    .filter(|b| !b.is_ascii_whitespace())
                    .collect::<Vec<_>>(),
            )
            .map_err(|_| CliError::Verification)?
    };
    let certificates = pem::parse_many(&decoded).map_err(|_| CliError::Verification)?;
    if certificates.is_empty()
        || certificates.len() > 8
        || certificates.iter().any(|cert| cert.tag() != "CERTIFICATE")
    {
        return Err(CliError::Verification);
    }
    Ok(decoded)
}
fn cosign_arguments(
    version: &str,
    manifest: &Path,
    signature: &Path,
    certificate: &Path,
) -> Vec<String> {
    vec![
        "verify-blob".into(),
        "--certificate".into(),
        certificate.to_string_lossy().into_owned(),
        "--signature".into(),
        signature.to_string_lossy().into_owned(),
        "--certificate-identity".into(),
        format!(
            "https://github.com/fluxcd/flux2/.github/workflows/release.yaml@refs/tags/v{version}"
        ),
        "--certificate-oidc-issuer".into(),
        "https://token.actions.githubusercontent.com".into(),
        manifest.to_string_lossy().into_owned(),
    ]
}
async fn install(plan: &Plan, cancel: CancellationToken) -> Result<CliVersion> {
    unchanged(&plan.binary)?;
    let version = &plan.review.version;
    let asset = platform_asset(version)?;
    let base = format!("https://github.com/fluxcd/flux2/releases/download/v{version}");
    let manifest_name = format!("flux_{version}_checksums.txt");
    let client = release_client()?;
    let manifest_url = format!("{base}/{manifest_name}");
    let signature_url = format!("{base}/{manifest_name}.sig");
    let certificate_url = format!("{base}/{manifest_name}.pem");
    let (manifest, signature, certificate) = tokio::try_join!(
        download(&client, &manifest_url, 1024 * 1024),
        download(&client, &signature_url, 65536),
        download(&client, &certificate_url, 65536)
    )?;
    let certificate = certificate_pem(certificate)?;
    let stage = tempfile::Builder::new()
        .prefix("consolecrypt-flux-verify-")
        .tempdir()
        .map_err(|_| CliError::Install)?;
    let manifest_path = stage.path().join("checksums.txt");
    let signature_path = stage.path().join("checksums.sig");
    let certificate_path = stage.path().join("checksums.pem");
    std::fs::write(&manifest_path, &manifest).map_err(|_| CliError::Install)?;
    std::fs::write(&signature_path, &signature).map_err(|_| CliError::Install)?;
    std::fs::write(&certificate_path, &certificate).map_err(|_| CliError::Install)?;
    let arguments = cosign_arguments(version, &manifest_path, &signature_path, &certificate_path);
    let verification = run(
        &plan.binary,
        &arguments,
        None,
        Duration::from_secs(120),
        cancel.clone(),
    )
    .await?;
    if verification.exit_code != 0 {
        return Err(CliError::Verification);
    }
    let expected = checksum(&manifest, &asset)?;
    let archive = download(&client, &format!("{base}/{asset}"), MAX_ARCHIVE).await?;
    if sha256(&archive) != expected {
        return Err(CliError::Verification);
    }
    let executable_name = binary_name("flux");
    let binary = tokio::task::spawn_blocking(move || {
        extract_binary(&archive, &executable_name, cfg!(windows))
    })
    .await
    .map_err(|_| CliError::Install)??;
    if cancel.is_cancelled() {
        return Err(CliError::Cancelled);
    }
    // Verify the signed, extracted binary before publishing it to the user's
    // command search path. A failed install leaves the previous version intact.
    let stage_binary = stage.path().join(binary_name("flux"));
    std::fs::write(&stage_binary, &binary).map_err(|_| CliError::Install)?;
    make_executable(&stage_binary)?;
    let binary = inspect(&stage_binary)?;
    let probe = run(
        &binary,
        &[
            "version".into(),
            "--client".into(),
            "-o".into(),
            "json".into(),
        ],
        None,
        Duration::from_secs(15),
        cancel.clone(),
    )
    .await?;
    if probe.exit_code != 0 {
        return Err(CliError::Verification);
    }
    let data: serde_json::Value =
        serde_json::from_slice(&probe.stdout).map_err(|_| CliError::Verification)?;
    let actual = data
        .get("flux")
        .and_then(|v| v.get("version").or(Some(v)))
        .and_then(|v| v.as_str())
        .or_else(|| data.get("version").and_then(|v| v.as_str()))
        .ok_or(CliError::Verification)?;
    if version_number(actual)? != *version {
        return Err(CliError::Verification);
    }
    publish(
        &stage_binary,
        &plan.review.destination,
        plan.previous.as_deref(),
        &cancel,
    )?;
    Ok(CliVersion {
        path: plan.review.destination.clone(),
        version: version.clone(),
        sha256: binary.hash,
    })
}
fn extract_binary(archive: &[u8], name: &str, windows: bool) -> Result<Vec<u8>> {
    let mut found = None;
    if windows {
        let mut zip = zip::ZipArchive::new(std::io::Cursor::new(archive))
            .map_err(|_| CliError::Verification)?;
        if zip.len() > 32 {
            return Err(CliError::Verification);
        }
        for index in 0..zip.len() {
            let mut entry = zip.by_index(index).map_err(|_| CliError::Verification)?;
            if entry.is_symlink()
                || entry.is_dir()
                || entry.enclosed_name().is_none()
                || entry
                    .name()
                    .map_err(|_| CliError::Verification)?
                    .contains('/')
                || entry
                    .name()
                    .map_err(|_| CliError::Verification)?
                    .contains('\\')
                || entry.size() > MAX_BINARY as u64
            {
                return Err(CliError::Verification);
            }
            if entry.name().map_err(|_| CliError::Verification)? != name {
                continue;
            }
            if found.is_some() {
                return Err(CliError::Verification);
            }
            let mut bytes = Vec::new();
            entry
                .by_ref()
                .take(MAX_BINARY as u64 + 1)
                .read_to_end(&mut bytes)
                .map_err(|_| CliError::Verification)?;
            if bytes.is_empty() || bytes.len() > MAX_BINARY {
                return Err(CliError::TooLarge);
            }
            found = Some(bytes);
        }
    } else {
        // A decompression cap covers ignored entries as well as the executable.
        let decoder = flate2::read::GzDecoder::new(archive);
        let mut tar = tar::Archive::new(decoder.take(MAX_BINARY as u64 + 1024 * 1024));
        for (index, entry) in tar
            .entries()
            .map_err(|_| CliError::Verification)?
            .enumerate()
        {
            if index >= 32 {
                return Err(CliError::Verification);
            }
            let mut entry = entry.map_err(|_| CliError::Verification)?;
            let path = entry.path().map_err(|_| CliError::Verification)?;
            if !entry.header().entry_type().is_file()
                || path.components().count() != 1
                || path.file_name().is_none()
                || entry.size() > MAX_BINARY as u64
            {
                return Err(CliError::Verification);
            }
            if path != Path::new(name) {
                continue;
            }
            if found.is_some() {
                return Err(CliError::Verification);
            }
            let mut bytes = Vec::new();
            entry
                .by_ref()
                .take(MAX_BINARY as u64 + 1)
                .read_to_end(&mut bytes)
                .map_err(|_| CliError::Verification)?;
            if bytes.is_empty() || bytes.len() > MAX_BINARY {
                return Err(CliError::TooLarge);
            }
            found = Some(bytes);
        }
    }
    found.ok_or(CliError::Verification)
}
fn make_executable(path: &Path) -> Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700))
            .map_err(|_| CliError::Install)?;
    }
    let _ = path;
    Ok(())
}
fn publish(
    source: &Path,
    destination: &Path,
    previous: Option<&str>,
    cancel: &CancellationToken,
) -> Result<()> {
    let parent = destination.parent().ok_or(CliError::Invalid)?;
    std::fs::create_dir_all(parent).map_err(|_| CliError::Install)?;
    // Do not replace a symlink or a destination changed since confirmation.
    match std::fs::symlink_metadata(destination) {
        Ok(metadata) => {
            if metadata.file_type().is_symlink() || !metadata.is_file() {
                return Err(CliError::Changed);
            }
            if Some(inspect(destination)?.hash.as_str()) != previous {
                return Err(CliError::Changed);
            }
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            if previous.is_some() {
                return Err(CliError::Changed);
            }
        }
        Err(_) => return Err(CliError::Install),
    }
    let mut staged = tempfile::NamedTempFile::new_in(parent).map_err(|_| CliError::Install)?;
    let mut source = std::fs::File::open(source).map_err(|_| CliError::Install)?;
    std::io::copy(&mut source, &mut staged).map_err(|_| CliError::Install)?;
    staged.flush().map_err(|_| CliError::Install)?;
    staged.as_file().sync_all().map_err(|_| CliError::Install)?;
    make_executable(staged.path())?;
    if cancel.is_cancelled() {
        return Err(CliError::Cancelled);
    }
    staged.persist(destination).map_err(|_| CliError::Install)?;
    Ok(())
}

#[cfg(test)]
#[path = "kubernetes_cli_tests.rs"]
mod tests;

fn sha256(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

struct InstallGuard<'a> {
    runtime: &'a FluxCliRuntime,
    ticket: String,
}
impl Drop for InstallGuard<'_> {
    fn drop(&mut self) {
        if let Ok(mut active) = self.runtime.active.lock() {
            if let Some(cancel) = active.remove(&self.ticket) {
                cancel.cancel();
            }
        }
    }
}

pub(crate) struct TrackedOperation<'a> {
    runtime: &'a FluxCliRuntime,
    ticket: String,
    pub(crate) cancellation: CancellationToken,
}
impl Drop for TrackedOperation<'_> {
    fn drop(&mut self) {
        self.cancellation.cancel();
        if let Ok(mut active) = self.runtime.active.lock() {
            active.remove(&self.ticket);
        }
    }
}
