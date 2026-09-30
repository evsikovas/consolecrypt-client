use cc_crypto_core::sharing::{
    open_shared_revision, seal_shared_revision, shared_body_hash, sign_shared_manifest,
    sign_shared_mutation, verify_shared_manifest, verify_shared_mutation, SharingCryptoError,
    SharingOwnerAnchor, SharingRevisionCheckpoint, VerifiedSharingManifest,
    VerifiedSharingMutation,
};
use cc_crypto_core::{CryptoError, DeviceSecretKeys};
use cc_protocol::sharing::{
    sharing_manifest_message, sharing_mutation_message, AccessManifest, SharedEncryptedBody,
    SharedItemKind, SharingContext, SharingMember, SharingMutation, SharingOperation, SharingRole,
    SignedAccessManifest, SignedSharingMutation,
};
use cc_protocol::{Bytes, DeviceId, MutationId, ObjectId, ShareId, UserId};
use ed25519_dalek::{Signer, SigningKey};
use uuid::Uuid;
use zeroize::Zeroizing;

const PAYLOAD: &[u8] = b"a selected host projection without credentials";

struct Fixture {
    owner: DeviceSecretKeys,
    owner_id: DeviceId,
    owner_user: UserId,
    editor: DeviceSecretKeys,
    editor_id: DeviceId,
    reader: DeviceSecretKeys,
    reader_id: DeviceId,
    context: SharingContext,
    members: Vec<SharingMember>,
}

impl Fixture {
    fn new() -> Self {
        let owner = DeviceSecretKeys::generate().unwrap();
        let owner_id = DeviceId::new();
        let owner_user = UserId::new();
        let editor = DeviceSecretKeys::generate().unwrap();
        let editor_id = DeviceId::new();
        let reader = DeviceSecretKeys::generate().unwrap();
        let reader_id = DeviceId::new();
        let member = |keys: &DeviceSecretKeys, user_id, device_id, role| SharingMember {
            user_id,
            device_id,
            encryption_public_key: keys.public_keys().encryption_bytes(),
            signing_public_key: keys.public_keys().signing_bytes(),
            role,
        };
        let members = vec![
            member(&owner, owner_user, owner_id, SharingRole::Editor),
            member(&editor, UserId::new(), editor_id, SharingRole::Editor),
            member(&reader, UserId::new(), reader_id, SharingRole::Reader),
        ];
        Self {
            owner,
            owner_id,
            owner_user,
            editor,
            editor_id,
            reader,
            reader_id,
            members,
            context: SharingContext {
                server_instance_id: Uuid::new_v4(),
                share_id: ShareId::new(),
                item_id: ObjectId::new(),
                revision: 1,
                access_epoch: 1,
                kind: SharedItemKind::Host,
            },
        }
    }

    fn anchor(&self) -> SharingOwnerAnchor {
        SharingOwnerAnchor {
            user_id: self.owner_user,
            device_id: self.owner_id,
            public_keys: self.owner.public_keys(),
        }
    }

    fn access(&self) -> AccessManifest {
        AccessManifest {
            format: 1,
            server_instance_id: self.context.server_instance_id,
            share_id: self.context.share_id,
            item_id: self.context.item_id,
            owner_user_id: self.owner_user,
            owner_device_id: self.owner_id,
            revision: 1,
            access_epoch: 1,
            previous_manifest_hash: Bytes::from([0; 32]),
            kind: self.context.kind,
            members: self.members.clone(),
        }
    }

    fn signed_manifest(&self) -> SignedAccessManifest {
        sign_shared_manifest(
            &self.owner,
            self.access(),
            &self.context,
            &self.anchor(),
            None,
        )
        .unwrap()
    }

    fn manifest(&self) -> VerifiedSharingManifest {
        verify_shared_manifest(&self.signed_manifest(), &self.context, &self.anchor(), None)
            .unwrap()
    }

    fn mutation(
        &self,
        manifest: &VerifiedSharingManifest,
        context: SharingContext,
        body: Option<&SharedEncryptedBody>,
        previous: Option<&SharingRevisionCheckpoint>,
        writer: DeviceId,
    ) -> SharingMutation {
        SharingMutation {
            base_revision: context.revision - 1,
            context,
            mutation_id: MutationId::new(),
            manifest_revision: manifest.manifest().revision,
            manifest_hash: Bytes::from(manifest.hash()),
            writer_device_id: writer,
            previous_revision_hash: Bytes::from(previous.map_or([0; 32], |p| p.hash)),
            operation: if body.is_some() {
                SharingOperation::Put
            } else {
                SharingOperation::Delete
            },
            body_hash: Bytes::from(body.map_or([0; 32], |b| shared_body_hash(b).unwrap())),
        }
    }

