//! Core lifecycle, app events, prompts, the server probe and the
//! device-local UI store.

use crate::api::error::BridgeError;
use crate::frb_generated::StreamSink;
use crate::state::{self, run, to_json};
use cc_app_core::platform::{ExternalOsAuthenticator, OsAuthAvailability, OsAuthKind};
use cc_app_core::{AppConfig, AppCore, AppError, HostKeyDecision, KdfPolicy, SecureStoreKind};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};
use tokio::sync::broadcast::error::RecvError;

/// Where the core keeps database keys, device identities and tokens.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SecureStoreChoice {
    /// OS keychain (macOS Keychain / Windows Credential Manager). Production.
    Os,
    /// Process memory only — nothing survives a restart (tests, demos).
    Memory,
}

/// Argon2id cost for new password envelopes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KdfChoice {
    /// Calibrate on this machine (production, ~0.75 s).
    Calibrate,
    /// ADR-0002 default parameters.
    Standard,
    /// Server floor — tests only.
    Floor,
}

/// Start-up configuration from Dart.
#[derive(Debug, Clone)]
pub struct CoreConfig {
    /// Data root; `None` = `CONSOLECRYPT_DATA_DIR` or the platform default.
    pub data_dir: Option<String>,
    pub secure_store: SecureStoreChoice,
    /// Keychain service name; `None` = `io.consolecrypt.ConsoleCrypt`.
    pub keychain_service: Option<String>,
    pub kdf: KdfChoice,
    /// App version (`x-cc-client-version`, backups).
    pub client_version: String,
    /// Default device name for registrations; `None` = host name.
    pub device_name: Option<String>,
    pub background_sync: bool,
    pub auto_start_tunnels: bool,
    /// `tracing` directives (stderr); `None` = `CC_LOG` or app-core defaults.
    pub log_directives: Option<String>,
}

/// Result of [`core_init`].
#[derive(Debug, Clone)]
pub struct CoreInfo {
    pub data_dir: String,
    /// `macos` | `windows` | `linux`.
    pub platform: String,
    pub core_version: String,
    /// The core was already running (Flutter hot restart); config ignored.
    pub already_initialized: bool,
}

/// Answer to an unknown-host-key prompt.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HostKeyAnswer {
    AcceptAndSave,
    AcceptOnce,
    Reject,
}

/// What the OS offers for user-presence checks (reported by the UI after
/// querying LocalAuthentication / Windows Hello).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OsAuthKindChoice {
    TouchId,
    FaceId,
    WindowsHello,
    /// OS account password / PIN.
    DeviceCredential,
}

/// The core's OS authenticator: fed by the UI's platform prompt (Touch ID
/// via the `consolecrypt/local_auth` channel), consumed once by the next
/// device unlock / passphrase reset (ADR-0101 §8, ADR-0107).
pub(crate) fn os_auth() -> Arc<ExternalOsAuthenticator> {
    static OS_AUTH: OnceLock<Arc<ExternalOsAuthenticator>> = OnceLock::new();
    OS_AUTH
        .get_or_init(|| Arc::new(ExternalOsAuthenticator::new()))
        .clone()
}

/// Report what the OS can do for user-presence checks (`None` = nothing;
/// `not_enrolled` = hardware without enrolled biometrics / passcode).
#[flutter_rust_bridge::frb(sync)]
pub fn os_auth_report(kind: Option<OsAuthKindChoice>, not_enrolled: bool) {
    let availability = match kind {
        Some(k) => OsAuthAvailability::Available(match k {
            OsAuthKindChoice::TouchId => OsAuthKind::TouchId,
            OsAuthKindChoice::FaceId => OsAuthKind::FaceId,
            OsAuthKindChoice::WindowsHello => OsAuthKind::WindowsHello,
            OsAuthKindChoice::DeviceCredential => OsAuthKind::DeviceCredential,
        }),
        None if not_enrolled => OsAuthAvailability::NotEnrolled,
        None => OsAuthAvailability::Unsupported,
    };
    os_auth().set_availability(availability);
}

#[flutter_rust_bridge::frb(init)]
pub fn init_app() {
    flutter_rust_bridge::setup_default_user_utils();
}

