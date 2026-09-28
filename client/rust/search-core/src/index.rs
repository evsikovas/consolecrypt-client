//! SQLite (SQLCipher) backed index: FTS5 + embeddings + hybrid ranking.

use crate::document::{DocKind, Document};
use crate::error::{Result, SearchError};
use crate::query::{
    build_match, fts_terms, is_exact, reciprocal_rank_fusion, HybridQuery, MatchInfo, MatchMode,
    SearchFilter, SearchHit, TextQuery,
};
use crate::vector::{decode, encode, CacheEntry, EmbeddingModel, VectorCache};
use cc_models::ObjectId;
use rusqlite::types::Value;
use rusqlite::{params, Connection, ErrorCode, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::fmt;
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::sync::{Mutex, MutexGuard};
use zeroize::Zeroizing;

/// On-disk schema version (`PRAGMA user_version`). A mismatch drops and
/// recreates the index (it is rebuildable) and sets [`SearchIndex::needs_rebuild`].
pub const SCHEMA_VERSION: i64 = 1;

/// bm25 column weights: title, body, tags.
const BM25_WEIGHTS: &str = "10.0, 1.0, 4.0";

const SCHEMA_SQL: &str = r#"
CREATE TABLE meta (key TEXT PRIMARY KEY, value TEXT NOT NULL) WITHOUT ROWID;
CREATE TABLE documents (
    pk INTEGER PRIMARY KEY,
    id TEXT NOT NULL UNIQUE,
    kind TEXT NOT NULL,
    title TEXT NOT NULL,
    body TEXT NOT NULL,
    tags TEXT NOT NULL,
    content_hash TEXT NOT NULL
);
CREATE INDEX documents_kind ON documents(kind);
CREATE TABLE document_tags (
    tag TEXT NOT NULL,
    doc_pk INTEGER NOT NULL REFERENCES documents(pk) ON DELETE CASCADE,
    PRIMARY KEY (tag, doc_pk)
) WITHOUT ROWID;
CREATE INDEX document_tags_doc ON document_tags(doc_pk);
CREATE VIRTUAL TABLE documents_fts USING fts5(
    title, body, tags,
    content='documents', content_rowid='pk',
    tokenize='unicode61 remove_diacritics 2',
    prefix='2 3'
);
CREATE TRIGGER documents_ai AFTER INSERT ON documents BEGIN
    INSERT INTO documents_fts(rowid, title, body, tags)
    VALUES (new.pk, new.title, new.body, new.tags);
END;
CREATE TRIGGER documents_ad AFTER DELETE ON documents BEGIN
    INSERT INTO documents_fts(documents_fts, rowid, title, body, tags)
    VALUES ('delete', old.pk, old.title, old.body, old.tags);
END;
CREATE TRIGGER documents_au AFTER UPDATE ON documents BEGIN
    INSERT INTO documents_fts(documents_fts, rowid, title, body, tags)
    VALUES ('delete', old.pk, old.title, old.body, old.tags);
    INSERT INTO documents_fts(rowid, title, body, tags)
    VALUES (new.pk, new.title, new.body, new.tags);
END;
CREATE TABLE embeddings (
    doc_id TEXT PRIMARY KEY,
    model_id TEXT NOT NULL,
    dim INTEGER NOT NULL,
    content_hash TEXT NOT NULL,
    vector BLOB NOT NULL
);
"#;

const DROP_SQL: &str = r#"
DROP TRIGGER IF EXISTS documents_ai;
DROP TRIGGER IF EXISTS documents_ad;
DROP TRIGGER IF EXISTS documents_au;
DROP TABLE IF EXISTS documents_fts;
DROP TABLE IF EXISTS document_tags;
DROP TABLE IF EXISTS embeddings;
DROP TABLE IF EXISTS documents;
DROP TABLE IF EXISTS meta;
"#;

const META_POPULATED: &str = "populated";
const META_MODEL_ID: &str = "embedding_model_id";
const META_MODEL_DIM: &str = "embedding_model_dim";

/// 32-byte SQLCipher raw key for the index file. Redacted `Debug`, zeroized
/// on drop. Typically a random key kept in the OS secure store by app-core.
pub struct IndexKey(Zeroizing<[u8; 32]>);

impl IndexKey {
    pub fn from_bytes(bytes: &[u8; 32]) -> Self {
        Self(Zeroizing::new(*bytes))
    }

    fn pragma_sql(&self) -> Zeroizing<String> {
        const HEX: &[u8; 16] = b"0123456789abcdef";
        let mut s = Zeroizing::new(String::with_capacity(80));
        s.push_str("PRAGMA key = \"x'");
        for b in self.0.iter() {
            s.push(HEX[usize::from(b >> 4)] as char);
            s.push(HEX[usize::from(b & 0x0f)] as char);
        }
        s.push_str("'\";");
        s
    }
}

impl fmt::Debug for IndexKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("IndexKey(<redacted>)")
    }
}

