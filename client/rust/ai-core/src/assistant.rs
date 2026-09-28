//! [`AiAssistant`]: the feature orchestrator app-core calls.
//!
//! Every feature follows the same path:
//! allowed context ([`AiContextProvider`]) → [`SanitizerSession`] (profile of
//! the provider, `Local` clamped for public endpoints) → prompt →
//! [`LlmProvider`] → robust parsing → local re-hydration of non-secret
//! placeholders → local risk rules → [`RunProposal`]. Nothing is executed.

use crate::context::{AiContextProvider, HostContext};
use crate::error::{AiError, Result};
use crate::features::output::{
    parse_command_output, parse_explanation, parse_snippet_output, RawSnippet,
};
use crate::features::prompts::{self, RawContext};
use crate::features::{
    CommandProposal, CommandSuggestion, CommandTarget, ExplainResult, Explanation, ParseQuality,
    SnippetDraft, SuggestedVariable,
};
use crate::parameterize::{parameterize, scrub_secrets};
use crate::pipeline::{self, SearchOptions, SearchOutcome};
use crate::policy::{HostRef, ProposalOrigin, RunProposal};
use crate::provider::{ChatEvent, ChatRequest, ChatStream, LlmProvider, Message, ResponseFormat};
use crate::risk::{classify, combine_with_ai, RiskLevel};
use crate::sanitizer::{
    rules_classify_secret_key, secret_spans, Category, PrivacyProfile, PromptBuilder, Rehydrator,
    SanitizeReport, SanitizerSession,
};
use crate::snippet::VariableForm;
use cc_models::snippet::{template_variables, SnippetSource, SnippetVariable};
use cc_models::ObjectId;
use cc_search_core::{normalize_tags, SearchIndex};
use futures::Stream;
use serde::{Deserialize, Serialize};
use std::fmt;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context as TaskContext, Poll};

/// Tunables.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AssistantOptions {
    pub temperature: f32,
    pub max_tokens: Option<u32>,
    /// Saved snippets added to generation prompts.
    pub snippet_context: usize,
    /// Messages kept in an Ask AI conversation (older ones are dropped).
    pub max_history: usize,
}

impl Default for AssistantOptions {
    fn default() -> Self {
        Self {
            temperature: 0.2,
            max_tokens: None,
            snippet_context: 5,
            max_history: 20,
        }
    }
}

/// Ask AI options.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AskOptions {
    pub host_id: Option<ObjectId>,
    /// Include selected terminal text / last error.
    pub include_terminal: bool,
}

/// Generate Command request.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct GenerateRequest {
    pub description: String,
    /// `None` → derived from the host's shell (default POSIX shell).
    pub target: Option<CommandTarget>,
    pub host_id: Option<ObjectId>,
    /// Include last command / error / selection from the terminal.
    pub include_terminal: bool,
}

/// Explain Command request.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExplainRequest {
    /// Empty → the selected terminal text, else the last command.
    pub command: String,
    pub target: Option<CommandTarget>,
    pub host_id: Option<ObjectId>,
}

/// Fix Last Error request.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct FixRequest {
    pub target: Option<CommandTarget>,
    pub host_id: Option<ObjectId>,
}

/// Create Snippet request.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CreateSnippetRequest {
    pub description: String,
    pub target: Option<CommandTarget>,
    pub host_id: Option<ObjectId>,
}

/// Convert command → parameterized snippet request.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConvertRequest {
    /// Empty → the last terminal command.
    pub command: String,
    pub target: Option<CommandTarget>,
    /// Let the model improve names/descriptions (sanitized); otherwise purely
    /// local rules.
    pub use_llm: bool,
}

/// An Ask AI conversation: history plus the sanitizer session that keeps
/// placeholders stable across turns. Lives in memory only.
pub struct Conversation {
    session: SanitizerSession,
    history: Vec<Message>,
}

impl fmt::Debug for Conversation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Conversation")
            .field("messages", &self.history.len())
            .finish_non_exhaustive()
    }
}

impl Conversation {
    pub fn new(profile: PrivacyProfile) -> Self {
        Self {
            session: SanitizerSession::new(profile),
            history: Vec::new(),
        }
    }

    pub fn len(&self) -> usize {
        self.history.len()
    }

    pub fn is_empty(&self) -> bool {
        self.history.is_empty()
    }

