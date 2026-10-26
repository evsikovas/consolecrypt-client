//! SQLite: one file (the `main` schema), type names with affinity rules,
//! keys, checks and foreign keys inside CREATE TABLE, separate CREATE INDEX
//! statements, no comments. ALTER TABLE only renames and adds columns; any
//! other change rebuilds the table the way SQLite documents it: create the
//! new table under a temporary name, copy the rows, drop the old table,
//! rename the new one and recreate the indexes. The script runs in one
//! transaction (the session turns foreign key enforcement off around it, so
//! dropping the old table never cascades).
use super::*;

pub struct Sqlite;

const TYPES: &[TypeInfo] = &[
    ty("INTEGER", "numeric"),
    ty("REAL", "numeric"),
    ty("NUMERIC", "numeric"),
    ty_ps("DECIMAL", "numeric"),
    ty("BOOLEAN", "other"),
    ty("TEXT", "text"),
    ty_len("VARCHAR", "text", false),
    ty("BLOB", "binary"),
    ty("DATE", "time"),
    ty("DATETIME", "time"),
    ty("ANY", "other"),
];

/// The only types a STRICT table accepts.
const STRICT_TYPES: [&str; 6] = ["INT", "INTEGER", "REAL", "TEXT", "BLOB", "ANY"];

/// Suffix of the temporary name a rebuilt table is created under.
const REBUILD_SUFFIX: &str = "__cc_rebuild";

impl Sqlite {
    fn base_type(column: &ColumnDesign) -> String {
        column
            .data_type
            .split('(')
            .next()
            .unwrap_or_default()
            .trim()
            .to_ascii_uppercase()
    }

    fn auto_increment(design: &TableDesign) -> Option<&ColumnDesign> {
        design
            .columns
            .iter()
            .find(|column| column.identity == "autoincrement")
    }

    fn column(&self, column: &ColumnDesign) -> String {
        let mut line = format!("{} {}", self.quote(&column.name), column_type(column));
        if column.identity == "autoincrement" {
            line.push_str(" PRIMARY KEY AUTOINCREMENT");
        }
        if !column.nullable {
            line.push_str(" NOT NULL");
        }
        if !column.default.trim().is_empty() {
            line.push_str(" DEFAULT ");
            line.push_str(&self.default(&column.default));
        }
        line
    }

    /// A default as SQLite takes it: literals, NULL and CURRENT_* as they
    /// are, any other expression in parentheses.
    fn default(&self, text: &str) -> String {
        let sql = default_sql(self, text);
        let lower = sql.to_ascii_lowercase();
        let literal = sql.starts_with('\'')
            || sql.starts_with('(')
            || DEFAULT_KEYWORDS.contains(&lower.as_str())
            || sql
                .strip_prefix('-')
                .unwrap_or(&sql)
                .chars()
                .all(|c| c.is_ascii_digit() || c == '.');
        if literal {
            sql
        } else {
            format!("({sql})")
        }
    }

    fn constraint_name(&self, name: &str) -> String {
        if name.trim().is_empty() {
            String::new()
        } else {
            format!("CONSTRAINT {} ", self.quote(name))
        }
    }

    fn foreign_key(&self, fk: &ForeignKeyDesign) -> String {
        let mut sql = format!(
            "{}FOREIGN KEY ({}) REFERENCES {} ({})",
            self.constraint_name(&fk.name),
            quote_list(self, &fk.columns),
            self.quote(&fk.ref_table),
            quote_list(self, &fk.ref_columns)
        );
        if !fk.on_delete.is_empty() {
            sql.push_str(&format!(" ON DELETE {}", fk.on_delete));
        }
        if !fk.on_update.is_empty() {
            sql.push_str(&format!(" ON UPDATE {}", fk.on_update));
        }
        sql
    }

