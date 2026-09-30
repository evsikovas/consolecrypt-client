//! Selective-object sharing, isolated from all personal-vault keys.
//!
//! An owner-pinned signed access manifest supplies device keys and roles.
//! Every revision has a fresh random DEK, sealed separately for each approved
//! device. A reader's possession of the DEK never grants authorship: mutations
//! additionally need the signature of a currently authorized editor.
//!
//! These primitives do not establish human identity. The caller must compare
//! the owner's and initial recipients' verification codes over a trusted
//! channel. They also do not prove that a server has disclosed its latest
//! state. Durable checkpoints detect rollback after a state has been seen;
//! an initial download may still be stale, and revoked users retain old data.

use crate::aead;
use crate::device::{random_x25519_secret, x25519, x25519_public};
use crate::kdf::hkdf_sha256_32;
use crate::keys::Dek;
use crate::object::padded_len;
use crate::rng::random_array;
use crate::{CryptoError, DevicePublicKeys, DeviceSecretKeys, ExposeSecret};
use cc_protocol::sharing::{
    labels, sharing_body_message, sharing_envelope_aad, sharing_manifest_message,
    sharing_mutation_message, sharing_object_aad, AccessManifest, SharedEncryptedBody,
    SharedKeyEnvelope, SharingContext, SharingMember, SharingMutation, SharingOperation,
    SharingRole, SignedAccessManifest, SignedSharingMutation,
};
use cc_protocol::{Bytes, DeviceId, UserId};
use ed25519_dalek::{Signature, VerifyingKey};
use sha2::{Digest, Sha256};
use std::collections::HashSet;
use std::fmt;
use zeroize::{Zeroize, Zeroizing};

const FORMAT: u16 = 1;
const NONCE_BYTES: usize = 24;
const ENVELOPE_BYTES: usize = 48;
const HASH_BYTES: usize = 32;
const SIGNATURE_BYTES: usize = 64;
/// Maximum selected plaintext after reserving the length prefix and AEAD tag.
pub const MAX_SHARED_PLAINTEXT_BYTES: usize = cc_protocol::sharing::MAX_CIPHERTEXT_BYTES - 16 - 4;
const MAX_PADDED_PLAINTEXT: usize = MAX_SHARED_PLAINTEXT_BYTES + 4;

/// Errors never include plaintext, device secrets, DEKs, or ciphertext bytes.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SharingCryptoError {
    #[error(transparent)]
    Crypto(#[from] CryptoError),
    #[error("invalid sharing structure: {0}")]
    Structure(&'static str),
    #[error("sharing identity or context differs from the trusted one")]
    ContextMismatch,
    #[error("shared access owner does not match the pinned owner")]
    OwnerMismatch,
    #[error("shared access history rolls back an accepted checkpoint")]
    Rollback,
    #[error("shared access history contains a conflicting state")]
    Fork,
    #[error("shared access history is missing intermediate signed states")]
    HistoryGap,
    #[error("device is not included in the verified shared access manifest")]
    NotRecipient,
    #[error("device cannot edit this shared item")]
    ReadOnly,
    #[error("device keys differ from the approved shared device keys")]
    DeviceKeyMismatch,
}

type Result<T> = std::result::Result<T, SharingCryptoError>;

/// Public owner identity pinned after an out-of-band code comparison.
/// Neither a server directory entry nor successful account login creates it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SharingOwnerAnchor {
    pub user_id: UserId,
    pub device_id: DeviceId,
    pub public_keys: DevicePublicKeys,
}

/// A manifest whose signature, owner, location, key lengths, roles, and
/// optional predecessor checkpoint have been checked. Fields are private so
/// untrusted wire DTOs cannot be passed to decryption or author verification.
#[derive(Debug, Clone)]
pub struct VerifiedSharingManifest {
    signed: SignedAccessManifest,
    hash: [u8; HASH_BYTES],
}

/// Persist this checkpoint with the encrypted local state. Replaying the same
/// revision is allowed only with the same canonical hash; restoring an older
/// database must never silently lower it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SharingManifestCheckpoint {
    pub revision: u64,
    pub access_epoch: u64,
    pub hash: [u8; HASH_BYTES],
}

/// Accepted revision of one object, including tombstones. Keep the access
/// state in this checkpoint too: an old editor must not extend the public
/// revision hash chain using an older, still correctly signed manifest.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SharingRevisionCheckpoint {
    pub revision: i64,
    pub hash: [u8; HASH_BYTES],
    pub manifest_revision: u64,
    pub manifest_hash: [u8; HASH_BYTES],
    pub access_epoch: u64,
}

