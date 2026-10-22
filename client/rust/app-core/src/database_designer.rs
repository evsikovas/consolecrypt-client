//! Table designer: the dialect catalogue, planning DDL from a design, the
//! current structure of a table, and applying a reviewed designer script.
//! The script is approved like any changing query (review ticket bound to
//! the exact text, vault passphrase in the client, production write window).
use crate::{database_runtime::db_error, AppCore, AppError, AppResult};
use cc_db_core::{
    designer::{self, DesignApplyResult, DesignPlan, DialectInfo, TableDesign},
    DbObjectRef, DbQueryRequest,
};
use cc_models::database::DbEngine;
use std::time::Instant;

impl AppCore {
    /// What the designer offers for `engine` (types, options, features).
    pub fn db_design_dialect(&self, engine: DbEngine) -> AppResult<DialectInfo> {
        designer::dialect(engine)
            .map(|dialect| dialect.info())
            .ok_or_else(|| AppError::invalid("database", "designer_unsupported"))
    }

    /// Validates a design and renders its statements: CREATE without
    /// `original`, ALTER from the loaded `original` otherwise. Pure; no
    /// session needed.
    pub fn db_design_plan(
        &self,
        engine: DbEngine,
        original: Option<TableDesign>,
        design: TableDesign,
    ) -> DesignPlan {
        designer::plan(engine, original.as_ref(), &design)
    }

    /// Current structure of a table, for alter mode.
    pub async fn db_table_design(
        &self,
        session_id: String,
        table: DbObjectRef,
    ) -> AppResult<TableDesign> {
        let (_, unlocked) = self.unlocked().await?;
        let session = unlocked.db.session(&session_id)?;
        session.table_design(&table).await.map_err(db_error)
    }

    /// Runs a designer script the user confirmed in the protected review
    /// dialog (the request carries its review ticket). Production changes
    /// are written to history like production queries.
    pub async fn db_apply_design(
        &self,
        session_id: String,
        request: DbQueryRequest,
    ) -> AppResult<DesignApplyResult> {
        let (owner, unlocked) = self.unlocked().await?;
        let session = unlocked.db.session(&session_id)?;
        let ticket = request
            .approval_ticket
            .as_deref()
            .ok_or_else(|| AppError::invalid("database", "approval_required"))?;
        session.guard().approve(ticket).map_err(db_error)?;
        let started = Instant::now();
        let result = session.apply_design(&request).await.map_err(db_error)?;
        let info = session.info();
        let entry = cc_models::history::DbHistoryEntry {
            id: cc_models::ObjectId::new(),
            connection_id: crate::dto::parse_id("connection_id", &info.connection_id)?,
            engine: info.engine,
            text: cc_models::secret::SecretValue::new(request.text.clone()),
            created_at: chrono::Utc::now(),
            elapsed_ms: u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX),
            rows: 0,
            error_code: result
                .statements
                .iter()
                .find_map(|statement| statement.error_code.map(str::to_owned)),
            favorite: false,
            production_write: info.production,
        };
        // History is best effort: a full or locked store never fails the change.
        let _ = self.db_record_query(&owner, &unlocked, entry).await;
        Ok(result)
    }
}
