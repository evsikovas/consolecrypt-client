//! # cc-app-core — the ConsoleCrypt application facade
//!
//! One facade ([`AppCore`]) composing every core crate, consumed by the
//! Flutter UI (via flutter_rust_bridge) and by the `cc` CLI. Documented in
//! `docs/adr/ADR-0107-app-core-facade.md`.
//!
//! * **Profiles** (ADR-0106): Local (no server) and Synced, list / open /
//!   rename / remove, enable sync, disconnect.
//! * **Secrets at rest**: per-profile SQLCipher key, device identity and
//!   session tokens in the OS secure store.
//! * **Vault session**: unlock by passphrase / device (OS auth) / Recovery
//!   Key, lock (zeroize, stop sync, close SSH), decrypted working set kept
//!   consistent with store and sync events.
//! * **Inventory**: hosts, groups, jump profiles, proxies, credentials (+
//!   Secret objects), tunnels, snippets, notes, known hosts, vault
//!   settings, AI provider configs.
//! * **SSH**: exec, terminals, tunnels, SFTP, OpenSSH fallback — planned by
//!   ssh-core from vault objects; secrets only via the credential resolver.
//! * **Sync, devices, recovery, backups.**
//! * **AI** (ADR-0105): search over a local per-profile index, Ask AI,
//!   generate / explain / fix commands, snippet drafts — context only via
//!   ai-core's six allowed reads; AI output runs only through
//!   `approve_run` → `exec_approved` / `terminal_run_approved`.
//!
//! Public API rules (FRB-friendly): async methods on [`AppCore`], plain
//! owned DTOs ([`dto`]), string ids, Unix-millisecond timestamps, one error
//! type [`AppError`] with stable [`AppError::code`]s, streams as tokio
//! `broadcast` / `mpsc` receivers.

mod account;
mod ai;
mod app;
mod backup;
mod codec;
mod config;
mod credentials;
mod database_ai;
mod database_csv_import;
mod database_designer;
mod database_forward_route;
mod database_history;
mod database_import;
pub mod database_inventory;
mod database_local;
mod database_route;
mod database_runtime;
mod docker_compose;
mod docker_runtime;
pub use database_ai::{DbAiConsent, DbAiRequest};
pub use database_import::{DbImportChoice, DbImportReviewDto};
pub use database_runtime::{DbLiveSession, DbPreparedQueryDto, DbQueryReviewDto};
pub use docker_compose::{
    ComposeEdit, ComposeFileDto, ComposeJobUpdate, ComposeProjectDto, ComposeSaveOutcome,
};
pub use docker_runtime::{
    docker_contains_secret, DockerConnectOutcome, DockerForwardDto, DockerHostDto, DockerSessionDto,
};
mod device_unlock;
mod devices;
pub mod dto;
mod edit;
pub mod enrollment_dto;
mod enrollment_highwater;
pub mod enterprise_auth;
pub mod error;
mod host_auth;
mod inventory;
mod keyfile;
mod kubernetes;
pub mod kubernetes_ai;
pub mod kubernetes_auth;
pub mod kubernetes_cli;
pub mod kubernetes_cli_preferences;
mod kubernetes_flux;
pub mod kubernetes_forward;
pub mod kubernetes_helm_cli;
pub mod kubernetes_history;
pub mod kubernetes_import;
pub mod kubernetes_insights;
pub mod kubernetes_insights_watch;
pub mod kubernetes_inventory;
pub mod kubernetes_logs;
pub mod kubernetes_operations;
pub mod kubernetes_proxy;
pub mod kubernetes_runtime;
mod kubernetes_terminal;
pub mod kubernetes_view_preferences;
mod kubernetes_wsl;
pub mod logging;
mod loopback;
mod plan;
mod profile_hints;
mod prompts;
mod rdp;
mod rdp_hosts;
mod recovery;
mod secrets;
mod session;
mod sftp_browser;
mod sharing_api;
mod sharing_bindings;
pub mod sharing_database;
pub mod sharing_dto;
pub mod sharing_highwater;
pub mod sharing_kubernetes;
pub mod sharing_projection;
pub mod sharing_state;
mod snippets;
mod ssh;
mod ssh_api;
mod ssh_config_import;
mod sync;
pub mod team_dto;
mod teams;
mod teams_api;
mod transfers;
pub mod validate;
mod working_set;
mod writer;

pub use account::RevealedSecret;
pub use app::AppCore;
pub use backup::{BackupFrequencyDto, BackupInfoDto, BackupScheduleDto};
pub use cc_rdp_core::{
    CertificateInfo as RdpCertificateInfo, ConnectConfig as RdpConnectConfig,
    DirectoryGrant as RdpDirectoryGrant, Input as RdpInput, MouseButton as RdpMouseButton,
    PollResult as RdpPollResult, RdpError, SessionCapabilities as RdpSessionCapabilities,
    SessionPermissions as RdpSessionPermissions, SessionStatus as RdpSessionStatus,
};
pub use config::{current_platform, default_device_name, AppConfig, KdfPolicy, SecureStoreKind};
pub use dto::*;
pub use edit::{
    AppRefDto, EditConflictResolutionDto, EditLeftoverDto, EditSessionDto, EditStatusDto,
    EditStopModeDto, EditStopOutcomeDto, OpenWithDto, RemoteMetaDto,
};
pub use enrollment_dto::*;
pub use error::{AppError, AppResult};
pub use host_auth::{AgentKind, HostAuth, HostAuthMode, META_AUTH_PROMPT, META_INLINE_CREDENTIAL};
pub use kubernetes::KubeInventory;
pub use plan::{
    PlanDiagnosticDto, PlanPreviewDto, PlanSeverityDto, ResolvedValueDto, RouteHopDto,
    ValueSourceDto,
};
pub use rdp_hosts::RdpSavedHostTicket;
pub use sftp_browser::{
    normalize_remote_path, RemoteFileInfoDto, SftpPreviewDto, MAX_PREVIEW_BYTES,
};
pub use sharing_api::{ShareNoteLinkDto, SharedNoteDto, SharedNoteInputDto, SharedNotesDto};
pub use sharing_dto::*;
pub use snippets::SnippetSearchHitDto;
pub use ssh_api::OpenSshCommandDto;
pub use ssh_config_import::{ssh_config_preview, SshConfigHostPreview, SshConfigPreview};
pub use transfers::{
    ErrorInfoDto, TransferDirectionDto, TransferJobDto, TransferRequestDto, TransferStateDto,
};

/// Re-exported for constructing [`AppConfig`].
pub use cc_protocol::version::Platform;

/// Rust-only integration points (not part of the FRB surface): custom
/// secure stores and device unlock protectors for [`AppCore::with_platform`].
pub mod platform {
    pub use cc_platform_core::open::{LaunchOutput, LaunchSpec};
    pub use cc_platform_core::{
        is_active_name, platform_device_keys, AppRef, DeviceKeyProtector, DeviceKeyStatus,
        ExternalDeviceKeys, FakePrompt, FileOpener, InMemoryDeviceKeys, InMemorySecureStore,
        Launcher, OpenError, OsAuthError, OsAuthKind, OsFamily, SecureStore, SecureStoreError,
        StoreBackedDeviceKeys, UnlockProtection, UnsupportedDeviceKeys,
    };
}

#[cfg(test)]
mod sftp_browser_tests;

#[cfg(test)]
mod tests {
    #[test]
    fn facade_is_send_sync() {
        fn assert_send_sync<T: Send + Sync + Clone + 'static>() {}
        assert_send_sync::<super::AppCore>();
    }
}
