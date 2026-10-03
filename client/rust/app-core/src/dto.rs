//! Plain owned DTOs of the facade (flutter_rust_bridge friendly): ids are
//! canonical UUID strings, timestamps are Unix milliseconds (`*_ms`), enums
//! are C-like (cc-models enums are re-exported and mirrored as-is).
//!
//! Secret material never appears in a DTO except [`RecoveryKitDto`] (shown
//! once during onboarding / regeneration; redacted `Debug`, zeroized on
//! drop). Credentials, proxies and AI providers report only *whether* a
//! secret is attached (`has_*`).

use crate::error::{AppError, AppResult};
use cc_models::ai::AiProviderConfig;
use cc_models::credential::Credential;
use cc_models::group::Group;
use cc_models::host::{Host, JumpProfile, Proxy};
use cc_models::known_host::KnownHost;
use cc_models::note::Note;
use cc_models::settings::VaultSettings;
use cc_models::snippet::{Snippet, SnippetVariable};
use cc_models::tunnel::Tunnel;
use cc_models::{ObjectId, Timestamp};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::str::FromStr;
use zeroize::{Zeroize, ZeroizeOnDrop};

pub use cc_models::ai::{AiProviderKind, PrivacyProfile};
pub use cc_models::credential::{CredentialKind, KeyAlgorithm};
pub use cc_models::host::{HostKeyPolicy, ProxyKind, SshBackend};
pub use cc_models::known_host::{KnownHostMarker, KnownHostSource};
pub use cc_models::settings::TerminalHistoryMode;
pub use cc_models::snippet::{RiskLevel, SnippetSource, SnippetType};
pub use cc_models::tunnel::TunnelKind;
pub use cc_models::ObjectKind;
pub use cc_ssh_core::keys::KeyGenAlgorithm;
pub use cc_ssh_core::HostKeyDecision;
pub use cc_storage_core::{LocalState, ProfileKind};

// AI facade DTOs (`crate::ai::dto`).
pub use crate::ai::dto::*;

// ---- helpers -----------------------------------------------------------------------

pub(crate) fn ms(t: &Timestamp) -> i64 {
    t.timestamp_millis()
}

pub(crate) fn opt_ms(t: &Option<Timestamp>) -> Option<i64> {
    t.as_ref().map(ms)
}

pub(crate) fn from_ms(v: i64) -> Option<Timestamp> {
    chrono::DateTime::from_timestamp_millis(v)
}

/// Parse a required object id.
pub(crate) fn parse_id(field: &str, s: &str) -> AppResult<ObjectId> {
    ObjectId::from_str(s.trim()).map_err(|_| AppError::invalid(field.to_owned(), "not a valid id"))
}

/// Parse an optional id; `None` and empty strings mean "none".
pub(crate) fn parse_opt_id(field: &str, s: &Option<String>) -> AppResult<Option<ObjectId>> {
    match s.as_deref().map(str::trim) {
        None | Some("") => Ok(None),
        Some(v) => parse_id(field, v).map(Some),
    }
}

fn parse_ids(field: &str, v: &[String]) -> AppResult<Vec<ObjectId>> {
    v.iter().map(|s| parse_id(field, s)).collect()
}

fn ids(v: &[ObjectId]) -> Vec<String> {
    v.iter().map(ToString::to_string).collect()
}

fn opt_id(v: &Option<ObjectId>) -> Option<String> {
    v.map(|i| i.to_string())
}

fn clean_opt(s: &Option<String>) -> Option<String> {
    s.as_deref()
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .map(str::to_owned)
}

fn require_name(field: &'static str, s: &str) -> AppResult<()> {
    if s.trim().is_empty() {
        return Err(AppError::invalid(field, "must not be empty"));
    }
    Ok(())
}

/// Fields every editable DTO shares; used by the generic save path.
pub(crate) trait EditableDto: Sized {
    type Model;
    const KIND: ObjectKind;
    fn id_str(&self) -> &str;
    fn from_model(m: &Self::Model) -> Self;
    /// Build the model for `id`. `existing` carries fields the DTO does not
    /// expose (e.g. attached secret ids) and the creation time.
    fn to_model(&self, id: ObjectId, existing: Option<&Self::Model>) -> AppResult<Self::Model>;
}

fn created(existing_created: Option<Timestamp>) -> (Timestamp, Timestamp) {
    let now = chrono::Utc::now();
    (existing_created.unwrap_or(now), now)
}

// ---- profiles --------------------------------------------------------------------

/// Lock state of the (open) profile's vault.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VaultStateDto {
    /// The profile is not open.
    Closed,
    /// Synced profile signed in, but no vault created/joined yet.
    NoVault,
    Locked,
    Unlocked,
}

/// A local profile (ADR-0106).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProfileInfo {
    pub id: String,
    pub display_name: String,
    pub kind: ProfileKind,
    /// The currently open profile.
    pub active: bool,
    pub created_at_ms: i64,
    pub last_opened_at_ms: Option<i64>,
    /// Server and account e-mail: from the database for the open profile,
    /// from a non-secret hint file (`profiles/<id>/account-hint.json`) for
    /// closed synced profiles.
    pub server_url: Option<String>,
    pub email: Option<String>,
    /// Filled for the open profile only.
    pub device_id: Option<String>,
    pub vault_id: Option<String>,
    pub vault_state: VaultStateDto,
    /// Open profile: the mandatory Recovery Kit check is still pending
    /// (persisted; see `recovery_kit_check_pending`).
    #[serde(default)]
    pub recovery_kit_pending: bool,
}

/// Recovery Kit (ADR-0002): shown once, then the user must pass the
/// 3-word check. Secret: redacted `Debug`, zeroized on drop.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize, Zeroize, ZeroizeOnDrop)]
pub struct RecoveryKitDto {
    pub vault_id: String,
    /// 24 words, in order.
    pub words: Vec<String>,
    /// The words joined by single spaces.
    pub phrase: String,
    /// `consolecrypt-recovery:v1:<vault_id>:<key>` for the QR code.
    pub qr_payload: String,
    pub server_url: Option<String>,
    pub created_at_ms: i64,
}

impl std::fmt::Debug for RecoveryKitDto {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RecoveryKitDto")
            .field("vault_id", &self.vault_id)
            .field("words", &"<redacted>")
            .field("server_url", &self.server_url)
            .finish_non_exhaustive()
    }
}

