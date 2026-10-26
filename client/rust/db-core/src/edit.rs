//! Editing cells of a single-table result (non-production, desktop). The grid
//! learns its source table and primary key from the native session; edits
//! become parameterized `UPDATE … WHERE <primary key>` statements that are
//! reviewed like any write and applied in one transaction, each changing
//! exactly one row. The review shows the same statements with the values
//! written out as SQL literals; that text is never executed.
use crate::{
    export::sql_literal,
    introspect::quote_identifier,
    session::{Collect, DbSession, Driver},
    streaming::RowConsumer,
    DbCell, DbColumn, DbError, DbObjectKind, DbObjectRef, DbParameter, DbQueryRequest,
};
use async_trait::async_trait;
use cc_models::database::DbEngine;
use serde::Deserialize;
use sqlparser::{
    ast::{ObjectNamePart, SetExpr, Statement, TableFactor},
    dialect::{Dialect, MySqlDialect, PostgreSqlDialect, SQLiteDialect},
    parser::Parser,
};
use std::collections::BTreeMap;
use zeroize::Zeroize;

/// Most cell edits applied in one review.
pub const MAX_CELL_EDITS: usize = 1000;

/// Longest value written out in a review preview, in characters.
pub const PREVIEW_VALUE_CHARS: usize = 200;

/// Generated edit statements: the parameterized request that runs, the
/// preview the user reviews and the number of rows they change.
#[derive(Debug, Clone)]
pub struct CellEditRequest {
    pub request: DbQueryRequest,
    /// The same statements with every value as an SQL literal (`NULL` for
    /// null, long values shortened with `…`). Display only.
    pub preview: String,
    pub rows: usize,
    /// Cassandra / ScyllaDB: partitions touched. Rows of one partition change
    /// together (one conditional BATCH); different partitions do not.
    pub partitions: usize,
}

