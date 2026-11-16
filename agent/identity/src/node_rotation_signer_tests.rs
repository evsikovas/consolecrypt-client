fn rotation_owner<T: Statement>(body: T, seed: u8) -> Signed<T> {
    Signed {
        signature: SignatureBytes::from_bytes(
            SigningKey::from_bytes(&[seed; 32])
                .sign(&body.signing_bytes().unwrap())
                .to_bytes(),
        ),
        body,
    }
}
fn rotation_fixture() -> (Temp, OwnerPin, PreparedNodeKeyPublic, PreparedRotationInput) {
    let root = Temp::new();
    let active = root.provision();
    let prepared = prepare_node_rotation(&root.node(), id(), active.key_fingerprint).unwrap();
    let time = |n| Timestamp::new(n).unwrap();
    let context = Context {
        tenant: Tenant::LocalProfile {
            id: LocalProfileId::new("11111111-1111-4111-8111-111111111111").unwrap(),
        },
        deployment_id: DeploymentId::new("22222222-2222-4222-8222-222222222222").unwrap(),
    };
    let pin = OwnerPin {
        context: context.clone(),
        key: PublicKey::from_bytes(SigningKey::from_bytes(&[11; 32]).verifying_key().to_bytes()),
        epoch: 1,
        checkpoint: 2,
        expires_at: time(10000),
    };
    let previous = rotation_owner(
        Certificate {
            header: Header {
                version: 1,
                context,
                owner_epoch: 1,
                checkpoint: 2,
            },
            agent_id: id(),
            key_epoch: 1,
            agent_key: active.public_key,
            scopes: Scopes(vec![Scope::TelemetryRead]),
            capabilities: Capabilities {
                version: 1,
                scopes: Scopes(vec![Scope::TelemetryRead]),
            },
            origin: CertificateOrigin::Enrollment {
                invitation_id: InvitationId::new("33333333-3333-4333-8333-333333333333").unwrap(),
                claim_hash: Hash::from_bytes([33; 32]),
                key_fingerprint: active.key_fingerprint,
            },
            not_before: time(1000),
            expires_at: time(1600),
        },
        11,
    );
    let certificate = rotation_owner(
        Certificate {
            header: Header {
                checkpoint: 3,
                ..previous.body.header.clone()
            },
            agent_id: id(),
            key_epoch: 2,
            agent_key: prepared.new_public_key,
            scopes: previous.body.scopes.clone(),
            capabilities: previous.body.capabilities.clone(),
            origin: CertificateOrigin::Rotation {
                previous_certificate: previous.body.digest().unwrap(),
                previous_key_epoch: 1,
            },
            not_before: time(1000),
            expires_at: time(1500),
        },
        11,
    );
    (
        root,
        pin,
        prepared,
        PreparedRotationInput {
            previous_certificate: previous,
            certificate,
        },
    )
}
fn key_bytes(root: &Temp) -> std::collections::BTreeMap<String, Hash> {
    fs::read_dir(root.node())
        .unwrap()
        .map(|v| {
            let p = v.unwrap().path();
            let b = Zeroizing::new(fs::read(&p).unwrap());
            (
                p.file_name().unwrap().to_str().unwrap().to_owned(),
                digest(&b),
            )
        })
        .collect()
}
#[test]
fn prepared_signer_is_readonly_typed_and_uses_fresh_nonce_for_explicit_calls() {
    let (root, pin, prepared, input) = rotation_fixture();
    let before = key_bytes(&root);
    let count = ROTATION_NONCES.with(Cell::get);
    let a = sign_prepared_node_rotation(
        &root.node(),
        id(),
        &prepared.rotation_id,
        &pin,
        input.clone(),
        Now(Timestamp::new(1000).unwrap()),
    )
    .unwrap();
    let b = sign_prepared_node_rotation(
        &root.node(),
        id(),
        &prepared.rotation_id,
        &pin,
        input,
        Now(Timestamp::new(1000).unwrap()),
    )
    .unwrap();
    assert_ne!(a.old.body.nonce, b.old.body.nonce);
    assert_eq!(a.old.body, a.new.body.0);
    assert_eq!(
        a.old.body.certificate_hash,
        a.certificate.body.digest().unwrap()
    );
    a.old.verify(root.provision().public_key).unwrap();
    a.new.verify(prepared.new_public_key).unwrap();
    assert!(Signed {
        body: a.new.body.clone(),
        signature: a.old.signature
    }
    .verify(prepared.new_public_key)
    .is_err());
    assert_eq!(ROTATION_NONCES.with(Cell::get), count + 2);
    assert_eq!(key_bytes(&root), before);
}
#[test]
fn prepared_signer_denies_invalid_owner_proposals_before_nonce_or_private_mutation() {
    let (root, pin, prepared, input) = rotation_fixture();
    let before = key_bytes(&root);
    let count = ROTATION_NONCES.with(Cell::get);
    type Change = fn(&mut PreparedRotationInput);
    assert!(matches!(
        sign_prepared_node_rotation(
            &root.node(),
            id(),
            &prepared.rotation_id,
            &pin,
            input.clone(),
            Now(Timestamp::new(1061).unwrap())
        ),
        Err(Error::InvalidTime)
    ));
    let cases: &[(&str, Change, Error)] = &[
        (
            "owner epoch",
            |i| i.certificate.body.header.owner_epoch = 2,
            Error::WrongEpoch,
        ),
        (
            "next checkpoint",
            |i| i.certificate.body.header.checkpoint = 2,
            Error::WrongCheckpoint,
        ),
        (
            "context",
            |i| {
                i.certificate.body.header.context.deployment_id =
                    DeploymentId::new("99999999-9999-4999-8999-999999999999").unwrap()
            },
            Error::WrongContext,
        ),
        (
            "new key",
            |i| {
                i.certificate.body.agent_key = PublicKey::from_bytes(
                    SigningKey::from_bytes(&[66; 32]).verifying_key().to_bytes(),
                )
            },
            Error::WrongClaim,
        ),
        (
            "agent",
            |i| {
                i.certificate.body.agent_id =
                    AgentId::new("99999999-9999-4999-8999-999999999999").unwrap()
            },
            Error::WrongClaim,
        ),
        (
            "key epoch",
            |i| i.certificate.body.key_epoch = 3,
            Error::WrongEpoch,
        ),
        (
            "origin",
            |i| i.certificate.body.origin = i.previous_certificate.body.origin.clone(),
            Error::WrongState,
        ),
        (
            "scope",
            |i| {
                i.certificate.body.scopes =
                    Scopes(vec![Scope::TelemetryRead, Scope::MetricsCollect])
            },
            Error::CapabilityDenied,
        ),
        (
            "capability",
            |i| {
                i.certificate.body.capabilities.scopes =
                    Scopes(vec![Scope::TelemetryRead, Scope::MetricsCollect])
            },
            Error::CapabilityDenied,
        ),
        (
            "expiry widening",
            |i| i.certificate.body.expires_at = Timestamp::new(1601).unwrap(),
            Error::Expired,
        ),
        (
            "backdate widening",
            |i| i.certificate.body.not_before = Timestamp::new(999).unwrap(),
            Error::Expired,
        ),
        (
            "future",
            |i| i.certificate.body.not_before = Timestamp::new(1001).unwrap(),
            Error::InvalidTime,
        ),
        (
            "previous epoch",
            |i| {
                i.previous_certificate.body.header.owner_epoch = 2;
            },
            Error::WrongEpoch,
        ),
        (
            "previous checkpoint",
            |i| i.previous_certificate.body.header.checkpoint = 3,
            Error::WrongCheckpoint,
        ),
    ];
    for (name, mutate, expected) in cases {
        let mut candidate = input.clone();
        mutate(&mut candidate);
        candidate.previous_certificate = rotation_owner(candidate.previous_certificate.body, 11);
        if let CertificateOrigin::Rotation {
            previous_certificate,
            ..
        } = &mut candidate.certificate.body.origin
        {
            *previous_certificate = candidate.previous_certificate.body.digest().unwrap();
        }
        candidate.certificate = rotation_owner(candidate.certificate.body, 11);
        assert!(
            matches!(sign_prepared_node_rotation(&root.node(),id(),&prepared.rotation_id,&pin,candidate,Now(Timestamp::new(1000).unwrap())),Err(error) if error==*expected),
            "{name}"
        );
    }
    for previous in [false, true] {
        let mut bad = input.clone();
        if previous {
            bad.previous_certificate.signature = SignatureBytes::from_bytes([0; 64]);
        } else {
            bad.certificate.signature = SignatureBytes::from_bytes([0; 64]);
        }
        assert!(matches!(
            sign_prepared_node_rotation(
                &root.node(),
                id(),
                &prepared.rotation_id,
                &pin,
                bad,
                Now(Timestamp::new(1000).unwrap())
            ),
            Err(Error::InvalidSignature)
        ));
    }
    assert!(matches!(
        sign_prepared_node_rotation(
            &root.node(),
            id(),
            "wrong-id",
            &pin,
            input.clone(),
            Now(Timestamp::new(1000).unwrap())
        ),
        Err(Error::WrongClaim)
    ));
    let mut bad = input;
    bad.certificate.body.origin = CertificateOrigin::Rotation {
        previous_certificate: Hash::from_bytes([99; 32]),
        previous_key_epoch: 1,
    };
    bad.certificate = rotation_owner(bad.certificate.body, 11);
    assert!(matches!(
        sign_prepared_node_rotation(
            &root.node(),
            id(),
            &prepared.rotation_id,
            &pin,
            bad,
            Now(Timestamp::new(1000).unwrap())
        ),
        Err(Error::WrongClaim)
    ));
    assert_eq!(ROTATION_NONCES.with(Cell::get), count);
    assert_eq!(key_bytes(&root), before);
}
#[test]
fn prepared_signer_current_owner_endorses_old_body_without_claiming_historical_wrapper() {
    let (root, mut pin, prepared, mut input) = rotation_fixture();
    pin.epoch = 2;
    pin.checkpoint = 3;
    pin.key = PublicKey::from_bytes(SigningKey::from_bytes(&[55; 32]).verifying_key().to_bytes());
    pin.expires_at = Timestamp::new(1400).unwrap();
    input.certificate.body.header.owner_epoch = 2;
    input.certificate.body.header.checkpoint = 4;
    input.certificate.body.expires_at = Timestamp::new(1350).unwrap();
    input.certificate = rotation_owner(input.certificate.body, 55);
    input.previous_certificate.signature = SignatureBytes::from_bytes([0; 64]);
    let result = sign_prepared_node_rotation(
        &root.node(),
        id(),
        &prepared.rotation_id,
        &pin,
        input.clone(),
        Now(Timestamp::new(1000).unwrap()),
    )
    .unwrap();
    result.old.verify(root.provision().public_key).unwrap();
    result.new.verify(prepared.new_public_key).unwrap();
    input.certificate = rotation_owner(input.certificate.body, 11);
    assert!(matches!(
        sign_prepared_node_rotation(
            &root.node(),
            id(),
            &prepared.rotation_id,
            &pin,
            input,
            Now(Timestamp::new(1000).unwrap())
        ),
        Err(Error::InvalidSignature)
    ));
}
#[test]
fn prepared_signer_missing_lock_never_creates_or_repairs_it_and_entropy_failure_is_readonly() {
    let (root, pin, prepared, input) = rotation_fixture();
    let before = key_bytes(&root);
    FAIL.with(|v| *v.borrow_mut() = Some(("rotation-nonce", MATERIAL, Error::Entropy)));
    assert!(matches!(
        sign_prepared_node_rotation(
            &root.node(),
            id(),
            &prepared.rotation_id,
            &pin,
            input.clone(),
            Now(Timestamp::new(1000).unwrap())
        ),
        Err(Error::Entropy)
    ));
    FAIL.with(|v| *v.borrow_mut() = None);
    assert_eq!(key_bytes(&root), before);
    fs::remove_file(root.node().join(LOCK)).unwrap();
    let count = ROTATION_NONCES.with(Cell::get);
    assert!(matches!(
        sign_prepared_node_rotation(
            &root.node(),
            id(),
            &prepared.rotation_id,
            &pin,
            input,
            Now(Timestamp::new(1000).unwrap())
        ),
        Err(Error::Storage)
    ));
    assert!(!root.node().join(LOCK).exists());
    assert_eq!(ROTATION_NONCES.with(Cell::get), count);
}
