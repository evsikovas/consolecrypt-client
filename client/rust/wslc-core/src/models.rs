use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, thiserror::Error)]
#[error("{message} ({operation}, {code})")]
pub struct AppError {
    pub code: String,
    pub message: String,
    pub operation: String,
    pub diagnostics: Option<String>,
}

impl AppError {
    pub fn new(code: &str, operation: &str, message: &str) -> Self {
        Self {
            code: code.into(),
            operation: operation.into(),
            message: message.into(),
            diagnostics: None,
        }
    }
}

pub type AppResult<T> = Result<T, AppError>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ContainerState {
    Created,
    Running,
    Exited,
    Unknown,
}

impl ContainerState {
    pub fn from_raw(raw: Option<&str>) -> Self {
        match raw {
            Some("created") => Self::Created,
            Some("running") => Self::Running,
            Some("exited") => Self::Exited,
            // In 3.0.1 "stopped" represents the Deleted enum, not a restartable container.
            _ => Self::Unknown,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContainerSummary {
    pub id: String,
    pub name: Option<String>,
    pub image: Option<String>,
    pub state: ContainerState,
    pub raw_state: Option<String>,
    // An empty list-output Ports string on an exited container does not prove no mappings.
    pub ports_display: Option<String>,
    pub created_at: Option<String>,
    pub health_status: Option<String>,
    // Human-readable runtime status such as "Up 5 minutes" (absent in 3.0.1).
    pub status_display: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MountInfo {
    pub kind: Option<String>,
    pub name: Option<String>,
    pub source: Option<String>,
    pub destination: Option<String>,
    pub read_write: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PortBinding {
    pub container_port: String,
    pub host_ip: Option<String>,
    pub host_port: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkAttachment {
    pub name: String,
    pub ip_address: Option<String>,
    pub gateway: Option<String>,
    pub mac_address: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContainerDetails {
    pub id: String,
    pub name: Option<String>,
    pub image_id: Option<String>,
    pub image_name: Option<String>,
    pub state: ContainerState,
    pub raw_state: Option<String>,
    pub auto_remove: Option<bool>,
    pub stop_timeout_seconds: Option<u64>,
    pub exit_code: Option<i64>,
    pub created_at: Option<String>,
    pub started_at: Option<String>,
    pub finished_at: Option<String>,
    pub health_status: Option<String>,
    pub command: Option<String>,
    pub mounts: Option<Vec<MountInfo>>,
    // Informational fields added after 3.0.1; empty when the runtime does not report them.
    pub ports: Vec<PortBinding>,
    pub networks: Vec<NetworkAttachment>,
    pub inspect_json: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImageSummary {
    pub id: String,
    pub repository: Option<String>,
    pub tag: Option<String>,
    pub size_display: Option<String>,
    pub created_at: Option<String>,
    /// Containers created from this image (absent in 3.0.1).
    pub containers: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VolumeSummary {
    pub name: String,
    pub driver: Option<String>,
    // Size from list is N/A; no claim about disk usage.
    pub size_display: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkSummary {
    pub id: String,
    pub name: Option<String>,
    pub driver: Option<String>,
    pub scope: Option<String>,
    pub ipv4: Option<bool>,
    pub ipv6: Option<bool>,
    pub internal: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PortSpec {
    /// `None` keeps the WSLc default (127.0.0.1); `0.0.0.0` publishes on every interface.
    pub host_ip: Option<String>,
    pub host_port: u16,
    pub container_port: u16,
    pub udp: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MountSpec {
    /// Named volume when true, otherwise an existing Windows folder or file.
    pub volume: bool,
    pub source: String,
    pub target: String,
    pub read_only: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnvVar {
    pub key: String,
    pub value: String,
}

/// One variable of a container; `from_image` marks values inherited unchanged from the image.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnvEntry {
    pub key: String,
    pub value: String,
    pub from_image: bool,
}

/// Saved preferences. Every field may be absent, so older and newer files keep loading.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct DeskSettings {
    /// `light`, `dark` or `system`.
    pub theme: Option<String>,
    /// Optional opaque sRGB colors, written as `#RRGGBB`; absent keeps the built-in palette.
    pub accent_color: Option<String>,
    pub background_tint: Option<String>,
    pub refresh_seconds: Option<u32>,
    pub notifications: Option<bool>,
    pub close_to_tray: Option<bool>,
    /// A `wslc.exe` outside the default location.
    pub cli_path: Option<String>,
    #[serde(deserialize_with = "crate::settings::lenient_templates")]
    pub templates: Vec<ContainerTemplate>,
}

/// Wizard settings saved under a name for the next container.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContainerTemplate {
    pub name: String,
    pub spec: RunSpec,
}

/// Everything the "new container" wizard can set; turned into validated `wslc run` arguments.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunSpec {
    pub image: String,
    pub name: Option<String>,
    pub ports: Vec<PortSpec>,
    pub mounts: Vec<MountSpec>,
    pub env: Vec<EnvVar>,
    pub command: Vec<String>,
    pub entrypoint: Option<String>,
    pub workdir: Option<String>,
    pub user: Option<String>,
    pub network: Option<String>,
    pub cpus: Option<String>,
    pub memory: Option<String>,
    pub auto_remove: bool,
    /// `run -d` when true, `create` otherwise.
    pub start: bool,
    /// Less common `run` options, filled by Docker Compose import.
    pub extras: Option<RunExtras>,
}

/// `wslc run` options beyond the wizard form.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct RunExtras {
    pub labels: Vec<EnvVar>,
    pub hostname: Option<String>,
    pub domainname: Option<String>,
    /// Aliases on the network the container is created in.
    pub network_aliases: Vec<String>,
    /// Fixed IPv4 address on that network.
    pub ip: Option<String>,
    pub health: Option<HealthSpec>,
    pub no_healthcheck: bool,
    /// `path[:options]`, e.g. `/run:size=64m`.
    pub tmpfs: Vec<String>,
    /// `name=soft[:hard]`.
    pub ulimits: Vec<String>,
    pub shm_size: Option<String>,
    pub stop_signal: Option<String>,
    pub stop_timeout: Option<u32>,
    pub dns: Vec<String>,
    pub dns_search: Vec<String>,
    pub dns_options: Vec<String>,
    pub tty: bool,
    pub interactive: bool,
    /// `all` or a GPU count.
    pub gpus: Option<String>,
}

/// Container health check; durations as `30s`, `1m30s`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HealthSpec {
    /// Shell command, as `--health-cmd` expects.
    pub cmd: String,
    pub interval: Option<String>,
    pub timeout: Option<String>,
    pub retries: Option<u32>,
    pub start_period: Option<String>,
}

/// What `docker compose up` would do, translated to WSLc steps.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComposePlan {
    pub project: String,
    /// Folder relative paths, `.env` and build contexts resolve against.
    pub working_dir: Option<String>,
    pub networks: Vec<ComposeNetwork>,
    pub volumes: Vec<ComposeVolume>,
    /// In start order: dependencies first.
    pub services: Vec<ComposeService>,
    /// Bind-mount folders created when missing, as Compose does.
    pub host_folders: Vec<String>,
    /// Network and volume commands, for the preview.
    pub commands: Vec<String>,
    /// Things WSLc cannot do or that work differently; shown before anything runs.
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComposeNetwork {
    /// Key in the file (`default`, `backend`…).
    pub key: String,
    pub is_external: bool,
    pub spec: NetworkSpec,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComposeVolume {
    pub key: String,
    pub is_external: bool,
    pub spec: VolumeSpec,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComposeService {
    pub service: String,
    pub container_name: String,
    pub image: String,
    /// Built before the container is created; the image name is its tag.
    pub build: Option<BuildSpec>,
    /// `missing`, `always`, `never` or `build`.
    pub pull_policy: String,
    /// Always `create`; the runner starts it once its networks are attached.
    pub spec: RunSpec,
    /// Networks after the first, connected before start.
    pub extra_networks: Vec<ComposeAttach>,
    pub depends_on: Vec<ComposeDependency>,
    /// Same hash as last time: the existing container is kept.
    pub config_hash: String,
    /// The commands this service turns into, for the preview.
    pub commands: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComposeAttach {
    pub network: String,
    pub aliases: Vec<String>,
    pub ip: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComposeDependency {
    pub service: String,
    /// `started`, `healthy` or `completed`.
    pub condition: String,
}

/// A container that belongs to a Compose project, read from its labels.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComposeContainer {
    pub id: String,
    pub name: String,
    pub service: Option<String>,
    pub state: ContainerState,
    pub config_hash: Option<String>,
    /// `starting`, `healthy`, `unhealthy` when the container has a health check.
    pub health: Option<String>,
    pub exit_code: Option<i64>,
}

/// One `wslc stats` snapshot row. Values are display strings from the runtime.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContainerStats {
    pub id: String,
    pub cpu_percent: Option<f64>,
    pub memory_percent: Option<f64>,
    pub memory_usage: Option<String>,
    pub net_io: Option<String>,
    pub block_io: Option<String>,
    pub pids: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileEntry {
    pub name: String,
    pub is_dir: bool,
    pub is_link: bool,
}

/// A runtime event: `kind` is container/image/volume/network, `action` e.g. start or die.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeskEvent {
    pub kind: String,
    pub action: String,
    pub id: String,
    pub name: Option<String>,
    pub image: Option<String>,
    pub exit_code: Option<i64>,
    pub time: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JobLine {
    pub text: String,
    /// Ended by a lone CR: replaces the previous progress line.
    pub progress: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JobBatch {
    pub lines: Vec<JobLine>,
    pub finished: bool,
    pub success: bool,
    pub exit_code: Option<i32>,
}

/// What one container uses, for the relations map.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContainerLinks {
    pub id: String,
    pub name: Option<String>,
    pub state: ContainerState,
    pub image_id: Option<String>,
    pub image_name: Option<String>,
    pub networks: Vec<String>,
    pub volumes: Vec<String>,
    /// Windows folders bind-mounted into the container.
    pub binds: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VolumeSpec {
    pub name: String,
    /// `guest` (default) or `vhd`.
    pub driver: Option<String>,
    pub options: Vec<EnvVar>,
    #[serde(default)]
    pub labels: Vec<EnvVar>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkSpec {
    pub name: String,
    pub driver: Option<String>,
    pub subnet: Option<String>,
    pub gateway: Option<String>,
    pub ip_range: Option<String>,
    pub internal: bool,
    #[serde(default)]
    pub labels: Vec<EnvVar>,
}

/// `wslc build` options from the build dialog.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BuildSpec {
    /// Existing Windows folder used as the build context.
    pub context: String,
    /// Dockerfile path; the context's Dockerfile when absent.
    pub dockerfile: Option<String>,
    pub tags: Vec<String>,
    pub build_args: Vec<EnvVar>,
    pub target: Option<String>,
    pub no_cache: bool,
    pub pull: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TerminalOutput {
    pub data: Vec<u8>,
    pub alive: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventBatch {
    pub events: Vec<DeskEvent>,
    pub running: bool,
}

/// History window for a log subscription; `follow` is always on.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogOptions {
    /// Last N lines; `None` with `since` reads everything after that moment.
    pub tail: Option<u32>,
    /// Unix seconds or RFC 3339, as accepted by `wslc container logs --since`.
    pub since: Option<String>,
    pub timestamps: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogEntry {
    pub sequence: u64,
    pub stream: String,
    pub text: String,
    pub truncated: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogBatch {
    pub entries: Vec<LogEntry>,
    pub status: String,
    pub dropped_total: u64,
    pub truncated_total: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimeInfo {
    pub cli_path: String,
    pub cli_version: String,
    pub version_supported: bool,
    // False for a newer 3.x release that is accepted but has no recorded compatibility matrix.
    pub version_verified: bool,
    pub readable: bool,
    pub error: Option<AppError>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ContainerAction {
    Start,
    Stop,
    Restart,
}

impl ContainerAction {
    pub fn command(self) -> &'static str {
        match self {
            Self::Start => "start",
            Self::Stop => "stop",
            Self::Restart => "restart",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OperationResult {
    pub container_id: String,
    pub uncertain: bool,
    pub command_error: Option<AppError>,
    pub current: Option<ContainerDetails>,
    pub verification_error: Option<AppError>,
}
