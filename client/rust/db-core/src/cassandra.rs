//! Cassandra / ScyllaDB over the CQL native protocol.
//!
//! The driver never opens a socket of its own: it dials one loopback relay
//! that reaches the contact points through the connection's route (direct,
//! SSH, jump, proxy, saved tunnel, Kubernetes or Docker forward) and performs
//! TLS with the connection's own policy. Peers learned from `system.peers`
//! are never dialled: every node address translates to the relay and the host
//! filter enables exactly one node, so one coordinator serves the session and
//! the shard-aware port stays off. Through a forward that is the only node
//! reachable anyway; directly, the relay fails over between the configured
//! contact points.
//!
//! TLS follows the other engines: "verify" checks the chain and name,
//! "require" only encrypts, "prefer" continues without TLS only when the
//! server offers none on a protected path (a tunnel or this computer) and
//! fails with `TlsUnavailable` over the network. CQL password
//! authentication sends the password as is, so it never travels without TLS
//! over the network (`CleartextRefused`).
use crate::{
    cassandra_value::{self, type_name},
    parser::cql::{qualified, quote_name},
    streaming::RowConsumer,
    transport::{DbConnector, DbCredentials, DbDestination, LocalRelay, TlsProfile},
    DbCell, DbColumn, DbError, DbObjectKind, DbObjectRef, DbParameter, DbTransportSecurity,
    DbTreeNode, DbTreePage,
};
use async_trait::async_trait;
use cc_models::database::{CqlConsistency, DbConnection, DbOptions, DbRoute, DbTlsMode};
use futures::future::BoxFuture;
use scylla::{
    client::{
        execution_profile::{ExecutionProfile, ExecutionProfileHandle},
        session::Session,
        session_builder::SessionBuilder,
        PoolSize, SelfIdentity,
    },
    cluster::metadata::Peer,
    errors::TranslationError,
    observability::tracing::TracingInfo,
    policies::{
        address_translator::{AddressTranslator, UntranslatedPeer},
        host_filter::HostFilter,
        load_balancing::DefaultPolicy,
    },
    response::PagingState,
    statement::{prepared::PreparedStatement, unprepared::Statement, Consistency},
    value::{CqlValue, Row},
};
use secrecy::ExposeSecret;
use std::{
    collections::HashMap,
    fmt,
    net::SocketAddr,
    num::NonZeroUsize,
    ops::ControlFlow,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

/// Most catalogue rows read for one structure or completion lookup.
const MAX_CATALOGUE_ROWS: usize = 20_000;

/// Most trace events kept (a scan touching many nodes logs thousands).
const MAX_TRACE_EVENTS: usize = 2_000;

/// A query trace for the plan view: the coordinator's session and each
/// node's events in the order they happened.
fn trace_json(info: &TracingInfo) -> serde_json::Value {
    let mut nodes: Vec<(String, Vec<serde_json::Value>)> = Vec::new();
    for event in info.events.iter().take(MAX_TRACE_EVENTS) {
        let source = event.source.map(|ip| ip.to_string()).unwrap_or_default();
        let entry = serde_json::json!({
            "activity": event.activity,
            "elapsed_us": event.source_elapsed,
            "thread": event.thread,
        });
        match nodes.iter_mut().find(|(known, _)| *known == source) {
            Some((_, events)) => events.push(entry),
            None => nodes.push((source, vec![entry])),
        }
    }
    serde_json::json!({
        "cql_trace": {
            "request": info.request,
            "coordinator": info.coordinator.map(|ip| ip.to_string()),
            "duration_us": info.duration,
            "started_at": info.started_at.map(|at| at.0),
            "parameters": info.parameters,
            "truncated": info.events.len() > MAX_TRACE_EVENTS,
            "nodes": nodes
                .into_iter()
                .map(|(source, events)| serde_json::json!({"source": source, "events": events}))
                .collect::<Vec<_>>(),
        }
    })
}

pub(crate) struct Cassandra {
    session: Session,
    _relay: LocalRelay,
    read: Consistency,
    write: Consistency,
    page_size: i32,
    pub version: String,
    pub security: DbTransportSecurity,
    pub datacenter: String,
}
impl fmt::Debug for Cassandra {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Cassandra([SCOPED])")
    }
}

/// Every node the cluster announces is reached through the relay.
struct ToRelay(SocketAddr);
#[async_trait]
impl AddressTranslator for ToRelay {
    async fn translate_address(
        &self,
        _: &UntranslatedPeer,
    ) -> Result<SocketAddr, TranslationError> {
        Ok(self.0)
    }
}

/// Most cluster nodes a direct session opens relays to.
const MAX_PEERS: usize = 64;

/// A direct route reaches every node (FEAT-12): each one the cluster
/// announces gets its own loopback relay with the session's TLS, opened when
/// the driver first dials it. The contact node keeps the probed relay.
struct PeerRelays {
    /// The contact node's host id and its probed relay.
    contact: (Uuid, SocketAddr),
    /// The cluster's CQL port: the driver gives peers the port it dialed,
    /// which is the loopback relay's.
    port: u16,
    connector: Arc<dyn DbConnector>,
    tls: Option<TlsProfile>,
    scope: CancellationToken,
    relays: tokio::sync::Mutex<HashMap<SocketAddr, LocalRelay>>,
}
#[async_trait]
impl AddressTranslator for PeerRelays {
    async fn translate_address(
        &self,
        peer: &UntranslatedPeer,
    ) -> Result<SocketAddr, TranslationError> {
        if peer.host_id() == self.contact.0 {
            return Ok(self.contact.1);
        }
        let target = SocketAddr::new(peer.untranslated_address().ip(), self.port);
        let refused = TranslationError::NoRuleForAddress(target);
        // A node announcing a loopback or unspecified address is misconfigured
        // for clients; dialing it would reach this computer instead.
        if target.ip().is_loopback() || target.ip().is_unspecified() {
            return Err(refused);
        }
        let mut relays = self.relays.lock().await;
        if let Some(relay) = relays.get(&target) {
            return Ok(relay.address);
        }
        if relays.len() >= MAX_PEERS {
            return Err(refused);
        }
        let destination =
            DbDestination::new(target.ip().to_string(), target.port()).map_err(|_| refused)?;
        let relay = LocalRelay::failover(
            self.connector.clone(),
            vec![destination],
            self.tls.clone(),
            self.scope.clone(),
        )
        .await
        .map_err(|_| TranslationError::NoRuleForAddress(target))?;
        let address = relay.address;
        relays.insert(target, relay);
        Ok(address)
    }
}

/// Nodes of the contact node's datacenter (LOCAL_* consistency and the
/// configured local datacenter refer to it).
struct InDatacenter(String);
impl HostFilter for InDatacenter {
    fn accept(&self, peer: &Peer) -> bool {
        peer.datacenter.as_deref() == Some(self.0.as_str())
    }
}

/// Enables one node (the first one the driver asks about) so the session
/// keeps a single pool behind the relay.
#[derive(Default)]
struct OneCoordinator(Mutex<Option<Uuid>>);
impl OneCoordinator {
    fn admit(&self, host: Uuid) -> bool {
        let mut pinned = self.0.lock().unwrap_or_else(|e| e.into_inner());
        *pinned.get_or_insert(host) == host
    }
}
impl HostFilter for OneCoordinator {
    fn accept(&self, peer: &Peer) -> bool {
        self.admit(peer.host_id)
    }
}

