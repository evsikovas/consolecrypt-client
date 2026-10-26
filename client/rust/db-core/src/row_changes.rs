//! Pending row changes of an editable single-table result: changed cells,
//! added rows and deleted rows, kept in the client until the user saves.
//! Saving turns them into parameterized `DELETE`, `UPDATE` and `INSERT`
//! statements (in that order, so a re-added key does not collide with the
//! row it replaces), identified by the table's primary key — or a unique key
//! of NOT NULL columns when there is no primary key. The statements are
//! reviewed like any write, applied in one transaction and each must change
//! exactly one row. The review shows them with the values as SQL literals;
//! that text never runs.
use crate::{
    edit::{preview_literal, MAX_CELL_EDITS},
    introspect::quote_identifier,
    session::{Collect, DbSession},
    DbCell, DbError, DbObjectKind, DbObjectRef, DbParameter, DbQueryRequest,
};
use cc_models::database::DbEngine;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// One pending change of a row. Keys are the row's key values as loaded.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum DbRowChange {
    /// Changed cells of a loaded row.
    Update {
        original_primary_key: BTreeMap<String, DbCell>,
        values: BTreeMap<String, DbCell>,
    },
    /// A new row; columns left out take their default (or NULL).
    Insert { values: BTreeMap<String, DbCell> },
    /// A loaded row to remove.
    Delete {
        original_primary_key: BTreeMap<String, DbCell>,
    },
}

/// What the grid needs to add a row: which columns must be filled in and
/// which the server fills (defaults, identity, auto-increment, generated).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DbEditColumn {
    pub name: String,
    pub type_name: String,
    pub nullable: bool,
    /// A default, identity or auto-increment value exists; leaving the
    /// column out of an `INSERT` is fine.
    pub has_default: bool,
    /// Computed by the server (generated columns, `GENERATED ALWAYS`
    /// identity): never written.
    pub generated: bool,
    /// Part of the key that identifies rows.
    pub key: bool,
}

impl DbEditColumn {
    /// A new row must give this column a non-NULL value.
    pub fn required(&self) -> bool {
        !self.nullable && !self.has_default && !self.generated
    }
}

/// Generated statements for row changes.
#[derive(Debug, Clone)]
pub struct RowChangeRequest {
    pub request: DbQueryRequest,
    /// The same statements with the values as SQL literals. Display only.
    pub preview: String,
    pub inserted: usize,
    pub updated: usize,
    pub deleted: usize,
}

impl RowChangeRequest {
    pub fn rows(&self) -> usize {
        self.inserted + self.updated + self.deleted
    }
}

fn column_ref(name: &str) -> DbObjectRef {
    DbObjectRef {
        database: String::new(),
        schema: None,
        kind: DbObjectKind::Column,
        name: name.into(),
        parent_name: None,
    }
}

