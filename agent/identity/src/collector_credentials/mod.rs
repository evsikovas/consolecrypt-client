//! Credential custody borrowed by the SAME installed service Permit owner.
//! No deserialized caller flag, service ID or policy clone grants access here.
use crate::{Actor, Certificate, CollectorCredentialScope, CollectorRecipientAttestation, CollectorServiceDelegation,
    CollectorServicePermit, CollectorServiceAccess, CompareAndSwap, Error, Hash, Nonce, Now, OwnerPin,
    ResourceId, Result, Signed, Statement, PROOF_TTL};
mod recipient;
mod registered;
pub(crate) use registered::RegisteredCollectorCredential;

/// An owned private recipient, distinct from the certified Node signing key.
/// Creation/opening is explicit. Failed mutation is never repaired or adopted.
pub(crate) struct CollectorCredentialRecipient {
    inner: recipient::Recipient,
}

fn fresh_lower_bound(clock: &mut impl FnMut() -> Result<Now>) -> Result<Now> {
    let lower = clock()?;
    let observed = crate::CollectorServiceClockObservation::observe_local()?;
    if observed.now().0 < lower.0 { return Err(Error::InvalidTime); }
    Ok(observed.now())
}


fn guarded_observation(clock: &mut impl FnMut() -> Result<Now>)
    -> Result<crate::CollectorServiceClockObservation> {
    let lower = clock()?;
    let observed = crate::CollectorServiceClockObservation::observe_local()?;
    if observed.now().0 < lower.0 { return Err(Error::InvalidTime); }
    Ok(observed)
}
fn check_original_read<S: CompareAndSwap>(actor: &mut Actor<S>, permit: &CollectorServicePermit,
    end: Option<std::time::Instant>, clock: &mut impl FnMut() -> Result<Now>) -> Result<CollectorServiceDelegation> {
    match end {
        Some(end) => actor.check_collector_service_waiting(permit, fresh_lower_bound(clock)?, end,
            &mut || guarded_observation(clock)),
        None => actor.check_collector_service(permit, fresh_lower_bound(clock)?,
            &mut crate::CollectorServiceClockObservation::observe_local),
    }
}
fn snapshot_original_read<S: CompareAndSwap>(actor: &mut Actor<S>,
    end: Option<std::time::Instant>, clock: &mut impl FnMut() -> Result<Now>) -> Result<crate::State> {
    match end {
        Some(end) => actor.snapshot_collector_service_waiting(fresh_lower_bound(clock)?, end,
            &mut || guarded_observation(clock)),
        None => actor.snapshot(),
    }
}
fn borrow_original_read<'a, S: CompareAndSwap>(actor: &'a mut Actor<S>, permit: &'a CollectorServicePermit,
    end: Option<std::time::Instant>, clock: &mut impl FnMut() -> Result<Now>) -> Result<CollectorServiceAccess<'a, S>> {
    match end {
        Some(end) => actor.borrow_collector_service_waiting(permit, fresh_lower_bound(clock)?, end,
            &mut || guarded_observation(clock)),
        None => actor.borrow_collector_service(permit, fresh_lower_bound(clock)?,
            &mut crate::CollectorServiceClockObservation::observe_local),
    }
}
fn recheck_original_read<S: CompareAndSwap>(access: &mut CollectorServiceAccess<'_, S>, lower: Now,
    end: Option<std::time::Instant>, clock: &mut impl FnMut() -> Result<Now>) -> Result<Now> {
    match end {
        Some(end) => access.recheck_waiting(lower, end, &mut || guarded_observation(clock)),
        None => access.recheck(lower, &mut crate::CollectorServiceClockObservation::observe_local),
    }
}
fn checked_original<S: CompareAndSwap>(
    actor: &mut Actor<S>, permit: &CollectorServicePermit,
    clock: &mut impl FnMut() -> Result<Now>,
) -> Result<(CollectorServiceDelegation, Certificate, OwnerPin)> {
    checked_original_read_phase(actor, permit, None, clock)
}
fn checked_original_read_phase<S: CompareAndSwap>(
    actor: &mut Actor<S>, permit: &CollectorServicePermit,
    read_end: Option<std::time::Instant>, clock: &mut impl FnMut() -> Result<Now>,
) -> Result<(CollectorServiceDelegation, Certificate, OwnerPin)> {
    let policy = check_original_read(actor, permit, read_end, clock)?;
    if policy.credential_targets.is_empty() { return Err(Error::CapabilityDenied); }
    let state = snapshot_original_read(actor, read_end, clock)?;
    let certificate = state.certificate(policy.agent_id).ok_or(Error::NotFound)?.clone();
    let pin = state.owner().clone();
    if state.revoked(policy.agent_id) || policy.certificate_hash != certificate.digest()?
        || policy.header.context != pin.context || policy.header.owner_epoch != pin.epoch
        || policy.header.checkpoint != pin.checkpoint || permit.delegation_hash() != policy.digest()?
        || permit.service_id() != policy.service_id
    { return Err(Error::WrongClaim); }
    let after = check_original_read(actor, permit, read_end, clock)?;
    if after != policy || snapshot_original_read(actor, read_end, clock)?.revision() != state.revision() {
        return Err(Error::Conflict);
    }
    Ok((policy, certificate, pin))
}
fn binding(policy: &CollectorServiceDelegation, pin: &OwnerPin, id: ResourceId) -> Result<recipient::Binding> {
    Ok(recipient::Binding { agent_id: policy.agent_id, key_epoch: policy.key_epoch,
        certificate_hash: policy.certificate_hash, owner_identity: recipient::Binding::owner_identity(pin)?,
        recipient_id: id })
}

