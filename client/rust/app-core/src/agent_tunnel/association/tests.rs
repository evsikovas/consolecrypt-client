//! Genuine crypto/original Local SQL/IO causals; no Agent installation claim.
use super::*;
use protocol::{Claimed, Committed, NodeIdentity, Probe};
fn time(s: u64) -> Timestamp {
    Timestamp::new(s).unwrap()
}
struct Key(rcgen::KeyPair);
impl Key {
    fn new() -> Self {
        Self(rcgen::KeyPair::generate_for(&rcgen::PKCS_ED25519).unwrap())
    }
    fn public(&self) -> PublicKey {
        PublicKey::from_bytes(self.0.public_key_raw().try_into().unwrap())
    }
    fn sign<T: Statement>(&self, body: T) -> Signed<T> {
        let key = rustls::crypto::ring::sign::any_supported_type(
            &rustls::pki_types::PrivatePkcs8KeyDer::from(self.0.serialize_der()).into(),
        )
        .unwrap();
        let signer = key
            .choose_scheme(&[rustls::SignatureScheme::ED25519])
            .unwrap();
        let signature = signer.sign(&body.signing_bytes().unwrap()).unwrap();
        Signed {
            body,
            signature: SignatureBytes::from_bytes(signature.as_slice().try_into().unwrap()),
        }
    }
}
fn fixture() -> (
    Probe,
    Signed<Offer>,
    Claimed,
    Signed<Certificate>,
    Committed,
) {
    let owner = Key::new();
    let node = Key::new();
    let p: TunnelRoutePolicy = serde_json::from_str(include_str!(
        "../../../../agent-node-wire/src/tunnel_policy_vector.json"
    ))
    .unwrap();
    let pin = OwnerPin {
        context: p.context,
        key: owner.public(),
        epoch: 1,
        checkpoint: 7,
        expires_at: time(2000),
    };
    let probe = Probe {
        node: NodeIdentity {
            format: 1,
            agent_id: p.agent_id,
            key_epoch: 1,
            public_key: node.public(),
            key_fingerprint: node.public().fingerprint(),
        },
        owner_pin: pin.clone(),
        actor_revision: 19,
        certificate: None,
        revoked: false,
    };
    let offer = owner.sign(Offer {
        header: Header {
            version: 1,
            context: pin.context.clone(),
            owner_epoch: 1,
            checkpoint: 8,
        },
        invitation_id: InvitationId::new(&Uuid::new_v4().to_string()).unwrap(),
        agent_id: p.agent_id,
        agent_key: node.public(),
        token_hash: Hash::from_bytes([9; 32]),
        scopes: Scopes(vec![Scope::TunnelOpen]),
        issued_at: time(1000),
        expires_at: time(1060),
    });
    let claim = node.sign(Claim {
        header: offer.body.header.clone(),
        invitation_id: offer.body.invitation_id,
        offer_hash: offer.body.digest().unwrap(),
        agent_id: p.agent_id,
        agent_key: node.public(),
        capabilities: Capabilities {
            version: 1,
            scopes: Scopes(vec![Scope::TunnelOpen]),
        },
        nonce: Nonce::from_bytes([6; 16]),
        issued_at: time(1001),
    });
    let claim = Claimed {
        claim_digest: claim.body.digest().unwrap(),
        claim,
        owner_pin: protocol::advance(&pin, 1).unwrap(),
        actor_revision: 21,
    };
    let cert = owner.sign(Certificate {
        header: Header {
            version: 1,
            context: pin.context.clone(),
            owner_epoch: 1,
            checkpoint: 9,
        },
        agent_id: p.agent_id,
        key_epoch: 1,
        agent_key: node.public(),
        scopes: Scopes(vec![Scope::TunnelOpen]),
        capabilities: claim.claim.body.capabilities.clone(),
        origin: CertificateOrigin::Enrollment {
            invitation_id: offer.body.invitation_id,
            claim_hash: claim.claim_digest,
            key_fingerprint: node.public().fingerprint(),
        },
        not_before: time(1001),
        expires_at: time(1500),
    });
    let committed = Committed {
        certificate: cert.body.clone(),
        certificate_digest: cert.body.digest().unwrap(),
        node_fingerprint: node.public().fingerprint(),
        owner_pin: protocol::advance(&pin, 2).unwrap(),
        actor_revision: 22,
        revoked: false,
    };
    (probe, offer, claim, cert, committed)
}
#[test]
fn canonical_frame_rejects_duplicate_nested_float_alias_correlation_and_extra_bytes() {
    let id = Uuid::new_v4();
    let good = serde_json::json!({"format":protocol::FORMAT,"requestId":id,"kind":"probe","payload":{"n":1}});
    let bytes = protocol::canonical(&good).unwrap();
    let parsed: serde_json::Value = protocol::reply(&bytes, id, "probe").unwrap();
    assert_eq!(parsed["n"], 1);
    assert!(protocol::reply::<serde_json::Value>(&bytes, Uuid::new_v4(), "probe").is_err());
    for bad in [
        String::from_utf8(bytes.to_vec())
            .unwrap()
            .replace("\"n\":1", "\"n\":1,\"n\":1"),
        String::from_utf8(bytes.to_vec())
            .unwrap()
            .replace("\"n\":1", "\"n\":1.0"),
        String::from_utf8(bytes.to_vec())
            .unwrap()
            .replace("\"payload\"", "\"metadata\""),
        format!("{}\n", std::str::from_utf8(&bytes).unwrap()),
        std::str::from_utf8(&bytes).unwrap().replace("\n", "\r\n"),
    ] {
        assert!(protocol::reply::<serde_json::Value>(bad.as_bytes(), id, "probe").is_err());
    }
    let error=protocol::canonical(&serde_json::json!({"format":protocol::FORMAT,"requestId":id,"kind":"error","payload":{"code":"bootstrapUnenrolled","effect":"notCommitted"}})).unwrap();
    assert!(
        protocol::reply::<Probe>(&error, id, "probe")
            .err()
            .unwrap()
            .to_string()
            .contains("bootstrap_unenrolled")
    );
    let old=protocol::canonical(&serde_json::json!({"format":protocol::FORMAT,"requestId":id,"kind":"error","code":"bootstrapUnenrolled","payload":{"effect":"notCommitted"}})).unwrap();
    assert!(protocol::reply::<Probe>(&old, id, "probe").is_err());
}
#[test]
fn zeroizing_stage_wire_is_exact_contract_not_old_aliases() {
    let (p, o, _, _, _) = fixture();
    let token = "a".repeat(64);
    let (id, raw) = protocol::stage_request(p.actor_revision, &p.owner_pin, &o, &token).unwrap();
    let expected=protocol::canonical(&serde_json::json!({"format":protocol::FORMAT,"requestId":id,"kind":"stageOffer","payload":{
        "expectedActorRevision":p.actor_revision,"expectedOwnerPin":p.owner_pin,"offer":o,"token":token}})).unwrap();
    assert_eq!(raw.as_slice(), expected.as_slice());
    assert!(protocol::stage_request(p.actor_revision, &p.owner_pin, &o, &"A".repeat(64)).is_err());
}
#[test]
fn actual_signed_claim_requires_preoffer_current_pin_and_exact_two_commits() {
    let (p, o, c, _, _) = fixture();
    let now = Now(time(1002));
    protocol::claimed(&p, &o, &c, now).unwrap();
    for key in ["claimDigest", "actorRevision", "ownerPin"] {
        let mut bad = serde_json::json!({"claim":c.claim,"claimDigest":c.claim_digest,"ownerPin":c.owner_pin,"actorRevision":c.actor_revision});
        match key {
            "claimDigest" => bad[key] = serde_json::json!(Hash::from_bytes([1; 32])),
            "actorRevision" => bad[key] = serde_json::json!(20),
            _ => bad[key]["checkpoint"] = serde_json::json!(p.owner_pin.checkpoint),
        }
        let bad: Claimed = serde_json::from_value(bad).unwrap();
        assert!(protocol::claimed(&p, &o, &bad, now).is_err());
    }
    let mut wrong = c.claim.clone();
    wrong.signature = SignatureBytes::from_bytes([0; 64]);
    let bad = Claimed {
        claim: wrong,
        claim_digest: c.claim_digest,
        owner_pin: c.owner_pin.clone(),
        actor_revision: c.actor_revision,
    };
    assert!(protocol::claimed(&p, &o, &bad, now).is_err());
    assert!(protocol::claimed(&p, &o, &c, Now(time(1060))).is_err());
    // Using the genesis/post-offer pin instead of ACTUAL pre-offer pin fails.
    let mut wrong_pin = p;
    wrong_pin.owner_pin.checkpoint = 0;
    assert!(protocol::claimed(&wrong_pin, &o, &c, now).is_err());
}
#[test]
fn certificate_ack_exact_body_signature_checkpoint_and_node_fingerprint_only() {
    let (p, _, _, cert, c) = fixture();
    let now = Now(time(1002));
    protocol::committed(&p, &cert, &c, now).unwrap();
    let good = serde_json::json!({"certificate":c.certificate,"certificateDigest":c.certificate_digest,"nodeFingerprint":c.node_fingerprint,
        "ownerPin":c.owner_pin,"actorRevision":c.actor_revision,"revoked":false});
    for field in [
        "actorRevision",
        "nodeFingerprint",
        "revoked",
        "ownerPin",
        "certificate",
    ] {
        let mut bad = good.clone();
        match field {
            "actorRevision" => bad[field] = serde_json::json!(21),
            "nodeFingerprint" => bad[field] = serde_json::json!(Hash::from_bytes([0; 32])),
            "revoked" => bad[field] = serde_json::json!(true),
            "ownerPin" => bad[field]["checkpoint"] = serde_json::json!(8),
            _ => bad[field]["keyEpoch"] = serde_json::json!(2),
        }
        let bad: Committed = serde_json::from_value(bad).unwrap();
        assert!(protocol::committed(&p, &cert, &bad, now).is_err());
    }
    let mut alias = good;
    let object = alias.as_object_mut().unwrap();
    let fp = object.remove("nodeFingerprint").unwrap();
    object.insert("nodeKeyFingerprint".into(), fp);
    assert!(serde_json::from_value::<Committed>(alias).is_err());
    let mut bad = cert.clone();
    bad.signature = SignatureBytes::from_bytes([0; 64]);
    assert!(protocol::committed(&p, &bad, &c, now).is_err());
}
pub(super) async fn local() -> (tempfile::TempDir, AppCore, Arc<OriginalScope>) {
    let root = tempfile::tempdir().unwrap();
    let app = AppCore::new(crate::AppConfig::for_tests(
        root.path().to_string_lossy().into_owned(),
    ))
    .unwrap();
    app.create_local_profile(
        "Association owned fixture".into(),
        "association local fixture password 42".into(),
    )
    .await
    .unwrap();
    let scope = Arc::new(
        OriginalScope::capture(&app, &CancellationToken::new(), Duration::from_secs(30))
            .await
            .unwrap(),
    );
    (root, app, scope)
}
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn original_sql_owner_cannot_be_recaptured_after_lock_or_last_core_drop() {
    let (_root, app, scope) = local().await;
    let ledger = ledger::Ledger::capture(&app, scope.clone()).await.unwrap();
    assert!(
        ledger
            .require_unassociated(&app, ObjectId::new(), fixture().0.node.agent_id)
            .await
            .is_ok()
    );
    app.lock().await.unwrap();
    assert!(
        ledger
            .require_unassociated(&app, ObjectId::new(), fixture().0.node.agent_id)
            .await
            .is_err()
    );
    drop(ledger);
    drop(app);
    assert!(scope.check_current().is_err());
}

