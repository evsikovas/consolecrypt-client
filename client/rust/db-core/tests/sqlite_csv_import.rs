//! CSV import into SQLite (FEAT-09): a file on this computer (one
//! transaction, savepoints for refused rows) and `sqlite3` on a host (every
//! statement commits on its own). Uses only temporary files.
use async_trait::async_trait;
use cc_db_core::{
    csv_import::{
        CsvImportPlan, CsvImportProgress, CsvInsertMode, CsvMapping, CsvOptions, CsvTransaction,
    },
    sqlite_cli::{CliOutput, SqliteCliRunner},
    DbCell, DbError, DbObjectKind, DbObjectRef, DbQueryEvent, DbQueryRequest, DbSession,
    SqliteTarget,
};
use cc_models::database::{DbConnection, DbEngine, DbOptions};
use std::{
    io::Write,
    path::Path,
    sync::{atomic::Ordering, Arc},
};
use tokio_util::sync::CancellationToken;

fn database(dir: &Path) -> std::path::PathBuf {
    let path = dir.join("shop.db");
    rusqlite::Connection::open(&path)
        .unwrap()
        .execute_batch(
            "CREATE TABLE customers (id INTEGER PRIMARY KEY, email TEXT NOT NULL UNIQUE, note TEXT);
             INSERT INTO customers VALUES (1, 'old@example.test', 'old');
             CREATE TABLE loose (a, b);",
        )
        .unwrap();
    path
}

fn connection(path: &Path) -> DbConnection {
    let mut connection = DbConnection::new("shop", "localhost", DbEngine::Sqlite);
    connection.database = path.to_string_lossy().into_owned();
    connection.options = DbOptions::Sqlite { read_only: false };
    connection.production = false;
    connection.validate().unwrap();
    connection
}

fn table(name: &str) -> DbObjectRef {
    DbObjectRef {
        database: "main".into(),
        schema: None,
        kind: DbObjectKind::Table,
        name: name.into(),
        parent_name: None,
    }
}

#[derive(Debug)]
struct LocalProgram;

#[async_trait]
impl SqliteCliRunner for LocalProgram {
    async fn run(
        &self,
        argv: Vec<String>,
        stdin: Vec<u8>,
        stdout_limit: usize,
        _cancel: CancellationToken,
    ) -> Result<CliOutput, DbError> {
        tokio::task::spawn_blocking(move || {
            let mut child = std::process::Command::new(&argv[0])
                .args(&argv[1..])
                .stdin(std::process::Stdio::piped())
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::piped())
                .spawn()
                .map_err(|_| DbError::SqliteCliMissing)?;
            child.stdin.take().unwrap().write_all(&stdin).unwrap();
            let output = child.wait_with_output().map_err(|_| DbError::QueryFailed)?;
            let truncated = output.stdout.len() > stdout_limit;
            let mut stdout = output.stdout;
            stdout.truncate(stdout_limit);
            Ok(CliOutput {
                stdout,
                stderr: output.stderr,
                status: output.status.code(),
                truncated,
            })
        })
        .await
        .unwrap()
    }
}

fn program_available() -> bool {
    std::process::Command::new("sqlite3")
        .arg("-version")
        .output()
        .is_ok_and(|output| output.status.success())
}

fn csv(dir: &Path, name: &str, text: &str) -> std::path::PathBuf {
    let path = dir.join(name);
    std::fs::write(&path, text).unwrap();
    path
}

fn plan(mode: CsvInsertMode, transaction: CsvTransaction) -> CsvImportPlan {
    CsvImportPlan {
        table: table("customers"),
        options: CsvOptions {
            delimiter: Some(','),
            ..CsvOptions::default()
        },
        mapping: ["id", "email", "note"]
            .iter()
            .enumerate()
            .map(|(source, target)| CsvMapping {
                source,
                target: target.to_string(),
            })
            .collect(),
        mode,
        batch_rows: 2,
        skip_bad_rows: true,
        transaction,
        truncate: false,
    }
}

