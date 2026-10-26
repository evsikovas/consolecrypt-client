//! Opt-in CSV import into ClickHouse (LIM-05):
//! CC_DB_LIVE_CLICKHOUSE = host:port:user:password:database (the HTTP
//! port), as in `designer_live.rs`. Every run uses its own tables and drops
//! them.
use cc_db_core::{
    csv_import::{
        create_table_sql, CsvImportPlan, CsvImportProgress, CsvInsertMode, CsvMapping, CsvOptions,
        CsvTransaction,
    },
    transport::{DbCredentials, DirectConnector},
    DbCell, DbError, DbObjectKind, DbObjectRef, DbQueryEvent, DbQueryRequest, DbSession,
};
use cc_models::database::{DbConnection, DbEngine, DbTlsMode};
use secrecy::SecretString;
use std::sync::{atomic::Ordering, Arc};
use tokio_util::sync::CancellationToken;

fn request(text: &str) -> DbQueryRequest {
    DbQueryRequest {
        text: text.into(),
        parameters: Vec::new(),
        approval_ticket: None,
    }
}

async fn consume(session: &Arc<DbSession>, query: &DbQueryRequest) -> Vec<Vec<DbCell>> {
    let mut stream = session.execute(query).expect("execute");
    let mut rows = Vec::new();
    while let Some(event) = stream.events.recv().await {
        match event {
            DbQueryEvent::Rows { rows: batch, .. } => rows.extend(batch),
            DbQueryEvent::Paused { .. } => stream.control().load_more(1000).unwrap(),
            DbQueryEvent::Error { code, message } => panic!("{code}: {message}"),
            DbQueryEvent::Done | DbQueryEvent::Cancelled => break,
            _ => {}
        }
    }
    rows
}

async fn write(session: &Arc<DbSession>, text: &str) {
    let review = session.guard().review(&request(text)).expect(text);
    session.guard().approve(&review.ticket).unwrap();
    let mut approved = request(text);
    approved.approval_ticket = Some(review.ticket);
    consume(session, &approved).await;
}

