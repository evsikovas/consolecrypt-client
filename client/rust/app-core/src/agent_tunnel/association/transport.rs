//! One original Native SSH plan, one fixed subsystem, no reconnect or exec.
use super::*;
use async_trait::async_trait;
use cc_models::{
    credential::{Credential, CredentialKind},
    known_host::KnownHost,
};
use cc_ssh_core::{
    ConnectionPlan, CredentialResolver, HostKeyDecision, HostKeyInfo, HostKeyPolicy, HostKeyPrompt,
    KnownHostsError, KnownHostsStore, ResolveError, ResolvedCredential, SshBackend, SshConnector,
    SshSession,
};
use std::{
    future::Future,
    pin::Pin,
    sync::Weak,
    task::{Context, Poll},
};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, ReadBuf};
const SUBSYSTEM: &str = "consolecrypt-agent-enroll-v1";
#[path = "transport_route.rs"]
mod route;
#[path = "transport_collector.rs"]
mod collector;
#[path = "transport_control.rs"]
mod control;
pub(super) use collector::CollectorTerminalObservation;
pub(super) use route::{OriginalRetirementObservation, OriginalTransportMode};
#[derive(Clone, Copy, PartialEq, Eq)]
enum CarrierState {
    NotStarted,
    Open,
    Closed,
}
struct ObservationInner {
    abort: std::sync::Mutex<Option<std::net::TcpStream>>,
    relay_abort: CancellationToken,
    control_slot:std::sync::Mutex<Option<cc_sync_core::api::RelaySlot>>,
    // 0 none/direct,1 owned,2 deletion in-flight,3 observed204,4 unknown,5 create uncertain.
    control_retirement:std::sync::atomic::AtomicU8,
    state: tokio::sync::watch::Sender<CarrierState>,
    lease: std::sync::Mutex<Option<Arc<tokio::sync::OwnedSemaphorePermit>>>,
}
/// Local ownership observation only, never an Agent commit/drain acknowledgement.
#[derive(Clone)]
pub(super) struct Observation(Arc<ObservationInner>);
impl Observation {
    pub(super) fn new() -> Self {
        let (state, _) = tokio::sync::watch::channel(CarrierState::NotStarted);
        Self(Arc::new(ObservationInner {
            abort: std::sync::Mutex::new(None),
            relay_abort: CancellationToken::new(),
            control_slot:std::sync::Mutex::new(None),
            control_retirement:std::sync::atomic::AtomicU8::new(0),
            state,
            lease: std::sync::Mutex::new(None),
        }))
    }
    pub(super) fn retain_permit(
        &self,
        permit: tokio::sync::OwnedSemaphorePermit,
    ) -> AppResult<Arc<tokio::sync::OwnedSemaphorePermit>> {
        let mut lease = self.0.lease.lock().map_err(|_| unknown())?;
        if lease.is_some() {
            return Err(unknown());
        }
        let permit = Arc::new(permit);
        *lease = Some(permit.clone());
        Ok(permit)
    }
    pub(super) fn release_closed(&self) {
        if self.closed_observed() {
            if let Ok(mut lease) = self.0.lease.lock() {
                lease.take();
            }
        }
    }
    fn bind(&self, socket: std::net::TcpStream) -> AppResult<()> {
        let mut slot = self.0.abort.lock().map_err(|_| unknown())?;
        if slot.is_some() || *self.0.state.borrow() != CarrierState::NotStarted {
            return Err(unknown());
        }
        *slot = Some(socket);
        self.0.state.send_replace(CarrierState::Open);
        Ok(())
    }
    fn bind_relay(&self)->AppResult<()> {
        let slot=self.0.abort.lock().map_err(|_|unknown())?;
        if slot.is_some() || *self.0.state.borrow()!=CarrierState::NotStarted
            || self.0.relay_abort.is_cancelled() {return Err(unknown())}
        self.0.state.send_replace(CarrierState::Open);Ok(())
    }
    pub(super) fn abort(&self) {
        self.0.relay_abort.cancel();
        if let Ok(mut slot) = self.0.abort.lock() {
            if let Some(socket) = slot.take() {
                let _ = socket.shutdown(std::net::Shutdown::Both);
                drop(socket);
            }
        }
    }
    pub(super) fn begin_control_create(&self)->AppResult<()> {
        self.0.control_retirement.compare_exchange(0,5,std::sync::atomic::Ordering::SeqCst,
            std::sync::atomic::Ordering::SeqCst).map(|_|()).map_err(|_|unknown())
    }
    pub(super) fn own_control_slot(&self,slot:cc_sync_core::api::RelaySlot)->AppResult<()> {
        let mut owned=self.0.control_slot.lock().map_err(|_|unknown())?;
        if owned.is_some() || !matches!(self.0.control_retirement.load(std::sync::atomic::Ordering::SeqCst),0|5) {return Err(unknown())}
        *owned=Some(slot);self.0.control_retirement.store(1,std::sync::atomic::Ordering::SeqCst);drop(owned);
        let current=*self.0.state.borrow();self.0.state.send_replace(current);Ok(())
    }
    pub(super) fn control_custody_unknown(&self) {
        self.0.control_retirement.store(4,std::sync::atomic::Ordering::SeqCst);
        let current=*self.0.state.borrow();self.0.state.send_replace(current);
    }
    pub(super) fn cleanup_state(&self) -> &'static str {
        match self.0.control_retirement.load(std::sync::atomic::Ordering::SeqCst) {
            1|2=>return "pending",4|5=>return "unknown",_=>{},
        }
        match *self.0.state.borrow() {
            CarrierState::NotStarted => "notStarted",
            CarrierState::Open => "pending",
            CarrierState::Closed => "closedObserved",
        }
    }
    pub(super) fn closed_observed(&self) -> bool {
        matches!(self.0.control_retirement.load(std::sync::atomic::Ordering::SeqCst),0|3) && matches!(
            *self.0.state.borrow(),
            CarrierState::NotStarted | CarrierState::Closed
        )
    }
    pub(super) async fn wait_closed(&self, end: Instant) -> AppResult<()> {
        let mut state = self.0.state.subscribe();
        loop {
            let carrier=*state.borrow();
            if matches!(
                carrier,
                CarrierState::NotStarted | CarrierState::Closed
            ) {
                match self.0.control_retirement.load(std::sync::atomic::Ordering::SeqCst) {
                    0|3=>return Ok(()),4=>return Err(unknown()),
                    1=>{self.retire_control_slot(end).await?;continue},_=>{},
                }
            }
            tokio::select! {biased;_=tokio::time::sleep_until(tokio::time::Instant::from_std(end))=>return Err(unknown()),r=state.changed()=>r.map_err(|_|unknown())?}
        }
    }
    async fn retire_control_slot(&self,end:Instant)->AppResult<()> {
        if self.0.control_retirement.compare_exchange(1,2,std::sync::atomic::Ordering::SeqCst,
            std::sync::atomic::Ordering::SeqCst).is_err() {return Ok(())}
        let mut guard=ControlRetirementGuard{observation:self.clone(),completed:false};
        let slot=self.0.control_slot.lock().map_err(|_|unknown())?.take().ok_or_else(unknown)?;
        let observed=tokio::time::timeout_at(tokio::time::Instant::from_std(end),slot.retire_control_carrier_once())
            .await.is_ok_and(|r|r.is_ok());
        self.0.control_retirement.store(if observed{3}else{4},std::sync::atomic::Ordering::SeqCst);
        guard.completed=true;
        let current=*self.0.state.borrow();self.0.state.send_replace(current);
        if observed {self.release_closed();Ok(())}else{Err(unknown())}
    }
    async fn retire_control_before_close(&self,end:Instant)->AppResult<()> {
        let mut changed=self.0.state.subscribe();
        loop {
            match self.0.control_retirement.load(std::sync::atomic::Ordering::SeqCst) {
                0|3=>return Ok(()),4=>return Err(unknown()),
                1=>{self.retire_control_slot(end).await?;continue},_=>{},
            }
            tokio::select!{biased;_=tokio::time::sleep_until(tokio::time::Instant::from_std(end))=>return Err(unknown()),
                r=changed.changed()=>r.map_err(|_|unknown())?}
        }
    }
}
struct ControlRetirementGuard {observation:Observation,completed:bool}
impl Drop for ControlRetirementGuard {
    fn drop(&mut self) {
        if !self.completed {
            self.observation.0.control_retirement.store(4,std::sync::atomic::Ordering::SeqCst);
            let current=*self.observation.0.state.borrow();self.observation.0.state.send_replace(current);
        }
    }
}
struct CarrierEnd(Observation);
impl Drop for CarrierEnd {
    fn drop(&mut self) {
        self.0.abort();
        self.0.0.state.send_replace(CarrierState::Closed);
        self.0.release_closed();
    }
}

