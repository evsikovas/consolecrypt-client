//! MongoDB through the official `mongodb` driver.
//!
//! **Routing.** The driver never dials the network itself. It reaches every
//! server through a loopback SOCKS5 relay ([`MongoRelay`]) with random
//! per-session credentials; the relay connects through the session's
//! [`DbConnector`] (direct or the SSH route) and performs TLS itself with the
//! vault TLS profile, verifying each server under its own host name (the
//! configured host keeps its server-name override). The driver therefore runs
//! without TLS on the loopback leg. Only the configured seed — or, for
//! `srv`, the hosts its SRV record lists — and the replica-set members a seed
//! announces in `hello` before the session opens are reachable. A connection
//! without `replica_set`/`srv` is a direct connection to the one host. SRV and
//! TXT records are resolved with the local resolver, not through the route.
//!
//! **Options.** Built programmatically, never from a URI with the password:
//! SCRAM (negotiated, SHA-256 or SHA-1) or X.509 (the relay presents the vault
//! client certificate), `authSource` (default: the connection database, else
//! `admin`, as in a connection string), `replicaSet`, the configured read
//! preference for reads (`secondaryPreferred` by default) and the primary for
//! writes, connect/server-selection timeouts from the connection limits,
//! no retried reads or writes, and `maxTimeMS` on every command (the smaller of
//! the requested value and `statement_timeout_seconds`).
//!
//! **Results.** Commands come from the guard's parsed form ([`KeyOrder`] restores
//! the written key order and puts the command name first). Cursor commands
//! (`find`, `aggregate`, `listCollections`, `listIndexes`) stream one server
//! batch at a time; the next `getMore` is sent only when the consumer asks for
//! more rows, and stopping or cancelling drops the cursor (`killCursors`).
//! Every document becomes one row: the top-level fields seen in the first
//! non-empty batch (first-seen order, `_id` first, at most 256) as typed cells,
//! then `_doc` — the whole document as relaxed Extended JSON, the source for
//! the document tree. Fields that appear only later are in `_doc` only. Top
//! level cells: ObjectId → hex text, date → RFC 3339 timestamp, int/long → int,
//! double → float, decimal → decimal, UUID binary → text, other binary →
//! base64, objects/arrays/other BSON types → relaxed Extended JSON. `_doc` is
//! marked sensitive when any field name in the first batch looks secret.
//! `count` returns `n`, `distinct` one `value` row per value, other commands
//! (stats, explain, writes, DDL) their reply as one document row; writes report
//! `n` as affected rows and fail after showing the reply if it has write errors.
//!
//! **Cancellation.** Read/write commands carry a random `comment` (unless the
//! user set one); on cancel or timeout the future is dropped and the session's
//! own matching operations are killed best effort (`currentOp` with
//! `$ownOps` + `killOp`) on every known server.
//!
//! **Server read-only.** There is no read-only session in MongoDB.
//! `connectionStatus` with privileges is inspected at connect: when the user is
//! authenticated and every granted action is a read action the session reports
//! [`DbServerReadOnly::RoleVerified`], otherwise `Unverified`.
use crate::{
    parser::KeyOrder,
    streaming::RowConsumer,
    transport::{DbConnector, DbCredentials, DbDestination, MongoRelay, TlsProfile},
    DbCell, DbColumn, DbError, DbObjectKind, DbObjectRef, DbServerReadOnly, DbTreeNode, DbTreePage,
    DEFAULT_TREE_PAGE,
};
use base64::{engine::general_purpose::STANDARD, Engine};
use bson::{doc, spec::BinarySubtype, Bson, Document, RawBsonRef};
use cc_models::database::{
    DbAuthMechanism, DbConnection, DbOptions, DbTlsMode, MongoReadPreference,
};
use futures::{future::BoxFuture, StreamExt, TryStreamExt};
use mongodb::{
    options::{
        AuthMechanism, ClientOptions, Credential, ReadPreference, SelectionCriteria, ServerAddress,
        ServerMonitoringMode, Socks5Proxy,
    },
    Client, ServerInfo,
};
use secrecy::ExposeSecret;
use serde_json::{json, Value};
use std::{
    collections::{BTreeMap, HashMap, HashSet},
    fmt,
    future::{Future, IntoFuture},
    sync::Arc,
    time::{Duration, Instant},
};
use tokio_util::sync::CancellationToken;

/// Command names, as in the guard's allow-list.
const OPERATIONS: [&str; 19] = [
    "find",
    "aggregate",
    "count",
    "distinct",
    "listCollections",
    "listIndexes",
    "collStats",
    "dbStats",
    "explain",
    "insert",
    "update",
    "delete",
    "findAndModify",
    "drop",
    "dropDatabase",
    "create",
    "createIndexes",
    "dropIndexes",
    "renameCollection",
];
/// Commands that may run long enough to be worth a `killOp`.
const TAGGED: [&str; 8] = [
    "find",
    "aggregate",
    "count",
    "distinct",
    "insert",
    "update",
    "delete",
    "findAndModify",
];
/// Extended JSON wrappers that become one typed BSON value.
const EXTENDED_JSON: [&str; 16] = [
    "$oid",
    "$symbol",
    "$regularExpression",
    "$numberInt",
    "$numberLong",
    "$numberDouble",
    "$numberDecimal",
    "$binary",
    "$uuid",
    "$code",
    "$timestamp",
    "$date",
    "$minKey",
    "$maxKey",
    "$dbPointer",
    "$undefined",
];
/// Actions of the built-in `read`/`readAnyDatabase` roles.
const READ_ACTIONS: [&str; 11] = [
    "changeStream",
    "collStats",
    "dbHash",
    "dbStats",
    "find",
    "killCursors",
    "listCollections",
    "listIndexes",
    "listSearchIndexes",
    "planCacheRead",
    "listDatabases",
];
pub(crate) const DOCUMENT_COLUMN: &str = "_doc";
const MAX_COLUMNS: usize = 256;
const MAX_LISTING: usize = 10_000;
const MAX_SAMPLE: u32 = 1000;
const MAX_SCHEMA_PATHS: usize = 2000;
const MAX_DEPTH: usize = 16;
const GET_MORE_BATCH: u32 = 1000;
const TREE_TIMEOUT: Duration = Duration::from_secs(30);
static LEAF: KeyOrder = KeyOrder::Leaf;

pub(crate) struct Mongo {
    client: Client,
    database: String,
    read: SelectionCriteria,
    servers: Vec<ServerAddress>,
    statement_timeout: Duration,
    _relay: MongoRelay,
    pub version: String,
    pub server_read_only: DbServerReadOnly,
}
impl fmt::Debug for Mongo {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Mongo([SCOPED])")
    }
}

impl Mongo {
    pub async fn connect(
        connection: &DbConnection,
        credentials: &DbCredentials,
        connector: Arc<dyn DbConnector>,
        scope: CancellationToken,
    ) -> Result<Self, DbError> {
        let DbOptions::Mongodb {
            auth_source,
            replica_set,
            srv,
            read_preference,
        } = &connection.options
        else {
            return Err(DbError::InvalidRequest);
        };
        let tls = TlsProfile::new(connection, credentials)?;
        let wait = Duration::from_secs(u64::from(
            connection.limits.connect_timeout_seconds.clamp(1, 120),
        ));
        let resolved = if *srv {
            resolve_srv(&connection.host, wait).await?
        } else {
            Seeds {
                hosts: vec![DbDestination::new(&connection.host, connection.port)?],
                replica_set: None,
                auth_source: None,
                load_balanced: false,
            }
        };
        let replica_set = replica_set.clone().or(resolved.replica_set);
        let seeds = resolved.hosts;
        let first = seeds.first().ok_or(DbError::ConnectionFailed)?;
        if tls.mode != DbTlsMode::Disable {
            // Classify certificate failures before the driver turns them into
            // a generic server-selection timeout.
            preflight(&connector, first, &tls, wait, &scope).await?;
        }
        let reachable = if replica_set.is_some() {
            members(&connector, &seeds, &tls, wait, &scope).await?
        } else {
            seeds.clone()
        };
        let relay = MongoRelay::new(
            connector,
            destinations(&reachable),
            tls.clone(),
            scope.clone(),
        )
        .await?;
        let mut options = base_options(&relay, seeds.iter().map(address).collect(), wait);
        let discover = replica_set.is_some() || seeds.len() > 1 || resolved.load_balanced;
        options.direct_connection = Some(!discover);
        if resolved.load_balanced {
            options.direct_connection = None;
            options.load_balanced = Some(true);
        }
        if discover {
            options.repl_set_name = replica_set;
        }
        let read = selection(*read_preference);
        options.selection_criteria = Some(read.clone());
        options.credential = credential(
            connection,
            credentials,
            auth_source.clone().or(resolved.auth_source),
        );
        let client = Client::with_options(options).map_err(classify)?;
        let admin = client.database("admin");
        let info = cancellable(
            &scope,
            wait + Duration::from_secs(2),
            admin
                .run_command(doc! {"buildInfo": 1})
                .selection_criteria(read.clone())
                .into_future(),
        )
        .await?;
        let version = info
            .get_str("version")
            .map_err(|_| DbError::QueryFailed)?
            .to_owned();
        let status = cancellable(
            &scope,
            wait + Duration::from_secs(2),
            admin
                .run_command(doc! {"connectionStatus": 1, "showPrivileges": true})
                .selection_criteria(read.clone())
                .into_future(),
        )
        .await;
        let server_read_only = match status {
            Ok(status) if roles_read_only(&status) => DbServerReadOnly::RoleVerified,
            _ => DbServerReadOnly::Unverified,
        };
        let mut servers: Vec<ServerAddress> = Vec::new();
        for destination in &reachable {
            let candidate = address(destination);
            if !servers.iter().any(|known| same_address(known, &candidate)) {
                servers.push(candidate);
            }
        }
        Ok(Self {
            client,
            database: if connection.database.is_empty() {
                "test".into()
            } else {
                connection.database.clone()
            },
            read,
            servers,
            statement_timeout: Duration::from_secs(u64::from(
                connection.limits.statement_timeout_seconds.clamp(1, 3600),
            )),
            _relay: relay,
            version,
            server_read_only,
        })
    }

