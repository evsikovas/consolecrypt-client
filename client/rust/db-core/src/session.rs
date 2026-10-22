//! One live database session: the native driver, the query guard built from the
//! vault connection, guarded query streams and read-only structure browsing.
//! Production and read-only policy come only from the stored connection.
use crate::{
    cassandra::Cassandra,
    clickhouse::ClickHouse,
    guard::{DbGuard, GuardPolicy},
    mongo::Mongo,
    mysql::Mysql,
    parser::ParsedStatement,
    postgres::Postgres,
    redis_driver::Redis,
    streaming::{DbQueryStream, QuerySink, RowConsumer},
    transport::{DbConnector, DbCredentials, DbPath},
    DbCell, DbColumn, DbError, DbObjectKind, DbObjectRef, DbParameter, DbQueryRequest,
    DbServerReadOnly, DbSessionInfo, DbTransportSecurity, DbTreeNode, DbTreePage,
    DEFAULT_RESULT_BYTES,
};
use async_trait::async_trait;
use cc_models::database::{DbConnection, DbEngine, DbOptions};
use futures::future::BoxFuture;
use std::{fmt, sync::Arc};
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;

/// One native driver per engine. SQL engines share the catalogue queries in
/// [`DbSession::tree`]; other engines browse and describe through the driver.
pub(crate) enum Driver {
    Postgres(Postgres),
    Mysql(Mysql),
    Redis(Redis),
    ClickHouse(ClickHouse),
    Mongo(Mongo),
    Cassandra(Cassandra),
}

impl Driver {
    /// Lets ClickHouse leave `readonly=2` for one production write the guard
    /// permitted; other engines take the access mode from `read_only`.
    pub(crate) fn set_production_write(&mut self, allowed: bool) {
        if let Self::ClickHouse(driver) = self {
            driver.production_write = allowed;
        }
    }

    fn version(&self) -> &str {
        match self {
            Self::Postgres(driver) => &driver.version,
            Self::Mysql(driver) => &driver.version,
            Self::Redis(driver) => &driver.version,
            Self::ClickHouse(driver) => &driver.version,
            Self::Mongo(driver) => &driver.version,
            Self::Cassandra(driver) => &driver.version,
        }
    }

