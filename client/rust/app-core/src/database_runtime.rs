//! Live database sessions of the unlocked vault. Sessions and query streams are
//! dropped on lock; credentials are read from the vault only while connecting.
use crate::{AppCore, AppError, AppResult};
use cc_db_core::{
    edit::DbCellEdit,
    export::{format_rows, ExportOptions, ExportProgress},
    introspect::{quote_identifier, DbCompletion},
    parser::editor::{self, QueryScope},
    streaming::DbQueryStream,
    transport::DbCredentials,
    DbCell, DbColumn, DbError, DbObjectRef, DbQueryEvent, DbQueryRequest, DbSession, DbSessionInfo,
    DbTreePage,
};
use cc_models::database::{DbConnection, DbEngine};
use secrecy::SecretString;
use serde::Serialize;
use std::{
    collections::HashMap,
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc, Mutex,
    },
    time::{Duration, Instant},
};
use tokio_util::sync::CancellationToken;

const MAX_SESSIONS: usize = 16;
const MAX_QUERIES: usize = 64;

struct LiveQuery {
    session_id: String,
    stream: tokio::sync::Mutex<DbQueryStream>,
    /// History record: the source template (never parameters or rows).
    connection_id: cc_models::ObjectId,
    engine: DbEngine,
    text: cc_models::secret::SecretValue,
    started: Instant,
    rows: AtomicU64,
    recorded: AtomicBool,
}

/// One statement result kept for copying until the query is released.
struct KeptResult {
    columns: Vec<DbColumn>,
    table: Option<DbObjectRef>,
    rows: Vec<Vec<DbCell>>,
}

/// What a query produced and how to run it again for a file export.
struct QueryResults {
    session_id: String,
    request: DbQueryRequest,
    sets: Mutex<Vec<KeptResult>>,
    bytes: std::sync::atomic::AtomicUsize,
}

struct RunningExport {
    progress: Arc<ExportProgress>,
    cancel: CancellationToken,
}

/// Results by query id plus their insertion order for eviction.
type KeptQueries = (
    HashMap<String, Arc<QueryResults>>,
    std::collections::VecDeque<String>,
);

#[derive(Default)]
pub(crate) struct DbRuntime {
    shutdown: CancellationToken,
    sessions: Mutex<HashMap<String, Arc<DbSession>>>,
    queries: Mutex<HashMap<String, Arc<LiveQuery>>>,
    /// Loaded rows per query id, oldest first, released with the query.
    results: Mutex<KeptQueries>,
    exports: Mutex<HashMap<String, RunningExport>>,
    /// Reviewed cell edits by ticket: (session id, generated request).
    edits: Mutex<HashMap<String, (String, DbQueryRequest)>>,
}

impl DbRuntime {
    pub(crate) fn close(&self) {
        self.shutdown.cancel();
        self.edits.lock().unwrap_or_else(|e| e.into_inner()).clear();
        {
            let mut results = self.results.lock().unwrap_or_else(|e| e.into_inner());
            results.0.clear();
            results.1.clear();
        }
        for (_, export) in self
            .exports
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .drain()
        {
            export.cancel.cancel();
        }
        self.queries
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clear();
        for (_, session) in self
            .sessions
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .drain()
        {
            session.close();
        }
    }

    fn session(&self, id: &str) -> AppResult<Arc<DbSession>> {
        self.sessions
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(id)
            .cloned()
            .ok_or_else(|| invalid("session_closed"))
    }

    fn results(&self, id: &str) -> AppResult<Arc<QueryResults>> {
        self.results
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .0
            .get(id)
            .cloned()
            .ok_or_else(|| invalid("query_closed"))
    }

    fn query(&self, id: &str) -> AppResult<Arc<LiveQuery>> {
        self.queries
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(id)
            .cloned()
            .ok_or_else(|| invalid("query_closed"))
    }
}