    pub fn clear(&mut self) {
        self.history.clear();
    }

    /// Cumulative redaction counts.
    pub fn redactions(&self) -> &SanitizeReport {
        self.session.report()
    }

    fn record_answer(&mut self, raw: &str, max: usize) {
        let s = self.session.sanitize(raw);
        self.history.push(Message::assistant(s));
        let excess = self.history.len().saturating_sub(max);
        if excess > 0 {
            self.history.drain(..excess);
        }
    }
}

/// Ask AI answer.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AskAnswer {
    /// Re-hydrated Markdown answer.
    pub text: String,
    pub unresolved_secrets: Vec<String>,
    pub redactions: SanitizeReport,
}

/// Streaming Ask AI answer: yields re-hydrated text chunks. Call
/// [`AskStream::finish`] afterwards to record the answer in the
/// conversation.
pub struct AskStream {
    inner: ChatStream,
    rehydrator: Rehydrator,
    raw: String,
    done: bool,
}

impl fmt::Debug for AskStream {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AskStream")
            .field("received", &self.raw.len())
            .field("done", &self.done)
            .finish()
    }
}

impl AskStream {
    /// Record the (placeholder) answer into the conversation history.
    pub fn finish(self, assistant: &AiAssistant, conv: &mut Conversation) {
        if !self.raw.is_empty() {
            conv.record_answer(&self.raw, assistant.options.max_history);
        }
    }
}

impl Stream for AskStream {
    type Item = Result<String>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut TaskContext<'_>) -> Poll<Option<Self::Item>> {
        loop {
            if self.done {
                return Poll::Ready(None);
            }
            match self.inner.as_mut().poll_next(cx) {
                Poll::Pending => return Poll::Pending,
                Poll::Ready(None) | Poll::Ready(Some(Ok(ChatEvent::Done { .. }))) => {
                    self.done = true;
                    let rest = self.rehydrator.finish();
                    if !rest.is_empty() {
                        return Poll::Ready(Some(Ok(rest)));
                    }
                }
                Poll::Ready(Some(Ok(ChatEvent::Delta(d)))) => {
                    self.raw.push_str(&d);
                    let out = self.rehydrator.push(&d);
                    if !out.is_empty() {
                        return Poll::Ready(Some(Ok(out)));
                    }
                }
                Poll::Ready(Some(Ok(ChatEvent::ToolCall(_)))) => {}
                Poll::Ready(Some(Err(e))) => {
                    self.done = true;
                    return Poll::Ready(Some(Err(e)));
                }
            }
        }
    }
}

/// The AI feature facade.
#[derive(Clone)]
pub struct AiAssistant {
    provider: Arc<dyn LlmProvider>,
    context: Arc<dyn AiContextProvider>,
    index: Option<Arc<SearchIndex>>,
    options: AssistantOptions,
}

impl fmt::Debug for AiAssistant {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AiAssistant")
            .field("provider", &self.provider.kind())
            .field("index", &self.index.is_some())
            .finish_non_exhaustive()
    }
}

impl AiAssistant {
    pub fn new(provider: Arc<dyn LlmProvider>, context: Arc<dyn AiContextProvider>) -> Self {
        Self {
            provider,
            context,
            index: None,
            options: AssistantOptions::default(),
        }
    }

    /// Use the local search index for the search pipeline.
    pub fn with_index(mut self, index: Arc<SearchIndex>) -> Self {
        self.index = Some(index);
        self
    }

    pub fn with_options(mut self, options: AssistantOptions) -> Self {
        self.options = options;
        self
    }

    pub fn provider(&self) -> &Arc<dyn LlmProvider> {
        &self.provider
    }

    pub(crate) fn context(&self) -> &Arc<dyn AiContextProvider> {
        &self.context
    }

    pub(crate) fn index(&self) -> Option<&Arc<SearchIndex>> {
        self.index.as_ref()
    }

    pub fn options(&self) -> &AssistantOptions {
        &self.options
    }

    /// Profile applied to everything sent to this provider.
    pub fn privacy_profile(&self) -> PrivacyProfile {
        self.provider.privacy_profile()
    }

    pub fn new_session(&self) -> SanitizerSession {
        SanitizerSession::new(self.privacy_profile())
    }

    pub fn new_conversation(&self) -> Conversation {
        Conversation::new(self.privacy_profile())
    }

