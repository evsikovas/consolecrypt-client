//! Plain DTOs of the AI facade (re-exported from [`crate::dto`]).
//!
//! Same rules as every other DTO: ids are UUID strings, timestamps are Unix
//! milliseconds, enums are C-like (ai-core / search-core enums re-exported
//! as-is, serde snake_case). Nothing here carries secret material: provider
//! API keys never leave app-core, prompts are never returned, and sanitizer
//! reports are counts only.

use crate::dto::{ms, SnippetDto, SnippetVariableDto};
use cc_ai_core::features::{ExplainResult, SnippetDraft};
use cc_ai_core::policy::RunProposal;
use cc_ai_core::risk::{RiskAssessment, RiskReason};
use cc_ai_core::sanitizer::{Category, SanitizeReport};
use cc_ai_core::snippet::{FieldError, FormField, VariableForm};
use cc_models::snippet::RiskLevel;
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;

pub use cc_ai_core::pipeline::{AnswerOrigin, LlmMode, ResultOrigin, StageKind};
pub use cc_ai_core::policy::ProposalOrigin;
pub use cc_ai_core::provider::Locality;
pub use cc_ai_core::risk::CommandDialect;
pub use cc_ai_core::{CommandTarget, ParseQuality};
pub use cc_search_core::DocKind;

// ---- context & requests ------------------------------------------------------------

/// What the AI may look at for a request (CLIENT_SPEC §14: host metadata,
/// the selected terminal text, the last command / error of a terminal).
/// Never credentials: the host context is built without any credential
/// field.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AiContextOptionsDto {
    /// Host the request is about (defaults to the terminal's host).
    pub host_id: Option<String>,
    /// Terminal whose last command / last error may be used.
    pub terminal_id: Option<String>,
    /// Text the user explicitly selected in the terminal (passed by the UI).
    pub selected_text: Option<String>,
    /// Include the selection and the terminal's last command / error.
    pub include_terminal: bool,
}

/// One Ask AI turn. `conversation_id = None` starts a new conversation.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AiAskRequestDto {
    pub conversation_id: Option<String>,
    pub question: String,
}

/// Streaming Ask AI answer (like [`crate::TerminalAttachment`]): chunks end
/// with exactly one [`AiAskChunk::Done`] or [`AiAskChunk::Error`]. Dropping
/// the receiver (or [`crate::AppCore::ai_cancel`] with `request_id`) aborts
/// the HTTP request.
#[derive(Debug)]
pub struct AiAskStream {
    pub request_id: String,
    pub conversation_id: String,
    pub chunks: mpsc::Receiver<AiAskChunk>,
}

/// A piece of a streamed answer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AiAskChunk {
    /// Re-hydrated answer text (non-secret placeholders restored locally).
    Delta { text: String },
    /// The answer is complete.
    Done(AiAskSummaryDto),
    /// The request failed (`code` = [`crate::AppError::code`]).
    Error { code: String, message: String },
}

/// End-of-answer summary.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AiAskSummaryDto {
    /// Secret placeholders left in the answer (e.g. `<PASSWORD_1>`).
    pub unresolved_secrets: Vec<String>,
    /// Cumulative redactions of the conversation.
    pub redactions: SanitizerReportDto,
    /// The answer was stored as an `AiConversation` vault object.
    pub persisted: bool,
}

// ---- sanitizer ---------------------------------------------------------------------

/// What the sanitizer redacted from what was sent (counts only).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SanitizerReportDto {
    pub total: u32,
    pub private_keys: u32,
    pub pem_blocks: u32,
    pub passwords: u32,
    pub secrets: u32,
    pub ips: u32,
    pub hosts: u32,
    pub users: u32,
    pub databases: u32,
}

fn count(n: usize) -> u32 {
    u32::try_from(n).unwrap_or(u32::MAX)
}

impl SanitizerReportDto {
    pub(crate) fn from_report(r: &SanitizeReport) -> Self {
        Self {
            total: count(r.total()),
            private_keys: count(r.count(Category::PrivateKey)),
            pem_blocks: count(r.count(Category::Pem)),
            passwords: count(r.count(Category::Password)),
            secrets: count(r.count(Category::Secret)),
            ips: count(r.count(Category::Ip)),
            hosts: count(r.count(Category::Host)),
            users: count(r.count(Category::User)),
            databases: count(r.count(Category::Database)),
        }
    }
}

// ---- risk & execution gate ---------------------------------------------------------

/// Why a command got its risk level.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RiskReasonDto {
    pub level: RiskLevel,
    pub rule: String,
    pub detail: String,
}

impl RiskReasonDto {
    pub(crate) fn from_reason(r: &RiskReason) -> Self {
        Self {
            level: r.level,
            rule: r.rule.clone(),
            detail: r.detail.clone(),
        }
    }
}

