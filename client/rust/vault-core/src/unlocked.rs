//! The in-memory state of an unlocked vault and everything that needs the
//! Vault Root Key: object encryption, new envelopes, passphrase change,
//! recovery-kit regeneration, attestation and enable-sync payloads.

use crate::error::{Result, VaultError, MIN_PASSPHRASE_CHARS};
use crate::identity::DeviceIdentity;
use crate::recovery_kit::RecoveryKit;
use crate::unlock::{envelope_for, EnvelopeSource};
use cc_crypto_core::{
    decrypt_object, encrypt_object, normalize_passphrase, seal_device_envelope,
    seal_password_envelope, seal_recovery_envelope, Argon2Params, KeyHierarchy, RecoveryKey, Vrk,
};
use cc_models::{KekClass, ObjectPayload};
use cc_protocol::devices::AttestDeviceRequest;
use cc_protocol::envelopes::{NewEnvelope, RecipientType};
use cc_protocol::recovery::ReplaceEnvelopeRequest;
use cc_protocol::sync::EncryptedBody;
use cc_protocol::vaults::CreateVaultRequest;
use cc_protocol::{Bytes, DeviceId, ObjectId, Timestamp, VaultId};
use secrecy::SecretString;
use std::fmt;

/// An unlocked vault: the VRK and its derived keys, in memory only.
/// Dropping it (or calling [`UnlockedVault::lock`]) zeroizes every key.
pub struct UnlockedVault {
    vault_id: VaultId,
    vrk: Vrk,
    keys: KeyHierarchy,
}

impl UnlockedVault {
    pub(crate) fn from_vrk(vault_id: VaultId, vrk: Vrk) -> Self {
        let keys = KeyHierarchy::derive(&vrk, vault_id);
        Self {
            vault_id,
            vrk,
            keys,
        }
    }

    /// The vault id.
    pub fn vault_id(&self) -> VaultId {
        self.vault_id
    }

    /// Derived keys (KEKs, VAK) — e.g. for an `ObjectCodec` implementation.
    pub fn keys(&self) -> &KeyHierarchy {
        &self.keys
    }

    pub(crate) fn vrk(&self) -> &Vrk {
        &self.vrk
    }

    /// Lock: drop and zeroize all key material.
    pub fn lock(self) {
        drop(self);
    }

    // -- objects -----------------------------------------------------------

    /// Encrypt `payload` for `revision` (= base_revision + 1). `object_id`
    /// must equal the payload's own id.
    pub fn encrypt_object(
        &self,
        object_id: ObjectId,
        revision: i64,
        payload: &ObjectPayload,
    ) -> Result<EncryptedBody> {
        Ok(encrypt_object(
            &self.keys,
            self.vault_id,
            object_id,
            revision,
            payload,
        )?)
    }

    /// Decrypt and verify an object body (AAD binding, class-confusion check).
    /// Pass the cached class as `class_hint` to skip trial unwrapping.
    pub fn decrypt_object(
        &self,
        object_id: ObjectId,
        revision: i64,
        body: &EncryptedBody,
        class_hint: Option<KekClass>,
    ) -> Result<(ObjectPayload, KekClass)> {
        Ok(decrypt_object(
            &self.keys,
            self.vault_id,
            object_id,
            revision,
            body,
            class_hint,
        )?)
    }

    /// Decrypt a body stored at `from_revision` and re-encrypt it for
    /// `to_revision` with a fresh DEK — e.g. uploading local objects as
    /// `revision = 1` creates when enabling sync (ADR-0106).
    pub fn reencrypt_object(
        &self,
        object_id: ObjectId,
        from_revision: i64,
        body: &EncryptedBody,
        to_revision: i64,
    ) -> Result<(EncryptedBody, KekClass)> {
        let (payload, class) = self.decrypt_object(object_id, from_revision, body, None)?;
        Ok((
            self.encrypt_object(object_id, to_revision, &payload)?,
            class,
        ))
    }

    // -- vault access key --------------------------------------------------

    /// The vault access key as protocol bytes (server-verifiable proof of
    /// VRK knowledge; not a decryption key). Drop the DTO after sending.
    pub fn vault_access_key(&self) -> Bytes {
        self.keys.vault_access_key().to_protocol_bytes()
    }

    /// `SHA-256(VAK)` — what the server stores.
    pub fn vault_access_key_verifier(&self) -> [u8; 32] {
        self.keys.vault_access_key().verifier()
    }

    // -- envelopes ---------------------------------------------------------

    /// New password envelope. Enforces [`MIN_PASSPHRASE_CHARS`].
    pub fn password_envelope(
        &self,
        passphrase: &SecretString,
        kdf: Argon2Params,
    ) -> Result<NewEnvelope> {
        check_new_passphrase(passphrase)?;
        Ok(seal_password_envelope(
            &self.vrk,
            self.vault_id,
            passphrase,
            kdf,
        )?)
    }

