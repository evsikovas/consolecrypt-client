//! Encrypted vault backup container (`.ccbackup`, ADR-0106 §Backups).
//!
//! A backup holds only what the server would hold: the password and recovery
//! envelopes and every object's ciphertext with its revision. No plaintext,
//! no device keys, no device envelopes. It restores on any machine with the
//! vault passphrase or the Recovery Key.
//!
//! Integrity: each envelope and object is AEAD-protected on its own; in
//! addition the whole backup carries a **manifest MAC** —
//! `HMAC-SHA256(HKDF(VRK, vault_id, BACKUP_MANIFEST_KEY_LABEL), manifest)` —
//! so that removing, adding, duplicating or rolling back individual objects
//! (or editing `created_at`/`app_version`) is detected after unlocking.
//! Byte layout of `manifest`: see [`VaultBackup::manifest_bytes`] and
//! `docs/adr/ADR-0102-client-crypto-implementation-notes.md`.

use crate::envelope::{RECIPIENT_CODE_PASSWORD, RECIPIENT_CODE_RECOVERY};
use crate::error::{CryptoError, Result};
use crate::kdf::hkdf_sha256_32;
use crate::keys::Vrk;
use cc_protocol::envelopes::{NewEnvelope, RecipientType};
use cc_protocol::limits::{MAX_OBJECT_CIPHERTEXT_BYTES, NONCE_LEN, TAG_LEN, WRAPPED_DEK_LEN};
use cc_protocol::sync::{EncryptedBody, OBJECT_FORMAT_V1};
use cc_protocol::{Bytes, ObjectId, Timestamp, VaultId};
use hmac::{Hmac, KeyInit, Mac};
use secrecy::ExposeSecret;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashSet;

/// Format tag of version 1 backups (also acts as the file's magic).
pub const BACKUP_FORMAT_V1: &str = "consolecrypt-backup/v1";

/// HKDF `info` of the backup manifest MAC key (`ikm = VRK`, `salt = vault_id`).
///
/// Client-only label: backups never reach the server, so it is not (yet) in
/// `cc_protocol::canonical::labels`; proposed for addition there.
pub const BACKUP_MANIFEST_KEY_LABEL: &[u8] = cc_protocol::canonical::labels::BACKUP_MANIFEST_KEY;

/// Prefix of the canonical manifest bytes.
pub const BACKUP_MANIFEST_PREFIX: &[u8] = cc_protocol::canonical::labels::BACKUP_MANIFEST;

/// Maximum `app_version` length accepted in a backup (bytes).
pub const MAX_BACKUP_APP_VERSION_LEN: usize = 128;

const MAC_LEN: usize = 32;

/// One object in a backup: its latest revision, either live (with body) or
/// a tombstone (without).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BackupObject {
    pub object_id: ObjectId,
    /// Revision the body was encrypted for (bound into its AAD).
    pub revision: i64,
    #[serde(default)]
    pub deleted: bool,
    /// `None` iff `deleted`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body: Option<EncryptedBody>,
}

/// The `.ccbackup` container (serialized as JSON by the caller).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VaultBackup {
    /// [`BACKUP_FORMAT_V1`].
    pub format: String,
    pub vault_id: VaultId,
    pub password_envelope: NewEnvelope,
    pub recovery_envelope: NewEnvelope,
    pub objects: Vec<BackupObject>,
    pub created_at: Timestamp,
    /// Version of the app that wrote the backup (informational).
    pub app_version: String,
    /// HMAC-SHA256 over [`VaultBackup::manifest_bytes`] (32 bytes).
    pub manifest_mac: Bytes,
}

impl VaultBackup {
    /// Assemble and MAC a backup. `vrk` must be the vault's root key (the
    /// caller has the vault unlocked). The envelopes are the vault's current
    /// password and recovery envelopes.
    pub fn seal(
        vrk: &Vrk,
        vault_id: VaultId,
        password_envelope: NewEnvelope,
        recovery_envelope: NewEnvelope,
        objects: Vec<BackupObject>,
        created_at: Timestamp,
        app_version: String,
    ) -> Result<Self> {
        let mut backup = Self {
            format: BACKUP_FORMAT_V1.to_owned(),
            vault_id,
            password_envelope,
            recovery_envelope,
            objects,
            created_at,
            app_version,
            manifest_mac: Bytes::default(),
        };
        backup.check_structure(false)?;
        backup.manifest_mac = Bytes::new(backup.compute_mac(vrk).to_vec());
        Ok(backup)
    }

