//! Syncing connections of engines added after 0.9.1 (FEAT-01): a device-local
//! Cassandra / ScyllaDB connection, or an SQLite one reached through a host
//! (SSH, Docker, Kubernetes), moves into the vault so it syncs and is backed
//! up with it. An SQLite file on this computer stays here: its path means
//! nothing on another device.
//!
//! Apps up to 0.9.1 cannot decode such a vault object (one unreadable object;
//! a backup holding one cannot be restored there), so the move needs every
//! active device of the account at 0.9.2 or later, as the devices report it
//! (`DeviceInfo.client_version`, protocol 1.13). Sharing them stays off: the
//! apps of other people are unknown.
use crate::{AppCore, AppError, AppResult};
use cc_models::{
    database::{DbConnection, DbEngine, DbRoute},
    ObjectKind, VaultObject,
};
use cc_protocol::devices::DeviceStatus;
use cc_storage_core::ProfileKind;
use serde::{Deserialize, Serialize};

/// The first app that decodes connections of later engines.
const MIN_APP_VERSION: (u64, u64, u64) = (0, 9, 2);

fn invalid(reason: &'static str) -> AppError {
    AppError::invalid("database", reason)
}

/// `1.2.3`, `1.2.3+45`, `1.2.3-beta` → (1, 2, 3).
fn parse_version(text: &str) -> Option<(u64, u64, u64)> {
    let core = text.trim().split(['+', '-']).next()?;
    let mut parts = core.split('.').map(|part| part.parse::<u64>().ok());
    let version = (parts.next()??, parts.next()??, parts.next()??);
    parts.next().is_none().then_some(version)
}

/// Whether this account's devices can all read synced connections of
/// engines added after 0.9.1.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DbSyncReadinessDto {
    pub ready: bool,
    /// Active devices on an older app, or one that never reported its
    /// version (it has not run 0.9.3 yet): name and reported version.
    pub outdated_devices: Vec<DbOutdatedDeviceDto>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DbOutdatedDeviceDto {
    pub name: String,
    pub client_version: Option<String>,
}

/// Which connections can move: Cassandra, or SQLite through a host.
pub(crate) fn movable(connection: &DbConnection) -> bool {
    match connection.engine {
        DbEngine::Cassandra => true,
        DbEngine::Sqlite => !matches!(connection.route, DbRoute::Direct),
        _ => false,
    }
}

impl AppCore {
    /// Can connections of later engines sync on this account?
    pub async fn db_sync_readiness(&self) -> AppResult<DbSyncReadinessDto> {
        let s = self.session().await?;
        if s.kind().await? == ProfileKind::Local {
            return Ok(DbSyncReadinessDto {
                ready: true,
                outdated_devices: Vec::new(),
            });
        }
        let devices = s.api()?.list_devices().await?.devices;
        let outdated: Vec<DbOutdatedDeviceDto> = devices
            .into_iter()
            .filter(|device| device.status == DeviceStatus::Active)
            .filter(|device| {
                device
                    .client_version
                    .as_deref()
                    .and_then(parse_version)
                    .is_none_or(|version| version < MIN_APP_VERSION)
            })
            .map(|device| DbOutdatedDeviceDto {
                name: device.name,
                client_version: device.client_version,
            })
            .collect();
        Ok(DbSyncReadinessDto {
            ready: outdated.is_empty(),
            outdated_devices: outdated,
        })
    }

    /// Ids of the connections kept only on this device.
    pub async fn db_device_only_ids(&self) -> AppResult<Vec<String>> {
        Ok(self
            .db_local_connections()
            .await?
            .iter()
            .map(|connection| connection.id.to_string())
            .collect())
    }

    /// Moves a device-local connection (and its credential secrets) into
    /// the vault, so it syncs with it. Refused for an SQLite file on this
    /// computer and while a device of the account runs an app before 0.9.2.
    pub async fn db_move_connection_to_vault(&self, id: String) -> AppResult<DbConnection> {
        let id = crate::dto::parse_id("connection_id", &id)?;
        let connection = self
            .db_local_connections()
            .await?
            .into_iter()
            .find(|connection| connection.id == id)
            .ok_or_else(|| invalid("connection_unavailable"))?;
        if !movable(&connection) {
            return Err(invalid("device_file"));
        }
        if !self.db_sync_readiness().await?.ready {
            return Err(invalid("devices_outdated"));
        }
        let (owner, unlocked) = self.unlocked().await?;
        let mut objects = Vec::new();
        for secret_id in connection.auth.secret_ids() {
            match self.db_local_secret(secret_id).await? {
                Some(secret) => objects.push(VaultObject::Secret(secret)),
                None if unlocked.working().kind_of(secret_id) == Some(ObjectKind::Secret) => {}
                None => return Err(invalid("credential_unavailable")),
            }
        }
        crate::inventory::validate_refs(
            unlocked.working(),
            &VaultObject::DbConnection(DbConnection {
                auth: Default::default(),
                ..connection.clone()
            }),
        )?;
        objects.push(VaultObject::DbConnection(connection.clone()));
        {
            let _profile = self.hold_session(&owner).await?;
            let _generation = owner.hold_unlocked(&unlocked).await?;
            unlocked.writer.put_batch(objects).await?;
        }
        // The vault copy exists: the device copy and its sealed secrets go.
        self.db_delete_local_connection(id).await?;
        self.db_prune_local_secrets().await?;
        Ok(connection)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions_compare_by_release() {
        assert_eq!(parse_version("0.9.2"), Some((0, 9, 2)));
        assert_eq!(parse_version("0.9.3+21059"), Some((0, 9, 3)));
        assert_eq!(parse_version("1.0.0-beta.1"), Some((1, 0, 0)));
        assert_eq!(parse_version("0.0.0-test"), Some((0, 0, 0)));
        assert_eq!(parse_version("0.9"), None);
        assert_eq!(parse_version("garbage"), None);
        assert!(parse_version("0.9.1").unwrap() < MIN_APP_VERSION);
        assert!(parse_version("0.10.0").unwrap() >= MIN_APP_VERSION);
    }
}
