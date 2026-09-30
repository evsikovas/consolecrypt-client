use cc_protocol::sharing::*;
use cc_protocol::{Bytes, DeviceId, MutationId, ObjectId, ShareId, UserId};
use uuid::Uuid;

// Synthetic public metadata, not cryptographic private-key fixtures.
fn member() -> SharingMember {
    SharingMember {
        user_id: UserId::new(),
        device_id: DeviceId::new(),
        encryption_public_key: Bytes::from([11; 32]),
        signing_public_key: Bytes::from([12; 32]),
        role: SharingRole::Editor,
    }
}

fn manifest() -> AccessManifest {
    let owner = member();
    AccessManifest {
        format: FORMAT,
        server_instance_id: Uuid::now_v7(),
        share_id: ShareId::new(),
        item_id: ObjectId::new(),
        owner_user_id: owner.user_id,
        owner_device_id: owner.device_id,
        revision: 1,
        access_epoch: 1,
        previous_manifest_hash: Bytes::from([0; 32]),
        kind: SharedItemKind::Host,
        members: vec![owner],
    }
}

fn context(a: &AccessManifest) -> SharingContext {
    SharingContext {
        server_instance_id: a.server_instance_id,
        share_id: a.share_id,
        item_id: a.item_id,
        revision: 1,
        access_epoch: 1,
        kind: a.kind,
    }
}

fn envelope(device_id: DeviceId) -> SharedKeyEnvelope {
    SharedKeyEnvelope {
        recipient_device_id: device_id,
        ephemeral_public_key: Bytes::from([13; 32]),
        nonce: Bytes::from([14; 24]),
        ciphertext: Bytes::from([15; 48]),
    }
}

#[test]
fn object_aad_has_a_fixed_cross_platform_vector() {
    let c = SharingContext {
        server_instance_id: Uuid::parse_str("00112233-4455-6677-8899-aabbccddeeff").unwrap(),
        share_id: "10112233-4455-6677-8899-aabbccddeeff".parse().unwrap(),
        item_id: "20112233-4455-6677-8899-aabbccddeeff".parse().unwrap(),
        revision: 1,
        access_epoch: 2,
        kind: SharedItemKind::Snippet,
    };
    let actual: String = sharing_object_aad(&c)
        .unwrap()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    assert_eq!(
        actual,
        concat!(
            "636f6e736f6c6563727970742f73686172696e672f76312f6f626a6563742d61616400",
            "0001",
            "00112233445566778899aabbccddeeff",
            "10112233445566778899aabbccddeeff",
            "20112233445566778899aabbccddeeff",
            "0000000000000001",
            "0000000000000002",
            "03"
        )
    );
}

#[test]
fn full_state_rejects_missing_envelopes_reader_writes_and_wrong_body_operation() {
    let mut a = manifest();
    let mut reader = member();
    reader.role = SharingRole::Reader;
    a.members.push(reader.clone());
    let header = SharingMutation {
        context: context(&a),
        mutation_id: MutationId::new(),
        base_revision: 0,
        manifest_revision: 1,
        manifest_hash: Bytes::from([18; 32]),
        writer_device_id: a.owner_device_id,
        previous_revision_hash: Bytes::from([0; 32]),
        operation: SharingOperation::Put,
        body_hash: Bytes::from([19; 32]),
    };
    let mut state = SharedItemState {
        access: SignedAccessManifest {
            manifest: a.clone(),
            signature: Bytes::from([16; 64]),
        },
        revision: SharedRevision {
            signed: SignedSharingMutation {
                mutation: header,
                signature: Bytes::from([17; 64]),
            },
            body: Some(SharedEncryptedBody {
                format: FORMAT,
                ciphertext: Bytes::from([1; 16]),
                nonce: Bytes::from([2; 24]),
                envelopes: a.members.iter().map(|m| envelope(m.device_id)).collect(),
            }),
        },
    };
    validate_state(&state).unwrap();
    state.revision.body.as_mut().unwrap().envelopes.pop();
    assert_eq!(
        validate_state(&state).unwrap_err(),
        SharingValidationError("envelope_members")
    );
    state
        .revision
        .body
        .as_mut()
        .unwrap()
        .envelopes
        .push(envelope(reader.device_id));
    state.revision.signed.mutation.writer_device_id = reader.device_id;
    assert_eq!(
        validate_state(&state).unwrap_err(),
        SharingValidationError("writer_role")
    );
    state.revision.signed.mutation.writer_device_id = a.owner_device_id;
    state.revision.body = None;
    assert_eq!(
        validate_state(&state).unwrap_err(),
        SharingValidationError("operation_body")
    );
}

#[test]
fn serialization_roundtrips_only_public_metadata_and_ciphertext() {
    let m = manifest();
    let signed = SignedAccessManifest {
        manifest: m.clone(),
        signature: Bytes::from([16; 64]),
    };
    let json = serde_json::to_string(&signed).unwrap();
    assert_eq!(
        serde_json::from_str::<SignedAccessManifest>(&json).unwrap(),
        signed
    );
    assert!(!json.contains("private_key"));
    assert!(!format!("{signed:?}").contains("16, 16"));
    assert_eq!(
        sharing_manifest_message(&m).unwrap().len(),
        labels::MANIFEST.len() + 2 + 16 * 5 + 8 * 2 + 32 + 1 + 4 + 97
    );
}

