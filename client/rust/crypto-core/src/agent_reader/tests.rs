use super::*;
use serde_json::{Value, json};

fn vectors() -> Value {
    serde_json::from_str(include_str!("vectors.json")).unwrap()
}
fn decode<T: serde::de::DeserializeOwned>(value: Value) -> T {
    serde_json::from_value(value).unwrap()
}
fn now(value: i64) -> cc_protocol::Timestamp {
    chrono::DateTime::<chrono::Utc>::from_timestamp(value, 0).unwrap()
}
fn vector<T: AgentReaderStatementV1>(v: &Value) {
    let body: T = decode(v["body"].clone());
    assert_eq!(
        hex::encode(body.signing_bytes().unwrap()),
        v["signing_bytes_hex"].as_str().unwrap()
    );
    assert_eq!(
        hex::encode(body.body_digest().unwrap().as_bytes()),
        v["body_digest_hex"].as_str().unwrap()
    );
}
#[test]
fn exact_agent_five_transcripts_and_hashes_match_public_vectors() {
    let v = vectors();
    let s = v["statements"].as_array().unwrap();
    vector::<AgentReaderAssociationV1>(&s[0]);
    vector::<AgentReaderGrantV1>(&s[1]);
    vector::<AgentReaderProofV1>(&s[2]);
    vector::<AgentReaderRevocationV1>(&s[3]);
    vector::<AgentReaderFreshnessV1>(&s[4]);
    let target: AgentReaderTargetDescriptorV1 = decode(v["target"]["body"].clone());
    assert_eq!(
        hex::encode(target.descriptor_hash().unwrap().as_bytes()),
        v["target"]["digest_hex"].as_str().unwrap()
    );
    let c: AgentReaderChallengeV1 = decode(v["challenge"]["body"].clone());
    assert_eq!(
        hex::encode(c.challenge_hash().unwrap().as_bytes()),
        v["challenge"]["digest_hex"].as_str().unwrap()
    );
    let a: AgentReaderAssociationV1 = decode(s[0]["body"].clone());
    assert_eq!(
        agent_reader_key_fingerprint(a.device_key),
        c.device_key_fingerprint
    );
}
#[test]
fn strict_native_reader_json_rejects_ambiguous_numbers_keys_ids_and_bounds() {
    for raw in [
        b"{\"x\":1,\"x\":2}".as_slice(),
        b"{\"x\":1e309}",
        b"{\"x\":1.0}",
        b"{\"x\":NaN}",
    ] {
        assert!(wire::strict_json::<Value>(raw, 16384).is_err());
    }
    assert!(
        wire::strict_json::<Value>(
            format!("{}0{}", "[".repeat(18), "]".repeat(18)).as_bytes(),
            16384
        )
        .is_err()
    );
    assert!(wire::strict_json::<Value>(&vec![b' '; 16385], 16384).is_err());
    assert!(AgentReaderDeviceId::from_device_id(cc_protocol::DeviceId::NIL).is_err());
    assert!(AgentReaderTimestamp::from_native(now(-1)).is_err());
    assert!(AgentReaderTimestamp::new(253_402_300_800).is_err());
    let mut p = vectors()["statements"][2]["body"].clone();
    p["deviceEpoch"] = json!(true);
    assert!(serde_json::from_value::<AgentReaderProofV1>(p).is_err());
    for value in ["AA".repeat(32), "a".repeat(63), "g".repeat(64)] {
        assert!(serde_json::from_value::<AgentReaderHash>(json!(value)).is_err());
    }
}
fn issuer() -> ReaderIssuerSecretV1 {
    // RFC8032 public test seed only, never a user/device/private-store read.
    let mut bytes = zeroize::Zeroizing::new(b"CCRI\x01".to_vec());
    bytes.extend(
        hex::decode("4ccd089b28ff96da9db6c346ec114e0f5b8a319f35aba624da8cf6ed4fb8a6fb").unwrap(),
    );
    ReaderIssuerSecretV1::from_secure_store_bytes(&bytes).unwrap()
}
fn fixture() -> (AgentReaderExpectedV1, Vec<u8>) {
    let v = vectors();
    let issuer = issuer();
    let pin: AgentReaderIssuerPinV1 = decode(v["pin"]["body"].clone());
    assert_eq!(issuer.public_key(), pin.issuer_key);
    let a: AgentReaderAssociationV1 = decode(v["statements"][0]["body"].clone());
    let g: AgentReaderGrantV1 = decode(v["statements"][1]["body"].clone());
    let f: AgentReaderFreshnessV1 = decode(v["statements"][4]["body"].clone());
    let c: AgentReaderChallengeV1 = decode(v["challenge"]["body"].clone());
    let target: AgentReaderTargetDescriptorV1 = decode(v["target"]["body"].clone());
    let bytes=serde_json::to_vec(&json!({"association":issuer.sign_association(a.clone(),&pin).unwrap(),
        "grant":issuer.sign_grant(g.clone(),&pin).unwrap(),"freshness":issuer.sign_freshness(f,&pin).unwrap(),"challenge":c})).unwrap();
    (
        AgentReaderExpectedV1 {
            issuer_pin: pin,
            current_head: g.head,
            association_body: a,
            target,
        },
        bytes,
    )
}
fn device() -> crate::DeviceSecretKeys {
    let seed: [u8; 32] =
        hex::decode("9d61b19deffd5a60ba844af492ec2cc44449c5697b326919703bac031cae7f60")
            .unwrap()
            .try_into()
            .unwrap();
    crate::DeviceSecretKeys::from_test_seeds(&[7; 32], &seed)
}
#[test]
fn real_dedicated_device_proof_signs_exact_transcript_after_issuer_validation() {
    let (expected, packet) = fixture();
    let id = expected.association_body.device_id.as_device_id();
    let inputs = AgentReaderValidatedInputsV1::from_packet(&packet, expected, now(1002)).unwrap();
    let key = device();
    let prepared = inputs
        .prepare_for_device_once(id, key.public_keys().signing, now(1002))
        .unwrap();
    let proof = key.sign_agent_reader_proof_v1(prepared).unwrap();
    proof
        .verify(AgentReaderPublicKey::from_bytes(key.public_keys().signing))
        .unwrap();
    assert_ne!(proof.body.nonce, AgentReaderHash::from_bytes([0; 32]));
    let (expected, packet) = fixture();
    let inputs = AgentReaderValidatedInputsV1::from_packet(&packet, expected, now(1002)).unwrap();
    let wrong = crate::DeviceSecretKeys::from_test_seeds(&[3; 32], &[4; 32]);
    assert!(
        inputs
            .prepare_for_device_once(id, wrong.public_keys().signing, now(1002))
            .is_err()
    );
}
#[test]
fn issuer_encoding_is_dedicated_and_never_device_secret_serialization() {
    let key = issuer();
    let bytes = key.to_secure_store_bytes();
    assert_eq!(bytes.len(), 37);
    assert!(bytes[..5] == *b"CCRI\x01", "issuer format marker");
    assert_eq!(
        ReaderIssuerSecretV1::from_secure_store_bytes(&bytes)
            .unwrap()
            .public_key(),
        key.public_key()
    );
    for bytes in [
        zeroize::Zeroizing::new(vec![0u8; 32]),
        zeroize::Zeroizing::new(vec![0u8; 37]),
        device().to_secret_bytes(),
    ] {
        assert!(ReaderIssuerSecretV1::from_secure_store_bytes(&bytes).is_err());
    }
}

