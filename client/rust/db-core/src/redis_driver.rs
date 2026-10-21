//! Redis over the vault route: one multiplexed RESP2/RESP3 connection on the
//! connector's stream (TLS verifies the original server name), guarded argv
//! only, replies rendered as result tables and a SCAN-paged key tree.
//!
//! Tree references: a database is `RedisDatabase { database: "3", name: "db3",
//! parent_name: None }`; a key folder is `RedisDatabase { database: "3", name:
//! "user:", parent_name: Some(<enclosing prefix>) }`; a key is `RedisKey {
//! database: "3", name: <full key>, parent_name: Some(<folder prefix>) }`.
use crate::{
    streaming::RowConsumer,
    transport::{DbConnector, DbCredentials, DbDestination, DbIo, TlsProfile},
    DbCell, DbColumn, DbError, DbObjectKind, DbObjectRef, DbTreeNode, DbTreePage,
    DEFAULT_TREE_PAGE,
};
use base64::{engine::general_purpose::STANDARD, Engine};
use cc_models::database::{DbConnection, DbOptions};
use futures::future::BoxFuture;
use redis::{
    aio::MultiplexedConnection, AsyncConnectionConfig, ProtocolVersion, RedisConnectionInfo,
    RedisError, Value,
};
use secrecy::ExposeSecret;
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    fmt,
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

/// Key-tree separator (spec default).
const SEPARATOR: &str = ":";
const SCAN_COUNT: u64 = 500;
/// SCAN round trips per tree page; sparse MATCH results return a short page
/// with a continuation cursor instead of walking the whole keyspace.
const SCAN_ROUNDS: usize = 32;
const DEFAULT_DATABASES: i64 = 16;
const MAX_DATABASES: i64 = 1024;
const TREE_TIMEOUT: Duration = Duration::from_secs(30);

/// One authenticated connection. Dropping it aborts the I/O task, which
/// closes the socket even while a long command is still running server side.
struct Live {
    connection: MultiplexedConnection,
    worker: JoinHandle<()>,
    db: i64,
}
impl Drop for Live {
    fn drop(&mut self) {
        self.worker.abort();
    }
}

pub(crate) struct Redis {
    /// User commands; stays on the configured database (SELECT is refused by
    /// the guard). Reopened lazily after a cancel, timeout or I/O failure.
    query: Option<Live>,
    /// Tree browsing in another database, so the query connection's selected
    /// database is never switched.
    browser: Option<Live>,
    connector: Arc<dyn DbConnector>,
    destination: DbDestination,
    tls: TlsProfile,
    /// Holds the password for lazy reconnects; its Debug redacts it and it is
    /// never formatted by this driver.
    settings: RedisConnectionInfo,
    db: i64,
    timeout: Duration,
    connect_timeout: Duration,
    scope: CancellationToken,
    pub version: String,
}
impl fmt::Debug for Redis {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Redis([SCOPED])")
    }
}

impl Redis {
    pub async fn connect(
        connection: &DbConnection,
        credentials: &DbCredentials,
        connector: Arc<dyn DbConnector>,
        scope: CancellationToken,
    ) -> Result<Self, DbError> {
        let destination = DbDestination::new(&connection.host, connection.port)?;
        let tls = TlsProfile::new(connection, credentials)?;
        let db = database_index(&connection.database)?;
        let resp3 = match &connection.options {
            DbOptions::Redis { resp3 } => *resp3,
            _ => return Err(DbError::InvalidRequest),
        };
        let mut settings = RedisConnectionInfo::default()
            .set_db(db)
            .set_skip_set_lib_name()
            .set_protocol(if resp3 {
                ProtocolVersion::RESP3
            } else {
                ProtocolVersion::RESP2
            });
        // An empty ACL user authenticates as `default`; a named user without a
        // stored password (ACL `nopass`) still authenticates as that user.
        if !connection.username.is_empty() {
            settings = settings.set_username(&connection.username);
        }
        match &credentials.password {
            Some(password) => settings = settings.set_password(password.expose_secret()),
            None if !connection.username.is_empty() => settings = settings.set_password(""),
            None => {}
        }
        let mut driver = Self {
            query: None,
            browser: None,
            connector,
            destination,
            tls,
            settings,
            db,
            timeout: Duration::from_secs(u64::from(
                connection.limits.statement_timeout_seconds.max(1),
            )),
            connect_timeout: Duration::from_secs(u64::from(
                connection.limits.connect_timeout_seconds.clamp(1, 120),
            )),
            scope,
            version: String::new(),
        };
        let mut live = driver.connection(db).await?;
        let info = bounded(driver.connect_timeout, &driver.scope.clone(), async move {
            redis::cmd("INFO")
                .arg("server")
                .query_async::<String>(&mut live)
                .await
                .map_err(classify)
        })
        .await?;
        driver.version = info_field(&info, "redis_version")
            .ok_or(DbError::QueryFailed)?
            .to_owned();
        Ok(driver)
    }

