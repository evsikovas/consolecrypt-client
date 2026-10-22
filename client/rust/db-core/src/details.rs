//! Object details for the structure tree: read-only catalogue facts about a
//! table, view or key, returned as labelled fields grouped in sections (the
//! client localizes the labels and formats sizes, counts and times). Every
//! lookup binds the object's names as parameters and runs on the read-only
//! catalogue path, so production sessions stay read-only.
use crate::{
    redis_driver::KeyFacts, session::Driver, DbCell, DbError, DbObjectKind, DbObjectRef,
    DbParameter, DbSession,
};
use cc_models::database::DbEngine;
use serde::Serialize;

/// How the client shows a value.
#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DetailKind {
    /// Shown as is.
    Text,
    /// Monospace (keys, expressions, engine names).
    Code,
    /// One of a few known words the client translates (relation kinds).
    Keyword,
    /// An exact number.
    Count,
    /// A planner/statistics estimate, shown with "≈".
    Estimate,
    /// Bytes, shown in KB/MB/GB.
    Bytes,
    /// A timestamp as the server printed it.
    Time,
    /// Milliseconds until expiry; negative means none.
    Ttl,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct DetailField {
    pub id: &'static str,
    pub value: String,
    pub kind: DetailKind,
    /// What a count counts (`fields`, `members`, …) when it is not rows.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unit: Option<&'static str>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct DetailSection {
    pub id: &'static str,
    pub fields: Vec<DetailField>,
}

#[derive(Debug, Clone, Default, Serialize, PartialEq, Eq)]
pub struct ObjectDetails {
    pub sections: Vec<DetailSection>,
}

impl ObjectDetails {
    fn section(&mut self, id: &'static str) -> &mut Self {
        self.sections.push(DetailSection {
            id,
            fields: Vec::new(),
        });
        self
    }

    /// Adds a field to the last section; empty values are left out.
    fn field(&mut self, id: &'static str, value: &str, kind: DetailKind) -> &mut Self {
        self.field_with_unit(id, value, kind, None)
    }

    fn field_with_unit(
        &mut self,
        id: &'static str,
        value: &str,
        kind: DetailKind,
        unit: Option<&'static str>,
    ) -> &mut Self {
        let value = value.trim();
        if !value.is_empty() {
            if let Some(section) = self.sections.last_mut() {
                section.fields.push(DetailField {
                    id,
                    value: value.to_owned(),
                    kind,
                    unit,
                });
            }
        }
        self
    }

    /// Drops sections that ended up without fields.
    fn done(mut self) -> Self {
        self.sections.retain(|section| !section.fields.is_empty());
        self
    }
}

fn text(name: &str, value: &str) -> DbParameter {
    DbParameter {
        name: name.into(),
        value: DbCell::Text(value.into()),
        type_hint: None,
    }
}

/// Whether details exist for `kind` on `engine` (MongoDB describes its own).
pub fn has_details(engine: DbEngine, object: &DbObjectRef) -> bool {
    match engine {
        DbEngine::Postgresql | DbEngine::Mysql | DbEngine::Mariadb => matches!(
            object.kind,
            DbObjectKind::Table | DbObjectKind::View | DbObjectKind::MaterializedView
        ),
        DbEngine::Clickhouse => matches!(
            object.kind,
            DbObjectKind::Table
                | DbObjectKind::View
                | DbObjectKind::MaterializedView
                | DbObjectKind::Dictionary
        ),
        DbEngine::Redis => {
            object.kind == DbObjectKind::RedisKey
                || (object.kind == DbObjectKind::RedisDatabase && object.parent_name.is_none())
        }
        DbEngine::Mongodb => matches!(
            object.kind,
            DbObjectKind::Database | DbObjectKind::Collection | DbObjectKind::View
        ),
    }
}