/// How a bound value reads in the review preview.
pub(crate) fn preview_literal(engine: DbEngine, cell: &DbCell) -> String {
    let mut shown = cell.clone();
    match &mut shown {
        DbCell::Null | DbCell::Bool(_) => {}
        DbCell::Int(v)
        | DbCell::Uint(v)
        | DbCell::Decimal(v)
        | DbCell::Float(v)
        | DbCell::Text(v)
        | DbCell::Date(v)
        | DbCell::Time(v)
        | DbCell::Timestamp(v)
        | DbCell::Binary(v)
        | DbCell::Json(v) => {
            if v.chars().count() > PREVIEW_VALUE_CHARS {
                let mut short: String = v.chars().take(PREVIEW_VALUE_CHARS).collect();
                short.push('…');
                v.zeroize();
                *v = short;
            }
        }
    }
    let literal = if engine == DbEngine::Cassandra {
        crate::cassandra_value::literal(&shown)
    } else {
        sql_literal(engine, &shown)
    };
    let literal = match (&shown, engine) {
        (DbCell::Binary(_), DbEngine::Postgresql) => format!("decode({literal}, 'base64')"),
        (DbCell::Binary(_), DbEngine::Mysql | DbEngine::Mariadb) => {
            format!("FROM_BASE64({literal})")
        }
        _ => literal,
    };
    shown.zeroize();
    literal
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DbCellEdit {
    /// Primary-key values of the row as it was loaded.
    pub original_primary_key: BTreeMap<String, DbCell>,
    pub column: String,
    pub value: DbCell,
}

/// `schema.table` (PostgreSQL) or `database.table` (MySQL) read by a plain
/// single-table `SELECT` without joins, grouping or set operations.
pub(crate) fn single_table(engine: DbEngine, sql: &str) -> Option<(Option<String>, String)> {
    let dialect: Box<dyn Dialect> = match engine {
        DbEngine::Postgresql => Box::new(PostgreSqlDialect {}),
        DbEngine::Mysql | DbEngine::Mariadb => Box::new(MySqlDialect {}),
        DbEngine::Sqlite => Box::new(SQLiteDialect {}),
        _ => return None,
    };
    let statements = Parser::parse_sql(dialect.as_ref(), sql).ok()?;
    let [Statement::Query(query)] = statements.as_slice() else {
        return None;
    };
    if query.with.is_some() {
        return None;
    }
    let SetExpr::Select(select) = query.body.as_ref() else {
        return None;
    };
    let plain_group = match &select.group_by {
        sqlparser::ast::GroupByExpr::Expressions(expressions, modifiers) => {
            expressions.is_empty() && modifiers.is_empty()
        }
        sqlparser::ast::GroupByExpr::All(_) => false,
    };
    if select.distinct.is_some()
        || select.from.len() != 1
        || !select.from[0].joins.is_empty()
        || !plain_group
        || select.having.is_some()
        || select.into.is_some()
    {
        return None;
    }
    let TableFactor::Table {
        name, args: None, ..
    } = &select.from[0].relation
    else {
        return None;
    };
    let parts: Vec<String> = name
        .0
        .iter()
        .map(|part| match part {
            ObjectNamePart::Identifier(ident) => Some(ident.value.clone()),
            _ => None,
        })
        .collect::<Option<_>>()?;
    match parts.as_slice() {
        [table] => Some((None, table.clone())),
        [schema, table] => Some((Some(schema.clone()), table.clone())),
        _ => None,
    }
}

/// Builds one `UPDATE` per edited row; values and keys travel as parameters.
pub fn cell_edit_request(
    engine: DbEngine,
    table: &DbObjectRef,
    primary_key: &[String],
    edits: &[DbCellEdit],
) -> Result<CellEditRequest, DbError> {
    cell_edit_request_in_partitions(engine, table, primary_key, &[], edits)
}

/// [`cell_edit_request`] for Cassandra / ScyllaDB with the table's partition
/// key: the rows of one partition become one conditional `BEGIN BATCH …
/// APPLY BATCH` (all or none), each partition its own statement.
pub fn cell_edit_request_in_partitions(
    engine: DbEngine,
    table: &DbObjectRef,
    primary_key: &[String],
    partition_key: &[String],
    edits: &[DbCellEdit],
) -> Result<CellEditRequest, DbError> {
    if edits.is_empty() || edits.len() > MAX_CELL_EDITS || primary_key.is_empty() {
        return Err(DbError::InvalidRequest);
    }
    let quote = |name: &str| {
        quote_identifier(
            engine,
            &DbObjectRef {
                database: String::new(),
                schema: None,
                kind: DbObjectKind::Column,
                name: name.into(),
                parent_name: None,
            },
        )
    };
    // Group by the original key so a row gets one statement.
    let mut rows: Vec<(Vec<DbCell>, BTreeMap<String, DbCell>)> = Vec::new();
    for edit in edits {
        let key: Vec<DbCell> = primary_key
            .iter()
            .map(|column| edit.original_primary_key.get(column).cloned())
            .collect::<Option<_>>()
            .ok_or(DbError::InvalidRequest)?;
        if edit.original_primary_key.len() != primary_key.len()
            || key.iter().any(|cell| matches!(cell, DbCell::Null))
            || edit.column.is_empty()
        {
            return Err(DbError::InvalidRequest);
        }
        match rows.iter_mut().find(|(existing, _)| *existing == key) {
            Some((_, changes)) => {
                changes.insert(edit.column.clone(), edit.value.clone());
            }
            None => rows.push((
                key,
                BTreeMap::from([(edit.column.clone(), edit.value.clone())]),
            )),
        }
    }
    let target = quote_identifier(engine, table);
    let mut parameters = Vec::new();
    let mut statements = Vec::new();
    let mut previews = Vec::new();
    // Each value becomes a `{{pN}}` placeholder in the request and a literal
    // in the preview.
    let bind = |value: &DbCell, parameters: &mut Vec<DbParameter>| {
        let name = format!("p{}", parameters.len());
        parameters.push(DbParameter {
            name: name.clone(),
            value: value.clone(),
            type_hint: None,
        });
        (format!("{{{{{name}}}}}"), preview_literal(engine, value))
    };
    for (key, changes) in &rows {
        let mut set = (Vec::new(), Vec::new());
        for (column, value) in changes {
            let (placeholder, literal) = bind(value, &mut parameters);
            set.0.push(format!("{} = {placeholder}", quote(column)));
            set.1.push(format!("{} = {literal}", quote(column)));
        }
        let mut condition = (Vec::new(), Vec::new());
        for (column, value) in primary_key.iter().zip(key) {
            let (placeholder, literal) = bind(value, &mut parameters);
            condition
                .0
                .push(format!("{} = {placeholder}", quote(column)));
            condition.1.push(format!("{} = {literal}", quote(column)));
        }
        // CQL updates are upserts: IF EXISTS keeps an edit of a row that
        // is gone from creating it again.
        let guard = if engine == DbEngine::Cassandra {
            " IF EXISTS"
        } else {
            ""
        };
        statements.push(format!(
            "UPDATE {target} SET {} WHERE {}{guard}",
            set.0.join(", "),
            condition.0.join(" AND ")
        ));
        previews.push(format!(
            "UPDATE {target} SET {} WHERE {}{guard}",
            set.1.join(", "),
            condition.1.join(" AND ")
        ));
    }
    // Rows grouped by partition (first appearance first).
    let positions: Vec<usize> = partition_key
        .iter()
        .filter_map(|column| primary_key.iter().position(|key| key == column))
        .collect();
    let mut groups: Vec<(Vec<DbCell>, Vec<usize>)> = Vec::new();
    for (index, (key, _)) in rows.iter().enumerate() {
        let partition: Vec<DbCell> = if engine == DbEngine::Cassandra
            && !positions.is_empty()
            && positions.len() == partition_key.len()
        {
            positions.iter().map(|&p| key[p].clone()).collect()
        } else {
            // One group per row.
            key.clone()
        };
        match groups
            .iter_mut()
            .find(|(existing, _)| *existing == partition)
        {
            Some((_, members)) => members.push(index),
            None => groups.push((partition, vec![index])),
        }
    }
    let combine = |texts: &[String]| -> Vec<String> {
        groups
            .iter()
            .map(|(_, members)| match members.as_slice() {
                [one] => texts[*one].clone(),
                many => format!(
                    "BEGIN BATCH\n{};\nAPPLY BATCH",
                    many.iter()
                        .map(|&i| texts[i].as_str())
                        .collect::<Vec<_>>()
                        .join(";\n")
                ),
            })
            .collect()
    };
    let (statements, previews) = if engine == DbEngine::Cassandra {
        (combine(&statements), combine(&previews))
    } else {
        (statements, previews)
    };
    Ok(CellEditRequest {
        request: DbQueryRequest {
            text: statements.join(";\n") + ";",
            parameters,
            approval_ticket: None,
        },
        preview: previews.join(";\n") + ";",
        rows: rows.len(),
        partitions: if engine == DbEngine::Cassandra {
            groups.len()
        } else {
            0
        },
    })
}

/// How many rows a generated CQL edit changes: one `UPDATE … IF EXISTS`, or a
/// `BEGIN BATCH … APPLY BATCH` of them (rows of one partition). Anything
/// else is not an edit this module wrote.
fn conditional_updates(text: &str) -> Option<u64> {
    let update = |statement: &str| {
        let statement = statement.trim();
        statement.starts_with("UPDATE ") && statement.ends_with(" IF EXISTS")
    };
    if update(text) {
        return Some(1);
    }
    let inner = text
        .strip_prefix("BEGIN BATCH")?
        .trim_end()
        .strip_suffix("APPLY BATCH")?;
    let statements: Vec<&str> = inner
        .split(';')
        .map(str::trim)
        .filter(|statement| !statement.is_empty())
        .collect();
    (statements.len() > 1 && statements.iter().all(|statement| update(statement)))
        .then_some(statements.len() as u64)
}

/// Rejects a statement that changed anything but exactly one row, which
/// rolls the whole transaction back.
#[derive(Default)]
struct OneRowEach {
    rows: u64,
}

#[async_trait]
impl RowConsumer for OneRowEach {
    async fn start(&mut self, _: Vec<DbColumn>, _: Option<DbObjectRef>) -> Result<(), DbError> {
        Ok(())
    }

    async fn row(&mut self, _: Vec<DbCell>) -> Result<bool, DbError> {
        Ok(true)
    }

    async fn statement_done(&mut self, _: u64, affected: Option<u64>) -> Result<(), DbError> {
        if affected != Some(1) {
            return Err(DbError::QueryFailed);
        }
        self.rows += 1;
        Ok(())
    }
}

impl DbSession {
    /// Columns that identify a row, in key order: the primary key, else the
    /// first unique key whose columns are all NOT NULL (empty when there is
    /// neither; a physical row address such as `ctid` is not stable).
    pub(crate) async fn primary_key(&self, table: &DbObjectRef) -> Result<Vec<String>, DbError> {
        let key = self.key_columns(table, true).await?;
        if !key.is_empty() {
            return Ok(key);
        }
        self.key_columns(table, false).await
    }

    async fn key_columns(
        &self,
        table: &DbObjectRef,
        primary: bool,
    ) -> Result<Vec<String>, DbError> {
        let text = |value: &str| DbParameter {
            name: String::new(),
            value: DbCell::Text(value.into()),
            type_hint: None,
        };
        if self.info.engine == DbEngine::Sqlite {
            return self.sqlite_primary_key(table).await;
        }
        let (sql, bindings) = match self.info.engine {
            DbEngine::Postgresql if primary => (
                "SELECT a.attname FROM pg_catalog.pg_index i \
                 JOIN pg_catalog.pg_class c ON c.oid = i.indrelid \
                 JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace \
                 JOIN LATERAL pg_catalog.unnest(i.indkey) WITH ORDINALITY AS k(attnum, position) ON true \
                 JOIN pg_catalog.pg_attribute a ON a.attrelid = c.oid AND a.attnum = k.attnum \
                 WHERE i.indisprimary AND n.nspname = $1 AND c.relname = $2 AND c.relkind IN ('r', 'p') \
                 ORDER BY k.position",
                vec![text(table.schema.as_deref().unwrap_or("public")), text(&table.name)],
            ),
            DbEngine::Postgresql => (
                "SELECT a.attname FROM pg_catalog.pg_index i \
                 JOIN pg_catalog.pg_class c ON c.oid = i.indrelid \
                 JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace \
                 JOIN LATERAL pg_catalog.unnest(i.indkey) WITH ORDINALITY AS k(attnum, position) ON true \
                 JOIN pg_catalog.pg_attribute a ON a.attrelid = c.oid AND a.attnum = k.attnum \
                 WHERE i.indexrelid = (SELECT u.indexrelid FROM pg_catalog.pg_index u \
                 WHERE u.indrelid = c.oid AND u.indisunique AND u.indisvalid AND u.indpred IS NULL \
                 AND u.indexprs IS NULL AND NOT EXISTS (SELECT 1 FROM pg_catalog.pg_attribute x \
                 WHERE x.attrelid = c.oid AND x.attnum = ANY (u.indkey) AND NOT x.attnotnull) \
                 ORDER BY u.indexrelid LIMIT 1) \
                 AND n.nspname = $1 AND c.relname = $2 AND c.relkind IN ('r', 'p') \
                 ORDER BY k.position",
                vec![text(table.schema.as_deref().unwrap_or("public")), text(&table.name)],
            ),
            DbEngine::Mysql | DbEngine::Mariadb if primary => (
                "SELECT k.column_name FROM information_schema.key_column_usage k \
                 JOIN information_schema.tables t ON t.table_schema = k.table_schema \
                 AND t.table_name = k.table_name AND t.table_type = 'BASE TABLE' \
                 WHERE k.constraint_name = 'PRIMARY' AND k.table_schema = ? AND k.table_name = ? \
                 ORDER BY k.ordinal_position",
                vec![text(&table.database), text(&table.name)],
            ),
            DbEngine::Mysql | DbEngine::Mariadb => (
                "SELECT s.column_name FROM information_schema.statistics s \
                 JOIN information_schema.tables t ON t.table_schema = s.table_schema \
                 AND t.table_name = s.table_name AND t.table_type = 'BASE TABLE' \
                 WHERE s.table_schema = ? AND s.table_name = ? AND s.index_name = \
                 (SELECT u.index_name FROM information_schema.statistics u \
                 WHERE u.table_schema = s.table_schema AND u.table_name = s.table_name AND u.non_unique = 0 \
                 GROUP BY u.index_name HAVING SUM(u.nullable = 'YES') = 0 AND SUM(u.sub_part IS NOT NULL) = 0 \
                 AND SUM(u.column_name IS NULL) = 0 ORDER BY u.index_name LIMIT 1) \
                 ORDER BY s.seq_in_index",
                vec![text(&table.database), text(&table.name)],
            ),
            DbEngine::Cassandra => {
                // Partition key then clustering columns of a table (views
                // cannot be updated).
                let columns = self.cassandra_columns(table).await?;
                return Ok(columns
                    .into_iter()
                    .filter(|column| {
                        matches!(
                            column.kind,
                            crate::cassandra::CqlColumnKind::PartitionKey
                                | crate::cassandra::CqlColumnKind::Clustering { .. }
                        )
                    })
                    .map(|column| column.name)
                    .collect());
            }
            _ => return Ok(Vec::new()),
        };
        let mut rows = Collect::default();
        self.driver
            .lock()
            .await
            .query(sql, &bindings, true, &mut rows, self.scope.child_token())
            .await?;
        Ok(rows
            .rows
            .into_iter()
            .filter_map(|row| row.into_iter().next())
            .collect())
    }

    /// Columns of a CQL table in definition order; empty for a view or a
    /// table that does not exist.
    pub(crate) async fn cassandra_columns(
        &self,
        table: &DbObjectRef,
    ) -> Result<Vec<crate::cassandra::CqlTableColumn>, DbError> {
        let driver = self.driver.lock().await;
        let Driver::Cassandra(driver) = &*driver else {
            return Err(DbError::InvalidRequest);
        };
        let cancel = self.scope.child_token();
        let tables = driver
            .catalogue(
                "SELECT table_name FROM system_schema.tables WHERE keyspace_name = ? AND table_name = ?",
                &[&table.database, &table.name],
                &cancel,
            )
            .await?;
        if tables.is_empty() {
            return Ok(Vec::new());
        }
        driver.columns(&table.database, &table.name, &cancel).await
    }

    /// The table and full primary key (partition and clustering columns) a
    /// plain single-table CQL `SELECT` can edit.
    async fn cassandra_editable(&self, cql: &str) -> Option<(DbObjectRef, Vec<String>)> {
        let shape = crate::parser::cql::select_source(cql)?;
        if !shape.plain {
            return None;
        }
        let keyspace = match shape.keyspace {
            Some(keyspace) => keyspace,
            None => match &*self.driver.lock().await {
                Driver::Cassandra(driver) => driver.keyspace()?,
                _ => return None,
            },
        };
        let table = DbObjectRef {
            database: keyspace,
            schema: None,
            kind: DbObjectKind::Table,
            name: shape.table,
            parent_name: None,
        };
        let key = self.primary_key(&table).await.ok()?;
        (!key.is_empty()).then_some((table, key))
    }

    /// The table and primary key a single-table read can edit, if any.
    pub(crate) async fn editable_source(&self, sql: &str) -> Option<(DbObjectRef, Vec<String>)> {
        // Production results are editable too, but only an open production
        // write window lets the edits through the guard.
        if self.guard.policy().mobile {
            return None;
        }
        if self.info.engine == DbEngine::Cassandra {
            return self.cassandra_editable(sql).await;
        }
        let (qualifier, name) = single_table(self.info.engine, sql)?;
        let default = self.guard.policy().default_schema.clone();
        let table = match self.info.engine {
            DbEngine::Postgresql => DbObjectRef {
                database: String::new(),
                schema: Some(qualifier.unwrap_or(default)),
                kind: DbObjectKind::Table,
                name,
                parent_name: None,
            },
            _ => DbObjectRef {
                database: qualifier.unwrap_or(default),
                schema: None,
                kind: DbObjectKind::Table,
                name,
                parent_name: None,
            },
        };
        let key = self.primary_key(&table).await.ok()?;
        (!key.is_empty()).then_some((table, key))
    }

    /// Reviewable statements for edits of `table`, checked against its real
    /// primary key.
    pub async fn review_cell_edits(
        &self,
        table: &DbObjectRef,
        edits: &[DbCellEdit],
    ) -> Result<CellEditRequest, DbError> {
        if self.guard.is_read_only() || table.kind != DbObjectKind::Table {
            return Err(DbError::PolicyDenied);
        }
        let key = self.primary_key(table).await?;
        if key.is_empty() {
            return Err(DbError::InvalidRequest);
        }
        if self.info.engine == DbEngine::Cassandra {
            // Key columns cannot be updated and counters only incremented.
            let columns = self.cassandra_columns(table).await?;
            let editable = |name: &str| {
                columns.iter().any(|column| {
                    column.name == name
                        && !key.iter().any(|key| key == name)
                        && column.type_name != "counter"
                })
            };
            if edits.iter().any(|edit| !editable(&edit.column)) {
                return Err(DbError::InvalidRequest);
            }
            let partition: Vec<String> = columns
                .iter()
                .filter(|column| column.kind == crate::cassandra::CqlColumnKind::PartitionKey)
                .map(|column| column.name.clone())
                .collect();
            return cell_edit_request_in_partitions(
                self.info.engine,
                table,
                &key,
                &partition,
                edits,
            );
        }
        cell_edit_request(self.info.engine, table, &key, edits)
    }

    /// Applies an approved edit request (generated `UPDATE`, `INSERT` and
    /// `DELETE` statements) in one transaction. Every statement must change
    /// exactly one row, otherwise nothing is kept.
    pub async fn apply_cell_edits(&self, request: &DbQueryRequest) -> Result<u64, DbError> {
        let statements = self.guard.prepare(request)?;
        let mut parsed = Vec::with_capacity(statements.len());
        for statement in &statements {
            self.guard.validate_permit(statement)?;
            match statement.parsed() {
                crate::parser::ParsedStatement::Sql { text, parameters }
                    if ["UPDATE", "INSERT", "DELETE"].iter().any(|verb| {
                        text.trim_start()
                            .get(..verb.len())
                            .is_some_and(|start| start.eq_ignore_ascii_case(verb))
                    }) =>
                {
                    parsed.push((text.as_str(), parameters.as_slice()));
                }
                crate::parser::ParsedStatement::Cql { .. }
                    if self.info.engine == DbEngine::Cassandra => {}
                _ => return Err(DbError::InvalidRequest),
            }
        }
        let mut sink = OneRowEach::default();
        let mut driver = self.driver.lock().await;
        if let Driver::Cassandra(driver) = &*driver {
            // Rows of one partition change together (one conditional batch);
            // partitions one by one, each row only if it still exists.
            let cancel = self.scope.child_token();
            for statement in &statements {
                let crate::parser::ParsedStatement::Cql {
                    text, parameters, ..
                } = statement.parsed()
                else {
                    return Err(DbError::InvalidRequest);
                };
                let updates = conditional_updates(text).ok_or(DbError::InvalidRequest)?;
                if !driver.conditional(text, parameters, &cancel).await? {
                    return Err(DbError::QueryFailed);
                }
                sink.rows += updates;
            }
            return Ok(sink.rows);
        }
        let run = match &mut *driver {
            Driver::Postgres(driver) => {
                driver.query_all(&parsed, false, &mut sink, self.scope.child_token())
            }
            Driver::Mysql(driver) => {
                driver.query_all(&parsed, false, &mut sink, self.scope.child_token())
            }
            Driver::Sqlite(driver) => {
                driver.query_all(&parsed, false, &mut sink, self.scope.child_token())
            }
            Driver::SqliteCli(driver) => {
                driver.query_all(&parsed, false, &mut sink, self.scope.child_token())
            }
            _ => return Err(DbError::InvalidRequest),
        };
        run.await?;
        Ok(sink.rows)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_the_single_source_table() {
        assert_eq!(
            single_table(
                DbEngine::Postgresql,
                "SELECT * FROM \"public\".\"t\" WHERE id > 1 ORDER BY id LIMIT 5"
            ),
            Some((Some("public".into()), "t".into()))
        );
        assert_eq!(
            single_table(DbEngine::Mysql, "SELECT a, b FROM `shop`.`orders`"),
            Some((Some("shop".into()), "orders".into()))
        );
        for sql in [
            "SELECT * FROM a JOIN b ON a.id = b.id",
            "SELECT count(*) FROM a GROUP BY x",
            "SELECT DISTINCT x FROM a",
            "SELECT * FROM a UNION SELECT * FROM b",
            "WITH c AS (SELECT 1) SELECT * FROM c",
            "SELECT * FROM generate_series(1, 3)",
            "SELECT 1",
        ] {
            assert_eq!(single_table(DbEngine::Postgresql, sql), None, "{sql}");
        }
    }

    #[test]
    fn edits_group_by_row_and_bind_every_value() {
        let table = DbObjectRef {
            database: String::new(),
            schema: Some("public".into()),
            kind: DbObjectKind::Table,
            name: "items".into(),
            parent_name: None,
        };
        let key = |id: &str| BTreeMap::from([("id".to_string(), DbCell::Int(id.into()))]);
        let edits = [
            DbCellEdit {
                original_primary_key: key("1"),
                column: "name".into(),
                value: DbCell::Text("x'; DROP".into()),
            },
            DbCellEdit {
                original_primary_key: key("1"),
                column: "note".into(),
                value: DbCell::Null,
            },
            DbCellEdit {
                original_primary_key: key("2"),
                column: "name".into(),
                value: DbCell::Text("y".into()),
            },
        ];
        let edit = cell_edit_request(DbEngine::Postgresql, &table, &["id".into()], &edits).unwrap();
        let request = &edit.request;
        assert_eq!(edit.rows, 2);
        assert_eq!(
            request.text,
            "UPDATE \"public\".\"items\" SET \"name\" = {{p0}}, \"note\" = {{p1}} WHERE \"id\" = {{p2}};\n\
             UPDATE \"public\".\"items\" SET \"name\" = {{p3}} WHERE \"id\" = {{p4}};"
        );
        assert_eq!(request.parameters.len(), 5);
        assert!(!request.text.contains("DROP"));
        // The review shows the values; only the parameterized text runs.
        assert_eq!(
            edit.preview,
            "UPDATE \"public\".\"items\" SET \"name\" = 'x''; DROP', \"note\" = NULL WHERE \"id\" = 1;\n\
             UPDATE \"public\".\"items\" SET \"name\" = 'y' WHERE \"id\" = 2;"
        );
        // A key that does not match the table's primary key is refused.
        let mut wrong = edits[0].clone();
        wrong.original_primary_key =
            BTreeMap::from([("name".to_string(), DbCell::Text("a".into()))]);
        assert!(cell_edit_request(DbEngine::Postgresql, &table, &["id".into()], &[wrong]).is_err());
    }

    #[test]
    fn cassandra_rows_of_one_partition_share_a_conditional_batch() {
        let table = DbObjectRef {
            database: "ks".into(),
            schema: None,
            kind: DbObjectKind::Table,
            name: "t".into(),
            parent_name: None,
        };
        let key = |p: &str, c: &str| {
            BTreeMap::from([
                ("p".to_string(), DbCell::Int(p.into())),
                ("c".to_string(), DbCell::Int(c.into())),
            ])
        };
        let edit = |p: &str, c: &str| DbCellEdit {
            original_primary_key: key(p, c),
            column: "v".into(),
            value: DbCell::Text(format!("{p}/{c}")),
        };
        let edits = [edit("1", "1"), edit("2", "1"), edit("1", "2")];
        let primary = ["p".to_string(), "c".to_string()];
        let request = cell_edit_request_in_partitions(
            DbEngine::Cassandra,
            &table,
            &primary,
            &["p".into()],
            &edits,
        )
        .unwrap();
        assert_eq!((request.rows, request.partitions), (3, 2));
        let statements: Vec<&str> = request.preview.split(";\nAPPLY BATCH;\n").collect();
        assert_eq!(statements.len(), 2, "{}", request.preview);
        assert!(statements[0].starts_with("BEGIN BATCH\nUPDATE "));
        assert!(statements[0].contains("'1/1'") && statements[0].contains("'1/2'"));
        assert!(statements[1].starts_with("UPDATE ") && statements[1].contains("'2/1'"));
        assert_eq!(request.request.text.matches(" IF EXISTS").count(), 3);
        assert_eq!(request.request.text.matches("BEGIN BATCH").count(), 1);
        // The batch is one guarded edit; anything else is not.
        let batch = request.request.text.split(";\nAPPLY BATCH").next().unwrap();
        assert_eq!(
            conditional_updates(&format!("{batch};\nAPPLY BATCH")),
            Some(2)
        );
        assert_eq!(
            conditional_updates("UPDATE t SET v = ? WHERE p = ? IF EXISTS"),
            Some(1)
        );
        for text in [
            "BEGIN BATCH\nUPDATE t SET v = ? WHERE p = ? IF EXISTS;\nAPPLY BATCH",
            "BEGIN BATCH\nUPDATE t SET v = ? WHERE p = ? IF EXISTS;\nDELETE FROM t WHERE p = ?;\nAPPLY BATCH",
            "BEGIN BATCH\nUPDATE t SET v = ? WHERE p = ?;\nUPDATE t SET v = ? WHERE p = ? IF EXISTS;\nAPPLY BATCH",
            "UPDATE t SET v = ? WHERE p = ?",
        ] {
            assert_eq!(conditional_updates(text), None, "{text}");
        }
        // Without the partition key every row stays its own statement.
        let single = cell_edit_request(DbEngine::Cassandra, &table, &primary, &edits).unwrap();
        assert_eq!((single.rows, single.partitions), (3, 3));
        assert!(!single.request.text.contains("BATCH"));
    }

    #[test]
    fn previews_quote_shorten_and_encode_values() {
        let table = DbObjectRef {
            database: "shop".into(),
            schema: None,
            kind: DbObjectKind::Table,
            name: "items".into(),
            parent_name: None,
        };
        let long = "ж".repeat(PREVIEW_VALUE_CHARS + 50);
        let edits = [
            DbCellEdit {
                original_primary_key: BTreeMap::from([(
                    "code".to_string(),
                    DbCell::Text("a\\b".into()),
                )]),
                column: "body".into(),
                value: DbCell::Text(long.clone()),
            },
            DbCellEdit {
                original_primary_key: BTreeMap::from([(
                    "code".to_string(),
                    DbCell::Text("a\\b".into()),
                )]),
                column: "blob".into(),
                value: DbCell::Binary("AP8=".into()),
            },
        ];
        let edit = cell_edit_request(DbEngine::Mysql, &table, &["code".into()], &edits).unwrap();
        let shown = format!("'{}…'", "ж".repeat(PREVIEW_VALUE_CHARS));
        assert_eq!(
            edit.preview,
            format!(
                "UPDATE `shop`.`items` SET `blob` = FROM_BASE64('AP8='), `body` = {shown} WHERE `code` = 'a\\\\b';"
            )
        );
        // The full value still travels as a parameter.
        assert!(edit
            .request
            .parameters
            .iter()
            .any(|p| p.value == DbCell::Text(long.clone())));
        assert!(!edit.request.text.contains('ж'));
    }
}
