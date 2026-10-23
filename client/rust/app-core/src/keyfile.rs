//! Per-profile key file `profiles/<id>/vault-keys.json` (S5).
//!
//! What a profile needs **before** its database can be opened. Since S5 the
//! SQLCipher key is not kept in the OS secret store: it is sealed under the
//! vault key, so the database opens only after the vault is unlocked with
//! the passphrase, the Recovery Key or device unlock.
//!
//! Contents — ciphertext and non-secret metadata only:
//! * the cached password / recovery envelopes (the same ciphertext the
//!   server and backups hold) — what unlocking starts from;
//! * the database key sealed under `HKDF(VRK)` (`db_key`, plus `db_key_next`
//!   while a re-key is in flight, so a crash at any point stays openable);
//! * the device unlock record when the user switched it on: the OS
//!   protector's handle and the VRK sealed under the OS-released key;
//! * a header for the locked state (kind, server, e-mail, name) and flags.
//!
//! A profile without this file is either from ≤ 0.8.x (database key still
//! in the OS store: migrated on the next passphrase / Recovery Key unlock)
//! or a synced profile without a vault yet (nothing vault-related in its
//! database; sealed as soon as a vault is created or joined).

use crate::error::{AppError, AppResult};
use cc_protocol::envelopes::KeyEnvelope;
use cc_protocol::{Bytes, DeviceId, Timestamp, UserId, VaultId};
use cc_storage_core::{Profile, ProfileId, ProfileKind};
use cc_vault_core::LocalSealed;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// File name inside the profile directory.
pub(crate) const KEYFILE_NAME: &str = "vault-keys.json";
const VERSION: u32 = 1;

/// Seal context of the database key.
pub(crate) fn db_key_context(profile: ProfileId) -> Vec<u8> {
    format!("db-key:{profile}").into_bytes()
}

/// Seal context of the device unlock envelope.
pub(crate) fn unlock_context(profile: ProfileId) -> Vec<u8> {
    format!("device-unlock:{profile}").into_bytes()
}

/// OS protector label of a profile's device unlock key.
pub(crate) fn unlock_label(profile: ProfileId) -> String {
    format!("profile-{profile}")
}

/// Non-secret profile header for the locked state.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct LockedHeader {
    pub kind: ProfileKind,
    #[serde(default)]
    pub display_name: Option<String>,
    #[serde(default)]
    pub server_url: Option<String>,
    #[serde(default)]
    pub email: Option<String>,
    #[serde(default)]
    pub user_id: Option<UserId>,
    pub device_id: DeviceId,
    pub created_at: Timestamp,
    pub updated_at: Timestamp,
}

impl LockedHeader {
    pub(crate) fn from_profile(p: &Profile) -> Self {
        Self {
            kind: p.kind,
            display_name: p.display_name.clone(),
            server_url: p.server_url.clone(),
            email: p.email.clone(),
            user_id: p.user_id,
            device_id: p.device_id,
            created_at: p.created_at,
            updated_at: p.updated_at,
        }
    }

    pub(crate) fn to_profile(&self, profile_id: ProfileId) -> Profile {
        Profile {
            profile_id,
            kind: self.kind,
            display_name: self.display_name.clone(),
            server_url: self.server_url.clone(),
            user_id: self.user_id,
            device_id: self.device_id,
            email: self.email.clone(),
            created_at: self.created_at,
            updated_at: self.updated_at,
        }
    }
}

/// Why device unlock should be offered again after the next passphrase
/// unlock.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ReofferReason {
    /// It was on before the upgrade to OS-protected device unlock.
    Migrated,
    /// The OS key became invalid (fingerprints changed, Hello reset).
    Invalidated,
}

/// Device unlock switched on: the OS protector's handle and the VRK sealed
/// under the key it releases.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct DeviceUnlockRecord {
    /// `touch_id`, `windows_hello`, `fingerprint`, `keyring`, … (UI label).
    pub kind: String,
    /// `system` or `convenience` (Linux).
    pub protection: String,
    /// The app performs the OS step (Android).
    #[serde(default)]
    pub external: bool,
    /// Opaque protector handle (not secret).
    pub handle: Bytes,
    pub envelope: LocalSealed,
    pub created_at: Timestamp,
    /// Step-up confirmations may use the OS check instead of the
    /// passphrase (`presence.rs`); on unless the user turned it off.
    #[serde(default = "confirm_actions_default")]
    pub confirm_actions: bool,
}

