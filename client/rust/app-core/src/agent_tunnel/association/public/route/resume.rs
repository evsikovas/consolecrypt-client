//! Original-operation transport loop. No startup scan or replacement approval.
use super::*;
use crate::agent_tunnel::v2::OriginalGenerationOutcome;

const MAX_GENERATIONS:usize=8;
pub(super) struct Controls { owned:Mutex<Vec<transport::Observation>> }
impl Controls {
    pub(super) fn new(initial:transport::Observation)->Self {Self{owned:Mutex::new(vec![initial])}}
    pub(super) fn next(&self)->AppResult<transport::Observation> {
        let mut owned=self.owned.lock().map_err(|_|unknown())?;
        if owned.len()>=MAX_GENERATIONS || owned.iter().any(|p|p.cleanup_state()!="closedObserved") {return Err(unknown())}
        let next=transport::Observation::new();owned.push(next.clone());Ok(next)
    }
    pub(super) fn abort(&self) {
        if let Ok(owned)=self.owned.lock(){for control in owned.iter(){control.abort()}}
    }
    pub(super) async fn wait_closed(&self,end:Instant)->AppResult<()> {
        let owned=self.owned.lock().map_err(|_|unknown())?.clone();
        for control in owned {control.wait_closed(end).await?;}
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn a_never_opened_or_aborted_control_is_not_a_previous_generation_close_witness() {
        let control=transport::Observation::new();
        let controls=Controls::new(control.clone());
        assert!(controls.next().is_err());
        controls.abort();
        assert_eq!(control.cleanup_state(),"notStarted");
        assert!(controls.next().is_err());
    }
    #[test]
    fn lower_bound_native_accounting_never_wraps_across_transports() {
        let mut total=u64::MAX-2;
        add(&mut total,2).unwrap();assert_eq!(total,u64::MAX);
        assert!(add(&mut total,1).is_err());assert_eq!(total,u64::MAX);
    }
}
fn add(total:&mut u64,n:u64)->AppResult<()> {*total=total.checked_add(n).ok_or_else(unknown)?;Ok(())}

pub(super) async fn run_original(shared:Arc<Shared>,mut prepared:producer::Prepared,
    ready:oneshot::Sender<AppResult<String>>) {
    let mut ready=Some(ready);let mut route=None;let mut slot=None;let mut audit=None;
    let mut slot_attempted=false;let mut audit_closed=true;
    let mut outcome="unknown";let mut sent=0;let mut received=0;
    let mut attempt_sent=0;let mut attempt_received=0;
    let association=prepared.association;let host=prepared.host;let revision=prepared.host_revision;
    let route_digest=prepared.route_digest();let certificate_hash=prepared.policy.route.agent_certificate_hash;
    let owner_key=prepared.pin.key;
    // Capture once from the ORIGINAL grant/scope, before any transport work.
    let revocation_end=shared.context.scope.remaining_until(prepared.lease.expires_at)
        .map(|remaining|shared.context.scope.deadline().min(Instant::now()+remaining))
        .unwrap_or_else(|_|Instant::now());
    let action=async {
        prepared.revalidate(&shared.context).await?;
        preference::save(&shared.context,&prepared).await?;
        shared.context.validate_host(prepared.host,prepared.host_revision).await?;
        let tls=prepared.load_tls(&shared.context).await?;
        let original={let app=shared.context.app()?;
            let review=TunnelRouteReviewV2::capture_original(&app,shared.context.scope.clone(),prepared.pin.clone(),
                prepared.certificate.clone(),prepared.policy.clone(),prepared.lease.clone(),prepared.freshness.clone(),tls).await?;
            Arc::new(review.approve_once(&app).await?)};
        let binding=original.original_binding()?.clone();
        route=Some(original.clone());
        audit=Some(Arc::new(RouteAudit::open(&shared.context.scope,prepared.host,prepared.host_revision,
            prepared.association,&binding,&prepared.policy).await?));
        {let mut state=shared.state.lock().map_err(|_|unknown())?;
            state.audit=audit.clone();state.route=Some(original.clone());}
        let listener=original.bind_loopback_original().await?;
        let mut logical=original.begin_original_channel()?;
        let generations=async {
            slot_attempted=true;
            slot=Some(prepared.create_slot(&shared.context).await?);
            prepared.start_transport(&shared.context,&binding,slot.as_mut().ok_or_else(unknown)?).await?;
            shared.context.check().await?;
            {let mut state=shared.state.lock().map_err(|_|unknown())?;
                state.view["phase"]=json!("listening");state.view["boundAddress"]=json!(listener.address().to_string());
                state.view["grantExpiresUtc"]=json!(data::utc(binding.lease.body.expires_at)?);}
            automatic::track_monitoring(&shared)?;
            if let Some(tx)=ready.take(){let _=tx.send(shared.publish());}
            loop {
                let accepted=listener.accept_once().await?;
                shared.monitoring.accepted(accepted.monitoring_peer()?);
                shared.phase("connecting");shared.context.scope.check_current()?;
                let carrier=relay_original(&shared.context,binding.lease.body.expires_at,async {
                    slot.as_ref().ok_or_else(unknown)?.connect_client_once().await.map_err(|_|unknown())
                }).await?;
                match original.serve_original_generation_weak(shared.context.core.clone(),&mut logical,accepted,carrier,
                    audit.as_ref().ok_or_else(unknown)?).await {
                    OriginalGenerationOutcome::StopRequested{sent:s,received:r}=>{
                        attempt_sent=s;attempt_received=r;
                        add(&mut sent,s)?;add(&mut received,r)?;
                        prepared.observe_terminal(&shared.context).await?;
                        outcome="stopRequested";return Ok::<(),AppError>(())
                    },
                    OriginalGenerationOutcome::Lost{sent:s,received:r}=>{
                        attempt_sent=s;attempt_received=r;
                        add(&mut sent,s)?;add(&mut received,r)?;
                        shared.phase("listening");
                        // A monitoring borrow is one GET, never a reconnect.
                        // Preserve its response/publication fence until that
                        // exact query releases, within the SAME original TTL.
                        // Actual streams have already dropped; no payload is
                        // sent while this original cleanup owner waits.
                        if shared.monitoring.wait_for_read_release().await {
                            return Err(unknown())
                        }
                        // The old byte stream has been dropped. Its uncertain
                        // delivery remains an Unknown access event forever.
                        let previous=audit.take().ok_or_else(unknown)?;
                        audit_closed=false;
                        previous.finish(RouteAuditEnd::Unknown,s,r).await?;
                        audit_closed=true;
                        original.check_original_resume_window()?;
                        let retired=prepared.observe_retired(&shared.context,&binding).await?;
                        prepared.close().await?;
                        shared.control.wait_closed(Instant::now()+Duration::from_secs(3)).await?;
                        let old=slot.take().ok_or_else(unknown)?;
                        tokio::time::timeout(Duration::from_secs(3),old.revoke_once()).await
                            .map_err(|_|unknown())?.map_err(|_|unknown())?;
                        slot_attempted=false;
                        original.check_original_resume_window()?;
                        let next=shared.control.next()?;
                        // Fixed finite pause, charged against SAME idle/TTL.
                        tokio::time::sleep(Duration::from_millis(250)).await;
                        original.check_original_resume_window()?;
                        prepared.reopen_original_resume(&shared.context,&binding,retired,next).await?;
                        attempt_sent=0;attempt_received=0;
                        audit=Some(Arc::new(RouteAudit::open(&shared.context.scope,prepared.host,prepared.host_revision,
                            prepared.association,&binding,&prepared.policy).await?));
                        {let mut state=shared.state.lock().map_err(|_|unknown())?;state.audit=audit.clone();}
                        slot_attempted=true;
                        slot=Some(prepared.create_slot(&shared.context).await?);
                        prepared.start_transport(&shared.context,&binding,slot.as_mut().ok_or_else(unknown)?).await?;
                        original.check_original_resume_window()?;
                        // Same listener and next fresh application TCP stream.
                        // No byte from the damaged connection is replayed.
                        shared.phase("listening");
                    },
                    OriginalGenerationOutcome::Unknown=>return Err(unknown()),
                }
            }
        };
        tokio::select!{biased;_=original.original_cancelled()=>Err(unknown()),r=generations=>r}
    };
    let watched=producer::watch_blocked(shared.context.clone(),association,host,revision,route_digest,certificate_hash,owner_key);
    let(result,mut local_end)=tokio::select!{biased;end=watched=>(Err(unknown()),Some(end)),
        result=bounded_original(&shared.context.scope,action)=>(result,None)};
    if let Some(route)=route.take(){route.cancel();}
    let transport_closed=match slot.take(){Some(slot)=>tokio::time::timeout(Duration::from_secs(3),slot.revoke_once())
        .await.is_ok_and(|r|r.is_ok()),None=>!slot_attempted};
    let control_retired=prepared.close().await.is_ok();
    let control_closed=shared.control.wait_closed(Instant::now()+Duration::from_secs(3)).await.is_ok();
    if let Some(end)=automatic::after_original_transport_end(&shared,revocation_end).await {local_end=Some(end);}
    if let Some(audit)=audit {
        let event=if let Some(end)=local_end{end}else if outcome=="stopRequested"{RouteAuditEnd::StopRequested}
            else if shared.context.cancel.is_cancelled(){RouteAuditEnd::Cancelled}else{RouteAuditEnd::Unknown};
        audit_closed=audit.finish(event,attempt_sent,attempt_received).await.is_ok();
    }
    if result.is_err() || !audit_closed {outcome="unknown";}
    shared.finished(outcome,transport_closed&&control_retired&&control_closed&&audit_closed,sent,received);
    if let Some(tx)=ready.take(){let _=tx.send(Err(unknown()));}
}
