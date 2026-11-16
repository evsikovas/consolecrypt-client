//! Explicit finite collector-service review, separate from interactive300s.
//! Registered MetricsOnly Node facts are captured by the original opaque owner.
use super::*;
use cc_agent_node_wire::{
    CollectorServiceClaim, CollectorServiceDelegation, CollectorServiceEffect,
    CollectorServiceReceipt, PreparedCollectorServiceOwner,
};
mod credentials;
mod management;
mod service_revoke;
pub use service_revoke::AgentCollectorServiceRevocationReview;
mod operation;
mod view;
pub use operation::AgentCollectorServiceReview;

pub(super) struct ServiceReviewFacts {
    pub(super) association: Uuid,
    pub(super) host: ObjectId,
    pub(super) host_revision: i64,
    pub(super) node: protocol::NodeIdentity,
    pub(super) pin: OwnerPin,
    pub(super) certificate: Signed<Certificate>,
    pub(super) catalog: protocol::MetricsCollector,
    pub(super) policy: CollectorServiceDelegation,
    pub(super) actor_revision: u64,
    pub(super) credentials: Vec<credentials::Original>,
}
impl ServiceReviewFacts {
    fn live(&self, context: &Context) -> AppResult<()> {
        context.scope.check_current()?;
        context.scope.bind_owner_pin(&self.pin)?;
        self.catalog.check(self.host)?;
        self.policy
            .check_credential_targets(&self.catalog.manifest)
            .map_err(|_| unavailable())?;
        if self.policy.credential_targets
            != self
                .credentials
                .iter()
                .map(|item| item.target.clone())
                .collect::<Vec<_>>()
        {
            return Err(unavailable());
        }
        if self.catalog.collector_id != self.policy.collector_id
            || self.catalog.manifest_sha256 != self.policy.manifest_sha256
            || self.catalog.manifest["profile"] != "maintained-prometheus-disk-512mib-2h-v1"
            || self.catalog.manifest["bufferMode"] != "persistentNativeWal"
            || self.node.agent_id != self.policy.agent_id
            || self.node.key_epoch != self.policy.key_epoch
            || self.node.public_key != self.policy.recipient
            || self.node.key_fingerprint != self.node.public_key.fingerprint()
        {
            return Err(unavailable());
        }
        // This prepares no signature and admits no Node action. It checks the
        // actual original cert/owner/body/scopes and the <=24h statement.
        PreparedCollectorServiceOwner::delegation(
            &self.pin,
            &self.certificate,
            self.policy.clone(),
            context.scope.now()?,
        )
        .map_err(|_| unavailable())?;
        Ok(())
    }
}

#[derive(Clone, Copy)]
pub(super) enum ServiceViewPhase {
    Review,
    Configuring,
    ConfigurationRecorded,
    Unknown,
    Cancelled,
}
impl ServiceViewPhase {
    fn label(self) -> &'static str {
        match self {
            Self::Review => "serviceReview",
            Self::Configuring => "configuring",
            Self::ConfigurationRecorded => "configurationRecorded",
            Self::Unknown => "unknown",
            Self::Cancelled => "cancelled",
        }
    }
}

/// No caller ACK/ready boolean constructor. Only exact original Node signatures
/// plus the expected committed Actor revision produce this projection input.
pub(super) struct VerifiedServiceConfiguration {
    receipt: Signed<CollectorServiceReceipt>,
    claim: Signed<CollectorServiceClaim>,
}
impl VerifiedServiceConfiguration {
    pub(super) fn checked(
        facts: &ServiceReviewFacts,
        context: &Context,
        receipt: Signed<CollectorServiceReceipt>,
        claim: Signed<CollectorServiceClaim>,
    ) -> AppResult<Self> {
        facts.live(context)?;
        let key = facts.node.public_key;
        cc_agent_node_wire::verify_statement(&receipt, key).map_err(|_| unavailable())?;
        cc_agent_node_wire::verify_statement(&claim, key).map_err(|_| unavailable())?;
        let policy = &facts.policy;
        let digest = policy.digest().map_err(|_| unavailable())?;
        if receipt.body.effect != CollectorServiceEffect::Configured
            || receipt.body.header != policy.header
            || claim.body.header != policy.header
            || receipt.body.service_id != policy.service_id
            || receipt.body.delegation_hash != digest
            || claim.body.delegation_hash != digest
            || receipt.body.agent_id != policy.agent_id
            || claim.body.agent_id != policy.agent_id
            || receipt.body.key_epoch != policy.key_epoch
            || claim.body.key_epoch != policy.key_epoch
            || receipt.body.policy_revision != policy.policy_revision
            || receipt.body.revocation_epoch != policy.revocation_epoch
            || receipt.body.actor_revision <= facts.actor_revision
            || claim.body.issued_at < policy.issued_at
            || receipt.body.issued_at < claim.body.issued_at
        {
            return Err(unavailable());
        }
        let now = context.scope.now()?;
        cc_agent_node_wire::check_recent(claim.body.issued_at, now).map_err(|_| unavailable())?;
        cc_agent_node_wire::check_recent(receipt.body.issued_at, now).map_err(|_| unavailable())?;
        let remaining = policy
            .expires_at
            .seconds()
            .checked_sub(claim.body.issued_at.seconds())
            .and_then(|n| n.checked_mul(1_000_000_000))
            .ok_or_else(unavailable)?;
        if claim.body.clock.deadline_boottime_ns - claim.body.clock.created_boottime_ns > remaining
        {
            return Err(unavailable());
        }
        Ok(Self { receipt, claim })
    }
}
