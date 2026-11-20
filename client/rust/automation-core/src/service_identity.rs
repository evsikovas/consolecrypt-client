//! Separate service-owned identity. No Desktop/Agent/Vault identity import.
use crate::{AutomationError, Result};
use cc_crypto_core::{DevicePublicKeys, DeviceSecretKeys};
use sha2::{Digest, Sha256};
use uuid::Uuid;
use zeroize::Zeroizing;

#[cfg(all(test, target_os = "linux"))]
mod fs_tests;
#[cfg(target_os = "linux")]
mod linux;
const MAGIC: &[u8; 8] = b"CCSI\0\0\0\x01";
const PREFIX: usize = 8 + 16 + 8 + 69 + 64;
const BYTES: usize = PREFIX + 32;
fn denied() -> AutomationError {
    AutomationError::Invalid("service identity unavailable")
}
fn checksum(bytes: &[u8]) -> [u8; 32] {
    let mut hash = Sha256::new();
    hash.update(b"consolecrypt.service.local-key-file.v1\0");
    hash.update(bytes);
    hash.finalize().into()
}
fn encode(id: Uuid, keys: &DeviceSecretKeys) -> Result<Zeroizing<Vec<u8>>> {
    if id.is_nil() {
        return Err(denied());
    }
    let secret = keys.to_secret_bytes();
    if secret.len() != 69 {
        return Err(denied());
    }
    let public = keys.public_keys();
    let mut bytes = Zeroizing::new(Vec::with_capacity(BYTES));
    bytes.extend_from_slice(MAGIC);
    bytes.extend_from_slice(id.as_bytes());
    bytes.extend_from_slice(&1u64.to_le_bytes());
    bytes.extend_from_slice(&secret);
    bytes.extend_from_slice(&public.signing);
    bytes.extend_from_slice(&public.encryption);
    let digest = checksum(&bytes);
    bytes.extend_from_slice(&digest);
    Ok(bytes)
}
fn decode(bytes: &[u8], expected: Uuid) -> Result<DeviceSecretKeys> {
    if expected.is_nil()
        || bytes.len() != BYTES
        || &bytes[..8] != MAGIC
        || &bytes[8..24] != expected.as_bytes()
        || bytes[24..32] != 1u64.to_le_bytes()
        || checksum(&bytes[..PREFIX]).as_slice() != &bytes[PREFIX..]
    {
        return Err(denied());
    }
    let keys = DeviceSecretKeys::from_secret_bytes(&bytes[32..101]).map_err(|_| denied())?;
    let public = keys.public_keys();
    if bytes[101..133] != public.signing || bytes[133..165] != public.encryption {
        return Err(denied());
    }
    Ok(keys)
}

/// Non-secret observation. Its UUID/epoch/public keys cannot enroll or grant a job.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ServiceIdentityFacts {
    pub identity_id: Uuid,
    pub key_epoch: u64,
    pub public_keys: DevicePublicKeys,
}
/// Public storage metadata only, not an Actor/Native profile or authority.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct ServiceLocalNamespace {
    pub version: u8,
    pub identity_id: Uuid,
    pub key_epoch: u64,
    pub namespace: String,
}
impl ServiceLocalNamespace {
    fn derive(identity_id: Uuid, public: &DevicePublicKeys) -> Result<Self> {
        use sha2::{Digest, Sha256};
        if identity_id.is_nil() { return Err(denied()); }
        let mut hash=Sha256::new();
        hash.update(b"ConsoleCrypt ServiceLocalNamespace v1\0");
        hash.update(identity_id.as_bytes()); hash.update(1u64.to_be_bytes());
        hash.update(public.signing); hash.update(public.encryption);
        Ok(Self { version:1, identity_id, key_epoch:1,
            namespace:format!("cc-service-local-v1-{}",hex::encode(hash.finalize())) })
    }
    /// Checks only deterministic metadata; caller authenticates original peer.
    pub fn validate_public(&self, public: &DevicePublicKeys) -> Result<()> {
        if self != &Self::derive(self.identity_id, public)? { return Err(denied()); }
        Ok(())
    }
}

