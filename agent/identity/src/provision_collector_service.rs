// Included ONLY inside the existing Linux Node-custody module. No provisioning,
// recovery, key export, generic message, caller nonce, or alternate key lookup.
pub(super) fn collector_service_claim(
    directory: &Path,
    agent: AgentId,
    pin: &OwnerPin,
    certificate: &Certificate,
    policy: &Signed<CollectorServiceDelegation>,
    now: Now,
) -> Result<Signed<CollectorServiceClaim>> {
    let directory = crate::durable::absolute(directory)?;
    crate::durable::secure_path(&directory)?;
    let guard = crate::node_prepare::read_lock(&directory)?;
    let material = load_material(&directory.join(KEY_FILE), agent)?;
    crate::node_prepare::check(&guard, Some(&material.public))?;
    let selected = crate::node_activation::selected(&guard, &material.public)?;
    let public = selected_public(&material.public, selected.as_ref());
    State::bootstrap(pin.clone(), now)?;
    policy.verify(pin.key)?;
    let p = &policy.body;
    current_header(&p.header, pin)?;
    crate::wire::recent(p.issued_at, now)?;
    crate::wire::live(p.issued_at, p.expires_at, now)?;
    crate::wire::live(certificate.not_before, certificate.expires_at, now)?;
    if certificate.scopes.0.as_slice() != [Scope::MetricsCollect]
        || certificate.capabilities.scopes != certificate.scopes
    {
        return Err(Error::CapabilityDenied);
    }
    if certificate.header.context != pin.context
        || certificate.header.owner_epoch != pin.epoch
        || certificate.header.checkpoint > pin.checkpoint
    {
        return Err(Error::WrongContext);
    }
    if certificate.agent_id != agent
        || certificate.agent_key != public.public_key
        || certificate.key_epoch != public.key_epoch
        || p.agent_id != agent
        || p.recipient != public.public_key
        || p.key_epoch != public.key_epoch
        || p.certificate_hash != certificate.digest()?
    {
        return Err(Error::WrongClaim);
    }
    if p.expires_at > certificate.expires_at || p.expires_at > pin.expires_at {
        return Err(Error::Expired);
    }
    match (&selected, &certificate.origin) {
        (
            None,
            CertificateOrigin::Enrollment {
                key_fingerprint, ..
            },
        ) if *key_fingerprint == public.key_fingerprint => (),
        (
            Some(selected),
            CertificateOrigin::Rotation {
                previous_certificate,
                previous_key_epoch: 1,
            },
        ) if *previous_certificate == selected.previous_certificate_hash
            && certificate.digest()? == selected.certificate_hash =>
        {
            ()
        }
        _ => return Err(Error::WrongClaim),
    }
    let body = CollectorServiceClaim {
        header: p.header.clone(),
        agent_id: agent,
        key_epoch: public.key_epoch,
        delegation_hash: p.digest()?,
        clock: CollectorServiceClockObservation::observe_local()?.stamp_for(p.expires_at)?,
        nonce: random_nonce()?,
        issued_at: now.0,
    };
    if selected.is_some() {
        return crate::node_prepare::sign_selected_collector_service_claim(
            &guard,
            &material.public,
            body,
        );
    }
    let signature =
        SignatureBytes::from_bytes(material.signing.sign(&body.signing_bytes()?).to_bytes());
    crate::node_prepare::check(&guard, Some(&material.public))?;
    Ok(Signed { body, signature })
}

// A fixed committed-state observation, not a policy signer or runtime receipt.
// The service-control worker constructs it only AFTER exact Actor readback.
pub(super) fn collector_service_receipt(
    directory: &Path,
    agent: AgentId,
    pin: &OwnerPin,
    policy: &Signed<CollectorServiceDelegation>,
    body: CollectorServiceReceipt,
    now: Now,
) -> Result<Signed<CollectorServiceReceipt>> {
    let directory = crate::durable::absolute(directory)?;
    crate::durable::secure_path(&directory)?;
    let guard = crate::node_prepare::read_lock(&directory)?;
    let material = load_material(&directory.join(KEY_FILE), agent)?;
    crate::node_prepare::check(&guard, Some(&material.public))?;
    let selected = crate::node_activation::selected(&guard, &material.public)?;
    let public = selected_public(&material.public, selected.as_ref());
    State::bootstrap(pin.clone(), now)?;
    policy.verify(pin.key)?;
    current_header(&body.header, pin)?;
    crate::wire::recent(body.issued_at, now)?;
    let p = &policy.body;
    if p.header.context != body.header.context
        || p.header.owner_epoch != body.header.owner_epoch
        || p.header.checkpoint > body.header.checkpoint
        || p.agent_id != agent
        || body.agent_id != agent
        || p.key_epoch != public.key_epoch
        || body.key_epoch != public.key_epoch
        || p.recipient != public.public_key
        || body.service_id != p.service_id
        || body.delegation_hash != p.digest()?
        || body.policy_revision != p.policy_revision
    {
        return Err(Error::WrongClaim);
    }
    match body.effect {
        CollectorServiceEffect::Configured if body.revocation_epoch == p.revocation_epoch => (),
        CollectorServiceEffect::Revoked if body.revocation_epoch > p.revocation_epoch => (),
        _ => return Err(Error::WrongClaim),
    }
    if selected.is_some() {
        return crate::node_prepare::sign_selected_collector_service_receipt(
            &guard,
            &material.public,
            body,
        );
    }
    let signature =
        SignatureBytes::from_bytes(material.signing.sign(&body.signing_bytes()?).to_bytes());
    crate::node_prepare::check(&guard, Some(&material.public))?;
    Ok(Signed { body, signature })
}