const POSTGRES: &str = "SELECT n.nspname, pg_catalog.pg_get_userbyid(c.relowner), c.relkind::text, \
     c.reltuples::bigint::text, \
     pg_catalog.pg_total_relation_size(c.oid)::text, pg_catalog.pg_table_size(c.oid)::text, \
     pg_catalog.pg_indexes_size(c.oid)::text, \
     COALESCE(pg_catalog.pg_total_relation_size(NULLIF(c.reltoastrelid, 0::oid))::text, ''), \
     (SELECT count(*) FROM pg_catalog.pg_attribute a WHERE a.attrelid = c.oid AND a.attnum > 0 AND NOT a.attisdropped)::text, \
     (SELECT count(*) FROM pg_catalog.pg_index i WHERE i.indrelid = c.oid)::text, \
     (SELECT count(*) FROM pg_catalog.pg_constraint k WHERE k.conrelid = c.oid)::text, \
     (SELECT count(*) FROM pg_catalog.pg_trigger g WHERE g.tgrelid = c.oid AND NOT g.tgisinternal)::text, \
     COALESCE(t.spcname, ''), COALESCE(pg_catalog.obj_description(c.oid, 'pg_class'), ''), \
     COALESCE(s.last_vacuum::text, ''), COALESCE(s.last_autovacuum::text, ''), \
     COALESCE(s.last_analyze::text, ''), COALESCE(s.last_autoanalyze::text, ''), \
     CASE WHEN c.relkind = 'p' THEN COALESCE(pg_catalog.pg_get_partkeydef(c.oid), '') ELSE '' END, \
     CASE WHEN c.relkind = 'p' THEN (SELECT count(*) FROM pg_catalog.pg_inherits h WHERE h.inhparent = c.oid)::text ELSE '' END, \
     CASE WHEN c.relispartition THEN COALESCE((SELECT pg_catalog.format('%I.%I', pn.nspname, pc.relname) \
     FROM pg_catalog.pg_inherits h JOIN pg_catalog.pg_class pc ON pc.oid = h.inhparent \
     JOIN pg_catalog.pg_namespace pn ON pn.oid = pc.relnamespace WHERE h.inhrelid = c.oid LIMIT 1), '') ELSE '' END, \
     CASE WHEN c.relkind IN ('v', 'm') THEN length(pg_catalog.pg_get_viewdef(c.oid))::text ELSE '' END, \
     COALESCE(s.n_live_tup::text, ''), COALESCE(s.n_dead_tup::text, ''), c.relpersistence::text \
     FROM pg_catalog.pg_class c JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace \
     LEFT JOIN pg_catalog.pg_tablespace t ON t.oid = c.reltablespace \
     LEFT JOIN pg_catalog.pg_stat_all_tables s ON s.relid = c.oid \
     WHERE n.nspname = $1 AND c.relname = $2 AND c.relkind IN ('r', 'p', 'v', 'm', 'f')";

const MYSQL: &str = "SELECT t.TABLE_TYPE, t.ENGINE, t.ROW_FORMAT, t.TABLE_ROWS, t.AVG_ROW_LENGTH, \
     t.DATA_LENGTH, t.INDEX_LENGTH, t.DATA_FREE, t.AUTO_INCREMENT, t.TABLE_COLLATION, \
     t.CREATE_TIME, t.UPDATE_TIME, t.TABLE_COMMENT, \
     (SELECT COUNT(*) FROM information_schema.COLUMNS c \
      WHERE c.TABLE_SCHEMA = t.TABLE_SCHEMA AND c.TABLE_NAME = t.TABLE_NAME), \
     (SELECT COUNT(DISTINCT x.INDEX_NAME) FROM information_schema.STATISTICS x \
      WHERE x.TABLE_SCHEMA = t.TABLE_SCHEMA AND x.TABLE_NAME = t.TABLE_NAME), \
     (SELECT COUNT(*) FROM information_schema.TRIGGERS g \
      WHERE g.EVENT_OBJECT_SCHEMA = t.TABLE_SCHEMA AND g.EVENT_OBJECT_TABLE = t.TABLE_NAME), \
     v.DEFINER, v.IS_UPDATABLE, v.SECURITY_TYPE \
     FROM information_schema.TABLES t LEFT JOIN information_schema.VIEWS v \
     ON v.TABLE_SCHEMA = t.TABLE_SCHEMA AND v.TABLE_NAME = t.TABLE_NAME \
     WHERE t.TABLE_SCHEMA = ? AND t.TABLE_NAME = ?";

