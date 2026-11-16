//! Draft source tests; NOT compiled/executed. Test material is public RFC8032.
use super::*;

fn issuer() -> ReaderIssuerSecretV1 {
    let mut bytes = zeroize::Zeroizing::new(b"CCRI\x01".to_vec());
    bytes.extend(
        hex::decode("4ccd089b28ff96da9db6c346ec114e0f5b8a319f35aba624da8cf6ed4fb8a6fb").unwrap(),
    );
    ReaderIssuerSecretV1::from_secure_store_bytes(&bytes).unwrap()
}
fn peer() -> PeerState {
    let pin:AgentReaderIssuerPinV1=serde_json::from_value(serde_json::json!({
        "format":"cc-reader-issuer-pin/v1","context":{"tenant":{"kind":"localProfile","id":"11111111-1111-4111-8111-111111111111"},"deploymentId":"22222222-2222-4222-8222-222222222222"},
        "nativeProfileId":"77777777-7777-4777-8777-777777777777","issuerId":"88888888-8888-4888-8888-888888888888","issuerEpoch":1,
        "issuerKey":issuer().public_key(),"audienceId":"99999999-9999-4999-8999-999999999999","expiresAt":2000,
        "nodeTrustOriginHash":"ab".repeat(32)
    })).unwrap();
    let identity = PeerIdentity {
        profile: pin.native_profile_id,
        owner_binding_hash: None,
        agent: AgentReaderAgentId::new("44444444-4444-4444-8444-444444444444").unwrap(),
        audience: pin.audience_id,
        node_origin: pin.node_trust_origin_hash,
        pin_genesis: pin.genesis_hash().unwrap(),
    };
    PeerState::new(identity, pin)
}
fn time(n: u64) -> AgentReaderTimestamp {
    AgentReaderTimestamp::new(n).unwrap()
}
fn signed_fixture() -> (
    PeerState,
    ReaderIssuerSecretV1,
    AgentReaderHash,
    AgentReaderTargetDescriptorV1,
    Value,
) {
    let mut p = peer();
    let issuer = issuer();
    let device = AgentReaderPublicKey::from_bytes(
        hex::decode("d75a980182b10ab7d54bfed3c964073a0ee172f3daa62325af021a68f707511a")
            .unwrap()
            .try_into()
            .unwrap(),
    );
    let target:AgentReaderTargetDescriptorV1=serde_json::from_value(serde_json::json!({
        "format":"cc-reader-target/v1","agentId":p.identity.agent,
        "configuredHostId":"33333333-3333-4333-8333-333333333333","action":"heartbeatRead",
        "resourceId":"66666666-6666-4666-8666-666666666666","dataset":{"kind":"heartbeatV1","schemaVersion":1,
        "descriptor":{"agent_id":p.identity.agent,"host_id":"33333333-3333-4333-8333-333333333333",
        "heartbeat_resource_id":"66666666-6666-4666-8666-666666666666","history_kind":"heartbeat","schema_version":1}},
        "maxResponseBytes":65536,"maxQueryPoints":0})).unwrap();
    let mut snapshot = serde_json::json!({"nativeProfileId":p.identity.profile,"notificationScopeId":"fixture",
        "nativeHostId":"aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa","nativeHostRevision":2,"hostBodyDigest":"12".repeat(32),
        "nativeSourceId":"fixture-source","nativeSourceRevision":3,"sourceConfigurationDigest":"13".repeat(32),
        "nativeBindingId":"bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb","nativeBindingRevision":4,"bindingBodyDigest":"14".repeat(32),
        "nativeBindingDigest":"00".repeat(32),"deviceId":"cccccccc-cccc-4ccc-8ccc-cccccccccccc","deviceSigningKey":device.as_bytes()});
    let binding = BTreeMap::from([
        ("format", serde_json::json!("cc-native-reader-binding/v1")),
        ("profileId", snapshot["nativeProfileId"].clone()),
        ("hostId", snapshot["nativeHostId"].clone()),
        ("hostRevision", snapshot["nativeHostRevision"].clone()),
        ("hostBodyDigest", snapshot["hostBodyDigest"].clone()),
        ("sourceId", snapshot["nativeSourceId"].clone()),
        ("sourceRevision", snapshot["nativeSourceRevision"].clone()),
        (
            "sourceConfigurationDigest",
            snapshot["sourceConfigurationDigest"].clone(),
        ),
        ("bindingId", snapshot["nativeBindingId"].clone()),
        ("bindingRevision", snapshot["nativeBindingRevision"].clone()),
        ("bindingBodyDigest", snapshot["bindingBodyDigest"].clone()),
    ]);
    let binding_hash = digest(
        b"consolecrypt.native.reader.binding.v1",
        &canonical(&binding, BYTES).unwrap(),
    );
    snapshot["nativeBindingDigest"] = serde_json::json!(binding_hash);
    let a:AgentReaderAssociationV1=serde_json::from_value(serde_json::json!({
        "version":1,"issuerId":p.pin.issuer_id,"issuerEpoch":1,"authorityRevision":1,"previousAuthorityHash":p.pin.genesis_hash().unwrap(),
        "context":p.pin.context,"nativeProfileId":p.pin.native_profile_id,"nativeHostId":snapshot["nativeHostId"],"nativeHostRevision":2,
        "nativeBindingId":snapshot["nativeBindingId"],"nativeBindingRevision":4,"nativeBindingDigest":binding_hash,"nativeSourceId":"fixture-source","nativeSourceRevision":3,
        "associationId":"dddddddd-dddd-4ddd-8ddd-dddddddddddd","agentId":p.identity.agent,"nodeTrustOriginHash":p.identity.node_origin,
        "agentCertificateHash":"ef".repeat(32),"agentKeyEpoch":1,"deviceId":snapshot["deviceId"],"deviceEpoch":1,"deviceKey":device,
        "audienceId":p.identity.audience,"transportPeerHash":"aa".repeat(32),"operation":target.operation(),
        "datasetKind":"heartbeatV1","datasetSchemaVersion":1,"targetDescriptorHash":target.descriptor_hash().unwrap(),
        "issuedAt":1000,"notBefore":1000,"expiresAt":1800})).unwrap();
    let binding = ReaderPeerBinding {
        native_profile_id: a.native_profile_id,
        agent_id: a.agent_id,
        audience_id: a.audience_id,
        node_trust_origin_hash: a.node_trust_origin_hash,
        issuer_pin_genesis_hash: p.pin.genesis_hash().unwrap(),
        transport_peer_hash: a.transport_peer_hash,
        agent_certificate_hash: a.agent_certificate_hash,
        agent_key_epoch: a.agent_key_epoch,
        target_hashes: [
            target.descriptor_hash().unwrap(),
            AgentReaderHash::from_bytes([2; 32]),
        ],
        ssh_server_key_digest: a.transport_peer_hash,
        http_port: 9469,
    };
    let hash = a.body_digest().unwrap();
    p.push(
        time(1000),
        Event::Association {
            signed: issuer.sign_association(a, &p.pin).unwrap(),
            target: target.clone(),
            native_snapshot: snapshot.clone(),
            transport_binding: binding.digest().unwrap(),
            transport_binding_body: binding.clone(),
        },
    )
    .unwrap();
    let head = p.replay().unwrap().head;
    p.push(time(1001), Event::Installed { head }).unwrap();
    (p, issuer, hash, target, snapshot)
}

