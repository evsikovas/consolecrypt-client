//! Draft pure record/replay tests. No mock auth observation, keys or storage I/O.
use super::*;
use serde_json::json;

fn vectors() -> Value {
    serde_json::from_str(include_str!("personal_owner_vectors.json")).unwrap()
}

#[test]
fn absent_local_owner_keeps_root_record_and_peer_bytes_and_digests_exact() {
    let v = vectors();
    let root: RootState = serde_json::from_value(v["root"].clone()).unwrap();
    let peer: PeerIdentity = serde_json::from_value(v["peer"].clone()).unwrap();
    assert!(root.owner_binding_hash.is_none() && peer.owner_binding_hash.is_none());
    assert_eq!(
        canonical(&root, BYTES).unwrap(),
        v["root_canonical_utf8"].as_str().unwrap().as_bytes()
    );
    assert_eq!(
        canonical(&peer, BYTES).unwrap(),
        v["peer_canonical_utf8"].as_str().unwrap().as_bytes()
    );
    assert_eq!(
        hex::encode(peer.hash().unwrap().as_bytes()),
        v["peer_digest_hex"].as_str().unwrap()
    );
    let record = Record::new(None, &root).unwrap();
    assert_eq!(
        canonical(&record, BYTES).unwrap(),
        v["record_canonical_utf8"].as_str().unwrap().as_bytes()
    );
    assert_eq!(
        hex::encode(record.digest.as_bytes()),
        v["record_digest_hex"].as_str().unwrap()
    );
}

#[test]
fn local_root_cannot_be_adopted_and_distinct_owner_namespaces_do_not_match() {
    let old: RootState = serde_json::from_value(vectors()["root"].clone()).unwrap();
    let before = canonical(&old, BYTES).unwrap();
    let owner_a = Some(AgentReaderHash::from_bytes([1; 32]));
    let other_server_or_account = Some(AgentReaderHash::from_bytes([2; 32]));
    assert_eq!(
        check_owner_binding(owner_a, old.owner_binding_hash),
        Err(AuthorityError::Rejected)
    );
    assert_eq!(
        check_owner_binding(None, owner_a),
        Err(AuthorityError::Rejected)
    );
    assert_eq!(
        check_owner_binding(owner_a, other_server_or_account),
        Err(AuthorityError::Rejected)
    );
    assert!(check_owner_binding(owner_a, owner_a).is_ok());
    assert!(
        canonical(&old, BYTES).unwrap() == before,
        "comparison must not adopt or mutate old root"
    );
    assert!(
        check_owner_binding(
            Some(AgentReaderHash::from_bytes([0; 32])),
            Some(AgentReaderHash::from_bytes([0; 32]))
        )
        .is_err()
    );
}

#[test]
fn optional_personal_snapshot_hash_is_absent_for_local_and_strict_when_present() {
    assert_eq!(snapshot_owner_binding(&json!({})).unwrap(), None);
    let owner = AgentReaderHash::from_bytes([1; 32]);
    assert_eq!(
        snapshot_owner_binding(&json!({"personalOwner":owner})).unwrap(),
        Some(owner)
    );
    for bad in [
        Value::Null,
        json!("00".repeat(32)),
        json!("AA".repeat(32)),
        json!(false),
        json!(1),
    ] {
        assert!(snapshot_owner_binding(&json!({"personalOwner":bad})).is_err());
    }
}

#[test]
fn replay_never_interchanges_personal_and_local_peer_owner_presence() {
    let mut pin: AgentReaderIssuerPinV1 = serde_json::from_value(json!({
        "format":"cc-reader-issuer-pin/v1","context":{"tenant":{"kind":"personalAccount","id":"11111111-1111-4111-8111-111111111111"},"deploymentId":"22222222-2222-4222-8222-222222222222"},
        "nativeProfileId":"77777777-7777-4777-8777-777777777777","issuerId":"88888888-8888-4888-8888-888888888888","issuerEpoch":1,
        "issuerKey":"3d4017c3e843895a92b70aa74d1b7ebc9c982ccf2ec4968cc0cd55f12af4660c","audienceId":"99999999-9999-4999-8999-999999999999","expiresAt":2000,"nodeTrustOriginHash":"ab".repeat(32)
    })).unwrap();
    let mut identity: PeerIdentity = serde_json::from_value(vectors()["peer"].clone()).unwrap();
    identity.pin_genesis = pin.genesis_hash().unwrap();
    identity.owner_binding_hash = Some(AgentReaderHash::from_bytes([1; 32]));
    assert!(
        PeerState::new(identity.clone(), pin.clone())
            .replay()
            .is_ok()
    );
    identity.owner_binding_hash = None;
    assert!(
        PeerState::new(identity.clone(), pin.clone())
            .replay()
            .is_err()
    );
    pin.context.tenant = AgentReaderTenantV1::LocalProfile {
        id: AgentReaderProfileId::new("11111111-1111-4111-8111-111111111111").unwrap(),
    };
    identity.pin_genesis = pin.genesis_hash().unwrap();
    assert!(
        PeerState::new(identity.clone(), pin.clone())
            .replay()
            .is_ok()
    );
    identity.owner_binding_hash = Some(AgentReaderHash::from_bytes([1; 32]));
    assert!(PeerState::new(identity, pin).replay().is_err());
}

