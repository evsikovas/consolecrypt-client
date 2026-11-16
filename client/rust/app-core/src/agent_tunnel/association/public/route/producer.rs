//! Fixed saved-Host SSH producer. No public path/URL/certificate/pin input.
//! The matched Agent role is a concrete product prerequisite, not test IPC.
use super::*;
use cc_storage_core::{NodeAssociationPhase,NodeAssociationRecord,TunnelTlsPhase};
use cc_sync_core::api::{ApiClient,OriginalRelayApi,RelaySlot,RelaySlotLimits};
use rustls::{RootCertStore,pki_types::{CertificateDer,ServerName}};
use crate::agent_tunnel::{managed_tls::ManagedTlsCustody,tls::TunnelTls};

#[derive(Deserialize)]
#[serde(rename_all="camelCase",deny_unknown_fields)]
struct Descriptor {
    node:protocol::NodeIdentity, owner_pin:OwnerPin, actor_revision:u64,
    certificate:Certificate, certificate_digest:Hash, revoked:bool,
    transport_format:String, server_certificate_hex:String, server_root_hex:String,
    server_name:String, relay_origin:String,
}
#[derive(Deserialize)]
#[serde(rename_all="camelCase",deny_unknown_fields)]
struct TransportReady {
    route:ResourceId, policy_hash:Hash, lease_hash:Hash, certificate_digest:Hash,
    slot_id:Uuid, expires_at:Timestamp, node:protocol::NodeIdentity,
}
pub(super) struct Prepared {
    pub(super) association_record:NodeAssociationRecord,
    pub(super) host:ObjectId,pub(super) host_revision:i64,pub(super) association:Uuid,
    pub(super) pin:OwnerPin,pub(super) certificate:Signed<Certificate>,
    pub(super) policy:TunnelRoutePolicyV2,pub(super) lease:Lease,pub(super) freshness:Freshness,
    peer:Option<transport::Peer>,descriptor:Descriptor,custody:ManagedTlsCustody,route_digest:String,
    tls_public:TunnelTlsPublic,api:OriginalRelayApi,_permit:Arc<tokio::sync::OwnedSemaphorePermit>,
    mode:transport::OriginalTransportMode,
    transport_selection:super::super::outbound_control::Selection,
}
fn der(value:&str)->AppResult<Vec<u8>> {
    if value.is_empty()||value.len()>4096||value.len()%2!=0
        ||!value.bytes().all(|b|b.is_ascii_digit()||(b'a'..=b'f').contains(&b)){return Err(unavailable())}
    hex::decode(value).map_err(|_|unavailable())
}
fn api_origin(api:&ApiClient)->AppResult<String> {
    let url=&api.config().server_url;
    if url.scheme()!="https"||!url.username().is_empty()||url.password().is_some()
        ||url.query().is_some()||url.fragment().is_some(){return Err(unavailable())}
    let mut wss=url.clone();wss.set_scheme("wss").map_err(|_|unavailable())?;
    Ok(wss.origin().ascii_serialization())
}
impl Prepared {
    pub(super) async fn capture(context:&Context,association:Uuid,request:&Request,
        observation:transport::Observation)->AppResult<Self> {
        let ledger=ledger::Ledger::capture_original(context.scope.clone()).await?;
        let snapshot=ledger.snapshot_original().await?;
        let row:NodeAssociationRecord=snapshot.records().iter().find(|r|r.public.association_id==association
            &&r.phase==NodeAssociationPhase::Associated&&r.revocation.is_none()).cloned().ok_or_else(unavailable)?;
        let key=context.scope.with_current_resources(|_,_,id|Ok(PublicKey::from_bytes(id.public_keys().signing)))?;
        data::association(&row,key)?;
        let certificate=ledger::current_certificate(&row,key)?;
        let trusted_pin=ledger::current_pin(&row,key)?;
        context.scope.bind_owner_pin(&trusted_pin)?;
        context.validate_host(row.public.saved_host_id,row.public.saved_host_revision).await?;
        let permit=observation.retain_permit(review_permit()?)?;
        let transport_selection=super::super::outbound_control::select(context,association).await?;
        let mut peer=super::super::outbound_control::open_selected(context,association,&transport_selection,observation).await?;
        if peer.route_digest!=row.public.route_trust_digest{return Err(unavailable())}
        // Fixed new role verb, read-only actual selected Node/cert/TLS. Missing
        // product role is a real prerequisite error, never fixture data import.
        let descriptor:Descriptor=peer.exchange_weak(&context.core,"probeTunnelTransport",json!({}),None).await?;
        let minimum_revision=if let Some(rotation)=row.rotation.as_ref().filter(|r|r.selected_observed) {
            rotation.observed_actor_revision.ok_or_else(unavailable)?
        }else{row.public.certificate.as_ref().ok_or_else(unavailable)?.expected_actor_revision};
        let mut current=trusted_pin.clone();current.checkpoint=descriptor.owner_pin.checkpoint;
        if descriptor.revoked||descriptor.actor_revision<minimum_revision||descriptor.owner_pin!=current||current.checkpoint<trusted_pin.checkpoint
            ||descriptor.node.agent_id!=certificate.body.agent_id||descriptor.node.public_key!=certificate.body.agent_key
            ||descriptor.node.key_epoch!=certificate.body.key_epoch||descriptor.node.key_fingerprint!=certificate.body.agent_key.fingerprint()
            ||!matches!((descriptor.node.format,descriptor.node.key_epoch),(1,1)|(3,2))
            ||descriptor.certificate!=certificate.body||descriptor.certificate_digest!=certificate.body.digest().map_err(|_|unavailable())?
            ||descriptor.transport_format!="cc-agent-tunnel-transport/v1" {return Err(unavailable())}
        // A selected epoch2 is resolved through actual verified historical
        // rotation, never coerced to epoch1 or reconstructed from public IDs.
        context.scope.bind_owner_pin(&current)?;
        let custody={let app=context.app()?;ManagedTlsCustody::capture(&app,context.scope.clone()).await?};
        let(phase,public)=custody.stored_metadata_original(&context.core).await?;
        if phase!=Some(TunnelTlsPhase::Stored){return Err(unavailable())}
        let tls_public=public.ok_or_else(unavailable)?;
        let server=der(&descriptor.server_certificate_hex)?;
        let root=der(&descriptor.server_root_hex)?;
        let(rest,parsed)=x509_parser::parse_x509_certificate(&server).map_err(|_|unavailable())?;
        if !rest.is_empty()||descriptor.server_name.is_empty()||descriptor.server_name.len()>253
            ||!descriptor.server_name.is_ascii()||descriptor.server_name.chars().any(char::is_control){return Err(unavailable())}
        let server_expiry:u64=parsed.validity().not_after.timestamp().try_into().map_err(|_|unavailable())?;
        let original_owner=context.scope.owner_context()?;
        let (subject,device,api)=context.scope.with_current_resources(|session,_,id|{
            let subject=match original_owner {
                crate::agent_tunnel::scope::OriginalOwnerContext::PersonalAccount{user_id}=>TunnelClientSubject::PersonalAccount{id:user_id.to_string()},
                // Existing Relay requires the SAME authenticated Personal API.
                // Never borrow an account for a local profile.
                crate::agent_tunnel::scope::OriginalOwnerContext::LocalProfile{..}=>return Err(unavailable()),
            };
            Ok((subject,id.device_id().to_string(),session.api()?))
        })?;
        if descriptor.relay_origin!=api_origin(&api)? {return Err(unavailable())}
        // Parse/check the actual public trust root now; no supplied caller CA.
        let mut roots=RootCertStore::empty();roots.add(CertificateDer::from(root)).map_err(|_|unavailable())?;
        let _=ServerName::try_from(descriptor.server_name.clone()).map_err(|_|unavailable())?;
        let now=context.scope.now()?;
        let ttl=context.scope.remaining_until(certificate.body.expires_at)?.as_secs()
            .min(request.ttl_seconds as u64).min(context.scope.deadline().saturating_duration_since(Instant::now()).as_secs())
            .min(context.scope.remaining_until(current.expires_at)?.as_secs())
            .min(context.scope.remaining_until(client_expiry(&tls_public)?)?.as_secs())
            .min(server_expiry.checked_sub(now.0.seconds()).ok_or_else(unavailable)?);
        let ttl=match peer.original_control_lease_window()? {Some(window)=>ttl.min(window.as_secs()),None=>ttl};
        if ttl==0{return Err(unavailable())}
        let expires=Timestamp::new(now.0.seconds().checked_add(ttl).ok_or_else(unavailable)?).map_err(|_|unavailable())?;
        let policy=TunnelRoutePolicyV2{format:"cc-agent-tunnel-route-policy/v2".into(),resume_original:request.resume_original,
            route:TunnelRoutePolicy{format:"cc-agent-tunnel-route-policy/v1".into(),context:current.context.clone(),
                agent_id:certificate.body.agent_id,agent_certificate_hash:certificate.body.digest().map_err(|_|unavailable())?,
                key_epoch:certificate.body.key_epoch,route_id:ResourceId::new(&Uuid::new_v4().to_string()).map_err(|_|unavailable())?,
                client_subject:subject,client_device_id:device,client_tls_certificate_sha256:Hash::from_bytes(Sha256::digest(&tls_public.certificate_der).into()),
                agent_tls_certificate_sha256:Hash::from_bytes(Sha256::digest(&server).into()),
                destination:TunnelDestination{protocol:"tcp".into(),ip:request.destination_ip.clone(),port:request.destination_port},
                client_bind:TunnelClientBind{ip:request.listen_ip.clone(),port:request.listen_port},
                limits:TunnelLimits{max_ttl_seconds:ttl,idle_timeout_seconds:(request.idle_seconds as u64).min(ttl),
                    max_channels:1,max_frame_bytes:8192,max_pending_frames_per_channel:1,max_pending_frames_total:2,
                    max_queued_bytes_per_channel:8192,max_queued_bytes_total:16384,max_bytes_per_channel:4*1024*1024,max_bytes_total:4*1024*1024}}};
        policy.validate().map_err(|_|unavailable())?;
        let header=Header{version:1,context:current.context.clone(),owner_epoch:current.epoch,checkpoint:current.checkpoint};
        let lease=Lease{header:header.clone(),id:LeaseId::new(&Uuid::new_v4().to_string()).map_err(|_|unavailable())?,
            agent_id:certificate.body.agent_id,key_epoch:certificate.body.key_epoch,certificate_hash:certificate.body.digest().map_err(|_|unavailable())?,
            operation:policy.operation().map_err(|_|unavailable())?,issued_at:now.0,expires_at:expires};
        let freshness=Freshness{header,issued_at:now.0,expires_at:expires};
        let prepared=Self{association_record:row.clone(),host:row.public.saved_host_id,host_revision:row.public.saved_host_revision,association,pin:current,
            certificate,policy,lease,freshness,route_digest:row.public.route_trust_digest,peer:Some(peer),descriptor,custody,tls_public,api:api.original_relay_api(),_permit:permit,mode:transport::OriginalTransportMode::Admit,transport_selection};
        prepared.check(context)?;Ok(prepared)
    }
    pub(super) fn check(&self,context:&Context)->AppResult<()> {
        context.scope.check_current()?;context.scope.bind_owner_policy(&self.pin,&self.policy.route)?;
        check_live(self.lease.issued_at,self.lease.expires_at,context.scope.now()?).map_err(|_|unavailable())?;
        verify_statement(&self.certificate,self.pin.key).map_err(|_|unavailable())?;
        self.policy.validate().map_err(|_|unavailable())
    }
    pub(super) async fn revalidate(&self,context:&Context)->AppResult<()> {
        self.check(context)?;
        let ledger=ledger::Ledger::capture_original(context.scope.clone()).await?;
        let snapshot=ledger.snapshot_original().await?;
        let row=snapshot.records().iter().find(|r|r.public.association_id==self.association
            &&r.phase==NodeAssociationPhase::Associated&&r.revocation.is_none()).ok_or_else(unavailable)?;
        if row.public.saved_host_id!=self.host||row.public.saved_host_revision!=self.host_revision
            ||row.public.route_trust_digest!=self.route_digest
            ||ledger::current_certificate(row,self.pin.key)?.body.digest().map_err(|_|unavailable())?!=self.policy.route.agent_certificate_hash {
            return Err(unavailable())
        }
        context.validate_host(self.host,self.host_revision).await?;
        self.check(context)
    }
    pub(super) async fn view(&self,context:&Context)->AppResult<Value> {
        self.check(context)?;
        Ok(json!({"phase":"routeReview","routeId":self.policy.route.route_id,"associationId":self.association,"hostId":self.host,"hostRevision":self.host_revision.to_string(),
            "owner":context.owner().await?,"node":data::node(&self.descriptor.node),"certificateDigest":self.policy.route.agent_certificate_hash,
            "destination":self.policy.route.destination,"clientBind":self.policy.route.client_bind,"limits":self.policy.route.limits,
            "resumeOriginalSignedOptIn":self.policy.resume_original,"resumeTransportImplemented":true,"grantExpiresUtc":data::utc(self.lease.expires_at)?,
            "remainingMs":0,"boundAddress":null,"actorAdmission":false,"remoteDrainObserved":false,"cleanupState":"notStarted",
            "sentBytes":"0","receivedBytes":"0","countersExact":false,"nativeAccountingOnly":true}))
    }
    pub(super) async fn load_tls(&self,context:&Context)->AppResult<TunnelTls> {
        self.check(context)?;
        let identity=self.custody.load_original(&context.core).await?;
        if identity.public_identity()?.identity_id!=self.tls_public.identity_id
            ||identity.public_identity()?.certificate_sha256!=self.tls_public.certificate_sha256{return Err(unavailable())}
        let mut roots=RootCertStore::empty();roots.add(CertificateDer::from(der(&self.descriptor.server_root_hex)?)).map_err(|_|unavailable())?;
        identity.into_tls(&context.scope,roots,ServerName::try_from(self.descriptor.server_name.clone()).map_err(|_|unavailable())?,&self.policy.route)
    }
    pub(super) async fn create_slot(&self,context:&Context)->AppResult<RelaySlot> {
        self.revalidate(context).await?;
        let api=super::super::outbound_control::upgrade_api(context,&self.api)?;
        let authenticated=relay_original(context,self.lease.expires_at,async {
            api.capture_reader_auth().await.map_err(|_|unavailable())
        }).await?;
        context.check().await?;
        let remaining=context.scope.remaining_until(self.lease.expires_at)?.as_secs();
        if remaining==0{return Err(unavailable())}
        relay_original(context,self.lease.expires_at,async {
            api.create_relay_slot_once(authenticated,RelaySlotLimits{ttl_seconds:remaining,
                idle_seconds:self.policy.route.limits.idle_timeout_seconds.min(remaining),max_bytes:8*1024*1024}).await.map_err(|_|unknown())
        }).await
    }
    pub(super) async fn start_transport(&mut self,context:&Context,binding:&NodeRequestBinding,slot:&mut RelaySlot)->AppResult<()> {
        self.revalidate(context).await?;
        self.policy.check_binding(binding).map_err(|_|unavailable())?;
        // Parent owns the slot BEFORE any fallible provisioning write, and
        // always attempts its original DELETE even when the reply is lost.
        let capability=slot.take_agent_capability().map_err(|_|unknown())?.provision_once();
        let observed=context.scope.now()?.0;
        let floor=slot.remaining_seconds().map_err(|_|unknown())?;
        let expires=Timestamp::new(observed.seconds().checked_add(floor).ok_or_else(unknown)?).map_err(|_|unknown())?.min(self.lease.expires_at);
        let reply:TransportReady=self.peer.as_mut().ok_or_else(unknown)?.start_tunnel_transport(&context.core,
            self.descriptor.actor_revision,&self.pin,binding,&self.policy,&self.tls_public.certificate_der,
            slot.id(),&capability,expires,self.policy.route.limits.idle_timeout_seconds.min(floor),self.mode).await?;
        if reply.route!=self.policy.route.route_id||reply.policy_hash!=self.policy.digest().map_err(|_|unknown())?
            ||reply.lease_hash!=binding.lease.body.digest().map_err(|_|unknown())?
            ||reply.certificate_digest!=self.policy.route.agent_certificate_hash||reply.slot_id!=slot.id()
            ||reply.expires_at!=expires||reply.node!=self.descriptor.node {return Err(unknown())}
        self.check(context)?;Ok(())
    }
    pub(super) async fn observe_terminal(&mut self,context:&Context)->AppResult<()> {
        self.check(context)?;
        self.peer.as_mut().ok_or_else(unknown)?.observe_tunnel_terminal().await?;
        self.check(context)
    }
    pub(super) async fn observe_retired(&mut self,context:&Context,binding:&NodeRequestBinding)
        ->AppResult<transport::OriginalRetirementObservation> {
        self.check(context)?;
        let retired=self.peer.as_mut().ok_or_else(unknown)?
            .observe_tunnel_retirement(binding,&self.policy,&self.descriptor.node).await?;
        self.check(context)?;Ok(retired)
    }
    pub(super) async fn reopen_original_resume(&mut self,context:&Context,binding:&NodeRequestBinding,
        retired:transport::OriginalRetirementObservation,observation:transport::Observation)->AppResult<()> {
        self.check(context)?;
        if !self.policy.resume_original || self.peer.is_some() {return Err(unavailable())}
        retired.consume(binding,&self.policy,&self.descriptor.node)?;
        self.revalidate(context).await?;
        let mut peer=super::super::outbound_control::open_selected(context,self.association,&self.transport_selection,observation).await?;
        if peer.original_control_lease_window()?.is_some_and(|window|
            context.scope.remaining_until(self.lease.expires_at).map_or(true,|remaining|remaining>window)) {return Err(unavailable())}
        if peer.route_digest!=self.route_digest {return Err(unavailable())}
        let descriptor:Descriptor=peer.exchange_weak(&context.core,"probeTunnelTransport",json!({}),None).await?;
        let minimum=self.descriptor.actor_revision.checked_add(1).ok_or_else(unknown)?;
        if descriptor.revoked || descriptor.actor_revision<minimum || descriptor.owner_pin!=self.pin
            || descriptor.node!=self.descriptor.node || descriptor.certificate!=self.descriptor.certificate
            || descriptor.certificate_digest!=self.policy.route.agent_certificate_hash
            || descriptor.transport_format!=self.descriptor.transport_format
            || descriptor.server_certificate_hex!=self.descriptor.server_certificate_hex
            || descriptor.server_root_hex!=self.descriptor.server_root_hex
            || descriptor.server_name!=self.descriptor.server_name || descriptor.relay_origin!=self.descriptor.relay_origin {
            return Err(unavailable())
        }
        self.peer=Some(peer);self.descriptor=descriptor;self.mode=transport::OriginalTransportMode::Resume;
        self.revalidate(context).await?;self.check(context)
    }
    pub(super) fn route_digest(&self)->String {self.route_digest.clone()}
    pub(super) async fn close(&mut self)->AppResult<()> {
        let result=match self.peer.as_ref() {Some(peer)=>peer.retire_control_transport(Instant::now()+Duration::from_secs(3)).await,None=>Ok(())};
        drop(self.peer.take());result
    }
}