#[test]
fn signed_foreign_links_and_stale_packets_fail_before_proof_entropy() {
    for field in ["operationId", "associationHash", "targetDescriptorHash"] {
        let (expected, packet) = fixture();
        let mut value: Value = serde_json::from_slice(&packet).unwrap();
        value["grant"]["body"][field] = if field == "operationId" {
            json!("88888888-8888-4888-8888-888888888888")
        } else {
            json!("88".repeat(32))
        };
        let body: AgentReaderGrantV1 = decode(value["grant"]["body"].clone());
        value["grant"] =
            serde_json::to_value(issuer().sign_grant(body, &expected.issuer_pin).unwrap()).unwrap();
        assert!(
            AgentReaderValidatedInputsV1::from_packet(
                &serde_json::to_vec(&value).unwrap(),
                expected,
                now(1002)
            )
            .is_err(),
            "signed foreign link must deny"
        );
    }
    let (expected, packet) = fixture();
    assert!(matches!(
        AgentReaderValidatedInputsV1::from_packet(&packet, expected, now(1010)),
        Err(AgentReaderError::Expired)
    ));
    let (mut expected, packet) = fixture();
    expected.target.max_query_points = 5760;
    assert!(AgentReaderValidatedInputsV1::from_packet(&packet, expected, now(1002)).is_err());
}
#[test]
fn nonce_entropy_is_once_and_only_after_actual_device_and_time_checks() {
    let key = device();
    let (expected, packet) = fixture();
    let id = expected.association_body.device_id.as_device_id();
    let inputs = AgentReaderValidatedInputsV1::from_packet(&packet, expected, now(1002)).unwrap();
    let mut calls = 0;
    assert!(matches!(
        inputs.prepare_with_test_entropy(id, key.public_keys().signing, now(1002), &mut |_| {
            calls += 1;
            Ok(())
        }),
        Err(AgentReaderError::Entropy)
    ));
    assert_eq!(calls, 1);
    let (expected, packet) = fixture();
    let inputs = AgentReaderValidatedInputsV1::from_packet(&packet, expected, now(1002)).unwrap();
    let wrong = crate::DeviceSecretKeys::from_test_seeds(&[2; 32], &[3; 32]);
    assert!(matches!(
        inputs.prepare_with_test_entropy(id, wrong.public_keys().signing, now(1002), &mut |_| {
            calls += 1;
            Ok(())
        }),
        Err(AgentReaderError::WrongDevice)
    ));
    assert_eq!(calls, 1);
    let (expected, packet) = fixture();
    let inputs = AgentReaderValidatedInputsV1::from_packet(&packet, expected, now(1002)).unwrap();
    assert!(matches!(
        inputs.prepare_with_test_entropy(id, key.public_keys().signing, now(1010), &mut |_| {
            calls += 1;
            Ok(())
        }),
        Err(AgentReaderError::Expired)
    ));
    assert_eq!(calls, 1);
}
#[test]
fn account_signature_cannot_be_relabelled_as_reader_proof() {
    let (expected, packet) = fixture();
    let id = expected.association_body.device_id.as_device_id();
    let key = device();
    let input = AgentReaderValidatedInputsV1::from_packet(&packet, expected, now(1002)).unwrap();
    let prepared = input
        .prepare_for_device_once(id, key.public_keys().signing, now(1002))
        .unwrap();
    let mut signed = key.sign_agent_reader_proof_v1(prepared).unwrap();
    signed.signature = AgentReaderSignature::from_bytes(key.sign_request(
        id,
        "GET",
        "/v1/history",
        &[0; 32],
        1002,
        &[9; 32],
    ));
    assert_eq!(
        signed.verify(AgentReaderPublicKey::from_bytes(key.public_keys().signing)),
        Err(AgentReaderError::InvalidSignature)
    );
}
#[test]
fn retained_association_install_revision_is_older_than_current_grant_head() {
    let (mut expected, packet) = fixture();
    let mut value: Value = serde_json::from_slice(&packet).unwrap();
    expected.current_head =
        advance_reader_head_v1(&expected.current_head, AgentReaderHash::from_bytes([5; 32]))
            .unwrap();
    let head = expected.current_head.clone();
    let mut challenge: AgentReaderChallengeV1 = decode(value["challenge"].clone());
    challenge.head = head.clone();
    let mut grant: AgentReaderGrantV1 = decode(value["grant"]["body"].clone());
    grant.head = head.clone();
    grant.challenge_hash = challenge.challenge_hash().unwrap();
    let mut fresh: AgentReaderFreshnessV1 = decode(value["freshness"]["body"].clone());
    fresh.head = head;
    value["challenge"] = serde_json::to_value(challenge).unwrap();
    value["grant"] =
        serde_json::to_value(issuer().sign_grant(grant, &expected.issuer_pin).unwrap()).unwrap();
    value["freshness"] = serde_json::to_value(
        issuer()
            .sign_freshness(fresh, &expected.issuer_pin)
            .unwrap(),
    )
    .unwrap();
    assert!(
        expected.association_body.authority_revision < expected.current_head.authority_revision
    );
    assert!(
        AgentReaderValidatedInputsV1::from_packet(
            &serde_json::to_vec(&value).unwrap(),
            expected,
            now(1002)
        )
        .is_ok()
    );
}

