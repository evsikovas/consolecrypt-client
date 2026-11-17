//! Genuine signed enrollment and actual Actor/CAS/replay. No OS/endurance proof.
use super::*;
use ed25519_dalek::{Signer, SigningKey};
use std::sync::{Arc, Mutex};

fn time(n: u64) -> Timestamp {
    Timestamp::new(n).unwrap()
}
fn now(n: u64) -> Now {
    Now(time(n))
}
fn boot() -> ResourceId {
    ResourceId::new("99999999-9999-4999-8999-999999999999").unwrap()
}
fn sample(at: u64) -> Result<CollectorServiceClockObservation> {
    CollectorServiceClockObservation::journal(boot(), (at - 1000 + 1) * 1_000_000_000, time(at))
}

fn sign<T: Statement>(body: T, key: &SigningKey) -> Signed<T> {
    Signed {
        signature: SignatureBytes::from_bytes(key.sign(&body.signing_bytes().unwrap()).to_bytes()),
        body,
    }
}
fn public(key: &SigningKey) -> PublicKey {
    PublicKey::from_bytes(key.verifying_key().to_bytes())
}
#[derive(Clone)]
struct Memory(Arc<Mutex<(State, Vec<Entry>, bool)>>);
impl CompareAndSwap for Memory {
    fn load(&self) -> Result<State> {
        Ok(self.0.lock().unwrap().0.clone())
    }
    fn compare_and_swap(&mut self, expected: u64, next: State) -> Result<()> {
        let mut state = self.0.lock().unwrap();
        if state.2 || state.0.revision() != expected {
            return Err(Error::Conflict);
        }
        state.1.push(next.latest.clone().unwrap());
        state.0 = next;
        Ok(())
    }
}
struct Fixture {
    owner: SigningKey,
    node: SigningKey,
    pin: OwnerPin,
    actor: Actor<Memory>,
    memory: Memory,
    policy: Signed<CollectorServiceDelegation>,
}
impl Fixture {
    fn new() -> Self {
        let owner = SigningKey::from_bytes(&[11; 32]);
        let node = SigningKey::from_bytes(&[22; 32]);
        let context: Context = serde_json::from_value(serde_json::json!({
            "tenant":{"kind":"localProfile","id":"11111111-1111-4111-8111-111111111111"},
            "deploymentId":"22222222-2222-4222-8222-222222222222"
        }))
        .unwrap();
        let pin = OwnerPin {
            context: context.clone(),
            key: public(&owner),
            epoch: 1,
            checkpoint: 0,
            expires_at: time(200000),
        };
        let memory = Memory(Arc::new(Mutex::new((
            State::bootstrap(pin.clone(), now(1000)).unwrap(),
            vec![],
            false,
        ))));
        let mut actor = Actor::new(memory.clone()).unwrap();
        let header = |checkpoint| Header {
            version: 1,
            context: context.clone(),
            owner_epoch: 1,
            checkpoint,
        };
        let token = InvitationToken::from_bytes([33; 32]).unwrap();
        let agent = AgentId::new("44444444-4444-4444-8444-444444444444").unwrap();
        let offer = sign(
            Offer {
                header: header(1),
                invitation_id: InvitationId::new("33333333-3333-4333-8333-333333333333").unwrap(),
                agent_id: agent,
                agent_key: public(&node),
                token_hash: token.digest(),
                scopes: Scopes(vec![Scope::MetricsCollect]),
                issued_at: time(1000),
                expires_at: time(1100),
            },
            &owner,
        );
        actor.offer(offer.clone(), now(1000)).unwrap();
        let claim = sign(
            Claim {
                header: header(1),
                invitation_id: offer.body.invitation_id,
                offer_hash: offer.body.digest().unwrap(),
                agent_id: agent,
                agent_key: public(&node),
                capabilities: Capabilities {
                    version: 1,
                    scopes: Scopes(vec![Scope::MetricsCollect]),
                },
                nonce: Nonce::from_bytes([1; 16]),
                issued_at: time(1000),
            },
            &node,
        );
        actor.consume(&token, claim.clone(), now(1000)).unwrap();
        let certificate = sign(
            Certificate {
                header: header(2),
                agent_id: agent,
                key_epoch: 1,
                agent_key: public(&node),
                scopes: Scopes(vec![Scope::MetricsCollect]),
                capabilities: claim.body.capabilities.clone(),
                origin: CertificateOrigin::Enrollment {
                    invitation_id: offer.body.invitation_id,
                    claim_hash: claim.body.digest().unwrap(),
                    key_fingerprint: public(&node).fingerprint(),
                },
                not_before: time(1000),
                expires_at: time(200000),
            },
            &owner,
        );
        actor.approve(certificate.clone(), now(1000)).unwrap();
        let policy = sign(
            CollectorServiceDelegation {
                header: header(2),
                service_id: ResourceId::new("55555555-5555-4555-8555-555555555555").unwrap(),
                policy_revision: 1,
                revocation_epoch: 1,
                agent_id: agent,
                key_epoch: 1,
                recipient: public(&node),
                certificate_hash: certificate.body.digest().unwrap(),
                collector_id: collector_service_resource(Hash::from_bytes([9; 32])).unwrap(),
                manifest_sha256: Hash::from_bytes([9; 32]),
                profile: CollectorServiceProfile::MaintainedPrometheusDisk512mib2hV1,
                quota: CollectorServiceQuota::fixed(),
                credential_targets: Vec::new(),
                issued_at: time(1000),
                expires_at: time(87400),
            },
            &owner,
        );
        Self {
            owner,
            node,
            pin,
            actor,
            memory,
            policy,
        }
    }
    fn proof(&self) -> Signed<CollectorServiceClaim> {
        sign(
            CollectorServiceClaim {
                header: self.policy.body.header.clone(),
                agent_id: self.policy.body.agent_id,
                key_epoch: 1,
                delegation_hash: self.policy.body.digest().unwrap(),
                clock: CollectorServiceClockStamp {
                    boot_id: boot(),
                    created_boottime_ns: 1_000_000_000,
                    deadline_boottime_ns: 86401_000_000_000,
                },
                nonce: Nonce::from_bytes([2; 16]),
                issued_at: time(1000),
            },
            &self.node,
        )
    }
    fn admit(&mut self) {
        let proof = self.proof();
        self.actor
            .admit_collector_service(self.policy.clone(), proof, now(1000), &mut || sample(1000))
            .unwrap();
    }
    fn start(&mut self, n: u8, at: u64) -> Result<CollectorServicePermit> {
        self.actor.reserve_collector_service_start(
            self.policy.body.service_id,
            Nonce::from_bytes([n; 16]),
            now(at),
            &mut || sample(at),
        )
    }
    fn ended(&mut self, permit: &CollectorServicePermit, at: u64) {
        // Actor-layer positive end input only. This does NOT attest a real unit.
        self.actor
            .finish_collector_service(
                OriginalServiceEnd {
                    service_id: permit.service_id,
                    delegation_hash: permit.delegation_hash,
                    start: permit.start,
                },
                now(at),
            )
            .unwrap();
    }
    fn restored(&self) -> Actor<crate::durable::Memory> {
        let mut actor = Actor::new(crate::durable::Memory(
            State::bootstrap(self.pin.clone(), now(1000)).unwrap(),
        ))
        .unwrap();
        for entry in self.memory.0.lock().unwrap().1.clone() {
            crate::durable::replay(&mut actor, entry).unwrap();
        }
        actor
    }
}

