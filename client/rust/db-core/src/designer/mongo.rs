//! MongoDB: a collection with options (capped, time series, validation,
//! collation). The field list becomes a `$jsonSchema` validator: each field
//! has a BSON type, required unless nullable, its comment as description.
//! Indexes become one `createIndexes` command.
use super::*;
use serde_json::{json, Value};

pub struct Mongo;

const TYPES: &[TypeInfo] = &[
    ty("string", "text"),
    ty("int", "numeric"),
    ty("long", "numeric"),
    ty("double", "numeric"),
    ty("decimal", "numeric"),
    ty("bool", "other"),
    ty("date", "time"),
    ty("timestamp", "time"),
    ty("objectId", "other"),
    ty("object", "json"),
    ty("array", "json"),
    ty("binData", "binary"),
];

fn text_option(id: &'static str) -> OptionInfo {
    OptionInfo {
        id,
        kind: OptionKind::Text,
        choices: vec![],
        default: "",
        create_only: true,
    }
}

fn number_option(id: &'static str) -> OptionInfo {
    OptionInfo {
        kind: OptionKind::Number,
        ..text_option(id)
    }
}

fn choice_option(id: &'static str, choices: Vec<&'static str>) -> OptionInfo {
    OptionInfo {
        kind: OptionKind::Choice,
        choices,
        ..text_option(id)
    }
}

/// A field name the validator can name: no `$` prefix, no dots, no NUL.
fn field_name(name: &str) -> bool {
    !name.is_empty() && !name.starts_with('$') && !name.contains('.')
}

/// `{"key": value, …}` keeping the written order (the command name first).
fn object(entries: &[(&str, Value)]) -> String {
    let body: Vec<String> = entries
        .iter()
        .map(|(key, value)| format!("{}: {value}", Value::String((*key).to_owned())))
        .collect();
    format!("{{{}}}", body.join(", "))
}

impl Dialect for Mongo {
    fn info(&self) -> DialectInfo {
        DialectInfo {
            engine: DbEngine::Mongodb,
            features: DesignFeatures {
                collection: true,
                indexes: true,
                column_comments: true,
                ..Default::default()
            },
            types: TYPES.to_vec(),
            default_type: "string",
            identity_kinds: vec![],
            default_presets: vec![],
            index_methods: vec!["asc", "desc", "hashed", "text"],
            fk_actions: vec![],
            options: vec![
                choice_option("capped", vec!["", "true", "false"]),
                number_option("size"),
                number_option("max"),
                text_option("time_field"),
                text_option("meta_field"),
                choice_option("granularity", vec!["", "seconds", "minutes", "hours"]),
                number_option("expire_after_seconds"),
                choice_option("validation_level", vec!["", "strict", "moderate", "off"]),
                choice_option("validation_action", vec!["", "error", "warn"]),
                text_option("collation_locale"),
                choice_option("collation_strength", vec!["", "1", "2", "3", "4", "5"]),
            ],
            max_identifier: 120,
        }
    }

    fn quote(&self, name: &str) -> String {
        Value::String(name.to_owned()).to_string()
    }

    fn literal(&self, text: &str) -> String {
        Value::String(text.to_owned()).to_string()
    }

