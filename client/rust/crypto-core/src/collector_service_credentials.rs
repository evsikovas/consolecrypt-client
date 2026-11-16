//! Closed finite service credential purpose; existing capsule v1 crypto only.
//! No I/O, vault lookup, Actor/Permit, launch, recovery or cleanup authority.
use crate::{DeviceSecretKeys, OpenedServiceCredential, ServiceCredentialBinding,
    ServiceCredentialCapsule, ServiceCredentialKind, ServiceTenantKind};
use cc_agent_node_wire::{Certificate, CollectorServiceClaim, CollectorServiceClockStamp,
    CollectorServiceCredentialAuthorization, CollectorServiceDelegation, Error, Hash,
    Nonce, Now, OwnerPin, PublicKey, ResourceId, Result, SignatureBytes, Signed, Statement,
    Tenant, Timestamp, PROOF_TTL, verify_collector_recipient, verify_statement};
use serde::Serialize;
use sha2::{Digest, Sha256};
#[path = "collector_service_credential_frames.rs"]
mod frames;
pub use frames::CollectorServiceCredentialFrame;

fn text(value: &impl Serialize) -> Result<String> {
    serde_json::to_value(value).map_err(|_| Error::InvalidWire)?.as_str()
        .map(str::to_owned).ok_or(Error::InvalidWire)
}
fn hex(bytes: &[u8]) -> String { bytes.iter().map(|b| format!("{b:02x}")).collect() }

/// Not consent. Only the bounded, explicitly confirmed Native flow may sign it.
pub struct PreparedCollectorServiceCredentialOwner {
    key: PublicKey,
    until: Timestamp,
    authorization: CollectorServiceCredentialAuthorization,
}
impl PreparedCollectorServiceCredentialOwner {
    pub fn owner_device_id(&self) -> ResourceId { self.authorization.owner_device_id }
    pub fn prepare(
        pin: &OwnerPin, certificate: &Signed<Certificate>,
        original_base: &Signed<CollectorServiceDelegation>,
        original_admission: &Signed<CollectorServiceClaim>,
        authorization: CollectorServiceCredentialAuthorization,
        challenge: Nonce, catalog: &serde_json::Value, now: Now,
    ) -> Result<Self> {
        authorization.verify_initial_binding(pin)?;
        if authorization.base.body != original_base.body
            || authorization.base.signature.as_bytes() != original_base.signature.as_bytes()
            || authorization.admission.body != original_admission.body
            || authorization.admission.signature.as_bytes() != original_admission.signature.as_bytes()
            || now.0 < authorization.issued_at
            || now.0.seconds() - authorization.issued_at.seconds() >= PROOF_TTL
        { return Err(Error::WrongClaim); }
        verify_collector_recipient(pin, certificate, original_base,
            &authorization.recipient, challenge, now)?;
        authorization.check_original(&original_base.body, &original_admission.body.clock, catalog, now)?;
        let until = authorization.issued_at.checked_add(PROOF_TTL)?
            .min(authorization.recipient.body.expires_at).min(authorization.expires_at)
            .min(certificate.body.expires_at).min(pin.expires_at);
        Ok(Self { key: pin.key, until, authorization })
    }
}
impl DeviceSecretKeys {
    pub fn sign_collector_service_credential_authorization_v1(
        &self, prepared: PreparedCollectorServiceCredentialOwner, now: Now,
    ) -> Result<Signed<CollectorServiceCredentialAuthorization>> {
        if self.public_keys().signing != *prepared.key.as_bytes()
            || now.0 < prepared.authorization.issued_at || now.0 >= prepared.until
        { return Err(Error::WrongClaim); }
        let signature = SignatureBytes::from_bytes(self.sign_message(&prepared.authorization.signing_bytes()?));
        let signed = Signed { body: prepared.authorization, signature };
        verify_statement(&signed, prepared.key)?;
        Ok(signed)
    }
}