#[test]
fn packet_trust_signature_head_and_expected_body_are_independent_fences() {
    let (mut expected, packet) = fixture();
    expected.issuer_pin.issuer_key =
        AgentReaderPublicKey::from_bytes(device().public_keys().signing);
    assert!(matches!(
        AgentReaderValidatedInputsV1::from_packet(&packet, expected, now(1002)),
        Err(AgentReaderError::InvalidSignature)
    ));
    let (expected, packet) = fixture();
    let mut value: Value = serde_json::from_slice(&packet).unwrap();
    value["freshness"]["signature"] = json!("00".repeat(64));
    assert!(matches!(
        AgentReaderValidatedInputsV1::from_packet(
            &serde_json::to_vec(&value).unwrap(),
            expected,
            now(1002)
        ),
        Err(AgentReaderError::InvalidSignature)
    ));
    let (mut expected, packet) = fixture();
    expected.current_head.authority_revision += 1;
    assert!(matches!(
        AgentReaderValidatedInputsV1::from_packet(&packet, expected, now(1002)),
        Err(AgentReaderError::WrongHead)
    ));
    let (mut expected, packet) = fixture();
    expected.association_body.native_host_revision += 1;
    assert!(matches!(
        AgentReaderValidatedInputsV1::from_packet(&packet, expected, now(1002)),
        Err(AgentReaderError::WrongAssociation)
    ));
}

