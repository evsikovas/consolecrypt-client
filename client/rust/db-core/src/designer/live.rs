//! The designer against a live session: the current structure of a table as
//! a [`TableDesign`] (read-only catalogue queries with bound names) and
//! applying an approved designer script with per-statement results.
use super::{
    clickhouse::split_top_level, postgres::short_type, CheckDesign, ColumnDesign, ForeignKeyDesign,
    IndexDesign, KeyDesign, TableDesign,
};
use crate::{
    parser::ParsedStatement,
    session::{Collect, Driver},
    streaming::RowConsumer,
    DbCell, DbColumn, DbError, DbObjectKind, DbObjectRef, DbParameter, DbQueryRequest, DbSession,
};
use async_trait::async_trait;
use cc_models::database::DbEngine;
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};

/// Separator of aggregated name lists (unit separator; never in a name).
const SEP: char = '\u{1f}';

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DesignStatementResult {
    /// done, failed, rolled_back (ran, then undone with the transaction) or
    /// skipped (not run after a failure).
    pub status: &'static str,
    pub error_code: Option<&'static str>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DesignApplyResult {
    pub statements: Vec<DesignStatementResult>,
    /// Every statement was applied.
    pub committed: bool,
}

pub(crate) fn error_code(error: &DbError) -> &'static str {
    crate::streaming::error_code(error)
}

/// Counts finished statements and drops any rows.
#[derive(Default)]
struct Progress {
    done: usize,
}

#[async_trait]
impl RowConsumer for Progress {
    async fn start(&mut self, _: Vec<DbColumn>, _: Option<DbObjectRef>) -> Result<(), DbError> {
        Ok(())
    }

    async fn row(&mut self, _: Vec<DbCell>) -> Result<bool, DbError> {
        Ok(false)
    }

    async fn statement_done(&mut self, _: u64, _: Option<u64>) -> Result<(), DbError> {
        self.done += 1;
        Ok(())
    }
}

fn text(name: &str, value: &str) -> DbParameter {
    DbParameter {
        name: name.into(),
        value: DbCell::Text(value.into()),
        type_hint: None,
    }
}

fn list(value: &str) -> Vec<String> {
    value
        .split(SEP)
        .filter(|part| !part.is_empty())
        .map(str::to_owned)
        .collect()
}

/// `name(a, b)` → (`name`, Some(`a, b`)) for the outermost parentheses.
fn split_params(text: &str) -> (String, Option<String>) {
    match (text.find('('), text.rfind(')')) {
        (Some(open), Some(close)) if close > open => {
            let base = format!("{}{}", &text[..open], &text[close + 1..]);
            (
                base.trim().to_owned(),
                Some(text[open + 1..close].to_owned()),
            )
        }
        _ => (text.trim().to_owned(), None),
    }
}

/// Numeric `(p)` or `(p, s)` parameters; None when they are anything else.
fn numeric_params(params: &str) -> Option<(u32, Option<u32>)> {
    let mut parts = params.split(',').map(|part| part.trim().parse::<u32>());
    let first = parts.next()?.ok()?;
    let second = match parts.next() {
        Some(value) => Some(value.ok()?),
        None => None,
    };
    parts.next().is_none().then_some((first, second))
}

/// PostgreSQL `format_type` text into the column type fields.
fn postgres_type(column: &mut ColumnDesign, formatted: &str) {
    let mut text = formatted.trim();
    while let Some(stripped) = text.strip_suffix("[]") {
        column.array = true;
        text = stripped;
    }
    let (base, params) = split_params(text);
    match params.as_deref().and_then(numeric_params) {
        Some((length, scale)) => {
            column.data_type = short_type(&base);
            column.length = Some(length);
            column.scale = scale;
        }
        None if params.is_none() => column.data_type = short_type(&base),
        None => column.data_type = text.to_owned(),
    }
}

fn postgres_action(code: &str) -> String {
    match code {
        "r" => "RESTRICT",
        "c" => "CASCADE",
        "n" => "SET NULL",
        "d" => "SET DEFAULT",
        _ => "",
    }
    .to_owned()
}

/// `CHECK ((a > 0))` → `(a > 0)`.
fn check_expression(definition: &str) -> String {
    let body = definition
        .trim()
        .strip_prefix("CHECK")
        .unwrap_or(definition)
        .trim()
        .trim_end_matches(" NOT VALID");
    match body
        .strip_prefix('(')
        .and_then(|inner| inner.strip_suffix(')'))
    {
        Some(inner) => inner.trim().to_owned(),
        None => body.to_owned(),
    }
}

