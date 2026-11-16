//! One original-owner revocation. No trust bootstrap, key export or retry.
use super::*;
use cc_storage_core::{NodeAssociationPhase, NodeAssociationRecord, NodeAssociationRevocation};
use std::sync::Mutex;
use tokio::sync::{oneshot, watch, OwnedSemaphorePermit};
fn unavailable() -> AppError { AppError::invalid("agent_node_revocation", "revocation_unavailable") }
fn unknown() -> AppError { AppError::invalid("agent_node_revocation", "revocation_unknown") }

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct RevokeRequest {
    version: u8,
    profile_id: String,
    notification_scope_id: String,
    association_id: String,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Revoked {
    node: protocol::NodeIdentity,
    owner_pin: OwnerPin,
    actor_revision: u64,
    certificate_digest: Hash,
    revoked: bool,
}
struct Ready {
    purpose:AssociationPurpose,
    peer: transport::Peer,
    ledger: ledger::Ledger,
    original_row: NodeAssociationRecord,
    certificate: Signed<Certificate>,
    probe: protocol::Probe,
    until: Instant,
    _permit: Arc<OwnedSemaphorePermit>,
}
impl Ready {
    fn check(&self, context: &Context) -> AppResult<()> {
        context.scope.check_current()?;
        consent_live(self.until)?;
        context.scope.bind_owner_pin(&self.probe.owner_pin)?;
        self.purpose.revoke(&self.probe.owner_pin, self.certificate.clone(),
            self.body(context.scope.now()?)?, context.scope.now()?)?;
        Ok(())
    }
    fn body(&self, now: Now) -> AppResult<Revocation> {
        Ok(Revocation {
            header: Header { version: 1, context: self.probe.owner_pin.context.clone(),
                owner_epoch: self.probe.owner_pin.epoch,
                checkpoint: self.probe.owner_pin.checkpoint.checked_add(1).ok_or_else(unknown)? },
            agent_id: self.certificate.body.agent_id,
            key_epoch: self.certificate.body.key_epoch,
            certificate_hash: self.certificate.body.digest().map_err(|_| unavailable())?,
            issued_at: now.0,
        })
    }
    fn verify_readback(&self, reply: &Revoked, pin: &OwnerPin, revision: u64) -> AppResult<()> {
        if reply.node != self.probe.node || &reply.owner_pin != pin
            || reply.actor_revision != revision || !reply.revoked
            || reply.certificate_digest != self.certificate.body.digest().map_err(|_| unknown())? {
            return Err(unknown());
        }
        Ok(())
    }
    async fn confirm(mut self, shared: &Shared) -> AppResult<()> {
        if matches!(self.purpose,AssociationPurpose::Metrics|AssociationPurpose::Exporters){return self.confirm_scoped_original(shared).await}
        self.check(&shared.context)?;
        let app = shared.context.app()?;
        let now = shared.context.scope.now()?;
        let body = self.body(now)?;
        let prepared = PreparedTunnelOwner::revoke(&self.probe.owner_pin,
            self.certificate.clone(), body, now).map_err(|_| unavailable())?;
        let signed = sign_reviewed(&app, &shared.context.scope, self.until,
            &self.probe.owner_pin, prepared).await?;
        let TunnelOwnerSigned::Revocation(revocation) = signed else { return Err(unknown()); };
        let mut final_pin = self.probe.owner_pin.clone();
        final_pin.checkpoint = final_pin.checkpoint.checked_add(1).ok_or_else(unknown)?;
        let revision = self.probe.actor_revision.checked_add(1).ok_or_else(unknown)?;
        let intent = NodeAssociationRevocation {
            signed_revocation_json: bytes(&revocation)?,
            committed_owner_pin_json: bytes(&final_pin)?, expected_actor_revision: revision,
        };
        // Durable local Blocked+Unknown is mandatory BEFORE the first write.
        // No retry/reconnect can be manufactured from the saved intent.
        if let Ok(mut state) = shared.state.lock() { state.view["localBlocked"] = Value::Null; }
        let route_completion=route::original_revocation_started(&shared.context,&self.original_row)?;
        self.ledger.begin_revocation(&app, self.original_row.clone(), intent.clone()).await?;
        shared.mark_blocked();
        self.check(&shared.context)?;
        let applied: Revoked = self.peer.exchange(&app, "revokeCertificate",
            json!({"expectedRev":self.probe.actor_revision,"expectedPin":self.probe.owner_pin,
                "revocation":revocation}), None).await?;
        self.verify_readback(&applied, &final_pin, revision)?;
        self.check(&shared.context)?;
        let observed: Revoked = self.peer.exchange(&app, "observeRevocation",
            json!({"expectedRev":revision,"expectedPin":final_pin,
                "agentId":self.certificate.body.agent_id,"keyEpoch":self.certificate.body.key_epoch,
                "certificateDigest":self.certificate.body.digest().map_err(|_| unknown())?}), None).await?;
        self.verify_readback(&observed, &final_pin, revision)?;
        self.check(&shared.context)?;
        self.ledger.complete_revocation(&app, self.original_row.public.association_id, intent).await?;
        shared.context.check().await?;
        route_completion.verified(&shared.context)?;
        if let Ok(mut state) = shared.state.lock() {
            state.view["actorRevision"] = json!(observed.actor_revision.to_string());
            state.view["checkpoint"] = json!(observed.owner_pin.checkpoint.to_string());
        }
        Ok(())
    }
}
struct State {
    ready: Option<Ready>,
    view: Value,
    busy: bool,
    consumed: bool,
    orphaned: bool,
}
struct Shared {
    context: Context,
    until: Instant,
    state: Mutex<State>,
    done: watch::Sender<bool>,
    carrier: transport::Observation,
}
/// Issued public product review. Idle state owns no strong Core/Session/Unlocked.
pub struct NodeRevocationReview { shared: Arc<Shared> }
impl std::fmt::Debug for NodeRevocationReview {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("NodeRevocationReview([redacted])")
    }
}
impl Drop for NodeRevocationReview { fn drop(&mut self) { self.cancel(); } }
impl Shared {
    fn mark_blocked(&self) {
        if let Ok(mut state) = self.state.lock() {
            state.view["localBlocked"] = json!(true);
            state.view["remoteRevocationObserved"] = json!(false);
        }
    }
    fn retire(&self) {
        self.context.cancel.cancel();
        self.carrier.abort();
        if let Ok(mut state) = self.state.lock() {
            if !state.busy {
                state.ready.take();
                if state.view["remoteRevocationObserved"] != true {
                    state.view["phase"] = json!(if state.consumed { "unknown" }
                        else if Instant::now() >= self.until { "expired" } else { "cancelled" });
                }
            }
        }
    }
    fn publish(&self, mut view: Value) -> AppResult<String> {
        self.context.scope.with_io_current(|_| {
            if self.context.cancel.is_cancelled() || Instant::now() >= self.until { return Err(unavailable()); }
            let ms = self.until.saturating_duration_since(Instant::now()).as_millis().min(60000);
            if ms == 0 { return Err(unavailable()); }
            view["version"] = json!(1);
            view["profileId"] = json!(self.context.frame.profile_id);
            view["notificationScopeId"] = json!(self.context.frame.notification_scope_id);
            view["remainingMs"] = json!(ms as u64);
            let raw = serde_json::to_string(&view).map_err(|_| unknown())?;
            if raw.len() > 16384 || Instant::now() >= self.until || self.context.cancel.is_cancelled() {
                return Err(unavailable());
            }
            Ok(raw)
        })
    }
    fn finish(&self, result: AppResult<()>) -> AppResult<String> {
        let mut state = self.state.lock().map_err(|_| unknown())?;
        let ok = result.is_ok() && self.context.scope.check_current().is_ok()
            && !self.context.cancel.is_cancelled() && Instant::now() < self.until;
        state.busy = false;
        state.view["phase"] = json!(if ok { "revoked" } else { "unknown" });
        state.view["remoteRevocationObserved"] = json!(ok);
        self.carrier.release_closed();
        state.view["cleanupState"] = json!(self.carrier.cleanup_state());
        let view = state.view.clone();
        drop(state);
        self.done.send_replace(true);
        if ok { self.publish(view) } else { Err(unknown()) }
    }
}
struct WorkGuard { shared: Arc<Shared>, finished: bool }
impl Drop for WorkGuard {
    fn drop(&mut self) {
        if !self.finished {
            self.shared.carrier.abort();
            if let Ok(mut state) = self.shared.state.lock() {
                state.busy = false; state.orphaned = true;
                state.view["phase"] = json!("unknown");
                state.view["remoteRevocationObserved"] = json!(false);
            }
            self.shared.done.send_replace(true);
        }
    }
}
impl NodeRevocationReview {
    pub fn cancel(&self) { self.shared.retire(); }
    pub async fn details_json(&self) -> AppResult<String> {
        self.shared.context.check().await?;
        let state = self.shared.state.lock().map_err(|_| unknown())?;
        if let Some(ready) = &state.ready { ready.check(&self.shared.context)?; }
        self.shared.publish(state.view.clone())
    }
    pub async fn confirm_once(&self) -> AppResult<String> {
        self.shared.context.check().await?;
        let ready = {
            let mut state = self.shared.state.lock().map_err(|_| unknown())?;
            if state.busy || state.consumed { return Err(unavailable()); }
            let ready = state.ready.take().ok_or_else(unavailable)?;
            ready.check(&self.shared.context)?;
            state.consumed = true; state.busy = true;
            state.view["phase"] = json!("revoking");
            self.shared.done.send_replace(false);
            ready
        };
        let shared = self.shared.clone(); let (tx, rx) = oneshot::channel();
        self.shared.context.runtime.spawn(async move {
            let mut guard = WorkGuard { shared: shared.clone(), finished: false };
            let result = bounded_original(&shared.context.scope,
                under_review(shared.until, ready.confirm(&shared))).await;
            let result = shared.finish(result);
            guard.finished = true; let _ = tx.send(result);
        });
        rx.await.map_err(|_| unknown())?
    }
    pub async fn joined_json(&self) -> AppResult<String> {
        let end = Instant::now() + Duration::from_secs(5);
        let mut done = self.shared.done.subscribe();
        let waited = tokio::time::timeout_at(tokio::time::Instant::from_std(end), async {
            while !*done.borrow() { done.changed().await.map_err(|_| unknown())?; }
            self.shared.carrier.wait_closed(end).await
        }).await;
        let state = self.shared.state.lock().map_err(|_| unknown())?;
        let closed = matches!(waited, Ok(Ok(()))) && !state.busy && !state.orphaned;
        serde_json::to_string(&json!({"version":1,"phase":state.view["phase"],
            "remoteRevocationObserved":state.view["remoteRevocationObserved"],
            "cleanupState":if closed {"closedObserved"} else {"unknown"}})).map_err(|_| unknown())
    }
}
impl AppCore {
    pub async fn agent_node_association_revoke_prepare_json(&self, raw: &str) -> AppResult<NodeRevocationReview> {
        let request: RevokeRequest = parse(raw).map_err(|_| unavailable())?;
        let association_id = Uuid::parse_str(&request.association_id).map_err(|_| unavailable())?;
        if association_id.is_nil() || association_id.to_string() != request.association_id { return Err(unavailable()); }
        let context = Context::capture(self, Frame {version:request.version,profile_id:request.profile_id,
            notification_scope_id:request.notification_scope_id}).await?;
        let original = context.scope.clone();
        bounded_original(&original, async move {
            let app = context.app()?;
            let mut ledger = ledger::Ledger::capture(&app, context.scope.clone()).await?;
            let snapshot = ledger.snapshot(&app).await?;
            let row = snapshot.records().iter().find(|r|r.public.association_id == association_id
                && r.phase == NodeAssociationPhase::Associated && r.revocation.is_none()).cloned().ok_or_else(unavailable)?;
            let key = context.scope.with_loaded(&app, |id| Ok(PublicKey::from_bytes(id.public_keys().signing))).await?;
            data::association(&row, key)?; // Stored row never supplies signing authority.
            let certificate=ledger::current_certificate(&row,key)?;
            let trusted_pin=ledger::current_pin(&row,key)?;
            context.scope.bind_owner_pin(&trusted_pin)?;
            context.validate_host(row.public.saved_host_id, row.public.saved_host_revision).await?;
            let permit = review_permit()?;
            let carrier = transport::Observation::new();
            let permit = carrier.retain_permit(permit)?;
            let mut peer = transport::Peer::open_observed(&app, context.scope.clone(),row.public.saved_host_id,
                row.public.saved_host_revision,carrier.clone()).await?;
            if peer.route_digest != row.public.route_trust_digest { return Err(unavailable()); }
            let probe: protocol::Probe = peer.exchange(&app,"probeRevocation",json!({}),None).await?;
            let mut expected_pin = trusted_pin.clone(); expected_pin.checkpoint = probe.owner_pin.checkpoint;
            if probe.owner_pin != expected_pin || probe.owner_pin.checkpoint < trusted_pin.checkpoint
                || probe.node.agent_id != certificate.body.agent_id || probe.node.public_key != certificate.body.agent_key
                || probe.node.key_epoch != certificate.body.key_epoch
                || probe.node.key_fingerprint != certificate.body.agent_key.fingerprint()
                || probe.certificate.as_ref() != Some(&certificate.body) || probe.revoked {
                return Err(unavailable());
            }
            context.scope.bind_owner_pin(&probe.owner_pin)?;
            let until = context.scope.deadline().min(Instant::now()+context.scope.remaining_until(probe.owner_pin.expires_at)?);
            peer.cap_review(until)?; ledger.cap_review(until)?;
            let ready = Ready {purpose:AssociationPurpose::Tunnel,peer,ledger,original_row:row,certificate,probe,until,_permit:permit};
            ready.check(&context)?;
            let owner = context.owner().await?;
            let view = json!({"phase":"revocationReview","associationId":association_id,
                "hostId":ready.original_row.public.saved_host_id,"hostRevision":ready.original_row.public.saved_host_revision.to_string(),
                "node":data::node(&ready.probe.node),"owner":owner,"nodeContext":data::node_context(&ready.probe.owner_pin.context)?,
                "certificateDigest":ready.certificate.body.digest().map_err(|_| unavailable())?,
                "actorRevision":ready.probe.actor_revision.to_string(),"checkpoint":ready.probe.owner_pin.checkpoint.to_string(),
                "localBlocked":false,"remoteRevocationObserved":false,"actorAdmission":false,"cleanupState":"pending"});
            let (done,_) = watch::channel(true);
            let shared = Arc::new(Shared {context,until,state:Mutex::new(State {ready:Some(ready),view,
                busy:false,consumed:false,orphaned:false}),done,carrier});
            let weak = Arc::downgrade(&shared); let scope = shared.context.scope.clone();
            shared.context.runtime.spawn(async move {scope.cancelled().await;
                if let Some(shared)=weak.upgrade(){shared.retire();}});
            shared.context.check().await?;
            Ok(NodeRevocationReview {shared})
        }).await.map_err(|_| unavailable())
    }
}


