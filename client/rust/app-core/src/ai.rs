//! AI runtime wiring (CLIENT_SPEC §11–§16, ADR-0105, ADR-0107 §AI).
//!
//! * [`context::AppAiContext`] implements ai-core's `AiContextProvider` (the
//!   six allowed read methods) over the decrypted working set, the local
//!   search index and terminal-core's command tracker. `HostContext` is
//!   built without any credential data; Secret objects are unreachable.
//! * Providers are built with `cc_ai_core::build_provider(&config, api_key)`
//!   in [`AiSession::provider_for`] — the only place that reads the API-key
//!   Secret — and cached per config revision.
//! * The per-profile search index (`ProfilePaths::search_db`, SQLCipher with
//!   a random key in the SecureStore) is rebuilt from snippets / notes /
//!   hosts (/ history unless disabled) when the vault unlocks, kept current
//!   from object events, and dropped on lock. A background `EmbeddingSync`
//!   follows the default provider's embedding model.
//! * Nothing AI-generated runs by itself: `approve_run` passes the command
//!   through ai-core's `ExecutionGate` and returns a single-use token; only
//!   `exec_approved` / `terminal_run_approved` accept it.

mod context;
pub(crate) mod dto;
mod index;
mod session;
#[cfg(test)]
mod tests;

pub(crate) use index::index_key_name;
pub(crate) use session::AiSession;

use crate::app::AppCore;
use crate::dto::*;
use crate::error::{protocol_code_name_of, AppError, AppResult};
use crate::inventory::host_of;
use crate::session::Unlocked;
use crate::writer::VaultWriter;
use async_trait::async_trait;
use cc_ai_core::features::CommandProposal;
use cc_ai_core::policy::{ExecutionGate, HostRef, PolicyError, RunProposal};
use cc_ai_core::provider::{
    Capabilities, ChatRequest, ChatResponse, ChatStream, EmbeddingRequest, EmbeddingResponse,
    LlmProvider, Message, ModelInfo, ProviderSettings, StructuredOutput,
};
use cc_ai_core::sanitizer::SanitizedText;
use cc_ai_core::snippet::{RenderError, SnippetRenderError};
use cc_ai_core::{
    with_cancellation, AiAssistant, AiError, AskOptions, CancellationToken, ConvertRequest,
    CreateSnippetRequest, ExplainRequest, FixRequest, GenerateRequest, PrivacyProfile,
    SearchOptions,
};
use cc_models::ai::{AiProviderConfig, AiProviderKind, ChatMessage};
use cc_models::host::Host;
use cc_models::{ObjectId, VaultObject};
use cc_search_core::{SearchFilter, SearchIndex};
use cc_terminal_core::TerminalId;
use context::{clamp_text, AppAiContext, MAX_SELECTION_BYTES};
use futures::StreamExt;
use session::ConversationState;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::mpsc;

/// Upper bound of a question / description / command sent by the UI.
const MAX_REQUEST_BYTES: usize = 64 * 1024;
/// Messages kept in a persisted conversation.
const MAX_PERSISTED_MESSAGES: usize = 100;
/// Longest message stored in a persisted conversation.
const MAX_PERSISTED_MESSAGE_BYTES: usize = 16 * 1024;

// ---- errors ----------------------------------------------------------------------------

impl From<AiError> for AppError {
    fn from(e: AiError) -> Self {
        let msg = e.to_string();
        match e {
            AiError::Unsupported(_) => AppError::Unsupported(msg),
            AiError::Config(m) => AppError::invalid("ai_provider", m),
            AiError::Timeout | AiError::Connect(_) | AiError::Transport(_) => {
                AppError::AiUnavailable(msg)
            }
            AiError::Cancelled => AppError::Cancelled,
            AiError::Auth { .. } => AppError::AiAuthFailed(msg),
            AiError::RateLimited { retry_after_secs } => AppError::RateLimited { retry_after_secs },
            AiError::ModelNotFound(_)
            | AiError::Http { .. }
            | AiError::Provider(_)
            | AiError::InvalidResponse(_) => AppError::AiProvider(msg),
            AiError::InvalidInput(m) => AppError::invalid("request", m),
            AiError::Search(s) => AppError::Storage(format!("search index: {s}")),
            AiError::Internal(m) => AppError::Internal(m),
        }
    }
}

fn policy_error(e: PolicyError) -> AppError {
    match e {
        PolicyError::ConfirmationRequired(level) => AppError::ConfirmationRequired(format!(
            "this command is {} and must be confirmed by the user",
            protocol_code_name_of(&level)
        )),
        PolicyError::Unresolved(v) => AppError::UnresolvedPlaceholders(v),
        PolicyError::Empty => AppError::invalid("command", "must not be empty"),
    }
}

