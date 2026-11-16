use super::*;
use ed25519_dalek::{Signer, SigningKey};
use serde_json::json;
use std::sync::{Arc, Mutex};

mod owner_role_audit {
    use super::*;
    include!("owner_role_audit_tests.rs");
}

const ID: &str = "11111111-1111-4111-8111-111111111111";
const DEPLOY: &str = "22222222-2222-4222-8222-222222222222";
const INVITE: &str = "33333333-3333-4333-8333-333333333333";
const AGENT: &str = "44444444-4444-4444-8444-444444444444";
fn time(n: u64) -> Timestamp {
    Timestamp::new(n).unwrap()
}
fn now() -> Now {
    Now(time(1000))
}
fn key(n: u8) -> SigningKey {
    SigningKey::from_bytes(&[n; 32])
}
fn public(k: &SigningKey) -> PublicKey {
    PublicKey::from_bytes(k.verifying_key().to_bytes())
}
fn sign<T: Statement>(body: T, k: &SigningKey) -> Signed<T> {
    let signature = SignatureBytes::from_bytes(k.sign(&body.signing_bytes().unwrap()).to_bytes());
    Signed { body, signature }
}
fn unsigned<T>(body: T) -> Signed<T> {
    Signed {
        body,
        signature: SignatureBytes::from_bytes([0; 64]),
    }
}
fn context() -> Context {
    Context {
        tenant: Tenant::LocalProfile {
            id: LocalProfileId::new(ID).unwrap(),
        },
        deployment_id: DeploymentId::new(DEPLOY).unwrap(),
    }
}
fn scopes() -> Scopes {
    Scopes(vec![
        Scope::TelemetryRead,
        Scope::MetricsCollect,
        Scope::ExportersManage,
        Scope::TunnelOpen,
        Scope::JobsExecute,
        Scope::AgentUpdate,
    ])
}
fn header(checkpoint: u64) -> Header {
    Header {
        version: 1,
        context: context(),
        owner_epoch: 1,
        checkpoint,
    }
}
fn uuid(n: u64) -> String {
    format!("55555555-5555-4555-8555-{n:012x}")
}

#[derive(Clone)]
struct Memory(Arc<Mutex<(State, bool)>>);
impl CompareAndSwap for Memory {
    fn load(&self) -> Result<State> {
        Ok(self.0.lock().unwrap().0.clone())
    }
    fn compare_and_swap(&mut self, expected: u64, next: State) -> Result<()> {
        let mut inner = self.0.lock().unwrap();
        if inner.1 || inner.0.revision() != expected {
            return Err(Error::Conflict);
        }
        inner.0 = next;
        Ok(())
    }
}
struct Harness {
    actor: Actor<Memory>,
    memory: Memory,
    owner: SigningKey,
    agent: SigningKey,
    token: InvitationToken,
    offer: Offer,
    claim: Claim,
    cert: Certificate,
}
impl Harness {
    fn offered(ceiling: Scopes, capabilities: Scopes) -> Self {
        let owner = key(11);
        let agent = key(22);
        let token = InvitationToken::from_bytes([33; 32]).unwrap();
        let state = State::bootstrap(
            OwnerPin {
                context: context(),
                key: public(&owner),
                epoch: 1,
                checkpoint: 0,
                expires_at: time(1000 + CERTIFICATE_TTL),
            },
            now(),
        )
        .unwrap();
        let memory = Memory(Arc::new(Mutex::new((state, false))));
        let mut actor = Actor::new(memory.clone()).unwrap();
        let offer = Offer {
            header: header(1),
            invitation_id: InvitationId::new(INVITE).unwrap(),
            agent_id: AgentId::new(AGENT).unwrap(),
            agent_key: public(&agent),
            token_hash: token.digest(),
            scopes: ceiling.clone(),
            issued_at: time(1000),
            expires_at: time(1600),
        };
        actor.offer(sign(offer.clone(), &owner), now()).unwrap();
        let claim = Claim {
            header: header(1),
            invitation_id: offer.invitation_id,
            offer_hash: offer.digest().unwrap(),
            agent_id: offer.agent_id,
            agent_key: offer.agent_key,
            capabilities: Capabilities {
                version: 1,
                scopes: capabilities,
            },
            nonce: Nonce::from_bytes([1; 16]),
            issued_at: time(1000),
        };
        let cert = Certificate {
            header: header(2),
            agent_id: offer.agent_id,
            key_epoch: 1,
            agent_key: offer.agent_key,
            scopes: ceiling,
            capabilities: claim.capabilities.clone(),
            origin: CertificateOrigin::Enrollment {
                invitation_id: offer.invitation_id,
                claim_hash: claim.digest().unwrap(),
                key_fingerprint: offer.agent_key.fingerprint(),
            },
            not_before: time(1000),
            expires_at: time(4600),
        };
        Self {
            actor,
            memory,
            owner,
            agent,
            token,
            offer,
            claim,
            cert,
        }
    }
    fn claimed() -> Self {
        let mut h = Self::offered(Scopes(vec![Scope::TelemetryRead]), scopes());
        h.actor
            .consume(&h.token, sign(h.claim.clone(), &h.agent), now())
            .unwrap();
        h
    }
    fn enrolled() -> Self {
        let mut h = Self::claimed();
        h.actor
            .approve(sign(h.cert.clone(), &h.owner), now())
            .unwrap();
        h
    }
    fn current_header(&self) -> Header {
        let state = self.actor.snapshot().unwrap();
        Header {
            version: 1,
            context: context(),
            owner_epoch: state.pin.epoch,
            checkpoint: state.pin.checkpoint,
        }
    }
    fn lease(&self, operation: Operation, n: u8) -> Lease {
        Lease {
            header: self.current_header(),
            id: LeaseId::new(&uuid(n as u64)).unwrap(),
            agent_id: self.offer.agent_id,
            key_epoch: self.cert.key_epoch,
            certificate_hash: self.cert.digest().unwrap(),
            operation,
            issued_at: time(1000),
            expires_at: time(1120),
        }
    }
    fn fresh(&self) -> Signed<Freshness> {
        sign(
            Freshness {
                header: self.current_header(),
                issued_at: time(1000),
                expires_at: time(1300),
            },
            &self.owner,
        )
    }
    fn proof(&self, lease: &Lease, n: u8) -> Signed<AgentRequest> {
        sign(
            AgentRequest {
                header: lease.header.clone(),
                agent_id: lease.agent_id,
                key_epoch: lease.key_epoch,
                lease_hash: lease.digest().unwrap(),
                nonce: Nonce::from_bytes([n; 16]),
                issued_at: time(1000),
            },
            &self.agent,
        )
    }
    fn admit(&mut self, operation: Operation, n: u8) -> Result<Permit> {
        let lease = self.lease(operation.clone(), n);
        let proof = self.proof(&lease, n);
        self.actor.admit(
            sign(lease, &self.owner),
            proof,
            Some(&self.fresh()),
            &operation,
            now(),
        )
    }
}

