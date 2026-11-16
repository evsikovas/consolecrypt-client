//! Distinct public prepare, SAME existing opaque review/confirm. No mode on confirm.
use super::*;

/// Private confirmation provenance, not a grant, policy token or caller DTO.
/// Captured only after the actual original scope/refs/Peer have been checked.
#[derive(Clone)]
pub(super) struct PreparedReviewIdentity {
    scope: Arc<OriginalScope>,
    until: Instant,
    review_id: Uuid,
    protocol: CredentialProtocol,
    host: ObjectId,
    host_revision: i64,
    route: String,
    delegation_hash: Hash,
    manifest_sha256: Hash,
    credentials: Vec<Value>,
}
impl PreparedReviewIdentity {
    pub(super) fn capture_checked(context: &Context, facts: &ServiceReviewFacts,
        route: &str, protocol: CredentialProtocol) -> AppResult<Self> {
        facts.live(context)?;
        context.scope.check_current()?;
        Ok(Self { scope: context.scope.clone(), until: context.scope.deadline(), review_id: Uuid::new_v4(),
            protocol, host: facts.host, host_revision: facts.host_revision, route: route.to_owned(),
            delegation_hash: facts.policy.digest().map_err(|_| unavailable())?,
            manifest_sha256: facts.catalog.manifest_sha256,
            credentials: facts.credentials.iter().map(|item|item.view()).collect() })
    }
    pub(super) fn protocol(&self) -> CredentialProtocol { self.protocol }
    fn require_identity(&self, other: &Self) -> AppResult<()> {
        self.scope.check_current()?;
        consent_live(self.until)?;
        if !Arc::ptr_eq(&self.scope, &other.scope) || self.until != self.scope.deadline()
            || other.until != self.until || self.review_id != other.review_id
            || self.protocol != other.protocol || self.host != other.host || self.host_revision != other.host_revision
            || self.route != other.route || self.delegation_hash != other.delegation_hash
            || self.manifest_sha256 != other.manifest_sha256 || self.credentials != other.credentials
        { return Err(unavailable()); }
        Ok(())
    }
    pub(super) fn require_ready(&self, context: &Context, facts: &ServiceReviewFacts, ready: &Prepared) -> AppResult<()> {
        self.require_identity(&ready.review_identity)?;
        if !Arc::ptr_eq(&self.scope, &context.scope) || self.protocol != ready.credential_protocol
            || self.route != ready.route || self.host != facts.host || self.host_revision != facts.host_revision
            || self.delegation_hash != facts.policy.digest().map_err(|_| unavailable())?
            || self.manifest_sha256 != facts.catalog.manifest_sha256
            || self.credentials != facts.credentials.iter().map(|item|item.view()).collect::<Vec<_>>()
        { return Err(unavailable()); }
        if self.protocol == CredentialProtocol::ServiceLifetime {
            ready.peer.as_ref().ok_or_else(unavailable)?.require_original_lifetime_service(
                &self.scope,self.host,self.host_revision,&self.route,&facts.node)?;
            ready.ledger.require_original_scope(&self.scope)?;
        }
        facts.live(context)
    }
    pub(super) fn publish_lifetime(&self, context: &Context, facts: &ServiceReviewFacts, raw: String) -> AppResult<String> {
        if !Arc::ptr_eq(&self.scope,&context.scope) { return Err(unavailable()); }
        self.scope.check_current()?;
        consent_live(self.until)?;
        facts.live(context)?;
        let mut value: Value = serde_json::from_str(&raw).map_err(|_| unavailable())?;
        value["kind"] = json!("finiteCollectorServiceLifetimeCredentials");
        value["credentialPurpose"] = json!("metrics-collector-service-credential-authorization-v1");
        value["reviewId"] = json!(self.review_id);
        value["requiresExplicitLifetimeConsent"] = json!(true);
        value["encryptedCredentialRetentionUntilUtc"] = json!(data::utc(facts.policy.expires_at)?);
        value["automaticCredentialRefresh"] = json!(false);
        // Actual BOOTTIME stamp is published by the existing view only once the
        // original signed Node admission claim exists. Never fabricate a stamp.
        context.publish(value, 16384)
    }
}

impl AppCore {
    /// Explicit service-lifetime credential intent. This prepares a bounded
    /// opaque review; it does not sign, configure, upgrade another review or
    /// imply consent. Confirm consumes ONLY this captured original review.
    pub async fn agent_collector_service_lifetime_prepare_json(self, raw: &str)
        -> AppResult<AgentCollectorServiceReview> {
        let request: Request = parse(raw)?;
        request.association()?;
        if request.credentials.is_empty() || request.credentials.len() > 2 { return Err(unavailable()); }
        let context = Context::capture(&self, Frame { version: request.version,
            profile_id: request.profile_id.clone(), notification_scope_id: request.notification_scope_id.clone() }).await?;
        drop(self);
        #[cfg(test)]
        bounded_original(&context.scope, tests::observe_original_capture(&context)).await?;
        let control = transport::Observation::new();
        let (ready, facts) = bounded_original(&context.scope,
            Prepared::capture_lifetime(&context, &request, control.clone())).await?;
        if ready.credential_protocol != CredentialProtocol::ServiceLifetime { return Err(unavailable()); }
        let confirmation = ready.review_identity.clone();
        confirmation.require_ready(&context,&facts,&ready)?;
        let (done, _) = watch::channel(true);
        let shared = Arc::new(Shared { context, facts, control, confirmation,
            state: Mutex::new(State { ready:Some(ready), phase:ServiceViewPhase::Review, ack:None,task:None,
                busy:false,consumed:false,mutation_attempted:false,joined:false,orphaned:false }), done });
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

#[cfg(test)]
#[path="lifetime_entry_tests.rs"]
mod tests;

#[cfg(any(test,all(feature="test-harness",any(target_os="linux",target_os="windows"))))]
#[path="lifetime_entry_paired.rs"]
pub(in crate::agent_tunnel::association::public::collector_service) mod paired;
