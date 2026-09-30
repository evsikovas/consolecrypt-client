//! Runtime identities only. No private-key fixtures or secret test output.
use cc_crypto_core::sharing::*;
use cc_crypto_core::sharing_enrollment::*;
use cc_crypto_core::DeviceSecretKeys;
use cc_protocol::sharing::*;
use cc_protocol::sharing_enrollment::*;
use cc_protocol::{Bytes, DeviceId, MutationId, ObjectId, ShareId, UserId};
use uuid::Uuid;
use zeroize::Zeroizing;

struct Fixture {
    owner: DeviceSecretKeys,
    anchor: DeviceSecretKeys,
    target: DeviceSecretKeys,
    pin: SharingOwnerAnchor,
    access: VerifiedSharingManifest,
    revision: VerifiedSharingMutation,
    body: SharedEncryptedBody,
    plain: Zeroizing<Vec<u8>>,
    grant: VerifiedEnrollmentGrant,
    request: VerifiedEnrollmentRequest,
    endorsement: VerifiedAnchorEndorsement,
    now: i64,
}
impl Fixture {
    fn new(role: SharingRole, quota: u32) -> Self {
        let owner = DeviceSecretKeys::generate().unwrap();
        let anchor = DeviceSecretKeys::generate().unwrap();
        let target = DeviceSecretKeys::generate().unwrap();
        let owner_id = DeviceId::new();
        let owner_user = UserId::new();
        let anchor_id = DeviceId::new();
        let target_id = DeviceId::new();
        let recipient_user = UserId::new();
        let pin = SharingOwnerAnchor {
            user_id: owner_user,
            device_id: owner_id,
            public_keys: owner.public_keys(),
        };
        let context = SharingContext {
            server_instance_id: Uuid::new_v4(),
            share_id: ShareId::new(),
            item_id: ObjectId::new(),
            revision: 1,
            access_epoch: 1,
            kind: SharedItemKind::Snippet,
        };
        let member = |user_id, device_id, keys: &DeviceSecretKeys, role| SharingMember {
            user_id,
            device_id,
            encryption_public_key: keys.public_keys().encryption_bytes(),
            signing_public_key: keys.public_keys().signing_bytes(),
            role,
        };
        let manifest = AccessManifest {
            format: 1,
            server_instance_id: context.server_instance_id,
            share_id: context.share_id,
            item_id: context.item_id,
            owner_user_id: owner_user,
            owner_device_id: owner_id,
            revision: 1,
            access_epoch: 1,
            previous_manifest_hash: Bytes::from([0; 32]),
            kind: context.kind,
            members: vec![
                member(owner_user, owner_id, &owner, SharingRole::Editor),
                member(recipient_user, anchor_id, &anchor, role),
            ],
        };
        let signed = sign_shared_manifest(&owner, manifest, &context, &pin, None).unwrap();
        let access = verify_shared_manifest(&signed, &context, &pin, None).unwrap();
        let plain = Zeroizing::new(format!("runtime content {}", Uuid::new_v4()).into_bytes());
        let body = seal_shared_revision(&access, &context, &plain).unwrap();
        let mutation = SharingMutation {
            context: context.clone(),
            mutation_id: MutationId::new(),
            base_revision: 0,
            manifest_revision: 1,
            manifest_hash: Bytes::from(access.hash()),
            writer_device_id: owner_id,
            previous_revision_hash: Bytes::from([0; 32]),
            operation: SharingOperation::Put,
            body_hash: Bytes::from(shared_body_hash(&body).unwrap()),
        };
        let signed = sign_shared_mutation(&access, &owner, mutation, Some(&body), None).unwrap();
        let revision = verify_shared_mutation(&access, &signed, None).unwrap();
        let now = chrono::Utc::now().timestamp();
        let scope = EnrollmentScope {
            server_instance_id: context.server_instance_id,
            share_id: context.share_id,
            item_id: context.item_id,
            kind: context.kind,
        };
        let binding = |device_id, keys: &DeviceSecretKeys| EnrollmentDeviceBinding {
            user_id: recipient_user,
            device_id,
            encryption_public_key: keys.public_keys().encryption_bytes(),
            signing_public_key: keys.public_keys().signing_bytes(),
        };
        let grant = SharingOwnDevicesGrantState {
            format: 1,
            scope: scope.clone(),
            owner_user_id: owner_user,
            owner_device_id: owner_id,
            grant_id: Uuid::new_v4(),
            grant_revision: 1,
            previous_grant_state_hash: Bytes::from([0; 32]),
            status: EnrollmentGrantStatus::Active,
            anchor: binding(anchor_id, &anchor),
            access_manifest_hash: Bytes::from(access.hash()),
            access_epoch: 1,
            role_ceiling: role,
            mode: EnrollmentMode::Manual,
            not_before: now,
            expires_at: now + 3600,
            max_admissions: quota,
            admitted_count: 0,
        };
        let signed = sign_grant_state(
            &owner,
            grant,
            &pin,
            None,
            GrantTransition::Genesis { access: &access },
            now,
        )
        .unwrap();
        let grant = verify_grant_state(
            &signed,
            &pin,
            None,
            GrantTransition::Genesis { access: &access },
            now,
        )
        .unwrap();
        let request = SharingOwnDeviceRequest {
            format: 1,
            scope,
            request_id: Uuid::new_v4(),
            grant_state_hash: Bytes::from(grant.hash()),
            access_manifest_hash: Bytes::from(access.hash()),
            access_epoch: 1,
            target: binding(target_id, &target),
            requested_role: role,
            nonce: Bytes::from(random32()),
            not_before: now,
            expires_at: now + 600,
        };
        let signed = sign_device_request(&target, request, &grant, &access, now).unwrap();
        let request = verify_device_request(&signed, &grant, &access, now).unwrap();
        let signed = endorse_device_request(
            &anchor,
            &request,
            &access,
            &enrollment_pairing_code(&request).unwrap(),
            now,
        )
        .unwrap();
        let endorsement = verify_anchor_endorsement(&signed, &request, &access, now).unwrap();
        Self {
            owner,
            anchor,
            target,
            pin,
            access,
            revision,
            body,
            plain,
            grant,
            request,
            endorsement,
            now,
        }
    }
    fn proof(
        &self,
    ) -> (
        SignedSharingDeviceChallenge,
        OwnerChallengeSecret,
        VerifiedDevicePossession,
    ) {
        let (challenge, pending) = create_device_challenge(
            &self.owner,
            &self.pin,
            &self.request,
            &self.endorsement,
            &self.access,
            self.now,
            None,
        )
        .unwrap();
        let verified = verify_device_challenge(
            &challenge,
            &self.pin,
            &self.request,
            &self.endorsement,
            &self.access,
            self.now,
            None,
        )
        .unwrap();
        let response = answer_device_challenge(
            &self.target,
            &self.pin,
            &self.request,
            &self.endorsement,
            &self.access,
            &challenge,
            self.now,
            None,
        )
        .unwrap();
        let proof = verify_device_possession(
            &response,
            &pending,
            &self.request,
            &self.access,
            self.now,
            &verified.checkpoint(),
        )
        .unwrap();
        (challenge, pending, proof)
    }
    fn accepted(
        &self,
        proof: &VerifiedDevicePossession,
        others: &[VerifiedEnrollmentGrant],
    ) -> AcceptOwnDeviceRequest {
        accept_own_device(
            &self.owner,
            &self.pin,
            &self.access,
            &self.revision,
            &self.body,
            &self.request,
            &self.endorsement,
            proof,
            &proof.challenge().checkpoint(),
            &self.grant,
            others,
            self.now,
        )
        .unwrap()
    }
    fn verify_accept(
        &self,
        value: &AcceptOwnDeviceRequest,
        proof: &VerifiedDevicePossession,
        others: &[VerifiedEnrollmentGrant],
    ) -> Result<VerifiedEnrollmentAcceptance, EnrollmentCryptoError> {
        verify_own_device_acceptance(
            value,
            &self.pin,
            &self.access,
            &self.revision,
            &self.request,
            &self.endorsement,
            proof.challenge(),
            proof.response(),
            others,
            self.now,
        )
    }
}
fn random32() -> [u8; 32] {
    let mut data = [0u8; 32];
    cc_crypto_core::fill_random(&mut data).unwrap();
    data
}