#[test]
fn actual_signed_approval_historical_replay_does_not_use_current_expiry_or_configured_host_as_native()
 {
    let (p, _, hash, target, _) = signed_fixture();
    let replay = p.replay().unwrap();
    let (a, _, _, _, _) = replay.live_association(hash, time(1010)).unwrap();
    assert_ne!(
        a.body.native_host_id.as_uuid(),
        target.configured_host_id.as_uuid()
    );
    assert!(replay.live_association(hash, time(1800)).is_err());
    assert!(p.replay().is_ok()); // Recorded-at replay remains valid after expiry.
}
#[test]
fn signed_association_does_not_cover_changed_native_selector_snapshot_or_response_budget() {
    let (mut p, _, _, _, _) = signed_fixture();
    if let Event::Association {
        native_snapshot, ..
    } = &mut p.events[0].event
    {
        native_snapshot["bindingBodyDigest"] = serde_json::json!("ff".repeat(32));
    }
    // Rehashing an unsigned storage envelope cannot alter the signed binding BODY.
    p.events[0].hash=digest(b"consolecrypt.native.reader.event.v1",&canonical(&serde_json::json!({"at":p.events[0].at,"previous":p.events[0].previous,"event":p.events[0].event}),BYTES).unwrap());
    assert!(p.replay().is_err());
    let (mut p, _, _, _, _) = signed_fixture();
    if let Event::Association { target, .. } = &mut p.events[0].event {
        target.max_response_bytes = 262144;
    }
    assert!(p.replay().is_err());
}
#[test]
fn real_issuer_retained_association_uses_new_head_exact_prepared_freshness_and_operation_tombstone()
{
    let (mut p, issuer, hash, target, snapshot) = signed_fixture();
    let original = p.replay().unwrap().associations[&hash].0.clone();
    let binding = p.replay().unwrap().associations[&hash].4.clone();
    let mut second = original.body.clone();
    second.association_id =
        AgentReaderAssociationId::new("eeeeeeee-eeee-4eee-8eee-eeeeeeeeeeee").unwrap();
    let head = p.replay().unwrap().head;
    second.authority_revision = 2;
    second.previous_authority_hash = head.authority_hash;
    second.issued_at = time(1002);
    second.not_before = time(1002);
    p.push(
        time(1002),
        Event::Association {
            signed: issuer.sign_association(second, &p.pin).unwrap(),
            target: target.clone(),
            native_snapshot: snapshot,
            transport_binding: binding.digest().unwrap(),
            transport_binding_body: binding.clone(),
        },
    )
    .unwrap();
    let head = p.replay().unwrap().head;
    p.push(time(1003), Event::Installed { head: head.clone() })
        .unwrap();
    let operation = AgentReaderOperationId::new("ffffffff-ffff-4fff-8fff-ffffffffffff").unwrap();
    p.push(
        time(1004),
        Event::Reserve {
            operation,
            association: hash,
        },
    )
    .unwrap();
    let freshness = issuer
        .sign_freshness(
            AgentReaderFreshnessV1 {
                version: 1,
                head: head.clone(),
                issued_at: time(1005),
                expires_at: time(1030),
            },
            &p.pin,
        )
        .unwrap();
    p.push(
        time(1005),
        Event::ChallengePrepared {
            operation,
            freshness: freshness.clone(),
        },
    )
    .unwrap();
    let a = &original.body;
    let challenge = AgentReaderChallengeV1 {
        version: 1,
        head: head.clone(),
        challenge_id: AgentReaderHash::from_bytes([1; 32]),
        server_nonce: AgentReaderHash::from_bytes([2; 32]),
        association_hash: hash,
        operation_id: operation,
        device_id: a.device_id,
        device_epoch: 1,
        device_key_fingerprint: agent_reader_key_fingerprint(a.device_key),
        agent_id: a.agent_id,
        agent_certificate_hash: a.agent_certificate_hash,
        agent_key_epoch: 1,
        node_owner_epoch: 1,
        node_checkpoint: 1,
        native_profile_id: a.native_profile_id,
        native_binding_id: a.native_binding_id,
        native_binding_revision: a.native_binding_revision,
        operation: a.operation.clone(),
        target_descriptor_hash: a.target_descriptor_hash,
        issued_at: time(1006),
        expires_at: time(1029),
    };
    let grant = issuer
        .sign_grant(
            AgentReaderGrantV1 {
                version: 1,
                head,
                grant_id: AgentReaderGrantId::new("aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaab").unwrap(),
                association_hash: hash,
                challenge_id: challenge.challenge_id,
                challenge_hash: challenge.challenge_hash().unwrap(),
                operation_id: operation,
                device_id: a.device_id,
                device_epoch: 1,
                agent_id: a.agent_id,
                operation: a.operation.clone(),
                target_descriptor_hash: a.target_descriptor_hash,
                issued_at: time(1007),
                expires_at: time(1028),
            },
            &p.pin,
        )
        .unwrap();
    p.push(
        time(1007),
        Event::Issued {
            operation,
            grant,
            freshness,
            challenge,
        },
    )
    .unwrap();
    p.push(time(1008), Event::Retired { operation }).unwrap();
    let replay = p.replay().unwrap();
    assert_eq!(replay.reservations[&operation], (hash, 3));
    assert_eq!(original.body.authority_revision, 1);
    assert_eq!(replay.head.authority_revision, 2);
    assert!(
        p.push(
            time(1009),
            Event::Reserve {
                operation,
                association: hash
            }
        )
        .is_err()
    );
}
#[test]
fn actual_local_signed_revocation_blocks_pending_head_and_current_association_without_erasing_history()
 {
    let (mut p, issuer, hash, _, _) = signed_fixture();
    let r = p.replay().unwrap();
    let a = &r.associations[&hash].0.body;
    let signed = issuer
        .sign_revocation(
            AgentReaderRevocationV1 {
                version: 1,
                context: p.pin.context.clone(),
                issuer_id: p.pin.issuer_id,
                issuer_epoch: 1,
                authority_revision: 2,
                previous_authority_hash: r.head.authority_hash,
                subject: AgentReaderRevocationSubjectV1::Association {
                    association_id: a.association_id,
                    body_hash: hash,
                },
                issued_at: time(1010),
            },
            &p.pin,
        )
        .unwrap();
    p.push(time(1010), Event::Revoke { signed }).unwrap();
    let replay = p.replay().unwrap();
    assert_ne!(replay.head, replay.installed);
    assert!(replay.live_association(hash, time(1011)).is_err());
    assert!(replay.associations.contains_key(&hash));
}

