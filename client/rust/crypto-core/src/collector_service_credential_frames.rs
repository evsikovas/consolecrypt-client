//! Strict bounded metadata/ciphertext frames; no journal/admission authority.
use super::*;
use serde::{Deserialize, Serialize};
#[derive(Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", rename_all_fields = "camelCase", deny_unknown_fields)]
enum Payload {
    Authorization { authorization: Signed<CollectorServiceCredentialAuthorization> },
    Capsule { authorization_digest: Hash, slot: u8, capsule: ServiceCredentialCapsule },
}
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CollectorServiceCredentialFrame {
    version: u8,
    payload: Payload,
}
impl std::fmt::Debug for CollectorServiceCredentialFrame {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("CollectorServiceCredentialFrame([redacted])")
    }
}
impl CollectorServiceCredentialFrame {
    pub fn authorization(authorization: Signed<CollectorServiceCredentialAuthorization>) -> Result<Self> {
        let frame = Self { version: 1, payload: Payload::Authorization { authorization } };
        frame.to_json()?;
        Ok(frame)
    }
    pub fn capsule(authorization_digest: Hash, slot: u8, capsule: ServiceCredentialCapsule) -> Result<Self> {
        let frame = Self { version: 1, payload: Payload::Capsule { authorization_digest, slot, capsule } };
        frame.to_json()?;
        Ok(frame)
    }
    fn structure(&self) -> Result<()> {
        if self.version != 1 { return Err(Error::InvalidWire); }
        match &self.payload {
            Payload::Authorization { authorization } => authorization.body.validate(),
            Payload::Capsule { slot, capsule, .. } => {
                if *slot > 1 || capsule.to_json().map_err(|_| Error::InvalidWire)?.len() > 8192 {
                    return Err(Error::Quota);
                }
                Ok(())
            }
        }
    }
    pub fn to_json(&self) -> Result<Vec<u8>> {
        self.structure()?;
        let raw = serde_json::to_vec(self).map_err(|_| Error::InvalidWire)?;
        if raw.len() > 12288 { return Err(Error::Quota); }
        Ok(raw)
    }
    pub fn from_json(raw: &[u8]) -> Result<Self> {
        if raw.is_empty() || raw.len() > 12288 { return Err(Error::Quota); }
        let value: Self = serde_json::from_slice(raw).map_err(|_| Error::InvalidWire)?;
        value.structure()?;
        Ok(value)
    }
    /// Complete assembly is metadata only. The actual Core journal must verify
    /// the whole set and commit it atomically before any use. No partial batch.
    pub fn into_complete_set(frames: Vec<Self>)
        -> Result<(Signed<CollectorServiceCredentialAuthorization>, Vec<ServiceCredentialCapsule>)> {
        if frames.len() < 2 || frames.len() > 3 { return Err(Error::Quota); }
        let mut authorization = None;
        let mut capsules = Vec::new();
        for frame in frames {
            frame.structure()?;
            match frame.payload {
                Payload::Authorization { authorization: value } => {
                    if authorization.replace(value).is_some() { return Err(Error::Replay); }
                }
                Payload::Capsule { authorization_digest, slot, capsule } => {
                    if capsules.iter().any(|(_, old, _)| *old == slot) { return Err(Error::Replay); }
                    capsules.push((authorization_digest, slot, capsule));
                }
            }
        }
        let authorization: Signed<CollectorServiceCredentialAuthorization> = authorization.ok_or(Error::WrongClaim)?;
        let expected = authorization.body.digest()?;
        if capsules.len() != authorization.body.credentials.len()
            || capsules.iter().any(|(digest, _, _)| *digest != expected)
        { return Err(Error::WrongClaim); }
        capsules.sort_by_key(|(_, slot, _)| *slot);
        for (expected, (_, actual, _)) in capsules.iter().enumerate() {
            if *actual as usize != expected { return Err(Error::WrongClaim); }
        }
        Ok((authorization, capsules.into_iter().map(|(_, _, capsule)| capsule).collect()))
    }
}
