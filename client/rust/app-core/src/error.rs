//! The single error type of the facade ([`AppError`]) with stable,
//! machine-readable codes ([`AppError::code`]).
//!
//! Messages are safe to show and to log: every source error in the cores is
//! already sanitized (no passphrases, keys, tokens, plaintext or
//! ciphertext), and conversions here never add secret material.

use cc_crypto_core::CryptoError;
use cc_platform_core::{DirsError, OsAuthError, SecureStoreError};
use cc_protocol::ErrorCode;
use cc_sftp_core::edit::EditError;
use cc_sftp_core::SftpError;
use cc_ssh_agent_core::AgentError;
use cc_ssh_core::keys::KeyError;
use cc_ssh_core::{PlanError, SshError};
use cc_storage_core::StorageError;
use cc_sync_core::{ApiError, CodecError, StopReason, SyncError};
use cc_terminal_core::TerminalError;
use cc_tunnel_core::TunnelError;
use cc_vault_core::VaultError;
use std::collections::BTreeMap;

/// Error of any [`crate::AppCore`] operation.
///
/// Branch on [`AppError::code`] (stable snake_case strings, listed in
/// ADR-0107), never on the message.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum AppError {
    /// A caller-supplied value is invalid (validation, malformed id, …).
    #[error("invalid {field}: {reason}")]
    InvalidInput { field: String, reason: String },
    /// An object, profile, request or session does not exist.
    #[error("{what} {id} not found")]
    NotFound { what: String, id: String },
    /// The object is still referenced and cannot be deleted.
    #[error("{what} {id} is still used by {used_by}")]
    InUse {
        what: String,
        id: String,
        used_by: String,
    },
    /// No profile is open (create or open one first).
    #[error("no profile is open")]
    NoActiveProfile,
    /// The operation needs the vault unlocked.
    #[error("the vault is locked")]
    VaultLocked,
    /// The independent OS checkpoint and encrypted cache need explicit
    /// reconciliation; no automatic trust reset or rollback is permitted.
    #[error(
        "shared cache requires signed-history reconciliation; its trusted checkpoint was preserved"
    )]
    SharingReconciliationRequired,
    /// The profile has no vault yet (synced profile before create/join).
    #[error("this profile has no vault yet; create or join one")]
    NoVault,
    /// The vault passphrase is wrong.
    #[error("wrong vault passphrase")]
    WrongPassphrase,
    /// The *current* vault passphrase given to a change is wrong (code
    /// `wrong_passphrase`, reason `current_passphrase_wrong`).
    #[error("the current vault passphrase is wrong")]
    WrongCurrentPassphrase,
    /// The *current* account password given to a change is wrong (code
    /// `invalid_credentials`, reason `current_password_wrong`).
    #[error("the current account password is wrong")]
    WrongCurrentPassword,
    /// The Recovery Key (words or QR payload) is wrong or malformed.
    #[error("wrong recovery key: {0}")]
    WrongRecoveryKey(String),
    /// A new passphrase does not satisfy the policy.
    #[error("passphrase too weak: {0}")]
    WeakPassphrase(String),
    /// This installation's device key cannot open the vault.
    #[error("this device is not authorized to open the vault with its device key")]
    DeviceNotAuthorized,
    /// OS / biometric authentication failed or is unavailable.
    #[error("OS authentication: {0}")]
    OsAuth(String),
    /// The operation needs a synced profile.
    #[error("this operation needs a synced profile")]
    LocalProfile,
    /// Enable-sync on a profile that is already synced.
    #[error("the profile is already synced")]
    AlreadySynced,
    /// The server could not be reached (network, DNS, timeout).
    #[error("server unreachable: {0}")]
    Offline(String),
    /// Wrong account email or password.
    #[error("wrong email or password")]
    InvalidCredentials,
    /// Unique constraint on the server (e.g. email already registered).
    #[error("already exists: {0}")]
    AlreadyExists(String),
    /// The session is gone; sign in again ([`crate::AppCore::reauthenticate`]).
    #[error("sign-in required")]
    ReauthRequired,
    /// Login refused for this device identity (revoked / taken): call
    /// [`crate::AppCore::reauthenticate`] with `allow_new_device_identity`.
    #[error("this installation needs a new device identity")]
    NewDeviceIdentityRequired,
    /// This device was revoked.
    #[error("this device has been revoked")]
    DeviceRevoked,
    /// The server refused this device's proof of possession at login
    /// (ADR-0006; reason = `device_proof_required` | `invalid_signature` |
    /// `stale` | `replayed`). Security-relevant: never retried without a
    /// proof; check the system clock for `stale`.
    #[error("the server rejected this device's login proof ({0})")]
    DeviceProofRejected(String),
    /// This device is not (yet) trusted for the vault.
    #[error("this device is not trusted for the vault")]
    DeviceNotTrusted,
    /// The server requires a newer client.
    #[error("client upgrade required")]
    UpgradeRequired,
    /// Rate limited by the server.
    #[error("rate limited")]
    RateLimited { retry_after_secs: Option<u64> },
    /// Any other structured server error (`code` = protocol error code).
    #[error("server error {status} ({code}): {message}")]
    Server {
        status: u16,
        code: String,
        message: String,
    },
    /// Device approval precondition failed.
    #[error("device approval: {0}")]
    Approval(String),
    /// No connection plan could be built for the host.
    #[error("connection plan: {0}")]
    ConnectionPlan(String),
    /// No connection plan could be built; `diagnostic` is the stable code of
    /// the planner finding (see `PlanDiagnosticDto`), `args` its parameters.
    #[error("connection plan: {message}")]
    ConnectionPlanRejected {
        message: String,
        diagnostic: String,
        args: BTreeMap<String, String>,
    },
    /// The host key changed — possible MITM; hard failure.
    #[error("{0}")]
    SshHostKeyChanged(String),
    /// The host key was rejected (user, strict policy or revoked key).
    #[error("{0}")]
    SshHostKeyRejected(String),
    /// SSH authentication failed.
    #[error("{0}")]
    SshAuthFailed(String),
    /// The private key needs a passphrase (none remembered / prompted).
    #[error("{0}")]
    SshPassphraseRequired(String),
    /// TCP / proxy / jump / timeout failure while connecting.
    #[error("{0}")]
    SshConnect(String),
    /// Other SSH failure.
    #[error("ssh: {0}")]
    Ssh(String),
    /// Tunnel failure.
    #[error("tunnel: {0}")]
    Tunnel(String),
    /// SFTP failure.
    #[error("sftp: {0}")]
    Sftp(String),
    /// A remote path does not exist (code `not_found`; reason
    /// `directory_not_found` when a directory was expected).
    #[error("no such remote file or directory: {path}")]
    RemoteNotFound { path: String, directory: bool },
    /// A remote path already exists (code `already_exists`, reason
    /// `already_exists`, args `name` / `path`).
    #[error("already exists: {path}")]
    RemoteExists { path: String },
    /// The server / OS refused access (code `permission_denied`).
    #[error("permission denied: {0}")]
    PermissionDenied(String),
    /// A size limit was exceeded (code `payload_too_large`, args `size` /
    /// `limit` in bytes).
    #[error("{what} too large: {size} bytes (limit {limit})")]
    TooLarge { what: String, size: u64, limit: u64 },
    /// Terminal failure.
    #[error("terminal: {0}")]
    Terminal(String),
    /// The OS failed to hand a working copy to its external editor.
    #[error("could not open editor: {0}")]
    EditorLaunch(String),
    /// Backup file invalid / unreadable.
    #[error("backup: {0}")]
    Backup(String),
    /// Local database failure.
    #[error("storage: {0}")]
    Storage(String),
    /// OS secure store failure.
    #[error("secure store: {0}")]
    SecureStore(String),
    /// Cryptographic failure (integrity, malformed data).
    #[error("crypto: {0}")]
    Crypto(String),
    /// Not supported on this platform / build / backend.
    #[error("unsupported: {0}")]
    Unsupported(String),
    /// Cancelled by the user.
    #[error("cancelled")]
    Cancelled,
    /// No AI provider is configured (or none is marked default).
    #[error("no AI provider is configured")]
    AiNotConfigured,
    /// The AI provider rejected the API key.
    #[error("{0}")]
    AiAuthFailed(String),
    /// The AI provider could not be reached or timed out.
    #[error("{0}")]
    AiUnavailable(String),
    /// Other AI provider failure (HTTP error, unknown model, bad response).
    #[error("{0}")]
    AiProvider(String),
    /// The execution gate needs the user's confirmation (modifying,
    /// destructive, unknown or multi-line command).
    #[error("{0}")]
    ConfirmationRequired(String),
    /// The command still contains `{{variables}}` / secret placeholders.
    #[error("fill in the placeholders first: {}", .0.join(", "))]
    UnresolvedPlaceholders(Vec<String>),
    /// Running an AI-originated command needs a valid approval token
    /// (`approve_run`); it is unknown, used, expired or for another host.
    #[error("approval required: {0}")]
    ApprovalRequired(String),
    /// Local file system error.
    #[error("io: {0}")]
    Io(String),
    /// Bug or unexpected state.
    #[error("internal error: {0}")]
    Internal(String),
}

