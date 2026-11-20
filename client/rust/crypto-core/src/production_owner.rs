//! Fixed OWNER-confirmation purpose. Cryptographic validity alone is not a
//! manual-owner grant: Core and Runner must also require their original manual
//! producer type. No generic signer or passphrase material crosses this module.
use crate::{CryptoError, DeviceSecretKeys, Result};
use base64::{engine::general_purpose::STANDARD, Engine};
use cc_models::automation::{JobManifest, JOB_SIGNATURE_DOMAIN};
use ed25519_dalek::{Signature, VerifyingKey};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use uuid::Uuid;

pub const PRODUCTION_OWNER_DOMAIN: &[u8] = b"ConsoleCrypt Production OwnerConfirmation v1\0";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ProductionOwnerPolicy {
    #[serde(rename = "ownerConfirmationVaultPassphraseV1")]
    OwnerConfirmationVaultPassphraseV1,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OwnerConfirmation {
    version: u8,
    policy: ProductionOwnerPolicy,
    signer_id: Uuid,
    manifest_digest: String,
    signature: String,
}

fn invalid() -> CryptoError {
    CryptoError::MalformedPlaintext("invalid production owner confirmation")
}
fn digest(manifest: &JobManifest) -> Result<[u8; 32]> {
    let raw = manifest
        .canonical_bytes()
        .map_err(CryptoError::MalformedPlaintext)?;
    let mut hasher = Sha256::new();
    hasher.update(JOB_SIGNATURE_DOMAIN);
    hasher.update(raw);
    Ok(hasher.finalize().into())
}
pub fn production_manifest_digest(manifest: &JobManifest) -> Result<String> {
    if !manifest.policy.production {
        return Err(invalid());
    }
    Ok(encode_digest(&digest(manifest)?))
}
fn encode_digest(digest: &[u8; 32]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut encoded = String::with_capacity(64);
    for byte in digest {
        encoded.push(HEX[(byte >> 4) as usize] as char);
        encoded.push(HEX[(byte & 15) as usize] as char);
    }
    encoded
}
fn message(signer: Uuid, digest: &[u8; 32]) -> Result<Vec<u8>> {
    if signer.is_nil() {
        return Err(invalid());
    }
    // Fixed binary encoding: purpose || version1 || canonical UUID16 || digest32.
    // No arbitrary JSON/strings/key/epoch input to the signing key.
    let mut raw = Vec::with_capacity(PRODUCTION_OWNER_DOMAIN.len() + 49);
    raw.extend_from_slice(PRODUCTION_OWNER_DOMAIN);
    raw.push(1);
    raw.extend_from_slice(signer.as_bytes());
    raw.extend_from_slice(digest);
    Ok(raw)
}
impl DeviceSecretKeys {
    /// AppCore MUST first perform original-session vault-passphrase step-up
    /// against the exact frozen manifest. This signature is not a grant and is
    /// rejected for all Service/Node producers even if cryptographically valid.
    pub fn sign_production_owner_confirmation(
        &self,
        manifest: &JobManifest,
        signer_id: Uuid,
    ) -> Result<OwnerConfirmation> {
        if !manifest.policy.production {
            return Err(invalid());
        }
        let digest = digest(manifest)?;
        Ok(OwnerConfirmation {
            version: 1,
            policy: ProductionOwnerPolicy::OwnerConfirmationVaultPassphraseV1,
            signer_id,
            manifest_digest: encode_digest(&digest),
            signature: STANDARD.encode(self.sign_message(&message(signer_id, &digest)?)),
        })
    }
}
impl OwnerConfirmation {
    pub fn manifest_digest(&self) -> &str {
        &self.manifest_digest
    }
    pub fn verify(
        &self,
        manifest: &JobManifest,
        original_signer: Uuid,
        original_key: &[u8; 32],
    ) -> Result<()> {
        if self.version != 1
            || self.policy != ProductionOwnerPolicy::OwnerConfirmationVaultPassphraseV1
            || !manifest.policy.production
            || self.signer_id != original_signer
            || self.signature.len() != 88
        {
            return Err(invalid());
        }
        let digest = digest(manifest)?;
        if self.manifest_digest != encode_digest(&digest) {
            return Err(invalid());
        }
        let signature: [u8; 64] = STANDARD
            .decode(&self.signature)
            .map_err(|_| invalid())?
            .try_into()
            .map_err(|_| invalid())?;
        VerifyingKey::from_bytes(original_key)
            .map_err(|_| invalid())?
            .verify_strict(
                &message(original_signer, &digest)?,
                &Signature::from_bytes(&signature),
            )
            .map_err(|_| invalid())
    }
}

#[cfg(test)]
mod tests;
