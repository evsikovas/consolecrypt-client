//! Opt-in acceptance of CSV import against disposable local databases:
//! CC_DB_LIVE_PG and CC_DB_LIVE_MYSQL, either
//! `scheme://user:password@host:port/database` or
//! `host:port:user:password:database`. Every run uses its own table and
//! file and drops them.
use cc_db_core::{
    csv_import::{
        CsvDateOrder, CsvImportPlan, CsvImportProgress, CsvInsertMode, CsvMapping, CsvOptions,
        CsvTransaction,
    },
    transport::{DbCredentials, DirectConnector},
    DbCell, DbError, DbObjectRef, DbQueryEvent, DbQueryRequest, DbSession,
};
use cc_models::database::{DbConnection, DbEngine, DbTlsMode};
use secrecy::SecretString;
use std::{
    io::Write,
    sync::{atomic::Ordering, Arc},
};
use tokio_util::sync::CancellationToken;

fn target(variable: &str, engine: DbEngine) -> Option<(DbConnection, DbCredentials)> {
    let spec = std::env::var(variable).ok()?;
    let (host, port, user, password, database) = match spec.split_once("://") {
        Some((_, rest)) => {
            let (auth, location) = rest.rsplit_once('@')?;
            let (user, password) = auth.split_once(':')?;
            let (address, database) = location.split_once('/')?;
            let (host, port) = address.rsplit_once(':')?;
            (
                host.to_owned(),
                port.to_owned(),
                user.to_owned(),
                password.to_owned(),
                database.to_owned(),
            )
        }
        None => {
            let parts: Vec<&str> = spec.splitn(5, ':').collect();
            (
                parts[0].into(),
                parts[1].into(),
                parts[2].into(),
                parts[3].into(),
                parts[4].into(),
            )
        }
    };
    let mut connection = DbConnection::new("live-csv", &host, engine);
    connection.port = port.parse().ok()?;
    connection.username = user;
    connection.database = database;
    connection.production = false;
    connection.tls.mode = DbTlsMode::Disable;
    Some((
        connection,
        DbCredentials {
            password: Some(SecretString::from(password)),
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

/// Rows of a query plus the editable table its first result reports.
async fn read(
    session: &Arc<DbSession>,
    sql: &str,
    approve: bool,
) -> Result<(Vec<Vec<String>>, Option<DbObjectRef>, Vec<String>), String> {
    let mut query = request(sql);
    if approve {
        let review = session
            .guard()
            .review(&query)
            .map_err(|e| format!("{e:?}"))?;
        session
            .guard()
            .approve(&review.ticket)
            .map_err(|e| format!("{e:?}"))?;
        query.approval_ticket = Some(review.ticket);
    }
    let mut stream = session.execute(&query).map_err(|e| format!("{e:?}"))?;
    let (mut rows, mut table, mut keys) = (Vec::new(), None, Vec::new());
    while let Some(event) = stream.events.recv().await {
        match event {
            DbQueryEvent::Started {
                columns,
                editable_table,
                result_index: 0,
            } => {
                table = editable_table;
                keys = columns
                    .iter()
                    .filter(|c| c.primary_key)
                    .map(|c| c.name.clone())
                    .collect();
            }
            DbQueryEvent::Rows { rows: batch, .. } => {
                rows.extend(batch.iter().map(|row| row.iter().map(text).collect()))
            }
            DbQueryEvent::Error { code, .. } => return Err(code),
            DbQueryEvent::Done | DbQueryEvent::Cancelled => break,
            _ => {}
        }
    }
    Ok((rows, table, keys))
}

async fn write(session: &Arc<DbSession>, sql: &str) {
    read(session, sql, true).await.expect(sql);
}

/// Review → approve → run, as the app does after the confirmation.
async fn import(
    session: &Arc<DbSession>,
    path: &std::path::Path,
    plan: &CsvImportPlan,
    cancel: CancellationToken,
) -> (Result<(), DbError>, Arc<CsvImportProgress>) {
    let mut template = session.import_template(plan).await.expect("template");
    let review = session.guard().review(&template).expect("review");
    session.guard().approve(&review.ticket).expect("approve");
    template.approval_ticket = Some(review.ticket);
    let progress = Arc::new(CsvImportProgress::default());
    let mut report = path.as_os_str().to_owned();
    report.push(".errors.csv");
    let result = session
        .import_csv(
            path,
            plan,
            &template,
            progress.clone(),
            cancel,
            Some(report.into()),
        )
        .await;
    (result, progress)
}

fn csv(text: &str) -> tempfile::NamedTempFile {
    let mut file = tempfile::NamedTempFile::new().unwrap();
    file.write_all(text.as_bytes()).unwrap();
    file
}

fn n(progress: &CsvImportProgress) -> (u64, u64, u64) {
    (
        progress.imported.load(Ordering::Acquire),
        progress.skipped.load(Ordering::Acquire),
        progress.failed.load(Ordering::Acquire),
    )
}

async fn exercise(variable: &str, engine: DbEngine) {
    let Some((connection, credentials)) = target(variable, engine) else {
        eprintln!("{variable} not set; skipped");
        return;
    };
    let session = Arc::new(
        DbSession::open(
            "csv".into(),
            &connection,
            &credentials,
            Arc::new(DirectConnector),
            false,
        )
        .await
        .unwrap(),
    );
    let name = format!("cc_csv_{:08x}", rand_suffix());
    write(
        &session,
        &format!(
            "CREATE TABLE {name} (id int PRIMARY KEY, name varchar(40) NOT NULL, \
             amount numeric(10,2), born date, note text)"
        ),
    )
    .await;
    let outcome = futures::FutureExt::catch_unwind(std::panic::AssertUnwindSafe(async {
        let (_, table, _) = read(&session, &format!("SELECT * FROM {name}"), false)
            .await
            .unwrap();
        let table: DbObjectRef = table.expect("editable");
        let mapping = |names: &[&str]| -> Vec<CsvMapping> {
            names
                .iter()
                .enumerate()
                .map(|(source, target)| CsvMapping {
                    source,
                    target: target.to_string(),
                })
                .collect()
        };
        let options = CsvOptions {
            decimal_comma: true,
            date_order: CsvDateOrder::Dmy,
            ..CsvOptions::default()
        };
        let mut plan = CsvImportPlan {
            table: table.clone(),
            options: options.clone(),
            mapping: mapping(&["id", "name", "amount", "born", "note"]),
            mode: CsvInsertMode::Insert,
            batch_rows: 2,
            skip_bad_rows: true,
            transaction: CsvTransaction::Whole,
            truncate: false,
        };
        let order = format!("SELECT id, name, amount, born, note FROM {name} ORDER BY id");

        // Semicolons, decimal comma, day-first dates, quotes, a multiline
        // field, an empty NULL and one row that does not convert.
        let file = csv("id;name;amount;born;note\n\
             1;\"Ann \"\"A\"\"\";1 234,50;22.10.1990;\"two\nlines\"\n\
             2;Bob;abc;01.01.2000;x\n\
             3;=SUM(A1);;31.12.1999;\n\
             4;Dan;7,5;29.02.2000;ok\n");
        let (result, progress) =
            import(&session, file.path(), &plan, CancellationToken::new()).await;
        result.unwrap();
        assert_eq!(n(&progress), (3, 0, 1));
        let report =
            std::fs::read_to_string(format!("{}.errors.csv", file.path().display())).unwrap();
        assert!(report.contains("4,type,2,Bob,abc"), "{report}");
        let (rows, ..) = read(&session, &order, false).await.unwrap();
        assert_eq!(rows.len(), 3);
        assert_eq!(rows[0][1], "Ann \"A\"");
        assert_eq!(rows[0][2], "1234.50");
        assert_eq!(rows[0][3], "1990-10-22");
        assert_eq!(rows[0][4], "two\nlines");
        assert_eq!(rows[1][1], "=SUM(A1)", "formulas are imported as they are");
        assert_eq!((rows[1][2].as_str(), rows[1][4].as_str()), ("NULL", "NULL"));

        // Upsert updates existing keys; skip-existing leaves them.
        let changes = csv("id;name\n1;Anna\n5;Eve\n");
        plan.mapping = mapping(&["id", "name"]);
        plan.mode = CsvInsertMode::SkipExisting;
        let (result, progress) =
            import(&session, changes.path(), &plan, CancellationToken::new()).await;
        result.unwrap();
        assert_eq!(n(&progress), (1, 1, 0));
        plan.mode = CsvInsertMode::Upsert;
        let (result, _) = import(&session, changes.path(), &plan, CancellationToken::new()).await;
        result.unwrap();
        let (rows, ..) = read(&session, &order, false).await.unwrap();
        assert_eq!(rows.len(), 4);
        assert_eq!(rows[0][1], "Anna");
        assert_eq!(
            rows[0][2], "1234.50",
            "columns not in the file keep their values"
        );

        // A refused row (duplicate key) is reported with per-batch
        // transactions; the other rows of its batch stay.
        let duplicates = csv("id;name\n6;Fay\n1;Again\n7;Gus\n");
        plan.mode = CsvInsertMode::Insert;
        plan.transaction = CsvTransaction::PerBatch;
        let (result, progress) =
            import(&session, duplicates.path(), &plan, CancellationToken::new()).await;
        result.unwrap();
        assert_eq!(n(&progress), (2, 0, 1));
        let (rows, ..) = read(&session, &order, false).await.unwrap();
        assert_eq!(rows.len(), 6);

        // Stop on the first bad row: with the whole-file transaction nothing
        // is kept, not even the emptied table.
        let bad = csv("id;name\n10;Ok\nx;Bad\n");
        plan.transaction = CsvTransaction::Whole;
        plan.skip_bad_rows = false;
        plan.truncate = true;
        let (result, progress) =
            import(&session, bad.path(), &plan, CancellationToken::new()).await;
        assert!(result.is_err());
        assert_eq!(
            progress.error.lock().unwrap().as_deref(),
            Some("row_type:3")
        );
        let (rows, ..) = read(&session, &order, false).await.unwrap();
        assert_eq!(rows.len(), 6, "rolled back");

        // A cancelled import keeps nothing.
        let cancel = CancellationToken::new();
        cancel.cancel();
        plan.truncate = false;
        let (result, _) = import(&session, changes.path(), &plan, cancel).await;
        assert!(matches!(result, Err(DbError::Cancelled)));

        // Truncate then import.
        plan.truncate = true;
        plan.skip_bad_rows = true;
        let (result, progress) =
            import(&session, changes.path(), &plan, CancellationToken::new()).await;
        result.unwrap();
        assert_eq!(n(&progress).0, 2);
        let (rows, ..) = read(&session, &order, false).await.unwrap();
        assert_eq!(rows.len(), 2);
        let _ = std::fs::remove_file(format!("{}.errors.csv", file.path().display()));
        let _ = std::fs::remove_file(format!("{}.errors.csv", duplicates.path().display()));
        let _ = DbCell::Null;
    }))
    .await;
    write(&session, &format!("DROP TABLE {name}")).await;
    session.close();
    if let Err(panic) = outcome {
        std::panic::resume_unwind(panic);
    }
}

fn rand_suffix() -> u32 {
    use std::time::{SystemTime, UNIX_EPOCH};
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .subsec_nanos();
    nanos ^ std::process::id().rotate_left(16)
}

#[tokio::test]
async fn postgres_csv_import() {
    exercise("CC_DB_LIVE_PG", DbEngine::Postgresql).await;
}

#[tokio::test]
async fn mysql_csv_import() {
    exercise("CC_DB_LIVE_MYSQL", DbEngine::Mysql).await;
}
