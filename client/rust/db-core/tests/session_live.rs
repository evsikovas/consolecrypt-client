//! Opt-in acceptance against disposable local databases:
//! CC_DB_LIVE_PG=host:port:user:password:database (PostgreSQL) and
//! CC_DB_LIVE_MYSQL=host:port:user:password:database (MySQL/MariaDB).
use cc_db_core::{
    transport::{DbCredentials, DirectConnector},
    DbCell, DbError, DbObjectKind, DbQueryEvent, DbQueryRequest, DbSession,
};
use cc_models::database::{DbConnection, DbEngine, DbTlsMode};
use secrecy::SecretString;
use std::sync::Arc;

fn target(
    variable: &str,
    engine: DbEngine,
    production: bool,
) -> Option<(DbConnection, DbCredentials)> {
    let spec = std::env::var(variable).ok()?;
    let parts: Vec<&str> = spec.splitn(5, ':').collect();
    let mut connection = DbConnection::new("live", parts[0], engine);
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

fn request(text: &str) -> DbQueryRequest {
    DbQueryRequest {
        text: text.into(),
        parameters: Vec::new(),
        approval_ticket: None,
    }
}

/// Writes go through the native review: ticket, explicit approval, execution.
async fn write(session: &Arc<DbSession>, sql: &str) -> Result<Vec<Vec<String>>, String> {
    let review = session
        .guard()
        .review(&request(sql))
        .map_err(|error| format!("{error:?}"))?;
    session
        .guard()
        .approve(&review.ticket)
        .map_err(|error| format!("{error:?}"))?;
    let mut approved = request(sql);
    approved.approval_ticket = Some(review.ticket.clone());
    consume(session, &approved).await
}

async fn run(session: &Arc<DbSession>, sql: &str) -> Result<Vec<Vec<String>>, String> {
    consume(session, &request(sql)).await
}

async fn consume(
    session: &Arc<DbSession>,
    query: &DbQueryRequest,
) -> Result<Vec<Vec<String>>, String> {
    let mut stream = session
        .execute(query)
        .map_err(|error| format!("{error:?}"))?;
    let mut rows = Vec::new();
    while let Some(event) = stream.events.recv().await {
        match event {
            DbQueryEvent::Rows { rows: batch, .. } => rows.extend(batch.iter().map(|row| {
                row.iter()
                    .map(|cell| match cell {
                        DbCell::Null => "NULL".into(),
                        DbCell::Bool(value) => value.to_string(),
                        DbCell::Int(v)
                        | DbCell::Uint(v)
                        | DbCell::Decimal(v)
                        | DbCell::Float(v)
                        | DbCell::Text(v) => v.clone(),
                        _ => "?".into(),
                    })
                    .collect()
            })),
            DbQueryEvent::Error { code, .. } => return Err(code),
            DbQueryEvent::Done | DbQueryEvent::Cancelled => break,
            _ => {}
        }
    }
    Ok(rows)
}

async fn exercise(variable: &str, engine: DbEngine) {
    let Some((connection, credentials)) = target(variable, engine, true) else {
        eprintln!("{variable} not set; skipped");
        return;
    };
    let session = Arc::new(
        DbSession::open(
            "prod".into(),
            &connection,
            &credentials,
            Arc::new(DirectConnector),
            false,
        )
        .await
        .unwrap(),
    );
    assert!(session.info().read_only && session.info().production);
    assert_eq!(
        run(&session, "SELECT 1 + 1").await.unwrap(),
        vec![vec!["2".to_string()]]
    );
    // Production refuses writes before they reach the server.
    assert!(matches!(
        session.execute(&request("CREATE TABLE cc_probe (id int)")),
        Err(DbError::PolicyDenied | DbError::ApprovalRequired | DbError::InvalidRequest)
    ));
    assert!(session.explain(&request("SELECT 1"), false).await.is_ok());
    assert!(session.explain(&request("SELECT 1"), true).await.is_err());
    let roots = session.tree(None, None).await.unwrap();
    assert!(!roots.nodes.is_empty());
    session.close();

    let (mut connection, credentials) = target(variable, engine, false).unwrap();
    connection.name = "dev".into();
    let dev = Arc::new(
        DbSession::open(
            "dev".into(),
            &connection,
            &credentials,
            Arc::new(DirectConnector),
            false,
        )
        .await
        .unwrap(),
    );
    assert!(!dev.info().read_only);
    write(&dev, "DROP TABLE IF EXISTS cc_items").await.ok();
    write(&dev, "CREATE TABLE cc_items (id int primary key, name varchar(40) not null, secret_token varchar(40))").await.unwrap();
    write(&dev, "CREATE INDEX cc_items_name ON cc_items (name)")
        .await
        .unwrap();
    write(&dev, "INSERT INTO cc_items (id, name, secret_token) VALUES (1, 'alpha', 't1'), (2, 'beta', 't2')").await.unwrap();
    assert_eq!(
        run(&dev, "SELECT name FROM cc_items ORDER BY id")
            .await
            .unwrap()
            .len(),
        2
    );
    // Structure: the table and its primary-key column are visible.
    let parent = match engine {
        DbEngine::Postgresql => {
            dev.tree(None, None)
                .await
                .unwrap()
                .nodes
                .into_iter()
                .find(|node| node.reference.name == "public")
                .unwrap()
                .reference
        }
        _ => {
            dev.tree(None, None)
                .await
                .unwrap()
                .nodes
                .into_iter()
                .find(|node| node.reference.name == connection.database)
                .unwrap()
                .reference
        }
    };
    let tables = dev.tree(Some(&parent), None).await.unwrap();
    let table = tables
        .nodes
        .iter()
        .find(|node| node.reference.name == "cc_items")
        .unwrap()
        .reference
        .clone();
    assert_eq!(table.kind, DbObjectKind::Table);
    let columns = dev.tree(Some(&table), None).await.unwrap();
    assert!(columns
        .nodes
        .iter()
        .any(|node| node.reference.name == "id" && node.primary_key));
    assert!(columns
        .nodes
        .iter()
        .any(|node| node.reference.name == "cc_items_name"
            && node.reference.kind == DbObjectKind::Index));
    let id = columns
        .nodes
        .iter()
        .position(|node| node.reference.name == "id")
        .unwrap();
    let index = columns
        .nodes
        .iter()
        .position(|node| node.reference.kind == DbObjectKind::Index)
        .unwrap();
    assert!(id < index, "columns come first");
    // DDL, plans and completion read the same objects.
    let ddl = dev.ddl(&table).await.unwrap();
    assert!(
        ddl.contains("CREATE TABLE") && ddl.contains("cc_items") && ddl.contains("secret_token"),
        "{ddl}"
    );
    let plan = dev
        .explain(&request("SELECT name FROM cc_items WHERE id = 1"), false)
        .await
        .unwrap();
    assert_eq!(plan["analyzed"], false);
    assert!(!plan["plan"].is_null());
    let analyzed = dev
        .explain(&request("SELECT count(*) FROM cc_items"), true)
        .await
        .unwrap();
    assert_eq!(analyzed["analyzed"], true);
    let completions = dev.complete("SELECT * FROM cc_it", 19).await.unwrap();
    assert!(completions
        .iter()
        .any(|c| c.label == "cc_items" && c.insert_text == "ems"));
    let columns = dev
        .complete("SELECT i.secr FROM cc_items i", 13)
        .await
        .unwrap();
    assert!(
        columns.iter().any(|c| c.label == "secret_token"),
        "{columns:?}"
    );
    write(&dev, "DROP TABLE cc_items").await.unwrap();
    dev.close();
}

#[tokio::test]
async fn postgres_production_reads_and_development_writes() {
    exercise("CC_DB_LIVE_PG", DbEngine::Postgresql).await;
}

#[tokio::test]
async fn mysql_production_reads_and_development_writes() {
    exercise("CC_DB_LIVE_MYSQL", DbEngine::Mysql).await;
}
