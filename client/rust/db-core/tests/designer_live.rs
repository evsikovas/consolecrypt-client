//! Opt-in acceptance of the table designer against disposable local
//! databases: each test creates a uniquely named table (or collection),
//! loads it back, alters it and drops it again.
//! CC_DB_LIVE_PG / CC_DB_LIVE_MYSQL / CC_DB_LIVE_CLICKHOUSE / CC_DB_LIVE_MONGO
//! = host:port:user:password:database (ClickHouse: the HTTP port; MongoDB:
//! the user lives in `admin`).
use cc_db_core::{
    designer::{
        plan, CheckDesign, ColumnDesign, DesignApplyResult, ForeignKeyDesign, IndexDesign,
        KeyDesign, TableDesign,
    },
    transport::{DbCredentials, DirectConnector},
    DbCell, DbObjectKind, DbObjectRef, DbQueryEvent, DbQueryRequest, DbSession,
};
use cc_models::database::{DbConnection, DbEngine, DbOptions, DbTlsMode, MongoReadPreference};
use secrecy::SecretString;
use std::sync::Arc;

async fn open(variable: &str, engine: DbEngine) -> Option<Arc<DbSession>> {
    let Ok(spec) = std::env::var(variable) else {
        eprintln!("{variable} not set; skipped");
        return None;
    };
    let parts: Vec<&str> = spec.splitn(5, ':').collect();
    let mut connection = DbConnection::new("live", parts[0], engine);
    connection.port = parts[1].parse().unwrap();
    connection.username = parts[2].into();
    connection.database = parts[4].into();
    connection.tls.mode = DbTlsMode::Disable;
    connection.production = false;
    if engine == DbEngine::Mongodb {
        connection.options = DbOptions::Mongodb {
            auth_source: Some("admin".into()),
            replica_set: None,
            srv: false,
            read_preference: MongoReadPreference::Primary,
        };
    }
    let credentials = DbCredentials {
        password: Some(SecretString::from(parts[3].to_owned())),
        client_key_pem: None,
    };
    let session = Arc::new(
        DbSession::open(
            "live".into(),
            &connection,
            &credentials,
            Arc::new(DirectConnector),
            false,
        )
        .await
        .unwrap(),
    );
    session.guard().allow_session_write().unwrap();
    Some(session)
}

fn request(text: &str) -> DbQueryRequest {
    DbQueryRequest {
        text: text.into(),
        parameters: Vec::new(),
        approval_ticket: None,
    }
}

/// The client's flow: review ticket, approval, then the designer apply.
async fn apply(session: &Arc<DbSession>, script: &str) -> DesignApplyResult {
    let review = session.guard().review(&request(script)).unwrap();
    session.guard().approve(&review.ticket).unwrap();
    let mut approved = request(script);
    approved.approval_ticket = Some(review.ticket);
    session.apply_design(&approved).await.unwrap()
}

async fn create(session: &Arc<DbSession>, original: Option<&TableDesign>, design: &TableDesign) {
    let engine = session.info().engine;
    let plan = plan(engine, original, design);
    assert!(!plan.has_errors(), "{:?}", plan.issues);
    assert!(!plan.statements.is_empty());
    let result = apply(session, &plan.script).await;
    assert!(result.committed, "{result:?}\n{}", plan.script);
}

async fn rows(session: &Arc<DbSession>, text: &str, approve: bool) -> Vec<Vec<String>> {
    let mut query = request(text);
    if approve {
        let review = session.guard().review(&query).unwrap();
        session.guard().approve(&review.ticket).unwrap();
        query.approval_ticket = Some(review.ticket);
    }
    let mut stream = session.execute(&query).unwrap();
    let mut rows = Vec::new();
    while let Some(event) = stream.events.recv().await {
        match event {
            DbQueryEvent::Rows { rows: batch, .. } => rows.extend(batch.iter().map(|row| {
                row.iter()
                    .map(|cell| match cell {
                        DbCell::Null => "NULL".to_owned(),
                        DbCell::Bool(value) => value.to_string(),
                        DbCell::Int(v)
                        | DbCell::Uint(v)
                        | DbCell::Decimal(v)
                        | DbCell::Float(v)
                        | DbCell::Text(v)
                        | DbCell::Date(v)
                        | DbCell::Time(v)
                        | DbCell::Timestamp(v)
                        | DbCell::Binary(v)
                        | DbCell::Json(v) => v.clone(),
                    })
                    .collect()
            })),
            DbQueryEvent::Error { code, .. } => panic!("{text}: {code}"),
            DbQueryEvent::Done | DbQueryEvent::Cancelled => break,
            _ => {}
        }
    }
    rows
}

