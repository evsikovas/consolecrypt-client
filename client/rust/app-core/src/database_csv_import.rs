//! CSV import into a table of a live session: preview, type check, review
//! and a background run with progress and cancellation. The file is read on
//! this device and its rows go only to the target database.
use crate::{
    database_runtime::{db_error, invalid},
    AppCore, AppResult,
};
use cc_db_core::{
    csv_import::{
        self, CsvColumnCheck, CsvImportPlan, CsvImportProgress, CsvMapping, CsvOptions, CsvPreview,
    },
    DbError, DbObjectRef, DbQueryRequest,
};
use serde::Serialize;
use std::{
    path::PathBuf,
    sync::{atomic::Ordering, Arc},
    time::Instant,
};
use tokio_util::sync::CancellationToken;

/// A reviewed import waiting for its start: session, plan, file and the
/// template the user approved.
pub(crate) struct PendingImport {
    session_id: String,
    plan: CsvImportPlan,
    path: PathBuf,
    request: DbQueryRequest,
}

pub(crate) struct RunningImport {
    progress: Arc<CsvImportProgress>,
    cancel: CancellationToken,
}

impl RunningImport {
    pub(crate) fn cancel(&self) {
        self.cancel.cancel();
    }
}

/// The review of an import: the statements as they will run (one row of
/// placeholders) and the guard's risk.
#[derive(Debug, Clone, Serialize)]
pub struct DbCsvImportReviewDto {
    pub ticket: String,
    pub sql: String,
    pub risk: cc_db_core::guard::QueryRisk,
    pub expires_in_seconds: u64,
}

fn file(path: &str) -> AppResult<PathBuf> {
    let path = PathBuf::from(path);
    if !path.is_absolute() || !path.is_file() {
        return Err(invalid("import_path_invalid"));
    }
    Ok(path)
}

async fn blocking<T: Send + 'static>(
    work: impl FnOnce() -> Result<T, DbError> + Send + 'static,
) -> AppResult<T> {
    tokio::task::spawn_blocking(work)
        .await
        .map_err(|_| invalid("import_failed"))?
        .map_err(|error| match error {
            DbError::ResultLimit => invalid("import_too_large"),
            DbError::InvalidRequest => invalid("import_unreadable"),
            other => db_error(other),
        })
}

impl AppCore {
    /// The first rows of a CSV file with the given options.
    pub async fn db_import_preview(
        &self,
        path: String,
        options: CsvOptions,
    ) -> AppResult<CsvPreview> {
        self.unlocked().await?;
        let path = file(&path)?;
        blocking(move || csv_import::preview(&path, &options)).await
    }

    /// A simple `CREATE TABLE` typed from the file's first rows.
    pub async fn db_import_create_table(
        &self,
        path: String,
        options: CsvOptions,
        engine: cc_models::database::DbEngine,
        table: DbObjectRef,
    ) -> AppResult<String> {
        self.unlocked().await?;
        let path = file(&path)?;
        blocking(move || csv_import::create_table_sql(&path, &options, engine, &table)).await
    }

    /// Values of the first rows that do not convert to the mapped columns.
    pub async fn db_import_check(
        &self,
        session_id: String,
        table: DbObjectRef,
        path: String,
        options: CsvOptions,
        mapping: Vec<CsvMapping>,
    ) -> AppResult<Vec<CsvColumnCheck>> {
        let (_, unlocked) = self.unlocked().await?;
        let session = unlocked.db.session(&session_id)?;
        let columns = session.edit_columns(&table).await.map_err(db_error)?;
        let path = file(&path)?;
        blocking(move || csv_import::check(&path, &options, &mapping, &columns)).await
    }

    /// Reviews an import like any write: the template statements go
    /// through the guard and wait for the user's approval.
    pub async fn db_import_review(
        &self,
        session_id: String,
        path: String,
        plan: CsvImportPlan,
    ) -> AppResult<DbCsvImportReviewDto> {
        let (_, unlocked) = self.unlocked().await?;
        let session = unlocked.db.session(&session_id)?;
        let path = file(&path)?;
        let request = session.import_template(&plan).await.map_err(db_error)?;
        let review = session.guard().review(&request).map_err(db_error)?;
        let sql = request.text.replace("{{p", "{p").replace("}}", "}");
        let mut pending = unlocked
            .db
            .pending_imports
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        if pending.len() >= 16 {
            pending.clear();
        }
        pending.insert(
            review.ticket.clone(),
            PendingImport {
                session_id,
                plan,
                path,
                request,
            },
        );
        Ok(DbCsvImportReviewDto {
            ticket: review.ticket,
            sql,
            risk: review.risk,
            expires_in_seconds: review.expires_in_seconds,
        })
    }

