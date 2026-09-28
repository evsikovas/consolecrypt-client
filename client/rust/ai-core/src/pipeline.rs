//! Search pipeline (CLIENT_SPEC §12.1):
//!
//! 1. Exact / FTS (local)
//! 2. Semantic local search (hybrid with FTS via reciprocal rank fusion)
//! 3. RAG over the local knowledge base (LLM, sanitized context)
//! 4. LLM generation (no local hits)
//!
//! Every result says where it came from ([`ResultOrigin`] /
//! [`AnswerOrigin`]) and [`SearchOutcome::stages`] reports which stages ran.

use crate::assistant::AiAssistant;
use crate::context::KbHit;
use crate::embed::blocking;
use crate::error::{AiError, Result};
use crate::features::output::parse_command_output;
use crate::features::prompts::{self, RawContext};
use crate::features::{CommandProposal, CommandTarget};
use crate::provider::{ChatRequest, EmbeddingRequest, Message};
use crate::sanitizer::{PromptBuilder, SanitizerSession};
use cc_models::ObjectId;
use cc_search_core::{Document, HitSource, HybridQuery, SearchFilter, TextQuery};
use serde::{Deserialize, Serialize};

/// Where a local hit came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResultOrigin {
    Exact,
    FullText,
    Semantic,
    /// Found by both full-text and semantic search.
    Hybrid,
}

impl From<HitSource> for ResultOrigin {
    fn from(s: HitSource) -> Self {
        match s {
            HitSource::Exact => ResultOrigin::Exact,
            HitSource::FullText => ResultOrigin::FullText,
            HitSource::Semantic => ResultOrigin::Semantic,
            HitSource::Hybrid => ResultOrigin::Hybrid,
        }
    }
}

/// Where a generated answer came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AnswerOrigin {
    /// LLM answer grounded on local knowledge-base hits.
    Rag,
    /// LLM answer without local hits.
    Generated,
}

/// A local search result.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PipelineHit {
    pub document: Document,
    pub score: f64,
    pub origin: ResultOrigin,
    /// Title equals the query or the body contains it verbatim.
    pub exact: bool,
}

fn is_exact(doc: &Document, query: &str) -> bool {
    let q = query.trim();
    !q.is_empty()
        && (doc.title.trim().eq_ignore_ascii_case(q)
            || doc
                .body
                .to_ascii_lowercase()
                .contains(&q.to_ascii_lowercase()))
}

/// LLM answer (stages 3/4).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GeneratedAnswer {
    pub origin: AnswerOrigin,
    pub proposal: CommandProposal,
    /// Knowledge-base documents the model says it used.
    pub sources: Vec<ObjectId>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StageKind {
    FullText,
    Semantic,
    Rag,
    Generation,
}

/// What a stage did.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StageReport {
    pub stage: StageKind,
    pub ran: bool,
    pub hits: usize,
    /// Why it was skipped / failed (never contains user content).
    pub note: Option<String>,
}

/// When to involve the LLM.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LlmMode {
    /// Local search only.
    Never,
    /// Only when there is no exact local match.
    #[default]
    Auto,
    /// Always produce an answer (RAG if there are hits).
    Always,
}

/// Pipeline options.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SearchOptions {
    pub limit: usize,
    pub filter: SearchFilter,
    pub semantic: bool,
    pub llm: LlmMode,
    /// Vector hits below this cosine similarity are ignored.
    pub min_similarity: f32,
    /// Knowledge-base entries passed to the model for RAG.
    pub rag_context: usize,
    pub target: Option<CommandTarget>,
    pub host_id: Option<ObjectId>,
}

impl Default for SearchOptions {
    fn default() -> Self {
        Self {
            limit: 10,
            filter: SearchFilter::default(),
            semantic: true,
            llm: LlmMode::Auto,
            min_similarity: 0.3,
            rag_context: 5,
            target: None,
            host_id: None,
        }
    }
}

/// Pipeline result.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SearchOutcome {
    pub hits: Vec<PipelineHit>,
    pub answer: Option<GeneratedAnswer>,
    pub stages: Vec<StageReport>,
}

