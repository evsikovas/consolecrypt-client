//! Job-only operations over already authenticated sessions. No vault resolver,
//! agent forwarding, local filesystem access, URL fetching or arbitrary dialing.
use crate::{
    AutomationError, Result, TargetFailure, TargetFuture, TargetHandler, TargetRequest, VerifiedJob,
};
use base64::{engine::general_purpose::STANDARD, Engine};
use cc_models::automation::ApprovedTarget;
use cc_sftp_core::SftpClient;
use cc_ssh_core::{russh::ChannelMsg, SshSession};
use serde::Deserialize;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    collections::HashMap,
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc,
    },
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use tokio::sync::{Mutex, OnceCell, Semaphore};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

type TargetResult<T> = std::result::Result<T, TargetFailure>;
const FILE_LIMIT: u64 = 100 * 1024 * 1024;

/// Freeze the actual native connection for the approval preview. App-core must
/// prepare shared hosts through sharing_prepare_connection first. Use separate
/// job connections; do not pass terminal sessions here.
pub fn target_from_session(alias: String, session: &SshSession) -> Result<ApprovedTarget> {
    if session.is_closed() { return Err(AutomationError::Closed); }
    target_from_connection(alias, session.connection_plan(), session.network_origin())
}

pub(crate) fn target_from_connection(alias:String, plan:&cc_ssh_core::ConnectionPlan,
    origin:&cc_ssh_core::client::NetworkOrigin)->Result<ApprovedTarget> {
    if !plan.forwards.is_empty() || plan.agent_forwarding {
        return Err(AutomationError::Invalid(
            "job session has forwarding or is closed",
        ));
    }
    let raw = serde_json::to_vec(&crate::project::canonical(
        json!({"plan": plan, "network_origin": origin}),
    ))
    .map_err(|_| AutomationError::Invalid("route serialization"))?;
    let mut references = Vec::new();
    for hop in plan.all_hops() {
        if let Some(credential) = hop.credential {
            references.push(
                credential
                    .id
                    .to_string()
                    .parse::<Uuid>()
                    .map_err(|_| AutomationError::Invalid("credential identifier"))?,
            );
        }
    }
    if let Some(secret) = plan
        .proxy
        .as_ref()
        .and_then(|proxy| proxy.password_secret_id)
    {
        references.push(
            secret
                .to_string()
                .parse()
                .map_err(|_| AutomationError::Invalid("proxy secret identifier"))?,
        );
    }
    references.sort();
    references.dedup();
    Ok(ApprovedTarget {
        host_id: plan
            .host_id
            .to_string()
            .parse()
            .map_err(|_| AutomationError::Invalid("host identifier"))?,
        alias,
        address: plan.target.host.clone(),
        port: plan.target.port,
        username: plan.username.clone(),
        route_digest: hex::encode(Sha256::digest(raw)),
        credential_refs: references,
    })
}

struct BoundTarget {
    session: Arc<SshSession>,
    sftp: OnceCell<Arc<SftpClient>>,
}
struct Transfer {
    path: String,
    offset: u64,
    total: u64,
}

pub struct JobTargetScope {
    approval: VerifiedJob,
    targets: HashMap<Uuid, Arc<BoundTarget>>,
    deadline: Instant,
    cancelled: CancellationToken,
    slots: Arc<Semaphore>,
    transfers: Arc<Mutex<HashMap<(Uuid, Uuid), Transfer>>>,
    file_bytes: Arc<AtomicU64>,
    active_operations: Arc<AtomicU64>,
    uncertain_operation: Arc<AtomicBool>,
    diagnostic_retired:CancellationToken,
    diagnostic_bytes:AtomicU64,
    become_input:Option<Arc<crate::ScopedBecomePassword>>,
}

impl std::fmt::Debug for JobTargetScope {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("JobTargetScope")
            .field("job_id", &self.approval.manifest().job_id)
            .field("targets", &self.targets.len())
            .field("cancelled", &self.cancelled.is_cancelled())
            .finish_non_exhaustive()
    }
}