/// Only mutations with a current editor signature can produce this wrapper.
#[derive(Debug, Clone)]
pub struct VerifiedSharingMutation {
    signed: SignedSharingMutation,
    hash: [u8; HASH_BYTES],
}

/// Decrypted bytes stay in a zeroizing allocation and never print through
/// Debug. Application-level payload validation must finish before use.
pub struct SharedPlaintext(Zeroizing<Vec<u8>>);

/// Narrow device operation for application identity wrappers. Implementors
/// never export private key bytes; input is already authenticated by this core.
pub trait SharingRevisionOpener {
    fn open_shared_revision(
        &self,
        manifest: &VerifiedSharingManifest,
        revision: &VerifiedSharingMutation,
        body: Option<&SharedEncryptedBody>,
        recipient_device_id: DeviceId,
    ) -> Result<Option<SharedPlaintext>>;
}

impl SharingRevisionOpener for DeviceSecretKeys {
    fn open_shared_revision(
        &self,
        manifest: &VerifiedSharingManifest,
        revision: &VerifiedSharingMutation,
        body: Option<&SharedEncryptedBody>,
        recipient_device_id: DeviceId,
    ) -> Result<Option<SharedPlaintext>> {
        open_shared_revision(manifest, revision, body, recipient_device_id, self)
    }
}

/// Human comparison code, distinct from login/device-enrolment codes. Binds
/// both public keys to the exact instance, account and installation identity.
pub fn sharing_identity_code(
    server_instance_id: uuid::Uuid,
    user_id: UserId,
    device_id: DeviceId,
    keys: &DevicePublicKeys,
) -> Result<String> {
    if server_instance_id.is_nil() || user_id == UserId::NIL || device_id == DeviceId::NIL {
        return Err(SharingCryptoError::ContextMismatch);
    }
    let mut hash = Sha256::new();
    hash.update(b"consolecrypt-sharing-identity-code-v1\0");
    hash.update(server_instance_id.as_bytes());
    hash.update(user_id.as_bytes());
    hash.update(device_id.as_bytes());
    hash.update(keys.encryption_bytes().as_slice());
    hash.update(keys.signing_bytes().as_slice());
    let digest = hash.finalize();
    let mut code = String::with_capacity(71);
    for (index, byte) in digest.iter().enumerate() {
        if index > 0 && index % 4 == 0 {
            code.push('-');
        }
        use std::fmt::Write as _;
        write!(code, "{byte:02X}").expect("writing comparison code to String cannot fail");
    }
    Ok(code)
}

impl SharedPlaintext {
    pub fn as_slice(&self) -> &[u8] {
        &self.0
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl fmt::Debug for SharedPlaintext {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("SharedPlaintext(<redacted>)")
    }
}

impl VerifiedSharingManifest {
    pub fn manifest(&self) -> &AccessManifest {
        &self.signed.manifest
    }

    pub fn hash(&self) -> [u8; HASH_BYTES] {
        self.hash
    }

    pub fn checkpoint(&self) -> SharingManifestCheckpoint {
        SharingManifestCheckpoint {
            revision: self.signed.manifest.revision,
            access_epoch: self.signed.manifest.access_epoch,
            hash: self.hash,
        }
    }

    pub fn member(&self, device_id: DeviceId) -> Option<&SharingMember> {
        self.signed
            .manifest
            .members
            .iter()
            .find(|m| m.device_id == device_id)
    }
}

impl VerifiedSharingMutation {
    pub fn mutation(&self) -> &SharingMutation {
        &self.signed.mutation
    }

