//! VRK envelopes (ADR-0002 §Envelopes): the Vault Root Key encrypted for a
//! passphrase, the Recovery Key, or a device's X25519 key.
//!
//! Every `open_*` function first runs the protocol's structural
//! [`NewEnvelope::validate`] (defence against a hostile server), then checks
//! recipient type / recipient id, then authenticates the ciphertext with the
//! AAD `envelope_aad(vault_id, recipient_code, recipient_id)`.

use crate::aead;
use crate::device::{random_x25519_secret, x25519, x25519_public, DeviceSecretKeys};
use crate::error::{CryptoError, Result};
use crate::kdf::{derive_password_key, hkdf_sha256_32, Argon2Params};
use crate::keys::{Secret32, Vrk};
use crate::recovery::RecoveryKey;
use crate::rng::random_array;
use cc_protocol::canonical::{envelope_aad, labels};
use cc_protocol::envelopes::{
    EnvelopeAlgorithm, EnvelopeKind, EnvelopeMetadata, KeyEnvelope, NewEnvelope, RecipientType,
};
use cc_protocol::limits::{ARGON2_SALT_LEN, NONCE_LEN, X25519_PUBLIC_KEY_LEN};
use cc_protocol::{Bytes, DeviceId, VaultId};
use secrecy::{ExposeSecret, SecretString};
use x25519_dalek::StaticSecret;

/// ADR-0002 recipient codes bound into the envelope AAD.
pub const RECIPIENT_CODE_PASSWORD: u8 = 1;
/// See [`RECIPIENT_CODE_PASSWORD`].
pub const RECIPIENT_CODE_RECOVERY: u8 = 2;
/// See [`RECIPIENT_CODE_PASSWORD`].
pub const RECIPIENT_CODE_DEVICE: u8 = 3;
/// See [`RECIPIENT_CODE_PASSWORD`]. Team Vault foundation, unused in MVP.
pub const RECIPIENT_CODE_USER: u8 = 4;

// ---------------------------------------------------------------------------
// password
// ---------------------------------------------------------------------------

/// Wrap `vrk` under `Argon2id(NFC(passphrase))` with a fresh salt and nonce.
pub fn seal_password_envelope(
    vrk: &Vrk,
    vault_id: VaultId,
    passphrase: &SecretString,
    params: Argon2Params,
) -> Result<NewEnvelope> {
    let salt = random_array::<ARGON2_SALT_LEN>()?;
    let nonce = random_array::<NONCE_LEN>()?;
    seal_password_envelope_with(vrk, vault_id, passphrase, params, &salt, &nonce)
}

/// Deterministic variant (explicit salt/nonce) — test vectors only.
pub(crate) fn seal_password_envelope_with(
    vrk: &Vrk,
    vault_id: VaultId,
    passphrase: &SecretString,
    params: Argon2Params,
    salt: &[u8; ARGON2_SALT_LEN],
    nonce: &[u8; NONCE_LEN],
) -> Result<NewEnvelope> {
    let pek = derive_password_key(passphrase, salt, params)?;
    let aad = envelope_aad(vault_id, RECIPIENT_CODE_PASSWORD, None);
    let ciphertext = aead::seal(pek.expose_secret(), nonce, &aad, vrk.expose())?;
    Ok(NewEnvelope {
        recipient_type: RecipientType::Password,
        recipient_id: None,
        kind: EnvelopeKind::VrkV1,
        metadata: EnvelopeMetadata {
            algorithm: EnvelopeAlgorithm::Argon2idXchacha20poly1305V1,
            kdf: Some(params.to_protocol(salt)),
            ephemeral_public_key: None,
        },
        ciphertext: Bytes::new(ciphertext),
        nonce: Bytes::new(nonce.to_vec()),
    })
}

/// Argon2id parameters of a (validated) password envelope, e.g. to decide
/// whether to re-wrap with stronger parameters after unlocking.
pub fn password_envelope_params(envelope: &NewEnvelope) -> Result<Argon2Params> {
    check_structure(envelope, RecipientType::Password)?;
    let kdf = envelope
        .metadata
        .kdf
        .as_ref()
        .ok_or(CryptoError::KdfParams("missing"))?;
    Argon2Params::from_protocol(kdf).map(|(p, _)| p)
}