impl AppError {
    /// Stable machine-readable code (see ADR-0107).
    pub fn code(&self) -> &'static str {
        match self {
            AppError::InvalidInput { .. } => "invalid_input",
            AppError::NotFound { .. } => "not_found",
            AppError::InUse { .. } => "in_use",
            AppError::NoActiveProfile => "no_active_profile",
            AppError::VaultLocked => "vault_locked",
            AppError::NoVault => "no_vault",
            AppError::WrongPassphrase | AppError::WrongCurrentPassphrase => "wrong_passphrase",
            AppError::WrongRecoveryKey(_) => "wrong_recovery_key",
            AppError::WeakPassphrase(_) => "weak_passphrase",
            AppError::DeviceNotAuthorized => "device_not_authorized",
            AppError::OsAuth(_) => "os_auth_failed",
            AppError::LocalProfile => "local_profile",
            AppError::AlreadySynced => "already_synced",
            AppError::Offline(_) => "offline",
            AppError::InvalidCredentials | AppError::WrongCurrentPassword => "invalid_credentials",
            AppError::AlreadyExists(_) => "already_exists",
            AppError::ReauthRequired => "reauth_required",
            AppError::NewDeviceIdentityRequired => "new_device_identity_required",
            AppError::DeviceRevoked => "device_revoked",
            AppError::DeviceProofRejected(_) => "device_proof_rejected",
            AppError::DeviceNotTrusted => "device_not_trusted",
            AppError::UpgradeRequired => "upgrade_required",
            AppError::RateLimited { .. } => "rate_limited",
            AppError::Server { .. } => "server",
            AppError::Approval(_) => "approval",
            AppError::ConnectionPlan(_) | AppError::ConnectionPlanRejected { .. } => {
                "connection_plan"
            }
            AppError::SshHostKeyChanged(_) => "ssh_host_key_changed",
            AppError::SshHostKeyRejected(_) => "ssh_host_key_rejected",
            AppError::SshAuthFailed(_) => "ssh_auth_failed",
            AppError::SshPassphraseRequired(_) => "ssh_passphrase_required",
            AppError::SshConnect(_) => "ssh_connect",
            AppError::Ssh(_) => "ssh",
            AppError::Tunnel(_) => "tunnel",
            AppError::Sftp(_) => "sftp",
            AppError::RemoteNotFound { .. } => "not_found",
            AppError::RemoteExists { .. } => "already_exists",
            AppError::PermissionDenied(_) => "permission_denied",
            AppError::TooLarge { .. } => "payload_too_large",
            AppError::Terminal(_) => "terminal",
            AppError::EditorLaunch(_) => "io",
            AppError::Backup(_) => "backup",
            AppError::Storage(_) => "storage",
            AppError::SecureStore(_) => "secure_store",
            AppError::Crypto(_) => "crypto",
            AppError::SharingReconciliationRequired => "sharing_reconciliation_required",
            AppError::Unsupported(_) => "unsupported",
            AppError::Cancelled => "cancelled",
            AppError::AiNotConfigured => "ai_not_configured",
            AppError::AiAuthFailed(_) => "ai_auth_failed",
            AppError::AiUnavailable(_) => "ai_unavailable",
            AppError::AiProvider(_) => "ai_provider",
            AppError::ConfirmationRequired(_) => "confirmation_required",
            AppError::UnresolvedPlaceholders(_) => "unresolved_placeholders",
            AppError::ApprovalRequired(_) => "approval_required",
            AppError::Io(_) => "io",
            AppError::Internal(_) => "internal",
        }
    }

    /// Human-readable message (same as `Display`).
    pub fn message(&self) -> String {
        self.to_string()
    }

    /// Refinement of [`AppError::code`] for a precise localized message:
    /// the wire name of the Dart `AppErrorReason`
    /// (`client/flutter/lib/core/services/errors.dart`,
    /// `lib/core/l10n/error_messages.dart`). `None` = the code-level text.
    pub fn reason(&self) -> Option<&'static str> {
        Some(match self {
            AppError::InvalidInput { field, reason } => invalid_reason(field, reason),
            AppError::NotFound { what, .. } => return not_found_reason(what),
            AppError::RemoteNotFound {
                directory: true, ..
            } => "directory_not_found",
            AppError::RemoteExists { .. } => "already_exists",
            // Code-level "email taken" (the UI maps `what = email`).
            AppError::AlreadyExists(what) if what == "email" => return None,
            AppError::AlreadyExists(what) if what == "vault_id" => "vault_exists",
            AppError::AlreadyExists(_) => "already_exists",
            AppError::NoActiveProfile => "no_active_profile",
            AppError::VaultLocked => "vault_locked",
            AppError::NoVault => "no_vault",
            AppError::WeakPassphrase(_) => "weak_passphrase",
            AppError::WrongCurrentPassphrase => "current_passphrase_wrong",
            AppError::WrongCurrentPassword => "current_password_wrong",
            AppError::LocalProfile => "synced_profile_required",
            AppError::DeviceNotAuthorized => "trusted_device_required",
            AppError::Backup(_) => "not_a_backup",
            AppError::SshPassphraseRequired(_) => "key_passphrase_required",
            AppError::EditorLaunch(_) => "editor_launch_failed",
            AppError::UnresolvedPlaceholders(_) => "missing_variables",
            AppError::AiNotConfigured => "provider_not_found",
            _ => return None,
        })
    }

    /// Non-secret parameters of [`AppError::reason`] / the code (field
    /// names, ids, paths, sizes, counts). Never contains secrets.
    pub fn args(&self) -> BTreeMap<String, String> {
        let mut m = BTreeMap::new();
        let mut put = |k: &str, v: String| {
            m.insert(k.to_owned(), v);
        };
        match self {
            AppError::InvalidInput { field, reason } => {
                put("field", field.clone());
                put("rule", reason.clone());
            }
            AppError::NotFound { what, id } => {
                put("what", what.clone());
                put("id", id.clone());
            }
            AppError::InUse { what, id, used_by } => {
                put("what", what.clone());
                put("id", id.clone());
                put("used_by", used_by.clone());
            }
            AppError::RemoteNotFound { path, .. } | AppError::RemoteExists { path } => {
                put("path", path.clone());
                put("name", remote_base_name(path));
            }
            AppError::PermissionDenied(path) => put("path", path.clone()),
            AppError::AlreadyExists(what) => {
                put("what", what.clone());
                put("name", what.clone());
            }
            AppError::TooLarge { what, size, limit } => {
                put("what", what.clone());
                put("size", size.to_string());
                put("limit", limit.to_string());
            }
            AppError::RateLimited {
                retry_after_secs: Some(s),
            } => put("retry_after_seconds", s.to_string()),
            AppError::Server { status, code, .. } => {
                put("status", status.to_string());
                put("protocol_code", code.clone());
            }
            AppError::DeviceProofRejected(reason) => put("proof", reason.clone()),
            AppError::EditorLaunch(detail) => put("detail", detail.chars().take(512).collect()),
            AppError::UnresolvedPlaceholders(names) => put("names", names.join(", ")),
            AppError::ConnectionPlanRejected {
                diagnostic, args, ..
            } => {
                for (k, v) in args {
                    put(k, v.clone());
                }
                put("diagnostic", diagnostic.clone());
            }
            _ => {}
        }
        m
    }

    pub(crate) fn invalid(field: impl Into<String>, reason: impl Into<String>) -> Self {
        AppError::InvalidInput {
            field: field.into(),
            reason: reason.into(),
        }
    }

    pub(crate) fn not_found(what: impl Into<String>, id: impl ToString) -> Self {
        AppError::NotFound {
            what: what.into(),
            id: id.to_string(),
        }
    }

    pub(crate) fn internal(e: impl std::fmt::Display) -> Self {
        AppError::Internal(e.to_string())
    }
}

