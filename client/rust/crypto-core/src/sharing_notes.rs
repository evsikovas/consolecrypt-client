//! Notes travelling with a shared object (`share_notes_v1`).
//!
//! The owner device creates a random notes key per access epoch, wraps it to
//! every member device of the verified manifest and signs the set; a member
//! accepts the key only with the owner's signature. Notes are padded, sealed
//! with a key derived from the epoch's notes key and signed by the author
//! device over the hash of the padded payload (whose random salt keeps short
//! notes from being guessed by the server).

use crate::aead;
use crate::device::{random_x25519_secret, x25519, x25519_public};
use crate::kdf::hkdf_sha256_32;
use crate::keys::Secret32;
use crate::rng::random_array;
use crate::sharing::{
    member_public_keys, pad, unpad, verify_signature, SharedPlaintext, SharingCryptoError,
    VerifiedSharingManifest,
};
use crate::{CryptoError, DeviceSecretKeys, ExposeSecret};
use cc_protocol::sharing::SharedKeyEnvelope;
use cc_protocol::sharing_notes::{
    labels, note_aad, note_key_info, note_message, notes_envelope_aad, notes_key_set_message,
    SharedNote, SharedNotesKeySet, FORMAT,
};
use cc_protocol::{Bytes, DeviceId, ObjectId, ShareId, UserId};
use sha2::{Digest, Sha256};
use uuid::Uuid;
use zeroize::Zeroizing;

type Result<T> = std::result::Result<T, SharingCryptoError>;

const SALT_BYTES: usize = 32;

/// The notes key of one access epoch.
pub struct NotesKey(Zeroizing<[u8; 32]>);

impl std::fmt::Debug for NotesKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("NotesKey([REDACTED])")
    }
}

impl NotesKey {
    pub fn generate() -> Result<Self> {
        Ok(Self(Zeroizing::new(random_array::<32>()?)))
    }
}

fn wrapping_key(
    shared: &Secret32,
    ephemeral_public: &[u8; 32],
    recipient_public: &[u8; 32],
    aad: &[u8],
) -> Secret32 {
    let mut salt = [0u8; 64];
    salt[..32].copy_from_slice(ephemeral_public);
    salt[32..].copy_from_slice(recipient_public);
    hkdf_sha256_32(shared.expose_secret(), &salt, &[labels::ENVELOPE_KEY, aad])
}

fn content_key(key: &NotesKey, note_id: ObjectId, access_epoch: u64) -> Secret32 {
    hkdf_sha256_32(
        key.0.as_ref(),
        note_id.as_bytes(),
        &[&note_key_info(access_epoch)],
    )
}

/// Wraps `key` to every member device of the verified current manifest and
/// signs the set with the owner device. Only the owner device may call this.
pub fn seal_notes_key_set(
    owner: &DeviceSecretKeys,
    manifest: &VerifiedSharingManifest,
    key: &NotesKey,
) -> Result<SharedNotesKeySet> {
    let m = manifest.manifest();
    let owner_member = manifest
        .member(m.owner_device_id)
        .ok_or(SharingCryptoError::OwnerMismatch)?;
    if member_public_keys(owner_member)? != owner.public_keys() {
        return Err(SharingCryptoError::OwnerMismatch);
    }
    let mut envelopes = Vec::with_capacity(m.members.len());
    for member in &m.members {
        let public = member_public_keys(member)?;
        let ephemeral = random_x25519_secret()?;
        let ephemeral_public = x25519_public(&ephemeral);
        let shared = x25519(&ephemeral, &public.encryption)?;
        let aad = notes_envelope_aad(
            m.server_instance_id,
            m.share_id,
            m.access_epoch,
            member,
            &ephemeral_public,
        )
        .map_err(|_| SharingCryptoError::Structure("notes envelope context"))?;
        let wrap = wrapping_key(&shared, &ephemeral_public, &public.encryption, &aad);
        let nonce = random_array::<24>()?;
        let ciphertext = aead::seal(wrap.expose_secret(), &nonce, &aad, key.0.as_ref())?;
        envelopes.push(SharedKeyEnvelope {
            recipient_device_id: member.device_id,
            ephemeral_public_key: Bytes::from(ephemeral_public),
            nonce: Bytes::from(nonce),
            ciphertext: Bytes::from(ciphertext),
        });
    }
    let mut set = SharedNotesKeySet {
        format: FORMAT,
        server_instance_id: m.server_instance_id,
        share_id: m.share_id,
        access_epoch: m.access_epoch,
        manifest_hash: Bytes::from(manifest.hash()),
        owner_device_id: m.owner_device_id,
        envelopes,
        signature: Bytes::from(vec![0u8; 64]),
    };
    let message =
        notes_key_set_message(&set).map_err(|_| SharingCryptoError::Structure("notes key set"))?;
    set.signature = Bytes::from(owner.sign_message(&message));
    Ok(set)
}

