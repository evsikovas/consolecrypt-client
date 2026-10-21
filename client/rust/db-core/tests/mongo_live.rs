//! Opt-in acceptance against a disposable MongoDB:
//! CC_DB_LIVE_MONGO=host:port:user:password:database (the user lives in `admin`).
//! For example: `docker run -d --name cc-db-mongo -p 127.0.0.1:57017:27017
//! -e MONGO_INITDB_ROOT_USERNAME=cc -e MONGO_INITDB_ROOT_PASSWORD=cc-test-mongo mongo:7.0`
//! and CC_DB_LIVE_MONGO=127.0.0.1:57017:cc:cc-test-mongo:ccapp.
//!
//! CC_DB_LIVE_MONGO_RS=host:port:set targets a replica set without
//! authentication whose members announce themselves under another name than
//! the seed, e.g. `docker run -d --name cc-db-mongo-rs -p 127.0.0.1:57018:57018
//! mongo:7.0 --replSet rs0 --port 57018 --bind_ip_all`, initiated with the member
//! `127.0.0.1:57018`, and CC_DB_LIVE_MONGO_RS=localhost:57018:rs0.
use bson::doc;
use cc_db_core::{
    transport::{DbCredentials, DirectConnector},
    DbCell, DbColumn, DbError, DbObjectKind, DbQueryEvent, DbQueryRequest, DbServerReadOnly,
    DbSession,
};
use cc_models::database::{DbConnection, DbEngine, DbOptions, DbTlsMode, MongoReadPreference};
use secrecy::SecretString;
use serde_json::Value;
use std::{
    sync::Arc,
    time::{Duration, Instant},
};

struct Target {
    host: String,
    port: u16,
    user: String,
    password: String,
    database: String,
}

fn target() -> Option<Target> {
    let spec = std::env::var("CC_DB_LIVE_MONGO").ok()?;
    let parts: Vec<&str> = spec.splitn(5, ':').collect();
    Some(Target {
        host: parts[0].into(),
        port: parts[1].parse().unwrap(),
        user: parts[2].into(),
        password: parts[3].into(),
        database: parts[4].into(),
    })
}

fn connection(target: &Target, user: &str, production: bool) -> DbConnection {
    let mut connection = DbConnection::new("live", &target.host, DbEngine::Mongodb);
    connection.port = target.port;
    connection.username = user.into();
    connection.database = target.database.clone();
    connection.production = production;
    connection.tls.mode = DbTlsMode::Disable;
    connection.options = DbOptions::Mongodb {
        auth_source: Some("admin".into()),
        replica_set: None,
        srv: false,
        read_preference: MongoReadPreference::SecondaryPreferred,
    };
    connection
}

async fn open(connection: &DbConnection, password: &str) -> Result<Arc<DbSession>, DbError> {
    let credentials = DbCredentials {
        password: Some(SecretString::from(password.to_owned())),
        client_key_pem: None,
    };
    DbSession::open(
        format!("mongo-{}", connection.production),
        connection,
        &credentials,
        Arc::new(DirectConnector),
        false,
    )
    .await
    .map(Arc::new)
}

/// Test-only administrative client for fixtures and server-side checks.
async fn admin(target: &Target) -> mongodb::Client {
    let mut options = mongodb::options::ClientOptions::default();
    options.hosts = vec![mongodb::options::ServerAddress::Tcp {
        host: target.host.clone(),
        port: Some(target.port),
    }];
    options.direct_connection = Some(true);
    options.credential = Some(
        mongodb::options::Credential::builder()
            .username(target.user.clone())
            .password(target.password.clone())
            .source("admin".to_owned())
            .build(),
    );
    mongodb::Client::with_options(options).unwrap()
}

fn request(text: &str) -> DbQueryRequest {
    DbQueryRequest {
        text: text.into(),
        parameters: Vec::new(),
        approval_ticket: None,
    }
}

#[derive(Default, Debug)]
struct Outcome {
    columns: Vec<DbColumn>,
    rows: Vec<Vec<String>>,
    pauses: usize,
    affected: Option<u64>,
    error: Option<String>,
    cancelled: bool,
}

impl Outcome {
    fn column(&self, name: &str) -> usize {
        self.columns
            .iter()
            .position(|column| column.name == name)
            .unwrap_or_else(|| panic!("no column {name}"))
    }