/// Reason of an `InvalidInput` (field name + English rule → wire name).
fn invalid_reason(field: &str, rule: &str) -> &'static str {
    let empty = matches!(rule, "must not be empty" | "required" | "empty");
    match field {
        "email" => "invalid_email",
        "server_url" | "base_url" => "invalid_base_url",
        "display_name" if empty => "profile_name_required",
        "name" if empty => "name_required",
        "password" | "account_password" | "secret" if empty => "password_required",
        "new_password" | "account_password" if rule.starts_with("at least") => {
            "account_password_too_short"
        }
        "agent_path" => "agent_path_required",
        "key" | "private_key" => "invalid_key",
        "certificate" => "not_a_certificate",
        "template" if empty => "template_required",
        "chat_model" if empty => "chat_model_required",
        "chain" if rule.starts_with("must contain") => "jump_chain_empty",
        "parent_id" if rule.contains("cycle") => "group_cycle",
        "algorithm" => "unsupported_key_algorithm",
        "backup_folder" => "backup_folder_required",
        "keep_last" => "keep_at_least_one",
        _ => "invalid_field",
    }
}

/// Reason of a `NotFound` by object kind.
fn not_found_reason(what: &str) -> Option<&'static str> {
    Some(match what {
        "host" => "host_not_found",
        "credential" => "credential_not_found",
        "profile" => "profile_not_found",
        "device" => "device_not_found",
        "tunnel" => "tunnel_not_found",
        "ai provider" | "ai_provider" => "provider_not_found",
        "prompt" | "trust request" | "device request" => "request_not_found",
        "secret" => "secret_not_stored",
        "sftp session" | "terminal" | "edit session" | "openssh session" => "session_closed",
        _ => return None,
    })
}

