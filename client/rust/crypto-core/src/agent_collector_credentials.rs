//! Collector-specific use of the EXISTING service credential capsule v1.
//! This module has no secret resolver, transport, file I/O or launch authority.
#[path = "agent_collector_bundle.rs"]
mod bundle;
pub use bundle::{CollectorCredentialBundle, CollectorCredentialEnvelope};
use crate::{DeviceSecretKeys, OpenedServiceCredential, ServiceCredentialBinding,
    ServiceCredentialCapsule, ServiceCredentialKind, ServiceTenantKind};
use cc_agent_node_wire::{Certificate, CollectorCredentialScope, CollectorRecipientAttestation,
    CollectorServiceDelegation, Error, Hash, Nonce, Now, OwnerPin, PublicKey, Result,
    SignatureBytes, Signed, Statement, Tenant, Timestamp, PROOF_TTL, verify_collector_recipient};
use serde::Serialize;
use sha2::{Digest, Sha256};

fn text_id(value: &impl Serialize) -> Result<String> {
    serde_json::to_value(value).map_err(|_| Error::InvalidWire)?
        .as_str().map(str::to_owned).ok_or(Error::InvalidWire)
}
fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}
fn live(start: Timestamp, end: Timestamp, now: Now) -> Result<()> {
    if now.0 < start || now.0 >= end { return Err(Error::Expired); }
    Ok(())
}

// Exact pinned Go1.27.4 unicode.IsSpace/White_Space set used by
// prometheus/common v0.72.1 FileSecret.Fetch -> strings.TrimSpace.
// Source/archive/module-h1 evidence lives beside this staged implementation.
fn pinned_go_space(c: char) -> bool {
    matches!(c, '\u{0009}'..='\u{000d}' | ' ' | '\u{0085}' | '\u{00a0}'
        | '\u{1680}' | '\u{2000}'..='\u{200a}' | '\u{2028}' | '\u{2029}'
        | '\u{202f}' | '\u{205f}' | '\u{3000}')
}
/// Closed collector password-file material policy. Validate before Native
/// capsule export AND before Node plaintext use; never normalize a secret.
pub fn collector_basic_password_supported(bytes: &[u8]) -> bool {
    if bytes.is_empty() || bytes.len() > 4096 || bytes.iter().any(|b| *b == 0 || *b == b'\r' || *b == b'\n') {
        return false;
    }
    let Ok(value) = std::str::from_utf8(bytes) else { return false; };
    !value.chars().next().is_some_and(pinned_go_space)
        && !value.chars().next_back().is_some_and(pinned_go_space)
}

/// Prepared data is not an approval. Native must retain its original owner,
/// HostPlan/ledger, catalog, credential metadata and bounded review fences.
pub struct PreparedCollectorCredentialOwner {
    key: PublicKey,
    until: Timestamp,
    scope: CollectorCredentialScope,
}
impl PreparedCollectorCredentialOwner {
    pub fn owner_device_id(&self) -> cc_agent_node_wire::ResourceId { self.scope.owner_device_id }

    pub fn prepare(
        pin: &OwnerPin, certificate: &Signed<Certificate>,
        base: &Signed<CollectorServiceDelegation>,
        recipient: &Signed<CollectorRecipientAttestation>, challenge: Nonce,
        scope: CollectorCredentialScope, catalog: &serde_json::Value, now: Now,
    ) -> Result<Self> {
        verify_collector_recipient(pin, certificate, base, recipient, challenge, now)?;
        base.body.check_credential_targets(catalog)?;
        scope.check_base_and_recipient(&base.body, &recipient.body)?;
        scope.check_catalog(catalog)?;
        live(scope.issued_at, scope.expires_at, now)?;
        if scope.issued_at > now.0 || now.0.seconds() - scope.issued_at.seconds() >= PROOF_TTL {
            return Err(Error::Expired);
        }
        let until = scope.issued_at.checked_add(PROOF_TTL)?
            .min(recipient.body.expires_at).min(scope.expires_at).min(pin.expires_at);
        Ok(Self { key: pin.key, until, scope })
    }
}
impl DeviceSecretKeys {
    /// Only the closed typed scope can reach the private Device signer.
    pub fn sign_agent_collector_credential_scope_v1(
        &self, prepared: PreparedCollectorCredentialOwner, now: Now,
    ) -> Result<Signed<CollectorCredentialScope>> {
        live(prepared.scope.issued_at, prepared.until, now)?;
        if prepared.key.as_bytes() != &self.public_keys().signing { return Err(Error::WrongClaim); }
        let signature = SignatureBytes::from_bytes(self.sign_message(&prepared.scope.signing_bytes()?));
        let signed = Signed { body: prepared.scope, signature };
        cc_agent_node_wire::verify_statement(&signed, prepared.key)?;
        Ok(signed)
    }
}