    /// Structural validation of a received backup, before any secret is
    /// involved: format tag, envelope `validate()` and recipient types,
    /// object shapes, sizes and uniqueness.
    pub fn validate_structure(&self) -> Result<()> {
        self.check_structure(true)
    }

    fn check_structure(&self, require_mac: bool) -> Result<()> {
        if self.format != BACKUP_FORMAT_V1 {
            return Err(CryptoError::Backup("unsupported backup format"));
        }
        for (env, expected) in [
            (&self.password_envelope, RecipientType::Password),
            (&self.recovery_envelope, RecipientType::Recovery),
        ] {
            env.validate()?;
            if env.recipient_type != expected {
                return Err(CryptoError::WrongRecipientType {
                    expected,
                    actual: env.recipient_type,
                });
            }
        }
        if self.app_version.len() > MAX_BACKUP_APP_VERSION_LEN {
            return Err(CryptoError::Backup("app_version too long"));
        }
        if require_mac && self.manifest_mac.len() != MAC_LEN {
            return Err(CryptoError::Backup("manifest MAC must be 32 bytes"));
        }
        let mut seen = HashSet::with_capacity(self.objects.len());
        for o in &self.objects {
            if !seen.insert(o.object_id) {
                return Err(CryptoError::Backup("duplicate object id"));
            }
            if o.revision < 1 {
                return Err(CryptoError::InvalidRevision(o.revision));
            }
            match (&o.body, o.deleted) {
                (None, true) => {}
                (Some(b), false) => check_body_shape(b)?,
                _ => return Err(CryptoError::Backup("body must be present iff not deleted")),
            }
        }
        Ok(())
    }

    /// Verify the manifest MAC with the vault's root key (after unlocking an
    /// envelope of this backup). Constant-time comparison.
    pub fn verify_manifest(&self, vrk: &Vrk) -> Result<()> {
        self.validate_structure()?;
        let mut mac = manifest_hmac(vrk, self.vault_id);
        mac.update(&self.manifest_bytes());
        mac.verify_slice(self.manifest_mac.as_slice())
            .map_err(|_| CryptoError::Backup("manifest MAC mismatch (backup modified)"))
    }

    fn compute_mac(&self, vrk: &Vrk) -> [u8; MAC_LEN] {
        let mut mac = manifest_hmac(vrk, self.vault_id);
        mac.update(&self.manifest_bytes());
        mac.finalize().into_bytes().into()
    }

    /// Canonical manifest bytes (all integers big-endian):
    ///
    /// ```text
    /// BACKUP_MANIFEST_PREFIX
    /// || u16 len(format) || format
    /// || vault_id (16)
    /// || created_at unix seconds (i64) || created_at subsecond nanos (u32)
    /// || u16 len(app_version) || app_version
    /// || envelope(password) || envelope(recovery)
    ///      envelope = recipient_code (u8) || nonce (24) || ciphertext (48)
    ///                 [password only: || salt (16) || memory_kib (u32)
    ///                                 || iterations (u32) || parallelism (u32)]
    /// || u32 object_count
    /// || per object, sorted by object_id bytes ascending:
    ///      object_id (16) || revision (i64) || deleted (u8: 0/1)
    ///      [live only: || format (u16) || nonce (24) || wrapped_dek_nonce (24)
    ///                  || wrapped_dek (48) || u32 len(ciphertext)
    ///                  || SHA-256(ciphertext) (32)]
    /// ```
    pub fn manifest_bytes(&self) -> Vec<u8> {
        let mut m = Vec::with_capacity(256 + self.objects.len() * 160);
        m.extend_from_slice(BACKUP_MANIFEST_PREFIX);
        push_str16(&mut m, &self.format);
        m.extend_from_slice(self.vault_id.as_bytes());
        m.extend_from_slice(&self.created_at.timestamp().to_be_bytes());
        m.extend_from_slice(&self.created_at.timestamp_subsec_nanos().to_be_bytes());
        push_str16(&mut m, &self.app_version);
        push_envelope(&mut m, &self.password_envelope, RECIPIENT_CODE_PASSWORD);
        push_envelope(&mut m, &self.recovery_envelope, RECIPIENT_CODE_RECOVERY);
        let mut objects: Vec<&BackupObject> = self.objects.iter().collect();
        objects.sort_by(|a, b| a.object_id.as_bytes().cmp(b.object_id.as_bytes()));
        m.extend_from_slice(&len_u32(objects.len()).to_be_bytes());
        for o in objects {
            m.extend_from_slice(o.object_id.as_bytes());
            m.extend_from_slice(&o.revision.to_be_bytes());
            m.push(u8::from(o.deleted));
            if let Some(b) = &o.body {
                m.extend_from_slice(&b.format.to_be_bytes());
                m.extend_from_slice(b.nonce.as_slice());
                m.extend_from_slice(b.wrapped_dek_nonce.as_slice());
                m.extend_from_slice(b.wrapped_dek.as_slice());
                m.extend_from_slice(&len_u32(b.ciphertext.len()).to_be_bytes());
                m.extend_from_slice(&Sha256::digest(b.ciphertext.as_slice()));
            }
        }
        m
    }
}