#[test]
fn reader_and_editor_admission_preserves_content_exact_acl_and_rekeys() {
    for role in [SharingRole::Reader, SharingRole::Editor] {
        let f = Fixture::new(role, 2);
        let (_, pending, proof) = f.proof();
        assert!(format!("{pending:?}").contains("<redacted>"));
        let value = f.accepted(&proof, &[]);
        f.verify_accept(&value, &proof, &[]).unwrap();
        let context = &value.rotation.revision.signed.mutation.context;
        let after = verify_shared_manifest(
            &value.rotation.access,
            context,
            &f.pin,
            Some(&f.access.checkpoint()),
        )
        .unwrap();
        let revision = verify_shared_mutation(
            &after,
            &value.rotation.revision.signed,
            Some(&f.revision.checkpoint()),
        )
        .unwrap();
        assert!(after.manifest().members.len() == 3);
        for member in &f.access.manifest().members {
            assert!(after.member(member.device_id) == Some(member));
        }
        let target = f.request.signed().request.target.device_id;
        assert!(after.member(target).unwrap().role == role);
        assert!(
            open_shared_revision(&f.access, &f.revision, Some(&f.body), target, &f.target).is_err()
        );
        let plain = open_shared_revision(
            &after,
            &revision,
            value.rotation.revision.body.as_ref(),
            target,
            &f.target,
        )
        .unwrap()
        .unwrap();
        assert!(plain.as_slice() == f.plain.as_slice());
        assert!(value.rotation.revision.body.as_ref().unwrap().ciphertext != f.body.ciphertext);
        assert!(
            value.consumed_grant_successor.grant.admitted_count == 1
                && value.consumed_grant_successor.grant.status == EnrollmentGrantStatus::Active
        );
        assert!(
            value
                .consumed_grant_successor
                .grant
                .previous_grant_state_hash
                .as_slice()
                == f.grant.hash()
        );
        assert!(
            verify_device_request(f.request.signed(), &f.grant, &after, f.now).is_err(),
            "old grant/request cannot retarget a new access head"
        );
    }
}

