//! This installation's device identity: a client-generated [`DeviceId`] plus
//! X25519/Ed25519 keys, persisted in the OS secure store per profile.

use crate::error::{Result, VaultError};
use cc_crypto_core::{DeviceApproval, DevicePublicKeys, DeviceSecretKeys, VerificationCode};
use cc_platform_core::{validate_profile_id, ExposeSecret, SecureStore};
use cc_protocol::devices::{DeviceProof, DeviceRegistration, RequestProof};
use cc_protocol::limits::{ED25519_SIGNATURE_LEN, MAX_DEVICE_NAME_LEN};
use cc_protocol::version::Platform;
use cc_protocol::DeviceId;
use std::fmt;
use uuid::Uuid;
use zeroize::Zeroizing;

const IDENTITY_MAGIC: &[u8; 4] = b"CCID";
const IDENTITY_VERSION: u8 = 1;

/// A device (installation × profile): id + private keys. Private keys never
/// leave the device; only [`DeviceIdentity::registration`] data is sent.
pub struct DeviceIdentity {
    device_id: DeviceId,
    keys: DeviceSecretKeys,
}

impl DeviceIdentity {
    /// Fresh identity: new UUIDv7 device id and fresh keys.
    pub fn generate() -> Result<Self> {
        Ok(Self {
            device_id: DeviceId::new(),
            keys: DeviceSecretKeys::generate()?,
        })
    }

    /// The device id (also the `recipient_id` of its device envelopes).
    pub fn device_id(&self) -> DeviceId {
        self.device_id
    }

    /// Public keys, as registered with the server.
    pub fn public_keys(&self) -> DevicePublicKeys {
        self.keys.public_keys()
    }

    /// This device's verification code — shown on the *new* device while a
    /// trusted device approves it (ADR-0004).
    pub fn verification_code(&self) -> VerificationCode {
        self.public_keys().verification_code(self.device_id)
    }

    pub(crate) fn secret_keys(&self) -> &DeviceSecretKeys {
        &self.keys
    }

    pub(crate) fn sign_approval(
        &self,
        approval: &DeviceApproval<'_>,
    ) -> [u8; ED25519_SIGNATURE_LEN] {
        self.keys.sign_device_approval(approval)
    }

    /// Registration DTO for register/login. `name` is trimmed; it must be
    /// 1..=128 characters without control characters.
    pub fn registration(
        &self,
        name: &str,
        platform: Platform,
        client_version: Option<String>,
    ) -> Result<DeviceRegistration> {
        let name = name.trim();
        if name.is_empty()
            || name.chars().count() > MAX_DEVICE_NAME_LEN
            || name.chars().any(char::is_control)
        {
            return Err(VaultError::InvalidDeviceName);
        }
        let keys = self.public_keys();
        Ok(DeviceRegistration {
            device_id: self.device_id,
            name: name.to_owned(),
            platform,
            encryption_public_key: keys.encryption_bytes(),
            signing_public_key: keys.signing_bytes(),
            client_version,
        })
    }

    /// Fresh proof of possession of this device's signing key for
    /// register/login (protocol 1.4, ADR-0006). Attach it to EVERY
    /// `LoginRequest`/`RegisterRequest`: the server requires it for known
    /// device ids, so a stolen account password alone can never log in as
    /// an existing trusted device. Single use — build a new one per request.
    pub fn login_proof(&self, now: cc_protocol::Timestamp) -> Result<DeviceProof> {
        let mut nonce = [0u8; 32];
        cc_crypto_core::fill_random(&mut nonce)?;
        let issued_at = now.timestamp();
        let signature = self
            .secret_keys()
            .sign_device_login(self.device_id, issued_at, &nonce);
        Ok(DeviceProof {
            issued_at,
            nonce: nonce.into(),
            signature: signature.into(),
        })
    }