fn manifest_hmac(vrk: &Vrk, vault_id: VaultId) -> Hmac<Sha256> {
    let key = hkdf_sha256_32(
        vrk.expose(),
        vault_id.as_bytes(),
        &[BACKUP_MANIFEST_KEY_LABEL],
    );
    // Infallible: HMAC accepts keys of any length.
    <Hmac<Sha256> as KeyInit>::new_from_slice(key.expose_secret())
        .expect("HMAC takes any key length")
}

/// Manifest MAC key (test vectors only).
#[cfg(test)]
pub(crate) fn manifest_key_for_tests(vrk: &Vrk, vault_id: VaultId) -> [u8; 32] {
    *hkdf_sha256_32(
        vrk.expose(),
        vault_id.as_bytes(),
        &[BACKUP_MANIFEST_KEY_LABEL],
    )
    .expose_secret()
}

fn check_body_shape(b: &EncryptedBody) -> Result<()> {
    if b.format != OBJECT_FORMAT_V1 {
        return Err(CryptoError::UnsupportedFormat(b.format));
    }
    if b.nonce.len() != NONCE_LEN || b.wrapped_dek_nonce.len() != NONCE_LEN {
        return Err(CryptoError::MalformedBody("nonce must be 24 bytes"));
    }
    if b.wrapped_dek.len() != WRAPPED_DEK_LEN {
        return Err(CryptoError::MalformedBody("wrapped DEK must be 48 bytes"));
    }
    if !(TAG_LEN..=MAX_OBJECT_CIPHERTEXT_BYTES).contains(&b.ciphertext.len()) {
        return Err(CryptoError::MalformedBody("ciphertext length out of range"));
    }
    Ok(())
}

fn len_u32(n: usize) -> u32 {
    // Bounded by validation (object size and count limits of a backup that
    // fits in memory); saturate rather than wrap just in case.
    u32::try_from(n).unwrap_or(u32::MAX)
}

fn push_str16(m: &mut Vec<u8>, s: &str) {
    let len = u16::try_from(s.len()).unwrap_or(u16::MAX);
    m.extend_from_slice(&len.to_be_bytes());
    m.extend_from_slice(&s.as_bytes()[..usize::from(len)]);
}