const CLICKHOUSE: &str = "SELECT engine, toString(total_rows), toString(total_bytes), partition_key, \
     sorting_key, primary_key, sampling_key, comment, engine_full, toString(metadata_modification_time), \
     (SELECT toString(count()) FROM system.columns \
      WHERE database = {cc_database:String} AND table = {cc_table:String}), \
     (SELECT toString(count()) FROM system.parts \
      WHERE database = {cc_database:String} AND table = {cc_table:String} AND active), \
     (SELECT toString(sum(data_uncompressed_bytes)) FROM system.parts \
      WHERE database = {cc_database:String} AND table = {cc_table:String} AND active) \
     FROM system.tables WHERE database = {cc_database:String} AND name = {cc_table:String}";

/// The catalogue query and its bindings for a SQL engine; names are never
/// written into the text.
pub fn details_query(
    engine: DbEngine,
    object: &DbObjectRef,
    default_schema: &str,
) -> Option<(&'static str, Vec<DbParameter>)> {
    if !has_details(engine, object) {
        return None;
    }
    match engine {
        DbEngine::Postgresql => Some((
            POSTGRES,
            vec![
                text("", object.schema.as_deref().unwrap_or(default_schema)),
                text("", &object.name),
            ],
        )),
        DbEngine::Mysql | DbEngine::Mariadb => {
            let database = if object.database.is_empty() {
                default_schema
            } else {
                &object.database
            };
            Some((MYSQL, vec![text("", database), text("", &object.name)]))
        }
        DbEngine::Clickhouse => Some((
            CLICKHOUSE,
            vec![
                text("cc_database", &object.database),
                text("cc_table", &object.name),
            ],
        )),
        _ => None,
    }
}

fn cell(row: &[String], index: usize) -> &str {
    row.get(index).map(String::as_str).unwrap_or_default()
}

pub fn postgres_details(row: &[String]) -> ObjectDetails {
    let relkind = cell(row, 2);
    let view = matches!(relkind, "v");
    let kind = match relkind {
        "p" => "partitioned_table",
        "v" => "view",
        "m" => "materialized_view",
        "f" => "foreign_table",
        _ => "table",
    };
    let persistence = match cell(row, 24) {
        "u" => "unlogged",
        "t" => "temporary",
        _ => "",
    };
    // -1 means "never analyzed" (PostgreSQL 14+), not an empty table.
    let estimate = cell(row, 3);
    let estimate = if estimate.starts_with('-') {
        ""
    } else {
        estimate
    };
    let mut details = ObjectDetails::default();
    details
        .section("general")
        .field("kind", kind, DetailKind::Keyword)
        .field("persistence", persistence, DetailKind::Keyword)
        .field("schema", cell(row, 0), DetailKind::Code)
        .field("owner", cell(row, 1), DetailKind::Code)
        .field("tablespace", cell(row, 12), DetailKind::Code)
        .field("comment", cell(row, 13), DetailKind::Text);
    if !view {
        details
            .section("size")
            .field("rows", estimate, DetailKind::Estimate)
            .field("live_rows", cell(row, 22), DetailKind::Estimate)
            .field("dead_rows", cell(row, 23), DetailKind::Estimate)
            .field("total_size", cell(row, 4), DetailKind::Bytes)
            .field("table_size", cell(row, 5), DetailKind::Bytes)
            .field("index_size", cell(row, 6), DetailKind::Bytes)
            .field("toast_size", cell(row, 7), DetailKind::Bytes);
    }
    details
        .section("structure")
        .field("columns", cell(row, 8), DetailKind::Count);
    if !view {
        details
            .field("indexes", cell(row, 9), DetailKind::Count)
            .field("constraints", cell(row, 10), DetailKind::Count)
            .field("triggers", cell(row, 11), DetailKind::Count);
    }
    details.field_with_unit(
        "definition_length",
        cell(row, 21),
        DetailKind::Count,
        Some("characters"),
    );
    details
        .section("partitioning")
        .field("partition_key", cell(row, 18), DetailKind::Code)
        .field("partitions", cell(row, 19), DetailKind::Count)
        .field("partition_of", cell(row, 20), DetailKind::Code);
    details
        .section("maintenance")
        .field("last_vacuum", cell(row, 14), DetailKind::Time)
        .field("last_autovacuum", cell(row, 15), DetailKind::Time)
        .field("last_analyze", cell(row, 16), DetailKind::Time)
        .field("last_autoanalyze", cell(row, 17), DetailKind::Time);
    details.done()
}