fn text(cell: &DbCell) -> String {
    match cell {
        DbCell::Null => "NULL".into(),
        DbCell::Bool(v) => v.to_string(),
        DbCell::Int(v)
        | DbCell::Uint(v)
        | DbCell::Text(v)
        | DbCell::Decimal(v)
        | DbCell::Date(v)
        | DbCell::Timestamp(v) => v.clone(),
        other => format!("{other:?}"),
    }
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

#[tokio::test]
async fn csv_import_into_clickhouse() {
    let Ok(spec) = std::env::var("CC_DB_LIVE_CLICKHOUSE") else {
        eprintln!("CC_DB_LIVE_CLICKHOUSE not set; skipped");
        return;
    };
    let parts: Vec<&str> = spec.splitn(5, ':').collect();
    let database = parts[4].to_owned();
    let mut connection = DbConnection::new("live", parts[0], DbEngine::Clickhouse);
    connection.port = parts[1].parse().unwrap();
    connection.username = parts[2].into();
    connection.database = database.clone();
    connection.tls.mode = DbTlsMode::Disable;
    connection.production = false;
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
    let suffix = uuid::Uuid::new_v4().simple().to_string();
    let items = format!("cc_csv_items_{}", &suffix[..8]);
    write(
        &session,
        &format!(
            "CREATE TABLE `{database}`.`{items}` (id UInt32, name Nullable(String), \
             qty Nullable(Int64), price Decimal(10, 2), at DateTime, day Date, flag Bool, \
             note String DEFAULT 'none', size UInt64 MATERIALIZED length(note)) \
             ENGINE = MergeTree ORDER BY id"
        ),
    )
    .await;
    let table = DbObjectRef {
        database: database.clone(),
        schema: None,
        kind: DbObjectKind::Table,
        name: items.clone(),
        parent_name: None,
    };
    let columns = session.edit_columns(&table).await.unwrap();
    let column = |name: &str| columns.iter().find(|c| c.name == name).unwrap().clone();
    assert!(column("name").nullable && !column("id").nullable);
    assert!(column("note").has_default && !column("note").generated);
    assert!(column("size").generated, "MATERIALIZED is never written");

    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("items.csv");
    std::fs::write(
        &file,
        "id,name,qty,price,at,day,flag\n\
         1,one,10,1.50,2024-01-02 03:04:05,2024-01-02,true\n\
         2,\"two, quoted\",,2.25,2024-02-03T04:05:06,2024-02-03,no\n\
         x,bad,1,1,2024-01-01 00:00:00,2024-01-01,true\n\
         3,three,-5,1234.56,2024-03-04 05:06:07,2024-03-04,1\n",
    )
    .unwrap();
    let names = ["id", "name", "qty", "price", "at", "day", "flag"];
    let plan = |mode| CsvImportPlan {
        table: table.clone(),
        options: CsvOptions {
            delimiter: Some(','),
            ..CsvOptions::default()
        },
        mapping: names
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
    let (result, progress) = import(&session, &file, &plan(CsvInsertMode::Insert)).await;
    result.unwrap();
    assert_eq!(progress.imported.load(Ordering::Acquire), 3);
    assert_eq!(
        progress.failed.load(Ordering::Acquire),
        1,
        "x is not a UInt32"
    );
    let select = format!(
        "SELECT id, name, qty, price, toString(at), toString(day), flag, note \
         FROM `{database}`.`{items}` ORDER BY id"
    );
    let read = |rows: Vec<Vec<DbCell>>| -> Vec<String> {
        rows.iter()
            .map(|row| row.iter().map(text).collect::<Vec<_>>().join("|"))
            .collect()
    };
    assert_eq!(
        read(consume(&session, &request(&select)).await),
        [
            "1|one|10|1.5|2024-01-02 03:04:05|2024-01-02|true|none",
            "2|two, quoted|NULL|2.25|2024-02-03 04:05:06|2024-02-03|false|none",
            "3|three|-5|1234.56|2024-03-04 05:06:07|2024-03-04|true|none",
        ]
    );

    // MergeTree keys are not unique: no skip-existing or upsert, no
    // whole-file transaction.
    for mode in [CsvInsertMode::SkipExisting, CsvInsertMode::Upsert] {
        assert!(session.import_template(&plan(mode)).await.is_err());
    }
    let mut whole = plan(CsvInsertMode::Insert);
    whole.transaction = CsvTransaction::Whole;
    let (result, _) = import(&session, &file, &whole).await;
    assert!(matches!(result, Err(DbError::InvalidRequest)), "{result:?}");

    // Clearing first is TRUNCATE TABLE.
    let mut clear = plan(CsvInsertMode::Insert);
    clear.truncate = true;
    let template = session.import_template(&clear).await.unwrap();
    assert!(
        template.text.starts_with("TRUNCATE TABLE "),
        "{}",
        template.text
    );
    let (result, _) = import(&session, &file, &clear).await;
    result.unwrap();
    assert_eq!(consume(&session, &request(&select)).await.len(), 3);

    // A table made from the file takes the same rows.
    let created = format!("cc_csv_new_{}", &suffix[..8]);
    let new_table = DbObjectRef {
        name: created.clone(),
        ..table.clone()
    };
    let ddl = create_table_sql(
        &file,
        &plan(CsvInsertMode::Insert).options,
        DbEngine::Clickhouse,
        &new_table,
    )
    .unwrap();
    assert!(ddl.contains("ENGINE = MergeTree"), "{ddl}");
    write(&session, &ddl).await;
    let mut fresh = plan(CsvInsertMode::Insert);
    fresh.table = new_table;
    let (result, progress) = import(&session, &file, &fresh).await;
    result.unwrap();
    // The bad row is text in a String column here.
    assert_eq!(progress.imported.load(Ordering::Acquire), 4);

    for name in [&items, &created] {
        write(&session, &format!("DROP TABLE `{database}`.`{name}`")).await;
    }
}