    fn first(
        &self,
    ) -> (
        VerifiedSharingManifest,
        SharedEncryptedBody,
        SignedSharingMutation,
        VerifiedSharingMutation,
    ) {
        let manifest = self.manifest();
        let body = seal_shared_revision(&manifest, &self.context, PAYLOAD).unwrap();
        let mutation = self.mutation(
            &manifest,
            self.context.clone(),
            Some(&body),
            None,
            self.owner_id,
        );
        let signed =
            sign_shared_mutation(&manifest, &self.owner, mutation, Some(&body), None).unwrap();
        let verified = verify_shared_mutation(&manifest, &signed, None).unwrap();
        (manifest, body, signed, verified)
    }
}

// Simulate a malicious participant bypassing the safe role-checking signer.
// Seeds are generated at runtime, remain in zeroizing buffers, and are never
// persisted or printed. This proves receiver verification enforces roles too.
fn raw_sign(keys: &DeviceSecretKeys, message: &[u8]) -> Bytes {
    let serialized = keys.to_secret_bytes();
    let mut seed = Zeroizing::new([0u8; 32]);
    seed.copy_from_slice(&serialized[37..69]);
    Bytes::from(SigningKey::from_bytes(&seed).sign(message).to_bytes())
}

#[test]
fn owner_editor_and_reader_open_the_same_authenticated_projection() {
    let f = Fixture::new();
    let (manifest, body, _, verified) = f.first();
    for (id, keys) in [
        (f.owner_id, &f.owner),
        (f.editor_id, &f.editor),
        (f.reader_id, &f.reader),
    ] {
        let plain = open_shared_revision(&manifest, &verified, Some(&body), id, keys)
            .unwrap()
            .unwrap();
        assert!(plain.as_slice() == PAYLOAD);
    }
}

#[test]
fn reader_cannot_publish_even_with_a_valid_signature_from_their_device() {
    let f = Fixture::new();
    let (manifest, body, _, _) = f.first();
    let mutation = f.mutation(&manifest, f.context.clone(), Some(&body), None, f.reader_id);
    assert_eq!(
        sign_shared_mutation(&manifest, &f.reader, mutation.clone(), Some(&body), None)
            .unwrap_err(),
        SharingCryptoError::ReadOnly
    );
    let signed = SignedSharingMutation {
        signature: raw_sign(&f.reader, &sharing_mutation_message(&mutation).unwrap()),
        mutation,
    };
    assert_eq!(
        verify_shared_mutation(&manifest, &signed, None).unwrap_err(),
        SharingCryptoError::ReadOnly
    );
}

#[test]
fn reader_cannot_claim_an_editors_device_identity() {
    let f = Fixture::new();
    let (manifest, body, mut signed, _) = f.first();
    signed.mutation.writer_device_id = f.editor_id;
    signed.signature = raw_sign(
        &f.reader,
        &sharing_mutation_message(&signed.mutation).unwrap(),
    );
    assert!(matches!(
        verify_shared_mutation(&manifest, &signed, None),
        Err(SharingCryptoError::Crypto(CryptoError::InvalidSignature))
    ));
    assert_eq!(
        sign_shared_mutation(&manifest, &f.reader, signed.mutation, Some(&body), None).unwrap_err(),
        SharingCryptoError::DeviceKeyMismatch
    );
}

#[test]
fn editor_can_publish_next_revision_and_other_devices_verify_it() {
    let f = Fixture::new();
    let (manifest, _, _, first) = f.first();
    let checkpoint = first.checkpoint();
    let mut context = f.context.clone();
    context.revision = 2;
    let body = seal_shared_revision(&manifest, &context, b"editor change").unwrap();
    let mutation = f.mutation(
        &manifest,
        context,
        Some(&body),
        Some(&checkpoint),
        f.editor_id,
    );
    let signed = sign_shared_mutation(
        &manifest,
        &f.editor,
        mutation,
        Some(&body),
        Some(&checkpoint),
    )
    .unwrap();
    let verified = verify_shared_mutation(&manifest, &signed, Some(&checkpoint)).unwrap();
    assert!(
        open_shared_revision(&manifest, &verified, Some(&body), f.reader_id, &f.reader)
            .unwrap()
            .unwrap()
            .as_slice()
            == b"editor change"
    );
}

#[test]
fn owner_anchor_cannot_be_replaced_by_a_directory_entry() {
    let f = Fixture::new();
    let attacker = DeviceSecretKeys::generate().unwrap();
    let mut access = f.access();
    let owner = access
        .members
        .iter_mut()
        .find(|m| m.device_id == f.owner_id)
        .unwrap();
    owner.encryption_public_key = attacker.public_keys().encryption_bytes();
    owner.signing_public_key = attacker.public_keys().signing_bytes();
    let signed = SignedAccessManifest {
        signature: raw_sign(&attacker, &sharing_manifest_message(&access).unwrap()),
        manifest: access,
    };
    assert_eq!(
        verify_shared_manifest(&signed, &f.context, &f.anchor(), None).unwrap_err(),
        SharingCryptoError::OwnerMismatch
    );
}

