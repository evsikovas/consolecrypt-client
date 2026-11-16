//! Explicit original-owner carrier bootstrap; never a tunnel/Actor permission.
use super::*;
use cc_storage_core::{NodeAssociationOwner,NodeAssociationRecord,NodeAssociationPhase,
    NodeOutboundControlReference,Storage};
use cc_sync_core::api::{ApiClient,OriginalRelayApi,RelayControlReference,RelaySlot,RelaySlotLimits};
use std::sync::Mutex;
use tokio::{sync::{oneshot,watch},task::JoinHandle};

fn default_ttl()->u64 {3600}
#[derive(Deserialize)]
#[serde(rename_all="camelCase",deny_unknown_fields)]
struct Request {version:u8,profile_id:String,notification_scope_id:String,association_id:String,
    #[serde(default="default_ttl")] ttl_seconds:u64}
#[derive(Deserialize)]
#[serde(rename_all="camelCase",deny_unknown_fields)]
struct Descriptor {
    node:protocol::NodeIdentity,owner_pin:OwnerPin,actor_revision:u64,certificate:Certificate,
    certificate_digest:Hash,revoked:bool,transport_format:String,server_certificate_hex:String,
    server_root_hex:String,server_name:String,relay_origin:String,
}
#[derive(Clone,PartialEq,Eq)]
pub(super) enum Selection {Direct,Outbound(NodeOutboundControlReference)}
fn api(context:&Context)->AppResult<ApiClient> {
    context.scope.with_current_resources(|session,_,_|session.api())
}
pub(super) fn upgrade_api(context:&Context,original:&OriginalRelayApi)->AppResult<ApiClient> {
    context.scope.with_current_resources(|session,_,_| {
        let actual=session.api()?;
        if !original.matches_original(&actual){return Err(unavailable())}
        original.upgrade_original().ok_or_else(unavailable)
    })
}
async fn api_original<T>(context:&Context,future:impl std::future::Future<Output=AppResult<T>>)->AppResult<T> {
    let end=context.scope.deadline();tokio::pin!(future);
    let guarded=std::future::poll_fn(|cx|context.scope.with_api_setup_current(|_| {
        Ok(std::future::Future::poll(future.as_mut(),cx))
    }).unwrap_or_else(|e|std::task::Poll::Ready(Err(e))));
    transport::bounded_weak(&context.core,&context.scope,end,guarded).await
}
fn reference(value:&NodeOutboundControlReference)->RelayControlReference {
    RelayControlReference{association_id:value.association_id,control_id:value.control_id,
        user_id:value.account_id,device_id:value.device_id,session_id:value.session_id,
        server_url:value.server_url.clone(),expires_at_unix_seconds:value.expires_at}
}
fn origin(api:&ApiClient)->AppResult<String> {
    let mut url=api.config().server_url.clone();
    if url.scheme()!="https"||!url.username().is_empty()||url.password().is_some()
        ||url.query().is_some()||url.fragment().is_some() {return Err(unavailable())}
    url.set_scheme("wss").map_err(|_|unavailable())?;Ok(url.origin().ascii_serialization())
}
async fn original(context:&Context,id:Uuid)->AppResult<(Storage,NodeAssociationOwner,NodeAssociationRecord,
    Option<NodeOutboundControlReference>)> {
    context.scope.check_current()?;
    let ledger=ledger::Ledger::capture_original(context.scope.clone()).await?;
    let snapshot=ledger.snapshot_original().await?;
    let row=snapshot.records().iter().find(|r|r.public.association_id==id&&r.phase==NodeAssociationPhase::Associated
        &&r.revocation.is_none()).cloned().ok_or_else(unavailable)?;
    let key=context.scope.with_current_resources(|_,_,id|Ok(PublicKey::from_bytes(id.public_keys().signing)))?;
    let public=data::association(&row,key)?;if public["purpose"]!="tunnel" {return Err(unavailable())}
    let cert=ledger::current_certificate(&row,key)?;let pin=ledger::current_pin(&row,key)?;
    context.scope.bind_owner_pin(&pin)?;
    check_live(cert.body.not_before,cert.body.expires_at,context.scope.now()?).map_err(|_|unavailable())?;
    context.validate_host(row.public.saved_host_id,row.public.saved_host_revision).await?;
    let owner=ledger.audit_owner()?;
    let storage=context.scope.with_current_resources(|s,_,_|s.storage())?;
    let original=context.scope.clone();let expected=row.clone();let captured=owner.clone();
    let found=storage.read(move|tx|original.with_io_current(|_| {
        let snapshot=tx.node_association_snapshot(&captured)?;
        if snapshot.records().iter().find(|r|r.public.association_id==id)!=Some(&expected) {return Err(unavailable())}
        tx.node_outbound_control_reference(&captured,id).map_err(AppError::from)
    })).await?;
    if let Some(value)=&found {
        if owner.personal.is_none(){return Err(unavailable())}
        if value.expires_at>cert.body.expires_at.seconds()||value.expires_at>pin.expires_at.seconds()
            || value.descriptor_origin!=origin(&api(context)?)? {return Err(unavailable())}
    }
    context.scope.check_current()?;Ok((storage,owner,row,found))
}
fn choose(owner:&NodeAssociationOwner,found:Option<NodeOutboundControlReference>)->AppResult<Selection> {
    match found {None=>Ok(Selection::Direct),Some(value) if owner.personal.is_some()=>Ok(Selection::Outbound(value)),
        Some(_)=>Err(unavailable())}
}
pub(super) async fn select(context:&Context,id:Uuid)->AppResult<Selection> {
    let(_,owner,_,found)=original(context,id).await?;choose(&owner,found)
}
// Retain the actual newly created slot across every fallible dispatch/connect
// await. Cancellation transfers it to the existing owned observation; no
// detached cleanup task, synthetic retirement or silently dropped capability.
struct SlotCustody {observation:transport::Observation,slot:Option<RelaySlot>}
impl Drop for SlotCustody {fn drop(&mut self){if let Some(slot)=self.slot.take(){
    if self.observation.own_control_slot(slot).is_err(){self.observation.control_custody_unknown();}
}}}
pub(super) async fn open_selected(context:&Context,id:Uuid,selected:&Selection,observation:transport::Observation)
    ->AppResult<transport::Peer> {
    let(_,_,row,actual)=original(context,id).await?;
    match (selected,actual) {
        (Selection::Direct,None)=>transport::Peer::open_observed_weak(&context.core,context.scope.clone(),
            row.public.saved_host_id,row.public.saved_host_revision,observation).await,
        (Selection::Outbound(saved),Some(actual)) if *saved==actual=>{
            let api=api(context)?;let original_reference=reference(saved);
            let left=saved.expires_at.checked_sub(context.scope.now()?.0.seconds()).ok_or_else(unavailable)?;
            let ttl=left.min(300).min(context.scope.deadline().saturating_duration_since(Instant::now()).as_secs());
            if ttl<=10 {return Err(unavailable())}
            let subject=api_original(context,async{api.capture_reader_auth().await.map_err(|_|unavailable())}).await?;
            observation.begin_control_create()?; // Lost create reply has no captured slot ID: remains Unknown.
            let slot=api_original(context,async{api.create_relay_slot_once(subject,RelaySlotLimits{
                ttl_seconds:ttl,idle_seconds:ttl.min(30),max_bytes:8*1024*1024}).await.map_err(|_|unknown())}).await?;
            let mut custody=SlotCustody{observation:observation.clone(),slot:Some(slot)};
            let issued=api_original(context,async {
                let subject=api.capture_reader_auth().await.map_err(|_|unavailable())?;
                let slot=custody.slot.as_mut().ok_or_else(unknown)?;
                let dispatched=api.dispatch_relay_control_once(subject,&original_reference,slot).await.map_err(|_|unknown())?;
                dispatched.connect_client_once(&api,slot).await.map_err(|_|unknown())
            }).await;
            drop(api);
            drop(custody); // Actual slot owner survives success, error AND cancellation.
            let issued=match issued {Ok(value)=>value,Err(error)=>{
                observation.abort();let _=observation.wait_closed(Instant::now()+Duration::from_secs(5)).await;
                return Err(error)
            }};
            let opened=transport::Peer::open_control_observed_weak(&context.core,context.scope.clone(),id,issued,observation.clone()).await;
            if opened.is_err() {
                observation.abort();let _=observation.wait_closed(Instant::now()+Duration::from_secs(5)).await;
            }
            opened
        },
        _=>Err(unavailable()), // no fallback, new registration or transport adoption
    }
}

struct Ready {
    peer:transport::Peer,storage:Storage,owner:NodeAssociationOwner,row:NodeAssociationRecord,
    pin:OwnerPin,certificate:Signed<Certificate>,descriptor:Descriptor,api:OriginalRelayApi,ttl:u64,
    _permit:Arc<tokio::sync::OwnedSemaphorePermit>,
}
impl Ready {
    async fn prepare(context:&Context,id:Uuid,ttl:u64,carrier:transport::Observation)->AppResult<Self> {
        let(storage,owner,row,found)=original(context,id).await?;
        if owner.personal.is_none()||found.is_some(){return Err(unavailable())} // explicit Personal create-only bootstrap, never renew/adopt
        let key=context.scope.with_current_resources(|_,_,id|Ok(PublicKey::from_bytes(id.public_keys().signing)))?;
        let certificate=ledger::current_certificate(&row,key)?;let trusted=ledger::current_pin(&row,key)?;
        let permit=carrier.retain_permit(review_permit()?)?;
        let mut peer=transport::Peer::open_observed_weak(&context.core,context.scope.clone(),row.public.saved_host_id,
            row.public.saved_host_revision,carrier).await?;
        if peer.route_digest!=row.public.route_trust_digest {return Err(unavailable())}
        let descriptor:Descriptor=peer.exchange_weak(&context.core,"probeTunnelTransport",json!({}),None).await?;
        let minimum=row.rotation.as_ref().filter(|r|r.selected_observed).and_then(|r|r.observed_actor_revision)
            .unwrap_or(row.public.certificate.as_ref().ok_or_else(unavailable)?.expected_actor_revision);
        let mut pin=trusted.clone();pin.checkpoint=descriptor.owner_pin.checkpoint;
        let api=api(context)?;
        if descriptor.revoked||descriptor.actor_revision<minimum||descriptor.owner_pin!=pin||pin.checkpoint<trusted.checkpoint
            ||descriptor.node.agent_id!=certificate.body.agent_id||descriptor.node.key_epoch!=certificate.body.key_epoch
            ||descriptor.node.public_key!=certificate.body.agent_key||descriptor.node.key_fingerprint!=certificate.body.agent_key.fingerprint()
            ||!matches!((descriptor.node.format,descriptor.node.key_epoch),(1,1)|(3,2))
            ||descriptor.certificate!=certificate.body||descriptor.certificate_digest!=certificate.body.digest().map_err(|_|unavailable())?
            ||descriptor.transport_format!="cc-agent-tunnel-transport/v1"||descriptor.relay_origin!=origin(&api)?
            ||descriptor.server_name.is_empty()||descriptor.server_name.len()>253
            ||descriptor.server_certificate_hex.is_empty()||descriptor.server_certificate_hex.len()>4096
            ||descriptor.server_root_hex.is_empty()||descriptor.server_root_hex.len()>4096 {return Err(unavailable())}
        context.scope.bind_owner_pin(&pin)?;
        let value=Self{peer,storage,owner,row,pin,certificate,descriptor,api:api.original_relay_api(),ttl,_permit:permit};
        drop(api); // No signer/key custody crosses the following SQL wait or idle review.
        value.revalidate(context).await?;if value.lifetime(context)?<30 {return Err(unavailable())} Ok(value)
    }
    fn lifetime(&self,context:&Context)->AppResult<u64> {
        context.scope.check_current()?;context.scope.bind_owner_pin(&self.pin)?;
        Ok(self.ttl.min(86400).min(context.scope.remaining_until(self.pin.expires_at)?.as_secs())
            .min(context.scope.remaining_until(self.certificate.body.expires_at)?.as_secs()))
    }
    async fn revalidate(&self,context:&Context)->AppResult<()> {
        let(_,owner,row,found)=original(context,self.row.public.association_id).await?;
        if owner!=self.owner||row!=self.row||found.is_some() {return Err(unavailable())}
        self.lifetime(context)?;Ok(())
    }
    async fn approve(mut self,shared:&Shared)->(AppResult<RelayControlReference>,bool) {
        let mut attempt=None;
        let action=bounded_original(&shared.context.scope,async {
            self.revalidate(&shared.context).await?;
            let subject={let api=upgrade_api(&shared.context,&self.api)?;
                api_original(&shared.context,async{api.capture_reader_auth().await.map_err(|_|unavailable())}).await?};
            let control_id=Uuid::new_v4(); // Fresh exactly once BEFORE dispatch, retained for cleanup.
            let prepared=upgrade_api(&shared.context,&self.api)?.prepare_relay_control_registration(subject,self.row.public.association_id,
                control_id,self.lifetime(&shared.context)?).map_err(|_|unavailable())?;
            if let Ok(mut state)=shared.state.lock(){state.view["controlId"]=json!(control_id);}
            attempt=Some(prepared);
            let mut registered={let api=upgrade_api(&shared.context,&self.api)?;
                api_original(&shared.context,async {
                    api.create_relay_control_once(attempt.as_mut().ok_or_else(unknown)?).await.map_err(|_|unknown())
                }).await?};
            self.revalidate(&shared.context).await?;
            let reference=registered.reference().clone();
            let credential=registered.take_bootstrap_once().map_err(|_|unknown())?;
            self.peer.bootstrap_tunnel_control_weak(&shared.context.core,&reference,&credential,self.descriptor.actor_revision,
                &self.pin,self.descriptor.certificate_digest,&self.descriptor.node).await?;
            drop(credential);
            self.revalidate(&shared.context).await?;
            let saved=NodeOutboundControlReference{association_id:reference.association_id,control_id:reference.control_id,
                account_id:reference.user_id,device_id:reference.device_id,session_id:reference.session_id,
                server_url:reference.server_url.clone(),descriptor_origin:self.descriptor.relay_origin.clone(),
                expires_at:reference.expires_at_unix_seconds};
            let scope=shared.context.scope.clone();let expected=self.row.clone();let owner=self.owner.clone();
            self.storage.write_node_association(move|tx|scope.with_io_current(|_| {
                tx.node_outbound_control_store(&owner,&expected,saved).map_err(AppError::from)
            })).await.map_err(|_|unknown())?;
            shared.context.scope.check_current()?;Ok(reference)
        }).await;
        let mut retired=true;
        if action.is_err() {
            if let Some(attempt)=attempt.as_mut() {
                retired=match self.api.upgrade_original() {
                    Some(api)=>tokio::time::timeout(Duration::from_secs(5),api.retire_relay_control_attempt_once(attempt))
                        .await.is_ok_and(|r|r.is_ok()),None=>false,
                };
            }
        }
        drop(self.peer);
        let closed=shared.carrier.wait_closed(Instant::now()+Duration::from_secs(5)).await.is_ok();
        (action,closed&&retired)
    }
}
struct State {ready:Option<Ready>,view:Value,busy:bool,spent:bool,orphaned:bool,task:Option<JoinHandle<()>>,task_started:bool,joined:bool}
struct Shared {context:Context,state:Mutex<State>,done:watch::Sender<bool>,carrier:transport::Observation}
pub struct NodeTunnelControlReview {shared:Arc<Shared>}
impl std::fmt::Debug for NodeTunnelControlReview {
    fn fmt(&self,f:&mut std::fmt::Formatter<'_>)->std::fmt::Result{f.write_str("NodeTunnelControlReview([redacted])")}
}
impl Drop for NodeTunnelControlReview {fn drop(&mut self){self.cancel()}}
impl NodeTunnelControlReview {
    pub fn cancel(&self) {
        self.shared.context.cancel.cancel();self.shared.carrier.abort();
        if let Ok(mut state)=self.shared.state.lock(){if !state.busy{state.ready.take();state.spent=true;}}
    }
    pub async fn details_json(&self)->AppResult<String> {
        self.shared.context.check().await?;
        let view=self.shared.state.lock().map_err(|_|unknown())?.view.clone();self.shared.context.publish(view,16384)
    }
    pub async fn approve_once(&self)->AppResult<String> {
        self.shared.context.check().await?;
        let ready={let mut state=self.shared.state.lock().map_err(|_|unknown())?;
            if state.spent||state.busy{return Err(unavailable())}
            let ready=state.ready.take().ok_or_else(unavailable)?;state.spent=true;state.busy=true;state.task_started=true;
            state.view["phase"]=json!("bootstrapping");self.shared.done.send_replace(false);ready};
        let shared=self.shared.clone();let(tx,rx)=oneshot::channel();
        let task=self.shared.context.runtime.spawn(async move {
            let mut guard=BootstrapWorkGuard{shared:shared.clone(),done:false};
            let(result,closed)=ready.approve(&shared).await;
            let live=shared.context.scope.check_current().is_ok()&&!shared.context.cancel.is_cancelled();
            let success=result.is_ok()&&closed&&live;
            let value={let mut state=match shared.state.lock(){Ok(v)=>v,Err(_)=>return};state.busy=false;
                state.view["phase"]=json!(if success{"configured"}else{"unknown"});
                state.view["cleanupState"]=json!(if closed{"closedObserved"}else{"unknown"});
                if let Ok(reference)=result {state.view["controlId"]=json!(reference.control_id);
                    state.view["registrationExpiresAt"]=json!(reference.expires_at_unix_seconds);}
                state.view.clone()};
            shared.done.send_replace(true);guard.done=true;
            let _=tx.send(if success{shared.context.publish(value,16384)}else{Err(unknown())});
        });
        self.shared.state.lock().map_err(|_|unknown())?.task=Some(task);rx.await.map_err(|_|unknown())?
    }
    pub async fn joined_json(&self)->AppResult<String> {
        let end=Instant::now()+Duration::from_secs(5);let mut done=self.shared.done.subscribe();
        let observed=tokio::time::timeout_at(tokio::time::Instant::from_std(end),async {
            while !*done.borrow(){done.changed().await.map_err(|_|unknown())?;}
            self.shared.carrier.wait_closed(end).await
        }).await;
        if matches!(observed,Ok(Ok(()))) {
            let task=self.shared.state.lock().map_err(|_|unknown())?.task.take();
            if let Some(mut task)=task {
                match tokio::time::timeout_at(tokio::time::Instant::from_std(end),&mut task).await {
                    Ok(Ok(()))=>self.shared.state.lock().map_err(|_|unknown())?.joined=true,
                    Ok(Err(_))=>self.shared.state.lock().map_err(|_|unknown())?.orphaned=true,
                    Err(_)=>self.shared.state.lock().map_err(|_|unknown())?.task=Some(task),
                }
            }
        }
        let state=self.shared.state.lock().map_err(|_|unknown())?;
        serde_json::to_string(&json!({"version":1,"phase":state.view["phase"],"carrierOnly":true,"nodeAdmission":false,
            "cleanupState":if !state.busy&&!state.orphaned&&(!state.task_started||state.joined)
                &&self.shared.carrier.closed_observed(){"closedObserved"}else{"unknown"}})).map_err(|_|unknown())
    }
}
struct BootstrapWorkGuard {shared:Arc<Shared>,done:bool}
impl Drop for BootstrapWorkGuard {fn drop(&mut self){if !self.done{
    self.shared.carrier.abort();if let Ok(mut state)=self.shared.state.lock(){state.busy=false;state.orphaned=true;
        state.view["phase"]=json!("unknown");state.view["cleanupState"]=json!("unknown");}
    self.shared.done.send_replace(true);
}}}
impl AppCore {
    pub async fn agent_tunnel_control_bootstrap_prepare_json(self,raw:&str)->AppResult<NodeTunnelControlReview> {
        let request:Request=parse(raw)?;let id=Uuid::parse_str(&request.association_id).map_err(|_|unavailable())?;
        if id.is_nil()||id.to_string()!=request.association_id||!(30..=86400).contains(&request.ttl_seconds){return Err(unavailable())}
        let context=Context::capture_control(&self,Frame{version:request.version,profile_id:request.profile_id,
            notification_scope_id:request.notification_scope_id}).await?;drop(self);
        let scope=context.scope.clone();bounded_original(&scope,async move {
            let carrier=transport::Observation::new();
            let ready=Ready::prepare(&context,id,request.ttl_seconds,carrier.clone()).await?;
            let lifetime=ready.lifetime(&context)?;let(node,owner)=(data::node(&ready.descriptor.node),context.scope.owner_context()?);
            let owner=match owner{crate::agent_tunnel::scope::OriginalOwnerContext::PersonalAccount{user_id}=>user_id.to_string(),
                _=>return Err(unavailable())};
            let view=json!({"phase":"bootstrapReview","associationId":id,"node":node,"ownerAccountId":owner,
                "requestedRegistrationLifetimeSeconds":request.ttl_seconds,"maximumRegistrationLifetimeSeconds":lifetime,
                "serverSessionMayShorten":true,"reviewLifetimeSeconds":300,"controlAttachmentMaximumSeconds":300,
                "registrationMaximumSeconds":86400,"registrationGenerationCap":64,"registrationCiphertextAllocationBytes":512u64*1024*1024,
                "carrierOnly":true,"nodeAdmission":false,"autoRenew":false,"controlId":null,"registrationExpiresAt":null,
                "remainingMs":0,"cleanupState":"notStarted"});
            let(done,_)=watch::channel(true);Ok(NodeTunnelControlReview{shared:Arc::new(Shared{context,carrier,done,
                state:Mutex::new(State{ready:Some(ready),view,busy:false,spent:false,orphaned:false,task:None,task_started:false,joined:false})})})
        }).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn bootstrap_lifetime_is_independent_explicit_carrier_choice() {
        let request:Request=parse(r#"{"version":1,"profileId":"p","notificationScopeId":"s","associationId":"a"}"#).unwrap();
        assert_eq!(request.ttl_seconds,3600);
        let chosen:Request=parse(r#"{"version":1,"profileId":"p","notificationScopeId":"s","associationId":"a","ttlSeconds":86400}"#).unwrap();
        assert_eq!(chosen.ttl_seconds,86400);
        // The short original review is captured separately; this is no Permit.
    }
    #[test]
    fn bootstrap_request_denies_unknown_path_secret_and_duplicate_ttl() {
        let prefix=r#"{"version":1,"profileId":"p","notificationScopeId":"s","associationId":"a""#;
        for extra in [r#", "path":"/tmp/agent"}"#,r#", "agentToken":"ccrc_opaque"}"#,
            r#", "ttlSeconds":3600,"ttlSeconds":86400}"#,r#", "ttlSeconds":true}"#,
            r#", "ttlSeconds":300.0}"#] {
            assert!(parse::<Request>(&format!("{prefix}{extra}")).is_err());
        }
    }

    async fn local_context()->(tempfile::TempDir,AppCore,Context) {
        let(root,app,_)=crate::agent_tunnel::association::tests::local().await;
        let(owner,u)=app.unlocked().await.unwrap();
        let frame=Frame{version:1,profile_id:owner.profile_id.to_string(),notification_scope_id:u.notices.scope_id().unwrap()};
        drop(u);drop(owner);let context=Context::capture_control(&app,frame).await.unwrap();
        (root,app,context)
    }
    #[tokio::test(flavor="multi_thread",worker_threads=2)]
    async fn actual_local_owner_without_reference_keeps_direct_selection_without_api() {
        let(_root,app,context)=local_context().await;
        assert!(api(&context).is_err(),"real Local owner has no Personal API");
        let owner=ledger::Ledger::capture_original(context.scope.clone()).await.unwrap().audit_owner().unwrap();
        assert!(owner.personal.is_none());assert!(matches!(choose(&owner,None).unwrap(),Selection::Direct));
        app.shutdown().await.unwrap();
    }
    #[tokio::test(flavor="multi_thread",worker_threads=2)]
    async fn cancel_before_approval_requires_actual_carrier_drop_without_invented_task_join() {
        let(_root,app,context)=local_context().await;
        let listener=tokio::net::TcpListener::bind(("127.0.0.1",0)).await.unwrap();
        let socket=tokio::net::TcpStream::connect(listener.local_addr().unwrap()).await.unwrap();
        let(_peer,_)=listener.accept().await.unwrap();let carrier=transport::Observation::new();
        let(release,held)=oneshot::channel();
        let owned=transport::held_carrier_for_test(socket,context.scope.clone(),carrier.clone(),held).unwrap();
        let task=tokio::spawn(owned);let(done,_)=watch::channel(true);
        // Component-only unstarted review: no Ready, Permit, Agent reply or authority fabricated.
        let review=NodeTunnelControlReview{shared:Arc::new(Shared{context,carrier,done,state:Mutex::new(State{
            ready:None,view:json!({"phase":"bootstrapReview"}),busy:false,spent:false,orphaned:false,
            task:None,task_started:false,joined:false})})};
        review.cancel();assert!(!review.shared.carrier.closed_observed());
        release.send(()).unwrap();task.await.unwrap();
        let closed:Value=serde_json::from_str(&review.joined_json().await.unwrap()).unwrap();
        assert_eq!(closed["cleanupState"],"closedObserved");
        assert!(!review.shared.state.lock().unwrap().joined,"no started task means no invented join witness");
        app.shutdown().await.unwrap();
    }
    #[tokio::test(flavor="multi_thread",worker_threads=2)]
    async fn actual_personal_original_api_handle_does_not_retain_core_session_or_device_keys() {
        let server=cc_sync_core::mock::MockServer::start().await;let root=tempfile::tempdir().unwrap();
        let app=AppCore::new(crate::AppConfig::for_tests(root.path().to_string_lossy().into_owned())).unwrap();
        app.create_synced_profile("Outbound weak original".into(),server.url().to_string(),
            "outbound-weak@example.org".into(),"outbound account password 42".into(),crate::AccountMode::Register).await.unwrap();
        app.create_vault("outbound original vault password 42".into()).await.unwrap();app.sync_now().await.unwrap();
        let(owner,u)=app.unlocked().await.unwrap();u.stop_engine().await;
        let frame=Frame{version:1,profile_id:owner.profile_id.to_string(),notification_scope_id:u.notices.scope_id().unwrap()};
        let weak_owner=Arc::downgrade(&owner);let weak_unlock=Arc::downgrade(&u);
        let cell=owner.identity_cell();let weak_cell=Arc::downgrade(&cell);drop(cell);
        let keys=owner.identity();let weak_keys=Arc::downgrade(&keys);drop(keys);
        let api=owner.api().unwrap();let original=api.original_relay_api();drop(api);drop(u);drop(owner);
        let context=Context::capture_control(&app,frame).await.unwrap();
        assert!(upgrade_api(&context,&original).is_ok());
        let weak_core=Arc::downgrade(&app.inner);drop(app);
        assert!(weak_core.upgrade().is_none());assert!(weak_owner.upgrade().is_none());assert!(weak_unlock.upgrade().is_none());
        assert!(weak_cell.upgrade().is_none());assert!(weak_keys.upgrade().is_none());assert!(original.upgrade_original().is_none());
        assert!(context.scope.check_current().is_err());
    }
}