impl QueryResults {
    /// Keeps started columns and loaded rows within the result memory budget.
    fn keep(&self, events: &[DbQueryEvent]) {
        let mut sets = self.sets.lock().unwrap_or_else(|e| e.into_inner());
        for event in events {
            match event {
                DbQueryEvent::Started {
                    result_index,
                    columns,
                    editable_table,
                } if *result_index as usize == sets.len() => sets.push(KeptResult {
                    columns: columns.clone(),
                    table: editable_table.clone(),
                    rows: Vec::new(),
                }),
                DbQueryEvent::Rows { result_index, rows } => {
                    let Some(set) = sets.get_mut(*result_index as usize) else {
                        continue;
                    };
                    for row in rows {
                        let size: usize = row.iter().map(cell_size).sum::<usize>() + 16;
                        let used = self.bytes.fetch_add(size, Ordering::Relaxed);
                        if used + size > cc_db_core::DEFAULT_RESULT_BYTES {
                            return;
                        }
                        set.rows.push(row.clone());
                    }
                }
                _ => {}
            }
        }
    }
}

fn cell_size(cell: &DbCell) -> usize {
    match cell {
        DbCell::Null | DbCell::Bool(_) => 1,
        DbCell::Int(v)
        | DbCell::Uint(v)
        | DbCell::Decimal(v)
        | DbCell::Float(v)
        | DbCell::Text(v)
        | DbCell::Date(v)
        | DbCell::Time(v)
        | DbCell::Timestamp(v)
        | DbCell::Binary(v)
        | DbCell::Json(v) => v.len() + 8,
    }
}

fn invalid(reason: &'static str) -> AppError {
    AppError::invalid("database", reason)
}

fn db_error(error: DbError) -> AppError {
    let code = match error {
        DbError::InvalidRequest => "invalid_request",
        DbError::PolicyDenied => "policy_denied",
        DbError::ApprovalRequired => "approval_required",
        DbError::ConnectionFailed => "connection_failed",
        DbError::AuthenticationFailed => "authentication_failed",
        DbError::TlsFailed => "tls_failed",
        DbError::QueryFailed => "query_failed",
        DbError::Timeout => "timeout",
        DbError::Cancelled => "cancelled",
        DbError::ResultLimit => "result_limit",
        DbError::SessionClosed => "session_closed",
    };
    AppError::invalid("database", code)
}

