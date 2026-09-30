//! Independent sharing high-water marks in the OS secure store.
//!
//! SecureStore does not implement compare-and-swap. Cooperating processes
//! therefore take an OS file lock in the same user's temporary namespace
//! before reading or replacing any mark. The lock is independent of database
//! paths, so a restored/copy database cannot race the original database and
//! lower a mark. Lock files contain no data and are never unlinked (unlinking
//! a held file would create a second lock domain).
//!
//! Marks contain only authenticated public identities and signed-document
//! hashes/counters. They contain neither plaintext projections nor DEKs.
//! This does not protect against rollback/loss of the secure store itself,
//! an external process bypassing this locking protocol, or a malicious server
//! withholding a newer state which this device has never observed.

use crate::sharing_state::SharingBinding;
use cc_crypto_core::sharing::{SharingOwnerAnchor, SharingRevisionCheckpoint};
use cc_crypto_core::DevicePublicKeys;
use cc_platform_core::{ExposeSecret, SecureStore, SecureStoreError, MAX_SECRET_LEN};
use cc_protocol::{DeviceId, UserId};
use cc_storage_core::ProfileId;
use serde::{Deserialize, Serialize};
use std::fs::{File, OpenOptions};
use std::sync::Arc;

const MARKER_FORMAT: u16 = 1;

/// Errors never include secure-store bytes or decrypted sharing data.
#[derive(Debug, thiserror::Error)]
pub enum SharingHighwaterError {
    #[error(transparent)]
    SecureStore(#[from] SecureStoreError),
    #[error("sharing high-water marker is invalid")]
    Corrupt,
    #[error("sharing high-water marker belongs to another profile or item")]
    BindingMismatch,
    #[error("sharing high-water coordination lock is unavailable")]
    LockUnavailable,
    #[error("sharing high-water marker write could not be verified")]
    WriteNotVerified,
}

/// Only constructed from a snapshot whose signatures have been authenticated.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct HighwaterMarker {
    format: u16,
    profile_id: ProfileId,
    binding: SharingBinding,
    owner_user_id: UserId,
    owner_device_id: DeviceId,
    owner_encryption_key: [u8; 32],
    owner_signing_key: [u8; 32],
    revision: i64,
    revision_hash: [u8; 32],
    manifest_revision: u64,
    manifest_hash: [u8; 32],
    access_epoch: u64,
}

impl HighwaterMarker {
    pub(crate) fn authenticated(
        profile_id: ProfileId,
        binding: SharingBinding,
        owner: SharingOwnerAnchor,
        checkpoint: SharingRevisionCheckpoint,
    ) -> Self {
        Self {
            format: MARKER_FORMAT,
            profile_id,
            binding,
            owner_user_id: owner.user_id,
            owner_device_id: owner.device_id,
            owner_encryption_key: owner
                .public_keys
                .encryption_bytes()
                .as_slice()
                .try_into()
                .expect("typed public key length"),
            owner_signing_key: owner
                .public_keys
                .signing_bytes()
                .as_slice()
                .try_into()
                .expect("typed public key length"),
            revision: checkpoint.revision,
            revision_hash: checkpoint.hash,
            manifest_revision: checkpoint.manifest_revision,
            manifest_hash: checkpoint.manifest_hash,
            access_epoch: checkpoint.access_epoch,
        }
    }

    fn validate(
        &self,
        profile: ProfileId,
        binding: &SharingBinding,
    ) -> Result<(), SharingHighwaterError> {
        if self.profile_id != profile || self.binding != *binding {
            return Err(SharingHighwaterError::BindingMismatch);
        }
        if self.format != MARKER_FORMAT
            || self.profile_id.0.is_nil()
            || self.owner_user_id == UserId::NIL
            || self.owner_device_id == DeviceId::NIL
            || self.revision <= 0
            || self.manifest_revision == 0
            || self.access_epoch == 0
            || DevicePublicKeys::from_slices(&self.owner_encryption_key, &self.owner_signing_key)
                .is_err()
        {
            return Err(SharingHighwaterError::Corrupt);
        }
        Ok(())
    }

    pub(crate) fn same_anchor(&self, other: &Self) -> bool {
        self.format == other.format
            && self.profile_id == other.profile_id
            && self.binding == other.binding
            && self.owner_user_id == other.owner_user_id
            && self.owner_device_id == other.owner_device_id
            && self.owner_encryption_key == other.owner_encryption_key
            && self.owner_signing_key == other.owner_signing_key
    }

    pub(crate) fn permits_reconciliation_base(&self, older: &Self) -> bool {
        self.same_anchor(older)
            && older.revision <= self.revision
            && older.manifest_revision <= self.manifest_revision
            && older.access_epoch <= self.access_epoch
            && (older.revision != self.revision || older == self)
            && (older.manifest_revision != self.manifest_revision
                || (older.manifest_hash == self.manifest_hash
                    && older.access_epoch == self.access_epoch))
    }
}