#[test]
fn whole_request_pairing_rejects_oracle_and_binds_target_role_nonce_and_deadline() {
    let f = Fixture::new(SharingRole::Editor, 2);
    let code = enrollment_pairing_code(&f.request).unwrap();
    assert_eq!(code.len(), 71);
    assert!(matches!(
        endorse_device_request(&f.anchor, &f.request, &f.access, "", f.now),
        Err(EnrollmentCryptoError::PairingRequired)
    ));
    let identity_code = sharing_identity_code(
        f.access.manifest().server_instance_id,
        f.request.signed().request.target.user_id,
        f.request.signed().request.target.device_id,
        &f.target.public_keys(),
    )
    .unwrap();
    assert!(
        endorse_device_request(&f.anchor, &f.request, &f.access, &identity_code, f.now).is_err()
    );
    for field in 0..5 {
        let mut request = f.request.signed().request.clone();
        match field {
            0 => request.request_id = Uuid::new_v4(),
            1 => request.target.device_id = DeviceId::new(),
            2 => request.requested_role = SharingRole::Reader,
            3 => request.nonce = Bytes::from(random32()),
            _ => request.expires_at -= 1,
        }
        let signed = sign_device_request(&f.target, request, &f.grant, &f.access, f.now).unwrap();
        let changed = verify_device_request(&signed, &f.grant, &f.access, f.now).unwrap();
        assert!(enrollment_pairing_code(&changed).unwrap() != code);
        assert!(endorse_device_request(&f.anchor, &changed, &f.access, &code, f.now).is_err());
    }
    let mut request = f.request.signed().clone();
    request.request.target.encryption_public_key = DeviceSecretKeys::generate()
        .unwrap()
        .public_keys()
        .encryption_bytes();
    assert!(verify_device_request(&request, &f.grant, &f.access, f.now).is_err());
    request = f.request.signed().clone();
    request.request.scope.server_instance_id = Uuid::new_v4();
    assert!(verify_device_request(&request, &f.grant, &f.access, f.now).is_err());
    assert!(verify_device_request(f.request.signed(), &f.grant, &f.access, f.now - 1).is_err());
    assert!(verify_device_request(f.request.signed(), &f.grant, &f.access, f.now + 600).is_err());
}