#[test]
fn unsigned_role_escalation_and_recipient_key_substitution_are_rejected() {
    let f = Fixture::new();
    let original = f.signed_manifest();
    let mut role = original.clone();
    role.manifest
        .members
        .iter_mut()
        .find(|m| m.device_id == f.reader_id)
        .unwrap()
        .role = SharingRole::Editor;
    let mut key = original;
    key.manifest
        .members
        .iter_mut()
        .find(|m| m.device_id == f.reader_id)
        .unwrap()
        .encryption_public_key = DeviceSecretKeys::generate()
        .unwrap()
        .public_keys()
        .encryption_bytes();
    for modified in [role, key] {
        assert!(matches!(
            verify_shared_manifest(&modified, &f.context, &f.anchor(), None),
            Err(SharingCryptoError::Crypto(CryptoError::InvalidSignature))
        ));
    }
}

#[test]
fn manifest_rejects_weak_signing_keys_and_malformed_signatures() {
    let f = Fixture::new();
    let mut weak = f.access();
    let mut identity = [0; 32];
    identity[0] = 1;
    weak.members
        .iter_mut()
        .find(|m| m.device_id == f.reader_id)
        .unwrap()
        .signing_public_key = Bytes::from(identity);
    let signed = SignedAccessManifest {
        signature: raw_sign(&f.owner, &sharing_manifest_message(&weak).unwrap()),
        manifest: weak,
    };
    assert!(matches!(
        verify_shared_manifest(&signed, &f.context, &f.anchor(), None),
        Err(SharingCryptoError::Crypto(CryptoError::InvalidKey { .. }))
    ));
    let mut short = f.signed_manifest();
    short.signature.0.truncate(63);
    assert!(verify_shared_manifest(&short, &f.context, &f.anchor(), None).is_err());
}

#[test]
fn every_mutation_metadata_field_is_authenticated() {
    let f = Fixture::new();
    let (manifest, _, signed, _) = f.first();
    let mut modifications = Vec::new();
    macro_rules! modify {
        ($edit:expr) => {{
            let mut modified = signed.clone();
            $edit(&mut modified.mutation);
            modifications.push(modified);
        }};
    }
    modify!(|m: &mut SharingMutation| m.context.server_instance_id = Uuid::new_v4());
    modify!(|m: &mut SharingMutation| m.context.share_id = ShareId::new());
    modify!(|m: &mut SharingMutation| m.context.item_id = ObjectId::new());
    modify!(|m: &mut SharingMutation| m.context.kind = SharedItemKind::Snippet);
    modify!(|m: &mut SharingMutation| m.context.access_epoch += 1);
    modify!(|m: &mut SharingMutation| {
        m.context.revision = 2;
        m.base_revision = 1;
    });
    modify!(|m: &mut SharingMutation| m.mutation_id = MutationId::new());
    modify!(|m: &mut SharingMutation| m.manifest_revision += 1);
    modify!(|m: &mut SharingMutation| m.manifest_hash.0[0] ^= 1);
    modify!(|m: &mut SharingMutation| m.writer_device_id = f.editor_id);
    modify!(|m: &mut SharingMutation| m.previous_revision_hash.0[0] ^= 1);
    modify!(|m: &mut SharingMutation| m.body_hash.0[0] ^= 1);
    for modified in modifications {
        assert!(verify_shared_mutation(&manifest, &modified, None).is_err());
    }
}

#[test]
fn ciphertext_nonces_ephemeral_keys_and_envelopes_are_authenticated() {
    let f = Fixture::new();
    let (manifest, body, _, verified) = f.first();
    let mut modifications = Vec::new();
    macro_rules! modify {
        ($edit:expr) => {{
            let mut modified = body.clone();
            $edit(&mut modified);
            modifications.push(modified);
        }};
    }
    modify!(|b: &mut SharedEncryptedBody| b.ciphertext.0[0] ^= 1);
    modify!(|b: &mut SharedEncryptedBody| b.nonce.0[0] ^= 1);
    modify!(|b: &mut SharedEncryptedBody| b.envelopes[0].ciphertext.0[0] ^= 1);
    modify!(|b: &mut SharedEncryptedBody| b.envelopes[0].nonce.0[0] ^= 1);
    modify!(|b: &mut SharedEncryptedBody| b.envelopes[0].ephemeral_public_key.0[0] ^= 1);
    modify!(|b: &mut SharedEncryptedBody| b.envelopes.pop());
    modify!(|b: &mut SharedEncryptedBody| {
        b.envelopes.push(b.envelopes[0].clone());
    });
    for modified in modifications {
        assert!(open_shared_revision(
            &manifest,
            &verified,
            Some(&modified),
            f.reader_id,
            &f.reader
        )
        .is_err());
    }
}