/// Handle for one profile. Blocking methods run on the storage worker, never
/// on a Tokio worker; the OS keychain may show an access prompt.
#[derive(Clone)]
pub(crate) struct SharingHighwater {
    profile_id: ProfileId,
    secure: Arc<dyn SecureStore>,
}

impl SharingHighwater {
    pub(crate) fn new(profile_id: ProfileId, secure: Arc<dyn SecureStore>) -> Self {
        Self { profile_id, secure }
    }

    fn name(&self, binding: &SharingBinding) -> String {
        format!(
            "cc.shw.v1:{}:{}:{}",
            self.profile_id, binding.server_instance_id, binding.share_id
        )
    }

    pub(crate) fn lock(&self) -> Result<ProfileLock, SharingHighwaterError> {
        ProfileLock::acquire(self.profile_id)
    }

    pub(crate) fn load(
        &self,
        binding: &SharingBinding,
    ) -> Result<Option<HighwaterMarker>, SharingHighwaterError> {
        let Some(bytes) = self.secure.get(&self.name(binding))? else {
            return Ok(None);
        };
        if bytes.expose_secret().len() > MAX_SECRET_LEN {
            return Err(SharingHighwaterError::Corrupt);
        }
        let marker: HighwaterMarker = serde_json::from_slice(bytes.expose_secret())
            .map_err(|_| SharingHighwaterError::Corrupt)?;
        marker.validate(self.profile_id, binding)?;
        Ok(Some(marker))
    }

    /// Caller holds the profile lock and has checked the previous exact mark.
    /// Read-back detects an acknowledged-but-lost/partial backend write. Any
    /// ambiguity is an error, so the application returns no decrypted result.
    pub(crate) fn save(&self, marker: &HighwaterMarker) -> Result<(), SharingHighwaterError> {
        marker.validate(self.profile_id, &marker.binding)?;
        let encoded = serde_json::to_vec(marker).map_err(|_| SharingHighwaterError::Corrupt)?;
        if encoded.len() > MAX_SECRET_LEN {
            return Err(SharingHighwaterError::Corrupt);
        }
        self.secure.set(&self.name(&marker.binding), &encoded)?;
        if self.load(&marker.binding)?.as_ref() != Some(marker) {
            return Err(SharingHighwaterError::WriteNotVerified);
        }
        Ok(())
    }
}

/// Kernel-managed advisory lock; process exit releases it, including a crash.
pub(crate) struct ProfileLock(File);

impl ProfileLock {
    fn acquire(profile: ProfileId) -> Result<Self, SharingHighwaterError> {
        if profile.0.is_nil() {
            return Err(SharingHighwaterError::BindingMismatch);
        }
        // This path intentionally ignores portable data-directory overrides:
        // every database using this user's secure store shares one lock domain.
        let path = std::env::temp_dir().join(format!("consolecrypt-sharing-v1-{profile}.lock"));
        let mut options = OpenOptions::new();
        options.read(true).write(true).create(true).truncate(false);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let file = options
            .open(&path)
            .map_err(|_| SharingHighwaterError::LockUnavailable)?;
        // A symlink/other file type must not let a second path bypass locking.
        let meta =
            std::fs::symlink_metadata(&path).map_err(|_| SharingHighwaterError::LockUnavailable)?;
        if !meta.file_type().is_file() {
            return Err(SharingHighwaterError::LockUnavailable);
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            let opened = file
                .metadata()
                .map_err(|_| SharingHighwaterError::LockUnavailable)?;
            if meta.mode() & 0o077 != 0 || opened.dev() != meta.dev() || opened.ino() != meta.ino()
            {
                return Err(SharingHighwaterError::LockUnavailable);
            }
        }
        file.lock()
            .map_err(|_| SharingHighwaterError::LockUnavailable)?;
        Ok(Self(file))
    }
}