fn unique(prefix: &str) -> String {
    format!(
        "{prefix}_{}",
        &uuid::Uuid::new_v4().simple().to_string()[..8]
    )
}

fn column(name: &str, data_type: &str) -> ColumnDesign {
    ColumnDesign {
        name: name.into(),
        data_type: data_type.into(),
        nullable: true,
        ..Default::default()
    }
}

fn table_ref(design: &TableDesign) -> DbObjectRef {
    DbObjectRef {
        database: design.database.clone(),
        schema: design.schema.clone(),
        kind: DbObjectKind::Table,
        name: design.name.clone(),
        parent_name: None,
    }
}

fn names(design: &TableDesign) -> Vec<&str> {
    design.columns.iter().map(|c| c.name.as_str()).collect()
}

#[tokio::test]
async fn postgres_create_load_alter_and_roll_back() {
    let Some(session) = open("CC_DB_LIVE_PG", DbEngine::Postgresql).await else {
        return;
    };
    let parent = TableDesign {
        schema: Some("public".into()),
        name: unique("cc_designer_parent"),
        columns: vec![ColumnDesign {
            nullable: false,
            ..column("id", "integer")
        }],
        primary_key: KeyDesign {
            columns: vec!["id".into()],
            ..Default::default()
        },
        ..Default::default()
    };
    create(&session, None, &parent).await;
    let child = TableDesign {
        schema: Some("public".into()),
        name: unique("cc_designer \"order\""),
        comment: "Designer test".into(),
        columns: vec![
            ColumnDesign {
                identity: "by_default".into(),
                nullable: false,
                ..column("id", "bigint")
            },
            ColumnDesign {
                length: Some(50),
                nullable: false,
                default: "new".into(),
                comment: "State".into(),
                ..column("select", "varchar")
            },
            ColumnDesign {
                length: Some(10),
                scale: Some(2),
                ..column("price", "numeric")
            },
            ColumnDesign {
                array: true,
                ..column("tags", "text")
            },
            ColumnDesign {
                default: "now()".into(),
                ..column("created", "timestamptz")
            },
            column("parent_id", "integer"),
        ],
        primary_key: KeyDesign {
            columns: vec!["id".into()],
            ..Default::default()
        },
        uniques: vec![KeyDesign {
            columns: vec!["select".into()],
            ..Default::default()
        }],
        foreign_keys: vec![ForeignKeyDesign {
            columns: vec!["parent_id".into()],
            ref_table: parent.name.clone(),
            ref_columns: vec!["id".into()],
            on_delete: "CASCADE".into(),
            ..Default::default()
        }],
        indexes: vec![IndexDesign {
            columns: vec!["created".into()],
            method: "brin".into(),
            ..Default::default()
        }],
        checks: vec![CheckDesign {
            name: "price_positive".into(),
            expression: "price >= 0".into(),
            ..Default::default()
        }],
        ..Default::default()
    };
    create(&session, None, &child).await;
    let loaded = session.table_design(&table_ref(&child)).await.unwrap();
    assert_eq!(
        names(&loaded),
        ["id", "select", "price", "tags", "created", "parent_id"]
    );
    assert_eq!(loaded.comment, "Designer test");
    let select = &loaded.columns[1];
    assert_eq!(
        (
            select.data_type.as_str(),
            select.length,
            select.nullable,
            select.comment.as_str()
        ),
        ("varchar", Some(50), false, "State")
    );
    assert_eq!(loaded.columns[0].identity, "by_default");
    assert_eq!(
        (loaded.columns[2].length, loaded.columns[2].scale),
        (Some(10), Some(2))
    );
    assert!(loaded.columns[3].array);
    assert_eq!(loaded.columns[4].data_type, "timestamptz");
    assert_eq!(loaded.primary_key.columns, ["id"]);
    assert_eq!(loaded.uniques.len(), 1);
    assert_eq!(loaded.foreign_keys[0].ref_table, parent.name);
    assert_eq!(loaded.foreign_keys[0].on_delete, "CASCADE");
    assert_eq!(loaded.indexes[0].method, "brin");
    assert_eq!(loaded.checks[0].name, "price_positive");
    // Loaded as is, nothing to change.
    assert!(plan(DbEngine::Postgresql, Some(&loaded), &loaded)
        .statements
        .is_empty());

    let mut altered = loaded.clone();
    altered.columns[1].name = "state".into();
    altered.columns[1].length = Some(80);
    altered.uniques[0].columns = vec!["state".into()];
    altered.columns.retain(|c| c.name != "tags");
    altered.columns.push(ColumnDesign {
        nullable: false,
        default: "0".into(),
        ..column("weight", "integer")
    });
    altered.indexes.push(IndexDesign {
        name: format!("{}_state_hash", altered.name),
        columns: vec!["state".into()],
        method: "hash".into(),
        ..Default::default()
    });
    altered.comment = "Changed".into();
    create(&session, Some(&loaded), &altered).await;
    let reloaded = session.table_design(&table_ref(&altered)).await.unwrap();
    assert_eq!(
        names(&reloaded),
        ["id", "state", "price", "created", "parent_id", "weight"]
    );
    assert_eq!(reloaded.columns[1].length, Some(80));
    assert_eq!(reloaded.comment, "Changed");
    assert_eq!(reloaded.indexes.len(), 2);

    // A failing statement rolls the whole script back.
    let mut broken = reloaded.clone();
    broken.columns.push(column("extra", "integer"));
    broken.checks.push(CheckDesign {
        name: "broken".into(),
        expression: "missing_column > 0".into(),
        ..Default::default()
    });
    let script = plan(DbEngine::Postgresql, Some(&reloaded), &broken);
    let result = apply(&session, &script.script).await;
    assert!(!result.committed);
    let statuses: Vec<_> = result.statements.iter().map(|s| s.status).collect();
    assert_eq!(statuses, ["rolled_back", "failed"]);
    let after = session.table_design(&table_ref(&altered)).await.unwrap();
    assert!(!names(&after).contains(&"extra"));

    // LIM-06: apostrophes in comments, UNLOGGED, identity switched and dropped.
    let mut logged = after.clone();
    logged.comment = r"Bob's \ orders".into();
    logged.columns[1].comment = "it's the state".into();
    logged.columns[0].identity = "always".into();
    logged.options.insert("unlogged".into(), "true".into());
    create(&session, Some(&after), &logged).await;
    let switched = session.table_design(&table_ref(&logged)).await.unwrap();
    assert_eq!(switched.comment, r"Bob's \ orders");
    assert_eq!(switched.columns[1].comment, "it's the state");
    assert_eq!(switched.columns[0].identity, "always");
    assert_eq!(
        switched.options.get("unlogged").map(String::as_str),
        Some("true")
    );
    let mut plain = switched.clone();
    plain.columns[0].identity = String::new();
    plain.options.remove("unlogged");
    create(&session, Some(&switched), &plain).await;
    let dropped = session.table_design(&table_ref(&plain)).await.unwrap();
    assert_eq!(dropped.columns[0].identity, "");
    assert!(!dropped.options.contains_key("unlogged"));
    assert!(plan(DbEngine::Postgresql, Some(&dropped), &dropped)
        .statements
        .is_empty());

    for name in [&child.name, &parent.name] {
        rows(
            &session,
            &format!("DROP TABLE \"public\".\"{}\"", name.replace('"', "\"\"")),
            true,
        )
        .await;
    }
}