/// Result of [`SearchIndex::upsert`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UpsertOutcome {
    Inserted,
    Updated,
    /// Same content hash as stored; nothing written.
    Unchanged,
}

/// Result of [`SearchIndex::rebuild`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct RebuildStats {
    pub documents: usize,
    /// Embeddings kept because the document content did not change.
    pub embeddings_kept: usize,
}

/// A document that has no up-to-date embedding for the active model.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EmbeddingJob {
    pub document: Document,
    /// Pass back to [`SearchIndex::store_embedding`]; a mismatch (document
    /// changed meanwhile) makes the store a no-op.
    pub content_hash: String,
}

impl EmbeddingJob {
    /// Text to embed.
    pub fn text(&self) -> String {
        self.document.embedding_text()
    }
}

/// Coverage of embeddings for the active model.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct EmbeddingStats {
    pub documents: usize,
    pub embedded: usize,
}

/// Local-only, rebuildable search index. Never synced (CLIENT_SPEC §12.3).
///
/// All methods are synchronous and fast; async callers should wrap them in
/// `spawn_blocking`. The type is `Send + Sync` (internally a mutex-guarded
/// connection plus a lazily loaded vector cache).
pub struct SearchIndex {
    inner: Mutex<Inner>,
    path: Option<PathBuf>,
}

struct Inner {
    conn: Connection,
    cache: Option<VectorCache>,
}

impl fmt::Debug for SearchIndex {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SearchIndex")
            .field("in_memory", &self.path.is_none())
            .finish_non_exhaustive()
    }
}

#[derive(Debug)]
struct RawHit {
    pk: i64,
    doc: Document,
    bm25: Option<f64>,
    cosine: Option<f32>,
    exact_level: u8,
}

impl SearchIndex {
    /// Open (or create) the index file. With `key`, the file is a SQLCipher
    /// database; without, a plain SQLite file (e.g. when the whole profile
    /// directory is already protected).
    pub fn open(path: impl AsRef<Path>, key: Option<&IndexKey>) -> Result<Self> {
        let path = path.as_ref().to_path_buf();
        let conn = Connection::open(&path)?;
        let conn = Self::configure(conn, key, true)?;
        Ok(Self {
            inner: Mutex::new(Inner { conn, cache: None }),
            path: Some(path),
        })
    }

    /// Like [`SearchIndex::open`], but an unreadable file (wrong key,
    /// corruption) is deleted and recreated empty — the index is derived data.
    /// Check [`SearchIndex::needs_rebuild`] afterwards.
    pub fn open_or_recreate(path: impl AsRef<Path>, key: Option<&IndexKey>) -> Result<Self> {
        let path = path.as_ref();
        match Self::open(path, key) {
            Err(SearchError::Unreadable) => {
                tracing::warn!("search index unreadable; recreating");
                for suffix in ["", "-wal", "-shm", "-journal"] {
                    let mut p = path.as_os_str().to_owned();
                    p.push(suffix);
                    match std::fs::remove_file(PathBuf::from(p)) {
                        Ok(()) => {}
                        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                        Err(e) => return Err(e.into()),
                    }
                }
                Self::open(path, key)
            }
            other => other,
        }
    }

    /// In-memory index (tests, or when no profile directory exists).
    pub fn open_in_memory() -> Result<Self> {
        let conn = Connection::open_in_memory()?;
        let conn = Self::configure(conn, None, false)?;
        Ok(Self {
            inner: Mutex::new(Inner { conn, cache: None }),
            path: None,
        })
    }

    fn configure(conn: Connection, key: Option<&IndexKey>, file: bool) -> Result<Connection> {
        if let Some(k) = key {
            conn.execute_batch(&k.pragma_sql())
                .map_err(map_unreadable)?;
        }
        conn.query_row("SELECT count(*) FROM sqlite_master", [], |r| {
            r.get::<_, i64>(0)
        })
        .map_err(map_unreadable)?;
        conn.execute_batch("PRAGMA foreign_keys = ON; PRAGMA synchronous = NORMAL;")?;
        if file {
            conn.query_row("PRAGMA journal_mode = WAL", [], |_| Ok(()))?;
        }
        let version: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
        if version != SCHEMA_VERSION {
            if version != 0 {
                tracing::info!(
                    from = version,
                    to = SCHEMA_VERSION,
                    "search index schema changed; recreating"
                );
            }
            conn.execute_batch(DROP_SQL)?;
            conn.execute_batch(SCHEMA_SQL)?;
            conn.execute_batch(&format!("PRAGMA user_version = {SCHEMA_VERSION};"))?;
        }
        Ok(conn)
    }

