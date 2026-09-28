//! Embedding encoding, cosine similarity and an in-memory brute-force cache.

use crate::document::DocKind;
use cc_models::ObjectId;
use serde::{Deserialize, Serialize};

/// Identity of the embedding model that produced the stored vectors. When it
/// changes, every stored vector is dropped and documents are re-embedded.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct EmbeddingModel {
    /// Stable identifier, e.g. `"ollama:nomic-embed-text"`.
    pub id: String,
    /// Vector dimension.
    pub dim: usize,
}

impl EmbeddingModel {
    pub fn new(id: impl Into<String>, dim: usize) -> Self {
        Self { id: id.into(), dim }
    }
}

/// Encode as little-endian f32 bytes.
pub(crate) fn encode(v: &[f32]) -> Vec<u8> {
    let mut out = Vec::with_capacity(v.len() * 4);
    for x in v {
        out.extend_from_slice(&x.to_le_bytes());
    }
    out
}

/// Decode little-endian f32 bytes; `None` if the length is not a multiple of 4.
pub(crate) fn decode(b: &[u8]) -> Option<Vec<f32>> {
    let (chunks, rest) = b.as_chunks::<4>();
    if !rest.is_empty() {
        return None;
    }
    Some(chunks.iter().map(|c| f32::from_le_bytes(*c)).collect())
}

/// L2-normalize in place; returns `false` for a zero vector (left unchanged).
pub(crate) fn normalize(v: &mut [f32]) -> bool {
    let norm = v.iter().map(|x| x * x).sum::<f32>().sqrt();
    if norm <= f32::EPSILON {
        return false;
    }
    for x in v.iter_mut() {
        *x /= norm;
    }
    true
}

/// Cosine similarity of two vectors (0 if either is zero or lengths differ).
pub fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() || a.is_empty() {
        return 0.0;
    }
    let (mut dot, mut na, mut nb) = (0.0f32, 0.0f32, 0.0f32);
    for (x, y) in a.iter().zip(b) {
        dot += x * y;
        na += x * x;
        nb += y * y;
    }
    if na <= f32::EPSILON || nb <= f32::EPSILON {
        return 0.0;
    }
    dot / (na.sqrt() * nb.sqrt())
}

#[derive(Debug, Clone)]
pub(crate) struct CacheEntry {
    pub rowid: i64,
    pub id: ObjectId,
    pub kind: DocKind,
}

/// All vectors of the active model, normalized, stored contiguously.
#[derive(Debug)]
pub(crate) struct VectorCache {
    pub model: EmbeddingModel,
    pub entries: Vec<CacheEntry>,
    data: Vec<f32>,
}

impl VectorCache {
    pub fn new(model: EmbeddingModel) -> Self {
        Self {
            model,
            entries: Vec::new(),
            data: Vec::new(),
        }
    }

    pub fn push(&mut self, entry: CacheEntry, mut v: Vec<f32>) {
        debug_assert_eq!(v.len(), self.model.dim);
        normalize(&mut v);
        self.entries.push(entry);
        self.data.extend_from_slice(&v);
    }

    /// Brute-force top-k by cosine similarity. `accept` filters entries.
    pub fn top_k(
        &self,
        query: &[f32],
        k: usize,
        min_similarity: f32,
        accept: impl Fn(&CacheEntry) -> bool,
    ) -> Vec<(usize, f32)> {
        let dim = self.model.dim;
        if k == 0 || query.len() != dim || dim == 0 {
            return Vec::new();
        }
        let mut q = query.to_vec();
        if !normalize(&mut q) {
            return Vec::new();
        }
        let mut scored: Vec<(usize, f32)> = self
            .entries
            .iter()
            .enumerate()
            .filter(|(_, e)| accept(e))
            .map(|(i, _)| {
                let v = &self.data[i * dim..(i + 1) * dim];
                let s: f32 = v.iter().zip(&q).map(|(a, b)| a * b).sum();
                (i, s)
            })
            .filter(|(_, s)| *s >= min_similarity)
            .collect();
        let by_score = |a: &(usize, f32), b: &(usize, f32)| b.1.total_cmp(&a.1);
        if scored.len() > k {
            scored.select_nth_unstable_by(k - 1, by_score);
            scored.truncate(k);
        }
        scored.sort_by(by_score);
        scored
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encode_decode_roundtrip() {
        let v = vec![0.0, -1.5, 3.25, f32::MIN_POSITIVE];
        assert_eq!(decode(&encode(&v)).unwrap(), v);
        assert!(decode(&[1, 2, 3]).is_none());
    }

    #[test]
    fn cosine() {
        assert!((cosine_similarity(&[1.0, 0.0], &[2.0, 0.0]) - 1.0).abs() < 1e-6);
        assert!(cosine_similarity(&[1.0, 0.0], &[0.0, 1.0]).abs() < 1e-6);
        assert!((cosine_similarity(&[1.0, 0.0], &[-1.0, 0.0]) + 1.0).abs() < 1e-6);
        assert_eq!(cosine_similarity(&[0.0, 0.0], &[1.0, 0.0]), 0.0);
        assert_eq!(cosine_similarity(&[1.0], &[1.0, 0.0]), 0.0);
    }

    #[test]
    fn top_k_orders_by_similarity() {
        let mut c = VectorCache::new(EmbeddingModel::new("m", 2));
        for (i, v) in [[1.0, 0.0], [0.7, 0.7], [0.0, 1.0], [-1.0, 0.0]]
            .iter()
            .enumerate()
        {
            c.push(
                CacheEntry {
                    rowid: i as i64,
                    id: ObjectId::new(),
                    kind: DocKind::Note,
                },
                v.to_vec(),
            );
        }
        let top = c.top_k(&[1.0, 0.1], 2, -1.0, |_| true);
        assert_eq!(top.iter().map(|t| t.0).collect::<Vec<_>>(), vec![0, 1]);
        let all = c.top_k(&[1.0, 0.0], 10, 0.5, |_| true);
        assert_eq!(all.len(), 2);
        assert!(c.top_k(&[0.0, 0.0], 2, -1.0, |_| true).is_empty());
    }
}