fn confirm_actions_default() -> bool {
    true
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct KeyFileFlags {
    /// Recovery Kit check pending (mirrors the database setting).
    #[serde(default)]
    pub kit_pending: bool,
    /// Attest (upload the trust envelope) on the next unlock.
    #[serde(default)]
    pub needs_attest: bool,
    #[serde(default)]
    pub reoffer_device_unlock: Option<ReofferReason>,
}

/// The key file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct KeyFile {
    pub version: u32,
    pub vault_id: VaultId,
    pub header: LockedHeader,
    #[serde(default)]
    pub password_envelope: Option<KeyEnvelope>,
    #[serde(default)]
    pub recovery_envelope: Option<KeyEnvelope>,
    pub db_key: LocalSealed,
    #[serde(default)]
    pub db_key_next: Option<LocalSealed>,
    #[serde(default)]
    pub device_unlock: Option<DeviceUnlockRecord>,
    #[serde(default)]
    pub flags: KeyFileFlags,
}

impl KeyFile {
    pub(crate) fn new(vault_id: VaultId, header: LockedHeader, db_key: LocalSealed) -> Self {
        Self {
            version: VERSION,
            vault_id,
            header,
            password_envelope: None,
            recovery_envelope: None,
            db_key,
            db_key_next: None,
            device_unlock: None,
            flags: KeyFileFlags::default(),
        }
    }
}

/// Path of the key file in `profile_dir`.
pub(crate) fn path_in(profile_dir: &Path) -> PathBuf {
    profile_dir.join(KEYFILE_NAME)
}

/// Read the key file (`None` = legacy or vault-less profile).
pub(crate) fn load(profile_dir: &Path) -> AppResult<Option<KeyFile>> {
    let path = path_in(profile_dir);
    let bytes = match std::fs::read(&path) {
        Ok(b) => b,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(AppError::Storage(format!("profile key file: {e}"))),
    };
    let kf: KeyFile = serde_json::from_slice(&bytes)
        .map_err(|_| AppError::Storage("the profile key file is corrupted".into()))?;
    if kf.version > VERSION {
        return Err(AppError::Storage(
            "the profile key file was written by a newer version".into(),
        ));
    }
    Ok(Some(kf))
}

/// Write the key file atomically (temp file, fsync, rename; 0600).
pub(crate) fn store(profile_dir: &Path, kf: &KeyFile) -> AppResult<()> {
    let io = |e: std::io::Error| AppError::Storage(format!("profile key file: {e}"));
    std::fs::create_dir_all(profile_dir).map_err(io)?;
    let path = path_in(profile_dir);
    let tmp = profile_dir.join(format!("{KEYFILE_NAME}.tmp"));
    let bytes = serde_json::to_vec_pretty(kf)
        .map_err(|e| AppError::internal(format!("key file encoding: {e}")))?;
    {
        use std::io::Write;
        let mut opts = std::fs::OpenOptions::new();
        opts.write(true).create(true).truncate(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            opts.mode(0o600);
        }
        let mut f = opts.open(&tmp).map_err(io)?;
        f.write_all(&bytes).map_err(io)?;
        f.sync_all().map_err(io)?;
    }
    std::fs::rename(&tmp, &path).map_err(io)?;
    // Make the rename durable (directory entry) where supported.
    #[cfg(unix)]
    if let Ok(dir) = std::fs::File::open(profile_dir) {
        let _ = dir.sync_all();
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sealed(b: u8) -> LocalSealed {
        LocalSealed {
            nonce: Bytes::new(vec![b; 24]),
            ciphertext: Bytes::new(vec![b; 48]),
        }
    }

    #[test]
    fn roundtrip_and_permissions() {
        let dir = tempfile::tempdir().unwrap();
        assert!(load(dir.path()).unwrap().is_none());
        let pid = ProfileId::new();
        let header = LockedHeader::from_profile(&Profile::new_local(
            pid,
            DeviceId::new(),
            Some("Home".into()),
        ));
        let mut kf = KeyFile::new(VaultId::new(), header.clone(), sealed(1));
        kf.flags.reoffer_device_unlock = Some(ReofferReason::Migrated);
        store(dir.path(), &kf).unwrap();
        assert_eq!(load(dir.path()).unwrap(), Some(kf.clone()));
        assert_eq!(header.to_profile(pid).display_name.as_deref(), Some("Home"));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(path_in(dir.path()))
                .unwrap()
                .permissions()
                .mode();
            assert_eq!(mode & 0o777, 0o600);
        }
        let text = std::fs::read_to_string(path_in(dir.path())).unwrap();
        assert!(text.contains("\"reoffer_device_unlock\": \"migrated\""));
    }

    #[test]
    fn corrupt_and_future_files_are_refused() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(path_in(dir.path()), b"{not json").unwrap();
        assert!(load(dir.path()).is_err());
        let pid = ProfileId::new();
        let header = LockedHeader::from_profile(&Profile::new_local(pid, DeviceId::new(), None));
        let mut kf = KeyFile::new(VaultId::new(), header, sealed(2));
        kf.version = VERSION + 1;
        store(dir.path(), &kf).unwrap();
        assert!(load(dir.path()).is_err());
    }
}
