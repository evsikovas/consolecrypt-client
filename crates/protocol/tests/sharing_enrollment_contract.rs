use cc_protocol::sharing::{self, SharedItemKind, SharingRole};
use cc_protocol::sharing_enrollment::*;
use cc_protocol::{Bytes, DeviceId, MutationId, ObjectId, ShareId, UserId};
use serde::{de::DeserializeOwned, Serialize};
use serde_json::Value;
use uuid::Uuid;

// Synthetic public identities/signature packets only. No private-key fixture
// or possession challenge plaintext is generated or stored in this crate.
fn id(value: u128) -> Uuid {
    Uuid::from_u128(value)
}
fn bytes(value: u8, length: usize) -> Bytes {
    Bytes::new(vec![value; length])
}
fn scope() -> EnrollmentScope {
    EnrollmentScope {
        server_instance_id: id(1),
        share_id: ShareId(id(2)),
        item_id: ObjectId(id(3)),
        kind: SharedItemKind::Host,
    }
}
fn anchor() -> EnrollmentDeviceBinding {
    EnrollmentDeviceBinding {
        user_id: UserId(id(7)),
        device_id: DeviceId(id(8)),
        encryption_public_key: bytes(0x11, 32),
        signing_public_key: bytes(0x12, 32),
    }
}
fn target() -> EnrollmentDeviceBinding {
    EnrollmentDeviceBinding {
        user_id: UserId(id(7)),
        device_id: DeviceId(id(9)),
        encryption_public_key: bytes(0x21, 32),
        signing_public_key: bytes(0x22, 32),
    }
}
fn grant() -> SharingOwnDevicesGrantState {
    SharingOwnDevicesGrantState {
        format: FORMAT,
        scope: scope(),
        owner_user_id: UserId(id(4)),
        owner_device_id: DeviceId(id(5)),
        grant_id: id(6),
        grant_revision: 1,
        previous_grant_state_hash: bytes(0, 32),
        status: EnrollmentGrantStatus::Active,
        anchor: anchor(),
        access_manifest_hash: bytes(0x32, 32),
        access_epoch: 3,
        role_ceiling: SharingRole::Editor,
        mode: EnrollmentMode::Automatic,
        not_before: 1_900_000_000,
        expires_at: 1_900_003_600,
        max_admissions: 2,
        admitted_count: 0,
    }
}
fn request() -> SharingOwnDeviceRequest {
    SharingOwnDeviceRequest {
        format: FORMAT,
        scope: scope(),
        request_id: id(10),
        grant_state_hash: bytes(0x31, 32),
        access_manifest_hash: bytes(0x32, 32),
        access_epoch: 3,
        target: target(),
        requested_role: SharingRole::Reader,
        nonce: bytes(0x44, 32),
        not_before: 1_900_000_010,
        expires_at: 1_900_000_600,
    }
}
fn endorsement() -> SharingAnchorEndorsement {
    SharingAnchorEndorsement {
        format: FORMAT,
        request_hash: bytes(0x33, 32),
        anchor_device_id: DeviceId(id(8)),
    }
}
fn challenge() -> SharingDeviceChallenge {
    SharingDeviceChallenge {
        format: FORMAT,
        request_hash: bytes(0x33, 32),
        anchor_endorsement_hash: bytes(0x34, 32),
        challenge_id: id(11),
        generation: 1,
        not_before: 1_900_000_020,
        expires_at: 1_900_000_250,
        ephemeral_public_key: bytes(0x41, 32),
        nonce: bytes(0x42, 24),
        ciphertext: bytes(0x43, 48),
    }
}
fn response() -> SharingDeviceChallengeResponse {
    SharingDeviceChallengeResponse {
        format: FORMAT,
        request_hash: bytes(0x33, 32),
        challenge_hash: bytes(0x35, 32),
        response_digest: bytes(0x36, 32),
    }
}
fn acceptance() -> SharingOwnDeviceAcceptance {
    SharingOwnDeviceAcceptance {
        format: FORMAT,
        request_hash: bytes(0x33, 32),
        anchor_endorsement_hash: bytes(0x34, 32),
        challenge_hash: bytes(0x35, 32),
        response_hash: bytes(0x37, 32),
        consumed_grant_state_hash: bytes(0x31, 32),
        result_access_manifest_hash: bytes(0x39, 32),
        result_revision_hash: bytes(0x3a, 32),
        consumed_grant_successor_hash: bytes(0x38, 32),
        other_grant_successor_hashes: vec![
            EnrollmentGrantSuccessorHash {
                grant_id: id(20),
                state_hash: bytes(0x3b, 32),
            },
            EnrollmentGrantSuccessorHash {
                grant_id: id(21),
                state_hash: bytes(0x3c, 32),
            },
        ],
    }
}
fn signed_grant() -> SignedSharingOwnDevicesGrantState {
    SignedSharingOwnDevicesGrantState {
        grant: grant(),
        signature: bytes(0x51, 64),
    }
}
fn signed_request() -> SignedSharingOwnDeviceRequest {
    SignedSharingOwnDeviceRequest {
        request: request(),
        signature: bytes(0x52, 64),
    }
}
fn signed_endorsement() -> SignedSharingAnchorEndorsement {
    SignedSharingAnchorEndorsement {
        endorsement: endorsement(),
        signature: bytes(0x53, 64),
    }
}
fn signed_challenge() -> SignedSharingDeviceChallenge {
    SignedSharingDeviceChallenge {
        challenge: challenge(),
        signature: bytes(0x54, 64),
    }
}
fn signed_response() -> SignedSharingDeviceChallengeResponse {
    SignedSharingDeviceChallengeResponse {
        response: response(),
        signature: bytes(0x55, 64),
    }
}
fn signed_acceptance() -> SignedSharingOwnDeviceAcceptance {
    SignedSharingOwnDeviceAcceptance {
        acceptance: acceptance(),
        signature: bytes(0x56, 64),
    }
}
fn hex(input: &[u8]) -> String {
    input.iter().map(|b| format!("{b:02x}")).collect()
}