// ---- helpers ---------------------------------------------------------------------------

fn parse_terminal(s: &str) -> AppResult<TerminalId> {
    uuid::Uuid::parse_str(s.trim()).map_err(|_| AppError::invalid("terminal_id", "not a valid id"))
}

/// Validate free text sent to the AI (non-empty unless allowed, bounded,
/// no NUL).
fn request_text(field: &str, s: &str, allow_empty: bool) -> AppResult<String> {
    if !allow_empty && s.trim().is_empty() {
        return Err(AppError::invalid(field, "must not be empty"));
    }
    if s.len() > MAX_REQUEST_BYTES {
        return Err(AppError::invalid(field, "too long"));
    }
    if s.contains('\0') {
        return Err(AppError::invalid(field, "must not contain NUL"));
    }
    Ok(s.to_owned())
}

fn host_display(h: &Host) -> String {
    if h.name.is_empty() || h.name == h.address {
        h.address.clone()
    } else {
        format!("{} ({})", h.name, h.address)
    }
}

/// Secret placeholders (`<PASSWORD_1>`, `<SECRET_2>`, …) left in a text.
fn secret_placeholders(text: &str) -> Vec<String> {
    const LABELS: [&str; 4] = ["PRIVATE_KEY", "PEM", "PASSWORD", "SECRET"];
    let mut out: Vec<String> = Vec::new();
    let mut rest = text;
    while let Some(start) = rest.find('<') {
        let tail = &rest[start + 1..];
        if let Some(end) = tail.find('>') {
            let inner = &tail[..end];
            if let Some((label, n)) = inner.rsplit_once('_') {
                if LABELS.contains(&label) && !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit())
                {
                    let p = format!("<{inner}>");
                    if !out.contains(&p) {
                        out.push(p);
                    }
                }
            }
        }
        rest = tail;
    }
    out
}

fn sync_conversations_enabled(u: &Unlocked) -> bool {
    u.working()
        .all_vault_settings()
        .into_iter()
        .max_by_key(|s| s.updated_at)
        .is_some_and(|s| s.sync_ai_conversations)
}

/// A provider that is never contacted: local-only search and
/// `ai_convert_to_snippet(use_llm = false)` without a configured provider.
#[derive(Debug)]
struct LocalOnlyProvider {
    settings: ProviderSettings,
}

impl LocalOnlyProvider {
    fn shared() -> AppResult<Arc<dyn LlmProvider>> {
        let now = chrono::Utc::now();
        let cfg = AiProviderConfig {
            id: ObjectId::new(),
            name: "local only".into(),
            provider: AiProviderKind::Ollama,
            base_url: "http://localhost:11434".into(),
            api_key_secret_id: None,
            chat_model: "none".into(),
            embedding_model: None,
            timeout_secs: 1,
            streaming: false,
            tool_support: false,
            privacy_profile: PrivacyProfile::Strict,
            is_default: false,
            created_at: now,
            updated_at: now,
        };
        Ok(Arc::new(Self {
            settings: ProviderSettings::from_config(&cfg)?,
        }))
    }
}

const NOT_CONFIGURED: &str = "no AI provider configured";

#[async_trait]
impl LlmProvider for LocalOnlyProvider {
    fn settings(&self) -> &ProviderSettings {
        &self.settings
    }
    fn capabilities(&self) -> Capabilities {
        Capabilities {
            chat: false,
            responses: false,
            embeddings: false,
            streaming: false,
            tool_calling: false,
            structured_output: StructuredOutput::None,
        }
    }
    async fn list_models(&self) -> cc_ai_core::Result<Vec<ModelInfo>> {
        Err(AiError::Unsupported(NOT_CONFIGURED))
    }
    async fn chat(&self, _req: &ChatRequest) -> cc_ai_core::Result<ChatResponse> {
        Err(AiError::Unsupported(NOT_CONFIGURED))
    }
    async fn chat_stream(&self, _req: &ChatRequest) -> cc_ai_core::Result<ChatStream> {
        Err(AiError::Unsupported(NOT_CONFIGURED))
    }
    async fn embed(&self, _req: &EmbeddingRequest) -> cc_ai_core::Result<EmbeddingResponse> {
        Err(AiError::Unsupported(NOT_CONFIGURED))
    }
}