impl RecoveryKitDto {
    pub(crate) fn from_kit(kit: &cc_vault_core::RecoveryKit) -> Self {
        let phrase = kit.phrase().expose_phrase().to_owned();
        Self {
            vault_id: kit.vault_id().to_string(),
            words: kit.phrase().words().map(str::to_owned).collect(),
            phrase,
            qr_payload: kit.expose_qr_payload().to_owned(),
            server_url: kit.server_url().map(str::to_owned),
            created_at_ms: ms(&kit.created_at()),
        }
    }
}

/// Result of creating a profile.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CreatedProfile {
    pub profile: ProfileInfo,
    /// Present when a vault was created (Local profiles).
    pub recovery_kit: Option<RecoveryKitDto>,
}

/// A vault of the signed-in account (server view).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RemoteVaultDto {
    pub vault_id: String,
    pub role: String,
    pub caller_trusted: bool,
    pub latest_sequence: i64,
    pub created_at_ms: i64,
}

/// How a synced profile signs in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AccountMode {
    Register,
    Login,
}

/// Result of signing in a synced profile.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SyncedAccountDto {
    pub profile: ProfileInfo,
    pub user_id: String,
    pub device_id: String,
    pub email_verified: bool,
    /// Vaults of the account; empty → create one, else join one.
    pub vaults: Vec<RemoteVaultDto>,
}

/// OS / biometric unlock availability of the open profile.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeviceUnlockDto {
    /// User opted in on this installation for this profile. Never synced.
    pub enabled: bool,
    /// Offer "Unlock with Touch ID / Windows Hello".
    pub available: bool,
    /// `touch_id` | `face_id` | `windows_hello` | `device_credential`.
    pub kind: Option<String>,
    /// This installation holds a device envelope for the vault.
    pub has_device_envelope: bool,
    /// Hardware present but nothing enrolled.
    pub not_enrolled: bool,
}

/// Result of [`crate::AppCore::reauthenticate`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuthStateDto {
    pub device_id: String,
    /// A new device identity was generated.
    pub new_device_identity: bool,
    /// This device is trusted for the profile's vault.
    pub trusted: bool,
}

/// Onboarding check positions (1-based word numbers).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecoveryCheckDto {
    pub positions: Vec<u32>,
}

// ---- inventory -------------------------------------------------------------------

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HostProtocol {
    #[default]
    Ssh,
    Rdp,
}

/// Host (editable). Empty `id` = create.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HostDto {
    #[serde(default)]
    pub protocol: HostProtocol,
    #[serde(default)]
    pub rdp_domain: Option<String>,
    #[serde(default = "cc_models::rdp::default_width")]
    pub rdp_width: u16,
    #[serde(default = "cc_models::rdp::default_height")]
    pub rdp_height: u16,
    pub id: String,
    pub name: String,
    pub address: String,
    pub port: Option<u16>,
    pub username: Option<String>,
    pub credential_id: Option<String>,
    pub group_id: Option<String>,
    pub jump_chain: Vec<String>,
    pub jump_profile_id: Option<String>,
    pub proxy_id: Option<String>,
    pub proxy_command: Option<String>,
    pub host_key_policy: HostKeyPolicy,
    pub backend: SshBackend,
    pub keepalive_secs: Option<u32>,
    pub agent_forwarding: bool,
    pub tags: Vec<String>,
    pub notes: String,
    pub metadata: HashMap<String, String>,
    pub created_at_ms: i64,
    pub updated_at_ms: i64,
    /// Output only: authentication mode (change it with
    /// [`crate::AppCore::save_host_with_auth`]).
    pub auth_mode: crate::HostAuthMode,
}

impl HostDto {
    /// New host draft.
    pub fn new(name: impl Into<String>, address: impl Into<String>) -> Self {
        Self::from_model(&Host::new(name, address)).with_empty_id()
    }

    fn with_empty_id(mut self) -> Self {
        self.id.clear();
        self
    }
}

impl EditableDto for HostDto {
    type Model = Host;
    const KIND: ObjectKind = ObjectKind::Host;
    fn id_str(&self) -> &str {
        &self.id
    }
    fn from_model(h: &Host) -> Self {
        Self {
            protocol: HostProtocol::Ssh,
            rdp_domain: None,
            rdp_width: cc_models::rdp::default_width(),
            rdp_height: cc_models::rdp::default_height(),
            id: h.id.to_string(),
            name: h.name.clone(),
            address: h.address.clone(),
            port: h.port,
            username: h.username.clone(),
            credential_id: opt_id(&h.credential_id),
            group_id: opt_id(&h.group_id),
            jump_chain: ids(&h.jump_chain),
            jump_profile_id: opt_id(&h.jump_profile_id),
            proxy_id: opt_id(&h.proxy_id),
            proxy_command: h.proxy_command.clone(),
            host_key_policy: h.host_key_policy,
            backend: h.backend,
            keepalive_secs: h.keepalive_secs,
            agent_forwarding: h.agent_forwarding,
            tags: h.tags.clone(),
            notes: h.notes.clone(),
            metadata: h.metadata.clone().into_iter().collect(),
            created_at_ms: ms(&h.created_at),
            updated_at_ms: ms(&h.updated_at),
            auth_mode: crate::HostAuthMode::of(h),
        }
    }
    fn to_model(&self, id: ObjectId, existing: Option<&Host>) -> AppResult<Host> {
        let h = self.to_model_unchecked(id, existing)?;
        h.validate()?;
        Ok(h)
    }
}

