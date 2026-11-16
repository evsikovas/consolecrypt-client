//! TelemetryRead-only Node enrollment. No Reader issuer or Actor admission.
use crate::*;

pub struct PreparedReaderNodeOwner {
    details: ReaderNodeOwnerReviewDetails,
    start: Timestamp,
    end: Timestamp,
}

/// Public unsigned review material. Constructing this DTO confers no authority.
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReaderNodeOwnerReviewDetails {
    pub owner_pin: OwnerPin,
    pub body: ReaderNodeOwnerReviewBody,
    pub signing_not_before: Timestamp,
    pub signing_expires_at: Timestamp,
}

#[derive(Clone, Serialize)]
#[serde(tag = "kind", content = "value", rename_all = "camelCase")]
pub enum ReaderNodeOwnerReviewBody {
    Offer(Offer),
    Certificate(Certificate),
}

pub enum ReaderNodeOwnerSigned {
    Offer(Signed<Offer>),
    Certificate(Signed<Certificate>),
}

impl std::fmt::Debug for PreparedReaderNodeOwner {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("PreparedReaderNodeOwner([redacted])")
    }
}
impl std::fmt::Debug for ReaderNodeOwnerSigned {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("ReaderNodeOwnerSigned([redacted])")
    }
}

fn telemetry(scopes: &Scopes) -> Result<()> {
    scopes.validate()?;
    if scopes.0.as_slice() != [Scope::TelemetryRead] {
        return Err(Error::CapabilityDenied);
    }
    Ok(())
}
fn next(pin: &OwnerPin, header: &Header) -> Result<()> {
    if header.version != 1
        || header.context != pin.context
        || header.owner_epoch != pin.epoch
        || header.checkpoint != pin.checkpoint.checked_add(1).ok_or(Error::Quota)?
    {
        return Err(Error::WrongCheckpoint);
    }
    Ok(())
}

impl PreparedReaderNodeOwner {
    pub fn offer(pin: &OwnerPin, offer: Offer, now: Now) -> Result<Self> {
        validate_pin(pin, now)?;
        offer.validate()?;
        next(pin, &offer.header)?;
        telemetry(&offer.scopes)?;
        if offer.agent_key == pin.key {
            return Err(Error::KeyReuse);
        }
        check_recent(offer.issued_at, now)?;
        check_live(offer.issued_at, offer.expires_at, now)?;
        if offer.expires_at > pin.expires_at {
            return Err(Error::Expired);
        }
        let start = now.0;
        let end = offer.issued_at.checked_add(PROOF_TTL)?
            .min(offer.expires_at).min(pin.expires_at);
        Self::prepared(pin, ReaderNodeOwnerReviewBody::Offer(offer), start, end, now)
    }

    /// Explicit fresh pin AFTER actual Offer apply. Claim consume is separate;
    /// only the Agent Actor can establish current registration and commit CAS.
    pub fn certificate(
        pin: &OwnerPin,
        offer: Signed<Offer>,
        claim: Signed<Claim>,
        cert: Certificate,
        now: Now,
    ) -> Result<Self> {
        validate_pin(pin, now)?;
        verify_statement(&offer, pin.key)?;
        verify_statement(&claim, offer.body.agent_key)?;
        cert.validate()?;
        next(pin, &cert.header)?;
        telemetry(&offer.body.scopes)?;
        telemetry(&claim.body.capabilities.scopes)?;
        telemetry(&cert.scopes)?;
        telemetry(&cert.capabilities.scopes)?;
        let o = &offer.body;
        let c = &claim.body;
        if o.agent_key == pin.key {
            return Err(Error::KeyReuse);
        }
        if o.header.version != 1
            || o.header.context != pin.context
            || o.header.owner_epoch != pin.epoch
            || o.header.checkpoint != pin.checkpoint
            || c.header != o.header
            || c.invitation_id != o.invitation_id
            || c.offer_hash != o.digest()?
            || c.agent_id != o.agent_id
            || c.agent_key != o.agent_key
            || cert.agent_id != o.agent_id
            || cert.agent_key != o.agent_key
            || cert.key_epoch != 1
            || cert.capabilities != c.capabilities
            || cert.origin != (CertificateOrigin::Enrollment {
                invitation_id: o.invitation_id,
                claim_hash: c.digest()?,
                key_fingerprint: c.agent_key.fingerprint(),
            })
        {
            return Err(Error::WrongClaim);
        }
        check_live(o.issued_at, o.expires_at, now)?;
        check_recent(c.issued_at, now)?;
        check_live(cert.not_before, cert.expires_at, now)?;
        if cert.expires_at > pin.expires_at {
            return Err(Error::Expired);
        }
        // A long certificate validity is not a long review/signing authority.
        let start = now.0;
        let end = start.checked_add(PROOF_TTL)?
            .min(c.issued_at.checked_add(PROOF_TTL)?)
            .min(o.expires_at).min(cert.expires_at).min(pin.expires_at);
        Self::prepared(pin, ReaderNodeOwnerReviewBody::Certificate(cert), start, end, now)
    }

    fn prepared(
        pin: &OwnerPin,
        body: ReaderNodeOwnerReviewBody,
        start: Timestamp,
        end: Timestamp,
        now: Now,
    ) -> Result<Self> {
        check_live(start, end, now)?;
        Ok(Self {
            details: ReaderNodeOwnerReviewDetails {
                owner_pin: pin.clone(), body,
                signing_not_before: start, signing_expires_at: end,
            },
            start, end,
        })
    }
    pub fn details(&self) -> &ReaderNodeOwnerReviewDetails {
        &self.details
    }
    pub fn recheck(&self, now: Now) -> Result<()> {
        validate_pin(&self.details.owner_pin, now)?;
        check_live(self.start, self.end, now)
    }
    pub fn expected_key(&self) -> PublicKey {
        self.details.owner_pin.key
    }
    /// Existing sealed Offer or Certificate transcript only; no arbitrary bytes.
    pub fn signing_transcript(&self) -> Result<Vec<u8>> {
        match &self.details.body {
            ReaderNodeOwnerReviewBody::Offer(body) => body.signing_bytes(),
            ReaderNodeOwnerReviewBody::Certificate(body) => body.signing_bytes(),
        }
    }
    pub fn finish(self, signature: [u8; 64]) -> Result<ReaderNodeOwnerSigned> {
        let key = self.expected_key();
        let signature = SignatureBytes::from_bytes(signature);
        match self.details.body {
            ReaderNodeOwnerReviewBody::Offer(body) => {
                let signed = Signed { body, signature };
                verify_statement(&signed, key)?;
                Ok(ReaderNodeOwnerSigned::Offer(signed))
            }
            ReaderNodeOwnerReviewBody::Certificate(body) => {
                let signed = Signed { body, signature };
                verify_statement(&signed, key)?;
                Ok(ReaderNodeOwnerSigned::Certificate(signed))
            }
        }
    }
}