/// One original operation's bounded local-state watcher. No new registry,
/// remote polling, grant reconstruction or authority from a display record.
pub(super) async fn watch_blocked(context:Context,association:Uuid,host:ObjectId,revision:i64,route_digest:String,certificate_hash:Hash,key:PublicKey)->RouteAuditEnd {
    let mut tick=tokio::time::interval(Duration::from_secs(1));
    loop {
        tokio::select!{biased;_=context.scope.cancelled()=>return RouteAuditEnd::Cancelled,_=tick.tick()=>{}}
        let observed=async {
            let ledger=ledger::Ledger::capture_original(context.scope.clone()).await?;
            let snapshot=ledger.snapshot_original().await?;
            let row=snapshot.records().iter().find(|r|r.public.association_id==association).ok_or_else(unavailable)?;
            if row.phase==NodeAssociationPhase::Revoked {return Ok(Some(RouteAuditEnd::Revoked))}
            if row.phase!=NodeAssociationPhase::Associated||row.revocation.is_some()
                ||row.public.saved_host_id!=host||row.public.saved_host_revision!=revision
                ||row.public.route_trust_digest!=route_digest{return Ok(Some(RouteAuditEnd::Cancelled))}
            let current=ledger::current_certificate(row,key)?;
            if current.body.digest().map_err(|_|unavailable())?!=certificate_hash {return Ok(Some(RouteAuditEnd::Cancelled))}
            context.validate_host(row.public.saved_host_id,row.public.saved_host_revision).await?;
            Ok::<_,AppError>(None)
        };
        match bounded_original(&context.scope,observed).await {
            Ok(None)=>{},Ok(Some(end))=>return end,Err(_)=>return RouteAuditEnd::Unknown,
        }
    }
}
