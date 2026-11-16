//! Genuine public-key signatures; no Actor/Permit, socket or drainage claim.
use crate::*;
use ed25519_dalek::{Signer, SigningKey};
use serde_json::{json, Value};

fn time(value: u64) -> Timestamp {
    Timestamp::new(value).unwrap()
}
fn policy() -> TunnelRoutePolicyV2 {
    serde_json::from_str(include_str!("tunnel_policy_v2_vector.json")).unwrap()
}
fn sign<T: Statement>(body: T, key: &SigningKey) -> Signed<T> {
    let signature = SignatureBytes::from_bytes(key.sign(&body.signing_bytes().unwrap()).to_bytes());
    Signed { body, signature }
}
fn frame(value: &Value) -> Vec<u8> {
    let mut bytes = Vec::new();
    wire::canonical_json(value, &mut bytes).unwrap();
    bytes.push(b'\n');
    bytes
}
struct Fixture {
    owner: SigningKey,
    node: SigningKey,
    pin: OwnerPin,
    policy: TunnelRoutePolicyV2,
    binding: NodeRequestBinding,
    proof: Signed<AgentRequest>,
}
fn fixture() -> Fixture { fixture_with_resume(true) }
fn fixture_with_resume(resume_original: bool) -> Fixture {
    let owner = SigningKey::from_bytes(&[11; 32]);
    let node = SigningKey::from_bytes(&[44; 32]);
    let mut policy = policy();
    policy.resume_original = resume_original;
    let header = Header {
        version: 1,
        context: policy.route.context.clone(),
        owner_epoch: 1,
        checkpoint: 2,
    };
    let pin = OwnerPin {
        context: header.context.clone(),
        key: PublicKey::from_bytes(owner.verifying_key().to_bytes()),
        epoch: 1,
        checkpoint: 2,
        expires_at: time(2000),
    };
    let cert = sign(
        Certificate {
            header: header.clone(),
            agent_id: policy.route.agent_id,
            key_epoch: 1,
            agent_key: PublicKey::from_bytes(node.verifying_key().to_bytes()),
            scopes: Scopes(vec![Scope::TunnelOpen]),
            capabilities: Capabilities {
                version: 1,
                scopes: Scopes(vec![Scope::TunnelOpen]),
            },
            origin: CertificateOrigin::Enrollment {
                invitation_id: InvitationId::new("aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa").unwrap(),
                claim_hash: Hash::from_bytes([3; 32]),
                key_fingerprint: PublicKey::from_bytes(node.verifying_key().to_bytes())
                    .fingerprint(),
            },
            not_before: time(900),
            expires_at: time(1500),
        },
        &owner,
    );
    policy.route.agent_certificate_hash = cert.body.digest().unwrap();
    let lease = Lease {
        header: header.clone(),
        id: LeaseId::new("bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb").unwrap(),
        agent_id: policy.route.agent_id,
        key_epoch: 1,
        certificate_hash: policy.route.agent_certificate_hash,
        operation: policy.operation().unwrap(),
        issued_at: time(1000),
        expires_at: time(1060),
    };
    let freshness = Freshness {
        header,
        issued_at: time(1000),
        expires_at: time(1060),
    };
    let prepared = PreparedTunnelOwner::grant_original(
        &pin,
        cert,
        &policy,
        lease,
        freshness,
        &policy.route.client_device_id,
        Now(time(1000)),
    )
    .unwrap();
    assert_eq!(
        prepared.expected_device_id(),
        Some(policy.route.client_device_id.as_str())
    );
    assert_eq!(prepared.expected_key(), pin.key);
    let signatures = prepared
        .signing_transcripts()
        .unwrap()
        .iter()
        .map(|v| owner.sign(v).to_bytes())
        .collect();
    let TunnelOwnerSigned::Grant(binding) = prepared.finish(signatures).unwrap() else {
        panic!()
    };
    let proof = sign(
        AgentRequest {
            header: binding.lease.body.header.clone(),
            agent_id: policy.route.agent_id,
            key_epoch: 1,
            lease_hash: binding.lease.body.digest().unwrap(),
            nonce: Nonce::from_bytes([2; 16]),
            issued_at: time(1001),
        },
        &node,
    );
    Fixture {
        owner,
        node,
        pin,
        policy,
        binding,
        proof,
    }
}
fn challenge(f: &Fixture) -> Value {
    json!({"format":"cc-agent-tunnel-channel-challenge/v2","proof":f.proof,"policyHash":f.policy.digest().unwrap(),"expiresAt":1060})
}
fn ack(f: &Fixture) -> Value {
    json!({"format":"cc-agent-tunnel-open-ack/v2","proof":f.proof,"policyHash":f.policy.digest().unwrap(),"expiresAt":1060})
}
fn pending(f: &Fixture) -> PendingOriginalChallenge {
    verify_original_challenge(
        &frame(&challenge(f)),
        &f.pin,
        &f.binding,
        &f.policy,
        Now(time(1001)),
    )
    .unwrap()
}