    /// Runs one guarded statement in the form its engine parsed.
    pub(crate) fn run<'a>(
        &'a mut self,
        statement: &'a ParsedStatement,
        read_only: bool,
        sink: &'a mut (dyn RowConsumer + Send),
        cancel: CancellationToken,
    ) -> BoxFuture<'a, Result<(), DbError>> {
        match statement {
            ParsedStatement::Sql { text, parameters } => match self {
                Self::Cassandra(_) => Box::pin(async { Err(DbError::InvalidRequest) }),
                _ => self.query(text, parameters, read_only, sink, cancel),
            },
            ParsedStatement::Cql {
                text,
                parameters,
                keyspace,
            } => match self {
                Self::Cassandra(driver) => driver.run(
                    text,
                    parameters,
                    keyspace.as_deref(),
                    read_only,
                    sink,
                    cancel,
                ),
                _ => Box::pin(async { Err(DbError::InvalidRequest) }),
            },
            ParsedStatement::Redis { argv } => match self {
                Self::Redis(driver) => driver.command(argv, sink, cancel),
                _ => Box::pin(async { Err(DbError::InvalidRequest) }),
            },
            ParsedStatement::Mongo { command, order } => match self {
                Self::Mongo(driver) => driver.command(command, order, read_only, sink, cancel),
                _ => Box::pin(async { Err(DbError::InvalidRequest) }),
            },
        }
    }

    /// Structure below `parent` for engines without SQL catalogues; `cursor`
    /// continues a paged listing (Redis SCAN).
    fn browse<'a>(
        &'a mut self,
        parent: Option<&'a DbObjectRef>,
        cursor: Option<&'a str>,
        cancel: CancellationToken,
    ) -> BoxFuture<'a, Result<DbTreePage, DbError>> {
        match self {
            Self::Cassandra(driver) => driver.browse(parent, cancel),
            Self::Redis(driver) => driver.browse(parent, cursor),
            Self::ClickHouse(driver) => driver.browse(parent, cursor),
            Self::Mongo(driver) => driver.browse(parent, cursor),
            _ => Box::pin(async {
                Ok(DbTreePage {
                    nodes: Vec::new(),
                    next_cursor: None,
                })
            }),
        }
    }

    /// Plain SQL with bound parameters (catalogue and DDL lookups).
    pub(crate) fn query<'a>(
        &'a mut self,
        sql: &'a str,
        bindings: &'a [DbParameter],
        read_only: bool,
        sink: &'a mut (dyn RowConsumer + Send),
        cancel: CancellationToken,
    ) -> BoxFuture<'a, Result<(), DbError>> {
        Box::pin(async move {
            match self {
                Self::Postgres(driver) => {
                    driver.query(sql, bindings, read_only, sink, cancel).await
                }
                Self::Mysql(driver) => driver.query(sql, bindings, read_only, sink, cancel).await,
                Self::Redis(_) => Err(DbError::InvalidRequest),
                Self::ClickHouse(driver) => {
                    driver.query(sql, bindings, read_only, sink, cancel).await
                }
                Self::Mongo(_) => Err(DbError::InvalidRequest),
                // Catalogue CQL with `?` markers and bound names.
                Self::Cassandra(driver) => {
                    driver
                        .run(sql, bindings, None, read_only, sink, cancel)
                        .await
                }
            }
        })
    }
}

/// Turns one catalogue row into a structure node.
type NodeBuilder = fn(&[String], Option<&DbObjectRef>) -> DbTreeNode;

pub struct DbSession {
    pub(crate) info: DbSessionInfo,
    pub(crate) guard: DbGuard,
    pub(crate) driver: Mutex<Driver>,
    pub(crate) scope: CancellationToken,
    pub(crate) completion: Mutex<Option<crate::introspect::CompletionCache>>,
    max_rows: u32,
    result_bytes: usize,
}

impl fmt::Debug for DbSession {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("DbSession")
            .field("id", &self.info.id)
            .finish_non_exhaustive()
    }
}

