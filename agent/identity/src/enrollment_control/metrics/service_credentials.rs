//! Specific credential-file hook on the SAME Files/lock/actual unit owner.
//! Credential decryptor itself never unlinks a leaf or creates an observer.
use super::service_credential_handoff::{
    open_physical_namespace, EncryptedHandoffFrame, HeldHandoffLeaf, RecipientReference,
};
use super::*;

/// Borrow of the original locked Files owner, not a portable path/IPC grant.
/// Sibling fixed handoff operations accept this type; nobody can construct it
/// from a caller namespace string, deserialized proof, or replacement owner.
pub(crate) struct RegisteredServiceNamespace<'a> {
    files: &'a Files,
}
impl RegisteredServiceNamespace<'_> {
    pub(crate) fn check_held_directory(&self, directory: &File) -> Result<()> {
        original(self.files)?;
        let before = directory.metadata().map_err(|_| Error::Storage)?;
        if !before.is_dir()
            || before.uid() != service_uid()?
            || before.mode() & 0o7777 != 0o700
            || (before.dev(), before.ino()) != self.files.identity
        {
            return Err(Error::Storage);
        }
        original(self.files)?;
        let after = directory.metadata().map_err(|_| Error::Storage)?;
        if !after.is_dir()
            || after.uid() != before.uid()
            || after.mode() & 0o7777 != 0o700
            || (after.dev(), after.ino()) != self.files.identity
        {
            return Err(Error::Storage);
        }
        original(self.files)
    }
    pub(crate) fn session_id(&self) -> &str {
        &self.files.session
    }
    pub(crate) fn checked_directory(&self) -> Result<&Path> {
        original(self.files)?;
        Ok(&self.files.directory)
    }
    pub(crate) fn registered_leaf(&self, index: u8) -> Result<PathBuf> {
        let leaf = match index {
            0 => "service-recipient.json",
            1 => "credential-0.capsule.json",
            2 => "credential-1.capsule.json",
            _ => return Err(Error::Quota),
        };
        let path = self.checked_directory()?.join(leaf);
        if !self.files.paths.iter().any(|(p, _)| *p == path) {
            return Err(Error::WrongState);
        }
        Ok(path)
    }
}