#[test]
fn all_six_canonical_signing_messages_have_independent_complete_fixed_vectors() {
    for (actual, expected) in [
        (enrollment_grant_message(&grant()).unwrap(), GRANT_VECTOR),
        (
            enrollment_request_message(&request()).unwrap(),
            REQUEST_VECTOR,
        ),
        (
            enrollment_endorsement_message(&endorsement()).unwrap(),
            ENDORSEMENT_VECTOR,
        ),
        (
            enrollment_challenge_message(&challenge()).unwrap(),
            CHALLENGE_VECTOR,
        ),
        (
            enrollment_response_message(&response()).unwrap(),
            RESPONSE_VECTOR,
        ),
        (
            enrollment_acceptance_message(&acceptance()).unwrap(),
            ACCEPTANCE_VECTOR,
        ),
    ] {
        assert_eq!(hex(&actual), expected);
    }
}

#[test]
fn every_signed_record_hash_input_commits_to_canonical_message_and_complete_signature() {
    for (actual, expected) in [
        (
            enrollment_grant_hash_input(&signed_grant()).unwrap(),
            GRANT_HASH_VECTOR,
        ),
        (
            enrollment_request_hash_input(&signed_request()).unwrap(),
            REQUEST_HASH_VECTOR,
        ),
        (
            enrollment_endorsement_hash_input(&signed_endorsement()).unwrap(),
            ENDORSEMENT_HASH_VECTOR,
        ),
        (
            enrollment_challenge_hash_input(&signed_challenge()).unwrap(),
            CHALLENGE_HASH_VECTOR,
        ),
        (
            enrollment_response_hash_input(&signed_response()).unwrap(),
            RESPONSE_HASH_VECTOR,
        ),
        (
            enrollment_acceptance_hash_input(&signed_acceptance()).unwrap(),
            ACCEPTANCE_HASH_VECTOR,
        ),
    ] {
        assert_eq!(hex(&actual), expected);
    }
    let mut changed = signed_request();
    changed.signature.0[63] ^= 1;
    assert_ne!(
        enrollment_request_hash_input(&changed).unwrap(),
        enrollment_request_hash_input(&signed_request()).unwrap()
    );
    changed.signature.0.pop();
    assert!(enrollment_request_hash_input(&changed).is_err());
}

#[test]
fn header_aad_key_info_and_pairing_have_distinct_complete_vectors_without_ciphertext_cycle() {
    assert_eq!(
        hex(&enrollment_challenge_header(&challenge()).unwrap()),
        HEADER_VECTOR
    );
    assert_eq!(
        hex(&enrollment_challenge_aad(&challenge()).unwrap()),
        AAD_VECTOR
    );
    assert_eq!(
        hex(&enrollment_challenge_key_info(&challenge(), &target()).unwrap()),
        KEY_INFO_VECTOR
    );
    assert_eq!(
        hex(&enrollment_pairing_message(&[0x33; 32]).unwrap()),
        PAIRING_VECTOR
    );
    let mut draft = challenge();
    draft.ciphertext = Bytes::default();
    assert_eq!(
        enrollment_challenge_aad(&draft).unwrap(),
        enrollment_challenge_aad(&challenge()).unwrap()
    );
    assert!(enrollment_challenge_message(&draft).is_err());
    let mut changed = challenge();
    changed.ciphertext.0[0] ^= 1;
    assert_eq!(
        enrollment_challenge_aad(&changed).unwrap(),
        enrollment_challenge_aad(&challenge()).unwrap()
    );
    assert_ne!(
        enrollment_challenge_message(&changed).unwrap(),
        enrollment_challenge_message(&challenge()).unwrap()
    );
    let domains = [
        labels::GRANT,
        labels::REQUEST,
        labels::ENDORSEMENT,
        labels::CHALLENGE,
        labels::RESPONSE,
        labels::ACCEPTANCE,
        labels::GRANT_HASH,
        labels::REQUEST_HASH,
        labels::ENDORSEMENT_HASH,
        labels::CHALLENGE_HASH,
        labels::RESPONSE_HASH,
        labels::ACCEPTANCE_HASH,
        labels::CHALLENGE_HEADER,
        labels::CHALLENGE_AAD,
        labels::CHALLENGE_KEY,
        labels::RESPONSE_DIGEST,
        labels::PAIRING,
    ];
    for (i, domain) in domains.iter().enumerate() {
        for other in &domains[i + 1..] {
            assert_ne!(domain, other);
        }
    }
}

