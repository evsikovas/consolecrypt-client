//! Separate fresh ExportersOnly enrollment; Metrics/Tunnel are never upgraded.
use super::*;
impl NodeAssociationReview {
    pub(in crate::agent_tunnel::association) async fn capture_exporters_original_weak(core:&std::sync::Weak<crate::app::Inner>,
        scope:Arc<OriginalScope>,host:ObjectId,revision:i64,permit:tokio::sync::OwnedSemaphorePermit,
        observation:transport::Observation)->AppResult<Self> {
        let permit=observation.retain_permit(permit)?;
        let mut ledger=ledger::Ledger::capture_original(scope.clone()).await?;
        let custody={
            let app=AppCore{inner:core.upgrade().ok_or_else(unavailable)?};
            ManagedTlsCustody::capture(&app,scope.clone()).await?
        };
        // Existing schema requires a REAL stored public client identity. ExportersOnly
        // does not load its OS private TLS key or claim TLS-key possession here.
        let(phase,public)=custody.stored_metadata_original(core).await?;
        if phase!=Some(cc_storage_core::TunnelTlsPhase::Stored){return Err(unavailable())}
        let tls=public.ok_or_else(unavailable)?;
        let mut peer=transport::Peer::open_observed_weak(core,scope.clone(),host,revision,observation).await?;
        let advertised:protocol::ExporterProbe=peer.exchange_weak(core,"probeExporters",serde_json::json!({}),None).await?;
        let(probe,exporter)=advertised.checked(&scope,host,true)?;
        ledger.require_unassociated_original(host,probe.node.agent_id).await?;
        let until=scope.deadline().min(Instant::now()+Duration::from_secs(60));
        peer.cap_review(until)?;ledger.cap_review(until)?;
        let value=Self{purpose:AssociationPurpose::Exporters,metrics:None,exporter:Some(exporter),scope,peer,ledger,probe,tls,until,_permit:permit};
        value.live()?;Ok(value)
    }
    pub(in crate::agent_tunnel::association) async fn approve_exporters_offer_once(mut self,core:&std::sync::Weak<crate::app::Inner>)->AppResult<NodeClaimReview> {
        if self.purpose!=AssociationPurpose::Exporters||self.exporter.is_none(){return Err(unavailable())}
        self.live()?;let now=self.scope.now()?;
        let remaining=self.until.checked_duration_since(Instant::now()).filter(|d|d.as_secs()>0).ok_or_else(unavailable)?;
        let expiry=now.0.checked_add(remaining.as_secs().min(60)).map_err(|_|unavailable())?.min(self.probe.owner_pin.expires_at);
        let mut token=Zeroizing::new([0u8;32]);cc_crypto_core::fill_random(token.as_mut()).map_err(|_|unavailable())?;
        let secret=InvitationToken::from_bytes(*token).map_err(|_|unavailable())?;
        let offer=Offer{header:Header{version:1,context:self.probe.owner_pin.context.clone(),owner_epoch:self.probe.owner_pin.epoch,
            checkpoint:protocol::advance(&self.probe.owner_pin,1)?.checkpoint},
            invitation_id:InvitationId::new(&Uuid::new_v4().to_string()).map_err(|_|unavailable())?,agent_id:self.probe.node.agent_id,
            agent_key:self.probe.node.public_key,token_hash:secret.digest(),scopes:Scopes(vec![Scope::ExportersManage]),issued_at:now.0,expires_at:expiry};
        let prepared=self.purpose.offer(&self.probe.owner_pin,offer,now)?;
        let TunnelOwnerSigned::Offer(offer)=sign_reviewed_original(core,&self.scope,self.until,&self.probe.owner_pin,prepared).await?
            else{return Err(unavailable())};
        let association_id=Uuid::new_v4();
        let public=NodeAssociationPublic{association_id,saved_host_id:self.peer.host_id,saved_host_revision:self.peer.host_revision,
            route_trust_digest:self.peer.route_digest.clone(),agent_id:agent_text(self.probe.node.agent_id)?,
            node_key_fingerprint:hex::encode(self.probe.node.key_fingerprint.as_bytes()),owner_epoch:self.probe.owner_pin.epoch,
            checkpoint:self.probe.owner_pin.checkpoint,key_epoch:self.probe.node.key_epoch,expected_actor_revision:self.probe.actor_revision,
            client_tls_identity_id:self.tls.identity_id,client_tls_certificate_sha256:self.tls.certificate_sha256.clone(),
            offer_digest:hex::encode(offer.body.digest().map_err(|_|unknown())?.as_bytes()),signed_offer_json:bytes(&offer)?,
            owner_pin_json:bytes(&self.probe.owner_pin)?,certificate:None};
        self.ledger.reserve_original(public).await?;
        self.ledger.mark_unknown_original(association_id).await?; // BEFORE either actual remote CAS.
        self.live().map_err(|_|unknown())?;
        let token_hex=Zeroizing::new(hex::encode(token.as_ref()));
        let claim=self.peer.stage_weak(core,self.probe.actor_revision,&self.probe.owner_pin,&offer,token_hex.as_str()).await?;
        protocol::claimed_for(self.purpose,&self.probe,&offer,&claim,self.scope.now()?)?;
        self.until=self.until.min(Instant::now()+self.scope.remaining_until(expiry)?);
        self.peer.cap_review(self.until)?;self.ledger.cap_review(self.until)?;
        Ok(NodeClaimReview{original:self,offer,claim,association_id})
    }
}
impl NodeClaimReview {
    pub(in crate::agent_tunnel::association) async fn approve_exporters_certificate_once(mut self,core:&std::sync::Weak<crate::app::Inner>,
        lifetime:Duration)->AppResult<AssociatedNode> {
        let o=&mut self.original;
        if o.purpose!=AssociationPurpose::Exporters||o.exporter.is_none(){return Err(unavailable())}
        o.live().map_err(|_|unknown())?;
        if lifetime.is_zero()||lifetime.subsec_nanos()!=0||lifetime>Duration::from_secs(CERTIFICATE_TTL){return Err(unavailable())}
        let now=o.scope.now()?;
        protocol::claimed_for(o.purpose,&o.probe,&self.offer,&self.claim,now)?;
        let expires=now.0.checked_add(lifetime.as_secs()).map_err(|_|unavailable())?
            .min(o.probe.owner_pin.expires_at).min(client_expiry(&o.tls)?);
        let cert=Certificate{header:Header{version:1,context:o.probe.owner_pin.context.clone(),owner_epoch:o.probe.owner_pin.epoch,
            checkpoint:protocol::advance(&o.probe.owner_pin,2)?.checkpoint},agent_id:o.probe.node.agent_id,key_epoch:1,
            agent_key:o.probe.node.public_key,scopes:Scopes(vec![Scope::ExportersManage]),capabilities:self.claim.claim.body.capabilities.clone(),
            origin:CertificateOrigin::Enrollment{invitation_id:self.offer.body.invitation_id,claim_hash:self.claim.claim_digest,
                key_fingerprint:o.probe.node.key_fingerprint},not_before:now.0,expires_at:expires};
        let prepared=o.purpose.certificate(&self.claim.owner_pin,self.offer.clone(),self.claim.claim.clone(),cert,now)?;
        let TunnelOwnerSigned::Certificate(cert)=sign_reviewed_original(core,&o.scope,o.until,&self.claim.owner_pin,prepared).await?
            else{return Err(unknown())};
        let payload=serde_json::json!({"expectedRev":self.claim.actor_revision,"expectedPin":self.claim.owner_pin,
            "claimDigest":self.claim.claim_digest,"certificate":cert});
        let final_pin=protocol::advance(&o.probe.owner_pin,2)?;
        let revision=o.probe.actor_revision.checked_add(3).ok_or_else(unknown)?;
        o.ledger.attach_original(self.association_id,NodeAssociationCertificate{
            request_digest:hash(protocol::canonical(&payload)?.as_slice()),certificate_digest:hex::encode(cert.body.digest().map_err(|_|unknown())?.as_bytes()),
            signed_certificate_json:bytes(&cert)?,committed_owner_pin_json:bytes(&final_pin)?,expected_actor_revision:revision}).await?;
        let applied:protocol::Committed=o.peer.exchange_weak(core,"applyCertificate",payload,Some(self.offer.body.expires_at)).await?;
        protocol::committed(&o.probe,&cert,&applied,o.scope.now()?)?;
        let observed:protocol::Committed=o.peer.exchange_weak(core,"observeCertificate",serde_json::json!({
            "expectedRev":revision,"expectedPin":final_pin,"agentId":o.probe.node.agent_id,
            "certificateDigest":cert.body.digest().map_err(|_|unknown())?}),Some(self.offer.body.expires_at)).await?;
        protocol::committed(&o.probe,&cert,&observed,o.scope.now()?)?;
        o.scope.check_current()?;o.ledger.complete_original(self.association_id).await?;
        Ok(AssociatedNode{purpose:AssociationPurpose::Exporters,scope:o.scope.clone(),association_id:self.association_id,
            certificate:cert,pin:final_pin,actor_revision:observed.actor_revision})
    }
}