    fn validate(&self, design: &TableDesign, issues: &mut Issues) {
        let info = self.info();
        if design.name.starts_with("system.") || design.name.contains('$') {
            issues.error("name_invalid", "table.name", &design.name);
        }
        for (i, column) in design.columns.iter().enumerate() {
            if !field_name(&column.name) {
                issues.error("name_invalid", format!("columns.{i}.name"), &column.name);
            }
            if !TYPES.iter().any(|t| t.name == column.data_type) {
                issues.error(
                    "type_invalid",
                    format!("columns.{i}.data_type"),
                    &column.data_type,
                );
            }
            if !column.default.is_empty() {
                issues.warning(
                    "default_ignored",
                    format!("columns.{i}.default"),
                    &column.name,
                );
            }
        }
        let capped = option(&info, design, "capped") == "true";
        if capped && option(&info, design, "size").is_empty() {
            issues.error("capped_size_required", "options.size", "");
        }
        let time_field = option(&info, design, "time_field");
        if capped && !time_field.is_empty() {
            issues.error("capped_time_series", "options.capped", "");
        }
        for id in ["time_field", "meta_field"] {
            let value = option(&info, design, id);
            if !value.is_empty() && !field_name(value) {
                issues.error("option_invalid", format!("options.{id}"), value);
            }
        }
        if time_field.is_empty()
            && (!option(&info, design, "meta_field").is_empty()
                || !option(&info, design, "granularity").is_empty())
        {
            issues.error("time_field_required", "options.time_field", "");
        }
        let locale = option(&info, design, "collation_locale");
        if !locale.is_empty()
            && !locale
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '@' || c == '=')
        {
            issues.error("option_invalid", "options.collation_locale", locale);
        }
        if locale.is_empty() && !option(&info, design, "collation_strength").is_empty() {
            issues.error("collation_locale_required", "options.collation_locale", "");
        }
    }

    fn create(&self, design: &TableDesign, _: &mut Issues) -> Vec<DesignStatement> {
        let info = self.info();
        let number = |id: &str| option(&info, design, id).parse::<u64>().ok();
        let text = |id: &str| option(&info, design, id).to_owned();
        let mut entries: Vec<(&str, Value)> = vec![("create", json!(design.name))];
        if text("capped") == "true" {
            entries.push(("capped", json!(true)));
            if let Some(size) = number("size") {
                entries.push(("size", json!(size)));
            }
            if let Some(max) = number("max") {
                entries.push(("max", json!(max)));
            }
        }
        if !text("time_field").is_empty() {
            let mut series = serde_json::Map::new();
            series.insert("timeField".into(), json!(text("time_field")));
            if !text("meta_field").is_empty() {
                series.insert("metaField".into(), json!(text("meta_field")));
            }
            if !text("granularity").is_empty() {
                series.insert("granularity".into(), json!(text("granularity")));
            }
            entries.push(("timeseries", Value::Object(series)));
        }
        if let Some(seconds) = number("expire_after_seconds") {
            entries.push(("expireAfterSeconds", json!(seconds)));
        }
        if !design.columns.is_empty() {
            let mut properties = serde_json::Map::new();
            for column in &design.columns {
                let mut field = serde_json::Map::new();
                field.insert("bsonType".into(), json!(column.data_type));
                if !column.comment.is_empty() {
                    field.insert("description".into(), json!(column.comment));
                }
                properties.insert(column.name.clone(), Value::Object(field));
            }
            let required: Vec<&str> = design
                .columns
                .iter()
                .filter(|column| !column.nullable)
                .map(|column| column.name.as_str())
                .collect();
            let mut schema = serde_json::Map::new();
            schema.insert("bsonType".into(), json!("object"));
            if !required.is_empty() {
                schema.insert("required".into(), json!(required));
            }
            schema.insert("properties".into(), Value::Object(properties));
            entries.push(("validator", json!({ "$jsonSchema": schema })));
        }
        for (id, key) in [
            ("validation_level", "validationLevel"),
            ("validation_action", "validationAction"),
        ] {
            if !text(id).is_empty() {
                entries.push((key, json!(text(id))));
            }
        }
        if !text("collation_locale").is_empty() {
            let mut collation = serde_json::Map::new();
            collation.insert("locale".into(), json!(text("collation_locale")));
            if let Some(strength) = number("collation_strength") {
                collation.insert("strength".into(), json!(strength));
            }
            entries.push(("collation", Value::Object(collation)));
        }
        let mut statements = vec![statement(format!("db.runCommand({})", object(&entries)))];
        if !design.indexes.is_empty() {
            let indexes: Vec<Value> = design
                .indexes
                .iter()
                .map(|index| {
                    let direction = match index.method.as_str() {
                        "desc" => json!(-1),
                        "hashed" => json!("hashed"),
                        "text" => json!("text"),
                        _ => json!(1),
                    };
                    let mut key = serde_json::Map::new();
                    for column in &index.columns {
                        key.insert(column.clone(), direction.clone());
                    }
                    let mut spec = serde_json::Map::new();
                    spec.insert("key".into(), Value::Object(key));
                    spec.insert(
                        "name".into(),
                        json!(name_or(
                            &info,
                            &index.name,
                            &design.name,
                            &index.columns,
                            "idx"
                        )),
                    );
                    if index.unique {
                        spec.insert("unique".into(), json!(true));
                    }
                    Value::Object(spec)
                })
                .collect();
            statements.push(statement(format!(
                "db.runCommand({})",
                object(&[
                    ("createIndexes", json!(design.name)),
                    ("indexes", Value::Array(indexes))
                ])
            )));
        }
        statements
    }

    fn alter(
        &self,
        _: &TableDesign,
        _: &TableDesign,
        _: &TableChanges<'_>,
        issues: &mut Issues,
    ) -> Vec<DesignStatement> {
        issues.error("alter_unsupported", "table", "");
        Vec::new()
    }

    fn script(&self, statements: &[DesignStatement]) -> String {
        statements
            .iter()
            .map(|statement| statement.sql.clone())
            .collect::<Vec<_>>()
            .join(";\n\n")
    }
}

