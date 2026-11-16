//! Explicit detach/attach of ONE live signed operation. The index is weak and
//! process-local; it cannot recreate a grant, original parent or expired task.
use super::*;
use cc_storage_core::{NodeAssociationRecord, NodeTunnelDestination};
use std::sync::{OnceLock, Weak, atomic::{AtomicU64, AtomicU8, Ordering}};

const MAX_RETAINED:usize=128;

const OWNED:u8=0;
const DETACHING:u8=1;
const DETACHED:u8=2;
const RELEASED:u8=3;
const ACTING:u8=4;
const WITHDRAWN:u8=5;
/// Finalization belongs to EACH issued UI operation, never the shared route.
/// A detached old operation stays detached after another operation claims it.
pub(super) struct Custody {state:AtomicU8,ticket:AtomicU64}
impl Custody {
    pub(super) fn new()->Self {Self{state:AtomicU8::new(OWNED),ticket:AtomicU64::new(0)}}
    pub(super) fn release(&self)->bool {
        loop {
            let state=self.state.load(Ordering::SeqCst);
            if state==DETACHED || state==RELEASED || state==WITHDRAWN {return false}
            if self.state.compare_exchange(state,RELEASED,Ordering::SeqCst,Ordering::SeqCst).is_ok() {
                return true
            }
        }
    }
    pub(super) fn begin_action(&self)->AppResult<ActionGuard<'_>> {
        self.state.compare_exchange(OWNED,ACTING,Ordering::SeqCst,Ordering::SeqCst)
            .map_err(|_|unavailable())?;
        Ok(ActionGuard(self))
    }
    pub(super) fn check_read(&self)->AppResult<()> {
        if self.state.load(Ordering::SeqCst)==DETACHED {return Err(unavailable())}
        Ok(())
    }
    fn withdraw_ticket(&self,tickets:&Tickets)->bool {
        if self.state.load(Ordering::SeqCst)!=DETACHED {return false}
        if !tickets.take(self.ticket.load(Ordering::SeqCst)) {return false}
        self.state.store(WITHDRAWN,Ordering::SeqCst);true
    }
    fn begin_detach(&self)->AppResult<DetachGuard<'_>> {
        self.state.compare_exchange(OWNED,DETACHING,Ordering::SeqCst,Ordering::SeqCst)
            .map_err(|_|unavailable())?;
        Ok(DetachGuard{custody:self,committed:false})
    }
}
pub(super) struct ActionGuard<'a>(&'a Custody);
impl Drop for ActionGuard<'_> {
    fn drop(&mut self) {
        let _=self.0.state.compare_exchange(ACTING,OWNED,Ordering::SeqCst,Ordering::SeqCst);
    }
}
struct DetachGuard<'a> {custody:&'a Custody,committed:bool}
impl DetachGuard<'_> {
    fn commit(mut self,ticket:u64)->AppResult<()> {
        self.custody.ticket.store(ticket,Ordering::SeqCst);
        self.custody.state.compare_exchange(DETACHING,DETACHED,Ordering::SeqCst,Ordering::SeqCst)
            .map_err(|_|unavailable())?;
        self.committed=true;Ok(())
    }
}
impl Drop for DetachGuard<'_> {
    fn drop(&mut self) {
        if !self.committed {
            let _=self.custody.state.compare_exchange(DETACHING,OWNED,Ordering::SeqCst,Ordering::SeqCst);
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Key {
    association:String, listen_ip:String, listen_port:u16,
    destination_ip:String, destination_port:u16, ttl_seconds:u32, idle_seconds:u32,
}
impl Key {
    fn from_request(request:&Request)->Self {
        Self{association:request.association_id.clone(),listen_ip:request.listen_ip.clone(),
            listen_port:request.listen_port,destination_ip:request.destination_ip.clone(),
            destination_port:request.destination_port,ttl_seconds:request.ttl_seconds,
            idle_seconds:request.idle_seconds}
    }
}

/// Native-only handoff generation. Zero means no unclaimed handoff. It is
/// never serialized or accepted from a caller and is not a route permission.
struct Tickets {next:AtomicU64,available:AtomicU64}
impl Tickets {
    fn new()->Self {Self{next:AtomicU64::new(0),available:AtomicU64::new(0)}}
    fn allocate(&self)->AppResult<u64> {
        self.next.fetch_update(Ordering::SeqCst,Ordering::SeqCst,|n|n.checked_add(1))
            .map(|n|n+1).map_err(|_|unavailable())
    }
    fn take(&self,ticket:u64)->bool {
        ticket!=0 && self.available.compare_exchange(ticket,0,Ordering::SeqCst,Ordering::SeqCst).is_ok()
    }
}

pub(super) struct Registration {
    key:Key, row:NodeAssociationRecord, opted:bool, tickets:Tickets, revocation:Mutex<RevocationSlot>,
}
impl Registration {
    pub(super) fn new(request:&Request,row:NodeAssociationRecord)->Self {
        Self{key:Key::from_request(request),row,opted:request.resume_original,
            tickets:Tickets::new(),revocation:Mutex::new(RevocationSlot::Unarmed)}
    }
    pub(super) fn disarm(&self) {self.tickets.available.store(0,Ordering::SeqCst)}
    pub(super) fn saved_host(&self)->ObjectId {self.row.public.saved_host_id}
    pub(super) fn association_id(&self)->Uuid {self.row.public.association_id}
    fn is_retained(&self)->bool {self.tickets.available.load(Ordering::SeqCst)!=0}
}

// No strong Shared/Core/session/key/SSH/socket is kept by this index. The
// original bounded task is the only owner that can outlive the released UI.
struct Index<T> { entries:Vec<Weak<T>>, monitoring:Vec<Weak<T>> }
impl<T> Index<T> {
    fn new()->Self {Self{entries:Vec::new(),monitoring:Vec::new()}}
    fn prune(&mut self) {
        self.entries.retain(|weak|weak.strong_count()!=0);
        self.monitoring.retain(|weak|weak.strong_count()!=0);
    }
    fn insert(&mut self,value:&Arc<T>)->AppResult<()> {
        self.prune();
        if self.entries.iter().any(|weak|Weak::ptr_eq(weak,&Arc::downgrade(value))) {return Ok(())}
        if self.entries.len()>=MAX_RETAINED {return Err(unavailable())}
        self.entries.push(Arc::downgrade(value));Ok(())
    }
}
fn index()->&'static Mutex<Index<Shared>> {
    static INDEX:OnceLock<Mutex<Index<Shared>>>=OnceLock::new();
    INDEX.get_or_init(||Mutex::new(Index::new()))
}
// Same bounded weak index, separate from detach tickets. A lookup neither
// claims UI custody nor keeps an original grant alive.
pub(super) fn track_monitoring(shared:&Arc<Shared>)->AppResult<()> {
    let mut registry=index().lock().map_err(|_|unknown())?;
    registry.prune();
    if registry.monitoring.iter().any(|w|Weak::ptr_eq(w,&Arc::downgrade(shared))) {return Ok(())}
    if registry.monitoring.len()>=MAX_RETAINED {return Err(unavailable())}
    registry.monitoring.push(Arc::downgrade(shared));Ok(())
}
pub(super) fn monitoring_candidates()->AppResult<Vec<Weak<Shared>>> {
    let mut registry=index().lock().map_err(|_|unknown())?;
    registry.prune();Ok(registry.monitoring.clone())
}

// Native-only lifecycle of the ORIGINAL explicit revoke task. Pending merely
// stops bytes; only that task's verified Node readbacks + durable commit can end
// it as Verified. EOF, Blocked metadata and serialized booleans cannot do so.
#[derive(Clone,Copy)]
enum RevocationCompletion { Pending, Verified, Failed }
enum RevocationSlot {
    Unarmed, Waiting(watch::Receiver<RevocationCompletion>), Retired,
}
pub(in crate::agent_tunnel::association::public) struct OriginalRevocation {
    context:Context, sender:Option<watch::Sender<RevocationCompletion>>,
}
impl OriginalRevocation {
    /// Called solely after verify_readback(apply), verify_readback(observe),
    /// complete_revocation and the SAME original context's final fence.
    pub(in crate::agent_tunnel::association::public) fn verified(mut self,context:&Context)->AppResult<()> {
        if !Weak::ptr_eq(&self.context.core,&context.core)
            || !self.context.scope.same_current_capture(&context.scope)? {return Err(unavailable())}
        self.sender.take().ok_or_else(unknown)?.send_replace(RevocationCompletion::Verified);
        Ok(())
    }
}
impl Drop for OriginalRevocation {
    fn drop(&mut self) {
        if let Some(sender)=self.sender.take() {sender.send_replace(RevocationCompletion::Failed);}
    }
}
/// Arm BEFORE the durable Blocked write/remote dispatch can wake a route's
/// watcher or EOF. Reuse the existing bounded weak index, never keep a route,
/// Core, secret or live grant alive. This can only stop the original route.
pub(in crate::agent_tunnel::association::public) fn original_revocation_started(context:&Context,row:&NodeAssociationRecord)->AppResult<OriginalRevocation> {
    context.scope.check_current()?;
    let(sender,_)=watch::channel(RevocationCompletion::Pending);
    let original=OriginalRevocation{context:context.clone(),sender:Some(sender)};
    for weak in monitoring_candidates()? {
        let Some(shared)=weak.upgrade() else{continue};
        if !Weak::ptr_eq(&shared.context.core,&context.core) || shared.automatic.row!=*row
            || !shared.context.scope.same_current_capture(&context.scope).unwrap_or(false) {continue}
        let mut slot=shared.automatic.revocation.lock().map_err(|_|unknown())?;
        if !matches!(*slot,RevocationSlot::Unarmed) {continue}
        *slot=RevocationSlot::Waiting(original.sender.as_ref().ok_or_else(unknown)?.subscribe());
        // Cause is installed BEFORE waking the original cancellation branch.
        // Do not cancel the shared scope: final metadata remains fenced by it.
        shared.automatic.disarm();shared.control.abort();
        if let Some(route)=shared.state.lock().map_err(|_|unknown())?.route.as_ref(){route.cancel();};
    }
    Ok(original)
}
async fn completion_event(mut receiver:watch::Receiver<RevocationCompletion>,
    scope:&OriginalScope,end:Instant)->RouteAuditEnd {
    loop {
        if scope.check_current().is_err() || Instant::now()>=end {return RouteAuditEnd::Unknown}
        let state=*receiver.borrow_and_update();
        match state {
            RevocationCompletion::Verified=>return RouteAuditEnd::Revoked,
            RevocationCompletion::Failed=>return RouteAuditEnd::Unknown,
            RevocationCompletion::Pending=>{},
        }
        tokio::select!{biased;
            _=scope.cancelled()=>return RouteAuditEnd::Unknown,
            _=tokio::time::sleep_until(tokio::time::Instant::from_std(end))=>return RouteAuditEnd::Unknown,
            result=receiver.changed()=>if result.is_err(){return RouteAuditEnd::Unknown},
        }
    }
}
/// Called after data-plane cancellation and owned stream/control retirement.
/// A known original revoke task may complete within the original absolute
/// deadline; no timer grace, retry, TTL extension or new request is introduced.
pub(super) async fn after_original_transport_end(shared:&Shared,end:Instant)->Option<RouteAuditEnd> {
    let receiver={
        let Ok(mut slot)=shared.automatic.revocation.lock() else{return Some(RouteAuditEnd::Unknown)};
        match std::mem::replace(&mut *slot,RevocationSlot::Retired) {
            RevocationSlot::Unarmed=>return None,
            RevocationSlot::Retired=>return Some(RouteAuditEnd::Unknown),
            RevocationSlot::Waiting(receiver)=>receiver,
        }
    };
    Some(completion_event(receiver,&shared.context.scope,end).await)
}

fn active(shared:&Shared)->AppResult<Arc<NativeTunnelRouteV2>> {
    if shared.context.core.strong_count()==0 {return Err(unavailable())}
    shared.context.scope.check_current()?;
    if !shared.automatic.opted {return Err(unavailable())}
    let route={
        let state=shared.state.lock().map_err(|_|unknown())?;
        if !state.consumed || !state.busy || state.orphaned || state.joined
            || !matches!(state.view["phase"].as_str(),Some("listening"|"connecting"|"connected"))
            || state.view["boundAddress"].as_str().is_none() {return Err(unavailable())}
        state.route.clone().ok_or_else(unavailable)?
    };
    // Verifies the actual signed opt-in, original binding, SAME Budget/idle,
    // absolute expiry and stop state. A preference/retained bit cannot pass it.
    route.check_original_resume_window()?;
    Ok(route)
}
/// Explicit UI cancel can retract ONLY its own unclaimed handoff. Lock the
/// same small synchronous index used by commit, so cancel cannot slip between
/// the local Detached transition and publication of that generation.
pub(super) fn withdraw(shared:&Shared,custody:&Custody)->bool {
    let Ok(_registry)=index().lock() else{return false};
    custody.withdraw_ticket(&shared.automatic.tickets)
}

pub(super) fn retained_live(shared:&Shared)->bool {
    shared.automatic.is_retained() && active(shared).is_ok()
}

async fn preference_current(context:&Context,registration:&Registration)->AppResult<()> {
    if !registration.opted {return Err(unavailable())}
    let id=registration.row.public.association_id;
    let(storage,owner,row)=preference::original(context,id).await?;
    if row!=registration.row {return Err(unavailable())}
    let target=NodeTunnelDestination{association_id:id,
        destination_ip:registration.key.destination_ip.clone(),destination_port:registration.key.destination_port};
    let original=context.scope.clone();let expected=row.clone();
    let enabled=storage.read(move|tx|original.with_io_current(|_| {
        let snapshot=tx.node_association_snapshot(&owner)?;
        if snapshot.records().iter().find(|r|r.public.association_id==id)!=Some(&expected) {
            return Err(unavailable())
        }
        tx.node_tunnel_resume_preference(&owner,&target).map_err(AppError::from)
    })).await?;
    context.scope.check_current()?;
    if !enabled {return Err(unavailable())}
    Ok(())
}

pub(super) async fn detach(shared:&Arc<Shared>,custody:&Custody)->AppResult<String> {
    let custody=custody.begin_detach()?;
    shared.context.check().await?;
    let original=shared.context.scope.clone();
    bounded_original(&original,async {
        active(shared)?;
        preference_current(&shared.context,&shared.automatic).await?;
        active(shared)?;
        // Serialize a distinct receipt, never a Closed/join assertion. No
        // existing route phase or audit/counter/transport state is rewritten.
        let receipt={
            let state=shared.state.lock().map_err(|_|unknown())?;
            shared.context.publish(json!({"phase":"detached",
                "associationId":shared.automatic.row.public.association_id,
                "destination":{"protocol":"tcp","ip":shared.automatic.key.destination_ip,
                    "port":shared.automatic.key.destination_port},
                "boundAddress":state.view["boundAddress"],"grantExpiresUtc":state.view["grantExpiresUtc"],
                "remainingMs":0,"originalOperationRetained":true,"cleanupState":"pending",
                "remoteDrainObserved":false,"countersExact":false,"nativeAccountingOnly":true}),8192)?
        };
        let mut registry=index().lock().map_err(|_|unknown())?;
        // Refuse ambiguous automatic ownership; two manually approved grants
        // are not merged or silently selected as a replacement for each other.
        for weak in &registry.entries {
            if let Some(previous)=weak.upgrade() {
                if !Arc::ptr_eq(&previous,shared) && previous.automatic.key==shared.automatic.key
                    && Weak::ptr_eq(&previous.context.core,&shared.context.core)
                    && previous.context.scope.same_current_capture(&shared.context.scope)? {
                    return Err(unavailable())
                }
            }
        }
        active(shared)?;
        registry.insert(shared)?;
        // No await/fallible work follows this per-handle commit. Attach cannot
        // see availability until the old finalizer is permanently detached.
        let ticket=shared.automatic.tickets.allocate()?;
        custody.commit(ticket)?;
        shared.automatic.tickets.available.store(ticket,Ordering::SeqCst);
        Ok(receipt)
    }).await
}

pub(super) async fn attach(context:Context,request:&Request)->AppResult<Option<AgentTunnelRouteOperation>> {
    if !request.resume_original {return Ok(None)}
    let key=Key::from_request(request);
    let candidate={
        let mut registry=index().lock().map_err(|_|unknown())?;
        registry.prune();
        let mut found=None;
        for weak in &registry.entries {
            if let Some(shared)=weak.upgrade() {
                if shared.automatic.key!=key || !Weak::ptr_eq(&shared.context.core,&context.core) {continue}
                if !shared.context.scope.same_current_capture(&context.scope)? {continue}
                if found.is_some() {return Err(unknown())}
                let ticket=shared.automatic.tickets.available.load(Ordering::SeqCst);
                if ticket==0 {return Err(unavailable())}
                found=Some((shared,ticket));
            }
        }
        found
    };
    let Some((shared,ticket))=candidate else{return Ok(None)};
    if !retained_live(&shared) {return Err(unavailable())}
    // Reads are fenced by the NEW UI context AND the original task's context.
    // Only the original context/operation is returned; no re-sign or recapture.
    preference_current(&context,&shared.automatic).await?;
    preference_current(&shared.context,&shared.automatic).await?;
    context.check().await?;
    if !retained_live(&shared) {return Err(unavailable())}
    // Exactly one concurrent attach claims the retained original. This is
    // availability/custody only; it creates no grant or new original task.
    if !shared.automatic.tickets.take(ticket) {return Err(unavailable())}
    Ok(Some(AgentTunnelRouteOperation{shared,custody:Custody::new()}))
}

#[cfg(test)]
mod tests;

#[cfg(any(test,all(feature="test-harness",target_os="linux")))]
#[path="automatic/revocation_completion_cases.rs"]
mod revocation_completion_cases;
#[cfg(all(feature="test-harness",target_os="linux"))]
impl AppCore {
    pub(crate) async fn __owned_revocation_pending_verified() { revocation_completion_cases::blocked_then_transport_eof_waits_for_verified_original_revocation_completion().await; }
    pub(crate) async fn __owned_revocation_failed_completion() { revocation_completion_cases::failed_or_lost_original_revoke_completion_after_eof_stays_unknown().await; }
    pub(crate) async fn __owned_revocation_deadline_generation() { revocation_completion_cases::original_revocation_cause_cannot_extend_deadline_or_cross_unlock_generation().await; }
}
