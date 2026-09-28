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
    pub created_at: Timestamp,
    pub updated_at: Timestamp,
}