#[test]
fn full_peer_namespace_changes_for_every_cross_agent_or_pin_origin_tuple() {
    let p = peer();
    let expected = p.identity.hash().unwrap();
    let mut cases = vec![p.identity.clone(); 4];
    cases[0].profile =
        AgentReaderNativeProfileId::new("aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa").unwrap();
    cases[1].agent = AgentReaderAgentId::new("aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa").unwrap();
    cases[2].node_origin = AgentReaderHash::from_bytes([3; 32]);
    cases[3].pin_genesis = AgentReaderHash::from_bytes([4; 32]);
    for value in cases {
        assert_ne!(value.hash().unwrap(), expected);
    }
}
#[test]
fn canonical_storage_rejects_duplicate_unknown_and_noninteger_forms() {
    let r = Record::new(None, &serde_json::json!({"x":1})).unwrap();
    let mut value = serde_json::to_value(&r).unwrap();
    value["extra"] = serde_json::json!(1);
    assert!(decode::<Record>(&serde_json::to_vec(&value).unwrap()).is_err());
    let bytes = canonical(&r, BYTES).unwrap();
    assert!(decode::<Record>(&[b" ".as_slice(), bytes.as_slice()].concat()).is_err());
    let text = String::from_utf8(bytes).unwrap().replacen(
        "\"revision\":1",
        "\"revision\":1,\"revision\":1",
        1,
    );
    assert!(decode::<Record>(text.as_bytes()).is_err());
    assert!(canonical(&serde_json::json!({"x":1.0}), BYTES).is_err());
}
#[test]
fn record_witness_binds_current_revision_full_row_key_and_body() {
    let a = Record::new(None, &serde_json::json!({"x":1})).unwrap();
    let b = Record::new(Some(&a), &serde_json::json!({"x":2})).unwrap();
    assert_ne!(a.witness("peer-a").unwrap(), b.witness("peer-a").unwrap());
    assert_ne!(b.witness("peer-a").unwrap(), b.witness("peer-b").unwrap());
    let mut tamper = b.clone();
    tamper.body["x"] = serde_json::json!(3);
    assert!(tamper.check().is_err());
    assert!(b.witness_pending); // Pending cannot be loaded as a ready authority.
}
#[test]
fn journal_denies_unsigned_installation_unknown_operation_and_modified_event() {
    let mut p = peer();
    let head = p.replay().unwrap().head;
    assert!(p.push(time(1000), Event::Installed { head }).is_err());
    let mut p = peer();
    let operation = AgentReaderOperationId::new("aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa").unwrap();
    assert!(
        p.push(
            time(1000),
            Event::Reserve {
                operation,
                association: AgentReaderHash::from_bytes([7; 32])
            }
        )
        .is_err()
    );
    // Even a hash-correct unsigned local record cannot make a nonexistent
    // signed association/operation into approval or an installed Reader head.
    p.events.clear();
    let event = Event::Retired { operation };
    let previous = p.pin.genesis_hash().unwrap();
    let hash = digest(
        b"consolecrypt.native.reader.event.v1",
        &canonical(
            &serde_json::json!({"at":1000,"previous":previous,"event":event}),
            BYTES,
        )
        .unwrap(),
    );
    p.events.push(Entry {
        at: time(1000),
        previous,
        event,
        hash,
    });
    assert!(p.replay().is_err());
}
#[test]
fn lifetime_audience_index_never_authorizes_cross_peer_overwrite() {
    let p = peer();
    let old = p.identity.hash().unwrap();
    let mut changed = p.identity.clone();
    changed.agent = AgentReaderAgentId::new("aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa").unwrap();
    let index = BTreeMap::from([(p.identity.audience, old)]);
    assert!(
        index
            .get(&p.identity.audience)
            .is_some_and(|v| *v != changed.hash().unwrap())
    );
    assert_eq!(index.get(&p.identity.audience), Some(&old));
}
#[test]
fn fixed_journal_and_encoded_byte_caps_refuse_growth_without_compaction() {
    assert!(canonical(&"x".repeat(BYTES), BYTES).is_err());
    let mut p = peer();
    let event = Event::Retired {
        operation: AgentReaderOperationId::new("aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa").unwrap(),
    };
    p.events = vec![
        Entry {
            at: time(1000),
            previous: AgentReaderHash::from_bytes([0; 32]),
            event,
            hash: AgentReaderHash::from_bytes([0; 32])
        };
        ENTRIES
    ];
    assert!(matches!(
        p.push(time(1001), p.events[0].event.clone()),
        Err(AuthorityError::Quota)
    ));
    assert_eq!(p.events.len(), ENTRIES);
}
#[test]
fn registered_pin_mutation_invalidates_peer_replay_and_no_issuer_adoption() {
    let mut p = peer();
    p.pin.issuer_key = AgentReaderPublicKey::from_bytes(
        hex::decode("d75a980182b10ab7d54bfed3c964073a0ee172f3daa62325af021a68f707511a")
            .unwrap()
            .try_into()
            .unwrap(),
    );
    assert!(p.replay().is_err()); // genesis binds full pin, not merely audience.
    let mut wrong = b"CCRI\x01".to_vec();
    wrong.extend([0; 32]);
    assert!(ReaderIssuerSecretV1::from_secure_store_bytes(&wrong).is_err());
}
#[test]
fn witness_leaf_names_fit_os_store_without_truncation_or_aliasing() {
    let profile = "77777777-7777-4777-8777-777777777777";
    for name in [
        format!("profiles/{profile}/reader-root-v1"),
        format!("profiles/{profile}/reader-issuer-v1"),
        format!("profiles/{profile}/reader-head/{}", "a".repeat(64)),
    ] {
        assert!(cc_platform_core::validate_secret_name(&name).is_ok());
    }
}

