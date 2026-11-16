//! Encrypted handoff data, never a service Permit or credential approval.
//! Transport MUST keep one envelope per original <=16KiB control frame.
//! Core chooses the two-stage original-supervisor lifecycle; this module does
//! not configure a service, persist data, create a socket or reset its deadline.
use super::*;
use cc_agent_node_wire::CollectorCredentialTarget;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

const MAX_CAPSULE_BYTES: usize = 8192;
// Reserve at least4096 bytes for the existing control envelope. The actual
// original outer parser MUST still enforce its unchanged full16KiB limit.
const MAX_ENVELOPE_BYTES: usize = 12 * 1024;

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CollectorCredentialEnvelope {
    version: u8,
    scope: Signed<CollectorCredentialScope>,
    capsule: ServiceCredentialCapsule,
}
impl std::fmt::Debug for CollectorCredentialEnvelope {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("CollectorCredentialEnvelope([redacted])")
    }
}
impl CollectorCredentialEnvelope {
    pub fn new(scope: Signed<CollectorCredentialScope>, capsule: ServiceCredentialCapsule) -> Result<Self> {
        let value = Self { version: 1, scope, capsule };
        value.to_json()?;
        Ok(value)
    }
    fn structure(&self) -> Result<()> {
        if self.version != 1 { return Err(Error::InvalidWire); }
        self.scope.body.validate()?;
        if self.capsule.to_json().map_err(|_| Error::InvalidWire)?.len() > MAX_CAPSULE_BYTES {
            return Err(Error::Quota);
        }
        Ok(())
    }
    pub fn from_json(raw: &[u8]) -> Result<Self> {
        if raw.is_empty() || raw.len() > MAX_ENVELOPE_BYTES { return Err(Error::Quota); }
        // Deserialize typed fields directly: duplicate/unknown fields are
        // refused recursively by the existing strict scope/capsule DTOs.
        let value: Self = serde_json::from_slice(raw).map_err(|_| Error::InvalidWire)?;
        value.structure()?;
        Ok(value)
    }
    pub fn to_json(&self) -> Result<Vec<u8>> {
        self.structure()?;
        let raw = serde_json::to_vec(self).map_err(|_| Error::InvalidWire)?;
        if raw.len() > MAX_ENVELOPE_BYTES { return Err(Error::Quota); }
        Ok(raw)
    }
    pub fn scope(&self) -> &Signed<CollectorCredentialScope> { &self.scope }
    pub fn into_parts(self) -> (Signed<CollectorCredentialScope>, ServiceCredentialCapsule) {
        (self.scope, self.capsule)
    }

    /// Verify the owner-signed scope against the ORIGINAL retained references
    /// and fresh Node recipient proof during bounded handoff. Capsule metadata
    /// equality is only preflight: its signature/AEAD and plaintext policy MUST
    /// still pass existing open_collector_credential with the actual private
    /// recipient under the SAME live Core Permit before material is used.
    /// This result grants no admission, readiness, persistence or cleanup.
    pub fn check_metadata_only(
        &self, pin: &OwnerPin, certificate: &Signed<Certificate>,
        base: &Signed<CollectorServiceDelegation>, proof: &Signed<CollectorRecipientAttestation>,
        challenge: Nonce, catalog: &serde_json::Value, now: Now,
    ) -> Result<()> {
        self.structure()?;
        verify_collector_recipient(pin, certificate, base, proof, challenge, now)?;
        self.scope.body.check_base_and_recipient(&base.body, &proof.body)?;
        self.scope.body.check_catalog(catalog)?;
        self.check_scope_and_capsule_metadata_only(pin, now)
    }

    /// Owner-signed scope plus public capsule binding/window preflight only.
    /// Node SSH uses original Actor metadata separately for certificate/base/
    /// current-start/catalog checks. Actual private-recipient open is required
    /// to authenticate capsule signature/AEAD and enforce the plaintext policy.
    pub fn check_scope_and_capsule_metadata_only(&self, pin: &OwnerPin, now: Now) -> Result<()> {
        self.structure()?;
        let expected = collector_credential_binding(&self.scope, pin, now)?;
        // Inspect only public metadata from the EXISTING v1 encoder. Do not
        // copy capsule signing/KDF/AEAD implementation or expose private fields.
        let raw = self.capsule.to_json().map_err(|_| Error::InvalidWire)?;
        let public: serde_json::Value = serde_json::from_slice(&raw).map_err(|_| Error::InvalidWire)?;
        if public.get("binding") != Some(&serde_json::to_value(expected).map_err(|_| Error::InvalidWire)?) {
            return Err(Error::WrongClaim);
        }
        let issued = public.get("issuedAt").and_then(serde_json::Value::as_u64).ok_or(Error::InvalidTime)?;
        let expires = self.capsule.access_expires_at().map_err(|_| Error::InvalidWire)?;
        if issued < self.scope.body.issued_at.seconds() || issued > now.0.seconds()
            || expires <= issued || now.0.seconds() >= expires
            || expires > self.scope.body.expires_at.seconds()
        { return Err(Error::Expired); }
        Ok(())
    }
}

/// Internal max2 aggregation of encrypted records, NOT a wire frame. There is
/// deliberately no Serde implementation: a transport must send/check EACH
/// envelope separately. The actual authenticated Core journal owns persistence.
pub struct CollectorCredentialBundle {
    envelopes: Vec<CollectorCredentialEnvelope>,
}
impl std::fmt::Debug for CollectorCredentialBundle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("CollectorCredentialBundle([redacted])")
    }
}
impl CollectorCredentialBundle {
    pub fn from_envelopes(envelopes: Vec<CollectorCredentialEnvelope>) -> Result<Self> {
        if envelopes.is_empty() || envelopes.len() > 2 { return Err(Error::Quota); }
        let (mut exporter, mut receiver) = (false, false);
        let mut hashes = BTreeSet::new();
        for envelope in &envelopes {
            envelope.to_json()?;
            if !hashes.insert(envelope.scope.body.digest()?) { return Err(Error::Replay); }
            let seen = match &envelope.scope.body.target {
                CollectorCredentialTarget::Scrape { .. } => &mut exporter,
                CollectorCredentialTarget::Receiver {} => &mut receiver,
            };
            if *seen { return Err(Error::Quota); }
            *seen = true;
        }
        Ok(Self { envelopes })
    }
    pub fn check_metadata_only(
        &self, pin: &OwnerPin, certificate: &Signed<Certificate>,
        base: &Signed<CollectorServiceDelegation>, proof: &Signed<CollectorRecipientAttestation>,
        challenge: Nonce, catalog: &serde_json::Value, now: Now,
    ) -> Result<()> {
        let actual = self.envelopes.iter().map(|item| item.scope.body.target.clone()).collect::<Vec<_>>();
        cc_agent_node_wire::check_collector_credential_target_set(&base.body, &actual, catalog)?;
        for envelope in &self.envelopes {
            envelope.check_metadata_only(pin, certificate, base, proof, challenge, catalog, now)?;
        }
        Ok(())
    }
    pub fn into_envelopes(self) -> Vec<CollectorCredentialEnvelope> { self.envelopes }
}
