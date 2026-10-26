//! Cassandra / ScyllaDB: a table in a keyspace whose primary key is the
//! partition key (the first `partition_key_columns` key columns) followed by
//! clustering columns with their `clustering_order`; secondary indexes on one
//! column; table options in `WITH`. No unique keys, foreign keys, checks or
//! defaults, and every non-key column may be NULL. The key, the clustering
//! order and column types are fixed after creation: ALTER TABLE adds, drops
//! and (key columns only) renames columns and changes options. Statements run
//! one by one (CQL DDL is not transactional).
use super::*;

pub struct Cassandra;

const TYPES: &[TypeInfo] = &[
    ty("int", "numeric"),
    ty("bigint", "numeric"),
    ty("smallint", "numeric"),
    ty("tinyint", "numeric"),
    ty("varint", "numeric"),
    ty("decimal", "numeric"),
    ty("float", "numeric"),
    ty("double", "numeric"),
    ty("counter", "numeric"),
    ty("boolean", "other"),
    ty("text", "text"),
    ty("ascii", "text"),
    ty("uuid", "other"),
    ty("timeuuid", "other"),
    ty("timestamp", "time"),
    ty("date", "time"),
    ty("time", "time"),
    ty("duration", "time"),
    ty("blob", "binary"),
    ty("inet", "other"),
    ty("list<text>", "other"),
    ty("set<text>", "other"),
    ty("map<text, text>", "other"),
];

impl Cassandra {
    fn table(&self, design: &TableDesign) -> String {
        if design.database.is_empty() {
            self.quote(&design.name)
        } else {
            format!(
                "{}.{}",
                self.quote(&design.database),
                self.quote(&design.name)
            )
        }
    }

    /// Number of leading primary key columns that form the partition key.
    fn partition_count(&self, design: &TableDesign) -> usize {
        option(&self.info(), design, "partition_key_columns")
            .parse::<usize>()
            .unwrap_or(1)
            .clamp(1, design.primary_key.columns.len().max(1))
    }

    /// `clustering_order` as (column, descending); None when it does not
    /// parse. A name loaded from the server follows a rename of its column.
    fn clustering_order(&self, design: &TableDesign) -> Option<Vec<(String, bool)>> {
        let info = self.info();
        let text = option(&info, design, "clustering_order").trim();
        if text.is_empty() {
            return Some(Vec::new());
        }
        text.split(',')
            .map(|part| {
                let mut words = part.split_whitespace();
                let mut name = words.next()?.trim_matches('"').to_owned();
                if !design.columns.iter().any(|c| c.name == name) {
                    if let Some(column) = design
                        .columns
                        .iter()
                        .find(|c| c.original.as_deref() == Some(name.as_str()))
                    {
                        name = column.name.clone();
                    }
                }
                let descending = match words.next().map(str::to_ascii_uppercase).as_deref() {
                    None | Some("ASC") => false,
                    Some("DESC") => true,
                    Some(_) => return None,
                };
                words.next().is_none().then_some((name, descending))
            })
            .collect()
    }

    fn primary_key(&self, design: &TableDesign) -> String {
        let key = &design.primary_key.columns;
        let count = self.partition_count(design);
        let partition = quote_list(self, &key[..count]);
        let mut text = if count == 1 {
            partition
        } else {
            format!("({partition})")
        };
        if key.len() > count {
            text.push_str(", ");
            text.push_str(&quote_list(self, &key[count..]));
        }
        format!("PRIMARY KEY ({text})")
    }

    fn index(&self, design: &TableDesign, index: &IndexDesign) -> String {
        format!(
            "CREATE INDEX {} ON {} ({})",
            self.quote(&name_or(
                &self.info(),
                &index.name,
                &design.name,
                &index.columns,
                "idx"
            )),
            self.table(design),
            quote_list(self, &index.columns)
        )
    }

    /// The table options a design sets, as `name = value`.
    fn table_options(&self, design: &TableDesign, ids: &[&str]) -> Vec<String> {
        let mut out = Vec::new();
        for id in ids {
            match *id {
                "comment" => out.push(format!("comment = {}", self.literal(&design.comment))),
                "default_time_to_live" | "gc_grace_seconds" => {
                    let info = self.info();
                    let value = option(&info, design, id);
                    if !value.is_empty() {
                        out.push(format!("{id} = {value}"));
                    }
                }
                _ => {}
            }
        }
        out
    }
}