#[test]
fn exact_key_offer_claim_and_explicit_approval_are_separate_atomic_transitions() {
    let mut h = Harness::offered(Scopes(vec![Scope::TelemetryRead]), scopes());
    assert!(h
        .actor
        .snapshot()
        .unwrap()
        .certificate(h.offer.agent_id)
        .is_none());
    let hash = h
        .actor
        .consume(&h.token, sign(h.claim.clone(), &h.agent), now())
        .unwrap();
    assert_eq!(hash, h.claim.digest().unwrap());
    assert_eq!(
        h.actor
            .snapshot()
            .unwrap()
            .invitation_status(h.offer.invitation_id),
        Some(InvitationStatus::Claimed)
    );
    assert!(h
        .actor
        .snapshot()
        .unwrap()
        .certificate(h.offer.agent_id)
        .is_none());
    h.actor
        .approve(sign(h.cert.clone(), &h.owner), now())
        .unwrap();
    let state = h.actor.snapshot().unwrap();
    assert_eq!(state.revision(), 3);
    assert_eq!(state.owner().checkpoint, 2);
    assert_eq!(
        state.invitation_status(h.offer.invitation_id),
        Some(InvitationStatus::Approved)
    );
    assert_eq!(state.certificate(h.offer.agent_id), Some(&h.cert));
    assert_eq!(
        h.actor
            .consume(&h.token, sign(h.claim.clone(), &h.agent), now()),
        Err(Error::Replay)
    );
}

#[test]
fn wrong_token_key_context_or_signature_does_not_consume_invitation() {
    let mut h = Harness::offered(Scopes(vec![Scope::TelemetryRead]), scopes());
    let wrong = InvitationToken::from_bytes([44; 32]).unwrap();
    assert_eq!(
        h.actor
            .consume(&wrong, sign(h.claim.clone(), &h.agent), now()),
        Err(Error::InvalidToken)
    );
    assert_eq!(
        h.actor
            .consume(&h.token, sign(h.claim.clone(), &key(99)), now()),
        Err(Error::InvalidSignature)
    );
    let mut claim = h.claim.clone();
    claim.header.context.tenant = Tenant::PersonalAccount {
        id: PersonalAccountId::new(ID).unwrap(),
    };
    assert_eq!(
        h.actor.consume(&h.token, sign(claim, &h.agent), now()),
        Err(Error::WrongContext)
    );
    let mut claim = h.claim.clone();
    claim.agent_key = public(&key(98));
    assert_eq!(
        h.actor.consume(&h.token, sign(claim, &key(98)), now()),
        Err(Error::WrongClaim)
    );
    assert_eq!(
        h.actor
            .snapshot()
            .unwrap()
            .invitation_status(h.offer.invitation_id),
        Some(InvitationStatus::Offered)
    );
    h.actor
        .consume(&h.token, sign(h.claim.clone(), &h.agent), now())
        .unwrap();
}

#[test]
fn approval_must_match_actual_claim_capabilities_and_oob_key_fingerprint() {
    let mut h = Harness::claimed();
    let mut certificate = h.cert.clone();
    certificate.origin = CertificateOrigin::Enrollment {
        invitation_id: h.offer.invitation_id,
        claim_hash: h.claim.digest().unwrap(),
        key_fingerprint: public(&key(99)).fingerprint(),
    };
    assert_eq!(
        h.actor.approve(sign(certificate, &h.owner), now()),
        Err(Error::WrongClaim)
    );
    let mut certificate = h.cert.clone();
    certificate.agent_key = public(&key(99));
    assert_eq!(
        h.actor.approve(sign(certificate, &h.owner), now()),
        Err(Error::WrongClaim)
    );
    let mut certificate = h.cert.clone();
    certificate.scopes = Scopes(vec![Scope::JobsExecute]);
    assert_eq!(
        h.actor.approve(sign(certificate, &h.owner), now()),
        Err(Error::ScopeDenied)
    );
    assert_eq!(
        h.actor.approve(sign(h.cert.clone(), &key(99)), now()),
        Err(Error::InvalidSignature)
    );
    h.actor
        .approve(sign(h.cert.clone(), &h.owner), now())
        .unwrap();
}

#[test]
fn rejection_and_expiry_leave_nonreusable_tombstones() {
    let mut h = Harness::claimed();
    h.actor
        .reject(
            sign(
                Rejection {
                    header: header(2),
                    invitation_id: h.offer.invitation_id,
                    claim_hash: h.claim.digest().unwrap(),
                    issued_at: time(1000),
                },
                &h.owner,
            ),
            now(),
        )
        .unwrap();
    assert_eq!(
        h.actor
            .snapshot()
            .unwrap()
            .invitation_status(h.offer.invitation_id),
        Some(InvitationStatus::Rejected)
    );
    assert!(h
        .actor
        .approve(sign(h.cert.clone(), &h.owner), now())
        .is_err());
    let mut h = Harness::offered(Scopes(vec![]), Scopes(vec![]));
    assert_eq!(
        h.actor.expire(h.offer.invitation_id, Now(time(1599))),
        Err(Error::InvalidTime)
    );
    h.actor
        .expire(h.offer.invitation_id, Now(time(1600)))
        .unwrap();
    assert_eq!(
        h.actor
            .snapshot()
            .unwrap()
            .invitation_status(h.offer.invitation_id),
        Some(InvitationStatus::Expired)
    );
    assert!(h
        .actor
        .consume(&h.token, sign(h.claim.clone(), &h.agent), Now(time(1600)))
        .is_err());
    let mut offer = h.offer.clone();
    offer.header.checkpoint = 2;
    offer.issued_at = time(1600);
    offer.expires_at = time(2200);
    assert_eq!(
        h.actor.offer(sign(offer, &h.owner), Now(time(1600))),
        Err(Error::Replay)
    );
}

#[test]
fn invitation_certificate_lease_and_freshness_time_limits_are_hard_bounds() {
    let mut h = Harness::claimed();
    let mut certificate = h.cert.clone();
    certificate.expires_at = time(1000 + CERTIFICATE_TTL + 1);
    assert_eq!(
        h.actor.approve(unsigned(certificate), now()),
        Err(Error::InvalidTime)
    );
    let mut certificate = h.cert.clone();
    certificate.expires_at = time(1000 + CERTIFICATE_TTL);
    certificate.not_before = time(999);
    assert_eq!(
        h.actor.approve(unsigned(certificate), now()),
        Err(Error::InvalidTime)
    );
    let h = Harness::enrolled();
    let mut lease = h.lease(Operation::OwnHeartbeat, 2);
    lease.expires_at = time(1301);
    assert_eq!(lease.signing_bytes(), Err(Error::InvalidTime));
    let mut offer = h.offer.clone();
    offer.expires_at = time(1601);
    assert_eq!(offer.signing_bytes(), Err(Error::InvalidTime));
    let mut fresh = h.fresh().body;
    fresh.expires_at = time(1301);
    assert_eq!(fresh.signing_bytes(), Err(Error::InvalidTime));
}

