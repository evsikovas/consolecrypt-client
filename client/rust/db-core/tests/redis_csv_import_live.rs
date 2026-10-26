//! Opt-in CSV import into Redis (LIM-05): CC_DB_LIVE_REDIS =
//! host:port:user:password:db, as in `redis_live.rs`. Every key it writes
//! lives below `cc:csv:<run>:` and is removed at the end.
use cc_db_core::{
    csv_import::{
        CsvImportPlan, CsvImportProgress, CsvInsertMode, CsvMapping, CsvOptions, CsvTransaction,
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

async fn read(session: &Arc<DbSession>, command: &str) -> Vec<String> {
    consume(session, &request(command))
        .await
        .iter()
        .map(|row| row.iter().map(text).collect::<Vec<_>>().join("|"))
        .collect()
}

#[tokio::test]
async fn csv_import_into_redis() {
    let Ok(spec) = std::env::var("CC_DB_LIVE_REDIS") else {
        eprintln!("CC_DB_LIVE_REDIS not set; skipped");
        return;
    };
    let parts: Vec<&str> = spec.splitn(5, ':').collect();
    let mut connection = DbConnection::new("live", parts[0], DbEngine::Redis);
    connection.port = parts[1].parse().unwrap();
    connection.username = parts[2].into();
    connection.database = parts[4].into();
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
    session.guard().allow_session_write().unwrap();
    let run = &uuid::Uuid::new_v4().simple().to_string()[..8];
    let prefix = format!("cc:csv:{run}:");
    // A key folder: its prefix goes in front of every key.
    let folder = DbObjectRef {
        database: parts[4].into(),
        schema: None,
        kind: DbObjectKind::RedisDatabase,
        name: prefix.clone(),
        parent_name: Some(String::new()),
    };
    write(&session, &format!("SET {prefix}a old")).await;
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("keys.csv");
    std::fs::write(
        &file,
        "id,name,qty\na,Alpha,1\nb,\"Beta, quoted\",2\n,nokey,3\nc,Gamma,\n",
    )
    .unwrap();
    let plan = |targets: &[&str], mode| CsvImportPlan {
        table: folder.clone(),
        options: CsvOptions {
            delimiter: Some(','),
            ..CsvOptions::default()
        },
        mapping: targets
            .iter()
            .enumerate()
            .filter(|(_, target)| !target.is_empty())
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

    // Strings: SET … NX skips the existing key; a row without a key fails.
    let strings = plan(&["key", "value", ""], CsvInsertMode::SkipExisting);
    let template = session.import_template(&strings).await.unwrap();
    assert_eq!(template.text, "SET {{k}} {{v}} NX");
    let (result, progress) = import(&session, &file, &strings).await;
    result.unwrap();
    assert_eq!(progress.imported.load(Ordering::Acquire), 2);
    assert_eq!(progress.skipped.load(Ordering::Acquire), 1);
    assert_eq!(progress.failed.load(Ordering::Acquire), 1);
    assert_eq!(read(&session, &format!("GET {prefix}a")).await, ["old"]);
    assert_eq!(
        read(&session, &format!("GET {prefix}b")).await,
        ["Beta, quoted"]
    );

    // Upsert overwrites.
    let (result, _) = import(
        &session,
        &file,
        &plan(&["key", "value", ""], CsvInsertMode::Upsert),
    )
    .await;
    result.unwrap();
    assert_eq!(read(&session, &format!("GET {prefix}a")).await, ["Alpha"]);

    // Hashes: named fields, NULL fields left out; an existing key (here a
    // string) is checked with EXISTS and refused in insert mode.
    let hashes = plan(&["key", "name", "qty"], CsvInsertMode::Insert);
    let template = session.import_template(&hashes).await.unwrap();
    assert_eq!(
        template.text,
        "EXISTS {{k}}\nHSET {{k}} name {{f1}} qty {{f2}}"
    );
    let mut renamed = hashes.clone();
    renamed.table.name = format!("{prefix}h:");
    let (result, progress) = import(&session, &file, &renamed).await;
    result.unwrap();
    assert_eq!(progress.imported.load(Ordering::Acquire), 3);
    // The empty qty is left out: only `name` is set.
    assert_eq!(
        read(&session, &format!("HGETALL {prefix}h:c")).await,
        ["name|Gamma"]
    );
    let (result, progress) = import(&session, &file, &renamed).await;
    result.unwrap();
    assert_eq!(progress.imported.load(Ordering::Acquire), 0);
    assert_eq!(
        progress.failed.load(Ordering::Acquire),
        4,
        "3 exist, 1 has no key"
    );

    // A field name must be one word; `value` and fields do not mix; no
    // FLUSHDB to clear first; no whole-file transaction.
    assert!(session
        .import_template(&plan(&["key", "a b", ""], CsvInsertMode::Insert))
        .await
        .is_err());
    assert!(session
        .import_template(&plan(&["key", "value", "qty"], CsvInsertMode::Insert))
        .await
        .is_err());
    let mut clear = plan(&["key", "value", ""], CsvInsertMode::Insert);
    clear.truncate = true;
    assert!(session.import_template(&clear).await.is_err());
    let mut whole = plan(&["key", "value", ""], CsvInsertMode::Insert);
    whole.transaction = CsvTransaction::Whole;
    let (result, _) = import(&session, &file, &whole).await;
    assert!(matches!(result, Err(DbError::InvalidRequest)), "{result:?}");

    for key in ["a", "b", "c", "h:a", "h:b", "h:c"] {
        write(&session, &format!("DEL {prefix}{key}")).await;
    }
}
