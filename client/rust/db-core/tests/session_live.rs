//! Opt-in acceptance against disposable local databases:
//! CC_DB_LIVE_PG=host:port:user:password:database (PostgreSQL) and
//! CC_DB_LIVE_MYSQL=host:port:user:password:database (MySQL/MariaDB).
//! CC_DB_LIVE_PG_TLS (same form, a server with ssl=on and SCRAM) with
//! CC_DB_LIVE_PG_TLS_CA=path of the CA that issued its certificate for the host.
use async_trait::async_trait;
use cc_db_core::{
    transport::{DbConnector, DbCredentials, DbDestination, DbIo, DbPath, DirectConnector},
    DbCell, DbError, DbObjectKind, DbQueryEvent, DbQueryRequest, DbSession, DbTransportSecurity,
};
use cc_models::database::{DbConnection, DbEngine, DbTlsMode};
use secrecy::SecretString;
use std::sync::Arc;
use tokio_util::sync::CancellationToken;

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
    // The test role is a superuser: production warns about it.
    let report = session.privileges().await.unwrap();
    assert_eq!(report.has_write_privileges, Some(true));
    assert!(report.warning_codes.contains(&"production_role_can_write"));
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
    // A single-table read is editable by its primary key.
    let mut stream = dev
        .execute(&request("SELECT * FROM cc_items ORDER BY id"))
        .unwrap();
    let mut editable = None;
    while let Some(event) = stream.events.recv().await {
        match event {
            DbQueryEvent::Started {
                columns,
                editable_table,
                ..
            } => {
                assert!(columns.iter().any(|c| c.name == "id" && c.primary_key));
                editable = editable_table;
            }
            DbQueryEvent::Done | DbQueryEvent::Error { .. } => break,
            _ => {}
        }
    }
    let editable = editable.expect("editable source table");
    let edit = |id: &str, value: &str| cc_db_core::edit::DbCellEdit {
        original_primary_key: [("id".to_string(), DbCell::Int(id.into()))].into(),
        column: "name".into(),
        value: DbCell::Text(value.into()),
    };
    let apply = |edits: Vec<cc_db_core::edit::DbCellEdit>| {
        let dev = dev.clone();
        let editable = editable.clone();
        async move {
            let mut edit_request = dev.review_cell_edits(&editable, &edits).await?.request;
            let review = dev.guard().review(&edit_request)?;
            dev.guard().approve(&review.ticket)?;
            edit_request.approval_ticket = Some(review.ticket.clone());
            dev.apply_cell_edits(&edit_request).await
        }
    };
    assert_eq!(apply(vec![edit("1", "it's alpha")]).await.unwrap(), 1);
    // One edit misses its row: the whole batch rolls back.
    assert!(apply(vec![edit("2", "changed"), edit("99", "ghost")])
        .await
        .is_err());
    assert_eq!(
        run(&dev, "SELECT name FROM cc_items ORDER BY id")
            .await
            .unwrap(),
        vec![vec!["it's alpha".to_string()], vec!["beta".to_string()]]
    );
    write(&dev, "UPDATE cc_items SET name = 'alpha' WHERE id = 1")
        .await
        .unwrap();
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
    // A file export runs the read-only query again and streams every row.
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("items.csv");
    let progress = std::sync::Arc::new(cc_db_core::export::ExportProgress::default());
    let options: cc_db_core::export::ExportOptions =
        serde_json::from_value(serde_json::json!({"format": "csv"})).unwrap();
    dev.export(
        &request("SELECT id, name FROM cc_items ORDER BY id"),
        0,
        options.clone(),
        path.clone(),
        progress.clone(),
        tokio_util::sync::CancellationToken::new(),
    )
    .await
    .unwrap();
    assert_eq!(
        std::fs::read_to_string(&path).unwrap(),
        "id,name\r\n1,alpha\r\n2,beta\r\n"
    );
    assert!(dev
        .export(
            &request("DELETE FROM cc_items WHERE id = 1"),
            0,
            options,
            directory.path().join("no.csv"),
            progress,
            tokio_util::sync::CancellationToken::new(),
        )
        .await
        .is_err());
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

