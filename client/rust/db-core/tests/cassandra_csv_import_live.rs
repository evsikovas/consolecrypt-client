//! Opt-in CSV import into ScyllaDB / Cassandra (FEAT-09):
//! CC_DB_LIVE_SCYLLA / CC_DB_LIVE_CASSANDRA = host:port[:user:password], as
//! in `cassandra_live.rs`. Writes to the keyspace `cc_csv_live`, dropped at
//! the end.
use cc_db_core::{
    csv_import::{
        CsvImportPlan, CsvImportProgress, CsvInsertMode, CsvMapping, CsvOptions, CsvTransaction,
    },
    transport::{DbCredentials, DirectConnector},
    DbCell, DbError, DbObjectKind, DbObjectRef, DbQueryEvent, DbQueryRequest, DbSession,
};
use cc_models::database::{CqlConsistency, DbConnection, DbEngine, DbOptions, DbTlsMode};
use secrecy::SecretString;
use std::{
    sync::{atomic::Ordering, Arc},
    time::Duration,
};
use tokio_util::sync::CancellationToken;

fn target(variable: &str) -> Option<(DbConnection, DbCredentials)> {
    let spec = std::env::var(variable).ok()?;
    let parts: Vec<&str> = spec.splitn(4, ':').collect();
    let mut connection = DbConnection::new("live", parts[0], DbEngine::Cassandra);
    connection.port = parts[1].parse().unwrap();
    connection.username = parts.get(2).copied().unwrap_or_default().into();
    connection.production = false;
    connection.tls.mode = DbTlsMode::Disable;
    connection.options = DbOptions::Cassandra {
        contact_points: Vec::new(),
        local_datacenter: None,
        read_consistency: CqlConsistency::LocalOne,
        write_consistency: CqlConsistency::LocalQuorum,
        page_size: 100,
    };
    connection.validate().unwrap();
    Some((
        connection,
        DbCredentials {
            password: parts.get(3).map(|p| SecretString::from((*p).to_owned())),
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

async fn consume(
    session: &Arc<DbSession>,
    query: &DbQueryRequest,
) -> Result<Vec<Vec<DbCell>>, String> {
    let mut stream = session.execute(query).map_err(|e| format!("{e:?}"))?;
    let mut rows = Vec::new();
    while let Some(event) = stream.events.recv().await {
        match event {
            DbQueryEvent::Rows { rows: batch, .. } => rows.extend(batch),
            DbQueryEvent::Paused { .. } => stream.control().load_more(1000).unwrap(),
            DbQueryEvent::Error { code, .. } => return Err(code),
            DbQueryEvent::Done | DbQueryEvent::Cancelled => break,
            _ => {}
        }
    }
    Ok(rows)
}

async fn write(session: &Arc<DbSession>, text: &str) {
    let review = session.guard().review(&request(text)).expect(text);
    session.guard().approve(&review.ticket).unwrap();
    let mut approved = request(text);
    approved.approval_ticket = Some(review.ticket);
    consume(session, &approved).await.expect(text);
}

fn text(cell: &DbCell) -> String {
    match cell {
        DbCell::Null => "NULL".into(),
        DbCell::Int(v) | DbCell::Text(v) | DbCell::Decimal(v) => v.clone(),
        other => format!("{other:?}"),
    }
}

async fn rows(session: &Arc<DbSession>) -> Vec<String> {
    let mut out: Vec<String> = consume(
        session,
        &request("SELECT id, name, qty FROM cc_csv_live.items"),
    )
    .await
    .unwrap()
    .iter()
    .map(|row| row.iter().map(text).collect::<Vec<_>>().join("|"))
    .collect();
    out.sort();
    out
}

async fn import(
    session: &Arc<DbSession>,
    path: &std::path::Path,
    plan: &CsvImportPlan,
) -> (Result<(), DbError>, Arc<CsvImportProgress>) {
    let mut template = session.import_template(plan).await.expect("template");
    let review = session.guard().review(&template).expect("review");
    session.guard().approve(&review.ticket).expect("approve");
    template.approval_ticket = Some(review.ticket);
    let progress = Arc::new(CsvImportProgress::default());
    let result = session
        .import_csv(
            path,
            plan,
            &template,
            progress.clone(),
            CancellationToken::new(),
            None,
        )
        .await;
    (result, progress)
}

async fn scenario(variable: &str) {
    let Some((connection, credentials)) = target(variable) else {
        eprintln!("{variable} not set; skipped");
        return;
    };
    let session = Arc::new(
        tokio::time::timeout(
            Duration::from_secs(60),
            DbSession::open(
                "live".into(),
                &connection,
                &credentials,
                Arc::new(DirectConnector),
                false,
            ),
        )
        .await
        .expect("open finishes")
        .expect("open"),
    );
    write(
        &session,
        "CREATE KEYSPACE IF NOT EXISTS cc_csv_live WITH replication = {'class': 'SimpleStrategy', 'replication_factor': 1}",
    )
    .await;
    write(
        &session,
        "CREATE TABLE IF NOT EXISTS cc_csv_live.items (id int PRIMARY KEY, name text, qty bigint)",
    )
    .await;
    write(
        &session,
        "INSERT INTO cc_csv_live.items (id, name, qty) VALUES (1, 'old', 10)",
    )
    .await;
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("items.csv");
    std::fs::write(
        &file,
        "id,name,qty\n1,changed,11\n2,two,2\n3,\"three, quoted\",\nx,bad,1\n",
    )
    .unwrap();
    let plan = |mode| CsvImportPlan {
        table: DbObjectRef {
            database: "cc_csv_live".into(),
            schema: None,
            kind: DbObjectKind::Table,
            name: "items".into(),
            parent_name: None,
        },
        options: CsvOptions {
            delimiter: Some(','),
            ..CsvOptions::default()
        },
        mapping: ["id", "name", "qty"]
            .iter()
            .enumerate()
            .map(|(source, target)| CsvMapping {
                source,
                target: target.to_string(),
            })
            .collect(),
        mode,
        batch_rows: 500,
        skip_bad_rows: true,
        transaction: CsvTransaction::PerBatch,
        truncate: false,
    };

    // Insert never replaces an existing key (CQL INSERT would); the row
    // that does not convert is reported.
    let (result, progress) = import(&session, &file, &plan(CsvInsertMode::Insert)).await;
    result.unwrap();
    assert_eq!(progress.imported.load(Ordering::Acquire), 2);
    assert_eq!(progress.skipped.load(Ordering::Acquire), 1, "id 1 exists");
    assert_eq!(
        progress.failed.load(Ordering::Acquire),
        1,
        "x is not an int"
    );
    assert_eq!(
        rows(&session).await,
        ["1|old|10", "2|two|2", "3|three, quoted|NULL"]
    );

    // Upsert replaces it.
    let (result, _) = import(&session, &file, &plan(CsvInsertMode::Upsert)).await;
    result.unwrap();
    assert_eq!(rows(&session).await[0], "1|changed|11");

    // No whole-file transaction in CQL.
    let mut whole = plan(CsvInsertMode::Upsert);
    whole.transaction = CsvTransaction::Whole;
    let (result, _) = import(&session, &file, &whole).await;
    assert!(matches!(result, Err(DbError::InvalidRequest)), "{result:?}");

    // Clearing the table first uses TRUNCATE.
    let mut clear = plan(CsvInsertMode::Insert);
    clear.truncate = true;
    let template = session.import_template(&clear).await.unwrap();
    assert!(
        template.text.starts_with("TRUNCATE cc_csv_live.items;"),
        "{}",
        template.text
    );
    let (result, _) = import(&session, &file, &clear).await;
    result.unwrap();
    assert_eq!(
        rows(&session).await,
        ["1|changed|11", "2|two|2", "3|three, quoted|NULL"]
    );

    // Counters change only through UPDATE: no import into them.
    write(
        &session,
        "CREATE TABLE IF NOT EXISTS cc_csv_live.hits (id int PRIMARY KEY, n counter)",
    )
    .await;
    let hits = DbObjectRef {
        name: "hits".into(),
        ..clear.table.clone()
    };
    let columns = session.edit_columns(&hits).await.unwrap();
    let n = columns.iter().find(|c| c.name == "n").unwrap();
    assert!(n.generated, "{columns:?}");

    write(&session, "DROP KEYSPACE cc_csv_live").await;
}

#[tokio::test]
async fn csv_import_into_scylla() {
    scenario("CC_DB_LIVE_SCYLLA").await;
}

#[tokio::test]
async fn csv_import_into_cassandra() {
    scenario("CC_DB_LIVE_CASSANDRA").await;
}
