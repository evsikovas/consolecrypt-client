// Closed ExportersOnly owner counterparts; existing Tunnel/Telemetry constructors
// and statement signing domains are unchanged. No generic scope selector.
fn exporters_scope(scopes: &Scopes) -> Result<()> {
    scopes.validate()?;
    if scopes.0.as_slice() != [Scope::ExportersManage] { return Err(Error::CapabilityDenied); }
    Ok(())
}
impl PreparedTunnelOwner {
    pub fn exporters_offer(pin: &OwnerPin, offer: Offer, now: Now) -> Result<Self> {
        validate_pin(pin, now)?;
        offer.validate()?;
        header(pin, &offer.header, true)?;
        exporters_scope(&offer.scopes)?;
        if offer.agent_key == pin.key {
            return Err(Error::KeyReuse);
        }
        check_recent(offer.issued_at, now)?;
        check_live(offer.issued_at, offer.expires_at, now)?;
        if offer.expires_at > pin.expires_at {
            return Err(Error::Expired);
        }
        Ok(Self {
            key: pin.key,
            start: offer.issued_at,
            end: offer.expires_at.min(pin.expires_at).min(offer.issued_at.checked_add(PROOF_TTL)?),
            device: None,
            statement: OwnerStatement::Offer(offer),
        })
    }
    pub fn exporters_certificate(
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
        header(pin, &cert.header, true)?;
        exporters_scope(&cert.scopes)?;
        exporters_scope(&cert.capabilities.scopes)?;
        exporters_scope(&offer.body.scopes)?;
        exporters_scope(&claim.body.capabilities.scopes)?;
        let o = &offer.body;
        let c = &claim.body;
        if o.header.context != pin.context
            || o.header.owner_epoch != pin.epoch
            || o.header.checkpoint > pin.checkpoint
            || c.header != o.header
            || c.invitation_id != o.invitation_id
            || c.offer_hash != o.digest()?
            || c.agent_id != o.agent_id
            || c.agent_key != o.agent_key
            || cert.agent_id != o.agent_id
            || cert.agent_key != o.agent_key
            || cert.key_epoch != 1
            || cert.capabilities != c.capabilities
            || cert.origin
                != (CertificateOrigin::Enrollment {
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
        Ok(Self {
            key: pin.key,
            start: now.0,
            end: cert.expires_at.min(pin.expires_at).min(now.0.checked_add(PROOF_TTL)?).min(c.issued_at.checked_add(PROOF_TTL)?).min(o.expires_at),
            device: None,
            statement: OwnerStatement::Certificate(cert),
        })
    }
    pub fn exporters_revoke(pin: &OwnerPin, certificate: Signed<Certificate>, revoke: Revocation, now: Now) -> Result<Self> {
        validate_pin(pin, now)?;
        verify_statement(&certificate, pin.key)?;
        certificate.body.validate()?;
        revoke.validate()?;
        header(pin, &revoke.header, true)?;
        exporters_scope(&certificate.body.scopes)?;
        exporters_scope(&certificate.body.capabilities.scopes)?;
        let cert = &certificate.body;
        if cert.header.context != pin.context || cert.header.owner_epoch != pin.epoch
            || cert.header.checkpoint > pin.checkpoint {
            return Err(Error::WrongContext);
        }
        if revoke.agent_id != cert.agent_id || revoke.key_epoch != cert.key_epoch
            || revoke.certificate_hash != cert.digest()? {
            return Err(Error::WrongClaim);
        }
        check_recent(revoke.issued_at, now)?;
        let end = revoke.issued_at.checked_add(PROOF_TTL)?.min(pin.expires_at);
        check_live(revoke.issued_at, end, now)?;
        Ok(Self { key: pin.key, start: revoke.issued_at, end, device: None,
            statement: OwnerStatement::Revocation(revoke) })
    }
    /// Existing ExportersManage lease purpose bound to the exact closed policy.
    pub fn exporters_grant(pin:&OwnerPin,certificate:Signed<Certificate>,policy:&ManagedExporterPolicy,
        lease:Lease,freshness:Freshness,now:Now)->Result<Self> {
        validate_pin(pin,now)?;verify_statement(&certificate,pin.key)?;
        let cert=&certificate.body;cert.validate()?;
        exporters_scope(&cert.scopes)?;exporters_scope(&cert.capabilities.scopes)?;
        if cert.header.context!=pin.context || cert.header.owner_epoch!=pin.epoch
            || cert.header.checkpoint>pin.checkpoint {return Err(Error::WrongContext)}
        lease.validate()?;freshness.validate()?;
        header(pin,&lease.header,false)?;header(pin,&freshness.header,false)?;
        check_live(cert.not_before,cert.expires_at,now)?;
        check_live(lease.issued_at,lease.expires_at,now)?;check_recent(lease.issued_at,now)?;
        check_live(freshness.issued_at,freshness.expires_at,now)?;check_recent(freshness.issued_at,now)?;
        let operation=policy.operation()?;
        if lease.operation!=operation || lease.agent_id!=cert.agent_id || lease.key_epoch!=cert.key_epoch
            || lease.certificate_hash!=cert.digest()? || lease.header!=freshness.header {
            return Err(Error::WrongClaim)
        }
        if lease.expires_at>cert.expires_at || lease.expires_at>pin.expires_at
            || lease.expires_at>freshness.expires_at || freshness.expires_at>pin.expires_at {
            return Err(Error::Expired)
        }
        let start=now.0.max(lease.issued_at).max(freshness.issued_at).max(cert.not_before);
        let end=lease.expires_at.min(freshness.expires_at).min(cert.expires_at)
            .min(pin.expires_at).min(now.0.checked_add(PROOF_TTL)?)
            .min(lease.issued_at.checked_add(PROOF_TTL)?).min(freshness.issued_at.checked_add(PROOF_TTL)?);
        check_live(start,end,now)?;
        let blank=SignatureBytes::from_bytes([0;64]);
        let binding=NodeRequestBinding {certificate,
            lease:Signed {body:lease,signature:blank},freshness:Signed {body:freshness,signature:blank},operation};
        policy.check_binding(&binding)?;
        Ok(Self {key:pin.key,start,end,device:None,statement:OwnerStatement::Grant {binding}})
    }
}