fn snapshot_from_fixture(v: &Value) -> ReaderContextSnapshot {
    ReaderContextSnapshot {
        personal_owner: None,
        native_profile_id: v["nativeProfileId"].as_str().unwrap().into(),
        notification_scope_id: v["notificationScopeId"].as_str().unwrap().into(),
        native_host_id: v["nativeHostId"].as_str().unwrap().into(),
        native_host_revision: v["nativeHostRevision"].as_u64().unwrap(),
        host_body_digest: v["hostBodyDigest"].as_str().unwrap().into(),
        native_source_id: v["nativeSourceId"].as_str().unwrap().into(),
        native_source_revision: v["nativeSourceRevision"].as_u64().unwrap(),
        source_configuration_digest: v["sourceConfigurationDigest"].as_str().unwrap().into(),
        native_binding_id: v["nativeBindingId"].as_str().unwrap().into(),
        native_binding_revision: v["nativeBindingRevision"].as_u64().unwrap(),
        binding_body_digest: v["bindingBodyDigest"].as_str().unwrap().into(),
        native_binding_digest: v["nativeBindingDigest"].as_str().unwrap().into(),
        device_id: v["deviceId"].as_str().unwrap().into(),
        device_signing_key: serde_json::from_value(v["deviceSigningKey"].clone()).unwrap(),
    }
}
#[test]
fn durable_signed_approval_survives_new_unlock_scope_only_and_full_attempt_snapshot_stays_distinct()
{
    let (p, _, hash, _, stored) = signed_fixture();
    let mut new = snapshot_from_fixture(&stored);
    new.notification_scope_id = "fresh-unlock-scope".into();
    assert_ne!(snapshot_value(&new).unwrap(), stored); // Existing attempt/review comparison remains FULL.
    assert!(stable_snapshot_matches(&stored, &new).unwrap());
    assert!(p.replay().unwrap().associations.contains_key(&hash));
    let cases: Vec<Box<dyn Fn(&mut ReaderContextSnapshot)>> = vec![
        Box::new(|s| s.native_profile_id = "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa".into()),
        Box::new(|s| s.native_host_id = "eeeeeeee-eeee-4eee-8eee-eeeeeeeeeeee".into()),
        Box::new(|s| s.native_host_revision += 1),
        Box::new(|s| s.host_body_digest = "15".repeat(32)),
        Box::new(|s| s.native_source_id = "another-source".into()),
        Box::new(|s| s.native_source_revision += 1),
        Box::new(|s| s.source_configuration_digest = "15".repeat(32)),
        Box::new(|s| s.native_binding_revision += 1),
        Box::new(|s| s.native_binding_id = "eeeeeeee-eeee-4eee-8eee-eeeeeeeeeeee".into()),
        Box::new(|s| s.binding_body_digest = "15".repeat(32)),
        Box::new(|s| s.native_binding_digest = "15".repeat(32)),
        Box::new(|s| s.device_id = "eeeeeeee-eeee-4eee-8eee-eeeeeeeeeeee".into()),
        Box::new(|s| s.device_signing_key = [7; 32]),
    ];
    for change in cases {
        let mut changed = new.clone();
        change(&mut changed);
        assert!(!stable_snapshot_matches(&stored, &changed).unwrap());
    }
    let mut missing = stored.clone();
    missing
        .as_object_mut()
        .unwrap()
        .remove("notificationScopeId");
    assert!(stable_snapshot_matches(&missing, &new).is_err());
}
#[test]
fn persisted_full_peer_binding_must_recompute_exact_approved_digest_before_reconnection() {
    let (p, _, hash, target, _) = signed_fixture();
    let r = p.replay().unwrap();
    let (a, _, _, expected, binding) = &r.associations[&hash];
    check_binding(binding, *expected, &p.pin, &a.body, &target).unwrap();
    let mut foreign = binding.clone();
    foreign.http_port += 1;
    assert!(check_binding(&foreign, *expected, &p.pin, &a.body, &target).is_err());
    let mut foreign = binding.clone();
    foreign.ssh_server_key_digest = AgentReaderHash::from_bytes([5; 32]);
    assert!(check_binding(&foreign, *expected, &p.pin, &a.body, &target).is_err());
    let mut foreign = binding.clone();
    foreign.target_hashes.swap(0, 1);
    // Even a newly hashed transport candidate cannot replace the signed per-action target position.
    assert!(
        check_binding(
            &foreign,
            foreign.digest().unwrap(),
            &p.pin,
            &a.body,
            &target
        )
        .is_err()
    );
}