/// One registry per owning profile/lifecycle, shared by its helper transports.
/// Registration is a native approval action, never a helper RPC operation.
#[derive(Default)]
pub struct JobScopeRegistry {
    scopes: std::sync::Mutex<HashMap<Uuid, Arc<JobTargetScope>>>,
    accepted: std::sync::Mutex<HashMap<Uuid, String>>,
}

impl std::fmt::Debug for JobScopeRegistry {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let active = self.scopes.lock().unwrap_or_else(|e| e.into_inner()).len();
        formatter
            .debug_struct("JobScopeRegistry")
            .field("active", &active)
            .finish_non_exhaustive()
    }
}

impl Drop for JobScopeRegistry {
    fn drop(&mut self) {
        self.cancel_all();
    }
}

impl JobScopeRegistry {
    /// Credential lifetime is not proof that a remote operation has finished.
    /// Keep administrative disconnects fenced until explicit job reconciliation.
    pub fn has_scopes(&self) -> bool {
        !self
            .scopes
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .is_empty()
    }

    pub fn register(&self, scope: Arc<JobTargetScope>) -> Result<()> {
        if scope.approval.owner_confirmation().is_some() && (scope.approval.is_access_expired() || scope.approval.is_submission_expired()) {
            return Err(AutomationError::Invalid("original job admission retired"));
        }
        let mut scopes = self
            .scopes
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        scopes.retain(|_, existing| !existing.is_cancelled() && Instant::now() < existing.deadline);
        let mut accepted = self
            .accepted
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let job = scope.approval.manifest().job_id;
        if scopes.len() >= 64 || accepted.len() >= 10000 || accepted.contains_key(&job) {
            return Err(AutomationError::Invalid(
                "job scope already registered or registry limit exceeded",
            ));
        }
        accepted.insert(job, scope.approval.digest().to_string());
        scopes.insert(job, scope);
        Ok(())
    }

    pub fn vault_locked(&self) {
        for scope in self
            .scopes
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .values()
        {
            scope.vault_locked();
        }
    }

    pub fn cancel_all(&self) {
        let mut scopes = self.scopes.lock().unwrap_or_else(|e| e.into_inner());
        for scope in scopes.values() {
            scope.cancel();
        }
        scopes.clear();
    }
}

impl TargetHandler for JobScopeRegistry {
    fn handle(&self, request: TargetRequest) -> TargetFuture {
        match self
            .scopes
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(&request.job_id)
            .cloned()
        {
            Some(scope) => scope.handle(request),
            None => Box::pin(async { Err(TargetFailure::TargetDenied) }),
        }
    }

    fn revoke(&self, job_id: Uuid, manifest_digest: &str) {
        let mut scopes = self.scopes.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(scope) = scopes.get(&job_id) {
            if scope.approval.digest() == manifest_digest {
                scope.cancel();
                scopes.remove(&job_id);
            }
        }
    }
}

impl JobTargetScope {
    pub fn bind(
        approval: VerifiedJob,
        sessions: HashMap<Uuid, Arc<SshSession>>,
    ) -> Result<Arc<Self>> {
        Self::bind_with_cancellation(approval, sessions, CancellationToken::new())
    }