#[test]
fn scoped_admission_binds_resource_lease_nonce_key_epoch_and_commits_before_permit() {
    let mut h = Harness::enrolled();
    let operation = Operation::TelemetryRead {
        source: ResourceId::new(ID).unwrap(),
    };
    let lease = h.lease(operation.clone(), 2);
    let proof = h.proof(&lease, 2);
    let fresh = h.fresh();
    let signed = sign(lease.clone(), &h.owner);
    let wrong = Operation::TelemetryRead {
        source: ResourceId::new(DEPLOY).unwrap(),
    };
    assert!(matches!(
        h.actor
            .admit(signed.clone(), proof.clone(), Some(&fresh), &wrong, now()),
        Err(Error::ScopeDenied)
    ));
    h.memory.0.lock().unwrap().1 = true;
    assert!(matches!(
        h.actor.admit(
            signed.clone(),
            proof.clone(),
            Some(&fresh),
            &operation,
            now()
        ),
        Err(Error::Conflict)
    ));
    h.memory.0.lock().unwrap().1 = false;
    let permit = h
        .actor
        .admit(
            signed.clone(),
            proof.clone(),
            Some(&fresh),
            &operation,
            now(),
        )
        .unwrap();
    assert_eq!(permit.operation(), &operation);
    assert_eq!(permit.agent_id(), h.offer.agent_id);
    assert_eq!(permit.expires_at(), time(1120));
    assert!(matches!(
        h.actor
            .admit(signed, proof, Some(&fresh), &operation, now()),
        Err(Error::Replay)
    ));
    let mut lease = h.lease(operation.clone(), 3);
    lease.key_epoch = 2;
    let proof = h.proof(&lease, 3);
    assert!(matches!(
        h.actor.admit(
            sign(lease, &h.owner),
            proof,
            Some(&fresh),
            &operation,
            now()
        ),
        Err(Error::WrongEpoch)
    ));
}

#[test]
fn telemetry_enrollment_does_not_authorize_other_five_scopes_or_implicit_privileges() {
    let mut h = Harness::enrolled();
    for operation in [
        Operation::MetricsCollect {
            collector: ResourceId::new(ID).unwrap(),
        },
        Operation::ExportersManage {
            manifest: Hash::from_bytes([1; 32]),
        },
        Operation::TunnelOpen {
            route: ResourceId::new(ID).unwrap(),
        },
        Operation::JobsExecute {
            manifest: Hash::from_bytes([1; 32]),
        },
        Operation::AgentUpdate {
            artifact: Hash::from_bytes([1; 32]),
        },
    ] {
        assert!(matches!(h.admit(operation, 2), Err(Error::ScopeDenied)));
    }
    h.admit(Operation::OwnHeartbeat, 2).unwrap();
    h.admit(
        Operation::TelemetryRead {
            source: ResourceId::new(ID).unwrap(),
        },
        3,
    )
    .unwrap();
    for unknown in ["metrics.write", "root", "docker.socket", "vault.read"] {
        assert!(serde_json::from_value::<Scope>(json!(unknown)).is_err());
    }
}

#[test]
fn identity_only_heartbeat_is_distinct_from_reading_other_telemetry() {
    let mut h = Harness::offered(Scopes(vec![]), Scopes(vec![]));
    h.actor
        .consume(&h.token, sign(h.claim.clone(), &h.agent), now())
        .unwrap();
    h.actor
        .approve(sign(h.cert.clone(), &h.owner), now())
        .unwrap();
    h.admit(Operation::OwnHeartbeat, 2).unwrap();
    assert!(matches!(
        h.admit(
            Operation::TelemetryRead {
                source: ResourceId::new(ID).unwrap()
            },
            3
        ),
        Err(Error::ScopeDenied)
    ));
}

#[test]
fn absent_stale_or_expired_owner_freshness_never_admits_offline_operation() {
    let mut h = Harness::enrolled();
    let operation = Operation::OwnHeartbeat;
    let lease = h.lease(operation.clone(), 2);
    let proof = h.proof(&lease, 2);
    assert!(matches!(
        h.actor.admit(
            sign(lease.clone(), &h.owner),
            proof.clone(),
            None,
            &operation,
            now()
        ),
        Err(Error::MissingFreshness)
    ));
    let mut fresh = h.fresh().body;
    fresh.header.checkpoint -= 1;
    assert!(matches!(
        h.actor.admit(
            sign(lease.clone(), &h.owner),
            proof.clone(),
            Some(&sign(fresh, &h.owner)),
            &operation,
            now()
        ),
        Err(Error::WrongCheckpoint)
    ));
    let mut fresh = h.fresh().body;
    fresh.issued_at = time(700);
    fresh.expires_at = time(1000);
    assert!(matches!(
        h.actor.admit(
            sign(lease, &h.owner),
            proof,
            Some(&sign(fresh, &h.owner)),
            &operation,
            now()
        ),
        Err(Error::Expired)
    ));
}

#[test]
fn lease_cannot_outlive_identity_certificate_freshness_or_owner_authority() {
    let mut h = Harness::claimed();
    h.cert.expires_at = time(1050);
    h.actor
        .approve(sign(h.cert.clone(), &h.owner), now())
        .unwrap();
    assert!(matches!(
        h.admit(Operation::OwnHeartbeat, 2),
        Err(Error::Expired)
    ));
    let mut h = Harness::enrolled();
    let op = Operation::OwnHeartbeat;
    let lease = h.lease(op.clone(), 2);
    let proof = h.proof(&lease, 2);
    let mut fresh = h.fresh().body;
    fresh.expires_at = time(1050);
    assert!(matches!(
        h.actor.admit(
            sign(lease, &h.owner),
            proof,
            Some(&sign(fresh, &h.owner)),
            &op,
            now()
        ),
        Err(Error::Expired)
    ));
}

#[test]
fn revoke_blocks_leases_and_reusing_agent_identity_even_with_valid_old_keys() {
    let mut h = Harness::enrolled();
    let before = h.actor.snapshot().unwrap();
    h.actor
        .revoke(
            sign(
                Revocation {
                    header: header(3),
                    agent_id: h.offer.agent_id,
                    key_epoch: 1,
                    certificate_hash: h.cert.digest().unwrap(),
                    issued_at: time(1000),
                },
                &h.owner,
            ),
            now(),
        )
        .unwrap();
    assert!(h.actor.snapshot().unwrap().revoked(h.offer.agent_id));
    assert!(matches!(
        h.admit(Operation::OwnHeartbeat, 2),
        Err(Error::Revoked)
    ));
    h.memory.0.lock().unwrap().0 = before;
    assert!(matches!(
        h.admit(Operation::OwnHeartbeat, 3),
        Err(Error::WrongCheckpoint)
    ));
}

fn rotated(h: &Harness, new: &SigningKey) -> (Certificate, RotationProof) {
    let mut cert = h.cert.clone();
    cert.header = header(3);
    cert.key_epoch = 2;
    cert.agent_key = public(new);
    cert.origin = CertificateOrigin::Rotation {
        previous_certificate: h.cert.digest().unwrap(),
        previous_key_epoch: 1,
    };
    let proof = RotationProof {
        header: header(3),
        agent_id: h.offer.agent_id,
        certificate_hash: cert.digest().unwrap(),
        nonce: Nonce::from_bytes([9; 16]),
        issued_at: time(1000),
    };
    (cert, proof)
}
#[test]
fn rotation_preserves_agent_id_requires_both_keys_and_owner_and_exact_next_epoch() {
    let mut h = Harness::enrolled();
    let new = key(44);
    let (cert, proof) = rotated(&h, &new);
    assert_eq!(
        h.actor.rotate(
            sign(cert.clone(), &h.owner),
            sign(proof.clone(), &h.agent),
            sign(NewKeyProof(proof.clone()), &key(99)),
            now()
        ),
        Err(Error::InvalidSignature)
    );
    assert_eq!(
        h.actor.rotate(
            sign(cert.clone(), &key(99)),
            sign(proof.clone(), &h.agent),
            sign(NewKeyProof(proof.clone()), &new),
            now()
        ),
        Err(Error::InvalidSignature)
    );
    h.actor
        .rotate(
            sign(cert.clone(), &h.owner),
            sign(proof.clone(), &h.agent),
            sign(NewKeyProof(proof), &new),
            now(),
        )
        .unwrap();
    assert_eq!(
        h.actor
            .snapshot()
            .unwrap()
            .certificate(h.offer.agent_id)
            .unwrap()
            .key_epoch,
        2
    );
    assert!(matches!(
        h.admit(Operation::OwnHeartbeat, 2),
        Err(Error::WrongEpoch)
    ));
    h.cert = cert;
    h.agent = new;
    h.admit(Operation::OwnHeartbeat, 2).unwrap();
}