#[test]
fn ciphertext_cannot_be_rebound_even_by_an_author_signing_a_new_header() {
    let f = Fixture::new();
    let (manifest, body, _, first) = f.first();
    let checkpoint = first.checkpoint();
    let mut context = f.context.clone();
    context.revision = 2;
    let mutation = f.mutation(
        &manifest,
        context,
        Some(&body),
        Some(&checkpoint),
        f.owner_id,
    );
    let signed = sign_shared_mutation(
        &manifest,
        &f.owner,
        mutation,
        Some(&body),
        Some(&checkpoint),
    )
    .unwrap();
    let verified = verify_shared_mutation(&manifest, &signed, Some(&checkpoint)).unwrap();
    assert!(matches!(
        open_shared_revision(&manifest, &verified, Some(&body), f.reader_id, &f.reader),
        Err(SharingCryptoError::Crypto(CryptoError::Decrypt))
    ));
}

#[test]
fn unrelated_device_and_wrong_recipient_private_key_cannot_open() {
    let f = Fixture::new();
    let (manifest, body, _, verified) = f.first();
    let unrelated = DeviceSecretKeys::generate().unwrap();
    assert_eq!(
        open_shared_revision(
            &manifest,
            &verified,
            Some(&body),
            DeviceId::new(),
            &unrelated
        )
        .unwrap_err(),
        SharingCryptoError::NotRecipient
    );
    assert_eq!(
        open_shared_revision(&manifest, &verified, Some(&body), f.reader_id, &unrelated)
            .unwrap_err(),
        SharingCryptoError::DeviceKeyMismatch
    );
}

#[test]
fn canonical_member_and_envelope_order_is_irrelevant() {
    let f = Fixture::new();
    let mut signed = f.signed_manifest();
    signed.manifest.members.reverse();
    let manifest = verify_shared_manifest(&signed, &f.context, &f.anchor(), None).unwrap();
    let (original, mut body, _, verified) = f.first();
    body.envelopes.reverse();
    assert!(original.hash() == manifest.hash());
    assert!(
        open_shared_revision(&manifest, &verified, Some(&body), f.reader_id, &f.reader)
            .unwrap()
            .unwrap()
            .as_slice()
            == PAYLOAD
    );
}

#[test]
fn manifest_checkpoint_rejects_rollback_fork_and_history_gap() {
    let f = Fixture::new();
    let first = f.manifest();
    let checkpoint = first.checkpoint();
    let mut context = f.context.clone();
    context.access_epoch = 2;
    let mut next = f.access();
    next.revision = 2;
    next.access_epoch = 2;
    next.previous_manifest_hash = Bytes::from(checkpoint.hash);
    let signed = sign_shared_manifest(
        &f.owner,
        next.clone(),
        &context,
        &f.anchor(),
        Some(&checkpoint),
    )
    .unwrap();
    let accepted =
        verify_shared_manifest(&signed, &context, &f.anchor(), Some(&checkpoint)).unwrap();
    assert_eq!(
        verify_shared_manifest(
            &f.signed_manifest(),
            &f.context,
            &f.anchor(),
            Some(&accepted.checkpoint())
        )
        .unwrap_err(),
        SharingCryptoError::Rollback
    );
    let mut fork = next;
    fork.members
        .iter_mut()
        .find(|m| m.device_id == f.reader_id)
        .unwrap()
        .role = SharingRole::Editor;
    let fork = SignedAccessManifest {
        signature: raw_sign(&f.owner, &sharing_manifest_message(&fork).unwrap()),
        manifest: fork,
    };
    assert_eq!(
        verify_shared_manifest(&fork, &context, &f.anchor(), Some(&accepted.checkpoint()))
            .unwrap_err(),
        SharingCryptoError::Fork
    );
    let mut gap = signed.manifest;
    gap.revision = 4;
    gap.access_epoch = 4;
    context.access_epoch = 4;
    let gap = SignedAccessManifest {
        signature: raw_sign(&f.owner, &sharing_manifest_message(&gap).unwrap()),
        manifest: gap,
    };
    assert_eq!(
        verify_shared_manifest(&gap, &context, &f.anchor(), Some(&checkpoint)).unwrap_err(),
        SharingCryptoError::HistoryGap
    );
}