#[test]
fn service_authority_survives_three_hour_point_but_never_original_end() {
    let mut f = Fixture::new();
    f.admit();
    let permit = f.start(3, 1000).unwrap();
    let policy = f
        .actor
        .check_collector_service(&permit, now(11801), &mut || sample(11801))
        .unwrap();
    assert_eq!(policy.expires_at, time(87400));
    // Fresh observations also advance the EXISTING Actor wall highwater.
    assert!(f
        .actor
        .check_collector_service(&permit, now(11801), &mut || sample(11800))
        .is_err());
    assert!(f
        .actor
        .check_collector_service(&permit, now(87400), &mut || sample(87400))
        .is_err());
    assert_eq!(LEASE_TTL, 300); // Existing interactive domain/window remains separate.
}
#[test]
fn service_24h_and_fixed_resource_caps_are_statement_invariants() {
    let f = Fixture::new();
    let mut policy = f.policy.body.clone();
    policy.expires_at = time(87401);
    assert!(policy.validate().is_err());
    policy = f.policy.body.clone();
    policy.quota.max_buffer_bytes += 1;
    assert!(policy.validate().is_err());
    policy = f.policy.body.clone();
    policy.quota.max_starts += 1;
    assert!(policy.validate().is_err());
}
#[test]
fn service_original_full_manifest_and_node_epoch_cannot_be_substituted() {
    let mut f = Fixture::new();
    let proof = f.proof();
    let mut changed = f.policy.body.clone();
    changed.manifest_sha256 = Hash::from_bytes([8; 32]);
    assert_eq!(changed.validate(), Err(Error::WrongClaim));
    changed.collector_id = collector_service_resource(changed.manifest_sha256).unwrap();
    let changed = sign(changed, &f.owner);
    assert_eq!(
        f.actor
            .admit_collector_service(changed, proof, now(1000), &mut || sample(1000)),
        Err(Error::WrongClaim)
    );
    let mut changed = f.policy.body.clone();
    changed.key_epoch = 2;
    let changed = sign(changed, &f.owner);
    assert!(f
        .actor
        .admit_collector_service(changed, f.proof(), now(1000), &mut || sample(1000))
        .is_err());
}
#[test]
fn service_start_cas_failure_does_not_return_a_permit_or_debit_counter() {
    let mut f = Fixture::new();
    f.admit();
    f.memory.0.lock().unwrap().2 = true;
    assert!(f.start(3, 1000).is_err());
    f.memory.0.lock().unwrap().2 = false;
    let permit = f.start(3, 1000).unwrap();
    assert_eq!(permit.start_count, 1);
}
#[test]
fn service_restart_requires_original_end_and_retains_absolute_expiry_and_start_count() {
    let mut f = Fixture::new();
    f.admit();
    let first = f.start(3, 1000).unwrap();
    f.actor = Actor::new(f.memory.clone()).unwrap();
    assert!(f.start(4, 1001).is_err());
    f.ended(&first, 1001);
    f.actor = Actor::new(f.memory.clone()).unwrap();
    let next = f.start(4, 1002).unwrap();
    assert_eq!(next.start_count, 2);
    assert_eq!(
        f.actor
            .check_collector_service(&next, now(1002), &mut || sample(1002))
            .unwrap()
            .expires_at,
        time(87400)
    );
    assert!(f
        .actor
        .check_collector_service(&first, now(1002), &mut || sample(1002))
        .is_err());
}
#[test]
fn service_actual_signed_revoke_persists_floor_and_does_not_release_unjoined_slot() {
    let mut f = Fixture::new();
    f.admit();
    let permit = f.start(3, 1000).unwrap();
    let revoke = sign(
        CollectorServiceRevocation {
            header: f.policy.body.header.clone(),
            service_id: f.policy.body.service_id,
            delegation_hash: f.policy.body.digest().unwrap(),
            policy_revision: 1,
            revocation_epoch: 2,
            issued_at: time(1001),
        },
        &f.owner,
    );
    f.actor
        .revoke_collector_service(revoke.clone(), now(1001))
        .unwrap();
    assert!(f
        .actor
        .check_collector_service(&permit, now(1001), &mut || sample(1001))
        .is_err());
    assert!(f
        .restored()
        .check_collector_service(&permit, now(1001), &mut || sample(1001))
        .is_err());
    assert_eq!(
        f.actor.revoke_collector_service(revoke, now(1001)),
        Err(Error::Replay)
    );
    let state = f.actor.snapshot().unwrap();
    let record = state.collector_services.get(&permit.service_id).unwrap();
    assert_eq!(record.revocation_floor, 2);
    assert_eq!(record.active, Some(permit.start));
}
#[test]
fn service_start_quota_is_durable_and_not_reset_by_signed_journal_replay() {
    let mut f = Fixture::new();
    f.admit();
    for n in 3..35 {
        let permit = f.start(n, 1000).unwrap();
        f.ended(&permit, 1000);
    }
    assert!(f.start(35, 1000).is_err());
    let mut restored = f.restored();
    assert!(restored
        .reserve_collector_service_start(
            f.policy.body.service_id,
            Nonce::from_bytes([35; 16]),
            now(1000),
            &mut || sample(1000)
        )
        .is_err());
}