    pub fn checkpoint(&self) -> SharingRevisionCheckpoint {
        SharingRevisionCheckpoint {
            revision: self.signed.mutation.context.revision,
            hash: self.hash,
            manifest_revision: self.signed.mutation.manifest_revision,
            manifest_hash: self
                .signed
                .mutation
                .manifest_hash
                .to_array()
                .expect("verified manifest hash is 32 bytes"),
            access_epoch: self.signed.mutation.context.access_epoch,
        }
    }
}

/// Verify a manifest against an owner pinned by the human, not keys supplied
/// by the same server response. The expected context's epoch is checked; its
/// object revision is unrelated to the access-manifest revision.
///
/// `previous` must be the durable last accepted manifest checkpoint. An exact
/// successor is required, so callers fetch and verify intermediate manifests
/// after an offline gap rather than discarding the checkpoint.
pub fn verify_shared_manifest(
    signed: &SignedAccessManifest,
    expected: &SharingContext,
    owner: &SharingOwnerAnchor,
    previous: Option<&SharingManifestCheckpoint>,
) -> Result<VerifiedSharingManifest> {
    let manifest = &signed.manifest;
    manifest_context(manifest, expected)?;
    let message = sharing_manifest_message(manifest)
        .map_err(|_| SharingCryptoError::Structure("access manifest"))?;
    if manifest.revision != manifest.access_epoch {
        return Err(SharingCryptoError::Structure(
            "manifest revision and epoch differ",
        ));
    }
    for member in &manifest.members {
        member_public_keys(member)?;
    }
    check_owner(manifest, owner)?;
    verify_signature(&owner.public_keys.signing, &message, &signed.signature)?;
    let hash = sha256(&message);
    if let Some(previous) = previous {
        if manifest.revision < previous.revision || manifest.access_epoch < previous.access_epoch {
            return Err(SharingCryptoError::Rollback);
        }
        if manifest.revision == previous.revision {
            if manifest.access_epoch != previous.access_epoch || hash != previous.hash {
                return Err(SharingCryptoError::Fork);
            }
        } else {
            if manifest.revision
                != previous
                    .revision
                    .checked_add(1)
                    .ok_or(SharingCryptoError::Structure("manifest revision overflow"))?
                || manifest.access_epoch
                    != previous
                        .access_epoch
                        .checked_add(1)
                        .ok_or(SharingCryptoError::Structure("access epoch overflow"))?
            {
                return Err(SharingCryptoError::HistoryGap);
            }
            if manifest.previous_manifest_hash.as_slice() != previous.hash {
                return Err(SharingCryptoError::Fork);
            }
        }
    }
    Ok(VerifiedSharingManifest {
        signed: signed.clone(),
        hash,
    })
}

/// Sign an access change after code confirmation and then apply the same
/// strict checks as a receiving client. Only the pinned owner can sign it.
pub fn sign_shared_manifest(
    keys: &DeviceSecretKeys,
    manifest: AccessManifest,
    expected: &SharingContext,
    owner: &SharingOwnerAnchor,
    previous: Option<&SharingManifestCheckpoint>,
) -> Result<SignedAccessManifest> {
    if keys.public_keys() != owner.public_keys {
        return Err(SharingCryptoError::OwnerMismatch);
    }
    let message = sharing_manifest_message(&manifest)
        .map_err(|_| SharingCryptoError::Structure("access manifest"))?;
    let signed = SignedAccessManifest {
        manifest,
        signature: Bytes::from(keys.sign_message(&message)),
    };
    verify_shared_manifest(&signed, expected, owner, previous)?;
    Ok(signed)
}

/// Fresh DEK and nonces on every call, including rotations of unchanged
/// plaintext. This never reads, copies, or wraps a personal VRK or KEK.
///
/// Caller supplies a sanitized projection of exactly the selected object.
/// Passing a full host/credential JSON would share all those fields; this
/// primitive deliberately does not guess which application fields are secret.
pub fn seal_shared_revision(
    manifest: &VerifiedSharingManifest,
    context: &SharingContext,
    plaintext: &[u8],
) -> Result<SharedEncryptedBody> {
    manifest_context(manifest.manifest(), context)?;
    let object_aad =
        sharing_object_aad(context).map_err(|_| SharingCryptoError::Structure("object context"))?;
    let plain = pad(plaintext)?;
    let dek = Dek::generate()?;
    let nonce = random_array::<NONCE_BYTES>()?;
    let ciphertext = aead::seal(dek.expose(), &nonce, &object_aad, &plain)?;
    let mut envelopes = Vec::with_capacity(manifest.manifest().members.len());
    for member in &manifest.manifest().members {
        let public = member_public_keys(member)?;
        let ephemeral = random_x25519_secret()?;
        let ephemeral_public = x25519_public(&ephemeral);
        let shared = x25519(&ephemeral, &public.encryption)?;
        let aad = sharing_envelope_aad(context, member, &ephemeral_public)
            .map_err(|_| SharingCryptoError::Structure("recipient envelope context"))?;
        let key = envelope_key(&shared, &ephemeral_public, &public.encryption, &aad);
        let envelope_nonce = random_array::<NONCE_BYTES>()?;
        let ciphertext = aead::seal(key.expose_secret(), &envelope_nonce, &aad, dek.expose())?;
        envelopes.push(SharedKeyEnvelope {
            recipient_device_id: member.device_id,
            ephemeral_public_key: Bytes::from(ephemeral_public),
            nonce: Bytes::from(envelope_nonce),
            ciphertext: Bytes::from(ciphertext),
        });
    }
    let body = SharedEncryptedBody {
        format: FORMAT,
        ciphertext: Bytes::from(ciphertext),
        nonce: Bytes::from(nonce),
        envelopes,
    };
    validate_body(manifest, &body)?;
    Ok(body)
}

/// SHA-256 of a structurally valid canonical shared body. This includes every
/// recipient envelope as well as both nonces and ciphertexts.
pub fn shared_body_hash(body: &SharedEncryptedBody) -> Result<[u8; HASH_BYTES]> {
    let message =
        sharing_body_message(body).map_err(|_| SharingCryptoError::Structure("encrypted body"))?;
    Ok(sha256(&message))
}

/// Sign an update or tombstone with an approved editor's device key. The full
/// body must match its header hash before a Put can be signed. Readers can
/// encrypt arbitrary bytes with an old DEK but cannot pass this role check or
/// create a signature accepted as an editor's signature by another client.
pub fn sign_shared_mutation(
    manifest: &VerifiedSharingManifest,
    keys: &DeviceSecretKeys,
    mutation: SharingMutation,
    body: Option<&SharedEncryptedBody>,
    previous: Option<&SharingRevisionCheckpoint>,
) -> Result<SignedSharingMutation> {
    let member = editor(manifest, mutation.writer_device_id)?;
    if keys.public_keys() != member_public_keys(member)? {
        return Err(SharingCryptoError::DeviceKeyMismatch);
    }
    check_mutation_context(manifest, &mutation)?;
    check_body(manifest, &mutation, body)?;
    let message = sharing_mutation_message(&mutation)
        .map_err(|_| SharingCryptoError::Structure("mutation header"))?;
    let signed = SignedSharingMutation {
        mutation,
        signature: Bytes::from(keys.sign_message(&message)),
    };
    verify_shared_mutation(manifest, &signed, previous)?;
    Ok(signed)
}

/// Verify a signed header independently of its body, so an offline client can
/// authenticate lightweight history before downloading the latest ciphertext.
/// Pass the manifest which was current at this header's manifest revision;
/// never authorize a historic author using an unverified server role field.
/// A successor must also preserve or advance the checkpoint's access state.
/// An access transition advances both chains by one and is owner-authored.
pub fn verify_shared_mutation(
    manifest: &VerifiedSharingManifest,
    signed: &SignedSharingMutation,
    previous: Option<&SharingRevisionCheckpoint>,
) -> Result<VerifiedSharingMutation> {
    let mutation = &signed.mutation;
    check_mutation_context(manifest, mutation)?;
    let member = editor(manifest, mutation.writer_device_id)?;
    let message = sharing_mutation_message(mutation)
        .map_err(|_| SharingCryptoError::Structure("mutation header"))?;
    verify_signature(
        &member_public_keys(member)?.signing,
        &message,
        &signed.signature,
    )?;
    let hash = sha256(&message);
    if let Some(previous) = previous {
        if mutation.context.revision < previous.revision
            || mutation.context.access_epoch < previous.access_epoch
            || mutation.manifest_revision < previous.manifest_revision
        {
            return Err(SharingCryptoError::Rollback);
        }
        if mutation.context.revision == previous.revision {
            if hash != previous.hash
                || mutation.context.access_epoch != previous.access_epoch
                || mutation.manifest_revision != previous.manifest_revision
                || mutation.manifest_hash.as_slice() != previous.manifest_hash
            {
                return Err(SharingCryptoError::Fork);
            }
        } else {
            if mutation.base_revision != previous.revision {
                return Err(SharingCryptoError::HistoryGap);
            }
            if mutation.previous_revision_hash.as_slice() != previous.hash {
                return Err(SharingCryptoError::Fork);
            }
            if mutation.manifest_revision == previous.manifest_revision {
                if mutation.context.access_epoch != previous.access_epoch
                    || mutation.manifest_hash.as_slice() != previous.manifest_hash
                {
                    return Err(SharingCryptoError::Fork);
                }
            } else {
                if previous.manifest_revision.checked_add(1) != Some(mutation.manifest_revision)
                    || previous.access_epoch.checked_add(1) != Some(mutation.context.access_epoch)
                {
                    return Err(SharingCryptoError::HistoryGap);
                }
                if manifest.manifest().previous_manifest_hash.as_slice() != previous.manifest_hash {
                    return Err(SharingCryptoError::Fork);
                }
                if mutation.writer_device_id != manifest.manifest().owner_device_id
                    || mutation.operation != SharingOperation::Put
                {
                    return Err(SharingCryptoError::Structure(
                        "access transition writer or operation",
                    ));
                }
            }
        }
    }
    Ok(VerifiedSharingMutation {
        signed: signed.clone(),
        hash,
    })
}

/// Authenticate the body and the current editor, open this device's envelope,
/// and return padded-decoded plaintext in a zeroizing allocation. Tombstones
/// return `None`; a Put without a body is an error.
pub fn open_shared_revision(
    manifest: &VerifiedSharingManifest,
    verified: &VerifiedSharingMutation,
    body: Option<&SharedEncryptedBody>,
    recipient_device_id: DeviceId,
    keys: &DeviceSecretKeys,
) -> Result<Option<SharedPlaintext>> {
    let mutation = verified.mutation();
    check_mutation_context(manifest, mutation)?;
    let recipient = manifest
        .member(recipient_device_id)
        .ok_or(SharingCryptoError::NotRecipient)?;
    let public = member_public_keys(recipient)?;
    if keys.public_keys() != public {
        return Err(SharingCryptoError::DeviceKeyMismatch);
    }
    check_body(manifest, mutation, body)?;
    let Some(body) = body else {
        return Ok(None);
    };
    let envelope = body
        .envelopes
        .iter()
        .find(|e| e.recipient_device_id == recipient_device_id)
        .ok_or(SharingCryptoError::NotRecipient)?;
    let ephemeral_public = envelope
        .ephemeral_public_key
        .to_array::<32>()
        .ok_or(SharingCryptoError::Structure("ephemeral public key"))?;
    let aad = sharing_envelope_aad(&mutation.context, recipient, &ephemeral_public)
        .map_err(|_| SharingCryptoError::Structure("recipient envelope context"))?;
    let shared = keys.diffie_hellman(&ephemeral_public)?;
    let wrapping_key = envelope_key(&shared, &ephemeral_public, &public.encryption, &aad);
    let nonce = envelope
        .nonce
        .to_array::<NONCE_BYTES>()
        .ok_or(SharingCryptoError::Structure("envelope nonce"))?;
    let dek = Dek::from_secret(aead::open_key32(
        wrapping_key.expose_secret(),
        &nonce,
        &aad,
        envelope.ciphertext.as_slice(),
    )?);
    let nonce = body
        .nonce
        .to_array::<NONCE_BYTES>()
        .ok_or(SharingCryptoError::Structure("object nonce"))?;
    let aad = sharing_object_aad(&mutation.context)
        .map_err(|_| SharingCryptoError::Structure("object context"))?;
    let plain = aead::open(dek.expose(), &nonce, &aad, body.ciphertext.as_slice())?;
    Ok(Some(SharedPlaintext(unpad(plain)?)))
}

fn manifest_context(manifest: &AccessManifest, context: &SharingContext) -> Result<()> {
    if manifest.server_instance_id != context.server_instance_id
        || manifest.share_id != context.share_id
        || manifest.item_id != context.item_id
        || manifest.access_epoch != context.access_epoch
        || manifest.kind != context.kind
    {
        return Err(SharingCryptoError::ContextMismatch);
    }
    Ok(())
}

fn check_owner(manifest: &AccessManifest, owner: &SharingOwnerAnchor) -> Result<()> {
    if manifest.owner_user_id != owner.user_id || manifest.owner_device_id != owner.device_id {
        return Err(SharingCryptoError::OwnerMismatch);
    }
    let member = manifest
        .members
        .iter()
        .find(|m| m.device_id == owner.device_id)
        .ok_or(SharingCryptoError::OwnerMismatch)?;
    if member.user_id != owner.user_id
        || member.role != SharingRole::Editor
        || member_public_keys(member)? != owner.public_keys
    {
        return Err(SharingCryptoError::OwnerMismatch);
    }
    Ok(())
}

fn editor(manifest: &VerifiedSharingManifest, id: DeviceId) -> Result<&SharingMember> {
    let member = manifest
        .member(id)
        .ok_or(SharingCryptoError::NotRecipient)?;
    if member.role != SharingRole::Editor {
        return Err(SharingCryptoError::ReadOnly);
    }
    Ok(member)
}

fn check_mutation_context(
    manifest: &VerifiedSharingManifest,
    mutation: &SharingMutation,
) -> Result<()> {
    manifest_context(manifest.manifest(), &mutation.context)?;
    if mutation.manifest_revision != manifest.manifest().revision
        || mutation.manifest_hash.as_slice() != manifest.hash
    {
        return Err(SharingCryptoError::ContextMismatch);
    }
    sharing_mutation_message(mutation)
        .map_err(|_| SharingCryptoError::Structure("mutation header"))?;
    Ok(())
}

fn check_body(
    manifest: &VerifiedSharingManifest,
    mutation: &SharingMutation,
    body: Option<&SharedEncryptedBody>,
) -> Result<()> {
    match (mutation.operation, body) {
        (SharingOperation::Put, Some(body)) => {
            validate_body(manifest, body)?;
            if mutation.body_hash.as_slice() != shared_body_hash(body)? {
                return Err(SharingCryptoError::Structure("body hash mismatch"));
            }
        }
        (SharingOperation::Delete, None) if mutation.body_hash.as_slice() == [0; HASH_BYTES] => {}
        _ => return Err(SharingCryptoError::Structure("operation and body differ")),
    }
    Ok(())
}

fn validate_body(manifest: &VerifiedSharingManifest, body: &SharedEncryptedBody) -> Result<()> {
    sharing_body_message(body).map_err(|_| SharingCryptoError::Structure("encrypted body"))?;
    let members: HashSet<DeviceId> = manifest
        .manifest()
        .members
        .iter()
        .map(|m| m.device_id)
        .collect();
    let recipients: HashSet<DeviceId> = body
        .envelopes
        .iter()
        .map(|e| e.recipient_device_id)
        .collect();
    if members != recipients || recipients.len() != body.envelopes.len() {
        return Err(SharingCryptoError::Structure(
            "envelopes differ from current approved devices",
        ));
    }
    if body.format != FORMAT
        || body.nonce.len() != NONCE_BYTES
        || body.ciphertext.len() < 256 + 16
        || body.ciphertext.len() > MAX_PADDED_PLAINTEXT + 16
        || body.envelopes.iter().any(|e| {
            e.ephemeral_public_key.len() != 32
                || e.nonce.len() != NONCE_BYTES
                || e.ciphertext.len() != ENVELOPE_BYTES
        })
    {
        return Err(SharingCryptoError::Structure(
            "shared body lengths or format",
        ));
    }
    Ok(())
}

fn member_public_keys(member: &SharingMember) -> Result<DevicePublicKeys> {
    Ok(DevicePublicKeys::from_slices(
        member.encryption_public_key.as_slice(),
        member.signing_public_key.as_slice(),
    )?)
}

fn envelope_key(
    shared: &crate::keys::Secret32,
    ephemeral_public: &[u8; 32],
    recipient_public: &[u8; 32],
    aad: &[u8],
) -> crate::keys::Secret32 {
    let mut salt = [0u8; 64];
    salt[..32].copy_from_slice(ephemeral_public);
    salt[32..].copy_from_slice(recipient_public);
    hkdf_sha256_32(shared.expose_secret(), &salt, &[labels::ENVELOPE_KEY, aad])
}

fn verify_signature(public: &[u8; 32], message: &[u8], signature: &Bytes) -> Result<()> {
    if signature.len() != SIGNATURE_BYTES {
        return Err(CryptoError::InvalidSignature.into());
    }
    let verifying = VerifyingKey::from_bytes(public).map_err(|_| CryptoError::InvalidKey {
        what: "Ed25519 public key",
    })?;
    if verifying.is_weak() {
        return Err(CryptoError::InvalidKey {
            what: "Ed25519 public key",
        }
        .into());
    }
    let signature =
        Signature::from_slice(signature.as_slice()).map_err(|_| CryptoError::InvalidSignature)?;
    verifying
        .verify_strict(message, &signature)
        .map_err(|_| CryptoError::InvalidSignature)?;
    Ok(())
}

fn sha256(bytes: &[u8]) -> [u8; HASH_BYTES] {
    Sha256::digest(bytes).into()
}

fn pad(plain: &[u8]) -> Result<Zeroizing<Vec<u8>>> {
    let length = plain
        .len()
        .checked_add(4)
        .ok_or(SharingCryptoError::Structure("plaintext length overflow"))?;
    if length > MAX_PADDED_PLAINTEXT {
        return Err(CryptoError::ObjectTooLarge {
            size: length,
            max: MAX_PADDED_PLAINTEXT,
        }
        .into());
    }
    let total = padded_len(length);
    if total > MAX_PADDED_PLAINTEXT {
        return Err(CryptoError::ObjectTooLarge {
            size: total,
            max: MAX_PADDED_PLAINTEXT,
        }
        .into());
    }
    let prefix = u32::try_from(plain.len())
        .map_err(|_| SharingCryptoError::Structure("plaintext length overflow"))?
        .to_be_bytes();
    let mut out = Zeroizing::new(Vec::with_capacity(total));
    out.extend_from_slice(&prefix);
    out.extend_from_slice(plain);
    out.resize(total, 0);
    Ok(out)
}

fn unpad(mut plain: Zeroizing<Vec<u8>>) -> Result<Zeroizing<Vec<u8>>> {
    if plain.len() < 4 || plain.len() > MAX_PADDED_PLAINTEXT {
        return Err(CryptoError::MalformedPlaintext("shared padding length").into());
    }
    let length = u32::from_be_bytes(plain[..4].try_into().expect("checked length")) as usize;
    let end = length
        .checked_add(4)
        .ok_or(CryptoError::MalformedPlaintext("shared length overflow"))?;
    if end > plain.len() || padded_len(end) != plain.len() || plain[end..].iter().any(|b| *b != 0) {
        return Err(CryptoError::MalformedPlaintext("shared padding or length prefix").into());
    }
    // Keep the original zeroizing allocation and erase the truncated tail,
    // including any overlapping source bytes, before changing its length.
    plain.copy_within(4..end, 0);
    plain[length..].zeroize();
    plain.truncate(length);
    Ok(plain)
}

#[cfg(test)]
mod tests {
    use super::*;
    use cc_protocol::sharing::SharedItemKind;
    use cc_protocol::{MutationId, ObjectId, ShareId};
    use uuid::Uuid;