#[test]
fn pairing_request_binds_every_context_identity_key_role_nonce_and_lifetime_field() {
    let original = enrollment_request_hash_input(&signed_request()).unwrap();
    for field in 0..15 {
        let mut other = signed_request();
        match field {
            0 => other.request.scope.server_instance_id = id(101),
            1 => other.request.scope.share_id = ShareId(id(102)),
            2 => other.request.scope.item_id = ObjectId(id(103)),
            3 => other.request.scope.kind = SharedItemKind::Snippet,
            4 => other.request.request_id = id(110),
            5 => other.request.grant_state_hash.0[0] ^= 1,
            6 => other.request.access_manifest_hash.0[0] ^= 1,
            7 => other.request.access_epoch += 1,
            8 => other.request.target.user_id = UserId(id(107)),
            9 => other.request.target.device_id = DeviceId(id(109)),
            10 => other.request.target.encryption_public_key.0[0] ^= 1,
            11 => other.request.target.signing_public_key.0[0] ^= 1,
            12 => other.request.requested_role = SharingRole::Editor,
            13 => other.request.nonce.0[0] ^= 1,
            _ => {
                other.request.not_before += 1;
                other.request.expires_at += 1;
            }
        }
        assert_ne!(enrollment_request_hash_input(&other).unwrap(), original);
    }
    let mut other = target();
    other.encryption_public_key.0[0] ^= 1;
    assert_ne!(
        enrollment_challenge_key_info(&challenge(), &other).unwrap(),
        enrollment_challenge_key_info(&challenge(), &target()).unwrap()
    );
    other = target();
    other.signing_public_key.0[0] ^= 1;
    assert_ne!(
        enrollment_challenge_key_info(&challenge(), &other).unwrap(),
        enrollment_challenge_key_info(&challenge(), &target()).unwrap()
    );
}

fn strict_schema<T: Serialize + DeserializeOwned>(record: &T, required: &str) {
    let value = serde_json::to_value(record).unwrap();
    assert!(serde_json::from_value::<T>(value.clone()).is_ok());
    let mut missing = value.clone();
    missing.as_object_mut().unwrap().remove(required);
    assert!(serde_json::from_value::<T>(missing).is_err());
    let mut extra = value;
    extra
        .as_object_mut()
        .unwrap()
        .insert("unsigned_extra".into(), Value::Bool(true));
    assert!(serde_json::from_value::<T>(extra).is_err());
}

#[test]
fn every_new_record_and_signed_wrapper_rejects_missing_required_or_extra_fields() {
    strict_schema(&scope(), "kind");
    strict_schema(&anchor(), "signing_public_key");
    strict_schema(&grant(), "mode");
    strict_schema(&request(), "grant_state_hash");
    strict_schema(&endorsement(), "anchor_device_id");
    strict_schema(&challenge(), "generation");
    strict_schema(&response(), "response_digest");
    strict_schema(&acceptance(), "consumed_grant_successor_hash");
    strict_schema(&signed_grant(), "signature");
    strict_schema(&signed_request(), "signature");
    strict_schema(&signed_endorsement(), "signature");
    strict_schema(&signed_challenge(), "signature");
    strict_schema(&signed_response(), "signature");
    strict_schema(&signed_acceptance(), "signature");
    let mut nested = serde_json::to_value(signed_request()).unwrap();
    nested["request"]["target"]["identity_root"] = Value::Bool(true);
    assert!(serde_json::from_value::<SignedSharingOwnDeviceRequest>(nested).is_err());
    let json = serde_json::to_string(&signed_challenge()).unwrap();
    assert!(
        !json.contains("secret") && !json.contains("plaintext") && !json.contains("private_key")
    );
    assert!(!format!("{:?}", signed_challenge()).contains("67, 67"));
    strict_schema(
        &OwnDevicesGrantPage {
            items: vec![signed_grant()],
            next_after: None,
            has_more: false,
        },
        "has_more",
    );
    strict_schema(
        &OwnDevicesGrantHistoryPage {
            states: vec![signed_grant()],
            latest_revision: 1,
            has_more: false,
        },
        "latest_revision",
    );
    strict_schema(&transcript(), "request");
    strict_schema(
        &OwnDeviceRequestPage {
            items: vec![transcript()],
            next_after: None,
            has_more: false,
        },
        "items",
    );
    strict_schema(&acceptance().other_grant_successor_hashes[0], "state_hash");
}

#[test]
fn json_numeric_enum_and_byte_fields_reject_ambiguous_or_out_of_range_values() {
    let good = serde_json::to_value(grant()).unwrap();
    for (name, value) in [
        ("grant_revision", serde_json::json!(-1)),
        ("grant_revision", serde_json::json!(1.0)),
        ("max_admissions", serde_json::json!(u64::MAX)),
        ("not_before", serde_json::json!("1900000000")),
        ("mode", serde_json::json!("offline")),
        ("status", serde_json::json!("renewed")),
        ("access_manifest_hash", serde_json::json!("invalid base64!")),
    ] {
        let mut invalid = good.clone();
        invalid[name] = value;
        assert!(serde_json::from_value::<SharingOwnDevicesGrantState>(invalid).is_err());
    }
    assert_eq!(MAX_GRANTS_PER_SHARE, 16);
    assert_eq!(MAX_RETAINED_GRANTS_PER_SHARE, 256);
    assert_eq!(MAX_RETAINED_REQUESTS_PER_SHARE, 1024);
}