fn reasons(v: &[RiskReason]) -> Vec<RiskReasonDto> {
    v.iter().map(RiskReasonDto::from_reason).collect()
}

/// Local risk assessment ([`crate::AppCore::assess_risk`]).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RiskAssessmentDto {
    pub level: RiskLevel,
    pub reasons: Vec<RiskReasonDto>,
    /// Running it needs `user_confirmed = true` in `approve_run`.
    pub requires_confirmation: bool,
}

impl RiskAssessmentDto {
    pub(crate) fn from_assessment(a: &RiskAssessment, requires_confirmation: bool) -> Self {
        Self {
            level: a.level,
            reasons: reasons(&a.reasons),
            requires_confirmation,
        }
    }
}

/// What the UI shows before "Run" (command, host, risk). Pass it back —
/// with the filled-in command and the target host — to
/// [`crate::AppCore::approve_run`]; the gate re-classifies the command text,
/// so editing `risk` / `requires_confirmation` has no effect.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunProposalDto {
    /// Id of an AI proposal kept by app-core (empty for user/snippet
    /// commands). The AI risk hint of that proposal is re-applied on
    /// approval.
    pub proposal_id: String,
    pub command: String,
    /// Host to run on (required by `approve_run`).
    pub host_id: Option<String>,
    pub host_display: Option<String>,
    pub dialect: CommandDialect,
    /// Effective risk (local rules, raised by the AI hint).
    pub risk: RiskLevel,
    pub local_risk: RiskLevel,
    pub ai_risk_suggestion: Option<RiskLevel>,
    pub reasons: Vec<RiskReasonDto>,
    pub requires_confirmation: bool,
    pub origin: ProposalOrigin,
    /// `{{vars}}` / secret placeholders that must be filled in first.
    pub unresolved_placeholders: Vec<String>,
}

impl RunProposalDto {
    pub(crate) fn from_run(proposal_id: String, r: &RunProposal) -> Self {
        Self {
            proposal_id,
            command: r.command.clone(),
            host_id: r.host.as_ref().and_then(|h| h.id).map(|i| i.to_string()),
            host_display: r.host.as_ref().map(|h| h.display.clone()),
            dialect: r.dialect,
            risk: r.risk,
            local_risk: r.local_risk,
            ai_risk_suggestion: r.ai_risk_suggestion,
            reasons: reasons(&r.reasons),
            requires_confirmation: r.requires_confirmation,
            origin: r.origin,
            unresolved_placeholders: r.unresolved_placeholders.clone(),
        }
    }
}

/// Capability to run exactly this command once on exactly this host, via
/// [`crate::AppCore::exec_approved`] or
/// [`crate::AppCore::terminal_run_approved`]. Expires; cleared on lock.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApprovedCommandDto {
    pub token: String,
    pub command: String,
    pub host_id: String,
    pub host_display: String,
    pub risk: RiskLevel,
    pub expires_at_ms: i64,
}

// ---- variable forms & snippets -----------------------------------------------------

/// One input of the "fill in values" form.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FormFieldDto {
    pub name: String,
    pub label: String,
    pub description: String,
    pub default: Option<String>,
    pub required: bool,
    /// Mask the input and never keep the value in history.
    pub secret: bool,
}

/// The value form shown before running a template.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct VariableFormDto {
    pub fields: Vec<FormFieldDto>,
}

impl VariableFormDto {
    pub(crate) fn from_form(f: &VariableForm) -> Self {
        Self {
            fields: f.fields.iter().map(field).collect(),
        }
    }
}

fn field(f: &FormField) -> FormFieldDto {
    FormFieldDto {
        name: f.name.clone(),
        label: f.label.clone(),
        description: f.description.clone(),
        default: f.default.clone(),
        required: f.required,
        secret: f.secret,
    }
}

/// Validation error of one form field.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FieldErrorDto {
    pub name: String,
    pub message: String,
}

impl FieldErrorDto {
    pub(crate) fn from_error(e: &FieldError) -> Self {
        Self {
            name: e.name.clone(),
            message: e.message.clone(),
        }
    }
}

/// Result of [`crate::AppCore::snippet_render`]: either the rendered command
/// (values quoted for the snippet's shell) with its run proposal, or
/// per-field errors.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SnippetRenderDto {
    pub command: Option<String>,
    pub field_errors: Vec<FieldErrorDto>,
    /// Set the target `host_id` and pass it to `approve_run`.
    pub run: Option<RunProposalDto>,
}