impl HostDto {
    pub(crate) fn from_rdp(h: &cc_models::rdp::RdpHost) -> Self {
        let mut dto = Self::from_model(&h.host);
        dto.protocol = HostProtocol::Rdp;
        dto.rdp_domain = h.domain.clone();
        dto.rdp_width = h.desktop_width;
        dto.rdp_height = h.desktop_height;
        dto
    }
    pub(crate) fn to_rdp_model(
        &self,
        id: ObjectId,
        existing: Option<&cc_models::rdp::RdpHost>,
    ) -> AppResult<cc_models::rdp::RdpHost> {
        let host = self.to_model(id, existing.map(|h| &h.host))?;
        let h = cc_models::rdp::RdpHost {
            host,
            domain: clean_opt(&self.rdp_domain),
            desktop_width: self.rdp_width,
            desktop_height: self.rdp_height,
        };
        h.validate()?;
        Ok(h)
    }
    /// The model without cc-models validation (planner previews of drafts
    /// being edited). Malformed ids are still refused.
    pub(crate) fn to_model_unchecked(
        &self,
        id: ObjectId,
        existing: Option<&Host>,
    ) -> AppResult<Host> {
        let (created_at, updated_at) = created(existing.map(|e| e.created_at));
        let h = Host {
            id,
            name: self.name.trim().to_owned(),
            address: self.address.trim().to_owned(),
            port: self.port,
            username: clean_opt(&self.username),
            credential_id: parse_opt_id("credential_id", &self.credential_id)?,
            group_id: parse_opt_id("group_id", &self.group_id)?,
            jump_chain: parse_ids("jump_chain", &self.jump_chain)?,
            jump_profile_id: parse_opt_id("jump_profile_id", &self.jump_profile_id)?,
            proxy_id: parse_opt_id("proxy_id", &self.proxy_id)?,
            proxy_command: clean_opt(&self.proxy_command),
            host_key_policy: self.host_key_policy,
            backend: self.backend,
            keepalive_secs: self.keepalive_secs,
            agent_forwarding: self.agent_forwarding,
            tags: self.tags.clone(),
            notes: self.notes.clone(),
            metadata: self.metadata.clone().into_iter().collect(),
            created_at,
            updated_at,
        };
        Ok(h)
    }
}

/// Host group with inheritable defaults. Empty `id` = create.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GroupDto {
    pub id: String,
    pub name: String,
    pub parent_id: Option<String>,
    pub inherited_username: Option<String>,
    pub inherited_port: Option<u16>,
    pub inherited_credential_id: Option<String>,
    pub inherited_jump_profile_id: Option<String>,
    pub tags: Vec<String>,
    pub created_at_ms: i64,
    pub updated_at_ms: i64,
}

impl GroupDto {
    pub fn new(name: impl Into<String>) -> Self {
        let mut d = Self::from_model(&Group::new(name));
        d.id.clear();
        d
    }
}

impl EditableDto for GroupDto {
    type Model = Group;
    const KIND: ObjectKind = ObjectKind::Group;
    fn id_str(&self) -> &str {
        &self.id
    }
    fn from_model(g: &Group) -> Self {
        Self {
            id: g.id.to_string(),
            name: g.name.clone(),
            parent_id: opt_id(&g.parent_id),
            inherited_username: g.inherited_username.clone(),
            inherited_port: g.inherited_port,
            inherited_credential_id: opt_id(&g.inherited_credential_id),
            inherited_jump_profile_id: opt_id(&g.inherited_jump_profile_id),
            tags: g.tags.clone(),
            created_at_ms: ms(&g.created_at),
            updated_at_ms: ms(&g.updated_at),
        }
    }
    fn to_model(&self, id: ObjectId, existing: Option<&Group>) -> AppResult<Group> {
        require_name("name", &self.name)?;
        if self.inherited_port == Some(0) {
            return Err(AppError::invalid("inherited_port", "must be 1..=65535"));
        }
        let (created_at, updated_at) = created(existing.map(|e| e.created_at));
        let parent_id = parse_opt_id("parent_id", &self.parent_id)?;
        if parent_id == Some(id) {
            return Err(AppError::invalid(
                "parent_id",
                "a group cannot be its own parent",
            ));
        }
        Ok(Group {
            id,
            name: self.name.trim().to_owned(),
            parent_id,
            inherited_username: clean_opt(&self.inherited_username),
            inherited_port: self.inherited_port,
            inherited_credential_id: parse_opt_id(
                "inherited_credential_id",
                &self.inherited_credential_id,
            )?,
            inherited_jump_profile_id: parse_opt_id(
                "inherited_jump_profile_id",
                &self.inherited_jump_profile_id,
            )?,
            tags: self.tags.clone(),
            created_at,
            updated_at,
        })
    }
}

/// Reusable jump chain. Empty `id` = create.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct JumpProfileDto {
    pub id: String,
    pub name: String,
    /// Host ids, first = closest to the client.
    pub chain: Vec<String>,
    pub created_at_ms: i64,
    pub updated_at_ms: i64,
}

impl EditableDto for JumpProfileDto {
    type Model = JumpProfile;
    const KIND: ObjectKind = ObjectKind::JumpProfile;
    fn id_str(&self) -> &str {
        &self.id
    }
    fn from_model(p: &JumpProfile) -> Self {
        Self {
            id: p.id.to_string(),
            name: p.name.clone(),
            chain: ids(&p.chain),
            created_at_ms: ms(&p.created_at),
            updated_at_ms: ms(&p.updated_at),
        }
    }
    fn to_model(&self, id: ObjectId, existing: Option<&JumpProfile>) -> AppResult<JumpProfile> {
        require_name("name", &self.name)?;
        let chain = parse_ids("chain", &self.chain)?;
        if chain.is_empty() {
            return Err(AppError::invalid("chain", "must contain at least one host"));
        }
        let (created_at, updated_at) = created(existing.map(|e| e.created_at));
        Ok(JumpProfile {
            id,
            name: self.name.trim().to_owned(),
            chain,
            created_at,
            updated_at,
        })
    }
}

/// SOCKS5 / HTTP CONNECT proxy. Empty `id` = create. The password is set
/// with [`crate::AppCore::set_proxy_password`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProxyDto {
    pub id: String,
    pub name: String,
    pub kind: ProxyKind,
    pub address: String,
    pub port: u16,
    pub username: Option<String>,
    /// Output only.
    pub has_password: bool,
    pub created_at_ms: i64,
    pub updated_at_ms: i64,
}

impl EditableDto for ProxyDto {
    type Model = Proxy;
    const KIND: ObjectKind = ObjectKind::Proxy;
    fn id_str(&self) -> &str {
        &self.id
    }
    fn from_model(p: &Proxy) -> Self {
        Self {
            id: p.id.to_string(),
            name: p.name.clone(),
            kind: p.kind,
            address: p.address.clone(),
            port: p.port,
            username: p.username.clone(),
            has_password: p.password_secret_id.is_some(),
            created_at_ms: ms(&p.created_at),
            updated_at_ms: ms(&p.updated_at),
        }
    }
    fn to_model(&self, id: ObjectId, existing: Option<&Proxy>) -> AppResult<Proxy> {
        require_name("name", &self.name)?;
        let addr = self.address.trim();
        if addr.is_empty() || addr.chars().any(char::is_whitespace) {
            return Err(AppError::invalid("address", "must be a host name or IP"));
        }
        if self.port == 0 {
            return Err(AppError::invalid("port", "must be 1..=65535"));
        }
        let (created_at, updated_at) = created(existing.map(|e| e.created_at));
        Ok(Proxy {
            id,
            name: self.name.trim().to_owned(),
            kind: self.kind,
            address: addr.to_owned(),
            port: self.port,
            username: clean_opt(&self.username),
            password_secret_id: existing.and_then(|e| e.password_secret_id),
            created_at,
            updated_at,
        })
    }
}