/// A resolved request context: the assistant plus the ids it is about.
struct Prepared {
    assistant: AiAssistant,
    provider_id: ObjectId,
    profile: PrivacyProfile,
    host_id: Option<ObjectId>,
}

impl AppCore {
    async fn ai_session(&self) -> AppResult<(Arc<Unlocked>, Arc<AiSession>)> {
        let (_, u) = self.unlocked().await?;
        let ai = u.ai.clone();
        Ok((u, ai))
    }

    /// Build the context provider for a request (host defaults to the
    /// terminal's host).
    fn ai_context(
        u: &Unlocked,
        ai: &AiSession,
        opts: &AiContextOptionsDto,
        index: Option<Arc<SearchIndex>>,
    ) -> AppResult<(AppAiContext, Option<ObjectId>)> {
        let terminal = match opts.terminal_id.as_deref().map(str::trim) {
            None | Some("") => None,
            Some(t) => Some(parse_terminal(t)?),
        };
        let backend = ai.backend();
        let host_id = match parse_opt_id("host_id", &opts.host_id)? {
            Some(h) => {
                host_of(u, h)?;
                Some(h)
            }
            None => match terminal {
                Some(t) => Some(backend.terminal_host(t)?),
                None => None,
            },
        };
        let selected_text = opts
            .selected_text
            .clone()
            .filter(|s| !s.trim().is_empty())
            .map(|s| clamp_text(s, MAX_SELECTION_BYTES));
        Ok((
            AppAiContext {
                working: u.working().clone(),
                index,
                backend,
                terminal,
                selected_text,
            },
            host_id,
        ))
    }

    /// Resolve provider + context for an LLM feature.
    async fn ai_prepare(
        &self,
        provider_id: Option<&str>,
        opts: &AiContextOptionsDto,
    ) -> AppResult<(Arc<AiSession>, Prepared)> {
        let (u, ai) = self.ai_session().await?;
        let cfg = ai.resolve_config(provider_id)?;
        let provider = ai.provider_for(&cfg).await?;
        let index = ai.fresh_index().await.ok();
        let (ctx, host_id) = Self::ai_context(&u, &ai, opts, index.clone())?;
        let profile = provider.privacy_profile();
        let mut assistant = AiAssistant::new(provider, Arc::new(ctx));
        if let Some(i) = index {
            assistant = assistant.with_index(i);
        }
        Ok((
            ai,
            Prepared {
                assistant,
                provider_id: cfg.id,
                profile,
                host_id,
            },
        ))
    }

    fn proposal_dto(
        ai: &AiSession,
        provider_id: ObjectId,
        profile: PrivacyProfile,
        p: &CommandProposal,
    ) -> CommandProposalDto {
        let id = ai.store_proposal(&p.run);
        CommandProposalDto {
            provider_id: provider_id.to_string(),
            target: p.target,
            command: p.suggestion.command.clone(),
            explanation: p.suggestion.explanation.clone(),
            alternatives: p.suggestion.alternatives.clone(),
            diagnosis: p.suggestion.diagnosis.clone(),
            run: RunProposalDto::from_run(id, &p.run),
            form: p.form.as_ref().map(VariableFormDto::from_form),
            unresolved_secrets: p.unresolved_secrets.clone(),
            redactions: SanitizerReportDto::from_report(&p.redactions),
            privacy_profile: profile,
            parse_quality: p.suggestion.parse_quality,
        }
    }

    // ---- index & providers -----------------------------------------------------------

    /// Status of the local search index (opens it if needed).
    pub async fn ai_index_status(&self) -> AppResult<AiIndexStatusDto> {
        let (_, ai) = self.ai_session().await?;
        let _ = ai.fresh_index().await;
        Ok(ai.status().await)
    }

    /// Rebuild the local search index from the vault (embeddings of
    /// unchanged documents are kept) and resume embedding.
    pub async fn ai_reindex(&self) -> AppResult<AiIndexStatusDto> {
        let (_, ai) = self.ai_session().await?;
        ai.rebuild().await?;
        ai.ensure_embedding().await;
        Ok(ai.status().await)
    }

