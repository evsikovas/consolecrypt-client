//! One live database session: the native driver, the query guard built from the
//! vault connection, guarded query streams and read-only structure browsing.
//! Production and read-only policy come only from the stored connection.
use crate::{
    guard::{DbGuard, GuardPolicy},
    mysql::Mysql,
    parser::ParsedStatement,
    postgres::Postgres,
    streaming::{DbQueryStream, QuerySink, RowConsumer},
    transport::{DbConnector, DbCredentials},
    DbCell, DbColumn, DbError, DbObjectKind, DbObjectRef, DbParameter, DbQueryRequest,
    DbServerReadOnly, DbSessionInfo, DbTreeNode, DbTreePage, DEFAULT_RESULT_BYTES,
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
}

impl Driver {
    fn version(&self) -> &str {
        match self {
            Self::Postgres(driver) => &driver.version,
            Self::Mysql(driver) => &driver.version,
        }
    }

    /// Runs one guarded statement in the form its engine parsed.
    fn run<'a>(
        &'a mut self,
        statement: &'a ParsedStatement,
        read_only: bool,
        sink: &'a mut (dyn RowConsumer + Send),
        cancel: CancellationToken,
    ) -> BoxFuture<'a, Result<(), DbError>> {
        match statement {
            ParsedStatement::Sql { text, parameters } => {
                self.query(text, parameters, read_only, sink, cancel)
            }
            _ => Box::pin(async { Err(DbError::InvalidRequest) }),
        }
    }

    /// Structure below `parent` for engines without SQL catalogues; `cursor`
    /// continues a paged listing (Redis SCAN).
    fn browse<'a>(
        &'a mut self,
        _parent: Option<&'a DbObjectRef>,
        _cursor: Option<&'a str>,
    ) -> BoxFuture<'a, Result<DbTreePage, DbError>> {
        Box::pin(async {
            Ok(DbTreePage {
                nodes: Vec::new(),
                next_cursor: None,
            })
        })
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
    /// Opens PostgreSQL or MySQL/MariaDB. Other engines are not available yet.
    pub async fn open(
        id: String,
        connection: &DbConnection,
        credentials: &DbCredentials,
        connector: Arc<dyn DbConnector>,
        mobile: bool,
    ) -> Result<Self, DbError> {
        let scope = CancellationToken::new();
        let driver = match connection.engine {
            DbEngine::Postgresql => Driver::Postgres(
                Postgres::connect(connection, credentials, connector, scope.clone()).await?,
            ),
            DbEngine::Mysql | DbEngine::Mariadb => Driver::Mysql(
                Mysql::connect(connection, credentials, connector, scope.clone()).await?,
            ),
            _ => return Err(DbError::InvalidRequest),
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
                production: connection.production,
                mobile,
            },
            scope.clone(),
        );
        let read_only = guard.is_read_only();
        Ok(Self {
            info: DbSessionInfo {
                id,
                connection_id: connection.id.to_string(),
                engine: connection.engine,
                server_version,
                production: connection.production,
                read_only,
                // Both drivers open every guarded read in a server read-only transaction.
                server_read_only: if read_only {
                    DbServerReadOnly::Enforced
                } else {
                    DbServerReadOnly::Unverified
                },
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
            let result = async {
                let mut driver = session.driver.lock().await;
                for statement in &statements {
                    session.guard.validate_permit(statement)?;
                    driver
                        .run(
                            statement.parsed(),
                            statement.is_read_only(),
                            &mut sink,
                            cancel.clone(),
                        )
                        .await?;
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
                    return driver.browse(parent, cursor).await
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