/// Credential metadata (never the secret). Created with the dedicated
/// `add_*` / `generate_*` / `import_*` methods; `name`, `username` and
/// `certificate` are editable via [`crate::AppCore::update_credential`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CredentialDto {
    pub id: String,
    pub name: String,
    pub kind: CredentialKind,
    pub username: Option<String>,
    /// A password / private key Secret is attached.
    pub has_secret: bool,
    /// The key passphrase is remembered (separate Secret object).
    pub has_remembered_passphrase: bool,
    pub key_encrypted: bool,
    pub key_algorithm: Option<KeyAlgorithm>,
    /// OpenSSH public key line (not secret).
    pub public_key: Option<String>,
    pub certificate: Option<String>,
    pub fingerprint: Option<String>,
    pub agent_path: Option<String>,
    pub created_at_ms: i64,
    pub updated_at_ms: i64,
    /// Output only: the host owning this credential (inline password,
    /// ADR-0101 §6a); hide it from shared-credential pickers.
    pub owner_host_id: Option<String>,
}

impl CredentialDto {
    pub(crate) fn from_model(c: &Credential) -> Self {
        Self {
            id: c.id.to_string(),
            name: c.name.clone(),
            kind: c.kind,
            username: c.username.clone(),
            has_secret: c.secret_id.is_some(),
            has_remembered_passphrase: c.passphrase_secret_id.is_some(),
            key_encrypted: c.key_encrypted,
            key_algorithm: c.key_algorithm,
            public_key: c.public_key.clone(),
            certificate: c.certificate.clone(),
            fingerprint: c.fingerprint.clone(),
            agent_path: c.agent_path.clone(),
            created_at_ms: ms(&c.created_at),
            updated_at_ms: ms(&c.updated_at),
            owner_host_id: None,
        }
    }
}

/// Port forwarding profile. Empty `id` = create.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TunnelDto {
    pub id: String,
    pub name: String,
    pub kind: TunnelKind,
    pub host_id: String,
    pub bind_host: String,
    pub bind_port: u16,
    pub target_host: Option<String>,
    pub target_port: Option<u16>,
    pub auto_start: bool,
    /// Output only: binds to a non-loopback address.
    pub binds_publicly: bool,
    pub created_at_ms: i64,
    pub updated_at_ms: i64,
}

impl EditableDto for TunnelDto {
    type Model = Tunnel;
    const KIND: ObjectKind = ObjectKind::Tunnel;
    fn id_str(&self) -> &str {
        &self.id
    }
    fn from_model(t: &Tunnel) -> Self {
        Self {
            id: t.id.to_string(),
            name: t.name.clone(),
            kind: t.kind,
            host_id: t.host_id.to_string(),
            bind_host: t.bind_host.clone(),
            bind_port: t.bind_port,
            target_host: t.target_host.clone(),
            target_port: t.target_port,
            auto_start: t.auto_start,
            binds_publicly: t.binds_publicly(),
            created_at_ms: ms(&t.created_at),
            updated_at_ms: ms(&t.updated_at),
        }
    }
    fn to_model(&self, id: ObjectId, existing: Option<&Tunnel>) -> AppResult<Tunnel> {
        require_name("name", &self.name)?;
        let (created_at, updated_at) = created(existing.map(|e| e.created_at));
        let t = Tunnel {
            id,
            name: self.name.trim().to_owned(),
            kind: self.kind,
            host_id: parse_id("host_id", &self.host_id)?,
            bind_host: self.bind_host.trim().to_owned(),
            bind_port: self.bind_port,
            target_host: clean_opt(&self.target_host),
            target_port: self.target_port,
            auto_start: self.auto_start,
            created_at,
            updated_at,
        };
        t.validate()?;
        Ok(t)
    }
}

/// Template variable of a snippet.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SnippetVariableDto {
    pub name: String,
    pub description: String,
    pub default: Option<String>,
    pub required: bool,
}

/// Command snippet. Empty `id` = create. Empty `variables` are derived from
/// the template's `{{placeholders}}`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SnippetDto {
    pub id: String,
    pub name: String,
    pub description: String,
    #[serde(default)]
    pub package_name: Option<String>,
    #[serde(default)]
    pub catalog_id: Option<String>,
    pub snippet_type: SnippetType,
    pub shell: Option<String>,
    pub template: String,
    pub variables: Vec<SnippetVariableDto>,
    pub tags: Vec<String>,
    pub risk_level: RiskLevel,
    pub source: SnippetSource,
    pub created_by_device_id: Option<String>,
    pub last_used_at_ms: Option<i64>,
    pub usage_count: u64,
    pub created_at_ms: i64,
    pub updated_at_ms: i64,
}