    /// Client-side bound of one server round trip; the server bound is `maxTimeMS`.
    fn round_trip(&self) -> Duration {
        self.statement_timeout + Duration::from_secs(5)
    }

    fn time_limit_ms(&self) -> i64 {
        i64::try_from(self.statement_timeout.as_millis()).unwrap_or(i64::MAX)
    }

    /// Runs one guarded command (see the module documentation for the result
    /// shape). Reads use the configured read preference, writes the primary.
    pub fn command<'a>(
        &'a mut self,
        command: &'a Value,
        order: &'a KeyOrder,
        read_only: bool,
        sink: &'a mut (dyn RowConsumer + Send),
        cancel: CancellationToken,
    ) -> BoxFuture<'a, Result<(), DbError>> {
        Box::pin(async move {
            let started = Instant::now();
            let mut document = command_document(command, order)?;
            let operation = operation(&document)?;
            limit_time(&mut document, self.time_limit_ms());
            let tag = (TAGGED.contains(&operation) && !document.contains_key("comment"))
                .then(|| format!("consolecrypt-{}", uuid::Uuid::new_v4().simple()));
            if let Some(tag) = &tag {
                document.insert("comment", tag.as_str());
            }
            let selection = if read_only {
                self.read.clone()
            } else {
                SelectionCriteria::ReadPreference(ReadPreference::Primary)
            };
            let work = self.execute(operation, document, selection, sink);
            let result = tokio::select! {
                biased;
                _ = cancel.cancelled() => Err(DbError::Cancelled),
                result = work => result,
            };
            match result {
                Ok(affected) => {
                    sink.statement_done(
                        started.elapsed().as_millis().min(u128::from(u64::MAX)) as u64,
                        affected,
                    )
                    .await
                }
                Err(error) => {
                    if matches!(error, DbError::Cancelled | DbError::Timeout) {
                        if let Some(tag) = &tag {
                            self.kill(tag).await;
                        }
                    }
                    Err(error)
                }
            }
        })
    }

    fn execute<'a>(
        &'a self,
        operation: &'static str,
        document: Document,
        selection: SelectionCriteria,
        sink: &'a mut (dyn RowConsumer + Send),
    ) -> BoxFuture<'a, Result<Option<u64>, DbError>> {
        Box::pin(async move {
            let database = self.client.database(&self.database);
            let limit = self.round_trip();
            match operation {
                "find" | "aggregate" | "listCollections" | "listIndexes" => {
                    let cursor = bounded(
                        limit,
                        database
                            .run_cursor_command(document)
                            .selection_criteria(selection)
                            .batch_size(GET_MORE_BATCH)
                            .batch(),
                    )
                    .await?;
                    stream(cursor, sink, limit).await?;
                    Ok(None)
                }
                _ => {
                    let reply = bounded(
                        limit,
                        database
                            .run_command(document)
                            .selection_criteria(selection)
                            .into_future(),
                    )
                    .await?;
                    let failed = reply
                        .get_array("writeErrors")
                        .is_ok_and(|errors| !errors.is_empty())
                        || reply.contains_key("writeConcernError");
                    let affected = affected(operation, &reply);
                    match operation {
                        "count" => {
                            let mut count = Document::new();
                            count.insert("n", reply.get("n").cloned().unwrap_or(Bson::Null));
                            emit(sink, vec![count], false).await?;
                        }
                        "distinct" => {
                            let values = reply
                                .get_array("values")
                                .map_err(|_| DbError::QueryFailed)?;
                            let rows = values
                                .iter()
                                .map(|value| doc! {"value": value.clone()})
                                .collect();
                            emit(sink, rows, false).await?;
                        }
                        _ => emit(sink, vec![strip_reply(reply)], true).await?,
                    }
                    if failed {
                        return Err(DbError::QueryFailed);
                    }
                    Ok(affected)
                }
            }
        })
    }

    /// Best effort: kill this session's operations tagged with `tag` on every
    /// known server. Dropping the driver future alone leaves the server working.
    fn kill<'a>(&'a self, tag: &'a str) -> BoxFuture<'a, ()> {
        Box::pin(async move {
            let admin = self.client.database("admin");
            let filter = doc! {
                "currentOp": 1,
                "$ownOps": true,
                "$or": [
                    {"command.comment": tag},
                    {"cursor.originatingCommand.comment": tag},
                ],
            };
            let attempts = self.servers.iter().cloned().map(|target| {
                let admin = admin.clone();
                let filter = filter.clone();
                async move {
                    let criteria =
                        SelectionCriteria::Predicate(Arc::new(move |server: &ServerInfo| {
                            same_address(server.address(), &target)
                        }));
                    let Ok(reply) = admin
                        .run_command(filter)
                        .selection_criteria(criteria.clone())
                        .await
                    else {
                        return;
                    };
                    let Ok(operations) = reply.get_array("inprog") else {
                        return;
                    };
                    for operation in operations {
                        let Some(opid) = operation.as_document().and_then(|op| op.get("opid"))
                        else {
                            continue;
                        };
                        let _ = admin
                            .run_command(doc! {"killOp": 1, "op": opid.clone()})
                            .selection_criteria(criteria.clone())
                            .await;
                    }
                }
            });
            let _ =
                tokio::time::timeout(Duration::from_secs(3), futures::future::join_all(attempts))
                    .await;
        })
    }

    /// Databases → collections and views → indexes. Listings are sorted by
    /// name and paged by [`DEFAULT_TREE_PAGE`]; `cursor` is the last name shown.
    pub fn browse<'a>(
        &'a mut self,
        parent: Option<&'a DbObjectRef>,
        cursor: Option<&'a str>,
    ) -> BoxFuture<'a, Result<DbTreePage, DbError>> {
        Box::pin(async move {
            let nodes = match parent.map(|parent| (parent.kind, parent)) {
                None => self.databases().await?,
                Some((DbObjectKind::Database, parent)) => self.collections(&parent.name).await?,
                Some((DbObjectKind::Collection, parent)) => self
                    .indexes(&parent.database, &parent.name)
                    .await?
                    .into_iter()
                    .map(|index| index_node(&parent.database, &parent.name, &index))
                    .collect(),
                Some(_) => Vec::new(),
            };
            Ok(page(nodes, cursor))
        })
    }

    fn databases(&self) -> BoxFuture<'_, Result<Vec<DbTreeNode>, DbError>> {
        Box::pin(async move {
            let reply = bounded(
                TREE_TIMEOUT,
                self.client
                    .database("admin")
                    .run_command(self.internal(doc! {
                        "listDatabases": 1, "nameOnly": true, "authorizedDatabases": true,
                    }))
                    .selection_criteria(self.read.clone())
                    .into_future(),
            )
            .await;
            let mut names: Vec<String> = match reply {
                Ok(reply) => reply
                    .get_array("databases")
                    .map_err(|_| DbError::QueryFailed)?
                    .iter()
                    .filter_map(|entry| entry.as_document()?.get_str("name").ok())
                    .map(str::to_owned)
                    .collect(),
                // Without listDatabases the configured database is still usable.
                Err(DbError::PolicyDenied) => Vec::new(),
                Err(error) => return Err(error),
            };
            if !names.contains(&self.database) {
                names.push(self.database.clone());
            }
            Ok(names
                .into_iter()
                .map(|name| {
                    node(
                        object(&name, DbObjectKind::Database, &name, None),
                        true,
                        None,
                    )
                })
                .collect())
        })
    }

    fn collections<'a>(
        &'a self,
        database: &'a str,
    ) -> BoxFuture<'a, Result<Vec<DbTreeNode>, DbError>> {
        Box::pin(async move {
            let entries = match self
                .list(database, doc! {"listCollections": 1})
                .await
            {
                Err(DbError::PolicyDenied) => {
                    self.list(
                        database,
                        doc! {"listCollections": 1, "nameOnly": true, "authorizedCollections": true},
                    )
                    .await?
                }
                other => other?,
            };
            Ok(entries
                .iter()
                .filter_map(|entry| {
                    let name = entry.get_str("name").ok()?;
                    let kind = entry.get_str("type").unwrap_or("collection");
                    let options = entry.get_document("options").ok();
                    let (object_kind, detail) = match kind {
                        "view" => (
                            DbObjectKind::View,
                            Some(match options.and_then(|o| o.get_str("viewOn").ok()) {
                                Some(source) => format!("view on {source}"),
                                None => "view".into(),
                            }),
                        ),
                        "timeseries" => (DbObjectKind::Collection, Some("timeseries".into())),
                        _ if options.is_some_and(|o| o.get_bool("capped").unwrap_or(false)) => {
                            (DbObjectKind::Collection, Some("capped".into()))
                        }
                        _ => (DbObjectKind::Collection, None),
                    };
                    Some(node(
                        object(database, object_kind, name, None),
                        object_kind == DbObjectKind::Collection,
                        detail,
                    ))
                })
                .collect())
        })
    }

    fn indexes<'a>(
        &'a self,
        database: &'a str,
        collection: &'a str,
    ) -> BoxFuture<'a, Result<Vec<Document>, DbError>> {
        Box::pin(async move {
            match self.list(database, doc! {"listIndexes": collection}).await {
                // A view or a missing collection has no indexes.
                Err(DbError::QueryFailed) => Ok(Vec::new()),
                other => other,
            }
        })
    }

    /// Runs a small cursor command and collects its documents (bounded).
    fn list<'a>(
        &'a self,
        database: &'a str,
        command: Document,
    ) -> BoxFuture<'a, Result<Vec<Document>, DbError>> {
        Box::pin(async move {
            let mut cursor = bounded(
                TREE_TIMEOUT,
                self.client
                    .database(database)
                    .run_cursor_command(self.internal(command))
                    .selection_criteria(self.read.clone())
                    .into_future(),
            )
            .await?;
            let mut documents = Vec::new();
            while let Some(document) = tokio::time::timeout(TREE_TIMEOUT, cursor.try_next())
                .await
                .map_err(|_| DbError::Timeout)?
                .map_err(classify)?
            {
                if documents.len() >= MAX_LISTING {
                    break;
                }
                documents.push(document);
            }
            Ok(documents)
        })
    }

    fn internal(&self, mut command: Document) -> Document {
        limit_time(&mut command, self.time_limit_ms());
        command
    }

    /// Sampled schema of a collection or view: `$sample` of up to 1000
    /// documents, then every field path with the BSON types seen and the
    /// share of sampled documents that contain it (see [`SchemaSampler`]).
    pub fn inspect(
        &mut self,
        object: &DbObjectRef,
        sample: u32,
    ) -> BoxFuture<'_, Result<Value, DbError>> {
        let object = object.clone();
        Box::pin(async move {
            if !matches!(object.kind, DbObjectKind::Collection | DbObjectKind::View) {
                return Err(DbError::InvalidRequest);
            }
            let size = sample.clamp(1, MAX_SAMPLE);
            let command = self.internal(doc! {
                "aggregate": object.name.as_str(),
                "pipeline": [{"$sample": {"size": i64::from(size)}}],
                "cursor": {},
            });
            let limit = self.round_trip();
            let mut cursor = bounded(
                limit,
                self.client
                    .database(&object.database)
                    .run_cursor_command(command)
                    .selection_criteria(self.read.clone())
                    .batch_size(GET_MORE_BATCH)
                    .into_future(),
            )
            .await?;
            let mut sampler = SchemaSampler::default();
            while let Some(document) = tokio::time::timeout(limit, cursor.try_next())
                .await
                .map_err(|_| DbError::Timeout)?
                .map_err(classify)?
            {
                sampler.add(&document);
            }
            Ok(sampler.report(&object.database, &object.name))
        })
    }

    /// The MongoDB "DDL": collection/view options and indexes as pretty
    /// relaxed Extended JSON. A database lists its collections; an index its
    /// specification.
    pub fn describe(&mut self, object: &DbObjectRef) -> BoxFuture<'_, Result<String, DbError>> {
        let object = object.clone();
        Box::pin(async move {
            let value = match object.kind {
                DbObjectKind::Database => {
                    let mut names: Vec<String> = self
                        .list(&object.name, doc! {"listCollections": 1, "nameOnly": true})
                        .await?
                        .iter()
                        .filter_map(|entry| entry.get_str("name").ok().map(str::to_owned))
                        .collect();
                    names.sort();
                    Bson::Document(doc! {"database": object.name.as_str(), "collections": names})
                }
                DbObjectKind::Collection | DbObjectKind::View => {
                    let entry = self
                        .list(
                            &object.database,
                            doc! {"listCollections": 1, "filter": {"name": object.name.as_str()}},
                        )
                        .await?
                        .into_iter()
                        .next()
                        .ok_or(DbError::InvalidRequest)?;
                    let mut description = Document::new();
                    description.insert("database", object.database.as_str());
                    description.insert("name", object.name.as_str());
                    for key in ["type", "options", "info"] {
                        if let Some(value) = entry.get(key) {
                            description.insert(key, value.clone());
                        }
                    }
                    if entry.get_str("type").unwrap_or("collection") != "view" {
                        let indexes = self.indexes(&object.database, &object.name).await?;
                        description.insert(
                            "indexes",
                            indexes.into_iter().map(Bson::Document).collect::<Vec<_>>(),
                        );
                    }
                    Bson::Document(description)
                }
                DbObjectKind::Index => {
                    let collection = object
                        .parent_name
                        .as_deref()
                        .ok_or(DbError::InvalidRequest)?;
                    let index = self
                        .indexes(&object.database, collection)
                        .await?
                        .into_iter()
                        .find(|index| index.get_str("name").ok() == Some(object.name.as_str()))
                        .ok_or(DbError::InvalidRequest)?;
                    Bson::Document(index)
                }
                _ => return Err(DbError::InvalidRequest),
            };
            Ok(extended_json(&value, true))
        })
    }
}