#[test]
fn grant_genesis_quota_numeric_id_hash_and_lifetime_bounds_are_fixed() {
    for field in 0..14 {
        let mut other = grant();
        match field {
            0 => other.format = 2,
            1 => other.scope.server_instance_id = Uuid::nil(),
            2 => other.grant_id = Uuid::nil(),
            3 => other.owner_user_id = UserId::NIL,
            4 => other.anchor.device_id = DeviceId::NIL,
            5 => other.grant_revision = 0,
            6 => other.grant_revision = i64::MAX as u64 + 1,
            7 => other.access_epoch = i64::MAX as u64 + 1,
            8 => other.previous_grant_state_hash.0[0] = 1,
            9 => other.admitted_count = 1,
            10 => other.max_admissions = 0,
            11 => other.max_admissions = MAX_ADMISSIONS + 1,
            12 => other.expires_at = other.not_before + MAX_GRANT_LIFETIME_SECONDS + 1,
            _ => other.anchor.signing_public_key.0.pop().map(|_| ()).unwrap(),
        }
        assert!(enrollment_grant_message(&other).is_err());
    }
    let mut revoked = grant();
    revoked.grant_revision = 2;
    revoked.previous_grant_state_hash = bytes(0x61, 32);
    revoked.status = EnrollmentGrantStatus::Revoked;
    revoked.not_before = 1;
    revoked.expires_at = 2;
    validate_grant(&revoked).unwrap(); // Expired records remain usable for revoke.
    revoked.admitted_count = revoked.max_admissions;
    validate_grant(&revoked).unwrap();
    revoked.status = EnrollmentGrantStatus::Active;
    assert!(validate_grant(&revoked).is_err());
    revoked.admitted_count = revoked.max_admissions + 1;
    assert!(validate_grant(&revoked).is_err());
    let mut overflow = grant();
    overflow.not_before = i64::MAX;
    overflow.expires_at = i64::MIN;
    assert!(validate_grant(&overflow).is_err());
}

#[test]
fn request_and_challenge_nested_deadlines_account_roles_and_context_are_not_widened() {
    validate_request_for_grant(&request(), &grant()).unwrap();
    validate_challenge_for_request(&challenge(), &request()).unwrap();
    for field in 0..10 {
        let mut other = request();
        match field {
            0 => other.scope.server_instance_id = id(101),
            1 => other.scope.share_id = ShareId(id(102)),
            2 => other.scope.item_id = ObjectId(id(103)),
            3 => other.scope.kind = SharedItemKind::Secret,
            4 => other.target.user_id = UserId(id(104)),
            5 => other.target.device_id = anchor().device_id,
            6 => other.access_epoch += 1,
            7 => other.access_manifest_hash.0[0] ^= 1,
            8 => other.not_before = grant().not_before - 1,
            _ => {
                other.not_before = grant().expires_at - 1;
                other.expires_at = grant().expires_at + 1;
            }
        }
        assert!(validate_request_for_grant(&other, &grant()).is_err());
    }
    let mut ceiling = grant();
    ceiling.role_ceiling = SharingRole::Reader;
    let mut editor = request();
    editor.requested_role = SharingRole::Editor;
    assert!(validate_request_for_grant(&editor, &ceiling).is_err());
    let mut long = request();
    long.expires_at = long.not_before + MAX_REQUEST_LIFETIME_SECONDS + 1;
    assert!(validate_request(&long).is_err());
    let mut boundary = challenge();
    boundary.not_before = request().not_before - 1;
    assert!(validate_challenge_for_request(&boundary, &request()).is_err());
    boundary = challenge();
    boundary.not_before = request().expires_at - 1;
    boundary.expires_at = request().expires_at + 1;
    assert!(validate_challenge_for_request(&boundary, &request()).is_err());
}

#[test]
fn nonce_key_cipher_hash_signature_and_challenge_generation_lengths_are_exact() {
    for field in 0..9 {
        let mut other = challenge();
        match field {
            0 => other.generation = 0,
            1 => other.generation = i64::MAX as u64 + 1,
            2 => other.challenge_id = Uuid::nil(),
            3 => other.ephemeral_public_key.0.pop().map(|_| ()).unwrap(),
            4 => other.nonce.0.push(1),
            5 => other.ciphertext.0.pop().map(|_| ()).unwrap(),
            6 => other.request_hash = bytes(0, 32),
            7 => other.anchor_endorsement_hash.0.pop().map(|_| ()).unwrap(),
            _ => other.expires_at = other.not_before + MAX_CHALLENGE_LIFETIME_SECONDS + 1,
        }
        assert!(enrollment_challenge_message(&other).is_err());
    }
    let mut req = request();
    req.nonce.0.pop();
    assert!(validate_request(&req).is_err());
    let mut response = response();
    response.response_digest.0.push(1);
    assert!(validate_response(&response).is_err());
    let mut signed = signed_endorsement();
    signed.signature.0.pop();
    assert!(validate_signed_endorsement(&signed).is_err());
    assert!(enrollment_pairing_message(&[0; 32]).is_err());
}

