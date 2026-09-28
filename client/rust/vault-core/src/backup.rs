//! Encrypted backups (`.ccbackup`, ADR-0106 §Backups): export from an
//! unlocked vault, restore with the passphrase or the Recovery Key.
//!
//! The container format and its manifest MAC live in
//! [`cc_crypto_core::VaultBackup`]; this module adds the flows. Reading and
//! writing the file is the caller's job (storage-core / app-core).

use crate::error::{Result, VaultError};
use crate::unlock::{
    parse_recovery_input, unlock_with_passphrase, unlock_with_recovery, EnvelopeSource,
};
use crate::unlocked::UnlockedVault;
use cc_crypto_core::{BackupObject, VaultBackup};
use cc_models::KekClass;
use cc_protocol::envelopes::RecipientType;
use cc_protocol::{ObjectId, Timestamp};
use secrecy::SecretString;
use std::fmt;

/// Hard cap on the serialized backup size accepted by [`decode_backup`]
/// (512 MiB) — protects against memory exhaustion from a hostile file.
pub const MAX_BACKUP_BYTES: usize = 512 * 1024 * 1024;

impl UnlockedVault {
    /// Build a backup of this vault. `objects` are the stored ciphertexts
    /// (latest revision per object, tombstones without body). Every live
    /// object is trial-decrypted first, so a backup that could not be
    /// restored is never written.
    pub fn create_backup(
        &self,
        password_envelope: &(impl EnvelopeSource + ?Sized),
        recovery_envelope: &(impl EnvelopeSource + ?Sized),
        objects: Vec<BackupObject>,
        created_at: Timestamp,
        app_version: &str,
    ) -> Result<VaultBackup> {
        let password = self.checked_envelope(password_envelope, RecipientType::Password)?;
        let recovery = self.checked_envelope(recovery_envelope, RecipientType::Recovery)?;
        verify_objects(self, &objects)?;
        Ok(VaultBackup::seal(
            self.vrk(),
            self.vault_id(),
            password,
            recovery,
            objects,
            created_at,
            app_version.to_owned(),
        )?)
    }
}

/// Serialize a backup to the `.ccbackup` file content (JSON).
pub fn encode_backup(backup: &VaultBackup) -> Result<Vec<u8>> {
    serde_json::to_vec(backup).map_err(|_| VaultError::BackupEncoding)
}

/// Parse `.ccbackup` file content and validate its structure (format tag,
/// envelope validation incl. KDF bounds, object shapes). No secret needed.
pub fn decode_backup(bytes: &[u8]) -> Result<VaultBackup> {
    if bytes.len() > MAX_BACKUP_BYTES {
        return Err(VaultError::BackupEncoding);
    }
    let backup: VaultBackup =
        serde_json::from_slice(bytes).map_err(|_| VaultError::BackupEncoding)?;
    backup.validate_structure()?;
    Ok(backup)
}

/// A verified, unlocked backup ready to import.
pub struct RestoredBackup {
    /// The vault, unlocked with the backup's envelope.
    pub unlocked: UnlockedVault,
    /// The verified backup (envelopes and ciphertexts to import as-is).
    pub backup: VaultBackup,
    /// KEK class of every live object (cache for `class_hint`).
    pub classes: Vec<(ObjectId, KekClass)>,
}

impl fmt::Debug for RestoredBackup {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RestoredBackup")
            .field("vault_id", &self.unlocked.vault_id())
            .field("objects", &self.backup.objects.len())
            .finish_non_exhaustive()
    }
}

/// Restore with the vault passphrase: unlock the backup's password envelope,
/// verify the manifest MAC, trial-decrypt every live object.
pub fn restore_backup_with_passphrase(
    backup: VaultBackup,
    passphrase: &SecretString,
) -> Result<RestoredBackup> {
    backup.validate_structure()?;
    let unlocked = unlock_with_passphrase(backup.vault_id, &backup.password_envelope, passphrase)?;
    finish_restore(backup, unlocked)
}

/// Restore with the Recovery Key (24 words or QR payload).
pub fn restore_backup_with_recovery(
    backup: VaultBackup,
    recovery_input: &SecretString,
) -> Result<RestoredBackup> {
    backup.validate_structure()?;
    let rk = parse_recovery_input(recovery_input, backup.vault_id)?;
    let unlocked = unlock_with_recovery(backup.vault_id, &backup.recovery_envelope, &rk)?;
    finish_restore(backup, unlocked)
}

fn finish_restore(backup: VaultBackup, unlocked: UnlockedVault) -> Result<RestoredBackup> {
    backup.verify_manifest(unlocked.vrk())?;
    let classes = verify_objects(&unlocked, &backup.objects)?;
    Ok(RestoredBackup {
        unlocked,
        backup,
        classes,
    })
}

fn verify_objects(
    vault: &UnlockedVault,
    objects: &[BackupObject],
) -> Result<Vec<(ObjectId, KekClass)>> {
    let mut classes = Vec::with_capacity(objects.len());
    for o in objects {
        if let Some(body) = &o.body {
            let (_, class) = vault
                .decrypt_object(o.object_id, o.revision, body, None)
                .map_err(|e| match e {
                    VaultError::Crypto(source) => VaultError::BackupObject {
                        object_id: o.object_id,
                        source,
                    },
                    other => other,
                })?;
            classes.push((o.object_id, class));
        }
    }
    Ok(classes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{create_vault, DeviceIdentity};
    use cc_crypto_core::Argon2Params;
    use cc_models::{host::Host, ObjectPayload, VaultObject};

    /// A backup MACed by a key holder but containing an object encrypted for
    /// another revision: trial decryption catches what the MAC cannot.
    #[test]
    fn undecryptable_object_with_valid_manifest_is_reported() {
        let identity = DeviceIdentity::generate().unwrap();
        let pass = SecretString::from("backup unit passphrase");
        let created = create_vault(
            &identity,
            &pass,
            Argon2Params::for_tests(),
            None,
            chrono::Utc::now(),
        )
        .unwrap();
        let u = &created.unlocked;
        let envs = created.local_envelopes();
        let h = Host::new("x", "192.0.2.9");
        let (id, p) = (h.id, ObjectPayload::new(VaultObject::Host(h)));
        let body = u.encrypt_object(id, 5, &p).unwrap();
        let bad = vec![BackupObject {
            object_id: id,
            revision: 6,
            deleted: false,
            body: Some(body),
        }];
        // create_backup refuses to write it…
        assert!(matches!(
            u.create_backup(&envs.password, &envs.recovery, bad.clone(), chrono::Utc::now(), "t"),
            Err(VaultError::BackupObject { object_id, .. }) if object_id == id
        ));
        // …and a validly MACed one is rejected on restore.
        let sealed = VaultBackup::seal(
            u.vrk(),
            u.vault_id(),
            envs.password.clone(),
            envs.recovery.clone(),
            bad,
            chrono::Utc::now(),
            "t".into(),
        )
        .unwrap();
        assert!(matches!(
            restore_backup_with_passphrase(sealed, &pass),
            Err(VaultError::BackupObject { object_id, .. }) if object_id == id
        ));
    }
}
