//! Private explicit initial enrollment, using original pinned SSH only.
//! No Bridge/UI activation; review types do not themselves prove human consent.
#[cfg(test)]
mod golden_tests;
mod ledger;
pub(super) mod access_audit;
mod protocol;
#[cfg(test)]
mod tests;
mod transport;
use super::{
    managed_tls::ManagedTlsCustody, scope::OriginalSessionDeviceIdentity as OriginalScope,
};
use crate::{AppCore, AppError, AppResult};
use cc_agent_node_wire::*;
use cc_models::ObjectId;
use cc_storage_core::{NodeAssociationCertificate, NodeAssociationPublic, TunnelTlsPublic};
use sha2::{Digest, Sha256};
use std::{
    sync::{Arc, OnceLock},
    time::{Duration, Instant},
};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;
use zeroize::Zeroizing;
fn review_permit() -> AppResult<tokio::sync::OwnedSemaphorePermit> {
    static JOBS: OnceLock<Arc<tokio::sync::Semaphore>> = OnceLock::new();
    JOBS.get_or_init(|| Arc::new(tokio::sync::Semaphore::new(2)))
        .clone()
        .try_acquire_owned()
        .map_err(|_| unavailable())
}
#[cfg(any(test, all(feature = "test-harness", any(target_os = "linux", target_os = "windows"))))]
#[track_caller]
fn paired_failure_location(kind: &'static str) {
    if std::env::var_os("CC_LIFETIME_PAIR_PROOF").as_deref() == Some(std::ffi::OsStr::new("1")) {
        let at = std::panic::Location::caller();
        let _ = std::io::Write::write_fmt(&mut std::io::stderr().lock(), format_args!(
            "CC_LIFETIME_NATIVE_FAILURE kind={} file={} line={}\n", kind, at.file(), at.line()));
    }
}
#[cfg_attr(any(test, all(feature = "test-harness", any(target_os = "linux", target_os = "windows"))), track_caller)]
fn unavailable() -> AppError {
    #[cfg(any(test, all(feature = "test-harness", any(target_os = "linux", target_os = "windows"))))]
    paired_failure_location("unavailable");
    AppError::invalid("agent_node_association", "bootstrap_unenrolled")
}
#[cfg_attr(any(test, all(feature = "test-harness", any(target_os = "linux", target_os = "windows"))), track_caller)]
fn unknown() -> AppError {
    #[cfg(any(test, all(feature = "test-harness", any(target_os = "linux", target_os = "windows"))))]
    paired_failure_location("unknown");
    AppError::invalid("agent_node_association", "enrollment_unknown")
}
fn hash(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}
fn json<T: serde::Serialize>(v: &T) -> AppResult<serde_json::Value> {
    serde_json::to_value(v).map_err(|_| unknown())
}
fn bytes<T: serde::Serialize>(v: &T) -> AppResult<Vec<u8>> {
    serde_json::to_vec(v).map_err(|_| unknown())
}
fn agent_text(agent: AgentId) -> AppResult<String> {
    json(&agent)?
        .as_str()
        .map(str::to_owned)
        .ok_or_else(unknown)
}
fn client_expiry(public: &TunnelTlsPublic) -> AppResult<Timestamp> {
    let (rest, cert) =
        x509_parser::parse_x509_certificate(&public.certificate_der).map_err(|_| unavailable())?;
    if !rest.is_empty() {
        return Err(unavailable());
    }
    let seconds: u64 = cert
        .validity()
        .not_after
        .timestamp()
        .try_into()
        .map_err(|_| unavailable())?;
    Timestamp::new(seconds).map_err(|_| unavailable())
}
fn consent_live(until: Instant) -> AppResult<()> {
    if Instant::now() >= until {
        return Err(unavailable());
    }
    Ok(())
}
async fn under_review<T>(
    until: Instant,
    f: impl std::future::Future<Output = AppResult<T>>,
) -> AppResult<T> {
    consent_live(until)?;
    tokio::select! {biased;
        _=tokio::time::sleep_until(tokio::time::Instant::from_std(until))=>Err(unknown()),
        result=f=>{consent_live(until).map_err(|_|unknown())?;result}
    }
}
async fn sign_reviewed(
    app: &AppCore,
    scope: &OriginalScope,
    until: Instant,
    pin: &OwnerPin,
    prepared: PreparedTunnelOwner,
) -> AppResult<TunnelOwnerSigned> {
    under_review(
        until,
        scope.with_loaded(app, |id| {
            consent_live(until)?;
            scope.bind_owner_pin(pin)?;
            let signed = id
                .sign_agent_tunnel_owner_v1(prepared, scope.now()?)
                .map_err(|_| unknown())?;
            consent_live(until)?;
            scope.bind_owner_pin(pin)?;
            Ok(signed)
        }),
    )
    .await
}

