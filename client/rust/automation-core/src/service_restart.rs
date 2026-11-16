//! Original private enrollment metadata; never a signer or reconstructed job.
use crate::{AutomationError, HelperLaunch, Result, ServiceCapsuleInput, ServiceIdentityFacts};
use cc_crypto_core::{
    DevicePublicKeys, ServiceExecutionOrigin, ServicePolicyTenant, SignedServiceDelegation,
    VerifiedServiceDelegation,
};
use cc_models::{
    automation::{ApprovedTarget, JobMode},
    known_host::KnownHost,
};
use cc_ssh_core::ConnectionPlan;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;
use zeroize::Zeroizing;

pub(crate) const MAX_RECORD: usize = 2 * 1024 * 1024;
pub(crate) fn denied() -> AutomationError {
    AutomationError::Invalid("original service restoration unavailable")
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct OwnerPin {
    pub owner_id: String,
    pub owner_epoch: u64,
    pub tenant: ServicePolicyTenant,
    pub signing: String,
    pub encryption: String,
    pub not_after: u64,
}
impl OwnerPin {
    pub fn public(&self) -> Result<DevicePublicKeys> {
        fn key(text: &str) -> Result<[u8; 32]> {
            let raw = hex::decode(text).map_err(|_| denied())?;
            if hex::encode(&raw) != text {
                return Err(denied());
            }
            raw.try_into().map_err(|_| denied())
        }
        Ok(DevicePublicKeys {
            signing: key(&self.signing)?,
            encryption: key(&self.encryption)?,
        })
    }
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Bootstrap {
    pub origin: ServiceExecutionOrigin,
    pub owner: OwnerPin,
    pub schedule: Value,
}
impl Bootstrap {
    pub fn helper(&self) -> Result<Value> {
        let public = self.owner.public()?;
        Ok(
            serde_json::json!({"version":1,"origin":self.origin,"schedule":self.schedule,"owner":{
            "owner_id":self.owner.owner_id,"owner_epoch":self.owner.owner_epoch,"tenant":self.owner.tenant,
            "public_key":hex::encode(public.signing),"not_after":self.owner.not_after,
            "minimum_policy_revision":1,"minimum_revocation_epoch":1}}),
        )
    }
}
#[derive(Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(crate) struct RuntimeIdentity {
    pub device: u64,
    pub inode: u64,
}

pub(crate) struct RuntimePin {
    path: std::path::PathBuf,
    #[cfg(target_os = "linux")]
    root: cap_std::fs::Dir,
    pub identity: RuntimeIdentity,
}
impl RuntimePin {
    pub fn open(launch: &HelperLaunch) -> Result<Self> {
        let HelperLaunch::Native { root: path, .. } = launch else {
            return Err(denied());
        };
        #[cfg(target_os = "linux")]
        {
            use cap_std::fs::MetadataExt;
            let root = crate::service_identity::runtime_dir(path)?;
            let m = root.dir_metadata()?;
            if m.ino() == 0 {
                return Err(denied());
            }
            let pin = Self {
                path: path.clone(),
                root,
                identity: RuntimeIdentity {
                    device: m.dev(),
                    inode: m.ino(),
                },
            };
            pin.check()?;
            Ok(pin)
        }
        #[cfg(not(target_os = "linux"))]
        {
            let _ = path;
            Err(denied())
        }
    }
    pub fn check(&self) -> Result<()> {
        #[cfg(target_os = "linux")]
        {
            use cap_std::fs::MetadataExt;
            let current = crate::service_identity::runtime_dir(&self.path)?;
            let held = self.root.dir_metadata()?;
            let named = current.dir_metadata()?;
            if held.dev() != self.identity.device
                || held.ino() != self.identity.inode
                || held.dev() != named.dev()
                || held.ino() != named.ino()
            {
                return Err(denied());
            }
            Ok(())
        }
        #[cfg(not(target_os = "linux"))]
        {
            Err(denied())
        }
    }
}

#[derive(Serialize,Deserialize)]
#[serde(deny_unknown_fields,rename_all="camelCase")]
pub(crate) struct HelperEnrollmentAck {pub enrolled:bool,pub delegation_digest:String,pub revision_id:Uuid}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct EnrollmentRecord {
    pub version: u8,
    pub identity_id: Uuid,
    pub key_epoch: u64,
    pub signing: String,
    pub recipient: String,
    pub runtime: RuntimeIdentity,
    pub bootstrap: Bootstrap,
    pub delegation: Value,
    pub helper_ack:HelperEnrollmentAck,
    pub targets: Vec<ApprovedTarget>,
    pub plans: Vec<ConnectionPlan>,
    pub known_hosts: Vec<KnownHost>,
    pub capsules: Vec<ServiceCapsuleInput>,
}
impl EnrollmentRecord {
    pub fn parse(raw: &[u8], facts: &ServiceIdentityFacts) -> Result<Self> {
        if raw.len() > MAX_RECORD {
            return Err(denied());
        }
        let original=crate::json::parse(raw)?;
        let record:Self=serde_json::from_value(original.clone()).map_err(|_|denied())?;
        // Existing model types need not deny unknown fields for ordinary sync.
        // A local restart record, however, accepts ONLY its Native writer's
        // complete known representation, including nested route metadata.
        if serde_json::to_value(&record).map_err(|_|denied())?!=original {return Err(denied());}
        if record.version != 1
            || record.identity_id != facts.identity_id
            || record.key_epoch != facts.key_epoch
            || record.signing != hex::encode(facts.public_keys.signing)
            || record.recipient != hex::encode(facts.public_keys.encryption)
            || record.runtime.inode == 0
            || record.plans.is_empty()
            || record.plans.len() > 32
            || record.targets.len() != record.plans.len()
            || record.known_hosts.is_empty()
            || record.known_hosts.len() > 512
            || record.capsules.len() > 64
        {
            return Err(denied());
        }
        let issued = record
            .delegation
            .get("body")
            .and_then(|v| v.get("issued_at"))
            .and_then(Value::as_u64)
            .ok_or_else(denied)?;
        record.policy(issued)?; // strict signature/schema, not current execution authority.
        Ok(record)
    }
    pub fn policy(&self, now: u64) -> Result<VerifiedServiceDelegation> {
        let owner = &self.bootstrap.owner;
        let public = owner.public()?;
        let verified = SignedServiceDelegation::verify_json(
            &serde_json::to_vec(&self.delegation).map_err(|_| denied())?,
            &owner.owner_id,
            &public,
            &owner.tenant,
            now,
        )
        .map_err(|_| denied())?;
        let p = verified.body();
        if owner.owner_epoch != p.owner_epoch
            || now >= owner.not_after
            || p.expires_at > owner.not_after
            || p.access_policy.mode != cc_crypto_core::ServiceAccessMode::BoundedOffline
            || p.execution_origin.as_ref() != Some(&self.bootstrap.origin)
            || self.bootstrap.origin.signer_key_sha256
                != hex::encode(sha2::Sha256::digest(
                    hex::decode(&self.signing).map_err(|_| denied())?,
                ))
            || self.bootstrap.origin.recipient_key_sha256
                != hex::encode(sha2::Sha256::digest(
                    hex::decode(&self.recipient).map_err(|_| denied())?,
                ))
        {
            return Err(denied());
        }
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct ScheduleReference {id:String,revision:u64,definition:Value}
        let schedule:ScheduleReference=serde_json::from_value(self.bootstrap.schedule.clone()).map_err(|_|denied())?;
        let raw=serde_json::to_vec(&crate::project::canonical(schedule.definition)).map_err(|_|denied())?;
        if schedule.id!=p.schedule_id || schedule.revision!=p.schedule_revision || raw.len()>4096
            || hex::encode(sha2::Sha256::digest(&raw))!=p.definition_digest {return Err(denied());}
        if !self.helper_ack.enrolled || self.helper_ack.revision_id.is_nil()
            || self.helper_ack.delegation_digest!=verified.digest().map_err(|_|denied())? {return Err(denied());}
        self.mode(&verified)?;
        Ok(verified)
    }
    pub fn mode(&self, verified: &VerifiedServiceDelegation) -> Result<JobMode> {
        match verified.body().modes.as_slice() {
            [mode] if mode == "check" => Ok(JobMode::Check),
            [mode] if mode == "apply" => Ok(JobMode::Apply),
            _ => Err(denied()),
        }
    }
    pub fn encode(&self) -> Result<Zeroizing<Vec<u8>>> {
        struct Budget(usize);
        impl std::io::Write for Budget {
            fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
                self.0 = self
                    .0
                    .checked_sub(b.len())
                    .ok_or(std::io::ErrorKind::InvalidData)?;
                Ok(b.len())
            }
            fn flush(&mut self) -> std::io::Result<()> {
                Ok(())
            }
        }
        serde_json::to_writer(&mut Budget(MAX_RECORD), self).map_err(|_| denied())?;
        serde_json::to_vec(self)
            .map(Zeroizing::new)
            .map_err(|_| denied())
    }
}
use sha2::Digest;

#[cfg(all(test, target_os = "linux"))]
#[path = "service_restart_tests.rs"]
mod tests;