#[test]
fn owner_hash_changes_peer_namespace_without_relabeling_profile_or_agent() {
    let local: PeerIdentity = serde_json::from_value(vectors()["peer"].clone()).unwrap();
    let mut personal = local.clone();
    personal.owner_binding_hash = Some(AgentReaderHash::from_bytes([1; 32]));
    let mut other_server_or_account = personal.clone();
    other_server_or_account.owner_binding_hash = Some(AgentReaderHash::from_bytes([2; 32]));
    assert_eq!(personal.profile, local.profile);
    assert_eq!(personal.agent, local.agent);
    assert_ne!(personal.hash().unwrap(), local.hash().unwrap());
    assert_ne!(
        personal.hash().unwrap(),
        other_server_or_account.hash().unwrap()
    );
}

#[test]
fn personal_snapshot_requires_v2_body_committing_the_exact_owner_hash() {
    let mut a: AgentReaderAssociationV1 =
        serde_json::from_value(vectors()["public_association_body"].clone()).unwrap();
    a.context.tenant = AgentReaderTenantV1::PersonalAccount {
        id: AgentReaderPersonalAccountId::new("11111111-1111-4111-8111-111111111111").unwrap(),
    };
    let owner = AgentReaderHash::from_bytes([1; 32]);
    let mut snapshot = json!({
        "nativeProfileId":a.native_profile_id,"personalOwner":owner,
        "nativeHostId":a.native_host_id,"nativeHostRevision":a.native_host_revision,
        "hostBodyDigest":"12".repeat(32),"nativeSourceId":a.native_source_id,
        "nativeSourceRevision":a.native_source_revision,"sourceConfigurationDigest":"13".repeat(32),
        "nativeBindingId":a.native_binding_id,"nativeBindingRevision":a.native_binding_revision,
        "bindingBodyDigest":"14".repeat(32),"deviceId":a.device_id,"deviceSigningKey":a.device_key.as_bytes()
    });
    let mut body = json!({
        "format":"cc-native-reader-binding/v2","personalOwner":owner,
        "profileId":a.native_profile_id,"hostId":a.native_host_id,"hostRevision":a.native_host_revision,
        "hostBodyDigest":snapshot["hostBodyDigest"],"sourceId":a.native_source_id,
        "sourceRevision":a.native_source_revision,"sourceConfigurationDigest":snapshot["sourceConfigurationDigest"],
        "bindingId":a.native_binding_id,"bindingRevision":a.native_binding_revision,
        "bindingBodyDigest":snapshot["bindingBodyDigest"]
    });
    a.native_binding_digest = digest(
        b"consolecrypt.native.reader.binding.v1",
        &canonical(&body, BYTES).unwrap(),
    );
    snapshot["nativeBindingDigest"] = json!(a.native_binding_digest);
    assert!(check_snapshot(&snapshot, &a).is_ok());
    let mut changed = snapshot.clone();
    changed["personalOwner"] = json!(AgentReaderHash::from_bytes([2; 32]));
    assert!(check_snapshot(&changed, &a).is_err());
    changed.as_object_mut().unwrap().remove("personalOwner");
    assert!(check_snapshot(&changed, &a).is_err());
    body["format"] = json!("cc-native-reader-binding/v1");
    a.native_binding_digest = digest(
        b"consolecrypt.native.reader.binding.v1",
        &canonical(&body, BYTES).unwrap(),
    );
    snapshot["nativeBindingDigest"] = json!(a.native_binding_digest);
    assert!(check_snapshot(&snapshot, &a).is_err());
}