    /// Per-request proof of possession for the `x-cc-device-proof` header
    /// (protocol 1.5): binds method, request target, body and time to this
    /// device's key, so a stolen bearer/refresh token is useless without
    /// the device. `path_and_query` must be exactly what goes on the wire.
    pub fn request_proof(
        &self,
        method: &str,
        path_and_query: &str,
        body: &[u8],
        now: cc_protocol::Timestamp,
    ) -> Result<RequestProof> {
        let mut nonce = [0u8; 32];
        cc_crypto_core::fill_random(&mut nonce)?;
        let issued_at = now.timestamp();
        let body_sha256 = cc_crypto_core::request_body_sha256(body);
        let signature = self.secret_keys().sign_request(
            self.device_id,
            method,
            path_and_query,
            &body_sha256,
            issued_at,
            &nonce,
        );
        Ok(RequestProof {
            issued_at,
            nonce,
            signature,
        })
    }

    /// Secure-store item name for `profile_id`'s identity.
    pub fn storage_name(profile_id: &str) -> Result<String> {
        validate_profile_id(profile_id).map_err(|_| VaultError::InvalidProfileId)?;
        Ok(format!("profiles/{profile_id}/device-identity"))
    }

    /// Load the identity of `profile_id`, if one was saved.
    pub fn load(store: &dyn SecureStore, profile_id: &str) -> Result<Option<Self>> {
        match store.get(&Self::storage_name(profile_id)?)? {
            Some(blob) => Self::decode(blob.expose_secret()).map(Some),
            None => Ok(None),
        }
    }

    /// Persist the identity for `profile_id` (overwrites).
    pub fn save(&self, store: &dyn SecureStore, profile_id: &str) -> Result<()> {
        store.set(&Self::storage_name(profile_id)?, &self.encode())?;
        Ok(())
    }

    /// Load the identity of `profile_id`, or generate and save a new one.
    pub fn load_or_create(store: &dyn SecureStore, profile_id: &str) -> Result<Self> {
        if let Some(existing) = Self::load(store, profile_id)? {
            return Ok(existing);
        }
        let fresh = Self::generate()?;
        fresh.save(store, profile_id)?;
        Ok(fresh)
    }

    /// Remove the identity of `profile_id` (e.g. when deleting a profile).
    /// Returns whether it existed.
    pub fn delete(store: &dyn SecureStore, profile_id: &str) -> Result<bool> {
        Ok(store.delete(&Self::storage_name(profile_id)?)?)
    }

    /// `"CCID" || 0x01 || device_id (16) || DeviceSecretKeys encoding`.
    fn encode(&self) -> Zeroizing<Vec<u8>> {
        let keys = self.keys.to_secret_bytes();
        let mut out = Zeroizing::new(Vec::with_capacity(4 + 1 + 16 + keys.len()));
        out.extend_from_slice(IDENTITY_MAGIC);
        out.push(IDENTITY_VERSION);
        out.extend_from_slice(self.device_id.as_bytes());
        out.extend_from_slice(&keys);
        out
    }

    fn decode(bytes: &[u8]) -> Result<Self> {
        if bytes.len() < 21 || &bytes[..4] != IDENTITY_MAGIC || bytes[4] != IDENTITY_VERSION {
            return Err(VaultError::CorruptedIdentity);
        }
        let mut id = [0u8; 16];
        id.copy_from_slice(&bytes[5..21]);
        let keys = DeviceSecretKeys::from_secret_bytes(&bytes[21..])
            .map_err(|_| VaultError::CorruptedIdentity)?;
        Ok(Self {
            device_id: DeviceId::from_uuid(Uuid::from_bytes(id)),
            keys,
        })
    }
}