    pub async fn db_import_discard(&self, ticket: String) -> AppResult<()> {
        let (_, unlocked) = self.unlocked().await?;
        unlocked
            .db
            .pending_imports
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(&ticket);
        Ok(())
    }

    /// Starts an approved import in the background; returns its id.
    pub async fn db_import_start(&self, session_id: String, ticket: String) -> AppResult<String> {
        let (owner, unlocked) = self.unlocked().await?;
        let pending = unlocked
            .db
            .pending_imports
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(&ticket)
            .ok_or_else(|| invalid("import_expired"))?;
        if pending.session_id != session_id {
            return Err(invalid("import_expired"));
        }
        let session = unlocked.db.session(&session_id)?;
        session.guard().approve(&ticket).map_err(db_error)?;
        let mut request = pending.request;
        request.approval_ticket = Some(ticket);
        let id = uuid::Uuid::new_v4().to_string();
        let progress = Arc::new(CsvImportProgress::default());
        let cancel = unlocked.db.cancellation().child_token();
        unlocked
            .db
            .imports
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(
                id.clone(),
                RunningImport {
                    progress: progress.clone(),
                    cancel: cancel.clone(),
                },
            );
        let mut report = pending.path.clone().into_os_string();
        report.push(".errors.csv");
        let core = self.clone();
        tokio::spawn(async move {
            let started = Instant::now();
            let run: futures::future::BoxFuture<'_, Result<(), DbError>> =
                Box::pin(session.import_csv(
                    &pending.path,
                    &pending.plan,
                    &request,
                    progress.clone(),
                    cancel.clone(),
                    Some(PathBuf::from(report)),
                ));
            let result = run.await;
            match &result {
                Ok(()) => {}
                Err(DbError::Cancelled) => progress.cancelled.store(true, Ordering::Release),
                Err(error) => {
                    let mut slot = progress.error.lock().unwrap_or_else(|e| e.into_inner());
                    if slot.is_none() {
                        *slot = Some(crate::database_runtime::db_error_code(error).to_owned());
                    }
                }
            }
            if session.info().production {
                // Production imports are written to history like production
                // queries: the reviewed template, never the values.
                if let Ok(connection_id) =
                    crate::dto::parse_id("connection_id", &session.info().connection_id)
                {
                    let entry = cc_models::history::DbHistoryEntry {
                        id: cc_models::ObjectId::new(),
                        connection_id,
                        engine: session.info().engine,
                        text: cc_models::secret::SecretValue::new(request.text.clone()),
                        created_at: chrono::Utc::now(),
                        elapsed_ms: u64::try_from(started.elapsed().as_millis())
                            .unwrap_or(u64::MAX),
                        rows: progress.imported.load(Ordering::Acquire),
                        error_code: result.as_ref().err().map(|_| "import_failed".to_owned()),
                        favorite: false,
                        production_write: true,
                    };
                    if let Ok((_, unlocked)) = core.unlocked().await {
                        let _ = core.db_record_query(&owner, &unlocked, entry).await;
                    }
                }
            }
            progress.done.store(true, Ordering::Release);
        });
        Ok(id)
    }

    /// Progress of an import; a finished import is forgotten once reported.
    pub async fn db_import_progress(&self, import_id: String) -> AppResult<serde_json::Value> {
        let (_, unlocked) = self.unlocked().await?;
        let mut imports = unlocked
            .db
            .imports
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let import = imports
            .get(&import_id)
            .ok_or_else(|| invalid("import_closed"))?;
        let mut snapshot = import.progress.snapshot();
        snapshot["id"] = serde_json::Value::String(import_id.clone());
        if import.progress.done.load(Ordering::Acquire) {
            imports.remove(&import_id);
        }
        Ok(snapshot)
    }

    pub async fn db_import_cancel(&self, import_id: String) -> AppResult<()> {
        let (_, unlocked) = self.unlocked().await?;
        if let Some(import) = unlocked
            .db
            .imports
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(&import_id)
        {
            import.cancel();
        }
        Ok(())
    }
}
