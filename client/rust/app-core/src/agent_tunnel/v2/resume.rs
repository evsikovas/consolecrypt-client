//! Sequential transports of ONE original logical channel. Never replay payload,
//! reset the Budget, recapture a signer, or turn a local EOF into Node drainage.
use super::*;
use cc_sync_core::api::RelayBinaryStream;
use tokio::io::AsyncWriteExt;

const MAX_GENERATIONS:u8=8;
pub(crate) struct OriginalChannelOwner { identity:Arc<()>, next:u8 }
pub(crate) enum OriginalGenerationOutcome {
    StopRequested{sent:u64,received:u64},
    // Local loss observation ONLY. The private producer must obtain actual
    // same-SSH Node retirement before it can open a subsequent transport.
    Lost{sent:u64,received:u64},
    Unknown,
}
impl NativeTunnelRouteV2 {
    pub(crate) fn begin_original_channel(&self)->AppResult<OriginalChannelOwner> {
        self.live()?;
        if self.io_started.swap(true,Ordering::SeqCst) {return Err(denied())}
        Ok(OriginalChannelOwner{identity:self.route_identity.clone(),next:0})
    }
    pub(crate) fn check_original_resume_window(&self)->AppResult<()> {
        self.live()?;
        if !self.policy.resume_original || self.stop.is_requested() {return Err(denied())}
        Ok(())
    }
    pub(crate) async fn original_cancelled(&self) {
        tokio::select!{biased;_=self.scope.cancelled()=>{},_=self.budget.cancelled()=>{}}
    }
    pub(crate) async fn serve_original_generation_weak(
        &self,core:std::sync::Weak<crate::app::Inner>,owner:&mut OriginalChannelOwner,
        accepted:AcceptedTunnelStream,carrier:RelayBinaryStream,
        audit:&super::super::association::access_audit::RouteAudit,
    )->OriginalGenerationOutcome {
        let original_app=||->AppResult<AppCore>{Ok(AppCore{inner:core.upgrade().ok_or_else(denied)?})};
        if !Arc::ptr_eq(&self.route_identity,&owner.identity)
            || !Arc::ptr_eq(&self.route_identity,&accepted.route_identity)
            || owner.next>=MAX_GENERATIONS
            || owner.next!=0 && self.check_original_resume_window().is_err()
            || audit.require_binding(&self.scope,&self.binding,&self.policy).is_err() {
            self.budget.stop();return OriginalGenerationOutcome::Unknown
        }
        let mode=if owner.next==0 {OriginalTunnelMode::Admit}else{OriginalTunnelMode::Resume};
        owner.next+=1; // A failed attempt is consumed, never silently retried.
        let mut local=accepted.stream;
        let action=async {
            self.scope.check(&original_app()?).await?;self.live()?;
            let actual=local.local_addr().map_err(|_|denied())?;
            let expected:std::net::IpAddr=self.policy.route.client_bind.ip.parse().map_err(|_|denied())?;
            if actual!=accepted.address || actual.ip()!=expected || !actual.ip().is_loopback()
                || self.policy.route.client_bind.port!=0 && actual.port()!=self.policy.route.client_bind.port {
                return Err(denied())
            }
            self.scope.bind_relay(&carrier)?;
            let io=self.io();
            let handshake=async {
                let mut tls=io.run(async{self.tls.connect(carrier,&self.policy.route).await.map_err(|_|denied())}).await?;
                self.live()?;
                let open=open_original_frame(&self.binding,&self.policy,mode).map_err(|_|denied())?;
                io.run(async{tls.write_all(&open).await.map_err(|_|denied())}).await?;
                io.run(async{tls.flush().await.map_err(|_|denied())}).await?;
                let challenge=io.run(client::read_frame(&mut tls)).await?;
                let mut pending=verify_original_challenge(&challenge,&self.pin,&self.binding,&self.policy,self.scope.now()?)
                    .map_err(|_|denied())?;
                self.scope.check(&original_app()?).await?;self.live()?;
                let echo=pending.echo_once().map_err(|_|denied())?;
                io.run(async{tls.write_all(&echo).await.map_err(|_|denied())}).await?;
                io.run(async{tls.flush().await.map_err(|_|denied())}).await?;
                let ack=io.run(client::read_frame(&mut tls)).await?;
                verify_original_ack(&ack,pending,&self.pin,&self.binding,&self.policy,self.scope.now()?)
                    .map_err(|_|denied())?;
                self.scope.check(&original_app()?).await?;self.live()?;
                audit.observe_opened().await?;
                self.scope.check(&original_app()?).await?;self.live()?;
                Ok::<_,AppError>(tls)
            };
            let mut tls=tokio::select!{biased;_=io.cancelled()=>return Err(denied()),
                _=self.stop.requested()=>return Err(denied()),r=handshake=>r?};
            let mut totals=client::Totals::default();
            let explicit_stop={
                let copy=client::copy_data(&mut tls,&mut local,&self.channel,&io,&mut totals);
                tokio::select!{biased;_=io.cancelled()=>false,
                    _=copy=>false,_=self.stop.requested()=>true}
            };
            if explicit_stop {
                self.live()?;
                if !self.stop.send_request_original(&mut tls,&io).await {return Err(denied())}
                return Ok(OriginalGenerationOutcome::StopRequested{sent:totals.sent,received:totals.received})
            }
            // No close_notify or payload replay on loss. Both actual local/TLS
            // streams drop before this outcome reaches the producer.
            self.check_original_resume_window()?;
            Ok(OriginalGenerationOutcome::Lost{sent:totals.sent,received:totals.received})
        };
        let result=tokio::select!{biased;_=self.scope.cancelled()=>Err(denied()),
            _=self.budget.cancelled()=>Err(denied()),r=action=>r};
        match result {Ok(value)=>value,Err(_)=>{self.budget.stop();OriginalGenerationOutcome::Unknown}}
    }
}
