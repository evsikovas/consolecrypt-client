//! Opt-in acceptance against a disposable local ClickHouse:
//! CC_DB_LIVE_CLICKHOUSE=host:port:user:password:database (HTTP port).
use cc_db_core::{
    transport::{DbCredentials, DirectConnector},
    DbCell, DbError, DbObjectKind, DbParameter, DbQueryEvent, DbQueryRequest, DbServerReadOnly,
    DbSession,
};
use cc_models::database::{DbConnection, DbEngine, DbTlsMode};
use secrecy::SecretString;
use std::{
    sync::Arc,
    time::{Duration, Instant},
};

const VARIABLE: &str = "CC_DB_LIVE_CLICKHOUSE";

fn target(production: bool) -> Option<(DbConnection, DbCredentials)> {
    let spec = std::env::var(VARIABLE).ok()?;
    let parts: Vec<&str> = spec.splitn(5, ':').collect();
    let mut connection = DbConnection::new("live", parts[0], DbEngine::Clickhouse);
    connection.port = parts[1].parse().unwrap();
    connection.username = parts[2].into();
    connection.database = parts[4].into();
    connection.production = production;
    connection.tls.mode = DbTlsMode::Disable;
    Some((
        connection,
        DbCredentials {
            password: Some(SecretString::from(parts[3].to_owned())),
            client_key_pem: None,
        },
    ))
}

async fn open(connection: &DbConnection, credentials: &DbCredentials) -> Arc<DbSession> {
    Arc::new(
        DbSession::open(
            connection.name.clone(),
            connection,
            credentials,
            Arc::new(DirectConnector),
            false,
        )
        .await
        .unwrap(),
    )
}

fn request(text: &str) -> DbQueryRequest {
    DbQueryRequest {
        text: text.into(),
        parameters: Vec::new(),
        approval_ticket: None,
    }
}

fn approved(session: &DbSession, query: DbQueryRequest) -> Result<DbQueryRequest, String> {
    let review = session
        .guard()
        .review(&query)
        .map_err(|error| format!("{error:?}"))?;
    session
        .guard()
        .approve(&review.ticket)
        .map_err(|error| format!("{error:?}"))?;
    let mut query = query;
    query.approval_ticket = Some(review.ticket.clone());
    Ok(query)
}

/// Writes go through the native review: ticket, explicit approval, execution.
async fn write(session: &Arc<DbSession>, sql: &str) -> Result<Vec<Vec<DbCell>>, String> {
    let query = approved(session, request(sql))?;
    consume(session, &query).await
}

async fn run(session: &Arc<DbSession>, sql: &str) -> Result<Vec<Vec<DbCell>>, String> {
    consume(session, &request(sql)).await
}

async fn consume(
    session: &Arc<DbSession>,
    query: &DbQueryRequest,
) -> Result<Vec<Vec<DbCell>>, String> {
    let mut stream = session
        .execute(query)
        .map_err(|error| format!("{error:?}"))?;
    let mut rows = Vec::new();
    while let Some(event) = stream.events.recv().await {
        match event {
            DbQueryEvent::Rows { rows: batch, .. } => rows.extend(batch),
            DbQueryEvent::Error { code, .. } => return Err(code),
            DbQueryEvent::Done => break,
            DbQueryEvent::Cancelled => return Err("cancelled".into()),
            _ => {}
        }
    }
    Ok(rows)
}

fn text(value: &str) -> DbCell {
    DbCell::Text(value.into())
}

/// Starts `query`, cancels it after `after` and returns how long the stream
/// took to report cancellation.
async fn cancel_after(
    session: &Arc<DbSession>,
    query: &DbQueryRequest,
    after: Duration,
) -> Duration {
    let mut stream = session.execute(query).unwrap();
    tokio::time::sleep(after).await;
    let cancelled = Instant::now();
    stream.cancel();
    let outcome = tokio::time::timeout(Duration::from_secs(10), async {
        while let Some(event) = stream.events.recv().await {
            match event {
                DbQueryEvent::Cancelled => return true,
                DbQueryEvent::Done | DbQueryEvent::Error { .. } => return false,
                _ => {}
            }
        }
        // The receiver closes once the cancelled sink is dropped.
        true
    })
    .await
    .unwrap();
    assert!(outcome, "query finished instead of being cancelled");
    cancelled.elapsed()
}

async fn running(session: &Arc<DbSession>, marker: &str) -> usize {
    let mut query = request(
        "SELECT count() FROM system.processes WHERE position(query, {{marker}}) > 0 \
         AND position(query, 'system.processes') = 0",
    );
    query.parameters = vec![DbParameter {
        name: "marker".into(),
        value: text(marker),
        type_hint: None,
    }];
    let rows = consume(session, &query).await.unwrap();
    match &rows[0][0] {
        DbCell::Uint(count) => count.parse().unwrap(),
        other => panic!("unexpected count cell {other:?}"),
    }
}

