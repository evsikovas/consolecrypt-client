//! Explicit database sharing projection: no personal object IDs, routes to
//! personal SSH/jump/proxy/tunnel/Kubernetes/Docker objects, groups, session
//! permissions, history or provider consents are transferable.
use crate::{inventory::validate_refs, AppCore, AppError, AppResult};
use cc_models::database::{
    DbAuth, DbAuthMechanism, DbClientCertificate, DbConnection, DbEngine, DbKubeTargetKind,
    DbLimits, DbOptions, DbRoute, DbTls,
};
use cc_models::secret::{Secret, SecretKind, SecretValue};
use cc_models::{ObjectId, VaultObject};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, fmt};
use zeroize::{Zeroize, ZeroizeOnDrop};

/// How the owner reaches the server. Personal route objects are never shared:
/// the receiver must select their own compatible route before saving a copy.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum SharedDbRoute {
    Direct,
    /// Through an SSH host, jump profile, proxy or saved tunnel; `host`/`port`
    /// are the endpoint as seen from that relay.
    Relay,
    /// A port-forward to a workload; the receiver selects their own context.
    Kubernetes {
        namespace: String,
        target_kind: DbKubeTargetKind,
        target_name: String,
    },
    /// A port-forward to a container by name; the receiver selects the engine.
    Docker {
        container: String,
    },
}

impl SharedDbRoute {
    fn of(route: &DbRoute) -> Self {
        match route {
            DbRoute::Direct => Self::Direct,
            DbRoute::Ssh { .. }
            | DbRoute::Jump { .. }
            | DbRoute::Proxy { .. }
            | DbRoute::Tunnel { .. } => Self::Relay,
            DbRoute::Kubernetes {
                namespace,
                target_kind,
                target_name,
                ..
            } => Self::Kubernetes {
                namespace: namespace.clone(),
                target_kind: *target_kind,
                target_name: target_name.clone(),
            },
            DbRoute::Docker { container, .. } => Self::Docker {
                container: container.clone(),
            },
        }
    }

    const fn wire_name(&self) -> &'static str {
        match self {
            Self::Direct => "direct",
            Self::Relay => "relay",
            Self::Kubernetes { .. } => "kubernetes",
            Self::Docker { .. } => "docker",
        }
    }

    /// The receiver's own route, completed with the shared forward target.
    /// A relayed or forwarded connection never silently becomes direct.
    fn resolve(&self, selected: DbRoute) -> AppResult<DbRoute> {
        let relay = matches!(
            selected,
            DbRoute::Ssh { .. }
                | DbRoute::Jump { .. }
                | DbRoute::Proxy { .. }
                | DbRoute::Tunnel { .. }
        );
        match (self, selected) {
            (Self::Direct, DbRoute::Direct) => Ok(DbRoute::Direct),
            (Self::Direct | Self::Relay, selected) if relay => Ok(selected),
            (
                Self::Kubernetes {
                    namespace,
                    target_kind,
                    target_name,
                },
                DbRoute::Kubernetes { context_id, .. },
            ) => Ok(DbRoute::Kubernetes {
                context_id,
                namespace: namespace.clone(),
                target_kind: *target_kind,
                target_name: target_name.clone(),
            }),
            (Self::Docker { container }, DbRoute::Docker { host_id, .. }) => Ok(DbRoute::Docker {
                host_id,
                container: container.clone(),
            }),
            _ => Err(AppError::invalid(
                "route",
                "select a compatible local route for this connection",
            )),
        }
    }
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize, Zeroize, ZeroizeOnDrop)]
#[serde(deny_unknown_fields)]
pub struct SharedDbAuthentication {
    pub password: Option<SecretValue>,
    /// Private key of the connection's TLS client certificate.
    pub client_key: Option<SecretValue>,
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize, Zeroize, ZeroizeOnDrop)]
#[serde(deny_unknown_fields)]
pub struct SharedDbProjection {
    pub name: String,
    #[zeroize(skip)]
    pub engine: DbEngine,
    pub host: String,
    pub port: u16,
    pub database: String,
    pub username: String,
    #[zeroize(skip)]
    pub tls: DbTls,
    #[zeroize(skip)]
    pub mechanism: DbAuthMechanism,
    /// Public certificate only; its key is part of `authentication`.
    pub client_certificate_pem: Option<String>,
    #[zeroize(skip)]
    pub route: SharedDbRoute,
    pub production: bool,
    #[zeroize(skip)]
    pub limits: DbLimits,
    #[zeroize(skip)]
    pub options: DbOptions,
    pub tags: Vec<String>,
    pub color: Option<String>,
    /// Present only after explicit credential-sharing consent.
    pub authentication: Option<SharedDbAuthentication>,
}

