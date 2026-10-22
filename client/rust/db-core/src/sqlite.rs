//! SQLite files on this computer. The connection uses the application's own
//! SQLite build (the SQLCipher-enabled `libsqlite3-sys` the vault store links;
//! without a key it reads plain SQLite files) and never sets a key.
//!
//! Safety layers, outermost first: the SQL guard; a read-only open for
//! read-only connections; `PRAGMA query_only` for every statement the guard
//! did not permit to write; an engine authorizer that refuses ATTACH/DETACH,
//! file and extension functions, key pragmas and (in read mode) every write
//! action; no attached databases; defensive mode and untrusted schema.
//! Queries run on the blocking pool and stream rows in batches; cancellation
//! and timeouts interrupt the running statement.
use crate::{
    streaming::RowConsumer, DbCell, DbColumn, DbError, DbParameter, MAX_EVENT_BYTES,
    MAX_EVENT_ROWS,
};
use base64::{engine::general_purpose::STANDARD, Engine};
use cc_models::database::{DbConnection, DbOptions};
use rusqlite::{
    config::DbConfig,
    hooks::{AuthAction, AuthContext, Authorization},
    limits::Limit,
    types::{Value, ValueRef},
    Connection, ErrorCode, InterruptHandle, OpenFlags,
};
use std::{
    fmt,
    io::Read,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicU8, Ordering},
        Arc, Mutex,
    },
    time::{Duration, Instant},
};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

/// The first 16 bytes of every SQLite database file.
pub const SQLITE_HEADER: &[u8; 16] = b"SQLite format 3\0";

/// Authorizer modes: the driver's own control statements, a read, a write.
const MODE_INTERNAL: u8 = 0;
const MODE_READ: u8 = 1;
const MODE_WRITE: u8 = 2;

/// How long a statement waits for another writer's lock.
const BUSY_TIMEOUT: Duration = Duration::from_secs(5);

pub(crate) enum Message {
    Started(Vec<DbColumn>),
    Rows(Vec<Vec<DbCell>>),
    Done(Option<u64>),
}

pub(crate) struct Sqlite {
    connection: Arc<Mutex<Option<Connection>>>,
    interrupt: Arc<InterruptHandle>,
    mode: Arc<AtomicU8>,
    /// Opened with SQLITE_OPEN_READONLY.
    read_only: bool,
    timeout: Duration,
    pub version: String,
    pub file_name: String,
}

impl fmt::Debug for Sqlite {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Sqlite([SCOPED])")
    }
}

/// Checks that `path` is an SQLite database file (an empty file is an empty
/// database) before anything opens it.
pub fn check_file(path: &Path) -> Result<(), DbError> {
    let metadata = std::fs::metadata(path).map_err(|error| match error.kind() {
        std::io::ErrorKind::NotFound => DbError::FileMissing,
        std::io::ErrorKind::PermissionDenied => DbError::PolicyDenied,
        _ => DbError::ConnectionFailed,
    })?;
    if !metadata.is_file() {
        return Err(DbError::NotSqlite);
    }
    if metadata.len() == 0 {
        return Ok(());
    }
    let mut header = [0u8; 16];
    let mut file = std::fs::File::open(path).map_err(|_| DbError::ConnectionFailed)?;
    file.read_exact(&mut header)
        .map_err(|_| DbError::NotSqlite)?;
    if &header != SQLITE_HEADER {
        return Err(DbError::NotSqlite);
    }
    Ok(())
}

/// Creates a new, empty database file. Refuses an existing path; this is the
/// only way the application creates an SQLite file.
pub fn create_file(path: &Path) -> Result<(), DbError> {
    if path.as_os_str().is_empty() || !path.is_absolute() {
        return Err(DbError::InvalidRequest);
    }
    std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|error| match error.kind() {
            std::io::ErrorKind::AlreadyExists => DbError::InvalidRequest,
            std::io::ErrorKind::PermissionDenied => DbError::PolicyDenied,
            _ => DbError::ConnectionFailed,
        })?;
    let connection = Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .map_err(classify)?;
    // Writes the header page so the file identifies itself.
    connection
        .execute_batch("PRAGMA user_version = 0; VACUUM;")
        .map_err(classify)?;
    Ok(())
}

