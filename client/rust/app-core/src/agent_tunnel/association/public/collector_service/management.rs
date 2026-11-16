//! Saved exact service metadata is a target for read/revoke, never a new grant.
use super::*;
use cc_storage_core::{NodeAssociationPhase, NodeAssociationRecord, NodeCollectorService};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct Request {
    pub(super) version: u8,
    pub(super) profile_id: String,
    pub(super) notification_scope_id: String,
    pub(super) association_id: String,
}
#[derive(Clone)]
pub(super) struct Saved {
    pub(super) row: NodeAssociationRecord,
    pub(super) service: NodeCollectorService,
    pub(super) delegation: Signed<CollectorServiceDelegation>,
    pub(super) catalog: protocol::MetricsCollector,
    pub(super) original_pin: OwnerPin,
    pub(super) certificate: Signed<Certificate>,
}
impl Saved {
    pub(super) async fn current_with_service(
        &self,
        context: &Context,
        service: &NodeCollectorService,
    ) -> AppResult<()> {
        let mut expected = self.row.clone();
        expected.collector_service = Some(service.clone());
        let ledger = ledger::Ledger::capture_original(context.scope.clone()).await?;
        let snapshot = ledger.snapshot_original().await?;
        if snapshot
            .records()
            .iter()
            .find(|r| r.public.association_id == expected.public.association_id)
            != Some(&expected)
        {
            return Err(unavailable());
        }
        context
            .validate_host(
                expected.public.saved_host_id,
                expected.public.saved_host_revision,
            )
            .await?;
        context.scope.check_current()
    }
    pub(super) async fn capture(context: &Context, id: Uuid) -> AppResult<Self> {
        let ledger = ledger::Ledger::capture_original(context.scope.clone()).await?;
        let rows = ledger.snapshot_original().await?;
        let row = rows
            .records()
            .iter()
            .find(|row| {
                row.public.association_id == id
                    && row.phase == NodeAssociationPhase::Associated
                    && row.revocation.is_none()
                    && row.rotation.as_ref().is_none_or(|r| r.selected_observed)
            })
            .ok_or_else(unavailable)?
            .clone();
        let service = row.collector_service.clone().ok_or_else(unavailable)?;
        let key = context.scope.with_current_resources(|_, _, id| {
            Ok(PublicKey::from_bytes(id.public_keys().signing))
        })?;
        let certificate = ledger::current_certificate(&row, key)?;
        let delegation: Signed<CollectorServiceDelegation> =
            serde_json::from_slice(&service.signed_delegation_json).map_err(|_| unavailable())?;
        verify_statement(&delegation, key).map_err(|_| unavailable())?;
        let original_pin: OwnerPin =
            serde_json::from_slice(&service.owner_pin_json).map_err(|_| unavailable())?;
        context.scope.bind_owner_pin(&original_pin)?;
        let p = &delegation.body;
        let catalog = protocol::MetricsCollector {
            collector_id: p.collector_id,
            manifest_sha256: p.manifest_sha256,
            manifest: serde_json::from_slice(&service.manifest_json).map_err(|_| unavailable())?,
        };
        catalog.check(row.public.saved_host_id)?;
        if p.service_id != ResourceId::new(&service.service_id.to_string()).map_err(|_| unavailable())?
            || hex::encode(p.digest().map_err(|_| unavailable())?.as_bytes()) != service.delegation_sha256
            || original_pin.key != key
            || p.header.context != original_pin.context
            || p.header.owner_epoch != original_pin.epoch
            || p.header.checkpoint != original_pin.checkpoint
            || p.agent_id != certificate.body.agent_id
            || p.recipient != certificate.body.agent_key
            || p.key_epoch != certificate.body.key_epoch
            || p.certificate_hash != certificate.body.digest().map_err(|_| unavailable())?
            || certificate.body.scopes.0.as_slice() != [Scope::MetricsCollect]
            || certificate.body.capabilities.scopes != certificate.body.scopes
            || catalog.manifest["profile"] != "maintained-prometheus-disk-512mib-2h-v1"
            || catalog.manifest["bufferMode"] != "persistentNativeWal"
        {
            return Err(unavailable());
        }
        context
            .validate_host(row.public.saved_host_id, row.public.saved_host_revision)
            .await?;
        Ok(Self {
            row,
            service,
            delegation,
            catalog,
            original_pin,
            certificate,
        })
    }
    pub(super) async fn current(&self, context: &Context) -> AppResult<()> {
        context.scope.check_current()?;
        let ledger = ledger::Ledger::capture_original(context.scope.clone()).await?;
        let rows = ledger.snapshot_original().await?;
        if rows
            .records()
            .iter()
            .find(|r| r.public.association_id == self.row.public.association_id)
            != Some(&self.row)
        {
            return Err(unavailable());
        }
        context
            .validate_host(
                self.row.public.saved_host_id,
                self.row.public.saved_host_revision,
            )
            .await?;
        context.scope.check_current()
    }
    pub(super) async fn probe(
        &self,
        context: &Context,
        control: transport::Observation,
    ) -> AppResult<(transport::Peer, protocol::Probe)> {
        self.current(context).await?;
        let mut peer = transport::Peer::open_observed_weak(
            &context.core,
            context.scope.clone(),
            self.row.public.saved_host_id,
            self.row.public.saved_host_revision,
            control,
        )
        .await?;
        peer.cap_review(context.scope.deadline())?;
        if peer.route_digest != self.row.public.route_trust_digest {
            return Err(unavailable());
        }
        let response: protocol::MetricsProbe = peer
            .exchange_weak(
                &context.core,
                "probeCollectorServiceStatus",
                json!({}),
                None,
            )
            .await?;
        let actual = response.checked_revocation(&context.scope)?;
        let mut expected = self.original_pin.clone();
        expected.checkpoint = actual.owner_pin.checkpoint;
        if actual.owner_pin != expected
            || expected.checkpoint < self.original_pin.checkpoint
            || actual.actor_revision < self.service.expected_actor_revision
            || !matches!((actual.node.format, actual.node.key_epoch), (1, 1) | (3, 2))
            || actual.node.agent_id != self.delegation.body.agent_id
            || actual.node.key_epoch != self.delegation.body.key_epoch
            || actual.node.public_key != self.delegation.body.recipient
            || actual.node.key_fingerprint != actual.node.public_key.fingerprint()
            || actual.certificate.as_ref() != Some(&self.certificate.body)
        {
            return Err(unavailable());
        }
        self.current(context).await?;
        Ok((peer, actual))
    }
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Observed {
    pub(super) receipt: Signed<CollectorServiceReceipt>,
    pub(super) claim: Signed<CollectorServiceClaim>,
    pub(super) delegation: Signed<CollectorServiceDelegation>,
}
// Preserve exact saved owner-signed envelope identity, including signature.
fn same_saved_delegation(a: &Signed<CollectorServiceDelegation>, b: &Signed<CollectorServiceDelegation>) -> bool {
    a.body == b.body && a.signature == b.signature
}
pub(super) fn verify_observed(
    saved: &Saved,
    probe: &protocol::Probe,
    observed: &Observed,
    context: &Context,
) -> AppResult<()> {
    verify_statement(&observed.receipt, probe.node.public_key).map_err(|_| unavailable())?;
    verify_statement(&observed.claim, probe.node.public_key).map_err(|_| unavailable())?;
    let p = &saved.delegation.body;
    let r = &observed.receipt.body;
    let claim = &observed.claim.body;
    if !same_saved_delegation(&observed.delegation, &saved.delegation)
        || r.header.context != p.header.context
        || r.header.owner_epoch != p.header.owner_epoch
        || r.header.checkpoint != probe.owner_pin.checkpoint
        || r.service_id != p.service_id
        || r.delegation_hash != p.digest().map_err(|_| unavailable())?
        || r.policy_revision != p.policy_revision
        || r.revocation_epoch < p.revocation_epoch
        || r.agent_id != p.agent_id
        || r.key_epoch != p.key_epoch
        || r.actor_revision < probe.actor_revision
        || r.actor_revision < saved.service.expected_actor_revision
        || claim.header != p.header
        || claim.agent_id != p.agent_id
        || claim.key_epoch != p.key_epoch
        || claim.delegation_hash != r.delegation_hash
        || claim.issued_at < p.issued_at
        || claim.issued_at >= p.expires_at
        || r.effect == CollectorServiceEffect::Configured
            && r.revocation_epoch != p.revocation_epoch
        || r.effect == CollectorServiceEffect::Revoked && r.revocation_epoch <= p.revocation_epoch
    {
        return Err(unavailable());
    }
    cc_agent_node_wire::check_recent(r.issued_at, context.scope.now()?).map_err(|_| unavailable())
}
impl AppCore {
    /// Explicit fresh same-Node observation. It never repairs/adopts Unknown,
    /// writes local configuration, renews a grant, or claims actual running.
    pub async fn agent_collector_service_status_json(self, raw: &str) -> AppResult<String> {
        let request: Request = parse(raw)?;
        let id = Uuid::parse_str(&request.association_id).map_err(|_| unavailable())?;
        if id.is_nil() || id.to_string() != request.association_id {
            return Err(unavailable());
        }
        let context = Context::capture(
            &self,
            Frame {
                version: request.version,
                profile_id: request.profile_id,
                notification_scope_id: request.notification_scope_id,
            },
        )
        .await?;
        drop(self);
        let control = transport::Observation::new();
        let _permit = control.retain_permit(review_permit()?)?;
        bounded_original(&context.scope,async {
            let saved=Saved::capture(&context,id).await?;
            let(mut peer,probe)=saved.probe(&context,control.clone()).await?;
            let p=&saved.delegation.body;
            let response:Observed=peer.exchange_weak(&context.core,"observeCollectorService",
                json!({"expectedRev":probe.actor_revision,"expectedPin":probe.owner_pin,
                    "serviceId":p.service_id,"delegationHash":p.digest().map_err(|_|unavailable())?}),None).await?;
            verify_observed(&saved,&probe,&response,&context)?;saved.current(&context).await?;
            drop(peer);let _=control.wait_closed(Instant::now()+Duration::from_secs(3)).await;
            saved.current(&context).await?;
            context.publish(json!({"kind":"finiteCollectorServiceStatus","associationId":id,
                "hostId":saved.row.public.saved_host_id,"hostRevision":saved.row.public.saved_host_revision.to_string(),
                "node":data::node(&probe.node),"serviceId":p.service_id,"serviceHash":response.receipt.body.delegation_hash,
                "collectorId":saved.catalog.collector_id,"manifestSha256":saved.catalog.manifest_sha256,
                "policyRevision":p.policy_revision.to_string(),"revocationEpoch":response.receipt.body.revocation_epoch.to_string(),
                "actorRevision":response.receipt.body.actor_revision.to_string(),
                "localConfigurationRecorded":saved.service.configuration_recorded,"remoteConfigurationRecorded":true,
                "remoteRevoked":response.receipt.body.effect==CollectorServiceEffect::Revoked,
                "expiresAtUtc":data::utc(p.expires_at)?,"bootStamp":{"bootId":response.claim.body.clock.boot_id,
                    "createdBoottimeNs":response.claim.body.clock.created_boottime_ns.to_string(),
                    "deadlineBoottimeNs":response.claim.body.clock.deadline_boottime_ns.to_string()},
                "runtimeState":"notObserved","deliveryGuaranteed":false,"lossUnknown":true,
                "cleanupState":control.cleanup_state()}),16384)
        }).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn status_rejects_replacement_signed_envelope_even_when_body_matches() {
        use cc_agent_node_wire::{AgentId, Context as WireContext, DeploymentId, Hash, Header,
            LocalProfileId, PublicKey, ResourceId, SignatureBytes, Tenant, Timestamp,
            CollectorServiceProfile, CollectorServiceQuota, collector_service_resource};
        let manifest = Hash::from_bytes([9;32]);
        let body = CollectorServiceDelegation {
            header: Header {version:1,context:WireContext {
                tenant:Tenant::LocalProfile {id:LocalProfileId::new("11111111-1111-4111-8111-111111111111").unwrap()},
                deployment_id:DeploymentId::new("22222222-2222-4222-8222-222222222222").unwrap()},owner_epoch:1,checkpoint:2},
            service_id:ResourceId::new("33333333-3333-4333-8333-333333333333").unwrap(),
            policy_revision:1,revocation_epoch:1,agent_id:AgentId::new("44444444-4444-4444-8444-444444444444").unwrap(),
            key_epoch:1,recipient:PublicKey::from_bytes([7;32]),certificate_hash:Hash::from_bytes([8;32]),
            collector_id:collector_service_resource(manifest).unwrap(),manifest_sha256:manifest,
            profile:CollectorServiceProfile::MaintainedPrometheusDisk512mib2hV1,quota:CollectorServiceQuota::fixed(),
            credential_targets:Vec::new(),issued_at:Timestamp::new(1000).unwrap(),expires_at:Timestamp::new(2000).unwrap(),
        };
        // This exercises saved-envelope identity, not signature validity. The
        // real Node receipt/claim verification remains before this guard.
        let saved = Signed {body,signature:SignatureBytes::from_bytes([1;64])};
        let exact:Signed<CollectorServiceDelegation> = serde_json::from_slice(&serde_json::to_vec(&saved).unwrap()).unwrap();
        assert!(same_saved_delegation(&exact,&saved));
        let mut replacement = exact.clone();replacement.signature=SignatureBytes::from_bytes([2;64]);
        assert_eq!(replacement.body,saved.body);
        assert!(!same_saved_delegation(&replacement,&saved));
        replacement=exact;replacement.body.policy_revision+=1;
        assert_eq!(replacement.signature,saved.signature);
        assert!(!same_saved_delegation(&replacement,&saved));
    }
}