#[test]
fn rotation_cannot_skip_epochs_expand_scopes_or_recover_without_old_key() {
    let mut h = Harness::enrolled();
    let new = key(44);
    let (mut cert, mut proof) = rotated(&h, &new);
    cert.key_epoch = 3;
    proof.certificate_hash = cert.digest().unwrap();
    assert_eq!(
        h.actor.rotate(
            sign(cert, &h.owner),
            sign(proof.clone(), &h.agent),
            sign(NewKeyProof(proof), &new),
            now()
        ),
        Err(Error::WrongEpoch)
    );
    let (mut cert, mut proof) = rotated(&h, &new);
    cert.scopes = scopes();
    proof.certificate_hash = cert.digest().unwrap();
    assert_eq!(
        h.actor.rotate(
            sign(cert, &h.owner),
            sign(proof.clone(), &h.agent),
            sign(NewKeyProof(proof), &new),
            now()
        ),
        Err(Error::ScopeDenied)
    );
    let (cert, proof) = rotated(&h, &new);
    assert_eq!(
        h.actor.rotate(
            sign(cert, &h.owner),
            sign(proof.clone(), &key(98)),
            sign(NewKeyProof(proof), &new),
            now()
        ),
        Err(Error::InvalidSignature)
    );
}

#[test]
fn owner_pin_rotation_is_monotonic_and_cannot_self_extend_expiry() {
    let mut h = Harness::enrolled();
    let new = key(55);
    let change = OwnerRotation {
        header: header(3),
        new_epoch: 2,
        new_key: public(&new),
        issued_at: time(1000),
        expires_at: time(4600),
    };
    h.actor
        .rotate_owner(
            sign(change.clone(), &h.owner),
            sign(NewOwnerProof(change.clone()), &new),
            now(),
        )
        .unwrap();
    assert_eq!(h.actor.snapshot().unwrap().owner().epoch, 2);
    let newer = key(66);
    let extension = OwnerRotation {
        header: Header {
            owner_epoch: 2,
            checkpoint: 4,
            ..header(4)
        },
        new_epoch: 3,
        new_key: public(&newer),
        issued_at: time(1000),
        expires_at: time(6000),
    };
    assert_eq!(
        h.actor.rotate_owner(
            sign(extension.clone(), &new),
            sign(NewOwnerProof(extension), &newer),
            now()
        ),
        Err(Error::Expired)
    );
    assert_eq!(
        h.actor.rotate_owner(
            sign(change.clone(), &h.owner),
            sign(NewOwnerProof(change), &new),
            now()
        ),
        Err(Error::WrongEpoch)
    );
    let op = Operation::OwnHeartbeat;
    let lease = h.lease(op.clone(), 2);
    let proof = h.proof(&lease, 2);
    let fresh = Freshness {
        header: h.current_header(),
        issued_at: time(1000),
        expires_at: time(1300),
    };
    assert!(h
        .actor
        .admit(
            sign(lease, &new),
            proof,
            Some(&sign(fresh, &new)),
            &op,
            now()
        )
        .is_ok());
}

#[test]
fn domains_are_not_interchangeable_even_for_identical_transition_bodies() {
    let mut h = Harness::enrolled();
    let new = key(44);
    let (cert, proof) = rotated(&h, &new);
    let wrong_domain = sign(proof.clone(), &new).signature;
    assert_eq!(
        h.actor.rotate(
            sign(cert, &h.owner),
            sign(proof.clone(), &h.agent),
            Signed {
                body: NewKeyProof(proof),
                signature: wrong_domain
            },
            now()
        ),
        Err(Error::InvalidSignature)
    );
    let fresh = h.fresh();
    let mut bytes = fresh.body.signing_bytes().unwrap();
    bytes.splice(..0, b"consolecrypt.device.login\0".iter().copied());
    let fake = Signed {
        body: fresh.body,
        signature: SignatureBytes::from_bytes(h.owner.sign(&bytes).to_bytes()),
    };
    let op = Operation::OwnHeartbeat;
    let lease = h.lease(op.clone(), 2);
    let proof = h.proof(&lease, 2);
    assert!(matches!(
        h.actor
            .admit(sign(lease, &h.owner), proof, Some(&fake), &op, now()),
        Err(Error::InvalidSignature)
    ));
}

#[test]
fn capabilities_unknown_versions_duplicate_scopes_and_weak_keys_fail_closed() {
    assert_eq!(
        Scopes(vec![Scope::TelemetryRead, Scope::TelemetryRead]).validate(),
        Err(Error::InvalidWire)
    );
    assert_eq!(
        Capabilities {
            version: 2,
            scopes: scopes()
        }
        .validate(),
        Err(Error::InvalidWire)
    );
    assert_eq!(
        PublicKey::from_bytes([0; 32]).validate(),
        Err(Error::InvalidKey)
    );
    let mut h = Harness::offered(Scopes(vec![Scope::TelemetryRead]), Scopes(vec![]));
    h.actor
        .consume(&h.token, sign(h.claim.clone(), &h.agent), now())
        .unwrap();
    assert_eq!(
        h.actor.approve(sign(h.cert.clone(), &h.owner), now()),
        Err(Error::CapabilityDenied)
    );
}

#[test]
fn strict_bounded_wire_rejects_unknown_fields_versions_duplicate_fields_and_future_scope() {
    let h = Harness::enrolled();
    let signed = sign(h.offer.clone(), &h.owner);
    let raw = serde_json::to_vec(&signed).unwrap();
    assert!(Signed::<Offer>::from_json(&raw).is_ok());
    let mut value = serde_json::to_value(&signed).unwrap();
    value["body"]["future"] = json!(true);
    assert!(Signed::<Offer>::from_json(&serde_json::to_vec(&value).unwrap()).is_err());
    let mut value = serde_json::to_value(&signed).unwrap();
    value["body"]["header"]["version"] = json!(2);
    assert!(Signed::<Offer>::from_json(&serde_json::to_vec(&value).unwrap()).is_err());
    let mut value = serde_json::to_value(&signed).unwrap();
    value["body"]["scopes"] = json!(["metrics.write"]);
    assert!(Signed::<Offer>::from_json(&serde_json::to_vec(&value).unwrap()).is_err());
    let raw = String::from_utf8(raw).unwrap().replacen(
        "\"signature\":",
        &format!(
            "\"signature\":\"{}\",\"signature\":",
            hex::encode(signed.signature.as_bytes())
        ),
        1,
    );
    assert!(Signed::<Offer>::from_json(raw.as_bytes()).is_err());
    assert!(Signed::<Offer>::from_json(&vec![b' '; MAX_WIRE_BYTES + 1]).is_err());
    assert!(AgentId::new("11111111111141118111111111111111").is_err());
    assert!(serde_json::from_str::<Tenant>(&format!(
        r#"{{"kind":"localProfile","id":"{ID}","organization":true}}"#
    ))
    .is_err());
}

