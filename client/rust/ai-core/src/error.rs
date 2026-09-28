//! Error type for the AI core.

/// Errors from providers, the pipeline and the policy layer.
///
/// Messages never contain prompts, API keys or terminal content. Provider
/// error bodies are truncated and passed through the sanitizer before they
/// are stored in an error (see [`crate::provider`]).
#[derive(Debug, thiserror::Error)]
pub enum AiError {
    /// The provider (or its configuration) lacks this capability.
    #[error("capability not supported by this provider: {0}")]
    Unsupported(&'static str),
    /// Invalid provider configuration (base URL, model, …).
    #[error("invalid provider configuration: {0}")]
    Config(String),
    /// Connect, first-byte or idle timeout elapsed.
    #[error("request to the AI provider timed out")]
    Timeout,
    /// Cancelled via a [`crate::CancellationToken`].
    #[error("request cancelled")]
    Cancelled,
    /// The provider could not be reached.
    #[error("could not connect to the AI provider: {0}")]
    Connect(String),
    /// Authentication failed; the API key is missing or wrong.
    #[error("authentication with the AI provider failed (HTTP {status}); check the API key")]
    Auth { status: u16 },
    /// HTTP 429.
    #[error("rate limited by the AI provider")]
    RateLimited { retry_after_secs: Option<u64> },
    /// Model not found / not pulled (HTTP 404 on a model route).
    #[error("model not found on the AI provider: {0}")]
    ModelNotFound(String),
    /// Any other non-success HTTP status.
    #[error("AI provider returned HTTP {status}: {message}")]
    Http { status: u16, message: String },
    /// The provider reported an error inside a 200 response or a stream.
    #[error("AI provider error: {0}")]
    Provider(String),
    /// The response did not match the expected wire format.
    #[error("invalid response from the AI provider: {0}")]
    InvalidResponse(String),
    /// Other transport failure.
    #[error("transport error: {0}")]
    Transport(String),
    /// The request cannot be served (e.g. nothing to explain).
    #[error("invalid input: {0}")]
    InvalidInput(String),
    /// Local search index failure.
    #[error("local search index error: {0}")]
    Search(#[from] cc_search_core::SearchError),
    /// A background task failed (panic / runtime shutdown).
    #[error("internal error: {0}")]
    Internal(String),
}

impl AiError {
    /// Whether retrying the same request later might succeed.
    pub fn is_transient(&self) -> bool {
        match self {
            AiError::Timeout | AiError::Connect(_) | AiError::RateLimited { .. } => true,
            AiError::Http { status, .. } => *status >= 500,
            _ => false,
        }
    }
}

/// Convenience alias.
pub type Result<T, E = AiError> = std::result::Result<T, E>;
