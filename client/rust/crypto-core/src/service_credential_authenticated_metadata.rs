//! Child of existing service_credential. Reuse its exact capsule transcript.
use super::*;
impl ServiceCredentialCapsule {
    pub fn verify_owner_metadata(&self, expected: &ServiceCredentialBinding,
        owner: &[u8; 32], now: u64) -> Result<()> {
        self.structure()?;
        self.window(now)?;
        expected.validate()?;
        if &self.binding != expected || expected.owner_signing_key_sha256 != hash(owner) {
            return Err(ServiceCredentialError::Context);
        }
        let signature: [u8;64] = self.signature.to_array().ok_or(ServiceCredentialError::Invalid)?;
        VerifyingKey::from_bytes(owner).map_err(|_| ServiceCredentialError::Signature)?
            .verify_strict(&self.signing_bytes()?, &Signature::from_bytes(&signature))
            .map_err(|_| ServiceCredentialError::Signature)
    }
}
