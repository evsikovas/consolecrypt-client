//! Device/profile-local SQLCipher connection metadata, like Automation projects.
//! No sync schema, token material, HTTP request or Runner enrollment is added.
use crate::session::{Session, Unlocked};
use crate::{AppCore, AppError, AppResult};
use cc_monitoring_core::{AuthRef, Route, SourceConfig};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{collections::BTreeMap, sync::Arc};
use uuid::Uuid;

pub(super) const KEY: &str = "automation.awx.connections.v1";
const MAX_CONNECTIONS: usize = 32;
const MAX_CATALOG: usize = 512 * 1024;
const MAX_REQUEST: usize = 64 * 1024;
const MAX_REVISION: u64 = 9_007_199_254_740_991;

pub(crate) struct AwxCatalogEpoch(tokio::sync::watch::Sender<u64>);
impl Default for AwxCatalogEpoch {
    fn default() -> Self {
        Self(tokio::sync::watch::channel(0).0)
    }
}
impl AwxCatalogEpoch {
    /// Metadata invalidation only, never a permission or a persisted Ready bit.
    pub(super) fn changes(&self) -> tokio::sync::watch::Receiver<u64> {
        self.0.subscribe()
    }
    fn invalidate(&self) {
        self.0.send_modify(|epoch| *epoch = epoch.saturating_add(1));
    }
}

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct AwxScope {
    pub version: u8,
    pub profile_id: String,
    pub notification_scope_id: String,
}
impl AwxScope {
    pub(super) fn check(&self, owner: &Session, unlocked: &Unlocked) -> AppResult<()> {
        if self.version != 1
            || canonical(&self.profile_id).is_err()
            || canonical(&self.notification_scope_id).is_err()
            || self.profile_id != owner.profile_id.to_string()
            || unlocked.notices.scope_id().as_deref() != Some(self.notification_scope_id.as_str())
        {
            return Err(invalid("awx_context_changed"));
        }
        Ok(())
    }
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ConnectionRecord {
    version: u8,
    pub revision: u64,
    pub configuration: SourceConfig,
}
#[derive(Default, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Catalog {
    version: u8,
    revision: u64,
    connections: BTreeMap<String, ConnectionRecord>,
}
impl Catalog {
    fn empty() -> Self {
        Self {
            version: 1,
            revision: 0,
            connections: BTreeMap::new(),
        }
    }
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AwxConnectionView {
    pub id: String,
    pub revision: Option<u64>,
    pub configuration: Option<SourceConfig>,
    pub read_only: bool,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AwxCatalogView {
    pub version: u8,
    pub catalog_revision: Option<u64>,
    pub read_only: bool,
    pub connections: Vec<AwxConnectionView>,
}
fn invalid(reason: &'static str) -> AppError {
    AppError::invalid("automation_awx", reason)
}
fn canonical(value: &str) -> AppResult<Uuid> {
    let id = Uuid::parse_str(value).map_err(|_| invalid("awx_invalid_input"))?;
    if id.is_nil() || id.to_string() != value {
        return Err(invalid("awx_invalid_input"));
    }
    Ok(id)
}
fn validate_configuration(c: &SourceConfig) -> AppResult<()> {
    canonical(&c.id)?;
    let endpoint = c
        .validate()
        .map_err(|_| invalid("awx_invalid_configuration"))?;
    if endpoint.scheme() != "https"
        || c.route != Route::Direct
        || c.name.trim().is_empty()
        || c.name.chars().any(char::is_control)
        || matches!(c.auth, AuthRef::None)
    {
        return Err(invalid("awx_invalid_configuration"));
    }
    match &c.auth {
        AuthRef::BearerRef { credential_id } | AuthRef::BasicRef { credential_id } => {
            canonical(credential_id)?;
        }
        AuthRef::None => unreachable!(),
    }
    Ok(())
}
pub(super) fn credential_metadata(
    u: &Unlocked,
    c: &SourceConfig,
) -> AppResult<crate::working_set::VaultCredentialMetadata> {
    let raw = match &c.auth {
        AuthRef::BearerRef { credential_id } | AuthRef::BasicRef { credential_id } => credential_id,
        AuthRef::None => return Err(invalid("awx_credential_unavailable")),
    };
    let id = crate::dto::parse_id("awx_credential", raw)?;
    let metadata = u
        .working()
        .vault_credential_metadata(id)
        .ok_or_else(|| invalid("awx_credential_unavailable"))?;
    if matches!(&c.auth, AuthRef::BasicRef { .. })
        && metadata
            .username()
            .is_none_or(|s| s.is_empty() || s.contains(':') || s.chars().any(char::is_control))
    {
        return Err(invalid("awx_credential_unavailable"));
    }
    Ok(metadata)
}
fn bounded(value: &impl Serialize) -> AppResult<()> {
    struct Limit(usize);
    impl std::io::Write for Limit {
        fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
            self.0 = self
                .0
                .checked_add(b.len())
                .filter(|n| *n <= MAX_CATALOG)
                .ok_or_else(|| std::io::Error::other("AWX catalog limit"))?;
            Ok(b.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    serde_json::to_writer(Limit(0), value).map_err(|_| invalid("awx_catalog_budget"))
}
fn decode(raw: Option<Value>) -> AppResult<Catalog> {
    let Some(raw) = raw else {
        return Ok(Catalog::empty());
    };
    bounded(&raw)?;
    let c: Catalog = serde_json::from_value(raw).map_err(|_| invalid("awx_catalog_read_only"))?;
    if c.version != 1 || c.revision > MAX_REVISION || c.connections.len() > MAX_CONNECTIONS {
        return Err(invalid("awx_catalog_read_only"));
    }
    for (id, record) in &c.connections {
        if record.version != 1
            || record.revision == 0
            || record.revision > MAX_REVISION
            || record.configuration.id != *id
        {
            return Err(invalid("awx_catalog_read_only"));
        }
        validate_configuration(&record.configuration)?;
    }
    Ok(c)
}
/// A bounded current SQL row, never a credential grant or a reconstructed review.
pub(super) fn selected_connection(raw: Option<Value>, id: &str) -> AppResult<ConnectionRecord> {
    canonical(id)?;
    decode(raw)?
        .connections
        .remove(id)
        .ok_or_else(|| invalid("awx_connection_unavailable"))
}

fn view(catalog: AppResult<Catalog>) -> AwxCatalogView {
    match catalog {
        Ok(c) => AwxCatalogView {
            version: 1,
            catalog_revision: Some(c.revision),
            read_only: false,
            connections: c
                .connections
                .into_iter()
                .map(|(id, r)| AwxConnectionView {
                    id,
                    revision: Some(r.revision),
                    configuration: Some(r.configuration),
                    read_only: false,
                })
                .collect(),
        },
        Err(_) => AwxCatalogView {
            version: 1,
            catalog_revision: None,
            read_only: true,
            connections: Vec::new(),
        },
    }
}

enum Mutation {
    Save {
        expected_catalog: u64,
        expected_revision: Option<u64>,
        configuration: SourceConfig,
    },
    Delete {
        expected_catalog: u64,
        id: String,
        expected_revision: u64,
    },
}
impl AppCore {
    pub(crate) async fn automation_awx_scope(&self) -> AppResult<AwxScope> {
        let (owner, u) = self.unlocked().await?;
        let _profile = self.hold_session(&owner).await?;
        let _unlock = owner.hold_unlocked(&u).await?;
        Ok(AwxScope {
            version: 1,
            profile_id: owner.profile_id.to_string(),
            notification_scope_id: u
                .notices
                .scope_id()
                .ok_or_else(|| invalid("awx_context_changed"))?,
        })
    }
    pub(crate) async fn automation_awx_connections(
        &self,
        scope: &AwxScope,
    ) -> AppResult<AwxCatalogView> {
        let (owner, u) = self.unlocked().await?;
        let _profile = self.hold_session(&owner).await?;
        let _unlock = owner.hold_unlocked(&u).await?;
        scope.check(&owner, &u)?;
        let raw = owner.storage()?.setting_get(KEY).await?;
        scope.check(&owner, &u)?;
        Ok(view(decode(raw)))
    }
    async fn mutate_awx(&self, scope: &AwxScope, mutation: Mutation) -> AppResult<AwxCatalogView> {
        let (owner, u) = self.unlocked().await?;
        let _profile = self.hold_session(&owner).await?;
        let _unlock = owner.hold_unlocked(&u).await?;
        scope.check(&owner, &u)?;
        let credential_guard = match &mutation {
            Mutation::Save { configuration, .. } => Some((
                configuration.clone(),
                credential_metadata(&u, configuration)?,
            )),
            Mutation::Delete { .. } => None,
        };
        let captured_scope = scope.clone();
        let original = Arc::downgrade(&u);
        let session = owner.clone();
        let catalog = owner
            .storage()?
            .write(move |tx| -> AppResult<Catalog> {
                let mut catalog = decode(tx.setting_get(KEY)?)?;
                let expected = match &mutation {
                    Mutation::Save {
                        expected_catalog, ..
                    }
                    | Mutation::Delete {
                        expected_catalog, ..
                    } => *expected_catalog,
                };
                if catalog.revision != expected {
                    return Err(invalid("awx_catalog_changed"));
                }
                let u = original.upgrade().ok_or(AppError::VaultLocked)?;
                captured_scope.check(&session, &u)?;
                if credential_guard.as_ref().is_some_and(|(config, original)| {
                    credential_metadata(&u, config).ok().as_ref() != Some(original)
                }) {
                    return Err(invalid("awx_credential_changed"));
                }
                match mutation {
                    Mutation::Save {
                        expected_revision,
                        mut configuration,
                        ..
                    } => {
                        let revision = match expected_revision {
                            None => {
                                if !configuration.id.is_empty()
                                    || catalog.connections.len() >= MAX_CONNECTIONS
                                {
                                    return Err(invalid("awx_invalid_input"));
                                }
                                configuration.id = Uuid::new_v4().to_string();
                                1
                            }
                            Some(expected) => {
                                let row = catalog
                                    .connections
                                    .get(&configuration.id)
                                    .ok_or_else(|| invalid("awx_connection_unavailable"))?;
                                if row.revision != expected {
                                    return Err(invalid("awx_connection_changed"));
                                }
                                expected
                                    .checked_add(1)
                                    .filter(|n| *n <= MAX_REVISION)
                                    .ok_or_else(|| invalid("awx_revision_limit"))?
                            }
                        };
                        validate_configuration(&configuration)?;
                        credential_metadata(&u, &configuration)?;
                        catalog.connections.insert(
                            configuration.id.clone(),
                            ConnectionRecord {
                                version: 1,
                                revision,
                                configuration,
                            },
                        );
                    }
                    Mutation::Delete {
                        id,
                        expected_revision,
                        ..
                    } => {
                        canonical(&id)?;
                        let row = catalog
                            .connections
                            .get(&id)
                            .ok_or_else(|| invalid("awx_connection_unavailable"))?;
                        if row.revision != expected_revision {
                            return Err(invalid("awx_connection_changed"));
                        }
                        catalog.connections.remove(&id);
                    }
                }
                catalog.revision = catalog
                    .revision
                    .checked_add(1)
                    .filter(|n| *n <= MAX_REVISION)
                    .ok_or_else(|| invalid("awx_revision_limit"))?;
                bounded(&catalog)?;
                captured_scope.check(&session, &u)?;
                if credential_guard.as_ref().is_some_and(|(config, original)| {
                    credential_metadata(&u, config).ok().as_ref() != Some(original)
                }) {
                    return Err(invalid("awx_credential_changed"));
                }
                // Invalidate pending readers before the actual catalog write. Failure
                // only retires a read conservatively; it never grants replacement access.
                u.awx_catalog.invalidate();
                tx.setting_set(KEY, &catalog)?;
                Ok(catalog)
            })
            .await?;
        scope.check(&owner, &u)?;
        Ok(view(Ok(catalog)))
    }
    pub(crate) async fn automation_awx_save_connection(
        &self,
        scope: &AwxScope,
        expected_catalog: u64,
        expected_revision: Option<u64>,
        configuration: SourceConfig,
    ) -> AppResult<AwxCatalogView> {
        self.mutate_awx(
            scope,
            Mutation::Save {
                expected_catalog,
                expected_revision,
                configuration,
            },
        )
        .await
    }
    pub(crate) async fn automation_awx_delete_connection(
        &self,
        scope: &AwxScope,
        expected_catalog: u64,
        id: String,
        expected_revision: u64,
    ) -> AppResult<AwxCatalogView> {
        self.mutate_awx(
            scope,
            Mutation::Delete {
                expected_catalog,
                id,
                expected_revision,
            },
        )
        .await
    }
    pub(crate) async fn automation_awx_catalog_json(&self, raw: &str) -> AppResult<String> {
        #[derive(Deserialize)]
        #[serde(
            tag = "kind",
            rename_all = "camelCase",
            rename_all_fields = "camelCase",
            deny_unknown_fields
        )]
        enum Request {
            List {
                version: u8,
                profile_id: String,
                notification_scope_id: String,
            },
            Save {
                version: u8,
                profile_id: String,
                notification_scope_id: String,
                expected_catalog_revision: u64,
                expected_revision: Option<u64>,
                configuration: SourceConfig,
            },
            Delete {
                version: u8,
                profile_id: String,
                notification_scope_id: String,
                expected_catalog_revision: u64,
                connection_id: String,
                expected_revision: u64,
            },
        }
        if raw.len() > MAX_REQUEST {
            return Err(invalid("awx_input_budget"));
        }
        let request: Request = serde_json::from_value(
            super::strict_json::parse(raw.as_bytes()).map_err(|_| invalid("awx_invalid_input"))?,
        )
        .map_err(|_| invalid("awx_invalid_input"))?;
        let result = match request {
            Request::List {
                version,
                profile_id,
                notification_scope_id,
            } => {
                self.automation_awx_connections(&AwxScope {
                    version,
                    profile_id,
                    notification_scope_id,
                })
                .await?
            }
            Request::Save {
                version,
                profile_id,
                notification_scope_id,
                expected_catalog_revision,
                expected_revision,
                configuration,
            } => {
                self.automation_awx_save_connection(
                    &AwxScope {
                        version,
                        profile_id,
                        notification_scope_id,
                    },
                    expected_catalog_revision,
                    expected_revision,
                    configuration,
                )
                .await?
            }
            Request::Delete {
                version,
                profile_id,
                notification_scope_id,
                expected_catalog_revision,
                connection_id,
                expected_revision,
            } => {
                self.automation_awx_delete_connection(
                    &AwxScope {
                        version,
                        profile_id,
                        notification_scope_id,
                    },
                    expected_catalog_revision,
                    connection_id,
                    expected_revision,
                )
                .await?
            }
        };
        serde_json::to_string(&result).map_err(|_| invalid("awx_response_encoding"))
    }
}

#[cfg(test)]
mod tests;