    async fn host(&self, host_id: Option<ObjectId>) -> Option<HostContext> {
        match host_id {
            Some(id) => self.context.get_host_context(id).await,
            None => None,
        }
    }

    fn resolve_target(
        explicit: Option<CommandTarget>,
        host: Option<&HostContext>,
    ) -> CommandTarget {
        explicit
            .unwrap_or_else(|| CommandTarget::for_shell_name(host.and_then(|h| h.shell.as_deref())))
    }

    fn request(&self, messages: Vec<Message>, format: ResponseFormat) -> ChatRequest {
        ChatRequest {
            messages,
            temperature: Some(self.options.temperature),
            max_tokens: self.options.max_tokens,
            response_format: format,
            ..Default::default()
        }
    }

    /// Chat with a structured format; retry once as plain text if the server
    /// rejects the format.
    pub(crate) async fn complete(&self, mut req: ChatRequest) -> Result<String> {
        match self.provider.chat(&req).await {
            Err(AiError::Http {
                status: 400 | 422, ..
            }) if req.response_format != ResponseFormat::Text => {
                tracing::debug!("provider rejected response_format; retrying without it");
                req.response_format = ResponseFormat::Text;
                Ok(self.provider.chat(&req).await?.content)
            }
            other => other.map(|r| r.content),
        }
    }

    // ------------------------------------------------------------------
    // Ask AI
    // ------------------------------------------------------------------

    async fn ask_request(
        &self,
        conv: &mut Conversation,
        question: &str,
        opts: &AskOptions,
    ) -> ChatRequest {
        let host = self.host(opts.host_id).await;
        let mut raw = RawContext {
            host,
            ..Default::default()
        };
        if opts.include_terminal {
            raw.selection = self.context.get_selected_terminal_text().await;
            raw.last_command = self.context.get_last_command().await;
            raw.last_error = self.context.get_last_error().await;
        }
        let ctx = prompts::format_context(&mut conv.session, &raw);
        let user = prompts::user_message(&mut conv.session, "", question, ctx.as_ref());
        let mut messages = vec![Message::system(prompts::system_prompt(prompts::ASK, None))];
        messages.extend(conv.history.iter().cloned());
        messages.push(Message::user(user.clone()));
        conv.history.push(Message::user(user));
        ChatRequest {
            messages,
            temperature: Some(self.options.temperature),
            max_tokens: self.options.max_tokens,
            ..Default::default()
        }
    }

    /// Ask AI (non-streaming).
    pub async fn ask(
        &self,
        conv: &mut Conversation,
        question: &str,
        opts: &AskOptions,
    ) -> Result<AskAnswer> {
        let req = self.ask_request(conv, question, opts).await;
        let resp = self.provider.chat(&req).await?;
        conv.record_answer(&resp.content, self.options.max_history);
        let r = conv.session.rehydrate(&resp.content);
        Ok(AskAnswer {
            text: r.text,
            unresolved_secrets: r.unresolved_secrets,
            redactions: conv.session.report().clone(),
        })
    }

    /// Ask AI (streaming, re-hydrated chunks).
    pub async fn ask_stream(
        &self,
        conv: &mut Conversation,
        question: &str,
        opts: &AskOptions,
    ) -> Result<AskStream> {
        let req = self.ask_request(conv, question, opts).await;
        let inner = self.provider.chat_stream(&req).await?;
        Ok(AskStream {
            inner,
            rehydrator: conv.session.rehydrator(),
            raw: String::new(),
            done: false,
        })
    }

    // ------------------------------------------------------------------
    // Commands
    // ------------------------------------------------------------------

