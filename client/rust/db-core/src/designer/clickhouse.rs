//! ClickHouse: table engines with their sorting key (ORDER BY is required by
//! the MergeTree family), PRIMARY KEY and PARTITION BY; `Nullable(T)` and
//! `LowCardinality(T)` instead of NOT NULL; no foreign keys, unique keys or
//! checks. DDL is not transactional.
use super::*;

pub struct ClickHouse;

const TYPES: &[TypeInfo] = &[
    ty("UInt8", "numeric"),
    ty("UInt16", "numeric"),
    ty("UInt32", "numeric"),
    ty("UInt64", "numeric"),
    ty("UInt128", "numeric"),
    ty("UInt256", "numeric"),
    ty("Int8", "numeric"),
    ty("Int16", "numeric"),
    ty("Int32", "numeric"),
    ty("Int64", "numeric"),
    ty("Int128", "numeric"),
    ty("Int256", "numeric"),
    ty("Float32", "numeric"),
    ty("Float64", "numeric"),
    ty_ps("Decimal", "numeric"),
    ty("Bool", "numeric"),
    ty("String", "text"),
    ty_len("FixedString", "text", true),
    ty("UUID", "other"),
    ty("Date", "time"),
    ty("Date32", "time"),
    ty("DateTime", "time"),
    ty_len("DateTime64", "time", true),
    ty("JSON", "json"),
    ty("IPv4", "other"),
    ty("IPv6", "other"),
];

const MERGE_TREES: [&str; 6] = [
    "MergeTree",
    "ReplacingMergeTree",
    "SummingMergeTree",
    "AggregatingMergeTree",
    "CollapsingMergeTree",
    "VersionedCollapsingMergeTree",
];

impl ClickHouse {
    fn table(&self, database: &str, name: &str) -> String {
        if database.is_empty() {
            self.quote(name)
        } else {
            format!("{}.{}", self.quote(database), self.quote(name))
        }
    }

    fn data_type(&self, column: &ColumnDesign) -> String {
        let mut text = column_type(column);
        if column.nullable {
            text = format!("Nullable({text})");
        }
        if column.low_cardinality {
            text = format!("LowCardinality({text})");
        }
        text
    }

    fn column(&self, column: &ColumnDesign) -> String {
        let mut line = format!("{} {}", self.quote(&column.name), self.data_type(column));
        if !column.default.trim().is_empty() {
            line.push_str(" DEFAULT ");
            line.push_str(&default_sql(self, &column.default));
        }
        if !column.comment.is_empty() {
            line.push_str(" COMMENT ");
            line.push_str(&self.literal(&column.comment));
        }
        line
    }

    /// Column names are quoted; anything else is an expression. Several
    /// parts become a tuple.
    fn key(&self, design: &TableDesign, text: &str) -> String {
        let parts: Vec<String> = split_top_level(text)
            .into_iter()
            .map(|part| {
                if design.columns.iter().any(|column| column.name == part) {
                    self.quote(&part)
                } else {
                    part
                }
            })
            .collect();
        match parts.as_slice() {
            [one] => one.clone(),
            _ => format!("({})", parts.join(", ")),
        }
    }

    /// The sorting key: the option, else the primary key columns.
    fn order_by(&self, design: &TableDesign) -> String {
        let info = self.info();
        let text = option(&info, design, "order_by").trim().to_owned();
        if text.is_empty() {
            design.primary_key.columns.join(", ")
        } else {
            text
        }
    }
}

/// Splits on commas outside parentheses and quotes; parts are trimmed.
pub(super) fn split_top_level(text: &str) -> Vec<String> {
    let mut parts = Vec::new();
    let mut current = String::new();
    let mut depth = 0;
    let mut quote: Option<char> = None;
    for c in text.chars() {
        match quote {
            Some(q) => {
                if c == q {
                    quote = None;
                }
            }
            None => match c {
                '\'' | '`' | '"' => quote = Some(c),
                '(' => depth += 1,
                ')' => depth -= 1,
                ',' if depth == 0 => {
                    parts.push(current.trim().to_owned());
                    current.clear();
                    continue;
                }
                _ => {}
            },
        }
        current.push(c);
    }
    if !current.trim().is_empty() {
        parts.push(current.trim().to_owned());
    }
    parts
}

