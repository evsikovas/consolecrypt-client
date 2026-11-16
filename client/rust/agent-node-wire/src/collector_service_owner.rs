//! Closed collector-service purposes using the existing real Device signer.
use crate::*;

enum Purpose {
    Delegate(CollectorServiceDelegation),
    Revoke(CollectorServiceRevocation),
}
pub enum CollectorServiceOwnerSigned {
    Delegation(Signed<CollectorServiceDelegation>),
    Revocation(Signed<CollectorServiceRevocation>),
}
/// Prepared bytes carry no UI approval or Agent admission.
pub struct PreparedCollectorServiceOwner {
    key: PublicKey,
    start: Timestamp,
    end: Timestamp,
    purpose: Purpose,
}
impl PreparedCollectorServiceOwner {
    pub fn delegation(
        pin: &OwnerPin,
        certificate: &Signed<Certificate>,
        policy: CollectorServiceDelegation,
        now: Now,
    ) -> Result<Self> {
        validate_pin(pin, now)?;
        verify_statement(certificate, pin.key)?;
        policy.validate()?;
        let cert = &certificate.body;
        if cert.scopes.0.as_slice() != [Scope::MetricsCollect]
            || cert.capabilities.scopes != cert.scopes
        {
            return Err(Error::CapabilityDenied);
        }
        if policy.header.context != pin.context
            || policy.header.owner_epoch != pin.epoch
            || policy.header.checkpoint != pin.checkpoint
            || cert.header.context != pin.context
            || cert.header.owner_epoch != pin.epoch
            || cert.header.checkpoint > pin.checkpoint
        {
            return Err(Error::WrongContext);
        }
        if policy.agent_id != cert.agent_id
            || policy.key_epoch != cert.key_epoch
            || policy.recipient != cert.agent_key
            || policy.certificate_hash != cert.digest()?
            || policy.collector_id != metrics_collector_id(policy.manifest_sha256)?
        {
            return Err(Error::WrongClaim);
        }
        check_live(cert.not_before, cert.expires_at, now)?;
        check_live(policy.issued_at, policy.expires_at, now)?;
        check_recent(policy.issued_at, now)?;
        if policy.expires_at > cert.expires_at || policy.expires_at > pin.expires_at {
            return Err(Error::Expired);
        }
        let end = policy
            .issued_at
            .checked_add(PROOF_TTL)?
            .min(policy.expires_at);
        Ok(Self {
            key: pin.key,
            start: policy.issued_at,
            end,
            purpose: Purpose::Delegate(policy),
        })
    }
    pub fn revocation(
        pin: &OwnerPin,
        original: &Signed<CollectorServiceDelegation>,
        revoke: CollectorServiceRevocation,
        now: Now,
    ) -> Result<Self> {
        validate_pin(pin, now)?;
        verify_statement(original, pin.key)?;
        revoke.validate()?;
        let old = &original.body;
        if revoke.header.context != pin.context
            || revoke.header.owner_epoch != pin.epoch
            || revoke.header.checkpoint != pin.checkpoint
            || old.header.context != pin.context
            || old.header.owner_epoch != pin.epoch
        {
            return Err(Error::WrongContext);
        }
        if revoke.service_id != old.service_id
            || revoke.delegation_hash != old.digest()?
            || revoke.policy_revision != old.policy_revision
            || revoke.revocation_epoch <= old.revocation_epoch
        {
            return Err(Error::WrongClaim);
        }
        check_recent(revoke.issued_at, now)?;
        let end = revoke.issued_at.checked_add(PROOF_TTL)?.min(pin.expires_at);
        Ok(Self {
            key: pin.key,
            start: revoke.issued_at,
            end,
            purpose: Purpose::Revoke(revoke),
        })
    }
    pub fn recheck(&self, now: Now) -> Result<()> {
        check_live(self.start, self.end, now)
    }
    pub fn expected_key(&self) -> PublicKey {
        self.key
    }
    pub fn signing_transcript(&self) -> Result<Vec<u8>> {
        match &self.purpose {
            Purpose::Delegate(body) => body.signing_bytes(),
            Purpose::Revoke(body) => body.signing_bytes(),
        }
    }
    pub fn finish(self, signature: [u8; 64]) -> Result<CollectorServiceOwnerSigned> {
        let signature = SignatureBytes::from_bytes(signature);
        match self.purpose {
            Purpose::Delegate(body) => {
                let signed = Signed { body, signature };
                verify_statement(&signed, self.key)?;
                Ok(CollectorServiceOwnerSigned::Delegation(signed))
            }
            Purpose::Revoke(body) => {
                let signed = Signed { body, signature };
                verify_statement(&signed, self.key)?;
                Ok(CollectorServiceOwnerSigned::Revocation(signed))
            }
        }
    }
}