#[test]
fn revision_checkpoint_rejects_rollback_fork_and_missing_history() {
    let f = Fixture::new();
    let (manifest, body, first, verified) = f.first();
    let checkpoint = verified.checkpoint();
    let mut context = f.context.clone();
    context.revision = 2;
    let nextbody = seal_shared_revision(&manifest, &context, b"next").unwrap();
    let next = f.mutation(
        &manifest,
        context,
        Some(&nextbody),
        Some(&checkpoint),
        f.owner_id,
    );
    let signed = sign_shared_mutation(
        &manifest,
        &f.owner,
        next,
        Some(&nextbody),
        Some(&checkpoint),
    )
    .unwrap();
    let accepted = verify_shared_mutation(&manifest, &signed, Some(&checkpoint)).unwrap();
    assert_eq!(
        verify_shared_mutation(&manifest, &first, Some(&accepted.checkpoint())).unwrap_err(),
        SharingCryptoError::Rollback
    );
    let fork = f.mutation(&manifest, f.context.clone(), Some(&body), None, f.editor_id);
    let fork = sign_shared_mutation(&manifest, &f.editor, fork, Some(&body), None).unwrap();
    assert_eq!(
        verify_shared_mutation(&manifest, &fork, Some(&checkpoint)).unwrap_err(),
        SharingCryptoError::Fork
    );
    let mut gap = signed.mutation;
    gap.context.revision = 3;
    gap.base_revision = 2;
    let gap = SignedSharingMutation {
        signature: raw_sign(&f.owner, &sharing_mutation_message(&gap).unwrap()),
        mutation: gap,
    };
    assert_eq!(
        verify_shared_mutation(&manifest, &gap, Some(&checkpoint)).unwrap_err(),
        SharingCryptoError::HistoryGap
    );
}

#[test]
fn mismatching_predecessor_hashes_are_rejected_even_when_validly_signed() {
    let f = Fixture::new();
    let (manifest, body, _, verified) = f.first();
    let mut context = f.context.clone();
    context.revision = 2;
    let mut mutation = f.mutation(
        &manifest,
        context,
        Some(&body),
        Some(&verified.checkpoint()),
        f.owner_id,
    );
    mutation.previous_revision_hash.0[0] ^= 1;
    assert_eq!(
        sign_shared_mutation(
            &manifest,
            &f.owner,
            mutation,
            Some(&body),
            Some(&verified.checkpoint())
        )
        .unwrap_err(),
        SharingCryptoError::Fork
    );
    let mut next = f.access();
    next.revision = 2;
    next.access_epoch = 2;
    next.previous_manifest_hash = Bytes::from([9; 32]);
    let mut context = f.context.clone();
    context.access_epoch = 2;
    assert_eq!(
        sign_shared_manifest(
            &f.owner,
            next,
            &context,
            &f.anchor(),
            Some(&manifest.checkpoint())
        )
        .unwrap_err(),
        SharingCryptoError::Fork
    );
}

#[test]
fn signed_tombstone_cannot_be_replayed_as_a_put_or_resurrected_without_an_editor() {
    let f = Fixture::new();
    let (manifest, body, _, first) = f.first();
    let checkpoint = first.checkpoint();
    let mut context = f.context.clone();
    context.revision = 2;
    let mutation = f.mutation(&manifest, context, None, Some(&checkpoint), f.editor_id);
    let signed =
        sign_shared_mutation(&manifest, &f.editor, mutation, None, Some(&checkpoint)).unwrap();
    let verified = verify_shared_mutation(&manifest, &signed, Some(&checkpoint)).unwrap();
    assert!(
        open_shared_revision(&manifest, &verified, None, f.reader_id, &f.reader)
            .unwrap()
            .is_none()
    );
    assert!(
        open_shared_revision(&manifest, &verified, Some(&body), f.reader_id, &f.reader).is_err()
    );
    let mut altered = signed;
    altered.mutation.operation = SharingOperation::Put;
    altered.mutation.body_hash = Bytes::from(shared_body_hash(&body).unwrap());
    assert!(verify_shared_mutation(&manifest, &altered, Some(&checkpoint)).is_err());
}

#[test]
fn revocation_changes_manifest_epoch_and_excludes_removed_recipient() {
    let f = Fixture::new();
    let (first, oldbody, olds, oldverified) = f.first();
    let mut access = first.manifest().clone();
    access.members.retain(|m| m.device_id != f.reader_id);
    access.revision = 2;
    access.access_epoch = 2;
    access.previous_manifest_hash = Bytes::from(first.hash());
    let mut context = f.context.clone();
    context.access_epoch = 2;
    context.revision = 2;
    let signed = sign_shared_manifest(
        &f.owner,
        access,
        &context,
        &f.anchor(),
        Some(&first.checkpoint()),
    )
    .unwrap();
    let next =
        verify_shared_manifest(&signed, &context, &f.anchor(), Some(&first.checkpoint())).unwrap();
    let body = seal_shared_revision(&next, &context, PAYLOAD).unwrap();
    assert!(!body
        .envelopes
        .iter()
        .any(|e| e.recipient_device_id == f.reader_id));
    let mutation = f.mutation(
        &next,
        context,
        Some(&body),
        Some(&oldverified.checkpoint()),
        f.owner_id,
    );
    let signed = sign_shared_mutation(
        &next,
        &f.owner,
        mutation,
        Some(&body),
        Some(&oldverified.checkpoint()),
    )
    .unwrap();
    let verified = verify_shared_mutation(&next, &signed, Some(&oldverified.checkpoint())).unwrap();
    assert_eq!(
        open_shared_revision(&next, &verified, Some(&body), f.reader_id, &f.reader).unwrap_err(),
        SharingCryptoError::NotRecipient
    );
    assert!(
        open_shared_revision(&next, &verified, Some(&body), f.editor_id, &f.editor)
            .unwrap()
            .unwrap()
            .as_slice()
            == PAYLOAD
    );
    assert_eq!(
        verify_shared_mutation(&next, &olds, None).unwrap_err(),
        SharingCryptoError::ContextMismatch
    );
    // Revocation cannot erase a previously readable revision.
    assert!(
        open_shared_revision(&first, &oldverified, Some(&oldbody), f.reader_id, &f.reader)
            .unwrap()
            .unwrap()
            .as_slice()
            == PAYLOAD
    );
}