#[test]
fn service_node_signature_covers_original_boot_and_boottime_deadline() {
    let f = Fixture::new();
    let mut proof = f.proof();
    proof.verify(public(&f.node)).unwrap();
    proof.body.clock.deadline_boottime_ns -= 1;
    assert_eq!(proof.verify(public(&f.node)), Err(Error::InvalidSignature));
}

#[test]
fn service_os_boot_change_is_explicit_refusal_without_new_start_debit() {
    let mut f = Fixture::new();
    f.admit();
    let other = ResourceId::new("88888888-8888-4888-8888-888888888888").unwrap();
    let result = f.actor.reserve_collector_service_start(
        f.policy.body.service_id,
        Nonce::from_bytes([3; 16]),
        now(1001),
        &mut || CollectorServiceClockObservation::journal(other, 2_000_000_000, time(1001)),
    );
    assert!(matches!(result, Err(Error::WrongState)));
    let record = f
        .actor
        .snapshot()
        .unwrap()
        .collector_services
        .get(&f.policy.body.service_id)
        .unwrap()
        .clone();
    assert_eq!(record.starts, 0);
    assert_eq!(record.active, None);
}

#[test]
fn service_daemon_restart_and_wall_rollback_never_derive_a_new_boottime_window() {
    let mut f = Fixture::new();
    f.admit();
    let permit = f.start(3, 1000).unwrap();
    f.ended(&permit, 1000);
    f.actor = Actor::new(f.memory.clone()).unwrap();
    // Wall clock appears only1s later, but the SAME boot has actually passed
    // the signed original24h deadline. A fresh daemon cannot reset that clock.
    let result = f.actor.reserve_collector_service_start(
        f.policy.body.service_id,
        Nonce::from_bytes([4; 16]),
        now(1001),
        &mut || CollectorServiceClockObservation::journal(boot(), 86401_000_000_001, time(1001)),
    );
    assert!(matches!(result, Err(Error::Expired)));
    let record = f
        .actor
        .snapshot()
        .unwrap()
        .collector_services
        .get(&f.policy.body.service_id)
        .unwrap()
        .clone();
    assert_eq!(record.clock.deadline_boottime_ns, 86401_000_000_000);
    assert_eq!(record.starts, 1);
}