    /// Re-hydrate and wrap a parsed suggestion into a proposal.
    pub(crate) fn finalize_command(
        &self,
        session: &SanitizerSession,
        mut s: CommandSuggestion,
        target: CommandTarget,
        host: Option<&HostContext>,
    ) -> CommandProposal {
        let r = session.rehydrate(&s.command);
        s.command = r.text;
        s.explanation = session.rehydrate(&s.explanation).text;
        s.alternatives = s
            .alternatives
            .iter()
            .map(|a| session.rehydrate(a).text)
            .collect();
        s.diagnosis = s.diagnosis.map(|d| session.rehydrate(&d).text);
        for v in &mut s.variables {
            v.default = v
                .default
                .take()
                .map(|d| session.rehydrate(&d).text)
                .filter(|d| default_allowed(&v.name, d));
        }
        let host_ref = host.map(|h| HostRef {
            id: h.host_id,
            display: if h.name.is_empty() || h.name == h.address {
                h.address.clone()
            } else {
                format!("{} ({})", h.name, h.address)
            },
        });
        let run = RunProposal::new(
            s.command.clone(),
            target.dialect(),
            host_ref,
            ProposalOrigin::Ai,
            s.risk_suggestion,
        );
        let form = (!template_variables(&s.command).is_empty()).then(|| {
            let meta: Vec<SnippetVariable> = s.variables.iter().map(to_snippet_var).collect();
            VariableForm::from_template(&s.command, &meta)
        });
        CommandProposal {
            target,
            unresolved_secrets: r.unresolved_secrets,
            suggestion: s,
            run,
            form,
            redactions: session.report().clone(),
        }
    }

    /// Generate Command.
    pub async fn generate_command(&self, req: &GenerateRequest) -> Result<CommandProposal> {
        if req.description.trim().is_empty() {
            return Err(AiError::InvalidInput(
                "describe what the command should do".into(),
            ));
        }
        let host = self.host(req.host_id).await;
        let target = Self::resolve_target(req.target, host.as_ref());
        let mut raw = RawContext {
            host: host.clone(),
            snippets: self
                .context
                .search_snippets(&req.description, self.options.snippet_context)
                .await,
            ..Default::default()
        };
        if req.include_terminal {
            raw.selection = self.context.get_selected_terminal_text().await;
            raw.last_command = self.context.get_last_command().await;
            raw.last_error = self.context.get_last_error().await;
        }
        let mut session = self.new_session();
        let ctx = prompts::format_context(&mut session, &raw);
        let user = prompts::user_message(&mut session, "Request: ", &req.description, ctx.as_ref());
        let chat = self.request(
            vec![
                Message::system(prompts::system_prompt(prompts::GENERATE, Some(target))),
                Message::user(user),
            ],
            prompts::command_format(false, false),
        );
        let text = self.complete(chat).await?;
        let suggestion = parse_command_output(&text);
        Ok(self.finalize_command(&session, suggestion, target, host.as_ref()))
    }

    /// Fix Last Error.
    pub async fn fix_last_error(&self, req: &FixRequest) -> Result<CommandProposal> {
        let last_command = self.context.get_last_command().await;
        let last_error = self.context.get_last_error().await;
        if last_command.is_none() && last_error.is_none() {
            return Err(AiError::InvalidInput(
                "no failed command in the active terminal".into(),
            ));
        }
        let host = self.host(req.host_id).await;
        let target = Self::resolve_target(req.target, host.as_ref());
        let raw = RawContext {
            host: host.clone(),
            last_command,
            last_error,
            ..Default::default()
        };
        let mut session = self.new_session();
        let ctx = prompts::format_context(&mut session, &raw);
        let mut user = PromptBuilder::new();
        user.push_static("Fix the failed command shown in the context.\n\n");
        if let Some(c) = &ctx {
            user.push(c);
        }
        let chat = self.request(
            vec![
                Message::system(prompts::system_prompt(prompts::FIX, Some(target))),
                Message::user(user.build()),
            ],
            prompts::command_format(true, false),
        );
        let text = self.complete(chat).await?;
        let suggestion = parse_command_output(&text);
        Ok(self.finalize_command(&session, suggestion, target, host.as_ref()))
    }

    /// Explain Command.
    pub async fn explain_command(&self, req: &ExplainRequest) -> Result<ExplainResult> {
        let command = if req.command.trim().is_empty() {
            match self.context.get_selected_terminal_text().await {
                Some(s) if !s.trim().is_empty() => s,
                _ => self.context.get_last_command().await.unwrap_or_default(),
            }
        } else {
            req.command.clone()
        };
        if command.trim().is_empty() {
            return Err(AiError::InvalidInput("no command to explain".into()));
        }
        let host = self.host(req.host_id).await;
        let target = Self::resolve_target(req.target, host.as_ref());
        let local = classify(&command, target.dialect());
        let raw = RawContext {
            host,
            ..Default::default()
        };
        let mut session = self.new_session();
        let ctx = prompts::format_context(&mut session, &raw);
        let user = prompts::user_message(
            &mut session,
            "Command to explain:\n",
            &command,
            ctx.as_ref(),
        );
        let chat = self.request(
            vec![
                Message::system(prompts::system_prompt(prompts::EXPLAIN, Some(target))),
                Message::user(user),
            ],
            prompts::explain_format(),
        );
        let text = self.complete(chat).await?;
        let e = parse_explanation(&text);
        let explanation = Explanation {
            summary: session.rehydrate(&e.summary).text,
            parts: e
                .parts
                .into_iter()
                .map(|mut p| {
                    p.text = session.rehydrate(&p.text).text;
                    p.meaning = session.rehydrate(&p.meaning).text;
                    p
                })
                .collect(),
            warnings: e
                .warnings
                .iter()
                .map(|w| session.rehydrate(w).text)
                .collect(),
            ..e
        };
        let effective_risk = combine_with_ai(local.level, explanation.risk_suggestion);
        Ok(ExplainResult {
            explanation,
            local_risk: local,
            effective_risk,
            redactions: session.report().clone(),
        })
    }