impl Drop for ProfileLock {
    fn drop(&mut self) {
        let _ = self.0.unlock();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cc_crypto_core::DeviceSecretKeys;
    use cc_platform_core::{InMemorySecureStore, MAX_SECRET_NAME_LEN};
    use cc_protocol::sharing::SharedItemKind;
    use cc_protocol::{ObjectId, ShareId};
    use std::process::Command;
    use std::time::{Duration, Instant};
    use uuid::Uuid;

    fn marker(profile: ProfileId, binding: SharingBinding) -> HighwaterMarker {
        let keys = DeviceSecretKeys::generate().unwrap();
        HighwaterMarker::authenticated(
            profile,
            binding,
            SharingOwnerAnchor {
                user_id: UserId::new(),
                device_id: DeviceId::new(),
                public_keys: keys.public_keys(),
            },
            SharingRevisionCheckpoint {
                revision: 1,
                hash: [255; 32],
                manifest_revision: 1,
                manifest_hash: [255; 32],
                access_epoch: 1,
            },
        )
    }

    #[test]
    fn marker_is_portable_public_metadata_and_rejects_wrong_profile_or_item() {
        let secure = Arc::new(InMemorySecureStore::new());
        let profile = ProfileId::new();
        let store = SharingHighwater::new(profile, secure.clone());
        let binding = SharingBinding {
            server_instance_id: Uuid::new_v4(),
            share_id: ShareId::new(),
            item_id: ObjectId::new(),
            kind: SharedItemKind::Host,
        };
        let marker = marker(profile, binding.clone());
        let _lock = store.lock().unwrap();
        assert!(store.name(&binding).len() <= MAX_SECRET_NAME_LEN);
        store.save(&marker).unwrap();
        let bytes = secure.get(&store.name(&binding)).unwrap().unwrap();
        assert!(bytes.expose_secret().len() <= MAX_SECRET_LEN);
        assert!(store.load(&binding).unwrap().as_ref() == Some(&marker));

        let other = SharingHighwater::new(ProfileId::new(), secure.clone());
        secure
            .set(&other.name(&binding), bytes.expose_secret())
            .unwrap();
        assert!(matches!(
            other.load(&binding),
            Err(SharingHighwaterError::BindingMismatch)
        ));
        let mut changed = binding.clone();
        changed.item_id = ObjectId::new();
        assert!(matches!(
            store.load(&changed),
            Err(SharingHighwaterError::BindingMismatch)
        ));
        secure.set(&store.name(&binding), b"{broken").unwrap();
        assert!(matches!(
            store.load(&binding),
            Err(SharingHighwaterError::Corrupt)
        ));
    }

    // Test helper also runs harmlessly without the parent-process environment.
    #[test]
    fn cross_process_probe() {
        let Some(profile) = std::env::var_os("CC_SHARING_LOCK_PROBE_PROFILE") else {
            return;
        };
        let profile: ProfileId = profile.to_str().unwrap().parse().unwrap();
        let ready = std::env::var_os("CC_SHARING_LOCK_PROBE_READY").unwrap();
        let acquired = std::env::var_os("CC_SHARING_LOCK_PROBE_ACQUIRED").unwrap();
        std::fs::write(ready, b"ready").unwrap();
        let _lock = ProfileLock::acquire(profile).unwrap();
        std::fs::write(acquired, b"acquired").unwrap();
        // Exit without dropping the guard, modelling an abrupt process crash.
        std::process::exit(0);
    }

    #[test]
    fn profile_lock_serializes_processes_and_kernel_releases_it_after_exit() {
        let profile = ProfileId::new();
        let dir = tempfile::tempdir().unwrap();
        let ready = dir.path().join("ready");
        let acquired = dir.path().join("acquired");
        let lock = ProfileLock::acquire(profile).unwrap();
        let child = Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "sharing_highwater::tests::cross_process_probe"])
            .env("CC_SHARING_LOCK_PROBE_PROFILE", profile.to_string())
            .env("CC_SHARING_LOCK_PROBE_READY", &ready)
            .env("CC_SHARING_LOCK_PROBE_ACQUIRED", &acquired)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .unwrap();
        let start = Instant::now();
        while !ready.exists() && start.elapsed() < Duration::from_secs(5) {
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(ready.exists(), "child process did not reach the lock");
        std::thread::sleep(Duration::from_millis(50));
        assert!(!acquired.exists(), "child bypassed the held profile lock");
        drop(lock);
        let result = child.wait_with_output().unwrap();
        assert!(result.status.success());
        assert!(acquired.exists());
        let _released_after_crash = ProfileLock::acquire(profile).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn coordination_lock_rejects_symlinks_and_public_permissions_without_truncation() {
        use std::os::unix::fs::{symlink, PermissionsExt};
        let profile = ProfileId::new();
        let path = std::env::temp_dir().join(format!("consolecrypt-sharing-v1-{profile}.lock"));
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("unrelated-file");
        std::fs::write(&target, b"preserve").unwrap();
        symlink(&target, &path).unwrap();
        assert!(matches!(
            ProfileLock::acquire(profile),
            Err(SharingHighwaterError::LockUnavailable)
        ));
        assert_eq!(std::fs::read(&target).unwrap(), b"preserve");
        std::fs::remove_file(&path).unwrap();
        std::fs::write(&path, b"preserve").unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o666)).unwrap();
        assert!(matches!(
            ProfileLock::acquire(profile),
            Err(SharingHighwaterError::LockUnavailable)
        ));
        assert_eq!(std::fs::read(&path).unwrap(), b"preserve");
        std::fs::remove_file(path).unwrap();
    }
}