    /// CREATE TABLE `name` for `design` (the design's name or the
    /// temporary name of a rebuild).
    fn create_table(&self, design: &TableDesign, name: &str) -> String {
        let info = self.info();
        let mut lines: Vec<String> = design.columns.iter().map(|c| self.column(c)).collect();
        if !design.primary_key.columns.is_empty() && Self::auto_increment(design).is_none() {
            lines.push(format!(
                "{}PRIMARY KEY ({})",
                self.constraint_name(&design.primary_key.name),
                quote_list(self, &design.primary_key.columns)
            ));
        }
        for key in &design.uniques {
            lines.push(format!(
                "{}UNIQUE ({})",
                self.constraint_name(&key.name),
                quote_list(self, &key.columns)
            ));
        }
        for check in &design.checks {
            lines.push(format!(
                "{}CHECK ({})",
                self.constraint_name(&check.name),
                check.expression.trim()
            ));
        }
        lines.extend(design.foreign_keys.iter().map(|fk| self.foreign_key(fk)));
        let mut sql = format!(
            "CREATE TABLE {} (\n    {}\n)",
            self.quote(name),
            lines.join(",\n    ")
        );
        if option(&info, design, "strict") == "true" {
            sql.push_str(" STRICT");
        } else if option(&info, design, "without_rowid") == "true" {
            sql.push_str(" WITHOUT ROWID");
        }
        sql
    }

    fn index(&self, design: &TableDesign, table: &str, index: &IndexDesign) -> String {
        format!(
            "CREATE {}INDEX {} ON {} ({})",
            if index.unique { "UNIQUE " } else { "" },
            self.quote(&name_or(
                &self.info(),
                &index.name,
                &design.name,
                &index.columns,
                "idx"
            )),
            self.quote(table),
            quote_list(self, &index.columns)
        )
    }

    /// ALTER TABLE … ADD COLUMN takes the column: no key, a constant
    /// default, and NOT NULL only with a non-NULL default.
    fn addable(column: &ColumnDesign) -> bool {
        let default = column.default.trim().to_ascii_lowercase();
        column.identity.is_empty()
            && !default.starts_with('(')
            && !default.starts_with("current_")
            && (column.nullable || (!default.is_empty() && default != "null"))
    }

    /// The change needs a rebuild: anything beyond renames, added columns
    /// ALTER TABLE accepts, and indexes.
    fn needs_rebuild(after: &TableDesign, changes: &TableChanges<'_>) -> bool {
        let added_keys = changes.added_columns.iter().any(|(_, column)| {
            after.primary_key.columns.contains(&column.name)
                || after
                    .uniques
                    .iter()
                    .any(|k| k.columns.contains(&column.name))
                || after
                    .foreign_keys
                    .iter()
                    .any(|fk| fk.columns.contains(&column.name))
        });
        !changes.dropped_columns.is_empty()
            || changes
                .kept_columns
                .iter()
                .any(|(old, new, _)| TableChanges::definition_changed(old, new))
            || changes
                .added_columns
                .iter()
                .any(|(_, column)| !Self::addable(column))
            || added_keys
            || changes.primary_key_changed
            || !changes.dropped_uniques.is_empty()
            || !changes.added_uniques.is_empty()
            || !changes.dropped_foreign_keys.is_empty()
            || !changes.added_foreign_keys.is_empty()
            || !changes.dropped_checks.is_empty()
            || !changes.added_checks.is_empty()
            || !changes.changed_options.is_empty()
            // A new column only goes last.
            || changes
                .added_columns
                .iter()
                .any(|(position, _)| *position < after.columns.len() - changes.added_columns.len())
    }

