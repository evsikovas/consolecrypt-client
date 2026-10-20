//! Native database AI consent and read-only tool plans. There are deliberately
//! no mutation tools and no executable model-generated command path here.
use crate::guard::{DbGuard, GuardedStatement};
use crate::types::{DbError, DbQueryRequest};
use cc_models::ObjectId;
use serde::Serialize;
use std::collections::BTreeMap;
use std::sync::{atomic::{AtomicBool, Ordering}, Arc, Mutex};
use std::time::{Duration, Instant};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

mod identifiers;
pub use identifiers::{AiColumn, AiSchema, AiTable, IdentifierPseudonyms};

const REVIEW_TTL: Duration = Duration::from_secs(60);
const CONSENT_TTL: Duration = Duration::from_secs(15 * 60);
pub const TOOL_TIMEOUT: Duration = Duration::from_secs(10);

/// Created from the active native Vault and provider, never deserialized from
/// synchronized settings. Reopening on a different revision creates a new
/// consent identity even if the visible connection name did not change.
#[derive(Clone, PartialEq, Eq)]
pub struct AiScope {
    pub profile_id: ObjectId,
    pub connection_id: ObjectId,
    pub connection_revision: String,
    pub provider_id: ObjectId,
}

#[derive(Clone, Serialize)]
pub struct ConsentReview {
    pub ticket: String,
    pub schema: bool,
    pub rows: bool,
    pub expires_in_seconds: u64,
}

struct PendingConsent {
    schema: bool,
    rows: bool,
    epoch: u64,
    expires: Instant,
}

#[derive(Default)]
struct State {
    epoch: u64,
    schema: Option<Instant>,
    rows: Option<Instant>,
    pending: BTreeMap<String, PendingConsent>,
}

pub struct DbAiConsent {
    scope: AiScope,
    identity: Uuid,
    cancel: CancellationToken,
    state: Mutex<State>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ReadTool {
    FindTables,
    DescribeTable,
    ListSchemas,
    ListTables,
    Explain,
    SampleRows,
    RunReadonly,
}

impl ReadTool {
    fn needs_rows(self) -> bool {
        matches!(self, Self::SampleRows | Self::RunReadonly)
    }
}

/// Opaque metadata capability. Call validate immediately before the native
/// catalog/explain/sampling operation and recheck consent before publishing its
/// output to a provider. Query execution also requires its DbGuard permit.
pub struct AiToolPlan {
    identity: Uuid,
    epoch: u64,
    tool: ReadTool,
    row_limit: u32,
    consumed: AtomicBool,
    statement: Option<GuardedStatement>,
}

impl AiToolPlan {
    pub fn tool(&self) -> ReadTool { self.tool }
    pub fn row_limit(&self) -> u32 { self.row_limit }
    pub fn timeout(&self) -> Duration { TOOL_TIMEOUT }
    pub fn statement(&self) -> Option<&GuardedStatement> { self.statement.as_ref() }
    pub fn into_statement(self) -> Option<GuardedStatement> { self.statement }
}

impl DbAiConsent {
    pub fn new(scope: AiScope, cancel: CancellationToken) -> Self {
        Self { scope, identity: Uuid::new_v4(), cancel, state: Mutex::new(State::default()) }
    }

    pub fn scope(&self) -> &AiScope { &self.scope }

    pub fn review(&self, schema: bool, rows: bool) -> Result<ConsentReview, DbError> {
        self.live()?;
        if !schema && !rows { return Err(DbError::InvalidRequest); }
        let mut state = self.state.lock().map_err(|_| DbError::SessionClosed)?;
        self.live()?;
        let now = Instant::now();
        state.pending.retain(|_, consent| consent.expires > now);
        if state.pending.len() >= 16 { return Err(DbError::ResultLimit); }
        let ticket = Uuid::new_v4().to_string();
        let epoch = state.epoch;
        state.pending.insert(ticket.clone(), PendingConsent { schema, rows, epoch, expires: now + REVIEW_TTL });
        Ok(ConsentReview { ticket, schema, rows, expires_in_seconds: REVIEW_TTL.as_secs() })
    }