pub(crate) const fn consistency(level: CqlConsistency) -> Consistency {
    match level {
        CqlConsistency::Any => Consistency::Any,
        CqlConsistency::One => Consistency::One,
        CqlConsistency::Two => Consistency::Two,
        CqlConsistency::Three => Consistency::Three,
        CqlConsistency::Quorum => Consistency::Quorum,
        CqlConsistency::All => Consistency::All,
        CqlConsistency::LocalQuorum => Consistency::LocalQuorum,
        CqlConsistency::EachQuorum => Consistency::EachQuorum,
        CqlConsistency::LocalOne => Consistency::LocalOne,
    }
}

/// Maps a driver error without its text (which may echo data).
pub(crate) fn classify(error: &(dyn std::error::Error + 'static)) -> DbError {
    let mut current: Option<&(dyn std::error::Error + 'static)> = Some(error);
    while let Some(error) = current {
        if let Some(scylla::errors::ExecutionError::RequestTimeout(_)) = error.downcast_ref() {
            return DbError::Timeout;
        }
        if let Some(scylla::errors::RequestAttemptError::DbError(server, _)) = error.downcast_ref()
        {
            return server_error(server);
        }
        current = error.source();
    }
    let text = error.to_string().to_ascii_lowercase();
    if text.contains("authentication") || text.contains("bad credentials") {
        DbError::AuthenticationFailed
    } else if text.contains("unauthorized") || text.contains("permission") {
        DbError::PolicyDenied
    } else if text.contains("timed out") || text.contains("timeout") {
        DbError::Timeout
    } else if text.contains("connection") || text.contains("broken") || text.contains("known nodes")
    {
        DbError::ConnectionFailed
    } else {
        DbError::QueryFailed
    }
}

fn server_error(error: &scylla::errors::DbError) -> DbError {
    use scylla::errors::DbError as Server;
    match error {
        Server::AuthenticationError => DbError::AuthenticationFailed,
        Server::Unauthorized => DbError::PolicyDenied,
        Server::ReadTimeout { .. } | Server::WriteTimeout { .. } => DbError::Timeout,
        _ => DbError::QueryFailed,
    }
}

async fn cancellable<T, E: std::error::Error + 'static>(
    cancel: &CancellationToken,
    operation: impl std::future::Future<Output = Result<T, E>>,
) -> Result<T, DbError> {
    tokio::select! {
        biased;
        _ = cancel.cancelled() => Err(DbError::Cancelled),
        result = operation => result.map_err(|error| classify(&error)),
    }
}

/// Text of a catalogue value: scalars as the grid shows them, collections
/// and user types as JSON.
fn text(value: Option<&CqlValue>) -> String {
    match &cassandra_value::cell(value) {
        Ok(DbCell::Null) | Err(_) => String::new(),
        Ok(DbCell::Bool(value)) => value.to_string(),
        Ok(
            DbCell::Int(v)
            | DbCell::Uint(v)
            | DbCell::Decimal(v)
            | DbCell::Float(v)
            | DbCell::Text(v)
            | DbCell::Date(v)
            | DbCell::Time(v)
            | DbCell::Timestamp(v)
            | DbCell::Binary(v)
            | DbCell::Json(v),
        ) => v.clone(),
    }
}

/// A CQL literal of a schema option value (`{'class': '…'}`, `0.01`, `'…'`).
pub(crate) fn option_literal(value: &CqlValue) -> String {
    let quote = |text: &str| format!("'{}'", text.replace('\'', "''"));
    match value {
        CqlValue::Text(text) | CqlValue::Ascii(text) => quote(text),
        CqlValue::Map(entries) => format!(
            "{{{}}}",
            entries
                .iter()
                .map(|(key, value)| format!("{}: {}", option_literal(key), option_literal(value)))
                .collect::<Vec<_>>()
                .join(", ")
        ),
        CqlValue::Set(items) => format!(
            "{{{}}}",
            items
                .iter()
                .map(option_literal)
                .collect::<Vec<_>>()
                .join(", ")
        ),
        CqlValue::List(items) => format!(
            "[{}]",
            items
                .iter()
                .map(option_literal)
                .collect::<Vec<_>>()
                .join(", ")
        ),
        CqlValue::Blob(bytes) => cassandra_value::literal(&DbCell::Binary(base64::Engine::encode(
            &base64::engine::general_purpose::STANDARD,
            bytes,
        ))),
        other => text(Some(other)),
    }
}

/// One column of a CQL table definition.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CqlTableColumn {
    pub name: String,
    pub type_name: String,
    pub kind: CqlColumnKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CqlColumnKind {
    PartitionKey,
    Clustering { descending: bool },
    Static,
    Regular,
}

/// `CREATE TABLE` for a table definition: the partition key in its own
/// parentheses, clustering columns in key order with their `CLUSTERING
/// ORDER`, then `options` (already CQL literals) in the given order. Columns
/// keep the order given; key columns are listed in key order.
pub fn create_table_cql(
    keyspace: &str,
    table: &str,
    columns: &[CqlTableColumn],
    options: &[(String, String)],
) -> Result<String, DbError> {
    let partition: Vec<&CqlTableColumn> = columns
        .iter()
        .filter(|column| column.kind == CqlColumnKind::PartitionKey)
        .collect();
    if partition.is_empty() || columns.iter().any(|column| column.name.is_empty()) {
        return Err(DbError::InvalidRequest);
    }
    let clustering: Vec<&CqlTableColumn> = columns
        .iter()
        .filter(|column| matches!(column.kind, CqlColumnKind::Clustering { .. }))
        .collect();
    let mut lines: Vec<String> = columns
        .iter()
        .map(|column| {
            format!(
                "    {} {}{}",
                quote_name(&column.name),
                column.type_name,
                if column.kind == CqlColumnKind::Static {
                    " static"
                } else {
                    ""
                }
            )
        })
        .collect();
    let names = |list: &[&CqlTableColumn]| {
        list.iter()
            .map(|column| quote_name(&column.name))
            .collect::<Vec<_>>()
            .join(", ")
    };
    let mut key = format!("({})", names(&partition));
    if !clustering.is_empty() {
        key.push_str(", ");
        key.push_str(&names(&clustering));
    }
    lines.push(format!("    PRIMARY KEY ({key})"));
    let mut with = Vec::new();
    if !clustering.is_empty() {
        with.push(format!(
            "CLUSTERING ORDER BY ({})",
            clustering
                .iter()
                .map(|column| format!(
                    "{} {}",
                    quote_name(&column.name),
                    match column.kind {
                        CqlColumnKind::Clustering { descending: true } => "DESC",
                        _ => "ASC",
                    }
                ))
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    with.extend(
        options
            .iter()
            .map(|(name, value)| format!("{name} = {value}")),
    );
    let mut ddl = format!(
        "CREATE TABLE {} (\n{}\n)",
        qualified(keyspace, table),
        lines.join(",\n")
    );
    if !with.is_empty() {
        ddl.push_str(" WITH ");
        ddl.push_str(&with.join("\n    AND "));
    }
    ddl.push(';');
    Ok(ddl)
}

/// Rows in batches of at most `per_batch` statements for a CSV import or a
/// generated script: `BEGIN UNLOGGED BATCH … APPLY BATCH;` per group of
/// `INSERT` statements (`statements` are complete, without `;`).
pub fn batch_cql(statements: &[String], per_batch: usize) -> Vec<String> {
    statements
        .chunks(per_batch.clamp(1, 1000))
        .map(|chunk| {
            format!(
                "BEGIN UNLOGGED BATCH\n{}\nAPPLY BATCH;",
                chunk
                    .iter()
                    .map(|statement| format!("  {statement};"))
                    .collect::<Vec<_>>()
                    .join("\n")
            )
        })
        .collect()
}

fn column_kind(kind: &str, order: &str) -> CqlColumnKind {
    match kind {
        "partition_key" => CqlColumnKind::PartitionKey,
        "clustering" => CqlColumnKind::Clustering {
            descending: order.eq_ignore_ascii_case("desc"),
        },
        "static" => CqlColumnKind::Static,
        _ => CqlColumnKind::Regular,
    }
}

/// A table's columns from `system_schema.columns` rows (name, type, kind,
/// position, clustering_order) in definition order: key columns by
/// position, then static and regular columns by name.
pub(crate) fn table_columns(rows: &[Vec<String>]) -> Vec<(CqlTableColumn, i32)> {
    let mut columns: Vec<(CqlTableColumn, i32)> = rows
        .iter()
        .filter(|row| row.len() >= 5)
        .map(|row| {
            (
                CqlTableColumn {
                    name: row[0].clone(),
                    type_name: row[1].clone(),
                    kind: column_kind(&row[2], &row[4]),
                },
                row[3].parse().unwrap_or(-1),
            )
        })
        .collect();
    let rank = |kind: CqlColumnKind| match kind {
        CqlColumnKind::PartitionKey => 0,
        CqlColumnKind::Clustering { .. } => 1,
        CqlColumnKind::Static => 2,
        CqlColumnKind::Regular => 3,
    };
    columns.sort_by(|(a, pa), (b, pb)| {
        rank(a.kind)
            .cmp(&rank(b.kind))
            .then(if rank(a.kind) < 2 {
                pa.cmp(pb)
            } else {
                std::cmp::Ordering::Equal
            })
            .then(a.name.cmp(&b.name))
    });
    columns
}

/// Short replication text for the tree (`SimpleStrategy 1`, `dc1:3 dc2:2`).
fn replication_summary(json: &str) -> String {
    let Ok(serde_json::Value::Object(map)) = serde_json::from_str::<serde_json::Value>(json) else {
        return String::new();
    };
    let class = map
        .get("class")
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .rsplit('.')
        .next()
        .unwrap_or_default()
        .to_owned();
    let mut parts: Vec<String> = map
        .iter()
        .filter(|(key, _)| key.as_str() != "class")
        .map(|(key, value)| {
            let value = value
                .as_str()
                .map_or_else(|| value.to_string(), str::to_owned);
            if key == "replication_factor" {
                value
            } else {
                format!("{key}:{value}")
            }
        })
        .collect();
    parts.insert(0, class);
    parts.join(" ")
}

impl Cassandra {
    pub async fn connect(
        connection: &DbConnection,
        credentials: &DbCredentials,
        connector: Arc<dyn DbConnector>,
        scope: CancellationToken,
    ) -> Result<Self, DbError> {
        let DbOptions::Cassandra {
            contact_points,
            local_datacenter,
            read_consistency,
            write_consistency,
            page_size,
        } = &connection.options
        else {
            return Err(DbError::InvalidRequest);
        };
        let _ = rustls::crypto::ring::default_provider().install_default();
        // A Kubernetes or Docker forward reaches exactly one endpoint.
        let mut hosts = vec![connection.host.clone()];
        if !matches!(
            connection.route,
            DbRoute::Kubernetes { .. } | DbRoute::Docker { .. }
        ) {
            hosts.extend(contact_points.iter().cloned());
        }
        let destinations = hosts
            .iter()
            .map(|host| DbDestination::new(host.clone(), connection.port))
            .collect::<Result<Vec<_>, _>>()?;
        let mode = connection.tls.mode;
        let tls = if mode == DbTlsMode::Disable {
            None
        } else {
            Some(TlsProfile::new(connection, credentials)?)
        };
        // Probe the contact points in order: the first that answers decides
        // whether this session runs over TLS.
        let mut failure = DbError::ConnectionFailed;
        let mut chosen = None;
        for (index, destination) in destinations.iter().enumerate() {
            let (stream, path) = match connector
                .connect_path(destination, scope.child_token())
                .await
            {
                Ok(opened) => opened,
                Err(DbError::Cancelled) => return Err(DbError::Cancelled),
                Err(error) => {
                    failure = error;
                    continue;
                }
            };
            let encrypted = match &tls {
                None => false,
                Some(profile) => match profile
                    .connect_checked(stream, Some(&destination.host))
                    .await
                {
                    Ok(_) => true,
                    Err(DbError::TlsUnavailable)
                        if mode == DbTlsMode::Prefer && path.protected() =>
                    {
                        false
                    }
                    Err(error) => return Err(error),
                },
            };
            chosen = Some((index, path, encrypted));
            break;
        }
        let (index, path, encrypted) = chosen.ok_or(failure)?;
        if !encrypted && !path.protected() && credentials.password.is_some() {
            return Err(DbError::CleartextRefused);
        }
        let security = DbTransportSecurity::of(mode, !encrypted, path);
        // Direct: other nodes are dialed on their own, with the same TLS.
        // Tunnels and forwards reach one endpoint: one coordinator there.
        let peer_dialer = (connection.route == DbRoute::Direct).then(|| {
            (
                connector.clone(),
                if encrypted { tls.clone() } else { None },
            )
        });
        let relay = if encrypted {
            let mut ordered = destinations.clone();
            ordered.rotate_left(index);
            LocalRelay::failover(connector, ordered, tls, scope.clone()).await?
        } else {
            // Without TLS only the probed endpoint (whose path is known).
            LocalRelay::failover(
                connector,
                vec![destinations[index].clone()],
                None,
                scope.clone(),
            )
            .await?
        };
        let timeout = Duration::from_secs(u64::from(connection.limits.statement_timeout_seconds));
        let profile = ExecutionProfile::builder()
            .consistency(consistency(*read_consistency))
            .request_timeout(Some(timeout));
        let contact = relay.address;
        let open = |translator: Arc<dyn AddressTranslator>,
                    filter: Arc<dyn HostFilter>,
                    profile: ExecutionProfileHandle| {
            let mut builder = SessionBuilder::new()
                .known_node_addr(contact)
                .connection_timeout(Duration::from_secs(u64::from(
                    connection.limits.connect_timeout_seconds.clamp(1, 120),
                )))
                .pool_size(PoolSize::PerHost(NonZeroUsize::MIN))
                .disallow_shard_aware_port(true)
                .address_translator(translator)
                .host_filter(filter)
                .fetch_schema_metadata(false)
                .compression(None)
                .default_execution_profile_handle(profile)
                .custom_identity(SelfIdentity::new().with_application_name("ConsoleCrypt"));
            if !connection.username.is_empty() || credentials.password.is_some() {
                builder = builder.user(
                    connection.username.clone(),
                    credentials
                        .password
                        .as_ref()
                        .map(|password| password.expose_secret().to_owned())
                        .unwrap_or_default(),
                );
            }
            if !connection.database.is_empty() {
                builder = builder.use_keyspace(connection.database.clone(), true);
            }
            let scope = scope.clone();
            async move {
                tokio::select! {
                    biased;
                    _ = scope.cancelled() => Err(DbError::Cancelled),
                    result = builder.build() => result.map_err(|error| classify(&error)),
                }
            }
        };
        let session = open(
            Arc::new(ToRelay(contact)),
            Arc::new(OneCoordinator::default()),
            profile.clone().build().into_handle(),
        )
        .await?;
        let mut driver = Self {
            session,
            _relay: relay,
            read: consistency(*read_consistency),
            write: consistency(*write_consistency),
            page_size: i32::try_from(*page_size).unwrap_or(500).clamp(1, 10_000),
            version: String::new(),
            security,
            datacenter: String::new(),
        };
        let cancel = scope.child_token();
        let local = driver
            .catalogue(
                "SELECT release_version, data_center, host_id FROM system.local WHERE key = 'local'",
                &[],
                &cancel,
            )
            .await?;
        let local = local.first().ok_or(DbError::QueryFailed)?;
        driver.datacenter = local.get(1).cloned().unwrap_or_default();
        // ScyllaDB reports a compatible release_version and its own version
        // in system.versions; Cassandra has no such table.
        driver.version = match driver
            .catalogue(
                "SELECT version FROM system.versions WHERE key = 'local'",
                &[],
                &cancel,
            )
            .await
        {
            Ok(rows) if rows.first().is_some_and(|row| !row[0].is_empty()) => {
                format!("ScyllaDB {}", rows[0][0])
            }
            _ => format!("Apache Cassandra {}", local[0]),
        };
        if let Some(expected) = local_datacenter {
            if !expected.is_empty() && *expected != driver.datacenter {
                return Err(DbError::DatacenterMismatch);
            }
        }
        // A cluster of several nodes (FEAT-12): a second session spreads
        // requests over the nodes of this datacenter and fails over when the
        // coordinator goes down. The contact node keeps the probed relay.
        let host = local.get(2).and_then(|id| Uuid::parse_str(id).ok());
        if let (Some((connector, tls)), Some(host)) = (peer_dialer, host) {
            let peers = driver
                .catalogue("SELECT peer FROM system.peers", &[], &cancel)
                .await
                .unwrap_or_default();
            if !peers.is_empty() && !driver.datacenter.is_empty() {
                let relays = PeerRelays {
                    contact: (host, contact),
                    port: connection.port,
                    connector,
                    tls,
                    scope: scope.clone(),
                    relays: tokio::sync::Mutex::new(HashMap::new()),
                };
                let balanced = profile
                    .load_balancing_policy(
                        DefaultPolicy::builder()
                            .prefer_datacenter(driver.datacenter.clone())
                            .permit_dc_failover(false)
                            .build(),
                    )
                    .build()
                    .into_handle();
                match open(
                    Arc::new(relays),
                    Arc::new(InDatacenter(driver.datacenter.clone())),
                    balanced,
                )
                .await
                {
                    Ok(session) => driver.session = session,
                    Err(DbError::Cancelled) => return Err(DbError::Cancelled),
                    // The single-coordinator session still works.
                    Err(_) => {}
                }
            }
        }
        Ok(driver)
    }

    /// The keyspace unqualified names refer to (`USE` or the connection's).
    pub(crate) fn keyspace(&self) -> Option<String> {
        self.session
            .get_keyspace()
            .map(|keyspace| keyspace.to_string())
    }

    /// Prepared when parameters are bound; values follow the server's types.
    async fn prepared(
        &self,
        text: &str,
        parameters: &[DbParameter],
        consistency: Consistency,
        cancel: &CancellationToken,
    ) -> Result<(PreparedStatement, Vec<Option<CqlValue>>), DbError> {
        let mut statement = Statement::new(text.to_owned());
        statement.set_consistency(consistency);
        statement.set_page_size(self.page_size);
        let prepared = cancellable(cancel, self.session.prepare(statement)).await?;
        let specs = prepared.get_variable_col_specs();
        if specs.len() != parameters.len() {
            return Err(DbError::InvalidRequest);
        }
        let values = specs
            .iter()
            .zip(parameters)
            .map(|(spec, parameter)| cassandra_value::bind(&parameter.value, spec.typ()))
            .collect::<Result<Vec<_>, _>>()?;
        Ok((prepared, values))
    }

    /// Runs one statement page by page into `sink`; the next page is read
    /// only when the sink asks for more rows.
    pub(crate) fn run<'a>(
        &'a mut self,
        text: &'a str,
        parameters: &'a [DbParameter],
        keyspace: Option<&'a str>,
        read_only: bool,
        sink: &'a mut (dyn RowConsumer + Send),
        cancel: CancellationToken,
    ) -> BoxFuture<'a, Result<(), DbError>> {
        Box::pin(async move {
            let started = Instant::now();
            if let Some(keyspace) = keyspace {
                cancellable(&cancel, self.session.use_keyspace(keyspace, true)).await?;
                sink.start(Vec::new(), None).await?;
                return sink
                    .statement_done(started.elapsed().as_millis() as u64, None)
                    .await;
            }
            let consistency = if read_only { self.read } else { self.write };
            let prepared = if parameters.is_empty() {
                None
            } else {
                Some(
                    self.prepared(text, parameters, consistency, &cancel)
                        .await?,
                )
            };
            let mut statement = Statement::new(text.to_owned());
            statement.set_consistency(consistency);
            statement.set_page_size(self.page_size);
            let mut paging = PagingState::start();
            let mut columns: Option<usize> = None;
            loop {
                let (result, next) = match &prepared {
                    Some((prepared, values)) => {
                        cancellable(
                            &cancel,
                            self.session.execute_single_page(prepared, values, paging),
                        )
                        .await?
                    }
                    None => {
                        cancellable(
                            &cancel,
                            self.session
                                .query_single_page(statement.clone(), (), paging),
                        )
                        .await?
                    }
                };
                for warning in result.warnings() {
                    sink.notice(warning.chars().take(4096).collect()).await?;
                }
                if !result.is_rows() {
                    if columns.is_none() {
                        sink.start(Vec::new(), None).await?;
                    }
                    break;
                }
                let rows = result
                    .into_rows_result()
                    .map_err(|_| DbError::QueryFailed)?;
                if columns.is_none() {
                    let specs: Vec<DbColumn> = rows
                        .column_specs()
                        .iter()
                        .map(|spec| DbColumn {
                            name: spec.name().to_owned(),
                            type_name: type_name(spec.typ()),
                            nullable: true,
                            sensitive: crate::sensitive_column(spec.name()),
                            primary_key: false,
                        })
                        .collect();
                    columns = Some(specs.len());
                    sink.start(specs, None).await?;
                }
                for row in rows.rows::<Row>().map_err(|_| DbError::QueryFailed)? {
                    let row = row.map_err(|_| DbError::QueryFailed)?;
                    let cells = row
                        .columns
                        .iter()
                        .map(|value| cassandra_value::cell(value.as_ref()))
                        .collect::<Result<Vec<_>, _>>()?;
                    if !sink.row(cells).await? {
                        return Err(DbError::ResultLimit);
                    }
                }
                match next.into_paging_control_flow() {
                    ControlFlow::Break(()) => break,
                    ControlFlow::Continue(state) => paging = state,
                }
            }
            sink.statement_done(started.elapsed().as_millis() as u64, None)
                .await
        })
    }

    /// One `UPDATE … IF EXISTS` of a cell edit; true when it changed a row.
    pub(crate) async fn conditional(
        &self,
        text: &str,
        parameters: &[DbParameter],
        cancel: &CancellationToken,
    ) -> Result<bool, DbError> {
        let (prepared, values) = self.prepared(text, parameters, self.write, cancel).await?;
        let result = cancellable(cancel, self.session.execute_unpaged(&prepared, &values)).await?;
        let rows = result
            .into_rows_result()
            .map_err(|_| DbError::QueryFailed)?;
        let applied = rows
            .rows::<Row>()
            .map_err(|_| DbError::QueryFailed)?
            .next()
            .and_then(Result::ok)
            .and_then(|row| row.columns.into_iter().next().flatten());
        Ok(matches!(applied, Some(CqlValue::Boolean(true))))
    }

    /// Runs one statement with tracing on (FEAT-14, CQL has no EXPLAIN) and
    /// returns the trace: the coordinator's session and every node's events
    /// in order. Only the first page is read; the rows are dropped.
    pub(crate) async fn trace(
        &self,
        text: &str,
        parameters: &[DbParameter],
        read_only: bool,
        cancel: &CancellationToken,
    ) -> Result<serde_json::Value, DbError> {
        let consistency = if read_only { self.read } else { self.write };
        let (result, _) = if parameters.is_empty() {
            let mut statement = Statement::new(text.to_owned());
            statement.set_consistency(consistency);
            statement.set_page_size(self.page_size);
            statement.set_tracing(true);
            cancellable(
                cancel,
                self.session
                    .query_single_page(statement, (), PagingState::start()),
            )
            .await?
        } else {
            let (mut prepared, values) =
                self.prepared(text, parameters, consistency, cancel).await?;
            prepared.set_tracing(true);
            cancellable(
                cancel,
                self.session
                    .execute_single_page(&prepared, &values, PagingState::start()),
            )
            .await?
        };
        let id = result.tracing_id().ok_or(DbError::QueryFailed)?;
        let info = cancellable(cancel, self.session.get_tracing_info(&id)).await?;
        Ok(trace_json(&info))
    }

    /// One CSV import write (FEAT-09): a conditional `… IF NOT EXISTS`
    /// answers 1 when it wrote the row and 0 when the key existed; a plain
    /// write answers 0 (its rows are counted by the caller).
    pub(crate) async fn import_step(
        &self,
        text: &str,
        parameters: &[DbParameter],
        cancel: CancellationToken,
    ) -> Result<u64, DbError> {
        if text
            .trim_end()
            .trim_end_matches(';')
            .ends_with("IF NOT EXISTS")
        {
            return Ok(u64::from(
                self.conditional(text, parameters, &cancel).await?,
            ));
        }
        let (prepared, values) = self.prepared(text, parameters, self.write, &cancel).await?;
        cancellable(&cancel, self.session.execute_unpaged(&prepared, &values)).await?;
        Ok(0)
    }

    /// Catalogue rows as text, every page up to a bound.
    pub(crate) async fn catalogue(
        &self,
        cql: &str,
        values: &[&str],
        cancel: &CancellationToken,
    ) -> Result<Vec<Vec<String>>, DbError> {
        Ok(self
            .catalogue_values(cql, values, cancel)
            .await?
            .1
            .iter()
            .map(|row| row.iter().map(|value| text(value.as_ref())).collect())
            .collect())
    }

    /// Catalogue rows with their column names and typed values.
    pub(crate) async fn catalogue_values(
        &self,
        cql: &str,
        values: &[&str],
        cancel: &CancellationToken,
    ) -> Result<(Vec<String>, Vec<Vec<Option<CqlValue>>>), DbError> {
        let mut statement = Statement::new(cql.to_owned());
        statement.set_consistency(self.read);
        statement.set_page_size(1000);
        let prepared = if values.is_empty() {
            None
        } else {
            Some(cancellable(cancel, self.session.prepare(statement.clone())).await?)
        };
        let values: Vec<String> = values.iter().map(|value| (*value).to_owned()).collect();
        let mut names = Vec::new();
        let mut output = Vec::new();
        let mut paging = PagingState::start();
        loop {
            let (result, next) = match &prepared {
                Some(prepared) => {
                    cancellable(
                        cancel,
                        self.session.execute_single_page(prepared, &values, paging),
                    )
                    .await?
                }
                None => {
                    cancellable(
                        cancel,
                        self.session
                            .query_single_page(statement.clone(), (), paging),
                    )
                    .await?
                }
            };
            let rows = result
                .into_rows_result()
                .map_err(|_| DbError::QueryFailed)?;
            if names.is_empty() {
                names = rows
                    .column_specs()
                    .iter()
                    .map(|spec| spec.name().to_owned())
                    .collect();
            }
            for row in rows.rows::<Row>().map_err(|_| DbError::QueryFailed)? {
                output.push(row.map_err(|_| DbError::QueryFailed)?.columns);
                if output.len() >= MAX_CATALOGUE_ROWS {
                    return Ok((names, output));
                }
            }
            match next.into_paging_control_flow() {
                ControlFlow::Break(()) => break,
                ControlFlow::Continue(state) => paging = state,
            }
        }
        Ok((names, output))
    }

    /// Columns of a table or view in definition order.
    pub(crate) async fn columns(
        &self,
        keyspace: &str,
        table: &str,
        cancel: &CancellationToken,
    ) -> Result<Vec<CqlTableColumn>, DbError> {
        let rows = self
            .catalogue(
                "SELECT column_name, type, kind, position, clustering_order FROM system_schema.columns \
                 WHERE keyspace_name = ? AND table_name = ?",
                &[keyspace, table],
                cancel,
            )
            .await?;
        Ok(table_columns(&rows)
            .into_iter()
            .map(|(column, _)| column)
            .collect())
    }

    /// For the table designer: the table's comment, default TTL and
    /// gc_grace_seconds, and its secondary indexes on one plain column as
    /// (index, column). Custom and collection indexes are left out.
    pub(crate) async fn design_metadata(
        &self,
        keyspace: &str,
        table: &str,
        cancel: &CancellationToken,
    ) -> Result<(Vec<String>, Vec<(String, String)>), DbError> {
        let options = self
            .catalogue(
                "SELECT comment, default_time_to_live, gc_grace_seconds FROM system_schema.tables \
                 WHERE keyspace_name = ? AND table_name = ?",
                &[keyspace, table],
                cancel,
            )
            .await?
            .into_iter()
            .next()
            .unwrap_or_default();
        let mut indexes = Vec::new();
        for index in self
            .catalogue_values(
                "SELECT index_name, kind, options FROM system_schema.indexes \
                 WHERE keyspace_name = ? AND table_name = ?",
                &[keyspace, table],
                cancel,
            )
            .await?
            .1
        {
            if text(index[1].as_ref()).eq_ignore_ascii_case("CUSTOM") {
                continue;
            }
            let target = serde_json::from_str::<serde_json::Value>(&text(index[2].as_ref()))
                .ok()
                .and_then(|o| o.get("target").and_then(|t| t.as_str()).map(str::to_owned))
                .unwrap_or_default();
            let column = match target.strip_prefix('"').and_then(|t| t.strip_suffix('"')) {
                Some(quoted) => quoted.replace("\"\"", "\""),
                None if !target.is_empty()
                    && target
                        .bytes()
                        .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_') =>
                {
                    target
                }
                None => continue,
            };
            indexes.push((text(index[0].as_ref()), column));
        }
        Ok((options, indexes))
    }

    /// Structure below `parent`: keyspaces, then their tables, views, user
    /// types, functions and aggregates, then columns, indexes and fields.
    pub(crate) fn browse<'a>(
        &'a mut self,
        parent: Option<&'a DbObjectRef>,
        cancel: CancellationToken,
    ) -> BoxFuture<'a, Result<DbTreePage, DbError>> {
        Box::pin(async move {
            let nodes = match parent {
                None => self.keyspaces(&cancel).await?,
                Some(parent) => match parent.kind {
                    DbObjectKind::Database => self.keyspace_objects(&parent.name, &cancel).await?,
                    DbObjectKind::Table | DbObjectKind::MaterializedView => {
                        self.table_children(parent, &cancel).await?
                    }
                    DbObjectKind::UserType => self.type_fields(parent, &cancel).await?,
                    _ => Vec::new(),
                },
            };
            Ok(DbTreePage {
                nodes,
                next_cursor: None,
            })
        })
    }

    async fn keyspaces(&self, cancel: &CancellationToken) -> Result<Vec<DbTreeNode>, DbError> {
        let mut rows = self
            .catalogue(
                "SELECT keyspace_name, replication FROM system_schema.keyspaces",
                &[],
                cancel,
            )
            .await?;
        // Application keyspaces first, system keyspaces last.
        rows.sort_by(|a, b| {
            (a[0].starts_with("system"), &a[0]).cmp(&(b[0].starts_with("system"), &b[0]))
        });
        Ok(rows
            .iter()
            .map(|row| {
                node(
                    DbObjectRef {
                        database: row[0].clone(),
                        schema: None,
                        kind: DbObjectKind::Database,
                        name: row[0].clone(),
                        parent_name: None,
                    },
                    true,
                    Some(replication_summary(&row[1])).filter(|s| !s.is_empty()),
                )
            })
            .collect())
    }

    async fn keyspace_objects(
        &self,
        keyspace: &str,
        cancel: &CancellationToken,
    ) -> Result<Vec<DbTreeNode>, DbError> {
        let reference = |kind, name: &str| DbObjectRef {
            database: keyspace.to_owned(),
            schema: None,
            kind,
            name: name.to_owned(),
            parent_name: None,
        };
        let mut nodes = Vec::new();
        for row in self
            .catalogue(
                "SELECT table_name, comment FROM system_schema.tables WHERE keyspace_name = ?",
                &[keyspace],
                cancel,
            )
            .await?
        {
            nodes.push(node(
                reference(DbObjectKind::Table, &row[0]),
                true,
                Some(row[1].clone()).filter(|c| !c.is_empty()),
            ));
        }
        for row in self
            .catalogue(
                "SELECT view_name, base_table_name FROM system_schema.views WHERE keyspace_name = ?",
                &[keyspace],
                cancel,
            )
            .await?
        {
            nodes.push(node(
                reference(DbObjectKind::MaterializedView, &row[0]),
                true,
                Some(format!("→ {}", row[1])),
            ));
        }
        for row in self
            .catalogue(
                "SELECT type_name FROM system_schema.types WHERE keyspace_name = ?",
                &[keyspace],
                cancel,
            )
            .await?
        {
            nodes.push(node(reference(DbObjectKind::UserType, &row[0]), true, None));
        }
        for (cql, kind) in [
            (
                "SELECT function_name, argument_types, return_type FROM system_schema.functions WHERE keyspace_name = ?",
                DbObjectKind::Function,
            ),
            (
                "SELECT aggregate_name, argument_types, return_type FROM system_schema.aggregates WHERE keyspace_name = ?",
                DbObjectKind::Aggregate,
            ),
        ] {
            // Overloads share one node; the detail lists their signatures.
            let mut signatures: Vec<(String, Vec<String>)> = Vec::new();
            for row in self.catalogue(cql, &[keyspace], cancel).await? {
                let arguments: Vec<String> =
                    serde_json::from_str::<Vec<String>>(&row[1]).unwrap_or_default();
                let signature = format!("({}) → {}", arguments.join(", "), row[2]);
                match signatures.iter_mut().find(|(name, _)| *name == row[0]) {
                    Some((_, list)) => list.push(signature),
                    None => signatures.push((row[0].clone(), vec![signature])),
                }
            }
            for (name, list) in signatures {
                nodes.push(node(reference(kind, &name), false, Some(list.join("; "))));
            }
        }
        Ok(nodes)
    }

    async fn table_children(
        &self,
        parent: &DbObjectRef,
        cancel: &CancellationToken,
    ) -> Result<Vec<DbTreeNode>, DbError> {
        let keyspace = parent.database.as_str();
        let mut nodes: Vec<DbTreeNode> = self
            .columns(keyspace, &parent.name, cancel)
            .await?
            .into_iter()
            .map(|column| {
                let suffix = match column.kind {
                    CqlColumnKind::PartitionKey => " PARTITION KEY",
                    CqlColumnKind::Clustering { descending: true } => " CLUSTERING DESC",
                    CqlColumnKind::Clustering { .. } => " CLUSTERING ASC",
                    CqlColumnKind::Static => " STATIC",
                    CqlColumnKind::Regular => "",
                };
                let key = matches!(
                    column.kind,
                    CqlColumnKind::PartitionKey | CqlColumnKind::Clustering { .. }
                );
                DbTreeNode {
                    reference: DbObjectRef {
                        database: keyspace.to_owned(),
                        schema: None,
                        kind: DbObjectKind::Column,
                        name: column.name,
                        parent_name: Some(parent.name.clone()),
                    },
                    has_children: false,
                    detail: Some(format!("{}{suffix}", column.type_name)),
                    nullable: Some(!key),
                    default_value: None,
                    primary_key: key,
                    foreign_key: None,
                }
            })
            .collect();
        if parent.kind == DbObjectKind::Table {
            for row in self
                .catalogue(
                    "SELECT index_name, kind, options FROM system_schema.indexes \
                     WHERE keyspace_name = ? AND table_name = ?",
                    &[keyspace, &parent.name],
                    cancel,
                )
                .await?
            {
                let target = serde_json::from_str::<serde_json::Value>(&row[2])
                    .ok()
                    .and_then(|options| {
                        options
                            .get("target")
                            .and_then(|t| t.as_str())
                            .map(str::to_owned)
                    })
                    .unwrap_or_default();
                nodes.push(DbTreeNode {
                    reference: DbObjectRef {
                        database: keyspace.to_owned(),
                        schema: None,
                        kind: DbObjectKind::Index,
                        name: row[0].clone(),
                        parent_name: Some(parent.name.clone()),
                    },
                    has_children: false,
                    detail: Some(format!("{} ({target})", row[1])),
                    nullable: None,
                    default_value: None,
                    primary_key: false,
                    foreign_key: None,
                });
            }
        }
        Ok(nodes)
    }

    async fn type_fields(
        &self,
        parent: &DbObjectRef,
        cancel: &CancellationToken,
    ) -> Result<Vec<DbTreeNode>, DbError> {
        let rows = self
            .catalogue(
                "SELECT field_names, field_types FROM system_schema.types \
                 WHERE keyspace_name = ? AND type_name = ?",
                &[&parent.database, &parent.name],
                cancel,
            )
            .await?;
        let Some(row) = rows.first() else {
            return Ok(Vec::new());
        };
        let names: Vec<String> = serde_json::from_str(&row[0]).unwrap_or_default();
        let types: Vec<String> = serde_json::from_str(&row[1]).unwrap_or_default();
        Ok(names
            .into_iter()
            .zip(types)
            .map(|(name, kind)| DbTreeNode {
                reference: DbObjectRef {
                    database: parent.database.clone(),
                    schema: None,
                    kind: DbObjectKind::Column,
                    name,
                    parent_name: Some(parent.name.clone()),
                },
                has_children: false,
                detail: Some(kind),
                nullable: Some(true),
                default_value: None,
                primary_key: false,
                foreign_key: None,
            })
            .collect())
    }

    /// The creation statement: the server's `DESCRIBE` where it has one
    /// (Cassandra 4+, recent ScyllaDB), otherwise built from `system_schema`.
    pub(crate) async fn ddl(
        &self,
        object: &DbObjectRef,
        cancel: &CancellationToken,
    ) -> Result<String, DbError> {
        let keyspace = object.database.as_str();
        let target = match object.kind {
            DbObjectKind::Database => format!("ONLY KEYSPACE {}", quote_name(keyspace)),
            DbObjectKind::Table => format!("TABLE {}", qualified(keyspace, &object.name)),
            DbObjectKind::MaterializedView => {
                format!("MATERIALIZED VIEW {}", qualified(keyspace, &object.name))
            }
            DbObjectKind::UserType => format!("TYPE {}", qualified(keyspace, &object.name)),
            DbObjectKind::Function => format!("FUNCTION {}", qualified(keyspace, &object.name)),
            DbObjectKind::Aggregate => format!("AGGREGATE {}", qualified(keyspace, &object.name)),
            DbObjectKind::Index => format!("INDEX {}", qualified(keyspace, &object.name)),
            _ => return Err(DbError::InvalidRequest),
        };
        if let Ok((names, rows)) = self
            .catalogue_values(&format!("DESCRIBE {target}"), &[], cancel)
            .await
        {
            if let Some(index) = names.iter().position(|name| name == "create_statement") {
                let statements: Vec<String> = rows
                    .iter()
                    .map(|row| text(row.get(index).and_then(Option::as_ref)))
                    .filter(|statement| !statement.trim().is_empty())
                    .collect();
                if !statements.is_empty() {
                    return Ok(statements.join("\n\n"));
                }
            }
        }
        self.built_ddl(object, cancel).await
    }

    pub(crate) async fn built_ddl(
        &self,
        object: &DbObjectRef,
        cancel: &CancellationToken,
    ) -> Result<String, DbError> {
        let keyspace = object.database.as_str();
        let name = object.name.as_str();
        let first = |rows: Vec<Vec<String>>| rows.into_iter().next().ok_or(DbError::QueryFailed);
        match object.kind {
            DbObjectKind::Database => {
                let (_, rows) = self
                    .catalogue_values(
                        "SELECT replication, durable_writes FROM system_schema.keyspaces WHERE keyspace_name = ?",
                        &[keyspace],
                        cancel,
                    )
                    .await?;
                let row = rows.first().ok_or(DbError::QueryFailed)?;
                Ok(format!(
                    "CREATE KEYSPACE {} WITH replication = {} AND durable_writes = {};",
                    quote_name(keyspace),
                    row[0].as_ref().map(option_literal).unwrap_or_default(),
                    row[1]
                        .as_ref()
                        .map(option_literal)
                        .unwrap_or_else(|| "true".into())
                ))
            }
            DbObjectKind::Table => {
                let columns = self.columns(keyspace, name, cancel).await?;
                let (names, rows) = self
                    .catalogue_values(
                        "SELECT * FROM system_schema.tables WHERE keyspace_name = ? AND table_name = ?",
                        &[keyspace, name],
                        cancel,
                    )
                    .await?;
                let row = rows.first().ok_or(DbError::QueryFailed)?;
                let options = table_options(&names, row);
                let mut ddl = create_table_cql(keyspace, name, &columns, &options)?;
                for index in self
                    .catalogue_values(
                        "SELECT index_name, kind, options FROM system_schema.indexes \
                         WHERE keyspace_name = ? AND table_name = ?",
                        &[keyspace, name],
                        cancel,
                    )
                    .await?
                    .1
                {
                    let index_name = text(index[0].as_ref());
                    let options = text(index[2].as_ref());
                    let target = serde_json::from_str::<serde_json::Value>(&options)
                        .ok()
                        .and_then(|o| o.get("target").and_then(|t| t.as_str()).map(str::to_owned))
                        .unwrap_or_default();
                    ddl.push_str(&format!(
                        "\nCREATE INDEX {} ON {} ({target});",
                        quote_name(&index_name),
                        qualified(keyspace, name)
                    ));
                }
                Ok(ddl)
            }
            DbObjectKind::MaterializedView => {
                let row = first(
                    self.catalogue(
                        "SELECT base_table_name, where_clause, include_all_columns FROM system_schema.views \
                         WHERE keyspace_name = ? AND view_name = ?",
                        &[keyspace, name],
                        cancel,
                    )
                    .await?,
                )?;
                let columns = self.columns(keyspace, name, cancel).await?;
                let selected = if row[2] == "true" {
                    "*".to_owned()
                } else {
                    columns
                        .iter()
                        .map(|column| quote_name(&column.name))
                        .collect::<Vec<_>>()
                        .join(", ")
                };
                let partition: Vec<String> = columns
                    .iter()
                    .filter(|c| c.kind == CqlColumnKind::PartitionKey)
                    .map(|c| quote_name(&c.name))
                    .collect();
                let clustering: Vec<String> = columns
                    .iter()
                    .filter(|c| matches!(c.kind, CqlColumnKind::Clustering { .. }))
                    .map(|c| quote_name(&c.name))
                    .collect();
                let mut key = format!("({})", partition.join(", "));
                if !clustering.is_empty() {
                    key.push_str(", ");
                    key.push_str(&clustering.join(", "));
                }
                Ok(format!(
                    "CREATE MATERIALIZED VIEW {} AS\n    SELECT {selected} FROM {}\n    WHERE {}\n    PRIMARY KEY ({key});",
                    qualified(keyspace, name),
                    qualified(keyspace, &row[0]),
                    row[1]
                ))
            }
            DbObjectKind::UserType => {
                let row = first(
                    self.catalogue(
                        "SELECT field_names, field_types FROM system_schema.types \
                         WHERE keyspace_name = ? AND type_name = ?",
                        &[keyspace, name],
                        cancel,
                    )
                    .await?,
                )?;
                let names: Vec<String> = serde_json::from_str(&row[0]).unwrap_or_default();
                let types: Vec<String> = serde_json::from_str(&row[1]).unwrap_or_default();
                Ok(format!(
                    "CREATE TYPE {} (\n{}\n);",
                    qualified(keyspace, name),
                    names
                        .iter()
                        .zip(&types)
                        .map(|(field, kind)| format!("    {} {kind}", quote_name(field)))
                        .collect::<Vec<_>>()
                        .join(",\n")
                ))
            }
            DbObjectKind::Function => {
                let rows = self
                    .catalogue(
                        "SELECT argument_names, argument_types, return_type, language, body, called_on_null_input \
                         FROM system_schema.functions WHERE keyspace_name = ? AND function_name = ?",
                        &[keyspace, name],
                        cancel,
                    )
                    .await?;
                if rows.is_empty() {
                    return Err(DbError::QueryFailed);
                }
                Ok(rows
                    .iter()
                    .map(|row| {
                        let names: Vec<String> = serde_json::from_str(&row[0]).unwrap_or_default();
                        let types: Vec<String> = serde_json::from_str(&row[1]).unwrap_or_default();
                        format!(
                            "CREATE FUNCTION {} ({})\n    {} ON NULL INPUT\n    RETURNS {}\n    LANGUAGE {}\n    AS $${}$$;",
                            qualified(keyspace, name),
                            names
                                .iter()
                                .zip(&types)
                                .map(|(n, t)| format!("{} {t}", quote_name(n)))
                                .collect::<Vec<_>>()
                                .join(", "),
                            if row[5] == "true" { "CALLED" } else { "RETURNS NULL" },
                            row[2],
                            row[3],
                            row[4]
                        )
                    })
                    .collect::<Vec<_>>()
                    .join("\n\n"))
            }
            DbObjectKind::Aggregate => {
                let rows = self
                    .catalogue(
                        "SELECT argument_types, state_func, state_type, final_func, initcond \
                         FROM system_schema.aggregates WHERE keyspace_name = ? AND aggregate_name = ?",
                        &[keyspace, name],
                        cancel,
                    )
                    .await?;
                if rows.is_empty() {
                    return Err(DbError::QueryFailed);
                }
                Ok(rows
                    .iter()
                    .map(|row| {
                        let types: Vec<String> = serde_json::from_str(&row[0]).unwrap_or_default();
                        let mut ddl = format!(
                            "CREATE AGGREGATE {} ({})\n    SFUNC {}\n    STYPE {}",
                            qualified(keyspace, name),
                            types.join(", "),
                            quote_name(&row[1]),
                            row[2]
                        );
                        if !row[3].is_empty() {
                            ddl.push_str(&format!("\n    FINALFUNC {}", quote_name(&row[3])));
                        }
                        if !row[4].is_empty() {
                            ddl.push_str(&format!("\n    INITCOND {}", row[4]));
                        }
                        ddl.push(';');
                        ddl
                    })
                    .collect::<Vec<_>>()
                    .join("\n\n"))
            }
            DbObjectKind::Index => {
                let table = object
                    .parent_name
                    .as_deref()
                    .ok_or(DbError::InvalidRequest)?;
                let row = first(
                    self.catalogue(
                        "SELECT options FROM system_schema.indexes \
                         WHERE keyspace_name = ? AND table_name = ? AND index_name = ?",
                        &[keyspace, table, name],
                        cancel,
                    )
                    .await?,
                )?;
                let target = serde_json::from_str::<serde_json::Value>(&row[0])
                    .ok()
                    .and_then(|o| o.get("target").and_then(|t| t.as_str()).map(str::to_owned))
                    .unwrap_or_default();
                Ok(format!(
                    "CREATE INDEX {} ON {} ({target});",
                    quote_name(name),
                    qualified(keyspace, table)
                ))
            }
            _ => Err(DbError::InvalidRequest),
        }
    }
}