#[tokio::test]
async fn clickhouse_production_reads_and_development_writes() {
    let Some((connection, credentials)) = target(true) else {
        eprintln!("{VARIABLE} not set; skipped");
        return;
    };
    let production = open(&connection, &credentials).await;
    let info = production.info();
    assert!(info.read_only && info.production);
    assert_eq!(info.server_read_only, DbServerReadOnly::Enforced);
    assert!(!info.server_version.is_empty());
    assert!(!format!("{production:?}").contains("cc-test-ch"));
    assert_eq!(
        run(&production, "SELECT 1 + 1").await.unwrap(),
        vec![vec![DbCell::Uint("2".into())]]
    );
    // Production refuses every write and every protection bypass before it
    // reaches the server (readonly=2 is the second layer, see the unit test).
    for sql in [
        "INSERT INTO cc_items (id) VALUES (1)",
        "INSERT INTO FUNCTION url('http://127.0.0.1:9/x', 'CSV', 'id UInt8') VALUES (1)",
        "INSERT INTO FUNCTION s3('http://127.0.0.1:9/b/x.csv', 'CSV', 'id UInt8') VALUES (1)",
        "SELECT * FROM url('http://127.0.0.1:9/x', 'CSV', 'id UInt8')",
        "SELECT * FROM remote('127.0.0.1', system.one)",
        "ALTER TABLE cc_items ADD COLUMN x UInt8",
        "ALTER TABLE cc_items DELETE WHERE 1",
        "CREATE TABLE cc_probe (id UInt8) ENGINE = Memory",
        "DROP TABLE cc_items",
        "TRUNCATE TABLE cc_items",
        "OPTIMIZE TABLE cc_items FINAL",
        "KILL QUERY WHERE 1",
        "SYSTEM DROP DNS CACHE",
        "SYSTEM SHUTDOWN",
        "SET readonly = 0",
        "SET readonly = 2",
        "SELECT 1 SETTINGS readonly = 0",
        "SELECT 1 INTO OUTFILE '/tmp/cc.csv'",
        "SELECT 1 FORMAT TabSeparated",
        "SELECT 1; INSERT INTO cc_items (id) VALUES (1)",
    ] {
        assert!(
            matches!(
                production.execute(&request(sql)),
                Err(DbError::PolicyDenied | DbError::ApprovalRequired | DbError::InvalidRequest)
            ),
            "{sql}"
        );
    }
    // Cancel stops a long server-side read quickly and kills it on the server.
    let marker = "cc_cancel_probe_production";
    let heavy = request(&format!(
        "SELECT count() AS {marker} FROM numbers(1000000000000) WHERE sipHash64(number) = 0"
    ));
    let elapsed = cancel_after(&production, &heavy, Duration::from_millis(700)).await;
    assert!(elapsed < Duration::from_secs(2), "cancel took {elapsed:?}");
    let deadline = Instant::now() + Duration::from_secs(5);
    while running(&production, marker).await > 0 {
        assert!(Instant::now() < deadline, "server query survived cancel");
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    production.close();

    let (mut connection, credentials) = target(false).unwrap();
    connection.name = "dev".into();
    let dev = open(&connection, &credentials).await;
    assert!(!dev.info().read_only);
    write(&dev, "DROP TABLE IF EXISTS cc_items").await.unwrap();
    write(&dev, "DROP VIEW IF EXISTS cc_items_view")
        .await
        .unwrap();
    write(
        &dev,
        "CREATE TABLE cc_items (id UInt64, name String DEFAULT 'none', secret_token Nullable(String), \
         price Decimal(18, 4), big Int128, ratio Float64, day Date, seen DateTime64(3, 'UTC'), \
         tags Array(String), attributes Map(String, UInt64), active Bool) \
         ENGINE = MergeTree ORDER BY id",
    )
    .await
    .unwrap();
    write(
        &dev,
        "CREATE VIEW cc_items_view AS SELECT id, name FROM cc_items",
    )
    .await
    .unwrap();
    write(
        &dev,
        "INSERT INTO cc_items VALUES \
         (1, 'alpha', 't1', 12345678901234.5678, -170141183460469231731687303715884105728, 0.1, \
          '2024-01-02', '2024-01-02 03:04:05.678', ['a', 'b'], {'k': 18446744073709551615}, true), \
         (18446744073709551615, 'beta', NULL, 0, 0, 2.5, '1970-01-01', '1970-01-01 00:00:00', [], {}, false)",
    )
    .await
    .unwrap();
    let rows = run(
        &dev,
        "SELECT id, name, secret_token, price, big, ratio, day, seen, tags, attributes, active \
         FROM cc_items ORDER BY id",
    )
    .await
    .unwrap();
    assert_eq!(
        rows,
        vec![
            vec![
                DbCell::Uint("1".into()),
                text("alpha"),
                text("t1"),
                DbCell::Decimal("12345678901234.5678".into()),
                DbCell::Int("-170141183460469231731687303715884105728".into()),
                DbCell::Float("0.1".into()),
                DbCell::Date("2024-01-02".into()),
                DbCell::Timestamp("2024-01-02 03:04:05.678".into()),
                DbCell::Json("[\"a\",\"b\"]".into()),
                DbCell::Json("{\"k\":\"18446744073709551615\"}".into()),
                DbCell::Bool(true),
            ],
            vec![
                DbCell::Uint("18446744073709551615".into()),
                text("beta"),
                DbCell::Null,
                DbCell::Decimal("0".into()),
                DbCell::Int("0".into()),
                DbCell::Float("2.5".into()),
                DbCell::Date("1970-01-01".into()),
                DbCell::Timestamp("1970-01-01 00:00:00.000".into()),
                DbCell::Json("[]".into()),
                DbCell::Json("{}".into()),
                DbCell::Bool(false),
            ],
        ]
    );
    // Values bind as typed server-side parameters, never as SQL text.
    let tricky = "it's a \"quote\", a \\ backslash,\ta tab\nand a newline; DROP TABLE cc_items";
    let mut bound = request(
        "SELECT {{value}} AS value, length({{value}}) AS size, name FROM cc_items WHERE id = {{id}}",
    );
    bound.parameters = vec![
        DbParameter {
            name: "value".into(),
            value: text(tricky),
            type_hint: None,
        },
        DbParameter {
            name: "id".into(),
            value: DbCell::Uint("18446744073709551615".into()),
            type_hint: None,
        },
    ];
    assert_eq!(
        consume(&dev, &bound).await.unwrap(),
        vec![vec![
            text(tricky),
            DbCell::Uint(tricky.len().to_string()),
            text("beta"),
        ]]
    );
    let mut nullable = request("SELECT isNull({{missing}})");
    nullable.parameters = vec![DbParameter {
        name: "missing".into(),
        value: DbCell::Null,
        type_hint: None,
    }];
    assert_eq!(
        consume(&dev, &nullable).await.unwrap(),
        vec![vec![DbCell::Uint("1".into())]]
    );
    // Read-only statements of a development session still carry readonly=2,
    // and statement errors come back as fixed codes without server text.
    assert_eq!(
        run(&dev, "SELECT * FROM cc_missing_table").await,
        Err("query_failed".into())
    );

    // Structure: databases (system last) → tables and views → columns.
    let roots = dev.tree(None, None).await.unwrap();
    let names: Vec<_> = roots
        .nodes
        .iter()
        .map(|node| node.reference.name.as_str())
        .collect();
    assert!(names.contains(&connection.database.as_str()));
    assert_eq!(names.last(), Some(&"system"));
    assert!(!names.contains(&"INFORMATION_SCHEMA"));
    let database = roots
        .nodes
        .iter()
        .find(|node| node.reference.name == connection.database)
        .unwrap();
    assert_eq!(database.reference.kind, DbObjectKind::Database);
    let relations = dev.tree(Some(&database.reference), None).await.unwrap();
    let table = relations
        .nodes
        .iter()
        .find(|node| node.reference.name == "cc_items")
        .unwrap();
    assert_eq!(table.reference.kind, DbObjectKind::Table);
    assert_eq!(table.detail.as_deref(), Some("MergeTree"));
    let view = relations
        .nodes
        .iter()
        .find(|node| node.reference.name == "cc_items_view")
        .unwrap();
    assert_eq!(view.reference.kind, DbObjectKind::View);
    let columns = dev.tree(Some(&table.reference), None).await.unwrap();
    let id = columns
        .nodes
        .iter()
        .find(|node| node.reference.name == "id")
        .unwrap();
    assert_eq!(id.reference.kind, DbObjectKind::Column);
    assert_eq!(id.reference.parent_name.as_deref(), Some("cc_items"));
    assert_eq!(id.detail.as_deref(), Some("UInt64"));
    assert!(id.primary_key);
    assert_eq!(id.nullable, Some(false));
    let token = columns
        .nodes
        .iter()
        .find(|node| node.reference.name == "secret_token")
        .unwrap();
    assert_eq!(token.nullable, Some(true));
    assert!(!token.primary_key);
    let name = columns
        .nodes
        .iter()
        .find(|node| node.reference.name == "name")
        .unwrap();
    assert_eq!(name.default_value.as_deref(), Some("'none'"));
    let view_columns = dev.tree(Some(&view.reference), None).await.unwrap();
    assert_eq!(view_columns.nodes.len(), 2);

    // Cancel of an approved long statement stops it quickly.
    let sleepy = approved(&dev, request("SELECT sleepEachRow(0.1) FROM numbers(30)")).unwrap();
    let elapsed = cancel_after(&dev, &sleepy, Duration::from_millis(300)).await;
    assert!(elapsed < Duration::from_secs(1), "cancel took {elapsed:?}");

    write(&dev, "DROP VIEW cc_items_view").await.unwrap();
    write(&dev, "DROP TABLE cc_items").await.unwrap();
    dev.close();

    // The statement timeout is enforced by the server (max_execution_time).
    let (mut connection, credentials) = target(true).unwrap();
    connection.name = "timeout".into();
    connection.limits.statement_timeout_seconds = 1;
    let limited = open(&connection, &credentials).await;
    let started = Instant::now();
    assert_eq!(
        run(
            &limited,
            "SELECT count() FROM numbers(1000000000000) WHERE sipHash64(number) = 0"
        )
        .await,
        Err("timeout".into())
    );
    assert!(started.elapsed() < Duration::from_secs(5));
    limited.close();
}

#[tokio::test]
async fn clickhouse_production_write_window() {
    let Some((mut connection, credentials)) = target(true) else {
        eprintln!("{VARIABLE} not set; skipped");
        return;
    };
    connection.name = "orders-prod".into();
    let prod = open(&connection, &credentials).await;
    let create = "CREATE TABLE cc_prod_window (id UInt32) ENGINE = MergeTree ORDER BY id";
    assert!(write(&prod, create).await.is_err());
    prod.guard()
        .open_production_write(Duration::from_secs(300))
        .unwrap();
    write(&prod, "DROP TABLE IF EXISTS cc_prod_window")
        .await
        .unwrap();
    write(&prod, create).await.unwrap();
    write(&prod, "INSERT INTO cc_prod_window VALUES (1)")
        .await
        .unwrap();
    assert_eq!(
        run(&prod, "SELECT count() FROM cc_prod_window")
            .await
            .unwrap()
            .len(),
        1
    );
    prod.guard().close_production_write();
    assert!(write(&prod, "INSERT INTO cc_prod_window VALUES (2)")
        .await
        .is_err());
    prod.guard()
        .open_production_write(Duration::from_secs(300))
        .unwrap();
    write(&prod, "DROP TABLE cc_prod_window").await.unwrap();
    prod.close();
}

#[tokio::test]
async fn clickhouse_wrong_password_is_an_authentication_error() {
    let Some((connection, _)) = target(true) else {
        eprintln!("{VARIABLE} not set; skipped");
        return;
    };
    let credentials = DbCredentials {
        password: Some(SecretString::from("wrong-canary".to_owned())),
        client_key_pem: None,
    };
    let error = DbSession::open(
        "bad".into(),
        &connection,
        &credentials,
        Arc::new(DirectConnector),
        false,
    )
    .await
    .unwrap_err();
    assert_eq!(error, DbError::AuthenticationFailed);
    assert!(!format!("{error:?} {error}").contains("canary"));
}

#[tokio::test]
async fn clickhouse_introspection_explain_and_completion() {
    let Some((mut connection, credentials)) = target(false) else {
        eprintln!("{VARIABLE} not set; skipped");
        return;
    };
    connection.name = "introspection".into();
    let dev = open(&connection, &credentials).await;
    write(&dev, "DROP TABLE IF EXISTS cc_intro").await.ok();
    write(
        &dev,
        "CREATE TABLE cc_intro (id UInt64, title String) ENGINE = MergeTree ORDER BY id",
    )
    .await
    .unwrap();
    let table = cc_db_core::DbObjectRef {
        database: connection.database.clone(),
        schema: None,
        kind: cc_db_core::DbObjectKind::Table,
        name: "cc_intro".into(),
        parent_name: None,
    };
    assert!(dev.ddl(&table).await.unwrap().contains("CREATE TABLE"));
    assert_eq!(
        dev.object_query(&table, 5).await.unwrap(),
        format!(
            "SELECT * FROM `{}`.`cc_intro` LIMIT 5;",
            connection.database
        )
    );
    let plan = dev
        .explain(&request("SELECT title FROM cc_intro WHERE id = 1"), false)
        .await
        .unwrap();
    assert!(!plan["plan"].is_null(), "{plan}");
    let found = dev.complete("SELECT * FROM cc_in", 18).await.unwrap();
    assert!(found.iter().any(|c| c.label == "cc_intro"), "{found:?}");
    write(&dev, "DROP TABLE cc_intro").await.unwrap();
}