impl EditableDto for SnippetDto {
    type Model = Snippet;
    const KIND: ObjectKind = ObjectKind::Snippet;
    fn id_str(&self) -> &str {
        &self.id
    }
    fn from_model(s: &Snippet) -> Self {
        Self {
            id: s.id.to_string(),
            name: s.name.clone(),
            description: s.description.clone(),
            package_name: s.package_name.clone(),
            catalog_id: s.catalog_id.clone(),
            snippet_type: s.snippet_type,
            shell: s.shell.clone(),
            template: s.template.clone(),
            variables: s
                .variables
                .iter()
                .map(|v| SnippetVariableDto {
                    name: v.name.clone(),
                    description: v.description.clone(),
                    default: v.default.clone(),
                    required: v.required,
                })
                .collect(),
            tags: s.tags.clone(),
            risk_level: s.risk_level,
            source: s.source,
            created_by_device_id: s.created_by.map(|d| d.to_string()),
            last_used_at_ms: opt_ms(&s.last_used_at),
            usage_count: s.usage_count,
            created_at_ms: ms(&s.created_at),
            updated_at_ms: ms(&s.updated_at),
        }
    }
    fn to_model(&self, id: ObjectId, existing: Option<&Snippet>) -> AppResult<Snippet> {
        require_name("name", &self.name)?;
        if self.template.trim().is_empty() {
            return Err(AppError::invalid("template", "must not be empty"));
        }
        let (created_at, updated_at) = created(existing.map(|e| e.created_at));
        let variables = if self.variables.is_empty() {
            cc_models::snippet::template_variables(&self.template)
                .into_iter()
                .map(|name| SnippetVariable {
                    name,
                    description: String::new(),
                    default: None,
                    required: true,
                })
                .collect()
        } else {
            self.variables
                .iter()
                .map(|v| SnippetVariable {
                    name: v.name.clone(),
                    description: v.description.clone(),
                    default: v.default.clone(),
                    required: v.required,
                })
                .collect()
        };
        Ok(Snippet {
            id,
            name: self.name.trim().to_owned(),
            description: self.description.clone(),
            snippet_type: self.snippet_type,
            package_name: clean_opt(&self.package_name),
            catalog_id: clean_opt(&self.catalog_id),
            shell: clean_opt(&self.shell),
            template: self.template.clone(),
            variables,
            tags: self.tags.clone(),
            risk_level: self.risk_level,
            source: self.source,
            created_by: existing.and_then(|e| e.created_by),
            created_at,
            updated_at,
            last_used_at: self.last_used_at_ms.and_then(from_ms),
            usage_count: self.usage_count,
        })
    }
}

/// Knowledge-base note. Empty `id` = create.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NoteDto {
    pub id: String,
    pub title: String,
    pub body: String,
    pub tags: Vec<String>,
    pub created_at_ms: i64,
    pub updated_at_ms: i64,
}

impl EditableDto for NoteDto {
    type Model = Note;
    const KIND: ObjectKind = ObjectKind::Note;
    fn id_str(&self) -> &str {
        &self.id
    }
    fn from_model(n: &Note) -> Self {
        Self {
            id: n.id.to_string(),
            title: n.title.clone(),
            body: n.body.clone(),
            tags: n.tags.clone(),
            created_at_ms: ms(&n.created_at),
            updated_at_ms: ms(&n.updated_at),
        }
    }
    fn to_model(&self, id: ObjectId, existing: Option<&Note>) -> AppResult<Note> {
        require_name("title", &self.title)?;
        let (created_at, updated_at) = created(existing.map(|e| e.created_at));
        Ok(Note {
            id,
            title: self.title.trim().to_owned(),
            body: self.body.clone(),
            tags: self.tags.clone(),
            created_at,
            updated_at,
        })
    }
}

/// Known host key (synced so every device verifies hosts the same way).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KnownHostDto {
    pub id: String,
    pub host_pattern: String,
    pub key_type: String,
    pub public_key: String,
    pub fingerprint_sha256: String,
    pub source: KnownHostSource,
    pub revoked: bool,
    pub marker: Option<KnownHostMarker>,
    pub added_at_ms: i64,
    pub updated_at_ms: i64,
}

impl KnownHostDto {
    pub(crate) fn from_model(k: &KnownHost) -> Self {
        Self {
            id: k.id.to_string(),
            host_pattern: k.host_pattern.clone(),
            key_type: k.key_type.clone(),
            public_key: k.public_key.clone(),
            fingerprint_sha256: k.fingerprint_sha256.clone(),
            source: k.source,
            revoked: k.is_revoked(),
            marker: k.marker,
            added_at_ms: ms(&k.added_at),
            updated_at_ms: ms(&k.updated_at),
        }
    }
}

/// Vault-wide synced settings (singleton).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VaultSettingsDto {
    pub id: String,
    pub vault_name: String,
    pub terminal_history_mode: TerminalHistoryMode,
    pub default_privacy_profile: PrivacyProfile,
    pub sync_ai_conversations: bool,
    pub created_at_ms: i64,
    pub updated_at_ms: i64,
}

impl EditableDto for VaultSettingsDto {
    type Model = VaultSettings;
    const KIND: ObjectKind = ObjectKind::VaultSettings;
    fn id_str(&self) -> &str {
        &self.id
    }
    fn from_model(s: &VaultSettings) -> Self {
        Self {
            id: s.id.to_string(),
            vault_name: s.vault_name.clone(),
            terminal_history_mode: s.terminal_history_mode,
            default_privacy_profile: s.default_privacy_profile,
            sync_ai_conversations: s.sync_ai_conversations,
            created_at_ms: ms(&s.created_at),
            updated_at_ms: ms(&s.updated_at),
        }
    }
    fn to_model(&self, id: ObjectId, existing: Option<&VaultSettings>) -> AppResult<VaultSettings> {
        require_name("vault_name", &self.vault_name)?;
        let (created_at, updated_at) = created(existing.map(|e| e.created_at));
        Ok(VaultSettings {
            id,
            vault_name: self.vault_name.trim().to_owned(),
            terminal_history_mode: self.terminal_history_mode,
            default_privacy_profile: self.default_privacy_profile,
            sync_ai_conversations: self.sync_ai_conversations,
            created_at,
            updated_at,
        })
    }
}

/// AI provider configuration. Empty `id` = create. The API key is set with
/// [`crate::AppCore::set_ai_provider_api_key`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AiProviderDto {
    pub id: String,
    pub name: String,
    pub provider: AiProviderKind,
    pub base_url: String,
    /// Output only.
    pub has_api_key: bool,
    pub chat_model: String,
    pub embedding_model: Option<String>,
    pub timeout_secs: u32,
    pub streaming: bool,
    pub tool_support: bool,
    pub privacy_profile: PrivacyProfile,
    pub is_default: bool,
    pub created_at_ms: i64,
    pub updated_at_ms: i64,
}