struct Seeds {
    hosts: Vec<DbDestination>,
    replica_set: Option<String>,
    auth_source: Option<String>,
    load_balanced: bool,
}

/// SRV seed list and TXT options through the driver's resolver. The URI
/// holds only the validated host name.
async fn resolve_srv(host: &str, wait: Duration) -> Result<Seeds, DbError> {
    DbDestination::new(host, 27017)?;
    let parsed = tokio::time::timeout(
        wait,
        ClientOptions::parse(format!("mongodb+srv://{host}/")).into_future(),
    )
    .await
    .map_err(|_| DbError::Timeout)?
    .map_err(|_| DbError::ConnectionFailed)?;
    let mut hosts = Vec::new();
    for address in &parsed.hosts {
        if let ServerAddress::Tcp { host, port } = address {
            hosts.push(DbDestination::new(host.clone(), port.unwrap_or(27017))?);
        }
    }
    if hosts.is_empty() || hosts.len() > 32 {
        return Err(DbError::ConnectionFailed);
    }
    Ok(Seeds {
        hosts,
        replica_set: parsed.repl_set_name.clone(),
        auth_source: parsed
            .credential
            .as_ref()
            .and_then(|credential| credential.source.clone()),
        load_balanced: parsed.load_balanced.unwrap_or(false),
    })
}

fn preflight<'a>(
    connector: &'a Arc<dyn DbConnector>,
    destination: &'a DbDestination,
    tls: &'a TlsProfile,
    wait: Duration,
    scope: &'a CancellationToken,
) -> BoxFuture<'a, Result<(), DbError>> {
    Box::pin(async move {
        let stream =
            tokio::time::timeout(wait, connector.connect(destination, scope.child_token()))
                .await
                .map_err(|_| DbError::Timeout)??;
        tokio::time::timeout(wait, tls.connect(stream, Some(&destination.host)))
            .await
            .map_err(|_| DbError::Timeout)?
            .map(drop)
    })
}

/// Replica-set members announced by the first answering seed (`hello` needs
/// no authentication). The relay for the session admits seeds and members.
fn members<'a>(
    connector: &'a Arc<dyn DbConnector>,
    seeds: &'a [DbDestination],
    tls: &'a TlsProfile,
    wait: Duration,
    scope: &'a CancellationToken,
) -> BoxFuture<'a, Result<Vec<DbDestination>, DbError>> {
    Box::pin(async move {
        let relay = MongoRelay::new(
            connector.clone(),
            destinations(seeds),
            tls.clone(),
            scope.child_token(),
        )
        .await?;
        for seed in seeds {
            let mut options = base_options(&relay, vec![address(seed)], wait);
            options.direct_connection = Some(true);
            let client = Client::with_options(options).map_err(classify)?;
            let hello = cancellable(
                scope,
                wait + Duration::from_secs(1),
                client
                    .database("admin")
                    .run_command(doc! {"hello": 1})
                    .selection_criteria(SelectionCriteria::ReadPreference(
                        ReadPreference::PrimaryPreferred { options: None },
                    ))
                    .into_future(),
            )
            .await;
            match hello {
                Ok(reply) => {
                    let mut found = seeds.to_vec();
                    for key in ["hosts", "passives", "arbiters"] {
                        for entry in reply.get_array(key).map(Vec::as_slice).unwrap_or(&[]) {
                            let Some(Ok(ServerAddress::Tcp { host, port })) =
                                entry.as_str().map(ServerAddress::parse)
                            else {
                                continue;
                            };
                            if let Ok(member) = DbDestination::new(host, port.unwrap_or(27017)) {
                                if !found.contains(&member) && found.len() < 32 {
                                    found.push(member);
                                }
                            }
                        }
                    }
                    return Ok(found);
                }
                Err(DbError::Cancelled) => return Err(DbError::Cancelled),
                Err(_) => continue,
            }
        }
        Err(DbError::ConnectionFailed)
    })
}

/// Allowed relay destinations; the driver lower-cases discovered host names.
fn destinations(hosts: &[DbDestination]) -> HashSet<DbDestination> {
    let mut set = HashSet::new();
    for host in hosts {
        set.insert(host.clone());
        if let Ok(lower) = DbDestination::new(host.host.to_ascii_lowercase(), host.port) {
            set.insert(lower);
        }
    }
    set
}

