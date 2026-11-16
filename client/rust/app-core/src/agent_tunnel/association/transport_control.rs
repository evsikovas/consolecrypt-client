//! Private original-association SSH over an issued outbound Relay carrier.
//! A stream/reference is transport data, never a Node permit or caller bridge.
use super::*;
use cc_sync_core::api::{RelayControlBootstrap,RelayControlReference,RelayControlStream};
use cc_storage_core::NodeAssociationPhase;

pub(super) struct OriginalControlInput {
    pub(super) stream:cc_sync_core::api::RelayBinaryStream,
    pub(super) deadline:Instant,
    pub(super) route_digest:String,
}
#[derive(serde::Deserialize)]
#[serde(rename_all="camelCase",deny_unknown_fields)]
struct BootstrapObserved {
    control_id:Uuid,expires_at:Timestamp,node:protocol::NodeIdentity,
    certificate_digest:Hash,actor_revision:u64,
}
impl Peer {
    pub(in crate::agent_tunnel::association) async fn retire_control_transport(&self,end:Instant)->AppResult<()> {
        self.observation.retire_control_before_close(end).await
    }
    /// Caller has already explicitly selected its encrypted public control ref.
    /// Repeat original SQL association/owner/cert checks; no wire Host/path is
    /// accepted and no unavailable control falls back to a different transport.
    pub(in crate::agent_tunnel::association) async fn open_control_observed_weak(
        core:&Weak<crate::app::Inner>,scope:Arc<OriginalScope>,association:Uuid,
        issued:RelayControlStream,observation:Observation,
    )->AppResult<Self> {
        if association.is_nil() || issued.reference().association_id!=association {return Err(unavailable())}
        let ledger=ledger::Ledger::capture_original(scope.clone()).await?;
        let snapshot=ledger.snapshot_original().await?;
        let row=snapshot.records().iter().find(|r|r.public.association_id==association
            &&r.phase==NodeAssociationPhase::Associated&&r.revocation.is_none()).cloned().ok_or_else(unavailable)?;
        let key=scope.with_current_resources(|_,_,id|Ok(PublicKey::from_bytes(id.public_keys().signing)))?;
        let certificate=ledger::current_certificate(&row,key)?;
        let pin=ledger::current_pin(&row,key)?;
        scope.bind_owner_pin(&pin)?;
        check_live(certificate.body.not_before,certificate.body.expires_at,scope.now()?).map_err(|_|unavailable())?;
        let owner=scope.owner_context()?;
        let reference=issued.reference();
        let same=scope.with_current_resources(|session,_,id| {
            let crate::agent_tunnel::scope::OriginalOwnerContext::PersonalAccount{user_id}=owner else {return Err(unavailable())};
            let api=session.api()?;
            if reference.user_id.to_string()!=user_id.to_string() || reference.device_id.to_string()!=id.device_id().to_string()
                || reference.server_url!=api.config().server_url.as_str() {return Err(unavailable())}
            Ok(())
        });same?;
        let (stream,_,deadline)=issued.into_original_stream();
        scope.bind_relay(&stream)?;
        let deadline=deadline.into_std().min(scope.deadline());
        consent_live(deadline)?;
        let mut peer=Self::open_planned_weak(core,scope.clone(),row.public.saved_host_id,
            row.public.saved_host_revision,observation,Some(OriginalControlInput {
                stream,deadline,route_digest:row.public.route_trust_digest.clone(),
            })).await?;
        // A SQL change during the real handshake cannot select a replacement
        // association or publish an authenticated session for the old row.
        let actual=ledger.snapshot_original().await?;
        if actual.records().iter().find(|r|r.public.association_id==association)!=Some(&row) {
            peer.observation.abort();drop(peer);return Err(unavailable())
        }
        peer.cap_review(deadline)?;
        scope.check_current()?;Ok(peer)
    }
    /// Use BEFORE signing/displaying a tunnel lease on a control carrier. The
    /// original actual stream deadline is clipped once; no future-now reset.
    pub(in crate::agent_tunnel::association) fn original_control_lease_window(&self)->AppResult<Option<Duration>> {
        if !self.control_transport {return Ok(None)}
        self.scope.check_current()?;
        let remaining=cap_read(&self.until)?.checked_duration_since(Instant::now()).ok_or_else(unavailable)?;
        Ok(Some(remaining.checked_sub(Duration::from_secs(10)).filter(|d|!d.is_zero()).ok_or_else(unavailable)?))
    }
    /// Explicit bootstrap only, after the SAME direct pinned SSH descriptor
    /// probe/review. Six exact fields; no endpoint, secret key or account token.
    /// The caller must revoke the registration on unknown reply/EOF, not save
    /// a public reference or automatically recover/adopt a token.
    pub(in crate::agent_tunnel::association) async fn bootstrap_tunnel_control_weak(
        &mut self,core:&Weak<crate::app::Inner>,reference:&RelayControlReference,
        credential:&RelayControlBootstrap,expected_revision:u64,pin:&OwnerPin,
        certificate_digest:Hash,node:&protocol::NodeIdentity,
    )->AppResult<()> {
        if self.control_transport || expected_revision==0 || reference.control_id.is_nil()
            || reference.association_id.is_nil() {return Err(unavailable())}
        self.scope.check_current()?;self.scope.bind_owner_pin(pin)?;
        let expires=Timestamp::new(reference.expires_at_unix_seconds).map_err(|_|unavailable())?;
        self.scope.remaining_until(expires)?;
        let secret=credential.expose_for_bootstrap();
        if secret.len()!=48 || !secret.starts_with("ccrc_")
            || !secret.bytes().all(|b|b.is_ascii_alphanumeric()||b==b'_'||b==b'-') {return Err(unavailable())}
        const MARKER:&str="CC_PRIVATE_NATIVE_CONTROL_BOOTSTRAP";
        let(id,template)=protocol::request("bootstrapTunnelControl",serde_json::json!({
            "expectedRev":expected_revision,"expectedPin":pin,"certificateDigest":certificate_digest,
            "controlId":reference.control_id,"agentToken":MARKER,"expiresAt":expires}))?;
        let needle=format!("\"{MARKER}\"").into_bytes();
        let locations=template.windows(needle.len()).enumerate().filter_map(|(i,b)|(b==needle).then_some(i)).collect::<Vec<_>>();
        if locations.len()!=1 {return Err(unavailable())}
        let at=locations[0];let mut raw=Zeroizing::new(Vec::new());
        raw.extend_from_slice(&template[..at]);raw.push(b'"');raw.extend_from_slice(secret.as_bytes());raw.push(b'"');
        raw.extend_from_slice(&template[at+needle.len()..]);
        if raw.len()>protocol::MAX_FRAME || !raw.is_ascii() {return Err(unavailable())}
        let reply:BootstrapObserved=self.exchange_frame_weak(core,"bootstrapTunnelControl",id,raw,Some(expires)).await?;
        if reply.control_id!=reference.control_id || reply.expires_at!=expires || reply.node!=*node
            || reply.certificate_digest!=certificate_digest || reply.actor_revision!=expected_revision {return Err(unknown())}
        let end=self.scope.deadline().min(cap_read(&self.until)?).min(Instant::now()+Duration::from_secs(5));
        let scope=self.scope.clone();
        let stream=self.stream.as_mut().and_then(|s|s.stream.as_mut()).ok_or_else(unknown)?;
        bounded(&scope,end,async {
            let mut tail=[0;1];
            if stream.read(&mut tail).await.map_err(|_|unknown())?!=0 {return Err(unknown())}
            Ok(())
        }).await?;
        self.scope.check_current()
    }
}
