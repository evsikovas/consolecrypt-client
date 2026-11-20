//! Private service credential capsule. Metadata is NOT an authorization grant.
//! Native must first approve exact saved refs/recipient and independently admit
//! each later job/channel. No VRK, lookup/resolver, I/O or generic signer exists.
use crate::{
    CryptoError, DeviceSecretKeys, ExposeSecret, aead,
    device::{random_x25519_secret, x25519, x25519_public},
    kdf::hkdf_sha256_32,
    rng::random_array,
};
use cc_protocol::Bytes;
use ed25519_dalek::{Signature, VerifyingKey};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fmt;
use uuid::Uuid;
use zeroize::Zeroizing;

const KEY_DOMAIN: &[u8] = b"ConsoleCrypt ServiceCredentialKey v1\0";
const AAD_DOMAIN: &[u8] = b"ConsoleCrypt ServiceCredentialAAD v1\0";
const SIGN_DOMAIN: &[u8] = b"ConsoleCrypt ServiceCredentialCapsule v1\0";
const MAX_SECRET: usize = 16 * 1024;
const MAX_WIRE: usize = 32 * 1024;
const MAX_TIME: u64 = 253_402_300_799;
const MAX_REV: u64 = 9_007_199_254_740_991;

type Result<T> = std::result::Result<T, ServiceCredentialError>;
#[derive(Debug, thiserror::Error)]
pub enum ServiceCredentialError {
    #[error("invalid bounded service capsule structure")]
    Invalid,
    #[error("service capsule context differs from the approved one")]
    Context,
    #[error("service capsule validity ended or clock is invalid")]
    Expired,
    #[error("service capsule issuer signature rejected")]
    Signature,
    #[error(transparent)]
    Crypto(#[from] CryptoError),
}
fn require(value: bool) -> Result<()> {
    if value {
        Ok(())
    } else {
        Err(ServiceCredentialError::Invalid)
    }
}
fn hash(value: &[u8]) -> String {
    Sha256::digest(value)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}
fn identifier(value: &str) -> bool {
    value.len() == 36
        && Uuid::parse_str(value).is_ok_and(|id| !id.is_nil() && id.to_string() == value)
}
fn digest(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ServiceTenantKind {
    LocalProfile,
    PersonalAccount,
    Team,
}
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ServiceCredentialKind {
    Password,
    SshPrivateKey,
    AnsibleVaultPassword,
}

/// Original approved Native metadata. Never infer these from an untrusted
/// server directory or a caller `approved` flag. String IDs remain canonical.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ServiceCredentialBinding {
    pub version: u8,
    pub owner_device_id: String,
    pub owner_signing_key_sha256: String,
    pub tenant_kind: ServiceTenantKind,
    pub tenant_id: String,
    pub service_id: String,
    pub delegation_digest: String,
    pub policy_revision: u64,
    pub revocation_epoch: u64,
    pub credential_id: String,
    pub credential_revision: u64,
    pub kind: ServiceCredentialKind,
    pub recipient_key_sha256: String,
    pub recipient_key_epoch: u64,
    pub recipient_certificate_epoch: u64,
    pub delegation_expires_at: u64,
}
impl ServiceCredentialBinding {
    pub fn validate(&self) -> Result<()> {
        require(self.version == 1 && self.tenant_kind != ServiceTenantKind::Team)?;
        for value in [
            &self.owner_device_id,
            &self.tenant_id,
            &self.service_id,
            &self.credential_id,
        ] {
            require(identifier(value))?;
        }
        for value in [
            &self.owner_signing_key_sha256,
            &self.delegation_digest,
            &self.recipient_key_sha256,
        ] {
            require(digest(value))?;
        }
        for value in [
            self.policy_revision,
            self.revocation_epoch,
            self.credential_revision,
            self.recipient_key_epoch,
            self.recipient_certificate_epoch,
        ] {
            require((1..=MAX_REV).contains(&value))?;
        }
        require(self.delegation_expires_at > 0 && self.delegation_expires_at <= MAX_TIME)
    }
}
impl fmt::Debug for ServiceCredentialBinding {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ServiceCredentialBinding")
            .finish_non_exhaustive()
    }
}

/// Non-Clone, non-Serde scoped plaintext. Only actual Native selected material
/// may reach this Rust-only constructor; it does not perform a vault lookup.
pub struct SelectedServiceCredential {
    binding: ServiceCredentialBinding,
    value: Zeroizing<Vec<u8>>,
}
impl SelectedServiceCredential {
    pub fn new(binding: ServiceCredentialBinding, value: Zeroizing<Vec<u8>>) -> Result<Self> {
        binding.validate()?;
        require(!value.is_empty() && value.len() <= MAX_SECRET)?;
        Ok(Self { binding, value })
    }
}
impl fmt::Debug for SelectedServiceCredential {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SelectedServiceCredential")
            .field("value", &"[REDACTED]")
            .finish_non_exhaustive()
    }
}