pub fn mysql_details(row: &[String]) -> ObjectDetails {
    let view = cell(row, 0).ends_with("VIEW");
    let mut details = ObjectDetails::default();
    details
        .section("general")
        .field(
            "kind",
            if view { "view" } else { "table" },
            DetailKind::Keyword,
        )
        .field("engine", cell(row, 1), DetailKind::Code)
        .field("row_format", cell(row, 2), DetailKind::Code)
        .field("collation", cell(row, 9), DetailKind::Code)
        .field("definer", cell(row, 16), DetailKind::Code)
        .field("updatable", cell(row, 17), DetailKind::Code)
        .field("security", cell(row, 18), DetailKind::Code);
    if !view {
        details.field("comment", cell(row, 12), DetailKind::Text);
        details
            .section("size")
            .field("rows", cell(row, 3), DetailKind::Estimate)
            .field("average_row", cell(row, 4), DetailKind::Bytes)
            .field("table_size", cell(row, 5), DetailKind::Bytes)
            .field("index_size", cell(row, 6), DetailKind::Bytes)
            .field("free_space", cell(row, 7), DetailKind::Bytes)
            .field("auto_increment", cell(row, 8), DetailKind::Count);
    }
    details
        .section("structure")
        .field("columns", cell(row, 13), DetailKind::Count);
    if !view {
        details
            .field("indexes", cell(row, 14), DetailKind::Count)
            .field("triggers", cell(row, 15), DetailKind::Count);
    }
    details
        .section("maintenance")
        .field("created", cell(row, 10), DetailKind::Time)
        .field("updated", cell(row, 11), DetailKind::Time);
    details.done()
}

/// The `TTL …` clause of a ClickHouse engine definition, if any.
fn clickhouse_ttl(engine_full: &str) -> &str {
    let Some(start) = engine_full.find(" TTL ") else {
        return "";
    };
    let rest = &engine_full[start + 5..];
    rest.find(" SETTINGS ")
        .map_or(rest, |end| &rest[..end])
        .trim()
}

pub fn clickhouse_details(row: &[String]) -> ObjectDetails {
    let mut details = ObjectDetails::default();
    details
        .section("general")
        .field("engine", cell(row, 0), DetailKind::Code)
        .field("comment", cell(row, 7), DetailKind::Text);
    details
        .section("size")
        .field("rows", cell(row, 1), DetailKind::Count)
        .field("total_size", cell(row, 2), DetailKind::Bytes)
        .field("uncompressed_size", cell(row, 12), DetailKind::Bytes)
        .field("parts", cell(row, 11), DetailKind::Count);
    details
        .section("structure")
        .field("columns", cell(row, 10), DetailKind::Count)
        .field("partition_key", cell(row, 3), DetailKind::Code)
        .field("sorting_key", cell(row, 4), DetailKind::Code)
        .field("primary_key", cell(row, 5), DetailKind::Code)
        .field("sampling_key", cell(row, 6), DetailKind::Code)
        .field("ttl", clickhouse_ttl(cell(row, 8)), DetailKind::Code);
    details
        .section("maintenance")
        .field("metadata_modified", cell(row, 9), DetailKind::Time);
    details.done()
}

pub fn redis_key_details(facts: &KeyFacts) -> ObjectDetails {
    let mut details = ObjectDetails::default();
    details
        .section("general")
        .field("type", &facts.kind, DetailKind::Code)
        .field(
            "encoding",
            facts.encoding.as_deref().unwrap_or_default(),
            DetailKind::Code,
        )
        .field("database", &facts.db.to_string(), DetailKind::Code);
    if facts.kind != "none" {
        // PTTL is -1 for a persistent key; -2 would mean it is gone.
        details.field(
            "ttl",
            &facts
                .ttl
                .map(|ttl| ttl.max(-1).to_string())
                .unwrap_or_default(),
            DetailKind::Ttl,
        );
        details.section("size");
        if let Some((length, unit)) = facts.length {
            details.field_with_unit("length", &length.to_string(), DetailKind::Count, Some(unit));
        }
        details.field(
            "memory",
            &facts.memory.map(|m| m.to_string()).unwrap_or_default(),
            DetailKind::Bytes,
        );
    }
    details.done()
}

