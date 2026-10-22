//! Vault-wide synced settings (singleton per vault; clients pick the newest
//! `VaultSettings` object if a conflict ever produces two).

use crate::ai::PrivacyProfile;
use crate::{ObjectId, Timestamp};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TerminalHistoryMode {
    #[default]
    LocalOnly,
    EncryptedSync,
    Disabled,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct VaultSettings {
    pub id: ObjectId,
    /// Display name of the vault (kept here, not on the server).
    pub vault_name: String,
    #[serde(default)]
    pub terminal_history_mode: TerminalHistoryMode,
    #[serde(default)]
    pub default_privacy_profile: PrivacyProfile,
    #[serde(default)]
    pub sync_ai_conversations: bool,
    /// Display colour (palette name or `#rrggbb`); teams and personal vaults.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
    /// Icon (symbol name of the app's set).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>,
    /// Free text shown in team settings.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Policies the client enforces for this vault's resources (teams).
    #[serde(default)]
    pub policies: VaultPolicies,
    pub created_at: Timestamp,
    pub updated_at: Timestamp,
}

/// Team policies (TZ teams §6.1 ТР-25). Defaults = no extra restriction.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct VaultPolicies {
    /// New resources are marked production by default.
    #[serde(default)]
    pub production_by_default: bool,
    /// Never send context of these resources to AI providers.
    #[serde(default)]
    pub block_ai_context: bool,
    /// Clear secrets copied from these resources after N seconds (0 = app default).
    #[serde(default)]
    pub clipboard_clear_seconds: u32,
    /// Ask before running risky commands on these resources.
    #[serde(default)]
    pub confirm_risky_commands: bool,
}

/// Validate a colour: a palette name (`[a-z]{1,16}`) or `#rrggbb`.
pub fn valid_color(c: &str) -> bool {
    (c.len() == 7 && c.starts_with('#') && c[1..].chars().all(|x| x.is_ascii_hexdigit()))
        || (!c.is_empty() && c.len() <= 16 && c.chars().all(|x| x.is_ascii_lowercase()))
}