impl fmt::Debug for SharedDbAuthentication {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("SharedDbAuthentication(<redacted>)")
    }
}
impl fmt::Debug for SharedDbProjection {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("SharedDbProjection(<redacted>)")
    }
}

fn invalid() -> AppError {
    AppError::invalid("shared_db_connection", "invalid connection or credentials")
}

fn valid_secret(value: &SecretValue) -> AppResult<()> {
    if value.is_empty() || value.len() > 1024 * 1024 || value.expose_secret().contains('\0') {
        return Err(invalid());
    }
    Ok(())
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DbSharingPreview {
    pub preview_json: String,
    pub snapshot_stamp: String,
}

pub(crate) async fn prepare(
    u: &crate::session::Unlocked,
    id: ObjectId,
    include_credentials: bool,
) -> AppResult<(SharedDbProjection, String)> {
    let connection = u.working().db_connection(id).ok_or_else(invalid)?;
    let mut ids = vec![connection.id];
    if include_credentials {
        ids.extend(connection.auth.secret_ids());
    }
    let revisions: Vec<_> = ids
        .iter()
        .map(|id| (*id, u.working().revision_of(*id)))
        .collect();
    let serialized =
        serde_json::to_vec(&(u.vault_id, &connection, &revisions, include_credentials))
            .map_err(|_| invalid())?;
    let stamp: String = cc_crypto_core::request_body_sha256(&serialized)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    let credentials = if include_credentials {
        let mut values = BTreeMap::new();
        for id in connection.auth.secret_ids() {
            values.insert(id, u.writer.read_secret(id).await?);
        }
        Some(values)
    } else {
        None
    };
    if u.working().db_connection(connection.id).as_ref() != Some(&connection)
        || revisions
            .iter()
            .any(|(id, revision)| u.working().revision_of(*id) != *revision)
    {
        return Err(AppError::invalid(
            "connection",
            "connection changed; review it again",
        ));
    }
    Ok((
        SharedDbProjection::from_connection(&connection, credentials.as_ref())?,
        stamp,
    ))
}

impl AppCore {
    pub async fn sharing_preview_db_connection(
        &self,
        id: String,
        include_credentials: bool,
    ) -> AppResult<DbSharingPreview> {
        let (_, u) = self.unlocked().await?;
        let id = id.parse().map_err(|_| invalid())?;
        let (projection, snapshot_stamp) = prepare(&u, id, include_credentials).await?;
        Ok(DbSharingPreview {
            preview_json: projection.preview_json()?,
            snapshot_stamp,
        })
    }

    /// Saves a personal copy with the receiver's own route. Shared
    /// credentials are imported only on explicit request.
    pub async fn sharing_copy_db_connection(
        &self,
        share_id: String,
        route: DbRoute,
        include_credentials: bool,
    ) -> AppResult<DbConnection> {
        self.sharing_with_verified_projection(
            share_id,
            cc_protocol::sharing::SharedItemKind::DbConnection,
            move |u, _, projection| {
                Box::pin(async move {
                    let crate::sharing_projection::SharedProjection::DbConnection(projection) =
                        projection
                    else {
                        return Err(invalid());
                    };
                    let (connection, secrets) =
                        projection.instantiate(route, include_credentials)?;
                    persist(u, connection, secrets).await
                })
            },
        )
        .await
    }
}

async fn persist(
    u: &crate::session::Unlocked,
    connection: DbConnection,
    secrets: Vec<Secret>,
) -> AppResult<DbConnection> {
    let mut references = connection.clone();
    references.auth = DbAuth::default();
    validate_refs(u.working(), &VaultObject::DbConnection(references))?;
    let expected: std::collections::BTreeSet<_> =
        connection.auth.secret_ids().into_iter().collect();
    let actual: std::collections::BTreeSet<_> = secrets.iter().map(|s| s.id).collect();
    if actual != expected || actual.len() != secrets.len() {
        return Err(invalid());
    }
    if std::iter::once(connection.id)
        .chain(actual.iter().copied())
        .any(|id| u.working().kind_of(id).is_some())
    {
        return Err(AppError::invalid(
            "id",
            "import requires new object identities",
        ));
    }
    let mut objects: Vec<_> = secrets.into_iter().map(VaultObject::Secret).collect();
    objects.push(VaultObject::DbConnection(connection.clone()));
    u.writer.put_batch(objects).await?;
    Ok(connection)
}

impl SharedDbProjection {
    /// `credentials: None` excludes the password and the client key.
    pub fn from_connection(
        connection: &DbConnection,
        credentials: Option<&BTreeMap<ObjectId, Secret>>,
    ) -> AppResult<Self> {
        connection.validate()?;
        let get = |id: &ObjectId| -> AppResult<SecretValue> {
            credentials
                .and_then(|values| values.get(id))
                .filter(|v| !v.value.is_empty())
                .map(|v| v.value.clone())
                .ok_or_else(invalid)
        };
        let authentication = match credentials {
            None => None,
            Some(_) => {
                let auth = SharedDbAuthentication {
                    password: connection
                        .auth
                        .password_secret_id
                        .as_ref()
                        .map(get)
                        .transpose()?,
                    client_key: connection
                        .auth
                        .client_certificate
                        .as_ref()
                        .map(|cert| get(&cert.key_secret_id))
                        .transpose()?,
                };
                (auth.password.is_some() || auth.client_key.is_some()).then_some(auth)
            }
        };
        let projection = Self {
            name: connection.name.clone(),
            engine: connection.engine,
            host: connection.host.clone(),
            port: connection.port,
            database: connection.database.clone(),
            username: connection.username.clone(),
            tls: connection.tls.clone(),
            mechanism: connection.auth.mechanism,
            client_certificate_pem: connection
                .auth
                .client_certificate
                .as_ref()
                .map(|cert| cert.certificate_pem.clone()),
            route: SharedDbRoute::of(&connection.route),
            production: connection.production,
            limits: connection.limits.clone(),
            options: connection.options.clone(),
            tags: connection.tags.clone(),
            color: connection.color.clone(),
            authentication,
        };
        projection.validate()?;
        Ok(projection)
    }

    pub fn requires_route(&self) -> bool {
        self.route != SharedDbRoute::Direct
    }

    /// A placeholder route of the shared shape, for validating the metadata.
    fn placeholder_route(&self) -> DbRoute {
        match &self.route {
            SharedDbRoute::Direct => DbRoute::Direct,
            SharedDbRoute::Relay => DbRoute::Ssh {
                host_id: ObjectId::new(),
            },
            SharedDbRoute::Kubernetes { .. } => DbRoute::Kubernetes {
                context_id: ObjectId::new(),
                namespace: String::new(),
                target_kind: DbKubeTargetKind::Pod,
                target_name: String::new(),
            },
            SharedDbRoute::Docker { .. } => DbRoute::Docker {
                host_id: None,
                container: String::new(),
            },
        }
    }

    pub fn validate(&self) -> AppResult<()> {
        if let Some(auth) = &self.authentication {
            if auth.password.is_none() && auth.client_key.is_none() {
                return Err(invalid());
            }
            for value in [&auth.password, &auth.client_key].into_iter().flatten() {
                valid_secret(value)?;
            }
            if auth.client_key.is_some() && self.client_certificate_pem.is_none() {
                return Err(invalid());
            }
        }
        let route = self.route.resolve(self.placeholder_route())?;
        // Metadata-only shares of certificate authentication stay reviewable;
        // saving a copy of one still requires the shared key.
        let (connection, _) = self.instantiate_with(route, true, false)?;
        connection.validate().map_err(|_| invalid())
    }

    fn instantiate_with(
        &self,
        route: DbRoute,
        include_credentials: bool,
        require_certificate: bool,
    ) -> AppResult<(DbConnection, Vec<Secret>)> {
        let mut connection = DbConnection::new(&self.name, &self.host, self.engine);
        connection.port = self.port;
        connection.database = self.database.clone();
        connection.username = self.username.clone();
        connection.tls = self.tls.clone();
        connection.route = route;
        connection.production = self.production;
        connection.limits = self.limits.clone();
        connection.options = self.options.clone();
        connection.tags = self.tags.clone();
        connection.color = self.color.clone();
        connection.auth.mechanism = self.mechanism;
        let mut secrets = Vec::new();
        let shared = self.authentication.as_ref().filter(|_| include_credentials);
        if let Some(password) = shared.and_then(|auth| auth.password.as_ref()) {
            let secret = Secret::new(SecretKind::Password, password.clone());
            connection.auth.password_secret_id = Some(secret.id);
            secrets.push(secret);
        }
        if let (Some(certificate_pem), Some(key)) = (
            &self.client_certificate_pem,
            shared.and_then(|auth| auth.client_key.as_ref()),
        ) {
            let secret = Secret::new(SecretKind::Other, key.clone());
            connection.auth.client_certificate = Some(DbClientCertificate {
                certificate_pem: certificate_pem.clone(),
                key_secret_id: secret.id,
            });
            secrets.push(secret);
        }
        if self.mechanism == DbAuthMechanism::MongoX509
            && connection.auth.client_certificate.is_none()
        {
            if require_certificate {
                return Err(AppError::invalid(
                    "credentials",
                    "this connection authenticates with a client certificate; import its credentials",
                ));
            }
            connection.auth.mechanism = DbAuthMechanism::Default;
        }
        connection.validate()?;
        Ok((connection, secrets))
    }

    pub fn instantiate(
        &self,
        route: DbRoute,
        include_credentials: bool,
    ) -> AppResult<(DbConnection, Vec<Secret>)> {
        self.validate()?;
        let route = self.route.resolve(route)?;
        // Session permissions, group membership and history start empty.
        self.instantiate_with(route, include_credentials, true)
    }

    /// UI previews deliberately omit secret values.
    pub fn preview_json(&self) -> AppResult<String> {
        let (namespace, target_kind, target_name, container) = match &self.route {
            SharedDbRoute::Kubernetes {
                namespace,
                target_kind,
                target_name,
            } => (
                Some(namespace.as_str()),
                Some(*target_kind),
                Some(target_name.as_str()),
                None,
            ),
            SharedDbRoute::Docker { container } => (None, None, None, Some(container.as_str())),
            SharedDbRoute::Direct | SharedDbRoute::Relay => (None, None, None, None),
        };
        let auth = self.authentication.as_ref();
        serde_json::to_string(&serde_json::json!({
            "kind": "db_connection",
            "data": {
                "name": self.name, "engine": self.engine, "host": self.host, "port": self.port,
                "database": self.database, "username": self.username,
                "tls_mode": self.tls.mode, "production": self.production,
                "route_kind": self.route.wire_name(),
                "route_namespace": namespace, "route_target_kind": target_kind,
                "route_target_name": target_name, "route_container": container,
                "requires_route": self.requires_route(),
                "includes_credentials": auth.is_some(),
                "includes_password": auth.is_some_and(|a| a.password.is_some()),
                "includes_client_key": auth.is_some_and(|a| a.client_key.is_some()),
                "client_certificate": self.client_certificate_pem.is_some(),
                "tags": self.tags,
            },
        }))
        .map_err(|_| invalid())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn connection() -> (DbConnection, Secret) {
        let mut connection = DbConnection::new("orders", "db.example.test", DbEngine::Postgresql);
        connection.username = "app".into();
        connection.database = "orders".into();
        connection.group_id = Some(ObjectId::new());
        let secret = Secret::new(
            SecretKind::Password,
            SecretValue::new(uuid::Uuid::new_v4().to_string()),
        );
        connection.auth.password_secret_id = Some(secret.id);
        (connection, secret)
    }

    #[test]
    fn sharing_excludes_credentials_by_default_and_rehomes_explicit_secrets() {
        let (connection, secret) = connection();
        let default = SharedDbProjection::from_connection(&connection, None).unwrap();
        assert!(default.authentication.is_none());
        let explicit = SharedDbProjection::from_connection(
            &connection,
            Some(&BTreeMap::from([(secret.id, secret.clone())])),
        )
        .unwrap();
        let canary = secret.value.expose_secret();
        assert!(!explicit.preview_json().unwrap().contains(canary));
        assert!(!format!("{explicit:?}").contains(canary));
        let encoded = serde_json::to_string(&explicit).unwrap();
        for id in [connection.id, secret.id, connection.group_id.unwrap()] {
            assert!(!encoded.contains(&id.to_string()));
        }
        let (received, secrets) = explicit.instantiate(DbRoute::Direct, true).unwrap();
        assert_ne!(received.id, connection.id);
        assert!(received.group_id.is_none());
        assert!(received.production);
        assert_eq!(secrets[0].value, secret.value);
        assert_ne!(secrets[0].id, secret.id);
        assert_eq!(received.auth.password_secret_id, Some(secrets[0].id));
        let (without, secrets) = explicit.instantiate(DbRoute::Direct, false).unwrap();
        assert_eq!(without.auth.password_secret_id, None);
        assert!(secrets.is_empty());
        // Credentials can never be imported from a projection that has none.
        let (_, secrets) = default.instantiate(DbRoute::Direct, true).unwrap();
        assert!(secrets.is_empty());
    }

    #[test]
    fn receiving_a_routed_connection_never_silently_connects_directly() {
        let (mut connection, _) = connection();
        connection.route = DbRoute::Ssh {
            host_id: ObjectId::new(),
        };
        let projection = SharedDbProjection::from_connection(&connection, None).unwrap();
        assert!(projection.requires_route());
        assert!(!serde_json::to_string(&projection)
            .unwrap()
            .contains(&connection.route.reference().unwrap().to_string()));
        assert!(projection.instantiate(DbRoute::Direct, false).is_err());
        let jump = DbRoute::Jump {
            jump_profile_id: ObjectId::new(),
        };
        assert_eq!(
            projection.instantiate(jump.clone(), false).unwrap().0.route,
            jump
        );
        assert!(projection
            .instantiate(
                DbRoute::Docker {
                    host_id: None,
                    container: "db".into()
                },
                false
            )
            .is_err());
    }

    #[test]
    fn forwarded_routes_keep_the_shared_target_and_take_the_receivers_object() {
        let mut connection = DbConnection::new("cache", "127.0.0.1", DbEngine::Redis);
        connection.production = false;
        connection.route = DbRoute::Kubernetes {
            context_id: ObjectId::new(),
            namespace: "data".into(),
            target_kind: DbKubeTargetKind::StatefulSet,
            target_name: "redis".into(),
        };
        let projection = SharedDbProjection::from_connection(&connection, None).unwrap();
        let mine = ObjectId::new();
        let (received, _) = projection
            .instantiate(
                DbRoute::Kubernetes {
                    context_id: mine,
                    namespace: "other".into(),
                    target_kind: DbKubeTargetKind::Pod,
                    target_name: "other".into(),
                },
                false,
            )
            .unwrap();
        assert_eq!(
            received.route,
            DbRoute::Kubernetes {
                context_id: mine,
                namespace: "data".into(),
                target_kind: DbKubeTargetKind::StatefulSet,
                target_name: "redis".into(),
            }
        );
        assert!(!received.production);
        connection.route = DbRoute::Docker {
            host_id: Some(ObjectId::new()),
            container: "redis-1".into(),
        };
        let projection = SharedDbProjection::from_connection(&connection, None).unwrap();
        let (received, _) = projection
            .instantiate(
                DbRoute::Docker {
                    host_id: None,
                    container: "ignored".into(),
                },
                false,
            )
            .unwrap();
        assert_eq!(
            received.route,
            DbRoute::Docker {
                host_id: None,
                container: "redis-1".into(),
            }
        );
        assert!(projection.instantiate(DbRoute::Direct, false).is_err());
    }

    #[test]
    fn certificate_authentication_requires_explicit_credentials() {
        let mut connection = DbConnection::new("events", "mongo.example.test", DbEngine::Mongodb);
        let key = Secret::new(SecretKind::Other, SecretValue::new("client-key-canary"));
        connection.auth = DbAuth {
            password_secret_id: None,
            client_certificate: Some(DbClientCertificate {
                certificate_pem: "-----BEGIN CERTIFICATE-----\nAQID\n-----END CERTIFICATE-----\n"
                    .into(),
                key_secret_id: key.id,
            }),
            mechanism: DbAuthMechanism::MongoX509,
        };
        connection.validate().unwrap();
        // Reviewable without the key, but a copy would silently change
        // authentication, so saving one requires the shared key.
        let metadata = SharedDbProjection::from_connection(&connection, None).unwrap();
        assert!(metadata.instantiate(DbRoute::Direct, true).is_err());
        let explicit = SharedDbProjection::from_connection(
            &connection,
            Some(&BTreeMap::from([(key.id, key.clone())])),
        )
        .unwrap();
        assert!(explicit.instantiate(DbRoute::Direct, false).is_err());
        let (received, secrets) = explicit.instantiate(DbRoute::Direct, true).unwrap();
        assert_eq!(
            received.auth.client_certificate.unwrap().key_secret_id,
            secrets[0].id
        );
        assert!(!explicit
            .preview_json()
            .unwrap()
            .contains("client-key-canary"));
    }

    #[test]
    fn tampered_projections_fail_closed() {
        let (connection, _) = connection();
        let mut projection = SharedDbProjection::from_connection(&connection, None).unwrap();
        projection.authentication = Some(SharedDbAuthentication {
            password: None,
            client_key: None,
        });
        assert!(projection.validate().is_err());
        projection.authentication = Some(SharedDbAuthentication {
            password: None,
            client_key: Some(SecretValue::new("key")),
        });
        assert!(projection.validate().is_err());
        projection.authentication = None;
        projection.limits.statement_timeout_seconds = 3600;
        assert!(
            projection.validate().is_err(),
            "production limits stay bounded"
        );
        let mut value =
            serde_json::to_value(SharedDbProjection::from_connection(&connection, None).unwrap())
                .unwrap();
        value["route"] = serde_json::json!({"kind": "ssh", "host_id": ObjectId::new()});
        assert!(serde_json::from_value::<SharedDbProjection>(value).is_err());
    }
}
