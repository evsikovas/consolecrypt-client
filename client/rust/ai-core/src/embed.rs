//! Keeps the local search index's embeddings in sync with the configured
//! embedding model (CLIENT_SPEC §12.3: embeddings are local-only and
//! rebuilt locally).
//!
//! Texts are sanitized with the provider's privacy profile before they are
//! sent to the embedding endpoint (a remote embedding API sees the content).

use crate::cancel::{with_cancellation, CancellationToken};
use crate::error::{AiError, Result};
use crate::provider::{EmbeddingRequest, LlmProvider};
use crate::sanitizer::{SanitizedText, SanitizerSession};
use cc_search_core::{EmbeddingModel, SearchError, SearchIndex};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

/// Run a blocking index operation on the blocking thread pool.
pub(crate) async fn blocking<T, F>(index: &Arc<SearchIndex>, f: F) -> Result<T>
where
    F: FnOnce(&SearchIndex) -> Result<T, SearchError> + Send + 'static,
    T: Send + 'static,
{
    let idx = Arc::clone(index);
    tokio::task::spawn_blocking(move || f(&idx))
        .await
        .map_err(|e| AiError::Internal(format!("index task failed: {e}")))?
        .map_err(AiError::from)
}

/// Outcome of [`EmbeddingSync::run`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct EmbeddingSyncStats {
    /// Vectors stored in this run.
    pub embedded: usize,
    /// The model (id or dimension) changed and all vectors were dropped.
    pub model_changed: bool,
    /// Documents still without an up-to-date vector.
    pub remaining: usize,
}

/// Embeds pending documents of a [`SearchIndex`] with a provider.
#[derive(Debug, Clone)]
pub struct EmbeddingSync {
    provider: Arc<dyn LlmProvider>,
    index: Arc<SearchIndex>,
    batch_size: usize,
    max_batches: usize,
}

impl EmbeddingSync {
    pub fn new(provider: Arc<dyn LlmProvider>, index: Arc<SearchIndex>) -> Self {
        Self {
            provider,
            index,
            batch_size: 32,
            max_batches: usize::MAX,
        }
    }

    pub fn with_batch_size(mut self, n: usize) -> Self {
        self.batch_size = n.max(1);
        self
    }

    /// Limit work per run (e.g. to embed incrementally in the background).
    pub fn with_max_batches(mut self, n: usize) -> Self {
        self.max_batches = n.max(1);
        self
    }

    /// Ensure the index model matches the provider (dropping stale vectors
    /// on a change) and embed pending documents.
    pub async fn run(&self, cancel: Option<&CancellationToken>) -> Result<EmbeddingSyncStats> {
        let token = cancel.cloned().unwrap_or_default();
        let model_id = self
            .provider
            .embedding_model_id()
            .ok_or(AiError::Unsupported("embeddings"))?;
        let current = blocking(&self.index, |i| i.embedding_model()).await?;
        let mut model_changed = false;
        let model = match current {
            Some(m) if m.id == model_id => m,
            _ => {
                let probe = with_cancellation(
                    &token,
                    self.provider.embed(&EmbeddingRequest {
                        inputs: vec![SanitizedText::from_static("dimension probe")],
                        model: None,
                    }),
                )
                .await?;
                let dim = probe.dim();
                if dim == 0 {
                    return Err(AiError::InvalidResponse("empty embedding vector".into()));
                }
                let m = EmbeddingModel::new(model_id, dim);
                let mm = m.clone();
                model_changed = blocking(&self.index, move |i| i.set_embedding_model(&mm)).await?;
                m
            }
        };
        let profile = self.provider.privacy_profile();
        let mut embedded = 0;
        for _ in 0..self.max_batches {
            let n = self.batch_size;
            let jobs = blocking(&self.index, move |i| i.pending_embeddings(n)).await?;
            if jobs.is_empty() {
                break;
            }
            let mut session = SanitizerSession::new(profile);
            let inputs: Vec<SanitizedText> =
                jobs.iter().map(|j| session.sanitize(&j.text())).collect();
            let resp = with_cancellation(
                &token,
                self.provider.embed(&EmbeddingRequest {
                    inputs,
                    model: None,
                }),
            )
            .await?;
            if resp.vectors.len() != jobs.len() || resp.dim() != model.dim {
                return Err(AiError::InvalidResponse(format!(
                    "embedding dimension changed ({} → {}); re-run to rebuild",
                    model.dim,
                    resp.dim()
                )));
            }
            let items: Vec<_> = jobs
                .into_iter()
                .zip(resp.vectors)
                .map(|(j, v)| (j.document.id, j.content_hash, v))
                .collect();
            let stored = blocking(&self.index, move |i| i.store_embeddings(&items)).await?;
            embedded += stored;
            if stored == 0 {
                break;
            }
        }
        let stats = blocking(&self.index, |i| i.embedding_stats()).await?;
        tracing::debug!(
            embedded,
            remaining = stats.documents - stats.embedded,
            "embedding sync"
        );
        Ok(EmbeddingSyncStats {
            embedded,
            model_changed,
            remaining: stats.documents.saturating_sub(stats.embedded),
        })
    }
}