#[test]
fn acceptance_commits_to_unique_other_grants_in_canonical_id_order() {
    let mut record = acceptance();
    let original = enrollment_acceptance_message(&record).unwrap();
    record.other_grant_successor_hashes.reverse();
    assert_eq!(enrollment_acceptance_message(&record).unwrap(), original);
    let mut changed = record.clone();
    changed.other_grant_successor_hashes[0].state_hash.0[0] ^= 1;
    assert_ne!(enrollment_acceptance_message(&changed).unwrap(), original);
    changed = record.clone();
    changed.other_grant_successor_hashes[0].grant_id = id(31);
    assert_ne!(enrollment_acceptance_message(&changed).unwrap(), original);
    record
        .other_grant_successor_hashes
        .push(record.other_grant_successor_hashes[0].clone());
    assert!(validate_acceptance(&record).is_err());
    record = acceptance();
    record.other_grant_successor_hashes[0].state_hash =
        record.consumed_grant_successor_hash.clone();
    assert!(validate_acceptance(&record).is_err());
    record = acceptance();
    record.other_grant_successor_hashes = (0..MAX_OTHER_GRANT_SUCCESSORS + 1)
        .map(|i| EnrollmentGrantSuccessorHash {
            grant_id: id(100 + i as u128),
            state_hash: bytes(1 + i as u8, 32),
        })
        .collect();
    assert!(validate_acceptance(&record).is_err());
}

fn transcript() -> OwnDeviceRequestState {
    OwnDeviceRequestState {
        grant_id: id(6),
        request: signed_request(),
        endorsement: signed_endorsement(),
        status: OwnDeviceRequestStatus::Accepted,
        challenge: Some(signed_challenge()),
        response: Some(signed_response()),
        acceptance: Some(signed_acceptance()),
    }
}

#[test]
fn request_status_transcript_does_not_allow_missing_proofs_or_cross_context_hashes() {
    validate_request_state(&transcript()).unwrap();
    for field in 0..8 {
        let mut state = transcript();
        match field {
            0 => state.challenge = None,
            1 => state.response = None,
            2 => state.acceptance = None,
            3 => state.challenge.as_mut().unwrap().challenge.request_hash.0[0] ^= 1,
            4 => state.response.as_mut().unwrap().response.request_hash.0[0] ^= 1,
            5 => {
                state
                    .acceptance
                    .as_mut()
                    .unwrap()
                    .acceptance
                    .challenge_hash
                    .0[0] ^= 1
            }
            6 => {
                state
                    .acceptance
                    .as_mut()
                    .unwrap()
                    .acceptance
                    .consumed_grant_state_hash
                    .0[0] ^= 1
            }
            _ => state.status = OwnDeviceRequestStatus::Pending,
        }
        assert!(validate_request_state(&state).is_err());
    }
}

#[test]
fn pages_reject_context_changes_duplicate_ids_nonadvancing_cursors_and_history_gaps() {
    let one = signed_grant();
    let page = OwnDevicesGrantPage {
        items: vec![one.clone()],
        next_after: None,
        has_more: false,
    };
    validate_grant_page(&page, &scope()).unwrap();
    let mut other = page.clone();
    other.items.push(one.clone());
    assert!(validate_grant_page(&other, &scope()).is_err());
    other = page.clone();
    other.has_more = true;
    assert!(validate_grant_page(&other, &scope()).is_err());
    other.next_after = Some(id(99));
    assert!(validate_grant_page(&other, &scope()).is_err());
    other.next_after = Some(one.grant.grant_id);
    validate_grant_page(&other, &scope()).unwrap();
    let mut changed = scope();
    changed.kind = SharedItemKind::Snippet;
    assert!(validate_grant_page(&page, &changed).is_err());
    let history = OwnDevicesGrantHistoryPage {
        states: vec![one.clone()],
        latest_revision: 1,
        has_more: false,
    };
    validate_grant_history_page(&history, &scope(), one.grant.grant_id, 0).unwrap();
    let mut bad = history.clone();
    bad.states[0].grant.grant_revision = 3;
    bad.states[0].grant.previous_grant_state_hash = bytes(0x71, 32);
    bad.latest_revision = 3;
    assert!(validate_grant_history_page(&bad, &scope(), one.grant.grant_id, 0).is_err());
    bad = history.clone();
    bad.latest_revision = 2;
    assert!(validate_grant_history_page(&bad, &scope(), one.grant.grant_id, 0).is_err());
    let page = OwnDeviceRequestPage {
        items: vec![transcript()],
        next_after: None,
        has_more: false,
    };
    validate_request_page(&page, &scope()).unwrap();
    let mut bad = page.clone();
    bad.items[0].request.request.scope.item_id = ObjectId(id(33));
    assert!(validate_request_page(&bad, &scope()).is_err());
    bad = page;
    bad.items = vec![transcript(); MAX_PAGE_SIZE + 1];
    assert!(validate_request_page(&bad, &scope()).is_err());
}