struct Pins {
    rows: Vec<KnownHost>,
}
#[async_trait]
impl KnownHostsStore for Pins {
    async fn lookup(
        &self,
        h: &str,
        p: u16,
    ) -> std::result::Result<Vec<KnownHost>, KnownHostsError> {
        Ok(self
            .rows
            .iter()
            .filter(|k| cc_ssh_core::known_hosts::pattern_matches(&k.host_pattern, h, p))
            .cloned()
            .collect())
    }
    async fn add(&self, _: KnownHost) -> std::result::Result<(), KnownHostsError> {
        Err(KnownHostsError("fixed enrollment trust".into()))
    }
    async fn revoke(&self, _: &str) -> std::result::Result<(), KnownHostsError> {
        Err(KnownHostsError("fixed enrollment trust".into()))
    }
    async fn remove(&self, _: ObjectId) -> std::result::Result<(), KnownHostsError> {
        Err(KnownHostsError("fixed enrollment trust".into()))
    }
}
struct NoPrompt;
#[async_trait]
impl HostKeyPrompt for NoPrompt {
    async fn confirm_unknown(&self, _: HostKeyInfo) -> HostKeyDecision {
        HostKeyDecision::Reject
    }
}
struct OriginalCredentials {
    scope: Arc<OriginalScope>,
    resolver: Weak<crate::ssh::VaultCredentialResolver>,
    expected: Vec<Credential>,
}
#[async_trait]
impl CredentialResolver for OriginalCredentials {
    async fn resolve(
        &self,
        c: &Credential,
    ) -> std::result::Result<ResolvedCredential, ResolveError> {
        self.scope
            .check_current()
            .map_err(|_| ResolveError::Cancelled)?;
        if !self.expected.contains(c) {
            return Err(ResolveError::Cancelled);
        }
        let original = self.resolver.upgrade().ok_or(ResolveError::Locked)?;
        let value = tokio::select! {biased;_=self.scope.cancelled()=>return Err(ResolveError::Cancelled),v=original.resolve(c)=>v?};
        self.scope
            .check_current()
            .map_err(|_| ResolveError::Cancelled)?;
        match (&value, c.kind) {
            (ResolvedCredential::Password(_), CredentialKind::Password)
            | (
                ResolvedCredential::PrivateKey { .. },
                CredentialKind::SshPrivateKey | CredentialKind::SshCertificate,
            ) => Ok(value),
            _ => Err(ResolveError::Cancelled),
        }
    }
    // No ambient agent, prompt, proxy or fallback. A bad remembered key fails.
}