/// Session metadata plus the live write state (non-production sessions can
/// allow writes for the session).
#[derive(Debug, Clone, Serialize)]
pub struct DbLiveSession {
    #[serde(flatten)]
    pub info: DbSessionInfo,
    pub writes_allowed: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct DbQueryReviewDto {
    pub ticket: String,
    pub risk: String,
    pub reason_codes: Vec<&'static str>,
    pub statement_count: usize,
    pub expires_in_seconds: u64,
    /// A runnable read-only alternative to a refused query.
    pub suggestion: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct DbCellEditReviewDto {
    pub ticket: String,
    /// Statement text with `{{p0}}` placeholders; values stay bound.
    pub sql: String,
    pub rows: usize,
    pub expires_in_seconds: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct DbPreparedQueryDto {
    pub text: String,
    pub parameter_names: Vec<String>,
}

fn live(session: &DbSession) -> DbLiveSession {
    let mut info = session.info().clone();
    info.read_only = session.guard().is_read_only();
    DbLiveSession {
        writes_allowed: !info.read_only,
        info,
    }
}

impl AppCore {
    async fn db_credentials(
        &self,
        connection: &DbConnection,
        password: Option<SecretString>,
    ) -> AppResult<DbCredentials> {
        let (_, unlocked) = self.unlocked().await?;
        let password = match (password, connection.auth.password_secret_id) {
            (Some(password), _) => Some(password),
            (None, Some(id)) => Some(SecretString::from(
                unlocked.writer.read_secret(id).await?.value.expose_secret(),
            )),
            (None, None) => None,
        };
        let client_key_pem = match &connection.auth.client_certificate {
            Some(certificate) if certificate.key_secret_id != cc_models::ObjectId::NIL => {
                Some(SecretString::from(
                    unlocked
                        .writer
                        .read_secret(certificate.key_secret_id)
                        .await?
                        .value
                        .expose_secret(),
                ))
            }
            _ => None,
        };
        Ok(DbCredentials {
            password,
            client_key_pem,
        })
    }

    async fn db_open(
        &self,
        connection: &DbConnection,
        password: Option<SecretString>,
    ) -> AppResult<Arc<DbSession>> {
        if !matches!(
            connection.engine,
            DbEngine::Postgresql
                | DbEngine::Mysql
                | DbEngine::Mariadb
                | DbEngine::Clickhouse
                | DbEngine::Redis
        ) {
            return Err(invalid("engine_unavailable"));
        }
        let (owner, unlocked) = self.unlocked().await?;
        let credentials = self.db_credentials(connection, password).await?;
        let cancel = unlocked.db.shutdown.child_token();
        // Boxed: keeps bridge futures Send for every lifetime (rust-lang/rust#102211).
        let resolve: futures::future::BoxFuture<'_, AppResult<_>> =
            Box::pin(self.db_resolve_route(&owner, &unlocked, &connection.route, &cancel));
        let connector = resolve.await?;
        let mobile = cfg!(any(target_os = "android", target_os = "ios"));
        let timeout = Duration::from_secs(u64::from(
            connection.limits.connect_timeout_seconds.clamp(1, 120),
        ));
        let id = uuid::Uuid::new_v4().to_string();
        let open: futures::future::BoxFuture<'_, Result<DbSession, DbError>> = Box::pin(
            DbSession::open(id, connection, &credentials, connector, mobile),
        );
        let session = tokio::select! {
            biased;
            _ = cancel.cancelled() => return Err(AppError::VaultLocked),
            result = tokio::time::timeout(timeout, open) => result.map_err(|_| db_error(DbError::Timeout))?.map_err(db_error)?,
        };
        Ok(Arc::new(session))
    }

    /// Opens a session for a saved connection. `password` is used once when
    /// the connection keeps no stored password.
    pub async fn db_connect(
        &self,
        connection_id: String,
        password: Option<SecretString>,
    ) -> AppResult<DbLiveSession> {
        let (_, unlocked) = self.unlocked().await?;
        let id = crate::dto::parse_id("connection_id", &connection_id)?;
        let connection = unlocked
            .working()
            .db_connection(id)
            .ok_or_else(|| invalid("connection_unavailable"))?;
        let session = self.db_open(&connection, password).await?;
        let mut sessions = unlocked
            .db
            .sessions
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        if sessions.len() >= MAX_SESSIONS {
            session.close();
            return Err(invalid("too_many_sessions"));
        }
        sessions.insert(session.info().id.clone(), Arc::clone(&session));
        Ok(live(&session))
    }

    /// Checks an unsaved draft: opens and closes a session.
    pub async fn db_test_connection(
        &self,
        connection: DbConnection,
        password: Option<SecretString>,
    ) -> AppResult<DbLiveSession> {
        connection.validate()?;
        let session = self.db_open(&connection, password).await?;
        let info = live(&session);
        session.close();
        Ok(info)
    }

    pub async fn db_disconnect(&self, session_id: String) -> AppResult<()> {
        let (_, unlocked) = self.unlocked().await?;
        unlocked
            .db
            .queries
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .retain(|_, query| query.session_id != session_id);
        if let Some(session) = unlocked
            .db
            .sessions
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(&session_id)
        {
            session.close();
        }
        Ok(())
    }

    pub async fn db_set_session_write(
        &self,
        session_id: String,
        allow: bool,
    ) -> AppResult<DbLiveSession> {
        let (_, unlocked) = self.unlocked().await?;
        let session = unlocked.db.session(&session_id)?;
        if allow {
            session.guard().allow_session_write().map_err(db_error)?;
        } else {
            session.guard().revoke_session_write();
        }
        Ok(live(&session))
    }

    pub async fn db_tree(
        &self,
        session_id: String,
        parent: Option<DbObjectRef>,
        cursor: Option<String>,
    ) -> AppResult<DbTreePage> {
        let (_, unlocked) = self.unlocked().await?;
        let session = unlocked.db.session(&session_id)?;
        if cursor.as_ref().is_some_and(|cursor| cursor.len() > 256) {
            return Err(invalid("invalid_cursor"));
        }
        if parent.is_none() && cursor.is_none() {
            // A reload from the top also refreshes the names the editor completes.
            session.refresh_completion().await;
        }
        session
            .tree(parent.as_ref(), cursor.as_deref())
            .await
            .map_err(db_error)
    }

    /// Quoted identifier for an object (PostgreSQL "schema"."name", MySQL `db`.`name`).
    pub async fn db_identifier(
        &self,
        session_id: String,
        object: DbObjectRef,
    ) -> AppResult<String> {
        let (_, unlocked) = self.unlocked().await?;
        let engine = unlocked.db.session(&session_id)?.info().engine;
        Ok(quote_identifier(engine, &object))
    }

    pub async fn db_object_query(
        &self,
        session_id: String,
        object: DbObjectRef,
        limit: u32,
    ) -> AppResult<String> {
        let (_, unlocked) = self.unlocked().await?;
        let session = unlocked.db.session(&session_id)?;
        session.object_query(&object, limit).await.map_err(db_error)
    }

    /// Creation statement (SQL engines) or description (Redis keys, Mongo
    /// collections) of one object.
    pub async fn db_ddl(&self, session_id: String, object: DbObjectRef) -> AppResult<String> {
        let (_, unlocked) = self.unlocked().await?;
        let session = unlocked.db.session(&session_id)?;
        session.ddl(&object).await.map_err(db_error)
    }

    /// Plan of one prepared statement; `analyze` runs it after the native review.
    pub async fn db_explain(
        &self,
        session_id: String,
        request: DbQueryRequest,
        analyze: bool,
    ) -> AppResult<serde_json::Value> {
        let (_, unlocked) = self.unlocked().await?;
        let session = unlocked.db.session(&session_id)?;
        session.explain(&request, analyze).await.map_err(db_error)
    }

    pub async fn db_complete(
        &self,
        session_id: String,
        text: String,
        cursor: usize,
    ) -> AppResult<Vec<DbCompletion>> {
        let (_, unlocked) = self.unlocked().await?;
        let session = unlocked.db.session(&session_id)?;
        if text.len() > cc_db_core::MAX_QUERY_BYTES || cursor > text.len() {
            return Err(invalid("invalid_request"));
        }
        session.complete(&text, cursor).await.map_err(db_error)
    }

    pub async fn db_privileges(
        &self,
        session_id: String,
    ) -> AppResult<cc_db_core::introspect::DbPrivilegeReport> {
        let (_, unlocked) = self.unlocked().await?;
        let session = unlocked.db.session(&session_id)?;
        session.privileges().await.map_err(db_error)
    }

    pub async fn db_prepare_query(
        &self,
        session_id: String,
        text: String,
        mode: String,
        selection_start: usize,
        selection_end: usize,
    ) -> AppResult<DbPreparedQueryDto> {
        let (_, unlocked) = self.unlocked().await?;
        let engine = unlocked.db.session(&session_id)?.info().engine;
        let scope = match mode.as_str() {
            "selection" => QueryScope::Selection,
            "script" => QueryScope::Script,
            _ => QueryScope::Current,
        };
        let prepared = editor::prepare_query(engine, &text, scope, selection_start, selection_end)
            .map_err(db_error)?;
        Ok(DbPreparedQueryDto {
            text: prepared.text,
            parameter_names: prepared.parameter_names,
        })
    }

    pub async fn db_format_query(&self, engine: DbEngine, text: String) -> AppResult<String> {
        editor::format_query(engine, &text).map_err(db_error)
    }

    pub async fn db_review(
        &self,
        session_id: String,
        request: DbQueryRequest,
    ) -> AppResult<DbQueryReviewDto> {
        let (_, unlocked) = self.unlocked().await?;
        let session = unlocked.db.session(&session_id)?;
        let review = match session.guard().review(&request) {
            Ok(review) => review,
            // Refused by policy: report it as blocked with a safe alternative
            // when one exists (Redis KEYS → SCAN), not as a failure.
            Err(DbError::PolicyDenied) => {
                let hint = session.guard().refusal_hint(&request);
                return Ok(DbQueryReviewDto {
                    ticket: String::new(),
                    risk: "blocked".into(),
                    reason_codes: hint
                        .as_ref()
                        .map(|hint| vec![hint.code])
                        .unwrap_or_default(),
                    statement_count: 0,
                    expires_in_seconds: 0,
                    suggestion: hint.map(|hint| hint.suggestion),
                });
            }
            Err(error) => return Err(db_error(error)),
        };
        Ok(DbQueryReviewDto {
            ticket: review.ticket.clone(),
            risk: serde_json::to_value(review.risk)
                .ok()
                .and_then(|v| v.as_str().map(str::to_owned))
                .unwrap_or_default(),
            reason_codes: review.reason_codes.clone(),
            statement_count: review.statement_count,
            expires_in_seconds: review.expires_in_seconds,
            suggestion: None,
        })
    }

    /// Starts a query. A request carrying a review ticket was confirmed by the
    /// user in the protected review dialog; the ticket is bound to the exact text.
    pub async fn db_execute(
        &self,
        session_id: String,
        request: DbQueryRequest,
    ) -> AppResult<String> {
        let (_, unlocked) = self.unlocked().await?;
        let session = unlocked.db.session(&session_id)?;
        if let Some(ticket) = &request.approval_ticket {
            session.guard().approve(ticket).map_err(db_error)?;
        }
        let stream = session.execute(&request).map_err(db_error)?;
        let info = session.info();
        let connection_id = crate::dto::parse_id("connection_id", &info.connection_id)?;
        let mut queries = unlocked
            .db
            .queries
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        if queries.len() >= MAX_QUERIES {
            stream.cancel();
            return Err(invalid("too_many_queries"));
        }
        let id = uuid::Uuid::new_v4().to_string();
        {
            let mut kept = unlocked
                .db
                .results
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            while kept.1.len() >= MAX_QUERIES {
                if let Some(oldest) = kept.1.pop_front() {
                    kept.0.remove(&oldest);
                }
            }
            kept.0.insert(
                id.clone(),
                Arc::new(QueryResults {
                    session_id: session_id.clone(),
                    request: DbQueryRequest {
                        text: request.text.clone(),
                        parameters: request.parameters.clone(),
                        approval_ticket: None,
                    },
                    sets: Mutex::new(Vec::new()),
                    bytes: Default::default(),
                }),
            );
            kept.1.push_back(id.clone());
        }
        queries.insert(
            id.clone(),
            Arc::new(LiveQuery {
                session_id,
                stream: tokio::sync::Mutex::new(stream),
                connection_id,
                engine: info.engine,
                text: cc_models::secret::SecretValue::new(request.text.clone()),
                started: Instant::now(),
                rows: AtomicU64::new(0),
                recorded: AtomicBool::new(false),
            }),
        );
        Ok(id)
    }

    /// Next events of a query (waits up to `wait_ms` for the first one).
    /// A terminal event (done, cancelled, error) releases the query.
    pub async fn db_poll(&self, query_id: String, wait_ms: u64) -> AppResult<Vec<DbQueryEvent>> {
        let (owner, unlocked) = self.unlocked().await?;
        let query = unlocked.db.query(&query_id)?;
        let mut stream = query.stream.lock().await;
        let mut events = Vec::new();
        let first = tokio::time::timeout(
            Duration::from_millis(wait_ms.min(2000)),
            stream.events.recv(),
        )
        .await;
        let mut terminal = false;
        match first {
            Ok(Some(event)) => events.push(event),
            Ok(None) => terminal = true,
            Err(_) => return Ok(events),
        }
        while events.len() < 16 {
            match stream.events.try_recv() {
                Ok(event) => events.push(event),
                Err(tokio::sync::mpsc::error::TryRecvError::Empty) => break,
                Err(tokio::sync::mpsc::error::TryRecvError::Disconnected) => {
                    terminal = true;
                    break;
                }
            }
        }
        terminal |= events.iter().any(|event| {
            matches!(
                event,
                DbQueryEvent::Done | DbQueryEvent::Cancelled | DbQueryEvent::Error { .. }
            )
        });
        drop(stream);
        let rows: usize = events
            .iter()
            .map(|event| match event {
                DbQueryEvent::Rows { rows, .. } => rows.len(),
                _ => 0,
            })
            .sum();
        query.rows.fetch_add(rows as u64, Ordering::Relaxed);
        if let Ok(kept) = unlocked.db.results(&query_id) {
            kept.keep(&events);
        }
        if terminal {
            unlocked
                .db
                .queries
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .remove(&query_id);
            if !query.recorded.swap(true, Ordering::AcqRel) {
                let error_code = events.iter().find_map(|event| match event {
                    DbQueryEvent::Error { code, .. } => Some(code.clone()),
                    DbQueryEvent::Cancelled => Some("cancelled".to_owned()),
                    _ => None,
                });
                let entry = cc_models::history::DbHistoryEntry {
                    id: cc_models::ObjectId::new(),
                    connection_id: query.connection_id,
                    engine: query.engine,
                    text: query.text.clone(),
                    created_at: chrono::Utc::now(),
                    elapsed_ms: u64::try_from(query.started.elapsed().as_millis())
                        .unwrap_or(u64::MAX),
                    rows: query.rows.load(Ordering::Relaxed),
                    error_code,
                    favorite: false,
                };
                // History is best effort: a full or locked store never fails the query.
                let _ = self.db_record_query(&owner, &unlocked, entry).await;
            }
        }
        Ok(events)
    }

    /// Formats loaded rows of one result (all or the selected rows/columns).
    pub async fn db_copy_result(
        &self,
        query_id: String,
        result_index: u32,
        rows: Option<Vec<u32>>,
        columns: Option<Vec<u32>>,
        options: ExportOptions,
    ) -> AppResult<String> {
        let (_, unlocked) = self.unlocked().await?;
        let kept = unlocked.db.results(&query_id)?;
        let engine = unlocked.db.session(&kept.session_id)?.info().engine;
        let sets = kept.sets.lock().unwrap_or_else(|e| e.into_inner());
        let set = sets
            .get(result_index as usize)
            .ok_or_else(|| invalid("result_unavailable"))?;
        let picked_columns: Vec<usize> = match &columns {
            Some(columns) => columns.iter().map(|c| *c as usize).collect(),
            None => (0..set.columns.len()).collect(),
        };
        if picked_columns.iter().any(|c| *c >= set.columns.len()) {
            return Err(invalid("result_unavailable"));
        }
        let pick = |row: &Vec<DbCell>| -> Vec<DbCell> {
            picked_columns.iter().map(|c| row[*c].clone()).collect()
        };
        let selected: Vec<Vec<DbCell>> = match &rows {
            Some(rows) => rows
                .iter()
                .map(|r| set.rows.get(*r as usize).map(pick))
                .collect::<Option<_>>()
                .ok_or_else(|| invalid("result_unavailable"))?,
            None => set.rows.iter().map(pick).collect(),
        };
        let column_list: Vec<DbColumn> = picked_columns
            .iter()
            .map(|c| set.columns[*c].clone())
            .collect();
        format_rows(engine, &column_list, &selected, set.table.as_ref(), options).map_err(db_error)
    }

    /// Starts streaming one result of a read-only query into `path`.
    pub async fn db_export_start(
        &self,
        query_id: String,
        result_index: u32,
        options: ExportOptions,
        path: String,
    ) -> AppResult<String> {
        let (_, unlocked) = self.unlocked().await?;
        let kept = unlocked.db.results(&query_id)?;
        let session = unlocked.db.session(&kept.session_id)?;
        let path = std::path::PathBuf::from(path);
        if !path.is_absolute() || !path.parent().is_some_and(std::path::Path::is_dir) {
            return Err(invalid("export_path_invalid"));
        }
        let id = uuid::Uuid::new_v4().to_string();
        let progress = Arc::new(ExportProgress::default());
        let cancel = unlocked.db.shutdown.child_token();
        unlocked
            .db
            .exports
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(
                id.clone(),
                RunningExport {
                    progress: progress.clone(),
                    cancel: cancel.clone(),
                },
            );
        let request = kept.request.clone();
        tokio::spawn(async move {
            let export: futures::future::BoxFuture<'_, Result<(), DbError>> =
                Box::pin(session.export(
                    &request,
                    result_index,
                    options,
                    path.clone(),
                    progress.clone(),
                    cancel.clone(),
                ));
            let result = export.await;
            match result {
                Ok(()) => {}
                Err(DbError::Cancelled) => {
                    progress.cancelled.store(true, Ordering::Release);
                    let _ = tokio::fs::remove_file(&path).await;
                }
                Err(error) => {
                    *progress.error.lock().unwrap_or_else(|e| e.into_inner()) = Some(match error {
                        DbError::PolicyDenied => "export_requires_read_only",
                        DbError::InvalidRequest => "export_path_invalid",
                        _ => "export_failed",
                    });
                    let _ = tokio::fs::remove_file(&path).await;
                }
            }
            progress.done.store(true, Ordering::Release);
        });
        Ok(id)
    }

    /// Progress of an export; a finished export is forgotten once reported.
    pub async fn db_export_progress(&self, export_id: String) -> AppResult<serde_json::Value> {
        let (_, unlocked) = self.unlocked().await?;
        let mut exports = unlocked
            .db
            .exports
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let export = exports
            .get(&export_id)
            .ok_or_else(|| invalid("export_closed"))?;
        let mut snapshot = export.progress.snapshot();
        snapshot["id"] = serde_json::Value::String(export_id.clone());
        if export.progress.done.load(Ordering::Acquire) {
            exports.remove(&export_id);
        }
        Ok(snapshot)
    }

    pub async fn db_export_cancel(&self, export_id: String) -> AppResult<()> {
        let (_, unlocked) = self.unlocked().await?;
        if let Some(export) = unlocked
            .db
            .exports
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(&export_id)
        {
            export.cancel.cancel();
        }
        Ok(())
    }

    /// Generated `UPDATE` statements for grid edits, reviewed like any write.
    pub async fn db_review_cell_edits(
        &self,
        session_id: String,
        table: DbObjectRef,
        edits: Vec<DbCellEdit>,
    ) -> AppResult<DbCellEditReviewDto> {
        let (_, unlocked) = self.unlocked().await?;
        let session = unlocked.db.session(&session_id)?;
        let (request, rows) = session
            .review_cell_edits(&table, &edits)
            .await
            .map_err(db_error)?;
        let review = session.guard().review(&request).map_err(db_error)?;
        let ticket = review.ticket.clone();
        let mut pending = unlocked.db.edits.lock().unwrap_or_else(|e| e.into_inner());
        if pending.len() >= MAX_QUERIES {
            pending.clear();
        }
        let sql = request.text.clone();
        pending.insert(ticket.clone(), (session_id, request));
        Ok(DbCellEditReviewDto {
            ticket,
            sql,
            rows,
            expires_in_seconds: review.expires_in_seconds,
        })
    }

    /// Applies reviewed edits in one transaction; returns the changed rows.
    pub async fn db_commit_cell_edits(&self, session_id: String, ticket: String) -> AppResult<u64> {
        let (_, unlocked) = self.unlocked().await?;
        let (owner, mut request) = unlocked
            .db
            .edits
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(&ticket)
            .ok_or_else(|| invalid("edits_expired"))?;
        if owner != session_id {
            return Err(invalid("edits_expired"));
        }
        let session = unlocked.db.session(&session_id)?;
        session.guard().approve(&ticket).map_err(db_error)?;
        request.approval_ticket = Some(ticket);
        session.apply_cell_edits(&request).await.map_err(db_error)
    }

    pub async fn db_discard_cell_edits(&self, ticket: String) -> AppResult<()> {
        let (_, unlocked) = self.unlocked().await?;
        unlocked
            .db
            .edits
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(&ticket);
        Ok(())
    }

    pub async fn db_load_more(&self, query_id: String, rows: u32) -> AppResult<()> {
        let (_, unlocked) = self.unlocked().await?;
        let query = unlocked.db.query(&query_id)?;
        let stream = query.stream.lock().await;
        stream
            .control()
            .load_more(rows.clamp(1, 1000))
            .map_err(db_error)
    }

    pub async fn db_cancel(&self, query_id: String) -> AppResult<()> {
        let (_, unlocked) = self.unlocked().await?;
        if let Ok(query) = unlocked.db.query(&query_id) {
            query.stream.lock().await.cancel();
        }
        Ok(())
    }

    pub async fn db_release(&self, query_id: String) -> AppResult<()> {
        let (_, unlocked) = self.unlocked().await?;
        {
            let mut kept = unlocked
                .db
                .results
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            kept.0.remove(&query_id);
            kept.1.retain(|id| id != &query_id);
        }
        if let Some(query) = unlocked
            .db
            .queries
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(&query_id)
        {
            if let Ok(stream) = query.stream.try_lock() {
                stream.cancel();
            }
        }
        Ok(())
    }
}