/// A snippet proposal (Create Snippet / Convert command). `snippet` has an
/// empty id: save it with [`crate::AppCore::save_snippet`] after review.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SnippetDraftDto {
    pub snippet: SnippetDto,
    pub local_risk: RiskLevel,
    pub ai_risk_suggestion: Option<RiskLevel>,
    /// Secret values removed from the template / defaults.
    pub secrets_removed: u32,
    pub parse_quality: ParseQuality,
    pub form: VariableFormDto,
}

impl SnippetDraftDto {
    pub(crate) fn from_draft(d: &SnippetDraft) -> Self {
        let now = ms(&chrono::Utc::now());
        let form = VariableForm::from_template(&d.template, &d.variables);
        Self {
            snippet: SnippetDto {
                package_name: None,
                catalog_id: None,
                id: String::new(),
                name: d.name.clone(),
                description: d.description.clone(),
                snippet_type: d.snippet_type,
                shell: d.shell.clone(),
                template: d.template.clone(),
                variables: d
                    .variables
                    .iter()
                    .map(|v| SnippetVariableDto {
                        name: v.name.clone(),
                        description: v.description.clone(),
                        default: v.default.clone(),
                        required: v.required,
                    })
                    .collect(),
                tags: d.tags.clone(),
                risk_level: d.risk,
                source: d.source,
                created_by_device_id: None,
                last_used_at_ms: None,
                usage_count: 0,
                created_at_ms: now,
                updated_at_ms: now,
            },
            local_risk: d.local_risk,
            ai_risk_suggestion: d.ai_risk_suggestion,
            secrets_removed: count(d.secrets_removed),
            parse_quality: d.parse_quality,
            form: VariableFormDto::from_form(&form),
        }
    }
}

// ---- generated commands & explanations ---------------------------------------------

/// A generated command (Generate Command / Fix Last Error / search answer).
/// Never executed by app-core: run it with `approve_run(run, confirmed)` →
/// `exec_approved` / `terminal_run_approved`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CommandProposalDto {
    pub provider_id: String,
    pub target: CommandTarget,
    /// Re-hydrated command (may contain `{{vars}}` / secret placeholders).
    pub command: String,
    pub explanation: String,
    pub alternatives: Vec<String>,
    /// Fix Last Error: what went wrong.
    pub diagnosis: Option<String>,
    /// Effective risk, local risk, AI hint and reasons live in `run`.
    pub run: RunProposalDto,
    /// Present when the command contains `{{variables}}`.
    pub form: Option<VariableFormDto>,
    /// Secret placeholders the user must replace (e.g. `<PASSWORD_1>`).
    pub unresolved_secrets: Vec<String>,
    pub redactions: SanitizerReportDto,
    /// Profile actually applied for this provider (`local` only for
    /// loopback / private endpoints).
    pub privacy_profile: crate::dto::PrivacyProfile,
    pub parse_quality: ParseQuality,
}

/// One explained piece of a command.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExplainedPartDto {
    pub text: String,
    pub meaning: String,
}

/// Explain Command result.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExplainResultDto {
    pub summary: String,
    pub parts: Vec<ExplainedPartDto>,
    pub warnings: Vec<String>,
    pub local_risk: RiskAssessmentDto,
    /// Local level raised by the model's opinion.
    pub effective_risk: RiskLevel,
    pub ai_risk_suggestion: Option<RiskLevel>,
    pub redactions: SanitizerReportDto,
    pub privacy_profile: crate::dto::PrivacyProfile,
    pub parse_quality: ParseQuality,
}

impl ExplainResultDto {
    pub(crate) fn from_result(r: &ExplainResult, profile: crate::dto::PrivacyProfile) -> Self {
        Self {
            summary: r.explanation.summary.clone(),
            parts: r
                .explanation
                .parts
                .iter()
                .map(|p| ExplainedPartDto {
                    text: p.text.clone(),
                    meaning: p.meaning.clone(),
                })
                .collect(),
            warnings: r.explanation.warnings.clone(),
            local_risk: RiskAssessmentDto::from_assessment(
                &r.local_risk,
                r.local_risk.requires_confirmation(),
            ),
            effective_risk: r.effective_risk,
            ai_risk_suggestion: r.explanation.risk_suggestion,
            redactions: SanitizerReportDto::from_report(&r.redactions),
            privacy_profile: profile,
            parse_quality: r.explanation.parse_quality,
        }
    }
}

// ---- search ------------------------------------------------------------------------

/// Search options ([`crate::AppCore::ai_search`]).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AiSearchOptionsDto {
    pub limit: u32,
    /// Empty = every kind.
    pub kinds: Vec<DocKind>,
    /// Every tag must match.
    pub tags: Vec<String>,
    /// Hybrid FTS + vector search (needs an embedding model).
    pub semantic: bool,
    /// When to involve the LLM (RAG / generation). Default `never` (local
    /// palette search); ignored without a provider.
    pub llm: LlmMode,
    /// `None` = the default provider (search stays local without one).
    pub provider_id: Option<String>,
    pub target: Option<CommandTarget>,
    pub host_id: Option<String>,
}