pub(crate) struct RegisteredCredentialWriter<'a, 'permit, S: CompareAndSwap> {
    files: &'a mut Files,
    catalog: &'a Catalog,
    access: &'a mut CollectorServiceAccess<'permit, S>,
    now: Now,
    clock: &'a mut dyn FnMut() -> Result<CollectorServiceClockObservation>,
}
impl<S: CompareAndSwap> RegisteredCredentialWriter<'_, '_, S> {
    fn adopt_held(&mut self, index: usize, directory: &File, held: &HeldHandoffLeaf) -> Result<()> {
        held.recheck_physical(directory, &self.files.session)?;
        let path = {
            let reference = self.handoff_namespace()?;
            reference.check_held_directory(directory)?;
            reference.registered_leaf(index as u8)?
        };
        let metadata = fs::symlink_metadata(&path).map_err(|_| Error::Storage)?;
        let expected = held.original_inode_metadata();
        if !metadata.is_file()
            || metadata.file_type().is_symlink()
            || metadata.uid() != service_uid()?
            || metadata.nlink() != 1
            || metadata.mode() & 0o7777 != 0o400
            || (metadata.dev(), metadata.ino()) != expected
        {
            return Err(Error::Storage);
        }
        let slot = self
            .files
            .paths
            .iter_mut()
            .find(|(p, _)| *p == path)
            .ok_or(Error::WrongState)?;
        let actual = (expected.0, expected.1, 0o400);
        if slot.1.as_ref().is_some_and(|old| old.identity != actual) {
            return Err(Error::Conflict);
        }
        let original = OpenOptions::new()
            .read(true)
            .custom_flags(NOFOLLOW | NONBLOCK | CLOEXEC)
            .open(&path)
            .map_err(|_| Error::Storage)?;
        let retained = RetainedFile::capture(&original)?;
        if retained.identity != actual {
            return Err(Error::Storage);
        }
        slot.1 = Some(retained);
        held.recheck_physical(directory, &self.files.session)?;
        self.handoff_namespace()?.check_held_directory(directory)?;
        self.recheck()?;
        Ok(())
    }
    /// These are authenticated public/encrypted leaf adoptions ONLY. No bytes
    /// are decrypted here and no original credential inode can be overwritten.
    pub(super) fn retain_original_reference(
        &mut self,
        permit: &CollectorServicePermit,
        held: &HeldHandoffLeaf,
    ) -> Result<RecipientReference> {
        if !self.matches_original_permit(permit) {
            return Err(Error::WrongClaim);
        }
        let now = self.recheck()?;
        let reference = held.recipient_reference()?;
        reference.matches_original(permit)?;
        reference.check_metadata_against(self.access.policy(), now)?;
        if reference.namespace_id() != self.files.session {
            return Err(Error::WrongContext);
        }
        let directory = open_physical_namespace(&self.files.session)?;
        if {
            let m = directory.metadata().map_err(|_| Error::Storage)?;
            (m.dev(), m.ino())
        } != self.files.identity
        {
            return Err(Error::Storage);
        }
        self.adopt_held(0, &directory, held)?;
        let now = self.recheck()?;
        reference.check_metadata_against(self.access.policy(), now)?;
        Ok(reference)
    }
    pub(super) fn retain_encrypted_frame(
        &mut self,
        permit: &CollectorServicePermit,
        reference: &RecipientReference,
        slot: u8,
        held: &HeldHandoffLeaf,
    ) -> Result<EncryptedHandoffFrame> {
        if !self.matches_original_permit(permit) || slot > 1 {
            return Err(Error::WrongClaim);
        }
        let now = self.recheck()?;
        reference.matches_original(permit)?;
        if reference.namespace_id() != self.files.session {
            return Err(Error::WrongContext);
        }
        let frame = held.encrypted_frame()?;
        frame.check_owner_metadata(
            reference,
            self.access.owner_pin(),
            self.access.policy(),
            &self.catalog.manifest,
            now,
        )?;
        let directory = open_physical_namespace(&self.files.session)?;
        if {
            let m = directory.metadata().map_err(|_| Error::Storage)?;
            (m.dev(), m.ino())
        } != self.files.identity
        {
            return Err(Error::Storage);
        }
        self.adopt_held(slot as usize + 1, &directory, held)?;
        let now = self.recheck()?;
        frame.check_owner_metadata(
            reference,
            self.access.owner_pin(),
            self.access.policy(),
            &self.catalog.manifest,
            now,
        )?;
        Ok(frame)
    }
    pub(crate) fn handoff_namespace(&mut self) -> Result<RegisteredServiceNamespace<'_>> {
        self.recheck()?;
        Ok(RegisteredServiceNamespace {
            files: &*self.files,
        })
    }
    pub(crate) fn matches_original_permit(&self, permit: &CollectorServicePermit) -> bool {
        self.access.matches_original_permit(permit).is_ok()
    }
    pub(crate) fn narrow_expiry(&mut self, expires: Timestamp) -> Result<()> {
        self.access.narrow_expiry(expires)?;
        self.recheck().map(|_| ())
    }
    pub(crate) fn recheck(&mut self) -> Result<Now> {
        self.catalog.check()?;
        original(self.files)?;
        let now = self.access.recheck(self.now, &mut || (self.clock)())?;
        if self.catalog.disk.is_none()
            || self.catalog.id != self.access.policy().collector_id
            || self.catalog.hash != self.access.policy().manifest_sha256
        {
            return Err(Error::WrongClaim);
        }
        Ok(now)
    }
    pub(crate) fn write_scope_password(
        &mut self,
        scope_sha256: Hash,
        raw: &[u8],
    ) -> Result<String> {
        if raw.is_empty() || raw.len() > 4096 {
            return Err(Error::Quota);
        }
        self.recheck()?;
        let (leaf, mut file) = self
            .files
            .create_service_credential(scope_sha256, raw.len())?;
        // O_EXCL has happened, but ZERO plaintext bytes exist yet.
        self.recheck()?;
        self.files.check_original_credential(&leaf, &file)?;
        file.write_all(raw).map_err(|_| Error::Storage)?;
        file.sync_all().map_err(|_| Error::Storage)?;
        self.recheck()?;
        self.files.check_original_credential(&leaf, &file)?;
        Ok(leaf)
    }
}

fn original(files: &Files) -> Result<()> {
    let m = fs::symlink_metadata(&files.directory).map_err(|_| Error::Storage)?;
    let directory = files.directory_fd.metadata().map_err(|_| Error::Storage)?;
    if !m.is_dir()
        || m.file_type().is_symlink()
        || (m.dev(), m.ino()) != files.identity
        || m.uid() != service_uid()?
        || m.mode() & 0o7777 != 0o700
        || !directory.is_dir()
        || (directory.dev(), directory.ino()) != files.identity
        || directory.uid() != m.uid()
        || directory.mode() & 0o7777 != 0o700
    {
        return Err(Error::Storage);
    }
    let root = if files.disk { disk::ROOT } else { SESSION_ROOT };
    if files.directory.parent() != Some(Path::new(root)) {
        return Err(Error::WrongContext);
    }
    let held = files._lock.metadata().map_err(|_| Error::Storage)?;
    let named = fs::symlink_metadata(Path::new(root).join(".collector.lock"))
        .map_err(|_| Error::Storage)?;
    if !held.is_file()
        || held.nlink() != 1
        || held.uid() != service_uid()?
        || held.mode() & 0o777 != 0o600
        || (held.dev(), held.ino()) != (named.dev(), named.ino())
    {
        return Err(Error::Storage);
    }
    Ok(())
}

