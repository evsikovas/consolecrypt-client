// Included only inside provision::linux, beside the existing closed service
// claim/receipt signer. A caller cannot deserialize the prepared private holder.
pub(super) fn sign_collector_recipient_attestation<S: CompareAndSwap>(
    directory: &Path,
    mut prepared: crate::collector_credentials::PreparedRecipientAttestation<'_, S>,
    clock: &mut impl FnMut() -> Result<Now>,
) -> Result<Signed<CollectorRecipientAttestation>> {
    let now = prepared.check_original(clock)?;
    let (body, policy, certificate, pin) = prepared.original();
    let directory = crate::durable::absolute(directory)?;
    crate::durable::secure_path(&directory)?;
    let guard = crate::node_prepare::read_lock(&directory)?;
    let material = load_material(&directory.join(KEY_FILE), body.agent_id)?;
    crate::node_prepare::check(&guard, Some(&material.public))?;
    let selected = crate::node_activation::selected(&guard, &material.public)?;
    let public = selected_public(&material.public, selected.as_ref());
    State::bootstrap(pin.clone(), now)?;
    current_header(&policy.header, &pin)?;
    crate::wire::live(policy.issued_at, policy.expires_at, now)?;
    crate::wire::live(certificate.not_before, certificate.expires_at, now)?;
    crate::wire::live(body.issued_at, body.expires_at, now)?;
    if policy.recipient != public.public_key || policy.key_epoch != public.key_epoch
        || policy.agent_id != public.agent_id || policy.certificate_hash != certificate.digest()?
        || certificate.agent_id != public.agent_id || certificate.agent_key != public.public_key
        || certificate.key_epoch != public.key_epoch || body.header != policy.header
        || body.agent_id != policy.agent_id || body.key_epoch != policy.key_epoch
        || body.certificate_hash != policy.certificate_hash || body.service_id != policy.service_id
        || body.manifest_sha256 != policy.manifest_sha256 || body.base_delegation_hash != policy.digest()?
        || body.expires_at > policy.expires_at || body.expires_at > certificate.expires_at
        || body.expires_at > pin.expires_at
    { return Err(Error::WrongClaim); }
    if certificate.scopes.0.as_slice() != [Scope::MetricsCollect]
        || certificate.capabilities.scopes != certificate.scopes
    { return Err(Error::CapabilityDenied); }
    match (&selected, &certificate.origin) {
        (None, CertificateOrigin::Enrollment { key_fingerprint, .. })
            if *key_fingerprint == public.key_fingerprint => (),
        (Some(selected), CertificateOrigin::Rotation { previous_certificate, previous_key_epoch: 1 })
            if *previous_certificate == selected.previous_certificate_hash
                && certificate.digest()? == selected.certificate_hash => (),
        _ => return Err(Error::WrongClaim),
    }
    prepared.check_recipient()?;
    let signed = if selected.is_some() {
        crate::node_prepare::sign_selected_collector_recipient(&guard, &material.public, body)?
    } else {
        let signature = SignatureBytes::from_bytes(material.signing.sign(&body.signing_bytes()?).to_bytes());
        Signed { body, signature }
    };
    crate::node_prepare::check(&guard, Some(&material.public))?;
    prepared.finish(signed, clock)
}
