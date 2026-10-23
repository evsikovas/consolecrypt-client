//! Local SQLCipher query history. E2EE sync applies only to future entries
//! after explicit opt-in, and never includes parameters or result rows.
use crate::{AppCore, AppError, AppResult};
use cc_models::{history::DbHistoryEntry, ObjectId, VaultObject};
use serde::{Deserialize, Serialize};

const RECORDS: &str = "database.history.v1";
/// History of connections that stay on this device (engines added after
/// 0.9.1): its own record so an older version still reads `RECORDS`.
const LOCAL_ENGINE_RECORDS: &str = "database.history.local_engines.v1";
const RECORD_KEYS: [&str; 2] = [RECORDS, LOCAL_ENGINE_RECORDS];

fn records_key(engine: cc_models::database::DbEngine) -> &'static str {
    if engine.local_only() {
        LOCAL_ENGINE_RECORDS
    } else {
        RECORDS
    }
}
const POLICIES: &str = "database.history.policies.v1";
const MAX_ENTRIES: usize = 5000;
const MAX_BYTES: usize = 8 * 1024 * 1024;
const MAX_TEXT: usize = 64 * 1024;
const RETENTION_DAYS: i64 = 90;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DbHistoryPolicy {
    #[default]
    Local,
    EncryptedSync,
    Disabled,
}

#[derive(Debug, Serialize)]
pub struct DbHistoryPage {
    pub entries: Vec<DbHistoryEntry>,
    pub next_cursor: Option<String>,
}

type Policies = std::collections::BTreeMap<String, DbHistoryPolicy>;

fn invalid(reason: &'static str) -> AppError {
    AppError::invalid("database_history", reason)
}

/// Bound plaintext held by local history independently of a result's row cap.
fn retain(
    mut rows: Vec<DbHistoryEntry>,
    now: chrono::DateTime<chrono::Utc>,
) -> Vec<DbHistoryEntry> {
    rows.sort_by(|a, b| b.created_at.cmp(&a.created_at).then(b.id.cmp(&a.id)));
    let oldest = now - chrono::Duration::days(RETENTION_DAYS);
    let mut bytes = 0usize;
    let mut ids = std::collections::HashSet::new();
    rows.retain(|row| {
        let fits = row.text.len() <= MAX_TEXT
            && row.created_at >= oldest
            && row.created_at <= now
            && ids.len() < MAX_ENTRIES
            && bytes.saturating_add(row.text.len()) <= MAX_BYTES;
        if !fits || !ids.insert(row.id) {
            return false;
        }
        bytes += row.text.len();
        true
    });
    rows
}

impl AppCore {
    pub async fn db_history_policy(&self, connection_id: String) -> AppResult<DbHistoryPolicy> {
        let (owner, _) = self.unlocked().await?;
        let id = crate::dto::parse_id("connection_id", &connection_id)?;
        if self.db_saved_connection(id).await?.is_none() {
            return Err(invalid("connection_unavailable"));
        }
        let policies: Policies = owner
            .storage()?
            .setting_get(POLICIES)
            .await?
            .unwrap_or_default();
        Ok(policies.get(&id.to_string()).copied().unwrap_or_default())
    }

    pub async fn db_set_history_policy(
        &self,
        connection_id: String,
        policy: DbHistoryPolicy,
    ) -> AppResult<()> {
        let (owner, unlocked) = self.unlocked().await?;
        let id = crate::dto::parse_id("connection_id", &connection_id)?;
        if self.db_saved_connection(id).await?.is_none() {
            return Err(invalid("connection_unavailable"));
        }
        let _gate = unlocked.db_history_lock.lock().await;
        let _profile = self.hold_session(&owner).await?;
        let _generation = owner.hold_unlocked(&unlocked).await?;
        owner
            .storage()?
            .write(move |tx| {
                let mut policies: Policies = tx.setting_get(POLICIES)?.unwrap_or_default();
                policies.insert(id.to_string(), policy);
                tx.setting_set(POLICIES, &policies)
            })
            .await?;
        Ok(())
    }

