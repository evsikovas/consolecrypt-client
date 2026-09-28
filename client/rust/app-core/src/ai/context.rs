//! [`AppAiContext`]: app-core's implementation of ai-core's security
//! boundary (CLIENT_SPEC §14, ADR-0105 §1) over the decrypted working set,
//! the local search index and terminal-core's command tracker.
//!
//! Only the six allowed read methods exist. The working set never holds
//! decrypted Secret objects, the host context is built by
//! [`HostContext::from_host`] (name, address, port, user, tags, OS, shell —
//! no credential reference), and this type has no access to the vault
//! writer, the credential resolver or the secure store.

use super::index::with_index;
use super::session::ExecBackend;
use crate::working_set::WorkingSet;
use async_trait::async_trait;
use cc_ai_core::context::{AiContextProvider, HostContext, KbHit};
use cc_models::snippet::Snippet;
use cc_models::ObjectId;
use cc_search_core::{DocKind, SearchFilter, SearchIndex, TextQuery};
use cc_terminal_core::TerminalId;
use std::sync::Arc;

/// Upper bound for text passed in by the UI (selection).
pub(crate) const MAX_SELECTION_BYTES: usize = 64 * 1024;

/// Truncate at a char boundary.
pub(crate) fn clamp_text(mut s: String, max: usize) -> String {
    if s.len() > max {
        let mut cut = max;
        while !s.is_char_boundary(cut) {
            cut -= 1;
        }
        s.truncate(cut);
    }
    s
}

/// Per-request context (host / terminal / selection chosen by the UI).
pub(crate) struct AppAiContext {
    pub working: Arc<WorkingSet>,
    pub index: Option<Arc<SearchIndex>>,
    pub backend: Arc<dyn ExecBackend>,
    pub terminal: Option<TerminalId>,
    pub selected_text: Option<String>,
}

impl std::fmt::Debug for AppAiContext {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AppAiContext")
            .field("index", &self.index.is_some())
            .field("terminal", &self.terminal)
            .field("selection", &self.selected_text.as_ref().map(String::len))
            .finish_non_exhaustive()
    }
}

fn substring_match(query: &str, fields: &[&str]) -> bool {
    let q = query.to_lowercase();
    let hay = fields.join(" ").to_lowercase();
    let terms: Vec<&str> = q.split_whitespace().collect();
    terms.is_empty() || terms.iter().any(|t| hay.contains(t))
}

impl AppAiContext {
    async fn search(&self, query: &str, limit: usize, filter: SearchFilter) -> Option<Vec<KbHit>> {
        let index = self.index.as_ref()?;
        let q = TextQuery::new(query, limit.max(1)).with_filter(filter);
        match with_index(index, move |i| i.search_text(&q)).await {
            Ok(hits) => Some(
                hits.into_iter()
                    .map(|h| KbHit {
                        id: h.document.id,
                        kind: h.document.kind,
                        title: h.document.title,
                        body: h.document.body,
                        tags: h.document.tags,
                        score: h.score,
                    })
                    .collect(),
            ),
            Err(e) => {
                tracing::debug!(error = %e, "context index search failed");
                None
            }
        }
    }
}

#[async_trait]
impl AiContextProvider for AppAiContext {
    async fn search_snippets(&self, query: &str, limit: usize) -> Vec<Snippet> {
        if let Some(hits) = self
            .search(query, limit, SearchFilter::kinds([DocKind::Snippet]))
            .await
        {
            return hits
                .into_iter()
                .filter_map(|h| self.working.snippet(h.id))
                .collect();
        }
        self.working
            .snippets()
            .into_iter()
            .filter(|s| substring_match(query, &[&s.name, &s.description, &s.template]))
            .take(limit)
            .collect()
    }

    async fn get_host_context(&self, host_id: ObjectId) -> Option<HostContext> {
        self.working
            .host(host_id)
            .map(|h| HostContext::from_host(&h))
    }

    async fn get_selected_terminal_text(&self) -> Option<String> {
        self.selected_text.clone()
    }

    async fn get_last_command(&self) -> Option<String> {
        self.terminal.and_then(|t| self.backend.last_command(t))
    }

    async fn get_last_error(&self) -> Option<String> {
        self.terminal.and_then(|t| self.backend.last_error(t))
    }

    async fn search_local_kb(&self, query: &str, limit: usize) -> Vec<KbHit> {
        self.search(query, limit, SearchFilter::default())
            .await
            .unwrap_or_default()
    }
}
