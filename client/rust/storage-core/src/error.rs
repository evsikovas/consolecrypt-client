//! Storage error type.

/// Errors returned by storage operations.
///
/// Messages never contain key material or object plaintext; SQLite errors
/// carry SQL text at most, never bound parameter values.
#[derive(Debug, thiserror::Error)]
pub enum StorageError {
    /// The file could not be decrypted with the supplied key, or it is not a
    /// ConsoleCrypt database at all.
    #[error("database is not readable with the provided key (wrong key or not a database)")]
    WrongKeyOrNotADatabase,
    /// The database was written by a newer client build (downgrade).
    #[error("database schema version {found} is newer than the supported version {supported}")]
    SchemaTooNew {
        /// Version found in the file.
        found: i64,
        /// Highest version this build knows.
        supported: i64,
    },
    /// Underlying SQLite / SQLCipher error.
    #[error("sqlite: {0}")]
    Sqlite(#[from] rusqlite::Error),
    /// A stored value could not be decoded (e.g. malformed UUID).
    #[error("corrupt value in {table}.{column}: {reason}")]
    Corrupt {
        /// Table name.
        table: &'static str,
        /// Column name.
        column: &'static str,
        /// What was wrong (never the value itself).
        reason: String,
    },
    /// JSON (de)serialization of a stored document failed.
    #[error("serialization: {0}")]
    Serde(#[from] serde_json::Error),
    /// The operation violates an invariant (caller bug or inconsistent input).
    #[error("invalid storage operation: {0}")]
    Invalid(String),
    /// The background storage thread is gone (database closed).
    #[error("storage worker has shut down")]
    Closed,
    /// Filesystem error.
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
}

impl StorageError {
    pub(crate) fn corrupt(
        table: &'static str,
        column: &'static str,
        reason: impl ToString,
    ) -> Self {
        StorageError::Corrupt {
            table,
            column,
            reason: reason.to_string(),
        }
    }
}

/// Result alias for storage operations.
pub type Result<T, E = StorageError> = std::result::Result<T, E>;
