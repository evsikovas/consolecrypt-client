//! SQLite structure, DDL, plans and object details. Catalogue reads are fixed
//! statements over `sqlite_master` and the table-valued PRAGMA functions
//! (SQLite 3.16+, so the remote `sqlite3` route reads them too); names travel
//! as bound values. Each listing is one statement, because the remote route
//! starts the program once per statement.
use crate::{
    details::{DetailKind, ObjectDetails},
    session::{Collect, DbSession, Driver},
    DbCell, DbError, DbObjectKind, DbObjectRef, DbParameter, DbQueryRequest, DbTreeNode,
    DbTreePage,
};
use serde_json::{json, Value};

/// The schema name of the opened file.
pub(crate) const MAIN: &str = "main";

/// Storage classes and the declared types that get each affinity.
pub const SQLITE_TYPES: [(&str, &str); 5] = [
    (
        "INTEGER",
        "INT, INTEGER, TINYINT, SMALLINT, MEDIUMINT, BIGINT, INT8",
    ),
    ("TEXT", "TEXT, CHAR(n), VARCHAR(n), NCHAR(n), CLOB"),
    ("REAL", "REAL, DOUBLE, DOUBLE PRECISION, FLOAT"),
    ("NUMERIC", "NUMERIC, DECIMAL(p,s), BOOLEAN, DATE, DATETIME"),
    ("BLOB", "BLOB"),
];

fn text(value: &str) -> DbParameter {
    DbParameter {
        name: String::new(),
        value: DbCell::Text(value.into()),
        type_hint: None,
    }
}

fn reference(kind: DbObjectKind, name: &str, parent: Option<&str>) -> DbObjectRef {
    DbObjectRef {
        database: MAIN.into(),
        schema: None,
        kind,
        name: name.into(),
        parent_name: parent.map(str::to_owned),
    }
}

fn leaf(reference: DbObjectRef, detail: &str) -> DbTreeNode {
    DbTreeNode {
        reference,
        has_children: false,
        detail: (!detail.is_empty()).then(|| detail.to_owned()),
        nullable: None,
        default_value: None,
        primary_key: false,
        foreign_key: None,
    }
}

/// `STRICT` and `WITHOUT ROWID` from the table options after the last `)`.
pub(crate) fn table_options(sql: &str) -> (bool, bool) {
    let tail = sql
        .rfind(')')
        .map_or("", |index| &sql[index + 1..])
        .to_ascii_uppercase();
    let words: Vec<&str> = tail
        .split(|c: char| c.is_whitespace() || c == ',' || c == ';')
        .filter(|word| !word.is_empty())
        .collect();
    let strict = words.contains(&"STRICT");
    let without_rowid = words.windows(2).any(|pair| pair == ["WITHOUT", "ROWID"]);
    (strict, without_rowid)
}

/// `BEFORE UPDATE`, `AFTER INSERT`, `INSTEAD OF DELETE` from a trigger's SQL.
fn trigger_event(sql: &str) -> String {
    let upper = sql.to_ascii_uppercase();
    let words: Vec<&str> = upper.split_whitespace().collect();
    let timing = if words.contains(&"BEFORE") {
        "BEFORE"
    } else if words.windows(2).any(|pair| pair == ["INSTEAD", "OF"]) {
        "INSTEAD OF"
    } else {
        "AFTER"
    };
    let event = words
        .iter()
        .find(|word| matches!(**word, "INSERT" | "UPDATE" | "DELETE"))
        .copied()
        .unwrap_or("");
    format!("{timing} {event}").trim().to_owned()
}