impl CollectorCredentialRecipient {
    pub(crate) fn create_original<S: CompareAndSwap>(
        actor: &mut Actor<S>, permit: &CollectorServicePermit, id: ResourceId,
        clock: &mut impl FnMut() -> Result<Now>,
    ) -> Result<Self> {
        Self::create_original_read_phase(actor, permit, id, None, clock)
    }
    pub(crate) fn create_original_waiting<S: CompareAndSwap>(
        actor: &mut Actor<S>, permit: &CollectorServicePermit, id: ResourceId,
        end: std::time::Instant, clock: &mut impl FnMut() -> Result<Now>,
    ) -> Result<Self> {
        Self::create_original_read_phase(actor, permit, id, Some(end), clock)
    }
    fn create_original_read_phase<S: CompareAndSwap>(
        actor: &mut Actor<S>, permit: &CollectorServicePermit, id: ResourceId,
        read_end: Option<std::time::Instant>, clock: &mut impl FnMut() -> Result<Now>,
    ) -> Result<Self> {
        let (policy, _, pin) = checked_original_read_phase(actor, permit, read_end, clock)?;
        let inner = recipient::Recipient::create(binding(&policy, &pin, id)?)?;
        // Mutation already happened: loss/change after creation is unknown.
        let mut result = Self { inner };
        let (after, _, after_pin) = checked_original_read_phase(actor, permit, read_end, clock).map_err(|_| Error::UncertainCommit)?;
        let after_binding = binding(&after, &after_pin, id).map_err(|_| Error::UncertainCommit)?;
        result.inner.check_binding(&after_binding).map_err(|_| Error::UncertainCommit)?;
        if after != policy { return Err(Error::UncertainCommit); }
        Ok(result)
    }
    pub(crate) fn open_existing_original<S: CompareAndSwap>(
        actor: &mut Actor<S>, permit: &CollectorServicePermit, id: ResourceId,
        clock: &mut impl FnMut() -> Result<Now>,
    ) -> Result<Self> {
        Self::open_existing_original_read_phase(actor, permit, id, None, clock)
    }
    pub(crate) fn open_existing_original_waiting<S: CompareAndSwap>(
        actor: &mut Actor<S>, permit: &CollectorServicePermit, id: ResourceId,
        end: std::time::Instant, clock: &mut impl FnMut() -> Result<Now>,
    ) -> Result<Self> {
        Self::open_existing_original_read_phase(actor, permit, id, Some(end), clock)
    }
    fn open_existing_original_read_phase<S: CompareAndSwap>(
        actor: &mut Actor<S>, permit: &CollectorServicePermit, id: ResourceId,
        read_end: Option<std::time::Instant>, clock: &mut impl FnMut() -> Result<Now>,
    ) -> Result<Self> {
        let (policy, _, pin) = checked_original_read_phase(actor, permit, read_end, clock)?;
        let inner = recipient::Recipient::open_existing(binding(&policy, &pin, id)?)?;
        let mut result = Self { inner };
        let (after, _, after_pin) = checked_original_read_phase(actor, permit, read_end, clock)?;
        result.inner.check_binding(&binding(&after, &after_pin, id)?)?;
        if after != policy { return Err(Error::Conflict); }
        Ok(result)
    }
    pub(crate) fn prepare_attestation<'a, S: CompareAndSwap>(
        &'a mut self, actor: &'a mut Actor<S>, permit: &'a CollectorServicePermit, challenge: Nonce,
        clock: &mut impl FnMut() -> Result<Now>,
    ) -> Result<PreparedRecipientAttestation<'a, S>> {
        self.prepare_attestation_read_phase(actor, permit, challenge, None, clock)
    }
    pub(crate) fn prepare_attestation_waiting<'a, S: CompareAndSwap>(
        &'a mut self, actor: &'a mut Actor<S>, permit: &'a CollectorServicePermit, challenge: Nonce,
        end: std::time::Instant, clock: &mut impl FnMut() -> Result<Now>,
    ) -> Result<PreparedRecipientAttestation<'a, S>> {
        self.prepare_attestation_read_phase(actor, permit, challenge, Some(end), clock)
    }
    fn prepare_attestation_read_phase<'a, S: CompareAndSwap>(
        &'a mut self, actor: &'a mut Actor<S>, permit: &'a CollectorServicePermit, challenge: Nonce,
        read_end: Option<std::time::Instant>, clock: &mut impl FnMut() -> Result<Now>,
    ) -> Result<PreparedRecipientAttestation<'a, S>> {
        let (policy, certificate, pin) = checked_original_read_phase(actor, permit, read_end, clock)?;
        self.inner.check_binding(&binding(&policy, &pin, self.inner.id())?)?;
        let recipient_public_key = Hash::from_bytes(self.inner.public()?);
        let mut access = borrow_original_read(actor, permit, read_end, clock)?;
        if access.policy() != &policy { return Err(Error::Conflict); }
        let now = recheck_original_read(&mut access, clock()?, read_end, clock)?;
        let body = CollectorRecipientAttestation {
            header: policy.header.clone(), agent_id: policy.agent_id, key_epoch: policy.key_epoch,
            certificate_hash: policy.certificate_hash, service_id: policy.service_id,
            base_delegation_hash: permit.delegation_hash(), manifest_sha256: policy.manifest_sha256,
            original_start_nonce: permit.start_id(),
            recipient_id: self.inner.id(), recipient_public_key,
            recipient_key_epoch: 1, recipient_certificate_epoch: 1, challenge, issued_at: now.0,
            expires_at: now.0.checked_add(PROOF_TTL)?.min(policy.expires_at)
                .min(certificate.expires_at).min(pin.expires_at),
        };
        body.validate()?;
        self.inner.check()?;
        recheck_original_read(&mut access, clock()?, read_end, clock)?;
        Ok(PreparedRecipientAttestation { body, policy, certificate, pin, recipient: &mut self.inner, access, read_wait_end: read_end })
    }

    /// Returns a private scoped plaintext holder, never JSON or a launch receipt.
    pub(crate) fn open_original<'a, S: CompareAndSwap>(
        &mut self, actor: &mut Actor<S>, permit: &'a CollectorServicePermit,
        scope: &Signed<CollectorCredentialScope>, proof: &Signed<CollectorRecipientAttestation>,
        capsule: &cc_crypto_core::ServiceCredentialCapsule, installed_catalog: &serde_json::Value,
        clock: &mut impl FnMut() -> Result<Now>,
    ) -> Result<OpenedCollectorCredential<'a>> {
        self.open_original_read_phase(actor, permit, scope, proof, capsule, installed_catalog, None, clock)
    }
    pub(crate) fn open_original_waiting<'a, S: CompareAndSwap>(
        &mut self, actor: &mut Actor<S>, permit: &'a CollectorServicePermit,
        scope: &Signed<CollectorCredentialScope>, proof: &Signed<CollectorRecipientAttestation>,
        capsule: &cc_crypto_core::ServiceCredentialCapsule, installed_catalog: &serde_json::Value,
        end: std::time::Instant, clock: &mut impl FnMut() -> Result<Now>,
    ) -> Result<OpenedCollectorCredential<'a>> {
        self.open_original_read_phase(actor, permit, scope, proof, capsule, installed_catalog, Some(end), clock)
    }
    fn open_original_read_phase<'a, S: CompareAndSwap>(
        &mut self, actor: &mut Actor<S>, permit: &'a CollectorServicePermit,
        scope: &Signed<CollectorCredentialScope>, proof: &Signed<CollectorRecipientAttestation>,
        capsule: &cc_crypto_core::ServiceCredentialCapsule,
        installed_catalog: &serde_json::Value, read_end: Option<std::time::Instant>, clock: &mut impl FnMut() -> Result<Now>,
    ) -> Result<OpenedCollectorCredential<'a>> {
        if capsule.to_json().map_err(|_| Error::InvalidWire)?.len() > 8192 { return Err(Error::Quota); }
        let (policy, _, pin) = checked_original_read_phase(actor, permit, read_end, clock)?;
        self.inner.check_binding(&binding(&policy, &pin, self.inner.id())?)?;
        proof.verify(policy.recipient)?;
        proof.body.validate()?;
        scope.body.check_base_and_recipient(&policy, &proof.body)?;
        let now = fresh_lower_bound(clock)?;
        if proof.body.original_start_nonce != permit.start_id()
            || scope.body.original_start_nonce != permit.start_id()
            || proof.body.recipient_id != self.inner.id()
            || proof.body.recipient_public_key.as_bytes() != &self.inner.public()?
            || now.0 < proof.body.issued_at || now.0 >= proof.body.expires_at
        { return Err(Error::WrongClaim); }
        scope.body.check_catalog(installed_catalog)?;
        // Agent and Native retain their existing separate wire crates. Only
        // these closed public DTOs cross the exact canonical-signature boundary.
        let native_scope = native_scope_metadata(scope)?;
        let native_pin = native_pin_metadata(&pin)?;
        let native_base = native_policy_metadata(&policy)?;
        let native_hash = cc_agent_node_wire::Hash::from_bytes(*permit.delegation_hash().as_bytes());
        let id = serde_json::to_value(self.inner.id()).map_err(|_| Error::InvalidWire)?;
        let native_id = cc_agent_node_wire::ResourceId::new(id.as_str().ok_or(Error::InvalidWire)?)
            .map_err(|_| Error::InvalidWire)?;
        let material_expires_at = crate::Timestamp::new(capsule.access_expires_at().map_err(|_| Error::InvalidWire)?)?;
        let mut access = borrow_original_read(actor, permit, read_end, clock)?;
        if access.policy() != &policy { return Err(Error::Conflict); }
        access.narrow_expiry(scope.body.expires_at.min(material_expires_at))?;
        recheck_original_read(&mut access, clock()?, read_end, clock)?;
        let material = self.inner.with_keys(|keys| {
            let now = recheck_original_read(&mut access, clock()?, read_end, clock)?;
            let native_now = cc_agent_node_wire::Now(cc_agent_node_wire::Timestamp::new(now.0.seconds())
                .map_err(|_| Error::InvalidTime)?);
            cc_crypto_core::open_collector_credential(&native_scope, capsule, &native_pin, &native_base,
                native_hash, installed_catalog, native_id, keys, native_now)
                .map_err(|_| Error::WrongClaim)
        })?;
        let after = recheck_original_read(&mut access, clock()?, read_end, clock)?;
        if after.0 < proof.body.issued_at || after.0 >= proof.body.expires_at { return Err(Error::Expired); }
        let native_now = cc_agent_node_wire::Now(cc_agent_node_wire::Timestamp::new(after.0.seconds())
            .map_err(|_| Error::InvalidTime)?);
        let expected = cc_crypto_core::collector_credential_binding(&native_scope, &native_pin, native_now)
            .map_err(|_| Error::WrongClaim)?;
        let final_now = recheck_original_read(&mut access, clock()?, read_end, clock)?;
        if final_now.0 < proof.body.issued_at || final_now.0 >= proof.body.expires_at { return Err(Error::Expired); }
        Ok(OpenedCollectorCredential { material, expected, scope: scope.clone(), permit, material_expires_at })
    }
}