fn address(destination: &DbDestination) -> ServerAddress {
    ServerAddress::Tcp {
        host: destination.host.clone(),
        port: Some(destination.port),
    }
}

fn same_address(left: &ServerAddress, right: &ServerAddress) -> bool {
    match (left, right) {
        (
            ServerAddress::Tcp { host, port },
            ServerAddress::Tcp {
                host: other,
                port: other_port,
            },
        ) => {
            host.eq_ignore_ascii_case(other) && port.unwrap_or(27017) == other_port.unwrap_or(27017)
        }
        _ => false,
    }
}

fn base_options(relay: &MongoRelay, hosts: Vec<ServerAddress>, wait: Duration) -> ClientOptions {
    let mut options = ClientOptions::default();
    options.hosts = hosts;
    options.app_name = Some("ConsoleCrypt".into());
    options.connect_timeout = Some(wait);
    options.server_selection_timeout = Some(wait);
    // One monitor connection per server keeps the relay below its stream cap.
    options.server_monitoring_mode = Some(ServerMonitoringMode::Poll);
    options.max_pool_size = Some(3);
    options.min_pool_size = Some(0);
    // An approved write runs once; nothing is replayed after a network error.
    options.retry_reads = Some(false);
    options.retry_writes = Some(false);
    // TLS is performed by the relay with the vault profile.
    options.tls = None;
    options.socks5_proxy = Some(
        Socks5Proxy::builder()
            .host(relay.relay.address.ip().to_string())
            .port(relay.relay.address.port())
            .authentication((
                relay.authentication.username.expose_secret().to_owned(),
                relay.authentication.password.expose_secret().to_owned(),
            ))
            .build(),
    );
    options
}

fn credential(
    connection: &DbConnection,
    credentials: &DbCredentials,
    auth_source: Option<String>,
) -> Option<Credential> {
    let username = (!connection.username.is_empty()).then(|| connection.username.clone());
    let password = credentials
        .password
        .as_ref()
        .map(|password| password.expose_secret().to_owned());
    match connection.auth.mechanism {
        DbAuthMechanism::MongoX509 => Some(
            Credential::builder()
                .username(username)
                .source("$external".to_owned())
                .mechanism(AuthMechanism::MongoDbX509)
                .build(),
        ),
        mechanism if username.is_some() || password.is_some() => {
            let source = auth_source.unwrap_or_else(|| {
                if connection.database.is_empty() {
                    "admin".into()
                } else {
                    connection.database.clone()
                }
            });
            Some(
                Credential::builder()
                    .username(username)
                    .password(password)
                    .source(source)
                    .mechanism(match mechanism {
                        DbAuthMechanism::ScramSha256 => Some(AuthMechanism::ScramSha256),
                        DbAuthMechanism::ScramSha1 => Some(AuthMechanism::ScramSha1),
                        _ => None,
                    })
                    .build(),
            )
        }
        _ => None,
    }
}

fn selection(preference: MongoReadPreference) -> SelectionCriteria {
    SelectionCriteria::ReadPreference(match preference {
        MongoReadPreference::Primary => ReadPreference::Primary,
        MongoReadPreference::PrimaryPreferred => ReadPreference::PrimaryPreferred { options: None },
        MongoReadPreference::Secondary => ReadPreference::Secondary { options: None },
        MongoReadPreference::SecondaryPreferred => {
            ReadPreference::SecondaryPreferred { options: None }
        }
        MongoReadPreference::Nearest => ReadPreference::Nearest { options: None },
    })
}

/// True only for an authenticated user whose every privilege is a read action.
fn roles_read_only(status: &Document) -> bool {
    let Ok(info) = status.get_document("authInfo") else {
        return false;
    };
    let authenticated = info
        .get_array("authenticatedUsers")
        .is_ok_and(|users| !users.is_empty());
    let Ok(privileges) = info.get_array("authenticatedUserPrivileges") else {
        return false;
    };
    authenticated
        && !privileges.is_empty()
        && privileges.iter().all(|privilege| {
            privilege
                .as_document()
                .and_then(|privilege| privilege.get_array("actions").ok())
                .is_some_and(|actions| {
                    !actions.is_empty()
                        && actions.iter().all(|action| {
                            action
                                .as_str()
                                .is_some_and(|action| READ_ACTIONS.contains(&action))
                        })
                })
        })
}

async fn cancellable<T>(
    scope: &CancellationToken,
    limit: Duration,
    future: impl Future<Output = mongodb::error::Result<T>>,
) -> Result<T, DbError> {
    tokio::select! {
        biased;
        _ = scope.cancelled() => Err(DbError::Cancelled),
        result = bounded(limit, future) => result,
    }
}

async fn bounded<T>(
    limit: Duration,
    future: impl Future<Output = mongodb::error::Result<T>>,
) -> Result<T, DbError> {
    tokio::time::timeout(limit, future)
        .await
        .map_err(|_| DbError::Timeout)?
        .map_err(classify)
}

/// Driver errors carry server messages and addresses; only the class leaves.
fn classify(error: mongodb::error::Error) -> DbError {
    use mongodb::error::ErrorKind;
    match error.kind.as_ref() {
        ErrorKind::Authentication { .. } => DbError::AuthenticationFailed,
        ErrorKind::Command(command) => match command.code {
            18 => DbError::AuthenticationFailed,
            13 => DbError::PolicyDenied,
            50 | 262 => DbError::Timeout,
            11600..=11602 => DbError::Cancelled,
            _ => DbError::QueryFailed,
        },
        ErrorKind::ServerSelection { .. }
        | ErrorKind::Io(_)
        | ErrorKind::ConnectionPoolCleared { .. }
        | ErrorKind::DnsResolve { .. }
        | ErrorKind::ProxyConnect { .. } => DbError::ConnectionFailed,
        ErrorKind::InvalidTlsConfig { .. } => DbError::TlsFailed,
        ErrorKind::InvalidArgument { .. } => DbError::InvalidRequest,
        _ => DbError::QueryFailed,
    }
}

fn command_document(command: &Value, order: &KeyOrder) -> Result<Document, DbError> {
    let Bson::Document(document) = to_bson(command, order, 0)? else {
        return Err(DbError::InvalidRequest);
    };
    let mut document = command_first(document)?;
    if let Some(Bson::Document(inner)) = document.get("explain") {
        let inner = command_first(inner.clone())?;
        document.insert("explain", inner);
    }
    Ok(document)
}

fn operation(document: &Document) -> Result<&'static str, DbError> {
    let name = document.keys().next().ok_or(DbError::InvalidRequest)?;
    OPERATIONS
        .iter()
        .find(|operation| **operation == name.as_str())
        .copied()
        .ok_or(DbError::InvalidRequest)
}

/// The server reads the first key as the command name.
fn command_first(document: Document) -> Result<Document, DbError> {
    let name = OPERATIONS
        .iter()
        .find(|name| document.contains_key(**name))
        .ok_or(DbError::InvalidRequest)?;
    let mut ordered = Document::new();
    ordered.insert(*name, document.get(*name).cloned().unwrap_or(Bson::Null));
    for (key, value) in document {
        if key != *name {
            ordered.insert(key, value);
        }
    }
    Ok(ordered)
}

/// JSON → BSON in written key order; Extended JSON wrappers become typed values.
fn to_bson(value: &Value, order: &KeyOrder, depth: usize) -> Result<Bson, DbError> {
    if depth > 128 {
        return Err(DbError::ResultLimit);
    }
    match value {
        Value::Object(map) => {
            if map.keys().any(|key| EXTENDED_JSON.contains(&key.as_str())) {
                return Bson::try_from(value.clone()).map_err(|_| DbError::InvalidRequest);
            }
            let children: HashMap<&str, &KeyOrder> = match order {
                KeyOrder::Object(entries) => entries
                    .iter()
                    .map(|(key, order)| (key.as_str(), order))
                    .collect(),
                _ => HashMap::new(),
            };
            let mut document = Document::new();
            for key in order.keys().filter(|key| map.contains_key(*key)) {
                document.insert(key, to_bson(&map[key], children[key], depth + 1)?);
            }
            for (key, value) in map {
                if !children.contains_key(key.as_str()) {
                    document.insert(key.as_str(), to_bson(value, &LEAF, depth + 1)?);
                }
            }
            Ok(Bson::Document(document))
        }
        Value::Array(items) => Ok(Bson::Array(
            items
                .iter()
                .enumerate()
                .map(|(index, item)| {
                    to_bson(item, order.element(index).unwrap_or(&LEAF), depth + 1)
                })
                .collect::<Result<_, _>>()?,
        )),
        scalar => Bson::try_from(scalar.clone()).map_err(|_| DbError::InvalidRequest),
    }
}

/// `maxTimeMS` = min(requested, statement timeout), always present.
fn limit_time(document: &mut Document, limit_ms: i64) {
    let requested = match document.get("maxTimeMS") {
        Some(Bson::Int32(value)) => Some(i64::from(*value)),
        Some(Bson::Int64(value)) => Some(*value),
        Some(Bson::Double(value)) if value.is_finite() => Some(*value as i64),
        _ => None,
    }
    .filter(|value| *value > 0);
    let limit = limit_ms.max(1);
    document.insert(
        "maxTimeMS",
        Bson::Int64(requested.map_or(limit, |value| value.min(limit))),
    );
}