#[test]
fn entropy_failure_is_not_retried_and_prepared_proof_checks_actual_signer_key() {
    let key = device();
    let (expected, packet) = fixture();
    let id = expected.association_body.device_id.as_device_id();
    let inputs = AgentReaderValidatedInputsV1::from_packet(&packet, expected, now(1002)).unwrap();
    let mut calls = 0;
    assert!(matches!(
        inputs.prepare_with_test_entropy(id, key.public_keys().signing, now(1002), &mut |_| {
            calls += 1;
            Err(AgentReaderError::Entropy)
        }),
        Err(AgentReaderError::Entropy)
    ));
    assert_eq!(calls, 1);
    let (expected, packet) = fixture();
    let inputs = AgentReaderValidatedInputsV1::from_packet(&packet, expected, now(1002)).unwrap();
    let prepared = inputs
        .prepare_with_test_entropy(id, key.public_keys().signing, now(1002), &mut |nonce| {
            nonce.fill(7);
            Ok(())
        })
        .unwrap();
    let foreign = crate::DeviceSecretKeys::from_test_seeds(&[2; 32], &[3; 32]);
    assert!(matches!(
        foreign.sign_agent_reader_proof_v1(prepared),
        Err(AgentReaderError::WrongDevice)
    ));
    assert_eq!(
        format!("{:?}", issuer()),
        "ReaderIssuerSecretV1([redacted])"
    );
}

#[test]
fn dedicated_issuer_exposes_only_four_typed_domains_and_enforces_own_pin() {
    let v = vectors();
    let issuer = issuer();
    let mut pin: AgentReaderIssuerPinV1 = decode(v["pin"]["body"].clone());
    let a: AgentReaderAssociationV1 = decode(v["statements"][0]["body"].clone());
    let g: AgentReaderGrantV1 = decode(v["statements"][1]["body"].clone());
    let r: AgentReaderRevocationV1 = decode(v["statements"][3]["body"].clone());
    let f: AgentReaderFreshnessV1 = decode(v["statements"][4]["body"].clone());
    issuer
        .sign_association(a.clone(), &pin)
        .unwrap()
        .verify(pin.issuer_key)
        .unwrap();
    issuer
        .sign_grant(g, &pin)
        .unwrap()
        .verify(pin.issuer_key)
        .unwrap();
    issuer
        .sign_revocation(r, &pin)
        .unwrap()
        .verify(pin.issuer_key)
        .unwrap();
    issuer
        .sign_freshness(f, &pin)
        .unwrap()
        .verify(pin.issuer_key)
        .unwrap();
    pin.issuer_key = AgentReaderPublicKey::from_bytes(device().public_keys().signing);
    assert!(matches!(
        issuer.sign_association(a, &pin),
        Err(AgentReaderError::WrongIssuer)
    ));
}