fn next_access(
    f: &Fixture,
    previous: &VerifiedSharingManifest,
    context: &SharingContext,
) -> VerifiedSharingManifest {
    let mut access = previous.manifest().clone();
    access.revision += 1;
    access.access_epoch += 1;
    access.previous_manifest_hash = Bytes::from(previous.hash());
    let signed = sign_shared_manifest(
        &f.owner,
        access,
        context,
        &f.anchor(),
        Some(&previous.checkpoint()),
    )
    .unwrap();
    verify_shared_manifest(&signed, context, &f.anchor(), Some(&previous.checkpoint())).unwrap()
}

#[test]
fn revoked_editor_cannot_extend_an_observed_rotation_using_a_historic_manifest() {
    let f = Fixture::new();
    let (old_manifest, _, _, old_revision) = f.first();
    let mut access = old_manifest.manifest().clone();
    access
        .members
        .retain(|member| member.device_id != f.editor_id);
    access.revision = 2;
    access.access_epoch = 2;
    access.previous_manifest_hash = Bytes::from(old_manifest.hash());
    let mut context = f.context.clone();
    context.revision = 2;
    context.access_epoch = 2;
    let signed_access = sign_shared_manifest(
        &f.owner,
        access,
        &context,
        &f.anchor(),
        Some(&old_manifest.checkpoint()),
    )
    .unwrap();
    let manifest = verify_shared_manifest(
        &signed_access,
        &context,
        &f.anchor(),
        Some(&old_manifest.checkpoint()),
    )
    .unwrap();
    let body = seal_shared_revision(&manifest, &context, PAYLOAD).unwrap();
    let mutation = f.mutation(
        &manifest,
        context,
        Some(&body),
        Some(&old_revision.checkpoint()),
        f.owner_id,
    );
    let signed = sign_shared_mutation(
        &manifest,
        &f.owner,
        mutation,
        Some(&body),
        Some(&old_revision.checkpoint()),
    )
    .unwrap();
    let rotation =
        verify_shared_mutation(&manifest, &signed, Some(&old_revision.checkpoint())).unwrap();

    // A revoked participant and a hostile server know the public rotation
    // header hash. Matching that hash is insufficient proof of a current grant.
    let mut context = f.context.clone();
    context.revision = 3;
    let body = seal_shared_revision(&old_manifest, &context, PAYLOAD).unwrap();
    let mutation = f.mutation(
        &old_manifest,
        context,
        Some(&body),
        Some(&rotation.checkpoint()),
        f.editor_id,
    );
    let forged = SignedSharingMutation {
        signature: raw_sign(&f.editor, &sharing_mutation_message(&mutation).unwrap()),
        mutation,
    };
    assert_eq!(
        verify_shared_mutation(&old_manifest, &forged, Some(&rotation.checkpoint())).unwrap_err(),
        SharingCryptoError::Rollback
    );
}

#[test]
fn revision_checkpoint_rejects_a_forked_manifest_at_the_same_access_revision() {
    let f = Fixture::new();
    let (_, _, _, first) = f.first();
    let mut access = f.access();
    access.members[2].role = SharingRole::Editor;
    let signed = sign_shared_manifest(&f.owner, access, &f.context, &f.anchor(), None).unwrap();
    let fork = verify_shared_manifest(&signed, &f.context, &f.anchor(), None).unwrap();
    let mut context = f.context.clone();
    context.revision = 2;
    let body = seal_shared_revision(&fork, &context, PAYLOAD).unwrap();
    let mutation = f.mutation(
        &fork,
        context,
        Some(&body),
        Some(&first.checkpoint()),
        f.owner_id,
    );
    let signed = SignedSharingMutation {
        signature: raw_sign(&f.owner, &sharing_mutation_message(&mutation).unwrap()),
        mutation,
    };
    assert_eq!(
        verify_shared_mutation(&fork, &signed, Some(&first.checkpoint())).unwrap_err(),
        SharingCryptoError::Fork
    );
}

