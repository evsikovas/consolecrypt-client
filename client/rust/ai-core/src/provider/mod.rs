//! LLM provider abstraction (CLIENT_SPEC §13).
//!
//! * [`LlmProvider`] — capabilities (chat, responses, embeddings, streaming,
//!   tool calling) and the async API.
//! * Implementations: [`OllamaProvider`] (native `/api/chat`, `/api/embed`,
//!   NDJSON streaming) and [`OpenAiCompatibleProvider`] (LM Studio, DeepSeek,
//!   generic OpenAI-compatible: `/v1/chat/completions` with SSE streaming,
//!   `/v1/embeddings`, `/v1/models`, optional `/v1/responses`).
//! * [`build_provider`] constructs one from a synced
//!   [`AiProviderConfig`] plus an optional
//!   API key. The key is handed over as a [`SecretString`] **at construction**
//!   and afterwards lives only inside the HTTP client's default header
//!   (marked sensitive, so `Debug` prints `Sensitive`). There is no API to
//!   read it back, and ai-core has no interface to fetch secrets.
//! * Requests only accept [`SanitizedText`], so nothing unsanitized can be
//!   sent (see [`crate::sanitizer`]).

mod http;
mod ollama;
mod openai;

pub use ollama::OllamaProvider;
pub use openai::OpenAiCompatibleProvider;

use crate::error::{AiError, Result};
use crate::sanitizer::SanitizedText;
use async_trait::async_trait;
use cc_models::ai::{AiProviderConfig, AiProviderKind, PrivacyProfile};
use futures::stream::BoxStream;
use secrecy::SecretString;
use serde::{Deserialize, Serialize};
use std::fmt;
use std::net::IpAddr;
use std::sync::Arc;
use std::time::Duration;
use url::Url;

/// Chat role.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    System,
    User,
    Assistant,
    Tool,
}

impl Role {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Role::System => "system",
            Role::User => "user",
            Role::Assistant => "assistant",
            Role::Tool => "tool",
        }
    }
}

/// A tool the model may call.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ToolDefinition {
    pub name: String,
    pub description: String,
    /// JSON Schema of the arguments object.
    pub parameters: serde_json::Value,
}

/// A tool call requested by the model.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    /// Parsed arguments (a JSON string if the model produced invalid JSON).
    pub arguments: serde_json::Value,
}

/// One chat message. Content is always [`SanitizedText`].
#[derive(Clone, PartialEq)]
pub struct Message {
    pub role: Role,
    content: SanitizedText,
    pub tool_calls: Vec<ToolCall>,
    pub tool_call_id: Option<String>,
}

impl Message {
    pub fn new(role: Role, content: SanitizedText) -> Self {
        Self {
            role,
            content,
            tool_calls: Vec::new(),
            tool_call_id: None,
        }
    }
    pub fn system(content: SanitizedText) -> Self {
        Self::new(Role::System, content)
    }
    pub fn user(content: SanitizedText) -> Self {
        Self::new(Role::User, content)
    }
    pub fn assistant(content: SanitizedText) -> Self {
        Self::new(Role::Assistant, content)
    }
    /// Assistant turn that requested tool calls.
    pub fn assistant_tool_calls(content: SanitizedText, calls: Vec<ToolCall>) -> Self {
        Self {
            tool_calls: calls,
            ..Self::new(Role::Assistant, content)
        }
    }
    /// Result of a tool call (sanitize it first).
    pub fn tool_result(tool_call_id: impl Into<String>, content: SanitizedText) -> Self {
        Self {
            tool_call_id: Some(tool_call_id.into()),
            ..Self::new(Role::Tool, content)
        }
    }
    pub fn content(&self) -> &SanitizedText {
        &self.content
    }
}

impl fmt::Debug for Message {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Message")
            .field("role", &self.role)
            .field("len", &self.content.len())
            .field("tool_calls", &self.tool_calls.len())
            .finish()
    }
}

/// Requested output format.
#[derive(Debug, Clone, PartialEq, Default)]
pub enum ResponseFormat {
    #[default]
    Text,
    /// Any JSON object.
    Json,
    /// JSON matching a schema (falls back to `Json` where unsupported).
    JsonSchema {
        name: String,
        schema: serde_json::Value,
    },
}

