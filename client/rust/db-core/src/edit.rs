//! Editing cells of a single-table result (non-production, desktop). The grid
//! learns its source table and primary key from the native session; edits
//! become parameterized `UPDATE … WHERE <primary key>` statements that are
//! reviewed like any write and applied in one transaction, each changing
//! exactly one row.
use crate::{
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
    dialect::{Dialect, MySqlDialect, PostgreSqlDialect},
    parser::Parser,
};
use std::collections::BTreeMap;

/// Most cell edits applied in one review.
pub const MAX_CELL_EDITS: usize = 1000;

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
) -> Result<(DbQueryRequest, usize), DbError> {
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
    let bind = |value: &DbCell, parameters: &mut Vec<DbParameter>| {
        let name = format!("p{}", parameters.len());
        parameters.push(DbParameter {
            name: name.clone(),
            value: value.clone(),
            type_hint: None,
        });
        format!("{{{{{name}}}}}")
    };
    for (key, changes) in &rows {
        let set: Vec<String> = changes
            .iter()
            .map(|(column, value)| format!("{} = {}", quote(column), bind(value, &mut parameters)))
            .collect();
        let condition: Vec<String> = primary_key
            .iter()
            .zip(key)
            .map(|(column, value)| format!("{} = {}", quote(column), bind(value, &mut parameters)))
            .collect();
        statements.push(format!(
            "UPDATE {target} SET {} WHERE {}",
            set.join(", "),
            condition.join(" AND ")
        ));
    }
    Ok((
        DbQueryRequest {
            text: statements.join(";\n") + ";",
            parameters,
            approval_ticket: None,
        },
        rows.len(),
    ))
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
    /// Primary-key columns of a table, in key order (empty when it has none).
    pub(crate) async fn primary_key(&self, table: &DbObjectRef) -> Result<Vec<String>, DbError> {
        let text = |value: &str| DbParameter {
            name: String::new(),
            value: DbCell::Text(value.into()),
            type_hint: None,
        };
        let (sql, bindings) = match self.info.engine {
            DbEngine::Postgresql => (
                "SELECT a.attname FROM pg_catalog.pg_index i \
                 JOIN pg_catalog.pg_class c ON c.oid = i.indrelid \
                 JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace \
                 JOIN LATERAL pg_catalog.unnest(i.indkey) WITH ORDINALITY AS k(attnum, position) ON true \
                 JOIN pg_catalog.pg_attribute a ON a.attrelid = c.oid AND a.attnum = k.attnum \
                 WHERE i.indisprimary AND n.nspname = $1 AND c.relname = $2 AND c.relkind IN ('r', 'p') \
                 ORDER BY k.position",
                vec![text(table.schema.as_deref().unwrap_or("public")), text(&table.name)],
            ),
            DbEngine::Mysql | DbEngine::Mariadb => (
                "SELECT k.column_name FROM information_schema.key_column_usage k \
                 JOIN information_schema.tables t ON t.table_schema = k.table_schema \
                 AND t.table_name = k.table_name AND t.table_type = 'BASE TABLE' \
                 WHERE k.constraint_name = 'PRIMARY' AND k.table_schema = ? AND k.table_name = ? \
                 ORDER BY k.ordinal_position",
                vec![text(&table.database), text(&table.name)],
            ),
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

    /// The table and primary key a single-table read can edit, if any.
    pub(crate) async fn editable_source(&self, sql: &str) -> Option<(DbObjectRef, Vec<String>)> {
        if self.info.production || self.guard.policy().mobile {
            return None;
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
    ) -> Result<(DbQueryRequest, usize), DbError> {
        if self.info.production || table.kind != DbObjectKind::Table {
            return Err(DbError::PolicyDenied);
        }
        let key = self.primary_key(table).await?;
        if key.is_empty() {
            return Err(DbError::InvalidRequest);
        }
        cell_edit_request(self.info.engine, table, &key, edits)
    }

    /// Applies an approved edit request in one transaction. Every statement
    /// must change exactly one row, otherwise nothing is kept.
    pub async fn apply_cell_edits(&self, request: &DbQueryRequest) -> Result<u64, DbError> {
        let statements = self.guard.prepare(request)?;
        let mut parsed = Vec::with_capacity(statements.len());
        for statement in &statements {
            self.guard.validate_permit(statement)?;
            match statement.parsed() {
                crate::parser::ParsedStatement::Sql { text, parameters }
                    if text.trim_start().to_ascii_uppercase().starts_with("UPDATE") =>
                {
                    parsed.push((text.as_str(), parameters.as_slice()));
                }
                _ => return Err(DbError::InvalidRequest),
            }
        }
        let mut sink = OneRowEach::default();
        let mut driver = self.driver.lock().await;
        let run = match &mut *driver {
            Driver::Postgres(driver) => {
                driver.query_all(&parsed, false, &mut sink, self.scope.child_token())
            }
            Driver::Mysql(driver) => {
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
        let (request, rows) =
            cell_edit_request(DbEngine::Postgresql, &table, &["id".into()], &edits).unwrap();
        assert_eq!(rows, 2);
        assert_eq!(
            request.text,
            "UPDATE \"public\".\"items\" SET \"name\" = {{p0}}, \"note\" = {{p1}} WHERE \"id\" = {{p2}};\n\
             UPDATE \"public\".\"items\" SET \"name\" = {{p3}} WHERE \"id\" = {{p4}};"
        );
        assert_eq!(request.parameters.len(), 5);
        assert!(!request.text.contains("DROP"));
        // A key that does not match the table's primary key is refused.
        let mut wrong = edits[0].clone();
        wrong.original_primary_key =
            BTreeMap::from([("name".to_string(), DbCell::Text("a".into()))]);
        assert!(cell_edit_request(DbEngine::Postgresql, &table, &["id".into()], &[wrong]).is_err());
    }
}