impl DbSession {
    /// Opens PostgreSQL, MySQL/MariaDB, ClickHouse, Redis or MongoDB.
    pub async fn open(
        id: String,
        connection: &DbConnection,
        credentials: &DbCredentials,
        connector: Arc<dyn DbConnector>,
        mobile: bool,
    ) -> Result<Self, DbError> {
        let scope = CancellationToken::new();
        // Other engines never continue without TLS unless TLS is disabled;
        // their status names the route (shown only, never a TLS decision).
        let path = if connector.tunnelled() {
            DbPath::Tunnel
        } else if connection.host.eq_ignore_ascii_case("localhost")
            || connection
                .host
                .parse::<std::net::IpAddr>()
                .is_ok_and(|ip| ip.to_canonical().is_loopback())
        {
            DbPath::Loopback
        } else {
            DbPath::Network
        };
        let mut transport_security = DbTransportSecurity::of(connection.tls.mode, false, path);
        let driver = match connection.engine {
            DbEngine::Postgresql => {
                let driver =
                    Postgres::connect(connection, credentials, connector, scope.clone()).await?;
                transport_security = driver.security;
                Driver::Postgres(driver)
            }
            DbEngine::Mysql | DbEngine::Mariadb => Driver::Mysql(
                Mysql::connect(connection, credentials, connector, scope.clone()).await?,
            ),
            DbEngine::Redis => Driver::Redis(
                Redis::connect(connection, credentials, connector, scope.clone()).await?,
            ),
            DbEngine::Clickhouse => Driver::ClickHouse(
                ClickHouse::connect(connection, credentials, connector, scope.clone()).await?,
            ),
            DbEngine::Mongodb => Driver::Mongo(
                Mongo::connect(connection, credentials, connector, scope.clone()).await?,
            ),
            DbEngine::Cassandra => {
                let driver =
                    Cassandra::connect(connection, credentials, connector, scope.clone()).await?;
                transport_security = driver.security;
                Driver::Cassandra(driver)
            }
            DbEngine::Unknown => return Err(DbError::InvalidRequest),
        };
        let server_version = driver.version().to_owned();
        let default_schema = match &connection.options {
            DbOptions::Postgresql { search_path, .. } => search_path
                .first()
                .cloned()
                .unwrap_or_else(|| "public".into()),
            _ if connection.database.is_empty() => "public".into(),
            _ => connection.database.clone(),
        };
        let guard = DbGuard::new(
            GuardPolicy {
                engine: connection.engine,
                session_id: id.clone(),
                connection_id: connection.id,
                connection_revision: connection.updated_at.to_string(),
                default_schema,
                connection_name: connection.name.clone(),
                production: connection.production,
                mobile,
            },
            scope.clone(),
        );
        let read_only = guard.is_read_only();
        let server_read_only = match &driver {
            // MongoDB has no read-only session; only its roles can be verified.
            Driver::Mongo(driver) => driver.server_read_only.clone(),
            // Redis has no server switch (use a read-only ACL user); CQL has
            // no read-only session either (use a role with SELECT only).
            Driver::Redis(_) | Driver::Cassandra(_) => DbServerReadOnly::Unverified,
            // SQL drivers open every guarded read in a server read-only
            // transaction; ClickHouse sends readonly=2 with every read.
            _ if read_only => DbServerReadOnly::Enforced,
            _ => DbServerReadOnly::Unverified,
        };
        Ok(Self {
            info: DbSessionInfo {
                id,
                connection_id: connection.id.to_string(),
                engine: connection.engine,
                server_version,
                production: connection.production,
                read_only,
                server_read_only,
                transport_security,
            },
            guard,
            driver: Mutex::new(driver),
            scope,
            completion: Mutex::new(None),
            max_rows: connection.limits.max_rows.clamp(1, 100_000),
            result_bytes: (connection.limits.result_memory_bytes as usize)
                .clamp(1 << 20, DEFAULT_RESULT_BYTES),
        })
    }

    pub fn info(&self) -> &DbSessionInfo {
        &self.info
    }

    pub fn guard(&self) -> &DbGuard {
        &self.guard
    }

    /// Runs the guarded statements one after another in a background task and
    /// streams their results. The first page holds `max_rows`; more is pulled
    /// with `load_more` on the returned control.
    pub fn execute(self: &Arc<Self>, request: &DbQueryRequest) -> Result<DbQueryStream, DbError> {
        let statements = self.guard.prepare(request)?;
        let (stream, mut sink) = QuerySink::channel(
            self.max_rows.min(1000),
            self.result_bytes,
            self.scope.clone(),
        );
        let session = Arc::clone(self);
        tokio::spawn(async move {
            let cancel = sink.cancel.clone();
            if let [statement] = statements.as_slice() {
                if let ParsedStatement::Sql { text, .. } | ParsedStatement::Cql { text, .. } =
                    statement.parsed()
                {
                    if statement.is_read_only() {
                        if let Some((table, key)) = session.editable_source(text).await {
                            sink.set_editable(table, key);
                        }
                    }
                }
            }
            let result = async {
                let mut driver = session.driver.lock().await;
                for statement in &statements {
                    session.guard.validate_permit(statement)?;
                    driver.set_production_write(statement.production_write());
                    let result = driver
                        .run(
                            statement.parsed(),
                            statement.is_read_only(),
                            &mut sink,
                            cancel.clone(),
                        )
                        .await;
                    driver.set_production_write(false);
                    result?;
                }
                Ok(())
            }
            .await;
            sink.finish(result).await;
        });
        Ok(stream)
    }