pub(crate) fn classify(error: rusqlite::Error) -> DbError {
    match error.sqlite_error_code() {
        Some(ErrorCode::OperationInterrupted) => DbError::Cancelled,
        Some(ErrorCode::DatabaseBusy | ErrorCode::DatabaseLocked) => DbError::Timeout,
        Some(ErrorCode::ReadOnly | ErrorCode::AuthorizationForStatementDenied) => {
            DbError::PolicyDenied
        }
        Some(ErrorCode::PermissionDenied) => DbError::PolicyDenied,
        Some(ErrorCode::NotADatabase) => DbError::NotSqlite,
        Some(ErrorCode::CannotOpen) => DbError::ConnectionFailed,
        Some(ErrorCode::TooBig) => DbError::ResultLimit,
        _ => DbError::QueryFailed,
    }
}

/// The engine authorizer: the last check before SQLite runs anything.
fn authorize(mode: &AtomicU8, context: AuthContext<'_>) -> Authorization {
    let mode = mode.load(Ordering::Acquire);
    if mode == MODE_INTERNAL {
        return Authorization::Allow;
    }
    let read = mode != MODE_WRITE;
    match context.action {
        AuthAction::Attach { .. } | AuthAction::Detach { .. } => Authorization::Deny,
        AuthAction::Function { function_name } => {
            if crate::parser::sqlite::denied_name(function_name) {
                Authorization::Deny
            } else {
                Authorization::Allow
            }
        }
        AuthAction::Pragma {
            pragma_name,
            pragma_value,
        } => {
            use crate::parser::sqlite::{pragma_kind, PragmaKind};
            let argument = pragma_value.is_some();
            let as_argument = pragma_kind(pragma_name, argument, false);
            let as_assignment = pragma_kind(pragma_name, false, argument);
            if as_argument == PragmaKind::Denied || as_assignment == PragmaKind::Denied {
                Authorization::Deny
            } else if read && as_argument != PragmaKind::Read {
                Authorization::Deny
            } else {
                Authorization::Allow
            }
        }
        AuthAction::Read { .. }
        | AuthAction::Select
        | AuthAction::Recursive
        | AuthAction::Unknown { .. } => Authorization::Allow,
        // Everything else changes a database or the connection.
        _ if read => Authorization::Deny,
        _ => Authorization::Allow,
    }
}

impl Sqlite {
    /// Opens the file the connection names. Read-only connections open with
    /// SQLITE_OPEN_READONLY; others open read-write but run every statement
    /// under `query_only` unless the guard permitted a write. Never creates.
    pub async fn open(connection: &DbConnection) -> Result<Self, DbError> {
        let DbOptions::Sqlite { read_only } = connection.options else {
            return Err(DbError::InvalidRequest);
        };
        let path = PathBuf::from(&connection.database);
        if !path.is_absolute() {
            return Err(DbError::InvalidRequest);
        }
        let timeout = Duration::from_secs(u64::from(connection.limits.statement_timeout_seconds));
        tokio::task::spawn_blocking(move || Self::open_blocking(&path, read_only, timeout))
            .await
            .map_err(|_| DbError::ConnectionFailed)?
    }