    fn values(&self, name: &str) -> Vec<String> {
        let index = self.column(name);
        self.rows.iter().map(|row| row[index].clone()).collect()
    }

    fn documents(&self) -> Vec<Value> {
        self.values("_doc")
            .iter()
            .map(|text| serde_json::from_str(text).unwrap())
            .collect()
    }
}

fn text(cell: &DbCell) -> String {
    match cell {
        DbCell::Null => "NULL".into(),
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
    }
}

/// Runs a request and pulls every page (1000 rows per `load_more`).
async fn consume(session: &Arc<DbSession>, query: &DbQueryRequest) -> Result<Outcome, DbError> {
    let mut stream = session.execute(query)?;
    let control = stream.control();
    let mut outcome = Outcome::default();
    while let Some(event) = stream.events.recv().await {
        match event {
            DbQueryEvent::Started { columns, .. } => outcome.columns = columns,
            DbQueryEvent::Rows { rows, .. } => outcome
                .rows
                .extend(rows.iter().map(|row| row.iter().map(text).collect())),
            DbQueryEvent::Paused { has_more, .. } => {
                outcome.pauses += 1;
                if has_more {
                    control.load_more(1000).unwrap();
                }
            }
            DbQueryEvent::StatementDone { affected_rows, .. } => outcome.affected = affected_rows,
            DbQueryEvent::Error { code, .. } => {
                outcome.error = Some(code);
                break;
            }
            DbQueryEvent::Cancelled => {
                outcome.cancelled = true;
                break;
            }
            DbQueryEvent::Done => break,
            DbQueryEvent::Notice { .. } => {}
        }
    }
    Ok(outcome)
}

async fn run(session: &Arc<DbSession>, text: &str) -> Outcome {
    let outcome = consume(session, &request(text)).await.unwrap();
    assert!(outcome.error.is_none(), "{text}: {:?}", outcome.error);
    outcome
}

/// Writes go through the native review: ticket, explicit approval, execution.
async fn write(session: &Arc<DbSession>, text: &str) -> Result<Outcome, String> {
    let review = session
        .guard()
        .review(&request(text))
        .map_err(|error| format!("{error:?}"))?;
    session
        .guard()
        .approve(&review.ticket)
        .map_err(|error| format!("{error:?}"))?;
    let mut approved = request(text);
    approved.approval_ticket = Some(review.ticket.clone());
    let outcome = consume(session, &approved)
        .await
        .map_err(|error| format!("{error:?}"))?;
    match outcome.error {
        Some(code) => Err(code),
        None => Ok(outcome),
    }
}