fn remote_base_name(path: &str) -> String {
    let t = path.trim_end_matches('/');
    t.rsplit('/')
        .next()
        .filter(|n| !n.is_empty())
        .unwrap_or(path)
        .to_owned()
}

/// Result alias of the facade.
pub type AppResult<T> = Result<T, AppError>;

impl From<cc_models::ValidationError> for AppError {
    fn from(e: cc_models::ValidationError) -> Self {
        AppError::invalid(e.field, e.reason)
    }
}

impl From<VaultError> for AppError {
    fn from(e: VaultError) -> Self {
        match e {
            VaultError::WrongPassphrase => AppError::WrongPassphrase,
            VaultError::WrongRecoveryKey => {
                AppError::WrongRecoveryKey("does not open the vault".into())
            }
            VaultError::RecoveryKeyForOtherVault => {
                AppError::WrongRecoveryKey("the recovery code belongs to a different vault".into())
            }
            VaultError::DeviceNotAuthorized => AppError::DeviceNotAuthorized,
            VaultError::WeakPassphrase => AppError::WeakPassphrase(e.to_string()),
            VaultError::Approval(m) => AppError::Approval(m.to_string()),
            VaultError::InvalidDeviceName => AppError::invalid("device_name", e.to_string()),
            VaultError::OsAuth(a) => a.into(),
            VaultError::SecureStore(s) => s.into(),
            VaultError::NoDeviceIdentity | VaultError::CorruptedIdentity => {
                AppError::SecureStore(e.to_string())
            }
            VaultError::BackupEncoding | VaultError::BackupObject { .. } => {
                AppError::Backup(e.to_string())
            }
            VaultError::Crypto(c) => c.into(),
            VaultError::InvalidProfileId => AppError::invalid("profile_id", e.to_string()),
            other => AppError::Crypto(other.to_string()),
        }
    }
}