    /// Dependency edits revoke exactly this job, including grants allowed to
    /// survive a vault lock. The token contains no vault or credential resolver.
    pub fn bind_with_cancellation(
        approval: VerifiedJob,
        sessions: HashMap<Uuid, Arc<SshSession>>,
        cancelled: CancellationToken,
    ) -> Result<Arc<Self>> {
        Self::bind_with_become(approval,sessions,cancelled,None)
    }
    /// Native-issued one-shot input; never deserialized from Runner requests.
    pub fn bind_with_become(approval:VerifiedJob,sessions:HashMap<Uuid,Arc<SshSession>>,
        cancelled:CancellationToken,become_input:Option<Arc<crate::ScopedBecomePassword>>)->Result<Arc<Self>> {
        if become_input.is_some() && !approval.manifest().policy.cancel_on_lock {
            return Err(AutomationError::Invalid("one-shot sudo requires cancellation on lock"))
        }
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|_| AutomationError::Invalid("system clock"))?
            .as_secs() as i64;
        let deadline = approval.access_deadline();
        if approval.is_access_expired()
            || approval.is_submission_expired()
            || now >= approval.manifest().access_expires_at
            || now < approval.manifest().issued_at.saturating_sub(30)
            || sessions.len() != approval.manifest().targets.len()
        {
            return Err(AutomationError::Invalid(
                "expired or incomplete job target sessions",
            ));
        }
        let mut targets = HashMap::new();
        for approved in &approval.manifest().targets {
            let session = sessions
                .get(&approved.host_id)
                .ok_or(AutomationError::Invalid("missing job target session"))?;
            let actual = target_from_session(approved.alias.clone(), session)?;
            if serde_json::to_value(actual).ok() != serde_json::to_value(approved).ok() {
                return Err(AutomationError::Invalid(
                    "connected route differs from approved route",
                ));
            }
            targets.insert(
                approved.host_id,
                Arc::new(BoundTarget {
                    session: session.clone(),
                    sftp: OnceCell::new(),
                }),
            );
            if let Some(password)=&become_input {
                let capture=session.diagnostic_capture().ok_or(AutomationError::Invalid("sudo diagnostic mask unavailable"))?;
                password.remember(capture.as_ref())?;
            }
        }
        let forks = approval.manifest().policy.forks as usize;
        let diagnostic_runtime=tokio::runtime::Handle::try_current().map_err(|_|AutomationError::Invalid("job diagnostic runtime unavailable"))?;
        let scope=Arc::new(Self {
            approval,
            targets,
            deadline,
            cancelled,
            slots: Arc::new(Semaphore::new(forks)),
            transfers: Arc::new(Mutex::new(HashMap::new())),
            file_bytes: Arc::new(AtomicU64::new(0)),
            active_operations: Arc::new(AtomicU64::new(0)),
            uncertain_operation: Arc::new(AtomicBool::new(false)),
            diagnostic_retired:CancellationToken::new(),
            diagnostic_bytes:AtomicU64::new(0),
            become_input,
        });
        let weak=Arc::downgrade(&scope);let cancelled=scope.cancelled.clone();let retired=scope.diagnostic_retired.clone();let until=scope.deadline;
        diagnostic_runtime.spawn(async move{tokio::select!{_=cancelled.cancelled()=>{},_=retired.cancelled()=>{},_=tokio::time::sleep_until(until.into())=>{}}if let Some(scope)=weak.upgrade(){scope.clear_diagnostics();}});
        Ok(scope)
    }

    /// Local service owner calls this after the actual default worker joins.
    /// TCP/SSH closure is not proof that remote changes were rolled back.
    pub async fn close_sessions(&self) -> Result<()> {
        self.cancel();
        tokio::time::timeout(Duration::from_secs(20), async {
            for target in self.targets.values() {
                if !target.session.is_closed() && target.session.disconnect().await.is_err() && !target.session.is_closed() {
                    return Err(AutomationError::Closed);
                }
            }
            Ok(())
        })
        .await
        .map_err(|_| AutomationError::Timeout)?
    }

    fn clear_diagnostics(&self){for target in self.targets.values(){if let Some(mask)=target.session.diagnostic_capture(){mask.clear();}}}
    pub fn cancel(&self) {
        if let Some(password)=&self.become_input{password.revoke();}
        self.clear_diagnostics();
        self.cancelled.cancel();
    }
    pub fn vault_locked(&self) {
        if self.approval.manifest().policy.cancel_on_lock {
            self.cancel();
        }
    }
    pub fn is_cancelled(&self) -> bool {
        self.cancelled.is_cancelled()
    }
    pub fn has_uncertain_operations(&self) -> bool {
        self.active_operations.load(Ordering::SeqCst) > 0
            || self.uncertain_operation.load(Ordering::SeqCst)
    }
}