#[test]
fn service_deadline_passing_during_start_cas_returns_no_permit_and_retains_unknown_slot() {
    let mut f = Fixture::new();
    f.admit();
    let mut calls = 0;
    let result = f.actor.reserve_collector_service_start(
        f.policy.body.service_id,
        Nonce::from_bytes([3; 16]),
        now(1000),
        &mut || {
            calls += 1;
            if calls == 1 {
                sample(1000)
            } else {
                CollectorServiceClockObservation::journal(boot(), 86401_000_000_001, time(1001))
            }
        },
    );
    assert!(matches!(result, Err(Error::Expired)));
    let record = f
        .actor
        .snapshot()
        .unwrap()
        .collector_services
        .get(&f.policy.body.service_id)
        .unwrap()
        .clone();
    assert_eq!(record.starts, 1);
    assert!(record.active.is_some());
}

#[test]
fn service_credential_scope_narrows_only_original_boot_deadline() {
    let mut f = Fixture::new();
    f.admit();
    let permit = f.start(3, 1000).unwrap();
    let mut access = f
        .actor
        .borrow_collector_service(&permit, now(1001), &mut || sample(1001))
        .unwrap();
    assert_eq!(access.deadline_boottime_ns(), 86401_000_000_000);
    access.narrow_expiry(time(83800)).unwrap();
    assert_eq!(access.deadline_boottime_ns(), 82801_000_000_000);
    assert!(access.narrow_expiry(time(83801)).is_err());
    access.recheck(now(1001), &mut || sample(1001)).unwrap();
    // Wall rollback does not reset the already narrowed original BOOTTIME end.
    assert!(access
        .recheck(
            now(1001),
            &mut || CollectorServiceClockObservation::journal(
                boot(),
                82801_000_000_000,
                time(1001)
            )
        )
        .is_err());
}

