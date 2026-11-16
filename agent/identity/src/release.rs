//! Signature/digest/compatibility checks only. No installation, service changes,
//! private key access or execution of the verified artifact.
use crate::*;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::io::Read;
use std::path::Path;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ReleaseSignerPin {
    pub format: u32,
    pub public_key: PublicKey,
}

/// Opaque verification result. It cannot be reconstructed from untrusted JSON.
#[derive(Debug)]
pub struct VerifiedRelease {
    manifest: ReleaseManifest,
}
impl VerifiedRelease {
    pub fn manifest(&self) -> &ReleaseManifest {
        &self.manifest
    }
}

pub fn verify_release(
    signed: Signed<ReleaseManifest>,
    pin: &ReleaseSignerPin,
    artifact: &Path,
    platform: &str,
    protocol: u32,
    highest_version: u64,
    now: Now,
) -> Result<VerifiedRelease> {
    if pin.format != 1 {
        return Err(Error::InvalidWire);
    }
    signed.verify(pin.public_key)?;
    let manifest = &signed.body;
    if manifest.platform != platform
        || manifest.protocol != protocol
        || protocol != 1
        || manifest.capabilities.version != 1
    {
        return Err(Error::CapabilityDenied);
    }
    if manifest.version <= highest_version {
        return Err(Error::WrongEpoch);
    }
    crate::wire::live(manifest.issued_at, manifest.expires_at, now)?;
    if !artifact.is_absolute() {
        return Err(Error::Storage);
    }
    for path in artifact.ancestors() {
        let meta = std::fs::symlink_metadata(path).map_err(|_| Error::Storage)?;
        if meta.file_type().is_symlink() {
            return Err(Error::Storage);
        }
        #[cfg(windows)]
        {
            use std::os::windows::fs::MetadataExt;
            if meta.file_attributes() & 0x400 != 0 {
                return Err(Error::Storage);
            }
        }
    }
    let meta = std::fs::symlink_metadata(artifact).map_err(|_| Error::Storage)?;
    if !meta.is_file() || meta.len() != manifest.artifact_bytes {
        return Err(Error::InvalidWire);
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if meta.nlink() != 1 {
            return Err(Error::Storage);
        }
    }
    let mut file = std::fs::File::open(artifact)
        .map_err(|_| Error::Storage)?
        .take(manifest.artifact_bytes + 1);
    let mut digest = Sha256::new();
    let mut total = 0u64;
    let mut buffer = [0u8; 32 * 1024];
    loop {
        let read = file.read(&mut buffer).map_err(|_| Error::Storage)?;
        if read == 0 {
            break;
        }
        digest.update(&buffer[..read]);
        total += read as u64;
        if total > manifest.artifact_bytes {
            return Err(Error::Quota);
        }
    }
    if total != manifest.artifact_bytes
        || digest.finalize().as_slice() != manifest.artifact_sha256.as_bytes()
    {
        return Err(Error::InvalidSignature);
    }
    Ok(VerifiedRelease {
        manifest: signed.body,
    })
}

/// In-process activation planning, not a filesystem updater. Rollback keeps the
/// high-water version; a previously rejected/replayed package cannot become a
/// fresh update after reverting the running slot. Durable delivery must retain
/// this high-water independently and verify disk bytes again before activation.
#[derive(Debug, Default)]
pub struct ReleaseSlots {
    current: Option<VerifiedRelease>,
    previous: Option<VerifiedRelease>,
    highest: u64,
}
impl ReleaseSlots {
    pub fn highest_version(&self) -> u64 {
        self.highest
    }
    pub fn current(&self) -> Option<&ReleaseManifest> {
        self.current.as_ref().map(VerifiedRelease::manifest)
    }
    pub fn activate(&mut self, verified: VerifiedRelease) -> Result<()> {
        if verified.manifest.version <= self.highest {
            return Err(Error::WrongEpoch);
        }
        self.highest = verified.manifest.version;
        self.previous = self.current.take();
        self.current = Some(verified);
        Ok(())
    }
    pub fn rollback(&mut self) -> Result<()> {
        let previous = self.previous.take().ok_or(Error::NotFound)?;
        self.current = Some(previous);
        Ok(())
    }
}