/// Opens this device's notes key from a key set of any epoch of the share.
/// The set must carry the signature of the owner device of the verified
/// manifest; the current epoch's set must also name the current manifest.
pub fn open_notes_key_set(
    set: &SharedNotesKeySet,
    manifest: &VerifiedSharingManifest,
    device_id: DeviceId,
    keys: &DeviceSecretKeys,
) -> Result<NotesKey> {
    let m = manifest.manifest();
    if set.share_id != m.share_id
        || set.server_instance_id != m.server_instance_id
        || set.owner_device_id != m.owner_device_id
        || set.access_epoch > m.access_epoch
        || (set.access_epoch == m.access_epoch
            && set.manifest_hash.as_slice() != manifest.hash().as_slice())
    {
        return Err(SharingCryptoError::ContextMismatch);
    }
    let owner = manifest
        .member(m.owner_device_id)
        .ok_or(SharingCryptoError::OwnerMismatch)?;
    let message =
        notes_key_set_message(set).map_err(|_| SharingCryptoError::Structure("notes key set"))?;
    verify_signature(
        &member_public_keys(owner)?.signing,
        &message,
        &set.signature,
    )?;
    // This device's keys are pinned across epochs; the current entry binds them.
    let me = manifest
        .member(device_id)
        .ok_or(SharingCryptoError::NotRecipient)?;
    let public = member_public_keys(me)?;
    if keys.public_keys() != public {
        return Err(SharingCryptoError::DeviceKeyMismatch);
    }
    let envelope = set
        .envelopes
        .iter()
        .find(|e| e.recipient_device_id == device_id)
        .ok_or(SharingCryptoError::NotRecipient)?;
    let ephemeral_public = envelope
        .ephemeral_public_key
        .to_array::<32>()
        .ok_or(SharingCryptoError::Structure("ephemeral public key"))?;
    let aad = notes_envelope_aad(
        set.server_instance_id,
        set.share_id,
        set.access_epoch,
        me,
        &ephemeral_public,
    )
    .map_err(|_| SharingCryptoError::Structure("notes envelope context"))?;
    let shared = keys.diffie_hellman(&ephemeral_public)?;
    let wrap = wrapping_key(&shared, &ephemeral_public, &public.encryption, &aad);
    let nonce = envelope
        .nonce
        .to_array::<24>()
        .ok_or(SharingCryptoError::Structure("envelope nonce"))?;
    let plain = aead::open(
        wrap.expose_secret(),
        &nonce,
        &aad,
        envelope.ciphertext.as_slice(),
    )?;
    let bytes: [u8; 32] = plain
        .as_slice()
        .try_into()
        .map_err(|_| CryptoError::MalformedPlaintext("notes key length"))?;
    Ok(NotesKey(Zeroizing::new(bytes)))
}

/// Who writes a note and where it belongs.
#[derive(Debug, Clone, Copy)]
pub struct NoteAuthor {
    pub user_id: UserId,
    pub device_id: DeviceId,
}