    fn setup() -> (
        DeviceSecretKeys,
        DeviceId,
        SharingContext,
        VerifiedSharingManifest,
    ) {
        let keys = DeviceSecretKeys::generate().unwrap();
        let device_id = DeviceId::new();
        let user_id = UserId::new();
        let context = SharingContext {
            server_instance_id: Uuid::new_v4(),
            share_id: ShareId::new(),
            item_id: ObjectId::new(),
            revision: 1,
            access_epoch: 1,
            kind: SharedItemKind::Snippet,
        };
        let anchor = SharingOwnerAnchor {
            user_id,
            device_id,
            public_keys: keys.public_keys(),
        };
        let access = AccessManifest {
            format: FORMAT,
            server_instance_id: context.server_instance_id,
            share_id: context.share_id,
            item_id: context.item_id,
            owner_user_id: user_id,
            owner_device_id: device_id,
            revision: 1,
            access_epoch: 1,
            previous_manifest_hash: Bytes::from([0; 32]),
            kind: context.kind,
            members: vec![SharingMember {
                user_id,
                device_id,
                encryption_public_key: keys.public_keys().encryption_bytes(),
                signing_public_key: keys.public_keys().signing_bytes(),
                role: SharingRole::Editor,
            }],
        };
        let signed = sign_shared_manifest(&keys, access, &context, &anchor, None).unwrap();
        let manifest = verify_shared_manifest(&signed, &context, &anchor, None).unwrap();
        (keys, device_id, context, manifest)
    }