#[cfg(test)]
mod tests {
    use super::super::plan as make_plan;
    use super::super::test_support::{assert_guard_accepts, column};
    use super::*;

    #[test]
    fn creates_a_validated_collection_with_indexes() {
        let design = TableDesign {
            database: "demo".into(),
            name: "users".into(),
            columns: vec![
                ColumnDesign {
                    nullable: false,
                    comment: "Login \"email\"".into(),
                    ..column("email", "string")
                },
                column("age", "int"),
            ],
            indexes: vec![IndexDesign {
                columns: vec!["email".into()],
                unique: true,
                ..Default::default()
            }],
            options: [
                ("validation_level".to_owned(), "strict".to_owned()),
                ("collation_locale".to_owned(), "en".to_owned()),
                ("collation_strength".to_owned(), "2".to_owned()),
            ]
            .into(),
            ..Default::default()
        };
        let plan = make_plan(DbEngine::Mongodb, None, &design);
        assert_eq!(
            plan.statements[0].sql,
            r#"db.runCommand({"create": "users", "validator": {"$jsonSchema":{"bsonType":"object","properties":{"age":{"bsonType":"int"},"email":{"bsonType":"string","description":"Login \"email\""}},"required":["email"]}}, "validationLevel": "strict", "collation": {"locale":"en","strength":2}})"#
        );
        assert_eq!(
            plan.statements[1].sql,
            r#"db.runCommand({"createIndexes": "users", "indexes": [{"key":{"email":1},"name":"users_email_idx","unique":true}]})"#
        );
        assert_guard_accepts(DbEngine::Mongodb, &plan);
    }

    #[test]
    fn capped_and_time_series_collections() {
        let capped = TableDesign {
            name: "log".into(),
            options: [
                ("capped".to_owned(), "true".to_owned()),
                ("size".to_owned(), "1048576".to_owned()),
                ("max".to_owned(), "1000".to_owned()),
            ]
            .into(),
            ..Default::default()
        };
        let plan = make_plan(DbEngine::Mongodb, None, &capped);
        assert_eq!(
            plan.statements[0].sql,
            r#"db.runCommand({"create": "log", "capped": true, "size": 1048576, "max": 1000})"#
        );
        assert_guard_accepts(DbEngine::Mongodb, &plan);
        let series = TableDesign {
            name: "metrics".into(),
            options: [
                ("time_field".to_owned(), "ts".to_owned()),
                ("meta_field".to_owned(), "host".to_owned()),
                ("granularity".to_owned(), "minutes".to_owned()),
                ("expire_after_seconds".to_owned(), "86400".to_owned()),
            ]
            .into(),
            ..Default::default()
        };
        let plan = make_plan(DbEngine::Mongodb, None, &series);
        assert_eq!(
            plan.statements[0].sql,
            r#"db.runCommand({"create": "metrics", "timeseries": {"granularity":"minutes","metaField":"host","timeField":"ts"}, "expireAfterSeconds": 86400})"#
        );
        assert_guard_accepts(DbEngine::Mongodb, &plan);
    }

    #[test]
    fn collection_rules() {
        let design = TableDesign {
            name: "x".into(),
            columns: vec![column("$bad", "string"), column("a.b", "nope")],
            options: [
                ("capped".to_owned(), "true".to_owned()),
                ("granularity".to_owned(), "hours".to_owned()),
            ]
            .into(),
            ..Default::default()
        };
        let codes: Vec<_> = make_plan(DbEngine::Mongodb, None, &design)
            .issues
            .into_iter()
            .map(|issue| issue.code)
            .collect();
        for code in [
            "name_invalid",
            "type_invalid",
            "capped_size_required",
            "time_field_required",
        ] {
            assert!(codes.contains(&code), "{code}: {codes:?}");
        }
    }
}