/// SAME original guards through one socket poll, including SSH background
/// handshake/control writes. No Session/Unlocked/key owner is held on Pending.
enum CarrierSocket {
    Tcp(tokio::net::TcpStream),
    Relay {
        stream:cc_sync_core::api::RelayBinaryStream,
    },
}
// One original held future, then a sticky terminal state. A completed async
// future is never polled again by a later read/write/flush/shutdown call.
enum CarrierCancellation {
    Direct,
    Armed(Pin<Box<dyn Future<Output=()>+Send>>),
    Aborted,
}
impl CarrierCancellation {
    fn aborted(&mut self,cx:&mut Context<'_>)->bool {
        match self {
            Self::Direct => false,
            Self::Aborted => true,
            Self::Armed(cancelled) => {
                if cancelled.as_mut().poll(cx).is_ready() {
                    *self=Self::Aborted;
                    true
                } else {false}
            }
        }
    }
}
impl CarrierSocket {
    fn poll_read(&mut self,cx:&mut Context<'_>,out:&mut ReadBuf<'_>)->Poll<std::io::Result<()>> {
        match self {Self::Tcp(s)=>Pin::new(s).poll_read(cx,out),Self::Relay{stream,..}=>Pin::new(stream).poll_read(cx,out)}
    }
    fn poll_write(&mut self,cx:&mut Context<'_>,bytes:&[u8])->Poll<std::io::Result<usize>> {
        match self {Self::Tcp(s)=>Pin::new(s).poll_write(cx,bytes),Self::Relay{stream,..}=>Pin::new(stream).poll_write(cx,bytes)}
    }
    fn poll_flush(&mut self,cx:&mut Context<'_>)->Poll<std::io::Result<()>> {
        match self {Self::Tcp(s)=>Pin::new(s).poll_flush(cx),Self::Relay{stream,..}=>Pin::new(stream).poll_flush(cx)}
    }
    fn poll_shutdown(&mut self,cx:&mut Context<'_>)->Poll<std::io::Result<()>> {
        match self {Self::Tcp(s)=>Pin::new(s).poll_shutdown(cx),Self::Relay{stream,..}=>Pin::new(stream).poll_shutdown(cx)}
    }
}
struct Carrier {
    socket: CarrierSocket,
    cancelled: CarrierCancellation,
    scope: Arc<OriginalScope>,
    until: Arc<std::sync::Mutex<Instant>>,
    // Declared LAST: Rust drops the actual socket before publishing closure.
    _end: CarrierEnd,
}
fn cap_read(until: &std::sync::Mutex<Instant>) -> AppResult<Instant> {
    let end = *until.lock().map_err(|_| unknown())?;
    consent_live(end)?;
    Ok(end)
}
fn io_error() -> std::io::Error {
    std::io::Error::other("original enrollment scope retired")
}
impl AsyncRead for Carrier {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        b: &mut ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        let s = self.get_mut();
        // cancelled() checks scope itself on first poll. Poll it OUTSIDE the
        // non-reentrant Personal/WorkingSet guard held below.
        if s.cancelled.aborted(cx) {return Poll::Ready(Err(io_error()))}
        if cap_read(&s.until).is_err() {
            return Poll::Ready(Err(io_error()));
        }
        match s.scope.with_io_current(|_| {
            cap_read(&s.until)?;
            Ok(s.socket.poll_read(cx, b))
        }) {
            Ok(v) => v,
            Err(_) => Poll::Ready(Err(io_error())),
        }
    }
}
impl AsyncWrite for Carrier {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        b: &[u8],
    ) -> Poll<std::io::Result<usize>> {
        let s = self.get_mut();
        if s.cancelled.aborted(cx) {return Poll::Ready(Err(io_error()))}
        if cap_read(&s.until).is_err() {
            return Poll::Ready(Err(io_error()));
        }
        match s.scope.with_io_current(|_| {
            cap_read(&s.until)?;
            Ok(s.socket.poll_write(cx, b))
        }) {
            Ok(v) => v,
            Err(_) => Poll::Ready(Err(io_error())),
        }
    }
    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        let s = self.get_mut();
        if s.cancelled.aborted(cx) {return Poll::Ready(Err(io_error()))}
        if cap_read(&s.until).is_err() {
            return Poll::Ready(Err(io_error()));
        }
        match s.scope.with_io_current(|_| {
            cap_read(&s.until)?;
            Ok(s.socket.poll_flush(cx))
        }) {
            Ok(v) => v,
            Err(_) => Poll::Ready(Err(io_error())),
        }
    }
    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        // Cleanup only, no new protocol request/authority dispatched here.
        let s=self.get_mut();
        if s.cancelled.aborted(cx) {return Poll::Ready(Err(io_error()))}
        s.socket.poll_shutdown(cx)
    }
}
pub(super) struct Peer {
    control_transport:bool,
    scope: Arc<OriginalScope>,
    session: Arc<SshSession>,
    stream: Option<OwnedStream>,
    pub host_id: ObjectId,
    pub host_revision: i64,
    pub route_digest: String,
    requests: Vec<Uuid>,
    request_limit: usize,
    lifetime_protocol: bool,
    authenticated_service_node: Option<protocol::NodeIdentity>,
    reply_bytes: usize,
    until: Arc<std::sync::Mutex<Instant>>,
    observation: Observation,
}
/// russh ChannelStream::Drop spawns a channel-close task. Always enter its
/// captured runtime, even if this wrapper/future is dropped on a plain thread
/// or after that runtime stopped accepting tasks. No current/global lookup.
struct OwnedStream {
    stream: Option<cc_ssh_core::SshStream>,
    runtime: tokio::runtime::Handle,
}
impl Drop for OwnedStream {
    fn drop(&mut self) {
        let _entered = self.runtime.enter();
        drop(self.stream.take());
    }
}
impl std::fmt::Debug for Peer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("EnrollmentPeer([redacted])")
    }
}
async fn bounded<T>(
    scope: &OriginalScope,
    end: std::time::Instant,
    f: impl Future<Output = AppResult<T>>,
) -> AppResult<T> {
    let mut tick = tokio::time::interval(Duration::from_millis(100));
    tokio::pin!(f);
    loop {
        scope.check_current()?;
        tokio::select! {biased;_=scope.cancelled()=>return Err(unknown()),
        _=tokio::time::sleep_until(tokio::time::Instant::from_std(end))=>return Err(unknown()),
        _=tick.tick()=>{},r=&mut f=>{scope.check_current()?;return r}}
    }
}
// Weak original Core is upgraded only for one poll/check, never across I/O Pending.
fn original_core(core:&Weak<crate::app::Inner>)->AppResult<AppCore> {
    Ok(AppCore{inner:core.upgrade().ok_or_else(unavailable)?})
}
async fn check_original_core(core:&Weak<crate::app::Inner>,scope:&OriginalScope)->AppResult<()> {
    let app=original_core(core)?;scope.check(&app).await
}
pub(super) async fn bounded_weak<T>(core:&Weak<crate::app::Inner>,scope:&OriginalScope,end:Instant,
    future:impl Future<Output=AppResult<T>>)->AppResult<T> {
    tokio::pin!(future);
    let guarded=std::future::poll_fn(|cx| {
        let app=match original_core(core){Ok(app)=>app,Err(error)=>return Poll::Ready(Err(error))};
        // Keep this exact original Inner alive for only this synchronous poll.
        // No Session/Unlocked/Core guard is retained while Pending.
        let _poll_owner=app;
        if let Err(error)=scope.check_current(){return Poll::Ready(Err(error));}
        // Carrier already guards each actual socket poll with with_io_current.
        // Do not nest that non-reentrant guard around SSH's outer future.
        let polled=future.as_mut().poll(cx);
        if let Err(error)=scope.check_current(){return Poll::Ready(Err(error));}
        polled
    });
    bounded(scope,end,guarded).await
}
fn validate(plan: &ConnectionPlan) -> AppResult<()> {
    if plan.backend != SshBackend::Native
        || plan.proxy.is_some()
        || !plan.forwards.is_empty()
        || plan.agent_forwarding
        || plan.route.len() > 32
    {
        return Err(unavailable());
    }
    for h in plan.all_hops() {
        let c = h.credential.as_ref().ok_or_else(unavailable)?;
        if h.host_key_policy != HostKeyPolicy::Strict
            || h.endpoint.port == 0
            || h.username.is_empty()
            || h.username.len() > 256
            || h.username.chars().any(char::is_control)
            || c.secret_id.is_none()
            || c.agent_path.is_some()
            || !matches!(
                c.kind,
                CredentialKind::Password
                    | CredentialKind::SshPrivateKey
                    | CredentialKind::SshCertificate
            )
        {
            return Err(unavailable());
        }
    }
    Ok(())
}
impl Peer {
    pub(super) async fn open(
        app: &AppCore,
        scope: Arc<OriginalScope>,
        host_id: ObjectId,
        revision: i64,
    ) -> AppResult<Self> {
        Self::open_observed(app, scope, host_id, revision, Observation::new()).await
    }
    pub(super) async fn open_observed(
        app: &AppCore,
        scope: Arc<OriginalScope>,
        host_id: ObjectId,
        revision: i64,
        observation: Observation,
    ) -> AppResult<Self> {
        Self::open_observed_weak(&Arc::downgrade(&app.inner),scope,host_id,revision,observation).await
    }
    pub(super) async fn open_observed_weak(
        core: &Weak<crate::app::Inner>, scope: Arc<OriginalScope>, host_id: ObjectId,
        revision: i64, observation: Observation,
    ) -> AppResult<Self> {
        Self::open_planned_weak(core,scope,host_id,revision,observation,None).await
    }
    async fn open_planned_weak(
        core:&Weak<crate::app::Inner>,scope:Arc<OriginalScope>,host_id:ObjectId,
        revision:i64,observation:Observation,control:Option<control::OriginalControlInput>,
    )->AppResult<Self> {
        let runtime = tokio::runtime::Handle::try_current().map_err(|_| unknown())?;
        // Planner owns plaintext inventory only. Resolver stays Weak until the
        // actual authentication call; server handlers retain only public pins.
        let (planner, resolver) = {
            let app=original_core(core)?;
            scope.with_resources(&app, |_, u, _| {
                if u.working().revision_of(host_id) != Some(revision) || revision <= 0 {
                    return Err(unavailable());
                }
                Ok((u.ssh.planner.clone(), Arc::downgrade(&u.ssh.resolver)))
            })
            .await?
        }; // No strong Core survives planner/dial/auth waits.
        let end = scope
            .deadline()
            .min(std::time::Instant::now() + Duration::from_secs(30));
        let mut plan = bounded_weak(core, &scope, end, async {
            planner.plan(host_id).await.map_err(|_| unavailable())
        })
        .await?;
        drop(planner);
        // A private subsystem never starts configured forwarding/agent export.
        plan.forwards.clear();
        plan.agent_forwarding = false;
        validate(&plan)?;
        if control.is_some() && (plan.all_hops().len()!=1 || plan.proxy.is_some()) {return Err(unavailable())}
        let pins = {
            let app=original_core(core)?;
            scope.with_resources(&app, |_, u, _| {
                let w = u.working();
                if w.revision_of(host_id) != Some(revision) {
                    return Err(unavailable());
                }
                for h in plan.all_hops() {
                    let host = w.host(h.host_id).ok_or_else(unavailable)?;
                    let c = h.credential.as_ref().ok_or_else(unavailable)?;
                    if w.vault_of(h.host_id).is_some()
                        || host.backend != SshBackend::Native
                        || host.proxy_command.is_some()
                        || crate::sharing_bindings::host_sharing_binding(&host)?.is_some()
                        || [Some(c.id), c.secret_id, c.passphrase_secret_id]
                            .into_iter()
                            .flatten()
                            .any(|id| w.vault_of(id).is_some())
                    {
                        return Err(unavailable());
                    }
                }
                let rows = w.service_key_trust_records().ok_or_else(unavailable)?;
                let rows: Vec<_> = rows
                    .into_iter()
                    .filter(|k| {
                        plan.all_hops().iter().any(|h| {
                            cc_ssh_core::known_hosts::pattern_matches(
                                &k.host_pattern,
                                &h.endpoint.host,
                                h.endpoint.port,
                            )
                        })
                    })
                    .collect();
                if rows.is_empty() {
                    return Err(unavailable());
                }
                Ok(rows)
            })
            .await?
        };
        let digest = hash(&serde_json::to_vec(&(&plan, &pins)).map_err(|_| unavailable())?);
        if control.as_ref().is_some_and(|c|c.route_digest!=digest) {return Err(unavailable())}
        let expected = plan
            .all_hops()
            .into_iter()
            .map(|h| h.credential.ok_or_else(unavailable))
            .collect::<AppResult<Vec<_>>>()?;
        let connector = SshConnector::new(
            Arc::new(OriginalCredentials {
                scope: scope.clone(),
                resolver,
                expected,
            }),
            Arc::new(Pins { rows: pins }),
            Arc::new(NoPrompt),
        );
        let control_transport=control.is_some();
        let original_end=control.as_ref().map(|c|c.deadline).unwrap_or(scope.deadline()).min(scope.deadline());
        consent_live(original_end)?;
        let until=Arc::new(std::sync::Mutex::new(original_end));
        let (socket,end_signal,cancelled)=if let Some(control)=control {
            scope.bind_relay(&control.stream)?;
            observation.bind_relay()?;
            let abort=observation.0.relay_abort.clone();let original=scope.clone();
            (CarrierSocket::Relay{stream:control.stream},CarrierEnd(observation.clone()),
             CarrierCancellation::Armed(Box::pin(async move {
                tokio::select!{biased;_=abort.cancelled()=>{},_=original.cancelled()=>{}}
             })))
        } else {
        let first = plan
            .all_hops()
            .into_iter()
            .next()
            .ok_or_else(unavailable)?
            .endpoint;
        let dial = tokio::net::TcpStream::connect((first.host.as_str(), first.port));
        tokio::pin!(dial);
        let socket = bounded_weak(
            core, &scope,
            end,
            std::future::poll_fn(|cx| {
                match scope.with_io_current(|_| Ok(dial.as_mut().poll(cx))) {
                    Ok(Poll::Ready(Ok(s))) => Poll::Ready(Ok(s)),
                    Ok(Poll::Ready(Err(_))) | Err(_) => Poll::Ready(Err(unavailable())),
                    Ok(Poll::Pending) => Poll::Pending,
                }
            }),
        )
        .await?;
        // A duplicate FD of the SAME connected socket is a private abort-only
        // handle, not another transport/endpoint. Shutdown wakes a pending SSH
        // read when the original scope retires; no protocol close is invented.
        let socket = socket.into_std().map_err(|_| unknown())?;
        observation.bind(socket.try_clone().map_err(|_| unknown())?)?;
        let end_signal = CarrierEnd(observation.clone());
        let socket = tokio::net::TcpStream::from_std(socket).map_err(|_| unknown())?;
        let weak = Arc::downgrade(&observation.0);
        let original = scope.clone();
        runtime.spawn(async move {
            original.cancelled().await;
            if let Some(inner) = weak.upgrade() {
                Observation(inner).abort();
            }
        });
        (CarrierSocket::Tcp(socket),end_signal,CarrierCancellation::Direct)
        };
        let scope_digest = sha2::Sha256::digest(digest.as_bytes()).into();
        let carrier = Carrier {
            socket,
            cancelled,
            scope: scope.clone(),
            until: until.clone(),
            _end: end_signal,
        };
        let session = bounded_weak(core, &scope, end, async {
            connector
                .connect_over_stream(&plan, carrier, scope_digest)
                .await
                .map_err(|_| unavailable())
        })
        .await?;
        let session = Arc::new(session);
        let stream = match bounded_weak(core, &scope, end, async {
            session
                .open_subsystem(SUBSYSTEM)
                .await
                .map_err(|_| unavailable())
        })
        .await
        {
            Ok(s) => s,
            Err(e) => {
                let _ = tokio::time::timeout(Duration::from_secs(2), session.disconnect()).await;
                return Err(e);
            }
        };
        Ok(Self {
            control_transport,
            scope,
            session,
            stream: Some(OwnedStream {
                stream: Some(stream),
                runtime,
            }),
            host_id,
            host_revision: revision,
            route_digest: digest,
            requests: Vec::new(),
            request_limit: 4,
            lifetime_protocol: false,
            authenticated_service_node: None,
            reply_bytes: 0,
            until,
            observation,
        })
    }
    /// Fixed rotation-only five-frame contract. Never a caller limit/exec API.
    pub(super) async fn open_rotation_observed(app:&AppCore,scope:Arc<OriginalScope>,host:ObjectId,
        revision:i64,observation:Observation)->AppResult<Self> {
        let mut peer=Self::open_observed(app,scope,host,revision,observation).await?;
        peer.request_limit=5;Ok(peer)
    }
    pub(super) fn cap_review(&mut self, end: Instant) -> AppResult<()> {
        consent_live(end)?;
        let mut original = self.until.lock().map_err(|_| unknown())?;
        *original = (*original).min(end);
        Ok(())
    }
    pub(super) async fn exchange<T: serde::de::DeserializeOwned>(
        &mut self,
        app: &AppCore,
        kind: &str,
        payload: serde_json::Value,
        expiry: Option<Timestamp>,
    ) -> AppResult<T> {
        let (id, raw) = protocol::request(kind, payload)?;
        self.exchange_frame(app, kind, id, raw, expiry).await
    }
    pub(super) async fn stage(
        &mut self,
        app: &AppCore,
        revision: u64,
        pin: &OwnerPin,
        offer: &Signed<Offer>,
        token: &str,
    ) -> AppResult<protocol::Claimed> {
        let (id, raw) = protocol::stage_request(revision, pin, offer, token)?;
        self.exchange_frame(app, "stageOffer", id, raw, Some(offer.body.expires_at))
            .await
    }
    async fn exchange_frame<T: serde::de::DeserializeOwned>(
        &mut self,
        app: &AppCore,
        kind: &str,
        id: Uuid,
        raw: Zeroizing<Vec<u8>>,
        expiry: Option<Timestamp>,
    ) -> AppResult<T> {
        self.exchange_frame_weak(&Arc::downgrade(&app.inner),kind,id,raw,expiry).await
    }
    pub(super) async fn exchange_weak<T: serde::de::DeserializeOwned>(
        &mut self,core:&Weak<crate::app::Inner>,kind:&str,payload:serde_json::Value,
        expiry:Option<Timestamp>,
    )->AppResult<T> {
        let(id,raw)=protocol::request(kind,payload)?;
        self.exchange_frame_weak(core,kind,id,raw,expiry).await
    }
    async fn exchange_frame_weak<T: serde::de::DeserializeOwned>(
        &mut self,core:&Weak<crate::app::Inner>,kind:&str,id:Uuid,
        raw:Zeroizing<Vec<u8>>,expiry:Option<Timestamp>,
    )->AppResult<T> {
        self.exchange_frame_weak_with_budget(core,kind,id,raw,expiry,Duration::from_secs(5)).await
    }
    // Private transport storage only. Public request JSON cannot choose this cap.
    // The closed collector child uses its fixed startup allowance; all existing
    // RPCs above retain the original five-second reply bound.
    async fn exchange_frame_weak_with_budget<T: serde::de::DeserializeOwned>(
        &mut self,core:&Weak<crate::app::Inner>,kind:&str,id:Uuid,
        raw:Zeroizing<Vec<u8>>,expiry:Option<Timestamp>,reply_budget:Duration,
    )->AppResult<T> {
        let consent_end = cap_read(&self.until)?;
        bounded(&self.scope, consent_end, check_original_core(core,&self.scope)).await?;
        if self.requests.len() >= self.request_limit || self.requests.contains(&id) || self.session.is_closed() {
            return Err(unknown());
        }
        self.requests.push(id); // Any dispatched or ambiguous request is consumed.
        let mut end = self
            .scope
            .deadline()
            .min(consent_end)
            .min(std::time::Instant::now() + reply_budget);
        if let Some(expiry) = expiry {
            end = end.min(std::time::Instant::now() + self.scope.remaining_until(expiry)?)
        }
        let scope = self.scope.clone();
        let stream = self
            .stream
            .as_mut()
            .and_then(|owned| owned.stream.as_mut())
            .ok_or_else(unknown)?;
        let response = bounded_weak(core, &scope, end, async {
            stream.write_all(&raw).await.map_err(|_| unknown())?;
            stream.flush().await.map_err(|_| unknown())?;
            let mut out = Zeroizing::new(Vec::new());
            let mut block = Zeroizing::new([0u8; 1024]);
            loop {
                let n = stream
                    .read(block.as_mut_slice())
                    .await
                    .map_err(|_| unknown())?;
                if n == 0 {
                    return Err(unknown());
                }
                out.extend_from_slice(&block[..n]);
                if out.len() > protocol::MAX_FRAME {
                    return Err(unknown());
                }
                if let Some(at) = out.iter().position(|b| *b == b'\n') {
                    if at + 1 != out.len() {
                        return Err(unknown());
                    }
                    break;
                }
            }
            if kind == "observeCertificate" {
                // Fixed worker is terminal after its fourth frame. Require its
                // actual channel EOF and reject an appended/trailing reply.
                if stream.read(&mut block[..1]).await.map_err(|_| unknown())? != 0 {
                    return Err(unknown());
                }
            }
            Ok(out)
        })
        .await?;
        self.reply_bytes = self
            .reply_bytes
            .checked_add(response.len())
            .ok_or_else(unknown)?;
        if self.reply_bytes > 65536 {
            return Err(unknown());
        }
        bounded(&self.scope, consent_end, check_original_core(core,&self.scope)).await?;
        protocol::reply(&response, id, kind)
    }
}
impl Drop for Peer {
    fn drop(&mut self) {
        self.observation.abort();
        if self.control_transport {
            // Abort wakes the owned russh carrier poll; dropping its actual
            // Relay stream precedes CarrierEnd. No detached cleanup task is
            // created on this path and no protocol closure is a drain proof.
            drop(self.stream.take());return
        }
        let session = self.session.clone();
        if let Some(stream) = self.stream.take() {
            let runtime = stream.runtime.clone();
            runtime.spawn(async move {
                let _ = tokio::time::timeout(Duration::from_secs(2), session.disconnect()).await;
                drop(stream);
            });
        }
    }
}
#[cfg(test)]
#[path = "transport_live.rs"]
mod live;
#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test(flavor="multi_thread",worker_threads=2)]
    async fn original_relay_cancellation_latches_across_actual_carrier_io_until_drop() {
        let (_root,app,scope)=super::super::tests::local().await;
        let (a,mut peer)=pair().await;
        let socket=a.into_std().unwrap();let observation=Observation::new();
        observation.bind(socket.try_clone().unwrap()).unwrap();
        let socket=tokio::net::TcpStream::from_std(socket).unwrap();
        let abort=observation.0.relay_abort.clone();let original=scope.clone();
        // Exercise the real Carrier I/O/drop implementation and SAME held
        // relay-cancellation future using an owned loopback socket. No fake
        // RelaySlot, authenticated subject or relay authority is created.
        let mut carrier=Carrier {
            socket:CarrierSocket::Tcp(socket),
            cancelled:CarrierCancellation::Armed(Box::pin(async move {
                tokio::select!{biased;_=abort.cancelled()=>{},_=original.cancelled()=>{}}
            })),
            until:Arc::new(std::sync::Mutex::new(scope.deadline())),scope,
            _end:CarrierEnd(observation.clone()),
        };
        carrier.write_all(b"A").await.unwrap();
        let mut byte=[0];peer.read_exact(&mut byte).await.unwrap();assert_eq!(byte,[b'A']);
        observation.abort();
        assert!(carrier.read_exact(&mut byte).await.is_err());
        assert!(carrier.write_all(b"B").await.is_err());
        assert!(carrier.flush().await.is_err());
        assert!(carrier.shutdown().await.is_err());
        assert!(carrier.read_exact(&mut byte).await.is_err());
        assert!(matches!(carrier.cancelled,CarrierCancellation::Aborted));
        assert_eq!(observation.cleanup_state(),"pending");
        drop(carrier);
        assert_eq!(observation.cleanup_state(),"closedObserved");
        app.shutdown().await.unwrap();
    }

    async fn pair() -> (tokio::net::TcpStream, tokio::net::TcpStream) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let (a, b) = tokio::join!(tokio::net::TcpStream::connect(address), listener.accept());
        (a.unwrap(), b.unwrap().0)
    }
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn actual_carrier_denies_later_control_bytes_after_original_working_set_mutation() {
        let (_root, app, scope) = super::super::tests::local().await;
        let (a, mut b) = pair().await;
        let until = Arc::new(std::sync::Mutex::new(scope.deadline()));
        let mut carrier = Carrier {
            socket: CarrierSocket::Tcp(a),
            cancelled: CarrierCancellation::Direct,
            scope,
            until,
            _end: CarrierEnd(Observation::new()),
        };
        carrier.write_all(b"A").await.unwrap();
        let mut byte = [0];
        b.read_exact(&mut byte).await.unwrap();
        assert_eq!(&byte, b"A");
        let (owner, u) = app.unlocked().await.unwrap();
        u.working().clear();
        drop(u);
        drop(owner);
        assert!(carrier.write_all(b"B").await.is_err());
        assert!(
            tokio::time::timeout(Duration::from_millis(30), b.read(&mut byte))
                .await
                .is_err()
        );
        app.shutdown().await.unwrap();
    }
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn saved_revision_or_original_lock_denies_before_any_tcp_attempt() {
        let (_root, app, initial) = super::super::tests::local().await;
        drop(initial);
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let credential = app
            .add_password_credential(
                "Owned pre-dial fixture".into(),
                Some("enrollment".into()),
                "owned enrollment fixture password42".into(),
            )
            .await
            .unwrap();
        let mut host = crate::HostDto::new("Owned selected enrollment", "127.0.0.1");
        host.port = Some(listener.local_addr().unwrap().port());
        host.username = Some("enrollment".into());
        host.credential_id = Some(credential.id);
        host.host_key_policy = HostKeyPolicy::Strict;
        let host = app.save_host(host).await.unwrap();
        let id = crate::dto::parse_id("host", &host.id).unwrap();
        let revision = app
            .unlocked()
            .await
            .unwrap()
            .1
            .working()
            .revision_of(id)
            .unwrap();
        let scope = Arc::new(
            OriginalScope::capture(&app, &CancellationToken::new(), Duration::from_secs(30))
                .await
                .unwrap(),
        );
        assert!(
            Peer::open(&app, scope.clone(), id, revision + 1)
                .await
                .is_err()
        );
        app.lock().await.unwrap();
        assert!(Peer::open(&app, scope, id, revision).await.is_err());
        assert!(
            tokio::time::timeout(Duration::from_millis(30), listener.accept())
                .await
                .is_err()
        );
    }
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn actual_carrier_original_consent_deadline_blocks_later_bytes_without_scope_expiry() {
        let (_root, app, scope) = super::super::tests::local().await;
        let (a, mut b) = pair().await;
        let until = Arc::new(std::sync::Mutex::new(
            Instant::now() + Duration::from_millis(200),
        ));
        let mut carrier = Carrier {
            socket: CarrierSocket::Tcp(a),
            cancelled: CarrierCancellation::Direct,
            scope: scope.clone(),
            until: until.clone(),
            _end: CarrierEnd(Observation::new()),
        };
        carrier.write_all(b"A").await.unwrap();
        let mut byte = [0];
        b.read_exact(&mut byte).await.unwrap();
        let end = *until.lock().unwrap();
        tokio::time::sleep_until(tokio::time::Instant::from_std(end)).await;
        scope.check_current().unwrap(); // The 300/30s owner scope is STILL live.
        assert!(carrier.write_all(b"B").await.is_err());
        assert!(
            tokio::time::timeout(Duration::from_millis(30), b.read(&mut byte))
                .await
                .is_err()
        );
        app.shutdown().await.unwrap();
    }
}

