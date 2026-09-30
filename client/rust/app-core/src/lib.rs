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
mod devices;
pub mod dto;
mod edit;
pub mod enrollment_dto;
mod enrollment_highwater;
pub mod error;
mod host_auth;
mod inventory;
pub mod logging;
mod plan;
mod profile_hints;
mod prompts;
mod recovery;
mod secrets;
mod session;
mod sftp_browser;
mod sharing_api;
mod sharing_bindings;
pub mod sharing_dto;
pub mod sharing_highwater;
pub mod sharing_projection;
pub mod sharing_state;
mod snippets;
mod ssh;
mod ssh_api;
mod sync;
mod transfers;
pub mod validate;
mod working_set;
mod writer;

pub use account::RevealedSecret;
pub use app::AppCore;
pub use backup::{BackupFrequencyDto, BackupInfoDto, BackupScheduleDto};
pub use config::{current_platform, default_device_name, AppConfig, KdfPolicy, SecureStoreKind};
pub use dto::*;
pub use edit::{
    AppRefDto, EditConflictResolutionDto, EditLeftoverDto, EditSessionDto, EditStatusDto,
    EditStopModeDto, EditStopOutcomeDto, OpenWithDto, RemoteMetaDto,
};
pub use enrollment_dto::*;
pub use error::{AppError, AppResult};
pub use host_auth::{AgentKind, HostAuth, HostAuthMode, META_AUTH_PROMPT, META_INLINE_CREDENTIAL};
pub use plan::{
    PlanDiagnosticDto, PlanPreviewDto, PlanSeverityDto, ResolvedValueDto, RouteHopDto,
    ValueSourceDto,
};
pub use sftp_browser::{
    normalize_remote_path, RemoteFileInfoDto, SftpPreviewDto, MAX_PREVIEW_BYTES,
};
pub use sharing_dto::*;
pub use snippets::SnippetSearchHitDto;
pub use ssh_api::OpenSshCommandDto;
pub use transfers::{
    ErrorInfoDto, TransferDirectionDto, TransferJobDto, TransferRequestDto, TransferStateDto,
};

/// Re-exported for constructing [`AppConfig`].
pub use cc_protocol::version::Platform;

/// Rust-only integration points (not part of the FRB surface): custom
/// secure stores and OS authenticators for [`AppCore::with_platform`].
pub mod platform {
    pub use cc_platform_core::open::{LaunchOutput, LaunchSpec};
    pub use cc_platform_core::{
        AppRef, ExternalOsAuthenticator, FileOpener, InMemorySecureStore, Launcher, OpenError,
        OsAuthAvailability, OsAuthError, OsAuthKind, OsAuthenticator, OsFamily, SecureStore,
        SecureStoreError, UnsupportedOsAuthenticator,
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
