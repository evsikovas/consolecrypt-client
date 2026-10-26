//! Database metadata and credentials use the existing encrypted vault writer.
//! Replacement secrets and their references commit in one transaction.
use crate::{inventory::validate_refs, AppCore, AppError, AppResult};
use cc_models::{
    database::{DbAuth, DbConnection},
    secret::{Secret, SecretKind, SecretValue},
    ObjectId, ObjectKind, VaultObject,
};

fn invalid(reason: &'static str) -> AppError {
    AppError::invalid("database", reason)
}

fn valid_secret(value: &SecretValue) -> AppResult<()> {
    if value.is_empty() || value.len() > 1024 * 1024 || value.expose_secret().contains('\0') {
        return Err(invalid("invalid_credential"));
    }
    Ok(())
}

/// A draft cannot redirect existing authentication to another vault secret.
/// New secret IDs are assigned exclusively by the native commit boundary.
fn prepare_credentials(
    connection: &mut DbConnection,
    existing: Option<&DbConnection>,
    password: Option<SecretValue>,
    private_key: Option<SecretValue>,
) -> AppResult<Vec<Secret>> {
    let previous = existing.map(|value| &value.auth);
    let password_ref = connection.auth.password_secret_id;
    if password_ref.is_some() && password_ref != previous.and_then(|auth| auth.password_secret_id) {
        return Err(invalid("credential_reference_changed"));
    }
    let key_ref = connection
        .auth
        .client_certificate
        .as_ref()
        .map(|cert| cert.key_secret_id);
    let previous_key = previous
        .and_then(|auth| auth.client_certificate.as_ref())
        .map(|cert| cert.key_secret_id);
    if key_ref.is_some_and(|id| id != ObjectId::NIL && Some(id) != previous_key) {
        return Err(invalid("credential_reference_changed"));
    }
    let mut secrets = Vec::new();
    if let Some(password) = password {
        valid_secret(&password)?;
        let secret = Secret::new(SecretKind::Password, password);
        connection.auth.password_secret_id = Some(secret.id);
        secrets.push(secret);
    }
    if let Some(key) = private_key {
        valid_secret(&key)?;
        let certificate = connection
            .auth
            .client_certificate
            .as_mut()
            .ok_or_else(|| invalid("certificate_required"))?;
        let secret = Secret::new(SecretKind::Other, key);
        certificate.key_secret_id = secret.id;
        secrets.push(secret);
    }
    connection.validate()?;
    Ok(secrets)
}

impl AppCore {
    pub async fn db_connections(&self) -> AppResult<Vec<DbConnection>> {
        let (_, unlocked) = self.unlocked().await?;
        let mut connections = unlocked.working().db_connections();
        connections.extend(self.db_local_connections().await?);
        connections.sort_by(|a, b| a.name.cmp(&b.name).then(a.id.cmp(&b.id)));
        Ok(connections)
    }

    /// NIL means create. Existing IDs must resolve to a database connection;
    /// secrets omitted by the form retain the supplied existing references.
    pub async fn db_save_connection(
        &self,
        mut connection: DbConnection,
        password: Option<SecretValue>,
        private_key: Option<SecretValue>,
    ) -> AppResult<DbConnection> {
        let (owner, unlocked) = self.unlocked().await?;
        let creating = connection.id == ObjectId::NIL;
        // Engines added after 0.9.1 stay on this device (see database_local)
        // unless the connection was moved into the vault
        // (database_vault_move).
        let in_vault = !creating && unlocked.working().db_connection(connection.id).is_some();
        let local =
            !creating && !in_vault && self.db_saved_connection(connection.id).await?.is_some();
        if (connection.engine.local_only() && !in_vault) || local {
            if !connection.engine.local_only() || (!creating && !local) {
                // An engine never moves between the vault and this device.
                return Err(invalid("engine_changed"));
            }
            // An SQLite file has no password or client key.
            if connection.engine == cc_models::database::DbEngine::Sqlite
                && (password.is_some() || private_key.is_some())
            {
                return Err(invalid("credential_unavailable"));
            }
            return self
                .db_save_local_with_credentials(connection, creating, password, private_key)
                .await;
        }
        let existing = if creating {
            connection.id = ObjectId::new();
            None
        } else {
            Some(
                unlocked
                    .working()
                    .db_connection(connection.id)
                    .ok_or_else(|| invalid("connection_unavailable"))?,
            )
        };
        // A vault connection never becomes one of an engine older apps cannot
        // read (that is a move, with its device check), nor the other way.
        if existing.as_ref().is_some_and(|old| {
            old.engine != connection.engine
                && (old.engine.local_only() || connection.engine.local_only())
        }) {
            return Err(invalid("engine_changed"));
        }
        let now = chrono::Utc::now();
        connection.created_at = existing.as_ref().map_or(now, |old| old.created_at);
        connection.updated_at = now;
        let secrets =
            prepare_credentials(&mut connection, existing.as_ref(), password, private_key)?;

        // Validate route/group and retained references before any write. Newly
        // generated credential references are checked against the exact batch.
        let fresh: std::collections::HashSet<_> = secrets.iter().map(|secret| secret.id).collect();
        let mut references = connection.clone();
        references.auth = DbAuth::default();
        validate_refs(unlocked.working(), &VaultObject::DbConnection(references))?;
        for id in connection.auth.secret_ids() {
            if !fresh.contains(&id) && unlocked.working().kind_of(id) != Some(ObjectKind::Secret) {
                return Err(invalid("credential_unavailable"));
            }
        }
        let _profile = self.hold_session(&owner).await?;
        let _generation = owner.hold_unlocked(&unlocked).await?;
        let mut objects: Vec<_> = secrets.into_iter().map(VaultObject::Secret).collect();
        objects.push(VaultObject::DbConnection(connection.clone()));
        unlocked.writer.put_batch(objects).await?;
        Ok(connection)
    }