// Three exact public metadata codecs, never a generic authority conversion.
// Actor, Permit, Access and private holders have no Serde implementation and
// never cross these functions. Signing transcript/digest parity is checked.
fn native_scope_metadata(value: &Signed<CollectorCredentialScope>)
    -> Result<cc_agent_node_wire::Signed<cc_agent_node_wire::CollectorCredentialScope>> {
    use cc_agent_node_wire::Statement as NativeStatement;
    let bytes = serde_json::to_vec(value).map_err(|_| Error::InvalidWire)?;
    if bytes.len() > crate::MAX_WIRE_BYTES { return Err(Error::Quota); }
    let converted: cc_agent_node_wire::Signed<cc_agent_node_wire::CollectorCredentialScope> =
        serde_json::from_slice(&bytes).map_err(|_| Error::InvalidWire)?;
    if value.body.signing_bytes()? != converted.body.signing_bytes().map_err(|_| Error::InvalidWire)?
        || value.signature.as_bytes() != converted.signature.as_bytes() { return Err(Error::WrongClaim); }
    Ok(converted)
}
fn native_policy_metadata(value: &CollectorServiceDelegation)
    -> Result<cc_agent_node_wire::CollectorServiceDelegation> {
    use cc_agent_node_wire::Statement as NativeStatement;
    let bytes = serde_json::to_vec(value).map_err(|_| Error::InvalidWire)?;
    if bytes.len() > crate::MAX_WIRE_BYTES { return Err(Error::Quota); }
    let converted: cc_agent_node_wire::CollectorServiceDelegation = serde_json::from_slice(&bytes).map_err(|_| Error::InvalidWire)?;
    if value.digest()?.as_bytes() != converted.digest().map_err(|_| Error::InvalidWire)?.as_bytes() {
        return Err(Error::WrongClaim);
    }
    Ok(converted)
}
fn native_pin_metadata(value: &OwnerPin) -> Result<cc_agent_node_wire::OwnerPin> {
    let original = serde_json::to_value(value).map_err(|_| Error::InvalidWire)?;
    let bytes = serde_json::to_vec(&original).map_err(|_| Error::InvalidWire)?;
    if bytes.len() > crate::MAX_WIRE_BYTES { return Err(Error::Quota); }
    let converted: cc_agent_node_wire::OwnerPin = serde_json::from_slice(&bytes).map_err(|_| Error::InvalidWire)?;
    if serde_json::to_value(&converted).map_err(|_| Error::InvalidWire)? != original { return Err(Error::WrongClaim); }
    Ok(converted)
}
/// Fields stay inside this module. The fixed-file projection will recheck the
/// SAME Permit/current catalog before any secret write. It is not a caller grant.
pub(crate) struct OpenedCollectorCredential<'a> {
    material: cc_crypto_core::OpenedServiceCredential,
    expected: cc_crypto_core::ServiceCredentialBinding,
    scope: Signed<CollectorCredentialScope>,
    permit: &'a CollectorServicePermit,
    material_expires_at: crate::Timestamp,
}
/// Private fields and no Serde/Clone. Only actual checked Permit + held private
/// recipient can prepare this exact purpose for the Node custody signer.
pub(crate) struct PreparedRecipientAttestation<'a, S: CompareAndSwap> {
    body: CollectorRecipientAttestation,
    policy: CollectorServiceDelegation,
    certificate: Certificate,
    pin: OwnerPin,
    recipient: &'a mut recipient::Recipient,
    access: CollectorServiceAccess<'a, S>,
    read_wait_end: Option<std::time::Instant>,
}
impl<S: CompareAndSwap> PreparedRecipientAttestation<'_, S> {
    pub(crate) fn original(&self) -> (CollectorRecipientAttestation, CollectorServiceDelegation, Certificate, OwnerPin) {
        (self.body.clone(), self.policy.clone(), self.certificate.clone(), self.pin.clone())
    }
    pub(crate) fn check_recipient(&mut self) -> Result<()> {
        let expected = binding(&self.policy, &self.pin, self.body.recipient_id)?;
        self.recipient.check_binding(&expected)?;
        if self.recipient.public()? != *self.body.recipient_public_key.as_bytes() { return Err(Error::WrongClaim); }
        Ok(())
    }
    pub(crate) fn check_original(&mut self, clock: &mut impl FnMut() -> Result<Now>) -> Result<Now> {
        let now = recheck_original_read(&mut self.access, clock()?, self.read_wait_end, clock)?;
        if self.access.policy() != &self.policy { return Err(Error::Conflict); }
        if now.0 < self.body.issued_at || now.0 >= self.body.expires_at { return Err(Error::Expired); }
        self.check_recipient()?;
        Ok(now)
    }
    pub(crate) fn finish(mut self, signed: Signed<CollectorRecipientAttestation>,
        clock: &mut impl FnMut() -> Result<Now>) -> Result<Signed<CollectorRecipientAttestation>> {
        if signed.body != self.body { return Err(Error::WrongClaim); }
        signed.verify(self.policy.recipient)?;
        self.check_original(clock)?;
        Ok(signed)
    }
}

// The fixed control worker must recheck SAME Permit and actual catalog after
// the Node signature and before replying. No public/raw Node signer is exported.
pub(crate) fn sign_original_recipient<S: CompareAndSwap>(
    node_directory: &std::path::Path, prepared: PreparedRecipientAttestation<'_, S>,
    clock: &mut impl FnMut() -> Result<Now>,
) -> Result<Signed<CollectorRecipientAttestation>> {
    crate::provision::sign_collector_recipient_attestation(node_directory, prepared, clock)
}

mod service_lifetime;
pub(crate) use service_lifetime::{OpenedLifetimeCollectorCredentials,RegisteredLifetimeCollectorCredential};