impl From<CryptoError> for AppError {
    fn from(e: CryptoError) -> Self {
        match e {
            CryptoError::Mnemonic(_) | CryptoError::RecoveryPayload(_) => {
                AppError::WrongRecoveryKey(e.to_string())
            }
            CryptoError::EmptyPassphrase => AppError::WeakPassphrase(e.to_string()),
            CryptoError::Backup(_) => AppError::Backup(e.to_string()),
            other => AppError::Crypto(other.to_string()),
        }
    }
}

impl From<OsAuthError> for AppError {
    fn from(e: OsAuthError) -> Self {
        match e {
            OsAuthError::Cancelled => AppError::Cancelled,
            other => AppError::OsAuth(other.to_string()),
        }
    }
}

impl From<SecureStoreError> for AppError {
    fn from(e: SecureStoreError) -> Self {
        AppError::SecureStore(e.to_string())
    }
}

impl From<DirsError> for AppError {
    fn from(e: DirsError) -> Self {
        match e {
            DirsError::InvalidProfileId => AppError::invalid("profile_id", e.to_string()),
            other => AppError::Io(other.to_string()),
        }
    }
}

impl From<StorageError> for AppError {
    fn from(e: StorageError) -> Self {
        AppError::Storage(e.to_string())
    }
}

impl From<std::io::Error> for AppError {
    fn from(e: std::io::Error) -> Self {
        AppError::Io(e.to_string())
    }
}