#[tokio::test]
async fn mysql_create_load_alter() {
    let Some(session) = open("CC_DB_LIVE_MYSQL", DbEngine::Mysql).await else {
        return;
    };
    let database = std::env::var("CC_DB_LIVE_MYSQL")
        .unwrap()
        .rsplit(':')
        .next()
        .unwrap()
        .to_owned();
    let design = TableDesign {
        database: database.clone(),
        name: unique("cc_designer `order`"),
        comment: "Designer's test".into(),
        columns: vec![
            ColumnDesign {
                identity: "auto_increment".into(),
                unsigned: true,
                nullable: false,
                ..column("id", "bigint")
            },
            ColumnDesign {
                length: Some(50),
                nullable: false,
                default: "it's".into(),
                comment: "State".into(),
                ..column("select", "varchar")
            },
            ColumnDesign {
                length: Some(10),
                scale: Some(2),
                default: "0".into(),
                ..column("price", "decimal")
            },
            ColumnDesign {
                default: "CURRENT_TIMESTAMP".into(),
                ..column("created", "timestamp")
            },
        ],
        primary_key: KeyDesign {
            columns: vec!["id".into()],
            ..Default::default()
        },
        uniques: vec![KeyDesign {
            columns: vec!["select".into()],
            ..Default::default()
        }],
        indexes: vec![IndexDesign {
            name: "created_idx".into(),
            columns: vec!["created".into()],
            ..Default::default()
        }],
        checks: vec![CheckDesign {
            name: unique("price_positive"),
            expression: "`price` >= 0".into(),
            ..Default::default()
        }],
        ..Default::default()
    };
    create(&session, None, &design).await;
    let loaded = session.table_design(&table_ref(&design)).await.unwrap();
    assert_eq!(names(&loaded), ["id", "select", "price", "created"]);
    let id = &loaded.columns[0];
    assert_eq!(
        (id.identity.as_str(), id.unsigned),
        ("auto_increment", true)
    );
    assert_eq!(loaded.columns[1].default, "'it''s'");
    assert_eq!(loaded.columns[1].comment, "State");
    assert_eq!(loaded.primary_key.columns, ["id"]);
    assert_eq!(loaded.uniques[0].columns, ["select"]);
    assert_eq!(loaded.indexes[0].name, "created_idx");
    assert_eq!(loaded.checks.len(), 1);
    assert_eq!(loaded.comment, "Designer's test");
    assert!(plan(DbEngine::Mysql, Some(&loaded), &loaded)
        .statements
        .is_empty());

    let mut altered = loaded.clone();
    altered.columns[1].name = "state".into();
    altered.columns[1].length = Some(80);
    altered.uniques[0].columns = vec!["state".into()];
    altered.columns[2].comment = "Price".into();
    altered.columns.push(ColumnDesign {
        nullable: false,
        default: "1".into(),
        ..column("active", "tinyint")
    });
    altered.indexes.clear();
    create(&session, Some(&loaded), &altered).await;
    let reloaded = session.table_design(&table_ref(&altered)).await.unwrap();
    assert_eq!(
        names(&reloaded),
        ["id", "state", "price", "created", "active"]
    );
    assert_eq!(reloaded.columns[1].length, Some(80));
    assert_eq!(reloaded.columns[2].comment, "Price");
    assert!(reloaded.indexes.is_empty());
    rows(
        &session,
        &format!(
            "DROP TABLE `{database}`.`{}`",
            design.name.replace('`', "``")
        ),
        true,
    )
    .await;
}