impl Drop for JobTargetScope{fn drop(&mut self){if let Some(password)=&self.become_input{password.revoke();}self.clear_diagnostics();self.diagnostic_retired.cancel();}}

impl TargetHandler for JobTargetScope {
    fn revoke(&self, job_id: Uuid, manifest_digest: &str) {
        if job_id == self.approval.manifest().job_id && manifest_digest == self.approval.digest() {
            self.cancel();
        }
    }

    fn handle(&self, request: TargetRequest) -> TargetFuture {
        if request.job_id != self.approval.manifest().job_id
            || request.manifest_digest != self.approval.digest()
        {
            return Box::pin(async { Err(TargetFailure::TargetDenied) });
        }
        if self.cancelled.is_cancelled() {
            return Box::pin(async { Err(TargetFailure::Cancelled) });
        }
        let wall_valid = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .ok()
            .and_then(|duration| i64::try_from(duration.as_secs()).ok())
            .is_some_and(|now| {
                now < self.approval.manifest().access_expires_at
                    && now >= self.approval.manifest().issued_at.saturating_sub(30)
            });
        if Instant::now() >= self.deadline || self.approval.is_access_expired() || !wall_valid {
            return Box::pin(async { Err(TargetFailure::ApprovalExpired) });
        }
        let Some(target) = self.targets.get(&request.host_id).cloned() else {
            return Box::pin(async { Err(TargetFailure::TargetDenied) });
        };
        if request.operation=="redact_event"{
            // Parent-only data lane; guest JobBroker never forwards this op.
            // No resolver, network, active-operation/uncertain flag or grant.
            let size=serde_json::to_vec(&request.body).map(|v|v.len() as u64).unwrap_or(u64::MAX);
            if self.diagnostic_bytes.fetch_update(Ordering::SeqCst,Ordering::SeqCst,|used|used.checked_add(size).filter(|n|*n<=50*1024*1024)).is_err(){return Box::pin(async{Err(TargetFailure::OutputLimit)})}
            let masks=self.targets.values().try_fold(Vec::new(),|mut masks:Vec<Arc<cc_ssh_core::DiagnosticCapture>>,target|{
                let mask=target.session.diagnostic_capture().ok_or(TargetFailure::UnsupportedOperation)?;
                if !masks.iter().any(|m|Arc::ptr_eq(m,&mask)){masks.push(mask);}Ok(masks)
            });
            let aliases=self.approval.manifest().targets.iter().map(|t|t.alias.clone()).collect();
            let projected=masks.and_then(|masks|crate::diagnostic::project(request.body,&masks,&aliases));
            if self.cancelled.is_cancelled(){return Box::pin(async{Err(TargetFailure::Cancelled)})}
            let wall_valid=SystemTime::now().duration_since(UNIX_EPOCH).ok()
                .and_then(|duration|i64::try_from(duration.as_secs()).ok())
                .is_some_and(|now|now<self.approval.manifest().access_expires_at&&now>=self.approval.manifest().issued_at.saturating_sub(30));
            if Instant::now()>=self.deadline||self.approval.is_access_expired()||!wall_valid{return Box::pin(async{Err(TargetFailure::ApprovalExpired)})}
            return Box::pin(async move{projected});
        }
        let permit = match self.slots.clone().try_acquire_owned() {
            Ok(permit) => permit,
            Err(_) => return Box::pin(async { Err(TargetFailure::TargetDenied) }),
        };
        let cancelled = self.cancelled.clone();
        let deadline = self.deadline;
        let transfers = self.transfers.clone();
        let quota = self.file_bytes.clone();
        let become_input=self.become_input.clone();
        let active = self.active_operations.clone();
        let uncertain = self.uncertain_operation.clone();
        active.fetch_add(1, Ordering::SeqCst);
        let fence = OperationFence {
            active,
            uncertain,
            known: false,
        };
        Box::pin(async move {
            let mut fence = fence;
            let _permit = permit;
            let result = tokio::select! {
                _ = cancelled.cancelled() => Err(TargetFailure::Cancelled),
                _ = tokio::time::sleep_until(deadline.into()) => Err(TargetFailure::ApprovalExpired),
                result = operate(target, request, transfers, quota,become_input) => result,
            };
            // Only an acknowledged operation is proof that it stopped. A
            // dropped future/connection/timeout must retain the global lease.
            fence.known = result.is_ok();
            result
        })
    }
}