/// Uses the SAME production Carrier/drop order, holding only actual owned
/// sockets. Test control delays resource destruction, never authorizes SSH.
#[cfg(test)]
pub(super) fn held_carrier_for_test(
    socket: tokio::net::TcpStream,
    scope: Arc<OriginalScope>,
    observation: Observation,
    release: tokio::sync::oneshot::Receiver<()>,
) -> AppResult<impl Future<Output = ()> + Send> {
    let socket = socket.into_std().map_err(|_| unknown())?;
    observation.bind(socket.try_clone().map_err(|_| unknown())?)?;
    let end = CarrierEnd(observation);
    let socket = tokio::net::TcpStream::from_std(socket).map_err(|_| unknown())?;
    let carrier = Carrier {
        socket:CarrierSocket::Tcp(socket),
        cancelled: CarrierCancellation::Direct,
        until: Arc::new(std::sync::Mutex::new(scope.deadline())),
        scope,
        _end: end,
    };
    Ok(async move {
        let _ = release.await;
        drop(carrier)
    })
}

// Closed invitation frame only. Raw tokens never enter a serde Value or log.
impl Peer {
    pub(super) async fn stage_weak(
        &mut self, core: &Weak<crate::app::Inner>, revision: u64,
        pin: &OwnerPin, offer: &Signed<Offer>, token: &str,
    ) -> AppResult<protocol::Claimed> {
        let (id, raw) = protocol::stage_request(revision, pin, offer, token)?;
        self.exchange_frame_weak(core, "stageOffer", id, raw, Some(offer.body.expires_at)).await
    }
}

#[path="transport_service.rs"]
mod collector_service;

#[path="transport_lifetime.rs"]
mod lifetime;

#[path = "transport_exporter.rs"]
mod exporter;
pub(super) use exporter::ExporterTerminalObservation;