impl Dialect for ClickHouse {
    fn info(&self) -> DialectInfo {
        let mut engines = MERGE_TREES.to_vec();
        engines.extend(["Memory", "Log", "TinyLog", "StripeLog"]);
        DialectInfo {
            engine: DbEngine::Clickhouse,
            features: DesignFeatures {
                primary_key: true,
                nullable_wrapper: true,
                low_cardinality: true,
                column_comments: true,
                alter: true,
                ..Default::default()
            },
            types: TYPES.to_vec(),
            default_type: "UInt64",
            identity_kinds: vec![],
            default_presets: vec!["now()", "today()", "generateUUIDv4()", "0", "''", "NULL"],
            index_methods: vec![],
            fk_actions: vec![],
            options: vec![
                OptionInfo {
                    id: "engine",
                    kind: OptionKind::Choice,
                    choices: engines,
                    default: "MergeTree",
                    create_only: true,
                },
                OptionInfo {
                    id: "engine_args",
                    kind: OptionKind::Text,
                    choices: vec![],
                    default: "",
                    create_only: true,
                },
                OptionInfo {
                    id: "order_by",
                    kind: OptionKind::Text,
                    choices: vec![],
                    default: "",
                    create_only: true,
                },
                OptionInfo {
                    id: "partition_by",
                    kind: OptionKind::Text,
                    choices: vec![],
                    default: "",
                    create_only: true,
                },
            ],
            max_identifier: 255,
        }
    }

    fn quote(&self, name: &str) -> String {
        format!("`{}`", name.replace('\\', "\\\\").replace('`', "\\`"))
    }

    fn literal(&self, text: &str) -> String {
        format!("'{}'", text.replace('\\', "\\\\").replace('\'', "\\'"))
    }

    fn validate(&self, design: &TableDesign, issues: &mut Issues) {
        let info = self.info();
        let engine = option(&info, design, "engine");
        let merge_tree = MERGE_TREES.contains(&engine);
        let order = split_top_level(&self.order_by(design));
        if merge_tree && order.is_empty() {
            issues.error("order_by_required", "options.order_by", "");
        }
        if !merge_tree {
            if !design.primary_key.columns.is_empty() {
                issues.error("primary_key_engine", "primary_key", engine);
            }
            for id in ["order_by", "partition_by"] {
                if !option(&info, design, id).is_empty() {
                    issues.error("option_engine", format!("options.{id}"), engine);
                }
            }
        }
        let key = &design.primary_key.columns;
        if merge_tree && !key.is_empty() && !order.is_empty() {
            let prefix = order
                .iter()
                .take(key.len())
                .map(|part| part.trim_matches('`'));
            if !prefix.eq(key.iter().map(String::as_str)) {
                issues.error("primary_key_prefix", "options.order_by", &key.join(", "));
            }
        }
        for part in &order {
            if let Some((i, column)) = design
                .columns
                .iter()
                .enumerate()
                .find(|(_, c)| c.name == *part)
            {
                if column.nullable {
                    issues.error(
                        "nullable_key",
                        format!("columns.{i}.nullable"),
                        &column.name,
                    );
                }
            }
        }
        let args = option(&info, design, "engine_args");
        if !args.is_empty() && engine == "MergeTree" {
            issues.warning("engine_args_ignored", "options.engine_args", args);
        }
        // The guard's parser cannot read an escaped backquote.
        if design.name.contains('`') || design.name.contains('\\') {
            issues.error("name_invalid", "table.name", &design.name);
        }
        for (i, column) in design.columns.iter().enumerate() {
            if column.name.contains('`') || column.name.contains('\\') {
                issues.error("name_invalid", format!("columns.{i}.name"), &column.name);
            }
            if column.data_type.trim_start().starts_with("Nullable(") && column.nullable {
                issues.error(
                    "nullable_twice",
                    format!("columns.{i}.data_type"),
                    &column.name,
                );
            }
        }
    }

    fn create(&self, design: &TableDesign, _: &mut Issues) -> Vec<DesignStatement> {
        let info = self.info();
        let engine = option(&info, design, "engine");
        let lines: Vec<String> = design.columns.iter().map(|c| self.column(c)).collect();
        let args = option(&info, design, "engine_args");
        let mut sql = format!(
            "CREATE TABLE {} (\n    {}\n) ENGINE = {engine}",
            self.table(&design.database, &design.name),
            lines.join(",\n    ")
        );
        if !args.is_empty() && engine != "MergeTree" {
            sql.push_str(&format!(
                "({})",
                self.key(design, args)
                    .trim_start_matches('(')
                    .trim_end_matches(')')
            ));
        }
        if MERGE_TREES.contains(&engine) {
            // The guard's parser takes the clauses in this order only.
            if !design.primary_key.columns.is_empty() {
                sql.push_str(&format!(
                    " PRIMARY KEY ({})",
                    quote_list(self, &design.primary_key.columns)
                ));
            }
            sql.push_str(&format!(
                " ORDER BY {}",
                self.key(design, &self.order_by(design))
            ));
            let partition = option(&info, design, "partition_by");
            if !partition.is_empty() {
                sql.push_str(&format!(" PARTITION BY {}", self.key(design, partition)));
            }
        }
        vec![statement(sql)]
    }