#[derive(Clone,Copy,Debug,PartialEq,Eq)]
enum AssociationPurpose { Tunnel, Metrics, Exporters }
impl AssociationPurpose {
    fn scope(self)->Scope {match self{Self::Tunnel=>Scope::TunnelOpen,Self::Metrics=>Scope::MetricsCollect,Self::Exporters=>Scope::ExportersManage}}
    fn label(self)->&'static str {match self{Self::Tunnel=>"tunnel",Self::Metrics=>"metrics",Self::Exporters=>"exporters"}}
    fn scope_name(self)->&'static str {match self{Self::Tunnel=>"tunnel.open",Self::Metrics=>"metrics.collect",Self::Exporters=>"exporters.manage"}}
    fn check_scopes(self,scopes:&Scopes)->AppResult<()> {
        if scopes.0.as_slice()!=[self.scope()]{return Err(unavailable())}Ok(())
    }
    fn offer(self,pin:&OwnerPin,offer:Offer,now:Now)->AppResult<PreparedTunnelOwner> {
        match self {Self::Tunnel=>PreparedTunnelOwner::offer(pin,offer,now),
            Self::Metrics=>PreparedTunnelOwner::metrics_offer(pin,offer,now),Self::Exporters=>PreparedTunnelOwner::exporters_offer(pin,offer,now)}.map_err(|_|unavailable())
    }
    fn certificate(self,pin:&OwnerPin,offer:Signed<Offer>,claim:Signed<Claim>,cert:Certificate,now:Now)->AppResult<PreparedTunnelOwner> {
        match self {Self::Tunnel=>PreparedTunnelOwner::certificate(pin,offer,claim,cert,now),
            Self::Metrics=>PreparedTunnelOwner::metrics_certificate(pin,offer,claim,cert,now),Self::Exporters=>PreparedTunnelOwner::exporters_certificate(pin,offer,claim,cert,now)}.map_err(|_|unavailable())
    }
    fn revoke(self,pin:&OwnerPin,cert:Signed<Certificate>,body:Revocation,now:Now)->AppResult<PreparedTunnelOwner> {
        match self {Self::Tunnel=>PreparedTunnelOwner::revoke(pin,cert,body,now),
            Self::Metrics=>PreparedTunnelOwner::metrics_revoke(pin,cert,body,now),Self::Exporters=>PreparedTunnelOwner::exporters_revoke(pin,cert,body,now)}.map_err(|_|unavailable())
    }
}
/// Actual Node public metadata obtained from a fixed SSH role. Not constructible
/// from a Dart pin, PEM, imported JSON, Reader grant or saved-row status flag.
pub(crate) struct NodeAssociationReview {
    purpose:AssociationPurpose,
    metrics:Option<protocol::MetricsCollector>,
    exporter:Option<protocol::ExporterCatalog>,
    scope: Arc<OriginalScope>,
    peer: transport::Peer,
    ledger: ledger::Ledger,
    probe: protocol::Probe,
    tls: TunnelTlsPublic,
    until: Instant,
    // Same quota lease outlives BOTH a held review and its actual carrier.
    _permit: Arc<tokio::sync::OwnedSemaphorePermit>,
}
pub(crate) struct NodeClaimReview {
    original: NodeAssociationReview,
    offer: Signed<Offer>,
    claim: protocol::Claimed,
    association_id: Uuid,
}
/// Observation of a completed initial transaction, not a Tunnel admission.
pub(crate) struct AssociatedNode {
    purpose:AssociationPurpose,
    scope: Arc<OriginalScope>,
    association_id: Uuid,
    certificate: Signed<Certificate>,
    pin: OwnerPin,
    actor_revision: u64,
}
macro_rules! redacted{($($t:ty),*)=>{$(impl std::fmt::Debug for $t{fn fmt(&self,f:&mut std::fmt::Formatter<'_>)->std::fmt::Result{f.write_str(concat!(stringify!($t),"([redacted])"))}})*}}
redacted!(NodeAssociationReview, NodeClaimReview, AssociatedNode);
impl NodeAssociationReview {
    pub(crate) async fn capture(
        app: &AppCore,
        host_id: ObjectId,
        expected_revision: i64,
        cancel: &CancellationToken,
    ) -> AppResult<Self> {
        let permit = review_permit()?;
        let scope = Arc::new(OriginalScope::capture(app, cancel, Duration::from_secs(300)).await?);
        Self::capture_original(
            app,
            scope,
            host_id,
            expected_revision,
            permit,
            transport::Observation::new(),
        )
        .await
    }
    pub(super) async fn capture_original(
        app: &AppCore,
        scope: Arc<OriginalScope>,
        host_id: ObjectId,
        expected_revision: i64,
        permit: tokio::sync::OwnedSemaphorePermit,
        observation: transport::Observation,
    ) -> AppResult<Self> {
        Self::capture_original_weak(
            &Arc::downgrade(&app.inner),
            scope,
            host_id,
            expected_revision,
            permit,
            observation,
        )
        .await
    }
    pub(super) async fn capture_original_weak(
        core: &std::sync::Weak<crate::app::Inner>,
        scope: Arc<OriginalScope>,
        host_id: ObjectId,
        expected_revision: i64,
        permit: tokio::sync::OwnedSemaphorePermit,
        observation: transport::Observation,
    ) -> AppResult<Self> {
        let permit = observation.retain_permit(permit)?;
        let app = AppCore {
            inner: core.upgrade().ok_or_else(unavailable)?,
        };
        scope.check(&app).await?;
        let mut ledger =
            public::bounded_original(&scope, ledger::Ledger::capture(&app, scope.clone())).await?;
        let custody = ManagedTlsCustody::capture(&app, scope.clone()).await?;
        drop(app);
        // Do not retain Core/Unlocked while the owned OS credential job joins.
        let identity = custody.load_original(core).await?;
        let tls = identity.public_identity()?.clone();
        drop(identity);
        let app = AppCore {
            inner: core.upgrade().ok_or_else(unavailable)?,
        };
        let mut peer = transport::Peer::open_observed(
            &app,
            scope.clone(),
            host_id,
            expected_revision,
            observation,
        )
        .await?;
        let probe: protocol::Probe = peer
            .exchange(&app, "probe", serde_json::json!({}), None)
            .await?;
        protocol::initial(&scope, &probe)?;
        public::bounded_original(
            &scope,
            ledger.require_unassociated(&app, host_id, probe.node.agent_id),
        )
        .await?;
        let until = scope
            .deadline()
            .min(Instant::now() + Duration::from_secs(60));
        peer.cap_review(until)?;
        ledger.cap_review(until)?;
        Ok(Self {
            purpose:AssociationPurpose::Tunnel,metrics:None,exporter:None,
            scope,
            peer,
            ledger,
            probe,
            tls,
            until,
            _permit: permit,
        })
    }
    fn live(&self) -> AppResult<()> {
        self.scope.check_current()?;
        if Instant::now() >= self.until {
            return Err(unavailable());
        }
        protocol::initial(&self.scope, &self.probe)
    }
    /// Fixed public projection of the actual observation, never a policy input.
    pub(crate) fn details(&self) -> AppResult<serde_json::Value> {
        self.live()?;
        let owner_context = self.scope.owner_context()?;
        self.scope.with_io_current(|wall|{
        consent_live(self.until)?;
        validate_pin(&self.probe.owner_pin,Now(Timestamp::new(wall.as_secs()).map_err(|_|unavailable())?)).map_err(|_|unavailable())?;
        let remaining=self.until.checked_duration_since(Instant::now()).filter(|d|d.as_millis()>0).ok_or_else(unavailable)?;
        Ok(
            serde_json::json!({"version":1,"hostId":self.peer.host_id,"hostRevision":self.peer.host_revision.to_string(),
            "node":self.probe.node,"ownerContext":owner_context,
            "ownerEpoch":self.probe.owner_pin.epoch.to_string(),"checkpoint":self.probe.owner_pin.checkpoint.to_string(),
            "actorRevision":self.probe.actor_revision.to_string(),"scope":self.purpose.scope_name(),
            "purpose":self.purpose.label(),"scopes":[self.purpose.scope_name()],"collector":self.metrics.as_ref().map(|m|m.public()),
            "clientTlsIdentityId":self.tls.identity_id,"clientTlsCertificateSha256":self.tls.certificate_sha256,
            "remainingMs":remaining.as_millis().min(60000) as u64}),
        )
        })
    }
    /// Explicit original-owner approval only. No signing occurs at capture.
    /// A consumed/failed attempt is never automatically retried or reconnected.
    pub(crate) async fn approve_offer_once(self, app: &AppCore) -> AppResult<NodeClaimReview> {
        if self.purpose!=AssociationPurpose::Tunnel{return Err(unavailable())}
        let until = self.until;
        under_review(until, self.approve_offer_inner(app)).await
    }
    async fn approve_offer_inner(mut self, app: &AppCore) -> AppResult<NodeClaimReview> {
        self.live()?;
        self.scope.check(app).await?;
        let now = self.scope.now()?;
        let remaining = self
            .until
            .checked_duration_since(Instant::now())
            .filter(|d| d.as_secs() > 0)
            .ok_or_else(unavailable)?;
        let expiry = now
            .0
            .checked_add(remaining.as_secs().min(60))
            .map_err(|_| unavailable())?
            .min(self.probe.owner_pin.expires_at);
        let mut token = Zeroizing::new([0u8; 32]);
        cc_crypto_core::fill_random(token.as_mut()).map_err(|_| unavailable())?;
        let secret = InvitationToken::from_bytes(*token).map_err(|_| unavailable())?;
        let offer = Offer {
            header: Header {
                version: 1,
                context: self.probe.owner_pin.context.clone(),
                owner_epoch: self.probe.owner_pin.epoch,
                checkpoint: protocol::advance(&self.probe.owner_pin, 1)?.checkpoint,
            },
            invitation_id: InvitationId::new(&Uuid::new_v4().to_string())
                .map_err(|_| unavailable())?,
            agent_id: self.probe.node.agent_id,
            agent_key: self.probe.node.public_key,
            token_hash: secret.digest(),
            scopes: Scopes(vec![Scope::TunnelOpen]),
            issued_at: now.0,
            expires_at: expiry,
        };
        let prepared = PreparedTunnelOwner::offer(&self.probe.owner_pin, offer, now)
            .map_err(|_| unavailable())?;
        let signed = sign_reviewed(
            app,
            &self.scope,
            self.until,
            &self.probe.owner_pin,
            prepared,
        )
        .await?;
        let TunnelOwnerSigned::Offer(offer) = signed else {
            return Err(unavailable());
        };
        let association_id = Uuid::new_v4();
        let public = NodeAssociationPublic {
            association_id,
            saved_host_id: self.peer.host_id,
            saved_host_revision: self.peer.host_revision,
            route_trust_digest: self.peer.route_digest.clone(),
            agent_id: agent_text(self.probe.node.agent_id)?,
            node_key_fingerprint: hex::encode(self.probe.node.key_fingerprint.as_bytes()),
            owner_epoch: self.probe.owner_pin.epoch,
            checkpoint: self.probe.owner_pin.checkpoint,
            key_epoch: self.probe.node.key_epoch,
            expected_actor_revision: self.probe.actor_revision,
            client_tls_identity_id: self.tls.identity_id,
            client_tls_certificate_sha256: self.tls.certificate_sha256.clone(),
            offer_digest: hex::encode(offer.body.digest().map_err(|_| unknown())?.as_bytes()),
            signed_offer_json: bytes(&offer)?,
            owner_pin_json: bytes(&self.probe.owner_pin)?,
            certificate: None,
        };
        self.ledger.reserve(app, public).await?;
        self.ledger.mark_unknown(app, association_id).await?; // BEFORE either remote CAS.
        self.live().map_err(|_| unknown())?;
        let token_hex = Zeroizing::new(hex::encode(token.as_ref()));
        let claim = self
            .peer
            .stage(
                app,
                self.probe.actor_revision,
                &self.probe.owner_pin,
                &offer,
                token_hex.as_str(),
            )
            .await?;
        protocol::claimed(&self.probe, &offer, &claim, self.scope.now()?)?;
        self.until = self
            .until
            .min(Instant::now() + self.scope.remaining_until(expiry)?);
        Ok(NodeClaimReview {
            original: self,
            offer,
            claim,
            association_id,
        })
    }
}
impl NodeClaimReview {
    pub(crate) fn details(&self) -> AppResult<serde_json::Value> {
        self.original.live()?;
        protocol::claimed_for(
            self.original.purpose,&self.original.probe,
            &self.offer,
            &self.claim,
            self.original.scope.now()?,
        )?;
        self.original.scope.with_io_current(|wall| {
            consent_live(self.original.until)?;
            check_live(
                self.offer.body.issued_at,
                self.offer.body.expires_at,
                Now(Timestamp::new(wall.as_secs()).map_err(|_| unknown())?),
            )
            .map_err(|_| unknown())?;
            Ok(
                serde_json::json!({"version":1,"associationId":self.association_id,
            "node":self.original.probe.node,"claimDigest":self.claim.claim_digest,
            "scope":self.original.purpose.scope_name(),"purpose":self.original.purpose.label(),"scopes":[self.original.purpose.scope_name()],
            "ownerEpoch":self.claim.owner_pin.epoch.to_string(),
            "checkpoint":self.claim.owner_pin.checkpoint.to_string()}),
            )
        })
    }
    /// Certificate lifetime is a visible bounded user choice, never an epoch,
    /// trust key, scope/capability or raw authority object supplied by callers.
    pub(crate) async fn approve_certificate_once(
        self,
        app: &AppCore,
        lifetime: Duration,
    ) -> AppResult<AssociatedNode> {
        if self.original.purpose!=AssociationPurpose::Tunnel{return Err(unavailable())}
        let until = self.original.until;
        under_review(until, self.approve_certificate_inner(app, lifetime)).await
    }
    async fn approve_certificate_inner(
        mut self,
        app: &AppCore,
        lifetime: Duration,
    ) -> AppResult<AssociatedNode> {
        let o = &mut self.original;
        o.live().map_err(|_| unknown())?;
        o.scope.check(app).await?;
        if lifetime.is_zero()
            || lifetime.subsec_nanos() != 0
            || lifetime > Duration::from_secs(CERTIFICATE_TTL)
        {
            return Err(unavailable());
        }
        let now = o.scope.now()?;
        protocol::claimed(&o.probe, &self.offer, &self.claim, now)?;
        let expires = now
            .0
            .checked_add(lifetime.as_secs())
            .map_err(|_| unavailable())?
            .min(o.probe.owner_pin.expires_at)
            .min(client_expiry(&o.tls)?);
        let cert = Certificate {
            header: Header {
                version: 1,
                context: o.probe.owner_pin.context.clone(),
                owner_epoch: o.probe.owner_pin.epoch,
                checkpoint: protocol::advance(&o.probe.owner_pin, 2)?.checkpoint,
            },
            agent_id: o.probe.node.agent_id,
            key_epoch: 1,
            agent_key: o.probe.node.public_key,
            scopes: Scopes(vec![Scope::TunnelOpen]),
            capabilities: self.claim.claim.body.capabilities.clone(),
            origin: CertificateOrigin::Enrollment {
                invitation_id: self.offer.body.invitation_id,
                claim_hash: self.claim.claim_digest,
                key_fingerprint: o.probe.node.key_fingerprint,
            },
            not_before: now.0,
            expires_at: expires,
        };
        let prepared = PreparedTunnelOwner::certificate(
            &self.claim.owner_pin,
            self.offer.clone(),
            self.claim.claim.clone(),
            cert,
            now,
        )
        .map_err(|_| unknown())?;
        let signed = sign_reviewed(app, &o.scope, o.until, &self.claim.owner_pin, prepared).await?;
        let TunnelOwnerSigned::Certificate(cert) = signed else {
            return Err(unknown());
        };
        let payload = serde_json::json!({"expectedRev":self.claim.actor_revision,"expectedPin":self.claim.owner_pin,
            "claimDigest":self.claim.claim_digest,"certificate":cert});
        // Correlation UUID is not authority; retain the exact PUBLIC payload
        // digest. No token, transport handle or private key enters the row.
        let final_pin = protocol::advance(&o.probe.owner_pin, 2)?;
        let expected_rev = o.probe.actor_revision.checked_add(3).ok_or_else(unknown)?;
        o.ledger
            .attach(
                app,
                self.association_id,
                NodeAssociationCertificate {
                    request_digest: hash(protocol::canonical(&payload)?.as_slice()),
                    certificate_digest: hex::encode(
                        cert.body.digest().map_err(|_| unknown())?.as_bytes(),
                    ),
                    signed_certificate_json: bytes(&cert)?,
                    committed_owner_pin_json: bytes(&final_pin)?,
                    expected_actor_revision: expected_rev,
                },
            )
            .await?;
        let applied: protocol::Committed = o
            .peer
            .exchange(
                app,
                "applyCertificate",
                payload,
                Some(self.offer.body.expires_at),
            )
            .await?;
        protocol::committed(&o.probe, &cert, &applied, o.scope.now()?)?;
        let observed: protocol::Committed = o
            .peer
            .exchange(
                app,
                "observeCertificate",
                serde_json::json!({
            "expectedRev":expected_rev,"expectedPin":final_pin,"agentId":o.probe.node.agent_id,
            "certificateDigest":cert.body.digest().map_err(|_|unknown())?}),
                Some(self.offer.body.expires_at),
            )
            .await?;
        protocol::committed(&o.probe, &cert, &observed, o.scope.now()?)?;
        o.scope.check(app).await?;
        o.ledger.complete(app, self.association_id).await?;
        Ok(AssociatedNode {
            purpose:AssociationPurpose::Tunnel,
            scope: o.scope.clone(),
            association_id: self.association_id,
            certificate: cert,
            pin: final_pin,
            actor_revision: observed.actor_revision,
        })
    }
}
impl AssociatedNode {
    pub(crate) fn details(&self) -> AppResult<serde_json::Value> {
        self.scope.bind_owner_pin(&self.pin)?;
        verify_statement(&self.certificate, self.pin.key).map_err(|_| unknown())?;
        check_live(
            self.certificate.body.not_before,
            self.certificate.body.expires_at,
            self.scope.now()?,
        )
        .map_err(|_| unknown())?;
        self.scope.with_io_current(|wall|{
        check_live(self.certificate.body.not_before,self.certificate.body.expires_at,Now(Timestamp::new(wall.as_secs()).map_err(|_|unknown())?)).map_err(|_|unknown())?;
        Ok(
            serde_json::json!({"version":1,"associationId":self.association_id,"state":"associated",
            "agentId":self.certificate.body.agent_id,"certificateDigest":self.certificate.body.digest().map_err(|_|unknown())?,
            "checkpoint":self.pin.checkpoint.to_string(),"actorAdmission":false}),
        )
        })
    }
}

#[cfg(test)]
#[path = "live_pair.rs"]
mod live_pair;

pub mod public;
pub(crate) use public::MonitoringTunnelRoute;
pub use public::{NodeAssociationOperation, NodeClientIdentityReview, NodeRevocationReview, NodeRotationReview, NodeTunnelControlReview, AgentTunnelRouteOperation, AgentCollectorOperation, AgentExporterOperation, AgentCollectorServiceReview, AgentCollectorServiceRevocationReview};

async fn sign_reviewed_original(core:&std::sync::Weak<crate::app::Inner>,scope:&OriginalScope,
    until:Instant,pin:&OwnerPin,prepared:PreparedTunnelOwner)->AppResult<TunnelOwnerSigned> {
    let app=AppCore{inner:core.upgrade().ok_or_else(unavailable)?};
    sign_reviewed(&app,scope,until,pin,prepared).await
}
impl NodeAssociationReview {
    pub(super) async fn capture_metrics_original_weak(core:&std::sync::Weak<crate::app::Inner>,
        scope:Arc<OriginalScope>,host:ObjectId,revision:i64,permit:tokio::sync::OwnedSemaphorePermit,
        observation:transport::Observation)->AppResult<Self> {
        let permit=observation.retain_permit(permit)?;
        let mut ledger=ledger::Ledger::capture_original(scope.clone()).await?;
        let custody={
            let app=AppCore{inner:core.upgrade().ok_or_else(unavailable)?};
            ManagedTlsCustody::capture(&app,scope.clone()).await?
        };
        // Existing schema requires a REAL stored public client identity. Metrics
        // does not load its OS private TLS key or claim TLS-key possession here.
        let(phase,public)=custody.stored_metadata_original(core).await?;
        if phase!=Some(cc_storage_core::TunnelTlsPhase::Stored){return Err(unavailable())}
        let tls=public.ok_or_else(unavailable)?;
        let mut peer=transport::Peer::open_observed_weak(core,scope.clone(),host,revision,observation).await?;
        let advertised:protocol::MetricsProbe=peer.exchange_weak(core,"probeMetrics",serde_json::json!({}),None).await?;
        let(probe,collector)=advertised.checked(&scope,host,true)?;
        ledger.require_unassociated_original(host,probe.node.agent_id).await?;
        let until=scope.deadline().min(Instant::now()+Duration::from_secs(60));
        peer.cap_review(until)?;ledger.cap_review(until)?;
        let value=Self{purpose:AssociationPurpose::Metrics,metrics:Some(collector),exporter:None,scope,peer,ledger,probe,tls,until,_permit:permit};
        value.live()?;Ok(value)
    }
    pub(super) async fn approve_metrics_offer_once(mut self,core:&std::sync::Weak<crate::app::Inner>)->AppResult<NodeClaimReview> {
        if self.purpose!=AssociationPurpose::Metrics||self.metrics.is_none(){return Err(unavailable())}
        self.live()?;let now=self.scope.now()?;
        let remaining=self.until.checked_duration_since(Instant::now()).filter(|d|d.as_secs()>0).ok_or_else(unavailable)?;
        let expiry=now.0.checked_add(remaining.as_secs().min(60)).map_err(|_|unavailable())?.min(self.probe.owner_pin.expires_at);
        let mut token=Zeroizing::new([0u8;32]);cc_crypto_core::fill_random(token.as_mut()).map_err(|_|unavailable())?;
        let secret=InvitationToken::from_bytes(*token).map_err(|_|unavailable())?;
        let offer=Offer{header:Header{version:1,context:self.probe.owner_pin.context.clone(),owner_epoch:self.probe.owner_pin.epoch,
            checkpoint:protocol::advance(&self.probe.owner_pin,1)?.checkpoint},
            invitation_id:InvitationId::new(&Uuid::new_v4().to_string()).map_err(|_|unavailable())?,agent_id:self.probe.node.agent_id,
            agent_key:self.probe.node.public_key,token_hash:secret.digest(),scopes:Scopes(vec![Scope::MetricsCollect]),issued_at:now.0,expires_at:expiry};
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
    pub(super) async fn approve_metrics_certificate_once(mut self,core:&std::sync::Weak<crate::app::Inner>,
        lifetime:Duration)->AppResult<AssociatedNode> {
        let o=&mut self.original;
        if o.purpose!=AssociationPurpose::Metrics||o.metrics.is_none(){return Err(unavailable())}
        o.live().map_err(|_|unknown())?;
        if lifetime.is_zero()||lifetime.subsec_nanos()!=0||lifetime>Duration::from_secs(CERTIFICATE_TTL){return Err(unavailable())}
        let now=o.scope.now()?;
        protocol::claimed_for(o.purpose,&o.probe,&self.offer,&self.claim,now)?;
        let expires=now.0.checked_add(lifetime.as_secs()).map_err(|_|unavailable())?
            .min(o.probe.owner_pin.expires_at).min(client_expiry(&o.tls)?);
        let cert=Certificate{header:Header{version:1,context:o.probe.owner_pin.context.clone(),owner_epoch:o.probe.owner_pin.epoch,
            checkpoint:protocol::advance(&o.probe.owner_pin,2)?.checkpoint},agent_id:o.probe.node.agent_id,key_epoch:1,
            agent_key:o.probe.node.public_key,scopes:Scopes(vec![Scope::MetricsCollect]),capabilities:self.claim.claim.body.capabilities.clone(),
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
        Ok(AssociatedNode{purpose:AssociationPurpose::Metrics,scope:o.scope.clone(),association_id:self.association_id,
            certificate:cert,pin:final_pin,actor_revision:observed.actor_revision})
    }
}

#[cfg(all(feature = "test-harness", target_os = "linux"))]
mod owned_public_route;

mod exporter_enrollment;