#[test]
fn known_choices_filter_real_signed_foreign_stale_future_and_unsupported_records() {
    let (state,_,hash,_,snapshot)=signed_fixture();let current=snapshot_from_fixture(&snapshot);
    let peer=state.identity.hash().unwrap();
    registered_value(&state,&state.replay().unwrap(),&current,peer,hash,time(1010)).unwrap();
    for field in ["native_profile_id","native_source_id","device_id"] {
        let mut foreign=snapshot_from_fixture(&snapshot);
        match field {"native_profile_id"=>foreign.native_profile_id="11111111-1111-4111-8111-111111111111".into(),
            "native_source_id"=>foreign.native_source_id="other-source".into(),_=>foreign.device_id="11111111-1111-4111-8111-111111111111".into()}
        assert!(registered_value(&state,&state.replay().unwrap(),&foreign,peer,hash,time(1010)).is_err());
    }
    let mut stale=snapshot_from_fixture(&snapshot);stale.native_source_revision+=1;
    assert!(registered_value(&state,&state.replay().unwrap(),&stale,peer,hash,time(1010)).is_err());
    assert!(registered_value(&state,&state.replay().unwrap(),&current,peer,hash,time(999)).is_err());
    assert!(registered_value(&state,&state.replay().unwrap(),&current,peer,hash,time(1800)).is_err());
    let mut unsupported=state.replay().unwrap();unsupported.associations.get_mut(&hash).unwrap().1.dataset.schema_version=2;
    assert!(registered_value(&state,&unsupported,&current,peer,hash,time(1010)).is_err());
    assert!(registered_value(&state,&state.replay().unwrap(),&current,AgentReaderHash::from_bytes([9;32]),hash,time(1010)).is_err());
}
#[test]
fn known_choices_keep_stable_approval_across_new_notice_but_deny_pending_head() {
    let (mut state,_,hash,_,snapshot)=signed_fixture();let mut current=snapshot_from_fixture(&snapshot);
    current.notification_scope_id="new opaque unlock / 日本語".into();let peer=state.identity.hash().unwrap();
    registered_value(&state,&state.replay().unwrap(),&current,peer,hash,time(1010)).unwrap();
    // Remove the UNIT FIXTURE's local installed event. No remote Actor ACK is claimed.
    assert!(matches!(state.events.pop().unwrap().event,Event::Installed{..}));
    assert!(matches!(registered_value(&state,&state.replay().unwrap(),&current,peer,hash,time(1010)),Err(AuthorityError::Unavailable)));
}