#[tokio::test]
async fn clickhouse_create_load_alter() {
    let Some(session) = open("CC_DB_LIVE_CLICKHOUSE", DbEngine::Clickhouse).await else {
        return;
    };
    let database = std::env::var("CC_DB_LIVE_CLICKHOUSE")
        .unwrap()
        .rsplit(':')
        .next()
        .unwrap()
        .to_owned();
    let design = TableDesign {
        database: database.clone(),
        name: unique("cc_designer"),
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
            column("note", "String"),
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
    };
    create(&session, None, &design).await;
    let loaded = session.table_design(&table_ref(&design)).await.unwrap();
    assert_eq!(names(&loaded), ["id", "at", "kind", "note", "amount"]);
    assert!(loaded.columns[2].low_cardinality && !loaded.columns[2].nullable);
    assert_eq!(loaded.columns[2].comment, "it's a kind");
    assert!(loaded.columns[3].nullable);
    assert_eq!(
        (loaded.columns[4].length, loaded.columns[4].scale),
        (Some(10), Some(2))
    );
    assert_eq!(loaded.primary_key.columns, ["id"]);
    assert_eq!(loaded.options["engine"], "MergeTree");
    assert!(plan(DbEngine::Clickhouse, Some(&loaded), &loaded)
        .statements
        .is_empty());

    let mut altered = loaded.clone();
    altered.columns[3].name = "remark".into();
    altered.columns[3].default = "'-'".into();
    altered.columns.push(column("source", "String"));
    altered.columns.retain(|c| c.name != "amount");
    create(&session, Some(&loaded), &altered).await;
    let reloaded = session.table_design(&table_ref(&altered)).await.unwrap();
    assert_eq!(names(&reloaded), ["id", "at", "kind", "remark", "source"]);
    assert_eq!(reloaded.columns[3].default, "'-'");

    // LIM-06: a default and a comment are removed, the table is renamed.
    let mut cleared = reloaded.clone();
    cleared.columns[3].default = String::new();
    cleared.columns[2].comment = String::new();
    cleared.name = unique("cc_designer_renamed");
    create(&session, Some(&reloaded), &cleared).await;
    let renamed = session.table_design(&table_ref(&cleared)).await.unwrap();
    assert_eq!(renamed.columns[3].default, "");
    assert_eq!(renamed.columns[2].comment, "");
    assert!(plan(DbEngine::Clickhouse, Some(&renamed), &renamed)
        .statements
        .is_empty());
    rows(
        &session,
        &format!("DROP TABLE `{database}`.`{}`", cleared.name),
        true,
    )
    .await;
}

