//! Original-owner Node key rotation. Epoch1->2 TunnelOnly, no auto grants.
use super::*;
use cc_storage_core::{NodeAssociationPhase,NodeAssociationRecord,NodeAssociationRotation};
use std::sync::Mutex;
use tokio::sync::{oneshot,watch,OwnedSemaphorePermit};
fn unavailable()->AppError {AppError::invalid("agent_node_rotation","rotation_unavailable")}
fn unknown()->AppError {AppError::invalid("agent_node_rotation","rotation_unknown")}
#[derive(Deserialize)]
#[serde(rename_all="camelCase",deny_unknown_fields)]
struct Request {version:u8,profile_id:String,notification_scope_id:String,association_id:String}
fn required<'de,D,T>(d:D)->std::result::Result<Option<T>,D::Error>
where D:serde::Deserializer<'de>,T:Deserialize<'de> {Option::<T>::deserialize(d)}
#[derive(Deserialize)]
#[serde(rename_all="camelCase",deny_unknown_fields)]
struct Probe {
    owner_pin:OwnerPin,actor_revision:u64,certificate:Certificate,revoked:bool,old_node:protocol::NodeIdentity,
    #[serde(deserialize_with="required")] node:Option<protocol::NodeIdentity>,
    #[serde(deserialize_with="required")] candidate:Option<protocol::RotationCandidate>,selection_state:String,
}
#[derive(Deserialize)]
#[serde(rename_all="camelCase",deny_unknown_fields)]
struct CandidateReply {candidate:protocol::RotationCandidate,owner_pin:OwnerPin,actor_revision:u64,
    certificate_digest:Hash,selected_key_epoch:u64}
#[derive(Deserialize)]
#[serde(rename_all="camelCase",deny_unknown_fields)]
struct Selected {node:protocol::NodeIdentity,owner_pin:OwnerPin,actor_revision:u64,certificate:Certificate,
    certificate_digest:Hash,revoked:bool,rotation_id:String,selection_state:String,required_reader_schema:u32}
