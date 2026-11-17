//! One original reviewed service configuration, never an interactive lease.
use super::*;
use cc_agent_node_wire::{
    CollectorServiceOwnerSigned, CollectorServiceProfile, CollectorServiceQuota,
};
use cc_storage_core::{NodeAssociationPhase, NodeAssociationRecord, NodeCollectorService};
use std::sync::Mutex;
use tokio::{
    sync::{oneshot, watch},
    task::JoinHandle,
};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Request {
    version: u8,
    profile_id: String,
    notification_scope_id: String,
    association_id: String,
    collector_id: String,
    manifest_sha256: String,
    #[serde(default)]
    credentials: Vec<credentials::Request>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct CatalogRequest {
    version: u8,
    profile_id: String,
    notification_scope_id: String,
    association_id: String,
}
impl Request {
    fn association(&self) -> AppResult<Uuid> {
        let id = Uuid::parse_str(&self.association_id).map_err(|_| unavailable())?;
        if id.is_nil() || id.to_string() != self.association_id {
            return Err(unavailable());
        }
        Ok(id)
    }
}
#[derive(Clone,Copy,PartialEq,Eq)]
enum CredentialProtocol { SingleStart, ServiceLifetime }
struct Prepared {
    review_identity: PreparedReviewIdentity,
    credential_protocol: CredentialProtocol,
    peer: Option<transport::Peer>,
    ledger: ledger::Ledger,
    row: NodeAssociationRecord,
    route: String,
    _permit: Arc<tokio::sync::OwnedSemaphorePermit>,
}
impl Prepared {
    async fn capture(context:&Context,request:&Request,control:transport::Observation,require_catalog_refs:bool)->AppResult<(Self,ServiceReviewFacts)>{
        Self::capture_for_protocol(context,request,control,require_catalog_refs,CredentialProtocol::SingleStart).await
    }
    // Private seam for the separate explicit lifetime consent entry ONLY.
    async fn capture_lifetime(context:&Context,request:&Request,control:transport::Observation)->AppResult<(Self,ServiceReviewFacts)>{
        Self::capture_for_protocol(context,request,control,true,CredentialProtocol::ServiceLifetime).await
    }
    async fn capture_for_protocol(
        context: &Context,
        request: &Request,
        control: transport::Observation,
        require_catalog_refs: bool,
        credential_protocol: CredentialProtocol,
    ) -> AppResult<(Self, ServiceReviewFacts)> {
        let permit = control.retain_permit(review_permit()?)?;
        let credential_refs = credentials::capture(context, &request.credentials).await?;
        let mut ledger = ledger::Ledger::capture_original(context.scope.clone()).await?;
        ledger.cap_review(context.scope.deadline())?;
        let rows = ledger.snapshot_original().await?;
        let id = request.association()?;
        let row = rows
            .records()
            .iter()
            .find(|r| {
                r.public.association_id == id
                    && r.phase == NodeAssociationPhase::Associated
                    && r.revocation.is_none()
                    && r.rotation.as_ref().is_none_or(|r| r.selected_observed)
                    && r.collector_service.is_none()
            })
            .ok_or_else(unavailable)?
            .clone();
        let key = context.scope.with_current_resources(|_, _, identity| {
            Ok(PublicKey::from_bytes(identity.public_keys().signing))
        })?;
        let certificate = ledger::current_certificate(&row, key)?;
        let trusted = ledger::current_pin(&row, key)?;
        if certificate.body.scopes.0.as_slice() != [Scope::MetricsCollect]
            || certificate.body.capabilities.scopes != certificate.body.scopes
        {
            return Err(AppError::invalid(
                "agent_collector_service",
                "metrics_enrollment_required",
            ));
        }
        context.scope.bind_owner_pin(&trusted)?;
        context
            .validate_host(row.public.saved_host_id, row.public.saved_host_revision)
            .await?;
        let mut peer = match credential_protocol {
            CredentialProtocol::SingleStart => transport::Peer::open_service_observed_weak(&context.core,context.scope.clone(),row.public.saved_host_id,row.public.saved_host_revision,control).await?,
            CredentialProtocol::ServiceLifetime => transport::Peer::open_lifetime_service_observed_weak(&context.core,context.scope.clone(),row.public.saved_host_id,row.public.saved_host_revision,control).await?,
        };
        peer.cap_review(context.scope.deadline())?;
        if peer.route_digest != row.public.route_trust_digest {
            return Err(unavailable());
        }
        let observed: protocol::MetricsProbe = peer
            .exchange_weak(&context.core, match credential_protocol {CredentialProtocol::SingleStart=>"probeCollectorService",CredentialProtocol::ServiceLifetime=>"probeCollectorServiceLifetime"}, json!({}), None)
            .await?;
        let (probe, catalog) = observed.checked(&context.scope, row.public.saved_host_id, false)?;
        let mut pin = trusted.clone();
        pin.checkpoint = probe.owner_pin.checkpoint;
        let minimum = row
            .rotation
            .as_ref()
            .and_then(|r| r.observed_actor_revision)
            .unwrap_or(
                row.public
                    .certificate
                    .as_ref()
                    .ok_or_else(unavailable)?
                    .expected_actor_revision,
            );
        if probe.revoked
            || !matches!((probe.node.format, probe.node.key_epoch), (1, 1) | (3, 2))
            || probe.owner_pin != pin
            || pin.checkpoint < trusted.checkpoint
            || probe.actor_revision < minimum
            || probe.certificate.as_ref() != Some(&certificate.body)
            || probe.node.agent_id != certificate.body.agent_id
            || probe.node.key_epoch != certificate.body.key_epoch
            || probe.node.public_key != certificate.body.agent_key
            || probe.node.key_fingerprint != probe.node.public_key.fingerprint()
            || require_catalog_refs
                && (catalog.collector_id != ResourceId::new(&request.collector_id).map_err(|_| unavailable())?
                    || hex::encode(catalog.manifest_sha256.as_bytes()) != request.manifest_sha256)
        {
            return Err(unavailable());
        }
        peer.bind_checked_service_node(&context.scope,&probe.node)?;
        let now = context.scope.now()?;
        let expires = now
            .0
            .checked_add(cc_agent_node_wire::COLLECTOR_SERVICE_TTL)
            .map_err(|_| unavailable())?
            .min(certificate.body.expires_at)
            .min(pin.expires_at);
        let policy = CollectorServiceDelegation {
            header: Header {
                version: 1,
                context: pin.context.clone(),
                owner_epoch: pin.epoch,
                checkpoint: pin.checkpoint,
            },
            service_id: ResourceId::new(&Uuid::new_v4().to_string()).map_err(|_| unavailable())?,
            policy_revision: 1,
            revocation_epoch: 1,
            agent_id: probe.node.agent_id,
            key_epoch: probe.node.key_epoch,
            recipient: probe.node.public_key,
            certificate_hash: certificate.body.digest().map_err(|_| unavailable())?,
            collector_id: catalog.collector_id,
            manifest_sha256: catalog.manifest_sha256,
            profile: CollectorServiceProfile::MaintainedPrometheusDisk512mib2hV1,
            quota: CollectorServiceQuota::fixed(),
            credential_targets: credential_refs
                .iter()
                .map(|item| item.target.clone())
                .collect(),
            issued_at: now.0,
            expires_at: expires,
        };
        let facts = ServiceReviewFacts {
            association: id,
            host: row.public.saved_host_id,
            host_revision: row.public.saved_host_revision,
            node: probe.node,
            pin,
            certificate,
            catalog,
            policy,
            actor_revision: probe.actor_revision,
            credentials: credential_refs,
        };
        facts.live(context)?;
        let review_identity=PreparedReviewIdentity::capture_checked(context,&facts,&peer.route_digest,credential_protocol)?;
        Ok((
            Self {
                review_identity,
                credential_protocol,
                route: peer.route_digest.clone(),
                peer: Some(peer),
                ledger,
                row,
                _permit: permit,
            },
            facts,
        ))
    }
    async fn revalidate(&self, context: &Context, facts: &ServiceReviewFacts) -> AppResult<()> {
        facts.live(context)?;
        let rows = self.ledger.snapshot_original().await?;
        if rows
            .records()
            .iter()
            .find(|r| r.public.association_id == facts.association)
            != Some(&self.row)
            || self.row.public.route_trust_digest != self.route
        {
            return Err(unavailable());
        }
        context
            .validate_host(facts.host, facts.host_revision)
            .await?;
        facts.live(context)
    }
}
struct State {
    ready: Option<Prepared>,
    phase: ServiceViewPhase,
    ack: Option<Arc<VerifiedServiceConfiguration>>,
    task: Option<JoinHandle<()>>,
    busy: bool,
    consumed: bool,
    mutation_attempted: bool,
    joined: bool,
    orphaned: bool,
}
struct Shared {
    confirmation: PreparedReviewIdentity,
    context: Context,
    facts: ServiceReviewFacts,
    control: transport::Observation,
    state: Mutex<State>,
    done: watch::Sender<bool>,
}
pub struct AgentCollectorServiceReview {
    shared: Arc<Shared>,
}
impl std::fmt::Debug for AgentCollectorServiceReview {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("AgentCollectorServiceReview([redacted])")
    }
}
impl Drop for AgentCollectorServiceReview {
    fn drop(&mut self) {
        self.cancel();
    }
}
impl Shared {
    fn cancel(&self) {
        self.context.cancel.cancel();
        self.control.abort();
        if let Ok(mut s) = self.state.lock() {
            if !s.busy && !s.consumed {
                s.ready.take();
                s.phase = ServiceViewPhase::Cancelled;
            }
        }
    }
    async fn publish(&self) -> AppResult<String> {
        let (phase, ack) = {
            let s = self.state.lock().map_err(|_| unknown())?;
            (s.phase, s.ack.clone())
        };
        let raw=view::publish(
            &self.context,&self.facts,phase,ack.as_deref(),&self.control,
        ).await?;
        if self.confirmation.protocol()==CredentialProtocol::SingleStart {return Ok(raw)}
        self.confirmation.publish_lifetime(&self.context,&self.facts,raw)
    }
    fn finish(&self, ack: Option<VerifiedServiceConfiguration>) {
        if let Ok(mut s) = self.state.lock() {
            s.ready.take();
            s.busy = false;
            s.phase = if ack.is_some() {
                ServiceViewPhase::ConfigurationRecorded
            } else {
                ServiceViewPhase::Unknown
            };
            s.ack = ack.map(Arc::new);
        }
        self.done.send_replace(true);
    }
}
struct WorkGuard {
    shared: Arc<Shared>,
    finished: bool,
}
impl Drop for WorkGuard {
    fn drop(&mut self) {
        if !self.finished {
            self.shared.control.abort();
            if let Ok(mut s) = self.shared.state.lock() {
                s.orphaned = true;
            }
            self.shared.finish(None);
        }
    }
}
impl AgentCollectorServiceReview {
    pub async fn details_json(&self) -> AppResult<String> {
        bounded_original(&self.shared.context.scope, self.shared.publish()).await
    }
    /// Exactly one explicit approval. Errors consume this original review;
    /// cancellation/closing the dialog never revokes an admitted service.
    pub async fn confirm_configuration(&self) -> AppResult<String> {
        let original = &self.shared.context;
        bounded_original(&original.scope, original.check()).await?;
        let ready = {
            let mut s = self.shared.state.lock().map_err(|_| unknown())?;
            if s.busy || s.consumed {
                return Err(unavailable());
            }
            self.shared.facts.live(original)?;
            self.shared.confirmation.require_ready(original,&self.shared.facts,
                s.ready.as_ref().ok_or_else(unavailable)?)?;
            let ready = s.ready.take().ok_or_else(unavailable)?;
            s.busy = true;
            s.consumed = true;
            s.phase = ServiceViewPhase::Configuring;
            self.shared.done.send_replace(false);
            ready
        };
        let shared = self.shared.clone();
        let (tx, rx) = oneshot::channel();
        let handle = original.runtime.spawn(async move {
            let mut guard = WorkGuard {
                shared: shared.clone(),
                finished: false,
            };
            configure(shared, ready, tx).await;
            guard.finished = true;
        });
        self.shared.state.lock().map_err(|_| unknown())?.task = Some(handle);
        rx.await.map_err(|_| unknown())?
    }
    pub fn cancel(&self) {
        self.shared.cancel();
    }
    /// Reports ONLY original caller worker/carrier retirement, never service
    /// stop, runtime readiness, remote delivery, or release of unknown units.
    pub async fn joined_json(&self) -> AppResult<String> {
        let end = Instant::now() + Duration::from_secs(5);
        let mut done = self.shared.done.subscribe();
        let observed = tokio::time::timeout_at(tokio::time::Instant::from_std(end), async {
            while !*done.borrow() {
                done.changed().await.map_err(|_| unknown())?;
            }
            self.shared.control.wait_closed(end).await
        })
        .await;
        let handle = if matches!(observed, Ok(Ok(()))) {
            self.shared.state.lock().map_err(|_| unknown())?.task.take()
        } else {
            None
        };
        if let Some(mut task) = handle {
            match tokio::time::timeout_at(tokio::time::Instant::from_std(end), &mut task).await {
                Ok(Ok(())) => self.shared.state.lock().map_err(|_| unknown())?.joined = true,
                Err(_) => self.shared.state.lock().map_err(|_| unknown())?.task = Some(task),
                Ok(Err(_)) => self.shared.state.lock().map_err(|_| unknown())?.orphaned = true,
            }
        }
        let s = self.shared.state.lock().map_err(|_| unknown())?;
        let joined =
            matches!(observed, Ok(Ok(()))) && !s.busy && !s.orphaned && (s.joined || !s.consumed);
        serde_json::to_string(&json!({"version":1,"phase":s.phase.label(),
            "cleanupState":if joined{"closedObserved"}else{"unknown"},
            "serviceMayRemainActive":s.mutation_attempted,"runtimeState":"notObserved",
            "deliveryGuaranteed":false,"lossUnknown":true}))
        .map_err(|_| unknown())
    }
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Configured {
    claim: Signed<CollectorServiceClaim>,
    receipt: Signed<CollectorServiceReceipt>,
}
async fn configure(
    shared: Arc<Shared>,
    mut ready: Prepared,
    tx: oneshot::Sender<AppResult<String>>,
) {
    let work = async {
        let context = &shared.context;
        let facts = &shared.facts;
        shared.confirmation.require_ready(context,facts,&ready)?;
        ready.revalidate(context, facts).await?;
        let prepared = PreparedCollectorServiceOwner::delegation(
            &facts.pin,
            &facts.certificate,
            facts.policy.clone(),
            context.scope.now()?,
        )
        .map_err(|_| unavailable())?;
        let signed = {
            let app = context.app()?;
            under_review(
                context.scope.deadline(),
                context.scope.with_loaded(&app, |identity| {
                    consent_live(context.scope.deadline())?;
                    facts.live(context)?;
                    let signed = identity
                        .sign_collector_service_owner_v1(prepared, context.scope.now()?)
                        .map_err(|_| unknown())?;
                    consent_live(context.scope.deadline())?;
                    facts.live(context)?;
                    Ok(signed)
                }),
            )
            .await?
        };
        let CollectorServiceOwnerSigned::Delegation(delegation) = signed else {
            return Err(unknown());
        };
        ready.revalidate(context, facts).await?;
        let mut intent = NodeCollectorService {
            credential_set: None,
            service_id: serde_json::from_value(json!(facts.policy.service_id))
                .map_err(|_| unknown())?,
            delegation_sha256: hex::encode(facts.policy.digest().map_err(|_| unknown())?.as_bytes()),
            signed_delegation_json: serde_json::to_vec(&delegation).map_err(|_| unknown())?,
            manifest_json: serde_json::to_vec(&facts.catalog.manifest).map_err(|_| unknown())?,
            owner_pin_json: serde_json::to_vec(&facts.pin).map_err(|_| unknown())?,
            expected_actor_revision: facts.actor_revision.checked_add(1).ok_or_else(unknown)?,
            signed_claim_json: None,
            signed_receipt_json: None,
            configuration_recorded: false,
            revocation: None,
        };
        // Same original SQL owner/CAS has completed BEFORE any mutation byte.
        ready
            .ledger
            .begin_service_original(ready.row.clone(), intent.clone())
            .await?;
        shared
            .state
            .lock()
            .map_err(|_| unknown())?
            .mutation_attempted = true;
        facts.live(context)?;
        let response: Configured = ready
            .peer
            .as_mut()
            .ok_or_else(unknown)?
            .exchange_weak(
                &context.core,
                "configureCollectorService",
                json!({"expectedRev":facts.actor_revision,
                "expectedPin":facts.pin,"delegation":delegation}),
                Some(facts.policy.expires_at),
            )
            .await?;
        let ack = VerifiedServiceConfiguration::checked(
            facts,
            context,
            response.receipt,
            response.claim,
        )?;
        match shared.confirmation.protocol() {
            CredentialProtocol::SingleStart => credentials::deliver(context,facts,ready.peer.as_mut().ok_or_else(unknown)?,&delegation).await?,
            CredentialProtocol::ServiceLifetime => {
                let set=credentials::prepare_original_from_peer(context,facts,&delegation,&ack,ready.peer.as_mut().ok_or_else(unknown)?,&ready.route).await?;
                intent=set.deliver_and_publish(ready.peer.as_mut().ok_or_else(unknown)?,&ready.ledger,intent).await?;
            }
        }
        ready
            .ledger
            .complete_service_original(
                facts.association,
                intent,
                serde_json::to_vec(&ack.claim).map_err(|_| unknown())?,
                serde_json::to_vec(&ack.receipt).map_err(|_| unknown())?,
            )
            .await?;
        facts.live(context)?;
        Ok::<_, AppError>(ack)
    };
    let result = bounded_original(&shared.context.scope, work).await;
    #[cfg(any(test, all(feature = "test-harness", any(target_os = "linux", target_os = "windows"))))]
    if std::env::var_os("CC_LIFETIME_PAIR_PROOF").as_deref() == Some(std::ffi::OsStr::new("1")) {
        if let Err(error) = &result {
            let _ = std::io::Write::write_fmt(&mut std::io::stderr().lock(), format_args!(
                "CC_LIFETIME_NATIVE_FAILURE stage=configure_work code={}\n", error.code()));
        }
    }
    drop(ready); // Original captured-runtime Peer cleanup, SAME quota lease.
    shared.finish(result.ok());
    let response = shared.publish().await;
    let _ = tx.send(response);
}
impl AppCore {
    /// Explicit read-only catalog observation for a reopened saved association.
    /// It returns refs only; no signing/configuration or implicit new review.
    pub async fn agent_collector_service_catalog_json(self, raw: &str) -> AppResult<String> {
        let request: CatalogRequest = parse(raw)?;
        let request = Request {
            version: request.version,
            profile_id: request.profile_id,
            notification_scope_id: request.notification_scope_id,
            association_id: request.association_id,
            collector_id: String::new(),
            manifest_sha256: String::new(),
            credentials: Vec::new(),
        };
        request.association()?;
        let context = Context::capture(
            &self,
            Frame {
                version: request.version,
                profile_id: request.profile_id.clone(),
                notification_scope_id: request.notification_scope_id.clone(),
            },
        )
        .await?;
        drop(self);
        let control = transport::Observation::new();
        bounded_original(&context.scope, async {
            let (ready, facts) =
                Prepared::capture(&context, &request, control.clone(), false).await?;
            facts.live(&context)?;
            drop(ready);
            let _ = control
                .wait_closed(Instant::now() + Duration::from_secs(3))
                .await;
            facts.live(&context)?;
            context.publish(
                json!({"kind":"finiteCollectorServiceCatalog","associationId":facts.association,
                "hostId":facts.host,"hostRevision":facts.host_revision.to_string(),
                "node":data::node(&facts.node),"collectorId":facts.catalog.collector_id,
                "manifestSha256":facts.catalog.manifest_sha256,"manifest":facts.catalog.manifest,
                "cleanupState":control.cleanup_state()}),
                16384,
            )
        })
        .await
    }
    pub async fn agent_collector_service_prepare_json(
        self,
        raw: &str,
    ) -> AppResult<AgentCollectorServiceReview> {
        let request: Request = parse(raw)?;
        request.association()?;
        let context = Context::capture(
            &self,
            Frame {
                version: request.version,
                profile_id: request.profile_id.clone(),
                notification_scope_id: request.notification_scope_id.clone(),
            },
        )
        .await?;
        drop(self);
        let control = transport::Observation::new();
        let (ready, facts) = bounded_original(
            &context.scope,
            Prepared::capture(&context, &request, control.clone(), true),
        )
        .await?;
        let (done, _) = watch::channel(true);
        let shared = Arc::new(Shared {
            confirmation: ready.review_identity.clone(),
            context,
            facts,
            control,
            state: Mutex::new(State {
                ready: Some(ready),
                phase: ServiceViewPhase::Review,
                ack: None,
                task: None,
                busy: false,
                consumed: false,
                mutation_attempted: false,
                joined: false,
                orphaned: false,
            }),
            done,
        });
        let weak = Arc::downgrade(&shared);
        let scope = shared.context.scope.clone();
        shared.context.runtime.spawn(async move {
            tokio::select!{_=scope.cancelled()=>{},_=tokio::time::sleep_until(tokio::time::Instant::from_std(scope.deadline()))=>{}}
            if let Some(shared)=weak.upgrade(){shared.cancel();}
        });
        shared.publish().await?;
        Ok(AgentCollectorServiceReview { shared })
    }
}

#[path="operation/lifetime_entry.rs"]
pub(super) mod lifetime_entry;
use lifetime_entry::PreparedReviewIdentity;