    /// Runs one guarded command and renders its reply as one result table.
    /// Redis has no per-command cancel: on cancel or timeout the connection is
    /// closed (a blocked reply cannot wedge later commands) and reopened lazily.
    pub fn command<'a>(
        &'a mut self,
        argv: &'a [Vec<u8>],
        sink: &'a mut (dyn RowConsumer + Send),
        cancel: CancellationToken,
    ) -> BoxFuture<'a, Result<(), DbError>> {
        Box::pin(async move {
            let name = argv
                .first()
                .map(|value| String::from_utf8_lossy(value).to_ascii_uppercase())
                .ok_or(DbError::InvalidRequest)?;
            let mut command = redis::Cmd::new();
            for argument in argv {
                command.arg(argument.as_slice());
            }
            let started = Instant::now();
            let mut live = self.connection(self.db).await?;
            let operation = async move {
                command
                    .query_async::<Value>(&mut live)
                    .await
                    .map_err(classify)
            };
            let result = tokio::select! {
                biased;
                _ = cancel.cancelled() => Err(DbError::Cancelled),
                result = tokio::time::timeout(self.timeout, operation) => result.unwrap_or(Err(DbError::Timeout)),
            };
            let value = match result {
                Ok(value) => value,
                Err(error) => {
                    if matches!(
                        error,
                        DbError::Cancelled | DbError::Timeout | DbError::ConnectionFailed
                    ) {
                        self.query = None;
                    }
                    return Err(error);
                }
            };
            let table = render(&name, argv, value);
            sink.start(table.columns, None).await?;
            for row in table.rows {
                if !sink.row(row).await? {
                    break;
                }
            }
            sink.statement_done(
                started.elapsed().as_millis().min(u128::from(u64::MAX)) as u64,
                None,
            )
            .await
        })
    }

    /// Structure below `parent`: databases at the top, then the key tree of
    /// one database grouped by `:`, loaded with SCAN (never KEYS) in pages.
    pub fn browse<'a>(
        &'a mut self,
        parent: Option<&'a DbObjectRef>,
        cursor: Option<&'a str>,
    ) -> BoxFuture<'a, Result<DbTreePage, DbError>> {
        Box::pin(async move {
            let Some(parent) = parent else {
                return self.databases().await;
            };
            match parent.kind {
                DbObjectKind::RedisDatabase => {
                    let db = database_index(&parent.database)?;
                    let prefix = if parent.parent_name.is_some() {
                        parent.name.clone()
                    } else {
                        String::new()
                    };
                    self.keys(db, prefix, cursor).await
                }
                DbObjectKind::RedisKey => Ok(DbTreePage {
                    nodes: Vec::new(),
                    next_cursor: None,
                }),
                _ => Err(DbError::InvalidRequest),
            }
        })
    }

    /// Human-readable summary of one key: type, encoding, length, TTL, memory.
    // Exposed through the session's describe action (wired separately).
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn describe<'a>(
        &'a mut self,
        object: &'a DbObjectRef,
    ) -> BoxFuture<'a, Result<String, DbError>> {
        Box::pin(async move {
            if object.kind != DbObjectKind::RedisKey || object.name.is_empty() {
                return Err(DbError::InvalidRequest);
            }
            let db = database_index(&object.database)?;
            let key = object.name.clone().into_bytes();
            let mut live = self.connection(db).await?;
            let operation = async move {
                let mut pipe = redis::pipe();
                pipe.ignore_errors();
                pipe.cmd("TYPE").arg(&key);
                pipe.cmd("PTTL").arg(&key);
                pipe.cmd("OBJECT").arg("ENCODING").arg(&key);
                pipe.cmd("MEMORY").arg("USAGE").arg(&key);
                let replies: Vec<redis::RedisResult<Value>> =
                    pipe.query_async(&mut live).await.map_err(classify)?;
                let mut replies = replies.into_iter().map(|reply| reply.ok());
                let kind = replies
                    .next()
                    .flatten()
                    .and_then(|value| text(&value))
                    .unwrap_or_else(|| "none".into());
                let ttl = replies.next().flatten().and_then(|value| integer(&value));
                let encoding = replies.next().flatten().and_then(|value| text(&value));
                let memory = replies.next().flatten().and_then(|value| integer(&value));
                let length = match length_command(&kind) {
                    Some((command, unit)) => redis::cmd(command)
                        .arg(&key)
                        .query_async::<i64>(&mut live)
                        .await
                        .ok()
                        .map(|length| format!("{length} {unit}")),
                    None => None,
                };
                Ok::<_, DbError>(describe_text(
                    &String::from_utf8_lossy(&key),
                    db,
                    &kind,
                    encoding.as_deref(),
                    length.as_deref(),
                    ttl,
                    memory,
                ))
            };
            let timeout = self.timeout.min(TREE_TIMEOUT);
            let scope = self.scope.clone();
            let result = bounded(timeout, &scope, operation).await;
            self.forget_on_failure(db, &result);
            result
        })
    }

    /// The database list: `CONFIG GET databases` when the ACL permits it
    /// (otherwise 16), with key counts from `INFO keyspace`.
    fn databases(&mut self) -> BoxFuture<'_, Result<DbTreePage, DbError>> {
        Box::pin(async move {
            let mut live = self.connection(self.db).await?;
            let operation = async move {
                let count = redis::cmd("CONFIG")
                    .arg("GET")
                    .arg("databases")
                    .query_async::<BTreeMap<String, String>>(&mut live)
                    .await
                    .ok()
                    .and_then(|values| values.get("databases")?.parse::<i64>().ok())
                    .filter(|count| (1..=MAX_DATABASES).contains(count))
                    .unwrap_or(DEFAULT_DATABASES);
                let keyspace = redis::cmd("INFO")
                    .arg("keyspace")
                    .query_async::<String>(&mut live)
                    .await
                    .unwrap_or_default();
                Ok::<_, DbError>((count, keyspace))
            };
            let scope = self.scope.clone();
            let result = bounded(self.timeout.min(TREE_TIMEOUT), &scope, operation).await;
            self.forget_on_failure(self.db, &result);
            let (count, keyspace) = result?;
            let counts = keyspace_counts(&keyspace);
            Ok(DbTreePage {
                nodes: (0..count)
                    .map(|index| DbTreeNode {
                        reference: DbObjectRef {
                            database: index.to_string(),
                            schema: None,
                            kind: DbObjectKind::RedisDatabase,
                            name: format!("db{index}"),
                            parent_name: None,
                        },
                        has_children: true,
                        detail: counts.get(&index).map(|keys| format!("{keys} keys")),
                        nullable: None,
                        default_value: None,
                        primary_key: false,
                        foreign_key: None,
                    })
                    .collect(),
                next_cursor: None,
            })
        })
    }

    fn keys<'a>(
        &'a mut self,
        db: i64,
        prefix: String,
        cursor: Option<&'a str>,
    ) -> BoxFuture<'a, Result<DbTreePage, DbError>> {
        Box::pin(async move {
            let start = match cursor {
                Some(cursor) => decode_cursor(cursor, db, &prefix)?,
                None => 0,
            };
            let mut live = self.connection(db).await?;
            let pattern = format!("{}*", escape_glob(&prefix));
            let scan_prefix = prefix.clone();
            let operation = async move {
                let mut scan = start;
                let mut keys = Vec::new();
                for _ in 0..SCAN_ROUNDS {
                    let (next, batch): (u64, Vec<Vec<u8>>) = redis::cmd("SCAN")
                        .arg(scan)
                        .arg("MATCH")
                        .arg(&pattern)
                        .arg("COUNT")
                        .arg(SCAN_COUNT)
                        .query_async(&mut live)
                        .await
                        .map_err(classify)?;
                    keys.extend(batch);
                    scan = next;
                    if scan == 0 || keys.len() >= DEFAULT_TREE_PAGE {
                        break;
                    }
                }
                let keys: Vec<String> = keys
                    .iter()
                    .map(|key| String::from_utf8_lossy(key).into_owned())
                    .collect();
                let (folders, leaves) = group_keys(&scan_prefix, &keys);
                let leaves: Vec<String> = leaves.into_iter().collect();
                let mut details = Vec::with_capacity(leaves.len());
                if !leaves.is_empty() {
                    let mut pipe = redis::pipe();
                    pipe.ignore_errors();
                    for key in &leaves {
                        pipe.cmd("TYPE").arg(key);
                        pipe.cmd("PTTL").arg(key);
                    }
                    let replies: Vec<redis::RedisResult<Value>> =
                        pipe.query_async(&mut live).await.map_err(classify)?;
                    for pair in replies.chunks(2) {
                        let kind = pair
                            .first()
                            .and_then(|reply| reply.as_ref().ok())
                            .and_then(text);
                        let ttl = pair
                            .get(1)
                            .and_then(|reply| reply.as_ref().ok())
                            .and_then(integer);
                        details.push((kind, ttl));
                    }
                }
                Ok::<_, DbError>((scan, folders, leaves, details))
            };
            let scope = self.scope.clone();
            let result = bounded(self.timeout.min(TREE_TIMEOUT), &scope, operation).await;
            self.forget_on_failure(db, &result);
            let (scan, folders, leaves, details) = result?;
            let database = db.to_string();
            let mut nodes = Vec::with_capacity(folders.len() + leaves.len());
            for folder in folders {
                nodes.push(tree_node(
                    DbObjectRef {
                        database: database.clone(),
                        schema: None,
                        kind: DbObjectKind::RedisDatabase,
                        name: folder,
                        parent_name: Some(prefix.clone()),
                    },
                    true,
                    None,
                ));
            }
            for (key, (kind, ttl)) in leaves.into_iter().zip(details) {
                // Expired between SCAN and TYPE.
                if kind.as_deref() == Some("none") {
                    continue;
                }
                nodes.push(tree_node(
                    DbObjectRef {
                        database: database.clone(),
                        schema: None,
                        kind: DbObjectKind::RedisKey,
                        name: key,
                        parent_name: Some(prefix.clone()),
                    },
                    false,
                    Some(key_detail(kind.as_deref(), ttl)),
                ));
            }
            Ok(DbTreePage {
                nodes,
                next_cursor: (scan != 0).then(|| encode_cursor(scan, db, &prefix)),
            })
        })
    }

    /// A clone of the authenticated connection for `db`, opened on demand.
    fn connection(&mut self, db: i64) -> BoxFuture<'_, Result<MultiplexedConnection, DbError>> {
        Box::pin(async move {
            if self.scope.is_cancelled() {
                return Err(DbError::SessionClosed);
            }
            let slot = if db == self.db {
                &self.query
            } else {
                &self.browser
            };
            let reusable = slot
                .as_ref()
                .filter(|live| live.db == db && !live.worker.is_finished())
                .map(|live| live.connection.clone());
            if let Some(connection) = reusable {
                return Ok(connection);
            }
            let live = self.open(db).await?;
            let connection = live.connection.clone();
            if db == self.db {
                self.query = Some(live);
            } else {
                self.browser = Some(live);
            }
            Ok(connection)
        })
    }

    fn forget_on_failure<T>(&mut self, db: i64, result: &Result<T, DbError>) {
        if matches!(
            result,
            Err(DbError::Cancelled | DbError::Timeout | DbError::ConnectionFailed)
        ) {
            if db == self.db {
                self.query = None;
            } else {
                self.browser = None;
            }
        }
    }

    /// Owned inputs: the handshake future borrows nothing from the driver.
    fn open(&self, db: i64) -> BoxFuture<'static, Result<Live, DbError>> {
        let connector = Arc::clone(&self.connector);
        let destination = self.destination.clone();
        let tls = self.tls.clone();
        let settings = self.settings.clone().set_db(db);
        let scope = self.scope.clone();
        let limit = self.connect_timeout;
        Box::pin(async move {
            // One task owns the settings, performs the handshake, hands the
            // connection back and then drives its I/O: the driver future may
            // borrow the settings, and 'static inputs keep the caller's future
            // Send for every lifetime (rust-lang/rust#102211).
            let (ready, connected) = tokio::sync::oneshot::channel();
            let task_scope = scope.clone();
            let worker = tokio::spawn(async move {
                let handshake = async {
                    let stream = connector.connect(&destination, task_scope.clone()).await?;
                    // Prefer never retries in plaintext: no password over a downgrade.
                    let stream = tls.connect(stream, None).await?;
                    let config = AsyncConnectionConfig::new()
                        .set_connection_timeout(None)
                        .set_response_timeout(None);
                    MultiplexedConnection::new_with_config(&settings, Socket(stream), config)
                        .await
                        .map_err(classify)
                };
                match handshake.await {
                    Ok((connection, driver)) => {
                        if ready.send(Ok(connection)).is_ok() {
                            tokio::select! {
                                biased;
                                _ = task_scope.cancelled() => {},
                                _ = driver => {},
                            }
                        }
                    }
                    Err(error) => {
                        let _ = ready.send(Err(error));
                    }
                }
            });
            let result = tokio::select! {
                biased;
                _ = scope.cancelled() => Err(DbError::Cancelled),
                result = tokio::time::timeout(limit, connected) => match result {
                    Ok(Ok(result)) => result,
                    Ok(Err(_)) => Err(DbError::ConnectionFailed),
                    Err(_) => Err(DbError::Timeout),
                },
            };
            let connection = match result {
                Ok(connection) => connection,
                Err(error) => {
                    worker.abort();
                    return Err(error);
                }
            };
            Ok(Live {
                connection,
                worker,
                db,
            })
        })
    }
}

