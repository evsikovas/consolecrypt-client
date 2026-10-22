//! Read-only introspection beyond the structure tree: object DDL, query plans
//! and editor completion. Catalogue lookups bind names as parameters; the few
//! statements that cannot take parameters (MySQL `SHOW CREATE`) use identifiers
//! quoted by [`quote_identifier`].
use crate::{
    session::{Collect, DbSession, Driver},
    DbCell, DbError, DbObjectKind, DbObjectRef, DbParameter, DbQueryEvent, DbQueryRequest,
};
use cc_models::database::DbEngine;
use serde::Serialize;
use std::{collections::BTreeSet, sync::Arc};

/// Most completion candidates kept per session (tables plus columns).
const MAX_COMPLETION_OBJECTS: usize = 20_000;
/// Most completions returned for one request.
const MAX_COMPLETIONS: usize = 200;

/// Quoted, qualified identifier: PostgreSQL/ClickHouse `"schema"."name"`
/// (ClickHouse uses the database), MySQL `` `db`.`name` ``.
pub fn quote_identifier(engine: DbEngine, object: &DbObjectRef) -> String {
    if engine == DbEngine::Cassandra {
        // CQL: keyspace-qualified, quoted only where a name needs it.
        return if object.kind == DbObjectKind::Database {
            crate::parser::cql::quote_name(&object.name)
        } else {
            crate::parser::cql::qualified(&object.database, &object.name)
        };
    }
    let quote = |name: &str| match engine {
        DbEngine::Mysql | DbEngine::Mariadb => format!("`{}`", name.replace('`', "``")),
        DbEngine::Clickhouse => format!("`{}`", name.replace('\\', "\\\\").replace('`', "\\`")),
        _ => format!("\"{}\"", name.replace('"', "\"\"")),
    };
    let qualifier = match engine {
        DbEngine::Mysql | DbEngine::Mariadb | DbEngine::Clickhouse => {
            (!object.database.is_empty()).then_some(object.database.as_str())
        }
        _ => object.schema.as_deref(),
    };
    match qualifier {
        Some(qualifier) => format!("{}.{}", quote(qualifier), quote(&object.name)),
        None => quote(&object.name),
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct DbPrivilegeReport {
    pub server_read_only: crate::DbServerReadOnly,
    /// None when the engine offers no reliable check.
    pub has_write_privileges: Option<bool>,
    pub warning_codes: Vec<&'static str>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DbCompletion {
    /// Text inserted at the cursor (the part after what was already typed).
    pub insert_text: String,
    pub label: String,
    pub detail: String,
}

/// Names the editor completes from, loaded once per session.
#[derive(Debug, Default)]
pub(crate) struct CompletionCache {
    /// (container, name, detail): tables have the schema/database as container,
    /// columns their table.
    tables: Vec<(String, String, String)>,
    columns: Vec<(String, String, String)>,
}

fn text(value: &str) -> DbParameter {
    DbParameter {
        name: String::new(),
        value: DbCell::Text(value.into()),
        type_hint: None,
    }
}

impl DbSession {
    pub(crate) async fn catalogue(
        &self,
        sql: &str,
        bindings: &[DbParameter],
    ) -> Result<Vec<Vec<String>>, DbError> {
        let mut rows = Collect::default();
        self.driver
            .lock()
            .await
            .query(sql, bindings, true, &mut rows, self.scope.child_token())
            .await?;
        Ok(rows.rows)
    }

    /// Creation statement of a table, view, sequence, function or procedure.
    pub async fn ddl(&self, object: &DbObjectRef) -> Result<String, DbError> {
        match self.info.engine {
            DbEngine::Postgresql => self.postgres_ddl(object).await,
            DbEngine::Mysql | DbEngine::Mariadb => {
                let (statement, column) = match object.kind {
                    DbObjectKind::Table => ("SHOW CREATE TABLE", 1),
                    DbObjectKind::View => ("SHOW CREATE VIEW", 1),
                    DbObjectKind::Procedure => ("SHOW CREATE PROCEDURE", 2),
                    DbObjectKind::Function => ("SHOW CREATE FUNCTION", 2),
                    DbObjectKind::Trigger => ("SHOW CREATE TRIGGER", 2),
                    _ => return Err(DbError::InvalidRequest),
                };
                let sql = format!("{statement} {}", quote_identifier(self.info.engine, object));
                let rows = self.catalogue(&sql, &[]).await?;
                let ddl = rows
                    .first()
                    .and_then(|row| row.get(column))
                    .cloned()
                    .ok_or(DbError::QueryFailed)?;
                Ok(format!("{ddl};"))
            }
            DbEngine::Clickhouse => match &mut *self.driver.lock().await {
                Driver::ClickHouse(driver) => driver.ddl(object).await,
                _ => Err(DbError::InvalidRequest),
            },
            DbEngine::Mongodb => match &mut *self.driver.lock().await {
                Driver::Mongo(driver) => driver.describe(object).await,
                _ => Err(DbError::InvalidRequest),
            },
            DbEngine::Redis => match &mut *self.driver.lock().await {
                Driver::Redis(driver) => driver.describe(object).await,
                _ => Err(DbError::InvalidRequest),
            },
            DbEngine::Cassandra => match &*self.driver.lock().await {
                Driver::Cassandra(driver) => driver.ddl(object, &self.scope.child_token()).await,
                _ => Err(DbError::InvalidRequest),
            },
            DbEngine::Unknown => Err(DbError::InvalidRequest),
        }
    }

    /// The CQL creation statement built from `system_schema`, the fallback
    /// for servers without `DESCRIBE` (exercised by the live tests).
    #[doc(hidden)]
    pub async fn cassandra_schema_ddl(&self, object: &DbObjectRef) -> Result<String, DbError> {
        match &*self.driver.lock().await {
            Driver::Cassandra(driver) => driver.built_ddl(object, &self.scope.child_token()).await,
            _ => Err(DbError::InvalidRequest),
        }
    }

    async fn postgres_ddl(&self, object: &DbObjectRef) -> Result<String, DbError> {
        let schema = object.schema.as_deref().unwrap_or("public");
        let first = |rows: Vec<Vec<String>>| {
            rows.into_iter()
                .next()
                .and_then(|row| row.into_iter().next())
                .filter(|value| !value.is_empty())
                .ok_or(DbError::QueryFailed)
        };
        match object.kind {
            DbObjectKind::View | DbObjectKind::MaterializedView => first(
                self.catalogue(
                    "SELECT 'CREATE ' || CASE c.relkind WHEN 'm' THEN 'MATERIALIZED ' ELSE '' END \
                     || 'VIEW ' || pg_catalog.format('%I.%I', n.nspname, c.relname) || ' AS' || chr(10) \
                     || pg_catalog.pg_get_viewdef(c.oid, true) \
                     FROM pg_catalog.pg_class c JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace \
                     WHERE n.nspname = $1 AND c.relname = $2 AND c.relkind IN ('v', 'm')",
                    &[text(schema), text(&object.name)],
                )
                .await?,
            ),
            DbObjectKind::Sequence => first(
                self.catalogue(
                    "SELECT 'CREATE SEQUENCE ' || pg_catalog.format('%I.%I', schemaname, sequencename) \
                     || ' AS ' || data_type::text || ' INCREMENT BY ' || increment_by \
                     || ' MINVALUE ' || min_value || ' MAXVALUE ' || max_value \
                     || ' START WITH ' || start_value || ' CACHE ' || cache_size \
                     || CASE WHEN cycle THEN ' CYCLE' ELSE ' NO CYCLE' END || ';' \
                     FROM pg_catalog.pg_sequences WHERE schemaname = $1 AND sequencename = $2",
                    &[text(schema), text(&object.name)],
                )
                .await?,
            ),
            DbObjectKind::Function | DbObjectKind::Procedure => {
                let rows = self
                    .catalogue(
                        "SELECT pg_catalog.pg_get_functiondef(p.oid) FROM pg_catalog.pg_proc p \
                         JOIN pg_catalog.pg_namespace n ON n.oid = p.pronamespace \
                         WHERE n.nspname = $1 AND p.proname = $2 AND p.prokind IN ('f', 'p') \
                         ORDER BY p.oid",
                        &[text(schema), text(&object.name)],
                    )
                    .await?;
                let bodies: Vec<String> = rows
                    .into_iter()
                    .filter_map(|row| row.into_iter().next())
                    .map(|body| format!("{};", body.trim_end()))
                    .collect();
                if bodies.is_empty() {
                    return Err(DbError::QueryFailed);
                }
                Ok(bodies.join("\n\n"))
            }
            DbObjectKind::Table => {
                let bindings = [text(schema), text(&object.name)];
                let columns = self
                    .catalogue(
                        "SELECT pg_catalog.format('%I', a.attname), pg_catalog.format_type(a.atttypid, a.atttypmod), \
                         a.attnotnull::text, COALESCE(pg_catalog.pg_get_expr(d.adbin, d.adrelid), ''), \
                         a.attidentity::text, a.attgenerated::text \
                         FROM pg_catalog.pg_attribute a JOIN pg_catalog.pg_class c ON c.oid = a.attrelid \
                         JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace \
                         LEFT JOIN pg_catalog.pg_attrdef d ON d.adrelid = a.attrelid AND d.adnum = a.attnum \
                         WHERE n.nspname = $1 AND c.relname = $2 AND c.relkind IN ('r', 'p', 'f') \
                         AND a.attnum > 0 AND NOT a.attisdropped ORDER BY a.attnum",
                        &bindings,
                    )
                    .await?;
                if columns.is_empty() {
                    return Err(DbError::QueryFailed);
                }
                let constraints = self
                    .catalogue(
                        "SELECT pg_catalog.format('%I', con.conname), pg_catalog.pg_get_constraintdef(con.oid, true) \
                         FROM pg_catalog.pg_constraint con JOIN pg_catalog.pg_class c ON c.oid = con.conrelid \
                         JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace \
                         WHERE n.nspname = $1 AND c.relname = $2 \
                         ORDER BY CASE con.contype WHEN 'p' THEN 0 WHEN 'u' THEN 1 WHEN 'f' THEN 2 ELSE 3 END, con.conname",
                        &bindings,
                    )
                    .await?;
                let extras = self
                    .catalogue(
                        "SELECT pg_catalog.pg_get_indexdef(i.indexrelid) || ';' FROM pg_catalog.pg_index i \
                         JOIN pg_catalog.pg_class c ON c.oid = i.indrelid \
                         JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace \
                         WHERE n.nspname = $1 AND c.relname = $2 AND NOT EXISTS \
                         (SELECT 1 FROM pg_catalog.pg_constraint con WHERE con.conindid = i.indexrelid) \
                         UNION ALL \
                         SELECT pg_catalog.pg_get_triggerdef(t.oid, true) || ';' FROM pg_catalog.pg_trigger t \
                         JOIN pg_catalog.pg_class c ON c.oid = t.tgrelid \
                         JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace \
                         WHERE n.nspname = $1 AND c.relname = $2 AND NOT t.tgisinternal \
                         UNION ALL \
                         SELECT 'COMMENT ON TABLE ' || pg_catalog.format('%I.%I', n.nspname, c.relname) \
                         || ' IS ' || pg_catalog.quote_literal(d.description) || ';' \
                         FROM pg_catalog.pg_description d JOIN pg_catalog.pg_class c ON c.oid = d.objoid \
                         JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace \
                         WHERE n.nspname = $1 AND c.relname = $2 AND d.objsubid = 0 \
                         AND d.classoid = 'pg_catalog.pg_class'::pg_catalog.regclass",
                        &bindings,
                    )
                    .await?;
                Ok(postgres_table_ddl(
                    &quote_identifier(DbEngine::Postgresql, object),
                    &columns,
                    &constraints,
                    &extras,
                ))
            }
            _ => Err(DbError::InvalidRequest),
        }
    }

    /// Plan of one statement. `analyze` executes it and therefore goes through
    /// the same review and approval as a write; production refuses it.
    pub async fn explain(
        self: &Arc<Self>,
        request: &DbQueryRequest,
        analyze: bool,
    ) -> Result<serde_json::Value, DbError> {
        let statement = request.text.trim().trim_end_matches(';').trim_end();
        if statement.is_empty() {
            return Err(DbError::InvalidRequest);
        }
        let (text, json) = match (self.info.engine, analyze) {
            (DbEngine::Postgresql, false) => (format!("EXPLAIN (FORMAT JSON) {statement}"), true),
            (DbEngine::Postgresql, true) => {
                (format!("EXPLAIN (ANALYZE, FORMAT JSON) {statement}"), true)
            }
            (DbEngine::Mysql | DbEngine::Mariadb, false) => {
                (format!("EXPLAIN FORMAT=JSON {statement}"), true)
            }
            (DbEngine::Mysql | DbEngine::Mariadb, true) => {
                (format!("EXPLAIN ANALYZE {statement}"), false)
            }
            (DbEngine::Clickhouse, false) => (format!("EXPLAIN {statement}"), false),
            (DbEngine::Mongodb, false) => {
                // The parsed command keeps its key order inside `explain`.
                let parsed = crate::parser::parse(
                    DbEngine::Mongodb,
                    &self.guard.policy().default_schema,
                    request,
                )?;
                let [statement] = parsed.as_slice() else {
                    return Err(DbError::InvalidRequest);
                };
                let crate::parser::ParsedStatement::Mongo { command, order } = &statement.parsed
                else {
                    return Err(DbError::InvalidRequest);
                };
                (
                    format!(
                        "{{\"explain\":{},\"verbosity\":\"queryPlanner\"}}",
                        crate::parser::ordered_json(command, order)
                    ),
                    true,
                )
            }
            _ => return Err(DbError::InvalidRequest),
        };
        let mut explain = DbQueryRequest {
            text,
            parameters: request.parameters.clone(),
            approval_ticket: None,
        };
        if analyze {
            let review = self.guard.review(&explain)?;
            self.guard.approve(&review.ticket)?;
            explain.approval_ticket = Some(review.ticket.clone());
        }
        let mut stream = self.execute(&explain)?;
        let mut output = String::new();
        // MongoDB rows carry the whole reply in `_doc`; SQL plans are one cell.
        let mut only: Option<usize> = None;
        while let Some(event) = stream.events.recv().await {
            match event {
                DbQueryEvent::Started { columns, .. } => {
                    only = columns.iter().position(|column| column.name == "_doc");
                }
                DbQueryEvent::Rows { rows, .. } => {
                    for row in rows {
                        for (index, cell) in row.into_iter().enumerate() {
                            if only.is_some_and(|only| only != index) {
                                continue;
                            }
                            match &cell {
                                DbCell::Json(value) | DbCell::Text(value) => {
                                    if !output.is_empty() {
                                        output.push('\n');
                                    }
                                    output.push_str(value);
                                }
                                _ => {}
                            }
                        }
                    }
                }
                DbQueryEvent::Error { code, .. } => {
                    return Err(match code.as_str() {
                        "policy_denied" => DbError::PolicyDenied,
                        "cancelled" => DbError::Cancelled,
                        _ => DbError::QueryFailed,
                    })
                }
                DbQueryEvent::Cancelled => return Err(DbError::Cancelled),
                DbQueryEvent::Done => break,
                _ => {}
            }
        }
        let plan = if json {
            serde_json::from_str(&output).map_err(|_| DbError::QueryFailed)?
        } else {
            serde_json::Value::String(output)
        };
        Ok(serde_json::json!({
            "engine": self.info.engine,
            "analyzed": analyze,
            "plan": plan,
        }))
    }

    /// Keywords, tables and columns that continue the word before `cursor`.
    pub async fn complete(&self, text: &str, cursor: usize) -> Result<Vec<DbCompletion>, DbError> {
        if !matches!(
            self.info.engine,
            DbEngine::Postgresql
                | DbEngine::Mysql
                | DbEngine::Mariadb
                | DbEngine::Clickhouse
                | DbEngine::Cassandra
        ) {
            return Ok(Vec::new());
        }
        let before = text
            .get(..cursor.min(text.len()))
            .ok_or(DbError::InvalidRequest)?;
        let (qualifier, prefix) = completion_word(before);
        let mut cache = self.completion.lock().await;
        if cache.is_none() {
            *cache = Some(self.load_completion().await?);
        }
        let cache = cache.as_ref().expect("loaded above");
        let keywords = if self.info.engine == DbEngine::Cassandra {
            CQL_KEYWORDS
        } else {
            KEYWORDS
        };
        Ok(complete_from(
            cache,
            keywords,
            text,
            qualifier.as_deref(),
            &prefix,
        ))
    }

    async fn load_completion(&self) -> Result<CompletionCache, DbError> {
        if self.info.engine == DbEngine::Cassandra {
            return self.cassandra_completion().await;
        }
        let (tables, columns) = match self.info.engine {
            DbEngine::Postgresql => (
                "SELECT n.nspname, c.relname, CASE c.relkind WHEN 'v' THEN 'view' WHEN 'm' THEN 'materialized view' ELSE 'table' END \
                 FROM pg_catalog.pg_class c JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace \
                 WHERE c.relkind IN ('r', 'p', 'v', 'm', 'f') AND n.nspname NOT LIKE 'pg\\_%' \
                 AND n.nspname <> 'information_schema' ORDER BY 1, 2 LIMIT 5000",
                "SELECT c.relname, a.attname, pg_catalog.format_type(a.atttypid, a.atttypmod) \
                 FROM pg_catalog.pg_attribute a JOIN pg_catalog.pg_class c ON c.oid = a.attrelid \
                 JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace \
                 WHERE c.relkind IN ('r', 'p', 'v', 'm', 'f') AND a.attnum > 0 AND NOT a.attisdropped \
                 AND n.nspname NOT LIKE 'pg\\_%' AND n.nspname <> 'information_schema' \
                 ORDER BY 1, a.attnum LIMIT 15000",
            ),
            DbEngine::Mysql | DbEngine::Mariadb => (
                "SELECT table_schema, table_name, LOWER(table_type) FROM information_schema.tables \
                 WHERE table_schema NOT IN ('mysql', 'sys', 'performance_schema', 'information_schema') \
                 ORDER BY 1, 2 LIMIT 5000",
                "SELECT table_name, column_name, column_type FROM information_schema.columns \
                 WHERE table_schema NOT IN ('mysql', 'sys', 'performance_schema', 'information_schema') \
                 ORDER BY table_schema, table_name, ordinal_position LIMIT 15000",
            ),
            DbEngine::Clickhouse => (
                "SELECT database, name, engine FROM system.tables \
                 WHERE database NOT IN ('system', 'INFORMATION_SCHEMA', 'information_schema') \
                 ORDER BY 1, 2 LIMIT 5000",
                "SELECT table, name, type FROM system.columns \
                 WHERE database NOT IN ('system', 'INFORMATION_SCHEMA', 'information_schema') \
                 ORDER BY database, table, position LIMIT 15000",
            ),
            _ => return Ok(CompletionCache::default()),
        };
        let triple = |rows: Vec<Vec<String>>| {
            rows.into_iter()
                .filter_map(|row| match row.as_slice() {
                    [a, b, c, ..] => Some((a.clone(), b.clone(), c.clone())),
                    _ => None,
                })
                .take(MAX_COMPLETION_OBJECTS)
                .collect::<Vec<_>>()
        };
        Ok(CompletionCache {
            tables: triple(self.catalogue(tables, &[]).await?),
            columns: triple(self.catalogue(columns, &[]).await?),
        })
    }

    /// Whether the signed-in role can change data, for the "check privileges"
    /// action: production connections should use a read-only role.
    pub async fn privileges(&self) -> Result<DbPrivilegeReport, DbError> {
        let writable = match self.info.engine {
            DbEngine::Postgresql => {
                let rows = self
                    .catalogue(
                        "SELECT (r.rolsuper OR r.rolcreatedb OR r.rolcreaterole \
                         OR pg_catalog.has_database_privilege(pg_catalog.current_database(), 'CREATE') \
                         OR EXISTS (SELECT 1 FROM pg_catalog.pg_roles w WHERE w.rolname = 'pg_write_all_data' \
                           AND pg_catalog.pg_has_role(w.oid, 'MEMBER')) \
                         OR EXISTS (SELECT 1 FROM information_schema.role_table_grants g \
                           WHERE g.privilege_type IN ('INSERT', 'UPDATE', 'DELETE', 'TRUNCATE')) \
                         OR EXISTS (SELECT 1 FROM pg_catalog.pg_class c JOIN pg_catalog.pg_namespace n \
                           ON n.oid = c.relnamespace WHERE c.relkind IN ('r', 'p') \
                           AND n.nspname NOT LIKE 'pg\\_%' AND n.nspname <> 'information_schema' \
                           AND pg_catalog.pg_has_role(c.relowner, 'USAGE')))::text \
                         FROM pg_catalog.pg_roles r WHERE r.rolname = current_user",
                        &[],
                    )
                    .await?;
                rows.first()
                    .and_then(|row| row.first())
                    .map(|value| value == "true")
            }
            DbEngine::Mysql | DbEngine::Mariadb => {
                let rows = self
                    .catalogue("SHOW GRANTS FOR CURRENT_USER()", &[])
                    .await?;
                Some(rows.iter().filter_map(|row| row.first()).any(|grant| {
                    let grant = grant.to_ascii_uppercase();
                    let privileges = grant
                        .strip_prefix("GRANT ")
                        .and_then(|rest| rest.split(" ON ").next())
                        .unwrap_or("");
                    privileges.split(',').map(str::trim).any(|privilege| {
                        matches!(
                            privilege,
                            "ALL PRIVILEGES"
                                | "INSERT"
                                | "UPDATE"
                                | "DELETE"
                                | "CREATE"
                                | "DROP"
                                | "ALTER"
                                | "INDEX"
                                | "CREATE VIEW"
                                | "TRIGGER"
                                | "EVENT"
                                | "CREATE ROUTINE"
                                | "ALTER ROUTINE"
                                | "FILE"
                                | "SUPER"
                        )
                    })
                }))
            }
            _ => None,
        };
        let mut warning_codes = Vec::new();
        if self.info.production && writable == Some(true) {
            warning_codes.push("production_role_can_write");
        }
        if writable.is_none() {
            warning_codes.push("privileges_unverified");
        }
        Ok(DbPrivilegeReport {
            server_read_only: self.info.server_read_only.clone(),
            has_write_privileges: writable,
            warning_codes,
        })
    }

    /// Editor text that shows the first `limit` rows, entries or keys of an
    /// object picked in the structure tree.
    pub async fn object_query(&self, object: &DbObjectRef, limit: u32) -> Result<String, DbError> {
        let limit = limit.clamp(1, 10_000);
        match (self.info.engine, object.kind) {
            (
                DbEngine::Postgresql | DbEngine::Mysql | DbEngine::Mariadb | DbEngine::Clickhouse,
                DbObjectKind::Table | DbObjectKind::View | DbObjectKind::MaterializedView,
            ) => Ok(format!(
                "SELECT * FROM {} LIMIT {limit};",
                quote_identifier(self.info.engine, object)
            )),
            (DbEngine::Redis, DbObjectKind::RedisKey) => {
                let key = crate::parser::redis_quote(object.name.as_bytes());
                let mut kind = Collect::default();
                if let Driver::Redis(driver) = &mut *self.driver.lock().await {
                    driver
                        .command(
                            &[b"TYPE".to_vec(), object.name.as_bytes().to_vec()],
                            &mut kind,
                            self.scope.child_token(),
                        )
                        .await?;
                }
                let kind = kind
                    .rows
                    .first()
                    .and_then(|row| row.first())
                    .cloned()
                    .unwrap_or_default();
                let last = limit - 1;
                Ok(match kind.as_str() {
                    "hash" => format!("HSCAN {key} 0 COUNT {limit}"),
                    "list" => format!("LRANGE {key} 0 {last}"),
                    "set" => format!("SSCAN {key} 0 COUNT {limit}"),
                    "zset" => format!("ZRANGE {key} 0 {last} WITHSCORES"),
                    "stream" => format!("XRANGE {key} - + COUNT {limit}"),
                    _ => format!("GET {key}"),
                })
            }
            (DbEngine::Cassandra, DbObjectKind::Table | DbObjectKind::MaterializedView) => {
                Ok(format!(
                    "SELECT * FROM {} LIMIT {limit};",
                    quote_identifier(self.info.engine, object)
                ))
            }
            (DbEngine::Mongodb, DbObjectKind::Collection) => Ok(format!(
                "db.getCollection({}).find({{}}).limit({limit})",
                serde_json::Value::String(object.name.clone())
            )),
            (DbEngine::Redis, DbObjectKind::RedisDatabase) => {
                // A folder is its prefix; a database lists everything.
                let pattern = if object.parent_name.is_some() {
                    format!("{}*", glob_escape(&object.name))
                } else {
                    "*".into()
                };
                Ok(format!(
                    "SCAN 0 MATCH {} COUNT {limit}",
                    crate::parser::redis_quote(pattern.as_bytes())
                ))
            }
            _ => Err(DbError::InvalidRequest),
        }
    }

    /// Sampled field types and presence of a MongoDB collection (`sample`
    /// documents, at most 1000), or its description without sampling. Other
    /// engines answer with grouped catalogue facts
    /// ([`crate::details::ObjectDetails`]).
    pub async fn inspect(
        &self,
        object: &DbObjectRef,
        sample: Option<u32>,
    ) -> Result<serde_json::Value, DbError> {
        if self.info.engine != DbEngine::Mongodb {
            if sample.is_some() {
                return Err(DbError::InvalidRequest);
            }
            return serde_json::to_value(self.object_details(object).await?)
                .map_err(|_| DbError::QueryFailed);
        }
        match &mut *self.driver.lock().await {
            Driver::Mongo(driver) => match sample {
                Some(sample) => driver.inspect(object, sample.clamp(1, 1000)).await,
                None => driver.describe(object).await.map(|text| {
                    serde_json::from_str(&text).unwrap_or(serde_json::Value::String(text))
                }),
            },
            _ => Err(DbError::InvalidRequest),
        }
    }

    /// Compact schema for an assistant: up to `limit` tables, those named in
    /// `hints` first, each with its columns and types. No data.
    pub async fn schema_context(&self, hints: &str, limit: usize) -> Result<Vec<String>, DbError> {
        let mut cache = self.completion.lock().await;
        if cache.is_none() {
            *cache = Some(self.load_completion().await?);
        }
        let cache = cache.as_ref().expect("loaded above");
        let hints = hints.to_lowercase();
        let mut tables: Vec<&(String, String, String)> = cache.tables.iter().collect();
        // Mentioned tables first, then the rest in catalogue order.
        tables.sort_by_key(|(_, table, _)| !hints.contains(&table.to_lowercase()));
        Ok(tables
            .into_iter()
            .take(limit.clamp(1, 100))
            .map(|(container, table, kind)| {
                let columns: Vec<String> = cache
                    .columns
                    .iter()
                    .filter(|(owner, _, _)| owner == table)
                    .take(200)
                    .map(|(_, column, kind)| format!("{column} {kind}"))
                    .collect();
                format!("{kind} {container}.{table} ({})", columns.join(", "))
            })
            .collect())
    }

    /// Table names matching `query` (case-insensitive substring).
    pub async fn find_tables(&self, query: &str, limit: usize) -> Result<Vec<String>, DbError> {
        let mut cache = self.completion.lock().await;
        if cache.is_none() {
            *cache = Some(self.load_completion().await?);
        }
        let query = query.to_lowercase();
        Ok(cache
            .as_ref()
            .expect("loaded above")
            .tables
            .iter()
            .filter(|(container, table, _)| {
                table.to_lowercase().contains(&query) || container.to_lowercase().contains(&query)
            })
            .take(limit.clamp(1, 200))
            .map(|(container, table, _)| format!("{container}.{table}"))
            .collect())
    }

    /// Runs a read-only request and keeps at most `limit` rows. Anything the
    /// guard does not classify as a read is refused, even on development.
    pub async fn read_rows(
        self: &Arc<Self>,
        request: &DbQueryRequest,
        limit: usize,
    ) -> Result<(Vec<crate::DbColumn>, Vec<Vec<DbCell>>), DbError> {
        let review = self.guard.review(request)?;
        if review.risk != crate::guard::QueryRisk::ReadOnly || review.statement_count != 1 {
            return Err(DbError::PolicyDenied);
        }
        let mut stream = self.execute(request)?;
        let (mut columns, mut rows) = (Vec::new(), Vec::new());
        while let Some(event) = stream.events.recv().await {
            match event {
                DbQueryEvent::Started {
                    columns: started, ..
                } if columns.is_empty() => columns = started,
                DbQueryEvent::Rows { rows: batch, .. } => {
                    rows.extend(batch.into_iter().take(limit - rows.len()));
                    if rows.len() >= limit {
                        stream.cancel();
                        break;
                    }
                }
                DbQueryEvent::Paused { .. } => {
                    stream.cancel();
                    break;
                }
                DbQueryEvent::Error { .. } => return Err(DbError::QueryFailed),
                DbQueryEvent::Done | DbQueryEvent::Cancelled => break,
                _ => {}
            }
        }
        Ok((columns, rows))
    }

    /// Forgets cached completion names (after DDL or an explicit refresh).
    pub async fn refresh_completion(&self) {
        *self.completion.lock().await = None;
    }
}

/// Escapes Redis glob characters in a literal key prefix.
fn glob_escape(prefix: &str) -> String {
    let mut out = String::with_capacity(prefix.len());
    for c in prefix.chars() {
        if matches!(c, '*' | '?' | '[' | ']' | '\\') {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

fn postgres_table_ddl(
    name: &str,
    columns: &[Vec<String>],
    constraints: &[Vec<String>],
    extras: &[Vec<String>],
) -> String {
    let mut lines: Vec<String> = columns
        .iter()
        .map(|row| {
            let mut line = format!("    {} {}", row[0], row[1]);
            match row.get(5).map(String::as_str) {
                Some("s") => line.push_str(&format!(" GENERATED ALWAYS AS ({}) STORED", row[3])),
                _ => match row.get(4).map(String::as_str) {
                    Some("a") => line.push_str(" GENERATED ALWAYS AS IDENTITY"),
                    Some("d") => line.push_str(" GENERATED BY DEFAULT AS IDENTITY"),
                    _ if !row[3].is_empty() => line.push_str(&format!(" DEFAULT {}", row[3])),
                    _ => {}
                },
            }
            if row[2] == "true" {
                line.push_str(" NOT NULL");
            }
            line
        })
        .collect();
    lines.extend(
        constraints
            .iter()
            .map(|row| format!("    CONSTRAINT {} {}", row[0], row[1])),
    );
    let mut ddl = format!("CREATE TABLE {name} (\n{}\n);", lines.join(",\n"));
    for row in extras {
        if let Some(statement) = row.first().filter(|value| !value.is_empty()) {
            ddl.push('\n');
            ddl.push_str(statement);
        }
    }
    ddl
}

/// The word being typed before the cursor and its `qualifier.` if any.
fn completion_word(before: &str) -> (Option<String>, String) {
    let is_word = |c: char| c.is_alphanumeric() || c == '_' || c == '$';
    let start = before
        .char_indices()
        .rev()
        .find(|(_, c)| !is_word(*c))
        .map_or(0, |(index, c)| index + c.len_utf8());
    let prefix = before[start..].to_owned();
    let qualifier = before[..start].strip_suffix('.').map(|head| {
        let begin = head
            .char_indices()
            .rev()
            .find(|(_, c)| !(is_word(*c) || *c == '"' || *c == '`'))
            .map_or(0, |(index, c)| index + c.len_utf8());
        head[begin..]
            .trim_matches(|c| c == '"' || c == '`')
            .to_owned()
    });
    (qualifier, prefix)
}

const KEYWORDS: &[&str] = &[
    "SELECT",
    "FROM",
    "WHERE",
    "GROUP BY",
    "ORDER BY",
    "HAVING",
    "LIMIT",
    "OFFSET",
    "JOIN",
    "LEFT JOIN",
    "INNER JOIN",
    "ON",
    "AND",
    "OR",
    "NOT",
    "IN",
    "EXISTS",
    "BETWEEN",
    "LIKE",
    "ILIKE",
    "IS NULL",
    "IS NOT NULL",
    "AS",
    "DISTINCT",
    "COUNT",
    "SUM",
    "AVG",
    "MIN",
    "MAX",
    "CASE",
    "WHEN",
    "THEN",
    "ELSE",
    "END",
    "UNION",
    "UNION ALL",
    "WITH",
    "INSERT INTO",
    "VALUES",
    "UPDATE",
    "SET",
    "DELETE FROM",
    "EXPLAIN",
    "DESC",
    "ASC",
    "COALESCE",
    "CAST",
    "NULL",
    "TRUE",
    "FALSE",
];

/// CQL statements and clauses offered by completion.
const CQL_KEYWORDS: &[&str] = &[
    "SELECT",
    "FROM",
    "WHERE",
    "AND",
    "IN",
    "CONTAINS",
    "CONTAINS KEY",
    "ORDER BY",
    "GROUP BY",
    "PER PARTITION LIMIT",
    "LIMIT",
    "ALLOW FILTERING",
    "DISTINCT",
    "JSON",
    "TOKEN",
    "WRITETIME",
    "TTL",
    "COUNT",
    "INSERT INTO",
    "VALUES",
    "IF NOT EXISTS",
    "IF EXISTS",
    "USING TTL",
    "USING TIMESTAMP",
    "UPDATE",
    "SET",
    "DELETE FROM",
    "BEGIN BATCH",
    "BEGIN UNLOGGED BATCH",
    "APPLY BATCH",
    "TRUNCATE",
    "CREATE TABLE",
    "CREATE KEYSPACE",
    "CREATE INDEX",
    "CREATE MATERIALIZED VIEW",
    "CREATE TYPE",
    "ALTER TABLE",
    "DROP TABLE",
    "PRIMARY KEY",
    "CLUSTERING ORDER BY",
    "WITH",
    "USE",
    "DESCRIBE",
    "ASC",
    "DESC",
    "NULL",
    "TRUE",
    "FALSE",
    "frozen",
    "list",
    "set",
    "map",
    "tuple",
    "text",
    "int",
    "bigint",
    "uuid",
    "timeuuid",
    "timestamp",
    "boolean",
    "decimal",
    "double",
    "blob",
    "counter",
    "now()",
    "toTimestamp",
    "toJson",
    "fromJson",
];

impl DbSession {
    /// Tables and views of every application keyspace with their columns.
    async fn cassandra_completion(&self) -> Result<CompletionCache, DbError> {
        let driver = self.driver.lock().await;
        let Driver::Cassandra(driver) = &*driver else {
            return Err(DbError::InvalidRequest);
        };
        let cancel = self.scope.child_token();
        let user = |keyspace: &str| !keyspace.starts_with("system");
        let mut tables = Vec::new();
        for (cql, kind) in [
            (
                "SELECT keyspace_name, table_name FROM system_schema.tables",
                "table",
            ),
            (
                "SELECT keyspace_name, view_name FROM system_schema.views",
                "materialized view",
            ),
        ] {
            for row in driver.catalogue(cql, &[], &cancel).await? {
                if user(&row[0]) && tables.len() < MAX_COMPLETION_OBJECTS {
                    tables.push((row[0].clone(), row[1].clone(), kind.to_owned()));
                }
            }
        }
        let columns = driver
            .catalogue(
                "SELECT keyspace_name, table_name, column_name, type FROM system_schema.columns",
                &[],
                &cancel,
            )
            .await?
            .into_iter()
            .filter(|row| user(&row[0]))
            .map(|row| (row[1].clone(), row[2].clone(), row[3].clone()))
            .take(MAX_COMPLETION_OBJECTS)
            .collect();
        Ok(CompletionCache { tables, columns })
    }
}

/// Table of an alias declared anywhere in the statement (`table alias`,
/// `table AS alias`).
fn alias_table(before: &str, alias: &str) -> Option<String> {
    let words: Vec<&str> = before
        .split(|c: char| c.is_whitespace() || c == ',' || c == '(' || c == ')')
        .filter(|word| !word.is_empty())
        .collect();
    words
        .windows(2)
        .rev()
        .find_map(|pair| {
            let (table, name) = (pair[0], pair[1]);
            (name
                .trim_matches(|c| c == '"' || c == '`')
                .eq_ignore_ascii_case(alias)
                && !table.eq_ignore_ascii_case("as"))
            .then(|| {
                table
                    .rsplit('.')
                    .next()
                    .unwrap_or(table)
                    .trim_matches(|c| c == '"' || c == '`')
                    .to_owned()
            })
        })
        .or_else(|| {
            words.windows(3).rev().find_map(|triple| {
                (triple[1].eq_ignore_ascii_case("as")
                    && triple[2]
                        .trim_matches(|c| c == '"' || c == '`')
                        .eq_ignore_ascii_case(alias))
                .then(|| {
                    triple[0]
                        .rsplit('.')
                        .next()
                        .unwrap_or(triple[0])
                        .trim_matches(|c| c == '"' || c == '`')
                        .to_owned()
                })
            })
        })
}

fn complete_from(
    cache: &CompletionCache,
    keywords: &[&str],
    statement: &str,
    qualifier: Option<&str>,
    prefix: &str,
) -> Vec<DbCompletion> {
    let lower = prefix.to_lowercase();
    let matches = |name: &str| {
        name.to_lowercase().starts_with(&lower) && name.len() > prefix.len() || prefix.is_empty()
    };
    let completion = |name: &str, detail: String| DbCompletion {
        insert_text: name.chars().skip(prefix.chars().count()).collect(),
        label: name.to_owned(),
        detail,
    };
    let mut seen = BTreeSet::new();
    let mut out = Vec::new();
    if let Some(qualifier) = qualifier {
        // `schema.` lists its tables; `table.` or `alias.` lists the columns.
        for (schema, table, kind) in &cache.tables {
            if schema.eq_ignore_ascii_case(qualifier)
                && matches(table)
                && seen.insert(table.clone())
            {
                out.push(completion(table, kind.clone()));
            }
        }
        let table = cache
            .tables
            .iter()
            .any(|(_, table, _)| table.eq_ignore_ascii_case(qualifier))
            .then(|| qualifier.to_owned())
            .or_else(|| alias_table(statement, qualifier));
        if let Some(table) = table {
            for (owner, column, kind) in &cache.columns {
                if owner.eq_ignore_ascii_case(&table)
                    && matches(column)
                    && seen.insert(column.clone())
                {
                    out.push(completion(column, kind.clone()));
                }
            }
        }
        out.truncate(MAX_COMPLETIONS);
        return out;
    }
    if prefix.is_empty() {
        return out;
    }
    for keyword in keywords {
        if keyword.to_lowercase().starts_with(&lower) && keyword.len() > prefix.len() {
            let keyword = if prefix.chars().all(|c| !c.is_uppercase()) {
                keyword.to_lowercase()
            } else {
                (*keyword).to_owned()
            };
            if seen.insert(keyword.clone()) {
                out.push(completion(&keyword, String::new()));
            }
        }
    }
    for (schema, table, kind) in &cache.tables {
        if matches(table) && seen.insert(table.clone()) {
            out.push(completion(table, format!("{kind} · {schema}")));
        }
    }
    for (table, column, kind) in &cache.columns {
        if matches(column) && seen.insert(column.clone()) {
            out.push(completion(column, format!("{kind} · {table}")));
        }
    }
    out.truncate(MAX_COMPLETIONS);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cache() -> CompletionCache {
        CompletionCache {
            tables: vec![
                ("public".into(), "orders".into(), "table".into()),
                ("public".into(), "order_items".into(), "table".into()),
                ("sales".into(), "customers".into(), "table".into()),
            ],
            columns: vec![
                ("orders".into(), "id".into(), "integer".into()),
                ("orders".into(), "ordered_at".into(), "timestamp".into()),
                ("customers".into(), "name".into(), "text".into()),
            ],
        }
    }

    #[test]
    fn words_and_qualifiers_are_split_at_the_cursor() {
        assert_eq!(completion_word("SELECT * FROM ord"), (None, "ord".into()));
        assert_eq!(
            completion_word("SELECT o.ord"),
            (Some("o".into()), "ord".into())
        );
        assert_eq!(
            completion_word("SELECT \"sales\"."),
            (Some("sales".into()), String::new())
        );
    }

    #[test]
    fn completes_keywords_tables_and_columns() {
        let labels =
            |items: Vec<DbCompletion>| items.into_iter().map(|c| c.label).collect::<Vec<_>>();
        let found = complete_from(&cache(), KEYWORDS, "SELECT * FROM ord", None, "ord");
        assert_eq!(
            labels(found.clone()),
            ["order by", "orders", "order_items", "ordered_at"]
        );
        assert_eq!(found[1].insert_text, "ers");
        assert_eq!(
            labels(complete_from(
                &cache(),
                KEYWORDS,
                "SELECT * FROM sales.",
                Some("sales"),
                ""
            )),
            ["customers"]
        );
        let before = "SELECT o. FROM orders o";
        assert_eq!(
            labels(complete_from(&cache(), KEYWORDS, before, Some("o"), "")),
            ["id", "ordered_at"]
        );
        assert_eq!(
            labels(complete_from(
                &cache(),
                KEYWORDS,
                "SELECT c.n FROM customers AS c",
                Some("c"),
                "n"
            )),
            ["name"]
        );
    }

    #[test]
    fn identifiers_are_quoted_per_engine() {
        let table = DbObjectRef {
            database: "shop".into(),
            schema: Some("sales".into()),
            kind: DbObjectKind::Table,
            name: "or\"de`rs".into(),
            parent_name: None,
        };
        assert_eq!(
            quote_identifier(DbEngine::Postgresql, &table),
            "\"sales\".\"or\"\"de`rs\""
        );
        assert_eq!(
            quote_identifier(DbEngine::Mysql, &table),
            "`shop`.`or\"de``rs`"
        );
        assert_eq!(
            quote_identifier(DbEngine::Cassandra, &table),
            "shop.\"or\"\"de`rs\""
        );
        assert_eq!(
            quote_identifier(DbEngine::Clickhouse, &table),
            "`shop`.`or\"de\\`rs`"
        );
    }

    #[test]
    fn postgres_table_ddl_is_assembled_in_order() {
        let ddl = postgres_table_ddl(
            "\"public\".\"t\"",
            &[
                vec![
                    "id".into(),
                    "integer".into(),
                    "true".into(),
                    String::new(),
                    "a".into(),
                    String::new(),
                ],
                vec![
                    "name".into(),
                    "text".into(),
                    "false".into(),
                    "'x'::text".into(),
                    String::new(),
                    String::new(),
                ],
            ],
            &[vec!["t_pkey".into(), "PRIMARY KEY (id)".into()]],
            &[vec![
                "CREATE INDEX t_name ON public.t USING btree (name);".into()
            ]],
        );
        assert_eq!(
            ddl,
            "CREATE TABLE \"public\".\"t\" (\n    id integer GENERATED ALWAYS AS IDENTITY NOT NULL,\n    name text DEFAULT 'x'::text,\n    CONSTRAINT t_pkey PRIMARY KEY (id)\n);\nCREATE INDEX t_name ON public.t USING btree (name);"
        );
    }
}
