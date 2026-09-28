//! Construction-time configuration of [`crate::AppCore`].

use cc_protocol::version::Platform;

/// Where database keys, device identities and session tokens live.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SecureStoreKind {
    /// OS keychain (macOS Keychain / Windows Credential Manager). Requires
    /// the `os-keychain` feature (default).
    Os,
    /// Process memory only — nothing survives a restart (tests, demos).
    InMemory,
    /// **Insecure** plaintext file `<data dir>/secure-store.json` (0600) for
    /// headless/CI use of the CLI. Requires the `insecure-file-store`
    /// feature; never used by the desktop app.
    InsecureFile,
}

/// Argon2id cost policy for new password envelopes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KdfPolicy {
    /// Measure this machine (ADR-0102 calibration; ~0.75 s target).
    Calibrate,
    /// ADR-0002 default (64 MiB, t=3, p=1).
    Default,
    /// The server floor (19 MiB, t=2) — **tests only**.
    Floor,
}

/// Configuration of an [`crate::AppCore`] instance.
#[derive(Debug, Clone)]
pub struct AppConfig {
    /// Data root; `None` = `CONSOLECRYPT_DATA_DIR` or the platform default.
    pub data_dir: Option<String>,
    pub secure_store: SecureStoreKind,
    /// Service name for [`SecureStoreKind::Os`] (isolates installations /
    /// test runs). `None` = `io.consolecrypt.ConsoleCrypt`.
    pub keychain_service: Option<String>,
    pub kdf: KdfPolicy,
    /// Reported to the server and used for the default device name.
    pub platform: Platform,
    /// App version (`x-cc-client-version`, backups).
    pub client_version: String,
    /// Default device name for registrations (e.g. the host name).
    pub device_name: String,
    /// Allow plain `http://` servers on non-loopback hosts (trusted LAN
    /// only; tokens travel unencrypted).
    pub allow_insecure_http: bool,
    /// Start tunnels with `auto_start = true` after unlocking.
    pub auto_start_tunnels: bool,
    /// Start the background sync engine + WebSocket after unlocking a synced
    /// profile. The CLI turns this off and syncs explicitly.
    pub background_sync: bool,
    /// Periodic background sync interval in seconds (`0` = only on triggers).
    pub sync_interval_secs: u64,
    /// How long a host-key / passphrase prompt waits for an answer.
    pub prompt_timeout_secs: u64,
    /// Edit sessions (ADR-0108): exclude the edit root from Time Machine
    /// (macOS `tmutil addexclusion`, best effort).
    pub edit_exclude_from_backup: bool,
    /// Parallel SFTP transfer jobs (queued beyond that).
    pub max_parallel_transfers: usize,
    /// Run due scheduled backups in the core while unlocked (ADR-0106).
    pub backup_scheduler: bool,
}

impl AppConfig {
    /// Production defaults for `platform`.
    pub fn new(platform: Platform) -> Self {
        Self {
            data_dir: None,
            secure_store: SecureStoreKind::Os,
            keychain_service: None,
            kdf: KdfPolicy::Calibrate,
            platform,
            client_version: env!("CARGO_PKG_VERSION").to_owned(),
            device_name: default_device_name(),
            allow_insecure_http: false,
            auto_start_tunnels: true,
            background_sync: true,
            sync_interval_secs: 300,
            prompt_timeout_secs: 300,
            edit_exclude_from_backup: true,
            max_parallel_transfers: 3,
            backup_scheduler: true,
        }
    }

    /// Test defaults: in-memory secure store, floor KDF, given data dir.
    pub fn for_tests(data_dir: impl Into<String>) -> Self {
        Self {
            data_dir: Some(data_dir.into()),
            secure_store: SecureStoreKind::InMemory,
            kdf: KdfPolicy::Floor,
            platform: Platform::Cli,
            device_name: "test device".into(),
            auto_start_tunnels: false,
            prompt_timeout_secs: 30,
            edit_exclude_from_backup: false,
            backup_scheduler: false,
            ..Self::new(Platform::Cli)
        }
    }
}

/// Best-effort host name for device registration.
pub fn default_device_name() -> String {
    let name = std::env::var("CC_DEVICE_NAME")
        .or_else(|_| std::env::var("COMPUTERNAME"))
        .or_else(|_| std::env::var("HOSTNAME"))
        .ok()
        .map(|s| s.trim().to_owned())
        .filter(|s| !s.is_empty() && !s.chars().any(char::is_control))
        .unwrap_or_else(|| "ConsoleCrypt device".to_owned());
    name.chars().take(64).collect()
}

/// Platform of this build (for [`AppConfig::new`]).
pub fn current_platform() -> Platform {
    if cfg!(target_os = "macos") {
        Platform::Macos
    } else if cfg!(target_os = "windows") {
        Platform::Windows
    } else if cfg!(target_os = "android") {
        Platform::Android
    } else if cfg!(target_os = "ios") {
        Platform::Ios
    } else {
        Platform::Linux
    }
}