/// Exact binding for the existing explicit saved-credential issuer.
/// This function does not itself establish the user's separate consent.
pub fn collector_service_credential_binding(
    authorization: &Signed<CollectorServiceCredentialAuthorization>,
    pin: &OwnerPin, slot: usize, now: Now,
) -> Result<ServiceCredentialBinding> {
    verify_statement(authorization, pin.key)?;
    let a = &authorization.body;
    a.verify_initial_binding(pin)?;
    if now.0 < a.issued_at || now.0 >= a.expires_at { return Err(Error::Expired); }
    let reference = a.credentials.get(slot).ok_or(Error::Quota)?;
    let (tenant_kind, tenant_id) = match &pin.context.tenant {
        Tenant::LocalProfile { id } => (ServiceTenantKind::LocalProfile, text(id)?),
        Tenant::PersonalAccount { id } => (ServiceTenantKind::PersonalAccount, text(id)?),
        Tenant::Team { .. } => return Err(Error::UnsupportedTeam),
    };
    let b = &a.base.body;
    let r = &a.recipient.body;
    let binding = ServiceCredentialBinding {
        version: 1, owner_device_id: text(&a.owner_device_id)?,
        owner_signing_key_sha256: hex(&Sha256::digest(pin.key.as_bytes())),
        tenant_kind, tenant_id, service_id: text(&b.service_id)?,
        delegation_digest: hex(a.capsule_delegation_digest(slot)?.as_bytes()),
        policy_revision: b.policy_revision, revocation_epoch: b.revocation_epoch,
        credential_id: text(&reference.credential_id)?, credential_revision: reference.credential_revision,
        kind: ServiceCredentialKind::Password,
        recipient_key_sha256: hex(&Sha256::digest(r.recipient_public_key.as_bytes())),
        recipient_key_epoch: r.recipient_key_epoch,
        recipient_certificate_epoch: r.recipient_certificate_epoch,
        delegation_expires_at: a.expires_at.seconds(),
    };
    binding.validate().map_err(|_| Error::InvalidWire)?;
    Ok(binding)
}

/// Whole-set metadata check occurs before ANY decryption. This is not proof of
/// authenticated ciphertext or a substitute for the actual Node Access guard.
pub fn preflight_collector_service_credential_set(
    authorization: &Signed<CollectorServiceCredentialAuthorization>,
    capsules: &[ServiceCredentialCapsule], pin: &OwnerPin,
    original_base: &CollectorServiceDelegation, original_clock: &CollectorServiceClockStamp,
    catalog: &serde_json::Value, now: Now,
) -> Result<Vec<ServiceCredentialBinding>> {
    authorization.body.check_original(original_base, original_clock, catalog, now)?;
    if capsules.len() != authorization.body.credentials.len() || capsules.is_empty() || capsules.len() > 2 {
        return Err(Error::WrongClaim);
    }
    let mut bindings = Vec::with_capacity(capsules.len());
    for (slot, capsule) in capsules.iter().enumerate() {
        let expected = collector_service_credential_binding(authorization, pin, slot, now)?;
        let raw = capsule.to_json().map_err(|_| Error::InvalidWire)?;
        if raw.len() > 8192 { return Err(Error::Quota); }
        let value: serde_json::Value = serde_json::from_slice(&raw).map_err(|_| Error::InvalidWire)?;
        let actual: ServiceCredentialBinding = serde_json::from_value(value.get("binding")
            .ok_or(Error::InvalidWire)?.clone()).map_err(|_| Error::InvalidWire)?;
        let expiry = capsule.access_expires_at().map_err(|_| Error::InvalidWire)?;
        let issued = value.get("issuedAt").and_then(|v| v.as_u64()).ok_or(Error::InvalidTime)?;
        if actual != expected || expiry > authorization.body.expires_at.seconds()
            || issued < authorization.body.issued_at.seconds() || issued > now.0.seconds()
            || expiry <= issued || now.0.seconds() >= expiry { return Err(Error::WrongClaim); }
        bindings.push(expected);
    }
    Ok(bindings)
}

pub fn open_collector_service_credential_set(
    authorization: &Signed<CollectorServiceCredentialAuthorization>,
    capsules: &[ServiceCredentialCapsule], pin: &OwnerPin,
    original_base: &CollectorServiceDelegation, original_clock: &CollectorServiceClockStamp,
    catalog: &serde_json::Value, recipient_id: ResourceId, recipient: &DeviceSecretKeys, now: Now,
) -> Result<Vec<OpenedServiceCredential>> {
    let bindings = preflight_collector_service_credential_set(authorization, capsules,
        pin, original_base, original_clock, catalog, now)?;
    let proof = &authorization.body.recipient.body;
    if proof.recipient_id != recipient_id
        || proof.recipient_public_key != Hash::from_bytes(recipient.public_keys().encryption)
    { return Err(Error::WrongClaim); }
    let mut opened = Vec::with_capacity(capsules.len());
    for (capsule, expected) in capsules.iter().zip(&bindings) {
        let material = crate::open_service_credential(capsule, expected, pin.key.as_bytes(), recipient,
            now.0.seconds()).map_err(|_| Error::WrongClaim)?;
        let supported = material.with_secret(expected, now.0.seconds(),
            crate::collector_basic_password_supported).map_err(|_| Error::WrongClaim)?;
        if !supported { return Err(Error::InvalidWire); }
        opened.push(material);
    }
    Ok(opened)
}

#[cfg(test)]
#[path = "collector_service_credentials_tests.rs"]
mod tests;