/// Chat request.
#[derive(Debug, Clone, Default)]
pub struct ChatRequest {
    pub messages: Vec<Message>,
    pub temperature: Option<f32>,
    pub max_tokens: Option<u32>,
    pub response_format: ResponseFormat,
    pub tools: Vec<ToolDefinition>,
    /// Override the configured chat model.
    pub model: Option<String>,
}

impl ChatRequest {
    pub fn new(messages: Vec<Message>) -> Self {
        Self {
            messages,
            ..Default::default()
        }
    }
}

/// Token usage, when reported.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Usage {
    pub prompt_tokens: Option<u32>,
    pub completion_tokens: Option<u32>,
}

/// Non-streaming chat response.
#[derive(Clone, PartialEq, Default)]
pub struct ChatResponse {
    pub content: String,
    pub tool_calls: Vec<ToolCall>,
    pub finish_reason: Option<String>,
    pub model: Option<String>,
    pub usage: Option<Usage>,
}

impl fmt::Debug for ChatResponse {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ChatResponse")
            .field("len", &self.content.len())
            .field("tool_calls", &self.tool_calls.len())
            .field("finish_reason", &self.finish_reason)
            .field("model", &self.model)
            .field("usage", &self.usage)
            .finish()
    }
}

/// Streaming chat event.
#[derive(Clone, PartialEq)]
pub enum ChatEvent {
    /// Incremental content.
    Delta(String),
    /// A complete tool call (accumulated from deltas where needed).
    ToolCall(ToolCall),
    /// End of the response.
    Done {
        finish_reason: Option<String>,
        usage: Option<Usage>,
    },
}

impl fmt::Debug for ChatEvent {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ChatEvent::Delta(d) => write!(f, "Delta(len={})", d.len()),
            ChatEvent::ToolCall(t) => write!(f, "ToolCall({})", t.name),
            ChatEvent::Done { finish_reason, .. } => write!(f, "Done({finish_reason:?})"),
        }
    }
}

/// Stream of chat events. Dropping it aborts the HTTP request.
pub type ChatStream = BoxStream<'static, Result<ChatEvent>>;

/// Embedding request (texts must be sanitized: remote embedding APIs see
/// the content).
#[derive(Debug, Clone, Default)]
pub struct EmbeddingRequest {
    pub inputs: Vec<SanitizedText>,
    pub model: Option<String>,
}

/// Embedding vectors in input order.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct EmbeddingResponse {
    pub vectors: Vec<Vec<f32>>,
    pub model: String,
}

impl EmbeddingResponse {
    pub fn dim(&self) -> usize {
        self.vectors.first().map_or(0, Vec::len)
    }
}

/// A model advertised by the provider.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelInfo {
    pub id: String,
    pub owned_by: Option<String>,
    pub size_bytes: Option<u64>,
}

/// Structured-output support.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StructuredOutput {
    None,
    JsonObject,
    JsonSchema,
}

/// What a provider can do (CLIENT_SPEC §13.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Capabilities {
    pub chat: bool,
    /// OpenAI Responses API (`/v1/responses`).
    pub responses: bool,
    pub embeddings: bool,
    pub streaming: bool,
    pub tool_calling: bool,
    pub structured_output: StructuredOutput,
}

/// Where the endpoint is, network-wise.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Locality {
    /// localhost / 127.0.0.0/8 / ::1.
    Loopback,
    /// RFC 1918, link-local, CGNAT (e.g. Tailscale), ULA, `.local`/`.lan`/
    /// `.internal`/`.home.arpa` names.
    PrivateNetwork,
    Public,
}

impl Locality {
    /// Classify a base URL.
    pub fn of_url(url: &Url) -> Self {
        match url.host() {
            Some(url::Host::Domain(d)) => {
                let d = d.to_ascii_lowercase();
                if d == "localhost" || d.ends_with(".localhost") {
                    Locality::Loopback
                } else if [".local", ".lan", ".internal", ".home.arpa", ".localdomain"]
                    .iter()
                    .any(|s| d.ends_with(s))
                {
                    Locality::PrivateNetwork
                } else {
                    Locality::Public
                }
            }
            Some(url::Host::Ipv4(ip)) => Self::of_ip(IpAddr::V4(ip)),
            Some(url::Host::Ipv6(ip)) => Self::of_ip(IpAddr::V6(ip)),
            None => Locality::Public,
        }
    }