#[test]
fn revision_checkpoint_rejects_skipped_or_forked_access_transitions() {
    let f = Fixture::new();
    let (manifest, _, _, first) = f.first();
    for skipped in [false, true] {
        let mut access = manifest.manifest().clone();
        access.revision = if skipped { 3 } else { 2 };
        access.access_epoch = access.revision;
        access.previous_manifest_hash = Bytes::from([9; 32]);
        let mut context = f.context.clone();
        context.revision = 2;
        context.access_epoch = access.access_epoch;
        // An initial manifest has no checkpoint. A caller verifying history
        // must nevertheless bind it to the access hash of its last revision.
        let signed = sign_shared_manifest(&f.owner, access, &context, &f.anchor(), None).unwrap();
        let next = verify_shared_manifest(&signed, &context, &f.anchor(), None).unwrap();
        let body = seal_shared_revision(&next, &context, PAYLOAD).unwrap();
        let mutation = f.mutation(
            &next,
            context,
            Some(&body),
            Some(&first.checkpoint()),
            f.owner_id,
        );
        let signed = SignedSharingMutation {
            signature: raw_sign(&f.owner, &sharing_mutation_message(&mutation).unwrap()),
            mutation,
        };
        assert_eq!(
            verify_shared_mutation(&next, &signed, Some(&first.checkpoint())).unwrap_err(),
            if skipped {
                SharingCryptoError::HistoryGap
            } else {
                SharingCryptoError::Fork
            }
        );
    }
}

#[test]
fn access_transition_requires_an_owner_put_even_when_an_editor_has_a_valid_grant() {
    let f = Fixture::new();
    let (manifest, _, _, first) = f.first();
    let mut context = f.context.clone();
    context.revision = 2;
    context.access_epoch = 2;
    let next = next_access(&f, &manifest, &context);
    for tombstone in [false, true] {
        let body = (!tombstone).then(|| seal_shared_revision(&next, &context, PAYLOAD).unwrap());
        let writer = if tombstone { f.owner_id } else { f.editor_id };
        let keys = if tombstone { &f.owner } else { &f.editor };
        let mutation = f.mutation(
            &next,
            context.clone(),
            body.as_ref(),
            Some(&first.checkpoint()),
            writer,
        );
        let signed = SignedSharingMutation {
            signature: raw_sign(keys, &sharing_mutation_message(&mutation).unwrap()),
            mutation,
        };
        assert_eq!(
            verify_shared_mutation(&next, &signed, Some(&first.checkpoint())).unwrap_err(),
            SharingCryptoError::Structure("access transition writer or operation")
        );
    }
}

#[test]
fn gap_free_history_keeps_editors_valid_before_and_after_owner_access_rotation() {
    let f = Fixture::new();
    let (manifest, _, _, mut verified) = f.first();
    let mut context = f.context.clone();
    context.revision = 2;
    let body = seal_shared_revision(&manifest, &context, PAYLOAD).unwrap();
    let mutation = f.mutation(
        &manifest,
        context.clone(),
        Some(&body),
        Some(&verified.checkpoint()),
        f.editor_id,
    );
    let signed = sign_shared_mutation(
        &manifest,
        &f.editor,
        mutation,
        Some(&body),
        Some(&verified.checkpoint()),
    )
    .unwrap();
    verified = verify_shared_mutation(&manifest, &signed, Some(&verified.checkpoint())).unwrap();

    context.revision = 3;
    context.access_epoch = 2;
    let next = next_access(&f, &manifest, &context);
    let body = seal_shared_revision(&next, &context, PAYLOAD).unwrap();
    let mutation = f.mutation(
        &next,
        context.clone(),
        Some(&body),
        Some(&verified.checkpoint()),
        f.owner_id,
    );
    let signed = sign_shared_mutation(
        &next,
        &f.owner,
        mutation,
        Some(&body),
        Some(&verified.checkpoint()),
    )
    .unwrap();
    verified = verify_shared_mutation(&next, &signed, Some(&verified.checkpoint())).unwrap();

    context.revision = 4;
    let body = seal_shared_revision(&next, &context, PAYLOAD).unwrap();
    let mutation = f.mutation(
        &next,
        context,
        Some(&body),
        Some(&verified.checkpoint()),
        f.editor_id,
    );
    let signed = sign_shared_mutation(
        &next,
        &f.editor,
        mutation,
        Some(&body),
        Some(&verified.checkpoint()),
    )
    .unwrap();
    verified = verify_shared_mutation(&next, &signed, Some(&verified.checkpoint())).unwrap();
    assert_eq!(verified.checkpoint().access_epoch, 2);
    assert_eq!(verified.checkpoint().manifest_revision, 2);
    assert_eq!(verified.checkpoint().manifest_hash, next.hash());
    assert!(verify_shared_mutation(&next, &signed, Some(&verified.checkpoint())).is_ok());
    assert_eq!(
        open_shared_revision(&next, &verified, Some(&body), f.reader_id, &f.reader)
            .unwrap()
            .unwrap()
            .as_slice(),
        PAYLOAD
    );
}