fn affected(operation: &str, reply: &Document) -> Option<u64> {
    let number = |document: &Document, key: &str| match document.get(key) {
        Some(Bson::Int32(value)) => u64::try_from(*value).ok(),
        Some(Bson::Int64(value)) => u64::try_from(*value).ok(),
        Some(Bson::Double(value)) if *value >= 0.0 => Some(*value as u64),
        _ => None,
    };
    match operation {
        "insert" | "update" | "delete" => number(reply, "n"),
        "findAndModify" => reply
            .get_document("lastErrorObject")
            .ok()
            .and_then(|last| number(last, "n")),
        _ => None,
    }
}

/// Drops wire bookkeeping from a command reply.
fn strip_reply(reply: Document) -> Document {
    reply
        .into_iter()
        .filter(|(key, _)| {
            !key.starts_with('$')
                && !matches!(
                    key.as_str(),
                    "ok" | "operationTime" | "opTime" | "electionId"
                )
        })
        .collect()
}

async fn emit(
    sink: &mut (dyn RowConsumer + Send),
    documents: Vec<Document>,
    document_column: bool,
) -> Result<(), DbError> {
    let layout = Layout::new(&documents, document_column);
    sink.start(layout.columns(), None).await?;
    for document in documents {
        if !sink.row(layout.row(document)?).await? {
            break;
        }
    }
    Ok(())
}

/// Streams a cursor one server batch at a time; the first non-empty batch
/// fixes the columns. Returning early drops the cursor (`killCursors`).
async fn stream(
    mut cursor: mongodb::raw_batch_cursor::RawBatchCursor,
    sink: &mut (dyn RowConsumer + Send),
    limit: Duration,
) -> Result<(), DbError> {
    let mut batch = Vec::new();
    let mut exhausted = false;
    while batch.is_empty() {
        match next_batch(&mut cursor, limit).await? {
            Some(documents) => batch = documents,
            None => {
                exhausted = true;
                break;
            }
        }
    }
    let layout = Layout::new(&batch, true);
    sink.start(layout.columns(), None).await?;
    loop {
        for document in batch {
            if !sink.row(layout.row(document)?).await? {
                return Ok(());
            }
        }
        if exhausted {
            return Ok(());
        }
        batch = match next_batch(&mut cursor, limit).await? {
            Some(documents) => documents,
            None => return Ok(()),
        };
    }
}

async fn next_batch(
    cursor: &mut mongodb::raw_batch_cursor::RawBatchCursor,
    limit: Duration,
) -> Result<Option<Vec<Document>>, DbError> {
    let Some(batch) = tokio::time::timeout(limit, cursor.next())
        .await
        .map_err(|_| DbError::Timeout)?
    else {
        return Ok(None);
    };
    let batch = batch.map_err(classify)?;
    let mut documents = Vec::new();
    for item in batch.doc_slices().map_err(classify)? {
        let RawBsonRef::Document(raw) = item.map_err(|_| DbError::QueryFailed)? else {
            return Err(DbError::QueryFailed);
        };
        documents.push(Document::try_from(raw).map_err(|_| DbError::QueryFailed)?);
    }
    Ok(Some(documents))
}

/// Grid columns derived from a batch of documents.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Layout {
    fields: Vec<(String, Option<&'static str>)>,
    document_column: bool,
    sensitive_document: bool,
}

impl Layout {
    pub fn new(documents: &[Document], document_column: bool) -> Self {
        let mut fields: Vec<(String, Option<&'static str>)> = Vec::new();
        let mut positions: HashMap<String, usize> = HashMap::new();
        let mut sensitive_document = false;
        for document in documents {
            for (key, value) in document {
                if document_column && key == DOCUMENT_COLUMN {
                    continue;
                }
                let kind =
                    (!matches!(value, Bson::Null | Bson::Undefined)).then(|| type_name(value));
                match positions.get(key) {
                    Some(&position) => {
                        let seen = &mut fields[position].1;
                        match (*seen, kind) {
                            (None, kind) => *seen = kind,
                            (Some(previous), Some(kind)) if previous != kind => {
                                *seen = Some("mixed")
                            }
                            _ => {}
                        }
                    }
                    None if fields.len() < MAX_COLUMNS => {
                        positions.insert(key.clone(), fields.len());
                        fields.push((key.clone(), kind));
                    }
                    None => {}
                }
            }
            sensitive_document |= secret_names(document, 0);
        }
        if let Some(position) = fields.iter().position(|(name, _)| name == "_id") {
            let id = fields.remove(position);
            fields.insert(0, id);
        }
        Self {
            fields,
            document_column,
            sensitive_document,
        }
    }

    pub fn columns(&self) -> Vec<DbColumn> {
        let mut columns: Vec<DbColumn> = self
            .fields
            .iter()
            .map(|(name, kind)| DbColumn {
                name: name.clone(),
                type_name: kind.unwrap_or("null").into(),
                nullable: true,
                sensitive: crate::sensitive_column(name),
                primary_key: name == "_id",
            })
            .collect();
        if self.document_column {
            columns.push(DbColumn {
                name: DOCUMENT_COLUMN.into(),
                type_name: "document".into(),
                nullable: false,
                sensitive: self.sensitive_document,
                primary_key: false,
            });
        }
        columns
    }

    pub fn row(&self, document: Document) -> Result<Vec<DbCell>, DbError> {
        let mut row: Vec<DbCell> = self
            .fields
            .iter()
            .map(|(name, _)| document.get(name).map_or(DbCell::Null, cell))
            .collect();
        if self.document_column {
            row.push(DbCell::Json(extended_json(
                &Bson::Document(document),
                false,
            )));
        }
        Ok(row)
    }
}

fn secret_names(document: &Document, depth: usize) -> bool {
    depth < MAX_DEPTH
        && document.iter().any(|(key, value)| {
            crate::sensitive_column(key) || match value {
                Bson::Document(inner) => secret_names(inner, depth + 1),
                Bson::Array(items) => items.iter().take(64).any(
                    |item| matches!(item, Bson::Document(inner) if secret_names(inner, depth + 1)),
                ),
                _ => false,
            }
        })
}

/// MongoDB `$type` aliases.
pub(crate) fn type_name(value: &Bson) -> &'static str {
    match value {
        Bson::Double(_) => "double",
        Bson::String(_) => "string",
        Bson::Document(_) => "object",
        Bson::Array(_) => "array",
        Bson::Binary(_) => "binData",
        Bson::Undefined => "undefined",
        Bson::ObjectId(_) => "objectId",
        Bson::Boolean(_) => "bool",
        Bson::DateTime(_) => "date",
        Bson::Null => "null",
        Bson::RegularExpression(_) => "regex",
        Bson::DbPointer(_) => "dbPointer",
        Bson::JavaScriptCode(_) => "javascript",
        Bson::Symbol(_) => "symbol",
        Bson::JavaScriptCodeWithScope(_) => "javascriptWithScope",
        Bson::Int32(_) => "int",
        Bson::Timestamp(_) => "timestamp",
        Bson::Int64(_) => "long",
        Bson::Decimal128(_) => "decimal",
        Bson::MaxKey => "maxKey",
        Bson::MinKey => "minKey",
    }
}

/// One top-level value as a grid cell (see the module documentation).
pub(crate) fn cell(value: &Bson) -> DbCell {
    match value {
        Bson::Null | Bson::Undefined => DbCell::Null,
        Bson::Boolean(value) => DbCell::Bool(*value),
        Bson::Int32(value) => DbCell::Int(value.to_string()),
        Bson::Int64(value) => DbCell::Int(value.to_string()),
        Bson::Double(value) => DbCell::Float(if value.is_nan() {
            "NaN".into()
        } else if value.is_infinite() {
            if value.is_sign_negative() {
                "-Infinity".into()
            } else {
                "Infinity".into()
            }
        } else {
            value.to_string()
        }),
        Bson::Decimal128(value) => DbCell::Decimal(value.to_string()),
        Bson::String(value) => DbCell::Text(value.clone()),
        Bson::ObjectId(value) => DbCell::Text(value.to_hex()),
        Bson::DateTime(date) => match date.try_to_rfc3339_string() {
            Ok(text) => DbCell::Timestamp(text),
            Err(_) => DbCell::Json(extended_json(value, false)),
        },
        Bson::Binary(binary)
            if binary.subtype == BinarySubtype::Uuid && binary.bytes.len() == 16 =>
        {
            match uuid::Uuid::from_slice(&binary.bytes) {
                Ok(uuid) => DbCell::Text(uuid.to_string()),
                Err(_) => DbCell::Binary(STANDARD.encode(&binary.bytes)),
            }
        }
        Bson::Binary(binary) => DbCell::Binary(STANDARD.encode(&binary.bytes)),
        other => DbCell::Json(extended_json(other, false)),
    }
}

/// Relaxed Extended JSON text in document key order (`serde_json` maps
/// would sort the keys of every document).
pub(crate) fn extended_json(value: &Bson, pretty: bool) -> String {
    let mut out = String::new();
    write_extended_json(value, pretty, 0, &mut out);
    out
}

