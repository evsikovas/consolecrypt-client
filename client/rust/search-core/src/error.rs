//! Error type for the search index.

/// Errors returned by [`crate::SearchIndex`]. Messages never contain document
/// content or key material.
#[derive(Debug, thiserror::Error)]
pub enum SearchError {
    /// Underlying SQLite/SQLCipher failure.
    #[error("search index database error: {0}")]
    Db(#[from] rusqlite::Error),
    /// The database file could not be decrypted with the given key or is not
    /// a database. The index is rebuildable: use
    /// [`crate::SearchIndex::open_or_recreate`] to start over.
    #[error("search index is unreadable (wrong key or corrupted file)")]
    Unreadable,
    /// I/O error while (re)creating the index file.
    #[error("search index I/O error: {0}")]
    Io(#[from] std::io::Error),
    /// No embedding model configured; call
    /// [`crate::SearchIndex::set_embedding_model`] first.
    #[error("no embedding model configured")]
    NoEmbeddingModel,
    /// Vector length does not match the configured model dimension.
    #[error("embedding dimension mismatch: expected {expected}, got {actual}")]
    DimensionMismatch { expected: usize, actual: usize },
    /// Vector contains NaN or infinite components.
    #[error("embedding contains non-finite values")]
    NonFiniteEmbedding,
    /// Invalid argument (e.g. empty model id, zero dimension).
    #[error("invalid argument: {0}")]
    InvalidArgument(&'static str),
    /// Internal lock was poisoned by a panic in another thread.
    #[error("search index lock poisoned")]
    Poisoned,
}

/// Convenience alias.
pub type Result<T, E = SearchError> = std::result::Result<T, E>;