#[test]
fn team_authority_is_unavailable_and_tenant_tags_are_not_interchangeable() {
    let h = Harness::enrolled();
    let mut pin = h.actor.snapshot().unwrap().pin;
    pin.context.tenant = Tenant::Team {
        id: TeamId::new(ID).unwrap(),
    };
    assert!(matches!(
        State::bootstrap(pin, now()),
        Err(Error::UnsupportedTeam)
    ));
    assert_ne!(
        context().tenant,
        Tenant::PersonalAccount {
            id: PersonalAccountId::new(ID).unwrap()
        }
    );
    let mut h = Harness::claimed();
    let mut cert = h.cert.clone();
    cert.header.context.deployment_id = DeploymentId::new(ID).unwrap();
    assert_eq!(
        h.actor.approve(sign(cert, &h.owner), now()),
        Err(Error::WrongContext)
    );
}

#[test]
fn pending_nonce_and_agent_quotas_fail_without_dropping_replay_records() {
    let mut h = Harness::enrolled();
    {
        let mut store = h.memory.0.lock().unwrap();
        for i in 0..MAX_NONCES - 1 {
            let mut nonce = [0; 16];
            nonce[..8].copy_from_slice(&(i as u64 + 100).to_be_bytes());
            store.0.nonces.insert(Nonce::from_bytes(nonce));
        }
    }
    assert!(matches!(
        h.admit(Operation::OwnHeartbeat, 2),
        Err(Error::Quota)
    ));
    assert!(h.actor.snapshot().unwrap().nonces.contains(&h.claim.nonce));
    let mut h = Harness::offered(Scopes(vec![]), Scopes(vec![]));
    {
        let mut store = h.memory.0.lock().unwrap();
        let row = store.0.invitations.values().next().unwrap().clone();
        for i in 1..MAX_PENDING {
            store
                .0
                .invitations
                .insert(InvitationId::new(&uuid(i as u64)).unwrap(), row.clone());
        }
    }
    let mut offer = h.offer.clone();
    offer.header.checkpoint = 2;
    offer.invitation_id = InvitationId::new(&uuid(99)).unwrap();
    offer.agent_id = AgentId::new(&uuid(99)).unwrap();
    assert_eq!(
        h.actor.offer(sign(offer, &h.owner), now()),
        Err(Error::Quota)
    );
}

#[test]
fn clock_rollback_and_replayed_checkpoint_are_denied() {
    let mut h = Harness::claimed();
    assert_eq!(
        h.actor
            .approve(sign(h.cert.clone(), &h.owner), Now(time(999))),
        Err(Error::InvalidTime)
    );
    h.actor
        .approve(sign(h.cert.clone(), &h.owner), now())
        .unwrap();
    assert_eq!(
        h.actor.approve(sign(h.cert.clone(), &h.owner), now()),
        Err(Error::WrongCheckpoint)
    );
}

#[test]
fn canonical_freshness_vector_has_fixed_domain_sorted_keys_and_integer_times() {
    let body = Freshness {
        header: header(2),
        issued_at: time(1000),
        expires_at: time(1300),
    };
    let expected = format!(
        r#"{{"expiresAt":1300,"header":{{"checkpoint":2,"context":{{"deploymentId":"{DEPLOY}","tenant":{{"id":"{ID}","kind":"localProfile"}}}},"ownerEpoch":1,"version":1}},"issuedAt":1000}}"#
    );
    let mut bytes = b"consolecrypt.agent.identity.personal-prototype\0v1\0freshness\0".to_vec();
    bytes.extend_from_slice(&(expected.len() as u32).to_be_bytes());
    bytes.extend(expected.as_bytes());
    assert_eq!(body.signing_bytes().unwrap(), bytes);
    assert_eq!(
        format!("{:?}", InvitationToken::from_bytes([42; 32]).unwrap()),
        "InvitationToken([redacted])"
    );
}

#[test]
fn permit_recheck_enforces_expiry_and_new_revocation_checkpoint() {
    let mut h = Harness::enrolled();
    let permit = h.admit(Operation::OwnHeartbeat, 2).unwrap();
    h.actor.check_permit(&permit, now()).unwrap();
    assert_eq!(
        h.actor.check_permit(&permit, Now(time(1120))),
        Err(Error::Expired)
    );
    assert_eq!(
        h.actor.check_permit(&permit, now()),
        Err(Error::InvalidTime)
    );
    let mut h = Harness::enrolled();
    let permit = h.admit(Operation::OwnHeartbeat, 2).unwrap();
    h.actor
        .revoke(
            sign(
                Revocation {
                    header: header(3),
                    agent_id: h.offer.agent_id,
                    key_epoch: 1,
                    certificate_hash: h.cert.digest().unwrap(),
                    issued_at: time(1000),
                },
                &h.owner,
            ),
            now(),
        )
        .unwrap();
    assert_eq!(
        h.actor.check_permit(&permit, now()),
        Err(Error::WrongCheckpoint)
    );
}

#[test]
fn failed_claim_cas_neither_consumes_offer_nor_commits_nonce() {
    let mut h = Harness::offered(Scopes(vec![]), scopes());
    h.memory.0.lock().unwrap().1 = true;
    assert_eq!(
        h.actor
            .consume(&h.token, sign(h.claim.clone(), &h.agent), now()),
        Err(Error::Conflict)
    );
    let state = h.actor.snapshot().unwrap();
    assert_eq!(
        state.invitation_status(h.offer.invitation_id),
        Some(InvitationStatus::Offered)
    );
    assert!(!state.nonces.contains(&h.claim.nonce));
    h.memory.0.lock().unwrap().1 = false;
    h.actor
        .consume(&h.token, sign(h.claim.clone(), &h.agent), now())
        .unwrap();
}

#[test]
fn agent_and_retained_invitation_limits_never_evict_terminal_identity() {
    let mut h = Harness::claimed();
    {
        let mut store = h.memory.0.lock().unwrap();
        for n in 0..MAX_AGENTS {
            let mut certificate = h.cert.clone();
            certificate.agent_id = AgentId::new(&uuid(n as u64)).unwrap();
            store.0.agents.insert(
                certificate.agent_id,
                Agent {
                    certificate,
                    revoked: true,
                },
            );
        }
    }
    assert_eq!(
        h.actor.approve(sign(h.cert.clone(), &h.owner), now()),
        Err(Error::Quota)
    );
    assert_eq!(h.actor.snapshot().unwrap().agents.len(), MAX_AGENTS);
    let mut h = Harness::offered(Scopes(vec![]), scopes());
    {
        let mut store = h.memory.0.lock().unwrap();
        let row = store.0.invitations.values().next().unwrap().clone();
        for n in 1..MAX_INVITATIONS {
            let mut terminal = row.clone();
            terminal.status = InvitationStatus::Expired;
            store
                .0
                .invitations
                .insert(InvitationId::new(&uuid(n as u64)).unwrap(), terminal);
        }
    }
    let mut offer = h.offer.clone();
    offer.header.checkpoint = 2;
    offer.invitation_id = InvitationId::new(&uuid(900)).unwrap();
    offer.agent_id = AgentId::new(&uuid(900)).unwrap();
    assert_eq!(
        h.actor.offer(sign(offer, &h.owner), now()),
        Err(Error::Quota)
    );
    assert_eq!(
        h.actor.snapshot().unwrap().invitations.len(),
        MAX_INVITATIONS
    );
}