impl EditableDto for AiProviderDto {
    type Model = AiProviderConfig;
    const KIND: ObjectKind = ObjectKind::AiProvider;
    fn id_str(&self) -> &str {
        &self.id
    }
    fn from_model(a: &AiProviderConfig) -> Self {
        Self {
            id: a.id.to_string(),
            name: a.name.clone(),
            provider: a.provider,
            base_url: a.base_url.clone(),
            has_api_key: a.api_key_secret_id.is_some(),
            chat_model: a.chat_model.clone(),
            embedding_model: a.embedding_model.clone(),
            timeout_secs: a.timeout_secs,
            streaming: a.streaming,
            tool_support: a.tool_support,
            privacy_profile: a.privacy_profile,
            is_default: a.is_default,
            created_at_ms: ms(&a.created_at),
            updated_at_ms: ms(&a.updated_at),
        }
    }
    fn to_model(
        &self,
        id: ObjectId,
        existing: Option<&AiProviderConfig>,
    ) -> AppResult<AiProviderConfig> {
        require_name("name", &self.name)?;
        let url = self.base_url.trim();
        if !(url.starts_with("http://") || url.starts_with("https://")) {
            return Err(AppError::invalid("base_url", "must be an http(s) URL"));
        }
        if self.chat_model.trim().is_empty() {
            return Err(AppError::invalid("chat_model", "must not be empty"));
        }
        let (created_at, updated_at) = created(existing.map(|e| e.created_at));
        Ok(AiProviderConfig {
            id,
            name: self.name.trim().to_owned(),
            provider: self.provider,
            base_url: url.to_owned(),
            api_key_secret_id: existing.and_then(|e| e.api_key_secret_id),
            chat_model: self.chat_model.trim().to_owned(),
            embedding_model: clean_opt(&self.embedding_model),
            timeout_secs: self.timeout_secs.max(1),
            streaming: self.streaming,
            tool_support: self.tool_support,
            privacy_profile: self.privacy_profile,
            is_default: self.is_default,
            created_at,
            updated_at,
        })
    }
}

/// Storage / sync metadata of one object.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ObjectMetaDto {
    pub id: String,
    pub kind: ObjectKind,
    pub revision: i64,
    pub local_state: LocalState,
    /// Set on conflict copies: the object this copy was made from.
    pub conflict_origin: Option<String>,
}

// ---- sync ------------------------------------------------------------------------

/// Coarse sync state for the status indicator.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SyncPhaseDto {
    /// Local profile: no sync ("Local only" indicator).
    LocalOnly,
    /// Synced profile, engine not running (locked or background sync off).
    NotRunning,
    Idle,
    Syncing,
    Offline,
    Error,
    Stopped,
}

/// Why the sync engine stopped for good.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SyncStopReasonDto {
    DeviceRevoked,
    ReauthRequired,
    NotTrusted,
    VaultGone,
    UpgradeRequired,
    Shutdown,
}

impl From<cc_sync_core::StopReason> for SyncStopReasonDto {
    fn from(r: cc_sync_core::StopReason) -> Self {
        use cc_sync_core::StopReason as S;
        match r {
            S::DeviceRevoked => Self::DeviceRevoked,
            S::ReauthRequired => Self::ReauthRequired,
            S::NotTrusted => Self::NotTrusted,
            S::VaultGone => Self::VaultGone,
            S::UpgradeRequired => Self::UpgradeRequired,
            S::Shutdown => Self::Shutdown,
        }
    }
}

/// Sync status snapshot.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SyncStatusDto {
    pub phase: SyncPhaseDto,
    pub pending: u64,
    pub conflicts: u64,
    pub failed: u64,
    pub last_error: Option<String>,
    pub stop_reason: Option<SyncStopReasonDto>,
    pub last_sync_at_ms: Option<i64>,
    pub last_sequence: i64,
    pub server_latest_sequence: i64,
    pub next_retry_in_ms: Option<u64>,
}

impl SyncStatusDto {
    pub(crate) fn local_only() -> Self {
        Self::with_phase(SyncPhaseDto::LocalOnly)
    }

    pub(crate) fn with_phase(phase: SyncPhaseDto) -> Self {
        Self {
            phase,
            pending: 0,
            conflicts: 0,
            failed: 0,
            last_error: None,
            stop_reason: None,
            last_sync_at_ms: None,
            last_sequence: 0,
            server_latest_sequence: 0,
            next_retry_in_ms: None,
        }
    }

    pub(crate) fn from_status(s: &cc_sync_core::SyncStatus) -> Self {
        use cc_sync_core::SyncPhase as P;
        Self {
            phase: match s.phase {
                P::Idle => SyncPhaseDto::Idle,
                P::Syncing => SyncPhaseDto::Syncing,
                P::Offline => SyncPhaseDto::Offline,
                P::Error => SyncPhaseDto::Error,
                P::Stopped => SyncPhaseDto::Stopped,
            },
            pending: s.pending,
            conflicts: s.conflicts,
            failed: s.failed,
            last_error: s.last_error.clone(),
            stop_reason: s.stop_reason.map(Into::into),
            last_sync_at_ms: opt_ms(&s.last_sync_at),
            last_sequence: s.last_sequence,
            server_latest_sequence: s.server_latest_sequence,
            next_retry_in_ms: s.next_retry_in.map(|d| d.as_millis() as u64),
        }
    }
}

/// What one sync cycle did.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SyncReportDto {
    pub pushed: u64,
    pub pulled: u64,
    pub conflicts: u64,
    pub resolved: u64,
    pub failed: u64,
    pub last_sequence: i64,
}

impl From<&cc_sync_core::SyncReport> for SyncReportDto {
    fn from(r: &cc_sync_core::SyncReport) -> Self {
        Self {
            pushed: r.pushed as u64,
            pulled: r.pulled as u64,
            conflicts: r.conflicts as u64,
            resolved: r.resolved as u64,
            failed: r.failed as u64,
            last_sequence: r.last_sequence,
        }
    }
}

// ---- devices ---------------------------------------------------------------------

/// A device of the account.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeviceDto {
    pub device_id: String,
    pub name: String,
    pub platform: String,
    /// `active` | `revoked`.
    pub status: String,
    /// Holds a device envelope for this profile's vault.
    pub trusted_for_vault: bool,
    pub is_current: bool,
    pub created_at_ms: i64,
    pub last_seen_at_ms: Option<i64>,
    pub revoked_at_ms: Option<i64>,
}

/// A pending "trust this device" request.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TrustRequestDto {
    pub request_id: String,
    pub device_id: String,
    pub device_name: String,
    pub platform: String,
    pub vault_ids: Vec<String>,
    /// `pending` | `approved` | `rejected` | `expired`.
    pub status: String,
    pub created_at_ms: i64,
    pub expires_at_ms: i64,
}

/// Devices and pending requests.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeviceListDto {
    pub devices: Vec<DeviceDto>,
    pub pending_requests: Vec<TrustRequestDto>,
}