    /// Check a provider: list its models (or send a one-token request when
    /// it cannot list them). Errors: `ai_auth_failed`, `ai_unavailable`,
    /// `ai_provider`, `invalid_input` (bad configuration).
    pub async fn test_ai_provider(&self, provider_id: String) -> AppResult<AiProviderTestDto> {
        let (_, ai) = self.ai_session().await?;
        let oid = parse_id("provider_id", &provider_id)?;
        let cfg = ai.resolve_config(Some(&oid.to_string()))?;
        let provider = ai.provider_for(&cfg).await?;
        let started = Instant::now();
        let (models, chat_model_available) = match provider.list_models().await {
            Ok(m) => {
                let ids: Vec<String> = m.into_iter().map(|m| m.id).collect();
                let prefix = format!("{}:", cfg.chat_model);
                let available = ids
                    .iter()
                    .any(|i| *i == cfg.chat_model || i.starts_with(&prefix));
                (ids, available)
            }
            Err(AiError::Unsupported(_) | AiError::ModelNotFound(_))
            | Err(AiError::Http { status: 404, .. }) => {
                let req = ChatRequest {
                    messages: vec![Message::user(SanitizedText::from_static("ping"))],
                    max_tokens: Some(1),
                    temperature: Some(0.0),
                    ..Default::default()
                };
                provider.chat(&req).await?;
                (Vec::new(), true)
            }
            Err(e) => return Err(e.into()),
        };
        let caps = provider.capabilities();
        Ok(AiProviderTestDto {
            provider_id: cfg.id.to_string(),
            models,
            chat_model_available,
            capabilities: AiCapabilitiesDto {
                chat: caps.chat,
                responses: caps.responses,
                embeddings: caps.embeddings,
                streaming: caps.streaming,
                tool_calling: caps.tool_calling,
            },
            locality: provider.locality(),
            effective_privacy_profile: provider.privacy_profile(),
            latency_ms: u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX),
        })
    }

    // ---- search ----------------------------------------------------------------------

    /// Search pipeline (CLIENT_SPEC §12.1): exact / FTS → semantic (when the
    /// provider has an embedding model) → RAG / generation (per
    /// `options.llm`; skipped without a provider).
    pub async fn ai_search(
        &self,
        query: String,
        options: AiSearchOptionsDto,
    ) -> AppResult<AiSearchResultDto> {
        let query = request_text("query", &query, false)?;
        let (u, ai) = self.ai_session().await?;
        let cfg = match options.provider_id.as_deref().map(str::trim) {
            Some(id) if !id.is_empty() => Some(ai.resolve_config(Some(id))?),
            _ => ai.default_config(),
        };
        let (provider, provider_id) = match &cfg {
            Some(c) => (ai.provider_for(c).await?, Some(c.id)),
            None => (LocalOnlyProvider::shared()?, None),
        };
        let profile = provider.privacy_profile();
        let index = ai.fresh_index().await?;
        let ctx_opts = AiContextOptionsDto {
            host_id: options.host_id.clone(),
            ..Default::default()
        };
        let (ctx, host_id) = Self::ai_context(&u, &ai, &ctx_opts, Some(index.clone()))?;
        let assistant = AiAssistant::new(provider, Arc::new(ctx)).with_index(index);
        let opts = SearchOptions {
            limit: options.limit.clamp(1, 100) as usize,
            filter: SearchFilter {
                kinds: options.kinds.clone(),
                tags: options.tags.clone(),
            },
            semantic: options.semantic && provider_id.is_some(),
            llm: if provider_id.is_some() {
                options.llm
            } else {
                LlmMode::Never
            },
            target: options.target,
            host_id,
            ..Default::default()
        };
        let outcome = assistant.search(&query, &opts).await?;
        Ok(AiSearchResultDto {
            hits: outcome
                .hits
                .iter()
                .map(|h| AiSearchHitDto {
                    id: h.document.id.to_string(),
                    kind: h.document.kind,
                    title: h.document.title.clone(),
                    body: h.document.body.clone(),
                    tags: h.document.tags.clone(),
                    score: h.score,
                    origin: h.origin,
                    exact: h.exact,
                })
                .collect(),
            answer: match (outcome.answer.as_ref(), provider_id) {
                (Some(a), Some(pid)) => Some(AiSearchAnswerDto {
                    origin: a.origin,
                    proposal: Self::proposal_dto(&ai, pid, profile, &a.proposal),
                    source_ids: a.sources.iter().map(ToString::to_string).collect(),
                }),
                _ => None,
            },
            stages: outcome
                .stages
                .iter()
                .map(|s| AiStageDto {
                    stage: s.stage,
                    ran: s.ran,
                    hits: u32::try_from(s.hits).unwrap_or(u32::MAX),
                    note: s.note.clone(),
                })
                .collect(),
        })
    }

    // ---- Ask AI ----------------------------------------------------------------------

    /// Ask AI, streaming the answer. `request.conversation_id = None` starts
    /// a conversation (bound to the provider; its placeholders stay stable
    /// across turns). When vault settings enable `sync_ai_conversations`,
    /// every completed turn is stored as an `AiConversation` object.
    pub async fn ai_ask(
        &self,
        provider_id: Option<String>,
        request: AiAskRequestDto,
        context: AiContextOptionsDto,
    ) -> AppResult<AiAskStream> {
        let question = request_text("question", &request.question, false)?;
        let (u, ai) = self.ai_session().await?;
        let conv_id = request
            .conversation_id
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty());
        let existing = conv_id.and_then(|id| ai.memory_conversation(id));
        let (conv_provider, record) = match (&existing, conv_id) {
            (Some(h), _) => (Some(h.provider_id), None),
            (None, Some(id)) => {
                let r = ai.stored_conversation(id)?;
                (r.provider_id, Some(r))
            }
            (None, None) => (None, None),
        };
        let wanted = provider_id
            .filter(|p| !p.trim().is_empty())
            .or_else(|| conv_provider.map(|p| p.to_string()));
        let cfg = ai.resolve_config(wanted.as_deref())?;
        if conv_provider.is_some_and(|p| p != cfg.id) {
            return Err(AppError::invalid(
                "provider_id",
                "the conversation belongs to another provider",
            ));
        }
        let provider = ai.provider_for(&cfg).await?;
        let (conversation_id, handle) = match existing {
            Some(h) => (conv_id.unwrap_or_default().to_owned(), h),
            None => ai.open_conversation(record, cfg.id, provider.privacy_profile()),
        };
        let index = ai.fresh_index().await.ok();
        let (ctx, host_id) = Self::ai_context(&u, &ai, &context, index.clone())?;
        let mut assistant = AiAssistant::new(provider, Arc::new(ctx));
        if let Some(i) = index {
            assistant = assistant.with_index(i);
        }
        let opts = AskOptions {
            host_id,
            include_terminal: context.include_terminal,
        };
        let (request_id, token) = ai.register_request();
        let (tx, rx) = mpsc::channel(64);
        let job = AskJob {
            assistant,
            question,
            opts,
            token,
            tx,
            persist: sync_conversations_enabled(&u),
            writer: u.writer.clone(),
        };
        let rid = request_id.clone();
        let ai2 = ai.clone();
        tokio::spawn(async move {
            job.run(handle.state).await;
            ai2.finish_request(&rid);
        });
        Ok(AiAskStream {
            request_id,
            conversation_id,
            chunks: rx,
        })
    }

    /// Cancel a running AI request (`ai_ask`). `false` if it already ended.
    pub async fn ai_cancel(&self, request_id: String) -> AppResult<bool> {
        let (_, ai) = self.ai_session().await?;
        Ok(ai.cancel_request(&request_id))
    }

    /// Conversations of this unlock plus persisted ones (newest first).
    pub async fn ai_list_conversations(&self) -> AppResult<Vec<AiConversationDto>> {
        let (_, ai) = self.ai_session().await?;
        let mut out: HashMap<String, AiConversationDto> = ai
            .stored_conversations()
            .iter()
            .map(|c| (c.id.to_string(), AiConversationDto::from_model(c, true)))
            .collect();
        for h in ai.memory_conversations() {
            // A conversation that is answering right now keeps its last
            // stored state.
            if let Ok(st) = h.state.try_lock() {
                if !st.record.messages.is_empty() {
                    out.insert(
                        st.record.id.to_string(),
                        AiConversationDto::from_model(&st.record, st.persisted),
                    );
                }
            }
        }
        let mut v: Vec<AiConversationDto> = out.into_values().collect();
        v.sort_by_key(|c| std::cmp::Reverse(c.updated_at_ms));
        Ok(v)
    }

    /// Forget a conversation (and delete its vault object if persisted).
    pub async fn ai_delete_conversation(&self, conversation_id: String) -> AppResult<()> {
        let (u, ai) = self.ai_session().await?;
        let forgotten = ai.forget_conversation(&conversation_id);
        let oid = parse_id("conversation_id", &conversation_id)?;
        let stored = ai.stored_conversations().iter().any(|c| c.id == oid);
        if stored {
            u.writer.delete(oid).await?;
        } else if !forgotten {
            return Err(AppError::not_found("conversation", oid));
        }
        Ok(())
    }

    // ---- commands --------------------------------------------------------------------

    /// Generate Command. `target` = the dialect (`None` → derived from the
    /// host's shell); the host is `context.host_id` (or the terminal's).
    pub async fn ai_generate_command(
        &self,
        provider_id: Option<String>,
        request: String,
        target: Option<CommandTarget>,
        context: AiContextOptionsDto,
    ) -> AppResult<CommandProposalDto> {
        let description = request_text("request", &request, false)?;
        let (ai, p) = self.ai_prepare(provider_id.as_deref(), &context).await?;
        let proposal = p
            .assistant
            .generate_command(&GenerateRequest {
                description,
                target,
                host_id: p.host_id,
                include_terminal: context.include_terminal,
            })
            .await?;
        Ok(Self::proposal_dto(&ai, p.provider_id, p.profile, &proposal))
    }

    /// Explain Command (`command` empty → the selection, else the
    /// terminal's last command).
    pub async fn ai_explain_command(
        &self,
        provider_id: Option<String>,
        command: String,
        target: Option<CommandTarget>,
        context: AiContextOptionsDto,
    ) -> AppResult<ExplainResultDto> {
        let command = request_text("command", &command, true)?;
        let (_, p) = self.ai_prepare(provider_id.as_deref(), &context).await?;
        let r = p
            .assistant
            .explain_command(&ExplainRequest {
                command,
                target,
                host_id: p.host_id,
            })
            .await?;
        Ok(ExplainResultDto::from_result(&r, p.profile))
    }

    /// Fix Last Error of a terminal (its last command + error output).
    pub async fn ai_fix_last_error(
        &self,
        provider_id: Option<String>,
        terminal_id: String,
        target: Option<CommandTarget>,
    ) -> AppResult<CommandProposalDto> {
        parse_terminal(&terminal_id)?;
        let context = AiContextOptionsDto {
            terminal_id: Some(terminal_id),
            include_terminal: true,
            ..Default::default()
        };
        let (ai, p) = self.ai_prepare(provider_id.as_deref(), &context).await?;
        let proposal = p
            .assistant
            .fix_last_error(&FixRequest {
                target,
                host_id: p.host_id,
            })
            .await?;
        Ok(Self::proposal_dto(&ai, p.provider_id, p.profile, &proposal))
    }

    // ---- snippets --------------------------------------------------------------------

    /// Create Snippet from a description → draft (save it with
    /// `save_snippet`).
    pub async fn ai_create_snippet(
        &self,
        provider_id: Option<String>,
        description: String,
        target: Option<CommandTarget>,
        context: AiContextOptionsDto,
    ) -> AppResult<SnippetDraftDto> {
        let description = request_text("description", &description, false)?;
        let (_, p) = self.ai_prepare(provider_id.as_deref(), &context).await?;
        let d = p
            .assistant
            .create_snippet(&CreateSnippetRequest {
                description,
                target,
                host_id: p.host_id,
            })
            .await?;
        Ok(SnippetDraftDto::from_draft(&d))
    }

    /// Convert a command into a parameterized snippet draft. Local rules
    /// always run (secrets are removed); with `use_llm` the provider refines
    /// names and descriptions (sanitized).
    pub async fn ai_convert_to_snippet(
        &self,
        provider_id: Option<String>,
        command: String,
        target: Option<CommandTarget>,
        use_llm: bool,
    ) -> AppResult<SnippetDraftDto> {
        let command = request_text("command", &command, false)?;
        let req = ConvertRequest {
            command,
            target,
            use_llm,
        };
        let d = if use_llm {
            let (_, p) = self
                .ai_prepare(provider_id.as_deref(), &AiContextOptionsDto::default())
                .await?;
            p.assistant.convert_to_snippet(&req).await?
        } else {
            let (u, ai) = self.ai_session().await?;
            let (ctx, _) = Self::ai_context(&u, &ai, &AiContextOptionsDto::default(), None)?;
            AiAssistant::new(LocalOnlyProvider::shared()?, Arc::new(ctx))
                .convert_to_snippet(&req)
                .await?
        };
        Ok(SnippetDraftDto::from_draft(&d))
    }

    /// The value form of a snippet (shown before running it).
    pub async fn snippet_form(&self, snippet_id: String) -> AppResult<VariableFormDto> {
        let (u, _) = self.ai_session().await?;
        let id = parse_id("snippet_id", &snippet_id)?;
        let s = u
            .working()
            .snippet(id)
            .ok_or_else(|| AppError::not_found("snippet", id))?;
        Ok(VariableFormDto::from_form(
            &cc_ai_core::VariableForm::for_snippet(&s),
        ))
    }

    /// Render a snippet with form values (quoted for the snippet's shell).
    /// Returns per-field errors, or the command with a run proposal whose
    /// risk is the local rules raised by the snippet's stored risk level.
    pub async fn snippet_render(
        &self,
        snippet_id: String,
        values: HashMap<String, String>,
    ) -> AppResult<SnippetRenderDto> {
        let (u, _) = self.ai_session().await?;
        let id = parse_id("snippet_id", &snippet_id)?;
        let s = u
            .working()
            .snippet(id)
            .ok_or_else(|| AppError::not_found("snippet", id))?;
        Ok(render_snippet_model(&s, &values))
    }

    // ---- risk & execution gate -------------------------------------------------------

    /// Local risk rules for a command (the model is not consulted).
    pub fn assess_risk(&self, command: String, dialect: CommandDialect) -> RiskAssessmentDto {
        let run = RunProposal::new(command, dialect, None, ProposalOrigin::User, None);
        RiskAssessmentDto {
            level: run.risk,
            reasons: run.reasons.iter().map(RiskReasonDto::from_reason).collect(),
            requires_confirmation: run.requires_confirmation,
        }
    }

    /// The user pressed "Run": pass the (filled-in) proposal through the
    /// execution gate. Modifying / destructive / unknown / multi-line
    /// commands need `user_confirmed` (`confirmation_required` otherwise);
    /// commands with placeholders are refused (`unresolved_placeholders`).
    /// The gate re-classifies the command text, and the AI risk hint of a
    /// stored proposal is re-applied, so edits cannot lower the risk.
    /// Returns a single-use token for `exec_approved` /
    /// `terminal_run_approved`.
    pub async fn approve_run(
        &self,
        proposal: RunProposalDto,
        user_confirmed: bool,
    ) -> AppResult<ApprovedCommandDto> {
        let (u, ai) = self.ai_session().await?;
        let command = request_text("command", &proposal.command, false)?;
        let host_id = parse_opt_id("host_id", &proposal.host_id)?
            .ok_or_else(|| AppError::invalid("host_id", "choose the host to run the command on"))?;
        let host = host_of(&u, host_id)?;
        let (origin, dialect, ai_hint) = match ai.proposal(&proposal.proposal_id) {
            Some(stored) => (
                stored.origin,
                stored.dialect,
                match (stored.ai_risk_suggestion, proposal.ai_risk_suggestion) {
                    (Some(a), Some(b)) => Some(cc_ai_core::risk::max_risk(a, b)),
                    (a, b) => a.or(b),
                },
            ),
            None => (
                proposal.origin,
                proposal.dialect,
                proposal.ai_risk_suggestion,
            ),
        };
        let display = host_display(&host);
        let run = RunProposal::new(
            command,
            dialect,
            Some(HostRef {
                id: Some(host_id),
                display: display.clone(),
            }),
            origin,
            ai_hint,
        );
        let approved = ExecutionGate
            .approve(&run, user_confirmed)
            .map_err(policy_error)?;
        let (command, risk) = (approved.command().to_owned(), approved.risk());
        let token = ai.store_approval(approved, host_id);
        let ttl = chrono::Duration::from_std(session::APPROVAL_TTL).unwrap_or_default();
        tracing::info!(risk = ?risk, origin = ?origin, "command approved to run");
        Ok(ApprovedCommandDto {
            token,
            command,
            host_id: host_id.to_string(),
            host_display: display,
            risk,
            expires_at_ms: ms(&(chrono::Utc::now() + ttl)),
        })
    }

    /// Run an approved command on its host (non-interactive exec). The token
    /// is consumed.
    pub async fn exec_approved(&self, token: String) -> AppResult<ExecResultDto> {
        let (u, ai) = self.ai_session().await?;
        let a = ai.take_approval(&token)?;
        host_of(&u, a.host_id)?;
        ai.backend().exec(a.host_id, a.approved.command()).await
    }

    /// Type an approved command into a terminal of the approved host and
    /// press Enter. The token is consumed.
    pub async fn terminal_run_approved(&self, terminal_id: String, token: String) -> AppResult<()> {
        let (_, ai) = self.ai_session().await?;
        let tid = parse_terminal(&terminal_id)?;
        let a = ai.take_approval(&token)?;
        let backend = ai.backend();
        if backend.terminal_host(tid)? != a.host_id {
            return Err(AppError::ApprovalRequired(
                "the command was approved for another host".into(),
            ));
        }
        let mut line = a.approved.command().as_bytes().to_vec();
        line.push(b'\r');
        backend.terminal_write(tid, &line).await
    }
}

