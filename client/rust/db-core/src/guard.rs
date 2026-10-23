//! Native, session-bound authorization for database requests. Neither policy nor
//! execution permits are deserializable; synchronized inventory cannot grant
//! an execution approval.
use crate::parser::{self, ParsedStatement, RefusalHint};
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
/// Lengths of a production write window the user can pick, in minutes.
pub const PRODUCTION_WRITE_MINUTES: [u64; 3] = [5, 15, 30];

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
    /// Saved connection name; opening a production write window requires
    /// the user to type it.
    pub connection_name: String,
    pub production: bool,
    pub mobile: bool,
    /// The connection is read-only by its own setting (a SQLite file opened
    /// read-only): nothing can allow writes, not even a production window.
    pub read_only: bool,
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
    /// End of a production write window opened by the user (vault
    /// passphrase and the typed connection name). Production sessions write
    /// only before it; it never outlives the session.
    production_write_until: Option<Instant>,
    epoch: u64,
    approvals: BTreeMap<String, Approval>,
}

impl GuardState {
    /// Re-locks an expired production window: the epoch changes, so every
    /// approval and permit issued in the window stops working.
    fn expire(&mut self) {
        if self
            .production_write_until
            .is_some_and(|until| until <= Instant::now())
        {
            self.production_write_until = None;
            self.revoke();
        }
    }

    fn revoke(&mut self) {
        self.epoch = self.epoch.wrapping_add(1);
        self.approvals.clear();
    }
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
    production_write: bool,
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

    /// A changing statement of a production session, permitted by an open
    /// production write window.
    pub fn production_write(&self) -> bool {
        self.production_write
    }
}

