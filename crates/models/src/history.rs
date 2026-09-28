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