#[derive(Clone,Copy,PartialEq,Eq)] enum Mode {Normal,Reconcile}
struct Ready {
    peer:transport::Peer,ledger:ledger::Ledger,row:NodeAssociationRecord,previous:Signed<Certificate>,
    probe:Probe,until:Instant,mode:Mode,candidate:Option<protocol::RotationCandidate>,
    intent:Option<NodeAssociationRotation>,proofs:Option<protocol::RotationProofs>,
    _permit:Arc<OwnedSemaphorePermit>,
}
impl Ready {
    fn live(&self,context:&Context)->AppResult<()> {
        context.scope.check_current()?;consent_live(self.until)?;
        context.scope.bind_owner_pin(&self.probe.owner_pin)?;
        let cert=if self.mode==Mode::Normal {&self.previous.body}else{&self.probe.certificate};
        check_live(cert.not_before,cert.expires_at,context.scope.now()?).map_err(|_|unavailable())?;
        Ok(())
    }
    fn candidate(&self)->AppResult<&protocol::RotationCandidate>{self.candidate.as_ref().ok_or_else(unavailable)}
    fn verify_selected(&self,value:&Selected,certificate:&Certificate,pin:&OwnerPin,revision:u64)->AppResult<()> {
        let candidate=self.candidate()?;
        if value.node.format!=3 || value.node.agent_id!=certificate.agent_id || value.node.key_epoch!=2
            || value.node.public_key!=candidate.new_public_key || value.node.key_fingerprint!=candidate.new_key_fingerprint
            || value.owner_pin!=*pin || value.actor_revision!=revision || value.revoked
            || value.certificate!=*certificate || value.certificate_digest!=certificate.digest().map_err(|_|unknown())?
            || value.rotation_id!=candidate.rotation_id || value.selection_state!="selected2" || value.required_reader_schema!=3 {
            return Err(unknown())
        }
        Ok(())
    }
    async fn save_intent(&mut self,app:&AppCore,next:NodeAssociationRotation)->AppResult<()> {
        let old=self.intent.clone().ok_or_else(unknown)?;
        self.ledger.update_rotation(app,self.row.public.association_id,old,next.clone()).await?;
        self.intent=Some(next);Ok(())
    }
    async fn prepare(mut self,shared:&Shared)->AppResult<Self> {
        if self.mode!=Mode::Normal {return Err(unavailable())}self.live(&shared.context)?;
        let app=shared.context.app()?;
        let intent=NodeAssociationRotation {candidate_json:None,signed_certificate_json:None,old_proof_json:None,new_proof_json:None,
            committed_owner_pin_json:None,expected_actor_revision:self.probe.actor_revision.checked_add(1).ok_or_else(unknown)?,
            observed_owner_pin_json:None,observed_actor_revision:None,selected_observed:false};
        shared.pending_block();self.ledger.begin_rotation(&app,self.row.clone(),intent.clone()).await?;
        self.intent=Some(intent);shared.blocked();self.live(&shared.context)?;
        let value:CandidateReply=self.peer.exchange(&app,"prepareRotationCandidate",json!({
            "expectedRev":self.probe.actor_revision,"expectedPin":self.probe.owner_pin,
            "certificateDigest":self.previous.body.digest().map_err(|_|unknown())?}),None).await?;
        protocol::rotation_candidate(&self.previous.body,&value.candidate)?;
        if value.owner_pin!=self.probe.owner_pin || value.actor_revision!=self.probe.actor_revision
            || value.certificate_digest!=self.previous.body.digest().map_err(|_|unknown())? || value.selected_key_epoch!=1 {
            return Err(unknown())
        }
        self.live(&shared.context)?;
        let mut next=self.intent.clone().ok_or_else(unknown)?;next.candidate_json=Some(bytes(&value.candidate)?);
        self.save_intent(&app,next).await?;self.candidate=Some(value.candidate);Ok(self)
    }
    async fn confirm(mut self,shared:&Shared)->AppResult<Selected> {
        self.live(&shared.context)?;let app=shared.context.app()?;
        let candidate=self.candidate()?.clone();
        let (certificate,final_pin,revision)=if self.mode==Mode::Normal {
            let now=shared.context.scope.now()?;let old=&self.previous.body;
            let mut pin=self.probe.owner_pin.clone();pin.checkpoint=pin.checkpoint.checked_add(1).ok_or_else(unknown)?;
            let cert=Certificate {header:Header {version:1,context:pin.context.clone(),owner_epoch:pin.epoch,checkpoint:pin.checkpoint},
                agent_id:old.agent_id,key_epoch:2,agent_key:candidate.new_public_key,scopes:old.scopes.clone(),capabilities:old.capabilities.clone(),
                origin:CertificateOrigin::Rotation {previous_certificate:old.digest().map_err(|_|unknown())?,previous_key_epoch:1},
                not_before:now.0,expires_at:old.expires_at.min(pin.expires_at)};
            let prepared=PreparedTunnelOwner::rotation_certificate(&self.probe.owner_pin,self.previous.clone(),cert,now).map_err(|_|unavailable())?;
            let signed=sign_reviewed(&app,&shared.context.scope,self.until,&self.probe.owner_pin,prepared).await?;
            let TunnelOwnerSigned::Certificate(cert)=signed else{return Err(unknown())};
            let mut next=self.intent.clone().ok_or_else(unknown)?;next.signed_certificate_json=Some(bytes(&cert)?);
            next.committed_owner_pin_json=Some(bytes(&pin)?);self.save_intent(&app,next).await?;
            self.live(&shared.context)?;
            let proofs:protocol::RotationProofs=self.peer.exchange(&app,"proveRotation",json!({
                "expectedRev":self.probe.actor_revision,"expectedPin":self.probe.owner_pin,"rotationId":candidate.rotation_id,
                "previousCertificate":self.previous,"certificate":cert}),None).await?;
            if proofs.certificate.body!=cert.body || proofs.certificate.signature!=cert.signature {return Err(unknown())}
            protocol::historical_rotation(&self.previous,&candidate,&proofs,self.probe.owner_pin.key)?;
            check_recent(proofs.old.body.issued_at,shared.context.scope.now()?).map_err(|_|unknown())?;
            self.live(&shared.context)?;
            let mut next=self.intent.clone().ok_or_else(unknown)?;next.old_proof_json=Some(bytes(&proofs.old)?);
            next.new_proof_json=Some(bytes(&proofs.new)?);self.save_intent(&app,next).await?;
            self.proofs=Some(proofs);
            let revision=self.probe.actor_revision.checked_add(1).ok_or_else(unknown)?;
            let applied:Selected=self.peer.exchange(&app,"applyRotation",json!({"expectedRev":self.probe.actor_revision,
                "expectedPin":self.probe.owner_pin,"rotationId":candidate.rotation_id,"certificateDigest":cert.body.digest().map_err(|_|unknown())?}),None).await?;
            self.verify_selected(&applied,&cert.body,&pin,revision)?;
            (cert,pin,revision)
        }else{
            // Exact already-accepted proposal/proofs only. No signing, new
            // entropy, Actor.rotate, recent() refresh or certificate mutation.
            let proofs=self.proofs.as_ref().ok_or_else(unavailable)?.clone();
            protocol::historical_rotation(&self.previous,&candidate,&proofs,self.probe.owner_pin.key)?;
            let reconciled:Selected=self.peer.exchange(&app,"reconcileRotation",json!({"expectedRev":self.probe.actor_revision,
                "expectedPin":self.probe.owner_pin,"rotationId":candidate.rotation_id,
                "previousCertificate":self.previous,"proofs":proofs}),None).await?;
            self.verify_selected(&reconciled,&self.probe.certificate,&self.probe.owner_pin,self.probe.actor_revision)?;
            (self.proofs.as_ref().ok_or_else(unavailable)?.certificate.clone(),self.probe.owner_pin.clone(),self.probe.actor_revision)
        };
        self.live(&shared.context)?;
        let observed:Selected=self.peer.exchange(&app,"observeRotation",json!({"expectedRev":revision,"expectedPin":final_pin,
            "rotationId":candidate.rotation_id,"certificateDigest":certificate.body.digest().map_err(|_|unknown())?}),None).await?;
        self.verify_selected(&observed,&certificate.body,&final_pin,revision)?;self.live(&shared.context)?;
        let mut next=self.intent.clone().ok_or_else(unknown)?;next.selected_observed=true;
        next.observed_owner_pin_json=Some(bytes(&observed.owner_pin)?);next.observed_actor_revision=Some(observed.actor_revision);
        self.save_intent(&app,next).await?;shared.context.check().await?;Ok(observed)
    }
}