/// Ciphertext + bounded metadata only. Signature authenticates the Native
/// issuer: possession of the public recipient key cannot poison a credential.
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ServiceCredentialCapsule {
    version: u8,
    binding: ServiceCredentialBinding,
    issued_at: u64,
    expires_at: u64,
    ephemeral_public: Bytes,
    nonce: Bytes,
    ciphertext: Bytes,
    signature: Bytes,
}
impl fmt::Debug for ServiceCredentialCapsule {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ServiceCredentialCapsule")
            .field("version", &self.version)
            .finish_non_exhaustive()
    }
}
impl ServiceCredentialCapsule {
    pub fn access_expires_at(&self) -> Result<u64> {
        self.structure()?;
        Ok(self.expires_at)
    }
    pub fn to_json(&self) -> Result<Vec<u8>> {
        self.structure()?;
        let raw = serde_json::to_vec(self).map_err(|_| ServiceCredentialError::Invalid)?;
        require(raw.len() <= MAX_WIRE)?;
        Ok(raw)
    }
    pub fn from_json(raw: &[u8]) -> Result<Self> {
        require(raw.len() <= MAX_WIRE)?;
        let value: Self =
            serde_json::from_slice(raw).map_err(|_| ServiceCredentialError::Invalid)?;
        value.structure()?;
        Ok(value)
    }
    fn structure(&self) -> Result<()> {
        self.binding.validate()?;
        require(
            self.version == 1
                && self.ephemeral_public.len() == 32
                && self.nonce.len() == 24
                && (17..=MAX_SECRET + 16).contains(&self.ciphertext.len())
                && self.signature.len() == 64,
        )
    }
    fn aad(&self) -> Result<Vec<u8>> {
        let binding =
            serde_json::to_vec(&self.binding).map_err(|_| ServiceCredentialError::Invalid)?;
        require(binding.len() <= 4096)?;
        let mut aad = AAD_DOMAIN.to_vec();
        aad.extend_from_slice(&(binding.len() as u32).to_be_bytes());
        aad.extend_from_slice(&binding);
        aad.extend_from_slice(&self.issued_at.to_be_bytes());
        aad.extend_from_slice(&self.expires_at.to_be_bytes());
        Ok(aad)
    }
    fn signing_bytes(&self) -> Result<Vec<u8>> {
        let aad = self.aad()?;
        let mut message = SIGN_DOMAIN.to_vec();
        message.extend_from_slice(&(aad.len() as u32).to_be_bytes());
        message.extend_from_slice(&aad);
        message.extend_from_slice(self.ephemeral_public.as_slice());
        message.extend_from_slice(self.nonce.as_slice());
        message.extend_from_slice(&(self.ciphertext.len() as u32).to_be_bytes());
        message.extend_from_slice(self.ciphertext.as_slice());
        Ok(message)
    }
    fn window(&self, now: u64) -> Result<()> {
        if now > MAX_TIME
            || self.issued_at > now
            || now >= self.expires_at
            || self.expires_at
                > self
                    .issued_at
                    .checked_add(86400)
                    .ok_or(ServiceCredentialError::Expired)?
            || self.expires_at > self.binding.delegation_expires_at
        {
            return Err(ServiceCredentialError::Expired);
        }
        Ok(())
    }
}