    /// Called only by the explicit UI confirmation handler. The one-use ticket
    /// already fixes which separate checkboxes were reviewed.
    pub fn approve(&self, ticket: &str) -> Result<(), DbError> {
        self.live()?;
        let mut state = self.state.lock().map_err(|_| DbError::SessionClosed)?;
        let consent = state.pending.remove(ticket).ok_or(DbError::ApprovalRequired)?;
        let now = Instant::now();
        if consent.epoch != state.epoch || consent.expires <= now { return Err(DbError::ApprovalRequired); }
        self.live()?;
        if consent.schema { state.schema = Some(now + CONSENT_TTL); }
        if consent.rows { state.rows = Some(now + CONSENT_TTL); }
        Ok(())
    }

    /// Use after every await, including provider output, to fence revocation.
    /// Result explanation can request rows without sending schema.
    pub fn check(&self, scope: &AiScope, schema: bool, rows: bool) -> Result<(), DbError> {
        self.live()?;
        if &self.scope != scope { return Err(DbError::PolicyDenied); }
        let state = self.state.lock().map_err(|_| DbError::SessionClosed)?;
        self.check_state(&state, schema, rows)
    }

    pub fn revoke(&self) {
        if let Ok(mut state) = self.state.lock() {
            state.epoch = state.epoch.wrapping_add(1);
            state.schema = None;
            state.rows = None;
            state.pending.clear();
        }
    }

    pub fn close(&self) {
        self.cancel.cancel();
        self.revoke();
    }

    pub fn prepare_metadata(&self, tool: ReadTool, row_limit: u32) -> Result<AiToolPlan, DbError> {
        if matches!(tool, ReadTool::RunReadonly | ReadTool::Explain) {
            return Err(DbError::InvalidRequest);
        }
        let maximum = if tool == ReadTool::SampleRows { 20 } else { 0 };
        if (maximum == 0 && row_limit != 0) || (maximum > 0 && !(1..=maximum).contains(&row_limit)) {
            return Err(DbError::InvalidRequest);
        }
        self.plan(tool, row_limit, None)
    }

    pub fn prepare_query(
        &self,
        guard: &Arc<DbGuard>,
        tool: ReadTool,
        query: &DbQueryRequest,
        row_limit: u32,
    ) -> Result<AiToolPlan, DbError> {
        if !matches!(tool, ReadTool::RunReadonly | ReadTool::Explain)
            || guard.policy().connection_id != self.scope.connection_id
            || guard.policy().connection_revision != self.scope.connection_revision
            || query.approval_ticket.is_some()
            || (tool == ReadTool::RunReadonly && !(1..=50).contains(&row_limit))
            || (tool == ReadTool::Explain && row_limit != 0)
        {
            return Err(DbError::PolicyDenied);
        }
        self.check(&self.scope, true, tool.needs_rows())?;
        let mut statements = guard.prepare(query)?;
        if statements.len() != 1 || !statements[0].is_read_only()
            || !statements[0].requires_server_read_only()
        {
            return Err(DbError::PolicyDenied);
        }
        self.plan(tool, row_limit, statements.pop())
    }

    pub fn validate(&self, plan: &AiToolPlan) -> Result<(), DbError> {
        self.live()?;
        let state = self.state.lock().map_err(|_| DbError::SessionClosed)?;
        self.check_state(&state, true, plan.tool.needs_rows())?;
        if plan.identity != self.identity || plan.epoch != state.epoch
            || plan.consumed.swap(true, Ordering::AcqRel)
        {
            return Err(DbError::PolicyDenied);
        }
        Ok(())
    }

    fn plan(&self, tool: ReadTool, row_limit: u32, statement: Option<GuardedStatement>) -> Result<AiToolPlan, DbError> {
        self.live()?;
        let state = self.state.lock().map_err(|_| DbError::SessionClosed)?;
        self.check_state(&state, true, tool.needs_rows())?;
        Ok(AiToolPlan { identity: self.identity, epoch: state.epoch, tool, row_limit, consumed: AtomicBool::new(false), statement })
    }

    fn live(&self) -> Result<(), DbError> {
        if self.cancel.is_cancelled() { Err(DbError::SessionClosed) } else { Ok(()) }
    }

    fn check_state(&self, state: &State, schema: bool, rows: bool) -> Result<(), DbError> {
        self.live()?;
        let now = Instant::now();
        if (schema && !state.schema.is_some_and(|expires| expires > now))
            || (rows && !state.rows.is_some_and(|expires| expires > now))
        {
            Err(DbError::ApprovalRequired)
        } else { Ok(()) }
    }
}
