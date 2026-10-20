//! A query owns one server cursor, at most two queued events and one bounded
//! row batch. Loading another page adds demand; it never executes SQL again.
use crate::{
    DbCell, DbColumn, DbError, DbObjectRef, DbQueryEvent, MAX_EVENT_BYTES, MAX_EVENT_ROWS,
};
use async_trait::async_trait;
use std::fmt;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

pub struct DbQueryStream {
    pub events: mpsc::Receiver<DbQueryEvent>,
    control: DbQueryControl,
}
impl fmt::Debug for DbQueryStream {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("DbQueryStream([SCOPED])")
    }
}
impl DbQueryStream {
    pub fn control(&self) -> DbQueryControl {
        self.control.clone()
    }
    pub fn cancel(&self) {
        self.control.cancel();
    }
}
impl Drop for DbQueryStream {
    fn drop(&mut self) {
        self.control.cancel();
    }
}

#[derive(Clone)]
pub struct DbQueryControl {
    cancel: CancellationToken,
    demand: mpsc::Sender<u32>,
}
impl fmt::Debug for DbQueryControl {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("DbQueryControl([SCOPED])")
    }
}
impl DbQueryControl {
    pub fn cancel(&self) {
        self.cancel.cancel();
    }
    pub fn load_more(&self, rows: u32) -> Result<(), DbError> {
        if rows == 0 || rows > 1000 {
            return Err(DbError::InvalidRequest);
        }
        if self.cancel.is_cancelled() {
            return Err(DbError::Cancelled);
        }
        self.demand
            .try_send(rows)
            .map_err(|_| DbError::InvalidRequest)
    }
}

#[async_trait]
pub(crate) trait RowConsumer: Send {
    async fn start(
        &mut self,
        columns: Vec<DbColumn>,
        editable_table: Option<DbObjectRef>,
    ) -> Result<(), DbError>;
    /// false means the consumer reached its byte budget; stop the backend cursor.
    async fn row(&mut self, row: Vec<DbCell>) -> Result<bool, DbError>;
    async fn statement_done(
        &mut self,
        elapsed_ms: u64,
        affected_rows: Option<u64>,
    ) -> Result<(), DbError>;
}

pub(crate) struct QuerySink {
    events: mpsc::Sender<DbQueryEvent>,
    demand: mpsc::Receiver<u32>,
    pub cancel: CancellationToken,
    initial_rows: u32,
    available_rows: u32,
    loaded_rows: u64,
    result_index: u32,
    maximum_bytes: usize,
    consumed_bytes: usize,
    batch: Vec<Vec<DbCell>>,
    batch_bytes: usize,
    truncated: bool,
}
impl fmt::Debug for QuerySink {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("QuerySink([SCOPED])")
    }
}
impl QuerySink {
    pub fn channel(
        initial_rows: u32,
        maximum_bytes: usize,
        scope: CancellationToken,
    ) -> (DbQueryStream, Self) {
        let cancel = scope.child_token();
        let (events, receiver) = mpsc::channel(2);
        let (sender, demand) = mpsc::channel(1);
        let stream = DbQueryStream {
            events: receiver,
            control: DbQueryControl {
                cancel: cancel.clone(),
                demand: sender,
            },
        };
        let sink = Self {
            events,
            demand,
            cancel,
            initial_rows,
            available_rows: initial_rows,
            loaded_rows: 0,
            result_index: 0,
            maximum_bytes,
            consumed_bytes: 0,
            batch: Vec::new(),
            batch_bytes: 0,
            truncated: false,
        };
        (stream, sink)
    }

    async fn emit(&self, event: DbQueryEvent) -> Result<(), DbError> {
        tokio::select! {
            biased;
            _ = self.cancel.cancelled() => Err(DbError::Cancelled),
            result = self.events.send(event) => result.map_err(|_| DbError::Cancelled),
        }
    }