fn accept_request() -> AcceptOwnDeviceRequest {
    let owner = sharing::SharingMember {
        user_id: UserId(id(4)),
        device_id: DeviceId(id(5)),
        encryption_public_key: bytes(1, 32),
        signing_public_key: bytes(2, 32),
        role: SharingRole::Editor,
    };
    let member = sharing::SharingMember {
        user_id: target().user_id,
        device_id: target().device_id,
        encryption_public_key: target().encryption_public_key,
        signing_public_key: target().signing_public_key,
        role: SharingRole::Reader,
    };
    let access = sharing::SignedAccessManifest {
        manifest: sharing::AccessManifest {
            format: sharing::FORMAT,
            server_instance_id: scope().server_instance_id,
            share_id: scope().share_id,
            item_id: scope().item_id,
            owner_user_id: owner.user_id,
            owner_device_id: owner.device_id,
            revision: 4,
            access_epoch: 4,
            previous_manifest_hash: bytes(0x32, 32),
            kind: scope().kind,
            members: vec![owner.clone(), member.clone()],
        },
        signature: bytes(0x61, 64),
    };
    let revision = sharing::SharedRevision {
        signed: sharing::SignedSharingMutation {
            mutation: sharing::SharingMutation {
                context: sharing::SharingContext {
                    server_instance_id: scope().server_instance_id,
                    share_id: scope().share_id,
                    item_id: scope().item_id,
                    revision: 5,
                    access_epoch: 4,
                    kind: scope().kind,
                },
                mutation_id: MutationId(id(40)),
                base_revision: 4,
                manifest_revision: 4,
                manifest_hash: bytes(0x39, 32),
                writer_device_id: owner.device_id,
                previous_revision_hash: bytes(0x3d, 32),
                operation: sharing::SharingOperation::Put,
                body_hash: bytes(0x3e, 32),
            },
            signature: bytes(0x62, 64),
        },
        body: Some(sharing::SharedEncryptedBody {
            format: sharing::FORMAT,
            ciphertext: bytes(0x63, 16),
            nonce: bytes(0x64, 24),
            envelopes: [owner, member]
                .iter()
                .map(|m| sharing::SharedKeyEnvelope {
                    recipient_device_id: m.device_id,
                    ephemeral_public_key: bytes(0x65, 32),
                    nonce: bytes(0x66, 24),
                    ciphertext: bytes(0x67, 48),
                })
                .collect(),
        }),
    };
    let mut consumed = signed_grant();
    consumed.grant.grant_revision = 2;
    consumed.grant.previous_grant_state_hash = bytes(0x31, 32);
    consumed.grant.access_epoch = 4;
    consumed.grant.access_manifest_hash = bytes(0x39, 32);
    consumed.grant.admitted_count = 1;
    let others = [20, 21]
        .iter()
        .map(|i| {
            let mut other = consumed.clone();
            other.grant.grant_id = id(*i);
            other.grant.admitted_count = 0;
            other
        })
        .collect();
    AcceptOwnDeviceRequest {
        rotation: sharing::RotateShareAccessRequest { access, revision },
        acceptance: signed_acceptance(),
        consumed_grant_successor: consumed,
        other_grant_successors: others,
    }
}

#[test]
fn atomic_accept_transport_binds_successor_set_owner_kind_access_epoch_and_receipt() {
    let good = accept_request();
    validate_accept_request(&good).unwrap();
    for field in 0..8 {
        let mut other = good.clone();
        match field {
            0 => other.consumed_grant_successor.grant.scope.kind = SharedItemKind::Secret,
            1 => other.consumed_grant_successor.grant.access_epoch += 1,
            2 => other.consumed_grant_successor.grant.owner_device_id = DeviceId(id(99)),
            3 => other.consumed_grant_successor.grant.access_manifest_hash.0[0] ^= 1,
            4 => other.other_grant_successors.pop().map(|_| ()).unwrap(),
            5 => other.other_grant_successors[0].grant.grant_id = id(99),
            6 => other.other_grant_successors[0]
                .grant
                .anchor
                .signing_public_key
                .0
                .pop()
                .map(|_| ())
                .unwrap(),
            _ => {
                other.other_grant_successors[0].grant.grant_id =
                    other.consumed_grant_successor.grant.grant_id
            }
        }
        assert!(validate_accept_request(&other).is_err());
    }
    strict_schema(&good, "consumed_grant_successor");
    strict_schema(
        &SubmitOwnDeviceRequest {
            grant_id: id(6),
            request: signed_request(),
            endorsement: signed_endorsement(),
        },
        "endorsement",
    );
    strict_schema(
        &PublishOwnDevicesGrantRequest {
            grant: signed_grant(),
        },
        "grant",
    );
    strict_schema(
        &PublishOwnDeviceChallengeRequest {
            challenge: signed_challenge(),
        },
        "challenge",
    );
    strict_schema(
        &SubmitOwnDeviceChallengeResponseRequest {
            response: signed_response(),
        },
        "response",
    );
    let result = OwnDeviceAcceptanceResult {
        state: sharing::SharedItemState {
            access: good.rotation.access.clone(),
            revision: good.rotation.revision.clone(),
        },
        acceptance: good.acceptance,
        consumed_grant_successor: good.consumed_grant_successor,
        other_grant_successors: good.other_grant_successors,
    };
    validate_acceptance_result(&result).unwrap();
    strict_schema(&result, "state");
}