impl Dialect for Cassandra {
    fn info(&self) -> DialectInfo {
        DialectInfo {
            engine: DbEngine::Cassandra,
            features: DesignFeatures {
                primary_key: true,
                indexes: true,
                table_comment: true,
                alter: true,
                ..Default::default()
            },
            types: TYPES.to_vec(),
            default_type: "text",
            identity_kinds: vec![],
            default_presets: vec![],
            index_methods: vec![],
            fk_actions: vec![],
            options: vec![
                OptionInfo {
                    id: "partition_key_columns",
                    kind: OptionKind::Number,
                    choices: vec![],
                    default: "1",
                    create_only: true,
                },
                OptionInfo {
                    id: "clustering_order",
                    kind: OptionKind::Text,
                    choices: vec![],
                    default: "",
                    create_only: true,
                },
                OptionInfo {
                    id: "default_time_to_live",
                    kind: OptionKind::Number,
                    choices: vec![],
                    default: "",
                    create_only: false,
                },
                OptionInfo {
                    id: "gc_grace_seconds",
                    kind: OptionKind::Number,
                    choices: vec![],
                    default: "",
                    create_only: false,
                },
            ],
            max_identifier: 48,
        }
    }

    fn quote(&self, name: &str) -> String {
        crate::parser::cql::quote_name(name)
    }

    fn literal(&self, text: &str) -> String {
        format!("'{}'", text.replace('\'', "''"))
    }