pub fn redis_database_details(object: &DbObjectRef, size: i64) -> ObjectDetails {
    let mut details = ObjectDetails::default();
    details
        .section("general")
        .field(
            "database",
            if object.database.is_empty() {
                "0"
            } else {
                &object.database
            },
            DetailKind::Code,
        )
        .field_with_unit("keys", &size.to_string(), DetailKind::Count, Some("keys"));
    details.done()
}

impl DbSession {
    /// Details of a table, view or key ([`ObjectDetails`]).
    pub async fn object_details(&self, object: &DbObjectRef) -> Result<ObjectDetails, DbError> {
        let engine = self.info.engine;
        if !has_details(engine, object) || engine == DbEngine::Mongodb {
            return Err(DbError::InvalidRequest);
        }
        if engine == DbEngine::Redis {
            return match &mut *self.driver.lock().await {
                Driver::Redis(driver) if object.kind == DbObjectKind::RedisKey => {
                    Ok(redis_key_details(&driver.key_facts(object).await?))
                }
                Driver::Redis(driver) => Ok(redis_database_details(
                    object,
                    driver.database_size(object).await?,
                )),
                _ => Err(DbError::InvalidRequest),
            };
        }
        let default = self.guard.policy().default_schema.clone();
        let (sql, bindings) =
            details_query(engine, object, &default).ok_or(DbError::InvalidRequest)?;
        let rows = self.catalogue(sql, &bindings).await?;
        let row = rows.first().ok_or(DbError::QueryFailed)?;
        Ok(match engine {
            DbEngine::Postgresql => postgres_details(row),
            DbEngine::Clickhouse => clickhouse_details(row),
            _ => mysql_details(row),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn object(engine: DbEngine, kind: DbObjectKind) -> DbObjectRef {
        DbObjectRef {
            database: if engine == DbEngine::Postgresql {
                String::new()
            } else {
                "shop".into()
            },
            schema: (engine == DbEngine::Postgresql).then(|| "sales".into()),
            kind,
            name: "o'rders; DROP TABLE x".into(),
            parent_name: None,
        }
    }

    #[test]
    fn queries_bind_names_and_never_interpolate_them() {
        for engine in [
            DbEngine::Postgresql,
            DbEngine::Mysql,
            DbEngine::Mariadb,
            DbEngine::Clickhouse,
        ] {
            let object = object(engine, DbObjectKind::Table);
            let (sql, bindings) = details_query(engine, &object, "public").unwrap();
            assert!(!sql.contains("rders"), "{engine:?}");
            assert!(!sql.contains("shop") && !sql.contains("sales"));
            assert_eq!(bindings.len(), 2);
            assert_eq!(bindings[1].value, DbCell::Text(object.name.clone()));
            assert!(sql.trim_start().starts_with("SELECT"));
        }
        let pg = details_query(
            DbEngine::Postgresql,
            &object(DbEngine::Postgresql, DbObjectKind::View),
            "public",
        )
        .unwrap();
        assert!(pg.0.contains("$1") && pg.0.contains("$2"));
        assert_eq!(pg.1[0].value, DbCell::Text("sales".into()));
        let ch = details_query(
            DbEngine::Clickhouse,
            &object(DbEngine::Clickhouse, DbObjectKind::Table),
            "default",
        )
        .unwrap();
        assert_eq!(
            ch.1.iter().map(|p| p.name.as_str()).collect::<Vec<_>>(),
            ["cc_database", "cc_table"]
        );
        // Functions, columns and indexes have no details.
        for kind in [
            DbObjectKind::Function,
            DbObjectKind::Column,
            DbObjectKind::Index,
        ] {
            assert!(details_query(
                DbEngine::Postgresql,
                &object(DbEngine::Postgresql, kind),
                "public"
            )
            .is_none());
        }
    }

    #[test]
    fn redis_folders_have_no_details_but_databases_do() {
        let mut folder = DbObjectRef {
            database: "3".into(),
            schema: None,
            kind: DbObjectKind::RedisDatabase,
            name: "user:".into(),
            parent_name: Some("db3".into()),
        };
        assert!(!has_details(DbEngine::Redis, &folder));
        folder.parent_name = None;
        assert!(has_details(DbEngine::Redis, &folder));
        let details = redis_database_details(&folder, 42);
        assert_eq!(details.sections[0].fields[1].value, "42");
    }

    fn row(values: &[(usize, &str)]) -> Vec<String> {
        let mut row = vec![String::new(); 25];
        for (index, value) in values {
            row[*index] = (*value).to_owned();
        }
        row
    }

    #[test]
    fn postgres_rows_become_grouped_fields() {
        let table = postgres_details(&row(&[
            (0, "public"),
            (1, "app"),
            (2, "r"),
            (3, "-1"),
            (4, "81920"),
            (5, "57344"),
            (6, "16384"),
            (8, "5"),
            (9, "2"),
            (10, "3"),
            (11, "0"),
            (16, "2026-10-22 04:33:00.1+00"),
            (24, "p"),
        ]));
        let ids: Vec<_> = table.sections.iter().map(|s| s.id).collect();
        assert_eq!(ids, ["general", "size", "structure", "maintenance"]);
        let size = &table.sections[1].fields;
        assert!(
            size.iter().all(|f| f.id != "rows"),
            "never analyzed: no estimate"
        );
        assert_eq!(size[0].id, "total_size");
        assert_eq!(size[0].kind, DetailKind::Bytes);
        let view = postgres_details(&row(&[
            (0, "public"),
            (2, "v"),
            (8, "4"),
            (21, "120"),
            (4, "0"),
        ]));
        let ids: Vec<_> = view.sections.iter().map(|s| s.id).collect();
        assert_eq!(ids, ["general", "structure"], "views have no sizes");
        assert_eq!(
            view.sections[1].fields[1].unit,
            Some("characters"),
            "definition length"
        );
        let partitioned = postgres_details(&row(&[(2, "p"), (18, "RANGE (day)"), (19, "12")]));
        assert_eq!(partitioned.sections[0].fields[0].value, "partitioned_table");
        assert!(partitioned
            .sections
            .iter()
            .any(|s| s.id == "partitioning" && s.fields.len() == 2));
    }

    #[test]
    fn mysql_and_clickhouse_rows_become_fields() {
        let mut values = vec![String::new(); 19];
        values[0] = "BASE TABLE".into();
        values[1] = "InnoDB".into();
        values[3] = "1000".into();
        values[5] = "16384".into();
        values[13] = "4".into();
        let table = mysql_details(&values);
        let size = table.sections.iter().find(|s| s.id == "size").unwrap();
        assert_eq!(size.fields[0].kind, DetailKind::Estimate);
        values[0] = "VIEW".into();
        values[12] = "VIEW".into();
        let view = mysql_details(&values);
        assert!(view.sections.iter().all(|s| s.id != "size"));
        assert!(view.sections[0].fields.iter().all(|f| f.id != "comment"));

        let mut ch = vec![String::new(); 13];
        ch[0] = "MergeTree".into();
        ch[8] =
            "MergeTree ORDER BY id TTL day + toIntervalDay(30) SETTINGS index_granularity = 8192"
                .into();
        let details = clickhouse_details(&ch);
        let structure = details
            .sections
            .iter()
            .find(|s| s.id == "structure")
            .unwrap();
        assert_eq!(structure.fields[0].value, "day + toIntervalDay(30)");
        assert_eq!(clickhouse_ttl("Memory"), "");
    }

    #[test]
    fn redis_keys_report_type_ttl_length_and_memory() {
        let facts = KeyFacts {
            db: 2,
            kind: "hash".into(),
            ttl: Some(-1),
            encoding: Some("listpack".into()),
            memory: Some(96),
            length: Some((3, "fields")),
        };
        let details = redis_key_details(&facts);
        let json = serde_json::to_value(&details).unwrap();
        assert_eq!(json["sections"][0]["fields"][3]["kind"], "ttl");
        assert_eq!(json["sections"][0]["fields"][3]["value"], "-1");
        assert_eq!(json["sections"][1]["fields"][0]["unit"], "fields");
        let missing = redis_key_details(&KeyFacts {
            kind: "none".into(),
            ttl: Some(-2),
            ..facts
        });
        assert_eq!(missing.sections.len(), 1);
    }
}
