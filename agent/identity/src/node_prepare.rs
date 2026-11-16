//! Prepare one inert epoch2 candidate. This cannot rotate or activate a key.
use crate::*;
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PreparedNodeKeyPublic {
    pub format: u32,
    pub agent_id: AgentId,
    pub rotation_id: String,
    pub from_key_epoch: u64,
    pub proposed_key_epoch: u64,
    pub old_key_fingerprint: Hash,
    pub new_key_fingerprint: Hash,
    pub new_public_key: PublicKey,
}

/// Trusted local preparation only: no active-key change, proof, scope or grant.
pub fn prepare_node_rotation(
    directory: &Path,
    agent_id: AgentId,
    expected_active_fingerprint: Hash,
) -> Result<PreparedNodeKeyPublic> {
    #[cfg(target_os = "linux")]
    {
        linux::prepare(directory, agent_id, expected_active_fingerprint)
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = (directory, agent_id, expected_active_fingerprint);
        Err(Error::CapabilityDenied)
    }
}

#[cfg(target_os = "linux")]
pub(crate) use linux::{check, existing_read_lock, lock, read_lock, sign_new_proof, Guard};
#[cfg(target_os = "linux")]
pub(crate) use linux::{existing_lock, guard_inode, open_checked, read, sign_selected_request};
#[cfg(target_os = "linux")]
pub(crate) use linux::{guard_identity, inspection_read_lock};
#[cfg(target_os = "linux")]
pub(crate) use linux::{sign_selected_collector_service_claim, sign_selected_collector_service_receipt};

#[cfg(all(test, target_os = "linux"))]
pub(crate) fn test_initial_directory_created() -> Result<()> {
    linux::test_initial_directory_created()
}
#[cfg(all(test, target_os = "linux"))]
pub(crate) fn test_rotation_nonce() -> Result<()> {
    linux::test_rotation_nonce()
}

