use super::*;
use crate::*;
use ed25519_dalek::{Signer, SigningKey};
use serde_json::{json, Value};
use std::sync::{Arc, Mutex};

fn vector() -> Value {
    serde_json::from_str(include_str!(
        "../../tests/vectors/CANONICAL_VECTORS.json"
    ))
    .unwrap()
}
fn check_vector<T: ReaderStatement>(value: &Value) {
    let body: T = serde_json::from_value(value["body"].clone()).unwrap();
    assert_eq!(
        hex::encode(body.signing_bytes().unwrap()),
        value["signing_bytes_hex"].as_str().unwrap()
    );
    assert_eq!(
        hex::encode(body.digest().unwrap().as_bytes()),
        value["body_digest_hex"].as_str().unwrap()
    );
}
#[test]
fn public_reader_canonical_vectors_match_five_sealed_domains_and_challenge_target() {
    let v = vector();
    let s = v["statements"].as_array().unwrap();
    check_vector::<ReaderAssociation>(&s[0]);
    check_vector::<ReaderGrant>(&s[1]);
    check_vector::<ReaderProof>(&s[2]);
    check_vector::<ReaderRevocation>(&s[3]);
    check_vector::<ReaderFreshness>(&s[4]);
    let target: ReaderTargetDescriptor =
        serde_json::from_value(v["target"]["body"].clone()).unwrap();
    assert_eq!(
        hex::encode(target.digest().unwrap().as_bytes()),
        v["target"]["digest_hex"].as_str().unwrap()
    );
    let c: ReaderChallenge = serde_json::from_value(v["challenge"]["body"].clone()).unwrap();
    assert_eq!(
        hex::encode(c.digest().unwrap().as_bytes()),
        v["challenge"]["digest_hex"].as_str().unwrap()
    );
    let a: ReaderAssociation = serde_json::from_value(s[0]["body"].clone()).unwrap();
    assert_eq!(
        reader_key_fingerprint(a.device_key),
        c.device_key_fingerprint
    );
    assert_ne!(
        reader_key_fingerprint(a.device_key),
        a.device_key.fingerprint()
    );
}
#[test]
fn malformed_reader_json_is_bounded_duplicate_free_integer_only() {
    for raw in [
        b"{\"x\":1,\"x\":2}".as_slice(),
        b"{\"x\":1.0}",
        b"{\"x\":1e309}",
        b"{\"x\":NaN}",
    ] {
        assert!(strict_json::<Value>(raw, 8192).is_err());
    }
    let deep = format!("{}0{}", "[".repeat(18), "]".repeat(18));
    assert!(strict_json::<Value>(deep.as_bytes(), 8192).is_err());
    assert!(strict_json::<Value>(&vec![b' '; 8193], 8192).is_err());
    let mut target = vector()["target"]["body"].clone();
    target["dataset"]["descriptor"]["max_points"] = json!(true);
    let target: ReaderTargetDescriptor = serde_json::from_value(target).unwrap();
    assert!(target.validate().is_err());
}
#[test]
fn full_dataset_budgets_query_and_retention_have_distinct_hash_meanings() {
    let value = vector()["target"]["body"].clone();
    let target: ReaderTargetDescriptor = serde_json::from_value(value.clone()).unwrap();
    assert_eq!(target.max_query_points, 100);
    assert_eq!(target.dataset.descriptor["max_points"], json!(5760));
    for field in [
        "max_points",
        "max_age_seconds",
        "plan_sha256",
        "agent_reserved_bytes",
        "snapshot_reserved_bytes",
        "total_max_bytes",
    ] {
        let mut changed = value.clone();
        let item = &mut changed["dataset"]["descriptor"][field];
        if field == "plan_sha256" {
            *item = json!("cd".repeat(32));
        } else {
            *item = json!(item.as_u64().unwrap() - 1);
        }
        let changed: ReaderTargetDescriptor = serde_json::from_value(changed).unwrap();
        match changed.digest() {
            Ok(descriptor_digest) => assert_ne!(descriptor_digest, target.digest().unwrap()),
            Err(_) => (),
        }
    }
    let mut changed = target;
    changed.max_query_points = 5760;
    assert!(changed.validate().is_err());
}
#[test]
fn standalone_reader_digest_does_not_certify_runtime_uuid_assignment_or_read_data() {
    let mut target: ReaderTargetDescriptor =
        serde_json::from_value(vector()["target"]["body"].clone()).unwrap();
    let original = target.digest().unwrap();
    // Exact Python uuid5(agent,"history-v1") for this public fixture AgentID.
    // Runtime BoundHistoryReader excludes it; Rust has no sha1/v5 dependency.
    let local = ResourceId::new("9d4c713f-b73a-58f1-b5aa-3bc7725536b9").unwrap();
    target.resource_id = local;
    target.dataset.descriptor["history_resource_id"] = serde_json::to_value(local).unwrap();
    assert!(target.validate().is_ok());
    assert_ne!(target.digest().unwrap(), original);
    // A digest is the exact approved descriptor commitment, not authority or
    // certification of the runtime's UUID assignment/storage-source checks.
}
#[test]
fn canonical_base64url_codec_rejects_aliases_and_bounds() {
    for (raw, text) in [
        (b"".as_slice(), ""),
        (b"f", "Zg"),
        (b"fo", "Zm8"),
        (b"foo", "Zm9v"),
        (b"foob", "Zm9vYg"),
    ] {
        assert_eq!(reader_base64url(raw), text);
        assert!(reader_decode_base64url(text, 8).unwrap() == raw);
    }
    for text in ["Zg=", "Zg==", "Zh", "Z", "Zm+v", "Zm/v", " Zg"] {
        assert!(reader_decode_base64url(text, 8192).is_err());
    }
    assert!(reader_decode_base64url(&"A".repeat(100), 8).is_err());
}