/// Runs a driver operation under the session scope and a deadline.
async fn bounded<T>(
    limit: Duration,
    scope: &CancellationToken,
    operation: impl std::future::Future<Output = Result<T, DbError>>,
) -> Result<T, DbError> {
    tokio::select! {
        biased;
        _ = scope.cancelled() => Err(DbError::Cancelled),
        result = tokio::time::timeout(limit, operation) => result.unwrap_or(Err(DbError::Timeout)),
    }
}

fn database_index(value: &str) -> Result<i64, DbError> {
    let value = if value.is_empty() { "0" } else { value };
    value
        .parse::<i64>()
        .ok()
        .filter(|index| (0..MAX_DATABASES).contains(index))
        .ok_or(DbError::InvalidRequest)
}

fn classify(error: RedisError) -> DbError {
    match error.code() {
        Some("WRONGPASS" | "NOAUTH") => return DbError::AuthenticationFailed,
        Some("NOPERM") => return DbError::PolicyDenied,
        _ => {}
    }
    match error.kind() {
        redis::ErrorKind::AuthenticationFailed => DbError::AuthenticationFailed,
        redis::ErrorKind::Io | redis::ErrorKind::Parse => DbError::ConnectionFailed,
        _ if error.is_io_error() || error.is_connection_dropped() => DbError::ConnectionFailed,
        _ => DbError::QueryFailed,
    }
}