/// Prepends a random salt to `payload`, seals it and signs it as `author`.
#[allow(clippy::too_many_arguments)]
pub fn seal_shared_note(
    keys: &DeviceSecretKeys,
    author: NoteAuthor,
    server_instance_id: Uuid,
    share_id: ShareId,
    note_id: ObjectId,
    revision: u64,
    access_epoch: u64,
    key: &NotesKey,
    payload: &[u8],
) -> Result<SharedNote> {
    let mut salted = Zeroizing::new(Vec::with_capacity(SALT_BYTES + payload.len()));
    salted.extend_from_slice(&random_array::<SALT_BYTES>()?);
    salted.extend_from_slice(payload);
    let padded = pad(&salted)?;
    let payload_hash: [u8; 32] = Sha256::digest(padded.as_slice()).into();
    let nonce = random_array::<24>()?;
    let aad = note_aad(
        server_instance_id,
        share_id,
        note_id,
        access_epoch,
        revision,
        author.device_id,
    );
    let ciphertext = aead::seal(
        content_key(key, note_id, access_epoch).expose_secret(),
        &nonce,
        &aad,
        &padded,
    )?;
    let mut note = SharedNote {
        note_id,
        author_user_id: author.user_id,
        author_device_id: author.device_id,
        revision,
        access_epoch,
        nonce: Bytes::from(nonce),
        ciphertext: Bytes::from(ciphertext),
        payload_hash: Bytes::from(payload_hash),
        signature: Bytes::from(vec![0u8; 64]),
    };
    let message = note_message(server_instance_id, share_id, &note)
        .map_err(|_| SharingCryptoError::Structure("shared note"))?;
    note.signature = Bytes::from(keys.sign_message(&message));
    Ok(note)
}

/// Decrypts and verifies a note, returning its padded salted payload.
fn open_padded(
    note: &SharedNote,
    server_instance_id: Uuid,
    share_id: ShareId,
    key: &NotesKey,
    author_signing_key: &[u8; 32],
) -> Result<Zeroizing<Vec<u8>>> {
    let message = note_message(server_instance_id, share_id, note)
        .map_err(|_| SharingCryptoError::Structure("shared note"))?;
    verify_signature(author_signing_key, &message, &note.signature)?;
    let nonce = note
        .nonce
        .to_array::<24>()
        .ok_or(SharingCryptoError::Structure("note nonce"))?;
    let aad = note_aad(
        server_instance_id,
        share_id,
        note.note_id,
        note.access_epoch,
        note.revision,
        note.author_device_id,
    );
    let padded = aead::open(
        content_key(key, note.note_id, note.access_epoch).expose_secret(),
        &nonce,
        &aad,
        note.ciphertext.as_slice(),
    )?;
    let hash: [u8; 32] = Sha256::digest(padded.as_slice()).into();
    if hash.as_slice() != note.payload_hash.as_slice() {
        return Err(SharingCryptoError::Structure("note payload hash"));
    }
    Ok(padded)
}

/// Decrypts a note sealed under `key` (the key of `note.access_epoch`),
/// checks the payload hash and the author's signature and returns the
/// payload without its salt.
pub fn open_shared_note(
    note: &SharedNote,
    server_instance_id: Uuid,
    share_id: ShareId,
    key: &NotesKey,
    author_signing_key: &[u8; 32],
) -> Result<SharedPlaintext> {
    let padded = open_padded(note, server_instance_id, share_id, key, author_signing_key)?;
    let mut salted = unpad(padded)?;
    if salted.len() < SALT_BYTES {
        return Err(CryptoError::MalformedPlaintext("note salt").into());
    }
    salted.drain(..SALT_BYTES);
    Ok(SharedPlaintext::new(salted))
}

