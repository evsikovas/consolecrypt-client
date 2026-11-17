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

mod agent_reader;

mod agent_tunnel;
pub use agent_tunnel::setup::AgentSetupOperation;

mod ai;

mod app;

mod automation_awx;

pub mod automation_projects;

pub mod automation_prepare;

mod automation_production_owner;

pub mod automation_jobs;

pub mod automation_runners;

mod automation_transition;

mod automation_inventory_export;

mod service_credential_issuer;

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

mod database_local_secrets;

mod database_route;

mod database_runtime;

pub mod database_vault_move;

mod docker_compose;

mod docker_runtime;

mod password_vault;

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

mod key_trust;

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

pub mod monitoring_sources;

mod settings_preservation;

mod monitoring_sync;

pub use monitoring_sync::MonitoringSettingsConsent;

mod monitoring_runtime;

mod plan;

mod presence;

mod security_key;

pub use presence::{StepUp, StepUpAction, PASSPHRASE_ONLY};

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

pub mod sharing_rdp;

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

pub use agent_reader::ReaderActionHandle;

pub use agent_tunnel::{NodeAssociationOperation, NodeClientIdentityReview, NodeRevocationReview, NodeRotationReview, NodeTunnelControlReview, AgentTunnelRouteOperation, AgentCollectorOperation, AgentExporterOperation, AgentCollectorServiceReview, AgentCollectorServiceRevocationReview};

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

pub use automation_awx::runtime::AwxNativeOperation;

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



pub use automation_projects::portable::gateway::{PortableOperation, PortablePasswordTransfer};



/// Fixed opt-in owned fixture; absent from production feature selection.

#[cfg(all(feature = "test-harness", target_os = "linux"))]

#[doc(hidden)]

pub async fn __test_harness_owned_public_tunnel_route() {

    AppCore::__owned_public_tunnel_fixture().await;

}


/// Fixed test-harness-only reuse of original affected assertion functions.
#[cfg(all(feature = "test-harness", not(test)))]
#[doc(hidden)]
pub mod focused_cadvisor_test_harness;

/// One fixed opt-in original owner fixture; no caller filter or authority API.
#[cfg(all(feature="test-harness",any(target_os="linux",target_os="windows")))]
#[doc(hidden)]
pub async fn __test_harness_owned_normal_lifetime_receiver() {
    AppCore::__owned_normal_lifetime_receiver_fixture().await;
}

// Fixed opt-in original revocation cases; no caller selector or authority API.
#[cfg(all(feature="test-harness",target_os="linux"))]
#[doc(hidden)]
pub async fn __test_harness_revocation_pending_verified() { AppCore::__owned_revocation_pending_verified().await; }
#[cfg(all(feature="test-harness",target_os="linux"))]
#[doc(hidden)]
pub async fn __test_harness_revocation_failed_completion() { AppCore::__owned_revocation_failed_completion().await; }
#[cfg(all(feature="test-harness",target_os="linux"))]
#[doc(hidden)]
pub async fn __test_harness_revocation_deadline_generation() { AppCore::__owned_revocation_deadline_generation().await; }

#[cfg(all(feature="test-harness",target_os="linux"))]
#[doc(hidden)]
pub async fn __test_harness_saved_destination_source_scope() { AppCore::__owned_saved_destination_source_scope().await; }

#[cfg(all(feature="test-harness",target_os="linux"))]
#[doc(hidden)]
pub async fn __test_harness_local_join_without_control_proof() { AppCore::__owned_local_join_without_control_proof().await; }

#[cfg(all(feature="test-harness",target_os="linux"))]
#[doc(hidden)]
pub async fn __test_harness_cancel_pending_join_preserves_handle() { AppCore::__owned_cancel_pending_join_preserves_handle().await; }
#[cfg(all(feature="test-harness",target_os="linux"))]
#[doc(hidden)]
pub async fn __test_harness_cancel_control_preserves_join_witness() { AppCore::__owned_cancel_control_preserves_join_witness().await; }

#[cfg(all(feature="test-harness",target_os="linux"))]
#[doc(hidden)]
pub fn __test_harness_relay_expiry_fractional() { AppCore::__owned_relay_expiry_fractional(); }
#[cfg(all(feature="test-harness",target_os="linux"))]
#[doc(hidden)]
pub fn __test_harness_relay_expiry_denials() { AppCore::__owned_relay_expiry_denials(); }

#[cfg(all(feature="test-harness",target_os="linux"))]
#[doc(hidden)]
pub fn __test_harness_outbound_url_canonical_positive() { AppCore::__owned_outbound_url_canonical_positive(); }
#[cfg(all(feature="test-harness",target_os="linux"))]
#[doc(hidden)]
pub fn __test_harness_outbound_url_canonical_negative() { AppCore::__owned_outbound_url_canonical_negative(); }