fn app_config(config: &CoreConfig) -> AppConfig {
    let mut c = AppConfig::new(cc_app_core::current_platform());
    c.data_dir = config.data_dir.clone().filter(|d| !d.trim().is_empty());
    c.secure_store = match config.secure_store {
        SecureStoreChoice::Os => SecureStoreKind::Os,
        SecureStoreChoice::Memory => SecureStoreKind::InMemory,
    };
    c.keychain_service = config
        .keychain_service
        .clone()
        .filter(|s| !s.trim().is_empty());
    c.kdf = match config.kdf {
        KdfChoice::Calibrate => KdfPolicy::Calibrate,
        KdfChoice::Standard => KdfPolicy::Default,
        KdfChoice::Floor => KdfPolicy::Floor,
    };
    if !config.client_version.trim().is_empty() {
        c.client_version = config.client_version.trim().to_owned();
    }
    if let Some(name) = config.device_name.as_deref().map(str::trim) {
        if !name.is_empty() && !name.chars().any(char::is_control) {
            c.device_name = name.chars().take(64).collect();
        }
    }
    c.background_sync = config.background_sync;
    c.auto_start_tunnels = config.auto_start_tunnels;
    c
}

fn info(core: &AppCore, already_initialized: bool) -> CoreInfo {
    CoreInfo {
        data_dir: core.data_dir(),
        platform: cc_app_core::current_platform().as_str().to_owned(),
        core_version: env!("CARGO_PKG_VERSION").to_owned(),
        already_initialized,
    }
}

/// Create the global core (idempotent: a running core is kept, e.g. after a
/// Flutter hot restart). Call once at start-up, before anything else.
pub async fn core_init(config: CoreConfig) -> Result<CoreInfo, BridgeError> {
    if let Some(core) = state::core_opt() {
        return Ok(info(&core, true));
    }
    let directives = config
        .log_directives
        .clone()
        .or_else(|| std::env::var("CC_LOG").ok())
        .filter(|d| !d.trim().is_empty());
    // A global subscriber can only be installed once per process.
    let _ = cc_app_core::logging::init(directives.as_deref());
    let app_config = app_config(&config);
    let auth = os_auth();
    // OS keychain initialization is blocking. In particular, Secret Service's
    // blocking executor must never be entered from a Tokio async worker.
    let core = run(initialize_blocking(move || {
        AppCore::with_os_authenticator(app_config, auth).map_err(BridgeError::from)
    }))
    .await?;
    let out = info(&core, false);
    state::set_core(Some(core));
    tracing::info!(data_dir = %out.data_dir, "bridge core initialized");
    Ok(out)
}

/// Keep blocking construction off the core's async workers. Join failures
/// have a fixed message: a panic payload may contain sensitive backend data.
async fn initialize_blocking<T, F>(construct: F) -> Result<T, BridgeError>
where
    T: Send + 'static,
    F: FnOnce() -> Result<T, BridgeError> + Send + 'static,
{
    tokio::task::spawn_blocking(construct)
        .await
        .map_err(|_| BridgeError::internal("core initialization task failed"))?
}

/// Close the open profile (locks the vault, stops sync/SSH, closes the
/// SQLCipher connection) and drop the core. Call before the process exits.
pub async fn core_shutdown() -> Result<(), BridgeError> {
    let Some(core) = state::core_opt() else {
        return Ok(());
    };
    let r = run(async move { core.shutdown().await.map_err(BridgeError::from) }).await;
    state::set_core(None);
    r
}

#[flutter_rust_bridge::frb(sync)]
pub fn core_is_initialized() -> bool {
    state::core_opt().is_some()
}

#[flutter_rust_bridge::frb(sync)]
pub fn core_data_dir() -> Result<String, BridgeError> {
    Ok(state::core()?.data_dir())
}