/// Re-encrypts a verified note into `access_epoch` under `new_key` for the
/// owner's rekey. The padded payload is reused byte for byte, so authorship,
/// revision, payload hash and signature stay valid.
pub fn rekey_shared_note(
    note: &SharedNote,
    server_instance_id: Uuid,
    share_id: ShareId,
    old_key: &NotesKey,
    author_signing_key: &[u8; 32],
    access_epoch: u64,
    new_key: &NotesKey,
) -> Result<SharedNote> {
    let padded = open_padded(
        note,
        server_instance_id,
        share_id,
        old_key,
        author_signing_key,
    )?;
    let nonce = random_array::<24>()?;
    let aad = note_aad(
        server_instance_id,
        share_id,
        note.note_id,
        access_epoch,
        note.revision,
        note.author_device_id,
    );
    let ciphertext = aead::seal(
        content_key(new_key, note.note_id, access_epoch).expose_secret(),
        &nonce,
        &aad,
        &padded,
    )?;
    Ok(SharedNote {
        access_epoch,
        nonce: Bytes::from(nonce),
        ciphertext: Bytes::from(ciphertext),
        ..note.clone()
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sharing::{sign_shared_manifest, verify_shared_manifest, SharingOwnerAnchor};
    use cc_protocol::sharing::{
        AccessManifest, SharedItemKind, SharingContext, SharingMember, SharingRole,
        FORMAT as SHARING_FORMAT,
    };

    struct Device {
        keys: DeviceSecretKeys,
        user: UserId,
        id: DeviceId,
    }
    impl Device {
        fn new() -> Self {
            Self {
                keys: DeviceSecretKeys::generate().unwrap(),
                user: UserId::new(),
                id: DeviceId::new(),
            }
        }
        fn member(&self, role: SharingRole) -> SharingMember {
            SharingMember {
                user_id: self.user,
                device_id: self.id,
                encryption_public_key: self.keys.public_keys().encryption_bytes(),
                signing_public_key: self.keys.public_keys().signing_bytes(),
                role,
            }
        }
        fn author(&self) -> NoteAuthor {
            NoteAuthor {
                user_id: self.user,
                device_id: self.id,
            }
        }
        fn signing(&self) -> [u8; 32] {
            self.keys.public_keys().signing
        }
    }

    /// A verified manifest of `epoch` (later epochs are signed but not chained).
    fn manifest(
        owner: &Device,
        members: &[&Device],
        epoch: u64,
        share: ShareId,
        instance: Uuid,
    ) -> VerifiedSharingManifest {
        let context = SharingContext {
            server_instance_id: instance,
            share_id: share,
            item_id: ObjectId::from_uuid(Uuid::from_u128(9)),
            revision: 1,
            access_epoch: epoch,
            kind: SharedItemKind::Host,
        };
        let anchor = SharingOwnerAnchor {
            user_id: owner.user,
            device_id: owner.id,
            public_keys: owner.keys.public_keys(),
        };
        let mut list = vec![owner.member(SharingRole::Editor)];
        list.extend(members.iter().map(|d| d.member(SharingRole::Reader)));
        let access = AccessManifest {
            format: SHARING_FORMAT,
            server_instance_id: instance,
            share_id: share,
            item_id: context.item_id,
            owner_user_id: owner.user,
            owner_device_id: owner.id,
            revision: epoch,
            access_epoch: epoch,
            previous_manifest_hash: Bytes::from([if epoch == 1 { 0 } else { 1 }; 32]),
            kind: SharedItemKind::Host,
            members: list,
        };
        let signed = sign_shared_manifest(&owner.keys, access, &context, &anchor, None).unwrap();
        verify_shared_manifest(&signed, &context, &anchor, None).unwrap()
    }

    #[test]
    fn members_open_the_owner_key_and_read_each_others_notes() {
        let (owner, reader, stranger) = (Device::new(), Device::new(), Device::new());
        let (share, instance) = (ShareId::new(), Uuid::new_v4());
        let m1 = manifest(&owner, &[&reader], 1, share, instance);
        let key = NotesKey::generate().unwrap();
        let set = seal_notes_key_set(&owner.keys, &m1, &key).unwrap();
        // Only the owner device can issue a key set.
        assert!(seal_notes_key_set(&reader.keys, &m1, &key).is_err());
        let reader_key = open_notes_key_set(&set, &m1, reader.id, &reader.keys).unwrap();
        assert!(open_notes_key_set(&set, &m1, stranger.id, &stranger.keys).is_err());
        // A substituted signature (e.g. the server's own key set) is refused.
        let mut forged = set.clone();
        forged.signature = Bytes::from(vec![7u8; 64]);
        assert!(open_notes_key_set(&forged, &m1, reader.id, &reader.keys).is_err());

        let id = ObjectId::new();
        let note = seal_shared_note(
            &reader.keys,
            reader.author(),
            instance,
            share,
            id,
            1,
            1,
            &reader_key,
            b"{\"title\":\"db\"}",
        )
        .unwrap();
        let plain = open_shared_note(&note, instance, share, &key, &reader.signing()).unwrap();
        assert_eq!(plain.as_slice(), b"{\"title\":\"db\"}");
        // The salt keeps equal notes from producing equal hashes.
        let again = seal_shared_note(
            &reader.keys,
            reader.author(),
            instance,
            share,
            id,
            1,
            1,
            &reader_key,
            b"{\"title\":\"db\"}",
        )
        .unwrap();
        assert_ne!(again.payload_hash, note.payload_hash);
        // Wrong signer, tampered ciphertext or another share fail.
        assert!(open_shared_note(&note, instance, share, &key, &owner.signing()).is_err());
        let mut tampered = note.clone();
        let mut bytes = tampered.ciphertext.as_slice().to_vec();
        bytes[0] ^= 1;
        tampered.ciphertext = Bytes::from(bytes);
        assert!(open_shared_note(&tampered, instance, share, &key, &reader.signing()).is_err());
        assert!(
            open_shared_note(&note, instance, ShareId::new(), &key, &reader.signing()).is_err()
        );
    }

    #[test]
    fn rekey_moves_a_note_into_the_next_epoch_with_the_author_signature() {
        let (owner, reader) = (Device::new(), Device::new());
        let (share, instance) = (ShareId::new(), Uuid::new_v4());
        let old_key = NotesKey::generate().unwrap();
        let note = seal_shared_note(
            &reader.keys,
            reader.author(),
            instance,
            share,
            ObjectId::new(),
            3,
            1,
            &old_key,
            b"runbook",
        )
        .unwrap();
        let m2 = manifest(&owner, &[&reader], 2, share, instance);
        let new_key = NotesKey::generate().unwrap();
        let set2 = seal_notes_key_set(&owner.keys, &m2, &new_key).unwrap();
        let moved = rekey_shared_note(
            &note,
            instance,
            share,
            &old_key,
            &reader.signing(),
            2,
            &new_key,
        )
        .unwrap();
        assert_eq!(
            (
                moved.access_epoch,
                moved.revision,
                &moved.signature,
                &moved.payload_hash
            ),
            (2, 3, &note.signature, &note.payload_hash)
        );
        let reader_key = open_notes_key_set(&set2, &m2, reader.id, &reader.keys).unwrap();
        let plain =
            open_shared_note(&moved, instance, share, &reader_key, &reader.signing()).unwrap();
        assert_eq!(plain.as_slice(), b"runbook");
        // The old key no longer opens it; the old ciphertext cannot be relabelled.
        assert!(open_shared_note(&moved, instance, share, &old_key, &reader.signing()).is_err());
        let mut relabelled = note.clone();
        relabelled.access_epoch = 2;
        assert!(
            open_shared_note(&relabelled, instance, share, &old_key, &reader.signing()).is_err()
        );
    }
}