    fn of_ip(ip: IpAddr) -> Self {
        match ip {
            IpAddr::V4(v4) => {
                let o = v4.octets();
                if v4.is_loopback() {
                    Locality::Loopback
                } else if v4.is_private()
                    || v4.is_link_local()
                    || (o[0] == 100 && (64..128).contains(&o[1]))
                {
                    Locality::PrivateNetwork
                } else {
                    Locality::Public
                }
            }
            IpAddr::V6(v6) => {
                let seg0 = v6.segments()[0];
                if v6.is_loopback() {
                    Locality::Loopback
                } else if (seg0 & 0xfe00) == 0xfc00 || (seg0 & 0xffc0) == 0xfe80 {
                    Locality::PrivateNetwork
                } else if let Some(v4) = v6.to_ipv4_mapped() {
                    Self::of_ip(IpAddr::V4(v4))
                } else {
                    Locality::Public
                }
            }
        }
    }

    /// Loopback or private network.
    pub fn is_local(self) -> bool {
        !matches!(self, Locality::Public)
    }
}

/// The profile actually applied: `Local` is only honoured for loopback /
/// private-network endpoints; public endpoints get at least `Standard`.
pub fn effective_profile(configured: PrivacyProfile, locality: Locality) -> PrivacyProfile {
    match (configured, locality.is_local()) {
        (PrivacyProfile::Local, false) => PrivacyProfile::Standard,
        (p, _) => p,
    }
}

/// Validated, resolved provider settings.
#[derive(Debug, Clone, PartialEq)]
pub struct ProviderSettings {
    pub kind: AiProviderKind,
    pub name: String,
    /// Normalized base URL (for OpenAI-compatible kinds it ends with the API
    /// version segment, e.g. `/v1`).
    pub base_url: Url,
    pub chat_model: String,
    pub embedding_model: Option<String>,
    /// Total timeout for non-streaming requests and idle timeout between
    /// stream chunks.
    pub timeout: Duration,
    pub connect_timeout: Duration,
    pub streaming: bool,
    pub tool_support: bool,
    pub privacy_profile: PrivacyProfile,
}

impl ProviderSettings {
    /// Resolve defaults and validate a synced provider config.
    pub fn from_config(cfg: &AiProviderConfig) -> Result<Self> {
        let raw = cfg.base_url.trim();
        let raw = if raw.is_empty() {
            match cfg.provider {
                AiProviderKind::Ollama => "http://localhost:11434",
                AiProviderKind::LmStudio => "http://localhost:1234/v1",
                AiProviderKind::Deepseek => "https://api.deepseek.com",
                AiProviderKind::OpenaiCompatible => {
                    return Err(AiError::Config("base URL is required".into()))
                }
            }
        } else {
            raw
        };
        let mut url =
            Url::parse(raw).map_err(|e| AiError::Config(format!("invalid base URL: {e}")))?;
        if !matches!(url.scheme(), "http" | "https") {
            return Err(AiError::Config("base URL must use http or https".into()));
        }
        if !url.username().is_empty() || url.password().is_some() {
            return Err(AiError::Config(
                "credentials in the base URL are not allowed; use the API key setting".into(),
            ));
        }
        url.set_query(None);
        url.set_fragment(None);
        if cfg.provider != AiProviderKind::Ollama {
            url = openai::normalize_base(url);
        }
        if cfg.chat_model.trim().is_empty() {
            return Err(AiError::Config("chat model is required".into()));
        }
        Ok(Self {
            kind: cfg.provider,
            name: cfg.name.clone(),
            base_url: url,
            chat_model: cfg.chat_model.trim().to_owned(),
            embedding_model: cfg
                .embedding_model
                .as_ref()
                .map(|m| m.trim().to_owned())
                .filter(|m| !m.is_empty()),
            timeout: Duration::from_secs(u64::from(cfg.timeout_secs.clamp(1, 3600))),
            connect_timeout: Duration::from_secs(10),
            streaming: cfg.streaming,
            tool_support: cfg.tool_support,
            privacy_profile: cfg.privacy_profile,
        })
    }

    pub fn locality(&self) -> Locality {
        Locality::of_url(&self.base_url)
    }

    /// Stable id for the embedding model (used as the search index model id).
    pub fn embedding_model_id(&self) -> Option<String> {
        self.embedding_model
            .as_ref()
            .map(|m| format!("{}:{}", kind_label(self.kind), m))
    }
}