/// Step 1 of approving a device: show `verification_code` and ask "does it
/// match the code on <device_name>?". Only then call
/// [`crate::AppCore::confirm_device_approval`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PendingApprovalDto {
    pub request_id: String,
    pub device_id: String,
    pub device_name: String,
    /// 6 groups of 5 digits, computed from the keys the server reports.
    pub verification_code: String,
    pub requested_vault_ids: Vec<String>,
}

/// The trust request this (new) device created.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OwnTrustRequestDto {
    pub request_id: String,
    /// This device's code; the approving device must show the same one.
    pub verification_code: String,
    pub expires_at_ms: i64,
}

// ---- ssh -------------------------------------------------------------------------

/// Output of a remote command.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExecResultDto {
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
    pub exit_status: Option<u32>,
    pub exit_signal: Option<String>,
}

/// Terminal session state.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum TerminalStatusDto {
    Connecting,
    Connected,
    Closed {
        exit_status: Option<u32>,
        reason: Option<String>,
    },
    Failed {
        message: String,
    },
}

impl From<&cc_terminal_core::TerminalStatus> for TerminalStatusDto {
    fn from(s: &cc_terminal_core::TerminalStatus) -> Self {
        use cc_terminal_core::TerminalStatus as S;
        match s {
            S::Connecting => Self::Connecting,
            S::Connected => Self::Connected,
            S::Closed {
                exit_status,
                reason,
            } => Self::Closed {
                exit_status: *exit_status,
                reason: reason.clone(),
            },
            S::Failed(m) => Self::Failed { message: m.clone() },
        }
    }
}

/// An open terminal tab.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TerminalInfoDto {
    pub id: String,
    pub host_id: String,
    pub title: String,
    pub status: TerminalStatusDto,
    pub cols: u32,
    pub rows: u32,
}

/// One item of a terminal output stream.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum TerminalChunk {
    Data(Vec<u8>),
    /// Output was dropped (slow consumer): re-attach for a fresh snapshot.
    Lagged,
    /// The session ended; no more chunks follow.
    Closed(TerminalStatusDto),
}

/// Tunnel runtime state.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", content = "reason", rename_all = "snake_case")]
pub enum TunnelStateDto {
    Running,
    Failed(String),
    Stopped,
}

/// Running tunnel.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TunnelStatusDto {
    pub id: String,
    pub name: String,
    pub kind: TunnelKind,
    pub state: TunnelStateDto,
    /// Actual listen address (port resolved if 0 was requested).
    pub listen: String,
    pub bytes_sent: u64,
    pub bytes_received: u64,
    pub active_connections: u64,
    pub total_connections: u64,
    pub failed_connections: u64,
    pub warnings: Vec<String>,
    pub started_at_ms: i64,
}

impl From<&cc_tunnel_core::TunnelStatus> for TunnelStatusDto {
    fn from(s: &cc_tunnel_core::TunnelStatus) -> Self {
        use cc_tunnel_core::TunnelState as S;
        Self {
            id: s.id.to_string(),
            name: s.name.clone(),
            kind: s.kind,
            state: match &s.state {
                S::Running => TunnelStateDto::Running,
                S::Failed(r) => TunnelStateDto::Failed(r.clone()),
                S::Stopped => TunnelStateDto::Stopped,
            },
            listen: s.listen.clone(),
            bytes_sent: s.bytes_sent,
            bytes_received: s.bytes_received,
            active_connections: s.active_connections,
            total_connections: s.total_connections,
            failed_connections: s.failed_connections,
            warnings: s.warnings.clone(),
            started_at_ms: ms(&s.started_at),
        }
    }
}

/// A remote directory entry (SFTP).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RemoteEntryDto {
    pub name: String,
    pub path: String,
    pub is_dir: bool,
    /// `file` | `dir` | `symlink` | `other`.
    pub kind: String,
    pub size: u64,
    pub permissions: Option<u32>,
    /// `drwxr-xr-x` style.
    pub mode: String,
    pub modified_at_ms: Option<i64>,
}

impl From<&cc_sftp_core::RemoteEntry> for RemoteEntryDto {
    fn from(e: &cc_sftp_core::RemoteEntry) -> Self {
        let kind = format!("{:?}", e.kind).to_ascii_lowercase();
        Self {
            name: e.name.clone(),
            path: e.path.clone(),
            is_dir: e.is_dir(),
            kind,
            size: e.size,
            permissions: e.permissions,
            mode: e.mode_string(),
            modified_at_ms: opt_ms(&e.modified),
        }
    }
}

/// Planned route of a host (`user@hop:port -> … -> user@target:port`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConnectionRouteDto {
    pub route: String,
    pub warnings: Vec<String>,
}

/// Result of a `known_hosts` import.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct KnownHostsImportDto {
    pub imported: u32,
    /// Unparsable lines.
    pub skipped: u32,
}

/// A tunnel that failed to auto-start.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TunnelFailureDto {
    pub tunnel_id: String,
    pub message: String,
}

/// Environment variable of a prepared process.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EnvVarDto {
    pub name: String,
    pub value: String,
}

/// Terminal output attachment: local scrollback snapshot + live stream
/// (ends with [`TerminalChunk::Closed`]). FRB: forward `output` into a
/// `StreamSink<TerminalChunk>`.
#[derive(Debug)]
pub struct TerminalAttachment {
    pub snapshot: Vec<u8>,
    pub output: tokio::sync::mpsc::Receiver<TerminalChunk>,
}

/// Completed transfer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TransferSummaryDto {
    pub transfer_id: String,
    pub bytes: u64,
    pub elapsed_ms: u64,
}

/// "Unknown host key" question (policy `Ask`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HostKeyPromptDto {
    pub request_id: String,
    pub host: String,
    pub port: u16,
    pub host_pattern: String,
    pub host_id: Option<String>,
    pub host_name: Option<String>,
    pub hop_index: u32,
    pub hop_count: u32,
    pub key_type: String,
    pub fingerprint_sha256: String,
    /// Other key types already known for the host — be extra careful.
    pub other_known_key_types: Vec<String>,
}

/// "Enter the passphrase of this key" question.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PassphrasePromptDto {
    pub request_id: String,
    pub credential_id: String,
    pub credential_name: String,
    /// 0 = first try; >0 after a wrong passphrase.
    pub attempt: u32,
}

/// "Enter the password for this host" question (host auth mode
/// `PasswordPrompt`, ADR-0101 §6a). The answer is used once, never stored.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PasswordPromptDto {
    pub request_id: String,
    pub host_id: Option<String>,
    pub host_name: String,
}