impl From<CodecError> for AppError {
    fn from(e: CodecError) -> Self {
        match e {
            CodecError::Locked => AppError::VaultLocked,
            other => AppError::Crypto(other.to_string()),
        }
    }
}

impl From<StopReason> for AppError {
    fn from(r: StopReason) -> Self {
        match r {
            StopReason::DeviceRevoked => AppError::DeviceRevoked,
            StopReason::ReauthRequired => AppError::ReauthRequired,
            StopReason::NotTrusted => AppError::DeviceNotTrusted,
            StopReason::VaultGone => AppError::not_found("vault on server", "-"),
            StopReason::UpgradeRequired => AppError::UpgradeRequired,
            StopReason::Shutdown => AppError::Internal("sync engine stopped".into()),
        }
    }
}

fn detail_str<'a>(details: &'a Option<serde_json::Value>, key: &str) -> Option<&'a str> {
    details.as_ref()?.get(key)?.as_str()
}

/// snake_case wire name of a protocol error code.
pub(crate) fn protocol_code_name(code: ErrorCode) -> String {
    protocol_code_name_of(&code)
}

/// snake_case serde name of a unit enum value (protocol enums).
pub(crate) fn protocol_code_name_of<T: serde::Serialize + std::fmt::Debug>(v: &T) -> String {
    serde_json::to_value(v)
        .ok()
        .and_then(|v| v.as_str().map(str::to_owned))
        .unwrap_or_else(|| format!("{v:?}").to_ascii_lowercase())
}

impl From<ApiError> for AppError {
    fn from(e: ApiError) -> Self {
        match e {
            ApiError::Network(_) | ApiError::Timeout => AppError::Offline(e.to_string()),
            ApiError::NotAuthenticated
            | ApiError::SessionExpired
            | ApiError::RefreshTokenReused => AppError::ReauthRequired,
            ApiError::DeviceRevoked => AppError::DeviceRevoked,
            ApiError::DeviceIdentityRevoked | ApiError::DeviceIdentityConflict => {
                AppError::NewDeviceIdentityRequired
            }
            ApiError::UpgradeRequired { .. } => AppError::UpgradeRequired,
            ApiError::RateLimited { retry_after } => AppError::RateLimited {
                retry_after_secs: retry_after.map(|d| d.as_secs()),
            },
            ApiError::InvalidConfig(m) => AppError::invalid("server_url", m),
            ApiError::TokenStore(m) => AppError::SecureStore(m),
            // Protocol 1.5 request proofs: the server rejected our proof
            // (missing/malformed/stale/replayed/invalid_signature) …
            ApiError::RequestProofRejected { reason } => {
                AppError::DeviceProofRejected(reason.as_str().to_owned())
            }
            // … or the device key could not sign (nothing was sent).
            ApiError::RequestSigning(e) => AppError::SecureStore(e.to_string()),
            ApiError::Server { status, error } => match error.code {
                ErrorCode::InvalidCredentials => AppError::InvalidCredentials,
                ErrorCode::DeviceRevoked => AppError::DeviceRevoked,
                ErrorCode::DeviceNotTrusted => AppError::DeviceNotTrusted,
                ErrorCode::Unauthorized | ErrorCode::RefreshTokenReused => AppError::ReauthRequired,
                ErrorCode::AlreadyExists => {
                    let field = error
                        .details
                        .as_ref()
                        .and_then(|d| d.get("field"))
                        .and_then(|f| f.as_str())
                        .map(str::to_owned)
                        .unwrap_or_else(|| error.message.clone());
                    AppError::AlreadyExists(field)
                }
                ErrorCode::UpgradeRequired => AppError::UpgradeRequired,
                ErrorCode::InvalidProof
                    if matches!(
                        detail_str(&error.details, "reason"),
                        Some("device_proof_required" | "invalid_signature" | "stale" | "replayed")
                    ) =>
                {
                    AppError::DeviceProofRejected(
                        detail_str(&error.details, "reason")
                            .unwrap_or_default()
                            .to_owned(),
                    )
                }
                code => AppError::Server {
                    status,
                    code: protocol_code_name(code),
                    message: error.message.clone(),
                },
            },
            ApiError::InvalidResponse { status, reason } => AppError::Server {
                status,
                code: "invalid_response".into(),
                message: reason,
            },
        }
    }
}

