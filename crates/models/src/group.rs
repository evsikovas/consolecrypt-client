//! Host groups with inheritable connection defaults.

use crate::{ObjectId, Timestamp};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Group {
    pub id: ObjectId,
    pub name: String,
    #[serde(default)]
    pub parent_id: Option<ObjectId>,
    #[serde(default)]
    pub inherited_username: Option<String>,
    #[serde(default)]
    pub inherited_port: Option<u16>,
    #[serde(default)]
    pub inherited_credential_id: Option<ObjectId>,
    #[serde(default)]
    pub inherited_jump_profile_id: Option<ObjectId>,
    #[serde(default)]
    pub tags: Vec<String>,
    pub created_at: Timestamp,
    pub updated_at: Timestamp,
}

impl Group {
    pub fn new(name: impl Into<String>) -> Self {
        let now = chrono::Utc::now();
        Self {
            id: ObjectId::new(),
            name: name.into(),
            parent_id: None,
            inherited_username: None,
            inherited_port: None,
            inherited_credential_id: None,
            inherited_jump_profile_id: None,
            tags: Vec::new(),
            created_at: now,
            updated_at: now,
        }
    }
}