    fn lock(&self) -> Result<MutexGuard<'_, Inner>> {
        self.inner.lock().map_err(|_| SearchError::Poisoned)
    }

    /// `true` until the first successful [`SearchIndex::rebuild`] on this
    /// file (fresh file, recreated file or schema change). After a new login
    /// app-core feeds all snippets/notes/history/hosts into `rebuild`.
    pub fn needs_rebuild(&self) -> Result<bool> {
        let g = self.lock()?;
        Ok(get_meta(&g.conn, META_POPULATED)?.as_deref() != Some("1"))
    }

    /// Insert or update a document (keyed by `id`).
    pub fn upsert(&self, doc: &Document) -> Result<UpsertOutcome> {
        let mut g = self.lock()?;
        let tx = g.conn.transaction()?;
        let out = upsert_in(&tx, doc)?;
        tx.commit()?;
        if out != UpsertOutcome::Unchanged {
            g.cache = None;
        }
        Ok(out)
    }

    /// Upsert many documents in one transaction; returns how many changed.
    pub fn upsert_many<'a, I>(&self, docs: I) -> Result<usize>
    where
        I: IntoIterator<Item = &'a Document>,
    {
        let mut g = self.lock()?;
        let tx = g.conn.transaction()?;
        let mut changed = 0;
        for d in docs {
            if upsert_in(&tx, d)? != UpsertOutcome::Unchanged {
                changed += 1;
            }
        }
        tx.commit()?;
        if changed > 0 {
            g.cache = None;
        }
        Ok(changed)
    }

    /// Remove a document and its embedding. Returns whether it existed.
    pub fn delete(&self, id: ObjectId) -> Result<bool> {
        let mut g = self.lock()?;
        let id = id.to_string();
        let tx = g.conn.transaction()?;
        let n = tx.execute("DELETE FROM documents WHERE id = ?1", params![id])?;
        tx.execute("DELETE FROM embeddings WHERE doc_id = ?1", params![id])?;
        tx.commit()?;
        g.cache = None;
        Ok(n > 0)
    }

    /// Fetch one document.
    pub fn get(&self, id: ObjectId) -> Result<Option<Document>> {
        let g = self.lock()?;
        g.conn
            .query_row(
                "SELECT pk, id, kind, title, body, tags FROM documents WHERE id = ?1",
                params![id.to_string()],
                row_to_doc,
            )
            .optional()?
            .map(|r| r.map(|(_, d)| d))
            .transpose()
    }

    /// Number of documents.
    pub fn len(&self) -> Result<usize> {
        let g = self.lock()?;
        let n: i64 = g
            .conn
            .query_row("SELECT count(*) FROM documents", [], |r| r.get(0))?;
        Ok(usize::try_from(n).unwrap_or(0))
    }

    /// Whether the index holds no documents.
    pub fn is_empty(&self) -> Result<bool> {
        Ok(self.len()? == 0)
    }

    /// Replace the whole corpus atomically. Embeddings of documents whose
    /// content did not change are kept (re-embedding is the expensive part);
    /// all others are dropped and show up in [`SearchIndex::pending_embeddings`].
    pub fn rebuild<I>(&self, docs: I) -> Result<RebuildStats>
    where
        I: IntoIterator<Item = Document>,
    {
        let mut g = self.lock()?;
        let tx = g.conn.transaction()?;
        tx.execute_batch(
            "DELETE FROM document_tags; DELETE FROM documents; \
             INSERT INTO documents_fts(documents_fts) VALUES('rebuild');",
        )?;
        for d in docs {
            upsert_in(&tx, &d)?;
        }
        tx.execute(
            "DELETE FROM embeddings WHERE NOT EXISTS (SELECT 1 FROM documents d \
             WHERE d.id = embeddings.doc_id AND d.content_hash = embeddings.content_hash)",
            [],
        )?;
        let documents: i64 = tx.query_row("SELECT count(*) FROM documents", [], |r| r.get(0))?;
        let kept: i64 = tx.query_row("SELECT count(*) FROM embeddings", [], |r| r.get(0))?;
        set_meta(&tx, META_POPULATED, "1")?;
        tx.commit()?;
        g.cache = None;
        let stats = RebuildStats {
            documents: usize::try_from(documents).unwrap_or(0),
            embeddings_kept: usize::try_from(kept).unwrap_or(0),
        };
        tracing::debug!(
            documents = stats.documents,
            embeddings_kept = stats.embeddings_kept,
            "search index rebuilt"
        );
        Ok(stats)
    }

    /// Merge FTS b-trees (call occasionally, e.g. after a rebuild).
    pub fn optimize(&self) -> Result<()> {
        let g = self.lock()?;
        g.conn
            .execute_batch("INSERT INTO documents_fts(documents_fts) VALUES('optimize');")?;
        Ok(())
    }

    /// Full-text search ranked by bm25 (title ×10, tags ×4, body ×1), with
    /// exact matches (title equal / phrase contained) first. An empty query
    /// with a filter lists matching documents by title.
    pub fn search_text(&self, q: &TextQuery) -> Result<Vec<SearchHit>> {
        let g = self.lock()?;
        let raw = text_search(&g.conn, q)?;
        Ok(raw
            .into_iter()
            .enumerate()
            .map(|(i, h)| SearchHit {
                score: h.bm25.map_or(0.0, |b| -b),
                matched: MatchInfo {
                    exact: h.exact_level > 0,
                    fts_rank: h.bm25.map(|_| i + 1),
                    bm25: h.bm25,
                    vector_rank: None,
                    cosine: None,
                },
                document: h.doc,
            })
            .collect())
    }

    /// Brute-force cosine top-k over the active model's vectors.
    pub fn search_vector(
        &self,
        embedding: &[f32],
        filter: &SearchFilter,
        limit: usize,
        min_similarity: f32,
    ) -> Result<Vec<SearchHit>> {
        let mut g = self.lock()?;
        let raw = vector_search(&mut g, embedding, filter, limit, min_similarity)?;
        Ok(raw
            .into_iter()
            .enumerate()
            .map(|(i, h)| SearchHit {
                score: f64::from(h.cosine.unwrap_or(0.0)),
                matched: MatchInfo {
                    exact: false,
                    fts_rank: None,
                    bm25: None,
                    vector_rank: Some(i + 1),
                    cosine: h.cosine,
                },
                document: h.doc,
            })
            .collect())
    }

    /// Hybrid ranking: exact (weight 2), FTS and vector lists fused with
    /// reciprocal rank fusion. Works text-only when `embedding` is `None` or
    /// no embedding model is configured.
    pub fn search_hybrid(&self, q: &HybridQuery) -> Result<Vec<SearchHit>> {
        let mut g = self.lock()?;
        let text_q = TextQuery {
            text: q.text.clone(),
            filter: q.filter.clone(),
            limit: q.candidates.max(q.limit),
            mode: MatchMode::AllThenAny,
            prefix: true,
        };
        let text_hits = if fts_terms(&q.text).is_empty() && q.embedding.is_some() {
            Vec::new()
        } else {
            text_search(&g.conn, &text_q)?
        };
        let vec_hits = match &q.embedding {
            Some(e) if current_model(&g.conn)?.is_some() => vector_search(
                &mut g,
                e,
                &q.filter,
                q.candidates.max(q.limit),
                q.min_similarity,
            )?,
            _ => Vec::new(),
        };

        let mut info: HashMap<i64, (Document, MatchInfo)> = HashMap::new();
        let mut exact_list = Vec::new();
        let mut fts_list = Vec::new();
        for (i, h) in text_hits.into_iter().enumerate() {
            if h.exact_level > 0 {
                exact_list.push(h.pk);
            }
            if h.bm25.is_some() {
                fts_list.push(h.pk);
            }
            let m = MatchInfo {
                exact: h.exact_level > 0,
                fts_rank: h.bm25.map(|_| i + 1),
                bm25: h.bm25,
                vector_rank: None,
                cosine: None,
            };
            info.insert(h.pk, (h.doc, m));
        }
        let mut vec_list = Vec::new();
        for (i, h) in vec_hits.into_iter().enumerate() {
            vec_list.push(h.pk);
            let e = info
                .entry(h.pk)
                .or_insert_with(|| (h.doc, MatchInfo::default()));
            e.1.vector_rank = Some(i + 1);
            e.1.cosine = h.cosine;
        }
        let fused = reciprocal_rank_fusion(
            &[(2.0, exact_list), (1.0, fts_list), (1.0, vec_list)],
            q.rrf_k,
        );
        Ok(fused
            .into_iter()
            .take(q.limit)
            .filter_map(|(pk, score)| {
                info.remove(&pk).map(|(document, matched)| SearchHit {
                    document,
                    score,
                    matched,
                })
            })
            .collect())
    }

    /// Active embedding model, if any.
    pub fn embedding_model(&self) -> Result<Option<EmbeddingModel>> {
        let g = self.lock()?;
        current_model(&g.conn)
    }

    /// Set the active embedding model. If it differs from the stored one (id
    /// or dimension), **all vectors are dropped** and every document becomes
    /// pending. Returns `true` when that happened.
    pub fn set_embedding_model(&self, model: &EmbeddingModel) -> Result<bool> {
        if model.id.trim().is_empty() {
            return Err(SearchError::InvalidArgument("empty embedding model id"));
        }
        if model.dim == 0 {
            return Err(SearchError::InvalidArgument("zero embedding dimension"));
        }
        let mut g = self.lock()?;
        if current_model(&g.conn)?.as_ref() == Some(model) {
            return Ok(false);
        }
        let tx = g.conn.transaction()?;
        tx.execute("DELETE FROM embeddings", [])?;
        set_meta(&tx, META_MODEL_ID, &model.id)?;
        set_meta(&tx, META_MODEL_DIM, &model.dim.to_string())?;
        tx.commit()?;
        g.cache = None;
        tracing::info!(dim = model.dim, "embedding model changed; vectors dropped");
        Ok(true)
    }

    /// Documents lacking an up-to-date vector for the active model.
    pub fn pending_embeddings(&self, limit: usize) -> Result<Vec<EmbeddingJob>> {
        let g = self.lock()?;
        let model = current_model(&g.conn)?.ok_or(SearchError::NoEmbeddingModel)?;
        let mut stmt = g.conn.prepare_cached(
            "SELECT d.pk, d.id, d.kind, d.title, d.body, d.tags, d.content_hash \
             FROM documents d LEFT JOIN embeddings e \
               ON e.doc_id = d.id AND e.content_hash = d.content_hash AND e.model_id = ?1 \
             WHERE e.doc_id IS NULL ORDER BY d.pk LIMIT ?2",
        )?;
        let rows = stmt.query_map(params![model.id, to_i64(limit)], |r| {
            let doc = row_to_doc(r)?;
            let hash: String = r.get(6)?;
            Ok((doc, hash))
        })?;
        let mut out = Vec::new();
        for row in rows {
            let (doc, content_hash) = row?;
            out.push(EmbeddingJob {
                document: doc?.1,
                content_hash,
            });
        }
        Ok(out)
    }

    /// Store a vector for a document. Returns `Ok(false)` (nothing stored)
    /// when the document was deleted or changed since the job was created.
    pub fn store_embedding(
        &self,
        id: ObjectId,
        content_hash: &str,
        vector: &[f32],
    ) -> Result<bool> {
        self.store_embeddings(&[(id, content_hash.to_owned(), vector.to_vec())])
            .map(|n| n == 1)
    }

    /// Batch variant of [`SearchIndex::store_embedding`] (one transaction);
    /// returns how many vectors were stored.
    pub fn store_embeddings(&self, items: &[(ObjectId, String, Vec<f32>)]) -> Result<usize> {
        let mut g = self.lock()?;
        let model = current_model(&g.conn)?.ok_or(SearchError::NoEmbeddingModel)?;
        for (_, _, v) in items {
            if v.len() != model.dim {
                return Err(SearchError::DimensionMismatch {
                    expected: model.dim,
                    actual: v.len(),
                });
            }
            if v.iter().any(|x| !x.is_finite()) {
                return Err(SearchError::NonFiniteEmbedding);
            }
        }
        let tx = g.conn.transaction()?;
        let mut stored = 0;
        {
            let mut check =
                tx.prepare_cached("SELECT 1 FROM documents WHERE id = ?1 AND content_hash = ?2")?;
            let mut put = tx.prepare_cached(
                "INSERT INTO embeddings(doc_id, model_id, dim, content_hash, vector) \
                 VALUES (?1, ?2, ?3, ?4, ?5) \
                 ON CONFLICT(doc_id) DO UPDATE SET model_id = excluded.model_id, \
                 dim = excluded.dim, content_hash = excluded.content_hash, vector = excluded.vector",
            )?;
            for (id, hash, v) in items {
                let id = id.to_string();
                let fresh = check
                    .query_row(params![id, hash], |_| Ok(()))
                    .optional()?
                    .is_some();
                if !fresh {
                    continue;
                }
                put.execute(params![id, model.id, to_i64(model.dim), hash, encode(v)])?;
                stored += 1;
            }
        }
        tx.commit()?;
        if stored > 0 {
            g.cache = None;
        }
        Ok(stored)
    }

    /// Embedding coverage for the active model.
    pub fn embedding_stats(&self) -> Result<EmbeddingStats> {
        let g = self.lock()?;
        let documents: i64 = g
            .conn
            .query_row("SELECT count(*) FROM documents", [], |r| r.get(0))?;
        let embedded: i64 = match current_model(&g.conn)? {
            None => 0,
            Some(m) => g.conn.query_row(
                "SELECT count(*) FROM embeddings e JOIN documents d \
                 ON d.id = e.doc_id AND d.content_hash = e.content_hash WHERE e.model_id = ?1",
                params![m.id],
                |r| r.get(0),
            )?,
        };
        Ok(EmbeddingStats {
            documents: usize::try_from(documents).unwrap_or(0),
            embedded: usize::try_from(embedded).unwrap_or(0),
        })
    }
}