    // ------------------------------------------------------------------
    // Snippets
    // ------------------------------------------------------------------

    fn finalize_snippet(
        &self,
        session: Option<&SanitizerSession>,
        raw: RawSnippet,
        target: CommandTarget,
        fallback_name: &str,
    ) -> SnippetDraft {
        let rh = |s: &str| session.map_or_else(|| s.to_owned(), |ss| ss.rehydrate(s).text);
        let template = rh(&raw.template);
        let scrubbed = scrub_secrets(&template);
        let template = scrubbed.template;
        let mut variables: Vec<SnippetVariable> = Vec::new();
        for name in template_variables(&template) {
            let from_model = raw.variables.iter().find(|v| v.name == name);
            let from_scrub = scrubbed.variables.iter().find(|v| v.name == name);
            let mut var = match (from_scrub, from_model) {
                (Some(s), _) if s.default.is_none() && !s.description.is_empty() => s.clone(),
                (_, Some(m)) => to_snippet_var(m),
                (Some(s), None) => s.clone(),
                (None, None) => SnippetVariable {
                    name: name.clone(),
                    description: String::new(),
                    default: None,
                    required: true,
                },
            };
            var.description = rh(&var.description);
            var.default = var
                .default
                .map(|d| rh(&d))
                .filter(|d| default_allowed(&var.name, d));
            variables.push(var);
        }
        let local = classify(&template, target.dialect()).level;
        let risk = combine_with_ai(local, raw.risk_suggestion);
        let name = if raw.name.trim().is_empty() {
            fallback_name.chars().take(60).collect::<String>()
        } else {
            rh(&raw.name)
        };
        let mut tags = raw.tags.iter().map(|t| rh(t)).collect::<Vec<_>>();
        tags.push(cc_search_core::snippet_type_tag(target.snippet_type()).to_owned());
        SnippetDraft {
            name: name.trim().to_owned(),
            description: rh(&raw.description),
            snippet_type: target.snippet_type(),
            shell: None,
            template,
            variables,
            tags: normalize_tags(tags),
            risk,
            local_risk: local,
            ai_risk_suggestion: raw.risk_suggestion,
            source: if session.is_some() {
                SnippetSource::Ai
            } else {
                SnippetSource::History
            },
            secrets_removed: scrubbed.secrets_removed,
            parse_quality: raw.parse_quality,
        }
    }

    /// Create Snippet from a description.
    pub async fn create_snippet(&self, req: &CreateSnippetRequest) -> Result<SnippetDraft> {
        if req.description.trim().is_empty() {
            return Err(AiError::InvalidInput("describe the snippet".into()));
        }
        let host = self.host(req.host_id).await;
        let target = Self::resolve_target(req.target, host.as_ref());
        let raw = RawContext {
            host,
            snippets: self
                .context
                .search_snippets(&req.description, self.options.snippet_context)
                .await,
            ..Default::default()
        };
        let mut session = self.new_session();
        let ctx = prompts::format_context(&mut session, &raw);
        let user = prompts::user_message(
            &mut session,
            "Snippet request: ",
            &req.description,
            ctx.as_ref(),
        );
        let chat = self.request(
            vec![
                Message::system(prompts::system_prompt(prompts::SNIPPET, Some(target))),
                Message::user(user),
            ],
            prompts::snippet_format(),
        );
        let text = self.complete(chat).await?;
        let parsed = parse_snippet_output(&text);
        Ok(self.finalize_snippet(Some(&session), parsed, target, &req.description))
    }

