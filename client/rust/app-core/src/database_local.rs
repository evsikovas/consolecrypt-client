//! Device-local database connections.
//!
//! Connections of engines added after 0.9.1 ([`DbEngine::local_only`], today
//! Cassandra / ScyllaDB) are kept in this profile's encrypted local store,
//! never in the synced vault: clients up to 0.9.1 cannot decode such a vault
//! object. They are not shared, synced or part of vault backups; the UI says
//! "this device only". Their credential secrets live beside them
//! (`database_local_secrets`).
use crate::{AppCore, AppError, AppResult};
use cc_models::{
    database::{DbConnection, DbEngine},
    ObjectId, VaultObject,
};

/// Local store key: a list of connection objects.
const LOCAL_CONNECTIONS: &str = "database.local_connections.v1";
const MAX_LOCAL_CONNECTIONS: usize = 1000;

fn invalid(reason: &'static str) -> AppError {
    AppError::invalid("database", reason)
}

/// Decodes stored connections one by one; an entry this version cannot read
/// (a later engine) is kept as stored and skipped.
fn decode(values: &[serde_json::Value]) -> Vec<DbConnection> {
    values
        .iter()
        .filter_map(|value| serde_json::from_value::<DbConnection>(value.clone()).ok())
        .filter(|connection| connection.engine.local_only() && connection.validate().is_ok())
        .collect()
}

fn same_id(value: &serde_json::Value, id: ObjectId) -> bool {
    value.get("id").and_then(|id| id.as_str()) == Some(id.to_string().as_str())
}

impl AppCore {
    /// Connections kept only on this device.
    pub(crate) async fn db_local_connections(&self) -> AppResult<Vec<DbConnection>> {
        let (owner, _) = self.unlocked().await?;
        let values: Vec<serde_json::Value> = owner
            .storage()?
            .setting_get(LOCAL_CONNECTIONS)
            .await?
            .unwrap_or_default();
        Ok(decode(&values))
    }

    /// A saved connection from the vault or from this device.
    pub(crate) async fn db_saved_connection(
        &self,
        id: ObjectId,
    ) -> AppResult<Option<DbConnection>> {
        let (_, unlocked) = self.unlocked().await?;
        if let Some(connection) = unlocked.working().db_connection(id) {
            return Ok(Some(connection));
        }
        Ok(self
            .db_local_connections()
            .await?
            .into_iter()
            .find(|connection| connection.id == id))
    }

    /// Creates or replaces a device-local connection. It keeps no secrets;
    /// its route references (SSH host, Kubernetes context) must exist.
    pub(crate) async fn db_save_local_connection(
        &self,
        mut connection: DbConnection,
        creating: bool,
    ) -> AppResult<DbConnection> {
        let (owner, unlocked) = self.unlocked().await?;
        if !connection.engine.local_only() || connection.engine == DbEngine::Unknown {
            return Err(invalid("engine_unavailable"));
        }
        let existing = if creating {
            None
        } else {
            Some(
                self.db_local_connections()
                    .await?
                    .into_iter()
                    .find(|old| old.id == connection.id)
                    .ok_or_else(|| invalid("connection_unavailable"))?,
            )
        };
        let now = chrono::Utc::now();
        connection.created_at = existing.as_ref().map_or(now, |old| old.created_at);
        connection.updated_at = now;
        // Credential secrets are sealed on this device (database_local_secrets),
        // not vault objects: only route and group references are checked here.
        let mut references = connection.clone();
        references.auth = Default::default();
        crate::inventory::validate_refs(
            unlocked.working(),
            &VaultObject::DbConnection(references),
        )?;
        let _profile = self.hold_session(&owner).await?;
        let _generation = owner.hold_unlocked(&unlocked).await?;
        let stored =
            serde_json::to_value(&connection).map_err(|_| invalid("invalid_connection"))?;
        let id = connection.id;
        owner
            .storage()?
            .write(move |tx| {
                let mut values: Vec<serde_json::Value> =
                    tx.setting_get(LOCAL_CONNECTIONS)?.unwrap_or_default();
                values.retain(|value| !same_id(value, id));
                values.push(stored);
                values.truncate(MAX_LOCAL_CONNECTIONS);
                tx.setting_set(LOCAL_CONNECTIONS, &values)
            })
            .await?;
        Ok(connection)
    }

    /// Removes a device-local connection; false when it is not one.
    pub(crate) async fn db_delete_local_connection(&self, id: ObjectId) -> AppResult<bool> {
        let (owner, unlocked) = self.unlocked().await?;
        let _profile = self.hold_session(&owner).await?;
        let _generation = owner.hold_unlocked(&unlocked).await?;
        Ok(owner
            .storage()?
            .write(move |tx| {
                let mut values: Vec<serde_json::Value> =
                    tx.setting_get(LOCAL_CONNECTIONS)?.unwrap_or_default();
                let before = values.len();
                values.retain(|value| !same_id(value, id));
                let removed = values.len() != before;
                if removed {
                    tx.setting_set(LOCAL_CONNECTIONS, &values)?;
                }
                Ok::<_, cc_storage_core::StorageError>(removed)
            })
            .await?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stored_entries_of_unknown_engines_are_skipped_not_fatal() {
        let ring = DbConnection::new("ring", "cass.example.test", DbEngine::Cassandra);
        let mut future = serde_json::to_value(&ring).unwrap();
        future["id"] = serde_json::json!(ObjectId::new().to_string());
        future["engine"] = serde_json::json!("graphdb");
        future["options"] = serde_json::json!({"kind": "graphdb"});
        let synced = DbConnection::new("pg", "db.example.test", DbEngine::Postgresql);
        let values = vec![
            serde_json::to_value(&ring).unwrap(),
            future,
            serde_json::json!({"broken": true}),
            serde_json::to_value(&synced).unwrap(),
        ];
        assert_eq!(decode(&values), vec![ring.clone()]);
        assert!(same_id(&values[0], ring.id));
    }
}