/// The `WITH` options of a table from its `system_schema.tables` row, in a
/// stable order, as CQL literals. Unknown or engine-specific columns are
/// left out.
pub(crate) fn table_options(names: &[String], row: &[Option<CqlValue>]) -> Vec<(String, String)> {
    const OPTIONS: [&str; 12] = [
        "bloom_filter_fp_chance",
        "caching",
        "comment",
        "compaction",
        "compression",
        "crc_check_chance",
        "default_time_to_live",
        "gc_grace_seconds",
        "max_index_interval",
        "memtable_flush_period_in_ms",
        "min_index_interval",
        "speculative_retry",
    ];
    OPTIONS
        .iter()
        .filter_map(|option| {
            let index = names.iter().position(|name| name == option)?;
            let value = row.get(index)?.as_ref()?;
            Some(((*option).to_owned(), option_literal(value)))
        })
        .collect()
}

fn node(reference: DbObjectRef, has_children: bool, detail: Option<String>) -> DbTreeNode {
    DbTreeNode {
        reference,
        has_children,
        detail,
        nullable: None,
        default_value: None,
        primary_key: false,
        foreign_key: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn column(name: &str, kind: CqlColumnKind) -> CqlTableColumn {
        CqlTableColumn {
            name: name.into(),
            type_name: "int".into(),
            kind,
        }
    }

    #[test]
    fn create_table_lists_partition_and_clustering_keys_with_order() {
        let ddl = create_table_cql(
            "shop",
            "Orders",
            &[
                column("customer", CqlColumnKind::PartitionKey),
                column("region", CqlColumnKind::PartitionKey),
                column("placed", CqlColumnKind::Clustering { descending: true }),
                column("id", CqlColumnKind::Clustering { descending: false }),
                column("note", CqlColumnKind::Static),
                column("total", CqlColumnKind::Regular),
            ],
            &[
                ("comment".into(), "'it''s'".into()),
                ("gc_grace_seconds".into(), "864000".into()),
            ],
        )
        .unwrap();
        assert_eq!(
            ddl,
            "CREATE TABLE shop.\"Orders\" (\n    customer int,\n    region int,\n    placed int,\n    id int,\n    note int static,\n    total int,\n    PRIMARY KEY ((customer, region), placed, id)\n) WITH CLUSTERING ORDER BY (placed DESC, id ASC)\n    AND comment = 'it''s'\n    AND gc_grace_seconds = 864000;"
        );
        assert!(
            create_table_cql("shop", "t", &[column("a", CqlColumnKind::Regular)], &[]).is_err()
        );
        let simple =
            create_table_cql("", "t", &[column("a", CqlColumnKind::PartitionKey)], &[]).unwrap();
        assert_eq!(
            simple,
            "CREATE TABLE t (\n    a int,\n    PRIMARY KEY ((a))\n);"
        );
    }

    #[test]
    fn catalogue_columns_sort_in_key_order_then_by_name() {
        let rows = vec![
            vec![
                "z".into(),
                "text".into(),
                "regular".into(),
                "-1".into(),
                "none".into(),
            ],
            vec![
                "ck2".into(),
                "int".into(),
                "clustering".into(),
                "1".into(),
                "desc".into(),
            ],
            vec![
                "pk".into(),
                "uuid".into(),
                "partition_key".into(),
                "0".into(),
                "none".into(),
            ],
            vec![
                "a".into(),
                "text".into(),
                "regular".into(),
                "-1".into(),
                "none".into(),
            ],
            vec![
                "ck1".into(),
                "int".into(),
                "clustering".into(),
                "0".into(),
                "asc".into(),
            ],
            vec![
                "s".into(),
                "int".into(),
                "static".into(),
                "-1".into(),
                "none".into(),
            ],
        ];
        let names: Vec<String> = table_columns(&rows)
            .into_iter()
            .map(|(column, _)| column.name)
            .collect();
        assert_eq!(names, ["pk", "ck1", "ck2", "s", "a", "z"]);
    }

    #[test]
    fn option_literals_and_replication_summaries() {
        let map = CqlValue::Map(vec![(
            CqlValue::Text("class".into()),
            CqlValue::Text("SizeTieredCompactionStrategy".into()),
        )]);
        assert_eq!(
            option_literal(&map),
            "{'class': 'SizeTieredCompactionStrategy'}"
        );
        assert_eq!(option_literal(&CqlValue::Double(0.01)), "0.01");
        assert_eq!(option_literal(&CqlValue::Text("a'b".into())), "'a''b'");
        assert_eq!(
            replication_summary(
                r#"{"class":"org.apache.cassandra.locator.SimpleStrategy","replication_factor":"1"}"#
            ),
            "SimpleStrategy 1"
        );
        assert_eq!(
            replication_summary(r#"{"class":"NetworkTopologyStrategy","dc1":"3"}"#),
            "NetworkTopologyStrategy dc1:3"
        );
        assert_eq!(
            batch_cql(
                &[
                    "INSERT INTO t (a) VALUES (1)".into(),
                    "INSERT INTO t (a) VALUES (2)".into()
                ],
                1
            )
            .len(),
            2
        );
        assert_eq!(
            batch_cql(&["INSERT INTO t (a) VALUES (1)".into()], 10)[0],
            "BEGIN UNLOGGED BATCH\n  INSERT INTO t (a) VALUES (1);\nAPPLY BATCH;"
        );
    }

    #[test]
    fn only_one_node_is_enabled() {
        let filter = OneCoordinator::default();
        let (first, second) = (Uuid::new_v4(), Uuid::new_v4());
        assert!(filter.admit(first));
        assert!(!filter.admit(second));
        assert!(filter.admit(first));
    }
}
