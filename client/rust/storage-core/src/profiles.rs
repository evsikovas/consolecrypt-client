//! On-disk profile layout (ADR-0106, CLIENT_ARCHITECTURE §4).
//!
//! ```text
//! <root>/profiles.json                  index: ids, display names, kinds (no secrets)
//! <root>/profiles/<profile_id>/vault.db one SQLCipher database per profile
//! ```
//!
//! The index lets the UI list profiles without opening (and thus without
//! fetching the keys of) every database. It never contains keys, server
//! URLs, emails, envelopes or object data. Each database key lives in the OS
//! secure store under a name derived from the profile id (app-core's job).
//!
//! All methods do small blocking file I/O; call them from `spawn_blocking`
//! if needed.

use crate::error::{Result, StorageError};
use crate::model::{ProfileId, ProfileKind};
use cc_protocol::Timestamp;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

const INDEX_FILE: &str = "profiles.json";
const INDEX_VERSION: u32 = 1;
/// File name of a profile database inside its directory.
pub const PROFILE_DB_FILE: &str = "vault.db";

/// Non-secret metadata of a profile, as kept in the index.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProfileEntry {
    pub profile_id: ProfileId,
    pub display_name: String,
    pub kind: ProfileKind,
    pub created_at: Timestamp,
    #[serde(default)]
    pub last_opened_at: Option<Timestamp>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct Index {
    version: u32,
    #[serde(default)]
    active: Option<ProfileId>,
    #[serde(default)]
    profiles: Vec<ProfileEntry>,
}

/// Manages the profiles directory and its index.
#[derive(Debug, Clone)]
pub struct ProfileDirectory {
    root: PathBuf,
}

impl ProfileDirectory {
    /// Use `root` (e.g. the app data directory) as the profiles root.
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    /// Root directory.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Directory of one profile.
    pub fn profile_dir(&self, id: ProfileId) -> PathBuf {
        self.root.join("profiles").join(id.to_string())
    }

    /// Database path of one profile (pass to [`crate::Storage::open`]).
    pub fn db_path(&self, id: ProfileId) -> PathBuf {
        self.profile_dir(id).join(PROFILE_DB_FILE)
    }

    fn index_path(&self) -> PathBuf {
        self.root.join(INDEX_FILE)
    }

    fn load(&self) -> Result<Index> {
        match std::fs::read(self.index_path()) {
            Ok(bytes) => {
                let idx: Index = serde_json::from_slice(&bytes)
                    .map_err(|e| StorageError::corrupt("profiles.json", "index", e))?;
                if idx.version > INDEX_VERSION {
                    return Err(StorageError::Invalid(
                        "profiles index written by a newer version".into(),
                    ));
                }
                Ok(idx)
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Index {
                version: INDEX_VERSION,
                ..Index::default()
            }),
            Err(e) => Err(e.into()),
        }
    }

    fn store(&self, idx: &Index) -> Result<()> {
        std::fs::create_dir_all(&self.root)?;
        let tmp = self.root.join(format!("{INDEX_FILE}.tmp"));
        let bytes = serde_json::to_vec_pretty(idx)?;
        {
            use std::io::Write;
            let mut f = std::fs::File::create(&tmp)?;
            f.write_all(&bytes)?;
            f.sync_all()?;
        }
        std::fs::rename(&tmp, self.index_path())?;
        Ok(())
    }

    /// All profiles, in creation order.
    pub fn list(&self) -> Result<Vec<ProfileEntry>> {
        Ok(self.load()?.profiles)
    }

    /// One profile.
    pub fn get(&self, id: ProfileId) -> Result<Option<ProfileEntry>> {
        Ok(self
            .load()?
            .profiles
            .into_iter()
            .find(|p| p.profile_id == id))
    }

    /// Register a new profile and create its directory.
    pub fn create(
        &self,
        display_name: impl Into<String>,
        kind: ProfileKind,
    ) -> Result<ProfileEntry> {
        let mut idx = self.load()?;
        let entry = ProfileEntry {
            profile_id: ProfileId::new(),
            display_name: display_name.into(),
            kind,
            created_at: chrono::Utc::now(),
            last_opened_at: None,
        };
        std::fs::create_dir_all(self.profile_dir(entry.profile_id))?;
        idx.version = INDEX_VERSION;
        idx.profiles.push(entry.clone());
        self.store(&idx)?;
        Ok(entry)
    }

    /// Insert or replace an entry (e.g. kind changed after enable-sync).
    pub fn update(&self, entry: &ProfileEntry) -> Result<()> {
        let mut idx = self.load()?;
        match idx
            .profiles
            .iter_mut()
            .find(|p| p.profile_id == entry.profile_id)
        {
            Some(p) => *p = entry.clone(),
            None => idx.profiles.push(entry.clone()),
        }
        idx.version = INDEX_VERSION;
        self.store(&idx)
    }

    /// Remember the active profile.
    pub fn set_active(&self, id: Option<ProfileId>) -> Result<()> {
        let mut idx = self.load()?;
        if let Some(id) = id {
            if !idx.profiles.iter().any(|p| p.profile_id == id) {
                return Err(StorageError::Invalid("unknown profile".into()));
            }
        }
        idx.active = id;
        idx.version = INDEX_VERSION;
        self.store(&idx)
    }

    /// The active profile, if any.
    pub fn active(&self) -> Result<Option<ProfileId>> {
        Ok(self.load()?.active)
    }

    /// Delete a profile: its index entry and its whole directory (database,
    /// WAL and SHM files). The caller must close the database first and
    /// remove the key from the OS secure store.
    pub fn remove(&self, id: ProfileId) -> Result<bool> {
        let mut idx = self.load()?;
        let before = idx.profiles.len();
        idx.profiles.retain(|p| p.profile_id != id);
        if idx.active == Some(id) {
            idx.active = None;
        }
        let existed = idx.profiles.len() != before;
        self.store(&idx)?;
        match std::fs::remove_dir_all(self.profile_dir(id)) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(e.into()),
        }
        Ok(existed)
    }
}