struct DurableTemp(std::path::PathBuf);
impl DurableTemp {
    fn new() -> Self {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "cc-agent-durable-unit-{}-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, std::sync::atomic::Ordering::SeqCst),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        #[cfg(unix)]
        {
            use std::os::unix::fs::DirBuilderExt;
            std::fs::DirBuilder::new()
                .mode(0o700)
                .create(&path)
                .unwrap();
        }
        #[cfg(not(unix))]
        std::fs::create_dir(&path).unwrap();
        Self(path)
    }
    fn pin(h: &Harness) -> OwnerPin {
        OwnerPin {
            context: context(),
            key: public(&h.owner),
            epoch: 1,
            checkpoint: 0,
            expires_at: time(1000 + CERTIFICATE_TTL),
        }
    }
    fn create(&self, h: &Harness) -> Actor<FileStore> {
        Actor::new(
            FileStore::create(
                self.0.join("data"),
                self.0.join("anchor"),
                Self::pin(h),
                now(),
            )
            .unwrap(),
        )
        .unwrap()
    }
    fn open(&self, h: &Harness) -> Actor<FileStore> {
        Actor::new(
            FileStore::open(self.0.join("data"), self.0.join("anchor"), Self::pin(h)).unwrap(),
        )
        .unwrap()
    }
}
impl Drop for DurableTemp {
    fn drop(&mut self) {
        let parent = std::env::temp_dir();
        assert_eq!(self.0.parent(), Some(parent.as_path()));
        assert!(self
            .0
            .file_name()
            .unwrap()
            .to_string_lossy()
            .starts_with("cc-agent-durable-unit-"));
        assert!(!std::fs::symlink_metadata(&self.0)
            .unwrap()
            .file_type()
            .is_symlink());
        std::fs::remove_dir_all(&self.0).unwrap();
    }
}
#[test]
fn durable_claim_registration_rotation_revoke_and_clock_watermark_survive_restart() {
    let mut h = Harness::enrolled();
    let temp = DurableTemp::new();
    let mut actor = temp.create(&h);
    actor.offer(sign(h.offer.clone(), &h.owner), now()).unwrap();
    drop(actor);
    let mut actor = temp.open(&h);
    assert!(matches!(
        actor.consume(
            &InvitationToken::from_bytes([99; 32]).unwrap(),
            sign(h.claim.clone(), &h.agent),
            now()
        ),
        Err(Error::InvalidToken)
    ));
    actor
        .consume(&h.token, sign(h.claim.clone(), &h.agent), now())
        .unwrap();
    drop(actor);
    let mut actor = temp.open(&h);
    assert_eq!(
        actor
            .snapshot()
            .unwrap()
            .invitation_status(h.offer.invitation_id),
        Some(InvitationStatus::Claimed)
    );
    assert!(actor
        .snapshot()
        .unwrap()
        .certificate(h.offer.agent_id)
        .is_none());
    assert!(matches!(
        actor.consume(&h.token, sign(h.claim.clone(), &h.agent), now()),
        Err(Error::Replay)
    ));
    actor
        .approve(sign(h.cert.clone(), &h.owner), now())
        .unwrap();
    drop(actor);
    let new = key(44);
    let (cert, proof) = rotated(&h, &new);
    let mut actor = temp.open(&h);
    actor
        .rotate(
            sign(cert.clone(), &h.owner),
            sign(proof.clone(), &h.agent),
            sign(NewKeyProof(proof), &new),
            now(),
        )
        .unwrap();
    drop(actor);
    let mut actor = temp.open(&h);
    let op = Operation::OwnHeartbeat;
    let mut lease = h.lease(op.clone(), 2);
    lease.header = header(3);
    let mut proof = h.proof(&lease, 2);
    proof.body.header = header(3);
    proof = sign(proof.body, &h.agent);
    let fresh = sign(
        Freshness {
            header: header(3),
            issued_at: time(1000),
            expires_at: time(1300),
        },
        &h.owner,
    );
    assert!(matches!(
        actor.admit(sign(lease, &h.owner), proof, Some(&fresh), &op, now()),
        Err(Error::WrongEpoch)
    ));
    h.cert = cert;
    h.agent = new;
    let mut lease = h.lease(op.clone(), 3);
    lease.header = header(3);
    let proof = h.proof(&lease, 3);
    let permit = actor
        .admit(sign(lease, &h.owner), proof, Some(&fresh), &op, now())
        .unwrap();
    actor
        .revoke(
            sign(
                Revocation {
                    header: header(4),
                    agent_id: h.offer.agent_id,
                    key_epoch: 2,
                    certificate_hash: h.cert.digest().unwrap(),
                    issued_at: time(1001),
                },
                &h.owner,
            ),
            Now(time(1001)),
        )
        .unwrap();
    drop(actor);
    let mut actor = temp.open(&h);
    assert!(actor.snapshot().unwrap().revoked(h.offer.agent_id));
    assert!(matches!(
        actor.check_permit(&permit, Now(time(1000))),
        Err(Error::InvalidTime)
    ));
    assert!(matches!(
        actor.check_permit(&permit, Now(time(1001))),
        Err(Error::WrongCheckpoint)
    ));
}
#[test]
#[ignore = "explicit bounded latency measurement; full journal replay is intentionally slow"]
fn durable_journal_bounded_50_and_100_lease_restore_latency_evidence() {
    let h = Harness::enrolled();
    let temp = DurableTemp::new();
    let mut actor = temp.create(&h);
    actor.offer(sign(h.offer.clone(), &h.owner), now()).unwrap();
    actor
        .consume(&h.token, sign(h.claim.clone(), &h.agent), now())
        .unwrap();
    actor
        .approve(sign(h.cert.clone(), &h.owner), now())
        .unwrap();
    for n in 2..=101u8 {
        let op = Operation::OwnHeartbeat;
        let lease = h.lease(op.clone(), n);
        let proof = h.proof(&lease, n);
        actor
            .admit(sign(lease, &h.owner), proof, Some(&h.fresh()), &op, now())
            .unwrap();
        if n == 51 || n == 101 {
            let started = std::time::Instant::now();
            let restored = temp.open(&h);
            let elapsed = started.elapsed();
            assert_eq!(restored.snapshot().unwrap().revision(), u64::from(n) + 2);
            eprintln!(
                "durable-restore: leases={} revision={} elapsed_ms={} debug_build=true",
                n - 1,
                n + 2,
                elapsed.as_millis()
            );
        }
    }
}

#[test]
fn durable_concurrent_claims_commit_exactly_one_pending_registration() {
    let h = Harness::offered(Scopes(vec![Scope::TelemetryRead]), scopes());
    let temp = DurableTemp::new();
    let mut initial = temp.create(&h);
    initial
        .offer(sign(h.offer.clone(), &h.owner), now())
        .unwrap();
    drop(initial);
    let mut first = temp.open(&h);
    let mut second = temp.open(&h);
    let claim = sign(h.claim.clone(), &h.agent);
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));
    std::thread::scope(|scope| {
        let one = scope.spawn(|| {
            barrier.wait();
            first.consume(
                &InvitationToken::from_bytes([33; 32]).unwrap(),
                claim.clone(),
                now(),
            )
        });
        let two = scope.spawn(|| {
            barrier.wait();
            second.consume(
                &InvitationToken::from_bytes([33; 32]).unwrap(),
                claim.clone(),
                now(),
            )
        });
        let one = one.join().unwrap();
        let two = two.join().unwrap();
        assert_eq!(usize::from(one.is_ok()) + usize::from(two.is_ok()), 1);
        let denied = if one.is_err() { one } else { two };
        assert!(matches!(denied, Err(Error::Replay | Error::Conflict)));
    });
    let restored = temp.open(&h).snapshot().unwrap();
    assert_eq!(restored.revision(), 2);
    assert_eq!(
        restored.invitation_status(h.offer.invitation_id),
        Some(InvitationStatus::Claimed)
    );
    assert!(restored.certificate(h.offer.agent_id).is_none());
}