impl Ready {
    async fn confirm_scoped_original(mut self,shared:&Shared)->AppResult<()> {
        if !matches!(self.purpose,AssociationPurpose::Metrics|AssociationPurpose::Exporters){return Err(unavailable())}
        self.check(&shared.context)?;
        let now=shared.context.scope.now()?;
        let prepared=self.purpose.revoke(&self.probe.owner_pin,self.certificate.clone(),self.body(now)?,now)?;
        let signed=sign_reviewed_original(&shared.context.core,&shared.context.scope,self.until,&self.probe.owner_pin,prepared).await?;
        let TunnelOwnerSigned::Revocation(revocation)=signed else{return Err(unknown())};
        let mut final_pin=self.probe.owner_pin.clone();
        final_pin.checkpoint=final_pin.checkpoint.checked_add(1).ok_or_else(unknown)?;
        let revision=self.probe.actor_revision.checked_add(1).ok_or_else(unknown)?;
        let intent=NodeAssociationRevocation{signed_revocation_json:bytes(&revocation)?,
            committed_owner_pin_json:bytes(&final_pin)?,expected_actor_revision:revision};
        if let Ok(mut state)=shared.state.lock(){state.view["localBlocked"]=Value::Null;}
        self.ledger.begin_revocation_original(self.original_row.clone(),intent.clone()).await?;
        shared.mark_blocked();self.check(&shared.context)?;
        let applied:Revoked=self.peer.exchange_weak(&shared.context.core,"revokeCertificate",json!({
            "expectedRev":self.probe.actor_revision,"expectedPin":self.probe.owner_pin,"revocation":revocation}),None).await?;
        self.verify_readback(&applied,&final_pin,revision)?;self.check(&shared.context)?;
        let observed:Revoked=self.peer.exchange_weak(&shared.context.core,"observeRevocation",json!({
            "expectedRev":revision,"expectedPin":final_pin,"agentId":self.certificate.body.agent_id,
            "keyEpoch":self.certificate.body.key_epoch,
            "certificateDigest":self.certificate.body.digest().map_err(|_|unknown())?}),None).await?;
        self.verify_readback(&observed,&final_pin,revision)?;self.check(&shared.context)?;
        self.ledger.complete_revocation_original(self.original_row.public.association_id,intent).await?;
        shared.context.check().await?;
        if let Ok(mut state)=shared.state.lock(){state.view["actorRevision"]=json!(observed.actor_revision.to_string());
            state.view["checkpoint"]=json!(observed.owner_pin.checkpoint.to_string());}
        Ok(())
    }
}
impl AppCore {
    /// Consumes the caller's short-lived Core clone before any SSH/SQL wait.
    /// Caller supplies association intent only; no catalog, grant or TLS key.
    pub async fn agent_metrics_node_association_revoke_prepare_json(self,raw:&str)->AppResult<NodeRevocationReview> {
        let request:RevokeRequest=parse(raw).map_err(|_|unavailable())?;
        let association_id=Uuid::parse_str(&request.association_id).map_err(|_|unavailable())?;
        if association_id.is_nil()||association_id.to_string()!=request.association_id{return Err(unavailable())}
        let context=Context::capture(&self,Frame{version:request.version,profile_id:request.profile_id,
            notification_scope_id:request.notification_scope_id}).await?;
        drop(self);
        let original=context.scope.clone();
        bounded_original(&original,async move{
            let mut ledger=ledger::Ledger::capture_original(context.scope.clone()).await?;
            let snapshot=ledger.snapshot_original().await?;
            let row=snapshot.records().iter().find(|r|r.public.association_id==association_id
                &&r.phase==NodeAssociationPhase::Associated&&r.revocation.is_none()&&r.rotation.is_none())
                .cloned().ok_or_else(unavailable)?;
            let key={let app=context.app()?;
                context.scope.with_loaded(&app,|id|Ok(PublicKey::from_bytes(id.public_keys().signing))).await?};
            data::association(&row,key)?;
            let certificate=ledger::current_certificate(&row,key)?;
            AssociationPurpose::Metrics.check_scopes(&certificate.body.scopes)?;
            AssociationPurpose::Metrics.check_scopes(&certificate.body.capabilities.scopes)?;
            if certificate.body.key_epoch!=1{return Err(unavailable())}
            let trusted_pin=ledger::current_pin(&row,key)?;context.scope.bind_owner_pin(&trusted_pin)?;
            context.validate_host(row.public.saved_host_id,row.public.saved_host_revision).await?;
            let carrier=transport::Observation::new();let permit=carrier.retain_permit(review_permit()?)?;
            let mut peer=transport::Peer::open_observed_weak(&context.core,context.scope.clone(),
                row.public.saved_host_id,row.public.saved_host_revision,carrier.clone()).await?;
            if peer.route_digest!=row.public.route_trust_digest{return Err(unavailable())}
            let advertised:protocol::MetricsProbe=peer.exchange_weak(&context.core,"probeMetricsRevocation",json!({}),None).await?;
            let probe=advertised.checked_revocation(&context.scope)?;
            let mut expected_pin=trusted_pin.clone();expected_pin.checkpoint=probe.owner_pin.checkpoint;
            if probe.owner_pin!=expected_pin||probe.owner_pin.checkpoint<trusted_pin.checkpoint
                ||probe.node.agent_id!=certificate.body.agent_id||probe.node.public_key!=certificate.body.agent_key
                ||probe.node.key_epoch!=certificate.body.key_epoch||probe.node.key_fingerprint!=certificate.body.agent_key.fingerprint()
                ||probe.certificate.as_ref()!=Some(&certificate.body)||probe.revoked{return Err(unavailable())}
            context.scope.bind_owner_pin(&probe.owner_pin)?;
            let until=context.scope.deadline().min(Instant::now()+context.scope.remaining_until(probe.owner_pin.expires_at)?);
            peer.cap_review(until)?;ledger.cap_review(until)?;
            let ready=Ready{purpose:AssociationPurpose::Metrics,peer,ledger,original_row:row,certificate,probe,until,_permit:permit};
            ready.check(&context)?;
            let owner=context.owner().await?;
            let view=json!({"phase":"revocationReview","purpose":"metrics","scope":"metrics.collect","scopes":["metrics.collect"],
                "associationId":association_id,"hostId":ready.original_row.public.saved_host_id,
                "hostRevision":ready.original_row.public.saved_host_revision.to_string(),"node":data::node(&ready.probe.node),
                "owner":owner,"nodeContext":data::node_context(&ready.probe.owner_pin.context)?,
                "certificateDigest":ready.certificate.body.digest().map_err(|_|unavailable())?,
                "actorRevision":ready.probe.actor_revision.to_string(),"checkpoint":ready.probe.owner_pin.checkpoint.to_string(),
                "localBlocked":false,"remoteRevocationObserved":false,"actorAdmission":false,"cleanupState":"pending"});
            let(done,_)=watch::channel(true);
            let shared=Arc::new(Shared{context,until,state:Mutex::new(State{ready:Some(ready),view,
                busy:false,consumed:false,orphaned:false}),done,carrier});
            let weak=Arc::downgrade(&shared);let scope=shared.context.scope.clone();
            shared.context.runtime.spawn(async move{scope.cancelled().await;if let Some(shared)=weak.upgrade(){shared.retire();}});
            shared.context.check().await?;Ok(NodeRevocationReview{shared})
        }).await.map_err(|_|unavailable())
    }
}

