//! Table designer on SQLite (FEAT-08): the structure loaded from a real file,
//! in-place ALTER, a rebuild that keeps rows, indexes and the rows of other
//! tables, a failing rebuild that leaves everything as it was, and a table
//! with triggers that is never rebuilt. A file on this computer and
//! `sqlite3` on a host. Uses only temporary files.
use async_trait::async_trait;
use cc_db_core::{
    designer::{self, CheckDesign, ColumnDesign, DesignPlan, TableDesign},
    sqlite_cli::{CliOutput, SqliteCliRunner},
    DbCell, DbError, DbObjectKind, DbObjectRef, DbQueryEvent, DbQueryRequest, DbSession,
    SqliteTarget,
};
use cc_models::database::{DbConnection, DbEngine, DbOptions};
use std::{io::Write, path::Path, sync::Arc};
use tokio_util::sync::CancellationToken;

fn database(dir: &Path) -> std::path::PathBuf {
    let path = dir.join("shop.db");
    rusqlite::Connection::open(&path)
        .unwrap()
        .execute_batch(
            "CREATE TABLE owners (id INTEGER PRIMARY KEY);
             INSERT INTO owners VALUES (1);
             CREATE TABLE items (
                 id INTEGER PRIMARY KEY AUTOINCREMENT,
                 name TEXT NOT NULL UNIQUE,
                 qty INTEGER DEFAULT 0 CHECK (qty >= 0),
                 owner INTEGER REFERENCES owners (id) ON DELETE CASCADE,
                 at TEXT DEFAULT CURRENT_TIMESTAMP
             );
             CREATE INDEX items_owner ON items (owner);
             INSERT INTO items (name, qty, owner) VALUES ('a', 1, 1), ('b', 2, 1);
             CREATE TABLE notes (item INTEGER REFERENCES items (id) ON DELETE CASCADE, body TEXT);
             INSERT INTO notes VALUES (1, 'kept');
             CREATE TABLE logged (id INTEGER PRIMARY KEY, v TEXT);
             CREATE TRIGGER logged_insert AFTER INSERT ON logged BEGIN SELECT 1; END;",
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

fn text(cell: &DbCell) -> String {
    match cell {
        DbCell::Null => "NULL".into(),
        DbCell::Int(v) | DbCell::Text(v) | DbCell::Decimal(v) | DbCell::Float(v) => v.clone(),
        other => format!("{other:?}"),
    }
}

async fn rows(session: &Arc<DbSession>, sql: &str) -> Vec<String> {
    let request = DbQueryRequest {
        text: sql.into(),
        parameters: Vec::new(),
        approval_ticket: None,
    };
    let mut stream = session.execute(&request).unwrap();
    let mut out = Vec::new();
    while let Some(event) = stream.events.recv().await {
        match event {
            DbQueryEvent::Rows { rows, .. } => out.extend(
                rows.iter()
                    .map(|row| row.iter().map(text).collect::<Vec<_>>().join("|")),
            ),
            DbQueryEvent::Paused { .. } => stream.control().load_more(1000).unwrap(),
            DbQueryEvent::Error { code, .. } => panic!("{sql}: {code}"),
            DbQueryEvent::Done | DbQueryEvent::Cancelled => break,
            _ => {}
        }
    }
    out
}

/// Reviews, approves and applies a plan; true when it committed.
async fn apply(session: &Arc<DbSession>, plan: &DesignPlan) -> bool {
    assert!(!plan.has_errors(), "{:?}", plan.issues);
    let mut request = DbQueryRequest {
        text: plan.script.clone(),
        parameters: Vec::new(),
        approval_ticket: None,
    };
    let review = session.guard().review(&request).unwrap();
    session.guard().approve(&review.ticket).unwrap();
    request.approval_ticket = Some(review.ticket);
    let result = session.apply_design(&request).await.unwrap();
    assert_eq!(result.statements.len(), plan.statements.len());
    result.committed
}

fn codes(plan: &DesignPlan) -> Vec<&'static str> {
    plan.issues.iter().map(|issue| issue.code).collect()
}

async fn exercise(target: impl Fn() -> SqliteTarget) {
    let dir = tempfile::tempdir().unwrap();
    let path = database(dir.path());
    let session = Arc::new(
        DbSession::open_sqlite("designer".into(), &connection(&path), target(), false)
            .await
            .unwrap(),
    );

    // The structure as the designer edits it.
    let loaded = session.table_design(&table("items")).await.unwrap();
    let names: Vec<&str> = loaded.columns.iter().map(|c| c.name.as_str()).collect();
    assert_eq!(names, ["id", "name", "qty", "owner", "at"]);
    assert_eq!(loaded.columns[0].identity, "autoincrement");
    assert!(!loaded.columns[1].nullable);
    assert_eq!(loaded.columns[2].default, "0");
    assert_eq!(loaded.columns[4].default, "CURRENT_TIMESTAMP");
    assert_eq!(loaded.primary_key.columns, ["id"]);
    assert_eq!(loaded.uniques.len(), 1);
    assert_eq!(loaded.uniques[0].columns, ["name"]);
    assert_eq!(loaded.checks.len(), 1);
    assert!(
        loaded.checks[0].expression.contains("qty >= 0"),
        "{:?}",
        loaded.checks
    );
    assert_eq!(loaded.foreign_keys.len(), 1);
    assert_eq!(loaded.foreign_keys[0].ref_table, "owners");
    assert_eq!(loaded.foreign_keys[0].ref_columns, ["id"]);
    assert_eq!(loaded.foreign_keys[0].on_delete, "CASCADE");
    assert_eq!(loaded.indexes.len(), 1);
    assert_eq!(loaded.indexes[0].name, "items_owner");
    assert!(loaded.unmodeled.is_empty(), "{:?}", loaded.unmodeled);
    let unchanged = designer::plan(DbEngine::Sqlite, Some(&loaded), &loaded);
    assert!(
        unchanged.statements.is_empty(),
        "{:?}",
        unchanged.statements
    );

    // Renames and an added column run in place.
    let mut renamed = loaded.clone();
    renamed.columns[1].name = "title".into();
    renamed.uniques[0].columns = vec!["title".into()];
    renamed.columns.push(ColumnDesign {
        name: "note".into(),
        data_type: "TEXT".into(),
        nullable: true,
        ..Default::default()
    });
    let plan = designer::plan(DbEngine::Sqlite, Some(&loaded), &renamed);
    assert!(
        !codes(&plan).contains(&"table_rebuild"),
        "{:?}",
        plan.issues
    );
    assert!(apply(&session, &plan).await);
    let loaded = session.table_design(&table("items")).await.unwrap();
    assert_eq!(loaded.columns[1].name, "title");
    assert_eq!(loaded.columns.last().unwrap().name, "note");

    // Anything else rebuilds the table; rows, indexes, AUTOINCREMENT and
    // the rows of tables that refer to it are kept.
    let mut rebuilt = loaded.clone();
    rebuilt.columns[2].nullable = false;
    rebuilt.columns.retain(|c| c.name != "at");
    let plan = designer::plan(DbEngine::Sqlite, Some(&loaded), &rebuilt);
    assert!(codes(&plan).contains(&"table_rebuild"), "{:?}", plan.issues);
    assert!(apply(&session, &plan).await);
    assert_eq!(
        rows(
            &session,
            "SELECT id, title, qty, owner, note FROM items ORDER BY id"
        )
        .await,
        ["1|a|1|1|NULL", "2|b|2|1|NULL"]
    );
    assert_eq!(rows(&session, "SELECT body FROM notes").await, ["kept"]);
    let loaded = session.table_design(&table("items")).await.unwrap();
    assert!(!loaded.columns.iter().any(|c| c.name == "at"));
    assert!(!loaded.columns[2].nullable);
    assert_eq!(loaded.columns[0].identity, "autoincrement");
    assert_eq!(loaded.indexes.len(), 1);
    assert_eq!(loaded.foreign_keys.len(), 1);
    assert_eq!(
        rows(
            &session,
            "SELECT count(*) FROM sqlite_master WHERE name LIKE '%cc_rebuild%'"
        )
        .await,
        ["0"]
    );

    // A rebuild that fails (an existing row breaks the new check) leaves the
    // table as it was.
    let mut failing = loaded.clone();
    failing.checks.push(CheckDesign {
        name: "qty_over_one".into(),
        expression: "qty > 1".into(),
        ..Default::default()
    });
    let plan = designer::plan(DbEngine::Sqlite, Some(&loaded), &failing);
    assert!(!apply(&session, &plan).await);
    let after = session.table_design(&table("items")).await.unwrap();
    assert_eq!(after.checks, loaded.checks);
    assert_eq!(rows(&session, "SELECT count(*) FROM items").await, ["2"]);

    // Triggers would be lost: no rebuild, renames still in place.
    let logged = session.table_design(&table("logged")).await.unwrap();
    assert_eq!(logged.unmodeled, ["triggers"]);
    let mut dropped = logged.clone();
    dropped.columns.pop();
    let plan = designer::plan(DbEngine::Sqlite, Some(&logged), &dropped);
    assert!(
        codes(&plan).contains(&"rebuild_blocked"),
        "{:?}",
        plan.issues
    );
    let mut renamed = logged.clone();
    renamed.columns[1].name = "value".into();
    assert!(
        apply(
            &session,
            &designer::plan(DbEngine::Sqlite, Some(&logged), &renamed)
        )
        .await
    );

    // A new table.
    let fresh = TableDesign {
        database: "main".into(),
        name: "tags".into(),
        columns: vec![
            ColumnDesign {
                name: "tag".into(),
                data_type: "TEXT".into(),
                ..Default::default()
            },
            ColumnDesign {
                name: "weight".into(),
                data_type: "REAL".into(),
                nullable: true,
                ..Default::default()
            },
        ],
        primary_key: designer::KeyDesign {
            columns: vec!["tag".into()],
            ..Default::default()
        },
        options: [("without_rowid".to_owned(), "true".to_owned())].into(),
        ..Default::default()
    };
    assert!(apply(&session, &designer::plan(DbEngine::Sqlite, None, &fresh)).await);
    let tags = session.table_design(&table("tags")).await.unwrap();
    assert_eq!(
        tags.options.get("without_rowid").map(String::as_str),
        Some("true")
    );
}

#[tokio::test]
async fn designer_on_a_local_sqlite_file() {
    exercise(|| SqliteTarget::Local).await;
}

#[tokio::test]
async fn designer_through_sqlite3_on_a_host() {
    if !program_available() {
        eprintln!("sqlite3 not installed; skipped");
        return;
    }
    exercise(|| SqliteTarget::Cli(Arc::new(LocalProgram))).await;
}