fn write_extended_json(value: &Bson, pretty: bool, depth: usize, out: &mut String) {
    let newline = |out: &mut String, depth: usize| {
        if pretty {
            out.push('\n');
            out.extend(std::iter::repeat_n("  ", depth));
        }
    };
    match value {
        Bson::Document(document) if !document.is_empty() => {
            out.push('{');
            for (index, (key, value)) in document.iter().enumerate() {
                if index > 0 {
                    out.push(',');
                }
                newline(out, depth + 1);
                out.push_str(&Value::String(key.clone()).to_string());
                out.push_str(if pretty { ": " } else { ":" });
                write_extended_json(value, pretty, depth + 1, out);
            }
            newline(out, depth);
            out.push('}');
        }
        Bson::Array(items) if !items.is_empty() => {
            out.push('[');
            for (index, item) in items.iter().enumerate() {
                if index > 0 {
                    out.push(',');
                }
                newline(out, depth + 1);
                write_extended_json(item, pretty, depth + 1, out);
            }
            newline(out, depth);
            out.push(']');
        }
        // Scalars and type wrappers; their inner key order is fixed by the spec.
        other => out.push_str(&other.clone().into_relaxed_extjson().to_string()),
    }
}

/// Field statistics over sampled documents. Paths are dotted for embedded
/// documents; `field[]` holds the types of array elements and `field[].x`
/// the fields of documents inside arrays. Presence counts each document once.
#[derive(Default)]
pub(crate) struct SchemaSampler {
    documents: u64,
    fields: Vec<FieldStats>,
    positions: HashMap<String, usize>,
    truncated: bool,
}

struct FieldStats {
    path: String,
    present: u64,
    types: BTreeMap<&'static str, u64>,
}

impl SchemaSampler {
    pub fn add(&mut self, document: &Document) {
        self.documents += 1;
        let mut seen = HashSet::new();
        self.visit_document("", document, 0, &mut seen);
    }

    fn visit_document(
        &mut self,
        prefix: &str,
        document: &Document,
        depth: usize,
        seen: &mut HashSet<usize>,
    ) {
        for (key, value) in document {
            let path = format!("{prefix}{key}");
            self.visit_value(path, value, depth, seen);
        }
    }

    fn visit_value(&mut self, path: String, value: &Bson, depth: usize, seen: &mut HashSet<usize>) {
        if depth >= MAX_DEPTH {
            return;
        }
        let Some(position) = self.position(&path) else {
            return;
        };
        if seen.insert(position) {
            self.fields[position].present += 1;
        }
        *self.fields[position]
            .types
            .entry(type_name(value))
            .or_default() += 1;
        match value {
            Bson::Document(inner) => {
                self.visit_document(&format!("{path}."), inner, depth + 1, seen)
            }
            Bson::Array(items) => {
                for item in items {
                    self.visit_value(format!("{path}[]"), item, depth + 1, seen);
                }
            }
            _ => {}
        }
    }

    fn position(&mut self, path: &str) -> Option<usize> {
        if let Some(position) = self.positions.get(path) {
            return Some(*position);
        }
        if self.fields.len() >= MAX_SCHEMA_PATHS {
            self.truncated = true;
            return None;
        }
        self.positions.insert(path.to_owned(), self.fields.len());
        self.fields.push(FieldStats {
            path: path.to_owned(),
            present: 0,
            types: BTreeMap::new(),
        });
        Some(self.fields.len() - 1)
    }

    pub fn report(&self, database: &str, collection: &str) -> Value {
        let total = self.documents.max(1) as f64;
        json!({
            "database": database,
            "collection": collection,
            "sampled": self.documents,
            "truncated": self.truncated,
            "fields": self.fields.iter().map(|field| json!({
                "path": field.path,
                "count": field.present,
                "presence": if self.documents == 0 { 0.0 } else { field.present as f64 / total },
                "types": field.types,
            })).collect::<Vec<_>>(),
        })
    }
}

