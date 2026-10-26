//! Table designer: a structured table (or collection) design, validated and
//! turned into DDL natively. The client edits a [`TableDesign`]; [`plan`]
//! returns the statements that create it, or — given the structure loaded
//! from the server — the ALTER statements that change it.
//!
//! Everything engine specific lives behind [`Dialect`]: the type catalogue
//! and options the client shows ([`DialectInfo`]), validation, quoting and
//! the CREATE/ALTER rendering. A new engine is one more `Dialect`
//! implementation registered in [`dialect`].
//!
//! The generated script is never trusted on its own: it runs through the
//! session guard (review ticket, vault passphrase, production write window)
//! like any editor text, so every statement must be something the guard's
//! parser accepts and renders back unchanged. Unit tests check that for
//! every golden statement.
use cc_models::database::DbEngine;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

mod cassandra;
mod clickhouse;
mod live;
mod mongo;
mod mysql;
mod postgres;
mod sqlite;

pub use live::{DesignApplyResult, DesignStatementResult};

/// Longest identifier, default text or expression accepted from a design.
const MAX_TEXT: usize = 4096;
/// Most columns, keys, indexes or checks in one design.
const MAX_ITEMS: usize = 1000;

/// One table (or collection) as the designer edits it. Items loaded from the
/// server carry their server name in `original`; new ones have none. Column
/// references in keys and indexes use the current column names.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct TableDesign {
    /// MySQL/ClickHouse/MongoDB database; for PostgreSQL the session database.
    pub database: String,
    /// PostgreSQL schema.
    pub schema: Option<String>,
    pub name: String,
    /// Name on the server when altering an existing table.
    pub original: Option<String>,
    pub comment: String,
    pub columns: Vec<ColumnDesign>,
    /// No columns: no primary key.
    pub primary_key: KeyDesign,
    pub uniques: Vec<KeyDesign>,
    pub foreign_keys: Vec<ForeignKeyDesign>,
    pub indexes: Vec<IndexDesign>,
    pub checks: Vec<CheckDesign>,
    /// Engine options by id (see [`OptionInfo`]); missing means the default.
    pub options: BTreeMap<String, String>,
    /// What the loaded table has but the design does not carry (SQLite
    /// triggers, generated columns, collations, …). A change that would
    /// recreate the table, and so lose it, is refused.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub unmodeled: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ColumnDesign {
    pub original: Option<String>,
    pub name: String,
    /// Type name from the dialect catalogue or typed freely (validated).
    pub data_type: String,
    /// Length or precision.
    pub length: Option<u32>,
    pub scale: Option<u32>,
    /// PostgreSQL `type[]`.
    pub array: bool,
    /// MySQL `UNSIGNED`.
    pub unsigned: bool,
    /// SQL: NULL allowed. ClickHouse: `Nullable(T)`. MongoDB: not required.
    pub nullable: bool,
    /// SQL expression; plain text that is not an expression becomes a
    /// string literal. Empty: no default.
    pub default: String,
    /// "", or one of [`DialectInfo::identity_kinds`].
    pub identity: String,
    /// ClickHouse `LowCardinality(T)`.
    pub low_cardinality: bool,
    pub comment: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct KeyDesign {
    pub original: Option<String>,
    /// Empty: generated from the table and column names.
    pub name: String,
    pub columns: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ForeignKeyDesign {
    pub original: Option<String>,
    pub name: String,
    pub columns: Vec<String>,
    /// PostgreSQL schema or MySQL database of the referenced table; None:
    /// the design's own.
    pub ref_schema: Option<String>,
    pub ref_table: String,
    pub ref_columns: Vec<String>,
    /// One of [`DialectInfo::fk_actions`]; empty: the server default.
    pub on_delete: String,
    pub on_update: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct IndexDesign {
    pub original: Option<String>,
    pub name: String,
    pub columns: Vec<String>,
    pub unique: bool,
    /// One of [`DialectInfo::index_methods`]; empty: the default.
    pub method: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct CheckDesign {
    pub original: Option<String>,
    pub name: String,
    pub expression: String,
}

/// How a type takes parameters.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TypeParams {
    None,
    /// `type(n)`.
    Length,
    /// `type(p, s)` with optional scale.
    PrecisionScale,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TypeInfo {
    pub name: &'static str,
    /// numeric, text, time, binary, json, other (the client groups by it).
    pub group: &'static str,
    pub params: TypeParams,
    /// The type cannot be used without a length.
    pub length_required: bool,
}

const fn ty(name: &'static str, group: &'static str) -> TypeInfo {
    TypeInfo {
        name,
        group,
        params: TypeParams::None,
        length_required: false,
    }
}

const fn ty_len(name: &'static str, group: &'static str, required: bool) -> TypeInfo {
    TypeInfo {
        name,
        group,
        params: TypeParams::Length,
        length_required: required,
    }
}

const fn ty_ps(name: &'static str, group: &'static str) -> TypeInfo {
    TypeInfo {
        name,
        group,
        params: TypeParams::PrecisionScale,
        length_required: false,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum OptionKind {
    Bool,
    Choice,
    Text,
    Number,
}

/// One engine option shown on the Options tab; the client localizes the
/// label by `id` and shows choices verbatim.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct OptionInfo {
    pub id: &'static str,
    pub kind: OptionKind,
    pub choices: Vec<&'static str>,
    pub default: &'static str,
    /// Only meaningful when creating; changes in alter mode are refused.
    pub create_only: bool,
}

/// What the designer offers for an engine.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct DesignFeatures {
    /// Designs a MongoDB collection (fields become a JSON-schema validator).
    pub collection: bool,
    /// The table lives in a schema (PostgreSQL) rather than a database.
    pub schemas: bool,
    pub primary_key: bool,
    pub unique: bool,
    pub foreign_keys: bool,
    pub checks: bool,
    pub indexes: bool,
    pub arrays: bool,
    pub unsigned: bool,
    /// NOT NULL is spelled as a `Nullable(T)` wrapper (ClickHouse).
    pub nullable_wrapper: bool,
    pub low_cardinality: bool,
    pub table_comment: bool,
    pub column_comments: bool,
    /// Existing tables can be altered.
    pub alter: bool,
    /// The script runs in one transaction: all or nothing.
    pub transactional: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DialectInfo {
    pub engine: DbEngine,
    pub features: DesignFeatures,
    pub types: Vec<TypeInfo>,
    pub default_type: &'static str,
    pub identity_kinds: Vec<&'static str>,
    /// Ready-made defaults offered next to the default field.
    pub default_presets: Vec<&'static str>,
    pub index_methods: Vec<&'static str>,
    pub fk_actions: Vec<&'static str>,
    pub options: Vec<OptionInfo>,
    pub max_identifier: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Severity {
    Error,
    Warning,
}

/// A validation finding. `target` points at the design item
/// (`table.name`, `columns.2.data_type`, `options.order_by`, …); `code` is
/// localized by the client; `value` names the item for the message.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DesignIssue {
    pub severity: Severity,
    pub code: &'static str,
    pub target: String,
    pub value: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DesignStatement {
    pub sql: String,
    /// Drops data or rewrites the table (dropped column, type change, …).
    pub destructive: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DesignPlan {
    pub statements: Vec<DesignStatement>,
    /// The statements as one script (what is reviewed and run).
    pub script: String,
    pub issues: Vec<DesignIssue>,
    /// Applied in one transaction.
    pub transactional: bool,
}

impl DesignPlan {
    pub fn has_errors(&self) -> bool {
        self.issues
            .iter()
            .any(|issue| issue.severity == Severity::Error)
    }
}

/// Issues collected while validating and rendering.
#[derive(Debug, Default)]
pub struct Issues(Vec<DesignIssue>);

impl Issues {
    pub fn error(&mut self, code: &'static str, target: impl Into<String>, value: &str) {
        self.push(Severity::Error, code, target.into(), value);
    }

    pub fn warning(&mut self, code: &'static str, target: impl Into<String>, value: &str) {
        self.push(Severity::Warning, code, target.into(), value);
    }

    fn push(&mut self, severity: Severity, code: &'static str, target: String, value: &str) {
        let issue = DesignIssue {
            severity,
            code,
            target,
            value: value.chars().take(120).collect(),
        };
        if !self.0.contains(&issue) && self.0.len() < 500 {
            self.0.push(issue);
        }
    }

    pub fn has_errors(&self) -> bool {
        self.0.iter().any(|issue| issue.severity == Severity::Error)
    }
}

/// What changed between the loaded structure and the design (alter mode).
/// Items are matched by their `original` server name.
#[derive(Debug, Default)]
pub struct TableChanges<'a> {
    pub renamed_table: bool,
    pub dropped_columns: Vec<&'a ColumnDesign>,
    pub added_columns: Vec<(usize, &'a ColumnDesign)>,
    /// (before, after, position in the design)
    pub kept_columns: Vec<(&'a ColumnDesign, &'a ColumnDesign, usize)>,
    pub primary_key_changed: bool,
    pub dropped_uniques: Vec<&'a KeyDesign>,
    pub added_uniques: Vec<&'a KeyDesign>,
    pub dropped_foreign_keys: Vec<&'a ForeignKeyDesign>,
    pub added_foreign_keys: Vec<&'a ForeignKeyDesign>,
    pub dropped_indexes: Vec<&'a IndexDesign>,
    pub added_indexes: Vec<&'a IndexDesign>,
    pub dropped_checks: Vec<&'a CheckDesign>,
    pub added_checks: Vec<&'a CheckDesign>,
    pub comment_changed: bool,
    pub changed_options: Vec<&'a str>,
}

/// Items of `before` missing from or changed in `after` are dropped; new or
/// changed ones are added. An item is unchanged when it equals its original.
fn diff_items<'b, 'a, T: PartialEq>(
    before: &'b [T],
    after: &'a [T],
    original: impl Fn(&T) -> Option<&str>,
    name: impl Fn(&T) -> &str,
) -> (Vec<&'b T>, Vec<&'a T>) {
    let mut dropped = Vec::new();
    let mut added = Vec::new();
    for old in before {
        let kept = after
            .iter()
            .find(|new| original(new) == Some(name(old)) && *new == old);
        if kept.is_none() {
            dropped.push(old);
        }
    }
    for new in after {
        let same = original(new)
            .and_then(|server| before.iter().find(|old| name(old) == server))
            .is_some_and(|old| old == new);
        if !same {
            added.push(new);
        }
    }
    (dropped, added)
}

impl<'a> TableChanges<'a> {
    pub fn between(before: &'a TableDesign, after: &'a TableDesign) -> Self {
        let mut changes = TableChanges {
            renamed_table: before.name != after.name,
            comment_changed: before.comment != after.comment,
            ..Default::default()
        };
        for old in &before.columns {
            if !after
                .columns
                .iter()
                .any(|new| new.original.as_deref() == Some(old.name.as_str()))
            {
                changes.dropped_columns.push(old);
            }
        }
        for (position, new) in after.columns.iter().enumerate() {
            match new
                .original
                .as_deref()
                .and_then(|server| before.columns.iter().find(|old| old.name == server))
            {
                Some(old) => changes.kept_columns.push((old, new, position)),
                None => changes.added_columns.push((position, new)),
            }
        }
        // A key over renamed columns is the same key: compare it with the
        // renames applied to the old column list.
        let renamed = |columns: &[String]| -> Vec<String> {
            columns
                .iter()
                .map(|column| {
                    after
                        .columns
                        .iter()
                        .find(|new| new.original.as_deref() == Some(column.as_str()))
                        .map_or_else(|| column.clone(), |new| new.name.clone())
                })
                .collect()
        };
        changes.primary_key_changed = renamed(&before.primary_key.columns)
            != after.primary_key.columns
            || (!after.primary_key.columns.is_empty()
                && !after.primary_key.name.is_empty()
                && after.primary_key.original.is_some()
                && after.primary_key.name != before.primary_key.name);
        let keys = |list: &[KeyDesign]| -> Vec<KeyDesign> {
            list.iter()
                .map(|key| KeyDesign {
                    original: Some(key.name.clone()),
                    name: key.name.clone(),
                    columns: renamed(&key.columns),
                })
                .collect()
        };
        let old_uniques = keys(&before.uniques);
        let (dropped, added) = diff_items(
            &old_uniques,
            &after.uniques,
            |k| k.original.as_deref(),
            |k| k.name.as_str(),
        );
        changes.dropped_uniques = dropped
            .into_iter()
            .filter_map(|k| before.uniques.iter().find(|old| old.name == k.name))
            .collect();
        changes.added_uniques = added;
        let old_fks: Vec<ForeignKeyDesign> = before
            .foreign_keys
            .iter()
            .map(|fk| ForeignKeyDesign {
                original: Some(fk.name.clone()),
                columns: renamed(&fk.columns),
                ..fk.clone()
            })
            .collect();
        let (dropped, added) = diff_items(
            &old_fks,
            &after.foreign_keys,
            |k| k.original.as_deref(),
            |k| k.name.as_str(),
        );
        changes.dropped_foreign_keys = dropped
            .into_iter()
            .filter_map(|k| before.foreign_keys.iter().find(|old| old.name == k.name))
            .collect();
        changes.added_foreign_keys = added;
        let old_indexes: Vec<IndexDesign> = before
            .indexes
            .iter()
            .map(|index| IndexDesign {
                original: Some(index.name.clone()),
                columns: renamed(&index.columns),
                ..index.clone()
            })
            .collect();
        let (dropped, added) = diff_items(
            &old_indexes,
            &after.indexes,
            |k| k.original.as_deref(),
            |k| k.name.as_str(),
        );
        changes.dropped_indexes = dropped
            .into_iter()
            .filter_map(|k| before.indexes.iter().find(|old| old.name == k.name))
            .collect();
        changes.added_indexes = added;
        let old_checks: Vec<CheckDesign> = before
            .checks
            .iter()
            .map(|check| CheckDesign {
                original: Some(check.name.clone()),
                ..check.clone()
            })
            .collect();
        let (dropped, added) = diff_items(
            &old_checks,
            &after.checks,
            |k| k.original.as_deref(),
            |k| k.name.as_str(),
        );
        changes.dropped_checks = dropped
            .into_iter()
            .filter_map(|k| before.checks.iter().find(|old| old.name == k.name))
            .collect();
        changes.added_checks = added;
        let ids: BTreeSet<&String> = before.options.keys().chain(after.options.keys()).collect();
        for id in ids {
            if before.options.get(id) != after.options.get(id) {
                changes.changed_options.push(id.as_str());
            }
        }
        changes
    }

    /// A column whose definition (not only its name) changed.
    pub fn definition_changed(before: &ColumnDesign, after: &ColumnDesign) -> bool {
        ColumnDesign {
            name: before.name.clone(),
            original: None,
            ..after.clone()
        } != ColumnDesign {
            original: None,
            ..before.clone()
        }
    }

    pub fn is_empty(&self) -> bool {
        !self.renamed_table
            && self.dropped_columns.is_empty()
            && self.added_columns.is_empty()
            && self
                .kept_columns
                .iter()
                .all(|(old, new, _)| old.name == new.name && !Self::definition_changed(old, new))
            && !self.primary_key_changed
            && self.dropped_uniques.is_empty()
            && self.added_uniques.is_empty()
            && self.dropped_foreign_keys.is_empty()
            && self.added_foreign_keys.is_empty()
            && self.dropped_indexes.is_empty()
            && self.added_indexes.is_empty()
            && self.dropped_checks.is_empty()
            && self.added_checks.is_empty()
            && !self.comment_changed
            && self.changed_options.is_empty()
    }
}

/// One engine of the designer. Implementations render statements only; the
/// shared checks in [`plan`] run first.
pub trait Dialect: Sync {
    fn info(&self) -> DialectInfo;
    /// Quotes one identifier (always quoted, so reserved words and any
    /// characters are safe).
    fn quote(&self, name: &str) -> String;
    /// Quotes a string literal.
    fn literal(&self, text: &str) -> String;
    /// Engine rules beyond the shared ones.
    fn validate(&self, design: &TableDesign, issues: &mut Issues);
    fn create(&self, design: &TableDesign, issues: &mut Issues) -> Vec<DesignStatement>;
    fn alter(
        &self,
        before: &TableDesign,
        after: &TableDesign,
        changes: &TableChanges<'_>,
        issues: &mut Issues,
    ) -> Vec<DesignStatement>;
    /// Joins statements into the reviewed script.
    fn script(&self, statements: &[DesignStatement]) -> String {
        statements
            .iter()
            .map(|statement| format!("{};", statement.sql))
            .collect::<Vec<_>>()
            .join("\n\n")
    }
}

/// The designer of `engine`; None where tables cannot be designed (Redis).
pub fn dialect(engine: DbEngine) -> Option<&'static dyn Dialect> {
    match engine {
        DbEngine::Postgresql => Some(&postgres::Postgres),
        DbEngine::Mysql => Some(&mysql::MYSQL),
        DbEngine::Mariadb => Some(&mysql::MARIADB),
        DbEngine::Clickhouse => Some(&clickhouse::ClickHouse),
        DbEngine::Mongodb => Some(&mongo::Mongo),
        DbEngine::Sqlite => Some(&sqlite::Sqlite),
        DbEngine::Cassandra => Some(&cassandra::Cassandra),
        DbEngine::Redis | DbEngine::Unknown => None,
    }
}

/// Validates `design` and renders the statements that create it (`original`
/// None) or turn `original` into it.
pub fn plan(engine: DbEngine, original: Option<&TableDesign>, design: &TableDesign) -> DesignPlan {
    let mut issues = Issues::default();
    let Some(dialect) = dialect(engine) else {
        issues.error("engine_unsupported", "table", engine_name(engine));
        return DesignPlan {
            statements: Vec::new(),
            script: String::new(),
            issues: issues.0,
            transactional: false,
        };
    };
    let info = dialect.info();
    validate_common(&info, design, &mut issues);
    dialect.validate(design, &mut issues);
    let statements = if issues.has_errors() {
        Vec::new()
    } else {
        match original {
            None => dialect.create(design, &mut issues),
            Some(before) if !info.features.alter => {
                let _ = before;
                issues.error("alter_unsupported", "table", &design.name);
                Vec::new()
            }
            Some(before) => {
                let changes = TableChanges::between(before, design);
                if changes.is_empty() {
                    Vec::new()
                } else {
                    dialect.alter(before, design, &changes, &mut issues)
                }
            }
        }
    };
    let statements = if issues.has_errors() {
        Vec::new()
    } else {
        statements
    };
    DesignPlan {
        script: dialect.script(&statements),
        transactional: info.features.transactional && !statements.is_empty(),
        statements,
        issues: issues.0,
    }
}

fn engine_name(engine: DbEngine) -> &'static str {
    match engine {
        DbEngine::Postgresql => "postgresql",
        DbEngine::Mysql => "mysql",
        DbEngine::Mariadb => "mariadb",
        DbEngine::Clickhouse => "clickhouse",
        DbEngine::Mongodb => "mongodb",
        DbEngine::Redis => "redis",
        DbEngine::Sqlite => "sqlite",
        DbEngine::Cassandra => "cassandra",
        DbEngine::Unknown => "unknown",
    }
}

fn bad_text(text: &str) -> bool {
    text.len() > MAX_TEXT || text.contains('\0')
}

/// A name that can be quoted: not empty, no NUL, not too long.
fn check_name(
    info: &DialectInfo,
    name: &str,
    target: impl Into<String>,
    issues: &mut Issues,
) -> bool {
    let target = target.into();
    if name.trim().is_empty() {
        issues.error("name_required", target, "");
        return false;
    }
    if name.contains('\0') || name.chars().any(char::is_control) {
        issues.error("name_invalid", target, name);
        return false;
    }
    if name.len() > info.max_identifier {
        issues.error("name_too_long", target, name);
        return false;
    }
    if name != name.trim() {
        issues.warning("name_spaces", target, name);
    }
    true
}

/// Free text inserted into a statement as an expression (defaults, checks,
/// ClickHouse keys): one expression, never a statement separator or comment.
fn check_expression(text: &str, target: impl Into<String>, issues: &mut Issues) -> bool {
    let target = target.into();
    if bad_text(text) || text.contains(';') || text.contains("--") || text.contains("/*") {
        issues.error("expression_invalid", target, text);
        return false;
    }
    let mut depth = 0i32;
    let mut quote: Option<char> = None;
    for c in text.chars() {
        match quote {
            Some(q) if c == q => quote = None,
            Some(_) => {}
            None => match c {
                '\'' | '"' | '`' => quote = Some(c),
                '(' => depth += 1,
                ')' => {
                    depth -= 1;
                    if depth < 0 {
                        break;
                    }
                }
                _ => {}
            },
        }
    }
    if depth != 0 || quote.is_some() {
        issues.error("expression_invalid", target, text);
        return false;
    }
    true
}

/// A free-typed data type: a name with optional balanced parameters
/// (`Array(Nullable(String))`, `DateTime64(3, 'UTC')`, `enum('a','b')`).
fn check_type(text: &str, target: impl Into<String>, issues: &mut Issues) -> bool {
    let target = target.into();
    let valid = !text.trim().is_empty()
        && text.len() <= 512
        && text
            .chars()
            .next()
            .is_some_and(|c| c.is_ascii_alphabetic() || c == '_' || c == '"')
        && text.chars().all(|c| {
            c.is_ascii_alphanumeric()
                || " _(),'=.-\"[]<>".contains(c)
                || (!c.is_ascii() && !c.is_control())
        });
    if !valid || !check_expression(text, target.clone(), &mut Issues::default()) {
        issues.error("type_invalid", target, text);
        return false;
    }
    true
}

fn validate_common(info: &DialectInfo, design: &TableDesign, issues: &mut Issues) {
    check_name(info, &design.name, "table.name", issues);
    if info.features.schemas {
        if let Some(schema) = &design.schema {
            check_name(info, schema, "table.schema", issues);
        }
    }
    if bad_text(&design.comment) {
        issues.error("comment_invalid", "table.comment", "");
    }
    let too_many = [
        design.columns.len(),
        design.uniques.len(),
        design.foreign_keys.len(),
        design.indexes.len(),
        design.checks.len(),
        design.options.len(),
    ]
    .iter()
    .any(|count| *count > MAX_ITEMS);
    if too_many {
        issues.error("too_many_items", "table", &design.name);
        return;
    }
    if design.columns.is_empty() && !info.features.collection {
        issues.error("columns_required", "columns", "");
    }
    let fold = |name: &str| {
        if matches!(info.engine, DbEngine::Mysql | DbEngine::Mariadb) {
            name.to_lowercase()
        } else {
            name.to_owned()
        }
    };
    let mut names = BTreeSet::new();
    for (i, column) in design.columns.iter().enumerate() {
        let target = format!("columns.{i}");
        if check_name(info, &column.name, format!("{target}.name"), issues)
            && !names.insert(fold(&column.name))
        {
            issues.error("name_duplicate", format!("{target}.name"), &column.name);
        }
        if check_type(&column.data_type, format!("{target}.data_type"), issues) {
            let known = info
                .types
                .iter()
                .find(|t| t.name.eq_ignore_ascii_case(column.data_type.trim()));
            if let Some(known) = known {
                if known.length_required && column.length.is_none() {
                    issues.error("length_required", format!("{target}.length"), &column.name);
                }
                if known.params == TypeParams::None
                    && (column.length.is_some() || column.scale.is_some())
                {
                    issues.warning("length_ignored", format!("{target}.length"), &column.name);
                }
            }
        }
        if column.length == Some(0) {
            issues.error("length_invalid", format!("{target}.length"), &column.name);
        }
        if let (Some(scale), Some(length)) = (column.scale, column.length) {
            if scale > length {
                issues.error("scale_invalid", format!("{target}.scale"), &column.name);
            }
        }
        if bad_text(&column.default) {
            issues.error(
                "expression_invalid",
                format!("{target}.default"),
                &column.name,
            );
        } else if default_is_sql(&column.default) {
            check_expression(&column.default, format!("{target}.default"), issues);
        }
        if !column.identity.is_empty() && !info.identity_kinds.contains(&column.identity.as_str()) {
            issues.error(
                "identity_unsupported",
                format!("{target}.identity"),
                &column.name,
            );
        }
        if !column.identity.is_empty() && !column.default.trim().is_empty() {
            issues.error(
                "identity_default",
                format!("{target}.default"),
                &column.name,
            );
        }
        if bad_text(&column.comment) {
            issues.error("comment_invalid", format!("{target}.comment"), &column.name);
        }
        if column.array && !info.features.arrays {
            issues.error(
                "array_unsupported",
                format!("{target}.data_type"),
                &column.name,
            );
        }
        if column.unsigned && !info.features.unsigned {
            issues.warning(
                "unsigned_ignored",
                format!("{target}.unsigned"),
                &column.name,
            );
        }
    }
    let known = |column: &str| names.contains(&fold(column));
    let mut key_names = BTreeSet::new();
    let mut check_key_name = |name: &str, target: String, issues: &mut Issues| {
        if !name.is_empty()
            && check_name(info, name, target.clone(), issues)
            && !key_names.insert(fold(name))
        {
            issues.error("name_duplicate", target, name);
        }
    };
    let check_columns = |columns: &[String], target: String, issues: &mut Issues| {
        if columns.is_empty() {
            issues.error("key_columns_required", target, "");
            return;
        }
        let mut seen = BTreeSet::new();
        for column in columns {
            if !known(column) {
                issues.error("column_unknown", target.clone(), column);
            } else if !seen.insert(fold(column)) {
                issues.error("column_repeated", target.clone(), column);
            }
        }
    };
    if !design.primary_key.columns.is_empty() {
        if !info.features.primary_key {
            issues.error("primary_key_unsupported", "primary_key", "");
        }
        check_key_name(&design.primary_key.name, "primary_key.name".into(), issues);
        check_columns(
            &design.primary_key.columns,
            "primary_key.columns".into(),
            issues,
        );
    }
    for (i, key) in design.uniques.iter().enumerate() {
        if !info.features.unique {
            issues.error("unique_unsupported", format!("uniques.{i}"), &key.name);
        }
        check_key_name(&key.name, format!("uniques.{i}.name"), issues);
        check_columns(&key.columns, format!("uniques.{i}.columns"), issues);
    }
    for (i, fk) in design.foreign_keys.iter().enumerate() {
        let target = format!("foreign_keys.{i}");
        if !info.features.foreign_keys {
            issues.error("foreign_keys_unsupported", target.clone(), &fk.name);
        }
        check_key_name(&fk.name, format!("{target}.name"), issues);
        check_columns(&fk.columns, format!("{target}.columns"), issues);
        if fk.ref_table.trim().is_empty() {
            issues.error(
                "ref_table_required",
                format!("{target}.ref_table"),
                &fk.name,
            );
        } else {
            check_name(info, &fk.ref_table, format!("{target}.ref_table"), issues);
        }
        if let Some(schema) = &fk.ref_schema {
            if !schema.is_empty() {
                check_name(info, schema, format!("{target}.ref_schema"), issues);
            }
        }
        if fk.ref_columns.len() != fk.columns.len() || fk.ref_columns.is_empty() {
            issues.error(
                "ref_columns_mismatch",
                format!("{target}.ref_columns"),
                &fk.name,
            );
        }
        for column in &fk.ref_columns {
            check_name(info, column, format!("{target}.ref_columns"), issues);
        }
        for action in [&fk.on_delete, &fk.on_update] {
            if !action.is_empty() && !info.fk_actions.contains(&action.as_str()) {
                issues.error("fk_action_invalid", format!("{target}.on_delete"), action);
            }
        }
    }
    for (i, index) in design.indexes.iter().enumerate() {
        let target = format!("indexes.{i}");
        if !info.features.indexes {
            issues.error("indexes_unsupported", target.clone(), &index.name);
        }
        check_key_name(&index.name, format!("{target}.name"), issues);
        check_columns(&index.columns, format!("{target}.columns"), issues);
        if !index.method.is_empty() && !info.index_methods.contains(&index.method.as_str()) {
            issues.error(
                "index_method_invalid",
                format!("{target}.method"),
                &index.method,
            );
        }
    }
    for (i, check) in design.checks.iter().enumerate() {
        let target = format!("checks.{i}");
        if !info.features.checks {
            issues.error("checks_unsupported", target.clone(), &check.name);
        }
        check_key_name(&check.name, format!("{target}.name"), issues);
        if check.expression.trim().is_empty() {
            issues.error(
                "expression_required",
                format!("{target}.expression"),
                &check.name,
            );
        } else {
            check_expression(&check.expression, format!("{target}.expression"), issues);
        }
    }
    for (id, value) in &design.options {
        let target = format!("options.{id}");
        match info.options.iter().find(|option| option.id == id) {
            None => issues.error("option_unknown", target, id),
            Some(option) => match option.kind {
                OptionKind::Bool if !matches!(value.as_str(), "" | "true" | "false") => {
                    issues.error("option_invalid", target, value)
                }
                OptionKind::Choice
                    if !value.is_empty() && !option.choices.contains(&value.as_str()) =>
                {
                    issues.error("option_invalid", target, value)
                }
                OptionKind::Number if !value.is_empty() && value.parse::<u64>().is_err() => {
                    issues.error("option_invalid", target, value)
                }
                OptionKind::Text if !value.is_empty() => {
                    check_expression(value, target, issues);
                }
                _ => {}
            },
        }
    }
}

/// The value of option `id`, or its default.
fn option<'a>(info: &'a DialectInfo, design: &'a TableDesign, id: &str) -> &'a str {
    design
        .options
        .get(id)
        .map(String::as_str)
        .filter(|value| !value.is_empty())
        .or_else(|| {
            info.options
                .iter()
                .find(|option| option.id == id)
                .map(|option| option.default)
        })
        .unwrap_or("")
}

/// Keywords and literals a default may be written as without quotes.
const DEFAULT_KEYWORDS: [&str; 10] = [
    "null",
    "true",
    "false",
    "current_timestamp",
    "current_date",
    "current_time",
    "localtimestamp",
    "localtime",
    "current_user",
    "session_user",
];

/// Whether a default is written as SQL: numbers, keywords, quoted strings,
/// casts and calls. Anything else is a plain value.
fn default_is_sql(text: &str) -> bool {
    let text = text.trim();
    let lower = text.to_ascii_lowercase();
    let numeric = text
        .strip_prefix('-')
        .unwrap_or(text)
        .chars()
        .all(|c| c.is_ascii_digit() || c == '.')
        && text.chars().any(|c| c.is_ascii_digit());
    numeric
        || text.starts_with('\'')
        || text.starts_with('(')
        || text.starts_with("E'")
        || text.contains('(')
        || text.contains("::")
        || DEFAULT_KEYWORDS.contains(&lower.as_str())
        || lower.starts_with("current_timestamp")
        || lower.starts_with("interval ")
        || lower.starts_with("array[")
        || text.starts_with('[')
        || text.starts_with('{')
}

/// The default as SQL: SQL stays as written; any other text becomes a
/// string literal (`active` → `'active'`), so typing a plain value does
/// what it looks like.
fn default_sql(dialect: &dyn Dialect, text: &str) -> String {
    if default_is_sql(text) {
        text.trim().to_owned()
    } else {
        dialect.literal(text.trim())
    }
}

/// `type(len[, scale])[]` from the column fields.
fn column_type(column: &ColumnDesign) -> String {
    let mut text = column.data_type.trim().to_owned();
    if !text.contains('(') {
        match (column.length, column.scale) {
            (Some(length), Some(scale)) => text.push_str(&format!("({length},{scale})")),
            (Some(length), None) => text.push_str(&format!("({length})")),
            _ => {}
        }
    }
    text
}

/// Generated name for an unnamed key or index: `table_col1_col2_suffix`,
/// cut to the identifier limit.
fn key_name(info: &DialectInfo, table: &str, columns: &[String], suffix: &str) -> String {
    let mut name = table.to_owned();
    for column in columns {
        name.push('_');
        name.push_str(column);
    }
    name.push('_');
    name.push_str(suffix);
    if name.len() > info.max_identifier {
        let keep = info.max_identifier.saturating_sub(suffix.len() + 1);
        let mut cut = name.chars().take(keep).collect::<String>();
        while cut.len() > keep {
            cut.pop();
        }
        name = format!("{cut}_{suffix}");
    }
    name
}

fn name_or(
    info: &DialectInfo,
    name: &str,
    table: &str,
    columns: &[String],
    suffix: &str,
) -> String {
    if name.trim().is_empty() {
        key_name(info, table, columns, suffix)
    } else {
        name.to_owned()
    }
}

fn quote_list(dialect: &dyn Dialect, names: &[String]) -> String {
    names
        .iter()
        .map(|name| dialect.quote(name))
        .collect::<Vec<_>>()
        .join(", ")
}

fn statement(sql: String) -> DesignStatement {
    DesignStatement {
        sql,
        destructive: false,
    }
}

fn destructive(sql: String) -> DesignStatement {
    DesignStatement {
        sql,
        destructive: true,
    }
}

#[cfg(test)]
pub(crate) mod test_support {
    use super::*;
    use crate::types::DbQueryRequest;

    /// Every statement passes the session guard's parser.
    pub fn assert_guard_accepts(engine: DbEngine, plan: &DesignPlan) {
        assert!(!plan.has_errors(), "{:?}", plan.issues);
        for statement in &plan.statements {
            let request = DbQueryRequest {
                text: statement.sql.clone(),
                parameters: vec![],
                approval_ticket: None,
            };
            assert!(
                crate::parser::parse(engine, "public", &request).is_ok(),
                "guard refused: {}",
                statement.sql
            );
        }
        let request = DbQueryRequest {
            text: plan.script.clone(),
            parameters: vec![],
            approval_ticket: None,
        };
        let parsed = crate::parser::parse(engine, "public", &request).expect("script parses");
        assert_eq!(parsed.len(), plan.statements.len());
    }

    pub fn column(name: &str, data_type: &str) -> ColumnDesign {
        ColumnDesign {
            name: name.into(),
            data_type: data_type.into(),
            nullable: true,
            ..Default::default()
        }
    }

    /// The design as loaded from the server: every item has its original name.
    pub fn loaded(mut design: TableDesign) -> TableDesign {
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
        design
    }
}

#[cfg(test)]
mod tests {
    use super::test_support::column;
    use super::*;

    #[test]
    fn shared_validation_reports_each_problem_once() {
        let design = TableDesign {
            name: String::new(),
            columns: vec![
                column("id", "integer"),
                column("id", "integer"),
                column("name", "varchar; DROP TABLE x"),
                ColumnDesign {
                    default: "now(); DROP TABLE x".into(),
                    ..column("d", "text")
                },
            ],
            primary_key: KeyDesign {
                columns: vec!["missing".into()],
                ..Default::default()
            },
            ..Default::default()
        };
        let plan = plan(DbEngine::Postgresql, None, &design);
        let codes: Vec<_> = plan
            .issues
            .iter()
            .map(|i| (i.code, i.target.as_str()))
            .collect();
        assert!(codes.contains(&("name_required", "table.name")));
        assert!(codes.contains(&("name_duplicate", "columns.1.name")));
        assert!(codes.contains(&("type_invalid", "columns.2.data_type")));
        assert!(codes.contains(&("expression_invalid", "columns.3.default")));
        assert!(codes.contains(&("column_unknown", "primary_key.columns")));
        assert!(plan.statements.is_empty() && plan.script.is_empty());
    }

    #[test]
    fn plain_defaults_become_literals() {
        let pg = dialect(DbEngine::Postgresql).unwrap();
        for (text, sql) in [
            ("0", "0"),
            ("-1.5", "-1.5"),
            ("now()", "now()"),
            ("CURRENT_TIMESTAMP", "CURRENT_TIMESTAMP"),
            ("null", "null"),
            ("'x'", "'x'"),
            ("'a'::text", "'a'::text"),
            ("active", "'active'"),
            ("it's", "'it''s'"),
        ] {
            assert_eq!(default_sql(pg, text), sql, "{text}");
        }
    }

    #[test]
    fn generated_key_names_fit_the_identifier_limit() {
        let info = dialect(DbEngine::Postgresql).unwrap().info();
        let long = "x".repeat(80);
        let name = key_name(&info, &long, &["id".into()], "key");
        assert!(name.len() <= 63 && name.ends_with("_key"), "{name}");
        assert_eq!(
            key_name(&info, "t", &["a".into(), "b".into()], "idx"),
            "t_a_b_idx"
        );
    }

    #[test]
    fn unchanged_designs_plan_nothing() {
        let design = test_support::loaded(TableDesign {
            schema: Some("public".into()),
            name: "t".into(),
            columns: vec![column("id", "integer")],
            ..Default::default()
        });
        let plan = plan(DbEngine::Postgresql, Some(&design), &design);
        assert!(plan.statements.is_empty() && !plan.has_errors(), "{plan:?}");
    }

    #[test]
    fn redis_has_no_designer() {
        assert!(dialect(DbEngine::Redis).is_none());
        let plan = plan(DbEngine::Redis, None, &TableDesign::default());
        assert_eq!(plan.issues[0].code, "engine_unsupported");
    }
}