fn map_unreadable(e: rusqlite::Error) -> SearchError {
    match &e {
        rusqlite::Error::SqliteFailure(f, _)
            if matches!(f.code, ErrorCode::NotADatabase | ErrorCode::DatabaseCorrupt) =>
        {
            SearchError::Unreadable
        }
        _ => SearchError::Db(e),
    }
}

fn to_i64(n: usize) -> i64 {
    i64::try_from(n).unwrap_or(i64::MAX)
}

fn get_meta(conn: &Connection, key: &str) -> Result<Option<String>> {
    Ok(conn
        .query_row("SELECT value FROM meta WHERE key = ?1", params![key], |r| {
            r.get(0)
        })
        .optional()?)
}

fn set_meta(conn: &Connection, key: &str, value: &str) -> Result<()> {
    conn.execute(
        "INSERT INTO meta(key, value) VALUES (?1, ?2) \
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        params![key, value],
    )?;
    Ok(())
}

fn current_model(conn: &Connection) -> Result<Option<EmbeddingModel>> {
    let id = get_meta(conn, META_MODEL_ID)?;
    let dim = get_meta(conn, META_MODEL_DIM)?.and_then(|d| d.parse::<usize>().ok());
    Ok(match (id, dim) {
        (Some(id), Some(dim)) if dim > 0 => Some(EmbeddingModel { id, dim }),
        _ => None,
    })
}

