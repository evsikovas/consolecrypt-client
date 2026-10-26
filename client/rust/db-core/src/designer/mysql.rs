//! MySQL and MariaDB: keys and indexes inline, AUTO_INCREMENT and UNSIGNED,
//! column and table comments, ENGINE/CHARSET/COLLATE. DDL commits
//! implicitly, so a failed script leaves the statements before it applied.
use super::*;

pub struct Mysql {
    mariadb: bool,
}

pub static MYSQL: Mysql = Mysql { mariadb: false };
pub static MARIADB: Mysql = Mysql { mariadb: true };

const TYPES: &[TypeInfo] = &[
    ty_len("tinyint", "numeric", false),
    ty("smallint", "numeric"),
    ty("mediumint", "numeric"),
    ty("int", "numeric"),
    ty("bigint", "numeric"),
    ty_ps("decimal", "numeric"),
    ty("float", "numeric"),
    ty("double", "numeric"),
    ty_len("bit", "binary", false),
    ty("boolean", "numeric"),
    ty_len("char", "text", false),
    ty_len("varchar", "text", true),
    ty("tinytext", "text"),
    ty("text", "text"),
    ty("mediumtext", "text"),
    ty("longtext", "text"),
    ty_len("binary", "binary", false),
    ty_len("varbinary", "binary", true),
    ty("blob", "binary"),
    ty("mediumblob", "binary"),
    ty("longblob", "binary"),
    ty("json", "json"),
    ty("date", "time"),
    ty_len("time", "time", false),
    ty_len("datetime", "time", false),
    ty_len("timestamp", "time", false),
    ty("year", "time"),
];

const NUMERIC: [&str; 9] = [
    "tinyint",
    "smallint",
    "mediumint",
    "int",
    "integer",
    "bigint",
    "decimal",
    "float",
    "double",
];

fn plain_word(text: &str) -> bool {
    !text.is_empty() && text.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
}

impl Mysql {
    fn table(&self, database: &str, name: &str) -> String {
        if database.is_empty() {
            self.quote(name)
        } else {
            format!("{}.{}", self.quote(database), self.quote(name))
        }
    }

    fn column(&self, column: &ColumnDesign) -> String {
        let mut line = format!("{} {}", self.quote(&column.name), column_type(column));
        if column.unsigned {
            line.push_str(" UNSIGNED");
        }
        if !column.nullable {
            line.push_str(" NOT NULL");
        }
        if !column.default.trim().is_empty() {
            line.push_str(" DEFAULT ");
            line.push_str(&default_sql(self, &column.default));
        }
        if column.identity == "auto_increment" {
            line.push_str(" AUTO_INCREMENT");
        }
        if !column.comment.is_empty() {
            line.push_str(" COMMENT ");
            line.push_str(&self.literal(&column.comment));
        }
        line
    }

    fn unique(&self, design: &TableDesign, key: &KeyDesign) -> String {
        format!(
            "CONSTRAINT {} UNIQUE ({})",
            self.quote(&name_or(
                &self.info(),
                &key.name,
                &design.name,
                &key.columns,
                "key"
            )),
            quote_list(self, &key.columns)
        )
    }

    fn check(&self, design: &TableDesign, check: &CheckDesign) -> String {
        format!(
            "CONSTRAINT {} CHECK ({})",
            self.quote(&name_or(
                &self.info(),
                &check.name,
                &design.name,
                &[],
                "chk"
            )),
            check.expression.trim()
        )
    }