/// A question the UI must answer (see [`crate::AppCore::subscribe_prompts`]).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum PromptRequest {
    HostKey(HostKeyPromptDto),
    Passphrase(PassphrasePromptDto),
    Password(PasswordPromptDto),
}

// ---- backup ----------------------------------------------------------------------

/// Written / read backup file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BackupSummaryDto {
    pub path: String,
    pub vault_id: String,
    pub objects: u64,
    pub created_at_ms: i64,
}

/// Secret used to open a backup.
#[derive(Clone, PartialEq, Eq, Zeroize, ZeroizeOnDrop)]
pub enum BackupUnlock {
    Passphrase(String),
    /// 24 words or the recovery QR payload.
    RecoveryKey(String),
}

impl std::fmt::Debug for BackupUnlock {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BackupUnlock::Passphrase(_) => f.write_str("BackupUnlock::Passphrase(<redacted>)"),
            BackupUnlock::RecoveryKey(_) => f.write_str("BackupUnlock::RecoveryKey(<redacted>)"),
        }
    }
}

// ---- events ----------------------------------------------------------------------

/// Who changed an object.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChangeOriginDto {
    Local,
    Remote,
    ConflictResolution,
}

/// Everything the UI may want to react to (see
/// [`crate::AppCore::subscribe_events`]). Metadata only — re-read objects
/// through the facade.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AppEvent {
    /// Profiles were created/removed/renamed or the active one changed.
    ProfilesChanged,
    VaultUnlocked {
        profile_id: String,
    },
    VaultLocked {
        profile_id: String,
    },
    /// Objects of the open vault changed (created, updated or deleted).
    ObjectsChanged {
        kind: ObjectKind,
        ids: Vec<String>,
        origin: ChangeOriginDto,
    },
    /// Automatic conflict resolution; warn the user for secrets / copies.
    ConflictResolved {
        object_id: String,
        conflict_copy_id: Option<String>,
        is_secret: bool,
    },
    /// Server ciphertext failed verification (possible tampering).
    IntegrityWarning {
        object_id: String,
    },
    SyncStatus(SyncStatusDto),
    SyncStopped {
        reason: SyncStopReasonDto,
    },
    /// The session expired: call `reauthenticate`.
    ReauthRequired,
    /// A device was revoked (`is_self`: this installation — sync stopped;
    /// reconnecting needs a new device identity).
    DeviceRevoked {
        device_id: String,
        is_self: bool,
    },
    /// Another device asks to be trusted (approve on a trusted device).
    DeviceApprovalRequested {
        request_id: String,
        device_id: String,
    },
    /// A device was approved (`is_self`: call `finish_device_approval`).
    DeviceApproved {
        device_id: String,
        is_self: bool,
    },
    /// Password or recovery envelope replaced on another device.
    RecoveryChanged,
    TerminalStatus {
        terminal_id: String,
        status: TerminalStatusDto,
    },
    TunnelStarted {
        tunnel_id: String,
    },
    TunnelStopped {
        tunnel_id: String,
    },
    TunnelFailed {
        tunnel_id: String,
        reason: String,
    },
    TransferProgress {
        transfer_id: String,
        transferred: u64,
        total: Option<u64>,
    },
    /// The server was restored from an older backup (vault epoch changed or
    /// sequences regressed / reused): the vault is re-downloaded and local
    /// objects the server lost are re-uploaded; edits made elsewhere after
    /// the backup may reappear as conflict copies. `reason` =
    /// `epoch_changed` | `sequence_regressed` | `sequence_reused`.
    ServerRollbackDetected {
        vault_id: String,
        reason: String,
    },
    /// Local AI search index status changed (opened, rebuilt, embeddings).
    AiIndexStatus(AiIndexStatusDto),
    /// A transfer job changed (state, progress — throttled).
    TransferUpdate(crate::transfers::TransferJobDto),
    /// An edit session changed status (`session_id` empty: events were
    /// dropped — re-read `edit_sessions`).
    EditStatus {
        session_id: String,
        status: crate::edit::EditStatusDto,
    },
    /// Edit working copies of earlier runs exist ("Recover unsaved
    /// edits?" → `edit_leftovers`). Emitted once after unlock.
    EditLeftovers {
        count: u32,
    },
    /// The account of the synced profile was signed out (`logout`).
    SignedOut {
        profile_id: String,
    },
    /// Progress of `enable_sync` (`step` = `authenticating` |
    /// `creating_remote_vault` | `reconnecting` | `uploading` | `finishing`
    /// | `done`; `uploaded` / `total` objects while uploading).
    EnableSyncProgress {
        step: String,
        uploaded: u64,
        total: u64,
    },
    /// A backup was written (scheduled or `backup_now`).
    BackupCompleted {
        backup: crate::backup::BackupInfoDto,
    },
    /// A scheduled backup failed (recorded in the schedule's `last_error`).
    BackupFailed {
        error: crate::transfers::ErrorInfoDto,
    },
    /// The backup schedule or the list of recent backups changed.
    BackupScheduleChanged,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn host_roundtrip_and_validation() {
        let mut d = HostDto::new("db", "10.0.0.5");
        assert!(d.id.is_empty());
        d.port = Some(2222);
        let id = ObjectId::new();
        let h = d.to_model(id, None).unwrap();
        assert_eq!(h.id, id);
        let back = HostDto::from_model(&h);
        assert_eq!(back.port, Some(2222));
        d.address = "bad address".into();
        assert!(matches!(
            d.to_model(id, None),
            Err(AppError::InvalidInput { .. })
        ));
        d.address = "ok".into();
        d.credential_id = Some("not-a-uuid".into());
        assert!(d.to_model(id, None).is_err());
        d.credential_id = Some(String::new());
        assert!(d.to_model(id, None).unwrap().credential_id.is_none());
    }

    #[test]
    fn recovery_kit_debug_is_redacted() {
        let k = RecoveryKitDto {
            vault_id: "v".into(),
            words: vec!["abandon".into(); 24],
            phrase: "abandon ".repeat(24),
            qr_payload: "consolecrypt-recovery:v1:x:y".into(),
            server_url: None,
            created_at_ms: 0,
        };
        let s = format!("{k:?}");
        assert!(
            !s.contains("abandon") && !s.contains("consolecrypt-recovery"),
            "{s}"
        );
    }
}