const TABLE_CHILDREN: &str = "SELECT * FROM ( \
    SELECT 'column' AS kind, c.name AS name, c.type AS detail, \
      CASE WHEN c.\"notnull\" THEN 'false' ELSE 'true' END AS nullable, \
      COALESCE(c.dflt_value, '') AS dflt, CASE WHEN c.pk > 0 THEN 'true' ELSE 'false' END AS pk, \
      COALESCE((SELECT f.\"table\" FROM pragma_foreign_key_list(?1) f WHERE f.\"from\" = c.name LIMIT 1), '') AS target, \
      0 AS grp, c.cid AS ord FROM pragma_table_info(?1) c \
    UNION ALL SELECT * FROM (SELECT 'key', 'PRIMARY KEY', '(' || group_concat(name, ', ') || ')' AS detail, \
      '', '', 'true', '', 1, -1 FROM (SELECT name FROM pragma_table_info(?1) WHERE pk > 0 ORDER BY pk)) WHERE detail IS NOT NULL \
    UNION ALL SELECT 'key', il.name, 'UNIQUE (' || COALESCE((SELECT group_concat(name, ', ') \
      FROM (SELECT name FROM pragma_index_info(il.name) ORDER BY seqno)), '') || ')', '', '', 'false', '', 1, il.seq \
      FROM pragma_index_list(?1) il WHERE il.origin = 'u' \
    UNION ALL SELECT 'foreign_key', group_concat(f.\"from\", ', '), \
      '→ ' || f.\"table\" || '(' || group_concat(COALESCE(f.\"to\", ''), ', ') || ')' \
      || CASE WHEN f.on_delete <> 'NO ACTION' THEN ' ON DELETE ' || f.on_delete ELSE '' END \
      || CASE WHEN f.on_update <> 'NO ACTION' THEN ' ON UPDATE ' || f.on_update ELSE '' END, \
      '', '', 'false', f.\"table\", 2, f.id FROM pragma_foreign_key_list(?1) f GROUP BY f.id \
    UNION ALL SELECT 'index', il.name, CASE WHEN il.\"unique\" THEN 'UNIQUE ' ELSE '' END || '(' \
      || COALESCE((SELECT group_concat(name, ', ') FROM (SELECT name FROM pragma_index_info(il.name) ORDER BY seqno)), '') \
      || ')' || CASE WHEN il.partial THEN ' WHERE …' ELSE '' END, '', '', \
      CASE WHEN il.origin = 'pk' THEN 'true' ELSE 'false' END, '', 3, il.seq FROM pragma_index_list(?1) il \
    UNION ALL SELECT 'reference', m.name, m.name || '(' || group_concat(f.\"from\", ', ') || ') → (' \
      || group_concat(COALESCE(f.\"to\", ''), ', ') || ')', '', '', 'false', m.name, 4, f.id \
      FROM sqlite_master m JOIN pragma_foreign_key_list(m.name) f \
      WHERE m.type = 'table' AND f.\"table\" = ?1 COLLATE NOCASE GROUP BY m.name, f.id \
    UNION ALL SELECT 'trigger', name, COALESCE(sql, ''), '', '', 'false', '', 5, 0 \
      FROM sqlite_master WHERE type = 'trigger' AND tbl_name = ?1 \
  ) ORDER BY grp, ord, name";