fn kb_to_doc(k: KbHit) -> (Document, f64) {
    (
        Document::new(k.id, k.kind, k.title, k.body).with_tags(k.tags),
        k.score,
    )
}

pub(crate) async fn run(
    a: &AiAssistant,
    query: &str,
    opts: &SearchOptions,
) -> Result<SearchOutcome> {
    let mut stages = Vec::new();
    let limit = opts.limit.max(1);

    // 1. Exact / FTS.
    let mut hits: Vec<PipelineHit> = match a.index() {
        Some(index) => {
            let q = TextQuery::new(query, limit).with_filter(opts.filter.clone());
            blocking(index, move |i| i.search_text(&q))
                .await?
                .into_iter()
                .map(|h| PipelineHit {
                    origin: h.matched.source().into(),
                    exact: h.matched.exact,
                    score: h.score,
                    document: h.document,
                })
                .collect()
        }
        None => a
            .context()
            .search_local_kb(query, limit)
            .await
            .into_iter()
            .map(kb_to_doc)
            .map(|(document, score)| PipelineHit {
                origin: if is_exact(&document, query) {
                    ResultOrigin::Exact
                } else {
                    ResultOrigin::FullText
                },
                exact: is_exact(&document, query),
                score,
                document,
            })
            .collect(),
    };
    stages.push(StageReport {
        stage: StageKind::FullText,
        ran: true,
        hits: hits.len(),
        note: a
            .index()
            .is_none()
            .then(|| "no local index; used context KB search".into()),
    });

    // 2. Semantic (hybrid).
    let provider = a.provider();
    let semantic = if !opts.semantic {
        Err("disabled")
    } else if a.index().is_none() {
        Err("no local index")
    } else if provider.embedding_model_id().is_none() {
        Err("provider has no embedding model")
    } else {
        Ok(())
    };
    match semantic {
        Err(note) => stages.push(StageReport {
            stage: StageKind::Semantic,
            ran: false,
            hits: 0,
            note: Some(note.into()),
        }),
        Ok(()) => {
            let index = a
                .index()
                .cloned()
                .ok_or(AiError::Internal("index vanished".into()))?;
            let model_id = provider.embedding_model_id();
            let index_model = blocking(&index, |i| i.embedding_model()).await?;
            if index_model.as_ref().map(|m| &m.id) != model_id.as_ref() {
                stages.push(StageReport {
                    stage: StageKind::Semantic,
                    ran: false,
                    hits: 0,
                    note: Some(
                        "index embeddings missing or built with another model; run EmbeddingSync"
                            .into(),
                    ),
                });
            } else {
                let mut session = SanitizerSession::new(provider.privacy_profile());
                let req = EmbeddingRequest {
                    inputs: vec![session.sanitize(query)],
                    model: None,
                };
                match provider.embed(&req).await {
                    Ok(resp) if resp.vectors.len() == 1 => {
                        let vector = resp.vectors.into_iter().next().unwrap_or_default();
                        let q = HybridQuery::new(query, Some(vector), limit)
                            .with_filter(opts.filter.clone())
                            .with_min_similarity(opts.min_similarity);
                        let fused = blocking(&index, move |i| i.search_hybrid(&q)).await?;
                        let semantic_hits = fused
                            .iter()
                            .filter(|h| h.matched.vector_rank.is_some())
                            .count();
                        hits = fused
                            .into_iter()
                            .map(|h| PipelineHit {
                                origin: h.matched.source().into(),
                                exact: h.matched.exact,
                                score: h.score,
                                document: h.document,
                            })
                            .collect();
                        stages.push(StageReport {
                            stage: StageKind::Semantic,
                            ran: true,
                            hits: semantic_hits,
                            note: None,
                        });
                    }
                    Ok(_) => stages.push(StageReport {
                        stage: StageKind::Semantic,
                        ran: false,
                        hits: 0,
                        note: Some("invalid embedding response".into()),
                    }),
                    Err(e) => stages.push(StageReport {
                        stage: StageKind::Semantic,
                        ran: false,
                        hits: 0,
                        note: Some(format!("embedding failed: {e}")),
                    }),
                }
            }
        }
    }

    // 3/4. LLM.
    let exact = hits.iter().any(|h| h.exact);
    let want_llm = match opts.llm {
        LlmMode::Never => false,
        LlmMode::Auto => !exact,
        LlmMode::Always => true,
    };
    let mut answer = None;
    if !want_llm {
        let note = if opts.llm == LlmMode::Never {
            "disabled"
        } else {
            "exact local match"
        };
        for stage in [StageKind::Rag, StageKind::Generation] {
            stages.push(StageReport {
                stage,
                ran: false,
                hits: 0,
                note: Some(note.into()),
            });
        }
    } else {
        let host = match opts.host_id {
            Some(id) => a.context().get_host_context(id).await,
            None => None,
        };
        let target = opts.target.unwrap_or_else(|| {
            CommandTarget::for_shell_name(host.as_ref().and_then(|h| h.shell.as_deref()))
        });
        let used: Vec<&PipelineHit> = hits.iter().take(opts.rag_context).collect();
        let origin = if used.is_empty() {
            AnswerOrigin::Generated
        } else {
            AnswerOrigin::Rag
        };
        let mut session = a.new_session();
        let raw = RawContext {
            host: host.clone(),
            kb: used
                .iter()
                .map(|h| KbHit {
                    id: h.document.id,
                    kind: h.document.kind,
                    title: h.document.title.clone(),
                    body: h.document.body.clone(),
                    tags: h.document.tags.clone(),
                    score: h.score,
                })
                .collect(),
            ..Default::default()
        };
        let ctx = prompts::format_context(&mut session, &raw);
        let (feature, format) = match origin {
            AnswerOrigin::Rag => (prompts::RAG, prompts::command_format(false, true)),
            AnswerOrigin::Generated => (prompts::GENERATE, prompts::command_format(false, false)),
        };
        let mut user = PromptBuilder::new();
        user.push_static("Search: ").push(&session.sanitize(query));
        if let Some(c) = &ctx {
            user.push_static("\n\n").push(c);
        }
        let req = ChatRequest {
            messages: vec![
                Message::system(prompts::system_prompt(feature, Some(target))),
                Message::user(user.build()),
            ],
            temperature: Some(a.options().temperature),
            max_tokens: a.options().max_tokens,
            response_format: format,
            ..Default::default()
        };
        let stage = match origin {
            AnswerOrigin::Rag => StageKind::Rag,
            AnswerOrigin::Generated => StageKind::Generation,
        };
        match a.complete(req).await {
            Ok(text) => {
                let suggestion = parse_command_output(&text);
                let sources: Vec<ObjectId> = suggestion
                    .sources
                    .iter()
                    .filter_map(|n| {
                        n.checked_sub(1)
                            .and_then(|i| used.get(i))
                            .map(|h| h.document.id)
                    })
                    .collect();
                let proposal = a.finalize_command(&session, suggestion, target, host.as_ref());
                stages.push(StageReport {
                    stage,
                    ran: true,
                    hits: usize::from(!proposal.suggestion.command.is_empty()),
                    note: None,
                });
                answer = Some(GeneratedAnswer {
                    origin,
                    proposal,
                    sources,
                });
            }
            Err(e) => {
                // Local results are still useful; report the failure.
                stages.push(StageReport {
                    stage,
                    ran: false,
                    hits: 0,
                    note: Some(format!("LLM failed: {e}")),
                });
            }
        }
        let skipped = match origin {
            AnswerOrigin::Rag => StageKind::Generation,
            AnswerOrigin::Generated => StageKind::Rag,
        };
        stages.push(StageReport {
            stage: skipped,
            ran: false,
            hits: 0,
            note: Some(match origin {
                AnswerOrigin::Rag => "answered with RAG".into(),
                AnswerOrigin::Generated => "no local hits".into(),
            }),
        });
        stages.sort_by_key(|s| s.stage as u8);
    }

    Ok(SearchOutcome {
        hits,
        answer,
        stages,
    })
}