/// Non-Clone/non-Serde local holder; retained through the actual service close.
pub struct ServiceIdentity {
    keys: Option<DeviceSecretKeys>,
    facts: ServiceIdentityFacts,
    #[cfg(target_os = "linux")]
    guard: linux::Guard,
}
impl ServiceIdentity {
    pub fn create(parent: cap_std::fs::Dir, identity_id: Uuid) -> Result<Self> {
        #[cfg(target_os = "linux")]
        {
            linux::create(parent, identity_id)
        }
        #[cfg(not(target_os = "linux"))]
        {
            let _ = (parent, identity_id);
            Err(denied())
        }
    }
    pub fn open_existing(parent: cap_std::fs::Dir, identity_id: Uuid) -> Result<Self> {
        #[cfg(target_os = "linux")]
        {
            linux::open(parent, identity_id)
        }
        #[cfg(not(target_os = "linux"))]
        {
            let _ = (parent, identity_id);
            Err(denied())
        }
    }
    pub fn facts(&self) -> &ServiceIdentityFacts {
        &self.facts
    }
    pub fn local_namespace(&self) -> Result<ServiceLocalNamespace> {
        self.recheck()?;
        if self.facts.key_epoch != 1 { return Err(denied()); }
        let value=ServiceLocalNamespace::derive(self.facts.identity_id, &self.facts.public_keys)?;
        self.recheck()?;
        Ok(value)
    }
    pub fn recheck(&self) -> Result<()> {
        #[cfg(target_os = "linux")]
        {
            self.guard.check(&self.facts)
        }
        #[cfg(not(target_os = "linux"))]
        {
            Err(denied())
        }
    }
    pub(crate) fn enrollment(&self) -> Result<Option<crate::service_restart::EnrollmentRecord>> {
        self.recheck()?;
        #[cfg(target_os = "linux")]
        {
            self.guard.enrollment(&self.facts)
        }
        #[cfg(not(target_os = "linux"))]
        {
            Err(denied())
        }
    }
    pub(crate) fn store_enrollment(
        &mut self,
        record: &crate::service_restart::EnrollmentRecord,
    ) -> Result<()> {
        self.recheck()?;
        let raw = record.encode()?;
        crate::service_restart::EnrollmentRecord::parse(&raw, &self.facts)?;
        #[cfg(target_os = "linux")]
        {
            self.guard.store_enrollment(&raw, &self.facts)
        }
        #[cfg(not(target_os = "linux"))]
        {
            Err(denied())
        }
    }
    pub(crate) fn take_keys(&mut self) -> Result<DeviceSecretKeys> {
        self.recheck()?;
        self.keys.take().ok_or_else(denied)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn strict_original_identity_encoding_roundtrip_and_foreign_future_corrupt_denials() {
        let id = Uuid::new_v4();
        let keys = DeviceSecretKeys::generate().unwrap();
        let bytes = encode(id, &keys).unwrap();
        assert_eq!(bytes.len(), BYTES);
        assert_eq!(
            decode(&bytes, id).unwrap().public_keys(),
            keys.public_keys()
        );
        assert!(decode(&bytes, Uuid::new_v4()).is_err());
        assert!(decode(&bytes, Uuid::nil()).is_err());
        assert!(decode(&bytes[..bytes.len() - 1], id).is_err());
        for index in [0, 7, 24, 31, 32, 101, 133, PREFIX] {
            let mut changed = bytes.clone();
            changed[index] ^= 1;
            assert!(decode(&changed, id).is_err());
        }
        let mut future = bytes.clone();
        future[7] = 2;
        let digest = checksum(&future[..PREFIX]);
        future[PREFIX..].copy_from_slice(&digest);
        assert!(decode(&future, id).is_err());
        let mut false_public = bytes.clone();
        false_public[101] ^= 1;
        let digest = checksum(&false_public[..PREFIX]);
        false_public[PREFIX..].copy_from_slice(&digest);
        assert!(decode(&false_public, id).is_err());
    }
}

#[cfg(target_os = "linux")]
pub(crate) fn runtime_dir(path: &std::path::Path) -> Result<cap_std::fs::Dir> {
    linux::runtime_dir(path)
}

#[cfg(all(test,target_os="linux"))]
pub(crate) fn test_faulted_store(identity:&mut ServiceIdentity,record:&crate::service_restart::EnrollmentRecord,phase:usize)->Result<()> {
    let raw=record.encode()?;linux::faulted_store(identity,&raw,phase)
}

#[cfg(test)]
mod namespace_tests;
