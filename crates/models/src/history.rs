//! Terminal command history. Synced only when
//! [`crate::settings::TerminalHistoryMode::EncryptedSync`] is selected;
//! otherwise it stays in local storage and never becomes a vault object.

use crate::{ObjectId, Timestamp};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HistoryEntry {
    pub id: ObjectId,
    #[serde(default)]
    pub host_id: Option<ObjectId>,
    pub command: String,
    #[serde(default)]
    pub exit_code: Option<i32>,
    pub executed_at: Timestamp,
}

/// Database query history contains the source template, never bound parameter
/// values, connection credentials, result rows, or raw database error messages.
/// It becomes a vault object only after an explicit encrypted-sync preference.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DbHistoryEntry {
    pub id: ObjectId,
    pub connection_id: ObjectId,
    pub engine: crate::database::DbEngine,
    pub text: crate::secret::SecretValue,
    pub created_at: Timestamp,
    pub elapsed_ms: u64,
    pub rows: u64,
    pub error_code: Option<String>,
    #[serde(default)]
    pub favorite: bool,
    /// A change made in a production session during its temporary write
    /// window (absent in entries written before windows existed).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub production_write: bool,
}