#[test]
fn signature_and_challenge_ciphertext_tampering_cannot_prove_possession() {
    let f = Fixture::new(SharingRole::Reader, 1);
    let (challenge, pending, proof) = f.proof();
    for field in 0..5 {
        let mut changed = challenge.clone();
        match field {
            0 => changed.challenge.nonce = Bytes::from([0; 24]),
            1 => changed.challenge.ephemeral_public_key = Bytes::from([0; 32]),
            2 => changed.challenge.ciphertext = Bytes::from([0; 48]),
            3 => changed.challenge.generation += 1,
            _ => changed.signature = Bytes::from([0; 64]),
        }
        assert!(answer_device_challenge(
            &f.target,
            &f.pin,
            &f.request,
            &f.endorsement,
            &f.access,
            &changed,
            f.now,
            None
        )
        .is_err());
    }
    assert!(answer_device_challenge(
        &DeviceSecretKeys::generate().unwrap(),
        &f.pin,
        &f.request,
        &f.endorsement,
        &f.access,
        &challenge,
        f.now,
        None
    )
    .is_err());
    let mut response = proof.response().signed().clone();
    response.response.response_digest = Bytes::from(random32());
    assert!(verify_device_possession(
        &response,
        &pending,
        &f.request,
        &f.access,
        f.now,
        &proof.challenge().checkpoint()
    )
    .is_err());
    let (_, other_secret, _) = f.proof();
    assert!(verify_device_possession(
        proof.response().signed(),
        &other_secret,
        &f.request,
        &f.access,
        f.now,
        &proof.challenge().checkpoint()
    )
    .is_err());
}

#[test]
fn replacing_challenge_invalidates_old_generation_and_old_owner_secret() {
    let f = Fixture::new(SharingRole::Editor, 2);
    let (first, secret, proof) = f.proof();
    let old = proof.challenge().checkpoint();
    let (second, new_secret) = create_device_challenge(
        &f.owner,
        &f.pin,
        &f.request,
        &f.endorsement,
        &f.access,
        f.now,
        Some(&old),
    )
    .unwrap();
    assert!(
        second.challenge.generation == 2
            && second.challenge.challenge_id != first.challenge.challenge_id
            && second.challenge.ciphertext != first.challenge.ciphertext
    );
    let verified = verify_device_challenge(
        &second,
        &f.pin,
        &f.request,
        &f.endorsement,
        &f.access,
        f.now,
        Some(&old),
    )
    .unwrap();
    assert!(
        accept_own_device(
            &f.owner,
            &f.pin,
            &f.access,
            &f.revision,
            &f.body,
            &f.request,
            &f.endorsement,
            &proof,
            &verified.checkpoint(),
            &f.grant,
            &[],
            f.now,
        )
        .is_err(),
        "a possession minted before replacement cannot sign an acceptance"
    );
    assert!(verify_device_challenge(
        &first,
        &f.pin,
        &f.request,
        &f.endorsement,
        &f.access,
        f.now,
        Some(&verified.checkpoint())
    )
    .is_err());
    assert!(verify_device_possession(
        proof.response().signed(),
        &secret,
        &f.request,
        &f.access,
        f.now,
        &verified.checkpoint()
    )
    .is_err());
    let response = answer_device_challenge(
        &f.target,
        &f.pin,
        &f.request,
        &f.endorsement,
        &f.access,
        &second,
        f.now,
        Some(&old),
    )
    .unwrap();
    verify_device_possession(
        &response,
        &new_secret,
        &f.request,
        &f.access,
        f.now,
        &verified.checkpoint(),
    )
    .unwrap();
    assert!(verify_device_possession(
        &response,
        &new_secret,
        &f.request,
        &f.access,
        f.now + 300,
        &verified.checkpoint()
    )
    .is_err());
}