    /// Children of `parent` (None = the top level). Fixed catalogue queries in
    /// a read-only transaction; names travel as bound parameters.
    pub async fn tree(
        &self,
        parent: Option<&DbObjectRef>,
        cursor: Option<&str>,
    ) -> Result<DbTreePage, DbError> {
        let mut driver = self.driver.lock().await;
        let engine = self.info.engine;
        let text = |value: &str| DbParameter {
            name: String::new(),
            value: DbCell::Text(value.into()),
            type_hint: None,
        };
        let (sql, bindings, build): (&str, Vec<DbParameter>, NodeBuilder) =
            match (engine, parent.map(|p| p.kind)) {
                (DbEngine::Postgresql, None) => (
                    "SELECT n.nspname FROM pg_catalog.pg_namespace n WHERE n.nspname NOT LIKE 'pg\\_%' \
                     AND n.nspname <> 'information_schema' ORDER BY 1",
                    vec![],
                    |row, _| node(DbObjectRef::new("", None, DbObjectKind::Schema, &row[0], None), true, None),
                ),
                (DbEngine::Postgresql, Some(DbObjectKind::Schema)) => (
                    "SELECT c.relname, c.relkind::text FROM pg_catalog.pg_class c \
                     JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace \
                     WHERE n.nspname = $1 AND c.relkind IN ('r', 'p', 'v', 'm', 'f', 'S') \
                     UNION SELECT p.proname, CASE p.prokind WHEN 'p' THEN 'P' ELSE 'F' END \
                     FROM pg_catalog.pg_proc p JOIN pg_catalog.pg_namespace n ON n.oid = p.pronamespace \
                     WHERE n.nspname = $1 AND p.prokind IN ('f', 'p') ORDER BY 2, 1",
                    vec![text(&parent.unwrap().name)],
                    |row, parent| {
                        let kind = match row[1].as_str() {
                            "v" => DbObjectKind::View,
                            "m" => DbObjectKind::MaterializedView,
                            "S" => DbObjectKind::Sequence,
                            "F" => DbObjectKind::Function,
                            "P" => DbObjectKind::Procedure,
                            _ => DbObjectKind::Table,
                        };
                        let schema = parent.map(|p| p.name.clone());
                        let children = matches!(kind, DbObjectKind::Table | DbObjectKind::View | DbObjectKind::MaterializedView);
                        node(DbObjectRef::new("", schema, kind, &row[0], None), children, None)
                    },
                ),
                (DbEngine::Postgresql, Some(DbObjectKind::Table | DbObjectKind::View | DbObjectKind::MaterializedView)) => (
                    "SELECT * FROM (SELECT a.attname, pg_catalog.format_type(a.atttypid, a.atttypmod), (NOT a.attnotnull)::text, \
                     COALESCE(pg_catalog.pg_get_expr(d.adbin, d.adrelid), ''), \
                     (EXISTS (SELECT 1 FROM pg_catalog.pg_index i WHERE i.indrelid = c.oid AND i.indisprimary \
                       AND a.attnum = ANY (i.indkey)))::text, 'column', \
                     COALESCE((SELECT fn.nspname || '.' || fc.relname FROM pg_catalog.pg_constraint fk \
                       JOIN pg_catalog.pg_class fc ON fc.oid = fk.confrelid \
                       JOIN pg_catalog.pg_namespace fn ON fn.oid = fc.relnamespace \
                       WHERE fk.conrelid = c.oid AND fk.contype = 'f' AND a.attnum = ANY (fk.conkey) LIMIT 1), ''), \
                     0 AS grp, a.attnum::int AS ord \
                     FROM pg_catalog.pg_attribute a JOIN pg_catalog.pg_class c ON c.oid = a.attrelid \
                     JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace \
                     LEFT JOIN pg_catalog.pg_attrdef d ON d.adrelid = a.attrelid AND d.adnum = a.attnum \
                     WHERE n.nspname = $1 AND c.relname = $2 AND a.attnum > 0 AND NOT a.attisdropped \
                     UNION ALL SELECT ic.relname, pg_catalog.pg_get_indexdef(i.indexrelid), '', '', i.indisprimary::text, 'index', '', 1, 0 \
                     FROM pg_catalog.pg_index i JOIN pg_catalog.pg_class ic ON ic.oid = i.indexrelid \
                     JOIN pg_catalog.pg_class c ON c.oid = i.indrelid JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace \
                     WHERE n.nspname = $1 AND c.relname = $2 \
                     UNION ALL SELECT con.conname, pg_catalog.pg_get_constraintdef(con.oid, true), '', '', '', 'constraint', '', 2, 0 \
                     FROM pg_catalog.pg_constraint con JOIN pg_catalog.pg_class c ON c.oid = con.conrelid \
                     JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace WHERE n.nspname = $1 AND c.relname = $2 \
                     UNION ALL SELECT t.tgname, pg_catalog.pg_get_triggerdef(t.oid, true), '', '', '', 'trigger', '', 3, 0 \
                     FROM pg_catalog.pg_trigger t JOIN pg_catalog.pg_class c ON c.oid = t.tgrelid \
                     JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace \
                     WHERE n.nspname = $1 AND c.relname = $2 AND NOT t.tgisinternal) children ORDER BY grp, ord, 1",
                    vec![text(parent.unwrap().schema.as_deref().unwrap_or("public")), text(&parent.unwrap().name)],
                    table_child,
                ),
                (DbEngine::Mysql | DbEngine::Mariadb, None) => (
                    "SELECT schema_name FROM information_schema.schemata ORDER BY 1",
                    vec![],
                    |row, _| node(DbObjectRef::new(&row[0], None, DbObjectKind::Database, &row[0], None), true, None),
                ),
                (DbEngine::Mysql | DbEngine::Mariadb, Some(DbObjectKind::Database)) => (
                    "SELECT table_name, table_type FROM information_schema.tables WHERE table_schema = ? \
                     UNION ALL SELECT routine_name, routine_type FROM information_schema.routines WHERE routine_schema = ? \
                     ORDER BY 2, 1",
                    vec![text(&parent.unwrap().name), text(&parent.unwrap().name)],
                    |row, parent| {
                        let kind = match row[1].as_str() {
                            "PROCEDURE" => DbObjectKind::Procedure,
                            "FUNCTION" => DbObjectKind::Function,
                            kind if kind.contains("VIEW") => DbObjectKind::View,
                            _ => DbObjectKind::Table,
                        };
                        let database = parent.map(|p| p.name.clone()).unwrap_or_default();
                        let children = matches!(kind, DbObjectKind::Table | DbObjectKind::View);
                        node(DbObjectRef::new(&database, None, kind, &row[0], None), children, None)
                    },
                ),
                (DbEngine::Mysql | DbEngine::Mariadb, Some(DbObjectKind::Table | DbObjectKind::View)) => (
                    "SELECT * FROM (SELECT c.column_name AS name, c.column_type AS detail, IF(c.is_nullable = 'YES', 'true', 'false'), \
                     COALESCE(c.column_default, ''), IF(c.column_key = 'PRI', 'true', 'false'), 'column', \
                     COALESCE((SELECT CONCAT(k.referenced_table_schema, '.', k.referenced_table_name) \
                       FROM information_schema.key_column_usage k WHERE k.table_schema = c.table_schema \
                       AND k.table_name = c.table_name AND k.column_name = c.column_name \
                       AND k.referenced_table_name IS NOT NULL LIMIT 1), ''), 0 AS grp, c.ordinal_position AS ord \
                     FROM information_schema.columns c WHERE c.table_schema = ? AND c.table_name = ? \
                     UNION ALL SELECT s.index_name, CONCAT(IF(MAX(s.non_unique) = 0, 'UNIQUE ', ''), s.index_type, ' (', \
                       GROUP_CONCAT(s.column_name ORDER BY s.seq_in_index SEPARATOR ', '), ')'), '', '', \
                       IF(s.index_name = 'PRIMARY', 'true', 'false'), 'index', '', 1, 0 \
                     FROM information_schema.statistics s WHERE s.table_schema = ? AND s.table_name = ? GROUP BY s.index_name, s.index_type \
                     UNION ALL SELECT t.trigger_name, CONCAT(t.action_timing, ' ', t.event_manipulation), '', '', 'false', 'trigger', '', 3, 0 \
                     FROM information_schema.triggers t WHERE t.event_object_schema = ? AND t.event_object_table = ?) children \
                     ORDER BY grp, ord, 1",
                    vec![
                        text(&parent.unwrap().database),
                        text(&parent.unwrap().name),
                        text(&parent.unwrap().database),
                        text(&parent.unwrap().name),
                        text(&parent.unwrap().database),
                        text(&parent.unwrap().name),
                    ],
                    table_child,
                ),
                (DbEngine::Postgresql | DbEngine::Mysql | DbEngine::Mariadb, _) => {
                    return Ok(DbTreePage { nodes: Vec::new(), next_cursor: None })
                }
                _ => {
                    return driver.browse(parent, cursor, self.scope.child_token()).await
                }
            };
        let mut rows = Collect::default();
        driver
            .query(sql, &bindings, true, &mut rows, self.scope.child_token())
            .await?;
        Ok(DbTreePage {
            nodes: rows.rows.iter().map(|row| build(row, parent)).collect(),
            next_cursor: None,
        })
    }