    fn foreign_key(&self, design: &TableDesign, fk: &ForeignKeyDesign) -> String {
        let database = fk
            .ref_schema
            .as_deref()
            .filter(|database| !database.is_empty())
            .unwrap_or(&design.database);
        let mut sql = format!(
            "CONSTRAINT {} FOREIGN KEY ({}) REFERENCES {} ({})",
            self.quote(&name_or(
                &self.info(),
                &fk.name,
                &design.name,
                &fk.columns,
                "fk"
            )),
            quote_list(self, &fk.columns),
            self.table(database, &fk.ref_table),
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

    /// `[UNIQUE|FULLTEXT] INDEX name (cols) [USING method]`.
    fn index(&self, design: &TableDesign, index: &IndexDesign) -> String {
        let kind = match (index.method.as_str(), index.unique) {
            ("FULLTEXT", _) => "FULLTEXT ",
            (_, true) => "UNIQUE ",
            _ => "",
        };
        let mut sql = format!(
            "{kind}INDEX {} ({})",
            self.quote(&name_or(
                &self.info(),
                &index.name,
                &design.name,
                &index.columns,
                "idx"
            )),
            quote_list(self, &index.columns)
        );
        if matches!(index.method.as_str(), "BTREE" | "HASH") {
            sql.push_str(&format!(" USING {}", index.method));
        }
        sql
    }
}

impl Dialect for Mysql {
    fn info(&self) -> DialectInfo {
        let mut engines = vec!["InnoDB", "MyISAM", "MEMORY", "ARCHIVE"];
        if self.mariadb {
            engines.push("Aria");
        }
        DialectInfo {
            engine: if self.mariadb {
                DbEngine::Mariadb
            } else {
                DbEngine::Mysql
            },
            features: DesignFeatures {
                primary_key: true,
                unique: true,
                foreign_keys: true,
                checks: true,
                indexes: true,
                unsigned: true,
                table_comment: true,
                column_comments: true,
                alter: true,
                ..Default::default()
            },
            types: TYPES.to_vec(),
            default_type: "int",
            identity_kinds: vec!["auto_increment"],
            default_presets: vec![
                "CURRENT_TIMESTAMP",
                "(uuid())",
                "0",
                "''",
                "NULL",
                "(JSON_OBJECT())",
            ],
            index_methods: vec!["BTREE", "HASH", "FULLTEXT"],
            fk_actions: vec!["RESTRICT", "CASCADE", "SET NULL", "NO ACTION"],
            options: vec![
                OptionInfo {
                    id: "engine",
                    kind: OptionKind::Choice,
                    choices: engines,
                    default: "InnoDB",
                    create_only: true,
                },
                OptionInfo {
                    id: "charset",
                    kind: OptionKind::Text,
                    choices: vec![],
                    default: "utf8mb4",
                    create_only: true,
                },
                OptionInfo {
                    id: "collation",
                    kind: OptionKind::Text,
                    choices: vec![],
                    default: "",
                    create_only: true,
                },
                OptionInfo {
                    id: "auto_increment",
                    kind: OptionKind::Number,
                    choices: vec![],
                    default: "",
                    create_only: true,
                },
            ],
            max_identifier: 64,
        }
    }

    fn quote(&self, name: &str) -> String {
        format!("`{}`", name.replace('`', "``"))
    }

    fn literal(&self, text: &str) -> String {
        format!("'{}'", text.replace('\\', "\\\\").replace('\'', "''"))
    }

    fn validate(&self, design: &TableDesign, issues: &mut Issues) {
        let info = self.info();
        for id in ["charset", "collation"] {
            let value = option(&info, design, id);
            if !value.is_empty() && !plain_word(value) {
                issues.error("option_invalid", format!("options.{id}"), value);
            }
        }
        let engine = option(&info, design, "engine");
        if engine != "InnoDB" && !design.foreign_keys.is_empty() {
            issues.error("foreign_keys_engine", "foreign_keys", engine);
        }
        let keyed = |name: &str| {
            design.primary_key.columns.first().map(String::as_str) == Some(name)
                || design
                    .uniques
                    .iter()
                    .any(|k| k.columns.first().map(String::as_str) == Some(name))
                || design
                    .indexes
                    .iter()
                    .any(|k| k.columns.first().map(String::as_str) == Some(name))
        };
        let mut auto = 0;
        for (i, column) in design.columns.iter().enumerate() {
            let base = column.data_type.trim().to_ascii_lowercase();
            if column.identity == "auto_increment" {
                auto += 1;
                if !keyed(&column.name) {
                    issues.error(
                        "auto_increment_key",
                        format!("columns.{i}.identity"),
                        &column.name,
                    );
                }
                if !NUMERIC.contains(&base.as_str()) {
                    issues.error(
                        "identity_type",
                        format!("columns.{i}.identity"),
                        &column.name,
                    );
                }
            }
            if column.unsigned && !NUMERIC.contains(&base.as_str()) {
                issues.error(
                    "unsigned_type",
                    format!("columns.{i}.unsigned"),
                    &column.name,
                );
            }
        }
        if auto > 1 {
            issues.error("auto_increment_single", "columns", "");
        }
        // The guard's parser keeps a backslash in a comment as written, and
        // the server reads it as an escape: the texts would differ.
        for (i, column) in design.columns.iter().enumerate() {
            if column.comment.contains('\\') {
                issues.error(
                    "comment_backslash_unsupported",
                    format!("columns.{i}.comment"),
                    &column.name,
                );
            }
        }
        if design.comment.contains('\\') {
            issues.error(
                "comment_backslash_unsupported",
                "table.comment",
                &design.name,
            );
        }
    }

    fn create(&self, design: &TableDesign, _: &mut Issues) -> Vec<DesignStatement> {
        let info = self.info();
        let mut lines: Vec<String> = design.columns.iter().map(|c| self.column(c)).collect();
        if !design.primary_key.columns.is_empty() {
            lines.push(format!(
                "PRIMARY KEY ({})",
                quote_list(self, &design.primary_key.columns)
            ));
        }
        lines.extend(design.uniques.iter().map(|key| self.unique(design, key)));
        lines.extend(design.indexes.iter().map(|index| self.index(design, index)));
        lines.extend(design.checks.iter().map(|check| self.check(design, check)));
        lines.extend(
            design
                .foreign_keys
                .iter()
                .map(|fk| self.foreign_key(design, fk)),
        );
        let mut sql = format!(
            "CREATE TABLE {} (\n    {}\n) ENGINE = {}",
            self.table(&design.database, &design.name),
            lines.join(",\n    "),
            option(&info, design, "engine")
        );
        let charset = option(&info, design, "charset");
        if !charset.is_empty() {
            sql.push_str(&format!(" DEFAULT CHARSET = {charset}"));
        }
        let collation = option(&info, design, "collation");
        if !collation.is_empty() {
            sql.push_str(&format!(" COLLATE = {collation}"));
        }
        let start = option(&info, design, "auto_increment");
        if !start.is_empty() {
            sql.push_str(&format!(" AUTO_INCREMENT = {start}"));
        }
        if !design.comment.is_empty() {
            sql.push_str(&format!(" COMMENT = {}", self.literal(&design.comment)));
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
        if before.database != after.database {
            issues.error("move_unsupported", "table.database", &after.database);
        }
        for id in &changes.changed_options {
            issues.warning("option_change_unsupported", format!("options.{id}"), id);
        }
        if changes.comment_changed {
            issues.warning("comment_change_unsupported", "table.comment", &after.name);
        }
        let info = self.info();
        let named = |name: &str, columns: &[String], suffix: &str| {
            name_or(&info, name, &before.name, columns, suffix)
        };
        for fk in &changes.dropped_foreign_keys {
            let name = named(&fk.name, &fk.columns, "fk");
            out.push(statement(alter(format!(
                "DROP FOREIGN KEY {}",
                self.quote(&name)
            ))));
        }
        for check in &changes.dropped_checks {
            let name = named(&check.name, &[], "chk");
            out.push(statement(alter(format!(
                "DROP CONSTRAINT {}",
                self.quote(&name)
            ))));
        }
        for key in &changes.dropped_uniques {
            let name = named(&key.name, &key.columns, "key");
            out.push(statement(alter(format!(
                "DROP INDEX {}",
                self.quote(&name)
            ))));
        }
        for index in &changes.dropped_indexes {
            let name = named(&index.name, &index.columns, "idx");
            out.push(statement(alter(format!(
                "DROP INDEX {}",
                self.quote(&name)
            ))));
        }
        if changes.primary_key_changed && !before.primary_key.columns.is_empty() {
            out.push(destructive(alter("DROP PRIMARY KEY".into())));
        }
        for column in &changes.dropped_columns {
            out.push(destructive(alter(format!(
                "DROP COLUMN {}",
                self.quote(&column.name)
            ))));
        }
        for (old, new, _) in &changes.kept_columns {
            let changed = TableChanges::definition_changed(old, new);
            let retyped = (&old.data_type, old.length, old.scale, old.unsigned)
                != (&new.data_type, new.length, new.scale, new.unsigned);
            let sql = match (old.name != new.name, changed) {
                (true, true) => alter(format!(
                    "CHANGE COLUMN {} {}",
                    self.quote(&old.name),
                    self.column(new)
                )),
                (true, false) => alter(format!(
                    "RENAME COLUMN {} TO {}",
                    self.quote(&old.name),
                    self.quote(&new.name)
                )),
                (false, true) => alter(format!("MODIFY COLUMN {}", self.column(new))),
                (false, false) => continue,
            };
            out.push(DesignStatement {
                sql,
                destructive: retyped,
            });
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
            if !column.nullable && column.default.trim().is_empty() && column.identity.is_empty() {
                issues.warning(
                    "not_null_without_default",
                    format!("columns.{position}.nullable"),
                    &column.name,
                );
            }
            out.push(statement(alter(format!(
                "ADD COLUMN {}",
                self.column(column)
            ))));
        }
        if changes.primary_key_changed && !after.primary_key.columns.is_empty() {
            out.push(statement(alter(format!(
                "ADD PRIMARY KEY ({})",
                quote_list(self, &after.primary_key.columns)
            ))));
        }
        for key in &changes.added_uniques {
            out.push(statement(alter(format!("ADD {}", self.unique(after, key)))));
        }
        for index in &changes.added_indexes {
            out.push(statement(alter(format!(
                "ADD {}",
                self.index(after, index)
            ))));
        }
        for check in &changes.added_checks {
            out.push(statement(alter(format!(
                "ADD {}",
                self.check(after, check)
            ))));
        }
        for fk in &changes.added_foreign_keys {
            out.push(statement(alter(format!(
                "ADD {}",
                self.foreign_key(after, fk)
            ))));
        }
        if changes.renamed_table {
            out.push(statement(alter(format!(
                "RENAME TO {}",
                self.table(&before.database, &after.name)
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

    fn users() -> TableDesign {
        TableDesign {
            database: "demo".into(),
            name: "users".into(),
            comment: "Accounts".into(),
            columns: vec![
                ColumnDesign {
                    identity: "auto_increment".into(),
                    unsigned: true,
                    nullable: false,
                    comment: "Key".into(),
                    ..column("id", "bigint")
                },
                ColumnDesign {
                    length: Some(255),
                    nullable: false,
                    default: "a\\b".into(),
                    ..column("email", "varchar")
                },
                ColumnDesign {
                    length: Some(3),
                    default: "CURRENT_TIMESTAMP(3)".into(),
                    ..column("created", "datetime")
                },
                column("group_id", "int"),
                column("order", "json"),
            ],
            primary_key: KeyDesign {
                columns: vec!["id".into()],
                ..Default::default()
            },
            uniques: vec![KeyDesign {
                columns: vec!["email".into()],
                ..Default::default()
            }],
            indexes: vec![IndexDesign {
                name: "users_created".into(),
                columns: vec!["created".into()],
                method: "BTREE".into(),
                ..Default::default()
            }],
            foreign_keys: vec![ForeignKeyDesign {
                name: "users_group".into(),
                columns: vec!["group_id".into()],
                ref_table: "groups".into(),
                ref_columns: vec!["id".into()],
                on_delete: "SET NULL".into(),
                ..Default::default()
            }],
            checks: vec![CheckDesign {
                name: "users_email".into(),
                expression: "`email` <> ''".into(),
                ..Default::default()
            }],
            options: [("collation".to_owned(), "utf8mb4_unicode_ci".to_owned())].into(),
            ..Default::default()
        }
    }

    #[test]
    fn creates_a_table_with_inline_keys_and_options() {
        let plan = make_plan(DbEngine::Mysql, None, &users());
        assert_eq!(plan.statements.len(), 1);
        assert_eq!(
            plan.statements[0].sql,
            "CREATE TABLE `demo`.`users` (\n    \
             `id` bigint UNSIGNED NOT NULL AUTO_INCREMENT COMMENT 'Key',\n    \
             `email` varchar(255) NOT NULL DEFAULT 'a\\\\b',\n    \
             `created` datetime(3) DEFAULT CURRENT_TIMESTAMP(3),\n    \
             `group_id` int,\n    \
             `order` json,\n    \
             PRIMARY KEY (`id`),\n    \
             CONSTRAINT `users_email_key` UNIQUE (`email`),\n    \
             INDEX `users_created` (`created`) USING BTREE,\n    \
             CONSTRAINT `users_email` CHECK (`email` <> ''),\n    \
             CONSTRAINT `users_group` FOREIGN KEY (`group_id`) REFERENCES `demo`.`groups` (`id`) ON DELETE SET NULL\n\
             ) ENGINE = InnoDB DEFAULT CHARSET = utf8mb4 COLLATE = utf8mb4_unicode_ci COMMENT = 'Accounts'"
        );
        assert!(!plan.transactional);
        assert_guard_accepts(DbEngine::Mysql, &plan);
    }

    #[test]
    fn engine_rules() {
        let mut design = users();
        design.options.insert("engine".into(), "MyISAM".into());
        design.columns[1].identity = "auto_increment".into();
        design.columns[2].unsigned = true;
        design.options.insert("charset".into(), "utf8; DROP".into());
        design.comment = r"C:\temp".into();
        let codes: Vec<_> = make_plan(DbEngine::Mysql, None, &design)
            .issues
            .into_iter()
            .map(|issue| issue.code)
            .collect();
        for code in [
            "foreign_keys_engine",
            "identity_type",
            "unsigned_type",
            "auto_increment_single",
            "option_invalid",
            "comment_backslash_unsupported",
        ] {
            assert!(codes.contains(&code), "{code}: {codes:?}");
        }
    }

    #[test]
    fn comments_with_apostrophes_pass_the_guard() {
        let mut design = users();
        design.comment = "Bob's accounts".into();
        design.columns[1].comment = "it's the key".into();
        let plan = make_plan(DbEngine::Mysql, None, &design);
        assert!(plan.issues.is_empty(), "{:?}", plan.issues);
        assert!(plan.statements[0]
            .sql
            .contains("COMMENT = 'Bob''s accounts'"));
        assert_guard_accepts(DbEngine::Mysql, &plan);
    }

    #[test]
    fn alters_with_change_modify_and_rename() {
        let before = loaded(users());
        let mut after = before.clone();
        after.name = "accounts".into();
        after.columns[1].name = "mail".into();
        after.columns[1].length = Some(320);
        after.uniques[0].columns = vec!["mail".into()];
        after.columns[2].comment = "When".into();
        after.columns[4].name = "settings".into();
        after.columns.remove(3);
        after.foreign_keys.clear();
        after.columns.push(ColumnDesign {
            nullable: false,
            default: "1".into(),
            ..column("active", "boolean")
        });
        after.indexes.push(IndexDesign {
            name: "accounts_active".into(),
            columns: vec!["active".into()],
            ..Default::default()
        });
        let plan = make_plan(DbEngine::Mysql, Some(&before), &after);
        let t = "ALTER TABLE `demo`.`users`";
        let sql: Vec<(String, bool)> = plan
            .statements
            .iter()
            .map(|s| (s.sql.clone(), s.destructive))
            .collect();
        assert_eq!(
            sql,
            [
                (format!("{t} DROP FOREIGN KEY `users_group`"), false),
                (format!("{t} DROP COLUMN `group_id`"), true),
                (format!("{t} CHANGE COLUMN `email` `mail` varchar(320) NOT NULL DEFAULT 'a\\\\b'"), true),
                (format!("{t} MODIFY COLUMN `created` datetime(3) DEFAULT CURRENT_TIMESTAMP(3) COMMENT 'When'"), false),
                (format!("{t} RENAME COLUMN `order` TO `settings`"), false),
                (format!("{t} ADD COLUMN `active` boolean NOT NULL DEFAULT 1"), false),
                (format!("{t} ADD INDEX `accounts_active` (`active`)"), false),
                (format!("{t} RENAME TO `demo`.`accounts`"), false),
            ]
        );
        assert_guard_accepts(DbEngine::Mysql, &plan);
    }

    #[test]
    fn mariadb_offers_aria() {
        let info = MARIADB.info();
        assert_eq!(info.engine, DbEngine::Mariadb);
        assert!(info.options[0].choices.contains(&"Aria"));
        assert!(!MYSQL.info().options[0].choices.contains(&"Aria"));
    }
}
