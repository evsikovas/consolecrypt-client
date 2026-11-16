//! Issued original-owner TCP operation. JSON is intent, never a route grant.
//! One captured owner survives review, TLS load, signing, listener and pump.
use super::*;
use crate::agent_tunnel::v2::{NativeTunnelRouteV2, TunnelRouteReviewV2};
use super::super::access_audit::{RouteAudit,RouteAuditEnd};
use std::sync::Mutex;
use tokio::{sync::{oneshot, watch}, task::JoinHandle};
mod producer;
mod preference;
mod resume;
mod automatic;
/// Private Native cause only; no caller flag or stored grant can construct it.
pub(super) use automatic::original_revocation_started;
mod monitoring;
pub(crate) use monitoring::MonitoringTunnelRoute;
fn unavailable()->AppError {AppError::invalid("agent_tunnel_route","route_unavailable")}
fn unknown()->AppError {AppError::invalid("agent_tunnel_route","route_unknown")}
#[cfg(test)]
#[path="route_tests.rs"]
mod tests;

#[derive(Deserialize)]
#[serde(rename_all="camelCase",deny_unknown_fields)]
struct Request {
    version:u8, profile_id:String, notification_scope_id:String, association_id:String,
    listen_ip:String, listen_port:u16, destination_ip:String, destination_port:u16,
    ttl_seconds:u32, idle_seconds:u32,
    #[serde(default)]
    resume_original:bool,
}
impl Request {
    fn validate(&self)->AppResult<()> {
        if self.version!=1 || !matches!(self.listen_ip.as_str(),"127.0.0.1"|"::1")
            || !matches!(self.destination_ip.as_str(),"127.0.0.1"|"::1")
            || self.destination_port==0 || !(1..=60).contains(&self.ttl_seconds)
            || self.idle_seconds==0 || self.idle_seconds>30 || self.idle_seconds>self.ttl_seconds {
            return Err(unavailable())
        }
        Ok(())
    }
}
// Only internally-auth-fenced Relay API setup futures use this Native frame.
// The API keeps its original ReaderAuthState locks; this outer frame never
// reacquires that Personal auth mutex. The SSH Carrier
// already has its own actual-socket guard and must not be nested here.
async fn relay_original<T>(context:&Context,expiry:Timestamp,
    future:impl std::future::Future<Output=AppResult<T>>)->AppResult<T> {
    let end=context.scope.deadline().min(Instant::now()+context.scope.remaining_until(expiry)?);
    tokio::pin!(future);
    let guarded=std::future::poll_fn(|cx|context.scope.with_api_setup_current(|_| {
        Ok(std::future::Future::poll(future.as_mut(),cx))
    }).unwrap_or_else(|error|std::task::Poll::Ready(Err(error))));
    transport::bounded_weak(&context.core,&context.scope,end,guarded).await
}
struct State {
    ready:Option<producer::Prepared>, route:Option<Arc<NativeTunnelRouteV2>>,
    task:Option<JoinHandle<()>>, audit:Option<Arc<RouteAudit>>, view:Value, consumed:bool, busy:bool, orphaned:bool, joined:bool,
}
struct Shared {
    context:Context, state:Mutex<State>, done:watch::Sender<bool>,
    control:resume::Controls,
    automatic:automatic::Registration,
    monitoring:monitoring::AcceptedSource,
}
/// Opaque issued operation; not deserializable, clonable as authority or reusable.
pub struct AgentTunnelRouteOperation { shared:Arc<Shared>, custody:automatic::Custody }
impl std::fmt::Debug for AgentTunnelRouteOperation {
    fn fmt(&self,f:&mut std::fmt::Formatter<'_>)->std::fmt::Result {f.write_str("AgentTunnelRouteOperation([redacted])")}
}
impl Drop for AgentTunnelRouteOperation {fn drop(&mut self){self.release_ui()}}
impl Shared {
    fn cancel(&self) {
        self.monitoring.close_pending();
        self.automatic.disarm();
        self.context.cancel.cancel(); self.control.abort();
        if let Ok(mut state)=self.state.lock() {
            if let Some(route)=&state.route {route.cancel()}
            if !state.busy&&!state.consumed {state.ready.take();state.view["phase"]=json!("cancelled");}
        }
    }
    fn publish(&self)->AppResult<String> {
        let state=self.state.lock().map_err(|_|unknown())?;
        let mut view=state.view.clone();
        if state.busy && state.view["phase"]=="connecting"
            && state.audit.as_ref().is_some_and(|audit|audit.opened_observed()) {
            // Genuine same-TLS Node ACK AND durable encrypted Opened succeeded.
            // A correlated transport start reply never changes this state.
            view["phase"]=json!("connected");view["actorAdmission"]=json!(true);
        }
        self.context.publish(view,16384)
    }
    fn phase(&self,phase:&str) {
        if let Ok(mut state)=self.state.lock(){state.view["phase"]=json!(phase);}
    }
    fn finished(&self,outcome:&str,closed:bool,sent:u64,received:u64) {
        self.monitoring.close_pending();
        self.automatic.disarm();
        if let Ok(mut state)=self.state.lock() {
            state.route.take();state.ready.take();state.busy=false;
            let admitted=state.audit.take().is_some_and(|audit|audit.opened_observed());
            state.view["actorAdmission"]=json!(admitted);
            state.view["phase"]=json!(outcome);
            state.view["cleanupState"]=json!(if closed&&!state.orphaned{"closedObserved"}else{"unknown"});
            state.view["sentBytes"]=json!(sent.to_string());state.view["receivedBytes"]=json!(received.to_string());
            // Remote Agent joins/CAS are not inferred from local EOF or StopRequested.
            state.view["remoteDrainObserved"]=json!(false);
            state.view["countersExact"]=json!(false);state.view["nativeAccountingOnly"]=json!(true);
        }
        self.done.send_replace(true);
    }
}
struct WorkGuard {shared:Arc<Shared>,finished:bool}
impl Drop for WorkGuard {
    fn drop(&mut self) {
        if !self.finished {
            self.shared.monitoring.close_pending();
            self.shared.control.abort();
            if let Ok(mut state)=self.shared.state.lock(){
                if let Some(route)=state.route.take(){route.cancel()}
                state.busy=false;state.orphaned=true;
                let admitted=state.audit.take().is_some_and(|audit|audit.opened_observed());
                state.view["actorAdmission"]=json!(admitted);
                state.view["phase"]=json!("unknown");state.view["cleanupState"]=json!("unknown");
            }
            self.shared.done.send_replace(true);
        }
    }
}
impl AgentTunnelRouteOperation {
    /// Finalizers release UI custody. Only successful explicit opted detach can
    /// retain the original live task; ordinary handles still cancel on release.
    pub fn release_ui(&self) {
        if self.custody.release() {self.shared.cancel()}
    }
    /// A successful receipt retains the original operation until its existing
    /// limits, never reports a join and never creates a replacement permission.
    pub async fn detach_auto_open_json(&self)->AppResult<String> {
        automatic::detach(&self.shared,&self.custody).await
    }
    pub async fn details_json(&self)->AppResult<String> {
        self.custody.check_read()?;
        self.shared.context.check().await?;
        self.custody.check_read()?;self.shared.publish()
    }
    /// Separate explicit owner action. No caller approval flag or signed input.
    pub async fn approve_once(&self)->AppResult<String> {
        let _custody=self.custody.begin_action()?;
        self.shared.context.check().await?;
        let prepared={
            let mut state=self.shared.state.lock().map_err(|_|unknown())?;
            if state.consumed||state.busy{return Err(unavailable())}
            let ready=state.ready.take().ok_or_else(unavailable)?;
            ready.check(&self.shared.context)?;
            state.consumed=true;state.busy=true;state.view["phase"]=json!("starting");
            self.shared.done.send_replace(false);ready
        };
        let shared=self.shared.clone();let(tx,rx)=oneshot::channel();
        let handle=self.shared.context.runtime.spawn(async move {
            let mut guard=WorkGuard{shared:shared.clone(),finished:false};
            run_original(shared.clone(),prepared,tx).await;
            guard.finished=true;
        });
        self.shared.state.lock().map_err(|_|unknown())?.task=Some(handle);
        rx.await.map_err(|_|unknown())?
    }
    /// Only actual original owner Stop may trigger authenticated TLS shutdown.
    pub async fn stop(&self)->AppResult<String> {
        let _custody=self.custody.begin_action()?;
        self.shared.context.check().await?;
        let (route,listening)={
            let state=self.shared.state.lock().map_err(|_|unknown())?;
            (state.route.clone().ok_or_else(unavailable)?,state.view["phase"]=="listening")
        };
        let app=self.shared.context.app()?;
        route.request_stop(&app).await?;
        if listening {route.cancel();} // No admitted TLS exists: abort accept, no close_notify.
        self.shared.publish()
    }
    pub fn cancel(&self){
        if self.custody.release() || automatic::withdraw(&self.shared,&self.custody) {
            self.shared.cancel()
        }
    }
    pub async fn joined_json(&self)->AppResult<String> {
        self.custody.check_read()?;
        let observed=joined_original(&self.shared).await?;
        self.custody.check_read()?;Ok(observed)
    }
    /// Test-only readonly custody of the SAME actual join target. It exposes
    /// no mutation, cancellation, approval or replacement-authority method.
    #[cfg(any(test, all(feature = "test-harness", target_os = "linux")))]
    pub(crate) fn original_join_observer(&self)->OriginalTunnelJoinObserver {
        OriginalTunnelJoinObserver{shared:self.shared.clone()}
    }
}

// Cancellation preserves custody of the SAME pending original JoinHandle.
struct PumpJoinCustody<'a> {state:&'a Mutex<State>,handle:Option<JoinHandle<()>>}
impl Drop for PumpJoinCustody<'_> {
    fn drop(&mut self) {
        if let Some(handle)=self.handle.take() {
            // Approval is one-use: no replacement task can enter this slot.
            let mut state=self.state.lock().unwrap_or_else(|poisoned|poisoned.into_inner());
            state.task=Some(handle);
        }
    }
}
async fn join_pump_then_control(state:&Mutex<State>,end:Instant,
    control:impl std::future::Future<Output=AppResult<()>>,
)->AppResult<bool> {
    let handle=state.lock().map_err(|_|unknown())?.task.take();
    let mut custody=PumpJoinCustody {state,handle};
    if let Some(handle)=custody.handle.as_mut() {
        match tokio::time::timeout_at(tokio::time::Instant::from_std(end),handle).await {
            Ok(Ok(()))=>{
                // No await between consuming the completed handle and recording
                // its actual join: cancellation during control cannot lose it.
                custody.handle.take();
                state.lock().unwrap_or_else(|poisoned|poisoned.into_inner()).joined=true;
            },
            Ok(Err(_))=>{custody.handle.take();},
            Err(_)=>{}, // Drop restores the SAME pending handle before control.
        }
    }
    drop(custody);
    Ok(matches!(tokio::time::timeout_at(tokio::time::Instant::from_std(end),control).await,Ok(Ok(()))))
}
async fn joined_original(shared:&Shared)->AppResult<String> {
        let end=Instant::now()+Duration::from_secs(5);
        let control_closed=join_pump_then_control(&shared.state,end,shared.control.wait_closed(end)).await?;
        let state=shared.state.lock().map_err(|_|unknown())?;
        serde_json::to_string(&json!({"version":1,"phase":state.view["phase"],
            "remoteDrainObserved":false,"countersExact":false,"nativeAccountingOnly":true,
            "sentBytes":state.view["sentBytes"],"receivedBytes":state.view["receivedBytes"],"actorAdmission":state.view["actorAdmission"],
            "cleanupState":if control_closed&&(state.joined||(!state.consumed&&!state.busy))&&!state.orphaned&&!state.busy&&(state.view["cleanupState"]=="closedObserved"||!state.consumed){"closedObserved"}else{"unknown"}})).map_err(|_|unknown())
}