/// Builds the statements for `changes` of `table` whose rows are identified
/// by `key`. `columns` (the table's columns, may be empty when unknown)
/// rejects unknown and generated columns and checks required values of new
/// rows. Every value travels as a parameter.
pub fn row_change_request(
    engine: DbEngine,
    table: &DbObjectRef,
    key: &[String],
    columns: &[DbEditColumn],
    changes: &[DbRowChange],
) -> Result<RowChangeRequest, DbError> {
    if !matches!(
        engine,
        DbEngine::Postgresql | DbEngine::Mysql | DbEngine::Mariadb
    ) || changes.is_empty()
        || changes.len() > MAX_CELL_EDITS
        || key.is_empty()
    {
        return Err(DbError::InvalidRequest);
    }
    let known = |name: &str| columns.is_empty() || columns.iter().any(|c| c.name == name);
    let generated = |name: &str| columns.iter().any(|c| c.name == name && c.generated);
    let row_key = |original: &BTreeMap<String, DbCell>| -> Result<Vec<DbCell>, DbError> {
        if original.len() != key.len() {
            return Err(DbError::InvalidRequest);
        }
        let values: Vec<DbCell> = key
            .iter()
            .map(|column| original.get(column).cloned())
            .collect::<Option<_>>()
            .ok_or(DbError::InvalidRequest)?;
        if values.iter().any(|cell| matches!(cell, DbCell::Null)) {
            return Err(DbError::InvalidRequest);
        }
        Ok(values)
    };
    let check_values = |values: &BTreeMap<String, DbCell>| -> Result<(), DbError> {
        if values
            .keys()
            .any(|name| name.is_empty() || !known(name) || generated(name))
        {
            return Err(DbError::InvalidRequest);
        }
        Ok(())
    };
    let mut deletes: Vec<Vec<DbCell>> = Vec::new();
    let mut updates: Vec<(Vec<DbCell>, BTreeMap<String, DbCell>)> = Vec::new();
    let mut inserts: Vec<&BTreeMap<String, DbCell>> = Vec::new();
    for change in changes {
        match change {
            DbRowChange::Delete {
                original_primary_key,
            } => {
                let row = row_key(original_primary_key)?;
                if !deletes.contains(&row) {
                    deletes.push(row);
                }
            }
            DbRowChange::Update {
                original_primary_key,
                values,
            } => {
                let row = row_key(original_primary_key)?;
                if values.is_empty() {
                    return Err(DbError::InvalidRequest);
                }
                check_values(values)?;
                match updates.iter_mut().find(|(existing, _)| *existing == row) {
                    Some((_, set)) => set.extend(values.clone()),
                    None => updates.push((row, values.clone())),
                }
            }
            DbRowChange::Insert { values } => {
                check_values(values)?;
                for column in columns.iter().filter(|c| c.required()) {
                    if values
                        .get(&column.name)
                        .is_none_or(|value| matches!(value, DbCell::Null))
                    {
                        return Err(DbError::InvalidRequest);
                    }
                }
                inserts.push(values);
            }
        }
    }
    // A row is either changed or deleted, not both.
    if updates.iter().any(|(row, _)| deletes.contains(row)) {
        return Err(DbError::InvalidRequest);
    }
    let target = quote_identifier(engine, table);
    let quote = |name: &str| quote_identifier(engine, &column_ref(name));
    let mut parameters = Vec::new();
    let mut statements = Vec::new();
    let mut previews = Vec::new();
    let mut bind = |value: &DbCell| {
        let name = format!("p{}", parameters.len());
        parameters.push(DbParameter {
            name: name.clone(),
            value: value.clone(),
            type_hint: None,
        });
        (format!("{{{{{name}}}}}"), preview_literal(engine, value))
    };
    let condition = |row: &[DbCell], bind: &mut dyn FnMut(&DbCell) -> (String, String)| {
        let mut parts = (Vec::new(), Vec::new());
        for (column, value) in key.iter().zip(row) {
            let (placeholder, literal) = bind(value);
            parts.0.push(format!("{} = {placeholder}", quote(column)));
            parts.1.push(format!("{} = {literal}", quote(column)));
        }
        (parts.0.join(" AND "), parts.1.join(" AND "))
    };
    for row in &deletes {
        let (run, shown) = condition(row, &mut bind);
        statements.push(format!("DELETE FROM {target} WHERE {run}"));
        previews.push(format!("DELETE FROM {target} WHERE {shown}"));
    }
    for (row, values) in &updates {
        let mut set = (Vec::new(), Vec::new());
        for (column, value) in values {
            let (placeholder, literal) = bind(value);
            set.0.push(format!("{} = {placeholder}", quote(column)));
            set.1.push(format!("{} = {literal}", quote(column)));
        }
        let (run, shown) = condition(row, &mut bind);
        statements.push(format!(
            "UPDATE {target} SET {} WHERE {run}",
            set.0.join(", ")
        ));
        previews.push(format!(
            "UPDATE {target} SET {} WHERE {shown}",
            set.1.join(", ")
        ));
    }
    for values in &inserts {
        if values.is_empty() {
            let text = match engine {
                DbEngine::Postgresql => format!("INSERT INTO {target} DEFAULT VALUES"),
                _ => format!("INSERT INTO {target} () VALUES ()"),
            };
            statements.push(text.clone());
            previews.push(text);
            continue;
        }
        let names: Vec<String> = values.keys().map(|name| quote(name)).collect();
        let (mut placeholders, mut literals) = (Vec::new(), Vec::new());
        for value in values.values() {
            let (placeholder, literal) = bind(value);
            placeholders.push(placeholder);
            literals.push(literal);
        }
        statements.push(format!(
            "INSERT INTO {target} ({}) VALUES ({})",
            names.join(", "),
            placeholders.join(", ")
        ));
        previews.push(format!(
            "INSERT INTO {target} ({}) VALUES ({})",
            names.join(", "),
            literals.join(", ")
        ));
    }
    Ok(RowChangeRequest {
        request: DbQueryRequest {
            text: statements.join(";\n") + ";",
            parameters,
            approval_ticket: None,
        },
        preview: previews.join(";\n") + ";",
        inserted: inserts.len(),
        updated: updates.len(),
        deleted: deletes.len(),
    })
}

