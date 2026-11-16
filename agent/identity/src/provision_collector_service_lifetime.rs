// Included inside existing provision::linux custody, not a generic signer.
pub(super) fn sign_collector_service_lifetime_receipt(
    directory: &Path,
    mut prepared: crate::enrollment_control::metrics::service_lifetime_control::PreparedLifetimeReceipt<'_>,
) -> Result<Signed<crate::CollectorServiceCredentialSetReceipt>> {
    let now = prepared.recheck()?;
    let (body, readback) = prepared.original();
    let body = body.clone();
    let policy = readback.policy.clone();
    let pin = readback.pin.clone();
    let certificate = readback.certificate.clone();
    let directory = crate::durable::absolute(directory)?;
    crate::durable::secure_path(&directory)?;
    let guard = crate::node_prepare::read_lock(&directory)?;
    let material = load_material(&directory.join(KEY_FILE), body.agent_id)?;
    crate::node_prepare::check(&guard, Some(&material.public))?;
    let selected = crate::node_activation::selected(&guard, &material.public)?;
    let public = selected_public(&material.public, selected.as_ref());
    State::bootstrap(pin.clone(), now)?;
    policy.verify(pin.key)?;
    current_header(&body.header, &pin)?;
    body.validate()?;
    crate::wire::live(certificate.not_before, certificate.expires_at, now)?;
    if body.agent_id != public.agent_id || body.key_epoch != public.key_epoch
        || policy.body.recipient != public.public_key || policy.body.key_epoch != public.key_epoch
        || body.service_id != policy.body.service_id || body.base_delegation_hash != policy.body.digest()?
        || certificate.agent_key != public.public_key || certificate.agent_id != public.agent_id
        || certificate.key_epoch != public.key_epoch || policy.body.certificate_hash != certificate.digest()?
        || certificate.scopes.0.as_slice() != [Scope::MetricsCollect] || certificate.capabilities.scopes != certificate.scopes
    { return Err(Error::WrongClaim); }
    match (&selected, &certificate.origin) {
        (None, CertificateOrigin::Enrollment { key_fingerprint, .. }) if *key_fingerprint == public.key_fingerprint => (),
        (Some(selected), CertificateOrigin::Rotation { previous_certificate, previous_key_epoch: 1 })
            if *previous_certificate == selected.previous_certificate_hash && certificate.digest()? == selected.certificate_hash => (),
        _ => return Err(Error::WrongClaim),
    }
    prepared.recheck()?;
    let signed = if selected.is_some() {
        crate::node_prepare::sign_selected_collector_service_set_receipt(&guard, &material.public, body)?
    } else {
        let signature = SignatureBytes::from_bytes(material.signing.sign(&body.signing_bytes()?).to_bytes());
        Signed { body, signature }
    };
    crate::node_prepare::check(&guard, Some(&material.public))?;
    prepared.finish(signed)
}
