//! Device unlock under OS protection and the server trust marker (S5).
//!
//! **Kept in one place on purpose** — a later rework of device-envelope
//! trust (audit H2: key commitment, cached own envelope) touches this module
//! only.
//!
//! * **Unlock envelope** (local, opt-in): the VRK sealed under an
//!   [`UnlockKey`] that only the operating system releases after biometrics /
//!   the device password ([`UnlockedVault::seal_unlock_envelope`],
//!   [`unlock_with_unlock_key`]). It exists only while the user has
//!   "Unlock with Touch ID / Windows Hello / fingerprint" switched on.
//! * **Local storage wrap**: the profile database key sealed under the VRK
//!   ([`UnlockedVault::seal_local_secret`]) — the database opens only after
//!   an unlock.
//! * **Trust envelope** (server): ADR-0004 trust T(V) of a device is "holds
//!   a live device envelope for V" (sync push, approvals, envelope listing).
//!   The device secret lives in a plain OS secret-store item (it signs
//!   requests and serves sharing / approvals), so an envelope holding the
//!   *real* VRK would let any program of the same OS user fetch it from the
//!   server and decrypt the vault. This installation therefore uploads for
//!   itself a [`UnlockedVault::trust_envelope_for`]: a well-formed device
//!   envelope sealing a random 32-byte value — a trust marker, never a key.
//!   The vault access key in the same request still proves VRK knowledge.
//!   Envelopes for *other* devices (approval) keep the real VRK: the new
//!   device needs it once and then replaces it with its own trust envelope.

use crate::error::{Result, VaultError};
use crate::identity::DeviceIdentity;
use crate::unlocked::UnlockedVault;
use cc_crypto_core::{
    open_local_secret, open_unlock_envelope, seal_device_envelope, seal_local_secret,
    seal_unlock_envelope, CryptoError, LocalSealed, UnlockKey, Vrk,
};
use cc_protocol::envelopes::NewEnvelope;
use cc_protocol::VaultId;
use zeroize::Zeroizing;

impl UnlockedVault {
    /// Seal the VRK for device unlock under the OS-released `key`, bound to
    /// `context` (the profile id).
    pub fn seal_unlock_envelope(&self, key: &UnlockKey, context: &[u8]) -> Result<LocalSealed> {
        Ok(seal_unlock_envelope(
            self.vrk(),
            self.vault_id(),
            key,
            context,
        )?)
    }

    /// Seal a local secret (the database key) under the VRK.
    pub fn seal_local_secret(&self, context: &[u8], secret: &[u8]) -> Result<LocalSealed> {
        Ok(seal_local_secret(
            self.vrk(),
            self.vault_id(),
            context,
            secret,
        )?)
    }

    /// Open a secret of [`UnlockedVault::seal_local_secret`].
    pub fn open_local_secret(
        &self,
        context: &[u8],
        sealed: &LocalSealed,
    ) -> Result<Zeroizing<Vec<u8>>> {
        Ok(open_local_secret(
            self.vrk(),
            self.vault_id(),
            context,
            sealed,
        )?)
    }

    /// This installation's server device envelope: a **trust marker**
    /// sealing a random value, never the VRK (see the module docs).
    pub fn trust_envelope_for(&self, identity: &DeviceIdentity) -> Result<NewEnvelope> {
        let filler = Vrk::generate()?;
        Ok(seal_device_envelope(
            &filler,
            self.vault_id(),
            identity.device_id(),
            &identity.public_keys().encryption,
        )?)
    }
}

/// Unlock with the device unlock key the OS released. A wrong key (another
/// enrollment, another profile) → [`VaultError::DeviceNotAuthorized`].
pub fn unlock_with_unlock_key(
    vault_id: VaultId,
    envelope: &LocalSealed,
    key: &UnlockKey,
    context: &[u8],
) -> Result<UnlockedVault> {
    let vrk = open_unlock_envelope(envelope, vault_id, key, context).map_err(|e| match e {
        CryptoError::Decrypt => VaultError::DeviceNotAuthorized,
        other => VaultError::Crypto(other),
    })?;
    Ok(UnlockedVault::from_vrk(vault_id, vrk))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::unlock::unlock_with_device;
    use cc_crypto_core::Argon2Params;
    use secrecy::SecretString;

    fn vault() -> (UnlockedVault, DeviceIdentity) {
        let identity = DeviceIdentity::generate().unwrap();
        let created = crate::create_vault(
            &identity,
            &SecretString::from("correct horse battery"),
            Argon2Params::for_tests(),
            None,
            chrono::Utc::now(),
        )
        .unwrap();
        (created.unlocked, identity)
    }

    #[test]
    fn unlock_envelope_roundtrip_needs_the_os_key() {
        let (v, _) = vault();
        let key = UnlockKey::from_material(&[3u8; 64]).unwrap();
        let env = v.seal_unlock_envelope(&key, b"profile-1").unwrap();
        let again = unlock_with_unlock_key(
            v.vault_id(),
            &env,
            &UnlockKey::from_material(&[3u8; 64]).unwrap(),
            b"profile-1",
        )
        .unwrap();
        assert_eq!(again.vault_access_key(), v.vault_access_key());
        let wrong = UnlockKey::from_material(&[4u8; 64]).unwrap();
        assert_eq!(
            unlock_with_unlock_key(v.vault_id(), &env, &wrong, b"profile-1").err(),
            Some(VaultError::DeviceNotAuthorized)
        );
        assert!(unlock_with_unlock_key(v.vault_id(), &env, &key, b"profile-2").is_err());
    }

    #[test]
    fn trust_envelope_is_valid_but_holds_no_key() {
        let (v, identity) = vault();
        let env = v.trust_envelope_for(&identity).unwrap();
        env.validate().unwrap();
        assert_eq!(env.recipient_id, Some(identity.device_id().0));
        // The device can open it, but what comes out is not this vault's VRK.
        let opened = unlock_with_device(v.vault_id(), &env, &identity).unwrap();
        assert_ne!(opened.vault_access_key(), v.vault_access_key());
        // Fresh filler every time.
        assert_ne!(env, v.trust_envelope_for(&identity).unwrap());
    }

    #[test]
    fn local_secret_roundtrip() {
        let (v, _) = vault();
        let sealed = v.seal_local_secret(b"db:p1", &[9u8; 32]).unwrap();
        assert_eq!(
            v.open_local_secret(b"db:p1", &sealed).unwrap().as_slice(),
            &[9u8; 32]
        );
        let (other, _) = vault();
        assert!(other.open_local_secret(b"db:p1", &sealed).is_err());
    }
}