impl DbSession {
    /// Columns of an editable table with their NULL, default and generated
    /// flags; the key columns are marked.
    pub async fn edit_columns(&self, table: &DbObjectRef) -> Result<Vec<DbEditColumn>, DbError> {
        if table.kind != DbObjectKind::Table {
            return Err(DbError::InvalidRequest);
        }
        if self.info.engine == DbEngine::Cassandra {
            // Key columns must be given; no defaults. Counters change only
            // through UPDATE, so they are not written like generated columns.
            let columns: Vec<DbEditColumn> = self
                .cassandra_columns(table)
                .await?
                .into_iter()
                .map(|column| {
                    let key = matches!(
                        column.kind,
                        crate::cassandra::CqlColumnKind::PartitionKey
                            | crate::cassandra::CqlColumnKind::Clustering { .. }
                    );
                    DbEditColumn {
                        key,
                        nullable: !key,
                        has_default: false,
                        generated: column.type_name == "counter",
                        type_name: column.type_name,
                        name: column.name,
                    }
                })
                .collect();
            if columns.is_empty() {
                return Err(DbError::InvalidRequest);
            }
            return Ok(columns);
        }
        let text = |value: &str| DbParameter {
            name: String::new(),
            value: DbCell::Text(value.into()),
            type_hint: None,
        };
        let (sql, bindings) = match self.info.engine {
            DbEngine::Postgresql => (
                "SELECT a.attname, pg_catalog.format_type(a.atttypid, a.atttypmod), \
                 CASE WHEN a.attnotnull THEN 'false' ELSE 'true' END, \
                 CASE WHEN a.atthasdef OR a.attidentity <> '' THEN 'true' ELSE 'false' END, \
                 CASE WHEN a.attidentity = 'a' OR a.attgenerated <> '' THEN 'true' ELSE 'false' END \
                 FROM pg_catalog.pg_attribute a \
                 JOIN pg_catalog.pg_class c ON c.oid = a.attrelid \
                 JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace \
                 WHERE n.nspname = $1 AND c.relname = $2 AND c.relkind IN ('r', 'p') \
                 AND a.attnum > 0 AND NOT a.attisdropped ORDER BY a.attnum",
                vec![
                    text(table.schema.as_deref().unwrap_or("public")),
                    text(&table.name),
                ],
            ),
            DbEngine::Mysql | DbEngine::Mariadb => (
                "SELECT column_name, column_type, IF(is_nullable = 'YES', 'true', 'false'), \
                 IF(column_default IS NOT NULL OR extra LIKE '%auto_increment%', 'true', 'false'), \
                 IF(extra LIKE '%VIRTUAL GENERATED%' OR extra LIKE '%STORED GENERATED%' \
                 OR extra LIKE '%PERSISTENT GENERATED%', 'true', 'false') \
                 FROM information_schema.columns WHERE table_schema = ? AND table_name = ? \
                 ORDER BY ordinal_position",
                vec![text(&table.database), text(&table.name)],
            ),
            // `hidden` 2/3 = generated; an INTEGER PRIMARY KEY (rowid alias)
            // gets a value when left out, so it counts as having a default.
            DbEngine::Sqlite => (
                "SELECT name, type, CASE WHEN \"notnull\" THEN 'false' ELSE 'true' END, \
                 CASE WHEN dflt_value IS NOT NULL OR (pk = 1 AND upper(type) = 'INTEGER') \
                 THEN 'true' ELSE 'false' END, \
                 CASE WHEN hidden IN (2, 3) THEN 'true' ELSE 'false' END \
                 FROM pragma_table_xinfo(?1) WHERE hidden <> 1 ORDER BY cid",
                vec![text(&table.name)],
            ),
            _ => return Err(DbError::InvalidRequest),
        };
        let key = self.primary_key(table).await?;
        let mut rows = Collect::default();
        self.driver
            .lock()
            .await
            .query(sql, &bindings, true, &mut rows, self.scope.child_token())
            .await?;
        let columns: Vec<DbEditColumn> = rows
            .rows
            .into_iter()
            .filter(|row| row.len() == 5)
            .map(|row| DbEditColumn {
                key: key.contains(&row[0]),
                nullable: row[2] == "true",
                has_default: row[3] == "true",
                generated: row[4] == "true",
                type_name: row[1].clone(),
                name: row[0].clone(),
            })
            .collect();
        if columns.is_empty() {
            return Err(DbError::InvalidRequest);
        }
        Ok(columns)
    }