#[test]
fn service_credential_checks_advance_time_without_rebasing_original_end() {
    let mut f = Fixture::new();
    f.admit();
    let permit = f.start(3, 1000).unwrap();
    let mut access = f
        .actor
        .borrow_collector_service(&permit, now(1001), &mut || sample(1001))
        .unwrap();
    access.narrow_expiry(time(1020)).unwrap();
    let original = access.deadline_boottime_ns();
    // The retained writer's original admission lower bound remains 1001.
    // Every real write runs checks both before AND after its filesystem I/O.
    for second in [1002, 1004, 1007, 1010] {
        let actual = access.recheck(now(1001), &mut || sample(second)).unwrap();
        assert_eq!(actual.0, time(second));
        assert_eq!(access.deadline_boottime_ns(), original);
    }
    assert!(access.recheck(now(1001), &mut || sample(1009)).is_err());
    assert!(access.recheck(now(1001), &mut || sample(1020)).is_err());
    assert_eq!(access.deadline_boottime_ns(), original);
}

#[test]
fn service_required_credential_set_is_covered_by_actual_owner_and_node_signatures() {
    let mut f = Fixture::new();
    let proof = f.proof();
    let old_hash = f.policy.body.digest().unwrap();
    f.policy.body.credential_targets = vec![CollectorCredentialTarget::Receiver {}];
    assert_eq!(
        f.policy.verify(public(&f.owner)),
        Err(Error::InvalidSignature)
    );
    f.policy = sign(f.policy.body.clone(), &f.owner);
    assert_ne!(f.policy.body.digest().unwrap(), old_hash);
    assert!(f
        .actor
        .admit_collector_service(f.policy.clone(), proof, now(1000), &mut || sample(1000))
        .is_err());
    f.admit();
    assert_eq!(
        f.actor
            .snapshot()
            .unwrap()
            .collector_services
            .get(&f.policy.body.service_id)
            .unwrap()
            .policy
            .body
            .credential_targets,
        vec![CollectorCredentialTarget::Receiver {}]
    );
}
#[test]
fn service_required_targets_are_closed_one_exporter_one_receiver_and_match_actual_catalog() {
    let f = Fixture::new();
    let mut p = f.policy.body;
    p.credential_targets = vec![
        CollectorCredentialTarget::Scrape {
            target_id: "app".into(),
        },
        CollectorCredentialTarget::Receiver {},
    ];
    let catalog = serde_json::json!({"targets":[{"id":"app","url":"http://127.0.0.1:9100/metrics"}],"receiver":{"url":"http://127.0.0.1:9090/api/v1/write"}});
    p.check_credential_targets(&catalog).unwrap();
    let mut missing = catalog.clone();
    missing["targets"][0]["id"] = serde_json::json!("other");
    assert_eq!(p.check_credential_targets(&missing), Err(Error::WrongClaim));
    p.credential_targets
        .push(CollectorCredentialTarget::Receiver {});
    assert!(p.validate().is_err());
    p.credential_targets = vec![
        CollectorCredentialTarget::Scrape {
            target_id: "app".into(),
        },
        CollectorCredentialTarget::Scrape {
            target_id: "other".into(),
        },
    ];
    assert!(p.validate().is_err());
}
#[test]
fn service_original_active_start_observation_is_readonly_and_retired_start_is_not_reused() {
    let mut f = Fixture::new();
    f.admit();
    let permit = f.start(3, 1000).unwrap();
    let before = f.actor.snapshot().unwrap().revision();
    let observed = f
        .actor
        .observe_active_collector_service(
            permit.service_id(),
            permit.delegation_hash(),
            now(1001),
            &mut || sample(1001),
        )
        .unwrap();
    assert_eq!(observed.original_start_nonce, permit.start_id());
    assert_eq!(f.actor.snapshot().unwrap().revision(), before);
    f.ended(&permit, 1001);
    assert!(f
        .actor
        .observe_active_collector_service(
            permit.service_id(),
            permit.delegation_hash(),
            now(1001),
            &mut || sample(1001)
        )
        .is_err());
    let next = f.start(4, 1001).unwrap();
    assert_ne!(next.start_id(), permit.start_id());
}

#[path="collector_service_lifetime_tests.rs"]
mod lifetime_tests;