// ---- Ask AI worker -------------------------------------------------------------------

/// One streamed Ask AI turn.
struct AskJob {
    assistant: AiAssistant,
    question: String,
    opts: AskOptions,
    token: CancellationToken,
    tx: mpsc::Sender<AiAskChunk>,
    persist: bool,
    writer: VaultWriter,
}

impl AskJob {
    async fn fail(&self, e: AppError) {
        let _ = self
            .tx
            .send(AiAskChunk::Error {
                code: e.code().to_owned(),
                message: e.message(),
            })
            .await;
    }

    async fn run(self, state: Arc<tokio::sync::Mutex<ConversationState>>) {
        let mut guard = tokio::select! {
            () = self.token.cancelled() => return self.fail(AppError::Cancelled).await,
            g = state.lock_owned() => g,
        };
        let st = &mut *guard;
        let started = with_cancellation(
            &self.token,
            self.assistant
                .ask_stream(&mut st.conv, &self.question, &self.opts),
        )
        .await;
        let mut stream = match started {
            Ok(s) => s,
            Err(e) => return self.fail(e.into()).await,
        };
        let mut answer = String::new();
        loop {
            let item = tokio::select! {
                () = self.token.cancelled() => return self.fail(AppError::Cancelled).await,
                item = stream.next() => item,
            };
            match item {
                Some(Ok(text)) => {
                    answer.push_str(&text);
                    if self.tx.send(AiAskChunk::Delta { text }).await.is_err() {
                        return; // receiver dropped: abort (drops the HTTP stream)
                    }
                }
                Some(Err(e)) => return self.fail(e.into()).await,
                None => break,
            }
        }
        stream.finish(&self.assistant, &mut st.conv);
        let now = chrono::Utc::now();
        // The record may be persisted (and synced): secrets the user typed
        // into the question are replaced by placeholders (`Local` profile —
        // host metadata stays). Answers never contain re-hydrated secrets.
        let question =
            cc_ai_core::sanitizer::sanitize(PrivacyProfile::Local, &self.question).into_string();
        if st.record.title.is_empty() {
            st.record.title = question
                .lines()
                .next()
                .unwrap_or_default()
                .chars()
                .take(80)
                .collect();
        }
        for (role, content) in [(ChatRole::User, &question), (ChatRole::Assistant, &answer)] {
            st.record.messages.push(ChatMessage {
                role,
                content: clamp_text(content.clone(), MAX_PERSISTED_MESSAGE_BYTES),
                created_at: now,
            });
        }
        let excess = st
            .record
            .messages
            .len()
            .saturating_sub(MAX_PERSISTED_MESSAGES);
        st.record.messages.drain(..excess);
        st.record.updated_at = now;
        let mut persisted = false;
        if self.persist {
            match self
                .writer
                .put(VaultObject::AiConversation(st.record.clone()))
                .await
            {
                Ok(()) => {
                    st.persisted = true;
                    persisted = true;
                }
                Err(e) => tracing::warn!(error = %e, "AI conversation not persisted"),
            }
        }
        let summary = AiAskSummaryDto {
            unresolved_secrets: secret_placeholders(&answer),
            redactions: SanitizerReportDto::from_report(st.conv.redactions()),
            persisted,
        };
        let _ = self.tx.send(AiAskChunk::Done(summary)).await;
    }
}