    /// Convert a terminal command into a parameterized snippet. Local rules
    /// always run first; with `use_llm` the model refines the draft.
    pub async fn convert_to_snippet(&self, req: &ConvertRequest) -> Result<SnippetDraft> {
        let command = if req.command.trim().is_empty() {
            self.context.get_last_command().await.unwrap_or_default()
        } else {
            req.command.clone()
        };
        if command.trim().is_empty() {
            return Err(AiError::InvalidInput("no command to convert".into()));
        }
        let target = req.target.unwrap_or_default();
        let local = parameterize(&command, target.shell_dialect());
        let fallback_name: String = command
            .split_whitespace()
            .take(3)
            .collect::<Vec<_>>()
            .join(" ");
        let local_raw = RawSnippet {
            name: String::new(),
            description: String::new(),
            template: local.template.clone(),
            variables: local
                .variables
                .iter()
                .map(|v| SuggestedVariable {
                    name: v.name.clone(),
                    description: v.description.clone(),
                    default: v.default.clone(),
                })
                .collect(),
            tags: Vec::new(),
            risk_suggestion: None,
            parse_quality: ParseQuality::Json,
        };
        if !req.use_llm {
            let mut d = self.finalize_snippet(None, local_raw, target, &fallback_name);
            d.secrets_removed += local.secrets_removed;
            return Ok(d);
        }
        let mut session = self.new_session();
        let mut user = PromptBuilder::new();
        user.push_static("Command:\n")
            .push(&session.sanitize(&command))
            .push_static("\n\nDraft template:\n")
            .push(&session.sanitize(&local.template));
        let chat = self.request(
            vec![
                Message::system(prompts::system_prompt(prompts::CONVERT, Some(target))),
                Message::user(user.build()),
            ],
            prompts::snippet_format(),
        );
        let text = self.complete(chat).await?;
        let mut parsed = parse_snippet_output(&text);
        if parsed.template.trim().is_empty() {
            parsed.template = local.template.clone();
            parsed.variables = local_raw.variables.clone();
        }
        // Keep local metadata for variables the model used but did not list,
        // and local defaults where the model did not provide one.
        for name in template_variables(&parsed.template) {
            if !parsed.variables.iter().any(|v| v.name == name) {
                if let Some(l) = local.variables.iter().find(|l| l.name == name) {
                    parsed.variables.push(SuggestedVariable {
                        name,
                        description: l.description.clone(),
                        default: l.default.clone(),
                    });
                }
            }
        }
        for v in &mut parsed.variables {
            if v.default.is_none() {
                v.default = local
                    .variables
                    .iter()
                    .find(|l| l.name == v.name)
                    .and_then(|l| l.default.clone());
            }
        }
        let mut d = self.finalize_snippet(Some(&session), parsed, target, &fallback_name);
        d.secrets_removed += local.secrets_removed;
        Ok(d)
    }

    // ------------------------------------------------------------------
    // Search
    // ------------------------------------------------------------------

    /// Search pipeline: exact/FTS → semantic → RAG → generation.
    pub async fn search(&self, query: &str, opts: &SearchOptions) -> Result<SearchOutcome> {
        if query.trim().is_empty() {
            return Err(AiError::InvalidInput("empty search".into()));
        }
        pipeline::run(self, query, opts).await
    }

    /// Effective risk of a command with an optional AI opinion (helper for
    /// UI flows that do not go through a feature).
    pub fn assess(&self, command: &str, target: CommandTarget, ai: Option<RiskLevel>) -> RiskLevel {
        combine_with_ai(classify(command, target.dialect()).level, ai)
    }
}

/// Defaults are dropped for secret-like variable names and for values that
/// look like secrets or still contain secret placeholders.
fn default_allowed(name: &str, value: &str) -> bool {
    !matches!(
        rules_classify_secret_key(name),
        Some(Category::Password | Category::Secret)
    ) && secret_spans(value).is_empty()
        && !["<PASSWORD_", "<SECRET_", "<PRIVATE_KEY_", "<PEM_"]
            .iter()
            .any(|p| value.contains(p))
}

fn to_snippet_var(v: &SuggestedVariable) -> SnippetVariable {
    SnippetVariable {
        name: v.name.clone(),
        description: v.description.clone(),
        default: v.default.clone(),
        required: true,
    }
}
