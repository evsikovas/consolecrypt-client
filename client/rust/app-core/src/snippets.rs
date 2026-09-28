//! Snippet palette search and rendering without an AI provider: local FTS
//! over the per-profile search index (always available — an in-memory index
//! when the file cannot be used), semantic ranking only when the default
//! provider has an embedding model, never an LLM call.

use crate::ai::render_snippet_model;
use crate::app::AppCore;
use crate::dto::EditableDto;
use crate::dto::{parse_id, AiSearchOptionsDto, DocKind, LlmMode, ResultOrigin, SnippetDto};
use crate::error::AppResult;
use crate::SnippetRenderDto;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// One snippet search result.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SnippetSearchHitDto {
    pub snippet_id: String,
    pub score: f64,
    /// How it was found (`None` for the "most used" list of an empty query).
    pub origin: Option<ResultOrigin>,
    /// Exact name / command match.
    pub exact: bool,
}

impl AppCore {
    /// Snippet palette: exact / full-text first, then semantic (with an
    /// embedding-capable default provider). An empty query lists the most
    /// used (then most recently used) snippets.
    pub async fn search_snippets(
        &self,
        query: String,
        limit: u32,
    ) -> AppResult<Vec<SnippetSearchHitDto>> {
        let limit = limit.clamp(1, 100);
        if query.trim().is_empty() {
            let (_, u) = self.unlocked().await?;
            let mut v = u.working().snippets();
            v.sort_by(|a, b| {
                b.usage_count
                    .cmp(&a.usage_count)
                    .then(b.last_used_at.cmp(&a.last_used_at))
                    .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
            });
            return Ok(v
                .iter()
                .take(limit as usize)
                .map(|s| SnippetSearchHitDto {
                    snippet_id: s.id.to_string(),
                    score: s.usage_count as f64,
                    origin: None,
                    exact: false,
                })
                .collect());
        }
        let r = self
            .ai_search(
                query,
                AiSearchOptionsDto {
                    limit,
                    kinds: vec![DocKind::Snippet],
                    semantic: true,
                    llm: LlmMode::Never,
                    ..Default::default()
                },
            )
            .await?;
        Ok(r.hits
            .into_iter()
            .filter(|h| h.kind == DocKind::Snippet)
            .map(|h| SnippetSearchHitDto {
                snippet_id: h.id,
                score: h.score,
                origin: Some(h.origin),
                exact: h.exact,
            })
            .collect())
    }

    /// [`AppCore::snippet_render`] for a snippet that may not be saved yet
    /// (editor preview): same quoting policy, per-field errors, run
    /// proposal with the snippet's risk as a floor.
    pub async fn snippet_render_dto(
        &self,
        snippet: SnippetDto,
        values: HashMap<String, String>,
    ) -> AppResult<SnippetRenderDto> {
        let (_, u) = self.unlocked().await?;
        let stored = parse_id("id", &snippet.id)
            .ok()
            .and_then(|id| u.working().snippet(id));
        let id = stored.as_ref().map(|s| s.id).unwrap_or_default();
        let model = snippet.to_model(id, stored.as_ref())?;
        Ok(render_snippet_model(&model, &values))
    }
}