#[test]
fn explicit_null_certificate_is_required_missing_nullable_field_is_not_unenrolled() {
    let (p, _, _, _, _) = fixture();
    let mut v = serde_json::json!({"node":p.node,"ownerPin":p.owner_pin,"actorRevision":0,"certificate":null,"revoked":false});
    assert!(serde_json::from_value::<Probe>(v.clone()).is_ok());
    v.as_object_mut().unwrap().remove("certificate");
    assert!(serde_json::from_value::<Probe>(v).is_err());
}
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn delayed_original_guard_expires_before_actual_typed_owner_signer() {
    use std::future::Future;
    let (_root, app, scope) = local().await;
    let (mut p, offer, _, _, _) = fixture();
    p.owner_pin.key = scope
        .with_loaded(&app, |id| {
            Ok(PublicKey::from_bytes(id.public_keys().signing))
        })
        .await
        .unwrap();
    p.owner_pin.context.tenant = Tenant::LocalProfile {
        id: LocalProfileId::new(&Uuid::new_v4().to_string()).unwrap(),
    };
    let now = scope.now().unwrap();
    p.owner_pin.expires_at = now.0.checked_add(90).unwrap();
    let mut offer = offer.body;
    offer.header.context = p.owner_pin.context.clone();
    offer.issued_at = now.0;
    offer.expires_at = now.0.checked_add(30).unwrap();
    let prepared = PreparedTunnelOwner::offer(&p.owner_pin, offer.clone(), now).unwrap();
    let original_guard = app.inner.active.lock().await;
    let until = Instant::now() + Duration::from_millis(200);
    let mut signing = Box::pin(sign_reviewed(&app, &scope, until, &p.owner_pin, prepared));
    let mut cx = std::task::Context::from_waker(futures::task::noop_waker_ref());
    assert!(signing.as_mut().poll(&mut cx).is_pending());
    // SAME actual guard stays held until the original budget has won. The
    // production signer future must be dropped without entering its closure.
    assert!(
        tokio::time::timeout(Duration::from_secs(1), &mut signing)
            .await
            .unwrap()
            .is_err()
    );
    drop(signing);
    drop(original_guard);
    // A fresh explicitly prepared positive control proves this is a deadline
    // denial, not a missing/mismatched original device signer.
    let prepared = PreparedTunnelOwner::offer(&p.owner_pin, offer, scope.now().unwrap()).unwrap();
    assert!(matches!(
        sign_reviewed(
            &app,
            &scope,
            Instant::now() + Duration::from_secs(1),
            &p.owner_pin,
            prepared
        )
        .await
        .unwrap(),
        TunnelOwnerSigned::Offer(_)
    ));
    app.shutdown().await.unwrap();
}

#[path = "exporter_tests.rs"]
mod exporters;