type DocRow = std::result::Result<(i64, Document), SearchError>;

/// Columns: pk, id, kind, title, body, tags (in this order, starting at 0).
fn row_to_doc(r: &rusqlite::Row<'_>) -> rusqlite::Result<DocRow> {
    let pk: i64 = r.get(0)?;
    let id: String = r.get(1)?;
    let kind: String = r.get(2)?;
    let title: String = r.get(3)?;
    let body: String = r.get(4)?;
    let tags: String = r.get(5)?;
    let parsed = (|| {
        let id = ObjectId::from_str(&id).map_err(|_| SearchError::Unreadable)?;
        let kind = DocKind::parse(&kind).ok_or(SearchError::Unreadable)?;
        Ok((
            pk,
            Document {
                id,
                kind,
                title,
                body,
                tags: tags.split_whitespace().map(str::to_owned).collect(),
            },
        ))
    })();
    Ok(parsed)
}

fn upsert_in(conn: &Connection, doc: &Document) -> Result<UpsertOutcome> {
    let doc = doc.normalized();
    let hash = doc.content_hash();
    let id = doc.id.to_string();
    let tags = doc.tags.join(" ");
    let existing: Option<(i64, String)> = conn
        .query_row(
            "SELECT pk, content_hash FROM documents WHERE id = ?1",
            params![id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?;
    let (pk, outcome) = match existing {
        Some((_, h)) if h == hash => return Ok(UpsertOutcome::Unchanged),
        Some((pk, _)) => {
            conn.execute(
                "UPDATE documents SET kind = ?2, title = ?3, body = ?4, tags = ?5, \
                 content_hash = ?6 WHERE pk = ?1",
                params![pk, doc.kind.as_str(), doc.title, doc.body, tags, hash],
            )?;
            conn.execute("DELETE FROM document_tags WHERE doc_pk = ?1", params![pk])?;
            conn.execute(
                "DELETE FROM embeddings WHERE doc_id = ?1 AND content_hash != ?2",
                params![id, hash],
            )?;
            (pk, UpsertOutcome::Updated)
        }
        None => {
            conn.execute(
                "INSERT INTO documents(id, kind, title, body, tags, content_hash) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![id, doc.kind.as_str(), doc.title, doc.body, tags, hash],
            )?;
            (conn.last_insert_rowid(), UpsertOutcome::Inserted)
        }
    };
    let mut put =
        conn.prepare_cached("INSERT OR IGNORE INTO document_tags(tag, doc_pk) VALUES (?1, ?2)")?;
    for t in &doc.tags {
        put.execute(params![t, pk])?;
    }
    Ok(outcome)
}

/// Append kind/tag filter clauses for alias `d`.
fn push_filter(sql: &mut String, params: &mut Vec<Value>, filter: &SearchFilter) {
    if !filter.kinds.is_empty() {
        let mut kinds: Vec<&str> = filter.kinds.iter().map(|k| k.as_str()).collect();
        kinds.sort_unstable();
        kinds.dedup();
        sql.push_str(" AND d.kind IN (");
        sql.push_str(&vec!["?"; kinds.len()].join(","));
        sql.push(')');
        params.extend(kinds.into_iter().map(|k| Value::Text(k.to_owned())));
    }
    let tags = filter.normalized_tags();
    if !tags.is_empty() {
        sql.push_str(" AND d.pk IN (SELECT doc_pk FROM document_tags WHERE tag IN (");
        sql.push_str(&vec!["?"; tags.len()].join(","));
        sql.push_str(") GROUP BY doc_pk HAVING count(*) = ?)");
        let n = tags.len();
        params.extend(tags.into_iter().map(Value::Text));
        params.push(Value::Integer(to_i64(n)));
    }
}

fn collect_docs(
    conn: &Connection,
    sql: &str,
    params: Vec<Value>,
    with_score: bool,
) -> Result<Vec<(i64, Document, Option<f64>)>> {
    let mut stmt = conn.prepare(sql)?;
    let rows = stmt.query_map(rusqlite::params_from_iter(params), |r| {
        let doc = row_to_doc(r)?;
        let score: Option<f64> = if with_score { Some(r.get(6)?) } else { None };
        Ok((doc, score))
    })?;
    let mut out = Vec::new();
    for row in rows {
        let (doc, score) = row?;
        let (pk, doc) = doc?;
        out.push((pk, doc, score));
    }
    Ok(out)
}

fn exact_level(doc: &Document, q: &str) -> u8 {
    if doc.title.trim().eq_ignore_ascii_case(q.trim()) && !q.trim().is_empty() {
        2
    } else if is_exact(doc, q) {
        1
    } else {
        0
    }
}

fn text_search(conn: &Connection, q: &TextQuery) -> Result<Vec<RawHit>> {
    if q.limit == 0 {
        return Ok(Vec::new());
    }
    let terms = fts_terms(&q.text);
    let limit = to_i64(q.limit);
    let rows = if terms.is_empty() {
        let text = q.text.trim();
        let mut params = Vec::new();
        let mut sql = String::from(
            "SELECT d.pk, d.id, d.kind, d.title, d.body, d.tags FROM documents d WHERE 1 = 1",
        );
        if !text.is_empty() {
            // Punctuation-only query (e.g. `|`, `>>`): substring scan.
            sql.push_str(
                " AND (instr(lower(d.title), lower(?)) > 0 OR instr(lower(d.body), lower(?)) > 0)",
            );
            params.push(Value::Text(text.to_owned()));
            params.push(Value::Text(text.to_owned()));
        } else if q.filter.kinds.is_empty() && q.filter.tags.is_empty() {
            return Ok(Vec::new());
        }
        push_filter(&mut sql, &mut params, &q.filter);
        sql.push_str(" ORDER BY d.title COLLATE NOCASE LIMIT ?");
        params.push(Value::Integer(limit));
        collect_docs(conn, &sql, params, false)?
    } else {
        let run = |any: bool| -> Result<Vec<(i64, Document, Option<f64>)>> {
            let mut sql = format!(
                "SELECT d.pk, d.id, d.kind, d.title, d.body, d.tags, \
                 bm25(documents_fts, {BM25_WEIGHTS}) AS score \
                 FROM documents_fts JOIN documents d ON d.pk = documents_fts.rowid \
                 WHERE documents_fts MATCH ?"
            );
            let mut params = vec![Value::Text(build_match(&terms, any, q.prefix))];
            push_filter(&mut sql, &mut params, &q.filter);
            sql.push_str(" ORDER BY score LIMIT ?");
            params.push(Value::Integer(limit));
            collect_docs(conn, &sql, params, true)
        };
        match q.mode {
            MatchMode::All => run(false)?,
            MatchMode::Any => run(true)?,
            MatchMode::AllThenAny => {
                let r = run(false)?;
                if r.is_empty() && terms.len() > 1 {
                    run(true)?
                } else {
                    r
                }
            }
        }
    };
    let mut hits: Vec<RawHit> = rows
        .into_iter()
        .map(|(pk, doc, bm25)| RawHit {
            exact_level: exact_level(&doc, &q.text),
            pk,
            doc,
            bm25,
            cosine: None,
        })
        .collect();
    // Stable: exact level first, then the SQL order (bm25 / title).
    hits.sort_by_key(|h| std::cmp::Reverse(h.exact_level));
    Ok(hits)
}

fn ensure_cache<'a>(g: &'a mut Inner, model: &EmbeddingModel) -> Result<&'a VectorCache> {
    let stale = g.cache.as_ref().is_none_or(|c| &c.model != model);
    if stale {
        let mut cache = VectorCache::new(model.clone());
        let mut stmt = g.conn.prepare(
            "SELECT d.pk, d.id, d.kind, e.vector FROM embeddings e \
             JOIN documents d ON d.id = e.doc_id AND d.content_hash = e.content_hash \
             WHERE e.model_id = ?1 AND e.dim = ?2",
        )?;
        let rows = stmt.query_map(params![model.id, to_i64(model.dim)], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, Vec<u8>>(3)?,
            ))
        })?;
        for row in rows {
            let (pk, id, kind, blob) = row?;
            let (Ok(id), Some(kind), Some(v)) = (
                ObjectId::from_str(&id),
                DocKind::parse(&kind),
                decode(&blob),
            ) else {
                continue;
            };
            if v.len() == model.dim {
                cache.push(
                    CacheEntry {
                        rowid: pk,
                        id,
                        kind,
                    },
                    v,
                );
            }
        }
        drop(stmt);
        g.cache = Some(cache);
    }
    g.cache.as_ref().ok_or(SearchError::Poisoned)
}