#[test]
fn grant_quota_terminal_revoke_and_immutable_permissions_are_enforced() {
    let f = Fixture::new(SharingRole::Reader, 1);
    let (_, _, proof) = f.proof();
    let accepted = f.accepted(&proof, &[]);
    assert!(
        accepted.consumed_grant_successor.grant.status == EnrollmentGrantStatus::Revoked
            && accepted.consumed_grant_successor.grant.admitted_count == 1
    );
    let mut revoke = f.grant.signed().grant.clone();
    revoke.grant_revision = 2;
    revoke.previous_grant_state_hash = Bytes::from(f.grant.hash());
    revoke.status = EnrollmentGrantStatus::Revoked;
    let signed = sign_grant_state(
        &f.owner,
        revoke.clone(),
        &f.pin,
        Some(&f.grant),
        GrantTransition::Revoke { access: &f.access },
        f.now + 4000,
    )
    .unwrap();
    let terminal = verify_grant_state(
        &signed,
        &f.pin,
        Some(&f.grant),
        GrantTransition::Revoke { access: &f.access },
        f.now + 4000,
    )
    .unwrap();
    assert!(
        accept_own_device(
            &f.owner,
            &f.pin,
            &f.access,
            &f.revision,
            &f.body,
            &f.request,
            &f.endorsement,
            &proof,
            &proof.challenge().checkpoint(),
            &terminal,
            &[],
            f.now,
        )
        .is_err(),
        "a cached possession cannot override the observed terminal grant"
    );
    let mut resurrection = signed.grant.clone();
    resurrection.grant_revision = 3;
    resurrection.previous_grant_state_hash = Bytes::from(terminal.hash());
    resurrection.status = EnrollmentGrantStatus::Active;
    assert!(sign_grant_state(
        &f.owner,
        resurrection,
        &f.pin,
        Some(&terminal),
        GrantTransition::Revoke { access: &f.access },
        f.now
    )
    .is_err());
    for field in 0..5 {
        let mut changed = revoke.clone();
        match field {
            0 => changed.expires_at += 1,
            1 => changed.role_ceiling = SharingRole::Editor,
            2 => changed.max_admissions += 1,
            3 => changed.mode = EnrollmentMode::Automatic,
            _ => changed.anchor.device_id = DeviceId::new(),
        }
        assert!(sign_grant_state(
            &f.owner,
            changed,
            &f.pin,
            Some(&f.grant),
            GrantTransition::Revoke { access: &f.access },
            f.now
        )
        .is_err());
    }
}

#[test]
fn atomic_acceptance_binds_exact_successors_and_rejects_receipt_substitution() {
    let f = Fixture::new(SharingRole::Editor, 3);
    let (_, _, proof) = f.proof();
    let mut other = f.grant.signed().grant.clone();
    other.grant_id = Uuid::new_v4();
    let signed = sign_grant_state(
        &f.owner,
        other,
        &f.pin,
        None,
        GrantTransition::Genesis { access: &f.access },
        f.now,
    )
    .unwrap();
    let other = verify_grant_state(
        &signed,
        &f.pin,
        None,
        GrantTransition::Genesis { access: &f.access },
        f.now,
    )
    .unwrap();
    let value = f.accepted(&proof, std::slice::from_ref(&other));
    f.verify_accept(&value, &proof, std::slice::from_ref(&other))
        .unwrap();
    assert!(
        value.other_grant_successors[0].grant.admitted_count == 0
            && value.other_grant_successors[0].grant.access_epoch == 2
    );
    let mut terminal_other = value.other_grant_successors[0].grant.clone();
    terminal_other.status = EnrollmentGrantStatus::Revoked;
    let after = verify_shared_manifest(
        &value.rotation.access,
        &value.rotation.revision.signed.mutation.context,
        &f.pin,
        Some(&f.access.checkpoint()),
    )
    .unwrap();
    assert!(
        sign_grant_state(
            &f.owner,
            terminal_other,
            &f.pin,
            Some(&other),
            GrantTransition::Acceptance {
                before: &f.access,
                after: &after,
                consumed: false
            },
            f.now,
        )
        .is_err(),
        "nonconsumed successors must remain active"
    );
    assert!(accept_own_device(
        &f.owner,
        &f.pin,
        &f.access,
        &f.revision,
        &f.body,
        &f.request,
        &f.endorsement,
        &proof,
        &proof.challenge().checkpoint(),
        &f.grant,
        &[other.clone(), other.clone()],
        f.now
    )
    .is_err());
    for field in 0..6 {
        let mut changed = value.clone();
        match field {
            0 => changed.acceptance.acceptance.request_hash = Bytes::from(random32()),
            1 => changed
                .rotation
                .access
                .manifest
                .members
                .pop()
                .map(|_| ())
                .unwrap(),
            2 => changed.consumed_grant_successor.grant.admitted_count = 0,
            3 => changed.other_grant_successors[0].signature = Bytes::from([0; 64]),
            4 => {
                changed.acceptance.acceptance.other_grant_successor_hashes[0].state_hash =
                    Bytes::from(random32())
            }
            _ => changed.rotation.revision.signed.mutation.context.revision += 1,
        }
        assert!(f
            .verify_accept(&changed, &proof, std::slice::from_ref(&other))
            .is_err());
    }
}