#[tokio::test]
async fn mongo_sessions_read_write_browse_page_inspect_and_cancel() {
    let Some(target) = target() else {
        eprintln!("CC_DB_LIVE_MONGO not set; skipped");
        return;
    };
    let administrator = admin(&target).await;
    administrator
        .database(&target.database)
        .drop()
        .await
        .unwrap();

    // Development session: writes go through the review flow.
    let dev = open(&connection(&target, &target.user, false), &target.password)
        .await
        .unwrap();
    assert!(!dev.info().read_only && !dev.info().production);
    assert_eq!(dev.info().engine, DbEngine::Mongodb);
    assert!(dev.info().server_version.starts_with('7'));
    // The root user may write; nothing verifies the server layer.
    assert_eq!(dev.info().server_read_only, DbServerReadOnly::Unverified);
    let documents: Vec<String> = (0..1500)
        .map(|n| {
            format!(
                r#"{{"n":{n},"grp":"g{}","name":"item{n}","price":{{"$numberDecimal":"{n}.50"}},"at":{{"$date":"2024-01-{:02}T00:00:00Z"}}{}}}"#,
                n % 3,
                n % 28 + 1,
                if n % 2 == 0 { r#","secret_token":"t""# } else { "" }
            )
        })
        .collect();
    let inserted = write(
        &dev,
        &format!("db.items.insertMany([{}])", documents.join(",")),
    )
    .await
    .unwrap();
    assert_eq!(inserted.affected, Some(1500));
    write(
        &dev,
        r#"{"createIndexes":"items","indexes":[{"key":{"grp":1,"n":-1},"name":"grp_n"}]}"#,
    )
    .await
    .unwrap();
    // Written key order survives into the stored document.
    write(
        &dev,
        r#"db.ordered.insertOne({"z":1,"a":2,"m":{"y":1,"b":2}})"#,
    )
    .await
    .unwrap();
    let ordered = run(&dev, r#"db.ordered.findOne({}, {"_id":0})"#).await;
    assert_eq!(
        ordered.values("_doc"),
        [r#"{"z":1,"a":2,"m":{"y":1,"b":2}}"#]
    );
    let updated = write(
        &dev,
        r#"db.items.updateMany({"grp":"g0"}, {"$set":{"flag":true}})"#,
    )
    .await
    .unwrap();
    assert_eq!(updated.affected, Some(500));
    let late = write(
        &dev,
        r#"db.items.updateMany({"n":{"$gte":1400}}, {"$set":{"late":1}})"#,
    )
    .await
    .unwrap();
    assert_eq!(late.affected, Some(100));
    // Destructive statements need the exact approval; without it they are refused.
    assert!(matches!(
        dev.execute(&request(r#"db.ordered.deleteMany({})"#)),
        Err(DbError::ApprovalRequired)
    ));
    let deleted = write(&dev, r#"db.ordered.deleteMany({})"#).await.unwrap();
    assert_eq!(deleted.affected, Some(1));
    write(&dev, "db.ordered.drop()").await.unwrap();

    // Production session: reads only, through the client guard.
    let prod = open(&connection(&target, &target.user, true), &target.password)
        .await
        .unwrap();
    assert!(prod.info().read_only && prod.info().production);
    let found = run(
        &prod,
        r#"db.items.find({"grp":"g1","n":{"$lt":100}}).sort({"n":-1}).limit(5)"#,
    )
    .await;
    assert_eq!(found.values("n"), ["97", "94", "91", "88", "85"]);
    assert_eq!(found.values("name")[0], "item97");
    let id = found.column("_id");
    assert!(found.columns[id].primary_key && id == 0);
    assert_eq!(found.columns[found.column("price")].type_name, "decimal");
    assert_eq!(found.values("price")[0], "97.50");
    assert_eq!(found.values("at")[0], "2024-01-14T00:00:00Z");
    assert!(found.columns[found.column("_doc")].sensitive);
    let document = &found.documents()[0];
    assert!(document["_id"]["$oid"].is_string());
    assert_eq!(document["price"]["$numberDecimal"], "97.50");
    assert_eq!(document["at"]["$date"], "2024-01-14T00:00:00Z");
    // A compound sort follows the written key order.
    let sorted = run(
        &prod,
        r#"db.items.find({"n":{"$lt":6}}, {"_id":0,"n":1,"grp":1}).sort({"grp":1,"n":-1})"#,
    )
    .await;
    assert_eq!(sorted.values("n"), ["3", "0", "4", "1", "5", "2"]);
    let grouped = run(
        &prod,
        r#"db.items.aggregate([{"$group":{"_id":"$grp","count":{"$sum":1}}},{"$sort":{"_id":1}}])"#,
    )
    .await;
    assert_eq!(grouped.values("_id"), ["g0", "g1", "g2"]);
    assert_eq!(grouped.values("count"), ["500", "500", "500"]);
    let counted = run(&prod, r#"db.items.countDocuments({"flag":true})"#).await;
    assert_eq!(counted.values("count"), ["500"]);
    let estimated = run(&prod, "db.items.estimatedDocumentCount()").await;
    assert_eq!(estimated.values("n"), ["1500"]);
    let distinct = run(&prod, r#"db.items.distinct("grp")"#).await;
    let mut groups = distinct.values("value");
    groups.sort();
    assert_eq!(groups, ["g0", "g1", "g2"]);
    let collections = run(&prod, r#"{"listCollections":1,"nameOnly":true}"#).await;
    assert!(collections.values("name").contains(&"items".to_string()));
    let indexes = run(&prod, "db.items.getIndexes()").await;
    assert_eq!(indexes.values("name"), ["_id_", "grp_n"]);
    let explained = run(
        &prod,
        r#"{"explain":{"find":"items","filter":{"grp":"g1"}},"verbosity":"queryPlanner"}"#,
    )
    .await;
    assert!(explained.values("_doc")[0].contains("grp_n"));
    let stats = run(&prod, "db.items.stats()").await;
    assert_eq!(stats.values("count"), ["1500"]);
    let parameterised = consume(
        &prod,
        &DbQueryRequest {
            text: r#"db.items.find({"name":{"$param":"name"}})"#.into(),
            parameters: vec![cc_db_core::DbParameter {
                name: "name".into(),
                value: DbCell::Text("item7".into()),
                type_hint: None,
            }],
            approval_ticket: None,
        },
    )
    .await
    .unwrap();
    assert_eq!(parameterised.values("n"), ["7"]);

    // The client guard refuses writes, DDL and $out/$merge before the server.
    for refused in [
        r#"db.items.insertOne({"x":1})"#,
        r#"db.items.updateOne({"n":1},{"$set":{"x":1}})"#,
        r#"db.items.deleteMany({"n":1})"#,
        "db.items.drop()",
        r#"{"createIndexes":"items","indexes":[{"key":{"x":1},"name":"x_1"}]}"#,
        r#"db.items.aggregate([{"$match":{}},{"$out":"stolen"}])"#,
        r#"db.items.aggregate([{"$merge":{"into":"stolen"}}])"#,
        r#"{"findAndModify":"items","query":{"n":1},"remove":true}"#,
    ] {
        assert!(
            matches!(
                prod.execute(&request(refused)),
                Err(DbError::PolicyDenied | DbError::ApprovalRequired)
            ),
            "accepted {refused}"
        );
    }
    let names = administrator
        .database(&target.database)
        .list_collection_names()
        .await
        .unwrap();
    assert!(!names.contains(&"stolen".to_string()));

    // More than one page streams through getMore as the consumer asks.
    let started = Instant::now();
    let all = run(&prod, r#"db.items.find({}).sort({"n":1})"#).await;
    assert_eq!(all.rows.len(), 1500);
    assert_eq!(all.pauses, 1, "the first page holds max_rows");
    assert_eq!(all.values("n")[1499], "1499");
    // `late` appears only in later batches: it stays inside `_doc`.
    assert!(all.columns.iter().all(|column| column.name != "late"));
    assert!(all.columns.iter().any(|column| column.name == "flag"));
    assert_eq!(all.documents()[1450]["late"], 1);
    assert!(started.elapsed() < Duration::from_secs(20));

    // Structure: databases -> collections -> indexes.
    let databases = prod.tree(None, None).await.unwrap();
    let database = databases
        .nodes
        .iter()
        .find(|node| node.reference.name == target.database)
        .unwrap()
        .reference
        .clone();
    assert_eq!(database.kind, DbObjectKind::Database);
    let collections = prod.tree(Some(&database), None).await.unwrap();
    let items = collections
        .nodes
        .iter()
        .find(|node| node.reference.name == "items")
        .unwrap();
    assert_eq!(items.reference.kind, DbObjectKind::Collection);
    assert!(items.has_children);
    let indexes = prod.tree(Some(&items.reference), None).await.unwrap();
    let names: Vec<_> = indexes
        .nodes
        .iter()
        .map(|node| node.reference.name.as_str())
        .collect();
    assert_eq!(names, ["_id_", "grp_n"]);
    let compound = &indexes.nodes[1];
    assert_eq!(compound.reference.kind, DbObjectKind::Index);
    assert_eq!(compound.reference.parent_name.as_deref(), Some("items"));
    assert_eq!(compound.detail.as_deref(), Some(r#"{"grp":1,"n":-1}"#));
    assert!(indexes.nodes[0].primary_key);

    // Cancel a slow aggregate: the stream ends promptly and the server
    // operation is killed, not left running.
    let slow = r#"db.items.aggregate([{"$project":{"x":{"$reduce":{"input":{"$range":[0,300000]},"initialValue":0,"in":{"$add":["$$value","$$this"]}}}}},{"$sort":{"x":-1}}])"#;
    let mut stream = prod.execute(&request(slow)).unwrap();
    tokio::time::sleep(Duration::from_millis(1000)).await;
    let cancelled_at = Instant::now();
    stream.cancel();
    let mut cancelled = false;
    while let Some(event) = stream.events.recv().await {
        if matches!(event, DbQueryEvent::Cancelled) {
            cancelled = true;
            break;
        }
    }
    assert!(cancelled);
    assert!(cancelled_at.elapsed() < Duration::from_secs(5));
    let mut killed = false;
    for _ in 0..20 {
        let operations = administrator
            .database("admin")
            .run_command(doc! {"currentOp": 1, "command.comment": {"$regex": "^consolecrypt-"}})
            .await
            .unwrap();
        if operations.get_array("inprog").unwrap().is_empty() {
            killed = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
    assert!(killed, "the cancelled aggregate is still running");
    // The session stays usable after a cancel.
    assert_eq!(
        run(&prod, "db.items.estimatedDocumentCount()")
            .await
            .values("n"),
        ["1500"]
    );
    prod.close();

    // maxTimeMS from the statement timeout stops the same aggregate on the server.
    let mut limited = connection(&target, &target.user, true);
    limited.limits.statement_timeout_seconds = 1;
    let limited = open(&limited, &target.password).await.unwrap();
    let started = Instant::now();
    let timed_out = consume(&limited, &request(slow)).await.unwrap();
    assert_eq!(timed_out.error.as_deref(), Some("timeout"));
    assert!(started.elapsed() < Duration::from_secs(8));
    limited.close();

    // A user with only the `read` role is verified read-only by its privileges,
    // and the server refuses its writes even where the client guard allows them.
    let _ = administrator
        .database("admin")
        .run_command(doc! {"dropUser": "cc_reader"})
        .await;
    administrator
        .database("admin")
        .run_command(doc! {
            "createUser": "cc_reader", "pwd": "cc-reader-password",
            "roles": [{"role": "read", "db": target.database.as_str()}],
        })
        .await
        .unwrap();
    let reader = open(
        &connection(&target, "cc_reader", true),
        "cc-reader-password",
    )
    .await
    .unwrap();
    assert_eq!(
        reader.info().server_read_only,
        DbServerReadOnly::RoleVerified
    );
    assert_eq!(
        run(&reader, "db.items.estimatedDocumentCount()")
            .await
            .values("n"),
        ["1500"]
    );
    reader.close();
    let reader_dev = open(
        &connection(&target, "cc_reader", false),
        "cc-reader-password",
    )
    .await
    .unwrap();
    assert_eq!(
        write(&reader_dev, r#"db.items.insertOne({"x":1})"#)
            .await
            .err()
            .as_deref(),
        Some("policy_denied")
    );
    reader_dev.close();
    assert!(matches!(
        open(&connection(&target, "cc_reader", true), "wrong-password").await,
        Err(DbError::AuthenticationFailed)
    ));

    write(&dev, "db.items.drop()").await.unwrap();
    dev.close();
    administrator
        .database("admin")
        .run_command(doc! {"dropUser": "cc_reader"})
        .await
        .unwrap();
    administrator
        .database(&target.database)
        .drop()
        .await
        .unwrap();
}

#[tokio::test]
async fn mongo_replica_set_members_are_reached_through_the_relay() {
    let Ok(spec) = std::env::var("CC_DB_LIVE_MONGO_RS") else {
        eprintln!("CC_DB_LIVE_MONGO_RS not set; skipped");
        return;
    };
    let parts: Vec<&str> = spec.splitn(3, ':').collect();
    let configure = |set: &str| {
        let mut connection = DbConnection::new("rs", parts[0], DbEngine::Mongodb);
        connection.port = parts[1].parse().unwrap();
        connection.database = "cc_rs".into();
        connection.production = false;
        connection.tls.mode = DbTlsMode::Disable;
        connection.limits.connect_timeout_seconds = 3;
        connection.options = DbOptions::Mongodb {
            auth_source: None,
            replica_set: Some(set.into()),
            srv: false,
            read_preference: MongoReadPreference::Primary,
        };
        connection
    };
    let session = Arc::new(
        DbSession::open(
            "rs".into(),
            &configure(parts[2]),
            &DbCredentials::default(),
            Arc::new(DirectConnector),
            false,
        )
        .await
        .unwrap(),
    );
    // Writes need the primary, which is known only by its announced name.
    write(&session, "db.probe.drop()").await.ok();
    write(&session, r#"db.probe.insertOne({"x":1})"#)
        .await
        .unwrap();
    assert_eq!(
        run(&session, r#"db.probe.find({}, {"_id":0})"#)
            .await
            .values("x"),
        ["1"]
    );
    write(&session, "db.probe.drop()").await.unwrap();
    session.close();
    assert!(DbSession::open(
        "rs".into(),
        &configure("not-this-set"),
        &DbCredentials::default(),
        Arc::new(DirectConnector),
        false,
    )
    .await
    .is_err());
}
