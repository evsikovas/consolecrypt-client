//! Query types, FTS5 expression building and reciprocal rank fusion.

use crate::document::{normalize_tag, DocKind, Document};
use serde::{Deserialize, Serialize};

/// Restricts results by kind and tags.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SearchFilter {
    /// Empty = all kinds.
    #[serde(default)]
    pub kinds: Vec<DocKind>,
    /// Every listed tag must be present (AND). Normalized like document tags.
    #[serde(default)]
    pub tags: Vec<String>,
}

impl SearchFilter {
    pub fn kinds(kinds: impl IntoIterator<Item = DocKind>) -> Self {
        Self {
            kinds: kinds.into_iter().collect(),
            tags: Vec::new(),
        }
    }

    pub fn with_tags<I, S>(mut self, tags: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        self.tags = tags
            .into_iter()
            .map(|t| normalize_tag(t.as_ref()))
            .collect();
        self.tags.retain(|t| !t.is_empty());
        self
    }

    pub(crate) fn normalized_tags(&self) -> Vec<String> {
        let mut t: Vec<String> = self
            .tags
            .iter()
            .map(|t| normalize_tag(t))
            .filter(|t| !t.is_empty())
            .collect();
        t.sort();
        t.dedup();
        t
    }

    pub(crate) fn accepts_kind(&self, k: DocKind) -> bool {
        self.kinds.is_empty() || self.kinds.contains(&k)
    }
}

/// How multiple query terms combine.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MatchMode {
    /// Every term must match.
    All,
    /// Any term may match (bm25 ranks documents matching more terms higher).
    Any,
    /// Try `All`; if nothing matches and there are several terms, use `Any`.
    #[default]
    AllThenAny,
}

/// Full-text query.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TextQuery {
    /// Free text. Tokenized locally, so FTS5 syntax characters are inert.
    pub text: String,
    #[serde(default)]
    pub filter: SearchFilter,
    pub limit: usize,
    #[serde(default)]
    pub mode: MatchMode,
    /// Treat every term as a prefix (`kub` matches `kubectl`).
    #[serde(default = "default_true")]
    pub prefix: bool,
}

fn default_true() -> bool {
    true
}

impl TextQuery {
    pub fn new(text: impl Into<String>, limit: usize) -> Self {
        Self {
            text: text.into(),
            filter: SearchFilter::default(),
            limit,
            mode: MatchMode::default(),
            prefix: true,
        }
    }

    pub fn with_filter(mut self, filter: SearchFilter) -> Self {
        self.filter = filter;
        self
    }

    pub fn with_mode(mut self, mode: MatchMode) -> Self {
        self.mode = mode;
        self
    }

    pub fn with_prefix(mut self, prefix: bool) -> Self {
        self.prefix = prefix;
        self
    }
}

/// Hybrid (FTS + vector) query fused with reciprocal rank fusion.
#[derive(Debug, Clone, PartialEq)]
pub struct HybridQuery {
    pub text: String,
    /// Query embedding from the same model as the stored vectors. `None` →
    /// text-only ranking.
    pub embedding: Option<Vec<f32>>,
    pub filter: SearchFilter,
    pub limit: usize,
    /// RRF constant `k` (default 60).
    pub rrf_k: f64,
    /// Candidates taken from each ranked list before fusion.
    pub candidates: usize,
    /// Vector hits below this cosine similarity are ignored.
    pub min_similarity: f32,
}

impl HybridQuery {
    pub fn new(text: impl Into<String>, embedding: Option<Vec<f32>>, limit: usize) -> Self {
        Self {
            text: text.into(),
            embedding,
            filter: SearchFilter::default(),
            limit,
            rrf_k: DEFAULT_RRF_K,
            candidates: (limit * 4).max(50),
            min_similarity: 0.0,
        }
    }

    pub fn with_filter(mut self, filter: SearchFilter) -> Self {
        self.filter = filter;
        self
    }

    pub fn with_min_similarity(mut self, min: f32) -> Self {
        self.min_similarity = min;
        self
    }
}

/// Default reciprocal-rank-fusion constant.
pub const DEFAULT_RRF_K: f64 = 60.0;

/// Which ranked lists produced a hit.
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
pub struct MatchInfo {
    /// Title equals the query or the body contains it verbatim
    /// (ASCII case-insensitive).
    pub exact: bool,
    /// 1-based rank in the FTS list.
    pub fts_rank: Option<usize>,
    /// Raw bm25 score (lower = better, FTS5 convention).
    pub bm25: Option<f64>,
    /// 1-based rank in the vector list.
    pub vector_rank: Option<usize>,
    pub cosine: Option<f32>,
}

impl MatchInfo {
    /// Where the hit came from, for display ("why is this here").
    pub fn source(&self) -> HitSource {
        let text = self.exact || self.fts_rank.is_some();
        match (self.exact, text, self.vector_rank.is_some()) {
            (_, true, true) => HitSource::Hybrid,
            (true, _, false) => HitSource::Exact,
            (false, true, false) => HitSource::FullText,
            (_, false, true) => HitSource::Semantic,
            (_, false, false) => HitSource::FullText,
        }
    }
}

