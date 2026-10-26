//! Opt-in CSV import into MongoDB (LIM-05): CC_DB_LIVE_MONGO =
//! host:port:user:password:database (the user lives in `admin`), as in
//! `designer_live.rs`. Every run uses its own collection and drops it.
use cc_db_core::{
    csv_import::{
        CsvImportPlan, CsvImportProgress, CsvInsertMode, CsvMapping, CsvOptions, CsvTransaction,
    },
    transport::{DbCredentials, DirectConnector},
    DbCell, DbError, DbObjectKind, DbObjectRef, DbQueryEvent, DbQueryRequest, DbSession,
};
use cc_models::database::{DbConnection, DbEngine, DbOptions, DbTlsMode, MongoReadPreference};
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
        DbCell::Json(v) => v.clone(),
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

async fn documents(session: &Arc<DbSession>, name: &str) -> Vec<String> {
    let rows = consume(
        session,
        &request(&format!(
            r#"{{"aggregate":"{name}","pipeline":[{{"$sort":{{"_id":1}}}},{{"$project":{{"_id":0,"doc":{{"$concat":[{{"$toString":"$_id"}},"|",{{"$ifNull":["$name",""]}},"|",{{"$type":"$qty"}},"|",{{"$type":"$seen"}},"|",{{"$type":"$price"}},"|",{{"$type":"$note"}}]}}}}}}],"cursor":{{}}}}"#
        )),
    )
    .await;
    rows.iter().map(|row| text(&row[0])).collect::<Vec<_>>()
}

#[tokio::test]
async fn csv_import_into_mongodb() {
    let Ok(spec) = std::env::var("CC_DB_LIVE_MONGO") else {
        eprintln!("CC_DB_LIVE_MONGO not set; skipped");
        return;
    };
    let parts: Vec<&str> = spec.splitn(5, ':').collect();
    let database = parts[4].to_owned();
    let mut connection = DbConnection::new("live", parts[0], DbEngine::Mongodb);
    connection.port = parts[1].parse().unwrap();
    connection.username = parts[2].into();
    connection.database = database.clone();
    connection.tls.mode = DbTlsMode::Disable;
    connection.production = false;
    connection.options = DbOptions::Mongodb {
        auth_source: Some("admin".into()),
        replica_set: None,
        srv: false,
        read_preference: MongoReadPreference::Primary,
    };
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
    let name = format!("cc_csv_{}", &uuid::Uuid::new_v4().simple().to_string()[..8]);
    write(
        &session,
        &format!(
            r#"{{"insert":"{name}","documents":[{{"_id":{{"$oid":"0123456789abcdef01234567"}},"name":"old","qty":{{"$numberLong":"1"}},"seen":{{"$date":"2024-01-01T00:00:00Z"}}}}]}}"#
        ),
    )
    .await;
    let collection = DbObjectRef {
        database: database.clone(),
        schema: None,
        kind: DbObjectKind::Collection,
        name: name.clone(),
        parent_name: None,
    };
    let columns = session.edit_columns(&collection).await.unwrap();
    let column = |field: &str| columns.iter().find(|c| c.name == field).unwrap().clone();
    assert_eq!(column("_id").type_name, "objectId");
    assert!(column("_id").key);
    assert_eq!(column("qty").type_name, "long");
    assert_eq!(column("seen").type_name, "datetime");

    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("items.csv");
    std::fs::write(
        &file,
        "_id,name,qty,seen,price,note\n\
         0123456789abcdef01234567,changed,5,2024-02-03 04:05:06,1.5,first\n\
         0123456789abcdef01234568,new,x,2024-02-04,2,\n\
         0123456789abcdef01234569,three,7,2024-02-05T01:02:03+03:00,2.5,note\n",
    )
    .unwrap();
    // `price` and `note` are new fields: each value as it reads.
    let fields = ["_id", "name", "qty", "seen", "price", "note"];
    let plan = |mode| CsvImportPlan {
        table: collection.clone(),
        options: CsvOptions {
            delimiter: Some(','),
            ..CsvOptions::default()
        },
        mapping: fields
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

    // Skip existing: the known _id is skipped, `x` is not a long.
    let (result, progress) = import(&session, &file, &plan(CsvInsertMode::SkipExisting)).await;
    result.unwrap();
    assert_eq!(progress.imported.load(Ordering::Acquire), 1);
    assert_eq!(progress.skipped.load(Ordering::Acquire), 1);
    assert_eq!(progress.failed.load(Ordering::Acquire), 1);
    assert_eq!(
        documents(&session, &name).await,
        [
            "0123456789abcdef01234567|old|long|date|missing|missing",
            "0123456789abcdef01234569|three|long|date|double|string",
        ]
    );

    // Upsert changes the known document by its ObjectId.
    let (result, progress) = import(&session, &file, &plan(CsvInsertMode::Upsert)).await;
    result.unwrap();
    assert_eq!(progress.imported.load(Ordering::Acquire), 2);
    assert_eq!(
        documents(&session, &name).await[0],
        "0123456789abcdef01234567|changed|long|date|double|string"
    );

    // Insert refuses a duplicate _id as a bad row.
    let (result, progress) = import(&session, &file, &plan(CsvInsertMode::Insert)).await;
    result.unwrap();
    assert_eq!(progress.imported.load(Ordering::Acquire), 0);
    assert_eq!(progress.failed.load(Ordering::Acquire), 3);

    // No whole-file transaction; clearing first deletes every document.
    let mut whole = plan(CsvInsertMode::Insert);
    whole.transaction = CsvTransaction::Whole;
    let (result, _) = import(&session, &file, &whole).await;
    assert!(matches!(result, Err(DbError::InvalidRequest)), "{result:?}");
    let mut clear = plan(CsvInsertMode::Insert);
    clear.truncate = true;
    let (result, progress) = import(&session, &file, &clear).await;
    result.unwrap();
    assert_eq!(progress.imported.load(Ordering::Acquire), 2);
    assert_eq!(documents(&session, &name).await.len(), 2);

    // Field names cannot be operators or paths.
    let mut bad = plan(CsvInsertMode::Insert);
    bad.mapping[1].target = "$where".into();
    assert!(session.import_template(&bad).await.is_err());
    bad.mapping[1].target = "a.b".into();
    assert!(session.import_template(&bad).await.is_err());

    write(&session, &format!(r#"{{"drop":"{name}"}}"#)).await;
}
