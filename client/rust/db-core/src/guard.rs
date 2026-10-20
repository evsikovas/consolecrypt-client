//! Native, session-bound authorization for database requests. Neither policy nor
//! execution permits are deserializable; synchronized inventory cannot grant
//! an execution approval.
use crate::parser::{self, ParsedStatement};
use crate::types::{DbError, DbQueryRequest};
use cc_models::{database::DbEngine, ObjectId};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;
use zeroize::Zeroizing;

const APPROVAL_LIFETIME: Duration = Duration::from_secs(300);
const MAX_APPROVALS: usize = 64;

/// Constructed only from the unlocked native connection and its live revision.
#[derive(Debug, Clone)]
pub struct GuardPolicy {
    pub engine: DbEngine,
    pub session_id: String,
    pub connection_id: ObjectId,
    pub connection_revision: String,
    /// First explicitly selected PostgreSQL schema, or "public". Never used
    /// for function lookup; read-only drivers set search_path to pg_catalog.
    pub default_schema: String,
    pub production: bool,
    pub mobile: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum QueryRisk {
    ReadOnly,
    Write,
    Destructive,
}

/// Display metadata only. The protected editor retains the reviewed query;
/// neither SQL nor parameter values are copied to a loggable review DTO.
#[derive(Clone, Serialize)]
pub struct QueryReview {
    pub ticket: String,
    pub risk: QueryRisk,
    pub reason_codes: Vec<&'static str>,
    pub statement_count: usize,
    pub expires_in_seconds: u64,
}

impl std::fmt::Debug for QueryReview {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("QueryReview")
            .field("risk", &self.risk)
            .field("statement_count", &self.statement_count)
            .finish_non_exhaustive()
    }
}

#[derive(Debug)]
struct Approval {
    digest: [u8; 32],
    epoch: u64,
    expires: Instant,
    approved: bool,
}

#[derive(Debug)]
struct GuardState {
    writes_allowed: bool,
    epoch: u64,
    approvals: BTreeMap<String, Approval>,
}

#[derive(Debug)]
pub struct DbGuard {
    policy: GuardPolicy,
    identity: Uuid,
    cancel: CancellationToken,
    state: Mutex<GuardState>,
}

/// A permit is opaque, uncloneable, non-serializable and usable exactly once.
/// Parsed content is immutable. Drivers validate immediately before sending.
pub struct GuardedStatement {
    guard: Uuid,
    epoch: u64,
    engine: DbEngine,
    parsed: ParsedStatement,
    risk: QueryRisk,
    requires_server_read_only: bool,
    consumed: AtomicBool,
}

impl std::fmt::Debug for GuardedStatement {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GuardedStatement")
            .field("engine", &self.engine)
            .field("risk", &self.risk)
            .finish_non_exhaustive()
    }
}

impl GuardedStatement {
    pub fn engine(&self) -> DbEngine {
        self.engine
    }

    pub fn parsed(&self) -> &ParsedStatement {
        &self.parsed
    }

    pub fn is_read_only(&self) -> bool {
        self.risk == QueryRisk::ReadOnly
    }

    pub fn requires_server_read_only(&self) -> bool {
        self.requires_server_read_only
    }
}

impl DbGuard {
    pub fn new(policy: GuardPolicy, cancel: CancellationToken) -> Self {
        let writes_allowed = !policy.production && !policy.mobile;
        Self {
            policy,
            identity: Uuid::new_v4(),
            cancel,
            state: Mutex::new(GuardState {
                writes_allowed,
                epoch: 0,
                approvals: BTreeMap::new(),
            }),
        }
    }

    pub fn policy(&self) -> &GuardPolicy {
        &self.policy
    }

    pub fn cancellation(&self) -> CancellationToken {
        self.cancel.clone()
    }

    pub fn is_read_only(&self) -> bool {
        self.policy.production
            || self
                .state
                .lock()
                .map(|state| !state.writes_allowed)
                .unwrap_or(true)
    }

    fn live(&self) -> Result<(), DbError> {
        if self.cancel.is_cancelled() {
            Err(DbError::SessionClosed)
        } else {
            Ok(())
        }
    }

    pub fn allow_session_write(&self) -> Result<(), DbError> {
        self.live()?;
        if self.policy.production {
            return Err(DbError::PolicyDenied);
        }
        let mut state = self.state.lock().map_err(|_| DbError::SessionClosed)?;
        self.live()?;
        state.writes_allowed = true;
        state.epoch = state.epoch.wrapping_add(1);
        state.approvals.clear();
        Ok(())
    }

    pub fn revoke_session_write(&self) {
        if let Ok(mut state) = self.state.lock() {
            state.writes_allowed = false;
            state.epoch = state.epoch.wrapping_add(1);
            state.approvals.clear();
        }
    }

