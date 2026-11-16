//! Fixed private recipient custody; no enrollment, generic key import or grant.
use crate::{AgentId, Error, Hash, OwnerPin, ResourceId, Result};
use cc_crypto_core::DeviceSecretKeys;
use sha2::{Digest, Sha256};
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::os::fd::AsRawFd;
use std::os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use zeroize::Zeroizing;

const ROOT: &str = "/var/lib/consolecrypt-agent/collector-credentials";
const KEY: &str = "recipient.key";
const NOFOLLOW: i32 = 0x20000;
const CLOEXEC: i32 = 0x80000;
const NONBLOCK: i32 = 0x800;
const DIRECTORY: i32 = 0x10000;
const MAGIC: &[u8; 8] = b"CCACR\0\0\x01";
const ENCODED: usize = 285;

/// Trusted binding is built from the SAME checked Actor/installed Node, never
/// deserialized as permission. Recipient ID is known before explicit creation.
#[derive(Clone, PartialEq, Eq)]
pub(super) struct Binding {
    pub(super) agent_id: AgentId,
    pub(super) key_epoch: u64,
    pub(super) certificate_hash: Hash,
    pub(super) owner_identity: Hash,
    pub(super) recipient_id: ResourceId,
}
impl Binding {
    pub(super) fn owner_identity(pin: &OwnerPin) -> Result<Hash> {
        let value = serde_json::json!({"context":pin.context,"key":pin.key,"epoch":pin.epoch});
        let mut bytes = Vec::new();
        crate::wire::canonical_json(&value, &mut bytes)?;
        Ok(Hash::from_bytes(Sha256::digest(bytes).into()))
    }
    fn validate(&self) -> Result<()> {
        if self.key_epoch == 0 { return Err(Error::InvalidWire); }
        Ok(())
    }
}
fn string_id(id: &impl serde::Serialize) -> Result<String> {
    serde_json::to_value(id).map_err(|_| Error::InvalidWire)?
        .as_str().map(str::to_owned).filter(|v| v.len() == 36).ok_or(Error::InvalidWire)
}
fn inode(m: &fs::Metadata) -> (u64, u64) { (m.dev(), m.ino()) }
fn stable(m: &fs::Metadata) -> (u64, u64, u32, u32, u64, u64, i64, i64, i64, i64) {
    (m.dev(), m.ino(), m.mode(), m.uid(), m.nlink(), m.len(), m.mtime(), m.mtime_nsec(), m.ctime(), m.ctime_nsec())
}
fn uid() -> Result<u32> {
    let value = fs::metadata("/proc/self").map_err(|_| Error::Storage)?.uid();
    if value == 0 { return Err(Error::CapabilityDenied); }
    Ok(value)
}
fn private_key(m: &fs::Metadata, owner: u32) -> Result<()> {
    if !m.is_file() || m.uid() != owner || m.nlink() != 1
        || m.permissions().mode() & 0o7777 != 0o600 || m.len() != ENCODED as u64
    { return Err(Error::Storage); }
    Ok(())
}
fn open_directory(owner: u32) -> Result<File> {
    for parent in Path::new(ROOT).ancestors().skip(1) {
        let info = fs::symlink_metadata(parent).map_err(|_| Error::Storage)?;
        if !info.is_dir() || info.file_type().is_symlink()
            || ![0, owner].contains(&info.uid()) || info.mode() & 0o022 != 0
        { return Err(Error::Storage); }
    }
    let named = fs::symlink_metadata(ROOT).map_err(|_| Error::Storage)?;
    if !named.is_dir() || named.file_type().is_symlink() || named.uid() != owner
        || named.mode() & 0o7777 != 0o700 { return Err(Error::Storage); }
    let file = OpenOptions::new().read(true).custom_flags(DIRECTORY | NOFOLLOW | CLOEXEC | NONBLOCK)
        .open(ROOT).map_err(|_| Error::Storage)?;
    if inode(&named) != inode(&file.metadata().map_err(|_| Error::Storage)?) { return Err(Error::Storage); }
    Ok(file)
}
fn leaf(directory: &File) -> PathBuf {
    // Kernel-resolved original held directory, fixed leaf. No input path exists.
    PathBuf::from(format!("/proc/self/fd/{}", directory.as_raw_fd())).join(KEY)
}
fn encode(binding: &Binding, keys: &DeviceSecretKeys) -> Result<Zeroizing<Vec<u8>>> {
    binding.validate()?;
    let secret = keys.to_secret_bytes();
    if secret.len() != 69 { return Err(Error::InvalidKey); }
    let mut bytes = Zeroizing::new(Vec::with_capacity(ENCODED));
    bytes.extend_from_slice(MAGIC);
    bytes.extend_from_slice(string_id(&binding.agent_id)?.as_bytes());
    bytes.extend_from_slice(&binding.key_epoch.to_be_bytes());
    bytes.extend_from_slice(binding.certificate_hash.as_bytes());
    bytes.extend_from_slice(binding.owner_identity.as_bytes());
    bytes.extend_from_slice(string_id(&binding.recipient_id)?.as_bytes());
    bytes.extend_from_slice(&keys.public_keys().encryption);
    bytes.extend_from_slice(&secret);
    let checksum = Sha256::digest(&bytes);
    bytes.extend_from_slice(&checksum);
    if bytes.len() != ENCODED { return Err(Error::InvalidWire); }
    Ok(bytes)
}
fn decode(bytes: &[u8], binding: &Binding) -> Result<DeviceSecretKeys> {
    if bytes.len() != ENCODED { return Err(Error::InvalidWire); }
    let keys = DeviceSecretKeys::from_secret_bytes(&bytes[184..253]).map_err(|_| Error::InvalidKey)?;
    if encode(binding, &keys)?.as_slice() != bytes { return Err(Error::WrongClaim); }
    Ok(keys)
}