/// A production session writes only inside its write window, through the
/// usual review, and is read-only again once the window closes.
async fn production_window(variable: &str, engine: DbEngine) {
    let Some((mut connection, credentials)) = target(variable, engine, true) else {
        eprintln!("{variable} not set; skipped");
        return;
    };
    connection.name = "orders-prod".into();
    let session = Arc::new(
        DbSession::open(
            "prod-window".into(),
            &connection,
            &credentials,
            Arc::new(DirectConnector),
            false,
        )
        .await
        .unwrap(),
    );
    let five = std::time::Duration::from_secs(300);
    assert!(write(&session, "CREATE TABLE cc_prod_window (id int)")
        .await
        .is_err());
    assert!(session
        .guard()
        .open_production_write("orders", five)
        .is_err());
    session
        .guard()
        .open_production_write("orders-prod", five)
        .unwrap();
    write(&session, "DROP TABLE IF EXISTS cc_prod_window")
        .await
        .unwrap();
    write(
        &session,
        "CREATE TABLE cc_prod_window (id int primary key, note varchar(20))",
    )
    .await
    .unwrap();
    write(&session, "INSERT INTO cc_prod_window VALUES (1, 'kept')")
        .await
        .unwrap();
    assert_eq!(
        run(&session, "SELECT note FROM cc_prod_window")
            .await
            .unwrap(),
        vec![vec!["kept".to_string()]]
    );
    // Reads still run in server read-only transactions inside the window.
    assert!(run(&session, "SELECT 1").await.is_ok());
    session.guard().close_production_write();
    assert!(session.guard().is_read_only());
    assert!(
        write(&session, "INSERT INTO cc_prod_window VALUES (2, 'lost')")
            .await
            .is_err()
    );
    assert_eq!(
        run(&session, "SELECT count(*) FROM cc_prod_window")
            .await
            .unwrap(),
        vec![vec!["1".to_string()]]
    );
    session
        .guard()
        .open_production_write("orders-prod", five)
        .unwrap();
    write(&session, "DROP TABLE cc_prod_window").await.unwrap();
    session.close();
}

#[tokio::test]
async fn postgres_production_write_window() {
    production_window("CC_DB_LIVE_PG", DbEngine::Postgresql).await;
}

#[tokio::test]
async fn mysql_production_write_window() {
    production_window("CC_DB_LIVE_MYSQL", DbEngine::Mysql).await;
}

/// Dials the address directly but reports the path of a real route: the
/// network (direct to another host) or a tunnel (Kubernetes forward).
#[derive(Debug)]
struct Route(DbPath);

#[async_trait]
impl DbConnector for Route {
    async fn connect(
        &self,
        destination: &DbDestination,
        cancel: CancellationToken,
    ) -> Result<DbIo, DbError> {
        DirectConnector.connect(destination, cancel).await
    }
    async fn connect_path(
        &self,
        destination: &DbDestination,
        cancel: CancellationToken,
    ) -> Result<(DbIo, DbPath), DbError> {
        Ok((self.connect(destination, cancel).await?, self.0))
    }
}

async fn open(
    connection: &DbConnection,
    credentials: &DbCredentials,
    connector: Arc<dyn DbConnector>,
) -> Result<DbSession, DbError> {
    DbSession::open("tls".into(), connection, credentials, connector, false).await
}

#[tokio::test]
async fn postgres_prefer_without_tls_only_on_loopback_or_through_a_tunnel() {
    let Some((mut connection, credentials)) = target("CC_DB_LIVE_PG", DbEngine::Postgresql, true)
    else {
        eprintln!("CC_DB_LIVE_PG not set; skipped");
        return;
    };
    connection.tls.mode = DbTlsMode::Prefer;
    for connector in [
        Arc::new(DirectConnector) as Arc<dyn DbConnector>,
        Arc::new(Route(DbPath::Tunnel)),
    ] {
        let session = open(&connection, &credentials, connector).await.unwrap();
        assert_eq!(
            session.info().transport_security,
            DbTransportSecurity::Tunnel
        );
    }
    // The same server reached over the network: no plaintext password.
    assert_eq!(
        open(&connection, &credentials, Arc::new(Route(DbPath::Network)))
            .await
            .err(),
        Some(DbError::TlsUnavailable)
    );
    connection.tls.mode = DbTlsMode::Disable;
    let session = open(&connection, &credentials, Arc::new(Route(DbPath::Network)))
        .await
        .unwrap();
    assert_eq!(
        session.info().transport_security,
        DbTransportSecurity::Plaintext
    );
}

#[tokio::test]
async fn postgres_tls_binds_scram_to_the_session() {
    let Some((mut connection, credentials)) =
        target("CC_DB_LIVE_PG_TLS", DbEngine::Postgresql, true)
    else {
        eprintln!("CC_DB_LIVE_PG_TLS not set; skipped");
        return;
    };
    let network = || Arc::new(Route(DbPath::Network)) as Arc<dyn DbConnector>;
    // Channel binding is required without verification: the session opens
    // only through SCRAM-SHA-256-PLUS.
    connection.tls.mode = DbTlsMode::Require;
    let session = open(&connection, &credentials, network()).await.unwrap();
    assert_eq!(
        session.info().transport_security,
        DbTransportSecurity::Unverified
    );
    assert_eq!(
        run(&Arc::new(session), "SELECT 1").await.unwrap(),
        vec![vec!["1".to_string()]]
    );
    let Ok(authority) = std::env::var("CC_DB_LIVE_PG_TLS_CA") else {
        return;
    };
    connection.tls.certificate_authority_pem = Some(std::fs::read_to_string(authority).unwrap());
    for mode in [DbTlsMode::Verify, DbTlsMode::Prefer] {
        connection.tls.mode = mode;
        for connector in [network(), Arc::new(DirectConnector)] {
            let session = open(&connection, &credentials, connector).await.unwrap();
            assert_eq!(
                session.info().transport_security,
                DbTransportSecurity::Verified
            );
        }
    }
}
