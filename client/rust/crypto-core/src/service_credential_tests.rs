//! Real existing-primitive E2EE tests; synthetic selected bytes, no lookup/grant.
use super::*;
const SECRET: &[u8] = b"SYNTHETIC_SELECTED_SERVICE_PASSWORD_NEVER_WIRE_PLAINTEXT";
fn binding(owner: &DeviceSecretKeys, service: &DeviceSecretKeys) -> ServiceCredentialBinding {
    ServiceCredentialBinding {
        version: 1,
        owner_device_id: Uuid::new_v4().to_string(),
        owner_signing_key_sha256: hash(&owner.public_keys().signing),
        tenant_kind: ServiceTenantKind::LocalProfile,
        tenant_id: Uuid::new_v4().to_string(),
        service_id: Uuid::new_v4().to_string(),
        delegation_digest: "a".repeat(64),
        policy_revision: 1,
        revocation_epoch: 1,
        credential_id: Uuid::new_v4().to_string(),
        credential_revision: 3,
        kind: ServiceCredentialKind::Password,
        recipient_key_sha256: hash(&service.public_keys().encryption),
        recipient_key_epoch: 1,
        recipient_certificate_epoch: 1,
        delegation_expires_at: 1000,
    }
}
fn sealed() -> (
    DeviceSecretKeys,
    DeviceSecretKeys,
    ServiceCredentialBinding,
    ServiceCredentialCapsule,
) {
    let owner = DeviceSecretKeys::generate().unwrap();
    let service = DeviceSecretKeys::generate().unwrap();
    let scope = binding(&owner, &service);
    let capsule = seal_service_credential(
        SelectedServiceCredential::new(scope.clone(), Zeroizing::new(SECRET.to_vec())).unwrap(),
        &service.public_keys().encryption,
        &owner,
        100,
        200,
    )
    .unwrap();
    (owner, service, scope, capsule)
}
#[test]
fn service_capsule_roundtrip_is_owner_authenticated_and_plaintext_never_json_or_debug() {
    let (owner, service, scope, capsule) = sealed();
    let raw = capsule.to_json().unwrap();
    assert!(!raw.windows(SECRET.len()).any(|v| v == SECRET));
    let wire = ServiceCredentialCapsule::from_json(&raw).unwrap();
    let opened =
        open_service_credential(&wire, &scope, &owner.public_keys().signing, &service, 101)
            .unwrap();
    assert_eq!(
        opened.with_secret(&scope, 101, |s| s.to_vec()).unwrap(),
        SECRET
    );
    let debug = format!("{opened:?} {wire:?}");
    assert!(debug.contains("REDACTED"));
    assert!(!debug.contains(std::str::from_utf8(SECRET).unwrap()));
}
#[test]
fn service_capsule_wrong_recipient_or_owner_never_yields_material() {
    let (owner, service, scope, capsule) = sealed();
    let other = DeviceSecretKeys::generate().unwrap();
    assert!(
        open_service_credential(&capsule, &scope, &owner.public_keys().signing, &other, 101)
            .is_err()
    );
    assert!(
        open_service_credential(
            &capsule,
            &scope,
            &other.public_keys().signing,
            &service,
            101
        )
        .is_err()
    );
}
#[test]
fn service_capsule_tenant_service_credential_revision_policy_and_recipient_epoch_are_exact() {
    let (owner, service, scope, capsule) = sealed();
    for cause in 0..6 {
        let mut changed = scope.clone();
        match cause {
            0 => changed.tenant_id = Uuid::new_v4().to_string(),
            1 => changed.service_id = Uuid::new_v4().to_string(),
            2 => changed.credential_revision += 1,
            3 => changed.policy_revision += 1,
            4 => changed.revocation_epoch += 1,
            _ => changed.recipient_key_epoch += 1,
        };
        assert!(
            open_service_credential(
                &capsule,
                &changed,
                &owner.public_keys().signing,
                &service,
                101
            )
            .is_err()
        );
    }
}
#[test]
fn service_capsule_expiry_is_checked_on_open_and_each_narrow_use() {
    let (owner, service, scope, capsule) = sealed();
    for now in [99, 200, MAX_TIME + 1] {
        assert!(
            open_service_credential(
                &capsule,
                &scope,
                &owner.public_keys().signing,
                &service,
                now
            )
            .is_err()
        );
    }
    let opened = open_service_credential(
        &capsule,
        &scope,
        &owner.public_keys().signing,
        &service,
        101,
    )
    .unwrap();
    assert!(opened.with_secret(&scope, 200, |_| ()).is_err());
    assert!(opened.with_secret(&scope, 99, |_| ()).is_err());
}
#[test]
fn service_capsule_ciphertext_nonce_and_ephemeral_tampering_are_rejected() {
    let (owner, service, scope, capsule) = sealed();
    let raw = capsule.to_json().unwrap();
    for cause in 0..3 {
        let mut changed = ServiceCredentialCapsule::from_json(&raw).unwrap();
        match cause {
            0 => {
                let mut b = changed.ciphertext.as_slice().to_vec();
                b[0] ^= 1;
                changed.ciphertext = Bytes::new(b)
            }
            1 => changed.nonce = Bytes::new(vec![7; 24]),
            _ => changed.ephemeral_public = Bytes::new(vec![8; 32]),
        };
        assert!(
            open_service_credential(
                &changed,
                &scope,
                &owner.public_keys().signing,
                &service,
                101
            )
            .is_err()
        );
    }
}
#[test]
fn service_capsule_valid_new_owner_signature_cannot_move_old_ciphertext_between_scopes() {
    let (owner, service, mut scope, mut capsule) = sealed();
    scope.credential_revision += 1;
    capsule.binding = scope.clone();
    capsule.signature = Bytes::new(
        owner
            .sign_message(&capsule.signing_bytes().unwrap())
            .to_vec(),
    );
    assert!(
        open_service_credential(
            &capsule,
            &scope,
            &owner.public_keys().signing,
            &service,
            101
        )
        .is_err(),
        "AAD/KDF binding must fail even after re-signing changed metadata"
    );
}
#[test]
fn service_capsule_future_duplicate_unknown_and_oversize_wire_fail_closed() {
    let (_, _, _, capsule) = sealed();
    let raw = capsule.to_json().unwrap();
    let mut value: serde_json::Value = serde_json::from_slice(&raw).unwrap();
    value["version"] = serde_json::json!(2);
    assert!(ServiceCredentialCapsule::from_json(&serde_json::to_vec(&value).unwrap()).is_err());
    value["version"] = serde_json::json!(1);
    value["approved"] = serde_json::json!(true);
    assert!(ServiceCredentialCapsule::from_json(&serde_json::to_vec(&value).unwrap()).is_err());
    let text = std::str::from_utf8(&raw).unwrap();
    let duplicate = text.replacen("{", "{\"version\":1,", 1);
    assert!(ServiceCredentialCapsule::from_json(duplicate.as_bytes()).is_err());
    assert!(ServiceCredentialCapsule::from_json(&vec![b' '; MAX_WIRE + 1]).is_err());
}
#[test]
fn service_capsule_separate_key_low_order_and_bad_source_pin_reject_before_delivery() {
    let owner = DeviceSecretKeys::generate().unwrap();
    let service = DeviceSecretKeys::generate().unwrap();
    for key in [[0; 32], owner.public_keys().encryption] {
        let mut scope = binding(&owner, &service);
        scope.recipient_key_sha256 = hash(&key);
        assert!(
            seal_service_credential(
                SelectedServiceCredential::new(scope, Zeroizing::new(SECRET.to_vec())).unwrap(),
                &key,
                &owner,
                100,
                200
            )
            .is_err()
        );
    }
    let mut scope = binding(&owner, &service);
    scope.owner_signing_key_sha256 = "f".repeat(64);
    assert!(
        seal_service_credential(
            SelectedServiceCredential::new(scope, Zeroizing::new(SECRET.to_vec())).unwrap(),
            &service.public_keys().encryption,
            &owner,
            100,
            200
        )
        .is_err()
    );
}
#[test]
fn service_capsule_selected_material_and_validity_are_bounded_and_redacted() {
    let owner = DeviceSecretKeys::generate().unwrap();
    let service = DeviceSecretKeys::generate().unwrap();
    let scope = binding(&owner, &service);
    for size in [0, MAX_SECRET + 1] {
        assert!(
            SelectedServiceCredential::new(scope.clone(), Zeroizing::new(vec![1; size])).is_err()
        );
    }
    let selected =
        SelectedServiceCredential::new(scope.clone(), Zeroizing::new(vec![1; MAX_SECRET])).unwrap();
    assert!(format!("{selected:?}").contains("REDACTED"));
    let capsule = seal_service_credential(
        selected,
        &service.public_keys().encryption,
        &owner,
        100,
        200,
    )
    .unwrap();
    assert!(capsule.to_json().unwrap().len() <= MAX_WIRE);
    assert!(
        seal_service_credential(
            SelectedServiceCredential::new(scope, Zeroizing::new(vec![1])).unwrap(),
            &service.public_keys().encryption,
            &owner,
            100,
            100
        )
        .is_err()
    );
}
#[test]
fn service_capsule_purpose_confusion_and_noncanonical_metadata_never_reuse_signatures() {
    let (owner, service, mut scope, mut capsule) = sealed();
    capsule.signature = Bytes::new(
        owner
            .sign_message(b"ConsoleCrypt JobManifest v1\0")
            .to_vec(),
    );
    assert!(
        open_service_credential(
            &capsule,
            &scope,
            &owner.public_keys().signing,
            &service,
            101
        )
        .is_err()
    );
    scope.tenant_kind = ServiceTenantKind::Team;
    assert!(scope.validate().is_err());
    scope.tenant_kind = ServiceTenantKind::PersonalAccount;
    scope.credential_id = "AAAAAAAA-AAAA-4AAA-8AAA-AAAAAAAAAAAA".into();
    assert!(scope.validate().is_err());
}