#[cfg(target_os = "linux")]
mod linux {
include!("node_prepare_collector_credentials.rs");
    use super::*;
    use ed25519_dalek::Signer;
    use ed25519_dalek::SigningKey;
    use std::fs::{self, File, OpenOptions};
    use std::io::{Read, Write};
    use std::os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt};
    use std::path::PathBuf;
    use std::time::{Duration, Instant};
    use zeroize::Zeroizing;

    const LOCK: &str = ".node-keys.lock";
    const INTENT: &str = "node-rotation.intent.v2";
    const MATERIAL: &str = "node-rotation.pending.v2";
    const SELECTOR: &str = "node-rotation.state.v2";
    const MAGIC: &[u8; 16] = b"CC-NODE-PEND-V2\0";
    const MATERIAL_BYTES: usize = 212;
    const PUBLIC_BYTES: usize = 2048;

    #[derive(Serialize, Deserialize, PartialEq, Eq)]
    #[serde(rename_all = "camelCase", deny_unknown_fields)]
    struct Intent {
        format: u32,
        agent_id: AgentId,
        from_key_epoch: u64,
        proposed_key_epoch: u64,
        old_key_fingerprint: Hash,
    }
    #[derive(Serialize, Deserialize)]
    #[serde(rename_all = "camelCase", deny_unknown_fields)]
    struct Selector {
        format: u32,
        phase: Phase,
        prepared: PreparedNodeKeyPublic,
        intent_hash: Hash,
        material_hash: Hash,
    }
    #[derive(Serialize, Deserialize)]
    enum Phase {
        Prepared,
    }

    pub(crate) struct Guard {
        directory: PathBuf,
        file: File,
    }
    impl Guard {
        pub(crate) fn directory(&self) -> &Path {
            &self.directory
        }
    }
    fn private_metadata(metadata: &fs::Metadata) -> Result<()> {
        if !metadata.is_file()
            || metadata.nlink() != 1
            || metadata.permissions().mode() & 0o777 != 0o600
        {
            return Err(Error::Storage);
        }
        Ok(())
    }
    fn same_file(a: &fs::Metadata, b: &fs::Metadata) -> bool {
        a.dev() == b.dev() && a.ino() == b.ino()
    }
    pub(crate) fn open_checked(path: &Path) -> Result<File> {
        let before = fs::symlink_metadata(path).map_err(|_| Error::Storage)?;
        private_metadata(&before)?;
        let file = File::open(path).map_err(|_| Error::Storage)?;
        let opened = file.metadata().map_err(|_| Error::Storage)?;
        private_metadata(&opened)?;
        let after = fs::symlink_metadata(path).map_err(|_| Error::Storage)?;
        if !same_file(&before, &opened) || !same_file(&opened, &after) {
            return Err(Error::Storage);
        }
        Ok(file)
    }
    pub(crate) fn guard_inode(guard: &Guard) -> Result<()> {
        crate::durable::secure_path(&guard.directory)?;
        let metadata =
            fs::symlink_metadata(guard.directory.join(LOCK)).map_err(|_| Error::Storage)?;
        private_metadata(&metadata)?;
        if !same_file(
            &metadata,
            &guard.file.metadata().map_err(|_| Error::Storage)?,
        ) {
            return Err(Error::Storage);
        }
        Ok(())
    }
    pub(crate) fn lock(directory: &Path) -> Result<Guard> {
        acquire(directory, false, false, true)
    }
    pub(crate) fn read_lock(directory: &Path) -> Result<Guard> {
        acquire(directory, true, true, true)
    }
    pub(crate) fn existing_read_lock(directory: &Path) -> Result<Guard> {
        acquire(directory, true, true, false)
    }
    pub(crate) fn existing_lock(directory: &Path) -> Result<Guard> {
        acquire(directory, false, true, false)
    }
    /// Inspector-only O_RDONLY existing shared guard. Never creates an inode.
    pub(crate) fn inspection_read_lock(directory: &Path) -> Result<Guard> {
        let directory = crate::durable::absolute(directory)?;
        crate::durable::secure_path(&directory)?;
        if fs::symlink_metadata(&directory)
            .map_err(|_| Error::Storage)?
            .permissions()
            .mode()
            & 0o777
            != 0o700
        {
            return Err(Error::Storage);
        }
        if !fs::symlink_metadata(directory.join("node-identity.key"))
            .map_err(|_| Error::Storage)?
            .is_file()
        {
            return Err(Error::Storage);
        }
        let file = open_checked(&directory.join(LOCK))?; // File::open: O_RDONLY
        let started = Instant::now();
        loop {
            match file.try_lock_shared() {
                Ok(()) => break,
                Err(std::fs::TryLockError::WouldBlock)
                    if started.elapsed() < Duration::from_millis(100) =>
                {
                    std::thread::sleep(Duration::from_millis(2))
                }
                Err(std::fs::TryLockError::WouldBlock) => return Err(Error::Conflict),
                Err(std::fs::TryLockError::Error(_)) => return Err(Error::Storage),
            }
        }
        let guard = Guard { directory, file };
        guard_inode(&guard)?;
        Ok(guard)
    }
    pub(crate) fn guard_identity(guard: &Guard) -> Result<(u64, u64)> {
        guard_inode(guard)?;
        let metadata = guard.file.metadata().map_err(|_| Error::Storage)?;
        Ok((metadata.dev(), metadata.ino()))
    }
    fn acquire(
        directory: &Path,
        shared: bool,
        require_active: bool,
        create_lock: bool,
    ) -> Result<Guard> {
        let directory = crate::durable::absolute(directory)?;
        crate::durable::secure_path(&directory)?;
        // Denial only, never a substitute for locked full key validation. A
        // missing-key reader cannot steal the freshly created directory's lock
        // from its sole legitimate initial publisher.
        if require_active
            && !fs::symlink_metadata(directory.join("node-identity.key"))
                .map_err(|_| Error::Storage)?
                .is_file()
        {
            return Err(Error::Storage);
        }
        let path = directory.join(LOCK);
        match fs::symlink_metadata(&path) {
            Ok(metadata) => private_metadata(&metadata)?,
            Err(error) if create_lock && error.kind() == std::io::ErrorKind::NotFound => {
                // Denial only: v3 must never bootstrap a different lock inode.
                // Full key/activation checks still require the existing guard.
                for entry in fs::read_dir(&directory).map_err(|_| Error::Storage)? {
                    let entry = entry.map_err(|_| Error::Storage)?;
                    let name = entry.file_name();
                    if name
                        .to_str()
                        .ok_or(Error::Storage)?
                        .starts_with("node-activation.")
                    {
                        return Err(Error::Storage);
                    }
                }
            }
            Err(_) => return Err(Error::Storage),
        }
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(create_lock)
            .truncate(false)
            .mode(0o600)
            .open(&path)
            .map_err(|_| Error::Storage)?;
        private_metadata(&file.metadata().map_err(|_| Error::Storage)?)?;
        let started = Instant::now();
        loop {
            let acquired = if shared {
                file.try_lock_shared()
            } else {
                file.try_lock()
            };
            match acquired {
                Ok(()) => break,
                Err(std::fs::TryLockError::WouldBlock)
                    if started.elapsed() < Duration::from_millis(100) =>
                {
                    std::thread::sleep(Duration::from_millis(2))
                }
                Err(std::fs::TryLockError::WouldBlock) => return Err(Error::Conflict),
                Err(std::fs::TryLockError::Error(_)) => return Err(Error::Storage),
            }
        }
        let guard = Guard { directory, file };
        guard_inode(&guard)?;
        Ok(guard)
    }
    fn digest(bytes: &[u8]) -> Hash {
        crate::wire::hash(b"consolecrypt.agent.node-prepare-file.v2", bytes)
    }
    pub(crate) fn read(path: &Path, limit: usize) -> Result<Zeroizing<Vec<u8>>> {
        let file = open_checked(path)?;
        if file.metadata().map_err(|_| Error::Storage)?.len() > limit as u64 {
            return Err(Error::Storage);
        }
        let mut bytes = Zeroizing::new(Vec::new());
        file.take(limit as u64 + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| Error::Storage)?;
        if bytes.len() > limit {
            return Err(Error::Storage);
        }
        Ok(bytes)
    }
    fn inventory(guard: &Guard) -> Result<usize> {
        guard_inode(guard)?;
        let mut count = 0;
        for item in fs::read_dir(&guard.directory).map_err(|_| Error::Storage)? {
            let item = item.map_err(|_| Error::Storage)?;
            let name = item.file_name();
            let name = name.to_str().ok_or(Error::Storage)?;
            if name.starts_with("node-rotation.") {
                if ![INTENT, MATERIAL, SELECTOR].contains(&name) {
                    return Err(Error::Storage);
                }
                private_metadata(&fs::symlink_metadata(item.path()).map_err(|_| Error::Storage)?)?;
                count += 1;
            }
        }
        Ok(count)
    }
    fn pending(bytes: &[u8], active: &PublicNodeIdentity) -> Result<PreparedNodeKeyPublic> {
        if bytes.len() != MATERIAL_BYTES
            || &bytes[..16] != MAGIC
            || digest(&bytes[..180]).as_bytes() != &bytes[180..]
        {
            return Err(Error::Storage);
        }
        let id = std::str::from_utf8(&bytes[16..52]).map_err(|_| Error::Storage)?;
        if AgentId::new(id)? != active.agent_id
            || bytes[68..76] != 1u64.to_be_bytes()
            || bytes[76..84] != 2u64.to_be_bytes()
            || bytes[84..116] != *active.key_fingerprint.as_bytes()
        {
            return Err(Error::WrongClaim);
        }
        let mut seed = Zeroizing::new([0u8; 32]);
        seed.copy_from_slice(&bytes[116..148]);
        let signing = SigningKey::from_bytes(&seed);
        let public = PublicKey::from_bytes(signing.verifying_key().to_bytes());
        public.validate()?;
        if public.as_bytes() != &bytes[148..180] || public == active.public_key {
            return Err(Error::InvalidKey);
        }
        Ok(PreparedNodeKeyPublic {
            format: 1,
            agent_id: active.agent_id,
            rotation_id: hex::encode(&bytes[52..68]),
            from_key_epoch: 1,
            proposed_key_epoch: 2,
            old_key_fingerprint: active.key_fingerprint,
            new_key_fingerprint: public.fingerprint(),
            new_public_key: public,
        })
    }
    pub(crate) fn check(
        guard: &Guard,
        active: Option<&PublicNodeIdentity>,
    ) -> Result<Option<PreparedNodeKeyPublic>> {
        let count = inventory(guard)?;
        if count == 0 {
            return Ok(None);
        }
        if count != 3 {
            return Err(Error::UncertainCommit);
        }
        let active = active.ok_or(Error::Storage)?;
        let intent_bytes = read(&guard.directory.join(INTENT), PUBLIC_BYTES)?;
        let intent: Intent = serde_json::from_slice(&intent_bytes).map_err(|_| Error::Storage)?;
        if intent
            != (Intent {
                format: 1,
                agent_id: active.agent_id,
                from_key_epoch: 1,
                proposed_key_epoch: 2,
                old_key_fingerprint: active.key_fingerprint,
            })
        {
            return Err(Error::WrongClaim);
        }
        let material = read(&guard.directory.join(MATERIAL), MATERIAL_BYTES)?;
        let public = pending(&material, active)?;
        let selector_bytes = read(&guard.directory.join(SELECTOR), PUBLIC_BYTES)?;
        let selector: Selector =
            serde_json::from_slice(&selector_bytes).map_err(|_| Error::Storage)?;
        if selector.format != 2
            || selector.prepared != public
            || selector.intent_hash != digest(&intent_bytes)
            || selector.material_hash != digest(&material)
        {
            return Err(Error::WrongClaim);
        }
        guard_inode(guard)?;
        Ok(Some(public))
    }
    /// Internal closed new-key domain only. Callers cannot obtain key material.
    pub(crate) fn sign_new_proof(
        guard: &Guard,
        active: &PublicNodeIdentity,
        expected: &PreparedNodeKeyPublic,
        body: NewKeyProof,
    ) -> Result<Signed<NewKeyProof>> {
        if check(guard, Some(active))?.as_ref() != Some(expected)
            || body.0.agent_id != active.agent_id
        {
            return Err(Error::WrongClaim);
        }
        let material = read(&guard.directory.join(MATERIAL), MATERIAL_BYTES)?;
        if pending(&material, active)? != *expected {
            return Err(Error::WrongClaim);
        }
        let mut seed = Zeroizing::new([0u8; 32]);
        seed.copy_from_slice(&material[116..148]);
        let key = SigningKey::from_bytes(&seed);
        let signature = SignatureBytes::from_bytes(key.sign(&body.signing_bytes()?).to_bytes());
        guard_inode(guard)?;
        Ok(Signed { body, signature })
    }
    /// Closed AgentRequest domain, only after complete selected epoch2 state.
    pub(crate) fn sign_selected_request(
        guard: &Guard,
        active: &PublicNodeIdentity,
        body: AgentRequest,
    ) -> Result<Signed<AgentRequest>> {
        let selected = crate::node_activation::selected(guard, active)?.ok_or(Error::WrongState)?;
        if body.agent_id != active.agent_id || body.key_epoch != 2 {
            return Err(Error::WrongClaim);
        }
        let material = read(&guard.directory.join(MATERIAL), MATERIAL_BYTES)?;
        if pending(&material, active)? != selected.prepared {
            return Err(Error::WrongClaim);
        }
        let mut seed = Zeroizing::new([0u8; 32]);
        seed.copy_from_slice(&material[116..148]);
        let key = SigningKey::from_bytes(&seed);
        let signature = SignatureBytes::from_bytes(key.sign(&body.signing_bytes()?).to_bytes());
        guard_inode(guard)?;
        Ok(Signed { body, signature })
    }
    // Closed collector-service purpose under the SAME selected Node guard.
    pub(crate) fn sign_selected_collector_service_claim(
        guard: &Guard, active: &PublicNodeIdentity, body: CollectorServiceClaim,
    ) -> Result<Signed<CollectorServiceClaim>> {
        let selected=crate::node_activation::selected(guard,active)?.ok_or(Error::WrongState)?;
        if body.agent_id!=active.agent_id || body.key_epoch!=2 { return Err(Error::WrongClaim); }
        let material=read(&guard.directory.join(MATERIAL),MATERIAL_BYTES)?;
        if pending(&material,active)?!=selected.prepared { return Err(Error::WrongClaim); }
        let mut seed=Zeroizing::new([0u8;32]);seed.copy_from_slice(&material[116..148]);
        let key=SigningKey::from_bytes(&seed);
        let signature=SignatureBytes::from_bytes(key.sign(&body.signing_bytes()?).to_bytes());
        guard_inode(guard)?;Ok(Signed {body,signature})
    }
    // Closed collector-service purpose under the SAME selected Node guard.
    pub(crate) fn sign_selected_collector_service_receipt(
        guard: &Guard, active: &PublicNodeIdentity, body: CollectorServiceReceipt,
    ) -> Result<Signed<CollectorServiceReceipt>> {
        let selected=crate::node_activation::selected(guard,active)?.ok_or(Error::WrongState)?;
        if body.agent_id!=active.agent_id || body.key_epoch!=2 { return Err(Error::WrongClaim); }
        let material=read(&guard.directory.join(MATERIAL),MATERIAL_BYTES)?;
        if pending(&material,active)?!=selected.prepared { return Err(Error::WrongClaim); }
        let mut seed=Zeroizing::new([0u8;32]);seed.copy_from_slice(&material[116..148]);
        let key=SigningKey::from_bytes(&seed);
        let signature=SignatureBytes::from_bytes(key.sign(&body.signing_bytes()?).to_bytes());
        guard_inode(guard)?;Ok(Signed {body,signature})
    }
    // New closed Node purpose under the original selected-key guard.
    pub(crate) fn sign_selected_collector_service_set_receipt(
        guard: &Guard, active: &PublicNodeIdentity, body: crate::CollectorServiceCredentialSetReceipt,
    ) -> Result<Signed<crate::CollectorServiceCredentialSetReceipt>> {
        let selected = crate::node_activation::selected(guard, active)?.ok_or(Error::WrongState)?;
        body.validate()?;
        if body.agent_id != active.agent_id || body.key_epoch != 2 { return Err(Error::WrongClaim); }
        let material = read(&guard.directory.join(MATERIAL), MATERIAL_BYTES)?;
        if pending(&material, active)? != selected.prepared { return Err(Error::WrongClaim); }
        let mut seed = Zeroizing::new([0u8;32]); seed.copy_from_slice(&material[116..148]);
        let key = SigningKey::from_bytes(&seed);
        let signature = SignatureBytes::from_bytes(key.sign(&body.signing_bytes()?).to_bytes());
        guard_inode(guard)?;
        Ok(Signed { body, signature })
    }

    fn write_new(guard: &Guard, name: &str, bytes: &[u8]) -> Result<()> {
        guard_inode(guard)?;
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(guard.directory.join(name))
            .map_err(io_failure)?;
        #[cfg(test)]
        tests::fault("after-create", name)?;
        file.write_all(bytes).map_err(io_failure)?;
        file.sync_all().map_err(|_| Error::UncertainCommit)?;
        #[cfg(test)]
        tests::fault("after-file-sync", name)?;
        File::open(&guard.directory)
            .and_then(|file| file.sync_all())
            .map_err(|_| Error::UncertainCommit)?;
        #[cfg(test)]
        tests::fault("after-dir-sync", name)?;
        Ok(())
    }
    fn io_failure(error: std::io::Error) -> Error {
        #[cfg(test)]
        tests::io_failure(error.raw_os_error());
        #[cfg(not(test))]
        let _ = error;
        Error::Storage
    }
    fn resync_prepared(guard: &Guard) -> Result<()> {
        guard_inode(guard)?;
        for name in [INTENT, MATERIAL, SELECTOR] {
            open_checked(&guard.directory.join(name))?
                .sync_all()
                .map_err(|_| Error::UncertainCommit)?;
        }
        File::open(&guard.directory)
            .and_then(|file| file.sync_all())
            .map_err(|_| Error::UncertainCommit)
    }
    pub(super) fn prepare(
        directory: &Path,
        agent_id: AgentId,
        expected: Hash,
    ) -> Result<PreparedNodeKeyPublic> {
        let guard = acquire(directory, false, true, true)?;
        crate::node_activation::deny_presence(&guard)?;
        let active = crate::provision::load_existing_under_guard(&guard, agent_id)?;
        if active.key_fingerprint != expected {
            return Err(Error::WrongClaim);
        }
        if check(&guard, Some(&active))?.is_some() {
            resync_prepared(&guard)?;
            return check(&guard, Some(&active))?.ok_or(Error::UncertainCommit);
        }
        let intent = Intent {
            format: 1,
            agent_id,
            from_key_epoch: 1,
            proposed_key_epoch: 2,
            old_key_fingerprint: expected,
        };
        let intent = serde_json::to_vec(&intent).map_err(|_| Error::Storage)?;
        // No entropy before the expected local identity and durable intent.
        write_new(&guard, INTENT, &intent)?;
        #[cfg(test)]
        tests::fault("before-entropy", MATERIAL)?;
        let mut random = Zeroizing::new([0u8; 48]);
        getrandom::fill(random.as_mut()).map_err(|_| Error::Entropy)?;
        let mut seed = Zeroizing::new([0u8; 32]);
        seed.copy_from_slice(&random[16..]);
        let signing = SigningKey::from_bytes(&seed);
        let public = PublicKey::from_bytes(signing.verifying_key().to_bytes());
        public.validate()?;
        let mut material = Zeroizing::new([0u8; MATERIAL_BYTES]);
        material[..16].copy_from_slice(MAGIC);
        let id = serde_json::to_value(agent_id).map_err(|_| Error::InvalidWire)?;
        material[16..52].copy_from_slice(id.as_str().ok_or(Error::InvalidWire)?.as_bytes());
        material[52..68].copy_from_slice(&random[..16]);
        material[68..76].copy_from_slice(&1u64.to_be_bytes());
        material[76..84].copy_from_slice(&2u64.to_be_bytes());
        material[84..116].copy_from_slice(expected.as_bytes());
        material[116..148].copy_from_slice(seed.as_ref());
        material[148..180].copy_from_slice(public.as_bytes());
        let checksum = digest(&material[..180]);
        material[180..].copy_from_slice(checksum.as_bytes());
        write_new(&guard, MATERIAL, material.as_ref())?;
        let public = pending(material.as_ref(), &active)?;
        let selector = Selector {
            format: 2,
            phase: Phase::Prepared,
            prepared: public.clone(),
            intent_hash: digest(&intent),
            material_hash: digest(material.as_ref()),
        };
        write_new(
            &guard,
            SELECTOR,
            &serde_json::to_vec(&selector).map_err(|_| Error::Storage)?,
        )?;
        let readback = check(&guard, Some(&active))?.ok_or(Error::UncertainCommit)?;
        #[cfg(test)]
        tests::fault("after-readback", SELECTOR)?;
        Ok(readback)
    }

    #[cfg(test)]
    pub(super) fn test_initial_directory_created() -> Result<()> {
        tests::initial_directory_created()
    }
    #[cfg(test)]
    pub(super) fn test_rotation_nonce() -> Result<()> {
        tests::rotation_nonce_point()
    }
    #[cfg(test)]
    mod tests {
        use super::*;
        include!("node_prepare_tests.rs");
        include!("node_rotation_signer_tests.rs");
    }
}

#[cfg(target_os = "linux")]
pub(crate) use linux::sign_selected_collector_recipient;

#[cfg(target_os="linux")]
pub(crate) use linux::sign_selected_collector_service_set_receipt;
