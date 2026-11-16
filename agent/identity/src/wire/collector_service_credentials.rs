//! Separate finite service-lifetime credential purpose; never SingleStart.
//! Public signed metadata is not an Actor/Permit or resource-retirement proof.
use super::*;
use crate::OwnerPin;
use sha2::Digest as _;

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CollectorServiceCredentialRef {
    pub target: CollectorCredentialTarget,
    pub endpoint_sha256: Hash,
    pub username: String,
    pub credential_id: ResourceId,
    pub credential_revision: u64,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CollectorServiceCredentialAuthorization {
    pub authorization_id: ResourceId,
    pub owner_device_id: ResourceId,
    pub base: Signed<CollectorServiceDelegation>,
    pub admission: Signed<CollectorServiceClaim>,
    /// Initial consent proof, NOT a fresh proof or start capability on reopen.
    pub recipient: Signed<CollectorRecipientAttestation>,
    pub credentials: Vec<CollectorServiceCredentialRef>,
    pub issued_at: Timestamp,
    pub expires_at: Timestamp,
}
impl std::fmt::Debug for CollectorServiceCredentialAuthorization {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CollectorServiceCredentialAuthorization").finish_non_exhaustive()
    }
}
impl sealed::Sealed for CollectorServiceCredentialAuthorization {}
impl Statement for CollectorServiceCredentialAuthorization {
    const DOMAIN: &'static str = "metrics-collector-service-credential-authorization-v1";
    fn validate(&self) -> Result<()> {
        let b = &self.base.body;
        let a = &self.admission.body;
        let r = &self.recipient.body;
        b.validate()?;
        b.header.context.personal()?;
        a.validate()?;
        r.validate()?;
        b.quota.validate()?;
        if self.credentials.is_empty() || self.credentials.len() > 2
            || self.credentials.len() != b.credential_targets.len()
            || a.header != b.header || a.agent_id != b.agent_id || a.key_epoch != b.key_epoch
            || a.delegation_hash != b.digest()? || a.issued_at < b.issued_at
            || r.header != b.header || r.agent_id != b.agent_id || r.key_epoch != b.key_epoch
            || r.certificate_hash != b.certificate_hash || r.service_id != b.service_id
            || r.base_delegation_hash != b.digest()? || r.manifest_sha256 != b.manifest_sha256
            || self.issued_at < b.issued_at || self.issued_at < a.issued_at
            || self.issued_at < r.issued_at || self.issued_at >= r.expires_at
            || self.expires_at > b.expires_at || r.expires_at > b.expires_at
        { return Err(Error::WrongClaim); }
        let available = b.expires_at.seconds().checked_sub(a.issued_at.seconds())
            .and_then(|v| v.checked_mul(1_000_000_000)).ok_or(Error::InvalidTime)?;
        if a.clock.deadline_boottime_ns - a.clock.created_boottime_ns > available {
            return Err(Error::Expired);
        }
        for expected in &b.credential_targets {
            if self.credentials.iter().filter(|c| &c.target == expected).count() != 1 {
                return Err(Error::WrongClaim);
            }
        }
        for c in &self.credentials {
            if !b.credential_targets.contains(&c.target)
                || !(1..=9_007_199_254_740_991).contains(&c.credential_revision)
                || c.username.is_empty() || c.username.len() > 128
                || c.username.chars().any(|ch| ch.is_control() || ch == ':')
            { return Err(Error::InvalidWire); }
        }
        window(self.issued_at, self.expires_at, COLLECTOR_SERVICE_TTL)
    }
}

impl CollectorServiceCredentialAuthorization {
    /// Full set and actual immutable base/clock must be checked before plaintext.
    /// These inputs alone are metadata; Node additionally holds actual Access.
    pub fn check_original(
        &self, base: &CollectorServiceDelegation, original_clock: &CollectorServiceClockStamp,
        manifest: &serde_json::Value, now: Now,
    ) -> Result<()> {
        self.validate()?;
        if &self.base.body != base || &self.admission.body.clock != original_clock
            || now.0 < self.issued_at || now.0 >= self.expires_at
        { return Err(Error::WrongClaim); }
        let targets: Vec<_> = self.credentials.iter().map(|c| c.target.clone()).collect();
        check_collector_credential_target_set(base, &targets, manifest)?;
        for c in &self.credentials {
            let endpoint = match &c.target {
                CollectorCredentialTarget::Receiver {} => manifest.get("receiver")
                    .and_then(|v| v.get("url")).and_then(|v| v.as_str()).ok_or(Error::WrongClaim)?,
                CollectorCredentialTarget::Scrape { target_id } => {
                    let rows = manifest.get("targets").and_then(|v| v.as_array()).ok_or(Error::WrongClaim)?;
                    if rows.is_empty() || rows.len() > 16 { return Err(Error::Quota); }
                    let mut found = rows.iter().filter(|v| v.get("id").and_then(|v| v.as_str()) == Some(target_id.as_str()));
                    let row = found.next().ok_or(Error::NotFound)?;
                    if found.next().is_some() { return Err(Error::WrongClaim); }
                    row.get("url").and_then(|v| v.as_str()).ok_or(Error::WrongClaim)?
                }
            };
            if c.endpoint_sha256 != Hash::from_bytes(Sha256::digest(endpoint.as_bytes()).into()) {
                return Err(Error::WrongClaim);
            }
        }
        Ok(())
    }