impl fmt::Debug for DeviceIdentity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("DeviceIdentity")
            .field("device_id", &self.device_id)
            .field("keys", &"<redacted>")
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cc_platform_core::InMemorySecureStore;

    #[test]
    fn persist_load_delete() {
        let store = InMemorySecureStore::new();
        assert!(DeviceIdentity::load(&store, "p1").unwrap().is_none());
        let a = DeviceIdentity::load_or_create(&store, "p1").unwrap();
        let b = DeviceIdentity::load_or_create(&store, "p1").unwrap();
        assert_eq!(a.device_id(), b.device_id());
        assert_eq!(a.public_keys(), b.public_keys());
        // Profiles are independent identities.
        let c = DeviceIdentity::load_or_create(&store, "p2").unwrap();
        assert_ne!(a.device_id(), c.device_id());
        assert!(DeviceIdentity::delete(&store, "p1").unwrap());
        assert!(DeviceIdentity::load(&store, "p1").unwrap().is_none());
    }

    #[test]
    fn corrupted_blob_and_bad_profile() {
        let store = InMemorySecureStore::new();
        let name = DeviceIdentity::storage_name("p").unwrap();
        store.set(&name, b"CCID\x01short").unwrap();
        assert_eq!(
            DeviceIdentity::load(&store, "p").err(),
            Some(VaultError::CorruptedIdentity)
        );
        store.set(&name, &[0u8; 90]).unwrap();
        assert_eq!(
            DeviceIdentity::load(&store, "p").err(),
            Some(VaultError::CorruptedIdentity)
        );
        assert_eq!(
            DeviceIdentity::load(&store, "../x").err(),
            Some(VaultError::InvalidProfileId)
        );
    }

    #[test]
    fn registration_validates_name() {
        let id = DeviceIdentity::generate().unwrap();
        let r = id
            .registration("  Work MacBook ", Platform::Macos, Some("0.1.0".into()))
            .unwrap();
        assert_eq!(r.name, "Work MacBook");
        assert_eq!(r.device_id, id.device_id());
        assert_eq!(
            r.encryption_public_key.as_slice(),
            &id.public_keys().encryption
        );
        assert_eq!(r.signing_public_key.as_slice(), &id.public_keys().signing);
        for bad in ["", "   ", "tab\tname", &"x".repeat(129)] {
            assert_eq!(
                id.registration(bad, Platform::Cli, None).err(),
                Some(VaultError::InvalidDeviceName)
            );
        }
    }

    #[test]
    fn debug_redacted() {
        let id = DeviceIdentity::generate().unwrap();
        let s = format!("{id:?}");
        assert!(s.contains("redacted") && s.contains(&id.device_id().to_string()));
    }
}

#[cfg(test)]
mod login_proof_tests {
    use super::*;

    #[test]
    fn login_proof_verifies_and_is_fresh_each_time() {
        let id = DeviceIdentity::generate().unwrap();
        let now = chrono::Utc::now();
        let a = id.login_proof(now).unwrap();
        let b = id.login_proof(now).unwrap();
        assert_ne!(a.nonce, b.nonce, "nonce must be fresh per request");
        let pk: [u8; 32] = id
            .public_keys()
            .signing_bytes()
            .as_slice()
            .try_into()
            .unwrap();
        let nonce: [u8; 32] = a.nonce.as_slice().try_into().unwrap();
        cc_crypto_core::verify_device_login(
            &pk,
            id.device_id(),
            a.issued_at,
            &nonce,
            a.signature.as_slice(),
        )
        .unwrap();
    }
}

#[cfg(test)]
mod request_proof_tests {
    use super::*;

    #[test]
    fn request_proof_header_roundtrip_and_verifies() {
        let id = DeviceIdentity::generate().unwrap();
        let body = br#"{"vault_id":"x"}"#;
        let p = id
            .request_proof("POST", "/v1/sync/push", body, chrono::Utc::now())
            .unwrap();
        let decoded = RequestProof::decode(&p.encode()).expect("header roundtrip");
        assert_eq!(decoded, p);
        let pk: [u8; 32] = id
            .public_keys()
            .signing_bytes()
            .as_slice()
            .try_into()
            .unwrap();
        cc_crypto_core::verify_request_proof(
            &pk,
            id.device_id(),
            "POST",
            "/v1/sync/push",
            &cc_crypto_core::request_body_sha256(body),
            p.issued_at,
            &p.nonce,
            &p.signature,
        )
        .unwrap();
    }
}