#[test]
fn identity_policy_regression_shared_pending_and_owner_key_must_be_denied() {
    for owner_key in [false, true] {
        let mut h = Harness::offered(Scopes(vec![Scope::TelemetryRead]), scopes());
        let mut second = h.offer.clone();
        second.header = header(2);
        second.invitation_id = InvitationId::new(&uuid(800)).unwrap();
        second.agent_id = AgentId::new(&uuid(801)).unwrap();
        if owner_key {
            second.agent_key = public(&h.owner);
        }
        assert!(
            h.actor.offer(sign(second, &h.owner), now()).is_err(),
            "authority accepted reused public key; owner_key={owner_key}"
        );
    }
}
#[test]
fn identity_policy_regression_terminal_invitation_retry_keeps_stable_node_id() {
    let h = Harness::offered(Scopes(vec![Scope::TelemetryRead]), scopes());
    let temp = DurableTemp::new();
    let mut actor = temp.create(&h);
    actor.offer(sign(h.offer.clone(), &h.owner), now()).unwrap();
    actor
        .expire(h.offer.invitation_id, Now(time(1600)))
        .unwrap();
    drop(actor);
    let mut actor = temp.open(&h);
    let mut renewed = h.offer.clone();
    renewed.header = header(2);
    renewed.invitation_id = InvitationId::new(&uuid(802)).unwrap();
    renewed.token_hash = InvitationToken::from_bytes([34; 32]).unwrap().digest();
    renewed.issued_at = time(1600);
    renewed.expires_at = time(2200);
    assert_eq!(
        actor.offer(sign(renewed, &h.owner), Now(time(1600))),
        Ok(()),
        "expired invitation prevents stable node retry"
    );
}

#[test]
fn durable_terminal_retry_preserves_old_token_claim_nonce_tombstones_and_stable_node_key() {
    for rejected in [false, true] {
        let h = Harness::offered(Scopes(vec![Scope::TelemetryRead]), scopes());
        let temp = DurableTemp::new();
        let mut actor = temp.create(&h);
        actor.offer(sign(h.offer.clone(), &h.owner), now()).unwrap();
        let observed = if rejected {
            actor
                .consume(&h.token, sign(h.claim.clone(), &h.agent), now())
                .unwrap();
            actor
                .reject(
                    sign(
                        Rejection {
                            header: header(2),
                            invitation_id: h.offer.invitation_id,
                            claim_hash: h.claim.digest().unwrap(),
                            issued_at: time(1000),
                        },
                        &h.owner,
                    ),
                    now(),
                )
                .unwrap();
            now()
        } else {
            actor
                .expire(h.offer.invitation_id, Now(time(1600)))
                .unwrap();
            Now(time(1600))
        };
        drop(actor);
        let mut actor = temp.open(&h);
        assert!(matches!(
            actor.consume(&h.token, sign(h.claim.clone(), &h.agent), observed),
            Err(Error::Replay)
        ));
        let cp = actor.snapshot().unwrap().owner().checkpoint + 1;
        let token = InvitationToken::from_bytes([34; 32]).unwrap();
        let mut offer = h.offer.clone();
        offer.header = header(cp);
        offer.invitation_id = InvitationId::new(&uuid(803)).unwrap();
        offer.token_hash = token.digest();
        offer.issued_at = observed.0;
        offer.expires_at = observed.0.checked_add(600).unwrap();
        let mut changed_key = offer.clone();
        changed_key.agent_key = public(&key(99));
        assert_eq!(
            actor.offer(sign(changed_key, &h.owner), observed),
            Err(Error::WrongClaim)
        );
        let mut reused_secret = offer.clone();
        reused_secret.token_hash = h.token.digest();
        assert_eq!(
            actor.offer(sign(reused_secret, &h.owner), observed),
            Err(Error::Replay)
        );
        actor
            .offer(sign(offer.clone(), &h.owner), observed)
            .unwrap();
        drop(actor);
        let mut actor = temp.open(&h);
        let mut claim = h.claim.clone();
        claim.header = offer.header.clone();
        claim.invitation_id = offer.invitation_id;
        claim.offer_hash = offer.digest().unwrap();
        claim.issued_at = observed.0;
        if rejected {
            assert!(matches!(
                actor.consume(&token, sign(claim.clone(), &h.agent), observed),
                Err(Error::Replay)
            ));
        }
        claim.nonce = Nonce::from_bytes([2; 16]);
        assert!(matches!(
            actor.consume(&h.token, sign(claim.clone(), &h.agent), observed),
            Err(Error::InvalidToken)
        ));
        actor
            .consume(&token, sign(claim.clone(), &h.agent), observed)
            .unwrap();
        drop(actor);
        let mut actor = temp.open(&h);
        assert!(matches!(
            actor.consume(&h.token, sign(h.claim.clone(), &h.agent), observed),
            Err(Error::Replay)
        ));
        let mut cert = h.cert.clone();
        cert.header = header(cp + 1);
        cert.not_before = observed.0;
        cert.expires_at = observed.0.checked_add(3600).unwrap();
        cert.origin = CertificateOrigin::Enrollment {
            invitation_id: offer.invitation_id,
            claim_hash: claim.digest().unwrap(),
            key_fingerprint: offer.agent_key.fingerprint(),
        };
        actor.approve(sign(cert, &h.owner), observed).unwrap();
        drop(actor);
        let restored = temp.open(&h).snapshot().unwrap();
        assert_eq!(
            restored.invitation_status(h.offer.invitation_id),
            Some(if rejected {
                InvitationStatus::Rejected
            } else {
                InvitationStatus::Expired
            })
        );
        assert_eq!(
            restored.invitation_status(offer.invitation_id),
            Some(InvitationStatus::Approved)
        );
        assert_eq!(
            restored.certificate(offer.agent_id).unwrap().agent_key,
            h.offer.agent_key
        );
        assert_eq!(restored.capacity().retained_invitations, 2);
    }
}
#[test]
fn active_or_unexpired_transition_must_be_explicitly_terminal_before_stable_node_retry() {
    for claimed in [false, true] {
        let mut h = if claimed {
            Harness::claimed()
        } else {
            Harness::offered(Scopes(vec![Scope::TelemetryRead]), scopes())
        };
        let mut offer = h.offer.clone();
        offer.header = header(2);
        offer.invitation_id = InvitationId::new(&uuid(804)).unwrap();
        offer.issued_at = time(1600);
        offer.expires_at = time(2200);
        assert_eq!(
            h.actor.offer(sign(offer, &h.owner), Now(time(1600))),
            Err(Error::Replay)
        );
        assert_eq!(
            h.actor
                .snapshot()
                .unwrap()
                .invitation_status(h.offer.invitation_id),
            Some(if claimed {
                InvitationStatus::Claimed
            } else {
                InvitationStatus::Offered
            })
        );
    }
}
#[test]
fn public_key_reuse_is_denied_for_claimed_registered_revoked_and_current_owner_roles() {
    for state in ["offered", "claimed", "registered", "revoked", "owner"] {
        let mut h = match state {
            "offered" | "owner" => Harness::offered(Scopes(vec![Scope::TelemetryRead]), scopes()),
            "claimed" => Harness::claimed(),
            _ => Harness::enrolled(),
        };
        if state == "revoked" {
            h.actor
                .revoke(
                    sign(
                        Revocation {
                            header: header(3),
                            agent_id: h.offer.agent_id,
                            key_epoch: 1,
                            certificate_hash: h.cert.digest().unwrap(),
                            issued_at: time(1000),
                        },
                        &h.owner,
                    ),
                    now(),
                )
                .unwrap();
        }
        let before = h.actor.snapshot().unwrap();
        let mut offer = h.offer.clone();
        offer.header = header(before.owner().checkpoint + 1);
        offer.agent_id = AgentId::new(&uuid(805)).unwrap();
        offer.invitation_id = InvitationId::new(&uuid(806)).unwrap();
        if state == "owner" {
            offer.agent_key = public(&h.owner);
        }
        assert_eq!(
            h.actor.offer(sign(offer, &h.owner), now()),
            Err(Error::KeyReuse),
            "accepted reuse from {state}"
        );
        assert_eq!(h.actor.snapshot().unwrap().revision(), before.revision());
    }
}
fn rotate_for_policy<S: CompareAndSwap>(
    actor: &mut Actor<S>,
    cert: &Certificate,
    old: &SigningKey,
    owner: &SigningKey,
    new: &SigningKey,
) -> Result<Certificate> {
    let snapshot = actor.snapshot()?;
    let mut next = cert.clone();
    next.header = Header {
        version: 1,
        context: snapshot.owner().context.clone(),
        owner_epoch: snapshot.owner().epoch,
        checkpoint: snapshot.owner().checkpoint + 1,
    };
    next.key_epoch = cert.key_epoch + 1;
    next.agent_key = public(new);
    next.origin = CertificateOrigin::Rotation {
        previous_certificate: cert.digest()?,
        previous_key_epoch: cert.key_epoch,
    };
    let proof = RotationProof {
        header: next.header.clone(),
        agent_id: next.agent_id,
        certificate_hash: next.digest()?,
        nonce: Nonce::from_bytes([20 + next.header.checkpoint as u8; 16]),
        issued_at: time(1000),
    };
    actor.rotate(
        sign(next.clone(), owner),
        sign(proof.clone(), old),
        sign(NewKeyProof(proof), new),
        now(),
    )?;
    Ok(next)
}
#[test]
fn agent_rotation_rejects_another_pending_key_and_current_owner_key() {
    for owner in [false, true] {
        let mut h = Harness::enrolled();
        let new = if owner { key(11) } else { key(44) };
        if !owner {
            let mut other = h.offer.clone();
            other.header = header(3);
            other.agent_id = AgentId::new(&uuid(807)).unwrap();
            other.invitation_id = InvitationId::new(&uuid(808)).unwrap();
            other.agent_key = public(&new);
            h.actor.offer(sign(other, &h.owner), now()).unwrap();
        }
        assert!(matches!(
            rotate_for_policy(&mut h.actor, &h.cert, &h.agent, &h.owner, &new),
            Err(Error::KeyReuse)
        ));
    }
}
#[test]
fn retired_agent_rotation_keys_remain_reserved_after_durable_restart() {
    let h = Harness::enrolled();
    let temp = DurableTemp::new();
    let mut actor = temp.create(&h);
    actor.offer(sign(h.offer.clone(), &h.owner), now()).unwrap();
    actor
        .consume(&h.token, sign(h.claim.clone(), &h.agent), now())
        .unwrap();
    actor
        .approve(sign(h.cert.clone(), &h.owner), now())
        .unwrap();
    let middle = key(44);
    let latest = key(55);
    let cert = rotate_for_policy(&mut actor, &h.cert, &h.agent, &h.owner, &middle).unwrap();
    let cert = rotate_for_policy(&mut actor, &cert, &middle, &h.owner, &latest).unwrap();
    drop(actor);
    let mut actor = temp.open(&h);
    assert_eq!(
        actor
            .snapshot()
            .unwrap()
            .capacity()
            .retained_agent_key_tombstones,
        3
    );
    assert!(matches!(
        rotate_for_policy(&mut actor, &cert, &latest, &h.owner, &middle),
        Err(Error::KeyReuse)
    ));
    let mut other = h.offer.clone();
    other.header = header(5);
    other.agent_id = AgentId::new(&uuid(809)).unwrap();
    other.invitation_id = InvitationId::new(&uuid(810)).unwrap();
    other.agent_key = public(&middle);
    assert_eq!(
        actor.offer(sign(other, &h.owner), now()),
        Err(Error::KeyReuse)
    );
}