/// Retains original directory/key descriptors, metadata and decoded private
/// material. No Clone/Serde/Debug or secret-byte accessor is exposed.
pub(super) struct Recipient {
    directory: File,
    file: File,
    directory_inode: (u64, u64),
    file_identity: (u64, u64, u32, u32, u64, u64, i64, i64, i64, i64),
    binding: Binding,
    keys: DeviceSecretKeys,
    owner: u32,
}
impl Recipient {
    pub(super) fn create(binding: Binding) -> Result<Self> {
        binding.validate()?;
        let owner = uid()?;
        let directory = open_directory(owner)?;
        let directory_inode = inode(&directory.metadata().map_err(|_| Error::Storage)?);
        let keys = DeviceSecretKeys::generate().map_err(|_| Error::Entropy)?;
        let bytes = encode(&binding, &keys)?;
        // Crossing O_EXCL is the mutation boundary. Failure afterwards is
        // unknown and never permits automatic delete, overwrite or regeneration.
        let mut file = OpenOptions::new().read(true).write(true).create_new(true).mode(0o600)
            .custom_flags(NOFOLLOW | CLOEXEC | NONBLOCK).open(leaf(&directory))
            .map_err(|e| if e.kind() == std::io::ErrorKind::AlreadyExists { Error::Conflict } else { Error::UncertainCommit })?;
        let finish = (|| -> Result<()> {
            file.try_lock().map_err(|_| Error::Storage)?;
            file.set_permissions(fs::Permissions::from_mode(0o600)).map_err(|_| Error::Storage)?;
            file.write_all(&bytes).map_err(|_| Error::Storage)?;
            file.sync_all().map_err(|_| Error::Storage)?;
            directory.sync_all().map_err(|_| Error::Storage)?;
            if inode(&fs::symlink_metadata(ROOT).map_err(|_| Error::Storage)?) != directory_inode {
                return Err(Error::Storage);
            }
            Ok(())
        })();
        finish.map_err(|_| Error::UncertainCommit)?;
        let file_identity = stable(&file.metadata().map_err(|_| Error::UncertainCommit)?);
        let mut result = Self { directory, file, directory_inode, file_identity, binding, keys, owner };
        result.check().map_err(|_| Error::UncertainCommit)?;
        Ok(result)
    }