impl Files {
    fn check_original_credential(&self, leaf: &str, file: &File) -> Result<()> {
        let path = self.directory.join(leaf);
        let expected = self
            .paths
            .iter()
            .find(|(p, _)| *p == path)
            .and_then(|(_, id)| id.as_ref())
            .ok_or(Error::WrongState)?;
        let held = file.metadata().map_err(|_| Error::Storage)?;
        let named = fs::symlink_metadata(&path).map_err(|_| Error::Storage)?;
        for m in [&held, &named] {
            if !m.is_file()
                || m.file_type().is_symlink()
                || m.nlink() != 1
                || m.uid() != service_uid()?
                || m.mode() & 0o7777 != 0o400
                || (m.dev(), m.ino(), m.mode() & 0o777) != expected.identity
            {
                return Err(Error::Storage);
            }
        }
        Ok(())
    }
    pub(super) fn register_service_handoff(&mut self) -> Result<()> {
        original(self)?;
        let leaves = [
            "service-recipient.json",
            "credential-0.capsule.json",
            "credential-1.capsule.json",
        ];
        if self.paths.len() + leaves.len() > 20
            || leaves.iter().any(|leaf| {
                self.paths
                    .iter()
                    .any(|(path, _)| *path == self.directory.join(leaf))
            })
        {
            return Err(Error::Quota);
        }
        for leaf in leaves {
            self.paths.push((self.directory.join(leaf), None));
        }
        Ok(())
    }
    /// ONLY the original permit-owning worker calls this before adapter spawn.
    /// Scope SHA comes from the fully verified credential scope/capsule, not a
    /// caller path. Secret bytes never enter a receipt, manifest or log.
    pub(super) fn with_original_credential_session<T, S: CompareAndSwap>(
        &mut self,
        catalog: &Catalog,
        access: &mut CollectorServiceAccess<'_, S>,
        now: Now,
        clock: &mut impl FnMut() -> Result<CollectorServiceClockObservation>,
        work: impl FnOnce(&mut RegisteredCredentialWriter<'_, '_, S>) -> Result<T>,
    ) -> Result<T> {
        if !self.disk || !self.cleanup_allowed {
            return Err(Error::WrongState);
        }
        let value = {
            let mut writer = RegisteredCredentialWriter {
                files: &mut *self,
                catalog,
                access: &mut *access,
                now,
                clock: &mut *clock,
            };
            writer.recheck()?;
            work(&mut writer)?
        };
        catalog.check()?;
        original(self)?;
        access.recheck(now, clock)?;
        Ok(value)
    }
    /// Registered BEFORE O_EXCL. A failed/existing create remains uncreated;
    /// global cleanup skips it rather than deleting a foreign named leaf.
    fn create_service_credential(
        &mut self,
        scope_sha256: Hash,
        bytes: usize,
    ) -> Result<(String, File)> {
        if bytes == 0 || bytes > 16 * 1024 || self.paths.len() >= 20 {
            return Err(Error::Quota);
        }
        original(self)?;
        let leaf = format!(
            "credential-{}.password",
            hex::encode(scope_sha256.as_bytes())
        );
        let path = self.directory.join(&leaf);
        if self.paths.iter().any(|(old, _)| old == &path) {
            return Err(Error::Replay);
        }
        let index = self.paths.len();
        self.paths.push((path.clone(), None));
        let file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o400)
            .custom_flags(NOFOLLOW | NONBLOCK | CLOEXEC)
            .open(&path)
            .map_err(|_| Error::Storage)?;
        let held = file.metadata().map_err(|_| Error::Storage)?;
        let named = fs::symlink_metadata(&path).map_err(|_| Error::Storage)?;
        if !held.is_file()
            || held.nlink() != 1
            || held.uid() != service_uid()?
            || held.mode() & 0o777 != 0o400
            || (held.dev(), held.ino()) != (named.dev(), named.ino())
        {
            return Err(Error::Storage);
        }
        self.paths[index].1 = Some(RetainedFile::capture(&file)?);
        Ok((leaf, file))
    }
}
