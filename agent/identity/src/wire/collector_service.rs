//! Finite, explicitly approved collection authority. Not an interactive Lease.
//! Same sealed canonical signature envelope and existing MetricsCollect scope.
use super::*;

pub const COLLECTOR_SERVICE_TTL: u64 = 24 * 60 * 60;
pub const COLLECTOR_SERVICE_MAX_STARTS: u32 = 32;

/// Existing UUIDv8 manifest address; the signed full SHA remains authority.
pub fn collector_service_resource(manifest: Hash) -> Result<ResourceId> {
    let mut bytes = [0u8; 16];
    bytes.copy_from_slice(&manifest.as_bytes()[..16]);
    bytes[6] = (bytes[6] & 0x0f) | 0x80;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    ResourceId::new(&uuid::Uuid::from_bytes(bytes).to_string())
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum CollectorServiceProfile {
    #[serde(rename = "maintained-prometheus-disk-512mib-2h-v1")]
    MaintainedPrometheusDisk512mib2hV1,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CollectorServiceQuota {
    pub max_concurrent: u32,
    pub max_starts: u32,
    pub max_buffer_bytes: u64,
    pub max_residence_seconds: u64,
    pub max_memory_bytes: u64,
}
impl CollectorServiceQuota {
    pub fn fixed() -> Self {
        Self {
            max_concurrent: 1,
            max_starts: COLLECTOR_SERVICE_MAX_STARTS,
            max_buffer_bytes: 536_870_912,
            max_residence_seconds: 7200,
            max_memory_bytes: 536_870_912,
        }
    }
    pub fn validate(&self) -> Result<()> {
        if *self != Self::fixed() {
            return Err(Error::Quota);
        }
        Ok(())
    }
}

/// Owner context/header is verified against the original independent OwnerPin.
/// The manifest hash covers ALL endpoint/receiver/profile and per-target limits.
/// Recipient is the actual certified Node key, never a common vault key.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CollectorServiceDelegation {
    pub header: Header,
    pub service_id: ResourceId,
    pub policy_revision: u64,
    pub revocation_epoch: u64,
    pub agent_id: AgentId,
    pub key_epoch: u64,
    pub recipient: PublicKey,
    pub certificate_hash: Hash,
    pub collector_id: ResourceId,
    pub manifest_sha256: Hash,
    pub profile: CollectorServiceProfile,
    pub quota: CollectorServiceQuota,
    pub credential_targets: Vec<CollectorCredentialTarget>,
    pub issued_at: Timestamp,
    pub expires_at: Timestamp,
}
impl sealed::Sealed for CollectorServiceDelegation {}
impl Statement for CollectorServiceDelegation {
    const DOMAIN: &'static str = "metrics-collector-service-delegation-v1";
    fn validate(&self) -> Result<()> {
        self.header.validate()?;
        self.recipient.validate()?;
        self.quota.validate()?;
        if self.credential_targets.len() > 2 {
            return Err(Error::Quota);
        }
        let (mut scrape, mut receiver) = (false, false);
        for target in &self.credential_targets {
            match target {
                CollectorCredentialTarget::Receiver {} => {
                    if receiver {
                        return Err(Error::Quota);
                    }
                    receiver = true;
                }
                CollectorCredentialTarget::Scrape { target_id } => {
                    let b = target_id.as_bytes();
                    if scrape
                        || b.is_empty()
                        || b.len() > 32
                        || !b[0].is_ascii_alphabetic()
                        || !b
                            .iter()
                            .all(|b| b.is_ascii_alphanumeric() || b"_-".contains(b))
                    {
                        return Err(Error::InvalidWire);
                    }
                    scrape = true;
                }
            }
        }
        if self.key_epoch == 0 || self.policy_revision == 0 || self.revocation_epoch == 0 {
            return Err(Error::InvalidWire);
        }
        if self.service_id.0.get_version_num() != 4 {
            return Err(Error::InvalidWire);
        }
        if self.collector_id != collector_service_resource(self.manifest_sha256)? {
            return Err(Error::WrongClaim);
        }
        window(self.issued_at, self.expires_at, COLLECTOR_SERVICE_TTL)
    }
}
impl CollectorServiceDelegation {
    /// Caller additionally verifies the FULL catalog digest and endpoint policy.
    /// This checks every explicitly required credential target before signing,
    /// admission, recipient publication or execution; absence never means auto.
    pub fn check_credential_targets(&self, manifest: &serde_json::Value) -> Result<()> {
        self.validate()?;
        for target in &self.credential_targets {
            match target {
                CollectorCredentialTarget::Receiver {} => {
                    if manifest
                        .get("receiver")
                        .and_then(|r| r.get("url"))
                        .and_then(|v| v.as_str())
                        .is_none()
                    {
                        return Err(Error::WrongClaim);
                    }
                }
                CollectorCredentialTarget::Scrape { target_id } => {
                    let rows = manifest
                        .get("targets")
                        .and_then(|v| v.as_array())
                        .ok_or(Error::WrongClaim)?;
                    if rows
                        .iter()
                        .filter(|row| {
                            row.get("id").and_then(|v| v.as_str()) == Some(target_id.as_str())
                        })
                        .count()
                        != 1
                    {
                        return Err(Error::WrongClaim);
                    }
                }
            }
        }
        Ok(())
    }
}

/// Fresh original Node proof for the one explicit policy admission.
/// Recovered service execution uses signed-journal replay, not a new proof/TTL.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CollectorServiceClockStamp {
    pub boot_id: ResourceId,
    pub created_boottime_ns: u64,
    pub deadline_boottime_ns: u64,
}
impl CollectorServiceClockStamp {
    pub fn validate(&self) -> Result<()> {
        if self.created_boottime_ns == 0
            || self
                .deadline_boottime_ns
                .checked_sub(self.created_boottime_ns)
                .filter(|n| *n > 0 && *n <= COLLECTOR_SERVICE_TTL * 1_000_000_000)
                .is_none()
        {
            return Err(Error::InvalidTime);
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CollectorServiceClaim {
    pub header: Header,
    pub agent_id: AgentId,
    pub key_epoch: u64,
    pub delegation_hash: Hash,
    pub clock: CollectorServiceClockStamp,
    pub nonce: Nonce,
    pub issued_at: Timestamp,
}
impl sealed::Sealed for CollectorServiceClaim {}
impl Statement for CollectorServiceClaim {
    const DOMAIN: &'static str = "metrics-collector-service-node-claim-v1";
    fn validate(&self) -> Result<()> {
        self.header.validate()?;
        self.nonce.validate()?;
        self.clock.validate()?;
        if self.key_epoch == 0 {
            return Err(Error::InvalidWire);
        }
        self.issued_at.valid()
    }
}

/// Exact-policy revocation retains a monotonically increasing tombstone.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CollectorServiceRevocation {
    pub header: Header,
    pub service_id: ResourceId,
    pub delegation_hash: Hash,
    pub policy_revision: u64,
    pub revocation_epoch: u64,
    pub issued_at: Timestamp,
}
impl sealed::Sealed for CollectorServiceRevocation {}
impl Statement for CollectorServiceRevocation {
    const DOMAIN: &'static str = "metrics-collector-service-revoke-v1";
    fn validate(&self) -> Result<()> {
        self.header.validate()?;
        if self.policy_revision == 0 || self.revocation_epoch == 0 {
            return Err(Error::InvalidWire);
        }
        self.issued_at.valid()
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum CollectorServiceEffect {
    Configured,
    Revoked,
}

/// Actual selected Node attests committed readback, never component readiness
/// or receiver delivery. Absence/loss of this exact receipt stays Unknown.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CollectorServiceReceipt {
    pub header: Header,
    pub service_id: ResourceId,
    pub delegation_hash: Hash,
    pub policy_revision: u64,
    pub revocation_epoch: u64,
    pub agent_id: AgentId,
    pub key_epoch: u64,
    pub actor_revision: u64,
    pub effect: CollectorServiceEffect,
    pub issued_at: Timestamp,
}
impl sealed::Sealed for CollectorServiceReceipt {}
impl Statement for CollectorServiceReceipt {
    const DOMAIN: &'static str = "metrics-collector-service-committed-receipt-v1";
    fn validate(&self) -> Result<()> {
        self.header.validate()?;
        if self.policy_revision == 0
            || self.revocation_epoch == 0
            || self.key_epoch == 0
            || self.actor_revision == 0
        {
            return Err(Error::InvalidWire);
        }
        self.issued_at.valid()
    }
}