/// Render a snippet model with form values (see [`AppCore::snippet_render`]).
pub(crate) fn render_snippet_model(
    s: &cc_models::snippet::Snippet,
    values: &HashMap<String, String>,
) -> SnippetRenderDto {
    match cc_ai_core::render_snippet(s, values) {
        Ok(command) => {
            let run = RunProposal::new(
                command.clone(),
                CommandDialect::for_snippet(s.snippet_type, s.shell.as_deref()),
                None,
                ProposalOrigin::Snippet,
                Some(s.risk_level),
            );
            SnippetRenderDto {
                command: Some(command),
                field_errors: Vec::new(),
                run: Some(RunProposalDto::from_run(String::new(), &run)),
            }
        }
        Err(SnippetRenderError::Form(errors)) => SnippetRenderDto {
            command: None,
            field_errors: errors.iter().map(FieldErrorDto::from_error).collect(),
            run: None,
        },
        Err(SnippetRenderError::Render(e)) => {
            let name = match &e {
                RenderError::MissingValue { name }
                | RenderError::ControlCharacter { name }
                | RenderError::UnsafeValue { name, .. } => name.clone(),
            };
            SnippetRenderDto {
                command: None,
                field_errors: vec![FieldErrorDto {
                    name,
                    message: e.to_string(),
                }],
                run: None,
            }
        }
    }
}