fn vector_search(
    g: &mut Inner,
    embedding: &[f32],
    filter: &SearchFilter,
    limit: usize,
    min_similarity: f32,
) -> Result<Vec<RawHit>> {
    let model = current_model(&g.conn)?.ok_or(SearchError::NoEmbeddingModel)?;
    if embedding.len() != model.dim {
        return Err(SearchError::DimensionMismatch {
            expected: model.dim,
            actual: embedding.len(),
        });
    }
    let tags = filter.normalized_tags();
    let allowed: Option<HashSet<i64>> = if tags.is_empty() {
        None
    } else {
        let mut sql = String::from("SELECT doc_pk FROM document_tags WHERE tag IN (");
        sql.push_str(&vec!["?"; tags.len()].join(","));
        sql.push_str(") GROUP BY doc_pk HAVING count(*) = ?");
        let n = tags.len();
        let mut params: Vec<Value> = tags.into_iter().map(Value::Text).collect();
        params.push(Value::Integer(to_i64(n)));
        let mut stmt = g.conn.prepare(&sql)?;
        let set = stmt
            .query_map(rusqlite::params_from_iter(params), |r| r.get::<_, i64>(0))?
            .collect::<rusqlite::Result<HashSet<i64>>>()?;
        Some(set)
    };
    let cache = ensure_cache(g, &model)?;
    let top = cache.top_k(embedding, limit, min_similarity, |e| {
        filter.accepts_kind(e.kind) && allowed.as_ref().is_none_or(|s| s.contains(&e.rowid))
    });
    let picked: Vec<(i64, ObjectId, f32)> = top
        .into_iter()
        .map(|(i, s)| (cache.entries[i].rowid, cache.entries[i].id, s))
        .collect();
    if picked.is_empty() {
        return Ok(Vec::new());
    }
    let mut sql = String::from(
        "SELECT d.pk, d.id, d.kind, d.title, d.body, d.tags FROM documents d WHERE d.pk IN (",
    );
    sql.push_str(&vec!["?"; picked.len()].join(","));
    sql.push(')');
    let params: Vec<Value> = picked.iter().map(|p| Value::Integer(p.0)).collect();
    let mut docs: HashMap<i64, Document> = collect_docs(&g.conn, &sql, params, false)?
        .into_iter()
        .map(|(pk, d, _)| (pk, d))
        .collect();
    Ok(picked
        .into_iter()
        .filter_map(|(pk, _, s)| {
            docs.remove(&pk).map(|doc| RawHit {
                pk,
                doc,
                bm25: None,
                cosine: Some(s),
                exact_level: 0,
            })
        })
        .collect())
}