#[tokio::test]
async fn mongo_creates_collections() {
    let Some(session) = open("CC_DB_LIVE_MONGO", DbEngine::Mongodb).await else {
        return;
    };
    let design = TableDesign {
        name: unique("cc_designer"),
        columns: vec![
            ColumnDesign {
                nullable: false,
                comment: "Login".into(),
                ..column("email", "string")
            },
            column("age", "int"),
        ],
        indexes: vec![IndexDesign {
            columns: vec!["email".into()],
            unique: true,
            ..Default::default()
        }],
        options: [("validation_level".to_owned(), "strict".to_owned())].into(),
        ..Default::default()
    };
    create(&session, None, &design).await;
    let listed = rows(
        &session,
        &format!(
            "db.runCommand({{\"listCollections\": 1, \"filter\": {{\"name\": \"{}\"}}}})",
            design.name
        ),
        false,
    )
    .await;
    let text = format!("{listed:?}");
    assert!(
        text.contains("$jsonSchema") && text.contains("email"),
        "{text}"
    );
    let indexes = rows(
        &session,
        &format!("db.getCollection(\"{}\").getIndexes()", design.name),
        false,
    )
    .await;
    assert!(format!("{indexes:?}").contains(&format!("{}_email_idx", design.name)));
    let capped = TableDesign {
        name: unique("cc_designer_capped"),
        options: [
            ("capped".to_owned(), "true".to_owned()),
            ("size".to_owned(), "65536".to_owned()),
        ]
        .into(),
        ..Default::default()
    };
    create(&session, None, &capped).await;
    for name in [&design.name, &capped.name] {
        rows(
            &session,
            &format!("db.getCollection(\"{name}\").drop()"),
            true,
        )
        .await;
    }
}