/// Exact v1 capsule AAD projection of the separately signed narrow scope.
/// Its delegation digest is NOT substituted for the base Actor delegation.
pub fn collector_credential_binding(
    scope: &Signed<CollectorCredentialScope>, pin: &OwnerPin, now: Now,
) -> Result<ServiceCredentialBinding> {
    cc_agent_node_wire::verify_statement(scope, pin.key)?;
    let s = &scope.body;
    if s.header.context != pin.context || s.header.owner_epoch != pin.epoch
        || s.header.checkpoint != pin.checkpoint || s.expires_at > pin.expires_at
    { return Err(Error::WrongContext); }
    live(s.issued_at, s.expires_at, now)?;
    let (tenant_kind, tenant_id) = match &pin.context.tenant {
        Tenant::LocalProfile { id } => (ServiceTenantKind::LocalProfile, text_id(id)?),
        Tenant::PersonalAccount { id } => (ServiceTenantKind::PersonalAccount, text_id(id)?),
        Tenant::Team { .. } => return Err(Error::UnsupportedTeam),
    };
    let binding = ServiceCredentialBinding {
        version: 1,
        owner_device_id: text_id(&s.owner_device_id)?,
        // Capsule v1 requires raw SHA256, not Node PublicKey::fingerprint().
        owner_signing_key_sha256: hex(&Sha256::digest(pin.key.as_bytes())),
        tenant_kind, tenant_id,
        service_id: text_id(&s.service_id)?,
        delegation_digest: hex(s.digest()?.as_bytes()),
        policy_revision: s.policy_revision,
        revocation_epoch: s.revocation_epoch,
        credential_id: text_id(&s.credential_id)?,
        credential_revision: s.credential_revision,
        kind: ServiceCredentialKind::Password,
        recipient_key_sha256: hex(s.recipient_key_sha256.as_bytes()),
        recipient_key_epoch: s.recipient_key_epoch,
        recipient_certificate_epoch: s.recipient_certificate_epoch,
        delegation_expires_at: s.expires_at.seconds(),
    };
    binding.validate().map_err(|_| Error::InvalidWire)?;
    Ok(binding)
}

/// The caller MUST independently hold/check the actual original Actor Permit
/// and installed catalog before this function and again before material use.
/// No bool, service ID, policy clone or successfully opened capsule is a Permit.
pub fn open_collector_credential(
    scope: &Signed<CollectorCredentialScope>, capsule: &ServiceCredentialCapsule,
    pin: &OwnerPin, original_base: &CollectorServiceDelegation,
    original_base_hash: Hash, catalog: &serde_json::Value,
    recipient_id: cc_agent_node_wire::ResourceId, recipient: &DeviceSecretKeys, now: Now,
) -> Result<OpenedServiceCredential> {
    let s = &scope.body;
    let public = recipient.public_keys().encryption;
    if original_base.digest()? != original_base_hash || s.base_delegation_hash != original_base_hash
        || !original_base.credential_targets.contains(&s.target)
        || s.header != original_base.header || s.service_id != original_base.service_id
        || s.agent_id != original_base.agent_id || s.key_epoch != original_base.key_epoch
        || s.certificate_hash != original_base.certificate_hash
        || s.policy_revision != original_base.policy_revision || s.revocation_epoch != original_base.revocation_epoch
        || s.collector_id != original_base.collector_id || s.manifest_sha256 != original_base.manifest_sha256
        || s.expires_at > original_base.expires_at || s.issued_at < original_base.issued_at
        || s.recipient_id != recipient_id
        || s.recipient_key_sha256 != Hash::from_bytes(Sha256::digest(public).into())
    { return Err(Error::WrongClaim); }
    original_base.validate()?;
    original_base.check_credential_targets(catalog)?;
    live(original_base.issued_at, original_base.expires_at, now)?;
    s.check_catalog(catalog)?;
    let expected = collector_credential_binding(scope, pin, now)?;
    let opened = crate::open_service_credential(capsule, &expected, pin.key.as_bytes(), recipient, now.0.seconds())
        .map_err(|_| Error::WrongClaim)?;
    // Maintained password-file readers can trim surrounding whitespace. Refuse
    // such material rather than silently changing an explicitly selected value.
    let valid = opened.with_secret(&expected, now.0.seconds(), collector_basic_password_supported)
        .map_err(|_| Error::WrongClaim)?;
    if !valid { return Err(Error::InvalidWire); }
    Ok(opened)
}

#[cfg(test)]
#[path = "agent_collector_credentials_tests.rs"]
mod tests;