    /// Digest commits this WHOLE owner authorization and the selected exact ref.
    /// Existing capsule v1 crypto is reused with a separate binding/AAD digest.
    pub fn capsule_delegation_digest(&self, slot: usize) -> Result<Hash> {
        self.validate()?;
        let selected = self.credentials.get(slot).ok_or(Error::Quota)?;
        let mut bytes = b"ConsoleCrypt CollectorServiceCredentialBinding v1\0".to_vec();
        bytes.extend_from_slice(self.digest()?.as_bytes());
        let value = serde_json::to_value(selected).map_err(|_| Error::InvalidWire)?;
        canonical_json(&value, &mut bytes)?;
        Ok(Hash::from_bytes(Sha256::digest(bytes).into()))
    }

    /// Historical initial proof is verified at original authorization time.
    /// Reopen DOES NOT refresh proof time, original service end, or start count.
    pub fn verify_initial_binding(&self, pin: &OwnerPin) -> Result<()> {
        self.validate()?;
        let b = &self.base.body;
        if b.header.context != pin.context || b.header.owner_epoch != pin.epoch
            || b.header.checkpoint != pin.checkpoint || self.expires_at > pin.expires_at
            || b.expires_at > pin.expires_at
        { return Err(Error::WrongContext); }
        self.base.verify(pin.key)?;
        self.admission.verify(b.recipient)?;
        self.recipient.verify(b.recipient)?;
        Ok(())
    }
}

/// Node-signed observation of ONE exact authenticated journal/CAS admission.
/// This never attests plaintext use, a running component, join or cleanup.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CollectorServiceCredentialSetReceipt {
    pub header: Header,
    pub agent_id: AgentId,
    pub key_epoch: u64,
    pub service_id: ResourceId,
    pub base_delegation_hash: Hash,
    pub authorization_digest: Hash,
    pub ordered_capsule_hashes: Vec<Hash>,
    pub original_start_nonce: Nonce,
    pub admitted_revision: u64,
    pub actor_revision: u64,
    pub issued_at: Timestamp,
    pub expires_at: Timestamp,
}
impl sealed::Sealed for CollectorServiceCredentialSetReceipt {}
impl Statement for CollectorServiceCredentialSetReceipt {
    const DOMAIN: &'static str = "metrics-collector-service-credential-set-receipt-v1";
    fn validate(&self) -> Result<()> {
        self.header.validate()?;
        self.header.context.personal()?;
        self.original_start_nonce.validate()?;
        if self.key_epoch == 0 || self.admitted_revision == 0 || self.actor_revision != self.admitted_revision
            || self.ordered_capsule_hashes.is_empty() || self.ordered_capsule_hashes.len() > 2
            || (self.ordered_capsule_hashes.len() == 2 && self.ordered_capsule_hashes[0] == self.ordered_capsule_hashes[1])
        { return Err(Error::WrongClaim); }
        window(self.issued_at, self.expires_at, PROOF_TTL)
    }
}

/// Signature and exact-content verification only. Actual Native Context/Peer,
/// original SQL/CAS readback and Node Actor/Permit checks remain mandatory.
pub fn verify_collector_service_set_receipt(
    pin: &OwnerPin, certificate: &Signed<Certificate>,
    authorization: &Signed<CollectorServiceCredentialAuthorization>,
    receipt: &Signed<CollectorServiceCredentialSetReceipt>,
    ordered_capsule_hashes: &[Hash], minimum_actor_revision: u64, now: Now,
) -> Result<()> {
    authorization.verify(pin.key)?;
    authorization.body.verify_initial_binding(pin)?;
    let a = &authorization.body;
    verify_collector_recipient(pin, certificate, &a.base, &a.recipient,
        a.recipient.body.challenge, Now(a.issued_at))?;
    receipt.verify(certificate.body.agent_key)?;
    let r = &receipt.body;
    r.validate()?;
    let b = &a.base.body;
    if minimum_actor_revision == 0 || ordered_capsule_hashes.len() != a.credentials.len()
        || r.header != b.header || r.agent_id != b.agent_id || r.key_epoch != b.key_epoch
        || r.service_id != b.service_id || r.base_delegation_hash != b.digest()?
        || r.authorization_digest != a.digest()? || r.ordered_capsule_hashes != ordered_capsule_hashes
        || r.original_start_nonce != a.recipient.body.original_start_nonce
        || r.admitted_revision < minimum_actor_revision || r.actor_revision != r.admitted_revision
        || now.0 < r.issued_at || now.0 >= r.expires_at || r.issued_at < a.issued_at
        || r.expires_at > a.expires_at || now.0 < certificate.body.not_before || now.0 >= certificate.body.expires_at
    { return Err(Error::WrongClaim); }
    Ok(())
}