    fn unwrap_dek(
        keys: &DeviceSecretKeys,
        member: &SharingMember,
        context: &SharingContext,
        body: &SharedEncryptedBody,
    ) -> Dek {
        let envelope = body
            .envelopes
            .iter()
            .find(|e| e.recipient_device_id == member.device_id)
            .unwrap();
        let ephemeral = envelope.ephemeral_public_key.to_array::<32>().unwrap();
        let aad = sharing_envelope_aad(context, member, &ephemeral).unwrap();
        let shared = keys.diffie_hellman(&ephemeral).unwrap();
        let key = envelope_key(&shared, &ephemeral, &keys.public_keys().encryption, &aad);
        Dek::from_secret(
            aead::open_key32(
                key.expose_secret(),
                &envelope.nonce.to_array().unwrap(),
                &aad,
                envelope.ciphertext.as_slice(),
            )
            .unwrap(),
        )
    }

    fn authenticate(
        keys: &DeviceSecretKeys,
        device_id: DeviceId,
        context: &SharingContext,
        manifest: &VerifiedSharingManifest,
        body: &SharedEncryptedBody,
    ) -> VerifiedSharingMutation {
        let mutation = SharingMutation {
            context: context.clone(),
            mutation_id: MutationId::new(),
            base_revision: context.revision - 1,
            manifest_revision: manifest.manifest().revision,
            manifest_hash: Bytes::from(manifest.hash()),
            writer_device_id: device_id,
            previous_revision_hash: Bytes::from([0; 32]),
            operation: SharingOperation::Put,
            body_hash: Bytes::from(shared_body_hash(body).unwrap()),
        };
        let signed = sign_shared_mutation(manifest, keys, mutation, Some(body), None).unwrap();
        verify_shared_mutation(manifest, &signed, None).unwrap()
    }