/// Explicit approved Native selected credential -> dedicated service recipient.
/// No owner key is exported. No personal vault key is wrapped or transferred.
pub fn seal_service_credential(
    selected: SelectedServiceCredential,
    recipient_public: &[u8; 32],
    owner: &DeviceSecretKeys,
    now: u64,
    expires_at: u64,
) -> Result<ServiceCredentialCapsule> {
    let public = owner.public_keys();
    if selected.binding.owner_signing_key_sha256 != hash(&public.signing)
        || selected.binding.recipient_key_sha256 != hash(recipient_public)
        || recipient_public == &public.encryption
        || recipient_public == &public.signing
    {
        return Err(ServiceCredentialError::Context);
    }
    let ephemeral = random_x25519_secret()?;
    let ephemeral_public = x25519_public(&ephemeral);
    let shared = x25519(&ephemeral, recipient_public)?;
    let mut capsule = ServiceCredentialCapsule {
        version: 1,
        binding: selected.binding,
        issued_at: now,
        expires_at,
        ephemeral_public: Bytes::new(ephemeral_public.to_vec()),
        nonce: Bytes::new(random_array::<24>()?.to_vec()),
        ciphertext: Bytes::new(Vec::new()),
        signature: Bytes::new(Vec::new()),
    };
    capsule.window(now)?;
    let aad = capsule.aad()?;
    let salt: [u8; 32] = Sha256::digest(&aad).into();
    let key = hkdf_sha256_32(
        shared.expose_secret(),
        &salt,
        &[KEY_DOMAIN, &ephemeral_public, recipient_public],
    );
    let nonce: [u8; 24] = capsule
        .nonce
        .to_array()
        .ok_or(ServiceCredentialError::Invalid)?;
    capsule.ciphertext = Bytes::new(aead::seal(
        key.expose_secret(),
        &nonce,
        &aad,
        &selected.value,
    )?);
    capsule.signature = Bytes::new(owner.sign_message(&capsule.signing_bytes()?).to_vec());
    capsule.structure()?;
    Ok(capsule)
}

/// Scoped plaintext holder. This is credential provisioning, NOT a job grant.
/// The runtime must independently check current delegation/head/replay/quotas
/// before a new job/channel. Same-UID trusted holders can copy selected secrets.
pub struct OpenedServiceCredential {
    binding: ServiceCredentialBinding,
    value: Zeroizing<Vec<u8>>,
    expires_at: u64,
    issued_at: u64,
}
impl fmt::Debug for OpenedServiceCredential {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("OpenedServiceCredential")
            .field("value", &"[REDACTED]")
            .finish_non_exhaustive()
    }
}
impl OpenedServiceCredential {
    /// Rust-only narrow use; never bind to FRB or return raw material in JSON.
    pub fn with_secret<R>(
        &self,
        expected: &ServiceCredentialBinding,
        now: u64,
        use_bytes: impl FnOnce(&[u8]) -> R,
    ) -> Result<R> {
        if &self.binding != expected {
            return Err(ServiceCredentialError::Context);
        }
        if now < self.issued_at || now >= self.expires_at {
            return Err(ServiceCredentialError::Expired);
        }
        Ok(use_bytes(&self.value))
    }
}
pub fn open_service_credential(
    capsule: &ServiceCredentialCapsule,
    expected: &ServiceCredentialBinding,
    pinned_owner_signing: &[u8; 32],
    recipient: &DeviceSecretKeys,
    now: u64,
) -> Result<OpenedServiceCredential> {
    capsule.structure()?;
    expected.validate()?;
    capsule.window(now)?;
    let recipient_public = recipient.public_keys().encryption;
    if &capsule.binding != expected
        || expected.owner_signing_key_sha256 != hash(pinned_owner_signing)
        || expected.recipient_key_sha256 != hash(&recipient_public)
    {
        return Err(ServiceCredentialError::Context);
    }
    let signature: [u8; 64] = capsule
        .signature
        .to_array()
        .ok_or(ServiceCredentialError::Invalid)?;
    VerifyingKey::from_bytes(pinned_owner_signing)
        .map_err(|_| ServiceCredentialError::Signature)?
        .verify_strict(
            &capsule.signing_bytes()?,
            &Signature::from_bytes(&signature),
        )
        .map_err(|_| ServiceCredentialError::Signature)?;
    let ephemeral: [u8; 32] = capsule
        .ephemeral_public
        .to_array()
        .ok_or(ServiceCredentialError::Invalid)?;
    let shared = recipient.diffie_hellman(&ephemeral)?;
    let aad = capsule.aad()?;
    let salt: [u8; 32] = Sha256::digest(&aad).into();
    let key = hkdf_sha256_32(
        shared.expose_secret(),
        &salt,
        &[KEY_DOMAIN, &ephemeral, &recipient_public],
    );
    let nonce: [u8; 24] = capsule
        .nonce
        .to_array()
        .ok_or(ServiceCredentialError::Invalid)?;
    let value = aead::open(
        key.expose_secret(),
        &nonce,
        &aad,
        capsule.ciphertext.as_slice(),
    )?;
    require(!value.is_empty() && value.len() <= MAX_SECRET)?;
    Ok(OpenedServiceCredential {
        binding: expected.clone(),
        value,
        expires_at: capsule.expires_at,
        issued_at: capsule.issued_at,
    })
}

#[cfg(test)]
#[path = "service_credential_tests.rs"]
mod tests;

#[path="service_credential_authenticated_metadata.rs"]
mod authenticated_metadata;