#[test]
fn low_order_x25519_recipient_is_never_sealed() {
    let f = Fixture::new();
    for bad in [[0; 32], {
        let mut one = [0; 32];
        one[0] = 1;
        one
    }] {
        let mut access = f.access();
        access
            .members
            .iter_mut()
            .find(|m| m.device_id == f.reader_id)
            .unwrap()
            .encryption_public_key = Bytes::from(bad);
        let signed = sign_shared_manifest(&f.owner, access, &f.context, &f.anchor(), None).unwrap();
        let manifest = verify_shared_manifest(&signed, &f.context, &f.anchor(), None).unwrap();
        assert!(matches!(
            seal_shared_revision(&manifest, &f.context, PAYLOAD),
            Err(SharingCryptoError::Crypto(CryptoError::WeakKeyAgreement))
        ));
    }
}

#[test]
fn unsupported_formats_duplicate_devices_and_excessive_plaintext_fail_closed() {
    let f = Fixture::new();
    let manifest = f.manifest();
    let mut access = f.access();
    access.format = 2;
    assert!(sign_shared_manifest(&f.owner, access, &f.context, &f.anchor(), None).is_err());
    let mut access = f.access();
    access.members.push(access.members[1].clone());
    assert!(sign_shared_manifest(&f.owner, access, &f.context, &f.anchor(), None).is_err());
    let plain = Zeroizing::new(vec![
        b'x';
        cc_crypto_core::sharing::MAX_SHARED_PLAINTEXT_BYTES + 1
    ]);
    assert!(seal_shared_revision(&manifest, &f.context, &plain).is_err());
    let (_, mut body, _, verified) = f.first();
    body.format = 2;
    assert!(
        open_shared_revision(&manifest, &verified, Some(&body), f.reader_id, &f.reader).is_err()
    );
}

#[test]
fn debug_never_prints_payload_or_device_private_keys() {
    let f = Fixture::new();
    let (manifest, body, signed, verified) = f.first();
    let debug = format!("{manifest:?}{body:?}{signed:?}{verified:?}{:?}", f.owner);
    assert!(!debug.contains(std::str::from_utf8(PAYLOAD).unwrap()));
    assert!(debug.contains("redacted"));
    assert!(debug.contains("Bytes(<"));
    let plaintext = open_shared_revision(&manifest, &verified, Some(&body), f.reader_id, &f.reader)
        .unwrap()
        .unwrap();
    assert_eq!(format!("{plaintext:?}"), "SharedPlaintext(<redacted>)");
}
#[test]
fn human_sharing_identity_code_binds_instance_account_device_and_both_keys() {
    use cc_crypto_core::sharing::sharing_identity_code;
    let first = cc_crypto_core::DeviceSecretKeys::generate()
        .unwrap()
        .public_keys();
    let second = cc_crypto_core::DeviceSecretKeys::generate()
        .unwrap()
        .public_keys();
    let instance = uuid::Uuid::new_v4();
    let user = cc_protocol::UserId::new();
    let device = cc_protocol::DeviceId::new();
    let code = sharing_identity_code(instance, user, device, &first).unwrap();
    assert!(code == sharing_identity_code(instance, user, device, &first).unwrap());
    assert_eq!(code.len(), 71);
    assert_eq!(code.split('-').count(), 8);
    for other in [
        sharing_identity_code(uuid::Uuid::new_v4(), user, device, &first).unwrap(),
        sharing_identity_code(instance, cc_protocol::UserId::new(), device, &first).unwrap(),
        sharing_identity_code(instance, user, cc_protocol::DeviceId::new(), &first).unwrap(),
        sharing_identity_code(
            instance,
            user,
            device,
            &cc_crypto_core::DevicePublicKeys::from_slices(
                second.encryption_bytes().as_slice(),
                first.signing_bytes().as_slice(),
            )
            .unwrap(),
        )
        .unwrap(),
        sharing_identity_code(
            instance,
            user,
            device,
            &cc_crypto_core::DevicePublicKeys::from_slices(
                first.encryption_bytes().as_slice(),
                second.signing_bytes().as_slice(),
            )
            .unwrap(),
        )
        .unwrap(),
    ] {
        assert!(other != code);
    }
    assert!(sharing_identity_code(uuid::Uuid::nil(), user, device, &first).is_err());
}