#[test]
fn service_read_phase_fresh_after_clock_is_returned_and_rollback_is_denied() {
    let mut f = Fixture::new(); f.admit();
    let revision = f.actor.snapshot().unwrap().revision();
    let before_entries = f.memory.0.lock().unwrap().1.len();
    let mut calls = 0;
    let (state, observed) = f.actor.read_collector_service_waiting(now(1000),
        std::time::Instant::now() + std::time::Duration::from_secs(3), &mut || {
            calls += 1; sample(if calls == 1 { 1000 } else { 1001 })
        }).unwrap();
    assert_eq!(calls, 2); assert_eq!(observed.now().0, time(1001));
    assert_eq!(state.revision(), revision);
    assert_eq!(f.memory.0.lock().unwrap().1.len(), before_entries);
    assert!(matches!(f.actor.read_collector_service_waiting(now(1001),
        std::time::Instant::now() + std::time::Duration::from_secs(3), &mut || sample(1000)), Err(Error::InvalidTime)));
}
#[test]
fn service_read_phase_after_clock_wall_boot_and_boottime_rollback_fail_closed() {
    for kind in 0..3 {
        let mut f = Fixture::new(); f.admit();
        let revision = f.actor.snapshot().unwrap().revision();
        let before_entries = f.memory.0.lock().unwrap().1.len();
        let mut calls = 0;
        let result = f.actor.read_collector_service_waiting(now(1000),
            std::time::Instant::now() + std::time::Duration::from_secs(3), &mut || {
                calls += 1;
                if calls == 1 { return CollectorServiceClockObservation::journal(boot(), 20, time(1001)); }
                match kind {
                    0 => CollectorServiceClockObservation::journal(boot(), 21, time(1000)),
                    1 => CollectorServiceClockObservation::journal(
                        ResourceId::new("aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa").unwrap(), 21, time(1001)),
                    _ => CollectorServiceClockObservation::journal(boot(), 19, time(1001)),
                }
            });
        assert!(matches!(result, Err(Error::InvalidTime))); assert_eq!(calls, 2);
        assert_eq!(f.actor.snapshot().unwrap().revision(), revision);
        assert_eq!(f.memory.0.lock().unwrap().1.len(), before_entries);
    }
}
#[test]
fn service_next_read_phase_never_retries_semantic_occupied_slot_uncertain_commit() {
    let mut f = Fixture::new(); f.admit(); let permit = f.start(3, 1000).unwrap();
    let before = f.actor.snapshot().unwrap();
    let entries = f.memory.0.lock().unwrap().1.len();
    let mut calls = 0;
    let started = std::time::Instant::now();
    let result = f.actor.next_collector_service_waiting(f.policy.body.agent_id,
        f.policy.body.manifest_sha256, now(1000), started + std::time::Duration::from_secs(3), &mut || {
            calls += 1; sample(1000)
        });
    assert!(matches!(result, Err(Error::UncertainCommit)));
    assert_eq!(calls, 2); assert!(started.elapsed() < std::time::Duration::from_secs(1));
    let after = f.actor.snapshot().unwrap();
    assert_eq!(after.revision(), before.revision());
    let record = after.collector_services.get(&permit.service_id).unwrap();
    assert_eq!(record.active, Some(permit.start)); assert_eq!(record.starts, 1);
    assert_eq!(f.memory.0.lock().unwrap().1.len(), entries);
}


#[test]
fn service_active_waiting_uses_fresh_verified_read_without_mutation_or_clock_rebase() {
    let mut f = Fixture::new();
    f.admit();
    let permit = f.start(3, 1000).unwrap();
    let revision = f.actor.snapshot().unwrap().revision();
    let entries = f.memory.0.lock().unwrap().1.len();
    let mut calls = 0;
    let observed = f.actor.observe_active_collector_service_waiting(
        permit.service_id(), permit.delegation_hash(), now(1000),
        std::time::Instant::now() + std::time::Duration::from_secs(3), &mut || {
            calls += 1;
            sample(if calls == 1 { 1000 } else { 1001 })
        },
    ).unwrap();
    assert_eq!(calls, 2);
    assert_eq!(observed.original_start_nonce, permit.start_id());
    assert_eq!(observed.stored.policy.body.digest().unwrap(), permit.delegation_hash());
    assert_eq!(observed.stored.actor_revision, revision);
    assert_eq!(observed.stored.revocation_floor, 1);
    assert_eq!(f.actor.snapshot().unwrap().revision(), revision);
    assert_eq!(f.memory.0.lock().unwrap().1.len(), entries);
    // The AFTER-read observation advances the existing wall highwater. A fresh
    // caller cannot fall back to the otherwise valid BEFORE-read observation.
    assert!(matches!(f.actor.observe_active_collector_service(
        permit.service_id(), permit.delegation_hash(), now(1000), &mut || sample(1000),
    ), Err(Error::InvalidTime)));
}