impl Default for AiSearchOptionsDto {
    fn default() -> Self {
        Self {
            limit: 10,
            kinds: Vec::new(),
            tags: Vec::new(),
            semantic: true,
            llm: LlmMode::Never,
            provider_id: None,
            target: None,
            host_id: None,
        }
    }
}

/// A local knowledge-base hit.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AiSearchHitDto {
    /// Object id (snippet / note / host / history entry).
    pub id: String,
    pub kind: DocKind,
    pub title: String,
    pub body: String,
    pub tags: Vec<String>,
    pub score: f64,
    pub origin: ResultOrigin,
    pub exact: bool,
}

/// LLM answer of the search pipeline (RAG or generation).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AiSearchAnswerDto {
    pub origin: AnswerOrigin,
    pub proposal: CommandProposalDto,
    /// Knowledge-base objects the model says it used.
    pub source_ids: Vec<String>,
}

/// What a pipeline stage did.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AiStageDto {
    pub stage: StageKind,
    pub ran: bool,
    pub hits: u32,
    /// Why it was skipped / failed (never user content).
    pub note: Option<String>,
}

/// Search result: local hits, optional LLM answer, stage report.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AiSearchResultDto {
    pub hits: Vec<AiSearchHitDto>,
    pub answer: Option<AiSearchAnswerDto>,
    pub stages: Vec<AiStageDto>,
}

// ---- index & providers ---------------------------------------------------------------

/// State of the local search index.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AiIndexStateDto {
    /// Not opened yet (just unlocked) or being rebuilt.
    Building,
    Ready,
    /// Could not be opened; search is unavailable (see `last_error`).
    Failed,
}

/// Local search index status (also sent as [`crate::AppEvent::AiIndexStatus`]).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AiIndexStatusDto {
    pub state: AiIndexStateDto,
    pub documents: u32,
    /// Documents with an up-to-date vector for `embedding_model`.
    pub embedded: u32,
    /// Embedding model id of the index (`<provider-kind>:<model>`).
    pub embedding_model: Option<String>,
    /// Last indexing / embedding failure (no user content).
    pub last_error: Option<String>,
}

impl AiIndexStatusDto {
    pub(crate) fn building() -> Self {
        Self {
            state: AiIndexStateDto::Building,
            documents: 0,
            embedded: 0,
            embedding_model: None,
            last_error: None,
        }
    }
}

/// Provider capabilities (CLIENT_SPEC §13.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct AiCapabilitiesDto {
    pub chat: bool,
    pub responses: bool,
    pub embeddings: bool,
    pub streaming: bool,
    pub tool_calling: bool,
}

/// Result of [`crate::AppCore::test_ai_provider`] (errors are returned as
/// [`crate::AppError`]: `ai_auth_failed`, `ai_unavailable`, `ai_provider`, …).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AiProviderTestDto {
    pub provider_id: String,
    /// Models advertised by the endpoint (empty when it cannot list them
    /// and a tiny chat request was used instead).
    pub models: Vec<String>,
    /// The configured chat model is among `models`.
    pub chat_model_available: bool,
    pub capabilities: AiCapabilitiesDto,
    pub locality: Locality,
    /// Profile actually applied (`local` only for loopback / LAN).
    pub effective_privacy_profile: crate::dto::PrivacyProfile,
    pub latency_ms: u64,
}

// ---- conversations -------------------------------------------------------------------

/// Chat role.
pub use cc_models::ai::ChatRole;

/// One message of a conversation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AiChatMessageDto {
    pub role: ChatRole,
    pub content: String,
    pub created_at_ms: i64,
}

/// An Ask AI conversation (in memory for this unlock, and — when vault
/// settings allow `sync_ai_conversations` — an `AiConversation` object).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AiConversationDto {
    pub id: String,
    pub title: String,
    pub provider_id: Option<String>,
    pub messages: Vec<AiChatMessageDto>,
    pub persisted: bool,
    pub created_at_ms: i64,
    pub updated_at_ms: i64,
}

impl AiConversationDto {
    pub(crate) fn from_model(c: &cc_models::ai::AiConversation, persisted: bool) -> Self {
        Self {
            id: c.id.to_string(),
            title: c.title.clone(),
            provider_id: c.provider_id.map(|p| p.to_string()),
            messages: c
                .messages
                .iter()
                .map(|m| AiChatMessageDto {
                    role: m.role,
                    content: m.content.clone(),
                    created_at_ms: ms(&m.created_at),
                })
                .collect(),
            persisted,
            created_at_ms: ms(&c.created_at),
            updated_at_ms: ms(&c.updated_at),
        }
    }
}
