//! Replacement for the unregistered 18fcc Native issuer registration.
//! Child of EXISTING credentials module: borrow its original Context/facts/refs.
//! No AppCore/session/unlock/OwnerScope capture and no per-ref review window.
use super::*;
use cc_agent_node_wire::{CollectorServiceCredentialAuthorization, CollectorServiceCredentialRef};
use cc_crypto_core::{CollectorServiceCredentialFrame, PreparedCollectorServiceCredentialOwner};

/// The actual scope retains original weak owner/unlock/auth/provenance. This
/// borrow is deliberately non-Serde/non-Clone, with no caller-supplied tuple.
struct OriginalLifetimeFrame<'a> {
    context: &'a Context,
    until: Instant,
}
impl<'a> OriginalLifetimeFrame<'a> {
    fn from_context(context: &'a Context) -> AppResult<Self> {
        let result = Self { context, until: context.scope.deadline() };
        result.check()?;
        Ok(result)
    }
    fn check(&self) -> AppResult<()> {
        if self.context.cancel.is_cancelled() || Instant::now() >= self.until {
            return Err(unavailable());
        }
        self.context.scope.check_current()
    }
    async fn run<T>(&self, work: impl std::future::Future<Output = AppResult<T>>) -> AppResult<T> {
        self.check()?;
        let result = under_review(self.until, bounded_original(&self.context.scope, work)).await?;
        self.check()?;
        Ok(result)
    }
    fn publish(&self, value: Value) -> AppResult<String> {
        self.check()?;
        // Context.publish performs its actual auth/provenance/version guarded
        // publication; an outer check is never a substitute for that guard.
        let raw = self.context.publish(value, 16384)?;
        self.check()?;
        Ok(raw)
    }
}

/// No raw fields/accessor to let a caller defer an unfenced frame publication.
/// The same original Context and facts remain borrowed through final handoff.
pub(in super::super) struct OriginalLifetimeSet<'a> {
    frame: OriginalLifetimeFrame<'a>,
    facts: &'a ServiceReviewFacts,
    authorization: Signed<CollectorServiceCredentialAuthorization>,
    capsules: Vec<cc_crypto_core::ServiceCredentialCapsule>,
    route: String,
    minimum_actor_revision: u64,
}