#[test]
fn service_active_waiting_wrong_hash_is_semantic_denial_without_retry_or_write() {
    let mut f = Fixture::new();
    f.admit();
    let permit = f.start(3, 1000).unwrap();
    let revision = f.actor.snapshot().unwrap().revision();
    let entries = f.memory.0.lock().unwrap().1.len();
    let mut calls = 0;
    let started = std::time::Instant::now();
    let result = f.actor.observe_active_collector_service_waiting(
        permit.service_id(), Hash::from_bytes([77; 32]), now(1001),
        started + std::time::Duration::from_secs(3), &mut || {
            calls += 1;
            sample(1001)
        },
    );
    assert!(matches!(result, Err(Error::WrongClaim)));
    assert_eq!(calls, 2);
    assert!(started.elapsed() < std::time::Duration::from_secs(1));
    let after = f.actor.snapshot().unwrap();
    assert_eq!(after.revision(), revision);
    let record = after.collector_services.get(&permit.service_id()).unwrap();
    assert_eq!(record.active, Some(permit.start));
    assert_eq!(record.starts, 1);
    assert_eq!(f.memory.0.lock().unwrap().1.len(), entries);
}

#[test]
fn service_active_waiting_signed_revoke_is_immediate_and_retains_original_slot() {
    let mut f = Fixture::new();
    f.admit();
    let permit = f.start(3, 1000).unwrap();
    let revoke = sign(CollectorServiceRevocation {
        header: f.policy.body.header.clone(),
        service_id: f.policy.body.service_id,
        delegation_hash: f.policy.body.digest().unwrap(),
        policy_revision: 1,
        revocation_epoch: 2,
        issued_at: time(1001),
    }, &f.owner);
    f.actor.revoke_collector_service(revoke, now(1001)).unwrap();
    let revision = f.actor.snapshot().unwrap().revision();
    let entries = f.memory.0.lock().unwrap().1.len();
    let mut calls = 0;
    let started = std::time::Instant::now();
    let result = f.actor.observe_active_collector_service_waiting(
        permit.service_id(), permit.delegation_hash(), now(1001),
        started + std::time::Duration::from_secs(3), &mut || {
            calls += 1;
            sample(1001)
        },
    );
    assert!(matches!(result, Err(Error::Revoked)));
    assert_eq!(calls, 2);
    assert!(started.elapsed() < std::time::Duration::from_secs(1));
    let after = f.actor.snapshot().unwrap();
    assert_eq!(after.revision(), revision);
    let record = after.collector_services.get(&permit.service_id()).unwrap();
    assert_eq!(record.revocation_floor, 2);
    assert_eq!(record.active, Some(permit.start));
    assert_eq!(record.starts, 1);
    assert_eq!(f.memory.0.lock().unwrap().1.len(), entries);
}

#[test]
fn service_active_waiting_absent_or_retired_start_is_never_replaced_or_retried() {
    for kind in 0..3 {
        let mut f = Fixture::new();
        f.admit();
        if kind == 1 {
            let permit = f.start(3, 1000).unwrap();
            f.ended(&permit, 1001);
        }
        let service = if kind == 2 {
            ResourceId::new("aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa").unwrap()
        } else {
            f.policy.body.service_id
        };
        let revision = f.actor.snapshot().unwrap().revision();
        let entries = f.memory.0.lock().unwrap().1.len();
        let mut calls = 0;
        let started = std::time::Instant::now();
        let result = f.actor.observe_active_collector_service_waiting(
            service, f.policy.body.digest().unwrap(), now(1001),
            started + std::time::Duration::from_secs(3), &mut || {
                calls += 1;
                sample(1001)
            },
        );
        if kind == 2 {
            assert!(matches!(result, Err(Error::NotFound)));
        } else {
            assert!(matches!(result, Err(Error::WrongState)));
        }
        assert_eq!(calls, 2);
        assert!(started.elapsed() < std::time::Duration::from_secs(1));
        let after = f.actor.snapshot().unwrap();
        assert_eq!(after.revision(), revision);
        let record = after.collector_services.get(&f.policy.body.service_id).unwrap();
        assert_eq!(record.active, None);
        assert_eq!(record.starts, if kind == 1 { 1 } else { 0 });
        assert_eq!(f.memory.0.lock().unwrap().1.len(), entries);
    }
}