#[test]
fn canonical_order_is_stable_but_duplicate_grants_are_not_normalized() {
    let mut m = manifest();
    m.members.push(member());
    let bytes = sharing_manifest_message(&m).unwrap();
    m.members.reverse();
    assert_eq!(sharing_manifest_message(&m).unwrap(), bytes);
    let mut changed = m.clone();
    changed.members[0].role = SharingRole::Reader;
    if changed.members[0].device_id == changed.owner_device_id {
        assert!(sharing_manifest_message(&changed).is_err());
    } else {
        assert_ne!(sharing_manifest_message(&changed).unwrap(), bytes);
    }
    m.members.push(m.members[0].clone());
    assert_eq!(
        sharing_manifest_message(&m).unwrap_err(),
        SharingValidationError("duplicate_device")
    );
}

#[test]
fn cross_instance_item_epoch_and_recipient_keys_change_aad() {
    let m = manifest();
    let c = context(&m);
    let aad = sharing_object_aad(&c).unwrap();
    for field in 0..6 {
        let mut other = c.clone();
        match field {
            0 => other.server_instance_id = Uuid::now_v7(),
            1 => other.share_id = ShareId::new(),
            2 => other.item_id = ObjectId::new(),
            3 => other.revision += 1,
            4 => other.access_epoch += 1,
            _ => other.kind = SharedItemKind::Snippet,
        }
        assert_ne!(sharing_object_aad(&other).unwrap(), aad);
    }
    let recipient = &m.members[0];
    let eaad = sharing_envelope_aad(&c, recipient, &[13; 32]).unwrap();
    let mut other = recipient.clone();
    other.signing_public_key.0[0] ^= 1;
    assert_ne!(sharing_envelope_aad(&c, &other, &[13; 32]).unwrap(), eaad);
    other = recipient.clone();
    other.user_id = UserId::new();
    assert_ne!(sharing_envelope_aad(&c, &other, &[13; 32]).unwrap(), eaad);
    assert_ne!(
        sharing_envelope_aad(&c, recipient, &[17; 32]).unwrap(),
        eaad
    );
    assert_ne!(eaad, aad);
}

#[test]
fn malformed_grants_and_oversized_payloads_fail_before_signing() {
    let good = manifest();
    for field in 0..7 {
        let mut m = good.clone();
        match field {
            0 => m.format += 1,
            1 => m.members.clear(),
            2 => m.members[0].role = SharingRole::Reader,
            3 => m.members[0].signing_public_key.0.pop().map(|_| ()).unwrap(),
            4 => m.previous_manifest_hash.0.pop().map(|_| ()).unwrap(),
            5 => m.access_epoch = 2,
            _ => m.server_instance_id = Uuid::nil(),
        }
        assert!(sharing_manifest_message(&m).is_err());
    }
    let mut body = SharedEncryptedBody {
        format: FORMAT,
        ciphertext: Bytes::from([1; 16]),
        nonce: Bytes::from([2; 24]),
        envelopes: vec![envelope(good.owner_device_id)],
    };
    assert!(sharing_body_message(&body).is_ok());
    body.ciphertext = Bytes::new(vec![1; MAX_CIPHERTEXT_BYTES + 1]);
    assert_eq!(
        sharing_body_message(&body).unwrap_err(),
        SharingValidationError("ciphertext")
    );
    body.ciphertext = Bytes::from([1; 16]);
    body.envelopes.push(body.envelopes[0].clone());
    assert_eq!(
        sharing_body_message(&body).unwrap_err(),
        SharingValidationError("duplicate_envelope")
    );
}

#[test]
fn body_encoding_commits_to_all_envelopes_and_ciphertext() {
    let mut body = SharedEncryptedBody {
        format: FORMAT,
        ciphertext: Bytes::from([1; 16]),
        nonce: Bytes::from([2; 24]),
        envelopes: vec![envelope(DeviceId::new()), envelope(DeviceId::new())],
    };
    let original = sharing_body_message(&body).unwrap();
    body.envelopes.reverse();
    assert_eq!(sharing_body_message(&body).unwrap(), original);
    for field in 0..4 {
        let mut other = body.clone();
        match field {
            0 => other.ciphertext.0[0] ^= 1,
            1 => other.nonce.0[0] ^= 1,
            2 => other.envelopes[0].ciphertext.0[0] ^= 1,
            _ => other.envelopes[0].ephemeral_public_key.0[0] ^= 1,
        }
        assert_ne!(sharing_body_message(&other).unwrap(), original);
    }
}

#[test]
fn revision_cas_and_signed_tombstones_are_structurally_unambiguous() {
    let a = manifest();
    let mut m = SharingMutation {
        context: context(&a),
        mutation_id: MutationId::new(),
        base_revision: 0,
        manifest_revision: 1,
        manifest_hash: Bytes::from([18; 32]),
        writer_device_id: a.owner_device_id,
        previous_revision_hash: Bytes::from([0; 32]),
        operation: SharingOperation::Put,
        body_hash: Bytes::from([19; 32]),
    };
    let original = sharing_mutation_message(&m).unwrap();
    m.base_revision = 1;
    assert!(sharing_mutation_message(&m).is_err());
    m.context.revision = 2;
    m.previous_revision_hash = Bytes::from([20; 32]);
    m.operation = SharingOperation::Delete;
    assert!(sharing_mutation_message(&m).is_err());
    m.body_hash = Bytes::from([0; 32]);
    assert_ne!(sharing_mutation_message(&m).unwrap(), original);
    m.context.revision = i64::MAX;
    m.base_revision = i64::MAX;
    assert!(sharing_mutation_message(&m).is_err());
}