/// Summary of [`MatchInfo`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HitSource {
    Exact,
    FullText,
    Semantic,
    Hybrid,
}

/// A ranked result.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SearchHit {
    pub document: Document,
    /// Higher = better. bm25 is negated for text search; cosine for vector
    /// search; fused RRF score for hybrid search.
    pub score: f64,
    pub matched: MatchInfo,
}

/// Split free text into FTS terms (roughly mirrors the `unicode61` tokenizer:
/// alphanumeric runs, lowercased).
pub(crate) fn fts_terms(text: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for t in text.split(|c: char| !c.is_alphanumeric()) {
        if t.is_empty() {
            continue;
        }
        let t = t.to_lowercase();
        if !out.contains(&t) {
            out.push(t);
        }
    }
    out
}

/// Build an FTS5 MATCH expression. Terms are double-quoted (they only contain
/// alphanumerics, so no escaping is needed) which makes user input inert.
pub(crate) fn build_match(terms: &[String], any: bool, prefix: bool) -> String {
    let star = if prefix { "*" } else { "" };
    let sep = if any { " OR " } else { " " };
    terms
        .iter()
        .map(|t| format!("\"{t}\"{star}"))
        .collect::<Vec<_>>()
        .join(sep)
}

/// ASCII case-insensitive "exact" check used for [`MatchInfo::exact`].
pub(crate) fn is_exact(doc: &Document, query: &str) -> bool {
    let q = query.trim();
    if q.is_empty() {
        return false;
    }
    doc.title.trim().eq_ignore_ascii_case(q)
        || doc
            .body
            .to_ascii_lowercase()
            .contains(&q.to_ascii_lowercase())
}

/// Weighted reciprocal rank fusion: `score(d) = Σ wᵢ / (k + rankᵢ(d))`.
/// `lists` are ordered best-first; returns `(key, score)` best-first, ties
/// broken by first appearance.
pub fn reciprocal_rank_fusion<K: Clone + Eq + std::hash::Hash>(
    lists: &[(f64, Vec<K>)],
    k: f64,
) -> Vec<(K, f64)> {
    use std::collections::HashMap;
    let mut scores: HashMap<K, (f64, usize)> = HashMap::new();
    let mut order = 0usize;
    for (weight, list) in lists {
        for (rank0, key) in list.iter().enumerate() {
            let contrib = weight / (k + (rank0 as f64 + 1.0));
            let e = scores.entry(key.clone()).or_insert_with(|| {
                order += 1;
                (0.0, order)
            });
            e.0 += contrib;
        }
    }
    let mut out: Vec<(K, f64, usize)> = scores.into_iter().map(|(k, (s, o))| (k, s, o)).collect();
    out.sort_by(|a, b| b.1.total_cmp(&a.1).then(a.2.cmp(&b.2)));
    out.into_iter().map(|(k, s, _)| (k, s)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn terms_and_match_expression() {
        let t = fts_terms("kubectl  logs -n \"prod\" OR NEAR(x) kubectl*");
        assert_eq!(t, vec!["kubectl", "logs", "n", "prod", "or", "near", "x"]);
        assert_eq!(build_match(&t[..2], false, true), "\"kubectl\"* \"logs\"*");
        assert_eq!(build_match(&t[..2], true, false), "\"kubectl\" OR \"logs\"");
        assert!(fts_terms("  -- ** ").is_empty());
    }

    #[test]
    fn rrf_prefers_items_in_both_lists() {
        let fused = reciprocal_rank_fusion(
            &[(1.0, vec!["a", "b", "c"]), (1.0, vec!["c", "d", "a"])],
            60.0,
        );
        let keys: Vec<&str> = fused.iter().map(|x| x.0).collect();
        // a: 1/61 + 1/63, c: 1/63 + 1/61 → tie broken by first appearance.
        assert_eq!(keys[..2], ["a", "c"]);
        assert_eq!(keys.len(), 4);
        assert!(fused[0].1 > fused[2].1);
    }

    #[test]
    fn source_summary() {
        let mut m = MatchInfo {
            fts_rank: Some(1),
            ..Default::default()
        };
        assert_eq!(m.source(), HitSource::FullText);
        m.vector_rank = Some(2);
        assert_eq!(m.source(), HitSource::Hybrid);
        let s = MatchInfo {
            vector_rank: Some(1),
            ..Default::default()
        };
        assert_eq!(s.source(), HitSource::Semantic);
        let e = MatchInfo {
            exact: true,
            fts_rank: Some(1),
            ..Default::default()
        };
        assert_eq!(e.source(), HitSource::Exact);
        let e2 = MatchInfo {
            exact: true,
            ..Default::default()
        };
        assert_eq!(e2.source(), HitSource::Exact);
    }
}