pub(crate) const fn kind_label(k: AiProviderKind) -> &'static str {
    match k {
        AiProviderKind::Ollama => "ollama",
        AiProviderKind::LmStudio => "lmstudio",
        AiProviderKind::Deepseek => "deepseek",
        AiProviderKind::OpenaiCompatible => "openai-compatible",
    }
}

/// An LLM backend. Implementations are cheap to clone behind `Arc` and safe
/// to share across tasks. All futures/streams are cancelled by dropping them.
#[async_trait]
pub trait LlmProvider: Send + Sync + fmt::Debug {
    fn settings(&self) -> &ProviderSettings;
    fn capabilities(&self) -> Capabilities;

    fn kind(&self) -> AiProviderKind {
        self.settings().kind
    }
    fn locality(&self) -> Locality {
        self.settings().locality()
    }
    /// Profile to sanitize with ([`effective_profile`] of the configured one).
    fn privacy_profile(&self) -> PrivacyProfile {
        effective_profile(self.settings().privacy_profile, self.locality())
    }
    /// Id of the embedding model for index versioning, if configured.
    fn embedding_model_id(&self) -> Option<String> {
        if self.capabilities().embeddings {
            self.settings().embedding_model_id()
        } else {
            None
        }
    }

    async fn list_models(&self) -> Result<Vec<ModelInfo>>;
    async fn chat(&self, req: &ChatRequest) -> Result<ChatResponse>;
    /// Streaming chat. Falls back to a single non-streaming call when the
    /// provider (or its configuration) does not stream.
    async fn chat_stream(&self, req: &ChatRequest) -> Result<ChatStream>;
    /// OpenAI Responses API.
    async fn respond(&self, _req: &ChatRequest) -> Result<ChatResponse> {
        Err(AiError::Unsupported("responses"))
    }
    async fn embed(&self, req: &EmbeddingRequest) -> Result<EmbeddingResponse>;
}

/// Construct a provider from a synced config. `api_key` is resolved by
/// app-core (from the referenced `Secret` object) and consumed here.
pub fn build_provider(
    cfg: &AiProviderConfig,
    api_key: Option<SecretString>,
) -> Result<Arc<dyn LlmProvider>> {
    let settings = ProviderSettings::from_config(cfg)?;
    Ok(match settings.kind {
        AiProviderKind::Ollama => Arc::new(OllamaProvider::new(settings, api_key)?),
        _ => Arc::new(OpenAiCompatibleProvider::new(settings, api_key)?),
    })
}

/// Turn a non-streaming call into a one-shot stream.
pub(crate) fn single_shot_stream(resp: ChatResponse) -> ChatStream {
    let mut events: Vec<Result<ChatEvent>> = Vec::new();
    if !resp.content.is_empty() {
        events.push(Ok(ChatEvent::Delta(resp.content)));
    }
    for t in resp.tool_calls {
        events.push(Ok(ChatEvent::ToolCall(t)));
    }
    events.push(Ok(ChatEvent::Done {
        finish_reason: resp.finish_reason,
        usage: resp.usage,
    }));
    Box::pin(futures::stream::iter(events))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn locality_classification() {
        let l = |u: &str| Locality::of_url(&Url::parse(u).unwrap());
        assert_eq!(l("http://localhost:11434"), Locality::Loopback);
        assert_eq!(l("http://127.0.0.1:1234/v1"), Locality::Loopback);
        assert_eq!(l("http://[::1]:8080"), Locality::Loopback);
        assert_eq!(l("http://192.168.1.20:11434"), Locality::PrivateNetwork);
        assert_eq!(l("http://100.101.1.2:11434"), Locality::PrivateNetwork);
        assert_eq!(l("http://gpu.lan:11434"), Locality::PrivateNetwork);
        assert_eq!(l("https://api.deepseek.com"), Locality::Public);
        assert_eq!(l("http://8.8.8.8"), Locality::Public);
    }

    #[test]
    fn local_profile_is_clamped_for_public_endpoints() {
        assert_eq!(
            effective_profile(PrivacyProfile::Local, Locality::Public),
            PrivacyProfile::Standard
        );
        assert_eq!(
            effective_profile(PrivacyProfile::Local, Locality::Loopback),
            PrivacyProfile::Local
        );
        assert_eq!(
            effective_profile(PrivacyProfile::Strict, Locality::Loopback),
            PrivacyProfile::Strict
        );
    }
}