    /// Called by the native query runner with its pinned connection and source
    /// template. Never accept history records as arbitrary bridge input.
    pub(crate) async fn db_record_query(
        &self,
        owner: &std::sync::Arc<crate::session::Session>,
        unlocked: &std::sync::Arc<crate::session::Unlocked>,
        row: DbHistoryEntry,
    ) -> AppResult<()> {
        if row.text.len() > MAX_TEXT || row.id == ObjectId::NIL {
            return Ok(());
        }
        if self.db_saved_connection(row.connection_id).await?.is_none() {
            return Ok(());
        }
        let _gate = unlocked.db_history_lock.lock().await;
        let _profile = self.hold_session(owner).await?;
        let _generation = owner.hold_unlocked(unlocked).await?;
        let policies: Policies = owner
            .storage()?
            .setting_get(POLICIES)
            .await?
            .unwrap_or_default();
        match policies
            .get(&row.connection_id.to_string())
            .copied()
            .unwrap_or_default()
        {
            DbHistoryPolicy::Disabled => {}
            // Their connection is not in the vault, so neither is history.
            DbHistoryPolicy::EncryptedSync if !row.engine.local_only() => {
                unlocked
                    .writer
                    .put(VaultObject::DbHistoryEntry(row))
                    .await?;
            }
            DbHistoryPolicy::Local | DbHistoryPolicy::EncryptedSync => {
                let key = records_key(row.engine);
                owner
                    .storage()?
                    .write(move |tx| {
                        let mut rows: Vec<DbHistoryEntry> =
                            tx.setting_get(key)?.unwrap_or_default();
                        rows.push(row);
                        tx.setting_set(key, &retain(rows, chrono::Utc::now()))
                    })
                    .await?;
            }
        }
        Ok(())
    }

    pub async fn db_history(
        &self,
        connection_id: String,
        query: String,
        succeeded: Option<bool>,
        favorites_only: bool,
        cursor: Option<String>,
        limit: u32,
    ) -> AppResult<DbHistoryPage> {
        let (owner, unlocked) = self.unlocked().await?;
        let id = crate::dto::parse_id("connection_id", &connection_id)?;
        if query.len() > 256 || !(1..=200).contains(&limit) {
            return Err(invalid("invalid_history_query"));
        }
        let _gate = unlocked.db_history_lock.lock().await;
        let _profile = self.hold_session(&owner).await?;
        let _generation = owner.hold_unlocked(&unlocked).await?;
        let mut rows: Vec<DbHistoryEntry> = Vec::new();
        for key in RECORD_KEYS {
            rows.extend(
                owner
                    .storage()?
                    .setting_get::<Vec<DbHistoryEntry>>(key)
                    .await?
                    .unwrap_or_default(),
            );
        }
        rows.extend(unlocked.working().db_history_entries());
        let query = query.to_lowercase();
        let mut rows: Vec<_> = retain(rows, chrono::Utc::now())
            .into_iter()
            .filter(|row| {
                row.connection_id == id
                    && (!favorites_only || row.favorite)
                    && succeeded.is_none_or(|success| success == row.error_code.is_none())
                    && (query.is_empty()
                        || row.text.expose_secret().to_lowercase().contains(&query))
            })
            .collect();
        let start = if let Some(cursor) = cursor {
            let cursor = crate::dto::parse_id("history_cursor", &cursor)?;
            rows.iter()
                .position(|row| row.id == cursor)
                .ok_or_else(|| invalid("expired_history_cursor"))?
                + 1
        } else {
            0
        };
        let end = (start + limit as usize).min(rows.len());
        let next_cursor = (end < rows.len()).then(|| rows[end - 1].id.to_string());
        let entries = rows.drain(start..end).collect();
        Ok(DbHistoryPage {
            entries,
            next_cursor,
        })
    }

    pub async fn db_history_favorite(&self, history_id: String, favorite: bool) -> AppResult<()> {
        let (owner, unlocked) = self.unlocked().await?;
        let id = crate::dto::parse_id("history_id", &history_id)?;
        let _gate = unlocked.db_history_lock.lock().await;
        let _profile = self.hold_session(&owner).await?;
        let _generation = owner.hold_unlocked(&unlocked).await?;
        if let Some(mut row) = unlocked.working().db_history_entry(id) {
            row.favorite = favorite;
            unlocked
                .writer
                .put(VaultObject::DbHistoryEntry(row))
                .await?;
        } else {
            let found = owner
                .storage()?
                .write(move |tx| {
                    for key in RECORD_KEYS {
                        let mut rows: Vec<DbHistoryEntry> =
                            tx.setting_get(key)?.unwrap_or_default();
                        if let Some(row) = rows.iter_mut().find(|row| row.id == id) {
                            row.favorite = favorite;
                            tx.setting_set(key, &rows)?;
                            return Ok::<_, cc_storage_core::StorageError>(true);
                        }
                    }
                    Ok(false)
                })
                .await?;
            if !found {
                return Err(invalid("history_unavailable"));
            }
        }
        Ok(())
    }