/// Unwrap the VRK from a password envelope. A wrong passphrase and a
/// tampered envelope both yield [`CryptoError::Decrypt`].
pub fn open_password_envelope(
    envelope: &NewEnvelope,
    vault_id: VaultId,
    passphrase: &SecretString,
) -> Result<Vrk> {
    let nonce = check_structure(envelope, RecipientType::Password)?;
    let kdf = envelope
        .metadata
        .kdf
        .as_ref()
        .ok_or(CryptoError::KdfParams("missing"))?;
    // Client-side ceiling check happens here, *before* any memory is
    // allocated for Argon2.
    let (params, salt) = Argon2Params::from_protocol(kdf)?;
    let pek = derive_password_key(passphrase, &salt, params)?;
    let aad = envelope_aad(vault_id, RECIPIENT_CODE_PASSWORD, None);
    open_vrk(&pek, &nonce, &aad, envelope)
}

// ---------------------------------------------------------------------------
// recovery
// ---------------------------------------------------------------------------

pub(crate) fn recovery_kek(rk: &RecoveryKey, vault_id: VaultId) -> Secret32 {
    hkdf_sha256_32(rk.expose(), vault_id.as_bytes(), &[labels::RECOVERY_KEK])
}

/// Wrap `vrk` under `HKDF(RK, vault_id, "…/recovery-kek")`.
pub fn seal_recovery_envelope(
    vrk: &Vrk,
    vault_id: VaultId,
    rk: &RecoveryKey,
) -> Result<NewEnvelope> {
    let nonce = random_array::<NONCE_LEN>()?;
    seal_recovery_envelope_with(vrk, vault_id, rk, &nonce)
}

/// Deterministic variant (explicit nonce) — test vectors only.
pub(crate) fn seal_recovery_envelope_with(
    vrk: &Vrk,
    vault_id: VaultId,
    rk: &RecoveryKey,
    nonce: &[u8; NONCE_LEN],
) -> Result<NewEnvelope> {
    let rek = recovery_kek(rk, vault_id);
    let aad = envelope_aad(vault_id, RECIPIENT_CODE_RECOVERY, None);
    let ciphertext = aead::seal(rek.expose_secret(), nonce, &aad, vrk.expose())?;
    Ok(NewEnvelope {
        recipient_type: RecipientType::Recovery,
        recipient_id: None,
        kind: EnvelopeKind::VrkV1,
        metadata: EnvelopeMetadata {
            algorithm: EnvelopeAlgorithm::HkdfSha256Xchacha20poly1305V1,
            kdf: None,
            ephemeral_public_key: None,
        },
        ciphertext: Bytes::new(ciphertext),
        nonce: Bytes::new(nonce.to_vec()),
    })
}

/// Unwrap the VRK from a recovery envelope.
pub fn open_recovery_envelope(
    envelope: &NewEnvelope,
    vault_id: VaultId,
    rk: &RecoveryKey,
) -> Result<Vrk> {
    let nonce = check_structure(envelope, RecipientType::Recovery)?;
    let rek = recovery_kek(rk, vault_id);
    let aad = envelope_aad(vault_id, RECIPIENT_CODE_RECOVERY, None);
    open_vrk(&rek, &nonce, &aad, envelope)
}

// ---------------------------------------------------------------------------
// device
// ---------------------------------------------------------------------------

/// `K = HKDF(shared, salt = E || D, info = "…/device-envelope" || vault_id)`.
pub(crate) fn device_envelope_key(
    shared: &Secret32,
    ephemeral_public: &[u8; 32],
    recipient_public: &[u8; 32],
    vault_id: VaultId,
) -> Secret32 {
    let mut salt = [0u8; 64];
    salt[..32].copy_from_slice(ephemeral_public);
    salt[32..].copy_from_slice(recipient_public);
    hkdf_sha256_32(
        shared.expose_secret(),
        &salt,
        &[labels::DEVICE_ENVELOPE, vault_id.as_bytes()],
    )
}

/// Wrap `vrk` for device `recipient_device_id` whose X25519 public key is
/// `recipient_encryption_key` (fresh ephemeral X25519 keypair per envelope).
///
/// When the key comes from the server (device approval), the caller must
/// have had the user compare the verification code first (ADR-0004).
pub fn seal_device_envelope(
    vrk: &Vrk,
    vault_id: VaultId,
    recipient_device_id: DeviceId,
    recipient_encryption_key: &[u8; X25519_PUBLIC_KEY_LEN],
) -> Result<NewEnvelope> {
    let ephemeral = random_x25519_secret()?;
    let nonce = random_array::<NONCE_LEN>()?;
    seal_device_envelope_with(
        vrk,
        vault_id,
        recipient_device_id,
        recipient_encryption_key,
        &ephemeral,
        &nonce,
    )
}