    #[test]
    fn every_encryption_gets_a_distinct_dek_even_for_identical_plaintext() {
        let (keys, device, context, manifest) = setup();
        let first = seal_shared_revision(&manifest, &context, b"same data").unwrap();
        let second = seal_shared_revision(&manifest, &context, b"same data").unwrap();
        let first_key = unwrap_dek(&keys, manifest.member(device).unwrap(), &context, &first);
        let second_key = unwrap_dek(&keys, manifest.member(device).unwrap(), &context, &second);
        assert!(
            first_key.expose() != second_key.expose(),
            "fresh revision DEK expected"
        );
        let aad = sharing_object_aad(&context).unwrap();
        assert!(aead::open(
            first_key.expose(),
            &second.nonce.to_array().unwrap(),
            &aad,
            second.ciphertext.as_slice()
        )
        .is_err());
        assert_eq!(format!("{first_key:?}"), "Dek(<redacted>)");
    }

    #[test]
    fn rotation_reencrypts_with_new_dek_instead_of_rewrapping_an_old_dek() {
        let (keys, device, mut context, manifest) = setup();
        let first = seal_shared_revision(&manifest, &context, b"unchanged selected data").unwrap();
        let first_key = unwrap_dek(&keys, manifest.member(device).unwrap(), &context, &first);
        let mut access = manifest.manifest().clone();
        access.revision = 2;
        access.access_epoch = 2;
        access.previous_manifest_hash = Bytes::from(manifest.hash());
        context.revision = 2;
        context.access_epoch = 2;
        let anchor = SharingOwnerAnchor {
            user_id: access.owner_user_id,
            device_id: device,
            public_keys: keys.public_keys(),
        };
        let signed = sign_shared_manifest(
            &keys,
            access,
            &context,
            &anchor,
            Some(&manifest.checkpoint()),
        )
        .unwrap();
        let next = verify_shared_manifest(&signed, &context, &anchor, Some(&manifest.checkpoint()))
            .unwrap();
        let second = seal_shared_revision(&next, &context, b"unchanged selected data").unwrap();
        let aad = sharing_object_aad(&context).unwrap();
        assert!(aead::open(
            first_key.expose(),
            &second.nonce.to_array().unwrap(),
            &aad,
            second.ciphertext.as_slice()
        )
        .is_err());
        let verified = authenticate(&keys, device, &context, &next, &second);
        assert!(
            open_shared_revision(&next, &verified, Some(&second), device, &keys)
                .unwrap()
                .unwrap()
                .as_slice()
                == b"unchanged selected data"
        );
    }

