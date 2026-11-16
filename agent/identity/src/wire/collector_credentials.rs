//! Closed collector credential metadata. Neither a service Permit nor URL authority.
use super::*;
use crate::OwnerPin;
use sha2::Digest as _;

const MAX_REVISION: u64 = 9_007_199_254_740_991;

/// Same bounded canonical manifest bytes as the installed catalog. Digest
/// computation alone does not validate or trust that catalog.
pub fn collector_credential_manifest_sha256(manifest: &serde_json::Value) -> Result<Hash> {
    let mut bytes = Vec::new();
    canonical_json(manifest, &mut bytes)?;
    if bytes.len() > 8192 { return Err(Error::Quota); }
    Ok(Hash::from_bytes(Sha256::digest(bytes).into()))
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", rename_all_fields = "camelCase", deny_unknown_fields)]
pub enum CollectorCredentialTarget {
    Scrape { target_id: String },
    Receiver {},
}

/// A computed public recipient authenticated by the ORIGINAL Node signing key.
/// This attestation alone is not independent X25519 proof of decryption.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CollectorRecipientAttestation {
    pub header: Header,
    pub agent_id: AgentId,
    pub key_epoch: u64,
    pub certificate_hash: Hash,
    pub service_id: ResourceId,
    pub base_delegation_hash: Hash,
    pub manifest_sha256: Hash,
    pub original_start_nonce: Nonce,
    pub recipient_id: ResourceId,
    pub recipient_public_key: Hash,
    pub recipient_key_epoch: u64,
    pub recipient_certificate_epoch: u64,
    pub challenge: Nonce,
    pub issued_at: Timestamp,
    pub expires_at: Timestamp,
}
impl sealed::Sealed for CollectorRecipientAttestation {}
impl Statement for CollectorRecipientAttestation {
    const DOMAIN: &'static str = "metrics-collector-credential-recipient-v1";
    fn validate(&self) -> Result<()> {
        self.header.validate()?;
        self.header.context.personal()?;
        self.challenge.validate()?;
        self.original_start_nonce.validate()?;
        if self.key_epoch == 0 || self.recipient_key_epoch != 1
            || self.recipient_certificate_epoch != 1
            || self.recipient_public_key.as_bytes() == &[0; 32]
        { return Err(Error::InvalidWire); }
        window(self.issued_at, self.expires_at, PROOF_TTL)
    }
}

/// A separate, narrow credential delegation. Its digest is capsule v1's
/// delegation_digest. The base service delegation remains separately bound.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CollectorCredentialScope {
    pub header: Header,
    pub scope_id: ResourceId,
    pub owner_device_id: ResourceId,
    pub service_id: ResourceId,
    pub base_delegation_hash: Hash,
    pub original_start_nonce: Nonce,
    pub recipient_attestation_digest: Hash,
    pub recipient_challenge: Nonce,
    pub policy_revision: u64,
    pub revocation_epoch: u64,
    pub agent_id: AgentId,
    pub key_epoch: u64,
    pub certificate_hash: Hash,
    pub collector_id: ResourceId,
    pub manifest_sha256: Hash,
    pub target: CollectorCredentialTarget,
    pub endpoint_sha256: Hash,
    pub username: String,
    pub credential_id: ResourceId,
    pub credential_revision: u64,
    pub recipient_id: ResourceId,
    pub recipient_key_sha256: Hash,
    pub recipient_key_epoch: u64,
    pub recipient_certificate_epoch: u64,
    pub issued_at: Timestamp,
    pub expires_at: Timestamp,
}
impl std::fmt::Debug for CollectorCredentialScope {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CollectorCredentialScope").finish_non_exhaustive()
    }
}
impl sealed::Sealed for CollectorCredentialScope {}
impl Statement for CollectorCredentialScope {
    const DOMAIN: &'static str = "metrics-collector-credential-scope-v1";
    fn validate(&self) -> Result<()> {
        self.header.validate()?;
        self.header.context.personal()?;
        self.original_start_nonce.validate()?;
        self.recipient_challenge.validate()?;
        if self.key_epoch == 0 || self.recipient_key_epoch != 1
            || self.recipient_certificate_epoch != 1
            || [self.policy_revision, self.revocation_epoch, self.credential_revision]
                .iter().any(|n| !(1..=MAX_REVISION).contains(n))
            || self.collector_id != collector_service_resource(self.manifest_sha256)?
            || self.username.is_empty() || self.username.len() > 128
            || self.username.chars().any(|c| c.is_control() || c == ':')
        { return Err(Error::InvalidWire); }
        if let CollectorCredentialTarget::Scrape { target_id } = &self.target {
            let b = target_id.as_bytes();
            if b.is_empty() || b.len() > 32 || !b[0].is_ascii_alphabetic()
                || !b.iter().all(|c| c.is_ascii_alphanumeric() || b"_-".contains(c))
            { return Err(Error::InvalidWire); }
        }
        window(self.issued_at, self.expires_at, COLLECTOR_SERVICE_TTL)
    }
}

fn credential_live(start: Timestamp, end: Timestamp, now: Now) -> Result<()> {
    now.0.valid()?;
    if now.0 < start || now.0 >= end { return Err(Error::Expired); }
    Ok(())
}

/// Exact target-set completeness, not service admission. The original signed
/// base includes the explicit target intent; [] is PUBLIC ONLY, never auto.
/// Call before any secret write and retain actual Actor/Permit/catalog checks.
pub fn check_collector_credential_target_set(base: &CollectorServiceDelegation,
    actual: &[CollectorCredentialTarget], manifest: &serde_json::Value) -> Result<()> {
    base.check_credential_targets(manifest)?;
    if collector_credential_manifest_sha256(manifest)? != base.manifest_sha256 {
        return Err(Error::WrongClaim);
    }
    if actual.len() != base.credential_targets.len() || actual.len() > 2 { return Err(Error::WrongClaim); }
    for expected in &base.credential_targets {
        if actual.iter().filter(|found| *found == expected).count() != 1 { return Err(Error::WrongClaim); }
    }
    if actual.iter().any(|target| !base.credential_targets.contains(target)) { return Err(Error::WrongClaim); }
    Ok(())
}