enum Outcome {Candidate(Ready),Selected(Selected)}
struct State {ready:Option<Ready>,view:Value,busy:bool,prepare_consumed:bool,confirm_consumed:bool,orphaned:bool}
struct Shared {context:Context,until:Instant,state:Mutex<State>,done:watch::Sender<bool>,carrier:transport::Observation}
pub struct NodeRotationReview {shared:Arc<Shared>}
impl std::fmt::Debug for NodeRotationReview {
    fn fmt(&self,f:&mut std::fmt::Formatter<'_>)->std::fmt::Result{f.write_str("NodeRotationReview([redacted])")}
}
impl Drop for NodeRotationReview {fn drop(&mut self){self.cancel();}}
impl Shared {
    fn pending_block(&self){if let Ok(mut state)=self.state.lock(){state.view["localBlocked"]=Value::Null;}}
    fn blocked(&self){if let Ok(mut state)=self.state.lock(){state.view["localBlocked"]=json!(true);}}
    fn retire(&self){self.context.cancel.cancel();self.carrier.abort();if let Ok(mut state)=self.state.lock(){
        if !state.busy {state.ready.take();if state.view["selectionObserved"]!=true {
            state.view["phase"]=json!(if state.prepare_consumed||state.confirm_consumed {"unknown"}
                else if Instant::now()>=self.until {"expired"}else{"cancelled"});
        }}
    }}
    fn publish(&self,mut view:Value)->AppResult<String>{self.context.scope.with_io_current(|_|{
        if self.context.cancel.is_cancelled()||Instant::now()>=self.until{return Err(unavailable())}
        let ms=self.until.saturating_duration_since(Instant::now()).as_millis().min(60000);if ms==0{return Err(unavailable())}
        view["version"]=json!(1);view["profileId"]=json!(self.context.frame.profile_id);
        view["notificationScopeId"]=json!(self.context.frame.notification_scope_id);view["remainingMs"]=json!(ms as u64);
        let raw=serde_json::to_string(&view).map_err(|_|unknown())?;
        if raw.len()>16384||Instant::now()>=self.until||self.context.cancel.is_cancelled(){return Err(unavailable())}Ok(raw)
    })}
    fn finish(&self,result:AppResult<Outcome>)->AppResult<String>{
        let mut state=self.state.lock().map_err(|_|unknown())?;
        let current=self.context.scope.check_current().is_ok()&&!self.context.cancel.is_cancelled()&&Instant::now()<self.until;
        let result=if current {result}else{Err(unknown())};
        let good=result.is_ok();
        match result {
            Ok(Outcome::Candidate(ready))=>{
                let candidate=ready.candidate()?;
                state.view["phase"]=json!("rotationCertificateReview");
                state.view["rotationId"]=json!(candidate.rotation_id);
                state.view["proposedNode"]=json!({"agentId":candidate.agent_id,"keyEpoch":"2",
                    "publicKey":candidate.new_public_key,"keyFingerprint":candidate.new_key_fingerprint});
                state.ready=Some(ready);
            }
            Ok(Outcome::Selected(value))=>{
                state.view["phase"]=json!("rotated");state.view["selectionObserved"]=json!(true);
                state.view["localBlocked"]=json!(false);state.view["node"]=data::node(&value.node);
                state.view["actorRevision"]=json!(value.actor_revision.to_string());
                state.view["checkpoint"]=json!(value.owner_pin.checkpoint.to_string());
                state.view["certificateDigest"]=json!(value.certificate_digest);
                state.view["certificateExpiresUtc"]=json!(data::utc(value.certificate.expires_at)?);
                state.ready=None;
            }
            Err(_)=>{state.ready=None;state.view["phase"]=json!("unknown");state.view["selectionObserved"]=json!(false);}
        }
        state.busy=false;self.carrier.release_closed();state.view["cleanupState"]=json!(self.carrier.cleanup_state());
        let view=state.view.clone();drop(state);self.done.send_replace(true);
        if good {self.publish(view)}else{Err(unknown())}
    }
}
struct WorkGuard {shared:Arc<Shared>,finished:bool}
impl Drop for WorkGuard {fn drop(&mut self){if !self.finished {
    self.shared.carrier.abort();if let Ok(mut state)=self.shared.state.lock(){state.busy=false;state.orphaned=true;
        state.ready=None;state.view["phase"]=json!("unknown");state.view["selectionObserved"]=json!(false);}
    self.shared.done.send_replace(true);
}}}
impl NodeRotationReview {
    pub fn cancel(&self){self.shared.retire();}
    pub async fn details_json(&self)->AppResult<String>{self.shared.context.check().await?;
        let state=self.shared.state.lock().map_err(|_|unknown())?;
        if let Some(ready)=&state.ready {ready.live(&self.shared.context)?;}self.shared.publish(state.view.clone())}
    pub async fn prepare_candidate_once(&self)->AppResult<String>{
        self.shared.context.check().await?;let ready={let mut state=self.shared.state.lock().map_err(|_|unknown())?;
            if state.busy||state.prepare_consumed||state.confirm_consumed||state.view["phase"]!="rotationPreparationReview"{return Err(unavailable())}
            let ready=state.ready.take().ok_or_else(unavailable)?;ready.live(&self.shared.context)?;
            state.prepare_consumed=true;state.busy=true;state.view["phase"]=json!("preparingCandidate");self.shared.done.send_replace(false);ready};
        let shared=self.shared.clone();let(tx,rx)=oneshot::channel();self.shared.context.runtime.spawn(async move{
            let mut guard=WorkGuard{shared:shared.clone(),finished:false};
            let result=bounded_original(&shared.context.scope,under_review(shared.until,ready.prepare(&shared))).await.map(Outcome::Candidate);
            let result=shared.finish(result);guard.finished=true;let _=tx.send(result);
        });rx.await.map_err(|_|unknown())?
    }
    pub async fn confirm_once(&self)->AppResult<String>{
        self.shared.context.check().await?;let ready={let mut state=self.shared.state.lock().map_err(|_|unknown())?;
            if state.busy||state.confirm_consumed||!matches!(state.view["phase"].as_str(),Some("rotationCertificateReview"|"rotationReconcileReview")){return Err(unavailable())}
            let ready=state.ready.take().ok_or_else(unavailable)?;ready.live(&self.shared.context)?;
            state.confirm_consumed=true;state.busy=true;state.view["phase"]=json!("rotating");self.shared.done.send_replace(false);ready};
        let shared=self.shared.clone();let(tx,rx)=oneshot::channel();self.shared.context.runtime.spawn(async move{
            let mut guard=WorkGuard{shared:shared.clone(),finished:false};
            let result=bounded_original(&shared.context.scope,under_review(shared.until,ready.confirm(&shared))).await.map(Outcome::Selected);
            let result=shared.finish(result);guard.finished=true;let _=tx.send(result);
        });rx.await.map_err(|_|unknown())?
    }
    pub async fn joined_json(&self)->AppResult<String>{let end=Instant::now()+Duration::from_secs(5);
        let mut done=self.shared.done.subscribe();let result=tokio::time::timeout_at(tokio::time::Instant::from_std(end),async{
            while !*done.borrow(){done.changed().await.map_err(|_|unknown())?;}self.shared.carrier.wait_closed(end).await
        }).await;let state=self.shared.state.lock().map_err(|_|unknown())?;
        let closed=matches!(result,Ok(Ok(())))&&!state.busy&&!state.orphaned;
        serde_json::to_string(&json!({"version":1,"phase":state.view["phase"],"selectionObserved":state.view["selectionObserved"],
            "cleanupState":if closed {"closedObserved"}else{"unknown"}})).map_err(|_|unknown())
    }
}
async fn prepare(app:&AppCore,raw:&str,mode:Mode)->AppResult<NodeRotationReview>{
    let request:Request=parse(raw).map_err(|_|unavailable())?;let id=Uuid::parse_str(&request.association_id).map_err(|_|unavailable())?;
    if id.is_nil()||id.to_string()!=request.association_id{return Err(unavailable())}
    let context=Context::capture(app,Frame{version:request.version,profile_id:request.profile_id,
        notification_scope_id:request.notification_scope_id}).await?;let original=context.scope.clone();
    bounded_original(&original,async move{
        let app=context.app()?;let mut ledger=ledger::Ledger::capture(&app,context.scope.clone()).await?;
        let snapshot=ledger.snapshot(&app).await?;let row=snapshot.records().iter().find(|r|r.public.association_id==id).cloned().ok_or_else(unavailable)?;
        if row.revocation.is_some() || if mode==Mode::Normal {
            row.phase!=NodeAssociationPhase::Associated||row.rotation.is_some()
        }else{row.phase!=NodeAssociationPhase::Blocked||row.rotation.as_ref().is_none_or(|r|r.selected_observed)} {
            return Err(unavailable())
        }
        let key=context.scope.with_loaded(&app,|id|Ok(PublicKey::from_bytes(id.public_keys().signing))).await?;
        data::association(&row,key)?;
        let previous=ledger::initial_certificate(&row,key)?;
        if previous.body.key_epoch!=1{return Err(unavailable())}
        let (candidate,proofs,trusted_pin,intent)=if mode==Mode::Normal {
            (None,None,ledger::current_pin(&row,key)?,None)
        }else{
            let(candidate,proofs,pin)=ledger::rotation_parts(&row,key)?;
            (Some(candidate),Some(proofs),pin,row.rotation.clone())
        };
        context.scope.bind_owner_pin(&trusted_pin)?;
        context.validate_host(row.public.saved_host_id,row.public.saved_host_revision).await?;
        let carrier=transport::Observation::new();let permit=carrier.retain_permit(review_permit()?)?;
        let mut peer=transport::Peer::open_rotation_observed(&app,context.scope.clone(),row.public.saved_host_id,
            row.public.saved_host_revision,carrier.clone()).await?;
        if peer.route_digest!=row.public.route_trust_digest{return Err(unavailable())}
        let probe:Probe=peer.exchange(&app,if mode==Mode::Normal{"probeRotation"}else{"probeRotationReconcile"},json!({}),None).await?;
        let mut expected_pin=trusted_pin.clone();expected_pin.checkpoint=probe.owner_pin.checkpoint;
        if probe.owner_pin!=expected_pin||probe.owner_pin.checkpoint<trusted_pin.checkpoint||probe.revoked
            || probe.old_node.format!=1||probe.old_node.agent_id!=previous.body.agent_id
            || probe.old_node.key_epoch!=1||probe.old_node.public_key!=previous.body.agent_key
            || probe.old_node.key_fingerprint!=previous.body.agent_key.fingerprint(){return Err(unavailable())}
        context.scope.bind_owner_pin(&probe.owner_pin)?;
        if mode==Mode::Normal {
            if probe.certificate!=previous.body||probe.selection_state!="legacy1"||probe.candidate.is_some()
                || probe.node.as_ref()!=Some(&probe.old_node){return Err(unavailable())}
        }else{
            let proofs=proofs.as_ref().ok_or_else(unavailable)?;let intent=intent.as_ref().ok_or_else(unavailable)?;
            if probe.certificate!=proofs.certificate.body||probe.candidate!=candidate
                || !matches!(probe.selection_state.as_str(),"activationPending"|"selected2"|"prepared1")
                || probe.actor_revision<intent.expected_actor_revision{return Err(unavailable())}
        }
        let cert=if mode==Mode::Normal{&previous.body}else{&probe.certificate};
        check_live(cert.not_before,cert.expires_at,context.scope.now()?).map_err(|_|unavailable())?;
        let until=context.scope.deadline().min(Instant::now()+context.scope.remaining_until(cert.expires_at.min(probe.owner_pin.expires_at))?);
        peer.cap_review(until)?;ledger.cap_review(until)?;
        let owner=context.owner().await?;let phase=if mode==Mode::Normal{"rotationPreparationReview"}else{"rotationReconcileReview"};
        let proposed=candidate.as_ref().map(|c|json!({"agentId":c.agent_id,"keyEpoch":"2","publicKey":c.new_public_key,"keyFingerprint":c.new_key_fingerprint}));
        let view=json!({"phase":phase,"associationId":id,"hostId":row.public.saved_host_id,"hostRevision":row.public.saved_host_revision.to_string(),
            "owner":owner,"nodeContext":data::node_context(&probe.owner_pin.context)?,"previousNode":data::node(&probe.old_node),"node":probe.node.as_ref().map(data::node),"proposedNode":proposed,
            "rotationId":candidate.as_ref().map(|c|&c.rotation_id),"actorRevision":probe.actor_revision.to_string(),"checkpoint":probe.owner_pin.checkpoint.to_string(),
            "certificateDigest":cert.digest().map_err(|_|unavailable())?,"certificateExpiresUtc":data::utc(cert.expires_at)?,
            "scope":"tunnel.open","capabilitiesUnchanged":true,"expiryExtension":false,"selectionObserved":false,
            "localBlocked":mode==Mode::Reconcile,"actorAdmission":false,"cleanupState":"pending"});
        let ready=Ready{peer,ledger,row,previous,probe,until,mode,candidate,intent,proofs,_permit:permit};ready.live(&context)?;
        let(done,_)=watch::channel(true);let shared=Arc::new(Shared{context,until,state:Mutex::new(State{ready:Some(ready),view,
            busy:false,prepare_consumed:mode==Mode::Reconcile,confirm_consumed:false,orphaned:false}),done,carrier});
        let weak=Arc::downgrade(&shared);let scope=shared.context.scope.clone();shared.context.runtime.spawn(async move{
            scope.cancelled().await;if let Some(shared)=weak.upgrade(){shared.retire();}
        });shared.context.check().await?;Ok(NodeRotationReview{shared})
    }).await.map_err(|_|unavailable())
}
impl AppCore {
    pub async fn agent_node_association_rotation_prepare_json(&self,raw:&str)->AppResult<NodeRotationReview>{prepare(self,raw,Mode::Normal).await}
    pub async fn agent_node_association_rotation_reconcile_prepare_json(&self,raw:&str)->AppResult<NodeRotationReview>{prepare(self,raw,Mode::Reconcile).await}
}


// Type-layout diagnostic only: the closures are NEVER invoked. No AppCore,
// private Ready, authority operation, or future value is constructed or polled.
#[cfg(test)]
#[test]
fn rotation_future_type_layout_without_construction() {
    fn bytes<F: std::future::Future>(_: impl FnOnce() -> F) -> usize {
        std::mem::size_of::<F>()
    }
    let app: Option<&'static AppCore> = None;
    let review: Option<&'static NodeRotationReview> = None;
    let shared: Option<&'static Shared> = None;
    let normal = bytes(move || app.unwrap().agent_node_association_rotation_prepare_json(""));
    let reconcile = bytes(move || app.unwrap().agent_node_association_rotation_reconcile_prepare_json(""));
    let preparation = bytes(move || prepare(app.unwrap(), "", Mode::Normal));
    let prepare_candidate = bytes(move || review.unwrap().prepare_candidate_once());
    let confirm = bytes(move || review.unwrap().confirm_once());
    let ready: Option<Ready> = None;
    let ready_prepare = bytes(move || ready.unwrap().prepare(shared.unwrap()));
    let ready: Option<Ready> = None;
    let ready_confirm = bytes(move || ready.unwrap().confirm(shared.unwrap()));
    println!("CC_ROTATION_FUTURE_LAYOUT normal={} reconcile={} prepare={} prepare_candidate={} confirm={} ready_prepare={} ready_confirm={} ready_value={}",
        normal, reconcile, preparation, prepare_candidate, confirm, ready_prepare,
        ready_confirm, std::mem::size_of::<Ready>());
}