    fn rebuild(
        &self,
        before: &TableDesign,
        after: &TableDesign,
        changes: &TableChanges<'_>,
        issues: &mut Issues,
    ) -> Vec<DesignStatement> {
        if !before.unmodeled.is_empty() {
            issues.error("rebuild_blocked", "table", &before.unmodeled.join(", "));
            return Vec::new();
        }
        issues.warning("table_rebuild", "table", &after.name);
        let info = self.info();
        let mut temporary = format!("{}{REBUILD_SUFFIX}", after.name);
        if temporary.len() > info.max_identifier {
            temporary = key_name(&info, "cc", &[], "rebuild");
        }
        let mut out = vec![statement(self.create_table(after, &temporary))];
        let kept: Vec<_> = changes
            .kept_columns
            .iter()
            .map(|(old, new, _)| (old.name.as_str(), new.name.as_str()))
            .collect();
        if !kept.is_empty() {
            out.push(statement(format!(
                "INSERT INTO {} ({}) SELECT {} FROM {}",
                self.quote(&temporary),
                kept.iter()
                    .map(|(_, new)| self.quote(new))
                    .collect::<Vec<_>>()
                    .join(", "),
                kept.iter()
                    .map(|(old, _)| self.quote(old))
                    .collect::<Vec<_>>()
                    .join(", "),
                self.quote(&before.name)
            )));
        }
        out.push(destructive(format!(
            "DROP TABLE {}",
            self.quote(&before.name)
        )));
        out.push(statement(format!(
            "ALTER TABLE {} RENAME TO {}",
            self.quote(&temporary),
            self.quote(&after.name)
        )));
        out.extend(
            after
                .indexes
                .iter()
                .map(|index| statement(self.index(after, &after.name, index))),
        );
        out
    }
}

impl Dialect for Sqlite {
    fn info(&self) -> DialectInfo {
        DialectInfo {
            engine: DbEngine::Sqlite,
            features: DesignFeatures {
                primary_key: true,
                unique: true,
                foreign_keys: true,
                checks: true,
                indexes: true,
                alter: true,
                transactional: true,
                ..Default::default()
            },
            types: TYPES.to_vec(),
            default_type: "INTEGER",
            identity_kinds: vec!["autoincrement"],
            default_presets: vec![
                "CURRENT_TIMESTAMP",
                "CURRENT_DATE",
                "CURRENT_TIME",
                "0",
                "1",
                "''",
                "NULL",
            ],
            index_methods: vec![],
            fk_actions: vec![
                "NO ACTION",
                "RESTRICT",
                "CASCADE",
                "SET NULL",
                "SET DEFAULT",
            ],
            options: vec![
                OptionInfo {
                    id: "strict",
                    kind: OptionKind::Bool,
                    choices: vec![],
                    default: "false",
                    create_only: false,
                },
                OptionInfo {
                    id: "without_rowid",
                    kind: OptionKind::Bool,
                    choices: vec![],
                    default: "false",
                    create_only: false,
                },
            ],
            max_identifier: 255,
        }
    }

    fn quote(&self, name: &str) -> String {
        format!("\"{}\"", name.replace('"', "\"\""))
    }

    fn literal(&self, text: &str) -> String {
        format!("'{}'", text.replace('\'', "''"))
    }

    fn validate(&self, design: &TableDesign, issues: &mut Issues) {
        let info = self.info();
        let strict = option(&info, design, "strict") == "true";
        let without_rowid = option(&info, design, "without_rowid") == "true";
        if design.name.to_ascii_lowercase().starts_with("sqlite_") {
            issues.error("name_invalid", "table.name", &design.name);
        }
        let autos: Vec<(usize, &ColumnDesign)> = design
            .columns
            .iter()
            .enumerate()
            .filter(|(_, column)| column.identity == "autoincrement")
            .collect();
        if autos.len() > 1 {
            issues.error("autoincrement_single", "columns", "");
        }
        for (i, column) in &autos {
            // AUTOINCREMENT is for the INTEGER PRIMARY KEY of a rowid table.
            if Self::base_type(column) != "INTEGER"
                || design.primary_key.columns != [column.name.clone()]
                || without_rowid
            {
                issues.error(
                    "autoincrement_key",
                    format!("columns.{i}.identity"),
                    &column.name,
                );
            }
        }
        // The guard's parser reads one of the two table options, not both.
        if strict && without_rowid {
            issues.error(
                "option_invalid",
                "options.without_rowid",
                "STRICT, WITHOUT ROWID",
            );
        }
        if without_rowid && design.primary_key.columns.is_empty() {
            issues.error("primary_key_required", "primary_key", &design.name);
        }
        for (i, column) in design.columns.iter().enumerate() {
            if strict && !STRICT_TYPES.contains(&Self::base_type(column).as_str()) {
                issues.error(
                    "type_strict",
                    format!("columns.{i}.data_type"),
                    &column.data_type,
                );
            }
            if !column.comment.is_empty() {
                issues.warning(
                    "comment_ignored",
                    format!("columns.{i}.comment"),
                    &column.name,
                );
            }
        }
        if !design.comment.is_empty() {
            issues.warning("comment_ignored", "table.comment", &design.name);
        }
        for (i, index) in design.indexes.iter().enumerate() {
            if !index.method.is_empty() {
                issues.error(
                    "option_invalid",
                    format!("indexes.{i}.method"),
                    &index.method,
                );
            }
        }
        for (i, fk) in design.foreign_keys.iter().enumerate() {
            if fk
                .ref_schema
                .as_deref()
                .is_some_and(|s| !s.is_empty() && s != "main")
            {
                issues.error(
                    "move_unsupported",
                    format!("foreign_keys.{i}.ref_schema"),
                    &fk.name,
                );
            }
        }
    }