#[cfg(target_os = "linux")]
mod linux {
    use super::*;
    fn key(n: u8) -> SigningKey {
        SigningKey::from_bytes(&[n; 32])
    }
    fn public(n: u8) -> PublicKey {
        PublicKey::from_bytes(key(n).verifying_key().to_bytes())
    }
    fn signed<T: ReaderStatement>(body: T, n: u8) -> ReaderSigned<T> {
        let signature =
            SignatureBytes::from_bytes(key(n).sign(&body.signing_bytes().unwrap()).to_bytes());
        ReaderSigned { body, signature }
    }
    fn node_signed<T: Statement>(body: T, n: u8) -> Signed<T> {
        let signature =
            SignatureBytes::from_bytes(key(n).sign(&body.signing_bytes().unwrap()).to_bytes());
        Signed { body, signature }
    }
    fn time(n: u64) -> Timestamp {
        Timestamp::new(n).unwrap()
    }
    fn clock() -> ReaderResult<Now> {
        Ok(Now(time(1000)))
    }
    #[derive(Clone)]
    struct Nodes(Arc<Mutex<State>>);
    impl CompareAndSwap for Nodes {
        fn load(&self) -> Result<State> {
            Ok(self.0.lock().unwrap().clone())
        }
        fn compare_and_swap(&mut self, e: u64, n: State) -> Result<()> {
            let mut s = self.0.lock().unwrap();
            if s.revision() != e {
                return Err(Error::Conflict);
            }
            *s = n;
            Ok(())
        }
    }
    #[derive(Clone)]
    struct Readers(Arc<Mutex<ReaderState>>);
    impl ReaderCompareAndSwap for Readers {
        fn load(&self) -> ReaderResult<ReaderState> {
            Ok(self.0.lock().unwrap().clone())
        }
        fn compare_and_swap(&mut self, e: u64, n: ReaderState) -> ReaderResult<()> {
            let mut s = self.0.lock().unwrap();
            if s.revision() != e {
                return Err(ReaderError::Conflict);
            }
            *s = n;
            Ok(())
        }
    }
    struct Fixture {
        nodes: Nodes,
        readers: Readers,
        node_pin: OwnerPin,
        cert: Certificate,
        pin: ReaderIssuerPin,
        targets: ReaderTargetSet,
        association: ReaderSigned<ReaderAssociation>,
    }
    impl Fixture {
        fn new() -> Self {
            let mut pin: ReaderIssuerPin =
                serde_json::from_value(vector()["pin"]["body"].clone()).unwrap();
            pin.issuer_key = public(33);
            let node_pin = OwnerPin {
                context: pin.context.clone(),
                key: public(11),
                epoch: 1,
                checkpoint: 0,
                expires_at: time(10000),
            };
            pin.node_trust_origin_hash = reader_node_trust_origin(&node_pin).unwrap();
            let targets: ReaderTargetSet = serde_json::from_str(include_str!(
                "../../tests/vectors/REPLAY_TARGETSET_FAILURE_SUPPLEMENT.json"
            ))
            .map(|v: Value| {
                serde_json::from_value(v["ReaderTargetSet"]["example"].clone()).unwrap()
            })
            .unwrap();
            let target = &targets.targets[1];
            let agent = target.agent_id;
            let nodes = Nodes(Arc::new(Mutex::new(
                State::bootstrap(node_pin.clone(), Now(time(1000))).unwrap(),
            )));
            let mut node = Actor::new(nodes.clone()).unwrap();
            let hdr = |checkpoint| Header {
                version: 1,
                context: pin.context.clone(),
                owner_epoch: 1,
                checkpoint,
            };
            let token = InvitationToken::from_bytes([99; 32]).unwrap();
            let offer = Offer {
                header: hdr(1),
                invitation_id: InvitationId::new("33333333-3333-4333-8333-333333333333").unwrap(),
                agent_id: agent,
                agent_key: public(22),
                token_hash: token.digest(),
                scopes: Scopes(vec![Scope::TelemetryRead]),
                issued_at: time(1000),
                expires_at: time(1600),
            };
            node.offer(node_signed(offer.clone(), 11), Now(time(1000)))
                .unwrap();
            let claim = Claim {
                header: hdr(1),
                invitation_id: offer.invitation_id,
                offer_hash: offer.digest().unwrap(),
                agent_id: agent,
                agent_key: offer.agent_key,
                capabilities: Capabilities {
                    version: 1,
                    scopes: Scopes(vec![Scope::TelemetryRead]),
                },
                nonce: Nonce::from_bytes([1; 16]),
                issued_at: time(1000),
            };
            node.consume(&token, node_signed(claim.clone(), 22), Now(time(1000)))
                .unwrap();
            let cert = Certificate {
                header: hdr(2),
                agent_id: agent,
                key_epoch: 1,
                agent_key: public(22),
                scopes: offer.scopes,
                capabilities: claim.capabilities.clone(),
                origin: CertificateOrigin::Enrollment {
                    invitation_id: offer.invitation_id,
                    claim_hash: claim.digest().unwrap(),
                    key_fingerprint: offer.agent_key.fingerprint(),
                },
                not_before: time(1000),
                expires_at: time(4600),
            };
            node.approve(node_signed(cert.clone(), 11), Now(time(1000)))
                .unwrap();
            let mut a: ReaderAssociation =
                serde_json::from_value(vector()["statements"][0]["body"].clone()).unwrap();
            a.device_key = public(44);
            a.node_trust_origin_hash = pin.node_trust_origin_hash;
            a.previous_authority_hash = pin.genesis_hash().unwrap();
            a.agent_certificate_hash = cert.digest().unwrap();
            a.target_descriptor_hash = target.digest().unwrap();
            let readers = Readers(Arc::new(Mutex::new(
                ReaderState::bootstrap(pin.clone(), Now(time(1000))).unwrap(),
            )));
            let f = Self {
                nodes,
                readers,
                node_pin,
                cert,
                pin,
                targets,
                association: signed(a, 33),
            };
            f.actor()
                .administer(
                    ReaderAdministration::InstallAssociation {
                        signed: f.association.clone(),
                    },
                    &mut clock,
                )
                .unwrap();
            f
        }
        fn actor(&self) -> ReaderActor<Readers, Nodes> {
            ReaderActor::new(
                self.readers.clone(),
                Actor::new(self.nodes.clone()).unwrap(),
                self.targets.clone(),
                self.pin.node_trust_origin_hash,
            )
            .unwrap()
        }
        fn fresh(&self) -> ReaderSigned<ReaderFreshness> {
            signed(
                ReaderFreshness {
                    version: 1,
                    head: self.readers.load().unwrap().authority().clone(),
                    issued_at: time(1000),
                    expires_at: time(1030),
                },
                33,
            )
        }
        fn challenge<R: ReaderCompareAndSwap>(
            &self,
            actor: &mut ReaderActor<R, Nodes>,
            n: u64,
        ) -> ReaderChallenge {
            actor
                .challenge(
                    self.association.body.digest().unwrap(),
                    ReaderOperationId::new(&format!("dddddddd-dddd-4ddd-8ddd-{n:012x}")).unwrap(),
                    self.cert.agent_id,
                    &self.association.body.operation,
                    self.association.body.target_descriptor_hash,
                    self.fresh(),
                    &mut clock,
                )
                .unwrap()
        }
        fn bundle(&self, c: &ReaderChallenge, n: u64) -> ReaderBundle {
            let g = ReaderGrant {
                version: 1,
                head: c.head.clone(),
                grant_id: ReaderGrantId::new(&format!("eeeeeeee-eeee-4eee-8eee-{n:012x}")).unwrap(),
                association_hash: c.association_hash,
                challenge_id: c.challenge_id,
                challenge_hash: c.digest().unwrap(),
                operation_id: c.operation_id,
                device_id: c.device_id,
                device_epoch: 1,
                agent_id: c.agent_id,
                operation: c.operation.clone(),
                target_descriptor_hash: c.target_descriptor_hash,
                issued_at: time(1000),
                expires_at: time(1020),
            };
            let p = ReaderProof {
                version: 1,
                head: c.head.clone(),
                operation_id: c.operation_id,
                device_id: c.device_id,
                device_epoch: 1,
                association_hash: c.association_hash,
                grant_hash: g.digest().unwrap(),
                challenge_hash: g.challenge_hash,
                nonce: Hash::from_bytes([n as u8; 32]),
                issued_at: time(1000),
            };
            ReaderBundle {
                association: self.association.clone(),
                grant: signed(g, 33),
                proof: signed(p, 44),
                freshness: self.fresh(),
            }
        }
        fn begin<R: ReaderCompareAndSwap>(
            &self,
            actor: &mut ReaderActor<R, Nodes>,
            b: ReaderBundle,
        ) -> ReaderResult<ReaderPermit> {
            actor.begin(
                b,
                self.cert.agent_id,
                &self.association.body.operation,
                self.association.body.target_descriptor_hash,
                &mut clock,
            )
        }
        fn revoke(&self) {
            let head = self.readers.load().unwrap().authority().clone();
            self.actor()
                .administer(
                    ReaderAdministration::Revoke {
                        signed: signed(
                            ReaderRevocation {
                                version: 1,
                                context: head.context,
                                issuer_id: head.issuer_id,
                                issuer_epoch: 1,
                                authority_revision: head.authority_revision + 1,
                                previous_authority_hash: head.authority_hash,
                                subject: ReaderRevocationSubject::Association {
                                    association_id: self.association.body.association_id,
                                    body_hash: self.association.body.digest().unwrap(),
                                },
                                issued_at: time(1000),
                            },
                            33,
                        ),
                    },
                    &mut clock,
                )
                .unwrap();
        }
    }
    #[test]
    fn actual_independent_reader_signatures_challenge_consume_check_and_replay() {
        let f = Fixture::new();
        let mut actor = f.actor();
        let c = f.challenge(&mut actor, 1);
        let b = f.bundle(&c, 1);
        let p = f.begin(&mut actor, b.clone()).unwrap();
        actor.check_permit(&p, &mut clock).unwrap();
        assert!(matches!(
            f.begin(&mut f.actor(), b),
            Err(ReaderError::Replay)
        ));
        let cap = actor.snapshot().unwrap().capacity();
        assert_eq!(cap.consumed_grants, 1);
        assert_eq!(cap.consumed_nonces, 1);
    }
    #[test]
    fn reader_policy_rejects_agent_key_and_account_or_other_purpose_signatures_without_consume() {
        let f = Fixture::new();
        let mut actor = f.actor();
        let c = f.challenge(&mut actor, 1);
        let original = f.bundle(&c, 1);
        let revision = actor.snapshot().unwrap().revision();
        let mut wrong = original.clone();
        wrong.proof = signed(wrong.proof.body, 22);
        assert!(matches!(
            f.begin(&mut actor, wrong),
            Err(ReaderError::InvalidSignature)
        ));
        let mut wrong = original.clone();
        wrong.proof.signature = wrong.grant.signature;
        assert!(matches!(
            f.begin(&mut actor, wrong),
            Err(ReaderError::InvalidSignature)
        ));
        assert_eq!(actor.snapshot().unwrap().revision(), revision);
        f.begin(&mut actor, original).unwrap();
    }
    #[test]
    fn challenge_operation_id_is_one_action_and_pending_cap_is_checked_before_commit() {
        let f = Fixture::new();
        let mut actor = f.actor();
        for n in 1..=8 {
            f.challenge(&mut actor, n);
        }
        let revision = actor.snapshot().unwrap().revision();
        for (n, error) in [(1, ReaderError::Replay), (9, ReaderError::Quota)] {
            assert_eq!(
                actor
                    .challenge(
                        f.association.body.digest().unwrap(),
                        ReaderOperationId::new(&format!("dddddddd-dddd-4ddd-8ddd-{n:012x}"))
                            .unwrap(),
                        f.cert.agent_id,
                        &f.association.body.operation,
                        f.association.body.target_descriptor_hash,
                        f.fresh(),
                        &mut clock
                    )
                    .err(),
                Some(error)
            );
            assert_eq!(actor.snapshot().unwrap().revision(), revision);
        }
    }
    #[test]
    fn issuer_and_node_revocations_are_current_checks_not_revision_inference() {
        for reader in [false, true] {
            let f = Fixture::new();
            let mut actor = f.actor();
            let c = f.challenge(&mut actor, 1);
            let p = f.begin(&mut actor, f.bundle(&c, 1)).unwrap();
            if reader {
                f.revoke();
                assert_eq!(
                    actor.check_permit(&p, &mut clock),
                    Err(ReaderError::WrongHead)
                );
            } else {
                let head = Header {
                    version: 1,
                    context: f.pin.context.clone(),
                    owner_epoch: 1,
                    checkpoint: 3,
                };
                Actor::new(f.nodes.clone())
                    .unwrap()
                    .revoke(
                        node_signed(
                            Revocation {
                                header: head,
                                agent_id: f.cert.agent_id,
                                key_epoch: 1,
                                certificate_hash: f.cert.digest().unwrap(),
                                issued_at: time(1000),
                            },
                            11,
                        ),
                        Now(time(1000)),
                    )
                    .unwrap();
                assert_eq!(
                    actor.check_permit(&p, &mut clock),
                    Err(ReaderError::Revoked)
                );
            }
        }
    }
    #[test]
    fn unrelated_reader_consume_preserves_permit_but_foreign_operation_does_not_consume() {
        let f = Fixture::new();
        let mut actor = f.actor();
        let c = f.challenge(&mut actor, 1);
        let p = f.begin(&mut actor, f.bundle(&c, 1)).unwrap();
        let c2 = f.challenge(&mut f.actor(), 2);
        let bundle = f.bundle(&c2, 2);
        let revision = actor.snapshot().unwrap().revision();
        assert!(matches!(
            actor.begin(
                bundle.clone(),
                f.cert.agent_id,
                &f.targets.targets[0].operation(),
                f.targets.targets[0].digest().unwrap(),
                &mut clock
            ),
            Err(ReaderError::WrongTarget)
        ));
        assert_eq!(actor.snapshot().unwrap().revision(), revision);
        f.begin(&mut f.actor(), bundle).unwrap();
        actor.check_permit(&p, &mut clock).unwrap();
    }
    #[test]
    fn node_owner_rotation_retains_association_but_invalidates_old_challenge_and_permit() {
        let f = Fixture::new();
        let mut actor = f.actor();
        let c = f.challenge(&mut actor, 1);
        let p = f.begin(&mut actor, f.bundle(&c, 1)).unwrap();
        let change = OwnerRotation {
            header: Header {
                version: 1,
                context: f.pin.context.clone(),
                owner_epoch: 1,
                checkpoint: 3,
            },
            new_epoch: 2,
            new_key: public(66),
            issued_at: time(1000),
            expires_at: f.node_pin.expires_at,
        };
        Actor::new(f.nodes.clone())
            .unwrap()
            .rotate_owner(
                node_signed(change.clone(), 11),
                node_signed(NewOwnerProof(change), 66),
                Now(time(1000)),
            )
            .unwrap();
        assert_eq!(
            actor.check_permit(&p, &mut clock),
            Err(ReaderError::WrongTarget)
        );
        let new = f.challenge(&mut actor, 2);
        assert_eq!(new.node_owner_epoch, 2);
        f.begin(&mut actor, f.bundle(&new, 2)).unwrap();
    }
    #[test]
    fn historical_replay_does_not_use_current_node_or_expired_current_wall() {
        let f = Fixture::new();
        let genesis = ReaderState::bootstrap(f.pin.clone(), Now(time(1000))).unwrap();
        let install = f.readers.load().unwrap().latest.clone().unwrap();
        let mut actor = f.actor();
        let c = f.challenge(&mut actor, 1);
        let issue = actor.snapshot().unwrap().latest.clone().unwrap();
        f.begin(&mut actor, f.bundle(&c, 1)).unwrap();
        let consume = actor.snapshot().unwrap().latest.clone().unwrap();
        f.revoke();
        let replayed = super::super::policy::replay(
            super::super::policy::replay(
                super::super::policy::replay(genesis, install).unwrap(),
                issue,
            )
            .unwrap(),
            consume,
        )
        .unwrap();
        assert_eq!(replayed.capacity().consumed_grants, 1);
        // Current access is denied separately; old valid consume remains replayable.
        assert!(matches!(
            f.begin(&mut actor, f.bundle(&c, 1)),
            Err(ReaderError::WrongHead | ReaderError::Revoked)
        ));
    }
    #[test]
    fn correctly_signed_foreign_transcripts_and_node_role_keys_do_not_consume() {
        let f = Fixture::new();
        let mut actor = f.actor();
        let c = f.challenge(&mut actor, 1);
        let good = f.bundle(&c, 1);
        let before = actor.snapshot().unwrap().revision();
        for field in [
            "operationId",
            "challengeHash",
            "associationHash",
            "targetDescriptorHash",
        ] {
            let mut b = good.clone();
            let mut body = serde_json::to_value(&b.grant.body).unwrap();
            body[field] = if field == "operationId" {
                json!("99999999-9999-4999-8999-999999999999")
            } else {
                json!("99".repeat(32))
            };
            b.grant = signed(serde_json::from_value(body).unwrap(), 33);
            assert!(
                f.begin(&mut actor, b).is_err(),
                "foreign issuer transcript must deny"
            );
            assert_eq!(actor.snapshot().unwrap().revision(), before);
        }
        for field in [
            "operationId",
            "grantHash",
            "challengeHash",
            "associationHash",
        ] {
            let mut b = good.clone();
            let mut body = serde_json::to_value(&b.proof.body).unwrap();
            body[field] = if field == "operationId" {
                json!("99999999-9999-4999-8999-999999999999")
            } else {
                json!("99".repeat(32))
            };
            b.proof = signed(serde_json::from_value(body).unwrap(), 44);
            assert!(
                f.begin(&mut actor, b).is_err(),
                "foreign device transcript must deny"
            );
            assert_eq!(actor.snapshot().unwrap().revision(), before);
        }
        let mut changed = good.clone();
        changed.association.body.native_host_revision += 1;
        changed.association = signed(changed.association.body, 33);
        assert!(
            f.begin(&mut actor, changed).is_err(),
            "issuer cannot replace registered body in a request"
        );
        assert_eq!(actor.snapshot().unwrap().revision(), before);
        f.begin(&mut actor, good).unwrap();

        let head = actor.snapshot().unwrap().authority().clone();
        let mut body = f.association.body.clone();
        body.association_id =
            ReaderAssociationId::new("99999999-9999-4999-8999-999999999999").unwrap();
        body.authority_revision = head.authority_revision + 1;
        body.previous_authority_hash = head.authority_hash;
        body.device_key = public(22);
        let before = actor.snapshot().unwrap().revision();
        assert_eq!(
            actor.administer(
                ReaderAdministration::InstallAssociation {
                    signed: signed(body, 33)
                },
                &mut clock
            ),
            Err(ReaderError::WrongDevice)
        );
        assert_eq!(actor.snapshot().unwrap().revision(), before);
    }
    #[test]
    fn last_reader_store_io_cannot_hide_expiry_or_rollback_from_permit_check() {
        use std::cell::Cell;
        #[derive(Clone)]
        struct AdvancingStore {
            inner: Readers,
            observed: std::rc::Rc<Cell<u64>>,
            loads: std::rc::Rc<Cell<usize>>,
            final_time: u64,
        }
        impl ReaderCompareAndSwap for AdvancingStore {
            fn load(&self) -> ReaderResult<ReaderState> {
                let state = self.inner.load()?;
                let n = self.loads.get() + 1;
                self.loads.set(n);
                // Construction is load 1; the second check_permit store read is 3.
                if n == 3 {
                    self.observed.set(self.final_time);
                }
                Ok(state)
            }
            fn compare_and_swap(&mut self, revision: u64, next: ReaderState) -> ReaderResult<()> {
                self.inner.compare_and_swap(revision, next)
            }
        }
        for (final_time, expected) in [
            (1020, ReaderError::Expired),
            (999, ReaderError::ClockUnavailable),
        ] {
            let f = Fixture::new();
            let mut actor = f.actor();
            let c = f.challenge(&mut actor, 1);
            let permit = f.begin(&mut actor, f.bundle(&c, 1)).unwrap();
            let observed = std::rc::Rc::new(Cell::new(1000));
            let store = AdvancingStore {
                inner: f.readers.clone(),
                observed: std::rc::Rc::clone(&observed),
                loads: std::rc::Rc::new(Cell::new(0)),
                final_time,
            };
            let mut actor = ReaderActor::new(
                store,
                Actor::new(f.nodes.clone()).unwrap(),
                f.targets.clone(),
                f.pin.node_trust_origin_hash,
            )
            .unwrap();
            assert_eq!(
                actor.check_permit(&permit, &mut || Ok(Now(time(observed.get())))),
                Err(expected)
            );
        }
    }
    include!("sqlite_tests.rs");
}