/// App events (`AppEvent` JSON, serde tag `type`). A `{"type":"lagged"}`
/// item means events were dropped: re-read everything.
pub fn core_events(sink: StreamSink<String>) -> Result<(), BridgeError> {
    let mut rx = state::core()?.subscribe_events();
    state::runtime().spawn(async move {
        loop {
            let item = match rx.recv().await {
                Ok(ev) => match serde_json::to_string(&ev) {
                    Ok(json) => json,
                    Err(e) => {
                        tracing::warn!(error = %e, "cannot encode app event");
                        continue;
                    }
                },
                Err(RecvError::Lagged(_)) => r#"{"type":"lagged"}"#.to_owned(),
                Err(RecvError::Closed) => break,
            };
            if sink.add(item).is_err() {
                break;
            }
        }
    });
    Ok(())
}

/// Questions the UI must answer (`PromptRequest` JSON:
/// `{"HostKey":{..}}`, `{"Passphrase":{..}}`, `{"Password":{..}}`) — for
/// terminals, SFTP, tunnels and exec alike (the Dart `PromptService` shows
/// the ones no terminal tab claims). Subscribe before connecting: without a
/// subscriber prompts are declined.
pub fn core_prompts(sink: StreamSink<String>) -> Result<(), BridgeError> {
    let mut rx = state::core()?.subscribe_prompts();
    state::runtime().spawn(async move {
        loop {
            match rx.recv().await {
                Ok(p) => {
                    let Ok(json) = serde_json::to_string(&p) else {
                        continue;
                    };
                    if sink.add(json).is_err() {
                        break;
                    }
                }
                Err(RecvError::Lagged(n)) => {
                    tracing::warn!(dropped = n, "prompt subscriber lagged");
                }
                Err(RecvError::Closed) => break,
            }
        }
    });
    Ok(())
}

pub fn prompt_answer_host_key(
    request_id: String,
    answer: HostKeyAnswer,
) -> Result<(), BridgeError> {
    let decision = match answer {
        HostKeyAnswer::AcceptAndSave => HostKeyDecision::AcceptAndSave,
        HostKeyAnswer::AcceptOnce => HostKeyDecision::AcceptOnce,
        HostKeyAnswer::Reject => HostKeyDecision::Reject,
    };
    Ok(state::core()?.answer_host_key_prompt(request_id, decision)?)
}

/// `None` cancels. Used for this connection only, never stored.
pub fn prompt_answer_password(
    request_id: String,
    password: Option<Vec<u8>>,
) -> Result<(), BridgeError> {
    let password = state::opt_secret_string("password", password)?;
    Ok(state::core()?.answer_password_prompt(request_id, password)?)
}

/// `None` cancels.
pub fn prompt_answer_passphrase(
    request_id: String,
    passphrase: Option<Vec<u8>>,
) -> Result<(), BridgeError> {
    let passphrase = state::opt_secret_string("passphrase", passphrase)?;
    Ok(state::core()?.answer_passphrase_prompt(request_id, passphrase)?)
}

/// `GET /v1/meta` of a server (no account needed) → `ServerInfo` JSON.
pub async fn server_probe(
    server_url: String,
    client_version: String,
) -> Result<String, BridgeError> {
    run(async move {
        let cfg = cc_sync_core::ApiConfig::new(
            server_url.trim(),
            client_version,
            cc_app_core::current_platform(),
        )
        .map_err(AppError::from)?;
        let api =
            cc_sync_core::ApiClient::new(cfg, Arc::new(cc_sync_core::MemoryTokenStore::new()))
                .map_err(AppError::from)?;
        let info = api.meta().await.map_err(AppError::from)?;
        to_json(&info)
    })
    .await
}

// ---- device-local UI store ---------------------------------------------------------

/// Serializes read-modify-write of the store file.
static UI_STORE_LOCK: Mutex<()> = Mutex::new(());

const UI_STORE_FILE: &str = "ui-state.json";

fn ui_store_path() -> Result<PathBuf, BridgeError> {
    Ok(Path::new(&state::core()?.data_dir()).join(UI_STORE_FILE))
}

pub(crate) fn read_store(path: &Path) -> BTreeMap<String, String> {
    std::fs::read(path)
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default()
}