    fn create(&self, design: &TableDesign, _: &mut Issues) -> Vec<DesignStatement> {
        let mut statements = vec![statement(self.create_table(design, &design.name))];
        statements.extend(
            design
                .indexes
                .iter()
                .map(|index| statement(self.index(design, &design.name, index))),
        );
        statements
    }

    fn alter(
        &self,
        before: &TableDesign,
        after: &TableDesign,
        changes: &TableChanges<'_>,
        issues: &mut Issues,
    ) -> Vec<DesignStatement> {
        if Self::needs_rebuild(after, changes) {
            return self.rebuild(before, after, changes, issues);
        }
        let table = self.quote(&before.name);
        let alter = |op: String| format!("ALTER TABLE {table} {op}");
        let info = self.info();
        let mut out = Vec::new();
        for index in &changes.dropped_indexes {
            let name = name_or(&info, &index.name, &before.name, &index.columns, "idx");
            out.push(statement(format!("DROP INDEX {}", self.quote(&name))));
        }
        for (old, new, _) in &changes.kept_columns {
            if old.name != new.name {
                out.push(statement(alter(format!(
                    "RENAME COLUMN {} TO {}",
                    self.quote(&old.name),
                    self.quote(&new.name)
                ))));
            }
        }
        for (_, column) in &changes.added_columns {
            out.push(statement(alter(format!(
                "ADD COLUMN {}",
                self.column(column)
            ))));
        }
        for index in &changes.added_indexes {
            out.push(statement(self.index(after, &before.name, index)));
        }
        if changes.renamed_table {
            out.push(statement(alter(format!(
                "RENAME TO {}",
                self.quote(&after.name)
            ))));
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::super::plan as make_plan;
    use super::super::test_support::{assert_guard_accepts, column, loaded};
    use super::*;

    fn items() -> TableDesign {
        TableDesign {
            database: "main".into(),
            name: "items".into(),
            columns: vec![
                ColumnDesign {
                    nullable: false,
                    identity: "autoincrement".into(),
                    ..column("id", "INTEGER")
                },
                ColumnDesign {
                    nullable: false,
                    ..column("name", "TEXT")
                },
                ColumnDesign {
                    default: "0".into(),
                    ..column("qty", "INTEGER")
                },
                ColumnDesign {
                    default: "CURRENT_TIMESTAMP".into(),
                    ..column("at", "DATETIME")
                },
                ColumnDesign {
                    default: "active".into(),
                    ..column("state", "TEXT")
                },
                column("owner", "INTEGER"),
            ],
            primary_key: KeyDesign {
                columns: vec!["id".into()],
                ..Default::default()
            },
            uniques: vec![KeyDesign {
                columns: vec!["name".into()],
                ..Default::default()
            }],
            checks: vec![CheckDesign {
                name: "qty_positive".into(),
                expression: "qty >= 0".into(),
                ..Default::default()
            }],
            foreign_keys: vec![ForeignKeyDesign {
                columns: vec!["owner".into()],
                ref_table: "owners".into(),
                ref_columns: vec!["id".into()],
                on_delete: "SET NULL".into(),
                ..Default::default()
            }],
            indexes: vec![IndexDesign {
                columns: vec!["state".into(), "at".into()],
                ..Default::default()
            }],
            ..Default::default()
        }
    }

    #[test]
    fn creates_a_table_with_constraints_and_indexes() {
        let plan = make_plan(DbEngine::Sqlite, None, &items());
        assert_eq!(
            plan.statements[0].sql,
            "CREATE TABLE \"items\" (\n    \
             \"id\" INTEGER PRIMARY KEY AUTOINCREMENT NOT NULL,\n    \
             \"name\" TEXT NOT NULL,\n    \
             \"qty\" INTEGER DEFAULT 0,\n    \
             \"at\" DATETIME DEFAULT CURRENT_TIMESTAMP,\n    \
             \"state\" TEXT DEFAULT 'active',\n    \
             \"owner\" INTEGER,\n    \
             UNIQUE (\"name\"),\n    \
             CONSTRAINT \"qty_positive\" CHECK (qty >= 0),\n    \
             FOREIGN KEY (\"owner\") REFERENCES \"owners\" (\"id\") ON DELETE SET NULL\n\
             )"
        );
        assert_eq!(
            plan.statements[1].sql,
            "CREATE INDEX \"items_state_at_idx\" ON \"items\" (\"state\", \"at\")"
        );
        assert!(plan.transactional);
        assert_guard_accepts(DbEngine::Sqlite, &plan);
    }

    #[test]
    fn strict_without_rowid_and_expression_defaults() {
        let design = TableDesign {
            name: "pairs".into(),
            columns: vec![
                ColumnDesign {
                    nullable: false,
                    ..column("k", "TEXT")
                },
                ColumnDesign {
                    default: "datetime('now')".into(),
                    ..column("v", "TEXT")
                },
            ],
            primary_key: KeyDesign {
                columns: vec!["k".into()],
                ..Default::default()
            },
            options: [("without_rowid".to_owned(), "true".to_owned())].into(),
            ..Default::default()
        };
        let plan = make_plan(DbEngine::Sqlite, None, &design);
        assert_eq!(
            plan.statements[0].sql,
            "CREATE TABLE \"pairs\" (\n    \
             \"k\" TEXT NOT NULL,\n    \
             \"v\" TEXT DEFAULT (datetime('now')),\n    \
             PRIMARY KEY (\"k\")\n\
             ) WITHOUT ROWID"
        );
        assert_guard_accepts(DbEngine::Sqlite, &plan);
        let mut strict = design.clone();
        strict.options = [("strict".to_owned(), "true".to_owned())].into();
        let plan = make_plan(DbEngine::Sqlite, None, &strict);
        assert!(plan.statements[0].sql.ends_with(") STRICT"));
        assert_guard_accepts(DbEngine::Sqlite, &plan);
        let mut both = strict.clone();
        both.options.insert("without_rowid".into(), "true".into());
        assert!(make_plan(DbEngine::Sqlite, None, &both)
            .issues
            .iter()
            .any(|issue| issue.code == "option_invalid"));
        let codes = |design: &TableDesign| -> Vec<&'static str> {
            make_plan(DbEngine::Sqlite, None, design)
                .issues
                .into_iter()
                .map(|issue| issue.code)
                .collect()
        };
        let mut loose = strict.clone();
        loose.columns[1].data_type = "DATETIME".into();
        assert!(codes(&loose).contains(&"type_strict"));
        let mut keyless = design.clone();
        keyless.primary_key.columns.clear();
        assert!(codes(&keyless).contains(&"primary_key_required"));
    }

    #[test]
    fn autoincrement_needs_the_integer_primary_key() {
        let mut design = items();
        design.columns[0].data_type = "BIGINT".into();
        let codes: Vec<_> = make_plan(DbEngine::Sqlite, None, &design)
            .issues
            .into_iter()
            .map(|issue| issue.code)
            .collect();
        assert!(codes.contains(&"autoincrement_key"), "{codes:?}");
    }

    #[test]
    fn renames_adds_columns_and_indexes_in_place() {
        let before = loaded(items());
        let mut after = before.clone();
        after.name = "goods".into();
        after.columns[1].name = "title".into();
        after.uniques[0].columns = vec!["title".into()];
        after.indexes.clear();
        after.indexes.push(IndexDesign {
            name: "goods_title".into(),
            columns: vec!["title".into()],
            unique: false,
            ..Default::default()
        });
        after.columns.push(column("note", "TEXT"));
        after.columns.push(ColumnDesign {
            nullable: false,
            default: "1".into(),
            ..column("level", "INTEGER")
        });
        let plan = make_plan(DbEngine::Sqlite, Some(&before), &after);
        let sql: Vec<&str> = plan.statements.iter().map(|s| s.sql.as_str()).collect();
        assert_eq!(
            sql,
            [
                "DROP INDEX \"items_state_at_idx\"",
                "ALTER TABLE \"items\" RENAME COLUMN \"name\" TO \"title\"",
                "ALTER TABLE \"items\" ADD COLUMN \"note\" TEXT",
                "ALTER TABLE \"items\" ADD COLUMN \"level\" INTEGER NOT NULL DEFAULT 1",
                "CREATE INDEX \"goods_title\" ON \"items\" (\"title\")",
                "ALTER TABLE \"items\" RENAME TO \"goods\"",
            ]
        );
        assert!(plan.issues.is_empty(), "{:?}", plan.issues);
        assert_guard_accepts(DbEngine::Sqlite, &plan);
    }

    #[test]
    fn other_changes_rebuild_the_table() {
        let before = loaded(items());
        let mut after = before.clone();
        after.columns.remove(5);
        after.foreign_keys.clear();
        after.columns[2].nullable = false;
        after.columns[3].name = "created".into();
        after.indexes[0].columns = vec!["state".into(), "created".into()];
        after.columns.insert(
            1,
            ColumnDesign {
                default: "''".into(),
                ..column("code", "TEXT")
            },
        );
        let plan = make_plan(DbEngine::Sqlite, Some(&before), &after);
        let sql: Vec<(&str, bool)> = plan
            .statements
            .iter()
            .map(|s| (s.sql.as_str(), s.destructive))
            .collect();
        assert_eq!(sql.len(), 5, "{sql:?}");
        assert!(sql[0]
            .0
            .starts_with("CREATE TABLE \"items__cc_rebuild\" (\n    \"id\""));
        assert!(sql[0].0.contains("\"code\" TEXT DEFAULT ''"));
        assert!(sql[0].0.contains("\"qty\" INTEGER NOT NULL DEFAULT 0"));
        assert!(!sql[0].0.contains("FOREIGN KEY"));
        assert_eq!(
            sql[1],
            (
                "INSERT INTO \"items__cc_rebuild\" (\"id\", \"name\", \"qty\", \"created\", \"state\") \
                 SELECT \"id\", \"name\", \"qty\", \"at\", \"state\" FROM \"items\"",
                false
            )
        );
        assert_eq!(sql[2], ("DROP TABLE \"items\"", true));
        assert_eq!(
            sql[3],
            (
                "ALTER TABLE \"items__cc_rebuild\" RENAME TO \"items\"",
                false
            )
        );
        assert_eq!(
            sql[4],
            (
                "CREATE INDEX \"items_state_created_idx\" ON \"items\" (\"state\", \"created\")",
                false
            )
        );
        assert!(plan.issues.iter().any(|i| i.code == "table_rebuild"));
        assert!(plan.transactional);
        assert_guard_accepts(DbEngine::Sqlite, &plan);

        // What the design does not carry would be lost: no rebuild then.
        let mut blocked = before.clone();
        blocked.unmodeled = vec!["triggers".into()];
        let plan = make_plan(DbEngine::Sqlite, Some(&blocked), &after);
        assert!(plan.statements.is_empty());
        assert!(plan
            .issues
            .iter()
            .any(|i| i.code == "rebuild_blocked" && i.value == "triggers"));
        // Renames still work in place.
        let mut renamed = blocked.clone();
        renamed.columns[1].name = "title".into();
        renamed.uniques[0].columns = vec!["title".into()];
        let plan = make_plan(DbEngine::Sqlite, Some(&blocked), &renamed);
        assert_eq!(plan.statements.len(), 1, "{:?}", plan.issues);
    }
}