pub(in super::super) async fn prepare_and_seal_original<'a>(
    context: &'a Context, facts: &'a ServiceReviewFacts,
    base: &Signed<CollectorServiceDelegation>,
    configured: &VerifiedServiceConfiguration, recipient: &RecipientReference, route: &str,
) -> AppResult<OriginalLifetimeSet<'a>> {
    // This is NOT a consent constructor. Only the original confirmed operation
    // may reach it; no new AppCore public method or confirmation flag is added.
    let frame = OriginalLifetimeFrame::from_context(context)?;
    let minimum_actor_revision = configured.receipt.body.actor_revision.checked_add(2).ok_or_else(unavailable)?;
    frame.run(async {
        facts.live(context)?;
        recipient.checked(facts, base, context)?;
        if facts.credentials.is_empty() || facts.credentials.len() > 2 || base.body != facts.policy {
            return Err(unavailable());
        }
        let app = context.app()?;
        let (device, references) = context.scope.with_resources(&app, |_, u, identity| {
            let mut references = Vec::with_capacity(facts.credentials.len());
            for original in &facts.credentials {
                original.check_held(u)?;
                let endpoint = match &original.target {
                    CollectorCredentialTarget::Receiver {} => facts.catalog.manifest["receiver"]["url"].as_str(),
                    CollectorCredentialTarget::Scrape { target_id } => facts.catalog.manifest["targets"].as_array()
                        .and_then(|rows| rows.iter().find(|row| row["id"].as_str() == Some(target_id.as_str())))
                        .and_then(|row| row["url"].as_str()),
                }.ok_or_else(unavailable)?;
                references.push(CollectorServiceCredentialRef { target: original.target.clone(),
                    endpoint_sha256: Hash::from_bytes(sha2::Sha256::digest(endpoint.as_bytes()).into()),
                    username: original.metadata.username().ok_or_else(unavailable)?.to_owned(),
                    credential_id: ResourceId::new(&original.metadata.credential().to_string()).map_err(|_| unavailable())?,
                    credential_revision: original.revision.try_into().map_err(|_| unavailable())? });
            }
            Ok((ResourceId::new(&identity.device_id().to_string()).map_err(|_| unavailable())?, references))
        }).await?;
        drop(app);
        let now = context.scope.now()?;
        let body = CollectorServiceCredentialAuthorization {
            authorization_id: ResourceId::new(&Uuid::new_v4().to_string()).map_err(|_| unavailable())?,
            owner_device_id: device, base: base.clone(), admission: configured.claim.clone(),
            recipient: recipient.attestation.clone(), credentials: references,
            issued_at: now.0, expires_at: facts.policy.expires_at,
        };
        let prepared = PreparedCollectorServiceCredentialOwner::prepare(&facts.pin, &facts.certificate,
            base, &configured.claim, body, recipient.attestation.body.challenge,
            &facts.catalog.manifest, now).map_err(|_| unavailable())?;
        let authorization = {
            let app = context.app()?;
            context.scope.with_loaded(&app, |identity| {
                frame.check()?;
                facts.live(context)?;
                let signed = identity.sign_collector_service_credential_authorization_v1(prepared, context.scope.now()?)
                    .map_err(|_| unavailable())?;
                frame.check()?;
                facts.live(context)?;
                Ok(signed)
            }).await?
        };
        let mut capsules = Vec::with_capacity(facts.credentials.len());
        for (slot, original) in facts.credentials.iter().enumerate() {
            frame.check()?;
            original.check(context).await?;
            facts.live(context)?;
            let writer = {
                let app = context.app()?;
                context.scope.with_resources(&app, |_, u, _| {
                    original.check_held(u)?;
                    Ok(u.writer.clone())
                }).await?
            };
            // SAME original worker; an expired/cancelled wait grants nothing.
            let secret = writer.read_secret(original.metadata.secret()).await.map_err(|_| unavailable())?;
            drop(writer);
            frame.check()?;
            original.check(context).await?;
            facts.live(context)?;
            if secret.id != original.metadata.secret()
                || secret.kind != cc_models::secret::SecretKind::Password
                || !cc_crypto_core::collector_basic_password_supported(secret.value.expose_secret().as_bytes())
            { return Err(unavailable()); }
            let app = context.app()?;
            let capsule = context.scope.with_resources(&app, |_, u, identity| {
                frame.check()?;
                original.check_held(u)?;
                facts.live(context)?;
                let now = context.scope.now()?;
                let binding = cc_crypto_core::collector_service_credential_binding(&authorization,
                    &facts.pin, slot, now).map_err(|_| unavailable())?;
                let value = identity.seal_service_credential(binding,
                    Zeroizing::new(secret.value.expose_secret().as_bytes().to_vec()),
                    recipient.attestation.body.recipient_public_key.as_bytes(), now.0.seconds(),
                    facts.policy.expires_at.seconds()).map_err(|_| unavailable())?;
                frame.check()?;
                original.check_held(u)?;
                facts.live(context)?;
                Ok(value)
            }).await?;
            drop(secret);
            drop(app);
            capsules.push(capsule);
        }
        for original in &facts.credentials { original.check(context).await?; }
        facts.live(context)?;
        cc_crypto_core::preflight_collector_service_credential_set(&authorization, &capsules, &facts.pin,
            &facts.policy, &configured.claim.body.clock, &facts.catalog.manifest, context.scope.now()?)
            .map_err(|_| unavailable())?;
        frame.check()?;
        Ok((authorization, capsules))
    }).await.map(|(authorization, capsules)| OriginalLifetimeSet { frame, facts, authorization, capsules, route: route.to_owned(), minimum_actor_revision })
}