#[cfg(any(test, all(feature = "test-harness", target_os = "linux")))]
pub(crate) struct OriginalTunnelJoinObserver {shared:Arc<Shared>}
#[cfg(any(test, all(feature = "test-harness", target_os = "linux")))]
impl OriginalTunnelJoinObserver {
    pub(crate) async fn joined_json(&self)->AppResult<String> {joined_original(&self.shared).await}
    /// Read-only actual local await witness; never proves remote retirement.
    pub(crate) fn local_pump_joined(&self)->bool {
        self.shared.state.lock().is_ok_and(|state|state.joined)
    }
}


async fn run_original(shared:Arc<Shared>,prepared:producer::Prepared,ready:oneshot::Sender<AppResult<String>>) {
    resume::run_original(shared,prepared,ready).await
}

impl AppCore {
    /// Attach to an already signed, explicitly retained live operation only.
    /// None permits an explicit new review; errors never authorize a fallback.
    pub async fn agent_tunnel_route_attach_saved_json(self,raw:&str)->AppResult<Option<AgentTunnelRouteOperation>> {
        let request:Request=parse(raw).map_err(|_|unavailable())?;request.validate()?;
        let id=Uuid::parse_str(&request.association_id).map_err(|_|unavailable())?;
        if id.is_nil()||id.to_string()!=request.association_id{return Err(unavailable())}
        let context=Context::capture(&self,Frame{version:request.version,profile_id:request.profile_id.clone(),
            notification_scope_id:request.notification_scope_id.clone()}).await?;
        drop(self);
        let original=context.scope.clone();
        bounded_original(&original,automatic::attach(context,&request)).await
    }
    pub async fn agent_tunnel_access_audit_json(self,raw:&str)->AppResult<String> {
        let frame:Frame=parse(raw).map_err(|_|unavailable())?;
        let context=Context::capture(&self,frame).await?;
        drop(self); // Bridge sole local is consumed; no Core survives SQL Pending.
        let rows=bounded_original(&context.scope,RouteAudit::list(&context.scope)).await?;
        context.publish(rows,196608)
    }
    pub async fn agent_tunnel_route_prepare_json(self,raw:&str)->AppResult<AgentTunnelRouteOperation> {
        let request:Request=parse(raw).map_err(|_|unavailable())?;request.validate()?;
        let id=Uuid::parse_str(&request.association_id).map_err(|_|unavailable())?;
        if id.is_nil()||id.to_string()!=request.association_id{return Err(unavailable())}
        // FIRST original capture; no profile/auth/probe/TLS/file I/O before it.
        let context=Context::capture(&self,Frame{version:request.version,profile_id:request.profile_id.clone(),
            notification_scope_id:request.notification_scope_id.clone()}).await?;
        drop(self); // Consumed Bridge sole local: no caller Core clone across Pending.
        let original=context.scope.clone();
        bounded_original(&original,async move {
            let control=transport::Observation::new();
            let prepared=producer::Prepared::capture(&context,id,&request,control.clone()).await?;
            let automatic=automatic::Registration::new(&request,prepared.association_record.clone());
            let view=prepared.view(&context).await?;let(done,_)=watch::channel(true);
            let shared=Arc::new(Shared{context,state:Mutex::new(State{ready:Some(prepared),route:None,task:None,audit:None,
                view,consumed:false,busy:false,orphaned:false,joined:false}),done,control:resume::Controls::new(control),automatic,
                monitoring:monitoring::AcceptedSource::default()});
            let weak=Arc::downgrade(&shared);let scope=shared.context.scope.clone();
            shared.context.runtime.spawn(async move{scope.cancelled().await;if let Some(shared)=weak.upgrade(){shared.cancel();}});
            Ok(AgentTunnelRouteOperation{shared,custody:automatic::Custody::new()})
        }).await
    }
}