impl DbSession {
    async fn sqlite_rows(
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

    /// The file → tables, views, indexes, sequences, triggers and storage
    /// classes; a table → columns, keys, foreign keys, indexes, references
    /// from other tables and triggers; a view → its columns.
    pub(crate) async fn sqlite_tree(
        &self,
        parent: Option<&DbObjectRef>,
    ) -> Result<DbTreePage, DbError> {
        let nodes = match parent.map(|parent| parent.kind) {
            None => {
                let name = self
                    .driver
                    .lock()
                    .await
                    .sqlite_file_name()
                    .unwrap_or(MAIN)
                    .to_owned();
                vec![DbTreeNode {
                    reference: reference(DbObjectKind::Database, &name, None),
                    has_children: true,
                    detail: Some(format!("SQLite {}", self.info.server_version)),
                    nullable: None,
                    default_value: None,
                    primary_key: false,
                    foreign_key: None,
                }]
            }
            Some(DbObjectKind::Database) => self.sqlite_database_children().await?,
            Some(DbObjectKind::Table | DbObjectKind::View) => {
                let parent = parent.expect("matched above");
                self.sqlite_table_children(parent).await?
            }
            _ => Vec::new(),
        };
        Ok(DbTreePage {
            nodes,
            next_cursor: None,
        })
    }

    async fn sqlite_database_children(&self) -> Result<Vec<DbTreeNode>, DbError> {
        let rows = self
            .sqlite_rows(
                "SELECT type, name, tbl_name, COALESCE(sql, '') FROM sqlite_master \
                 WHERE type IN ('table', 'view', 'index', 'trigger') \
                 AND (type = 'index' OR name NOT LIKE 'sqlite\\_%' ESCAPE '\\') \
                 UNION ALL SELECT 'sequences', name, '', '' FROM sqlite_master \
                 WHERE type = 'table' AND name = 'sqlite_sequence' ORDER BY 1, 2",
                &[],
            )
            .await?;
        let mut nodes = Vec::new();
        let mut sequences = false;
        for row in &rows {
            let [kind, name, table, sql, ..] = row.as_slice() else {
                continue;
            };
            match kind.as_str() {
                "table" | "view" => {
                    let table_kind = if kind == "view" {
                        DbObjectKind::View
                    } else {
                        DbObjectKind::Table
                    };
                    let (strict, without_rowid) = table_options(sql);
                    let detail = [
                        strict.then_some("STRICT"),
                        without_rowid.then_some("WITHOUT ROWID"),
                    ]
                    .into_iter()
                    .flatten()
                    .collect::<Vec<_>>()
                    .join(", ");
                    let mut node = leaf(reference(table_kind, name, None), &detail);
                    node.has_children = true;
                    nodes.push(node);
                }
                "index" => nodes.push(leaf(
                    reference(DbObjectKind::Index, name, Some(table)),
                    table,
                )),
                "trigger" => nodes.push(leaf(
                    reference(DbObjectKind::Trigger, name, Some(table)),
                    &format!("{table} · {}", trigger_event(sql)),
                )),
                "sequences" => sequences = true,
                _ => {}
            }
        }
        if sequences {
            for row in self
                .sqlite_rows(
                    "SELECT name, CAST(seq AS TEXT) FROM sqlite_sequence ORDER BY name",
                    &[],
                )
                .await?
            {
                if let [name, value, ..] = row.as_slice() {
                    nodes.push(leaf(reference(DbObjectKind::Sequence, name, None), value));
                }
            }
        }
        for (name, detail) in SQLITE_TYPES {
            nodes.push(leaf(reference(DbObjectKind::DataType, name, None), detail));
        }
        Ok(nodes)
    }

    async fn sqlite_table_children(&self, table: &DbObjectRef) -> Result<Vec<DbTreeNode>, DbError> {
        let rows = self
            .sqlite_rows(TABLE_CHILDREN, &[text(&table.name)])
            .await?;
        let mut nodes = Vec::new();
        for row in rows {
            let [kind, name, detail, nullable, default, primary, target, ..] = row.as_slice()
            else {
                continue;
            };
            let object_kind = match kind.as_str() {
                "column" => DbObjectKind::Column,
                "key" => DbObjectKind::Key,
                "foreign_key" => DbObjectKind::ForeignKey,
                "index" => DbObjectKind::Index,
                "reference" => DbObjectKind::Reference,
                "trigger" => DbObjectKind::Trigger,
                _ => continue,
            };
            let detail = if object_kind == DbObjectKind::Trigger {
                trigger_event(detail)
            } else {
                detail.clone()
            };
            let mut node = leaf(reference(object_kind, name, Some(&table.name)), &detail);
            if object_kind == DbObjectKind::Column {
                node.nullable = Some(nullable == "true");
                node.default_value = (!default.is_empty()).then(|| default.clone());
            }
            node.primary_key = primary == "true";
            if !target.is_empty() {
                node.foreign_key = Some(reference(DbObjectKind::Table, target, None));
            }
            nodes.push(node);
        }
        Ok(nodes)
    }

    /// `CREATE` statements from `sqlite_master`; a table comes with its
    /// indexes and triggers.
    pub(crate) async fn sqlite_ddl(&self, object: &DbObjectRef) -> Result<String, DbError> {
        let (sql, name) = match object.kind {
            DbObjectKind::Table => (
                "SELECT sql FROM sqlite_master WHERE tbl_name = ?1 AND sql IS NOT NULL \
                 ORDER BY CASE type WHEN 'table' THEN 0 WHEN 'index' THEN 1 ELSE 2 END, name",
                object.name.as_str(),
            ),
            // A sequence is the AUTOINCREMENT counter of its table.
            DbObjectKind::Sequence => (
                "SELECT sql FROM sqlite_master WHERE type = 'table' AND name = ?1",
                object.name.as_str(),
            ),
            DbObjectKind::View | DbObjectKind::Index | DbObjectKind::Trigger => (
                "SELECT sql FROM sqlite_master WHERE name = ?1 AND sql IS NOT NULL",
                object.name.as_str(),
            ),
            _ => return Err(DbError::InvalidRequest),
        };
        let statements: Vec<String> = self
            .sqlite_rows(sql, &[text(name)])
            .await?
            .into_iter()
            .filter_map(|row| row.into_iter().next())
            .filter(|sql| !sql.is_empty())
            .map(|sql| format!("{};", sql.trim_end().trim_end_matches(';')))
            .collect();
        if statements.is_empty() {
            return Err(DbError::QueryFailed);
        }
        Ok(statements.join("\n\n"))
    }

    /// Primary-key columns in key order; `rowid` for a rowid table without
    /// one; empty for a view or a WITHOUT ROWID table without a key.
    pub(crate) async fn sqlite_primary_key(
        &self,
        table: &DbObjectRef,
    ) -> Result<Vec<String>, DbError> {
        let rows = self
            .sqlite_rows(
                "SELECT 'key', name FROM (SELECT name FROM pragma_table_info(?1) WHERE pk > 0 ORDER BY pk) \
                 UNION ALL SELECT type, COALESCE(sql, '') FROM sqlite_master \
                 WHERE type IN ('table', 'view') AND name = ?1",
                &[text(&table.name)],
            )
            .await?;
        let key: Vec<String> = rows
            .iter()
            .filter(|row| row.first().is_some_and(|kind| kind == "key"))
            .filter_map(|row| row.get(1).cloned())
            .collect();
        if !key.is_empty() {
            return Ok(key);
        }
        let rowid_table = rows.iter().any(|row| {
            row.first().is_some_and(|kind| kind == "table")
                && !table_options(row.get(1).map_or("", String::as_str)).1
        });
        Ok(if rowid_table {
            vec!["rowid".into()]
        } else {
            Vec::new()
        })
    }

    /// `EXPLAIN QUERY PLAN` as a tree: `{"sqlite_plan": [{detail, children}]}`.
    /// The statement passes the guard like any read.
    pub(crate) async fn sqlite_explain(
        &self,
        statement: &str,
        request: &DbQueryRequest,
    ) -> Result<Value, DbError> {
        let explain = DbQueryRequest {
            text: format!("EXPLAIN QUERY PLAN {statement}"),
            parameters: request.parameters.clone(),
            approval_ticket: None,
        };
        let permits = self.guard.prepare(&explain)?;
        let [permit] = permits.as_slice() else {
            return Err(DbError::InvalidRequest);
        };
        if !permit.is_read_only() {
            return Err(DbError::PolicyDenied);
        }
        self.guard.validate_permit(permit)?;
        let crate::parser::ParsedStatement::Sql { text, parameters } = permit.parsed() else {
            return Err(DbError::InvalidRequest);
        };
        let mut driver = self.driver.lock().await;
        let steps = match &mut *driver {
            Driver::Sqlite(_) => {
                let mut rows = Collect::default();
                driver
                    .query(text, parameters, true, &mut rows, self.scope.child_token())
                    .await?;
                plan_from_rows(&rows.rows)
            }
            Driver::SqliteCli(cli) => {
                let output = cli.raw(text, parameters, self.scope.child_token()).await?;
                plan_from_text(&String::from_utf8_lossy(&output))
            }
            _ => return Err(DbError::InvalidRequest),
        };
        Ok(json!({
            "engine": self.info.engine,
            "analyzed": false,
            "plan": {"sqlite_plan": steps},
        }))
    }

    /// Size, page and mode facts of the file, or of one table, view or index
    /// (row estimate from `sqlite_stat1` when ANALYZE ran).
    pub(crate) async fn sqlite_details(
        &self,
        object: &DbObjectRef,
    ) -> Result<ObjectDetails, DbError> {
        let file = self
            .sqlite_rows(
                "SELECT (SELECT page_size FROM pragma_page_size), (SELECT page_count FROM pragma_page_count), \
                 (SELECT freelist_count FROM pragma_freelist_count), \
                 (SELECT count(*) FROM sqlite_master WHERE type = 'table' AND name = 'sqlite_stat1')",
                &[],
            )
            .await?;
        let first = file.into_iter().next().unwrap_or_default();
        let value = |index: usize| first.get(index).cloned().unwrap_or_default();
        let number = |text: &str| text.parse::<i64>().ok();
        let (page_size, page_count) = (value(0), value(1));
        let stats = value(3) == "1";
        let yes_no = |flag: bool| if flag { "yes" } else { "no" };
        let mut details = ObjectDetails::default();
        if object.kind == DbObjectKind::Database {
            let size = number(&page_size)
                .zip(number(&page_count))
                .map(|(size, count)| (size * count).to_string())
                .unwrap_or_default();
            details
                .section("general")
                .field("engine", "SQLite", DetailKind::Code)
                .field("version", &self.info.server_version, DetailKind::Code);
            details
                .section("size")
                .field("total_size", &size, DetailKind::Bytes)
                .field("page_size", &page_size, DetailKind::Bytes)
                .field("page_count", &page_count, DetailKind::Count)
                .field("freelist_count", &value(2), DetailKind::Count);
            return Ok(details.done());
        }
        let rows = self
            .sqlite_rows(
                "SELECT type, tbl_name, COALESCE(sql, ''), \
                 (SELECT count(*) FROM pragma_table_info(?1)), \
                 (SELECT count(*) FROM pragma_index_list(?1)), \
                 (SELECT count(*) FROM pragma_foreign_key_list(?1)), \
                 (SELECT count(*) FROM sqlite_master WHERE type = 'trigger' AND tbl_name = ?1) \
                 FROM sqlite_master WHERE name = ?1",
                &[text(&object.name)],
            )
            .await?;
        let row = rows.into_iter().next().ok_or(DbError::QueryFailed)?;
        let cell = |index: usize| row.get(index).cloned().unwrap_or_default();
        let (kind, table, sql) = (cell(0), cell(1), cell(2));
        let (strict, without_rowid) = table_options(&sql);
        // sqlite_stat1 (after ANALYZE) holds the row count estimate.
        let estimate = if stats && kind != "view" {
            self.sqlite_rows(
                "SELECT stat FROM sqlite_stat1 WHERE tbl = ?1 AND (?2 = '' OR idx = ?2) \
                 ORDER BY idx IS NULL DESC LIMIT 1",
                &[
                    text(&table),
                    text(if kind == "index" { &object.name } else { "" }),
                ],
            )
            .await?
            .first()
            .and_then(|row| row.first())
            .and_then(|stat| stat.split_whitespace().next().map(str::to_owned))
            .unwrap_or_default()
        } else {
            String::new()
        };
        details
            .section("general")
            .field("kind", &kind, DetailKind::Keyword);
        if kind == "index" {
            details.field("table", &table, DetailKind::Code);
        }
        if kind == "table" {
            details
                .field("strict", yes_no(strict), DetailKind::Keyword)
                .field("without_rowid", yes_no(without_rowid), DetailKind::Keyword)
                .field(
                    "autoincrement",
                    yes_no(sql.to_ascii_uppercase().contains("AUTOINCREMENT")),
                    DetailKind::Keyword,
                );
        }
        details
            .section("size")
            .field("rows", &estimate, DetailKind::Estimate);
        if kind != "index" {
            details
                .section("structure")
                .field("columns", &cell(3), DetailKind::Count);
        }
        if kind == "table" {
            details
                .field("indexes", &cell(4), DetailKind::Count)
                .field("foreign_keys", &cell(5), DetailKind::Count)
                .field("triggers", &cell(6), DetailKind::Count);
        }
        details.field_with_unit(
            "definition_length",
            &sql.chars().count().to_string(),
            DetailKind::Count,
            Some("characters"),
        );
        Ok(details.done())
    }
}

/// `EXPLAIN QUERY PLAN` rows (id, parent, notused, detail) as nested steps.
pub(crate) fn plan_from_rows(rows: &[Vec<String>]) -> Vec<Value> {
    fn children(rows: &[Vec<String>], parent: &str, depth: usize) -> Vec<Value> {
        if depth > 64 {
            return Vec::new();
        }
        rows.iter()
            .filter(|row| row.get(1).map(String::as_str) == Some(parent))
            .map(|row| {
                json!({
                    "detail": row.get(3).cloned().unwrap_or_default(),
                    "children": children(rows, &row[0], depth + 1),
                })
            })
            .collect()
    }
    children(rows, "0", 0)
}

/// The `sqlite3` program's plan drawing (`|--SCAN t`, `` `--SEARCH … ``) as
/// nested steps.
pub(crate) fn plan_from_text(text: &str) -> Vec<Value> {
    #[derive(Default)]
    struct Step {
        detail: String,
        children: Vec<Step>,
    }
    fn value(step: Step) -> Value {
        json!({
            "detail": step.detail,
            "children": step.children.into_iter().map(value).collect::<Vec<_>>(),
        })
    }
    let mut root = Step::default();
    // Path of child indexes from the root to the last step at each depth.
    let mut path: Vec<usize> = Vec::new();
    for line in text.lines() {
        let Some(marker) = line.find("--") else {
            continue;
        };
        if marker == 0 || !matches!(&line[marker - 1..marker], "|" | "`") {
            continue;
        }
        let depth = (marker - 1) / 3;
        let detail = line[marker + 2..].trim().to_owned();
        path.truncate(depth);
        let mut node = &mut root;
        for &index in &path {
            node = &mut node.children[index];
        }
        node.children.push(Step {
            detail,
            children: Vec::new(),
        });
        path.push(node.children.len() - 1);
    }
    root.children.into_iter().map(value).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn table_options_and_trigger_events_read_from_sql() {
        assert_eq!(
            table_options("CREATE TABLE t (a INTEGER PRIMARY KEY) STRICT, WITHOUT ROWID"),
            (true, true)
        );
        assert_eq!(
            table_options("CREATE TABLE t (a, b TEXT CHECK (b <> ')'))"),
            (false, false)
        );
        assert_eq!(
            trigger_event("CREATE TRIGGER x BEFORE UPDATE OF a ON t BEGIN SELECT 1; END"),
            "BEFORE UPDATE"
        );
        assert_eq!(
            trigger_event("CREATE TRIGGER x INSTEAD OF DELETE ON v BEGIN SELECT 1; END"),
            "INSTEAD OF DELETE"
        );
    }

    #[test]
    fn plans_nest_from_rows_and_from_the_program_drawing() {
        let rows = vec![
            vec!["2".into(), "0".into(), "0".into(), "SCAN a".into()],
            vec!["5".into(), "0".into(), "0".into(), "LIST SUBQUERY 1".into()],
            vec!["7".into(), "5".into(), "0".into(), "SCAN t".into()],
        ];
        let from_rows = plan_from_rows(&rows);
        assert_eq!(from_rows.len(), 2);
        assert_eq!(from_rows[1]["children"][0]["detail"], "SCAN t");
        let drawing = "QUERY PLAN\n|--SCAN a\n|--LIST SUBQUERY 1\n|  |--SCAN t\n|  `--CREATE BLOOM FILTER\n`--SEARCH b USING INDEX i (a=?)\n";
        let from_text = plan_from_text(drawing);
        assert_eq!(from_text.len(), 3);
        assert_eq!(from_text[1]["detail"], "LIST SUBQUERY 1");
        assert_eq!(from_text[1]["children"].as_array().unwrap().len(), 2);
        assert_eq!(from_text[2]["detail"], "SEARCH b USING INDEX i (a=?)");
    }
}