    #[test]
    fn aead_binds_every_object_context_field_independently_of_signatures() {
        let (keys, device, context, manifest) = setup();
        let body = seal_shared_revision(&manifest, &context, b"data").unwrap();
        let dek = unwrap_dek(&keys, manifest.member(device).unwrap(), &context, &body);
        let mut changes = Vec::new();
        let mut changed = context.clone();
        changed.server_instance_id = Uuid::new_v4();
        changes.push(changed);
        let mut changed = context.clone();
        changed.share_id = ShareId::new();
        changes.push(changed);
        let mut changed = context.clone();
        changed.item_id = ObjectId::new();
        changes.push(changed);
        let mut changed = context.clone();
        changed.revision += 1;
        changes.push(changed);
        let mut changed = context.clone();
        changed.access_epoch += 1;
        changes.push(changed);
        let mut changed = context.clone();
        changed.kind = SharedItemKind::Host;
        changes.push(changed);
        for changed in changes {
            let aad = sharing_object_aad(&changed).unwrap();
            assert!(aead::open(
                dek.expose(),
                &body.nonce.to_array().unwrap(),
                &aad,
                body.ciphertext.as_slice()
            )
            .is_err());
        }
    }

    #[test]
    fn even_an_authenticated_author_cannot_supply_malformed_padding() {
        let (keys, device, context, manifest) = setup();
        let original = seal_shared_revision(&manifest, &context, b"data").unwrap();
        let dek = unwrap_dek(&keys, manifest.member(device).unwrap(), &context, &original);
        let aad = sharing_object_aad(&context).unwrap();
        for mode in 0..3 {
            let mut plain = Zeroizing::new(vec![0; 256]);
            match mode {
                0 => plain[..4].copy_from_slice(&300u32.to_be_bytes()),
                1 => {
                    plain[..4].copy_from_slice(&1u32.to_be_bytes());
                    plain[4] = b'x';
                    plain[255] = 1;
                }
                _ => {
                    plain[..4].copy_from_slice(&1u32.to_be_bytes());
                    plain.resize(512, 0);
                }
            }
            let mut body = original.clone();
            body.ciphertext = Bytes::from(
                aead::seal(dek.expose(), &body.nonce.to_array().unwrap(), &aad, &plain).unwrap(),
            );
            let verified = authenticate(&keys, device, &context, &manifest, &body);
            assert!(matches!(
                open_shared_revision(&manifest, &verified, Some(&body), device, &keys),
                Err(SharingCryptoError::Crypto(CryptoError::MalformedPlaintext(
                    _
                )))
            ));
        }
    }

    #[test]
    fn padding_roundtrips_at_bucket_boundaries() {
        for length in [0, 1, 251, 252, 253, 16 * 1024 - 4, 16 * 1024 + 1] {
            let source = Zeroizing::new(vec![b'x'; length]);
            let encoded = pad(&source).unwrap();
            assert!(encoded.len() == padded_len(length + 4));
            let decoded = unpad(encoded).unwrap();
            assert!(decoded.as_slice() == source.as_slice());
        }
    }
}