#[cfg(any(test,all(feature="test-harness",target_os="linux")))]
fn join_test_state(handle:JoinHandle<()>)->Mutex<State> {
    Mutex::new(State {ready:None,route:None,task:Some(handle),audit:None,view:json!({}),
        consumed:true,busy:false,orphaned:false,joined:false})
}
#[cfg(any(test,all(feature="test-harness",target_os="linux")))]
impl AppCore {
    pub(crate) async fn __owned_local_join_without_control_proof() {
        let completed=Arc::new(std::sync::atomic::AtomicBool::new(false));
        let child_completed=completed.clone();
        let state=join_test_state(tokio::spawn(async move {
            tokio::task::yield_now().await;
            child_completed.store(true,std::sync::atomic::Ordering::SeqCst);
        }));
        let closed=join_pump_then_control(&state,Instant::now()+Duration::from_secs(1),async {
            assert!(completed.load(std::sync::atomic::Ordering::SeqCst));
            assert!(state.lock().unwrap().joined);
            Err(unknown())
        }).await.unwrap();
        let state=state.lock().unwrap();
        assert!(state.task.is_none());assert!(state.joined);assert!(!closed);
    }
    pub(crate) async fn __owned_cancel_pending_join_preserves_handle() {
        use std::future::Future;
        let (release,wait)=oneshot::channel::<()>();
        let handle=tokio::spawn(async move {wait.await.unwrap();});
        let original_id=handle.id();let state=join_test_state(handle);
        let end=Instant::now()+Duration::from_secs(1);
        let mut joined=Box::pin(join_pump_then_control(&state,end,async {Err(unknown())}));
        std::future::poll_fn(|cx| {
            assert!(joined.as_mut().poll(cx).is_pending());std::task::Poll::Ready(())
        }).await;
        drop(joined); // Same cancellation that previously detached the pump.
        {
            let state=state.lock().unwrap();
            assert_eq!(state.task.as_ref().unwrap().id(),original_id);assert!(!state.joined);
        }
        release.send(()).unwrap();
        assert!(!join_pump_then_control(&state,end,async {Err(unknown())}).await.unwrap());
        let state=state.lock().unwrap();assert!(state.task.is_none());assert!(state.joined);
    }
    pub(crate) async fn __owned_cancel_control_preserves_join_witness() {
        use std::future::Future;
        let state=join_test_state(tokio::spawn(async {}));
        let entered=std::sync::atomic::AtomicBool::new(false);
        let mut joined=Box::pin(join_pump_then_control(&state,Instant::now()+Duration::from_secs(1),async {
            assert!(state.lock().unwrap().joined);
            entered.store(true,std::sync::atomic::Ordering::SeqCst);
            std::future::pending::<AppResult<()>>().await
        }));
        std::future::poll_fn(|cx| {
            assert!(joined.as_mut().poll(cx).is_pending());
            if entered.load(std::sync::atomic::Ordering::SeqCst) {std::task::Poll::Ready(())}
            else {std::task::Poll::Pending}
        }).await;
        drop(joined);
        let state=state.lock().unwrap();assert!(state.joined);assert!(state.task.is_none());
    }
}