/// Deterministic variant (explicit ephemeral key and nonce) — test vectors only.
pub(crate) fn seal_device_envelope_with(
    vrk: &Vrk,
    vault_id: VaultId,
    recipient_device_id: DeviceId,
    recipient_encryption_key: &[u8; X25519_PUBLIC_KEY_LEN],
    ephemeral: &StaticSecret,
    nonce: &[u8; NONCE_LEN],
) -> Result<NewEnvelope> {
    let ephemeral_public = x25519_public(ephemeral);
    let shared = x25519(ephemeral, recipient_encryption_key)?;
    let key = device_envelope_key(
        &shared,
        &ephemeral_public,
        recipient_encryption_key,
        vault_id,
    );
    let aad = envelope_aad(
        vault_id,
        RECIPIENT_CODE_DEVICE,
        Some(recipient_device_id.as_bytes()),
    );
    let ciphertext = aead::seal(key.expose_secret(), nonce, &aad, vrk.expose())?;
    Ok(NewEnvelope {
        recipient_type: RecipientType::Device,
        recipient_id: Some(recipient_device_id.0),
        kind: EnvelopeKind::VrkV1,
        metadata: EnvelopeMetadata {
            algorithm: EnvelopeAlgorithm::X25519HkdfSha256Xchacha20poly1305V1,
            kdf: None,
            ephemeral_public_key: Some(Bytes::new(ephemeral_public.to_vec())),
        },
        ciphertext: Bytes::new(ciphertext),
        nonce: Bytes::new(nonce.to_vec()),
    })
}

/// Unwrap the VRK from a device envelope addressed to `device_id`, using
/// that device's secret keys.
pub fn open_device_envelope(
    envelope: &NewEnvelope,
    vault_id: VaultId,
    device_id: DeviceId,
    keys: &DeviceSecretKeys,
) -> Result<Vrk> {
    let nonce = check_structure(envelope, RecipientType::Device)?;
    if envelope.recipient_id != Some(device_id.0) {
        return Err(CryptoError::WrongRecipientDevice);
    }
    let ephemeral_public: [u8; 32] = envelope
        .metadata
        .ephemeral_public_key
        .as_ref()
        .and_then(|k| k.to_array())
        .ok_or(CryptoError::InvalidKey {
            what: "ephemeral public key",
        })?;
    let shared = keys.diffie_hellman(&ephemeral_public)?;
    let own_public = keys.public_keys().encryption;
    let key = device_envelope_key(&shared, &ephemeral_public, &own_public, vault_id);
    let aad = envelope_aad(vault_id, RECIPIENT_CODE_DEVICE, Some(device_id.as_bytes()));
    open_vrk(&key, &nonce, &aad, envelope)
}

// ---------------------------------------------------------------------------
// shared helpers
// ---------------------------------------------------------------------------

/// Accept an envelope as stored by the server for `vault_id`: checks the
/// server-reported vault id and returns the uploadable shape for the
/// `open_*` functions (which validate the rest).
pub fn stored_envelope_for_vault(envelope: &KeyEnvelope, vault_id: VaultId) -> Result<NewEnvelope> {
    if envelope.vault_id != vault_id {
        return Err(CryptoError::VaultMismatch);
    }
    Ok(envelope.to_new())
}

/// Structural validation + recipient-type check; returns the nonce.
fn check_structure(envelope: &NewEnvelope, expected: RecipientType) -> Result<[u8; NONCE_LEN]> {
    envelope.validate()?;
    if envelope.recipient_type != expected {
        return Err(CryptoError::WrongRecipientType {
            expected,
            actual: envelope.recipient_type,
        });
    }
    let EnvelopeKind::VrkV1 = envelope.kind;
    envelope
        .nonce
        .to_array::<NONCE_LEN>()
        .ok_or(CryptoError::InvalidEnvelope(
            cc_protocol::envelopes::EnvelopeValidationError::Nonce,
        ))
}

fn open_vrk(
    key: &Secret32,
    nonce: &[u8; NONCE_LEN],
    aad: &[u8],
    envelope: &NewEnvelope,
) -> Result<Vrk> {
    aead::open_key32(
        key.expose_secret(),
        nonce,
        aad,
        envelope.ciphertext.as_slice(),
    )
    .map(Vrk::from_secret)
}