fn push_envelope(m: &mut Vec<u8>, e: &NewEnvelope, code: u8) {
    m.push(code);
    m.extend_from_slice(e.nonce.as_slice());
    m.extend_from_slice(e.ciphertext.as_slice());
    if let Some(k) = &e.metadata.kdf {
        m.extend_from_slice(k.salt.as_slice());
        m.extend_from_slice(&k.memory_kib.to_be_bytes());
        m.extend_from_slice(&k.iterations.to_be_bytes());
        m.extend_from_slice(&k.parallelism.to_be_bytes());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        encrypt_object, seal_password_envelope, seal_recovery_envelope, Argon2Params, KeyHierarchy,
        RecoveryKey,
    };
    use cc_models::{host::Host, ObjectPayload, VaultObject};
    use secrecy::SecretString;

    fn sample() -> (Vrk, VaultBackup) {
        let vault = VaultId::new();
        let vrk = Vrk::generate().unwrap();
        let keys = KeyHierarchy::derive(&vrk, vault);
        let pw = seal_password_envelope(
            &vrk,
            vault,
            &SecretString::from("backup passphrase"),
            Argon2Params::for_tests(),
        )
        .unwrap();
        let rec = seal_recovery_envelope(&vrk, vault, &RecoveryKey::generate().unwrap()).unwrap();
        let mut objects = Vec::new();
        for i in 1..=3 {
            let h = Host::new(format!("host-{i}"), "192.0.2.1");
            let id = h.id;
            let body = encrypt_object(
                &keys,
                vault,
                id,
                i,
                &ObjectPayload::new(VaultObject::Host(h)),
            )
            .unwrap();
            objects.push(BackupObject {
                object_id: id,
                revision: i,
                deleted: false,
                body: Some(body),
            });
        }
        objects.push(BackupObject {
            object_id: ObjectId::new(),
            revision: 4,
            deleted: true,
            body: None,
        });
        let b = VaultBackup::seal(
            &vrk,
            vault,
            pw,
            rec,
            objects,
            chrono::Utc::now(),
            "0.1.0".into(),
        )
        .unwrap();
        (vrk, b)
    }

    #[test]
    fn seal_verify_and_json_roundtrip() {
        let (vrk, b) = sample();
        b.validate_structure().unwrap();
        b.verify_manifest(&vrk).unwrap();
        let json = serde_json::to_vec(&b).unwrap();
        let back: VaultBackup = serde_json::from_slice(&json).unwrap();
        assert_eq!(back, b);
        back.verify_manifest(&vrk).unwrap();
        // Object order in the file does not matter.
        let mut shuffled = back.clone();
        shuffled.objects.reverse();
        shuffled.verify_manifest(&vrk).unwrap();
    }

    #[test]
    fn any_modification_breaks_the_manifest() {
        let (vrk, b) = sample();
        let (_, other) = sample();
        type Edit = Box<dyn Fn(&mut VaultBackup)>;
        let edits: Vec<Edit> = vec![
            Box::new(|b| {
                b.objects.remove(0);
            }),
            Box::new(|b| b.objects[0].revision += 1),
            Box::new(|b| b.objects[1].body.as_mut().unwrap().ciphertext.0[0] ^= 1),
            Box::new(|b| b.objects[1].body.as_mut().unwrap().wrapped_dek.0[0] ^= 1),
            Box::new(|b| b.objects[3].revision = 9),
            Box::new(|b| b.created_at += chrono::Duration::seconds(1)),
            Box::new(|b| b.app_version.push('x')),
            Box::new(move |b| b.password_envelope = other.password_envelope.clone()),
            Box::new(|b| b.manifest_mac.0[0] ^= 1),
            Box::new(|b| {
                let mut dup = b.objects[0].clone();
                dup.object_id = ObjectId::new();
                b.objects.push(dup);
            }),
        ];
        for (i, edit) in edits.iter().enumerate() {
            let mut t = b.clone();
            edit(&mut t);
            t.validate_structure().unwrap();
            assert_eq!(
                t.verify_manifest(&vrk).err(),
                Some(CryptoError::Backup(
                    "manifest MAC mismatch (backup modified)"
                )),
                "edit {i}"
            );
        }
        // Wrong root key.
        assert!(b.verify_manifest(&Vrk::generate().unwrap()).is_err());
    }

    #[test]
    fn structural_errors() {
        let (_, b) = sample();
        let mut t = b.clone();
        t.format = "consolecrypt-backup/v2".into();
        assert!(matches!(
            t.validate_structure(),
            Err(CryptoError::Backup(_))
        ));
        let mut t = b.clone();
        t.objects[1].object_id = t.objects[0].object_id;
        assert_eq!(
            t.validate_structure().err(),
            Some(CryptoError::Backup("duplicate object id"))
        );
        let mut t = b.clone();
        t.objects[0].deleted = true;
        assert!(matches!(
            t.validate_structure(),
            Err(CryptoError::Backup(_))
        ));
        let mut t = b.clone();
        t.objects[3].deleted = false;
        assert!(matches!(
            t.validate_structure(),
            Err(CryptoError::Backup(_))
        ));
        let mut t = b.clone();
        t.objects[0].revision = 0;
        assert_eq!(
            t.validate_structure().err(),
            Some(CryptoError::InvalidRevision(0))
        );
        let mut t = b.clone();
        std::mem::swap(&mut t.password_envelope, &mut t.recovery_envelope);
        assert!(t.validate_structure().is_err());
        let mut t = b.clone();
        t.manifest_mac = Bytes::new(vec![0; 16]);
        assert!(t.validate_structure().is_err());
        let mut t = b.clone();
        t.objects[0].body.as_mut().unwrap().nonce.0.pop();
        assert!(matches!(
            t.validate_structure(),
            Err(CryptoError::MalformedBody(_))
        ));
        let mut t = b.clone();
        t.password_envelope
            .metadata
            .kdf
            .as_mut()
            .unwrap()
            .memory_kib = u32::MAX;
        assert!(matches!(
            t.validate_structure(),
            Err(CryptoError::InvalidEnvelope(_))
        ));
    }
}