    fn open_blocking(path: &Path, read_only: bool, timeout: Duration) -> Result<Self, DbError> {
        check_file(path)?;
        let flags = if read_only {
            OpenFlags::SQLITE_OPEN_READ_ONLY
        } else {
            OpenFlags::SQLITE_OPEN_READ_WRITE
        } | OpenFlags::SQLITE_OPEN_NO_MUTEX;
        let connection = Connection::open_with_flags(path, flags).map_err(|error| {
            match error.sqlite_error_code() {
                Some(ErrorCode::CannotOpen) if !path.exists() => DbError::FileMissing,
                _ => classify(error),
            }
        })?;
        connection.busy_timeout(BUSY_TIMEOUT).map_err(classify)?;
        // No other file can join this connection.
        connection
            .set_limit(Limit::SQLITE_LIMIT_ATTACHED, 0)
            .map_err(classify)?;
        for (option, value) in [
            (DbConfig::SQLITE_DBCONFIG_DEFENSIVE, true),
            (DbConfig::SQLITE_DBCONFIG_TRUSTED_SCHEMA, false),
            (DbConfig::SQLITE_DBCONFIG_ENABLE_FTS3_TOKENIZER, false),
            (DbConfig::SQLITE_DBCONFIG_WRITABLE_SCHEMA, false),
        ] {
            connection.set_db_config(option, value).map_err(classify)?;
        }
        connection
            .execute_batch("PRAGMA query_only = 1;")
            .map_err(classify)?;
        let mode = Arc::new(AtomicU8::new(MODE_READ));
        let hook = Arc::clone(&mode);
        connection
            .authorizer(Some(move |context: AuthContext<'_>| {
                authorize(&hook, context)
            }))
            .map_err(classify)?;
        // A file that is not a database (or an encrypted one) fails here.
        let version: String = connection
            .query_row(
                "SELECT sqlite_version(), (SELECT count(*) FROM sqlite_master)",
                [],
                |row| row.get(0),
            )
            .map_err(classify)?;
        let interrupt = Arc::new(connection.get_interrupt_handle());
        Ok(Self {
            connection: Arc::new(Mutex::new(Some(connection))),
            interrupt,
            mode,
            read_only,
            timeout,
            version,
            file_name: path
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_default(),
        })
    }

    pub fn query<'a>(
        &'a mut self,
        sql: &'a str,
        bindings: &'a [DbParameter],
        read_only: bool,
        sink: &'a mut (dyn RowConsumer + Send),
        cancel: CancellationToken,
    ) -> futures::future::BoxFuture<'a, Result<(), DbError>> {
        Box::pin(async move {
            self.query_all(&[(sql, bindings)], read_only, sink, cancel)
                .await
        })
    }

    /// Runs statements in order; several run in one transaction (all or
    /// nothing). A write runs only when the guard permitted it.
    pub fn query_all<'a>(
        &'a mut self,
        statements: &'a [(&'a str, &'a [DbParameter])],
        read_only: bool,
        sink: &'a mut (dyn RowConsumer + Send),
        cancel: CancellationToken,
    ) -> futures::future::BoxFuture<'a, Result<(), DbError>> {
        let owned: Vec<(String, Vec<DbParameter>)> = statements
            .iter()
            .map(|(sql, bindings)| ((*sql).to_owned(), bindings.to_vec()))
            .collect();
        Box::pin(async move {
            let writable = !read_only && !self.read_only;
            let (sender, mut receiver) = mpsc::channel::<Result<Message, DbError>>(2);
            let connection = Arc::clone(&self.connection);
            let mode = Arc::clone(&self.mode);
            let task = tokio::task::spawn_blocking(move || {
                run_blocking(&connection, &mode, writable, owned, &sender)
            });
            let started = Instant::now();
            let forward = async {
                while let Some(message) = receiver.recv().await {
                    match message? {
                        Message::Started(columns) => sink.start(columns, None).await?,
                        Message::Rows(rows) => {
                            for row in rows {
                                if !sink.row(row).await? {
                                    return Err(DbError::ResultLimit);
                                }
                            }
                        }
                        Message::Done(affected) => {
                            sink.statement_done(started.elapsed().as_millis() as u64, affected)
                                .await?
                        }
                    }
                }
                Ok(())
            };
            let result = tokio::select! {
                biased;
                _ = cancel.cancelled() => Err(DbError::Cancelled),
                result = tokio::time::timeout(self.timeout, forward) => result.unwrap_or(Err(DbError::Timeout)),
            };
            if result.is_err() {
                self.interrupt.interrupt();
            }
            drop(receiver);
            let joined = task.await;
            if joined.is_err() {
                // The worker panicked: never reuse the connection.
                self.connection
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .take();
            }
            result
        })
    }
}

impl Drop for Sqlite {
    fn drop(&mut self) {
        self.interrupt.interrupt();
    }
}

fn internal(connection: &Connection, mode: &AtomicU8, sql: &str) -> Result<(), DbError> {
    let previous = mode.swap(MODE_INTERNAL, Ordering::AcqRel);
    let result = connection.execute_batch(sql).map_err(classify);
    mode.store(previous, Ordering::Release);
    result
}

type Sender = mpsc::Sender<Result<Message, DbError>>;

fn run_blocking(
    connection: &Mutex<Option<Connection>>,
    mode: &AtomicU8,
    writable: bool,
    statements: Vec<(String, Vec<DbParameter>)>,
    sender: &Sender,
) {
    let mut guard = connection.lock().unwrap_or_else(|e| e.into_inner());
    let Some(connection) = guard.as_ref() else {
        let _ = sender.blocking_send(Err(DbError::SessionClosed));
        return;
    };
    let transaction = statements.len() > 1;
    let result = (|| {
        if writable {
            internal(connection, mode, "PRAGMA query_only = 0;")?;
        }
        mode.store(
            if writable { MODE_WRITE } else { MODE_READ },
            Ordering::Release,
        );
        if transaction {
            internal(
                connection,
                mode,
                if writable { "BEGIN IMMEDIATE;" } else { "BEGIN;" },
            )?;
        }
        for (sql, parameters) in &statements {
            run_one(connection, sql, parameters, sender)?;
        }
        if transaction {
            internal(connection, mode, "COMMIT;")?;
        }
        Ok(())
    })();
    if result.is_err() && !connection.is_autocommit() {
        let _ = internal(connection, mode, "ROLLBACK;");
    }
    // Back to read-only before anything else can run; a connection that
    // cannot return is closed.
    let restored = !writable || internal(connection, mode, "PRAGMA query_only = 1;").is_ok();
    mode.store(MODE_READ, Ordering::Release);
    if !restored {
        guard.take();
    }
    drop(guard);
    if let Err(error) = result {
        let _ = sender.blocking_send(Err(error));
    }
}