impl AppCore {
    /// Consumes the caller's short-lived Core clone before any SSH/SQL wait.
    /// Caller supplies association intent only; no catalog, grant or TLS key.
    pub async fn agent_exporter_node_association_revoke_prepare_json(self,raw:&str)->AppResult<NodeRevocationReview> {
        let request:RevokeRequest=parse(raw).map_err(|_|unavailable())?;
        let association_id=Uuid::parse_str(&request.association_id).map_err(|_|unavailable())?;
        if association_id.is_nil()||association_id.to_string()!=request.association_id{return Err(unavailable())}
        let context=Context::capture(&self,Frame{version:request.version,profile_id:request.profile_id,
            notification_scope_id:request.notification_scope_id}).await?;
        drop(self);
        let original=context.scope.clone();
        bounded_original(&original,async move{
            let mut ledger=ledger::Ledger::capture_original(context.scope.clone()).await?;
            let snapshot=ledger.snapshot_original().await?;
            let row=snapshot.records().iter().find(|r|r.public.association_id==association_id
                &&r.phase==NodeAssociationPhase::Associated&&r.revocation.is_none()&&r.rotation.is_none())
                .cloned().ok_or_else(unavailable)?;
            let key={let app=context.app()?;
                context.scope.with_loaded(&app,|id|Ok(PublicKey::from_bytes(id.public_keys().signing))).await?};
            data::association(&row,key)?;
            let certificate=ledger::current_certificate(&row,key)?;
            AssociationPurpose::Exporters.check_scopes(&certificate.body.scopes)?;
            AssociationPurpose::Exporters.check_scopes(&certificate.body.capabilities.scopes)?;
            if certificate.body.key_epoch!=1{return Err(unavailable())}
            let trusted_pin=ledger::current_pin(&row,key)?;context.scope.bind_owner_pin(&trusted_pin)?;
            context.validate_host(row.public.saved_host_id,row.public.saved_host_revision).await?;
            let carrier=transport::Observation::new();let permit=carrier.retain_permit(review_permit()?)?;
            let mut peer=transport::Peer::open_observed_weak(&context.core,context.scope.clone(),
                row.public.saved_host_id,row.public.saved_host_revision,carrier.clone()).await?;
            if peer.route_digest!=row.public.route_trust_digest{return Err(unavailable())}
            let advertised:protocol::ExporterProbe=peer.exchange_weak(&context.core,"probeExportersRevocation",json!({}),None).await?;
            let probe=advertised.checked_revocation(&context.scope)?;
            let mut expected_pin=trusted_pin.clone();expected_pin.checkpoint=probe.owner_pin.checkpoint;
            if probe.owner_pin!=expected_pin||probe.owner_pin.checkpoint<trusted_pin.checkpoint
                ||probe.node.agent_id!=certificate.body.agent_id||probe.node.public_key!=certificate.body.agent_key
                ||probe.node.key_epoch!=certificate.body.key_epoch||probe.node.key_fingerprint!=certificate.body.agent_key.fingerprint()
                ||probe.certificate.as_ref()!=Some(&certificate.body)||probe.revoked{return Err(unavailable())}
            context.scope.bind_owner_pin(&probe.owner_pin)?;
            let until=context.scope.deadline().min(Instant::now()+context.scope.remaining_until(probe.owner_pin.expires_at)?);
            peer.cap_review(until)?;ledger.cap_review(until)?;
            let ready=Ready{purpose:AssociationPurpose::Exporters,peer,ledger,original_row:row,certificate,probe,until,_permit:permit};
            ready.check(&context)?;
            let owner=context.owner().await?;
            let view=json!({"phase":"revocationReview","purpose":"exporters","scope":"exporters.manage","scopes":["exporters.manage"],
                "associationId":association_id,"hostId":ready.original_row.public.saved_host_id,
                "hostRevision":ready.original_row.public.saved_host_revision.to_string(),"node":data::node(&ready.probe.node),
                "owner":owner,"nodeContext":data::node_context(&ready.probe.owner_pin.context)?,
                "certificateDigest":ready.certificate.body.digest().map_err(|_|unavailable())?,
                "actorRevision":ready.probe.actor_revision.to_string(),"checkpoint":ready.probe.owner_pin.checkpoint.to_string(),
                "localBlocked":false,"remoteRevocationObserved":false,"actorAdmission":false,"cleanupState":"pending"});
            let(done,_)=watch::channel(true);
            let shared=Arc::new(Shared{context,until,state:Mutex::new(State{ready:Some(ready),view,
                busy:false,consumed:false,orphaned:false}),done,carrier});
            let weak=Arc::downgrade(&shared);let scope=shared.context.scope.clone();
            shared.context.runtime.spawn(async move{scope.cancelled().await;if let Some(shared)=weak.upgrade(){shared.retire();}});
            shared.context.check().await?;Ok(NodeRevocationReview{shared})
        }).await.map_err(|_|unavailable())
    }
}