    pub fn close(&self) {
        self.cancel.cancel();
        self.revoke_session_write();
    }

    fn digest(&self, request: &DbQueryRequest) -> Result<[u8; 32], DbError> {
        let serialized = Zeroizing::new(
            serde_json::to_vec(&(
                self.policy.engine,
                self.policy.connection_id,
                &self.policy.connection_revision,
                &request.text,
                &request.parameters,
            ))
            .map_err(|_| DbError::InvalidRequest)?,
        );
        Ok(Sha256::digest(serialized.as_slice()).into())
    }

    pub fn review(&self, request: &DbQueryRequest) -> Result<QueryReview, DbError> {
        self.live()?;
        let batch = parser::parse(self.policy.engine, &self.policy.default_schema, request)?;
        let risk = batch
            .iter()
            .map(|statement| statement.risk)
            .max()
            .ok_or(DbError::InvalidRequest)?;
        let mut state = self.state.lock().map_err(|_| DbError::SessionClosed)?;
        self.live()?;
        if risk != QueryRisk::ReadOnly && (self.policy.production || !state.writes_allowed) {
            return Err(DbError::PolicyDenied);
        }
        let now = Instant::now();
        state.approvals.retain(|_, approval| approval.expires > now);
        if state.approvals.len() >= MAX_APPROVALS {
            return Err(DbError::ResultLimit);
        }
        let ticket = Uuid::new_v4().to_string();
        let epoch = state.epoch;
        state.approvals.insert(
            ticket.clone(),
            Approval {
                digest: self.digest(request)?,
                epoch,
                expires: now + APPROVAL_LIFETIME,
                approved: false,
            },
        );
        let mut reason_codes: Vec<_> = batch.iter().map(|statement| statement.reason).collect();
        reason_codes.sort_unstable();
        reason_codes.dedup();
        Ok(QueryReview {
            ticket,
            risk,
            reason_codes,
            statement_count: batch.len(),
            expires_in_seconds: APPROVAL_LIFETIME.as_secs(),
        })
    }

    /// Caller invokes this only after its explicit protected review UI. The
    /// opaque ticket was created natively for the exact query and parameters.
    pub fn approve(&self, ticket: &str) -> Result<(), DbError> {
        self.live()?;
        let mut state = self.state.lock().map_err(|_| DbError::SessionClosed)?;
        let epoch = state.epoch;
        let approval = state
            .approvals
            .get_mut(ticket)
            .filter(|approval| approval.expires > Instant::now() && approval.epoch == epoch)
            .ok_or(DbError::ApprovalRequired)?;
        self.live()?;
        approval.approved = true;
        Ok(())
    }

    pub fn prepare(&self, request: &DbQueryRequest) -> Result<Vec<GuardedStatement>, DbError> {
        self.live()?;
        let batch = parser::parse(self.policy.engine, &self.policy.default_schema, request)?;
        let risk = batch
            .iter()
            .map(|statement| statement.risk)
            .max()
            .ok_or(DbError::InvalidRequest)?;
        let mut state = self.state.lock().map_err(|_| DbError::SessionClosed)?;
        self.live()?;
        if risk != QueryRisk::ReadOnly && (self.policy.production || !state.writes_allowed) {
            return Err(DbError::PolicyDenied);
        }
        if risk == QueryRisk::Destructive {
            let ticket = request
                .approval_ticket
                .as_ref()
                .ok_or(DbError::ApprovalRequired)?;
            let approval = state
                .approvals
                .remove(ticket)
                .ok_or(DbError::ApprovalRequired)?;
            if !approval.approved
                || approval.expires <= Instant::now()
                || approval.epoch != state.epoch
                || approval.digest != self.digest(request)?
            {
                return Err(DbError::ApprovalRequired);
            }
        }
        let epoch = state.epoch;
        let force_read_only = self.policy.production || !state.writes_allowed;
        Ok(batch
            .into_iter()
            .map(|statement| GuardedStatement {
                guard: self.identity,
                epoch,
                engine: self.policy.engine,
                requires_server_read_only: force_read_only || statement.risk == QueryRisk::ReadOnly,
                parsed: statement.parsed,
                risk: statement.risk,
                consumed: AtomicBool::new(false),
            })
            .collect())
    }

    pub fn validate_permit(&self, statement: &GuardedStatement) -> Result<(), DbError> {
        self.live()?;
        let state = self.state.lock().map_err(|_| DbError::SessionClosed)?;
        self.live()?;
        if statement.guard != self.identity
            || statement.engine != self.policy.engine
            || statement.epoch != state.epoch
            || (statement.risk != QueryRisk::ReadOnly
                && (self.policy.production || !state.writes_allowed))
            || statement.consumed.swap(true, Ordering::AcqRel)
        {
            return Err(DbError::PolicyDenied);
        }
        Ok(())
    }
}