    fn alter(
        &self,
        before: &TableDesign,
        after: &TableDesign,
        changes: &TableChanges<'_>,
        issues: &mut Issues,
    ) -> Vec<DesignStatement> {
        let table = self.table(&before.database, &before.name);
        let alter = |op: String| format!("ALTER TABLE {table} {op}");
        let mut out = Vec::new();
        if changes.primary_key_changed {
            issues.error("primary_key_change_unsupported", "primary_key", &after.name);
        }
        for id in &changes.changed_options {
            issues.error("option_create_only", format!("options.{id}"), id);
        }
        for column in &changes.dropped_columns {
            out.push(destructive(alter(format!(
                "DROP COLUMN {}",
                self.quote(&column.name)
            ))));
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
        for (old, new, _) in &changes.kept_columns {
            if !TableChanges::definition_changed(old, new) {
                continue;
            }
            let retyped = self.data_type(old) != self.data_type(new);
            out.push(DesignStatement {
                sql: alter(format!("MODIFY COLUMN {}", self.column(new))),
                destructive: retyped,
            });
            // MODIFY COLUMN keeps a default or comment it does not mention.
            let name = self.quote(&new.name);
            if !old.default.trim().is_empty() && new.default.trim().is_empty() {
                out.push(statement(alter(format!(
                    "MODIFY COLUMN {name} REMOVE DEFAULT"
                ))));
            }
            if !old.comment.is_empty() && new.comment.is_empty() {
                out.push(statement(alter(format!(
                    "MODIFY COLUMN {name} REMOVE COMMENT"
                ))));
            }
        }
        let last = after
            .columns
            .len()
            .saturating_sub(changes.added_columns.len());
        for (position, column) in &changes.added_columns {
            if *position < last {
                issues.warning(
                    "column_order_ignored",
                    format!("columns.{position}.name"),
                    &column.name,
                );
            }
            out.push(statement(alter(format!(
                "ADD COLUMN {}",
                self.column(column)
            ))));
        }
        // Everything above addresses the current name; the rename runs last.
        if changes.renamed_table || before.database != after.database {
            out.push(statement(format!(
                "RENAME TABLE {table} TO {}",
                self.table(&after.database, &after.name)
            )));
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::super::plan as make_plan;
    use super::super::test_support::{assert_guard_accepts, column, loaded};
    use super::*;

    fn events() -> TableDesign {
        TableDesign {
            database: "demo".into(),
            name: "events".into(),
            columns: vec![
                ColumnDesign {
                    nullable: false,
                    ..column("id", "UInt64")
                },
                ColumnDesign {
                    nullable: false,
                    default: "now()".into(),
                    ..column("at", "DateTime")
                },
                ColumnDesign {
                    nullable: false,
                    low_cardinality: true,
                    comment: "it's a kind".into(),
                    ..column("kind", "String")
                },
                ColumnDesign {
                    default: "NULL".into(),
                    ..column("note", "String")
                },
                ColumnDesign {
                    nullable: false,
                    length: Some(10),
                    scale: Some(2),
                    ..column("amount", "Decimal")
                },
            ],
            primary_key: KeyDesign {
                columns: vec!["id".into()],
                ..Default::default()
            },
            options: [
                ("order_by".to_owned(), "id, at".to_owned()),
                ("partition_by".to_owned(), "toYYYYMM(at)".to_owned()),
            ]
            .into(),
            ..Default::default()
        }
    }

    #[test]
    fn creates_a_merge_tree_with_keys() {
        let plan = make_plan(DbEngine::Clickhouse, None, &events());
        assert_eq!(
            plan.statements[0].sql,
            "CREATE TABLE `demo`.`events` (\n    \
             `id` UInt64,\n    \
             `at` DateTime DEFAULT now(),\n    \
             `kind` LowCardinality(String) COMMENT 'it\\'s a kind',\n    \
             `note` Nullable(String) DEFAULT NULL,\n    \
             `amount` Decimal(10,2)\n\
             ) ENGINE = MergeTree PRIMARY KEY (`id`) ORDER BY (`id`, `at`) PARTITION BY toYYYYMM(at)"
        );
        assert!(!plan.transactional);
        assert_guard_accepts(DbEngine::Clickhouse, &plan);
    }

    #[test]
    fn replacing_engine_and_memory() {
        let mut design = events();
        design
            .options
            .insert("engine".into(), "ReplacingMergeTree".into());
        design.options.insert("engine_args".into(), "at".into());
        design.options.remove("partition_by");
        let plan = make_plan(DbEngine::Clickhouse, None, &design);
        assert!(plan.statements[0].sql.ends_with(
            "ENGINE = ReplacingMergeTree(`at`) PRIMARY KEY (`id`) ORDER BY (`id`, `at`)"
        ));
        assert_guard_accepts(DbEngine::Clickhouse, &plan);
        let memory = TableDesign {
            database: "demo".into(),
            name: "scratch".into(),
            columns: vec![column("x", "String")],
            options: [("engine".to_owned(), "Memory".to_owned())].into(),
            ..Default::default()
        };
        let plan = make_plan(DbEngine::Clickhouse, None, &memory);
        assert_eq!(
            plan.statements[0].sql,
            "CREATE TABLE `demo`.`scratch` (\n    `x` Nullable(String)\n) ENGINE = Memory"
        );
        assert_guard_accepts(DbEngine::Clickhouse, &plan);
    }

    #[test]
    fn requires_a_sorting_key_and_rejects_sql_only_items() {
        let mut design = events();
        design.primary_key.columns.clear();
        design.options.remove("order_by");
        design.uniques.push(KeyDesign {
            columns: vec!["id".into()],
            ..Default::default()
        });
        design.foreign_keys.push(ForeignKeyDesign {
            columns: vec!["id".into()],
            ref_table: "x".into(),
            ref_columns: vec!["id".into()],
            ..Default::default()
        });
        let codes: Vec<_> = make_plan(DbEngine::Clickhouse, None, &design)
            .issues
            .into_iter()
            .map(|issue| issue.code)
            .collect();
        for code in [
            "order_by_required",
            "unique_unsupported",
            "foreign_keys_unsupported",
        ] {
            assert!(codes.contains(&code), "{code}: {codes:?}");
        }
        let mut design = events();
        design.options.insert("order_by".into(), "at, id".into());
        design.columns[1].nullable = true;
        let codes: Vec<_> = make_plan(DbEngine::Clickhouse, None, &design)
            .issues
            .into_iter()
            .map(|issue| issue.code)
            .collect();
        assert!(
            codes.contains(&"primary_key_prefix") && codes.contains(&"nullable_key"),
            "{codes:?}"
        );
    }

    #[test]
    fn alters_columns() {
        let before = loaded(events());
        let mut after = before.clone();
        after.columns[3].name = "remark".into();
        after.columns[3].nullable = false;
        after.columns[3].default = "''".into();
        after.columns.remove(4);
        after.columns.push(column("source", "String"));
        let plan = make_plan(DbEngine::Clickhouse, Some(&before), &after);
        let t = "ALTER TABLE `demo`.`events`";
        let sql: Vec<(String, bool)> = plan
            .statements
            .iter()
            .map(|s| (s.sql.clone(), s.destructive))
            .collect();
        assert_eq!(
            sql,
            [
                (format!("{t} DROP COLUMN `amount`"), true),
                (format!("{t} RENAME COLUMN `note` TO `remark`"), false),
                (
                    format!("{t} MODIFY COLUMN `remark` String DEFAULT ''"),
                    true
                ),
                (format!("{t} ADD COLUMN `source` Nullable(String)"), false),
            ]
        );
        assert_guard_accepts(DbEngine::Clickhouse, &plan);
        let mut renamed = before.clone();
        renamed.name = "other".into();
        let plan = make_plan(DbEngine::Clickhouse, Some(&before), &renamed);
        assert!(plan.issues.is_empty(), "{:?}", plan.issues);
        let sql: Vec<&str> = plan.statements.iter().map(|s| s.sql.as_str()).collect();
        assert_eq!(sql, ["RENAME TABLE `demo`.`events` TO `demo`.`other`"]);
        assert_guard_accepts(DbEngine::Clickhouse, &plan);

        // A removed default or comment is removed explicitly (LIM-06).
        let mut cleared = before.clone();
        cleared.columns[1].default = String::new();
        cleared.columns[2].comment = String::new();
        let plan = make_plan(DbEngine::Clickhouse, Some(&before), &cleared);
        assert!(plan.issues.is_empty(), "{:?}", plan.issues);
        let sql: Vec<&str> = plan.statements.iter().map(|s| s.sql.as_str()).collect();
        assert_eq!(
            sql,
            [
                "ALTER TABLE `demo`.`events` MODIFY COLUMN `at` DateTime",
                "ALTER TABLE `demo`.`events` MODIFY COLUMN `at` REMOVE DEFAULT",
                "ALTER TABLE `demo`.`events` MODIFY COLUMN `kind` LowCardinality(String)",
                "ALTER TABLE `demo`.`events` MODIFY COLUMN `kind` REMOVE COMMENT",
            ]
        );
        assert_guard_accepts(DbEngine::Clickhouse, &plan);
    }

    #[test]
    fn splits_keys_outside_parentheses() {
        assert_eq!(
            split_top_level("a, toDate(b, 'x,y'), c"),
            ["a", "toDate(b, 'x,y')", "c"]
        );
        assert!(split_top_level("  ").is_empty());
    }
}
