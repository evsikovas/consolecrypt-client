//! SQLite files on this computer and through the `sqlite3` program, against
//! sample databases created in a temporary directory. The program route runs
//! the local `sqlite3` with the same argument vector and standard input the
//! SSH, Docker and Kubernetes runners send (skipped when it is not installed).
use async_trait::async_trait;
use cc_db_core::{
    edit::DbCellEdit,
    sqlite_cli::{CliOutput, SqliteCliRunner},
    DbCell, DbError, DbObjectKind, DbObjectRef, DbQueryEvent, DbQueryRequest, DbServerReadOnly,
    DbSession, SqliteTarget,
};
use cc_models::database::{DbConnection, DbEngine, DbOptions};
use std::{collections::BTreeMap, path::Path, sync::Arc, time::Duration};
use tokio_util::sync::CancellationToken;

fn sample(dir: &Path, wal: bool) -> std::path::PathBuf {
    let path = dir.join("shop.db");
    let connection = rusqlite::Connection::open(&path).unwrap();
    if wal {
        connection
            .query_row("PRAGMA journal_mode = WAL", [], |_| Ok(()))
            .unwrap();
    }
    connection
        .execute_batch(
            "PRAGMA foreign_keys = ON;
             CREATE TABLE customers (id INTEGER PRIMARY KEY AUTOINCREMENT, email TEXT NOT NULL UNIQUE, note TEXT);
             CREATE TABLE orders (id INTEGER PRIMARY KEY, customer_id INTEGER NOT NULL REFERENCES customers(id) ON DELETE CASCADE,
               total REAL DEFAULT 0, payload BLOB) STRICT;
             CREATE TABLE tags (name TEXT PRIMARY KEY, weight INTEGER) WITHOUT ROWID;
             CREATE TABLE loose (a, b);
             CREATE INDEX orders_by_customer ON orders(customer_id, total);
             CREATE VIEW big_orders AS SELECT * FROM orders WHERE total > 100;
             CREATE TRIGGER orders_touch AFTER UPDATE ON orders BEGIN SELECT 1; END;
             INSERT INTO customers(email, note) VALUES ('a@example.test', 'it''s'), ('b@example.test', NULL);
             INSERT INTO orders VALUES (1, 1, 150.5, X'00FF41'), (2, 2, 20, NULL);
             INSERT INTO tags VALUES ('x', 1);
             INSERT INTO loose VALUES (1, 'one'), (2, 'two');
             WITH RECURSIVE n(i) AS (SELECT 1 UNION ALL SELECT i + 1 FROM n WHERE i < 2000)
               INSERT INTO loose SELECT i + 10, 'row' FROM n;
             ANALYZE;",
        )
        .unwrap();
    path
}

fn connection(path: &Path, read_only: bool, production: bool) -> DbConnection {
    let mut connection = DbConnection::new("shop", "localhost", DbEngine::Sqlite);
    connection.database = path.to_string_lossy().into_owned();
    connection.options = DbOptions::Sqlite { read_only };
    connection.production = production;
    connection.validate().unwrap();
    connection
}

fn request(text: &str) -> DbQueryRequest {
    DbQueryRequest {
        text: text.into(),
        parameters: Vec::new(),
        approval_ticket: None,
    }
}