impl DbSession {
    async fn design_rows(
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

    /// The current structure of `table` for alter mode. Items the designer
    /// cannot represent (expression or partial indexes, generated columns)
    /// are left out and therefore left alone.
    pub async fn table_design(&self, table: &DbObjectRef) -> Result<TableDesign, DbError> {
        if table.kind != DbObjectKind::Table {
            return Err(DbError::InvalidRequest);
        }
        let mut design = match self.info.engine {
            DbEngine::Postgresql => self.postgres_design(table).await?,
            DbEngine::Mysql | DbEngine::Mariadb => self.mysql_design(table).await?,
            DbEngine::Clickhouse => self.clickhouse_design(table).await?,
            DbEngine::Sqlite => self.sqlite_design(table).await?,
            DbEngine::Cassandra => self.cassandra_design(table).await?,
            _ => return Err(DbError::InvalidRequest),
        };
        if design.columns.is_empty() {
            return Err(DbError::QueryFailed);
        }
        design.original = Some(design.name.clone());
        for column in &mut design.columns {
            column.original = Some(column.name.clone());
        }
        if !design.primary_key.columns.is_empty() {
            design.primary_key.original = Some(design.primary_key.name.clone());
        }
        for key in &mut design.uniques {
            key.original = Some(key.name.clone());
        }
        for fk in &mut design.foreign_keys {
            fk.original = Some(fk.name.clone());
        }
        for index in &mut design.indexes {
            index.original = Some(index.name.clone());
        }
        for check in &mut design.checks {
            check.original = Some(check.name.clone());
        }
        Ok(design)
    }

    async fn postgres_design(&self, table: &DbObjectRef) -> Result<TableDesign, DbError> {
        let schema = table.schema.clone().unwrap_or_else(|| "public".into());
        let bindings = [text("", &schema), text("", &table.name)];
        let mut design = TableDesign {
            database: table.database.clone(),
            schema: Some(schema),
            name: table.name.clone(),
            ..Default::default()
        };
        for row in self
            .design_rows(
                "SELECT a.attname, pg_catalog.format_type(a.atttypid, a.atttypmod), (NOT a.attnotnull)::text, \
                 COALESCE(pg_catalog.pg_get_expr(d.adbin, d.adrelid), ''), a.attidentity::text, \
                 COALESCE(pg_catalog.col_description(c.oid, a.attnum), ''), a.attgenerated::text \
                 FROM pg_catalog.pg_attribute a JOIN pg_catalog.pg_class c ON c.oid = a.attrelid \
                 JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace \
                 LEFT JOIN pg_catalog.pg_attrdef d ON d.adrelid = a.attrelid AND d.adnum = a.attnum \
                 WHERE n.nspname = $1 AND c.relname = $2 AND c.relkind IN ('r', 'p') \
                 AND a.attnum > 0 AND NOT a.attisdropped ORDER BY a.attnum",
                &bindings,
            )
            .await?
        {
            let mut column = ColumnDesign {
                name: row[0].clone(),
                nullable: row[2] == "true",
                comment: row[5].clone(),
                ..Default::default()
            };
            postgres_type(&mut column, &row[1]);
            match row[4].as_str() {
                "a" => column.identity = "always".into(),
                "d" => column.identity = "by_default".into(),
                _ if row[6] == "s" => {}
                _ if row[3].starts_with("nextval(")
                    && matches!(column.data_type.as_str(), "integer" | "bigint" | "smallint") =>
                {
                    column.identity = "serial".into();
                }
                _ => column.default = row[3].clone(),
            }
            design.columns.push(column);
        }
        let sep = "pg_catalog.chr(31)";
        for row in self
            .design_rows(
                &format!(
                    "SELECT con.conname, con.contype::text, \
                     COALESCE((SELECT pg_catalog.string_agg(a.attname::text, {sep} ORDER BY k.ord) \
                       FROM pg_catalog.unnest(con.conkey) WITH ORDINALITY k(attnum, ord) \
                       JOIN pg_catalog.pg_attribute a ON a.attrelid = con.conrelid AND a.attnum = k.attnum), ''), \
                     COALESCE(fn.nspname, ''), COALESCE(fc.relname, ''), \
                     COALESCE((SELECT pg_catalog.string_agg(a.attname::text, {sep} ORDER BY k.ord) \
                       FROM pg_catalog.unnest(con.confkey) WITH ORDINALITY k(attnum, ord) \
                       JOIN pg_catalog.pg_attribute a ON a.attrelid = con.confrelid AND a.attnum = k.attnum), ''), \
                     con.confdeltype::text, con.confupdtype::text, \
                     CASE WHEN con.contype = 'c' THEN pg_catalog.pg_get_constraintdef(con.oid, true) ELSE '' END \
                     FROM pg_catalog.pg_constraint con JOIN pg_catalog.pg_class c ON c.oid = con.conrelid \
                     JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace \
                     LEFT JOIN pg_catalog.pg_class fc ON fc.oid = con.confrelid \
                     LEFT JOIN pg_catalog.pg_namespace fn ON fn.oid = fc.relnamespace \
                     WHERE n.nspname = $1 AND c.relname = $2 AND con.contype IN ('p', 'u', 'f', 'c') \
                     ORDER BY con.conname"
                ),
                &bindings,
            )
            .await?
        {
            let columns = list(&row[2]);
            match row[1].as_str() {
                "p" => {
                    design.primary_key = KeyDesign {
                        name: row[0].clone(),
                        columns,
                        ..Default::default()
                    }
                }
                "u" => design.uniques.push(KeyDesign {
                    name: row[0].clone(),
                    columns,
                    ..Default::default()
                }),
                "f" => design.foreign_keys.push(ForeignKeyDesign {
                    name: row[0].clone(),
                    columns,
                    ref_schema: Some(row[3].clone()).filter(|s| Some(s.as_str()) != table.schema.as_deref().or(Some("public"))),
                    ref_table: row[4].clone(),
                    ref_columns: list(&row[5]),
                    on_delete: postgres_action(&row[6]),
                    on_update: postgres_action(&row[7]),
                    ..Default::default()
                }),
                _ => design.checks.push(CheckDesign {
                    name: row[0].clone(),
                    expression: check_expression(&row[8]),
                    ..Default::default()
                }),
            }
        }
        for row in self
            .design_rows(
                &format!(
                    "SELECT ic.relname, i.indisunique::text, am.amname, \
                     COALESCE((SELECT pg_catalog.string_agg(a.attname::text, {sep} ORDER BY k.ord) \
                       FROM pg_catalog.unnest(i.indkey::pg_catalog.int2[]) WITH ORDINALITY k(attnum, ord) \
                       JOIN pg_catalog.pg_attribute a ON a.attrelid = i.indrelid AND a.attnum = k.attnum), ''), \
                     (i.indexprs IS NOT NULL OR i.indpred IS NOT NULL)::text \
                     FROM pg_catalog.pg_index i JOIN pg_catalog.pg_class ic ON ic.oid = i.indexrelid \
                     JOIN pg_catalog.pg_class c ON c.oid = i.indrelid \
                     JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace \
                     JOIN pg_catalog.pg_am am ON am.oid = ic.relam \
                     WHERE n.nspname = $1 AND c.relname = $2 AND NOT EXISTS \
                     (SELECT 1 FROM pg_catalog.pg_constraint con WHERE con.conindid = i.indexrelid) \
                     ORDER BY ic.relname"
                ),
                &bindings,
            )
            .await?
        {
            if row[4] == "true" {
                continue;
            }
            design.indexes.push(IndexDesign {
                name: row[0].clone(),
                unique: row[1] == "true",
                method: row[2].clone(),
                columns: list(&row[3]),
                ..Default::default()
            });
        }
        if let Some(row) = self
            .design_rows(
                "SELECT COALESCE(pg_catalog.obj_description(c.oid, 'pg_class'), ''), \
                 COALESCE(pg_catalog.pg_get_partkeydef(c.oid), ''), \
                 (c.relpersistence = 'u')::text \
                 FROM pg_catalog.pg_class c JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace \
                 WHERE n.nspname = $1 AND c.relname = $2",
                &bindings,
            )
            .await?
            .into_iter()
            .next()
        {
            design.comment = row[0].clone();
            if row.get(2).is_some_and(|unlogged| unlogged == "true") {
                design.options.insert("unlogged".into(), "true".into());
            }
            let (by, key) = split_params(&row[1]);
            if !by.is_empty() {
                design.options.insert("partition_by".into(), by.to_ascii_uppercase());
                design.options.insert(
                    "partition_key".into(),
                    key.unwrap_or_default().replace('"', ""),
                );
            }
        }
        Ok(design)
    }

    async fn mysql_design(&self, table: &DbObjectRef) -> Result<TableDesign, DbError> {
        let mariadb = self.info.engine == DbEngine::Mariadb;
        let database = if table.database.is_empty() {
            table.schema.clone().unwrap_or_default()
        } else {
            table.database.clone()
        };
        let bindings = [text("", &database), text("", &table.name)];
        let mut design = TableDesign {
            database: database.clone(),
            name: table.name.clone(),
            ..Default::default()
        };
        for row in self
            .design_rows(
                "SELECT column_name, data_type, column_type, is_nullable, COALESCE(column_default, ''), \
                 IF(column_default IS NULL, 'false', 'true'), extra, column_comment \
                 FROM information_schema.columns WHERE table_schema = ? AND table_name = ? \
                 ORDER BY ordinal_position",
                &bindings,
            )
            .await?
        {
            let base = row[1].to_ascii_lowercase();
            let column_type = row[2].to_ascii_lowercase();
            let extra = row[6].to_ascii_lowercase();
            let mut column = ColumnDesign {
                name: row[0].clone(),
                nullable: row[3] == "YES",
                unsigned: column_type.contains(" unsigned"),
                comment: row[7].clone(),
                ..Default::default()
            };
            let plain = column_type.replace(" unsigned", "").replace(" zerofill", "");
            let (_, params) = split_params(&plain);
            match params.as_deref().and_then(numeric_params) {
                Some((length, scale)) => {
                    column.data_type = base;
                    column.length = Some(length);
                    column.scale = scale;
                }
                None if params.is_none() => column.data_type = base,
                None => column.data_type = row[2].replace(" unsigned", "").replace(" zerofill", ""),
            }
            if extra.contains("auto_increment") {
                column.identity = "auto_increment".into();
            }
            if row[5] == "true" {
                let value = row[4].clone();
                column.default = if mariadb {
                    if value.eq_ignore_ascii_case("NULL") {
                        String::new()
                    } else {
                        value
                    }
                } else if extra.contains("default_generated") {
                    if value.to_ascii_lowercase().starts_with("current_timestamp") {
                        value
                    } else {
                        format!("({value})")
                    }
                } else if !value.is_empty() && value.parse::<f64>().is_ok() {
                    value
                } else {
                    format!("'{}'", value.replace('\\', "\\\\").replace('\'', "''"))
                };
            }
            if let Some(start) = extra.find("on update ") {
                let update = row[6][start..].trim().to_owned();
                if column.default.is_empty() {
                    column.default = "NULL".into();
                }
                column.default = format!("{} {}", column.default, update.to_ascii_uppercase());
            }
            design.columns.push(column);
        }
        for row in self
            .design_rows(
                "SELECT index_name, MAX(non_unique), GROUP_CONCAT(column_name ORDER BY seq_in_index SEPARATOR '\\Z'), \
                 MAX(index_type), COUNT(*) - COUNT(column_name) \
                 FROM information_schema.statistics WHERE table_schema = ? AND table_name = ? \
                 GROUP BY index_name ORDER BY index_name",
                &bindings,
            )
            .await?
        {
            if row[4] != "0" {
                continue; // a functional key part
            }
            let columns: Vec<String> = row[2].split('\u{1a}').map(str::to_owned).collect();
            if row[0] == "PRIMARY" {
                design.primary_key = KeyDesign {
                    name: "PRIMARY".into(),
                    columns,
                    ..Default::default()
                };
            } else if row[1] == "0" {
                design.uniques.push(KeyDesign {
                    name: row[0].clone(),
                    columns,
                    ..Default::default()
                });
            } else {
                design.indexes.push(IndexDesign {
                    name: row[0].clone(),
                    columns,
                    method: row[3].clone(),
                    ..Default::default()
                });
            }
        }
        for row in self
            .design_rows(
                "SELECT k.constraint_name, \
                 GROUP_CONCAT(k.column_name ORDER BY k.ordinal_position SEPARATOR '\\Z'), \
                 MAX(k.referenced_table_schema), MAX(k.referenced_table_name), \
                 GROUP_CONCAT(k.referenced_column_name ORDER BY k.ordinal_position SEPARATOR '\\Z'), \
                 MAX(r.delete_rule), MAX(r.update_rule) \
                 FROM information_schema.key_column_usage k \
                 JOIN information_schema.referential_constraints r ON r.constraint_schema = k.constraint_schema \
                 AND r.constraint_name = k.constraint_name AND r.table_name = k.table_name \
                 WHERE k.table_schema = ? AND k.table_name = ? AND k.referenced_table_name IS NOT NULL \
                 GROUP BY k.constraint_name ORDER BY k.constraint_name",
                &bindings,
            )
            .await?
        {
            let action = |rule: &str| if rule == "NO ACTION" { String::new() } else { rule.to_owned() };
            design.foreign_keys.push(ForeignKeyDesign {
                name: row[0].clone(),
                columns: row[1].split('\u{1a}').map(str::to_owned).collect(),
                ref_schema: Some(row[2].clone()).filter(|schema| *schema != database),
                ref_table: row[3].clone(),
                ref_columns: row[4].split('\u{1a}').map(str::to_owned).collect(),
                on_delete: action(&row[5]),
                on_update: action(&row[6]),
                ..Default::default()
            });
        }
        // The index MySQL keeps for a foreign key is part of the key.
        let fk_names: Vec<String> = design
            .foreign_keys
            .iter()
            .map(|fk| fk.name.clone())
            .collect();
        design
            .indexes
            .retain(|index| !fk_names.contains(&index.name));
        let checks = self
            .design_rows(
                "SELECT c.constraint_name, c.check_clause FROM information_schema.check_constraints c \
                 JOIN information_schema.table_constraints t ON t.constraint_schema = c.constraint_schema \
                 AND t.constraint_name = c.constraint_name \
                 WHERE t.table_schema = ? AND t.table_name = ? AND t.constraint_type = 'CHECK' \
                 ORDER BY c.constraint_name",
                &bindings,
            )
            .await
            .unwrap_or_default();
        for row in checks {
            let expression = row[1].trim();
            let expression = expression
                .strip_prefix('(')
                .and_then(|inner| inner.strip_suffix(')'))
                .unwrap_or(expression);
            design.checks.push(CheckDesign {
                name: row[0].clone(),
                expression: expression.to_owned(),
                ..Default::default()
            });
        }
        if let Some(row) = self
            .design_rows(
                "SELECT COALESCE(engine, ''), COALESCE(table_collation, ''), COALESCE(table_comment, '') \
                 FROM information_schema.tables WHERE table_schema = ? AND table_name = ?",
                &bindings,
            )
            .await?
            .into_iter()
            .next()
        {
            design.options.insert("engine".into(), row[0].clone());
            if let Some((charset, _)) = row[1].split_once('_') {
                design.options.insert("charset".into(), charset.to_owned());
            }
            design.options.insert("collation".into(), row[1].clone());
            design.comment = row[2].clone();
        }
        Ok(design)
    }

    async fn clickhouse_design(&self, table: &DbObjectRef) -> Result<TableDesign, DbError> {
        let bindings = [
            text("cc_database", &table.database),
            text("cc_table", &table.name),
        ];
        let mut design = TableDesign {
            database: table.database.clone(),
            name: table.name.clone(),
            ..Default::default()
        };
        for row in self
            .design_rows(
                "SELECT name, type, default_kind, default_expression, comment FROM system.columns \
                 WHERE database = {cc_database:String} AND table = {cc_table:String} ORDER BY position",
                &bindings,
            )
            .await?
        {
            let mut column = ColumnDesign {
                name: row[0].clone(),
                comment: row[4].clone(),
                ..Default::default()
            };
            let mut kind = row[1].trim().to_owned();
            if let Some(inner) = kind.strip_prefix("LowCardinality(").and_then(|k| k.strip_suffix(')')) {
                column.low_cardinality = true;
                kind = inner.to_owned();
            }
            if let Some(inner) = kind.strip_prefix("Nullable(").and_then(|k| k.strip_suffix(')')) {
                column.nullable = true;
                kind = inner.to_owned();
            }
            let (base, params) = split_params(&kind);
            match params.as_deref().and_then(numeric_params) {
                Some((length, scale)) if !base.contains(' ') && !kind.ends_with("))") => {
                    column.data_type = base;
                    column.length = Some(length);
                    column.scale = scale;
                }
                _ => column.data_type = kind,
            }
            if row[2] == "DEFAULT" {
                column.default = row[3].clone();
            }
            design.columns.push(column);
        }
        if let Some(row) = self
            .design_rows(
                "SELECT engine, sorting_key, partition_key, primary_key, comment, engine_full FROM system.tables \
                 WHERE database = {cc_database:String} AND name = {cc_table:String}",
                &bindings,
            )
            .await?
            .into_iter()
            .next()
        {
            design.options.insert("engine".into(), row[0].clone());
            if let Some(rest) = row[5].strip_prefix(row[0].as_str()) {
                if rest.starts_with('(') {
                    let mut depth = 0;
                    let mut end = 0;
                    for (i, c) in rest.char_indices() {
                        match c {
                            '(' => depth += 1,
                            ')' => {
                                depth -= 1;
                                if depth == 0 {
                                    end = i;
                                    break;
                                }
                            }
                            _ => {}
                        }
                    }
                    let args = rest[1..end].trim();
                    if !args.is_empty() {
                        design.options.insert("engine_args".into(), args.to_owned());
                    }
                }
            }
            if !row[1].is_empty() {
                design.options.insert("order_by".into(), row[1].clone());
            }
            if !row[2].is_empty() {
                design.options.insert("partition_by".into(), row[2].clone());
            }
            let key = split_top_level(&row[3]);
            if !key.is_empty() && key.iter().all(|part| design.columns.iter().any(|c| c.name == *part)) {
                design.primary_key.columns = key;
            }
            design.comment = row[4].clone();
        }
        Ok(design)
    }

    /// SQLite: columns, keys, indexes and foreign keys from the PRAGMA
    /// functions; checks, AUTOINCREMENT, STRICT and WITHOUT ROWID from the
    /// stored CREATE TABLE. What the design cannot carry (generated columns,
    /// collations, expression or partial indexes, triggers) is listed in
    /// `unmodeled`, so a change that would rebuild the table is refused.
    async fn sqlite_design(&self, table: &DbObjectRef) -> Result<TableDesign, DbError> {
        use sqlparser::ast::{ColumnOption, Statement, TableConstraint};
        let name = [text("", &table.name)];
        let mut design = TableDesign {
            database: table.database.clone(),
            name: table.name.clone(),
            ..Default::default()
        };
        let mut unmodeled = BTreeSet::new();
        let definition = self
            .design_rows(
                "SELECT sql FROM sqlite_master WHERE type = 'table' AND name = ?1",
                &name,
            )
            .await?
            .into_iter()
            .next()
            .and_then(|row| row.into_iter().next())
            .ok_or(DbError::QueryFailed)?;
        if definition
            .trim_start()
            .to_ascii_uppercase()
            .starts_with("CREATE VIRTUAL")
        {
            return Err(DbError::InvalidRequest);
        }
        let mut key: Vec<(u32, String)> = Vec::new();
        for row in self
            .design_rows(
                "SELECT name, type, \"notnull\", dflt_value, pk, hidden \
                 FROM pragma_table_xinfo(?1) ORDER BY cid",
                &name,
            )
            .await?
        {
            match row[5].as_str() {
                "0" => {}
                "2" | "3" => {
                    unmodeled.insert("generated_columns");
                    continue;
                }
                _ => continue,
            }
            let mut column = ColumnDesign {
                name: row[0].clone(),
                nullable: row[2] == "0",
                default: row[3].clone(),
                ..Default::default()
            };
            let (base, params) = split_params(&row[1]);
            match params.as_deref().and_then(numeric_params) {
                Some((length, scale)) => {
                    column.data_type = base;
                    column.length = Some(length);
                    column.scale = scale;
                }
                None => column.data_type = row[1].trim().to_owned(),
            }
            if let Ok(position) = row[4].parse::<u32>() {
                if position > 0 {
                    key.push((position, column.name.clone()));
                }
            }
            design.columns.push(column);
        }
        key.sort();
        design.primary_key.columns = key.into_iter().map(|(_, name)| name).collect();
        match sqlparser::parser::Parser::parse_sql(
            &sqlparser::dialect::SQLiteDialect {},
            &definition,
        ) {
            Ok(statements) => match statements.as_slice() {
                [Statement::CreateTable(create)] => {
                    if create.strict {
                        design.options.insert("strict".into(), "true".into());
                    }
                    if create.without_rowid {
                        design.options.insert("without_rowid".into(), "true".into());
                    }
                    for column in &create.columns {
                        for option in &column.options {
                            match &option.option {
                                ColumnOption::Check(check) => design.checks.push(CheckDesign {
                                    name: option
                                        .name
                                        .as_ref()
                                        .or(check.name.as_ref())
                                        .map(|n| n.value.clone())
                                        .unwrap_or_default(),
                                    expression: check.expr.to_string(),
                                    ..Default::default()
                                }),
                                ColumnOption::DialectSpecific(tokens)
                                    if tokens.iter().any(|t| {
                                        t.to_string().eq_ignore_ascii_case("AUTOINCREMENT")
                                    }) =>
                                {
                                    if let Some(designed) = design
                                        .columns
                                        .iter_mut()
                                        .find(|c| c.name == column.name.value)
                                    {
                                        designed.identity = "autoincrement".into();
                                    }
                                }
                                ColumnOption::Collation(_) => {
                                    unmodeled.insert("collations");
                                }
                                ColumnOption::Null
                                | ColumnOption::NotNull
                                | ColumnOption::Default(_)
                                | ColumnOption::PrimaryKey(_)
                                | ColumnOption::Unique(_)
                                | ColumnOption::ForeignKey(_)
                                | ColumnOption::Generated { .. } => {}
                                _ => {
                                    unmodeled.insert("column_options");
                                }
                            }
                        }
                    }
                    for constraint in &create.constraints {
                        if let TableConstraint::Check(check) = constraint {
                            design.checks.push(CheckDesign {
                                name: check
                                    .name
                                    .as_ref()
                                    .map(|n| n.value.clone())
                                    .unwrap_or_default(),
                                expression: check.expr.to_string(),
                                ..Default::default()
                            });
                        }
                    }
                }
                _ => {
                    unmodeled.insert("definition");
                }
            },
            Err(_) => {
                unmodeled.insert("definition");
            }
        }
        for row in self
            .design_rows(
                "SELECT name, \"unique\", origin, partial FROM pragma_index_list(?1)",
                &name,
            )
            .await?
        {
            if row[2] == "pk" {
                continue;
            }
            if row[3] == "1" {
                unmodeled.insert("partial_indexes");
                continue;
            }
            let mut columns = Vec::new();
            let mut plain = true;
            for part in self
                .design_rows(
                    "SELECT cid, name, \"desc\", coll FROM pragma_index_xinfo(?1) \
                     WHERE key = 1 ORDER BY seqno",
                    &[text("", &row[0])],
                )
                .await?
            {
                if part[0] == "-2" {
                    unmodeled.insert("expression_indexes");
                    plain = false;
                } else if part[2] == "1" || !part[3].eq_ignore_ascii_case("BINARY") {
                    unmodeled.insert("index_order");
                    plain = false;
                }
                columns.push(part[1].clone());
            }
            if !plain || columns.is_empty() {
                continue;
            }
            if row[2] == "u" {
                design.uniques.push(KeyDesign {
                    columns,
                    ..Default::default()
                });
            } else {
                design.indexes.push(IndexDesign {
                    name: row[0].clone(),
                    columns,
                    unique: row[1] == "1",
                    ..Default::default()
                });
            }
        }
        let mut foreign: BTreeMap<String, ForeignKeyDesign> = BTreeMap::new();
        for row in self
            .design_rows(
                "SELECT id, \"table\", \"from\", \"to\", on_update, on_delete \
                 FROM pragma_foreign_key_list(?1) ORDER BY id, seq",
                &name,
            )
            .await?
        {
            let action = |value: &str| {
                if value.eq_ignore_ascii_case("NO ACTION") {
                    String::new()
                } else {
                    value.to_ascii_uppercase()
                }
            };
            let fk = foreign
                .entry(row[0].clone())
                .or_insert_with(|| ForeignKeyDesign {
                    ref_table: row[1].clone(),
                    on_update: action(&row[4]),
                    on_delete: action(&row[5]),
                    ..Default::default()
                });
            fk.columns.push(row[2].clone());
            if !row[3].is_empty() {
                fk.ref_columns.push(row[3].clone());
            }
        }
        for mut fk in foreign.into_values() {
            if fk.ref_columns.is_empty() {
                // REFERENCES t without columns: the primary key of t.
                fk.ref_columns = self
                    .design_rows(
                        "SELECT name FROM pragma_table_info(?1) WHERE pk > 0 ORDER BY pk",
                        &[text("", &fk.ref_table)],
                    )
                    .await?
                    .into_iter()
                    .filter_map(|row| row.into_iter().next())
                    .collect();
            }
            if fk.ref_columns.len() != fk.columns.len() {
                unmodeled.insert("foreign_keys");
                continue;
            }
            design.foreign_keys.push(fk);
        }
        let triggers = self
            .design_rows(
                "SELECT count(*) FROM sqlite_master WHERE type = 'trigger' AND tbl_name = ?1",
                &name,
            )
            .await?;
        if triggers
            .first()
            .and_then(|row| row.first())
            .is_some_and(|n| n != "0")
        {
            unmodeled.insert("triggers");
        }
        design.unmodeled = unmodeled.into_iter().map(str::to_owned).collect();
        Ok(design)
    }

    /// Cassandra / ScyllaDB: key columns in key order, the partition key
    /// size and clustering order, table options and plain secondary indexes.
    async fn cassandra_design(&self, table: &DbObjectRef) -> Result<TableDesign, DbError> {
        use crate::cassandra::CqlColumnKind;
        let columns = self.cassandra_columns(table).await?;
        let (options, indexes) = {
            let driver = self.driver.lock().await;
            let Driver::Cassandra(driver) = &*driver else {
                return Err(DbError::InvalidRequest);
            };
            driver
                .design_metadata(&table.database, &table.name, &self.scope.child_token())
                .await?
        };
        let mut design = TableDesign {
            database: table.database.clone(),
            name: table.name.clone(),
            ..Default::default()
        };
        let mut partition = 0;
        let mut order = Vec::new();
        for column in columns {
            match column.kind {
                CqlColumnKind::PartitionKey => {
                    partition += 1;
                    design.primary_key.columns.push(column.name.clone());
                }
                CqlColumnKind::Clustering { descending } => {
                    design.primary_key.columns.push(column.name.clone());
                    order.push(format!(
                        "{} {}",
                        crate::parser::cql::quote_name(&column.name),
                        if descending { "DESC" } else { "ASC" }
                    ));
                }
                CqlColumnKind::Static | CqlColumnKind::Regular => {}
            }
            design.columns.push(ColumnDesign {
                name: column.name,
                data_type: column.type_name,
                nullable: true,
                ..Default::default()
            });
        }
        design
            .options
            .insert("partition_key_columns".into(), partition.to_string());
        if !order.is_empty() {
            design
                .options
                .insert("clustering_order".into(), order.join(", "));
        }
        if let [comment, ttl, gc] = options.as_slice() {
            design.comment = comment.clone();
            if !ttl.is_empty() && ttl != "0" {
                design
                    .options
                    .insert("default_time_to_live".into(), ttl.clone());
            }
            if !gc.is_empty() {
                design.options.insert("gc_grace_seconds".into(), gc.clone());
            }
        }
        design.indexes = indexes
            .into_iter()
            .map(|(name, column)| IndexDesign {
                name,
                columns: vec![column],
                ..Default::default()
            })
            .collect();
        Ok(design)
    }

    /// Runs an approved designer script. PostgreSQL and SQLite apply it in
    /// one transaction (all or nothing); other engines run the statements in
    /// order and stop at the first failure.
    pub async fn apply_design(
        &self,
        request: &DbQueryRequest,
    ) -> Result<DesignApplyResult, DbError> {
        let statements = self.guard.prepare(request)?;
        for statement in &statements {
            self.guard.validate_permit(statement)?;
        }
        let count = statements.len();
        let mut progress = Progress::default();
        let mut driver = self.driver.lock().await;
        let result = if let Driver::Postgres(postgres) = &mut *driver {
            let mut parsed = Vec::with_capacity(count);
            for statement in &statements {
                match statement.parsed() {
                    ParsedStatement::Sql { text, parameters } => {
                        parsed.push((text.as_str(), parameters.as_slice()))
                    }
                    _ => return Err(DbError::InvalidRequest),
                }
            }
            let result = postgres
                .query_all(&parsed, false, &mut progress, self.scope.child_token())
                .await;
            let failed = result.as_ref().err().map(error_code);
            DesignApplyResult {
                statements: (0..count)
                    .map(|i| DesignStatementResult {
                        status: match (&failed, i.cmp(&progress.done)) {
                            (None, _) => "done",
                            (Some(_), std::cmp::Ordering::Less) => "rolled_back",
                            (Some(_), std::cmp::Ordering::Equal) => "failed",
                            (Some(_), std::cmp::Ordering::Greater) => "skipped",
                        },
                        error_code: failed.filter(|_| i == progress.done),
                    })
                    .collect(),
                committed: failed.is_none(),
            }
        } else if matches!(&*driver, Driver::Sqlite(_) | Driver::SqliteCli(_)) {
            let mut parsed = Vec::with_capacity(count);
            for statement in &statements {
                match statement.parsed() {
                    ParsedStatement::Sql { text, parameters } => {
                        parsed.push((text.clone(), parameters.clone()))
                    }
                    _ => return Err(DbError::InvalidRequest),
                }
            }
            let cancel = self.scope.child_token();
            let (done, outcome) = match &mut *driver {
                Driver::Sqlite(sqlite) => sqlite.apply_script(parsed, cancel).await,
                Driver::SqliteCli(cli) => cli.apply_script(&parsed, cancel).await,
                _ => unreachable!("matched above"),
            };
            let failed = outcome.as_ref().err().map(error_code);
            DesignApplyResult {
                statements: (0..count)
                    .map(|i| DesignStatementResult {
                        status: match (&failed, i.cmp(&done)) {
                            (None, _) => "done",
                            (Some(_), std::cmp::Ordering::Less) => "rolled_back",
                            (Some(_), std::cmp::Ordering::Equal) => "failed",
                            (Some(_), std::cmp::Ordering::Greater) => "skipped",
                        },
                        error_code: failed.filter(|_| i == done),
                    })
                    .collect(),
                committed: failed.is_none(),
            }
        } else {
            let mut results = Vec::with_capacity(count);
            let mut failed = false;
            for statement in &statements {
                if failed {
                    results.push(DesignStatementResult {
                        status: "skipped",
                        error_code: None,
                    });
                    continue;
                }
                driver.set_production_write(statement.production_write());
                let result = driver
                    .run(
                        statement.parsed(),
                        false,
                        &mut progress,
                        self.scope.child_token(),
                    )
                    .await;
                driver.set_production_write(false);
                failed = result.is_err();
                results.push(DesignStatementResult {
                    status: if failed { "failed" } else { "done" },
                    error_code: result.err().as_ref().map(error_code),
                });
            }
            DesignApplyResult {
                statements: results,
                committed: !failed,
            }
        };
        drop(driver);
        self.refresh_completion().await;
        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_catalogue_types() {
        let mut column = ColumnDesign::default();
        postgres_type(&mut column, "character varying(100)[]");
        assert_eq!(
            (column.data_type.as_str(), column.length, column.array),
            ("varchar", Some(100), true)
        );
        let mut column = ColumnDesign::default();
        postgres_type(&mut column, "timestamp(3) with time zone");
        assert_eq!(
            (column.data_type.as_str(), column.length),
            ("timestamptz", Some(3))
        );
        let mut column = ColumnDesign::default();
        postgres_type(&mut column, "numeric(10,2)");
        assert_eq!((column.length, column.scale), (Some(10), Some(2)));
        assert_eq!(numeric_params("3, 'UTC'"), None);
        assert_eq!(
            check_expression("CHECK ((total >= (0)::numeric))"),
            "(total >= (0)::numeric)"
        );
        assert_eq!(list("a\u{1f}b"), ["a", "b"]);
        let _ = SEP;
    }
}