fn run_one(
    connection: &Connection,
    sql: &str,
    parameters: &[DbParameter],
    sender: &Sender,
) -> Result<(), DbError> {
    let mut statement = connection.prepare(sql).map_err(classify)?;
    if statement.parameter_count() != parameters.len() {
        return Err(DbError::InvalidRequest);
    }
    for (index, parameter) in parameters.iter().enumerate() {
        statement
            .raw_bind_parameter(index + 1, bind_value(&parameter.value)?)
            .map_err(classify)?;
    }
    let columns: Vec<DbColumn> = statement
        .columns()
        .iter()
        .map(|column| DbColumn {
            name: column.name().to_owned(),
            type_name: column.decl_type().unwrap_or_default().to_owned(),
            nullable: true,
            sensitive: crate::sensitive_column(column.name()),
            primary_key: false,
        })
        .collect();
    let count = columns.len();
    let changes = !statement.readonly();
    send(sender, Message::Started(columns))?;
    let mut rows = statement.raw_query();
    let mut batch = Vec::new();
    let mut bytes = 0usize;
    while let Some(row) = rows.next().map_err(classify)? {
        let mut cells = Vec::with_capacity(count);
        for index in 0..count {
            let cell = cell(row.get_ref(index).map_err(classify)?)?;
            bytes += cell_bytes(&cell);
            cells.push(cell);
        }
        batch.push(cells);
        if batch.len() >= MAX_EVENT_ROWS || bytes >= MAX_EVENT_BYTES / 2 {
            send(sender, Message::Rows(std::mem::take(&mut batch)))?;
            bytes = 0;
        }
    }
    drop(rows);
    if !batch.is_empty() {
        send(sender, Message::Rows(batch))?;
    }
    send(
        sender,
        Message::Done(changes.then(|| connection.changes())),
    )
}

fn send(sender: &Sender, message: Message) -> Result<(), DbError> {
    sender
        .blocking_send(Ok(message))
        .map_err(|_| DbError::Cancelled)
}

fn cell_bytes(cell: &DbCell) -> usize {
    match cell {
        DbCell::Null | DbCell::Bool(_) => 4,
        DbCell::Int(v)
        | DbCell::Uint(v)
        | DbCell::Decimal(v)
        | DbCell::Float(v)
        | DbCell::Text(v)
        | DbCell::Date(v)
        | DbCell::Time(v)
        | DbCell::Timestamp(v)
        | DbCell::Binary(v)
        | DbCell::Json(v) => v.len() + 16,
    }
}

/// Storage class → cell: NULL, INTEGER, REAL, TEXT (binary when it is not
/// UTF-8), BLOB (base64).
pub(crate) fn cell(value: ValueRef<'_>) -> Result<DbCell, DbError> {
    Ok(match value {
        ValueRef::Null => DbCell::Null,
        ValueRef::Integer(value) => DbCell::Int(value.to_string()),
        ValueRef::Real(value) => DbCell::Float(real_text(value)),
        ValueRef::Text(bytes) | ValueRef::Blob(bytes) if bytes.len() > MAX_EVENT_BYTES => {
            return Err(DbError::ResultLimit)
        }
        ValueRef::Text(bytes) => match std::str::from_utf8(bytes) {
            Ok(text) => DbCell::Text(text.to_owned()),
            Err(_) => DbCell::Binary(STANDARD.encode(bytes)),
        },
        ValueRef::Blob(bytes) => DbCell::Binary(STANDARD.encode(bytes)),
    })
}

/// The shortest text that reads back as the same REAL (`1.0`, `1e300`).
pub(crate) fn real_text(value: f64) -> String {
    if value.is_infinite() {
        return if value > 0.0 { "Inf" } else { "-Inf" }.into();
    }
    format!("{value:?}")
}