// Complete golden bytes generated once by an independent Python struct encoder.
const GRANT_VECTOR: &str = concat!(
    "636f6e736f6c6563727970742f73686172696e672f656e726f6c6c6d656e742f76312f6772616e740000010000000000",
    "000000000000000000000100000000000000000000000000000002000000000000000000000000000000030100000000",
    "000000000000000000000004000000000000000000000000000000050000000000000000000000000000000600000000",
    "000000010000000000000000000000000000000000000000000000000000000000000000010000000000000000000000",
    "000000000700000000000000000000000000000008111111111111111111111111111111111111111111111111111111",
    "111111111112121212121212121212121212121212121212121212121212121212121212123232323232323232323232",
    "3232323232323232323232323232323232323232320000000000000003020200000000713fb30000000000713fc11000",
    "00000200000000",
);
const REQUEST_VECTOR: &str = concat!(
    "636f6e736f6c6563727970742f73686172696e672f656e726f6c6c6d656e742f76312f72657175657374000001000000",
    "000000000000000000000000010000000000000000000000000000000200000000000000000000000000000003010000",
    "000000000000000000000000000a31313131313131313131313131313131313131313131313131313131313131313232",
    "323232323232323232323232323232323232323232323232323232323232000000000000000300000000000000000000",
    "000000000007000000000000000000000000000000092121212121212121212121212121212121212121212121212121",
    "212121212121222222222222222222222222222222222222222222222222222222222222222201444444444444444444",
    "444444444444444444444444444444444444444444444400000000713fb30a00000000713fb558",
);
const ENDORSEMENT_VECTOR: &str = concat!(
    "636f6e736f6c6563727970742f73686172696e672f656e726f6c6c6d656e742f76312f656e646f7273656d656e740000",
    "013333333333333333333333333333333333333333333333333333333333333333000000000000000000000000000000",
    "08",
);
const CHALLENGE_VECTOR: &str = concat!(
    "636f6e736f6c6563727970742f73686172696e672f656e726f6c6c6d656e742f76312f6368616c6c656e676500000133",
    "333333333333333333333333333333333333333333333333333333333333333434343434343434343434343434343434",
    "3434343434343434343434343434340000000000000000000000000000000b000000000000000100000000713fb31400",
    "000000713fb3fa4141414141414141414141414141414141414141414141414141414141414141424242424242424242",
    "424242424242424242424242424242434343434343434343434343434343434343434343434343434343434343434343",
    "434343434343434343434343434343",
);
const RESPONSE_VECTOR: &str = concat!(
    "636f6e736f6c6563727970742f73686172696e672f656e726f6c6c6d656e742f76312f726573706f6e73650000013333",
    "333333333333333333333333333333333333333333333333333333333333353535353535353535353535353535353535",
    "35353535353535353535353535353636363636363636363636363636363636363636363636363636363636363636",
);
const ACCEPTANCE_VECTOR: &str = concat!(
    "636f6e736f6c6563727970742f73686172696e672f656e726f6c6c6d656e742f76312f616363657074616e6365000001",
    "333333333333333333333333333333333333333333333333333333333333333334343434343434343434343434343434",
    "343434343434343434343434343434343535353535353535353535353535353535353535353535353535353535353535",
    "373737373737373737373737373737373737373737373737373737373737373731313131313131313131313131313131",
    "313131313131313131313131313131313939393939393939393939393939393939393939393939393939393939393939",
    "3a3a3a3a3a3a3a3a3a3a3a3a3a3a3a3a3a3a3a3a3a3a3a3a3a3a3a3a3a3a3a3a38383838383838383838383838383838",
    "3838383838383838383838383838383800000002000000000000000000000000000000143b3b3b3b3b3b3b3b3b3b3b3b",
    "3b3b3b3b3b3b3b3b3b3b3b3b3b3b3b3b3b3b3b3b000000000000000000000000000000153c3c3c3c3c3c3c3c3c3c3c3c",
    "3c3c3c3c3c3c3c3c3c3c3c3c3c3c3c3c3c3c3c3c",
);
const GRANT_HASH_VECTOR: &str = concat!(
    "636f6e736f6c6563727970742f73686172696e672f656e726f6c6c6d656e742f76312f6772616e742d6861736800636f",
    "6e736f6c6563727970742f73686172696e672f656e726f6c6c6d656e742f76312f6772616e7400000100000000000000",
    "000000000000000001000000000000000000000000000000020000000000000000000000000000000301000000000000",
    "000000000000000000040000000000000000000000000000000500000000000000000000000000000006000000000000",
    "000100000000000000000000000000000000000000000000000000000000000000000100000000000000000000000000",
    "000007000000000000000000000000000000081111111111111111111111111111111111111111111111111111111111",
    "111111121212121212121212121212121212121212121212121212121212121212121232323232323232323232323232",
    "323232323232323232323232323232323232320000000000000003020200000000713fb30000000000713fc110000000",
    "020000000051515151515151515151515151515151515151515151515151515151515151515151515151515151515151",
    "515151515151515151515151515151515151515151",
);
const REQUEST_HASH_VECTOR: &str = concat!(
    "636f6e736f6c6563727970742f73686172696e672f656e726f6c6c6d656e742f76312f726571756573742d6861736800",
    "636f6e736f6c6563727970742f73686172696e672f656e726f6c6c6d656e742f76312f72657175657374000001000000",
    "000000000000000000000000010000000000000000000000000000000200000000000000000000000000000003010000",
    "000000000000000000000000000a31313131313131313131313131313131313131313131313131313131313131313232",
    "323232323232323232323232323232323232323232323232323232323232000000000000000300000000000000000000",
    "000000000007000000000000000000000000000000092121212121212121212121212121212121212121212121212121",
    "212121212121222222222222222222222222222222222222222222222222222222222222222201444444444444444444",
    "444444444444444444444444444444444444444444444400000000713fb30a00000000713fb558525252525252525252",
    "525252525252525252525252525252525252525252525252525252525252525252525252525252525252525252525252",
    "52525252525252",
);
const ENDORSEMENT_HASH_VECTOR: &str = concat!(
    "636f6e736f6c6563727970742f73686172696e672f656e726f6c6c6d656e742f76312f656e646f7273656d656e742d68",
    "61736800636f6e736f6c6563727970742f73686172696e672f656e726f6c6c6d656e742f76312f656e646f7273656d65",
    "6e7400000133333333333333333333333333333333333333333333333333333333333333330000000000000000000000",
    "000000000853535353535353535353535353535353535353535353535353535353535353535353535353535353535353",
    "535353535353535353535353535353535353535353",
);
const CHALLENGE_HASH_VECTOR: &str = concat!(
    "636f6e736f6c6563727970742f73686172696e672f656e726f6c6c6d656e742f76312f6368616c6c656e67652d686173",
    "6800636f6e736f6c6563727970742f73686172696e672f656e726f6c6c6d656e742f76312f6368616c6c656e67650000",
    "013333333333333333333333333333333333333333333333333333333333333333343434343434343434343434343434",
    "34343434343434343434343434343434340000000000000000000000000000000b000000000000000100000000713fb3",
    "1400000000713fb3fa414141414141414141414141414141414141414141414141414141414141414142424242424242",
    "424242424242424242424242424242424243434343434343434343434343434343434343434343434343434343434343",
    "434343434343434343434343434343434354545454545454545454545454545454545454545454545454545454545454",
    "545454545454545454545454545454545454545454545454545454545454545454",
);
const RESPONSE_HASH_VECTOR: &str = concat!(
    "636f6e736f6c6563727970742f73686172696e672f656e726f6c6c6d656e742f76312f726573706f6e73652d68617368",
    "00636f6e736f6c6563727970742f73686172696e672f656e726f6c6c6d656e742f76312f726573706f6e736500000133",
    "333333333333333333333333333333333333333333333333333333333333333535353535353535353535353535353535",
    "353535353535353535353535353535363636363636363636363636363636363636363636363636363636363636363655",
    "555555555555555555555555555555555555555555555555555555555555555555555555555555555555555555555555",
    "555555555555555555555555555555",
);
const ACCEPTANCE_HASH_VECTOR: &str = concat!(
    "636f6e736f6c6563727970742f73686172696e672f656e726f6c6c6d656e742f76312f616363657074616e63652d6861",
    "736800636f6e736f6c6563727970742f73686172696e672f656e726f6c6c6d656e742f76312f616363657074616e6365",
    "000001333333333333333333333333333333333333333333333333333333333333333334343434343434343434343434",
    "343434343434343434343434343434343434343535353535353535353535353535353535353535353535353535353535",
    "353535373737373737373737373737373737373737373737373737373737373737373731313131313131313131313131",
    "313131313131313131313131313131313131313939393939393939393939393939393939393939393939393939393939",
    "3939393a3a3a3a3a3a3a3a3a3a3a3a3a3a3a3a3a3a3a3a3a3a3a3a3a3a3a3a3a3a3a3a38383838383838383838383838",
    "3838383838383838383838383838383838383800000002000000000000000000000000000000143b3b3b3b3b3b3b3b3b",
    "3b3b3b3b3b3b3b3b3b3b3b3b3b3b3b3b3b3b3b3b3b3b3b000000000000000000000000000000153c3c3c3c3c3c3c3c3c",
    "3c3c3c3c3c3c3c3c3c3c3c3c3c3c3c3c3c3c3c3c3c3c3c56565656565656565656565656565656565656565656565656",
    "565656565656565656565656565656565656565656565656565656565656565656565656565656",
);
const HEADER_VECTOR: &str = concat!(
    "636f6e736f6c6563727970742f73686172696e672f656e726f6c6c6d656e742f76312f6368616c6c656e67652d686561",
    "646572000001333333333333333333333333333333333333333333333333333333333333333334343434343434343434",
    "343434343434343434343434343434343434343434340000000000000000000000000000000b00000000000000010000",
    "0000713fb31400000000713fb3fa41414141414141414141414141414141414141414141414141414141414141414242",
    "42424242424242424242424242424242424242424242",
);
const AAD_VECTOR: &str = concat!(
    "636f6e736f6c6563727970742f73686172696e672f656e726f6c6c6d656e742f76312f6368616c6c656e67652d616164",
    "00636f6e736f6c6563727970742f73686172696e672f656e726f6c6c6d656e742f76312f6368616c6c656e67652d6865",
    "616465720000013333333333333333333333333333333333333333333333333333333333333333343434343434343434",
    "34343434343434343434343434343434343434343434340000000000000000000000000000000b000000000000000100",
    "000000713fb31400000000713fb3fa414141414141414141414141414141414141414141414141414141414141414142",
    "4242424242424242424242424242424242424242424242",
);
const KEY_INFO_VECTOR: &str = concat!(
    "636f6e736f6c6563727970742f73686172696e672f656e726f6c6c6d656e742f76312f6368616c6c656e67652d6b6579",
    "00636f6e736f6c6563727970742f73686172696e672f656e726f6c6c6d656e742f76312f6368616c6c656e67652d6865",
    "616465720000013333333333333333333333333333333333333333333333333333333333333333343434343434343434",
    "34343434343434343434343434343434343434343434340000000000000000000000000000000b000000000000000100",
    "000000713fb31400000000713fb3fa414141414141414141414141414141414141414141414141414141414141414142",
    "424242424242424242424242424242424242424242424200000000000000000000000000000007000000000000000000",
    "000000000000092121212121212121212121212121212121212121212121212121212121212121222222222222222222",
    "2222222222222222222222222222222222222222222222",
);
const PAIRING_VECTOR: &str = concat!(
    "636f6e736f6c6563727970742f73686172696e672f656e726f6c6c6d656e742f76312f70616972696e67003333333333",
    "333333333333333333333333333333333333333333333333333333",
);