#[test]
fn original_v2_golden_full_wrapper_hash_and_legacy_bytes_stay_distinct() {
    let p = policy();
    let vector: Value =
        serde_json::from_str(include_str!("tunnel_v2_public_vectors.json")).unwrap();
    assert_eq!(
        serde_json::to_value(p.route.digest().unwrap()).unwrap(),
        json!("4b3315b5dd76ab0f8ecfd687c920918bea3fc8e62f50610a2499cfbfc369b07d")
    );
    assert_eq!(
        serde_json::to_value(p.digest().unwrap()).unwrap(),
        json!("4f3ab49bc360fa8a992997663004e858b8933beba92559cc5f04ba5949a695db")
    );
    let mut bytes = Vec::new();
    wire::canonical_json(&serde_json::to_value(&p).unwrap(), &mut bytes).unwrap();
    assert_eq!(bytes.len(), 1169);
    assert_eq!(
        bytes,
        vector["V2_wrapper"]["canonical_utf8"]
            .as_str()
            .unwrap()
            .as_bytes()
    );
    let mut legacy = Vec::new();
    wire::canonical_json(&serde_json::to_value(&p.route).unwrap(), &mut legacy).unwrap();
    assert_eq!(
        legacy,
        vector["V1_existing"]["canonical_utf8"]
            .as_str()
            .unwrap()
            .as_bytes()
    );
    assert_eq!(
        p.operation().unwrap(),
        Operation::TunnelOpenBound {
            route: p.route.route_id,
            policy_hash: p.digest().unwrap()
        }
    );
    assert_ne!(p.operation().unwrap(), p.route.operation().unwrap());
}