/// Value of `field` in an INFO reply (`field:value` lines).
fn info_field<'a>(info: &'a str, field: &str) -> Option<&'a str> {
    info.lines().find_map(|line| {
        let (name, value) = line.trim_end_matches('\r').split_once(':')?;
        (name == field).then_some(value)
    })
}

/// `db3:keys=12,expires=0,avg_ttl=0` lines → key counts per database.
fn keyspace_counts(info: &str) -> BTreeMap<i64, u64> {
    info.lines()
        .filter_map(|line| {
            let (name, fields) = line.trim_end_matches('\r').split_once(':')?;
            let index = name.strip_prefix("db")?.parse().ok()?;
            let keys = fields
                .split(',')
                .find_map(|field| field.strip_prefix("keys="))?
                .parse()
                .ok()?;
            Some((index, keys))
        })
        .collect()
}

fn escape_glob(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len());
    for character in value.chars() {
        if matches!(character, '*' | '?' | '[' | ']' | '\\') {
            escaped.push('\\');
        }
        escaped.push(character);
    }
    escaped
}

/// Splits the keys below `prefix` into folders (the next `:` segment, prefix
/// included, e.g. `user:` → `user:session:`) and leaf keys. A key equal to a
/// folder prefix is a leaf inside that folder.
pub(crate) fn group_keys(prefix: &str, keys: &[String]) -> (BTreeSet<String>, BTreeSet<String>) {
    let mut folders = BTreeSet::new();
    let mut leaves = BTreeSet::new();
    for key in keys {
        let Some(rest) = key.strip_prefix(prefix) else {
            continue;
        };
        match rest.find(SEPARATOR) {
            Some(position) => {
                folders.insert(format!("{prefix}{}", &rest[..position + SEPARATOR.len()]));
            }
            None => {
                leaves.insert(key.clone());
            }
        }
    }
    (folders, leaves)
}

/// Continuation for one database and prefix; a cursor from another node is
/// refused rather than silently resumed elsewhere.
fn encode_cursor(scan: u64, db: i64, prefix: &str) -> String {
    format!("{scan}.{}", cursor_tag(db, prefix))
}

fn decode_cursor(cursor: &str, db: i64, prefix: &str) -> Result<u64, DbError> {
    let (scan, tag) = cursor.split_once('.').ok_or(DbError::InvalidRequest)?;
    if tag != cursor_tag(db, prefix) {
        return Err(DbError::InvalidRequest);
    }
    scan.parse().map_err(|_| DbError::InvalidRequest)
}