    /// Stops running queries and refuses new ones; the driver closes on drop.
    pub fn close(&self) {
        self.guard.close();
        self.scope.cancel();
    }
}

impl Drop for DbSession {
    fn drop(&mut self) {
        self.scope.cancel();
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

/// A column, index, constraint or trigger of a table: name, detail, nullable,
/// default, primary key, kind and the referenced `schema.table` of a foreign key.
fn table_child(row: &[String], parent: Option<&DbObjectRef>) -> DbTreeNode {
    let parent = parent
        .cloned()
        .unwrap_or_else(|| DbObjectRef::new("", None, DbObjectKind::Table, "", None));
    let kind = match row[5].as_str() {
        "index" => DbObjectKind::Index,
        "constraint" => DbObjectKind::Constraint,
        "trigger" => DbObjectKind::Trigger,
        _ => DbObjectKind::Column,
    };
    let foreign_key = row[6].split_once('.').map(|(schema, table)| DbObjectRef {
        database: if parent.schema.is_none() {
            schema.to_owned()
        } else {
            parent.database.clone()
        },
        schema: parent.schema.as_ref().map(|_| schema.to_owned()),
        kind: DbObjectKind::Table,
        name: table.to_owned(),
        parent_name: None,
    });
    DbTreeNode {
        reference: DbObjectRef::new(
            &parent.database,
            parent.schema.clone(),
            kind,
            &row[0],
            Some(parent.name),
        ),
        has_children: false,
        detail: Some(row[1].clone()),
        nullable: (kind == DbObjectKind::Column).then(|| row[2] == "true"),
        default_value: (!row[3].is_empty()).then(|| row[3].clone()),
        primary_key: row[4] == "true",
        foreign_key,
    }
}

impl DbObjectRef {
    fn new(
        database: &str,
        schema: Option<String>,
        kind: DbObjectKind,
        name: &str,
        parent_name: Option<String>,
    ) -> Self {
        Self {
            database: database.into(),
            schema,
            kind,
            name: name.into(),
            parent_name,
        }
    }
}

/// Collects a small catalogue result as text.
#[derive(Default)]
pub(crate) struct Collect {
    pub rows: Vec<Vec<String>>,
}

#[async_trait]
impl RowConsumer for Collect {
    async fn start(&mut self, _: Vec<DbColumn>, _: Option<DbObjectRef>) -> Result<(), DbError> {
        Ok(())
    }

    async fn row(&mut self, row: Vec<DbCell>) -> Result<bool, DbError> {
        if self.rows.len() >= crate::DEFAULT_TREE_PAGE * 25 {
            return Ok(false);
        }
        self.rows.push(
            row.iter()
                .map(|cell| match cell {
                    DbCell::Null => String::new(),
                    DbCell::Bool(value) => value.to_string(),
                    DbCell::Int(v)
                    | DbCell::Uint(v)
                    | DbCell::Decimal(v)
                    | DbCell::Float(v)
                    | DbCell::Text(v)
                    | DbCell::Date(v)
                    | DbCell::Time(v)
                    | DbCell::Timestamp(v)
                    | DbCell::Binary(v)
                    | DbCell::Json(v) => v.clone(),
                })
                .collect(),
        );
        Ok(true)
    }

    async fn statement_done(&mut self, _: u64, _: Option<u64>) -> Result<(), DbError> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transport::DirectConnector;
    use cc_models::database::DbTlsMode;
    use secrecy::SecretString;
    use std::time::Duration;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    const PASSWORD: &str = "canary-engine-password";

    /// A plaintext server on loopback that answers every connection with a
    /// non-TLS reply and records what each client sent.
    async fn plaintext() -> (u16, Arc<std::sync::Mutex<Vec<u8>>>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let seen = Arc::new(std::sync::Mutex::new(Vec::new()));
        let log = seen.clone();
        tokio::spawn(async move {
            while let Ok((mut stream, _)) = listener.accept().await {
                let log = log.clone();
                tokio::spawn(async move {
                    let mut buffer = [0; 4096];
                    while let Ok(Ok(count @ 1..)) =
                        tokio::time::timeout(Duration::from_secs(2), stream.read(&mut buffer)).await
                    {
                        log.lock().unwrap().extend_from_slice(&buffer[..count]);
                        let _ = stream.write_all(b"HTTP/1.1 400 Bad Request\r\n\r\n").await;
                    }
                });
            }
        });
        (port, seen)
    }

    #[tokio::test]
    async fn engines_without_tls_negotiation_never_downgrade_even_on_loopback() {
        for engine in [DbEngine::Clickhouse, DbEngine::Redis, DbEngine::Mongodb] {
            for mode in [DbTlsMode::Prefer, DbTlsMode::Require] {
                let (port, seen) = plaintext().await;
                let mut connection = DbConnection::new("fake", "127.0.0.1", engine);
                connection.port = port;
                connection.username = "cc".into();
                connection.tls.mode = mode;
                connection.limits.connect_timeout_seconds = 2;
                let credentials = DbCredentials {
                    password: Some(SecretString::from(PASSWORD)),
                    client_key_pem: None,
                };
                let opened = tokio::time::timeout(
                    Duration::from_secs(20),
                    DbSession::open(
                        "fake".into(),
                        &connection,
                        &credentials,
                        Arc::new(DirectConnector),
                        false,
                    ),
                )
                .await
                .expect("open finishes");
                assert!(opened.is_err(), "{engine:?} {mode:?}");
                let seen = seen.lock().unwrap();
                // Only TLS handshakes reached the server: no AUTH, header or SASL.
                assert!(!seen.is_empty() && seen[0] == 0x16, "{engine:?} {mode:?}");
                assert!(
                    !seen
                        .windows(PASSWORD.len())
                        .any(|w| w == PASSWORD.as_bytes()),
                    "{engine:?} {mode:?}"
                );
            }
        }
    }
}