#[test]
fn agent_rotation_rejects_a_different_registered_or_revoked_agent_key() {
    let mut h = Harness::enrolled();
    let other = key(44);
    let token = InvitationToken::from_bytes([34; 32]).unwrap();
    let mut offer = h.offer.clone();
    offer.header = header(3);
    offer.agent_id = AgentId::new(&uuid(811)).unwrap();
    offer.invitation_id = InvitationId::new(&uuid(812)).unwrap();
    offer.agent_key = public(&other);
    offer.token_hash = token.digest();
    h.actor.offer(sign(offer.clone(), &h.owner), now()).unwrap();
    let claim = Claim {
        header: offer.header.clone(),
        invitation_id: offer.invitation_id,
        offer_hash: offer.digest().unwrap(),
        agent_id: offer.agent_id,
        agent_key: offer.agent_key,
        capabilities: Capabilities {
            version: 1,
            scopes: Scopes(vec![Scope::TelemetryRead]),
        },
        nonce: Nonce::from_bytes([81; 16]),
        issued_at: time(1000),
    };
    h.actor
        .consume(&token, sign(claim.clone(), &other), now())
        .unwrap();
    let mut certificate = h.cert.clone();
    certificate.header = header(4);
    certificate.agent_id = offer.agent_id;
    certificate.agent_key = offer.agent_key;
    certificate.capabilities = claim.capabilities.clone();
    certificate.origin = CertificateOrigin::Enrollment {
        invitation_id: offer.invitation_id,
        claim_hash: claim.digest().unwrap(),
        key_fingerprint: offer.agent_key.fingerprint(),
    };
    h.actor
        .approve(sign(certificate.clone(), &h.owner), now())
        .unwrap();
    assert!(matches!(
        rotate_for_policy(&mut h.actor, &h.cert, &h.agent, &h.owner, &other),
        Err(Error::KeyReuse)
    ));
    h.actor
        .revoke(
            sign(
                Revocation {
                    header: header(5),
                    agent_id: certificate.agent_id,
                    key_epoch: 1,
                    certificate_hash: certificate.digest().unwrap(),
                    issued_at: time(1000),
                },
                &h.owner,
            ),
            now(),
        )
        .unwrap();
    assert!(matches!(
        rotate_for_policy(&mut h.actor, &h.cert, &h.agent, &h.owner, &other),
        Err(Error::KeyReuse)
    ));
}

// New bound-tunnel cases use this actual existing signed Actor harness.
mod tunnel_bound {
    include!("tunnel_actor_tests.rs");
}

// Actor-only private clean-drain fixture; no physical transport proof.
mod tunnel_resumption { include!("tunnel_resume_tests.rs"); }

// Actual owned TCP read -> Actor debit boundary, no TLS/physical drain claim.
#[cfg(target_os = "linux")]
mod supervised_io { include!("tunnel_supervisor_actor_tests.rs"); }