impl OriginalLifetimeSet<'_> {
    /// PROPOSED new protocol handler, NOT registered/available in v1 runtime.
    /// Caller must pass the SAME prepared original Peer, already pinned to facts.
    pub(in super::super) async fn deliver_and_publish(self, peer: &mut transport::Peer, ledger: &crate::agent_tunnel::association::ledger::Ledger, intent: cc_storage_core::NodeCollectorService) -> AppResult<cc_storage_core::NodeCollectorService> {
        let Self { frame, facts, authorization, capsules, route, minimum_actor_revision } = self;
        frame.run(async {
            facts.live(frame.context)?;
            for original in &facts.credentials { original.check(frame.context).await?; }
            peer.require_original_lifetime_service(&frame.context.scope,facts.host,facts.host_revision,&route,&facts.node)?;
            let original_start = authorization.body.recipient.body.original_start_nonce;
            let hashes:Vec<Hash>=capsules.iter().map(|c|c.to_json().map(|raw|Hash::from_bytes(sha2::Sha256::digest(raw).into())).map_err(|_|unavailable())).collect::<AppResult<_>>()?;
            let digest = authorization.body.digest().map_err(|_| unavailable())?;
            let sql_intent=cc_storage_core::NodeCollectorServiceCredentialSet {
                authorization_json:serde_json::to_vec(&authorization).map_err(|_|unavailable())?,
                authorization_sha256:hex::encode(digest.as_bytes()),
                ordered_capsule_sha256:hashes.iter().map(|h|hex::encode(h.as_bytes())).collect(),
                minimum_actor_revision,signed_admission_receipt_json:None,
            };
            let staged=ledger.begin_lifetime_set_original(&frame.context.scope,facts.association,intent,sql_intent).await?;
            frame.check()?;
            let retained_authorization=authorization.clone();

            let mut frames = vec![CollectorServiceCredentialFrame::authorization(authorization).map_err(|_| unavailable())?];
            for (slot, capsule) in capsules.into_iter().enumerate() {
                frames.push(CollectorServiceCredentialFrame::capsule(digest, slot as u8, capsule).map_err(|_| unavailable())?);
            }
            peer.cap_review(frame.until)?;
            for (sequence, payload) in frames.into_iter().enumerate() {
                frame.check()?;
                facts.live(frame.context)?;
                let payload = payload.to_json().map_err(|_| unavailable())?;
                let value: Value = serde_json::from_slice(&payload).map_err(|_| unavailable())?;
                peer.require_original_lifetime_service(&frame.context.scope,facts.host,facts.host_revision,&route,&facts.node)?;
                let ack: FrameAck = peer.exchange_weak(&frame.context.core, "stageCollectorServiceLifetimeFrame",
                    json!({"version":1,"sequence":sequence,"frame":value}), None).await?;
                frame.check()?;
                if ack.kind != "collectorServiceLifetimeFrameStaged" || ack.sequence != sequence
                    || ack.authorization_digest != digest { return Err(unavailable()); }
            }
            peer.require_original_lifetime_service(&frame.context.scope,facts.host,facts.host_revision,&route,&facts.node)?;
            #[cfg(any(test,all(feature="test-harness",any(target_os="linux",target_os="windows"))))]
            super::super::operation::lifetime_entry::paired::checkpoint(frame.context,super::super::operation::lifetime_entry::paired::Point::BeforeLifetimeAdmit).await?;
            let ack: Signed<cc_agent_node_wire::CollectorServiceCredentialSetReceipt> = peer.exchange_weak(&frame.context.core, "admitCollectorServiceLifetimeSet",
                json!({"version":1,"authorizationDigest":digest}), None).await?;
            frame.check()?;
            facts.live(frame.context)?;
            for original in &facts.credentials { original.check(frame.context).await?; }
            peer.require_original_lifetime_service(&frame.context.scope,facts.host,facts.host_revision,&route,&facts.node)?;
            cc_agent_node_wire::verify_statement(&ack,facts.node.public_key).map_err(|_|unavailable())?;
            let r=&ack.body;
            let now=frame.context.scope.now()?;
            cc_agent_node_wire::verify_collector_service_set_receipt(&facts.pin,&facts.certificate,
                &retained_authorization,&ack,&hashes,minimum_actor_revision,now).map_err(|_|unavailable())?;
            if r.header!=facts.policy.header || r.agent_id!=facts.node.agent_id || r.key_epoch!=facts.node.key_epoch
                || r.service_id!=facts.policy.service_id || r.base_delegation_hash!=facts.policy.digest().map_err(|_|unavailable())?
                || r.authorization_digest!=digest || r.ordered_capsule_hashes!=hashes || r.original_start_nonce!=original_start
                || r.admitted_revision<minimum_actor_revision || r.actor_revision!=r.admitted_revision
                || r.issued_at>now.0 || now.0>=r.expires_at || r.expires_at>facts.policy.expires_at {return Err(unavailable())}
            #[cfg(any(test,all(feature="test-harness",any(target_os="linux",target_os="windows"))))]
            super::super::operation::lifetime_entry::paired::checkpoint(frame.context,super::super::operation::lifetime_entry::paired::Point::AfterVerifiedReceipt).await?;
            let recorded=ledger.complete_lifetime_set_original(&frame.context.scope,facts.association,staged,
                serde_json::to_vec(&ack).map_err(|_|unavailable())?).await?;
            frame.check()?;
            facts.live(frame.context)?;
            #[cfg(any(test,all(feature="test-harness",any(target_os="linux",target_os="windows"))))]
            super::super::operation::lifetime_entry::paired::checkpoint(frame.context,super::super::operation::lifetime_entry::paired::Point::BeforePublish).await?;
            let _view=frame.publish(json!({"kind":"collectorServiceLifetimeSetAdmitted","authorizationDigest":digest,
                "actorRevision":r.actor_revision,"materialState":"notObserved","runtimeState":"notObserved"}))?;
            #[cfg(any(test,all(feature="test-harness",any(target_os="linux",target_os="windows"))))]
            super::super::operation::lifetime_entry::paired::published(frame.context)?;
            Ok(recorded)
        }).await
    }
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct FrameAck { kind: String, sequence: usize, authorization_digest: Hash }

pub(in super::super) async fn prepare_original_from_peer<'a>(context:&'a Context,facts:&'a ServiceReviewFacts,
    base:&Signed<CollectorServiceDelegation>,configured:&VerifiedServiceConfiguration,
    peer:&mut transport::Peer,route:&str)->AppResult<OriginalLifetimeSet<'a>> {
    peer.require_original_lifetime_service(&context.scope,facts.host,facts.host_revision,route,&facts.node)?;
    let reference:RecipientReference=bounded_original(&context.scope,
        peer.exchange_weak(&context.core,"collectorServiceRecipient",json!({}),None)).await?;
    reference.checked(facts,base,context)?;
    peer.require_original_lifetime_service(&context.scope,facts.host,facts.host_revision,route,&facts.node)?;
    prepare_and_seal_original(context,facts,base,configured,&reference,route).await
}
#[cfg(test)]
#[path="service_lifetime_tests.rs"]
mod tests;