async fn import(
    session: &Arc<DbSession>,
    path: &Path,
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

async fn rows(session: &Arc<DbSession>) -> Vec<String> {
    let request = DbQueryRequest {
        text: "SELECT id, email, note FROM customers ORDER BY id".into(),
        parameters: Vec::new(),
        approval_ticket: None,
    };
    let mut stream = session.execute(&request).unwrap();
    let mut out = Vec::new();
    while let Some(event) = stream.events.recv().await {
        match event {
            DbQueryEvent::Rows { rows, .. } => out.extend(rows.into_iter().map(|row| {
                row.iter()
                    .map(|cell| match cell {
                        DbCell::Null => "NULL".to_string(),
                        DbCell::Int(v) | DbCell::Text(v) => v.clone(),
                        other => format!("{other:?}"),
                    })
                    .collect::<Vec<_>>()
                    .join("|")
            })),
            DbQueryEvent::Done | DbQueryEvent::Cancelled => break,
            DbQueryEvent::Error { code, .. } => panic!("{code}"),
            _ => {}
        }
    }
    out
}

fn counts(progress: &CsvImportProgress) -> (u64, u64) {
    (
        progress.imported.load(Ordering::Acquire),
        progress.skipped.load(Ordering::Acquire),
    )
}

async fn exercise(target: impl Fn() -> SqliteTarget, transactional: bool) {
    let dir = tempfile::tempdir().unwrap();
    let path = database(dir.path());
    let session = Arc::new(
        DbSession::open_sqlite("csv".into(), &connection(&path), target(), false)
            .await
            .unwrap(),
    );
    let tx = if transactional {
        CsvTransaction::Whole
    } else {
        CsvTransaction::PerBatch
    };

    // Plain insert: a duplicate email is refused row by row and reported.
    let file = csv(
        dir.path(),
        "a.csv",
        "id,email,note\n2,ann@example.test,\"it's, quoted\"\n3,old@example.test,dup\n4,bob@example.test,\n",
    );
    let (result, progress) = import(&session, &file, &plan(CsvInsertMode::Insert, tx)).await;
    result.unwrap();
    assert_eq!(
        progress.failed.load(Ordering::Acquire),
        1,
        "the duplicate is refused"
    );
    assert_eq!(
        rows(&session).await,
        [
            "1|old@example.test|old",
            "2|ann@example.test|it's, quoted",
            "4|bob@example.test|NULL"
        ]
    );

    // Skip existing keeps row 1; upsert updates it.
    let file = csv(
        dir.path(),
        "b.csv",
        "id,email,note\n1,changed@example.test,new\n5,eve@example.test,e\n",
    );
    let (result, progress) = import(&session, &file, &plan(CsvInsertMode::SkipExisting, tx)).await;
    result.unwrap();
    assert_eq!(counts(&progress), (1, 1));
    assert_eq!(rows(&session).await[0], "1|old@example.test|old");
    let (result, _) = import(&session, &file, &plan(CsvInsertMode::Upsert, tx)).await;
    result.unwrap();
    assert_eq!(rows(&session).await[0], "1|changed@example.test|new");

    // A rowid table without a declared key has no conflict target.
    let mut loose = plan(CsvInsertMode::SkipExisting, CsvTransaction::PerBatch);
    loose.table = table("loose");
    loose.mapping.truncate(2);
    loose.mapping[0].target = "a".into();
    loose.mapping[1].target = "b".into();
    assert!(session.import_template(&loose).await.is_err());
}

#[tokio::test]
async fn csv_import_into_a_local_sqlite_file() {
    exercise(|| SqliteTarget::Local, true).await;

    // Whole-file transaction without skipping: one bad row rolls back all.
    let dir = tempfile::tempdir().unwrap();
    let path = database(dir.path());
    let session = Arc::new(
        DbSession::open_sqlite("csv".into(), &connection(&path), SqliteTarget::Local, false)
            .await
            .unwrap(),
    );
    let file = csv(
        dir.path(),
        "c.csv",
        "id,email,note\n7,x@example.test,\n8,old@example.test,dup\n",
    );
    let mut all = plan(CsvInsertMode::Insert, CsvTransaction::Whole);
    all.skip_bad_rows = false;
    all.batch_rows = 1;
    let (result, _) = import(&session, &file, &all).await;
    assert!(result.is_err());
    assert_eq!(
        rows(&session).await,
        ["1|old@example.test|old"],
        "nothing kept"
    );
    // The connection stays usable: an approved ordinary write still works.
    let mut write = DbQueryRequest {
        text: "INSERT INTO customers (id, email) VALUES (9, 'z@example.test')".into(),
        parameters: Vec::new(),
        approval_ticket: None,
    };
    let review = session.guard().review(&write).unwrap();
    session.guard().approve(&review.ticket).unwrap();
    write.approval_ticket = Some(review.ticket);
    let mut stream = session.execute(&write).unwrap();
    while let Some(event) = stream.events.recv().await {
        match event {
            DbQueryEvent::Error { code, .. } => panic!("{code}"),
            DbQueryEvent::Done | DbQueryEvent::Cancelled => break,
            _ => {}
        }
    }
    assert_eq!(rows(&session).await.len(), 2);
}

#[tokio::test]
async fn csv_import_through_sqlite3_on_a_host() {
    if !program_available() {
        eprintln!("sqlite3 not installed; skipped");
        return;
    }
    exercise(|| SqliteTarget::Cli(Arc::new(LocalProgram)), false).await;
    // `sqlite3` commits each statement: no whole-file transaction.
    let dir = tempfile::tempdir().unwrap();
    let path = database(dir.path());
    let session = Arc::new(
        DbSession::open_sqlite(
            "csv".into(),
            &connection(&path),
            SqliteTarget::Cli(Arc::new(LocalProgram)),
            false,
        )
        .await
        .unwrap(),
    );
    let file = csv(dir.path(), "d.csv", "id,email,note\n9,y@example.test,\n");
    let (result, _) = import(
        &session,
        &file,
        &plan(CsvInsertMode::Insert, CsvTransaction::Whole),
    )
    .await;
    assert!(matches!(result, Err(DbError::InvalidRequest)), "{result:?}");
}