    /// Saves a history entry as a snippet (SnippetSource::History). Its
    /// `{{variables}}` become snippet variables; the dialect follows the engine.
    pub async fn db_save_history_snippet(
        &self,
        history_id: String,
        name: String,
    ) -> AppResult<String> {
        use cc_models::database::DbEngine;
        use cc_models::snippet::{
            template_variables, RiskLevel, Snippet, SnippetSource, SnippetType, SnippetVariable,
        };
        let (owner, unlocked) = self.unlocked().await?;
        let id = crate::dto::parse_id("history_id", &history_id)?;
        let name = name.trim().to_owned();
        if name.is_empty() || name.len() > 200 {
            return Err(invalid("invalid_snippet_name"));
        }
        let row = match unlocked.working().db_history_entry(id) {
            Some(row) => row,
            None => {
                let mut found = None;
                for key in RECORD_KEYS {
                    let rows: Vec<DbHistoryEntry> =
                        owner.storage()?.setting_get(key).await?.unwrap_or_default();
                    if let Some(row) = rows.into_iter().find(|row| row.id == id) {
                        found = Some(row);
                        break;
                    }
                }
                found.ok_or_else(|| invalid("history_unavailable"))?
            }
        };
        let (snippet_type, shell) = match row.engine {
            DbEngine::Postgresql => (SnippetType::Postgresql, None),
            DbEngine::Mysql => (SnippetType::Sql, Some("mysql")),
            DbEngine::Mariadb => (SnippetType::Sql, Some("mariadb")),
            DbEngine::Clickhouse => (SnippetType::Sql, Some("clickhouse")),
            DbEngine::Redis => (SnippetType::RedisCli, None),
            DbEngine::Mongodb => (SnippetType::Shell, Some("mongosh")),
            DbEngine::Sqlite => (SnippetType::Sql, Some("sqlite")),
            DbEngine::Cassandra => (SnippetType::Cql, None),
            DbEngine::Unknown => (SnippetType::Sql, None),
        };
        let template = row.text.expose_secret().to_owned();
        let now = chrono::Utc::now();
        let snippet = Snippet {
            id: ObjectId::new(),
            name,
            description: String::new(),
            package_name: None,
            catalog_id: None,
            snippet_type,
            shell: shell.map(str::to_owned),
            variables: template_variables(&template)
                .into_iter()
                .map(|name| SnippetVariable {
                    name,
                    description: String::new(),
                    default: None,
                    required: true,
                })
                .collect(),
            template,
            tags: vec!["database".into()],
            risk_level: RiskLevel::Unknown,
            source: SnippetSource::History,
            created_by: None,
            created_at: now,
            updated_at: now,
            last_used_at: None,
            usage_count: 0,
        };
        let snippet_id = snippet.id.to_string();
        let _profile = self.hold_session(&owner).await?;
        let _generation = owner.hold_unlocked(&unlocked).await?;
        unlocked.writer.put(VaultObject::Snippet(snippet)).await?;
        Ok(snippet_id)
    }

    pub async fn db_delete_history(&self, connection_id: String) -> AppResult<()> {
        let (owner, unlocked) = self.unlocked().await?;
        let id = crate::dto::parse_id("connection_id", &connection_id)?;
        let _gate = unlocked.db_history_lock.lock().await;
        let _profile = self.hold_session(&owner).await?;
        let _generation = owner.hold_unlocked(&unlocked).await?;
        for row in unlocked.working().db_history_entries() {
            if row.connection_id == id {
                unlocked.writer.delete(row.id).await?;
            }
        }
        owner
            .storage()?
            .write(move |tx| {
                for key in RECORD_KEYS {
                    let mut rows: Vec<DbHistoryEntry> = tx.setting_get(key)?.unwrap_or_default();
                    rows.retain(|row| row.connection_id != id);
                    tx.setting_set(key, &rows)?;
                }
                Ok::<_, cc_storage_core::StorageError>(())
            })
            .await?;
        Ok(())
    }
}
