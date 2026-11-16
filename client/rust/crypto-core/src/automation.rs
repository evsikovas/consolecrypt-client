//! A dedicated domain prevents reusing a job approval as a device login.
use crate::{
    device::DeviceSecretKeys,
    error::{CryptoError, Result},
};
use cc_models::automation::{JobManifest, JOB_SIGNATURE_DOMAIN};
use ed25519_dalek::{Signature, VerifyingKey};

fn message(manifest: &JobManifest) -> Result<Vec<u8>> {
    let canonical = manifest
        .canonical_bytes()
        .map_err(CryptoError::MalformedPlaintext)?;
    let mut message = Vec::with_capacity(JOB_SIGNATURE_DOMAIN.len() + canonical.len());
    message.extend_from_slice(JOB_SIGNATURE_DOMAIN);
    message.extend_from_slice(&canonical);
    Ok(message)
}

impl DeviceSecretKeys {
    /// Invoke only after explicit approval of this frozen manifest in app-core.
    /// No generic signing endpoint or private key is exposed.
    pub fn sign_automation_manifest(&self, manifest: &JobManifest) -> Result<[u8; 64]> {
        Ok(self.sign_message(&message(manifest)?))
    }
}

pub fn verify_automation_manifest(
    public: &[u8; 32],
    manifest: &JobManifest,
    signature: &[u8; 64],
) -> Result<()> {
    let key = VerifyingKey::from_bytes(public).map_err(|_| CryptoError::InvalidKey {
        what: "job approval public key",
    })?;
    key.verify_strict(&message(manifest)?, &Signature::from_bytes(signature))
        .map_err(|_| CryptoError::InvalidKey {
            what: "job approval signature",
        })
}