    fn validate(&self, design: &TableDesign, issues: &mut Issues) {
        let key = &design.primary_key.columns;
        if key.is_empty() {
            issues.error("primary_key_required", "primary_key", &design.name);
            return;
        }
        let info = self.info();
        let requested = option(&info, design, "partition_key_columns");
        if !requested
            .parse::<usize>()
            .is_ok_and(|n| n >= 1 && n <= key.len())
        {
            issues.error("option_invalid", "options.partition_key_columns", requested);
        }
        let clustering = &key[self.partition_count(design)..];
        match self.clustering_order(design) {
            Some(order) => {
                // A prefix of the clustering columns, in key order.
                let names: Vec<&str> = order.iter().map(|(name, _)| name.as_str()).collect();
                let prefix: Vec<&str> = clustering
                    .iter()
                    .take(names.len())
                    .map(String::as_str)
                    .collect();
                if names != prefix {
                    issues.error(
                        "option_invalid",
                        "options.clustering_order",
                        option(&info, design, "clustering_order"),
                    );
                }
            }
            None => issues.error(
                "option_invalid",
                "options.clustering_order",
                option(&info, design, "clustering_order"),
            ),
        }
        let counter =
            |column: &ColumnDesign| column.data_type.trim().eq_ignore_ascii_case("counter");
        let counters = design
            .columns
            .iter()
            .any(|c| !key.contains(&c.name) && counter(c));
        for (i, column) in design.columns.iter().enumerate() {
            let in_key = key.contains(&column.name);
            if (in_key && counter(column)) || (counters && !in_key && !counter(column)) {
                issues.error(
                    "counter_mixed",
                    format!("columns.{i}.data_type"),
                    &column.name,
                );
            }
            if !column.default.trim().is_empty() {
                issues.warning(
                    "default_ignored",
                    format!("columns.{i}.default"),
                    &column.name,
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
        for (i, index) in design.indexes.iter().enumerate() {
            if index.columns.len() != 1 {
                issues.error(
                    "index_single_column",
                    format!("indexes.{i}.columns"),
                    &index.name,
                );
            }
            if index.unique {
                issues.error(
                    "unique_unsupported",
                    format!("indexes.{i}.unique"),
                    &index.name,
                );
            }
            if !index.method.is_empty() {
                issues.error(
                    "option_invalid",
                    format!("indexes.{i}.method"),
                    &index.method,
                );
            }
        }
    }

    fn create(&self, design: &TableDesign, _: &mut Issues) -> Vec<DesignStatement> {
        let mut lines: Vec<String> = design
            .columns
            .iter()
            .map(|column| format!("{} {}", self.quote(&column.name), column.data_type.trim()))
            .collect();
        lines.push(self.primary_key(design));
        let mut sql = format!(
            "CREATE TABLE {} (\n    {}\n)",
            self.table(design),
            lines.join(",\n    ")
        );
        let mut with = Vec::new();
        let order = self.clustering_order(design).unwrap_or_default();
        if !order.is_empty() {
            with.push(format!(
                "CLUSTERING ORDER BY ({})",
                order
                    .iter()
                    .map(|(name, descending)| format!(
                        "{} {}",
                        self.quote(name),
                        if *descending { "DESC" } else { "ASC" }
                    ))
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        }
        let mut ids = vec!["default_time_to_live", "gc_grace_seconds"];
        if !design.comment.is_empty() {
            ids.insert(0, "comment");
        }
        with.extend(self.table_options(design, &ids));
        if !with.is_empty() {
            sql.push_str(" WITH ");
            sql.push_str(&with.join(" AND "));
        }
        let mut statements = vec![statement(sql)];
        statements.extend(
            design
                .indexes
                .iter()
                .map(|index| statement(self.index(design, index))),
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
        if changes.renamed_table {
            issues.error("rename_unsupported", "table.name", &after.name);
        }
        if before.database != after.database {
            issues.error("move_unsupported", "table.database", &after.database);
        }
        if changes.primary_key_changed {
            issues.error("primary_key_change_unsupported", "primary_key", &after.name);
        }
        let mut options = Vec::new();
        for id in &changes.changed_options {
            match *id {
                "default_time_to_live" | "gc_grace_seconds" => options.push(*id),
                _ => issues.error("option_create_only", format!("options.{id}"), id),
            }
        }
        let table = self.table(before);
        let alter = |op: String| format!("ALTER TABLE {table} {op}");
        let info = self.info();
        let mut out = Vec::new();
        for index in &changes.dropped_indexes {
            let name = name_or(&info, &index.name, &before.name, &index.columns, "idx");
            let qualified = TableDesign {
                database: before.database.clone(),
                name,
                ..Default::default()
            };
            out.push(statement(format!("DROP INDEX {}", self.table(&qualified))));
        }
        for column in &changes.dropped_columns {
            out.push(destructive(alter(format!(
                "DROP {}",
                self.quote(&column.name)
            ))));
        }
        for (old, new, position) in &changes.kept_columns {
            if old.name != new.name {
                if after.primary_key.columns.contains(&new.name) {
                    out.push(statement(alter(format!(
                        "RENAME {} TO {}",
                        self.quote(&old.name),
                        self.quote(&new.name)
                    ))));
                } else {
                    issues.error(
                        "column_rename_unsupported",
                        format!("columns.{position}.name"),
                        &new.name,
                    );
                }
            }
            if !old
                .data_type
                .trim()
                .eq_ignore_ascii_case(new.data_type.trim())
            {
                issues.error(
                    "type_change_unsupported",
                    format!("columns.{position}.data_type"),
                    &new.name,
                );
            }
        }
        for (_, column) in &changes.added_columns {
            out.push(statement(alter(format!(
                "ADD {} {}",
                self.quote(&column.name),
                column.data_type.trim()
            ))));
        }
        for index in &changes.added_indexes {
            out.push(statement(self.index(before, index)));
        }
        if changes.comment_changed {
            options.insert(0, "comment");
        }
        let with = self.table_options(after, &options);
        if !with.is_empty() {
            out.push(statement(alter(format!("WITH {}", with.join(" AND ")))));
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::super::plan as make_plan;
    use super::super::test_support::{assert_guard_accepts, column, loaded};
    use super::*;

    fn readings() -> TableDesign {
        TableDesign {
            database: "metrics".into(),
            name: "readings".into(),
            comment: "sensor's data".into(),
            columns: vec![
                column("sensor", "uuid"),
                column("day", "date"),
                column("at", "timestamp"),
                column("value", "double"),
                column("Tags", "set<text>"),
            ],
            primary_key: KeyDesign {
                columns: vec!["sensor".into(), "day".into(), "at".into()],
                ..Default::default()
            },
            indexes: vec![IndexDesign {
                columns: vec!["value".into()],
                ..Default::default()
            }],
            options: [
                ("partition_key_columns".to_owned(), "2".to_owned()),
                ("clustering_order".to_owned(), "at DESC".to_owned()),
                ("default_time_to_live".to_owned(), "86400".to_owned()),
            ]
            .into(),
            ..Default::default()
        }
    }

    #[test]
    fn creates_a_table_with_partition_and_clustering_keys() {
        let plan = make_plan(DbEngine::Cassandra, None, &readings());
        assert_eq!(
            plan.statements[0].sql,
            "CREATE TABLE metrics.readings (\n    \
             sensor uuid,\n    \
             day date,\n    \
             at timestamp,\n    \
             value double,\n    \
             \"Tags\" set<text>,\n    \
             PRIMARY KEY ((sensor, day), at)\n\
             ) WITH CLUSTERING ORDER BY (at DESC) AND comment = 'sensor''s data' \
             AND default_time_to_live = 86400"
        );
        assert_eq!(
            plan.statements[1].sql,
            "CREATE INDEX readings_value_idx ON metrics.readings (value)"
        );
        assert!(!plan.transactional);
        assert_guard_accepts(DbEngine::Cassandra, &plan);
        let mut simple = readings();
        simple.options.clear();
        simple.comment.clear();
        simple.indexes.clear();
        let plan = make_plan(DbEngine::Cassandra, None, &simple);
        assert!(plan.statements[0]
            .sql
            .ends_with("PRIMARY KEY (sensor, day, at)\n)"));
        assert_guard_accepts(DbEngine::Cassandra, &plan);
    }

    #[test]
    fn requires_a_key_and_checks_the_options() {
        let mut design = readings();
        design.primary_key.columns.clear();
        let codes: Vec<_> = make_plan(DbEngine::Cassandra, None, &design)
            .issues
            .into_iter()
            .map(|issue| issue.code)
            .collect();
        assert!(codes.contains(&"primary_key_required"), "{codes:?}");
        let mut design = readings();
        design
            .options
            .insert("clustering_order".into(), "day DESC".into());
        design
            .options
            .insert("partition_key_columns".into(), "4".into());
        design.columns[3].data_type = "counter".into();
        design.uniques.push(KeyDesign {
            columns: vec!["value".into()],
            ..Default::default()
        });
        let codes: Vec<_> = make_plan(DbEngine::Cassandra, None, &design)
            .issues
            .into_iter()
            .map(|issue| issue.code)
            .collect();
        for code in ["option_invalid", "counter_mixed", "unique_unsupported"] {
            assert!(codes.contains(&code), "{code}: {codes:?}");
        }
    }

    #[test]
    fn alters_columns_options_and_indexes() {
        let before = loaded(readings());
        let mut after = before.clone();
        after.columns.remove(4);
        after.columns[2].name = "ts".into();
        after.primary_key.columns[2] = "ts".into();
        after.columns.push(column("unit", "text"));
        after.indexes.clear();
        after.comment = "readings".into();
        after
            .options
            .insert("gc_grace_seconds".into(), "3600".into());
        // The loaded clustering order still names `at`: it follows the rename.
        let plan = make_plan(DbEngine::Cassandra, Some(&before), &after);
        let sql: Vec<(&str, bool)> = plan
            .statements
            .iter()
            .map(|s| (s.sql.as_str(), s.destructive))
            .collect();
        let t = "ALTER TABLE metrics.readings";
        assert_eq!(
            sql,
            [
                ("DROP INDEX metrics.readings_value_idx", false),
                (format!("{t} DROP \"Tags\"").as_str(), true),
                (format!("{t} RENAME at TO ts").as_str(), false),
                (format!("{t} ADD unit text").as_str(), false),
                (
                    format!("{t} WITH comment = 'readings' AND gc_grace_seconds = 3600").as_str(),
                    false
                ),
            ]
        );
        assert_guard_accepts(DbEngine::Cassandra, &plan);

        let mut bad = before.clone();
        bad.indexes.clear();
        bad.name = "other".into();
        bad.columns[3].name = "reading".into();
        bad.columns[3].data_type = "float".into();
        let codes: Vec<_> = make_plan(DbEngine::Cassandra, Some(&before), &bad)
            .issues
            .into_iter()
            .map(|issue| issue.code)
            .collect();
        for code in [
            "rename_unsupported",
            "column_rename_unsupported",
            "type_change_unsupported",
        ] {
            assert!(codes.contains(&code), "{code}: {codes:?}");
        }
    }
}