impl From<SyncError> for AppError {
    fn from(e: SyncError) -> Self {
        match e {
            SyncError::Api(a) => a.into(),
            SyncError::Storage(s) => s.into(),
            SyncError::Codec(c) => c.into(),
            SyncError::LocalProfile => AppError::LocalProfile,
            SyncError::Stopped(r) => r.into(),
            SyncError::ObjectTooLarge { .. } => AppError::invalid("object", e.to_string()),
            SyncError::VaultExists => AppError::AlreadyExists("vault_id".into()),
            SyncError::Invalid(m) => AppError::Internal(m),
            SyncError::MalformedResponse(m) => AppError::Server {
                status: 0,
                code: "malformed_response".into(),
                message: m,
            },
        }
    }
}

impl From<PlanError> for AppError {
    fn from(e: PlanError) -> Self {
        let d = crate::plan::plan_error_diagnostic(&e);
        AppError::ConnectionPlanRejected {
            message: e.to_string(),
            diagnostic: d.code,
            args: d.args,
        }
    }
}

impl From<SshError> for AppError {
    fn from(e: SshError) -> Self {
        let msg = e.to_string();
        match e {
            SshError::Plan(p) => p.into(),
            SshError::HostKeyChanged { .. } => AppError::SshHostKeyChanged(msg),
            SshError::HostKeyUnknown { .. }
            | SshError::HostKeyRejected { .. }
            | SshError::HostKeyRevoked { .. } => AppError::SshHostKeyRejected(msg),
            SshError::AuthFailed { .. } | SshError::WrongPassphrase { .. } => {
                AppError::SshAuthFailed(msg)
            }
            SshError::PassphraseRequired { .. } => AppError::SshPassphraseRequired(msg),
            SshError::Connect { .. }
            | SshError::Timeout { .. }
            | SshError::Proxy(_)
            | SshError::JumpChannel { .. } => AppError::SshConnect(msg),
            SshError::Credential(cc_ssh_core::ResolveError::Locked) => AppError::VaultLocked,
            SshError::Credential(cc_ssh_core::ResolveError::Cancelled) => AppError::Cancelled,
            SshError::Unsupported(m) => AppError::Unsupported(m),
            _ => AppError::Ssh(msg),
        }
    }
}

impl From<KeyError> for AppError {
    fn from(e: KeyError) -> Self {
        match e {
            KeyError::InvalidFormat(_)
            | KeyError::Unsupported(_)
            | KeyError::CertificateMismatch => AppError::invalid("key", e.to_string()),
            KeyError::PassphraseRequired => AppError::SshPassphraseRequired(e.to_string()),
            KeyError::WrongPassphrase => AppError::invalid("passphrase", e.to_string()),
            other => AppError::Crypto(other.to_string()),
        }
    }
}

impl From<TunnelError> for AppError {
    fn from(e: TunnelError) -> Self {
        match e {
            TunnelError::Invalid(m) => AppError::invalid("tunnel", m),
            other => AppError::Tunnel(other.to_string()),
        }
    }
}

impl From<SftpError> for AppError {
    fn from(e: SftpError) -> Self {
        match e {
            SftpError::Ssh(s) => s.into(),
            SftpError::Cancelled => AppError::Cancelled,
            SftpError::NotFound(path) => AppError::RemoteNotFound {
                path,
                directory: false,
            },
            SftpError::PermissionDenied(path) => AppError::PermissionDenied(path),
            SftpError::AlreadyExists(path) => AppError::RemoteExists { path },
            SftpError::Unsupported(m) => AppError::Unsupported(m),
            SftpError::Local { path, source } => match source.kind() {
                std::io::ErrorKind::NotFound => {
                    AppError::Io(format!("no such local file or directory: {path}"))
                }
                std::io::ErrorKind::PermissionDenied => AppError::PermissionDenied(path),
                std::io::ErrorKind::AlreadyExists => AppError::AlreadyExists(path),
                _ => AppError::Io(format!("{path}: {source}")),
            },
            other => AppError::Sftp(other.to_string()),
        }
    }
}

