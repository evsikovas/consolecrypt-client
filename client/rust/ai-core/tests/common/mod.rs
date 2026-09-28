//! Shared test helpers: a scripted in-process provider that records every
//! request it receives.
#![allow(dead_code)]

use async_trait::async_trait;
use cc_ai_core::provider::{
    Capabilities, ChatEvent, ChatRequest, ChatResponse, ChatStream, EmbeddingRequest,
    EmbeddingResponse, LlmProvider, ModelInfo, ProviderSettings, StructuredOutput,
};
use cc_ai_core::sanitizer::PrivacyProfile;
use cc_ai_core::AiError;
use cc_models::ai::{AiProviderConfig, AiProviderKind};
use cc_models::ObjectId;
use std::collections::VecDeque;
use std::sync::Mutex;

pub fn config(kind: AiProviderKind, base: &str, profile: PrivacyProfile) -> AiProviderConfig {
    let now = chrono::Utc::now();
    AiProviderConfig {
        id: ObjectId::new(),
        name: "mock".into(),
        provider: kind,
        base_url: base.into(),
        api_key_secret_id: None,
        chat_model: "mock-model".into(),
        embedding_model: Some("toy-embed".into()),
        timeout_secs: 5,
        streaming: true,
        tool_support: false,
        privacy_profile: profile,
        is_default: false,
        created_at: now,
        updated_at: now,
    }
}

/// Deterministic toy embedding over a few concept groups.
pub fn toy_embed(text: &str) -> Vec<f32> {
    let t = text.to_lowercase();
    let groups: [&[&str]; 5] = [
        &[
            "kubectl",
            "pod",
            "k8s",
            "kubernetes",
            "deployment",
            "container",
        ],
        &["log", "journal", "tail", "error"],
        &["disk", "space", "storage", "du ", "df "],
        &["postgres", "database", "sql", "vacuum", "psql"],
        &["restart", "service", "systemctl", "nginx"],
    ];
    groups
        .iter()
        .map(|g| g.iter().filter(|w| t.contains(*w)).count() as f32 + 0.01)
        .collect()
}

#[derive(Debug)]
pub struct MockProvider {
    settings: ProviderSettings,
    responses: Mutex<VecDeque<String>>,
    /// Concatenated message contents of every chat request.
    pub chats: Mutex<Vec<String>>,
    /// Every embedding input.
    pub embeds: Mutex<Vec<String>>,
    pub fail_format: bool,
}

impl MockProvider {
    pub fn new(kind: AiProviderKind, base: &str, profile: PrivacyProfile) -> Self {
        Self {
            settings: ProviderSettings::from_config(&config(kind, base, profile)).unwrap(),
            responses: Mutex::new(VecDeque::new()),
            chats: Mutex::new(Vec::new()),
            embeds: Mutex::new(Vec::new()),
            fail_format: false,
        }
    }

    /// Local endpoint with a specific embedding model.
    pub fn with_embedding_model(model: &str) -> Self {
        let mut cfg = config(
            AiProviderKind::Ollama,
            "http://localhost:11434",
            PrivacyProfile::Local,
        );
        cfg.embedding_model = Some(model.into());
        Self {
            settings: ProviderSettings::from_config(&cfg).unwrap(),
            responses: Mutex::new(VecDeque::new()),
            chats: Mutex::new(Vec::new()),
            embeds: Mutex::new(Vec::new()),
            fail_format: false,
        }
    }

    /// Remote (public) endpoint.
    pub fn remote(profile: PrivacyProfile) -> Self {
        Self::new(
            AiProviderKind::OpenaiCompatible,
            "https://llm.example.com/v1",
            profile,
        )
    }

    /// Local endpoint.
    pub fn local(profile: PrivacyProfile) -> Self {
        Self::new(AiProviderKind::Ollama, "http://localhost:11434", profile)
    }

    pub fn push(&self, response: impl Into<String>) {
        self.responses.lock().unwrap().push_back(response.into());
    }

    pub fn all_sent(&self) -> String {
        let mut s = self.chats.lock().unwrap().join("\n---\n");
        s.push_str(&self.embeds.lock().unwrap().join("\n"));
        s
    }

    fn record(&self, req: &ChatRequest) -> String {
        let text: Vec<&str> = req.messages.iter().map(|m| m.content().as_str()).collect();
        self.chats.lock().unwrap().push(text.join("\n"));
        self.responses
            .lock()
            .unwrap()
            .pop_front()
            .unwrap_or_else(|| "{}".into())
    }
}

#[async_trait]
impl LlmProvider for MockProvider {
    fn settings(&self) -> &ProviderSettings {
        &self.settings
    }

    fn capabilities(&self) -> Capabilities {
        Capabilities {
            chat: true,
            responses: false,
            embeddings: true,
            streaming: true,
            tool_calling: false,
            structured_output: StructuredOutput::JsonSchema,
        }
    }

    async fn list_models(&self) -> cc_ai_core::Result<Vec<ModelInfo>> {
        Ok(Vec::new())
    }

    async fn chat(&self, req: &ChatRequest) -> cc_ai_core::Result<ChatResponse> {
        if self.fail_format && req.response_format != cc_ai_core::provider::ResponseFormat::Text {
            self.chats.lock().unwrap().push(String::new());
            return Err(AiError::Http {
                status: 400,
                message: "response_format not supported".into(),
            });
        }
        let content = self.record(req);
        Ok(ChatResponse {
            content,
            ..Default::default()
        })
    }

    async fn chat_stream(&self, req: &ChatRequest) -> cc_ai_core::Result<ChatStream> {
        let content = self.record(req);
        let chars: Vec<char> = content.chars().collect();
        let mut events: Vec<cc_ai_core::Result<ChatEvent>> = chars
            .chunks(3)
            .map(|c| Ok(ChatEvent::Delta(c.iter().collect())))
            .collect();
        events.push(Ok(ChatEvent::Done {
            finish_reason: Some("stop".into()),
            usage: None,
        }));
        Ok(Box::pin(futures::stream::iter(events)))
    }

    async fn embed(&self, req: &EmbeddingRequest) -> cc_ai_core::Result<EmbeddingResponse> {
        let mut seen = self.embeds.lock().unwrap();
        let vectors = req
            .inputs
            .iter()
            .map(|t| {
                seen.push(t.as_str().to_owned());
                toy_embed(t.as_str())
            })
            .collect();
        Ok(EmbeddingResponse {
            vectors,
            model: "toy-embed".into(),
        })
    }
}