/// A request value as the SQLite value it binds as.
pub(crate) fn bind_value(cell: &DbCell) -> Result<Value, DbError> {
    Ok(match cell {
        DbCell::Null => Value::Null,
        DbCell::Bool(value) => Value::Integer(i64::from(*value)),
        DbCell::Int(value) => Value::Integer(value.trim().parse().map_err(|_| DbError::InvalidRequest)?),
        DbCell::Uint(value) => {
            let value: u64 = value.trim().parse().map_err(|_| DbError::InvalidRequest)?;
            Value::Integer(i64::try_from(value).map_err(|_| DbError::InvalidRequest)?)
        }
        DbCell::Float(value) => Value::Real(value.trim().parse().map_err(|_| DbError::InvalidRequest)?),
        DbCell::Binary(value) => Value::Blob(
            STANDARD
                .decode(value)
                .map_err(|_| DbError::InvalidRequest)?,
        ),
        // Column affinity converts decimal text the way SQLite stores it.
        DbCell::Decimal(value)
        | DbCell::Text(value)
        | DbCell::Date(value)
        | DbCell::Time(value)
        | DbCell::Timestamp(value)
        | DbCell::Json(value) => Value::Text(value.clone()),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::Collect;

    async fn writable_file(dir: &Path) -> Sqlite {
        let path = dir.join("t.db");
        Connection::open(&path)
            .unwrap()
            .execute_batch("CREATE TABLE t (a INTEGER, b TEXT); INSERT INTO t VALUES (1, 'x');")
            .unwrap();
        let mut connection = DbConnection::new("t", "localhost", cc_models::database::DbEngine::Sqlite);
        connection.database = path.to_string_lossy().into_owned();
        connection.options = DbOptions::Sqlite { read_only: false };
        Sqlite::open(&connection).await.unwrap()
    }

    async fn query(driver: &mut Sqlite, sql: &str, read_only: bool) -> Result<Vec<Vec<String>>, DbError> {
        let mut rows = Collect::default();
        driver
            .query(sql, &[], read_only, &mut rows, CancellationToken::new())
            .await?;
        Ok(rows.rows)
    }

    /// Even a statement that reached the driver as a read cannot change the
    /// file: query_only and the authorizer refuse it.
    #[tokio::test]
    async fn reads_cannot_write_attach_or_unlock_even_past_the_guard() {
        let dir = tempfile::tempdir().unwrap();
        let mut driver = writable_file(dir.path()).await;
        for sql in [
            "INSERT INTO t VALUES (2, 'y')",
            "UPDATE t SET b = 'z'",
            "CREATE TABLE u (x)",
            "PRAGMA query_only = 0",
            "PRAGMA user_version = 3",
            "PRAGMA writable_schema = 1",
            "PRAGMA key = 'k'",
            "PRAGMA rekey = 'k'",
        ] {
            assert!(query(&mut driver, sql, true).await.is_err(), "{sql}");
        }
        for sql in [
            "ATTACH DATABASE ':memory:' AS other",
            "SELECT load_extension('x')",
        ] {
            assert!(query(&mut driver, sql, false).await.is_err(), "{sql}");
        }
        assert_eq!(
            query(&mut driver, "SELECT count(*) FROM t", true).await.unwrap(),
            [["1"]]
        );
        // A permitted write works and the connection returns to query_only.
        query(&mut driver, "INSERT INTO t VALUES (2, 'y')", false)
            .await
            .unwrap();
        assert!(query(&mut driver, "DELETE FROM t", true).await.is_err());
        assert_eq!(
            query(&mut driver, "PRAGMA query_only", true).await.unwrap(),
            [["1"]]
        );
    }

    #[test]
    fn storage_classes_map_to_cells_and_back() {
        assert_eq!(cell(ValueRef::Null).unwrap(), DbCell::Null);
        assert_eq!(cell(ValueRef::Integer(-3)).unwrap(), DbCell::Int("-3".into()));
        assert_eq!(cell(ValueRef::Real(1.0)).unwrap(), DbCell::Float("1.0".into()));
        assert_eq!(cell(ValueRef::Real(1e300)).unwrap(), DbCell::Float("1e300".into()));
        assert_eq!(cell(ValueRef::Text(b"hi")).unwrap(), DbCell::Text("hi".into()));
        assert_eq!(cell(ValueRef::Text(&[0xff])).unwrap(), DbCell::Binary("/w==".into()));
        assert_eq!(cell(ValueRef::Blob(&[0, 1])).unwrap(), DbCell::Binary("AAE=".into()));
        assert_eq!(bind_value(&DbCell::Bool(true)).unwrap(), Value::Integer(1));
        assert_eq!(
            bind_value(&DbCell::Binary("AAE=".into())).unwrap(),
            Value::Blob(vec![0, 1])
        );
        assert!(bind_value(&DbCell::Uint(u64::MAX.to_string())).is_err());
        assert_eq!(bind_value(&DbCell::Decimal("1.50".into())).unwrap(), Value::Text("1.50".into()));
    }
}