    async fn flush(&mut self) -> Result<(), DbError> {
        if self.batch.is_empty() {
            return Ok(());
        }
        let rows = std::mem::take(&mut self.batch);
        self.batch_bytes = 0;
        self.emit(DbQueryEvent::Rows {
            result_index: self.result_index,
            rows,
        })
        .await
    }

    pub async fn finish(mut self, result: Result<(), DbError>) {
        self.batch.clear();
        let event = match result {
            Ok(()) => DbQueryEvent::Done,
            Err(DbError::Cancelled | DbError::SessionClosed) => DbQueryEvent::Cancelled,
            Err(error) => DbQueryEvent::Error {
                code: error_code(&error).into(),
                message: error.to_string(),
            },
        };
        // Cancellation must not retain old result events while waiting for a
        // paused UI. Session-level runtime drops/drains its receiver on lock.
        if self.cancel.is_cancelled() {
            let _ = self.events.try_send(event);
        } else {
            let _ = self.emit(event).await;
        }
    }
}
#[async_trait]
impl RowConsumer for QuerySink {
    async fn start(
        &mut self,
        columns: Vec<DbColumn>,
        editable_table: Option<DbObjectRef>,
    ) -> Result<(), DbError> {
        if columns.len() > 4096 || bounded_size(&columns)? > MAX_EVENT_BYTES / 2 {
            return Err(DbError::ResultLimit);
        }
        self.available_rows = self.initial_rows;
        self.loaded_rows = 0;
        self.truncated = false;
        self.emit(DbQueryEvent::Started {
            result_index: self.result_index,
            columns,
            editable_table,
        })
        .await
    }

    async fn row(&mut self, row: Vec<DbCell>) -> Result<bool, DbError> {
        let size = bounded_size(&row)?;
        if size > MAX_EVENT_BYTES - 512
            || self.consumed_bytes.saturating_add(size) > self.maximum_bytes
        {
            self.truncated = true;
            self.flush().await?;
            return Ok(false);
        }
        if self.available_rows == 0 {
            self.flush().await?;
            self.emit(DbQueryEvent::Paused {
                result_index: self.result_index,
                loaded_rows: self.loaded_rows,
                has_more: true,
            })
            .await?;
            self.available_rows = tokio::select! {
                biased;
                _ = self.cancel.cancelled() => return Err(DbError::Cancelled),
                count = self.demand.recv() => count.ok_or(DbError::Cancelled)?,
            };
        }
        if self.batch.len() >= MAX_EVENT_ROWS
            || self.batch_bytes.saturating_add(size) > MAX_EVENT_BYTES - 512
        {
            self.flush().await?;
        }
        self.consumed_bytes += size;
        self.batch_bytes += size + 1;
        self.batch.push(row);
        self.loaded_rows += 1;
        self.available_rows -= 1;
        if self.batch.len() >= MAX_EVENT_ROWS {
            self.flush().await?;
        }
        Ok(true)
    }

    async fn statement_done(
        &mut self,
        elapsed_ms: u64,
        affected_rows: Option<u64>,
    ) -> Result<(), DbError> {
        self.flush().await?;
        self.emit(DbQueryEvent::StatementDone {
            result_index: self.result_index,
            elapsed_ms,
            affected_rows,
            truncated: self.truncated,
        })
        .await?;
        self.result_index = self
            .result_index
            .checked_add(1)
            .ok_or(DbError::ResultLimit)?;
        Ok(())
    }
}

fn bounded_size(value: &impl serde::Serialize) -> Result<usize, DbError> {
    struct Counter(usize);
    impl std::io::Write for Counter {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            self.0 = self.0.saturating_add(bytes.len());
            if self.0 > MAX_EVENT_BYTES {
                return Err(std::io::ErrorKind::OutOfMemory.into());
            }
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let mut counter = Counter(0);
    serde_json::to_writer(&mut counter, value).map_err(|_| DbError::ResultLimit)?;
    Ok(counter.0)
}

pub(crate) fn error_code(error: &DbError) -> &'static str {
    match error {
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
    }
}