    pub(super) fn open_existing(binding: Binding) -> Result<Self> {
        binding.validate()?;
        let owner = uid()?;
        let directory = open_directory(owner)?;
        let directory_inode = inode(&directory.metadata().map_err(|_| Error::Storage)?);
        let mut file = OpenOptions::new().read(true).custom_flags(NOFOLLOW | CLOEXEC | NONBLOCK)
            .open(leaf(&directory)).map_err(|_| Error::Storage)?;
        file.try_lock_shared().map_err(|_| Error::Storage)?;
        private_key(&file.metadata().map_err(|_| Error::Storage)?, owner)?;
        let file_identity = stable(&file.metadata().map_err(|_| Error::Storage)?);
        let mut bytes = Zeroizing::new(vec![0; ENCODED]);
        file.read_exact(&mut bytes).map_err(|_| Error::Storage)?;
        let mut extra = [0];
        if file.read(&mut extra).map_err(|_| Error::Storage)? != 0 { return Err(Error::Storage); }
        let keys = decode(&bytes, &binding)?;
        let mut result = Self { directory, file, directory_inode, file_identity, binding, keys, owner };
        result.check()?;
        Ok(result)
    }

    pub(super) fn check(&mut self) -> Result<()> {
        let current_directory = open_directory(self.owner)?;
        if inode(&current_directory.metadata().map_err(|_| Error::Storage)?) != self.directory_inode {
            return Err(Error::Storage);
        }
        if uid()? != self.owner || inode(&self.directory.metadata().map_err(|_| Error::Storage)?) != self.directory_inode
            || inode(&fs::symlink_metadata(ROOT).map_err(|_| Error::Storage)?) != self.directory_inode
        { return Err(Error::Storage); }
        let named = fs::symlink_metadata(leaf(&self.directory)).map_err(|_| Error::Storage)?;
        let held = self.file.metadata().map_err(|_| Error::Storage)?;
        private_key(&named, self.owner)?;
        private_key(&held, self.owner)?;
        if stable(&named) != self.file_identity || stable(&held) != self.file_identity { return Err(Error::Storage); }
        self.file.seek(SeekFrom::Start(0)).map_err(|_| Error::Storage)?;
        let mut bytes = Zeroizing::new(vec![0; ENCODED]);
        self.file.read_exact(&mut bytes).map_err(|_| Error::Storage)?;
        if encode(&self.binding, &self.keys)?.as_slice() != bytes.as_slice()
            || stable(&self.file.metadata().map_err(|_| Error::Storage)?) != self.file_identity
            || stable(&fs::symlink_metadata(leaf(&self.directory)).map_err(|_| Error::Storage)?) != self.file_identity
        { return Err(Error::Storage); }
        Ok(())
    }
    pub(super) fn public(&mut self) -> Result<[u8; 32]> {
        self.check()?;
        Ok(self.keys.public_keys().encryption)
    }
    pub(super) fn id(&self) -> ResourceId { self.binding.recipient_id }
    pub(super) fn check_binding(&mut self, expected: &Binding) -> Result<()> {
        if &self.binding != expected { return Err(Error::WrongClaim); }
        self.check()
    }
    pub(super) fn with_keys<T>(&mut self, action: impl FnOnce(&DeviceSecretKeys) -> Result<T>) -> Result<T> {
        self.check()?;
        let result = action(&self.keys)?;
        self.check()?;
        Ok(result)
    }
}

#[cfg(test)]
#[path = "recipient_tests.rs"]
mod tests;
