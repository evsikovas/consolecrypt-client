//! # cc-ai-core — AI Command Knowledge Base
//!
//! Local-first AI features for ConsoleCrypt (CLIENT_SPEC §12–§16). LLM
//! providers are called **directly by the client**, never via the sync
//! server.
//!
//! ## Security boundary
//!
//! * [`context::AiContextProvider`] is the only way this crate reads user
//!   data: six read methods, none of which can return secrets
//!   ([`context::HostContext`] has no credential fields). The crate has no
//!   dependency on vault/crypto/platform/ssh crates.
//! * Provider API keys are passed as [`secrecy::SecretString`] when a
//!   provider is constructed ([`provider::build_provider`]) and then live only
//!   in the HTTP client's sensitive `Authorization` header.
//! * Everything sent to a provider is a [`sanitizer::SanitizedText`], which
//!   only the [`sanitizer::SanitizerSession`] can produce (plus `&'static`
//!   prompt constants). Private keys and passwords are redacted under every
//!   profile; `Local` is honoured only for loopback/private endpoints.
//! * Nothing executes automatically: features return a
//!   [`policy::RunProposal`]; only [`policy::ExecutionGate::approve`] turns it
//!   into an [`policy::ApprovedCommand`], requiring confirmation for
//!   modifying/destructive/unknown commands. Local [`risk`] rules decide; the
//!   model's opinion can only raise the level.
//!
//! ## Modules
//!
//! * [`provider`] — `LlmProvider` + Ollama / OpenAI-compatible (LM Studio,
//!   DeepSeek, generic) with streaming, embeddings, tool calling.
//! * [`sanitizer`] — context sanitizer with Strict/Standard/Local profiles,
//!   stable placeholders, local re-hydration.
//! * [`assistant`] — feature facade: Ask AI, Generate Command, Explain, Fix
//!   Last Error, Create Snippet, Convert command → snippet, Search.
//! * [`pipeline`] — exact/FTS → semantic → RAG → generation.
//! * [`embed`] — keeps local search-index embeddings in sync with the model.
//! * [`snippet`] — templates, value form, per-shell quoting.
//! * [`parameterize`] — command → parameterized snippet template.
//! * [`risk`], [`policy`] — local risk rules and the execution gate.
//! * [`shell`] — shell tokenizer shared by the above.

pub mod assistant;
pub mod cancel;
pub mod context;
pub mod embed;
pub mod error;
pub mod features;
pub mod parameterize;
pub mod pipeline;
pub mod policy;
pub mod provider;
pub mod risk;
pub mod sanitizer;
pub mod shell;
pub mod snippet;

pub use assistant::{
    AiAssistant, AskAnswer, AskOptions, AskStream, AssistantOptions, Conversation, ConvertRequest,
    CreateSnippetRequest, ExplainRequest, FixRequest, GenerateRequest,
};
pub use cancel::{cancellable, with_cancellation, CancellationToken};
pub use context::{AiContextProvider, HostContext, KbHit, NoContext, StaticContext};
pub use embed::{EmbeddingSync, EmbeddingSyncStats};
pub use error::{AiError, Result};
pub use features::{
    CommandProposal, CommandSuggestion, CommandTarget, ExplainResult, Explanation, ParseQuality,
    SnippetDraft,
};
pub use pipeline::{LlmMode, SearchOptions, SearchOutcome};
pub use policy::{
    ApprovedCommand, ExecutionGate, HostRef, PolicyError, ProposalOrigin, RunProposal,
};
pub use provider::{build_provider, Capabilities, LlmProvider, ProviderSettings};
pub use risk::{classify, combine_with_ai, CommandDialect, RiskAssessment, RiskLevel};
pub use sanitizer::{PrivacyProfile, SanitizedText, SanitizerSession};
pub use snippet::{render_snippet, RenderDialect, Template, VariableForm};