#[test]
fn original_v2_shared_unsigned_transcripts_keep_existing_domains_and_body_digests() {
    let vector: Value =
        serde_json::from_str(include_str!("tunnel_v2_public_vectors.json")).unwrap();
    let lease: Lease =
        serde_json::from_value(vector["unsigned_lease_body"]["body"].clone()).unwrap();
    assert_eq!(
        hex::encode(lease.signing_bytes().unwrap()),
        vector["unsigned_lease_body"]["signing_transcript_hex"]
            .as_str()
            .unwrap()
    );
    assert_eq!(
        serde_json::to_value(lease.digest().unwrap()).unwrap(),
        vector["unsigned_lease_body"]["body_digest_leaseHash"]
    );
    let proof: AgentRequest =
        serde_json::from_value(vector["unsigned_AgentRequest_body"]["body"].clone()).unwrap();
    assert_eq!(
        hex::encode(proof.signing_bytes().unwrap()),
        vector["unsigned_AgentRequest_body"]["signing_transcript_hex"]
            .as_str()
            .unwrap()
    );
    assert_eq!(
        serde_json::to_value(proof.digest().unwrap()).unwrap(),
        vector["unsigned_AgentRequest_body"]["body_digest"]
    );
}
#[test]
fn original_v2_owner_signer_existing_purpose_and_real_challenge_echo_ack_roundtrip() {
    let f = fixture();
    verify_binding_v2(&f.pin, &f.binding, &f.policy, Now(time(1001))).unwrap();
    assert!(verify_binding(&f.pin, &f.binding, &f.policy.route, Now(time(1001))).is_err());
    for (mode, word) in [
        (OriginalTunnelMode::Admit, "admit"),
        (OriginalTunnelMode::Resume, "resume"),
    ] {
        let raw = open_original_frame(&f.binding, &f.policy, mode).unwrap();
        let v: Value = serde_json::from_slice(&raw).unwrap();
        assert_eq!(v["format"], "cc-agent-tunnel-open/v2");
        assert_eq!(v["mode"], word);
        assert_eq!(v.as_object().unwrap().len(), 3);
        let mut p = pending(&f);
        let echo = p.echo_once().unwrap();
        assert_eq!(
            echo,
            frame(
                &json!({"format":"cc-agent-tunnel-channel-echo/v2","nonce":f.proof.body.nonce,"policyHash":f.policy.digest().unwrap(),"leaseHash":f.binding.lease.body.digest().unwrap(),"deviceId":f.policy.route.client_device_id})
            )
        );
        assert_eq!(
            verify_original_ack(
                &frame(&ack(&f)),
                p,
                &f.pin,
                &f.binding,
                &f.policy,
                Now(time(1002))
            )
            .unwrap()
            .expires_at(),
            time(1060)
        );
    }
    // Existing Statement domain/header: no V2 signing purpose is introduced.
    let signed = f.binding.lease.body.signing_bytes().unwrap();
    assert!(signed
        .windows(b"operation-lease".len())
        .any(|v| v == b"operation-lease"));
    assert_eq!(f.binding.lease.body.header.version, 1);
}
#[test]
fn original_v2_echo_once_and_ack_before_echo_reject_replay_phases() {
    let f = fixture();
    let mut p = pending(&f);
    p.echo_once().unwrap();
    assert!(p.echo_once().is_err());
    assert!(verify_original_ack(
        &frame(&ack(&f)),
        pending(&f),
        &f.pin,
        &f.binding,
        &f.policy,
        Now(time(1001))
    )
    .is_err());
}
#[test]
fn original_v2_rejects_v1_hash_unbound_or_foreign_device_before_signing() {
    let f = fixture();
    for operation in [
        f.policy.route.operation().unwrap(),
        Operation::TunnelOpen {
            route: f.policy.route.route_id,
        },
    ] {
        let mut lease = f.binding.lease.body.clone();
        lease.operation = operation;
        assert!(PreparedTunnelOwner::grant_original(
            &f.pin,
            f.binding.certificate.clone(),
            &f.policy,
            lease,
            f.binding.freshness.body.clone(),
            &f.policy.route.client_device_id,
            Now(time(1001))
        )
        .is_err());
    }
    assert!(PreparedTunnelOwner::grant_original(
        &f.pin,
        f.binding.certificate.clone(),
        &f.policy,
        f.binding.lease.body.clone(),
        f.binding.freshness.body.clone(),
        "cccccccc-cccc-4ccc-8ccc-cccccccccccc",
        Now(time(1001))
    )
    .is_err());
    assert!(PreparedTunnelOwner::grant(
        &f.pin,
        f.binding.certificate.clone(),
        &f.policy.route,
        f.binding.lease.body.clone(),
        f.binding.freshness.body.clone(),
        &f.policy.route.client_device_id,
        Now(time(1001))
    )
    .is_err());
}
#[test]
fn original_v2_policy_requires_a_boolean_and_rejects_missing_unknown_duplicates_and_float() {
    let p = serde_json::to_value(policy()).unwrap();
    for value in [json!(1), json!("true"), Value::Null] {
        let mut v = p.clone();
        v["resumeOriginal"] = value;
        assert!(serde_json::from_value::<TunnelRoutePolicyV2>(v)
            .map(|p| p.validate().is_err())
            .unwrap_or(true));
    }
    let mut v = p.clone();
    v.as_object_mut().unwrap().remove("resumeOriginal");
    assert!(serde_json::from_value::<TunnelRoutePolicyV2>(v).is_err());
    let mut v = p.clone();
    v["unknown"] = json!(1);
    assert!(serde_json::from_value::<TunnelRoutePolicyV2>(v).is_err());
    let mut v = p;
    v["route"]["limits"]["maxChannels"] = json!(1.0);
    assert!(serde_json::from_value::<TunnelRoutePolicyV2>(v).is_err());
    let raw = include_str!("tunnel_policy_v2_vector.json").replacen(
        "\"resumeOriginal\": true",
        "\"resumeOriginal\": true, \"resumeOriginal\": true",
        1,
    );
    assert!(serde_json::from_str::<TunnelRoutePolicyV2>(&raw).is_err());
    for raw in ["\"future\"", "true", "1"] {
        assert!(serde_json::from_str::<OriginalTunnelMode>(raw).is_err());
    }
}
#[test]
fn original_v2_challenge_strict_shape_canonical_frame_bounds() {
    let f = fixture();
    let base = frame(&challenge(&f));
    let mut cases = vec![vec![], vec![b'x'; 16385], base[..base.len() - 1].to_vec()];
    let mut cr = base.clone();
    cr.insert(cr.len() - 1, b'\r');
    cases.push(cr);
    let mut lines = base.clone();
    lines.push(b'\n');
    cases.push(lines);
    let mut whitespace = base.clone();
    whitespace.insert(1, b' ');
    cases.push(whitespace);
    let duplicate = String::from_utf8(base.clone())
        .unwrap()
        .replace("\"keyEpoch\":1", "\"keyEpoch\":1,\"keyEpoch\":1");
    cases.push(duplicate.into_bytes());
    for (field, value) in [
        ("unknown", json!(1)),
        ("format", json!("cc-agent-tunnel-open-ack/v2")),
    ] {
        let mut v = challenge(&f);
        v[field] = value;
        cases.push(frame(&v));
    }
    cases.push(
        String::from_utf8(base)
            .unwrap()
            .replace("\"expiresAt\":1060", "\"expiresAt\":1060.0")
            .into_bytes(),
    );
    for raw in cases {
        assert!(
            verify_original_challenge(&raw, &f.pin, &f.binding, &f.policy, Now(time(1001)))
                .is_err()
        );
    }
}
#[test]
fn original_v2_challenge_rejects_changed_claim_signature_epoch_and_old_proof() {
    let mut f = fixture();
    let base = f.proof.clone();
    for which in 0..8 {
        let mut body = base.body.clone();
        match which {
            0 => body.header.checkpoint += 1,
            1 => body.key_epoch += 1,
            2 => body.lease_hash = Hash::from_bytes([8; 32]),
            3 => body.agent_id = AgentId::new("cccccccc-cccc-4ccc-8ccc-cccccccccccc").unwrap(),
            4 => body.issued_at = time(900),
            5 => body.issued_at = time(1002),
            _ => {}
        }
        f.proof = sign(body, if which == 6 { &f.owner } else { &f.node });
        if which == 7 {
            f.proof.body.nonce = Nonce::from_bytes([0; 16]);
        }
        assert!(
            verify_original_challenge(
                &frame(&challenge(&f)),
                &f.pin,
                &f.binding,
                &f.policy,
                Now(time(1001))
            )
            .is_err(),
            "{which}"
        );
    }
    f.proof = base;
    for (key, value) in [
        ("policyHash", json!("88".repeat(32))),
        ("expiresAt", json!(1059)),
    ] {
        let mut v = challenge(&f);
        v[key] = value;
        assert!(verify_original_challenge(
            &frame(&v),
            &f.pin,
            &f.binding,
            &f.policy,
            Now(time(1001))
        )
        .is_err());
    }
}
#[test]
fn original_v2_ack_requires_exact_pending_proof_hash_expiry_and_original_binding() {
    let f = fixture();
    for which in 0..7 {
        let mut p = pending(&f);
        p.echo_once().unwrap();
        let mut v = ack(&f);
        match which {
            0 => v["format"] = json!("cc-agent-tunnel-open-ack/v1"),
            1 => v["expiresAt"] = json!(1059),
            2 => v["policyHash"] = json!("88".repeat(32)),
            3 => {
                let mut b = f.proof.body.clone();
                b.nonce = Nonce::from_bytes([3; 16]);
                v["proof"] = serde_json::to_value(sign(b, &f.node)).unwrap();
            }
            4 => v["proof"]["signature"] = json!("00".repeat(64)),
            5 => v["route"] = json!(f.policy.route.route_id),
            _ => v["proof"]["body"]["unexpected"] = json!(1),
        };
        assert!(
            verify_original_ack(
                &frame(&v),
                p,
                &f.pin,
                &f.binding,
                &f.policy,
                Now(time(1001))
            )
            .is_err(),
            "{which}"
        );
    }
    let mut p = pending(&f);
    p.echo_once().unwrap();
    let mut b = f.binding.clone();
    b.freshness = sign(
        Freshness {
            expires_at: time(1059),
            ..b.freshness.body.clone()
        },
        &f.owner,
    );
    assert!(
        verify_original_ack(&frame(&ack(&f)), p, &f.pin, &b, &f.policy, Now(time(1001))).is_err()
    );
}
#[test]
fn original_v2_current_pin_policy_or_expiry_change_cannot_resume_or_renew() {
    let f = fixture();
    for which in 0..5 {
        let mut p = pending(&f);
        p.echo_once().unwrap();
        let mut pin = f.pin.clone();
        let mut policy = f.policy.clone();
        let mut now = Now(time(1001));
        match which {
            0 => pin.checkpoint += 1,
            1 => pin.epoch += 1,
            2 => {
                pin.key = PublicKey::from_bytes(
                    SigningKey::from_bytes(&[77; 32]).verifying_key().to_bytes(),
                )
            }
            3 => policy.route.destination.port += 1,
            _ => now = Now(time(1060)),
        };
        assert!(verify_original_ack(&frame(&ack(&f)), p, &pin, &f.binding, &policy, now).is_err());
    }
}

#[test]
fn original_v2_signed_false_allows_first_admit_but_never_resume_or_bit_retarget() {
    let f = fixture_with_resume(false);
    verify_binding_v2(&f.pin, &f.binding, &f.policy, Now(time(1001))).unwrap();
    let raw = open_original_frame(&f.binding, &f.policy, OriginalTunnelMode::Admit).unwrap();
    let value: Value = serde_json::from_slice(&raw).unwrap();
    assert_eq!(value["mode"], "admit");
    assert_eq!(open_original_frame(&f.binding, &f.policy, OriginalTunnelMode::Resume).unwrap_err(), Error::ScopeDenied);
    let mut altered = f.policy.clone();
    altered.resume_original = true;
    assert_ne!(f.policy.digest().unwrap(), altered.digest().unwrap());
    assert!(verify_binding_v2(&f.pin, &f.binding, &altered, Now(time(1001))).is_err());
    // First admission still uses actual owner signatures and Node challenge/ACK.
    let mut challenge = pending(&f);
    challenge.echo_once().unwrap();
    verify_original_ack(&frame(&ack(&f)), challenge, &f.pin, &f.binding, &f.policy, Now(time(1002))).unwrap();
}