fn cursor_tag(db: i64, prefix: &str) -> String {
    let mut hash = Sha256::new();
    hash.update(db.to_be_bytes());
    hash.update(prefix.as_bytes());
    hash.finalize()[..8]
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn tree_node(reference: DbObjectRef, has_children: bool, detail: Option<String>) -> DbTreeNode {
    DbTreeNode {
        reference,
        has_children,
        detail,
        nullable: None,
        default_value: None,
        primary_key: false,
        foreign_key: None,
    }
}

fn key_detail(kind: Option<&str>, ttl: Option<i64>) -> String {
    let kind = kind.unwrap_or("unknown");
    match ttl {
        Some(milliseconds) if milliseconds >= 0 => {
            format!("{kind} · TTL {}", duration_text(milliseconds))
        }
        _ => kind.to_owned(),
    }
}

fn duration_text(milliseconds: i64) -> String {
    if milliseconds < 1000 {
        return format!("{milliseconds} ms");
    }
    let seconds = milliseconds / 1000;
    let (days, hours, minutes, seconds) = (
        seconds / 86_400,
        seconds % 86_400 / 3600,
        seconds % 3600 / 60,
        seconds % 60,
    );
    let mut parts = Vec::new();
    if days > 0 {
        parts.push(format!("{days}d"));
    }
    if hours > 0 {
        parts.push(format!("{hours}h"));
    }
    if minutes > 0 {
        parts.push(format!("{minutes}m"));
    }
    if seconds > 0 || parts.is_empty() {
        parts.push(format!("{seconds}s"));
    }
    parts.join(" ")
}

#[cfg_attr(not(test), allow(dead_code))]
fn length_command(kind: &str) -> Option<(&'static str, &'static str)> {
    Some(match kind {
        "string" => ("STRLEN", "bytes"),
        "hash" => ("HLEN", "fields"),
        "list" => ("LLEN", "elements"),
        "set" => ("SCARD", "members"),
        "zset" => ("ZCARD", "members"),
        "stream" => ("XLEN", "entries"),
        _ => return None,
    })
}

#[cfg_attr(not(test), allow(dead_code))]
fn describe_text(
    key: &str,
    db: i64,
    kind: &str,
    encoding: Option<&str>,
    length: Option<&str>,
    ttl: Option<i64>,
    memory: Option<i64>,
) -> String {
    let mut lines = vec![format!("key: {key}"), format!("database: {db}")];
    if kind == "none" {
        lines.push("type: none (the key does not exist)".into());
        return lines.join("\n");
    }
    lines.push(format!("type: {kind}"));
    if let Some(encoding) = encoding {
        lines.push(format!("encoding: {encoding}"));
    }
    if let Some(length) = length {
        lines.push(format!("length: {length}"));
    }
    lines.push(match ttl {
        Some(milliseconds) if milliseconds >= 0 => {
            format!("ttl: {} ({milliseconds} ms)", duration_text(milliseconds))
        }
        _ => "ttl: none (persistent)".into(),
    });
    lines.push(match memory {
        Some(bytes) => format!("memory: {bytes} bytes"),
        None => "memory: unavailable".into(),
    });
    lines.join("\n")
}

fn text(value: &Value) -> Option<String> {
    match value {
        Value::SimpleString(text) => Some(text.clone()),
        Value::BulkString(bytes) => String::from_utf8(bytes.clone()).ok(),
        Value::VerbatimString { text, .. } => Some(text.clone()),
        Value::Okay => Some("OK".into()),
        _ => None,
    }
}

fn integer(value: &Value) -> Option<i64> {
    match value {
        Value::Int(value) => Some(*value),
        other => text(other)?.parse().ok(),
    }
}

/// One rendered reply.
pub(crate) struct Table {
    pub columns: Vec<DbColumn>,
    pub rows: Vec<Vec<DbCell>>,
}

fn column(name: &str, type_name: &str) -> DbColumn {
    DbColumn {
        name: name.into(),
        type_name: type_name.into(),
        nullable: true,
        sensitive: crate::sensitive_column(name),
        primary_key: false,
    }
}

fn table(columns: &[(&str, &str)], rows: Vec<Vec<DbCell>>) -> Table {
    Table {
        columns: columns
            .iter()
            .map(|(name, type_name)| column(name, type_name))
            .collect(),
        rows,
    }
}

fn has_option(argv: &[Vec<u8>], option: &[u8]) -> bool {
    argv.iter()
        .skip(1)
        .any(|argument| argument.eq_ignore_ascii_case(option))
}

/// Reply → table. Scalars: `value`; arrays: `index`,`value`; hashes and other
/// maps: `field`,`value`; WITHSCORES: `member`,`score`; streams: `id`,`fields`;
/// SCAN family: items plus `next_cursor`. Nested replies become JSON cells.
pub(crate) fn render(command: &str, argv: &[Vec<u8>], value: Value) -> Table {
    let value = match value {
        Value::Attribute { data, .. } => *data,
        value => value,
    };
    match (command, value) {
        ("INFO", value @ (Value::BulkString(_) | Value::VerbatimString { .. })) => {
            match text(&value) {
                Some(info) => info_table(&info),
                None => scalar(value),
            }
        }
        ("SCAN" | "SSCAN" | "HSCAN" | "ZSCAN", Value::Array(parts)) if parts.len() == 2 => {
            scan_table(command, argv, parts)
        }
        ("XRANGE" | "XREVRANGE", Value::Array(entries)) => table(
            &[("id", "stream_id"), ("fields", "json")],
            entries.into_iter().map(stream_entry).collect(),
        ),
        ("XREAD", value @ (Value::Array(_) | Value::Map(_))) => {
            let streams = match value {
                Value::Map(pairs) => pairs,
                Value::Array(items) => items
                    .into_iter()
                    .filter_map(|item| match item {
                        Value::Array(mut pair) if pair.len() == 2 => {
                            let entries = pair.pop()?;
                            Some((pair.pop()?, entries))
                        }
                        _ => None,
                    })
                    .collect(),
                _ => Vec::new(),
            };
            let mut rows = Vec::new();
            for (stream, entries) in streams {
                let Value::Array(entries) = entries else {
                    continue;
                };
                for entry in entries {
                    let mut row = vec![cell(stream.clone())];
                    row.extend(stream_entry(entry));
                    rows.push(row);
                }
            }
            table(
                &[
                    ("stream", "string"),
                    ("id", "stream_id"),
                    ("fields", "json"),
                ],
                rows,
            )
        }
        ("MGET", Value::Array(items)) => labeled(&argv[1..], items, "key", "value"),
        ("HMGET", Value::Array(items)) => {
            labeled(argv.get(2..).unwrap_or(&[]), items, "field", "value")
        }
        ("SMISMEMBER", Value::Array(items)) => {
            labeled(argv.get(2..).unwrap_or(&[]), items, "member", "value")
        }
        ("ZMSCORE", Value::Array(items)) => {
            labeled(argv.get(2..).unwrap_or(&[]), items, "member", "score")
        }
        (_, Value::Array(items)) if has_option(argv, b"WITHSCORES") => {
            pairs_table(pairs(items), "member", "score")
        }
        ("HGETALL", Value::Array(items)) => pairs_table(pairs(items), "field", "value"),
        ("HRANDFIELD", Value::Array(items)) if has_option(argv, b"WITHVALUES") => {
            pairs_table(pairs(items), "field", "value")
        }
        ("XINFO", Value::Array(items))
            if argv
                .get(1)
                .is_some_and(|sub| sub.eq_ignore_ascii_case(b"STREAM"))
                && items.len() % 2 == 0 =>
        {
            pairs_table(pairs(items), "field", "value")
        }
        (_, Value::Map(entries)) => pairs_table(entries, "field", "value"),
        (_, Value::Array(items) | Value::Set(items)) => table(
            &[("index", "integer"), ("value", "value")],
            items
                .into_iter()
                .enumerate()
                .map(|(index, item)| vec![DbCell::Int(index.to_string()), cell(item)])
                .collect(),
        ),
        (_, value) => scalar(value),
    }
}

fn scalar(value: Value) -> Table {
    table(&[("value", "value")], vec![vec![cell(value)]])
}

/// Flat `[a, b, a, b]` (RESP2) or nested `[[a, b], …]` (RESP3) pairs.
fn pairs(items: Vec<Value>) -> Vec<(Value, Value)> {
    if items
        .iter()
        .all(|item| matches!(item, Value::Array(pair) if pair.len() == 2))
        && !items.is_empty()
    {
        items
            .into_iter()
            .filter_map(|item| match item {
                Value::Array(mut pair) => {
                    let second = pair.pop()?;
                    Some((pair.pop()?, second))
                }
                _ => None,
            })
            .collect()
    } else {
        let mut items = items.into_iter();
        let mut pairs = Vec::new();
        while let Some(first) = items.next() {
            pairs.push((first, items.next().unwrap_or(Value::Nil)));
        }
        pairs
    }
}

fn pairs_table(pairs: Vec<(Value, Value)>, first: &str, second: &str) -> Table {
    let score = second == "score";
    table(
        &[
            (first, "string"),
            (second, if score { "double" } else { "value" }),
        ],
        pairs
            .into_iter()
            .map(|(name, value)| {
                vec![
                    cell(name),
                    if score {
                        score_cell(value)
                    } else {
                        cell(value)
                    },
                ]
            })
            .collect(),
    )
}

fn labeled(labels: &[Vec<u8>], items: Vec<Value>, first: &str, second: &str) -> Table {
    let score = second == "score";
    table(
        &[
            (first, "string"),
            (second, if score { "double" } else { "value" }),
        ],
        items
            .into_iter()
            .enumerate()
            .map(|(index, item)| {
                vec![
                    labels
                        .get(index)
                        .map(|label| bytes_cell(label.clone()))
                        .unwrap_or(DbCell::Null),
                    if score { score_cell(item) } else { cell(item) },
                ]
            })
            .collect(),
    )
}

fn scan_table(command: &str, argv: &[Vec<u8>], mut parts: Vec<Value>) -> Table {
    let items = match parts.pop() {
        Some(Value::Array(items) | Value::Set(items)) => items,
        _ => Vec::new(),
    };
    let next = parts.pop().map(cell).unwrap_or(DbCell::Null);
    let (columns, width): (&[(&str, &str)], usize) = match command {
        "HSCAN" if has_option(argv, b"NOVALUES") => {
            (&[("field", "string"), ("next_cursor", "string")], 1)
        }
        "HSCAN" => (
            &[
                ("field", "string"),
                ("value", "value"),
                ("next_cursor", "string"),
            ],
            2,
        ),
        "ZSCAN" => (
            &[
                ("member", "string"),
                ("score", "double"),
                ("next_cursor", "string"),
            ],
            2,
        ),
        "SSCAN" => (&[("member", "string"), ("next_cursor", "string")], 1),
        _ => (&[("key", "string"), ("next_cursor", "string")], 1),
    };
    let mut rows = Vec::new();
    let mut items = items.into_iter();
    while let Some(first) = items.next() {
        let mut row = vec![cell(first)];
        if width == 2 {
            let second = items.next().unwrap_or(Value::Nil);
            row.push(if command == "ZSCAN" {
                score_cell(second)
            } else {
                cell(second)
            });
        }
        row.push(next.clone());
        rows.push(row);
    }
    // An empty iteration still tells the user where to continue.
    if rows.is_empty() {
        let mut row = vec![DbCell::Null; width];
        row.push(next);
        rows.push(row);
    }
    table(columns, rows)
}

fn stream_entry(entry: Value) -> Vec<DbCell> {
    match entry {
        Value::Array(mut parts) if parts.len() == 2 => {
            let fields = parts.pop().unwrap_or(Value::Nil);
            let id = parts.pop().map(cell).unwrap_or(DbCell::Null);
            let fields = match fields {
                Value::Array(items) => object(pairs_flat(items)),
                Value::Map(entries) => object(entries),
                Value::Nil => serde_json::Value::Null,
                other => json(&other),
            };
            vec![id, DbCell::Json(fields.to_string())]
        }
        other => vec![DbCell::Null, DbCell::Json(json(&other).to_string())],
    }
}

fn pairs_flat(items: Vec<Value>) -> Vec<(Value, Value)> {
    let mut items = items.into_iter();
    let mut pairs = Vec::new();
    while let Some(first) = items.next() {
        pairs.push((first, items.next().unwrap_or(Value::Nil)));
    }
    pairs
}

fn info_table(info: &str) -> Table {
    let mut section = String::new();
    let mut rows = Vec::new();
    for line in info.lines().map(|line| line.trim_end_matches('\r')) {
        if let Some(name) = line.strip_prefix('#') {
            section = name.trim().to_owned();
        } else if let Some((field, value)) = line.split_once(':') {
            rows.push(vec![
                DbCell::Text(section.clone()),
                DbCell::Text(field.into()),
                DbCell::Text(value.into()),
            ]);
        }
    }
    table(
        &[
            ("section", "string"),
            ("field", "string"),
            ("value", "string"),
        ],
        rows,
    )
}

/// Binary-safe: bytes that are not UTF-8 travel as base64 `Binary` cells.
fn bytes_cell(bytes: Vec<u8>) -> DbCell {
    match String::from_utf8(bytes) {
        Ok(text) => DbCell::Text(text),
        Err(error) => DbCell::Binary(STANDARD.encode(error.as_bytes())),
    }
}

fn score_cell(value: Value) -> DbCell {
    match &value {
        Value::BulkString(bytes) => match std::str::from_utf8(bytes) {
            Ok(text) if text.parse::<f64>().is_ok() => DbCell::Float(text.to_owned()),
            _ => cell(value),
        },
        _ => cell(value),
    }
}

fn cell(value: Value) -> DbCell {
    match value {
        Value::Nil => DbCell::Null,
        Value::Int(value) => DbCell::Int(value.to_string()),
        Value::BulkString(bytes) => bytes_cell(bytes),
        Value::SimpleString(text) => DbCell::Text(text),
        Value::Okay => DbCell::Text("OK".into()),
        Value::Double(value) => DbCell::Float(value.to_string()),
        Value::Boolean(value) => DbCell::Bool(value),
        Value::VerbatimString { text, .. } => DbCell::Text(text),
        Value::BigNumber(value) => DbCell::Decimal(value.to_string()),
        Value::Attribute { data, .. } => cell(*data),
        Value::ServerError(error) => DbCell::Text(format!("(error) {error}")),
        // Arrays, maps, sets, pushes and future reply kinds.
        value => DbCell::Json(json(&value).to_string()),
    }
}

fn object(pairs: Vec<(Value, Value)>) -> serde_json::Value {
    let keyed = pairs.iter().all(|(key, _)| text(key).is_some());
    if keyed {
        serde_json::Value::Object(
            pairs
                .iter()
                .filter_map(|(key, value)| Some((text(key)?, json(value))))
                .collect(),
        )
    } else {
        serde_json::Value::Array(
            pairs
                .iter()
                .map(|(key, value)| serde_json::json!([json(key), json(value)]))
                .collect(),
        )
    }
}

fn json(value: &Value) -> serde_json::Value {
    use serde_json::Value as Json;
    match value {
        Value::Nil => Json::Null,
        Value::Int(value) => Json::from(*value),
        Value::BulkString(bytes) => match std::str::from_utf8(bytes) {
            Ok(text) => Json::String(text.to_owned()),
            Err(_) => serde_json::json!({ "base64": STANDARD.encode(bytes) }),
        },
        Value::SimpleString(text) => Json::String(text.clone()),
        Value::Okay => Json::String("OK".into()),
        Value::Double(value) => serde_json::Number::from_f64(*value)
            .map(Json::Number)
            .unwrap_or_else(|| Json::String(value.to_string())),
        Value::Boolean(value) => Json::Bool(*value),
        Value::VerbatimString { text, .. } => Json::String(text.clone()),
        Value::BigNumber(value) => Json::String(value.to_string()),
        Value::Array(items) | Value::Set(items) => Json::Array(items.iter().map(json).collect()),
        Value::Map(pairs) => object(pairs.clone()),
        Value::Attribute { data, .. } => json(data),
        Value::Push { kind, data } => {
            serde_json::json!({ "push": kind.to_string(), "data": data.iter().map(json).collect::<Vec<_>>() })
        }
        Value::ServerError(error) => serde_json::json!({ "error": error.to_string() }),
        _ => Json::Null,
    }
}

/// The connector stream as a concrete type for the RESP codec.
struct Socket(DbIo);

impl tokio::io::AsyncRead for Socket {
    fn poll_read(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &mut tokio::io::ReadBuf<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        std::pin::Pin::new(&mut *self.0).poll_read(cx, buf)
    }
}

impl tokio::io::AsyncWrite for Socket {
    fn poll_write(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &[u8],
    ) -> std::task::Poll<std::io::Result<usize>> {
        std::pin::Pin::new(&mut *self.0).poll_write(cx, buf)
    }
    fn poll_flush(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        std::pin::Pin::new(&mut *self.0).poll_flush(cx)
    }
    fn poll_shutdown(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        std::pin::Pin::new(&mut *self.0).poll_shutdown(cx)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transport::DirectConnector;
    use cc_models::database::{DbEngine, DbTlsMode};
    use secrecy::SecretString;

    fn bulk(text: &str) -> Value {
        Value::BulkString(text.as_bytes().to_vec())
    }

    fn argv(text: &str) -> Vec<Vec<u8>> {
        text.split(' ')
            .map(|part| part.as_bytes().to_vec())
            .collect()
    }

    fn names(table: &Table) -> Vec<&str> {
        table
            .columns
            .iter()
            .map(|column| column.name.as_str())
            .collect()
    }

    #[test]
    fn scalars_render_as_one_value_and_binary_stays_binary() {
        let table = render("GET", &argv("GET k"), bulk("hello"));
        assert_eq!(names(&table), ["value"]);
        assert_eq!(table.rows, vec![vec![DbCell::Text("hello".into())]]);
        let table = render(
            "GET",
            &argv("GET k"),
            Value::BulkString(vec![0xff, 0x00, 0x80]),
        );
        assert_eq!(
            table.rows[0][0],
            DbCell::Binary(STANDARD.encode([0xff, 0x00, 0x80]))
        );
        assert_eq!(
            render("TTL", &argv("TTL k"), Value::Int(-1)).rows[0][0],
            DbCell::Int("-1".into())
        );
        assert_eq!(
            render("GET", &argv("GET k"), Value::Nil).rows[0][0],
            DbCell::Null
        );
        assert_eq!(
            render("PING", &argv("PING"), Value::SimpleString("PONG".into())).rows[0][0],
            DbCell::Text("PONG".into())
        );
    }

    #[test]
    fn arrays_hashes_scores_and_labels_become_tables() {
        let table = render(
            "LRANGE",
            &argv("LRANGE l 0 -1"),
            Value::Array(vec![bulk("a"), Value::Array(vec![bulk("x")])]),
        );
        assert_eq!(names(&table), ["index", "value"]);
        assert_eq!(table.rows[1][1], DbCell::Json(r#"["x"]"#.into()));

        // RESP2 flat pairs and RESP3 maps render the same hash table.
        let flat = render(
            "HGETALL",
            &argv("HGETALL h"),
            Value::Array(vec![bulk("f1"), bulk("v1"), bulk("f2"), bulk("v2")]),
        );
        let map = render(
            "HGETALL",
            &argv("HGETALL h"),
            Value::Map(vec![(bulk("f1"), bulk("v1")), (bulk("f2"), bulk("v2"))]),
        );
        assert_eq!(names(&flat), ["field", "value"]);
        assert_eq!(flat.rows, map.rows);
        assert_eq!(flat.rows.len(), 2);

        let resp2 = render(
            "ZRANGE",
            &argv("ZRANGE z 0 -1 WITHSCORES"),
            Value::Array(vec![bulk("a"), bulk("1.5"), bulk("b"), bulk("2")]),
        );
        let resp3 = render(
            "ZRANGE",
            &argv("ZRANGE z 0 -1 withscores"),
            Value::Array(vec![
                Value::Array(vec![bulk("a"), Value::Double(1.5)]),
                Value::Array(vec![bulk("b"), Value::Double(2.0)]),
            ]),
        );
        assert_eq!(names(&resp2), ["member", "score"]);
        assert_eq!(
            resp2.rows[0],
            vec![DbCell::Text("a".into()), DbCell::Float("1.5".into())]
        );
        assert_eq!(resp3.rows[0], resp2.rows[0]);

        let table = render(
            "MGET",
            &argv("MGET a b"),
            Value::Array(vec![bulk("1"), Value::Nil]),
        );
        assert_eq!(names(&table), ["key", "value"]);
        assert_eq!(table.rows[1], vec![DbCell::Text("b".into()), DbCell::Null]);
    }

    #[test]
    fn streams_scan_and_info_have_dedicated_shapes() {
        let entry = |id: &str, field: &str, value: Value| {
            Value::Array(vec![bulk(id), Value::Array(vec![bulk(field), value])])
        };
        let table = render(
            "XRANGE",
            &argv("XRANGE s - +"),
            Value::Array(vec![
                entry("1-0", "name", bulk("alpha")),
                entry("2-0", "blob", Value::BulkString(vec![0xfe])),
            ]),
        );
        assert_eq!(names(&table), ["id", "fields"]);
        assert_eq!(table.rows[0][0], DbCell::Text("1-0".into()));
        assert_eq!(table.rows[0][1], DbCell::Json(r#"{"name":"alpha"}"#.into()));
        assert_eq!(
            table.rows[1][1],
            DbCell::Json(r#"{"blob":{"base64":"/g=="}}"#.into())
        );

        let scan = render(
            "SCAN",
            &argv("SCAN 0 COUNT 10"),
            Value::Array(vec![bulk("17"), Value::Array(vec![bulk("a"), bulk("b")])]),
        );
        assert_eq!(names(&scan), ["key", "next_cursor"]);
        assert_eq!(scan.rows.len(), 2);
        assert_eq!(scan.rows[1][1], DbCell::Text("17".into()));
        let empty = render(
            "SCAN",
            &argv("SCAN 17"),
            Value::Array(vec![bulk("0"), Value::Array(vec![])]),
        );
        assert_eq!(
            empty.rows,
            vec![vec![DbCell::Null, DbCell::Text("0".into())]]
        );
        let zscan = render(
            "ZSCAN",
            &argv("ZSCAN z 0"),
            Value::Array(vec![bulk("0"), Value::Array(vec![bulk("m"), bulk("3")])]),
        );
        assert_eq!(names(&zscan), ["member", "score", "next_cursor"]);
        assert_eq!(zscan.rows[0][1], DbCell::Float("3".into()));

        let info = render(
            "INFO",
            &argv("INFO server"),
            bulk("# Server\r\nredis_version:7.4.0\r\nrun_id:x\r\n"),
        );
        assert_eq!(names(&info), ["section", "field", "value"]);
        assert_eq!(
            info.rows[0],
            vec![
                DbCell::Text("Server".into()),
                DbCell::Text("redis_version".into()),
                DbCell::Text("7.4.0".into())
            ]
        );
        assert_eq!(
            info_field("# Server\r\nredis_version:7.4.0\r\n", "redis_version"),
            Some("7.4.0")
        );
        assert_eq!(
            keyspace_counts(
                "# Keyspace\r\ndb0:keys=12,expires=1,avg_ttl=0\r\ndb3:keys=2,expires=0,avg_ttl=0\r\n"
            ),
            BTreeMap::from([(0, 12), (3, 2)])
        );
    }

    #[test]
    fn keys_group_into_folders_by_the_next_separator_segment() {
        let keys: Vec<String> = [
            "user:1",
            "user:2:session",
            "user:2:profile",
            "user:",
            "plain",
            "order:9",
        ]
        .iter()
        .map(|key| key.to_string())
        .collect();
        let (folders, leaves) = group_keys("", &keys);
        assert_eq!(folders.into_iter().collect::<Vec<_>>(), ["order:", "user:"]);
        assert_eq!(leaves.into_iter().collect::<Vec<_>>(), ["plain"]);
        let (folders, leaves) = group_keys("user:", &keys);
        assert_eq!(folders.into_iter().collect::<Vec<_>>(), ["user:2:"]);
        // A key equal to the folder prefix is a leaf inside that folder.
        assert_eq!(leaves.into_iter().collect::<Vec<_>>(), ["user:", "user:1"]);
        let (folders, leaves) = group_keys("user:2:", &keys);
        assert!(folders.is_empty());
        assert_eq!(
            leaves.into_iter().collect::<Vec<_>>(),
            ["user:2:profile", "user:2:session"]
        );
    }

    #[test]
    fn scan_cursors_are_bound_to_their_database_and_prefix() {
        let cursor = encode_cursor(1234, 2, "user:");
        assert!(cursor.len() < 64);
        assert_eq!(decode_cursor(&cursor, 2, "user:"), Ok(1234));
        assert!(decode_cursor(&cursor, 3, "user:").is_err());
        assert!(decode_cursor(&cursor, 2, "order:").is_err());
        assert!(decode_cursor("1234", 2, "user:").is_err());
        assert_eq!(escape_glob("a*b?[c]\\"), "a\\*b\\?\\[c\\]\\\\");
        assert_eq!(database_index(""), Ok(0));
        assert_eq!(database_index("15"), Ok(15));
        assert!(database_index("-1").is_err());
        assert!(database_index("db1").is_err());
    }

    #[test]
    fn key_details_and_descriptions_are_readable() {
        assert_eq!(key_detail(Some("hash"), Some(-1)), "hash");
        assert_eq!(
            key_detail(Some("string"), Some(90_500)),
            "string · TTL 1m 30s"
        );
        assert_eq!(duration_text(250), "250 ms");
        assert_eq!(duration_text(90_061_000), "1d 1h 1m 1s");
        let text = describe_text(
            "k",
            0,
            "hash",
            Some("listpack"),
            Some("2 fields"),
            Some(-1),
            Some(80),
        );
        assert!(text.contains("type: hash") && text.contains("encoding: listpack"));
        assert!(text.contains("ttl: none") && text.contains("memory: 80 bytes"));
    }

    #[test]
    fn debug_never_prints_connection_secrets() {
        let info = RedisConnectionInfo::default()
            .set_username("canary-user")
            .set_password("canary-password");
        assert!(!format!("{info:?}").contains("canary-password"));
    }

    /// Opt-in: CC_DB_LIVE_REDIS=host:port:user:password:db. Exercises describe,
    /// a cancelled blocking command (the driver accepts argv the guard would
    /// refuse) and the lazy reconnect afterwards.
    #[tokio::test]
    async fn live_describe_cancel_and_reconnect() {
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
        connection.limits.statement_timeout_seconds = 5;
        let credentials = DbCredentials {
            password: Some(SecretString::from(parts[3].to_owned())),
            client_key_pem: None,
        };
        let scope = CancellationToken::new();
        let mut redis = Redis::connect(
            &connection,
            &credentials,
            Arc::new(DirectConnector),
            scope.clone(),
        )
        .await
        .unwrap();
        assert!(!redis.version.is_empty());
        let mut sink = Rows::default();
        redis
            .command(
                &argv("HSET cc:describe a 1 b 2"),
                &mut sink,
                CancellationToken::new(),
            )
            .await
            .unwrap();
        let text = redis
            .describe(&DbObjectRef {
                database: connection.database.clone(),
                schema: None,
                kind: DbObjectKind::RedisKey,
                name: "cc:describe".into(),
                parent_name: Some("cc:".into()),
            })
            .await
            .unwrap();
        assert!(text.contains("type: hash"), "{text}");
        assert!(text.contains("length: 2 fields"), "{text}");
        assert!(text.contains("memory: "), "{text}");
        assert!(redis
            .describe(&DbObjectRef {
                database: connection.database.clone(),
                schema: None,
                kind: DbObjectKind::RedisDatabase,
                name: "db0".into(),
                parent_name: None,
            })
            .await
            .is_err());

        // A blocked reply must not wedge the session: cancel closes the socket.
        let cancel = CancellationToken::new();
        let trigger = cancel.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(200)).await;
            trigger.cancel();
        });
        let started = Instant::now();
        let result = redis
            .command(&argv("BLPOP cc:missing 30"), &mut sink, cancel)
            .await;
        assert_eq!(result, Err(DbError::Cancelled));
        assert!(started.elapsed() < Duration::from_secs(2));
        assert!(redis.query.is_none());
        let started = Instant::now();
        let mut sink = Rows::default();
        redis
            .command(&argv("PING"), &mut sink, CancellationToken::new())
            .await
            .unwrap();
        assert!(started.elapsed() < Duration::from_secs(2));
        assert_eq!(sink.rows, vec![vec![DbCell::Text("PONG".into())]]);

        // The statement timeout applies the same way.
        redis.timeout = Duration::from_millis(300);
        assert_eq!(
            redis
                .command(
                    &argv("BLPOP cc:missing 30"),
                    &mut sink,
                    CancellationToken::new()
                )
                .await,
            Err(DbError::Timeout)
        );
        redis.timeout = Duration::from_secs(5);
        let mut sink = Rows::default();
        redis
            .command(
                &argv("DEL cc:describe"),
                &mut sink,
                CancellationToken::new(),
            )
            .await
            .unwrap();
        assert_eq!(sink.rows, vec![vec![DbCell::Int("1".into())]]);
        scope.cancel();
    }

    #[derive(Default)]
    struct Rows {
        rows: Vec<Vec<DbCell>>,
    }

    #[async_trait::async_trait]
    impl RowConsumer for Rows {
        async fn start(&mut self, _: Vec<DbColumn>, _: Option<DbObjectRef>) -> Result<(), DbError> {
            Ok(())
        }
        async fn row(&mut self, row: Vec<DbCell>) -> Result<bool, DbError> {
            self.rows.push(row);
            Ok(true)
        }
        async fn statement_done(&mut self, _: u64, _: Option<u64>) -> Result<(), DbError> {
            Ok(())
        }
    }
}