impl From<EditError> for AppError {
    fn from(e: EditError) -> Self {
        match e {
            EditError::TooLarge { size, limit } => AppError::TooLarge {
                what: "file".into(),
                size,
                limit,
            },
            EditError::NotAFile(path) => AppError::InvalidInput {
                field: "path".into(),
                reason: format!("not a regular file: {path}"),
            },
            EditError::Sftp(s) => s.into(),
            EditError::Local { path, source } => {
                AppError::Io(format!("{}: {source}", path.display()))
            }
            EditError::Cancelled => AppError::Cancelled,
            EditError::Opener(detail) => AppError::EditorLaunch(detail),
            EditError::Upload(m) => AppError::Sftp(m),
            EditError::Busy(path) => AppError::InUse {
                what: "file".into(),
                id: path,
                used_by: "an edit session that is opening".into(),
            },
            EditError::UnknownSession | EditError::Closed => {
                AppError::not_found("edit session", "-")
            }
            EditError::InvalidState(m) => AppError::invalid("edit_session", m),
            other => AppError::Io(other.to_string()),
        }
    }
}

impl From<TerminalError> for AppError {
    fn from(e: TerminalError) -> Self {
        match e {
            TerminalError::NotFound(id) => AppError::not_found("terminal", id),
            other => AppError::Terminal(other.to_string()),
        }
    }
}

impl From<AgentError> for AppError {
    fn from(e: AgentError) -> Self {
        match e {
            AgentError::Unsupported(m) => AppError::Unsupported(m),
            other => AppError::Ssh(other.to_string()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn editor_launch_errors_preserve_a_bounded_diagnostic_for_localization() {
        let e: AppError = EditError::Opener("application not found".into()).into();
        assert_eq!(e.code(), "io");
        assert_eq!(e.reason(), Some("editor_launch_failed"));
        assert_eq!(e.args()["detail"], "application not found");
        let e: AppError = EditError::Opener("я".repeat(600)).into();
        assert_eq!(e.args()["detail"].chars().count(), 512);
        assert_eq!(AppError::from(EditError::Cancelled), AppError::Cancelled);
    }

    #[test]
    fn codes_are_stable_snake_case() {
        let samples = [
            AppError::VaultLocked,
            AppError::WrongPassphrase,
            AppError::invalid("name", "empty"),
            AppError::NewDeviceIdentityRequired,
            AppError::SshHostKeyChanged("x".into()),
        ];
        for e in samples {
            let c = e.code();
            assert!(
                c.chars().all(|ch| ch.is_ascii_lowercase() || ch == '_'),
                "{c}"
            );
        }
        assert_eq!(AppError::VaultLocked.code(), "vault_locked");
    }

    #[test]
    fn api_errors_map_to_codes() {
        let e: AppError = ApiError::Network("refused".into()).into();
        assert_eq!(e.code(), "offline");
        let e: AppError = ApiError::DeviceIdentityRevoked.into();
        assert_eq!(e.code(), "new_device_identity_required");
        let e: AppError = ApiError::Server {
            status: 409,
            error: Box::new(
                cc_protocol::ApiError::new(ErrorCode::AlreadyExists, "taken")
                    .with_details(serde_json::json!({"field": "email"})),
            ),
        }
        .into();
        assert_eq!(e, AppError::AlreadyExists("email".into()));
        let e: AppError = ApiError::Server {
            status: 422,
            error: Box::new(cc_protocol::ApiError::new(ErrorCode::InvalidProof, "bad")),
        }
        .into();
        assert!(matches!(e, AppError::Server { ref code, .. } if code == "invalid_proof"));
        let e: AppError = ApiError::Server {
            status: 422,
            error: Box::new(
                cc_protocol::ApiError::new(ErrorCode::InvalidProof, "stale")
                    .with_details(serde_json::json!({"reason": "stale"})),
            ),
        }
        .into();
        assert_eq!(e, AppError::DeviceProofRejected("stale".into()));
        assert_eq!(e.code(), "device_proof_rejected");
    }
}