impl DbGuard {
    pub fn new(policy: GuardPolicy, cancel: CancellationToken) -> Self {
        let writes_allowed = !policy.production && !policy.mobile && !policy.read_only;
        Self {
            policy,
            identity: Uuid::new_v4(),
            cancel,
            state: Mutex::new(GuardState {
                writes_allowed,
                production_write_until: None,
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
        self.state
            .lock()
            .map(|mut state| !self.writable(&mut state))
            .unwrap_or(true)
    }

    /// Whether changing statements may run now: a production session only
    /// inside its write window, any other session after the user allowed it.
    fn writable(&self, state: &mut GuardState) -> bool {
        if self.cancel.is_cancelled() || self.policy.read_only {
            return false;
        }
        if self.policy.production {
            state.expire();
            !self.policy.mobile && state.production_write_until.is_some()
        } else {
            state.writes_allowed
        }
    }

    /// Opens a production write window for `duration` (one of
    /// [`PRODUCTION_WRITE_MINUTES`]). Call only after the vault passphrase
    /// checked out. Mobile sessions and non-production sessions never get
    /// one.
    pub fn open_production_write(&self, duration: Duration) -> Result<Instant, DbError> {
        self.live()?;
        if !self.policy.production
            || self.policy.mobile
            || self.policy.read_only
            || !PRODUCTION_WRITE_MINUTES
                .iter()
                .any(|minutes| Duration::from_secs(minutes * 60) == duration)
        {
            return Err(DbError::PolicyDenied);
        }
        let mut state = self.state.lock().map_err(|_| DbError::SessionClosed)?;
        self.live()?;
        let until = Instant::now() + duration;
        state.production_write_until = Some(until);
        state.revoke();
        Ok(until)
    }

    /// Time left in the production write window, if one is open.
    pub fn production_write_remaining(&self) -> Option<Duration> {
        let mut state = self.state.lock().ok()?;
        if !self.policy.production || !self.writable(&mut state) {
            return None;
        }
        state
            .production_write_until
            .and_then(|until| until.checked_duration_since(Instant::now()))
    }

    /// Ends the production write window now (the user locked it, the vault
    /// locked or the session closes).
    pub fn close_production_write(&self) {
        if let Ok(mut state) = self.state.lock() {
            if state.production_write_until.take().is_some() {
                state.revoke();
            }
        }
    }

    /// The highest risk of a request, without issuing anything.
    pub fn risk(&self, request: &DbQueryRequest) -> Result<QueryRisk, DbError> {
        parser::parse(self.policy.engine, &self.policy.default_schema, request)?
            .iter()
            .map(|statement| statement.risk)
            .max()
            .ok_or(DbError::InvalidRequest)
    }

    /// Role and permission management never runs on a production session,
    /// not even inside a production write window.
    fn refuse_on_production(&self, batch: &[parser::ClassifiedStatement]) -> Result<(), DbError> {
        if self.policy.production
            && batch
                .iter()
                .any(|statement| statement.reason == parser::cql::ROLE_MANAGEMENT)
        {
            return Err(DbError::PolicyDenied);
        }
        Ok(())
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
        if self.policy.production || self.policy.read_only {
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
            state.production_write_until = None;
            state.revoke();
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
        self.refuse_on_production(&batch)?;
        let risk = batch
            .iter()
            .map(|statement| statement.risk)
            .max()
            .ok_or(DbError::InvalidRequest)?;
        let mut state = self.state.lock().map_err(|_| DbError::SessionClosed)?;
        self.live()?;
        let writable = self.writable(&mut state);
        if risk != QueryRisk::ReadOnly && !writable {
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
        state.expire();
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
        self.refuse_on_production(&batch)?;
        let risk = batch
            .iter()
            .map(|statement| statement.risk)
            .max()
            .ok_or(DbError::InvalidRequest)?;
        let mut state = self.state.lock().map_err(|_| DbError::SessionClosed)?;
        self.live()?;
        let writable = self.writable(&mut state);
        if risk != QueryRisk::ReadOnly && !writable {
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
        let force_read_only = !writable;
        Ok(batch
            .into_iter()
            .map(|statement| GuardedStatement {
                guard: self.identity,
                epoch,
                engine: self.policy.engine,
                requires_server_read_only: force_read_only || statement.risk == QueryRisk::ReadOnly,
                production_write: self.policy.production
                    && !force_read_only
                    && statement.risk != QueryRisk::ReadOnly,
                parsed: statement.parsed,
                risk: statement.risk,
                consumed: AtomicBool::new(false),
            })
            .collect())
    }

    /// A safe replacement to offer when `prepare`/`review` refuse the request
    /// (Redis `KEYS` → an equivalent `SCAN`). It never grants a permit.
    pub fn refusal_hint(&self, request: &DbQueryRequest) -> Option<RefusalHint> {
        parser::refusal_hint(self.policy.engine, request)
    }

    pub fn validate_permit(&self, statement: &GuardedStatement) -> Result<(), DbError> {
        self.live()?;
        let mut state = self.state.lock().map_err(|_| DbError::SessionClosed)?;
        self.live()?;
        let writable = self.writable(&mut state);
        if statement.guard != self.identity
            || statement.engine != self.policy.engine
            || statement.epoch != state.epoch
            || (statement.risk != QueryRisk::ReadOnly && !writable)
            || statement.consumed.swap(true, Ordering::AcqRel)
        {
            return Err(DbError::PolicyDenied);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn production() -> DbGuard {
        DbGuard::new(
            GuardPolicy {
                engine: DbEngine::Postgresql,
                session_id: "s".into(),
                connection_id: ObjectId::new(),
                connection_revision: "r".into(),
                default_schema: "public".into(),
                connection_name: "orders-prod".into(),
                production: true,
                mobile: false,
                read_only: false,
            },
            CancellationToken::new(),
        )
    }

    fn request(text: &str) -> DbQueryRequest {
        DbQueryRequest {
            text: text.into(),
            parameters: vec![],
            approval_ticket: None,
        }
    }

    #[test]
    fn an_expired_production_window_relocks_and_voids_its_permits() {
        let guard = production();
        guard
            .open_production_write(Duration::from_secs(300))
            .unwrap();
        let review = guard.review(&request("DELETE FROM items")).unwrap();
        let permits = guard
            .prepare(&request("INSERT INTO items VALUES (1)"))
            .unwrap();
        assert!(permits[0].production_write());
        // The window runs out.
        guard.state.lock().unwrap().production_write_until =
            Instant::now().checked_sub(Duration::from_millis(1));
        assert!(guard.is_read_only());
        assert_eq!(guard.production_write_remaining(), None);
        assert!(matches!(
            guard.validate_permit(&permits[0]),
            Err(DbError::PolicyDenied)
        ));
        assert!(matches!(
            guard.approve(&review.ticket),
            Err(DbError::ApprovalRequired)
        ));
        assert!(matches!(
            guard.prepare(&request("INSERT INTO items VALUES (1)")),
            Err(DbError::PolicyDenied)
        ));
        assert!(guard.state.lock().unwrap().production_write_until.is_none());
        // Reads keep working.
        let read = guard.prepare(&request("SELECT 1")).unwrap();
        guard.validate_permit(&read[0]).unwrap();
        assert!(!read[0].production_write());
    }

    #[test]
    fn remaining_time_counts_down_from_the_chosen_length() {
        let guard = production();
        let until = guard
            .open_production_write(Duration::from_secs(15 * 60))
            .unwrap();
        let remaining = guard.production_write_remaining().unwrap();
        assert!(remaining <= Duration::from_secs(15 * 60));
        assert!(remaining > Duration::from_secs(14 * 60));
        assert!(until > Instant::now());
    }
}