    /// Reviewable statements for row changes of `table`, checked against its
    /// real key and columns.
    pub async fn review_row_changes(
        &self,
        table: &DbObjectRef,
        changes: &[DbRowChange],
    ) -> Result<RowChangeRequest, DbError> {
        if self.guard.is_read_only() || self.guard.policy().mobile {
            return Err(DbError::PolicyDenied);
        }
        let key = self.primary_key(table).await?;
        let columns = self.edit_columns(table).await?;
        if key.is_empty() {
            return Err(DbError::InvalidRequest);
        }
        row_change_request(self.info.engine, table, &key, &columns, changes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn table(engine: DbEngine) -> DbObjectRef {
        match engine {
            DbEngine::Postgresql => DbObjectRef {
                database: String::new(),
                schema: Some("public".into()),
                kind: DbObjectKind::Table,
                name: "items".into(),
                parent_name: None,
            },
            _ => DbObjectRef {
                database: "shop".into(),
                schema: None,
                kind: DbObjectKind::Table,
                name: "items".into(),
                parent_name: None,
            },
        }
    }

    fn column(name: &str, nullable: bool, has_default: bool, generated: bool) -> DbEditColumn {
        DbEditColumn {
            name: name.into(),
            type_name: "text".into(),
            nullable,
            has_default,
            generated,
            key: name == "id",
        }
    }

    fn columns() -> Vec<DbEditColumn> {
        vec![
            column("id", false, true, false),
            column("name", false, false, false),
            column("note", true, false, false),
            column("total", true, false, true),
        ]
    }

    fn key(id: &str) -> BTreeMap<String, DbCell> {
        BTreeMap::from([("id".to_string(), DbCell::Int(id.into()))])
    }

    fn values(pairs: &[(&str, DbCell)]) -> BTreeMap<String, DbCell> {
        pairs
            .iter()
            .map(|(name, value)| (name.to_string(), value.clone()))
            .collect()
    }

    #[test]
    fn changes_deserialize_by_kind_and_refuse_unknown_fields() {
        let change: DbRowChange = serde_json::from_value(serde_json::json!({
            "kind": "insert",
            "values": {"name": {"kind": "text", "value": "a"}}
        }))
        .unwrap();
        assert!(matches!(change, DbRowChange::Insert { .. }));
        let delete: DbRowChange = serde_json::from_value(serde_json::json!({
            "kind": "delete",
            "original_primary_key": {"id": {"kind": "int", "value": "1"}}
        }))
        .unwrap();
        assert_eq!(
            delete,
            DbRowChange::Delete {
                original_primary_key: key("1")
            }
        );
        assert!(serde_json::from_value::<DbRowChange>(serde_json::json!({
            "kind": "delete",
            "original_primary_key": {},
            "sql": "DROP TABLE x"
        }))
        .is_err());
        assert!(serde_json::from_value::<DbRowChange>(serde_json::json!({
            "kind": "truncate",
        }))
        .is_err());
    }

    #[test]
    fn postgres_statements_run_deletes_then_updates_then_inserts() {
        let changes = [
            DbRowChange::Insert {
                values: values(&[("name", DbCell::Text("new".into())), ("note", DbCell::Null)]),
            },
            DbRowChange::Update {
                original_primary_key: key("2"),
                values: values(&[("name", DbCell::Text("x'; DROP".into()))]),
            },
            DbRowChange::Delete {
                original_primary_key: key("3"),
            },
            DbRowChange::Update {
                original_primary_key: key("2"),
                values: values(&[("note", DbCell::Null)]),
            },
        ];
        let edit = row_change_request(
            DbEngine::Postgresql,
            &table(DbEngine::Postgresql),
            &["id".into()],
            &columns(),
            &changes,
        )
        .unwrap();
        assert_eq!((edit.deleted, edit.updated, edit.inserted), (1, 1, 1));
        assert_eq!(edit.rows(), 3);
        assert_eq!(
            edit.request.text,
            "DELETE FROM \"public\".\"items\" WHERE \"id\" = {{p0}};\n\
             UPDATE \"public\".\"items\" SET \"name\" = {{p1}}, \"note\" = {{p2}} WHERE \"id\" = {{p3}};\n\
             INSERT INTO \"public\".\"items\" (\"name\", \"note\") VALUES ({{p4}}, {{p5}});"
        );
        assert_eq!(edit.request.parameters.len(), 6);
        assert_eq!(edit.request.parameters[3].value, DbCell::Int("2".into()));
        assert_eq!(edit.request.parameters[5].value, DbCell::Null);
        assert!(!edit.request.text.contains("DROP"));
        assert_eq!(
            edit.preview,
            "DELETE FROM \"public\".\"items\" WHERE \"id\" = 3;\n\
             UPDATE \"public\".\"items\" SET \"name\" = 'x''; DROP', \"note\" = NULL WHERE \"id\" = 2;\n\
             INSERT INTO \"public\".\"items\" (\"name\", \"note\") VALUES ('new', NULL);"
        );
    }

    #[test]
    fn mysql_quotes_with_backticks_and_inserts_defaults() {
        let changes = [
            DbRowChange::Insert {
                values: BTreeMap::new(),
            },
            DbRowChange::Insert {
                values: values(&[
                    ("we`ird", DbCell::Text("a\\b'c".into())),
                    ("blob", DbCell::Binary("AP8=".into())),
                ]),
            },
            DbRowChange::Delete {
                original_primary_key: BTreeMap::from([
                    ("a".to_string(), DbCell::Int("1".into())),
                    ("b".to_string(), DbCell::Text("x".into())),
                ]),
            },
        ];
        let edit = row_change_request(
            DbEngine::Mysql,
            &table(DbEngine::Mysql),
            &["a".into(), "b".into()],
            &[],
            &changes,
        )
        .unwrap();
        assert_eq!(
            edit.request.text,
            "DELETE FROM `shop`.`items` WHERE `a` = {{p0}} AND `b` = {{p1}};\n\
             INSERT INTO `shop`.`items` () VALUES ();\n\
             INSERT INTO `shop`.`items` (`blob`, `we``ird`) VALUES ({{p2}}, {{p3}});"
        );
        assert_eq!(
            edit.preview,
            "DELETE FROM `shop`.`items` WHERE `a` = 1 AND `b` = 'x';\n\
             INSERT INTO `shop`.`items` () VALUES ();\n\
             INSERT INTO `shop`.`items` (`blob`, `we``ird`) VALUES (FROM_BASE64('AP8='), 'a\\\\b''c');"
        );
        let empty = row_change_request(
            DbEngine::Postgresql,
            &table(DbEngine::Postgresql),
            &["id".into()],
            &[],
            &[DbRowChange::Insert {
                values: BTreeMap::new(),
            }],
        )
        .unwrap();
        assert_eq!(
            empty.request.text,
            "INSERT INTO \"public\".\"items\" DEFAULT VALUES;"
        );
    }

    #[test]
    fn refuses_invalid_changes() {
        let pg = |changes: &[DbRowChange]| {
            row_change_request(
                DbEngine::Postgresql,
                &table(DbEngine::Postgresql),
                &["id".into()],
                &columns(),
                changes,
            )
        };
        // A required column without a value, or with NULL.
        assert!(pg(&[DbRowChange::Insert {
            values: values(&[("note", DbCell::Text("n".into()))])
        }])
        .is_err());
        assert!(pg(&[DbRowChange::Insert {
            values: values(&[("name", DbCell::Null)])
        }])
        .is_err());
        // Generated and unknown columns are never written.
        assert!(pg(&[DbRowChange::Insert {
            values: values(&[
                ("name", DbCell::Text("n".into())),
                ("total", DbCell::Int("1".into()))
            ])
        }])
        .is_err());
        assert!(pg(&[DbRowChange::Update {
            original_primary_key: key("1"),
            values: values(&[("missing", DbCell::Null)])
        }])
        .is_err());
        // Keys must be the whole key and not NULL.
        assert!(pg(&[DbRowChange::Delete {
            original_primary_key: BTreeMap::new()
        }])
        .is_err());
        assert!(pg(&[DbRowChange::Delete {
            original_primary_key: BTreeMap::from([("id".to_string(), DbCell::Null)])
        }])
        .is_err());
        assert!(pg(&[DbRowChange::Delete {
            original_primary_key: BTreeMap::from([("name".to_string(), DbCell::Int("1".into()))])
        }])
        .is_err());
        // A row is changed or deleted, not both; an update needs values.
        assert!(pg(&[
            DbRowChange::Delete {
                original_primary_key: key("1")
            },
            DbRowChange::Update {
                original_primary_key: key("1"),
                values: values(&[("name", DbCell::Text("n".into()))])
            },
        ])
        .is_err());
        assert!(pg(&[DbRowChange::Update {
            original_primary_key: key("1"),
            values: BTreeMap::new()
        }])
        .is_err());
        assert!(pg(&[]).is_err());
        // Engines without row identity here.
        for engine in [DbEngine::Clickhouse, DbEngine::Redis, DbEngine::Mongodb] {
            assert!(row_change_request(
                engine,
                &table(DbEngine::Mysql),
                &["id".into()],
                &[],
                &[DbRowChange::Delete {
                    original_primary_key: key("1")
                }],
            )
            .is_err());
        }
        // A valid insert with the required value passes; the default key is left out.
        let ok = pg(&[DbRowChange::Insert {
            values: values(&[("name", DbCell::Text("n".into()))]),
        }])
        .unwrap();
        assert_eq!(
            ok.request.text,
            "INSERT INTO \"public\".\"items\" (\"name\") VALUES ({{p0}});"
        );
    }

    #[test]
    fn required_columns_follow_null_default_and_generated_flags() {
        let all = columns();
        let required: Vec<&str> = all
            .iter()
            .filter(|c| c.required())
            .map(|c| c.name.as_str())
            .collect();
        assert_eq!(required, ["name"]);
    }
}