pub(crate) fn write_store(path: &Path, map: &BTreeMap<String, String>) -> Result<(), BridgeError> {
    use std::io::Write;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| BridgeError::new("io", e.to_string()))?;
    }
    let bytes = serde_json::to_vec_pretty(map).map_err(|e| BridgeError::internal(e.to_string()))?;
    let tmp = path.with_extension("json.tmp");
    let io = |e: std::io::Error| BridgeError::new("io", format!("ui store: {e}"));
    {
        let mut o = std::fs::OpenOptions::new();
        o.write(true).create(true).truncate(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            o.mode(0o600);
        }
        let mut f = o.open(&tmp).map_err(io)?;
        f.write_all(&bytes).map_err(io)?;
        f.sync_all().map_err(io)?;
    }
    std::fs::rename(&tmp, path).map_err(io)
}

/// Device-local, **non-secret** UI state (`<data dir>/ui-state.json`):
/// `LocalSettings` (language, theme, …), backup schedules, the list of
/// recent backups, pending Recovery Kit checks. Never synced.
pub fn ui_store_get(key: String) -> Result<Option<String>, BridgeError> {
    let path = ui_store_path()?;
    let _g = UI_STORE_LOCK.lock().unwrap_or_else(|p| p.into_inner());
    Ok(read_store(&path).remove(&key))
}

/// Set (`Some`) or remove (`None`) a key of the device-local UI store.
pub fn ui_store_set(key: String, value: Option<String>) -> Result<(), BridgeError> {
    let path = ui_store_path()?;
    let _g = UI_STORE_LOCK.lock().unwrap_or_else(|p| p.into_inner());
    let mut map = read_store(&path);
    match value {
        Some(v) => {
            map.insert(key, v);
        }
        None => {
            map.remove(&key);
        }
    }
    write_store(&path, &map)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blocking_initialization_can_enter_a_sync_executor_from_the_core_runtime() {
        let result = state::tests::futures_lite_block_on(run(initialize_blocking(|| {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap();
            Ok(runtime.block_on(async { 7 }))
        })));
        assert_eq!(result.unwrap(), 7);
    }

    #[test]
    fn blocking_initialization_preserves_structured_errors() {
        let result = state::tests::futures_lite_block_on(run(initialize_blocking(|| {
            Err::<(), _>(BridgeError::new(
                "secure_store",
                "test provider unavailable",
            ))
        })));
        let error = result.unwrap_err();
        assert_eq!(error.code, "secure_store");
        assert_eq!(error.message, "test provider unavailable");
    }

    #[test]
    fn blocking_initialization_sanitizes_join_failure() {
        let result = state::tests::futures_lite_block_on(run(initialize_blocking(
            || -> Result<(), BridgeError> {
                panic!("synthetic constructor failure");
            },
        )));
        let error = result.unwrap_err();
        assert_eq!(error.code, "internal");
        assert_eq!(error.message, "core initialization task failed");
    }

    #[test]
    fn ui_store_roundtrip_is_atomic_and_private() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("sub").join(UI_STORE_FILE);
        assert!(read_store(&path).is_empty());
        let mut m = BTreeMap::new();
        m.insert(
            "local_settings".to_owned(),
            r#"{"app_locale":"ru"}"#.to_owned(),
        );
        write_store(&path, &m).unwrap();
        assert_eq!(read_store(&path), m);
        assert!(!path.with_extension("json.tmp").exists());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&path).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o600);
        }
    }

    #[test]
    fn corrupt_ui_store_reads_as_empty() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(UI_STORE_FILE);
        std::fs::write(&path, b"{not json").unwrap();
        assert!(read_store(&path).is_empty());
    }

    #[test]
    fn app_config_maps_choices() {
        let c = app_config(&CoreConfig {
            data_dir: Some("  ".into()),
            secure_store: SecureStoreChoice::Memory,
            keychain_service: Some("io.consolecrypt.test".into()),
            kdf: KdfChoice::Floor,
            client_version: "1.2.3".into(),
            device_name: Some("bad\u{0}name".into()),
            background_sync: false,
            auto_start_tunnels: false,
            log_directives: None,
        });
        assert!(c.data_dir.is_none());
        assert_eq!(c.secure_store, SecureStoreKind::InMemory);
        assert_eq!(c.kdf, KdfPolicy::Floor);
        assert_eq!(c.client_version, "1.2.3");
        assert_ne!(c.device_name, "bad\u{0}name");
        assert!(!c.background_sync && !c.auto_start_tunnels);
    }
}
