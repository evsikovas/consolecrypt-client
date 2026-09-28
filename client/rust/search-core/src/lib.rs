//! Local-only search index: SQLite FTS5 over snippets/notes/history/hosts,
//! an embeddings store and brute-force vector search, fused with reciprocal
//! rank fusion. Rebuilt locally after login; **never synced** (CLIENT_SPEC
//! §12.3).
//!
//! * Storage: its own SQLite file ([`SearchIndex::open`], optionally
//!   SQLCipher-encrypted with an [`IndexKey`]) or in memory
//!   ([`SearchIndex::open_in_memory`]).
//! * Full text: FTS5 (`unicode61`, diacritics folded, prefix indexes), bm25
//!   ranking with title/tags boosted, prefix search, kind and tag filters
//!   ([`SearchIndex::search_text`]).
//! * Vectors: f32 BLOBs tagged with the model id and dimension; cosine top-k
//!   over an in-memory cache ([`SearchIndex::search_vector`]). Changing the
//!   model ([`SearchIndex::set_embedding_model`]) drops all vectors, and
//!   [`SearchIndex::pending_embeddings`] lists what must be (re-)embedded.
//! * Hybrid: [`SearchIndex::search_hybrid`] fuses exact, FTS and vector lists.
//!
//! ```
//! use cc_search_core::{Document, DocKind, SearchIndex, TextQuery};
//! use cc_models::ObjectId;
//!
//! let index = SearchIndex::open_in_memory().unwrap();
//! index.upsert(&Document::new(ObjectId::new(), DocKind::Snippet,
//!     "Tail pod logs", "kubectl logs -n {{namespace}} {{pod}} --tail=100")
//!     .with_tags(["k8s"])).unwrap();
//! let hits = index.search_text(&TextQuery::new("kub log", 10)).unwrap();
//! assert_eq!(hits[0].document.title, "Tail pod logs");
//! ```

mod document;
mod error;
mod index;
mod query;
mod vector;

pub use document::{normalize_tags, snippet_type_tag, DocKind, Document};
pub use error::{Result, SearchError};
pub use index::{
    EmbeddingJob, EmbeddingStats, IndexKey, RebuildStats, SearchIndex, UpsertOutcome,
    SCHEMA_VERSION,
};
pub use query::{
    reciprocal_rank_fusion, HitSource, HybridQuery, MatchInfo, MatchMode, SearchFilter, SearchHit,
    TextQuery, DEFAULT_RRF_K,
};
pub use vector::{cosine_similarity, EmbeddingModel};