fn object(database: &str, kind: DbObjectKind, name: &str, parent: Option<&str>) -> DbObjectRef {
    DbObjectRef {
        database: database.into(),
        schema: None,
        kind,
        name: name.into(),
        parent_name: parent.map(str::to_owned),
    }
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

/// Index node: detail = key specification plus its flags.
fn index_node(database: &str, collection: &str, index: &Document) -> DbTreeNode {
    let name = index.get_str("name").unwrap_or_default();
    let mut detail = index
        .get_document("key")
        .map(|key| extended_json(&Bson::Document(key.clone()), false))
        .unwrap_or_default();
    for flag in ["unique", "sparse", "hidden"] {
        if index.get_bool(flag).unwrap_or(false) {
            detail.push(' ');
            detail.push_str(flag);
        }
    }
    if index.contains_key("partialFilterExpression") {
        detail.push_str(" partial");
    }
    if index.contains_key("expireAfterSeconds") {
        detail.push_str(" ttl");
    }
    let mut node = node(
        object(database, DbObjectKind::Index, name, Some(collection)),
        false,
        Some(detail),
    );
    node.primary_key = name == "_id_";
    node
}

/// Sorted by name; continues after `cursor`.
fn page(mut nodes: Vec<DbTreeNode>, cursor: Option<&str>) -> DbTreePage {
    nodes.sort_by(|left, right| left.reference.name.cmp(&right.reference.name));
    let mut nodes: Vec<DbTreeNode> = nodes
        .into_iter()
        .filter(|node| cursor.is_none_or(|cursor| node.reference.name.as_str() > cursor))
        .collect();
    let next_cursor = (nodes.len() > DEFAULT_TREE_PAGE).then(|| {
        nodes.truncate(DEFAULT_TREE_PAGE);
        nodes
            .last()
            .map(|node| node.reference.name.clone())
            .unwrap_or_default()
    });
    DbTreePage { nodes, next_cursor }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bson::{oid::ObjectId, Binary, DateTime, Decimal128};

    fn ordered(text: &str) -> (Value, KeyOrder) {
        (
            serde_json::from_str(text).unwrap(),
            serde_json::from_str(text).unwrap(),
        )
    }

    #[test]
    fn commands_keep_written_key_order_and_lead_with_the_command_name() {
        let (value, order) = ordered(
            r#"{"sort":{"b":1,"a":-1},"filter":{"addr":{"zip":1,"city":"x"}},"find":"items","limit":5}"#,
        );
        let document = command_document(&value, &order).unwrap();
        assert_eq!(
            document.keys().collect::<Vec<_>>(),
            ["find", "sort", "filter", "limit"]
        );
        assert_eq!(
            document
                .get_document("sort")
                .unwrap()
                .keys()
                .collect::<Vec<_>>(),
            ["b", "a"]
        );
        let address = document
            .get_document("filter")
            .unwrap()
            .get_document("addr")
            .unwrap();
        assert_eq!(address.keys().collect::<Vec<_>>(), ["zip", "city"]);
        assert_eq!(operation(&document).unwrap(), "find");

        let (value, order) =
            ordered(r#"{"verbosity":"queryPlanner","explain":{"filter":{},"find":"items"}}"#);
        let document = command_document(&value, &order).unwrap();
        assert_eq!(document.keys().next().unwrap(), "explain");
        assert_eq!(
            document
                .get_document("explain")
                .unwrap()
                .keys()
                .next()
                .unwrap(),
            "find"
        );
    }

    #[test]
    fn extended_json_becomes_typed_bson() {
        let (value, order) = ordered(
            r#"{"find":"items","filter":{"_id":{"$oid":"64b7f0c2a1b2c3d4e5f60718"},
                "at":{"$gte":{"$date":"2024-01-02T03:04:05Z"}},"n":{"$numberLong":"9007199254740993"},
                "price":{"$numberDecimal":"10.50"},"small":7}}"#,
        );
        let document = command_document(&value, &order).unwrap();
        let filter = document.get_document("filter").unwrap();
        assert!(matches!(filter.get("_id"), Some(Bson::ObjectId(_))));
        assert!(matches!(
            filter.get_document("at").unwrap().get("$gte"),
            Some(Bson::DateTime(_))
        ));
        assert_eq!(filter.get("n"), Some(&Bson::Int64(9_007_199_254_740_993)));
        assert!(matches!(filter.get("price"), Some(Bson::Decimal128(_))));
        assert_eq!(filter.get("small"), Some(&Bson::Int32(7)));
        let (value, order) = ordered(r#"{"find":"items","filter":{"_id":{"$oid":"zz"}}}"#);
        assert_eq!(
            command_document(&value, &order),
            Err(DbError::InvalidRequest)
        );
    }

    #[test]
    fn statement_timeout_caps_max_time() {
        let mut document = doc! {"find": "items"};
        limit_time(&mut document, 60_000);
        assert_eq!(document.get("maxTimeMS"), Some(&Bson::Int64(60_000)));
        let mut document = doc! {"find": "items", "maxTimeMS": 500, "limit": 1};
        limit_time(&mut document, 60_000);
        assert_eq!(document.get("maxTimeMS"), Some(&Bson::Int64(500)));
        assert_eq!(document.keys().nth(1).unwrap(), "maxTimeMS");
        let mut document = doc! {"find": "items", "maxTimeMS": 300_000};
        limit_time(&mut document, 60_000);
        assert_eq!(document.get("maxTimeMS"), Some(&Bson::Int64(60_000)));
    }

    #[test]
    fn bson_values_render_as_typed_cells() {
        let id = ObjectId::parse_str("64b7f0c2a1b2c3d4e5f60718").unwrap();
        assert_eq!(
            cell(&Bson::ObjectId(id)),
            DbCell::Text("64b7f0c2a1b2c3d4e5f60718".into())
        );
        assert_eq!(
            cell(&Bson::DateTime(DateTime::from_millis(1_704_164_645_123))),
            DbCell::Timestamp("2024-01-02T03:04:05.123Z".into())
        );
        assert_eq!(
            cell(&Bson::Int64(i64::MIN)),
            DbCell::Int("-9223372036854775808".into())
        );
        assert_eq!(cell(&Bson::Int32(-5)), DbCell::Int("-5".into()));
        assert_eq!(cell(&Bson::Double(1.5)), DbCell::Float("1.5".into()));
        assert_eq!(cell(&Bson::Double(f64::NAN)), DbCell::Float("NaN".into()));
        assert_eq!(
            cell(&Bson::Double(f64::NEG_INFINITY)),
            DbCell::Float("-Infinity".into())
        );
        assert_eq!(
            cell(&Bson::Decimal128(
                "12345678901234567890.50".parse::<Decimal128>().unwrap()
            )),
            DbCell::Decimal("12345678901234567890.50".into())
        );
        assert_eq!(cell(&Bson::Null), DbCell::Null);
        assert_eq!(cell(&Bson::Boolean(true)), DbCell::Bool(true));
        assert_eq!(
            cell(&Bson::Binary(Binary {
                subtype: BinarySubtype::Generic,
                bytes: vec![0, 1, 2, 255],
            })),
            DbCell::Binary("AAEC/w==".into())
        );
        let uuid = uuid::Uuid::parse_str("67e55044-10b1-426f-9247-bb680e5fe0c8").unwrap();
        assert_eq!(
            cell(&Bson::Binary(Binary {
                subtype: BinarySubtype::Uuid,
                bytes: uuid.as_bytes().to_vec(),
            })),
            DbCell::Text("67e55044-10b1-426f-9247-bb680e5fe0c8".into())
        );
        let DbCell::Json(ref nested) = cell(&Bson::Document(
            doc! {"at": DateTime::from_millis(0), "big": 1i64 << 40},
        )) else {
            panic!("documents render as JSON");
        };
        let nested: Value = serde_json::from_str(nested).unwrap();
        assert_eq!(nested["at"]["$date"], "1970-01-01T00:00:00Z");
        assert_eq!(nested["big"], json!(1_099_511_627_776i64));
        assert!(
            matches!(cell(&Bson::Array(vec![Bson::Int32(1)])), DbCell::Json(ref text) if text == "[1]")
        );
    }

    #[test]
    fn documents_become_typed_columns_plus_a_json_document() {
        let id = ObjectId::new();
        let documents = vec![
            doc! {"name": "alpha", "_id": id, "qty": 1, "api_token": "t1", "tags": ["a"]},
            doc! {"_id": 2, "name": Bson::Null, "qty": 2.5, "extra": {"password": "p"}},
        ];
        let layout = Layout::new(&documents, true);
        let columns = layout.columns();
        let names: Vec<_> = columns.iter().map(|column| column.name.as_str()).collect();
        assert_eq!(
            names,
            ["_id", "name", "qty", "api_token", "tags", "extra", "_doc"]
        );
        let types: Vec<_> = columns
            .iter()
            .map(|column| column.type_name.as_str())
            .collect();
        assert_eq!(
            types,
            ["mixed", "string", "mixed", "string", "array", "object", "document"]
        );
        assert!(columns[0].primary_key);
        assert!(columns[3].sensitive && !columns[1].sensitive);
        assert!(
            columns[6].sensitive,
            "the document holds secret-looking fields"
        );

        let row = layout.row(documents[1].clone()).unwrap();
        assert_eq!(row[0], DbCell::Int("2".into()));
        assert_eq!(row[1], DbCell::Null);
        assert_eq!(row[3], DbCell::Null, "missing fields are empty cells");
        let DbCell::Json(text) = &row[6] else {
            panic!()
        };
        let document: Value = serde_json::from_str(text).unwrap();
        assert_eq!(document["extra"]["password"], "p");
        let row = layout.row(documents[0].clone()).unwrap();
        let DbCell::Json(text) = &row[6] else {
            panic!()
        };
        assert_eq!(
            serde_json::from_str::<Value>(text).unwrap()["_id"]["$oid"],
            id.to_hex()
        );

        let plain = Layout::new(&[doc! {"value": 1}], false);
        assert_eq!(plain.columns().len(), 1);
        assert!(!Layout::new(&[doc! {"a": {"b": 1}}], true).columns()[1].sensitive);
        assert_eq!(Layout::new(&[], true).columns()[0].name, DOCUMENT_COLUMN);
    }

    #[test]
    fn extended_json_keeps_document_order() {
        let id = ObjectId::parse_str("64b7f0c2a1b2c3d4e5f60718").unwrap();
        let document = Bson::Document(doc! {
            "zeta": 1, "_id": id, "nested": {"b": 1i64 << 40, "a": [1.5, "x", {}]}, "empty": [],
        });
        assert_eq!(
            extended_json(&document, false),
            r#"{"zeta":1,"_id":{"$oid":"64b7f0c2a1b2c3d4e5f60718"},"nested":{"b":1099511627776,"a":[1.5,"x",{}]},"empty":[]}"#
        );
        let pretty = extended_json(&Bson::Document(doc! {"key": {"b": 1, "a": -1}}), true);
        assert_eq!(
            pretty,
            "{\n  \"key\": {\n    \"b\": 1,\n    \"a\": -1\n  }\n}"
        );
        let parsed: Value = serde_json::from_str(&extended_json(&document, true)).unwrap();
        assert_eq!(parsed["_id"]["$oid"], id.to_hex());
        let layout = Layout::new(&[doc! {"z": 1, "a": 2}], true);
        assert_eq!(
            layout.row(doc! {"z": 1, "a": 2}).unwrap()[2],
            DbCell::Json(r#"{"z":1,"a":2}"#.into())
        );
    }

    #[test]
    fn schema_sampler_reports_types_and_presence_per_path() {
        let mut sampler = SchemaSampler::default();
        sampler.add(&doc! {"_id": 1, "name": "a", "address": {"city": "x"}, "tags": ["t", 1]});
        sampler.add(&doc! {"_id": 2, "name": 5, "items": [{"sku": "s1"}, {"sku": "s2", "qty": 1}]});
        sampler.add(&doc! {"_id": 3, "name": Bson::Null, "address": {"city": "y", "zip": 1}});
        sampler.add(&doc! {"_id": 4});
        let report = sampler.report("db", "people");
        assert_eq!(report["sampled"], 4);
        let field = |path: &str| {
            report["fields"]
                .as_array()
                .unwrap()
                .iter()
                .find(|field| field["path"] == path)
                .cloned()
                .unwrap_or_else(|| panic!("missing {path}"))
        };
        assert_eq!(field("_id")["presence"], 1.0);
        assert_eq!(field("_id")["types"], json!({"int": 4}));
        assert_eq!(field("name")["presence"], 0.75);
        assert_eq!(
            field("name")["types"],
            json!({"string": 1, "int": 1, "null": 1})
        );
        assert_eq!(field("address.city")["presence"], 0.5);
        assert_eq!(field("address.zip")["count"], 1);
        assert_eq!(field("tags[]")["types"], json!({"string": 1, "int": 1}));
        assert_eq!(field("items")["types"], json!({"array": 1}));
        assert_eq!(
            field("items[].sku")["count"],
            1,
            "presence counts documents"
        );
        assert_eq!(field("items[].sku")["types"], json!({"string": 2}));
        assert_eq!(field("items[].qty")["presence"], 0.25);
        assert_eq!(report["truncated"], false);
    }

    #[test]
    fn read_only_roles_are_verified_only_for_read_actions() {
        let status = |actions: Vec<&str>, users: bool| {
            doc! {"authInfo": {
                "authenticatedUsers": if users { vec![doc!{"user": "u", "db": "admin"}] } else { vec![] },
                "authenticatedUserPrivileges": [
                    {"resource": {"db": "app", "collection": ""}, "actions": actions},
                ],
            }}
        };
        assert!(roles_read_only(&status(
            vec!["find", "listCollections", "collStats"],
            true
        )));
        assert!(!roles_read_only(&status(vec!["find", "insert"], true)));
        assert!(!roles_read_only(&status(vec!["find"], false)));
        assert!(!roles_read_only(
            &doc! {"authInfo": {"authenticatedUsers": [], "authenticatedUserPrivileges": []}}
        ));
    }

    #[test]
    fn replies_drop_wire_bookkeeping_and_report_affected_rows() {
        let reply = doc! {"n": 2, "nModified": 1, "ok": 1.0, "$clusterTime": {}, "operationTime": 1, "electionId": 1};
        assert_eq!(affected("update", &reply), Some(2));
        assert_eq!(affected("find", &reply), None);
        assert_eq!(
            affected("findAndModify", &doc! {"lastErrorObject": {"n": 1i64}}),
            Some(1)
        );
        assert_eq!(
            strip_reply(reply).keys().collect::<Vec<_>>(),
            ["n", "nModified"]
        );
    }

    #[test]
    fn listings_page_by_name() {
        let nodes: Vec<_> = (0..DEFAULT_TREE_PAGE + 5)
            .rev()
            .map(|index| {
                node(
                    object(
                        "db",
                        DbObjectKind::Collection,
                        &format!("c{index:04}"),
                        None,
                    ),
                    true,
                    None,
                )
            })
            .collect();
        let first = page(nodes.clone(), None);
        assert_eq!(first.nodes.len(), DEFAULT_TREE_PAGE);
        assert_eq!(first.nodes[0].reference.name, "c0000");
        let second = page(nodes, first.next_cursor.as_deref());
        assert_eq!(second.nodes.len(), 5);
        assert!(second.next_cursor.is_none());
        let index = index_node(
            "db",
            "items",
            &doc! {"name": "email_1", "key": {"email": 1, "at": -1}, "unique": true},
        );
        assert_eq!(
            index.detail.as_deref(),
            Some(r#"{"email":1,"at":-1} unique"#)
        );
        assert_eq!(index.reference.parent_name.as_deref(), Some("items"));
    }

    /// Every destination goes to one local endpoint, like an SSH forward.
    #[derive(Debug)]
    struct Tunnel(std::net::SocketAddr);
    #[async_trait::async_trait]
    impl DbConnector for Tunnel {
        async fn connect(
            &self,
            _: &DbDestination,
            _: CancellationToken,
        ) -> Result<crate::transport::DbIo, DbError> {
            let stream = tokio::net::TcpStream::connect(self.0)
                .await
                .map_err(|_| DbError::ConnectionFailed)?;
            Ok(Box::new(stream))
        }
    }

    /// TLS echo server presenting a self-signed certificate for `name`.
    async fn tls_echo(name: &str) -> (std::net::SocketAddr, String) {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let certificate = rcgen::generate_simple_self_signed(vec![name.into()]).unwrap();
        let config = rustls::ServerConfig::builder_with_provider(Arc::new(
            rustls::crypto::ring::default_provider(),
        ))
        .with_safe_default_protocol_versions()
        .unwrap()
        .with_no_client_auth()
        .with_single_cert(
            vec![certificate.cert.der().clone()],
            rustls::pki_types::PrivatePkcs8KeyDer::from(certificate.signing_key.serialize_der())
                .into(),
        )
        .unwrap();
        let acceptor = tokio_rustls::TlsAcceptor::from(Arc::new(config));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        tokio::spawn(async move {
            while let Ok((socket, _)) = listener.accept().await {
                let acceptor = acceptor.clone();
                tokio::spawn(async move {
                    let Ok(mut stream) = acceptor.accept(socket).await else {
                        return;
                    };
                    let mut buffer = [0; 64];
                    while let Ok(read) = stream.read(&mut buffer).await {
                        if read == 0 || stream.write_all(&buffer[..read]).await.is_err() {
                            return;
                        }
                        let _ = stream.flush().await;
                    }
                });
            }
        });
        (address, certificate.cert.pem())
    }

    /// The driver's side of the relay: SOCKS5 with the session credentials.
    async fn socks(
        relay: &MongoRelay,
        host: &str,
        port: u16,
    ) -> std::io::Result<tokio::net::TcpStream> {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let mut stream = tokio::net::TcpStream::connect(relay.relay.address).await?;
        stream.write_all(&[5, 1, 2]).await?;
        let mut reply = [0; 2];
        stream.read_exact(&mut reply).await?;
        let user = relay.authentication.username.expose_secret();
        let password = relay.authentication.password.expose_secret();
        let mut authentication = vec![1, user.len() as u8];
        authentication.extend(user.as_bytes());
        authentication.push(password.len() as u8);
        authentication.extend(password.as_bytes());
        stream.write_all(&authentication).await?;
        stream.read_exact(&mut reply).await?;
        let mut request = vec![5, 1, 0, 3, host.len() as u8];
        request.extend(host.as_bytes());
        request.extend(port.to_be_bytes());
        stream.write_all(&request).await?;
        let mut answer = [0; 10];
        stream.read_exact(&mut answer).await?;
        if answer[1] != 0 {
            return Err(std::io::ErrorKind::ConnectionRefused.into());
        }
        Ok(stream)
    }

    #[tokio::test]
    async fn relay_verifies_the_database_host_name_through_a_tunnel() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        // (certificate name, server-name override, accepted)
        for (certificate, server_name, accepted) in [
            ("db.internal", None, true),
            ("spoofed.internal", None, false),
            ("spoofed.internal", Some("spoofed.internal"), true),
        ] {
            let (address, authority) = tls_echo(certificate).await;
            let mut connection =
                DbConnection::new("tls", "db.internal", cc_models::database::DbEngine::Mongodb);
            connection.tls.mode = DbTlsMode::Verify;
            connection.tls.certificate_authority_pem = Some(authority);
            connection.tls.server_name = server_name.map(str::to_owned);
            let tls = TlsProfile::new(&connection, &DbCredentials::default()).unwrap();
            let destination = DbDestination::new("db.internal", 27017).unwrap();
            let connector: Arc<dyn DbConnector> = Arc::new(Tunnel(address));
            let scope = CancellationToken::new();
            let preflight = preflight(
                &connector,
                &destination,
                &tls,
                Duration::from_secs(5),
                &scope,
            )
            .await;
            assert_eq!(preflight.is_ok(), accepted, "{certificate}");
            if !accepted {
                assert_eq!(preflight, Err(DbError::TlsFailed));
            }
            let relay = MongoRelay::new(
                connector,
                destinations(std::slice::from_ref(&destination)),
                tls,
                scope.clone(),
            )
            .await
            .unwrap();
            match socks(&relay, "db.internal", 27017).await {
                Ok(mut stream) => {
                    assert!(accepted, "{certificate} was accepted for db.internal");
                    stream.write_all(b"ping").await.unwrap();
                    let mut echo = [0; 4];
                    stream.read_exact(&mut echo).await.unwrap();
                    assert_eq!(&echo, b"ping");
                }
                Err(_) => assert!(!accepted, "{certificate} was refused"),
            }
            // Only admitted destinations are relayed.
            assert!(socks(&relay, "other.internal", 27017).await.is_err());
            assert!(socks(&relay, "db.internal", 27018).await.is_err());
            scope.cancel();
        }
    }

    /// Opt-in: CC_DB_LIVE_MONGO=host:port:user:password:database (authSource admin).
    #[tokio::test]
    async fn live_sampled_schema_and_description() {
        let Ok(spec) = std::env::var("CC_DB_LIVE_MONGO") else {
            eprintln!("CC_DB_LIVE_MONGO not set; skipped");
            return;
        };
        let parts: Vec<&str> = spec.splitn(5, ':').collect();
        let mut connection =
            DbConnection::new("live", parts[0], cc_models::database::DbEngine::Mongodb);
        connection.port = parts[1].parse().unwrap();
        connection.username = parts[2].into();
        connection.database = format!("{}_inspect", parts[4]);
        connection.tls.mode = DbTlsMode::Disable;
        connection.production = false;
        connection.options = DbOptions::Mongodb {
            auth_source: Some("admin".into()),
            replica_set: None,
            srv: false,
            read_preference: MongoReadPreference::Primary,
        };
        let credentials = DbCredentials {
            password: Some(secrecy::SecretString::from(parts[3].to_owned())),
            client_key_pem: None,
        };
        let mut mongo = Mongo::connect(
            &connection,
            &credentials,
            Arc::new(crate::transport::DirectConnector),
            CancellationToken::new(),
        )
        .await
        .unwrap();
        assert!(mongo.version.starts_with(char::is_numeric));
        assert!(!format!("{mongo:?}").contains(parts[3]));
        let database = mongo.client.database(&connection.database);
        database
            .collection::<Document>("people")
            .drop()
            .await
            .unwrap();
        let people: Vec<Document> = (0..200)
            .map(|index| {
                let mut person = doc! {"_id": index, "name": format!("p{index}")};
                if index % 4 == 0 {
                    person.insert("email", format!("p{index}@example.test"));
                }
                if index % 2 == 0 {
                    person.insert("address", doc! {"city": "x"});
                }
                person
            })
            .collect();
        database
            .collection::<Document>("people")
            .insert_many(people)
            .await
            .unwrap();
        database
            .collection::<Document>("people")
            .create_index(
                mongodb::IndexModel::builder()
                    .keys(doc! {"name": 1})
                    .build(),
            )
            .await
            .unwrap();
        let collection = object(
            &connection.database,
            DbObjectKind::Collection,
            "people",
            None,
        );
        let report = mongo.inspect(&collection, 5000).await.unwrap();
        assert_eq!(
            report["sampled"], 200,
            "the sample is the whole small collection"
        );
        let field = |path: &str| {
            report["fields"]
                .as_array()
                .unwrap()
                .iter()
                .find(|field| field["path"] == path)
                .cloned()
                .unwrap()
        };
        assert_eq!(field("_id")["presence"], 1.0);
        assert_eq!(field("email")["presence"], 0.25);
        assert_eq!(field("address.city")["presence"], 0.5);
        assert_eq!(field("name")["types"], json!({"string": 200}));
        let description: Value =
            serde_json::from_str(&mongo.describe(&collection).await.unwrap()).unwrap();
        assert_eq!(description["name"], "people");
        assert_eq!(description["indexes"].as_array().unwrap().len(), 2);
        let index = object(
            &connection.database,
            DbObjectKind::Index,
            "name_1",
            Some("people"),
        );
        let description: Value =
            serde_json::from_str(&mongo.describe(&index).await.unwrap()).unwrap();
        assert_eq!(description["key"], json!({"name": 1}));
        let database_description: Value = serde_json::from_str(
            &mongo
                .describe(&object(
                    &connection.database,
                    DbObjectKind::Database,
                    &connection.database,
                    None,
                ))
                .await
                .unwrap(),
        )
        .unwrap();
        assert_eq!(database_description["collections"], json!(["people"]));
        database.drop().await.unwrap();
    }
}