fn plain(cell: &DbCell) -> String {
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

/// Rows of every result, or the error code.
async fn run(session: &Arc<DbSession>, query: &DbQueryRequest) -> Result<Vec<Vec<DbCell>>, String> {
    let mut stream = session
        .execute(query)
        .map_err(|error| format!("{error:?}"))?;
    let control = stream.control();
    let mut rows = Vec::new();
    while let Some(event) = stream.events.recv().await {
        match event {
            DbQueryEvent::Rows { rows: batch, .. } => rows.extend(batch),
            DbQueryEvent::Paused { .. } => control.load_more(1000).unwrap(),
            DbQueryEvent::Error { code, .. } => return Err(code),
            DbQueryEvent::Done => break,
            DbQueryEvent::Cancelled => return Err("cancelled".into()),
            _ => {}
        }
    }
    Ok(rows)
}

async fn approved(session: &Arc<DbSession>, text: &str) -> Result<Vec<Vec<DbCell>>, String> {
    let review = session
        .guard()
        .review(&request(text))
        .map_err(|error| format!("{error:?}"))?;
    session.guard().approve(&review.ticket).unwrap();
    let mut query = request(text);
    query.approval_ticket = Some(review.ticket);
    run(session, &query).await
}

async fn open(connection: &DbConnection, target: SqliteTarget) -> Arc<DbSession> {
    Arc::new(
        DbSession::open_sqlite("s".into(), connection, target, false)
            .await
            .unwrap(),
    )
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

/// Runs the local `sqlite3` with the argument vector and standard input.
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
            use std::io::Write;
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

#[tokio::test]
async fn read_only_files_browse_query_and_refuse_every_write() {
    let dir = tempfile::tempdir().unwrap();
    let path = sample(dir.path(), true);
    let session = open(&connection(&path, true, false), SqliteTarget::Local).await;
    assert!(session.info().read_only);
    assert_eq!(session.info().server_read_only, DbServerReadOnly::Enforced);

    // Types: INTEGER, REAL, TEXT (with a quote), BLOB, NULL.
    let rows = run(
        &session,
        &request("SELECT id, customer_id, total, payload FROM orders ORDER BY id"),
    )
    .await
    .unwrap();
    assert_eq!(rows[0][0], DbCell::Int("1".into()));
    assert_eq!(rows[0][2], DbCell::Float("150.5".into()));
    assert_eq!(rows[0][3], DbCell::Binary("AP9B".into()));
    assert_eq!(rows[1][3], DbCell::Null);
    let mut bound = request("SELECT note FROM customers WHERE email = {{email}}");
    bound.parameters.push(cc_db_core::DbParameter {
        name: "email".into(),
        value: DbCell::Text("a@example.test".into()),
        type_hint: None,
    });
    assert_eq!(
        run(&session, &bound).await.unwrap(),
        [[DbCell::Text("it's".into())]]
    );

    // Streams past the first page.
    assert_eq!(
        run(&session, &request("SELECT * FROM loose"))
            .await
            .unwrap()
            .len(),
        2002
    );

    // Writes, pragmas that write, ATTACH and file functions never run.
    for text in [
        "INSERT INTO loose VALUES (0, 'x')",
        "DELETE FROM loose WHERE a = 1",
        "PRAGMA user_version = 5",
        "PRAGMA query_only = 0",
        "ATTACH DATABASE '/tmp/other.db' AS other",
        "SELECT load_extension('evil')",
        "SELECT writefile('/tmp/x', 'y')",
        "VACUUM INTO '/tmp/copy.db'",
        "PRAGMA rekey = 'secret'",
    ] {
        assert!(session.execute(&request(text)).is_err(), "{text}");
    }
    assert!(session.guard().allow_session_write().is_err());
    // Read pragmas work.
    let info = run(&session, &request("PRAGMA table_info(customers)"))
        .await
        .unwrap();
    assert_eq!(info.len(), 3);
    assert_eq!(
        run(&session, &request("PRAGMA page_count"))
            .await
            .unwrap()
            .len(),
        1
    );
    // The file is unchanged.
    let count = run(&session, &request("SELECT count(*) FROM loose"))
        .await
        .unwrap();
    assert_eq!(count, [[DbCell::Int("2002".into())]]);
}

#[tokio::test]
async fn structure_tree_ddl_details_plan_and_completion() {
    let dir = tempfile::tempdir().unwrap();
    let path = sample(dir.path(), false);
    let session = open(&connection(&path, true, true), SqliteTarget::Local).await;
    let root = session.tree(None, None).await.unwrap();
    assert_eq!(root.nodes.len(), 1);
    assert_eq!(root.nodes[0].reference.kind, DbObjectKind::Database);
    assert_eq!(root.nodes[0].reference.name, "shop.db");
    let database = session
        .tree(Some(&root.nodes[0].reference), None)
        .await
        .unwrap();
    let names = |kind: DbObjectKind| {
        database
            .nodes
            .iter()
            .filter(|node| node.reference.kind == kind)
            .map(|node| node.reference.name.clone())
            .collect::<Vec<_>>()
    };
    assert_eq!(
        names(DbObjectKind::Table),
        ["customers", "loose", "orders", "tags"]
    );
    assert_eq!(names(DbObjectKind::View), ["big_orders"]);
    assert!(names(DbObjectKind::Index).contains(&"orders_by_customer".to_owned()));
    assert_eq!(names(DbObjectKind::Trigger), ["orders_touch"]);
    assert_eq!(names(DbObjectKind::Sequence), ["customers"]);
    assert_eq!(names(DbObjectKind::DataType).len(), 5);
    let orders = database
        .nodes
        .iter()
        .find(|node| node.reference.name == "orders")
        .unwrap();
    assert_eq!(orders.detail.as_deref(), Some("STRICT"));

    let children = session
        .tree(Some(&table("orders")), None)
        .await
        .unwrap()
        .nodes;
    let of = |kind: DbObjectKind| {
        children
            .iter()
            .filter(|node| node.reference.kind == kind)
            .collect::<Vec<_>>()
    };
    let columns = of(DbObjectKind::Column);
    assert_eq!(columns.len(), 4);
    assert!(columns[0].primary_key);
    assert_eq!(columns[1].detail.as_deref(), Some("INTEGER"));
    assert_eq!(columns[1].nullable, Some(false));
    assert_eq!(columns[1].foreign_key.as_ref().unwrap().name, "customers");
    assert_eq!(columns[2].default_value.as_deref(), Some("0"));
    assert_eq!(of(DbObjectKind::Key)[0].detail.as_deref(), Some("(id)"));
    let foreign = of(DbObjectKind::ForeignKey);
    assert_eq!(foreign[0].reference.name, "customer_id");
    assert!(foreign[0]
        .detail
        .as_deref()
        .unwrap()
        .contains("customers(id) ON DELETE CASCADE"));
    assert_eq!(
        of(DbObjectKind::Index)[0].detail.as_deref(),
        Some("(customer_id, total)")
    );
    assert_eq!(
        of(DbObjectKind::Trigger)[0].detail.as_deref(),
        Some("AFTER UPDATE")
    );
    let customers = session
        .tree(Some(&table("customers")), None)
        .await
        .unwrap()
        .nodes;
    let reference = customers
        .iter()
        .find(|node| node.reference.kind == DbObjectKind::Reference)
        .unwrap();
    assert_eq!(reference.reference.name, "orders");
    assert!(customers
        .iter()
        .any(|node| node.reference.kind == DbObjectKind::Key
            && node.detail.as_deref() == Some("UNIQUE (email)")));

    let ddl = session.ddl(&table("orders")).await.unwrap();
    assert!(ddl.starts_with("CREATE TABLE orders"));
    assert!(ddl.contains("CREATE INDEX orders_by_customer"));
    assert!(ddl.contains("CREATE TRIGGER orders_touch"));

    let details =
        serde_json::to_value(session.object_details(&table("tags")).await.unwrap()).unwrap();
    let text = details.to_string();
    assert!(text.contains("\"without_rowid\"") && text.contains("\"yes\""));
    let file = serde_json::to_value(
        session
            .object_details(&root.nodes[0].reference)
            .await
            .unwrap(),
    )
    .unwrap()
    .to_string();
    assert!(file.contains("page_size") && file.contains("total_size"));
    // ANALYZE ran: an estimate exists.
    let loose = serde_json::to_value(session.object_details(&table("loose")).await.unwrap())
        .unwrap()
        .to_string();
    assert!(loose.contains("\"rows\""), "{loose}");

    let plan = session
        .explain(
            &request("SELECT * FROM orders WHERE customer_id = 1"),
            false,
        )
        .await
        .unwrap();
    assert!(plan["plan"]["sqlite_plan"][0]["detail"]
        .as_str()
        .unwrap()
        .contains("orders"));
    assert!(session.explain(&request("SELECT 1"), true).await.is_err());

    let completions = session.complete("SELECT * FROM ord", 17).await.unwrap();
    assert!(completions.iter().any(|item| item.label == "orders"));
    let completions = session.complete("SELECT orders.cus", 17).await.unwrap();
    assert!(completions.iter().any(|item| item.label == "customer_id"));
    assert_eq!(
        session.object_query(&table("loose"), 10).await.unwrap(),
        "SELECT rowid, * FROM \"loose\" LIMIT 10;"
    );
    assert_eq!(
        session.object_query(&table("orders"), 10).await.unwrap(),
        "SELECT * FROM \"orders\" LIMIT 10;"
    );
}

#[tokio::test]
async fn writable_files_edit_cells_by_key_or_rowid_after_review() {
    let dir = tempfile::tempdir().unwrap();
    let path = sample(dir.path(), true);
    let session = open(&connection(&path, false, false), SqliteTarget::Local).await;
    assert!(!session.info().read_only);
    run(&session, &request("INSERT INTO loose VALUES (99, 'new')"))
        .await
        .unwrap();
    assert!(approved(&session, "DELETE FROM loose").await.is_ok());

    // Edit by primary key.
    let edit = DbCellEdit {
        original_primary_key: BTreeMap::from([("id".into(), DbCell::Int("2".into()))]),
        column: "note".into(),
        value: DbCell::Text("o'k".into()),
    };
    let review = session
        .review_cell_edits(&table("customers"), &[edit])
        .await
        .unwrap();
    assert!(review.preview.contains("'o''k'"));
    let ticket = session.guard().review(&review.request).unwrap().ticket;
    session.guard().approve(&ticket).unwrap();
    let mut apply = review.request.clone();
    apply.approval_ticket = Some(ticket);
    assert_eq!(session.apply_cell_edits(&apply).await.unwrap(), 1);
    assert_eq!(
        run(
            &session,
            &request("SELECT note FROM customers WHERE id = 2")
        )
        .await
        .unwrap(),
        [[DbCell::Text("o'k".into())]]
    );

    // A rowid table without a key edits by rowid.
    run(&session, &request("INSERT INTO loose VALUES (1, 'one')"))
        .await
        .unwrap();
    let rowid = plain(
        &run(&session, &request("SELECT rowid FROM loose"))
            .await
            .unwrap()[0][0],
    );
    let edit = DbCellEdit {
        original_primary_key: BTreeMap::from([("rowid".into(), DbCell::Int(rowid))]),
        column: "b".into(),
        value: DbCell::Binary("AAE=".into()),
    };
    let review = session
        .review_cell_edits(&table("loose"), &[edit])
        .await
        .unwrap();
    assert!(review.preview.contains("X'0001'"));
    let ticket = session.guard().review(&review.request).unwrap().ticket;
    session.guard().approve(&ticket).unwrap();
    let mut apply = review.request.clone();
    apply.approval_ticket = Some(ticket);
    assert_eq!(session.apply_cell_edits(&apply).await.unwrap(), 1);

    // Production: read-only until a write window opens.
    let production = open(&connection(&path, false, true), SqliteTarget::Local).await;
    assert!(production
        .execute(&request("INSERT INTO loose VALUES (5, 'x')"))
        .is_err());
    production
        .guard()
        .open_production_write(Duration::from_secs(300))
        .unwrap();
    run(&production, &request("INSERT INTO loose VALUES (5, 'x')"))
        .await
        .unwrap();
}

#[tokio::test]
async fn long_queries_cancel_and_files_are_checked_before_opening() {
    let dir = tempfile::tempdir().unwrap();
    let path = sample(dir.path(), false);
    let session = open(&connection(&path, true, false), SqliteTarget::Local).await;
    let mut stream = session
        .execute(&request(
            "WITH RECURSIVE n(i) AS (SELECT 1 UNION ALL SELECT i + 1 FROM n) SELECT count(*) FROM n",
        ))
        .unwrap();
    tokio::time::sleep(Duration::from_millis(200)).await;
    stream.cancel();
    let ended = tokio::time::timeout(Duration::from_secs(5), async {
        while let Some(event) = stream.events.recv().await {
            if matches!(event, DbQueryEvent::Cancelled | DbQueryEvent::Error { .. }) {
                return true;
            }
        }
        true
    })
    .await;
    assert_eq!(ended, Ok(true));
    // The session keeps working after the interrupt.
    assert_eq!(
        run(&session, &request("SELECT 1")).await.unwrap(),
        [[DbCell::Int("1".into())]]
    );

    let missing = connection(&dir.path().join("absent.db"), true, false);
    assert_eq!(
        DbSession::open_sqlite("m".into(), &missing, SqliteTarget::Local, false)
            .await
            .unwrap_err(),
        DbError::FileMissing
    );
    assert!(!dir.path().join("absent.db").exists(), "never created");
    let text = dir.path().join("notes.txt");
    std::fs::write(&text, "this is not a database at all").unwrap();
    assert_eq!(
        DbSession::open_sqlite(
            "t".into(),
            &connection(&text, true, false),
            SqliteTarget::Local,
            false
        )
        .await
        .unwrap_err(),
        DbError::NotSqlite
    );
    // Creating is explicit and refuses an existing file.
    let fresh = dir.path().join("fresh.db");
    cc_db_core::sqlite_create_file(&fresh).unwrap();
    assert!(cc_db_core::sqlite_create_file(&fresh).is_err());
    cc_db_core::sqlite_check_file(&fresh).unwrap();
    open(&connection(&fresh, true, false), SqliteTarget::Local).await;
}

#[tokio::test]
async fn the_sqlite3_program_route_reads_writes_and_browses() {
    if !program_available() {
        eprintln!("sqlite3 is not installed; skipping the program route");
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let path = sample(dir.path(), true);
    let target = || SqliteTarget::Cli(Arc::new(LocalProgram));
    let session = open(&connection(&path, true, false), target()).await;
    let rows = run(&session, &request("SELECT orders.id, total, payload, note FROM orders LEFT JOIN customers c ON c.id = orders.customer_id ORDER BY 1"))
        .await
        .unwrap();
    assert_eq!(rows[0][0], DbCell::Int("1".into()));
    assert_eq!(rows[0][1], DbCell::Float("150.5".into()));
    assert_eq!(rows[0][2], DbCell::Binary("AP9B".into()));
    assert_eq!(rows[0][3], DbCell::Text("it's".into()));
    assert_eq!(rows[1][3], DbCell::Null);
    let mut bound = request("SELECT {{v}} || '!'");
    bound.parameters.push(cc_db_core::DbParameter {
        name: "v".into(),
        value: DbCell::Text("'; DROP TABLE orders; --".into()),
        type_hint: None,
    });
    assert_eq!(
        run(&session, &bound).await.unwrap(),
        [[DbCell::Text("'; DROP TABLE orders; --!".into())]]
    );
    assert_eq!(
        run(&session, &request("SELECT * FROM loose"))
            .await
            .unwrap()
            .len(),
        2002
    );
    assert!(session
        .execute(&request("DELETE FROM loose WHERE a = 1"))
        .is_err());
    let database = session
        .tree(
            Some(&session.tree(None, None).await.unwrap().nodes[0].reference),
            None,
        )
        .await
        .unwrap();
    assert!(database
        .nodes
        .iter()
        .any(|node| node.reference.name == "big_orders"));
    let columns = session.tree(Some(&table("orders")), None).await.unwrap();
    assert!(columns
        .nodes
        .iter()
        .any(|node| node.reference.kind == DbObjectKind::ForeignKey));
    assert!(session
        .ddl(&table("tags"))
        .await
        .unwrap()
        .contains("WITHOUT ROWID"));
    let plan = session
        .explain(
            &request("SELECT * FROM orders WHERE customer_id = 1"),
            false,
        )
        .await
        .unwrap();
    assert!(!plan["plan"]["sqlite_plan"].as_array().unwrap().is_empty());

    // A writable connection writes only what the guard permitted.
    let writable = open(&connection(&path, false, false), target()).await;
    run(&writable, &request("INSERT INTO loose VALUES (7, 'seven')"))
        .await
        .unwrap();
    assert_eq!(
        run(&writable, &request("SELECT b FROM loose WHERE a = 7"))
            .await
            .unwrap(),
        [[DbCell::Text("seven".into())]]
    );

    // Missing files and missing programs fail clearly.
    let missing = connection(&dir.path().join("absent.db"), true, false);
    assert_eq!(
        DbSession::open_sqlite("m".into(), &missing, target(), false)
            .await
            .unwrap_err(),
        DbError::FileMissing
    );
    assert!(!dir.path().join("absent.db").exists());
}