struct OperationFence {
    active: Arc<AtomicU64>,
    uncertain: Arc<AtomicBool>,
    known: bool,
}
impl Drop for OperationFence {
    fn drop(&mut self) {
        if !self.known {
            self.uncertain.store(true, Ordering::SeqCst);
        }
        self.active.fetch_sub(1, Ordering::SeqCst);
    }
}

fn path_valid(path: &str) -> bool {
    !path.is_empty() && path.len() <= 4096 && !path.chars().any(char::is_control)
}
fn account_bytes(quota: &AtomicU64, bytes: u64) -> TargetResult<()> {
    quota
        .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |used| {
            used.checked_add(bytes)
                .filter(|total| *total <= 1024 * 1024 * 1024)
        })
        .map(|_| ())
        .map_err(|_| TargetFailure::OutputLimit)
}

async fn operate(
    target: Arc<BoundTarget>,
    request: TargetRequest,
    transfers: Arc<Mutex<HashMap<(Uuid, Uuid), Transfer>>>,
    quota: Arc<AtomicU64>,
    become_input:Option<Arc<crate::ScopedBecomePassword>>,
) -> TargetResult<Value> {
    if request.operation=="exec_become" {
        return crate::sudo_input::execute(&target.session,request.body,
            become_input.as_ref().ok_or(TargetFailure::TargetDenied)?).await;
    }
    if request.operation == "exec" {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Exec {
            command: String,
            stdin: String,
        }
        let input: Exec = serde_json::from_value(request.body)
            .map_err(|_| TargetFailure::UnsupportedOperation)?;
        if input.command.len() > 64 * 1024
            || input.command.contains('\0')
            || input.stdin.len() > 350000
        {
            return Err(TargetFailure::OutputLimit);
        }
        let data = STANDARD
            .decode(input.stdin)
            .map_err(|_| TargetFailure::UnsupportedOperation)?;
        if data.len() > 256 * 1024 {
            return Err(TargetFailure::OutputLimit);
        }
        let mut channel = tokio::time::timeout(
            Duration::from_secs(15),
            target.session.open_exec(&input.command),
        )
        .await
        .map_err(|_| TargetFailure::ConnectionLost)?
        .map_err(|_| TargetFailure::ConnectionLost)?;
        if !data.is_empty() {
            channel
                .data(data.as_slice())
                .await
                .map_err(|_| TargetFailure::ConnectionLost)?;
        }
        channel
            .eof()
            .await
            .map_err(|_| TargetFailure::ConnectionLost)?;
        let (mut stdout, mut stderr, mut exit) = (Vec::new(), Vec::new(), None);
        while let Some(message) = channel.wait().await {
            match message {
                ChannelMsg::Data { data } => {
                    if stdout.len() + data.len() > 256 * 1024 {
                        let _ = channel.close().await;
                        return Err(TargetFailure::OutputLimit);
                    }
                    stdout.extend_from_slice(&data);
                }
                ChannelMsg::ExtendedData { data, .. } => {
                    if stderr.len() + data.len() > 256 * 1024 {
                        let _ = channel.close().await;
                        return Err(TargetFailure::OutputLimit);
                    }
                    stderr.extend_from_slice(&data);
                }
                ChannelMsg::ExitStatus { exit_status } => exit = Some(exit_status),
                ChannelMsg::Failure | ChannelMsg::ExitSignal { .. } => {
                    return Err(TargetFailure::OperationFailed)
                }
                ChannelMsg::Close => break,
                _ => {}
            }
        }
        let code = exit
            .filter(|code| *code <= 255)
            .ok_or(TargetFailure::ConnectionLost)?;
        return Ok(
            json!({"exit_code": code, "stdout": STANDARD.encode(stdout), "stderr": STANDARD.encode(stderr)}),
        );
    }
    if request.operation == "file_close" {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Close {
            transfer_id: Uuid,
        }
        let close: Close = serde_json::from_value(request.body)
            .map_err(|_| TargetFailure::UnsupportedOperation)?;
        transfers
            .lock()
            .await
            .remove(&(request.host_id, close.transfer_id));
        return Ok(json!({"closed": true}));
    }
    let sftp = target
        .sftp
        .get_or_try_init(|| async {
            SftpClient::open(&target.session)
                .await
                .map(Arc::new)
                .map_err(|_| TargetFailure::ConnectionLost)
        })
        .await?;
    if request.operation == "get_chunk" {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Get {
            path: String,
            offset: u64,
            limit: usize,
        }
        let get: Get = serde_json::from_value(request.body)
            .map_err(|_| TargetFailure::UnsupportedOperation)?;
        if !path_valid(&get.path)
            || get.limit == 0
            || get.limit > 128 * 1024
            || get.offset > FILE_LIMIT
        {
            return Err(TargetFailure::OutputLimit);
        }
        account_bytes(&quota, get.limit as u64)?;
        let (data, total) = sftp
            .read_range(&get.path, get.offset, get.limit)
            .await
            .map_err(|_| TargetFailure::ConnectionLost)?;
        return Ok(json!({"data": STANDARD.encode(data), "total": total}));
    }
    if request.operation == "put_chunk" {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Put {
            transfer_id: Uuid,
            path: String,
            offset: u64,
            total: u64,
            finish: bool,
            data: String,
        }
        let put: Put = serde_json::from_value(request.body)
            .map_err(|_| TargetFailure::UnsupportedOperation)?;
        if !path_valid(&put.path) || put.total > FILE_LIMIT || put.data.len() > 175000 {
            return Err(TargetFailure::OutputLimit);
        }
        let data = STANDARD
            .decode(put.data)
            .map_err(|_| TargetFailure::UnsupportedOperation)?;
        let end = put
            .offset
            .checked_add(data.len() as u64)
            .ok_or(TargetFailure::OutputLimit)?;
        if data.len() > 128 * 1024
            || end > put.total
            || (end == put.total) != put.finish
            || (data.is_empty() && put.total != 0)
        {
            return Err(TargetFailure::UnsupportedOperation);
        }
        let mut active = transfers.lock().await;
        let key = (request.host_id, put.transfer_id);
        if !active.contains_key(&key) {
            if put.offset != 0 || active.len() >= 10 {
                return Err(TargetFailure::TargetDenied);
            }
            active.insert(
                key,
                Transfer {
                    path: put.path.clone(),
                    offset: 0,
                    total: put.total,
                },
            );
        }
        let cursor = active.get_mut(&key).ok_or(TargetFailure::TargetDenied)?;
        if cursor.path != put.path || cursor.offset != put.offset || cursor.total != put.total {
            return Err(TargetFailure::TargetDenied);
        }
        account_bytes(&quota, data.len() as u64)?;
        sftp.write_range(&put.path, put.offset, &data, put.offset == 0)
            .await
            .map_err(|_| TargetFailure::ConnectionLost)?;
        cursor.offset = end;
        if put.finish {
            active.remove(&key);
        }
        return Ok(json!({"offset": end}));
    }
    Err(TargetFailure::UnsupportedOperation)
}
