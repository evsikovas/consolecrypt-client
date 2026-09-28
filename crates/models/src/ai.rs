//! AI provider settings and (optionally synced) conversations.

use crate::{ObjectId, Timestamp};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AiProviderKind {
    Ollama,
    LmStudio,
    Deepseek,
    OpenaiCompatible,
}

impl AiProviderKind {
    /// Local providers may receive the `Local` privacy profile by default.
    pub const fn is_local_by_default(&self) -> bool {
        matches!(self, AiProviderKind::Ollama | AiProviderKind::LmStudio)
    }
}

/// How much context may leave the device (see CLIENT_SPEC §15). Private keys
/// and passwords are never sent under any profile.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PrivacyProfile {
    /// Redact secrets and also IPs, hostnames, usernames, DB names.
    #[default]
    Strict,
    /// Redact secrets, keep host metadata.
    Standard,
    /// For local models: more context allowed, secrets still redacted.
    Local,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AiProviderConfig {
    pub id: ObjectId,
    pub name: String,
    pub provider: AiProviderKind,
    pub base_url: String,
    /// `Secret` holding the API key; the AI core never reads it directly — the
    /// HTTP layer injects it into the request header.
    #[serde(default)]
    pub api_key_secret_id: Option<ObjectId>,
    pub chat_model: String,
    #[serde(default)]
    pub embedding_model: Option<String>,
    #[serde(default = "default_timeout")]
    pub timeout_secs: u32,
    #[serde(default = "default_true")]
    pub streaming: bool,
    #[serde(default)]
    pub tool_support: bool,
    #[serde(default)]
    pub privacy_profile: PrivacyProfile,
    #[serde(default)]
    pub is_default: bool,
    pub created_at: Timestamp,
    pub updated_at: Timestamp,
}

fn default_timeout() -> u32 {
    60
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChatRole {
    System,
    User,
    Assistant,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: ChatRole,
    pub content: String,
    pub created_at: Timestamp,
}

/// Synced only if the user enables "sync AI conversations".
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AiConversation {
    pub id: ObjectId,
    pub title: String,
    #[serde(default)]
    pub provider_id: Option<ObjectId>,
    pub messages: Vec<ChatMessage>,
    pub created_at: Timestamp,
    pub updated_at: Timestamp,
}