    /// New recovery envelope for `rk`.
    pub fn recovery_envelope(&self, rk: &RecoveryKey) -> Result<NewEnvelope> {
        Ok(seal_recovery_envelope(&self.vrk, self.vault_id, rk)?)
    }

    /// Device envelope for device `device_id` with X25519 key
    /// `encryption_public_key`. For keys received from a server, go through
    /// [`crate::PendingApproval`] (verification code) instead.
    pub fn device_envelope(
        &self,
        device_id: DeviceId,
        encryption_public_key: &[u8; 32],
    ) -> Result<NewEnvelope> {
        Ok(seal_device_envelope(
            &self.vrk,
            self.vault_id,
            device_id,
            encryption_public_key,
        )?)
    }

    /// Device envelope for this installation (`identity`) — stored locally
    /// for OS/biometric unlock or uploaded with create/attest.
    pub fn device_envelope_for(&self, identity: &DeviceIdentity) -> Result<NewEnvelope> {
        self.device_envelope(identity.device_id(), &identity.public_keys().encryption)
    }

    /// Change the vault passphrase → request for
    /// `POST /v1/recovery/vault/password-envelope/replace`. Local-only
    /// profiles persist `request.envelope` instead of sending it.
    pub fn change_passphrase(
        &self,
        new_passphrase: &SecretString,
        kdf: Argon2Params,
    ) -> Result<ReplaceEnvelopeRequest> {
        Ok(ReplaceEnvelopeRequest {
            vault_id: self.vault_id,
            vault_access_key: self.vault_access_key(),
            envelope: self.password_envelope(new_passphrase, kdf)?,
        })
    }

    /// New Recovery Key → (kit to show once, request for
    /// `POST /v1/recovery/vault/recovery-envelope/replace`). The old Recovery
    /// Key stops working once the old envelope is replaced.
    pub fn regenerate_recovery_kit(
        &self,
        server_url: Option<&str>,
        created_at: Timestamp,
    ) -> Result<(RecoveryKit, ReplaceEnvelopeRequest)> {
        let rk = RecoveryKey::generate()?;
        let envelope = self.recovery_envelope(&rk)?;
        let kit = RecoveryKit::new(self.vault_id, &rk, server_url, created_at);
        Ok((
            kit,
            ReplaceEnvelopeRequest {
                vault_id: self.vault_id,
                vault_access_key: self.vault_access_key(),
                envelope,
            },
        ))
    }

    /// Self-trust after unlocking with passphrase / Recovery Key →
    /// `POST /v1/devices/{self}/attest` (ADR-0004 §Attestation).
    pub fn attest_device_request(&self, identity: &DeviceIdentity) -> Result<AttestDeviceRequest> {
        Ok(AttestDeviceRequest {
            vault_id: self.vault_id,
            vault_access_key: self.vault_access_key(),
            envelope: self.device_envelope_for(identity)?,
        })
    }

    /// Enable sync for an existing local vault (ADR-0106 §Enable sync):
    /// `POST /v1/vaults` with the same `vault_id`, the VAK of the existing
    /// VRK, the existing password and recovery envelopes (still valid — they
    /// are bound only to `vault_id`) and a fresh device envelope for the
    /// device registered with the server.
    pub fn enable_sync_request(
        &self,
        password_envelope: &(impl EnvelopeSource + ?Sized),
        recovery_envelope: &(impl EnvelopeSource + ?Sized),
        registered_device: &DeviceIdentity,
    ) -> Result<CreateVaultRequest> {
        let password = self.checked_envelope(password_envelope, RecipientType::Password)?;
        let recovery = self.checked_envelope(recovery_envelope, RecipientType::Recovery)?;
        Ok(CreateVaultRequest {
            vault_id: self.vault_id,
            vault_access_key: self.vault_access_key(),
            password_envelope: password,
            recovery_envelope: recovery,
            device_envelope: self.device_envelope_for(registered_device)?,
        })
    }

    /// Envelope of this vault with structural validation and type check.
    pub(crate) fn checked_envelope(
        &self,
        source: &(impl EnvelopeSource + ?Sized),
        expected: RecipientType,
    ) -> Result<NewEnvelope> {
        let env = envelope_for(source, self.vault_id)?.into_owned();
        env.validate().map_err(cc_crypto_core::CryptoError::from)?;
        if env.recipient_type != expected {
            return Err(cc_crypto_core::CryptoError::WrongRecipientType {
                expected,
                actual: env.recipient_type,
            }
            .into());
        }
        Ok(env)
    }
}

impl fmt::Debug for UnlockedVault {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("UnlockedVault")
            .field("vault_id", &self.vault_id)
            .field("keys", &"<redacted>")
            .finish()
    }
}

/// Passphrase policy for *setting* a passphrase.
pub(crate) fn check_new_passphrase(passphrase: &SecretString) -> Result<()> {
    if normalize_passphrase(passphrase).chars().count() < MIN_PASSPHRASE_CHARS {
        return Err(VaultError::WeakPassphrase);
    }
    Ok(())
}