/// Original pin and certificate must come from the retained Native ledger or
/// verified Node Actor, never from a response's self-asserted owner.
pub fn verify_collector_recipient(
    pin: &OwnerPin, certificate: &Signed<Certificate>,
    base: &Signed<CollectorServiceDelegation>,
    proof: &Signed<CollectorRecipientAttestation>, challenge: Nonce, now: Now,
) -> Result<()> {
    certificate.verify(pin.key)?;
    base.verify(pin.key)?;
    let c = &certificate.body;
    let p = &base.body;
    let r = &proof.body;
    if p.header.context != pin.context || p.header.owner_epoch != pin.epoch
        || p.header.checkpoint != pin.checkpoint || r.header != p.header
        || c.header.context != pin.context || c.header.owner_epoch != pin.epoch
        || c.header.checkpoint > pin.checkpoint
    { return Err(Error::WrongContext); }
    if c.scopes.0.as_slice() != [Scope::MetricsCollect] || c.capabilities.scopes != c.scopes {
        return Err(Error::CapabilityDenied);
    }
    if p.agent_id != c.agent_id || p.key_epoch != c.key_epoch || p.recipient != c.agent_key
        || p.certificate_hash != c.digest()? || r.agent_id != p.agent_id
        || r.key_epoch != p.key_epoch || r.certificate_hash != p.certificate_hash
        || r.service_id != p.service_id || r.base_delegation_hash != p.digest()?
        || r.manifest_sha256 != p.manifest_sha256 || r.challenge != challenge
        || p.expires_at > c.expires_at || p.expires_at > pin.expires_at
        || r.expires_at > p.expires_at
    { return Err(Error::WrongClaim); }
    credential_live(c.not_before, c.expires_at, now)?;
    credential_live(p.issued_at, p.expires_at, now)?;
    credential_live(r.issued_at, r.expires_at, now)?;
    proof.verify(c.agent_key)
}

impl CollectorCredentialScope {
    /// Structural checks do not establish current Actor/certificate authority.
    pub fn check_base_and_recipient(
        &self, base: &CollectorServiceDelegation, recipient: &CollectorRecipientAttestation,
    ) -> Result<()> {
        self.validate()?;
        base.validate()?;
        recipient.validate()?;
        if self.header != base.header || self.agent_id != base.agent_id
            || !base.credential_targets.contains(&self.target)
            || self.key_epoch != base.key_epoch || self.certificate_hash != base.certificate_hash
            || self.service_id != base.service_id || self.base_delegation_hash != base.digest()?
            || self.policy_revision != base.policy_revision || self.revocation_epoch != base.revocation_epoch
            || self.collector_id != base.collector_id || self.manifest_sha256 != base.manifest_sha256
            || recipient.header != base.header || recipient.agent_id != base.agent_id
            || recipient.key_epoch != base.key_epoch || recipient.certificate_hash != base.certificate_hash
            || recipient.service_id != base.service_id || recipient.base_delegation_hash != base.digest()?
            || recipient.manifest_sha256 != base.manifest_sha256 || recipient.expires_at > base.expires_at
            || self.original_start_nonce != recipient.original_start_nonce
            || self.recipient_challenge != recipient.challenge
            || self.recipient_attestation_digest != recipient.digest()?
            || self.issued_at < recipient.issued_at
            || self.recipient_id != recipient.recipient_id
            || self.recipient_key_sha256 != Hash::from_bytes(Sha256::digest(recipient.recipient_public_key.as_bytes()).into())
            || self.recipient_key_epoch != recipient.recipient_key_epoch
            || self.recipient_certificate_epoch != recipient.recipient_certificate_epoch
            || self.expires_at > base.expires_at || self.issued_at < base.issued_at
        { return Err(Error::WrongClaim); }
        Ok(())
    }

    /// Selects only an existing catalog entry. No caller URL or pathname exists
    /// in this request. Caller must first verify the installed catalog itself.
    pub fn check_catalog<'a>(&self, manifest: &'a serde_json::Value) -> Result<&'a str> {
        self.validate()?;
        if collector_credential_manifest_sha256(manifest)? != self.manifest_sha256 {
            return Err(Error::WrongClaim);
        }
        let endpoint = match &self.target {
            CollectorCredentialTarget::Receiver {} => manifest.get("receiver")
                .and_then(|r| r.get("url")).and_then(|u| u.as_str()).ok_or(Error::InvalidWire)?,
            CollectorCredentialTarget::Scrape { target_id } => {
                let targets = manifest.get("targets").and_then(|t| t.as_array()).ok_or(Error::InvalidWire)?;
                if targets.is_empty() || targets.len() > 16 { return Err(Error::Quota); }
                let mut selected = targets.iter().filter(|t| t.get("id").and_then(|id| id.as_str()) == Some(target_id.as_str()));
                let target = selected.next().ok_or(Error::NotFound)?;
                if selected.next().is_some() { return Err(Error::InvalidWire); }
                target.get("url").and_then(|u| u.as_str()).ok_or(Error::InvalidWire)?
            }
        };
        if Hash::from_bytes(Sha256::digest(endpoint.as_bytes()).into()) != self.endpoint_sha256 {
            return Err(Error::WrongClaim);
        }
        Ok(endpoint)
    }
}