    pub async fn db_delete_connection(&self, id: String) -> AppResult<()> {
        let (owner, unlocked) = self.unlocked().await?;
        let id = crate::dto::parse_id("connection_id", &id)?;
        if unlocked.working().db_connection(id).is_none() {
            if self.db_delete_local_connection(id).await? {
                unlocked.db.close_connection(&id.to_string());
                self.db_prune_local_secrets().await?;
                return Ok(());
            }
            return Err(invalid("connection_unavailable"));
        }
        let _profile = self.hold_session(&owner).await?;
        let _generation = owner.hold_unlocked(&unlocked).await?;
        // Working-set revision guards immediately revoke live database routes.
        // Credentials may be shared with other objects and are retained.
        unlocked.writer.delete(id).await?;
        Ok(())
    }
}

impl AppCore {
    /// Saves a device-local connection; a new password or client key is
    /// sealed on this device too (`database_local_secrets`), never in the
    /// vault. Live sessions of the connection close.
    async fn db_save_local_with_credentials(
        &self,
        mut connection: DbConnection,
        creating: bool,
        password: Option<SecretValue>,
        private_key: Option<SecretValue>,
    ) -> AppResult<DbConnection> {
        let existing = if creating {
            connection.id = ObjectId::new();
            None
        } else {
            self.db_saved_connection(connection.id).await?
        };
        let secrets =
            prepare_credentials(&mut connection, existing.as_ref(), password, private_key)?;
        let fresh: std::collections::HashSet<_> = secrets.iter().map(|secret| secret.id).collect();
        for id in connection.auth.secret_ids() {
            if !fresh.contains(&id) && !self.db_has_local_secret(id).await? {
                return Err(invalid("credential_unavailable"));
            }
        }
        self.db_save_local_secrets(secrets).await?;
        let saved = self.db_save_local_connection(connection, creating).await?;
        self.db_prune_local_secrets().await?;
        let (_, unlocked) = self.unlocked().await?;
        unlocked.db.close_connection(&saved.id.to_string());
        Ok(saved)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cc_models::database::DbEngine;

    #[test]
    fn new_password_is_separate_from_connection_payload_and_replacement_is_explicit() {
        let mut connection = DbConnection::new("fixture", "db.example.test", DbEngine::Postgresql);
        let secrets = prepare_credentials(
            &mut connection,
            None,
            Some(SecretValue::new("database-password-canary")),
            None,
        )
        .unwrap();
        assert_eq!(connection.auth.password_secret_id, Some(secrets[0].id));
        let encoded = serde_json::to_string(&connection).unwrap();
        assert!(!encoded.contains("database-password-canary"));
        let previous = connection.clone();
        assert!(
            prepare_credentials(&mut connection, Some(&previous), None, None)
                .unwrap()
                .is_empty()
        );
        assert_eq!(connection.auth, previous.auth);
        connection.auth.password_secret_id = Some(ObjectId::new());
        assert!(prepare_credentials(&mut connection, Some(&previous), None, None).is_err());
    }

    #[test]
    fn create_cannot_reference_an_unrelated_secret_or_accept_invalid_secret_bytes() {
        let mut connection = DbConnection::new("fixture", "db.example.test", DbEngine::Redis);
        connection.auth.password_secret_id = Some(ObjectId::new());
        assert!(prepare_credentials(&mut connection, None, None, None).is_err());
        connection.auth.password_secret_id = None;
        for value in ["", "password\0canary"] {
            assert!(prepare_credentials(
                &mut connection,
                None,
                Some(SecretValue::new(value)),
                None,
            )
            .is_err());
        }
        assert!(prepare_credentials(
            &mut connection,
            None,
            None,
            Some(SecretValue::new("private-key-without-certificate")),
        )
        .is_err());
    }
}
