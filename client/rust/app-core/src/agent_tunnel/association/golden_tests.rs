//! Cross-implementation compatibility with genuine recorded Agent output.
//! Fixed historical fixture time only; no live SSH, current owner or admission.
use super::*;
use protocol::{Claimed, Committed, NodeIdentity, Probe};
use serde::{Deserialize, de::DeserializeOwned};
use serde_json::{Value, json};

const MANIFEST_SHA256: &str = "0ec1a4e94d8ff52fcd49a108fef105f571db15830d92296e26c3479c1a8dbf06";
const FILES: [(&str, &[u8]); 9] = [
    (
        "GOLDEN_MANIFEST.jsonl",
        include_bytes!("testdata/enrollment_v1/GOLDEN_MANIFEST.jsonl"),
    ),
    (
        "01-probe.request.jsonl",
        include_bytes!("testdata/enrollment_v1/01-probe.request.jsonl"),
    ),
    (
        "01-probe.reply.jsonl",
        include_bytes!("testdata/enrollment_v1/01-probe.reply.jsonl"),
    ),
    (
        "02-stageOffer.request.jsonl",
        include_bytes!("testdata/enrollment_v1/02-stageOffer.request.jsonl"),
    ),
    (
        "02-stageOffer.reply.jsonl",
        include_bytes!("testdata/enrollment_v1/02-stageOffer.reply.jsonl"),
    ),
    (
        "03-applyCertificate.request.jsonl",
        include_bytes!("testdata/enrollment_v1/03-applyCertificate.request.jsonl"),
    ),
    (
        "03-applyCertificate.reply.jsonl",
        include_bytes!("testdata/enrollment_v1/03-applyCertificate.reply.jsonl"),
    ),
    (
        "04-observeCertificate.request.jsonl",
        include_bytes!("testdata/enrollment_v1/04-observeCertificate.request.jsonl"),
    ),
    (
        "04-observeCertificate.reply.jsonl",
        include_bytes!("testdata/enrollment_v1/04-observeCertificate.reply.jsonl"),
    ),
];
fn raw(name: &str) -> &'static [u8] {
    FILES
        .iter()
        .find(|(n, _)| *n == name)
        .expect("closed repository fixture name")
        .1
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct FilePin {
    bytes: usize,
    file: String,
    sha256: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Pair {
    kind: String,
    request: FilePin,
    reply: FilePin,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Manifest {
    actual_final_checkpoint: u64,
    actual_final_revision: u64,
    certificate_digest: String,
    claim_digest: String,
    expires_as_recorded_not_live_authority: bool,
    fixture_owner_seed_decimal: u8,
    fixture_token_byte_hex: String,
    format: String,
    genesis_owner_pin: OwnerPin,
    node_key_and_nonce: String,
    observed_at: u64,
    pairs: Vec<Pair>,
    private_node_and_journal_removed: bool,
    product_ssh_authentication_proved: bool,
    public_node: NodeIdentity,
    synthetic_fixture_only: bool,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Request<T> {
    format: String,
    request_id: Uuid,
    kind: String,
    payload: T,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Empty {}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Stage {
    expected_actor_revision: u64,
    expected_owner_pin: OwnerPin,
    offer: Signed<Offer>,
    token: String, // Deliberately public, expired synthetic fixture token; never Debug/logged.
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Apply {
    expected_rev: u64,
    expected_pin: OwnerPin,
    claim_digest: Hash,
    certificate: Signed<Certificate>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Observe {
    expected_rev: u64,
    expected_pin: OwnerPin,
    agent_id: AgentId,
    certificate_digest: Hash,
}
fn request<T: DeserializeOwned>(name: &str, kind: &str) -> Request<T> {
    let bytes = raw(name);
    let value: Value = serde_json::from_slice(bytes).expect("recorded envelope JSON");
    assert!(
        protocol::canonical(&value).unwrap().as_slice() == bytes,
        "recorded envelope canonical bytes"
    );
    let id = value["requestId"].as_str().unwrap().to_owned();
    let result: Request<T> = serde_json::from_value(value).expect("closed actual request payload");
    assert_eq!(result.format, protocol::FORMAT);
    assert_eq!(result.kind, kind);
    assert!(!result.request_id.is_nil());
    assert_eq!(result.request_id.to_string(), id);
    result
}
struct Captured {
    manifest: Manifest,
    probe: Probe,
    stage: Request<Stage>,
    claim: Claimed,
    apply: Request<Apply>,
    applied: Committed,
    observe: Request<Observe>,
    observed: Committed,
}
impl Captured {
    fn now(&self) -> Now {
        Now(Timestamp::new(self.manifest.observed_at).unwrap())
    }
}
fn captured() -> Captured {
    assert_eq!(hash(raw("GOLDEN_MANIFEST.jsonl")), MANIFEST_SHA256);
    let manifest: Manifest = serde_json::from_slice(raw("GOLDEN_MANIFEST.jsonl")).unwrap();
    assert_eq!(manifest.format, "cc-actual-enrollment-public-golden/v1");
    assert!(manifest.synthetic_fixture_only && manifest.expires_as_recorded_not_live_authority);
    assert!(
        manifest.private_node_and_journal_removed && !manifest.product_ssh_authentication_proved
    );
    assert_eq!(manifest.fixture_owner_seed_decimal, 11); // Never constructs a Native signer.
    assert_eq!(manifest.fixture_token_byte_hex, "21");
    assert_eq!(
        manifest.node_key_and_nonce,
        "actual OS custody, never exported"
    );
    assert_eq!(manifest.pairs.len(), 4);
    for (kind, pair) in [
        "probe",
        "stageOffer",
        "applyCertificate",
        "observeCertificate",
    ]
    .iter()
    .zip(&manifest.pairs)
    {
        assert_eq!(pair.kind.as_str(), *kind);
        for pin in [&pair.request, &pair.reply] {
            let body = raw(&pin.file);
            assert_eq!(body.len(), pin.bytes);
            assert_eq!(hash(body), pin.sha256);
            assert!(body.len() <= protocol::MAX_FRAME && body.is_ascii());
        }
    }
    let probe_request: Request<Empty> = request("01-probe.request.jsonl", "probe");
    let _closed_empty = probe_request.payload;
    let probe: Probe = protocol::reply(
        raw("01-probe.reply.jsonl"),
        probe_request.request_id,
        "probe",
    )
    .unwrap();
    let stage: Request<Stage> = request("02-stageOffer.request.jsonl", "stageOffer");
    let claim = protocol::reply(
        raw("02-stageOffer.reply.jsonl"),
        stage.request_id,
        "stageOffer",
    )
    .unwrap();
    let apply: Request<Apply> = request("03-applyCertificate.request.jsonl", "applyCertificate");
    let applied = protocol::reply(
        raw("03-applyCertificate.reply.jsonl"),
        apply.request_id,
        "applyCertificate",
    )
    .unwrap();
    let observe: Request<Observe> =
        request("04-observeCertificate.request.jsonl", "observeCertificate");
    let observed = protocol::reply(
        raw("04-observeCertificate.reply.jsonl"),
        observe.request_id,
        "observeCertificate",
    )
    .unwrap();
    let ids = [
        probe_request.request_id,
        stage.request_id,
        apply.request_id,
        observe.request_id,
    ];
    assert_eq!(
        ids.into_iter()
            .collect::<std::collections::BTreeSet<_>>()
            .len(),
        4
    );
    Captured {
        manifest,
        probe,
        stage,
        claim,
        apply,
        applied,
        observe,
        observed,
    }
}

#[test]
fn actual_recorded_node_golden_frames_bind_crypto_claim_certificate_and_two_actor_commits() {
    let c = captured();
    let now = c.now(); // Fixed historical observation ONLY, never a live authority clock.
    assert!(c.probe.node == c.manifest.public_node);
    assert!(c.probe.owner_pin == c.manifest.genesis_owner_pin);
    assert!(c.probe.certificate.is_none() && !c.probe.revoked);
    assert_eq!(c.probe.actor_revision, 0);
    assert_eq!(c.probe.node.format, 1);
    assert_eq!(c.probe.node.key_epoch, 1);
    c.probe.node.public_key.validate().unwrap();
    assert!(c.probe.node.key_fingerprint == c.probe.node.public_key.fingerprint());
    validate_pin(&c.probe.owner_pin, now).unwrap();
    assert_eq!(
        c.stage.payload.expected_actor_revision,
        c.probe.actor_revision
    );
    assert!(c.stage.payload.expected_owner_pin == c.probe.owner_pin);
    verify_statement(&c.stage.payload.offer, c.probe.owner_pin.key).unwrap();
    PreparedTunnelOwner::offer(&c.probe.owner_pin, c.stage.payload.offer.body.clone(), now)
        .unwrap();
    let token = Zeroizing::new(hex::decode(&c.stage.payload.token).unwrap());
    assert_eq!(token.len(), 32);
    assert!(token.iter().all(|byte| *byte == 0x21));
    let token = InvitationToken::from_bytes(token.as_slice().try_into().unwrap()).unwrap();
    assert!(token.digest() == c.stage.payload.offer.body.token_hash);
    // Production Native validators consume the ACTUAL Agent reply bytes.
    protocol::claimed(&c.probe, &c.stage.payload.offer, &c.claim, now).unwrap();
    verify_statement(&c.claim.claim, c.probe.node.public_key).unwrap();
    assert_eq!(
        hex::encode(c.claim.claim_digest.as_bytes()),
        c.manifest.claim_digest
    );
    assert!(c.claim.claim.body.offer_hash == c.stage.payload.offer.body.digest().unwrap());
    assert_eq!(c.claim.actor_revision, c.probe.actor_revision + 2);
    assert_eq!(
        c.claim.owner_pin.checkpoint,
        c.probe.owner_pin.checkpoint + 1
    );
    assert_eq!(c.apply.payload.expected_rev, c.claim.actor_revision);
    assert!(c.apply.payload.expected_pin == c.claim.owner_pin);
    assert!(c.apply.payload.claim_digest == c.claim.claim_digest);
    let certificate = &c.apply.payload.certificate;
    verify_statement(certificate, c.probe.owner_pin.key).unwrap();
    PreparedTunnelOwner::certificate(
        &c.claim.owner_pin,
        c.stage.payload.offer.clone(),
        c.claim.claim.clone(),
        certificate.body.clone(),
        now,
    )
    .unwrap();
    assert_eq!(
        hex::encode(certificate.body.digest().unwrap().as_bytes()),
        c.manifest.certificate_digest
    );
    protocol::committed(&c.probe, certificate, &c.applied, now).unwrap();
    protocol::committed(&c.probe, certificate, &c.observed, now).unwrap();
    assert_eq!(c.observe.payload.expected_rev, c.applied.actor_revision);
    assert!(c.observe.payload.expected_pin == c.applied.owner_pin);
    assert!(c.observe.payload.agent_id == c.probe.node.agent_id);
    assert!(c.observe.payload.certificate_digest == c.applied.certificate_digest);
    assert!(c.observed.certificate == c.applied.certificate);
    assert_eq!(c.observed.actor_revision, c.manifest.actual_final_revision);
    assert_eq!(
        c.observed.owner_pin.checkpoint,
        c.manifest.actual_final_checkpoint
    );
    assert_eq!(c.observed.actor_revision, c.probe.actor_revision + 3);
    assert_eq!(
        c.observed.owner_pin.checkpoint,
        c.probe.owner_pin.checkpoint + 2
    );
}

#[test]
fn actual_golden_closed_envelopes_reject_foreign_correlation_unknown_fields_and_noncanonical_frames()
 {
    let c = captured();
    assert!(
        protocol::reply::<Claimed>(
            raw("02-stageOffer.reply.jsonl"),
            c.apply.request_id,
            "stageOffer"
        )
        .is_err()
    );
    assert!(
        protocol::reply::<Claimed>(
            raw("02-stageOffer.reply.jsonl"),
            c.stage.request_id,
            "applyCertificate"
        )
        .is_err()
    );
    let probe_request: Request<Empty> = request("01-probe.request.jsonl", "probe");
    for path in ["top", "node", "missingCertificate"] {
        let mut value: Value = serde_json::from_slice(raw("01-probe.reply.jsonl")).unwrap();
        match path {
            "top" => value["unexpected"] = json!(true),
            "node" => value["payload"]["node"]["unexpected"] = json!(true),
            _ => {
                value["payload"]
                    .as_object_mut()
                    .unwrap()
                    .remove("certificate");
            }
        }
        let bytes = protocol::canonical(&value).unwrap();
        assert!(protocol::reply::<Probe>(&bytes, probe_request.request_id, "probe").is_err());
    }
    let mut trailing = raw("01-probe.reply.jsonl").to_vec();
    trailing.push(b'\n');
    assert!(protocol::reply::<Probe>(&trailing, probe_request.request_id, "probe").is_err());
    let mut value: Value = serde_json::from_slice(raw("02-stageOffer.request.jsonl")).unwrap();
    value["payload"]["callerAuthority"] = json!(true);
    assert!(serde_json::from_value::<Request<Stage>>(value).is_err());
}

#[test]
fn genuine_signed_golden_rejects_claim_digest_actor_checkpoint_signature_and_expired_window_changes()
 {
    let mut c = captured();
    let now = c.now();
    c.claim.claim_digest = Hash::from_bytes([0; 32]);
    assert!(protocol::claimed(&c.probe, &c.stage.payload.offer, &c.claim, now).is_err());
    let mut c = captured();
    c.claim.actor_revision += 1;
    assert!(protocol::claimed(&c.probe, &c.stage.payload.offer, &c.claim, now).is_err());
    let mut c = captured();
    c.claim.owner_pin.checkpoint += 1;
    assert!(protocol::claimed(&c.probe, &c.stage.payload.offer, &c.claim, now).is_err());
    let mut c = captured();
    c.claim.claim.signature = SignatureBytes::from_bytes([0; 64]);
    assert!(protocol::claimed(&c.probe, &c.stage.payload.offer, &c.claim, now).is_err());
    let mut c = captured();
    c.applied.certificate_digest = Hash::from_bytes([0; 32]);
    assert!(protocol::committed(&c.probe, &c.apply.payload.certificate, &c.applied, now).is_err());
    let mut c = captured();
    c.applied.actor_revision += 1;
    assert!(protocol::committed(&c.probe, &c.apply.payload.certificate, &c.applied, now).is_err());
    let mut c = captured();
    c.applied.owner_pin.checkpoint += 1;
    assert!(protocol::committed(&c.probe, &c.apply.payload.certificate, &c.applied, now).is_err());
    let mut c = captured();
    c.apply.payload.certificate.signature = SignatureBytes::from_bytes([0; 64]);
    assert!(protocol::committed(&c.probe, &c.apply.payload.certificate, &c.applied, now).is_err());
    let c = captured();
    let expired = Now(c
        .apply
        .payload
        .certificate
        .body
        .expires_at
        .checked_add(1)
        .unwrap());
    assert!(
        protocol::committed(&c.probe, &c.apply.payload.certificate, &c.applied, expired).is_err()
    );
}
