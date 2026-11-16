use super::*;
use ed25519_dalek::{Signer, SigningKey};
use std::sync::{Arc, Mutex};

// Synthetic test-only authority keys. No production key import/export.
fn key(n: u8) -> SigningKey {
    SigningKey::from_bytes(&[n; 32])
}
fn public(k: &SigningKey) -> PublicKey {
    PublicKey::from_bytes(k.verifying_key().to_bytes())
}
fn sign<T: Statement>(body: T, n: u8) -> Signed<T> {
    let signature =
        SignatureBytes::from_bytes(key(n).sign(&body.signing_bytes().unwrap()).to_bytes());
    Signed { body, signature }
}
fn time(n: u64) -> Timestamp {
    Timestamp::new(n).unwrap()
}
fn now() -> Now {
    Now(time(1000))
}
fn agent() -> AgentId {
    AgentId::new("44444444-4444-4444-8444-444444444444").unwrap()
}
fn operation() -> Operation {
    Operation::TelemetryRead {
        source: ResourceId::new("55555555-5555-4555-8555-555555555555").unwrap(),
    }
}
fn context() -> Context {
    Context {
        tenant: Tenant::LocalProfile {
            id: LocalProfileId::new("11111111-1111-4111-8111-111111111111").unwrap(),
        },
        deployment_id: DeploymentId::new("22222222-2222-4222-8222-222222222222").unwrap(),
    }
}
fn header(checkpoint: u64) -> Header {
    Header {
        version: 1,
        context: context(),
        owner_epoch: 1,
        checkpoint,
    }
}
#[derive(Clone)]
struct Memory(Arc<Mutex<State>>);
impl CompareAndSwap for Memory {
    fn load(&self) -> Result<State> {
        Ok(self.0.lock().unwrap().clone())
    }
    fn compare_and_swap(&mut self, expected: u64, next: State) -> Result<()> {
        let mut state = self.0.lock().unwrap();
        if state.revision() != expected {
            return Err(Error::Conflict);
        }
        *state = next;
        Ok(())
    }
}
struct Fixture {
    store: Memory,
    cert: Certificate,
}
impl Fixture {
    fn new() -> Self {
        let state = State::bootstrap(
            OwnerPin {
                context: context(),
                key: public(&key(11)),
                epoch: 1,
                checkpoint: 0,
                expires_at: time(4600),
            },
            now(),
        )
        .unwrap();
        let store = Memory(Arc::new(Mutex::new(state)));
        let mut actor = Actor::new(store.clone()).unwrap();
        let cert = enroll(&mut actor);
        Self { store, cert }
    }
    fn actor(&self) -> Actor<Memory> {
        Actor::new(self.store.clone()).unwrap()
    }
    fn request(&self, n: u8) -> (Signed<Lease>, Signed<AgentRequest>, Signed<Freshness>) {
        let lease = Lease {
            header: header(2),
            id: LeaseId::new(&format!("66666666-6666-4666-8666-{n:012x}")).unwrap(),
            agent_id: agent(),
            key_epoch: 1,
            certificate_hash: self.cert.digest().unwrap(),
            operation: operation(),
            issued_at: time(1000),
            expires_at: time(1120),
        };
        let proof = AgentRequest {
            header: header(2),
            agent_id: agent(),
            key_epoch: 1,
            lease_hash: lease.digest().unwrap(),
            nonce: Nonce::from_bytes([n; 16]),
            issued_at: time(1000),
        };
        (
            sign(lease, 11),
            sign(proof, 22),
            sign(
                Freshness {
                    header: header(2),
                    issued_at: time(1000),
                    expires_at: time(1300),
                },
                11,
            ),
        )
    }
    fn custody(&self) -> Custody<Memory> {
        Custody::new(self.actor())
    }
    fn begin(&self, custody: &mut Custody<Memory>, n: u8) -> Result<Begun> {
        script(custody, &[(1000.0, 0.0), (1000.0, 0.0), (1000.0, 0.0)]);
        custody.entropy = Some(Ok([n; 32]));
        let (lease, proof, fresh) = self.request(n);
        custody.begin(agent(), operation(), lease, proof, fresh)
    }
    fn revoke(&self) {
        self.actor()
            .revoke(
                sign(
                    Revocation {
                        header: header(3),
                        agent_id: agent(),
                        key_epoch: 1,
                        certificate_hash: self.cert.digest().unwrap(),
                        issued_at: time(1000),
                    },
                    11,
                ),
                now(),
            )
            .unwrap();
    }
}
fn enroll<S: CompareAndSwap>(actor: &mut Actor<S>) -> Certificate {
    let token = InvitationToken::from_bytes([33; 32]).unwrap();
    let offer = Offer {
        header: header(1),
        invitation_id: InvitationId::new("33333333-3333-4333-8333-333333333333").unwrap(),
        agent_id: agent(),
        agent_key: public(&key(22)),
        token_hash: token.digest(),
        scopes: Scopes(vec![Scope::TelemetryRead]),
        issued_at: time(1000),
        expires_at: time(1600),
    };
    actor.offer(sign(offer.clone(), 11), now()).unwrap();
    let claim = Claim {
        header: header(1),
        invitation_id: offer.invitation_id,
        offer_hash: offer.digest().unwrap(),
        agent_id: agent(),
        agent_key: offer.agent_key,
        capabilities: Capabilities {
            version: 1,
            scopes: Scopes(vec![Scope::TelemetryRead]),
        },
        nonce: Nonce::from_bytes([1; 16]),
        issued_at: time(1000),
    };
    actor
        .consume(&token, sign(claim.clone(), 22), now())
        .unwrap();
    let cert = Certificate {
        header: header(2),
        agent_id: agent(),
        key_epoch: 1,
        agent_key: offer.agent_key,
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
    actor.approve(sign(cert.clone(), 11), now()).unwrap();
    cert
}
fn sample(wall: f64, monotonic: f64) -> Sample {
    Sample {
        wall: Duration::from_secs_f64(wall),
        monotonic: Duration::from_secs_f64(monotonic),
    }
}
fn script<S: CompareAndSwap>(custody: &mut Custody<S>, observations: &[(f64, f64)]) {
    custody.clock.scripted = observations
        .iter()
        .map(|&(wall, mono)| Ok(sample(wall, mono)))
        .collect();
}
fn check(custody: &mut Custody<Memory>, handle: &Handle) -> Result<()> {
    script(custody, &[(1000.0, 1.0), (1000.0, 1.0)]);
    custody.check(handle, agent(), &operation())
}
fn finalize(custody: &mut Custody<Memory>, handle: &Handle) -> Result<()> {
    script(custody, &[(1000.0, 2.0), (1000.0, 2.0)]);
    custody.finalize(handle, agent(), &operation())
}

#[test]
fn actual_permit_one_shot_and_worker_loss_leave_consumed_proof() {
    let fixture = Fixture::new();
    let mut custody = fixture.custody();
    let begun = fixture.begin(&mut custody, 2).unwrap();
    check(&mut custody, &begun.handle).unwrap();
    finalize(&mut custody, &begun.handle).unwrap();
    assert_eq!(finalize(&mut custody, &begun.handle), Err(Error::NotFound));
    drop(custody);
    let mut restarted = fixture.custody();
    assert_eq!(check(&mut restarted, &begun.handle), Err(Error::NotFound));
    assert!(!restarted.clock_failed());
    // check(old handle) above already observed monotonic=1 in this process.
    // The ordinary fresh-process begin helper starts at0; using it here would
    // test an intentional clock rollback instead of durable proof consumption.
    script(
        &mut restarted,
        &[(1000.0, 1.0), (1000.0, 1.0), (1000.0, 1.0)],
    );
    restarted.entropy = Some(Ok([2; 32]));
    let (lease, proof, fresh) = fixture.request(2);
    match restarted.begin(agent(), operation(), lease, proof, fresh) {
        Err(error) => assert_eq!(error, Error::Replay, "consumed proof must remain rejected"),
        Ok(_) => panic!("consumed proof must not produce a new admission"),
    }
    assert!(!restarted.clock_failed());
}

#[test]
fn external_revoke_before_each_phase_is_live_not_cached_revision() {
    for during_query in [false, true] {
        let fixture = Fixture::new();
        let mut custody = fixture.custody();
        let begun = fixture.begin(&mut custody, 2).unwrap();
        if during_query {
            check(&mut custody, &begun.handle).unwrap();
        }
        fixture.revoke();
        let result = if during_query {
            finalize(&mut custody, &begun.handle)
        } else {
            check(&mut custody, &begun.handle)
        };
        assert_eq!(result, Err(Error::WrongCheckpoint));
        assert!(custody.entries.is_empty());
    }
}

#[test]
fn unrelated_admission_revision_does_not_invalidate_actual_permit() {
    let fixture = Fixture::new();
    let mut custody = fixture.custody();
    let begun = fixture.begin(&mut custody, 2).unwrap();
    let before = fixture.actor().snapshot().unwrap().revision();
    let (lease, proof, fresh) = fixture.request(3);
    let other = fixture
        .actor()
        .admit(lease, proof, Some(&fresh), &operation(), now())
        .unwrap();
    assert_eq!(fixture.actor().snapshot().unwrap().revision(), before + 1);
    drop(other);
    check(&mut custody, &begun.handle).unwrap();
    finalize(&mut custody, &begun.handle).unwrap();
}

#[test]
fn owner_and_agent_rotation_invalidate_pending_permits() {
    for owner_rotation in [false, true] {
        let fixture = Fixture::new();
        let mut custody = fixture.custody();
        let begun = fixture.begin(&mut custody, 2).unwrap();
        check(&mut custody, &begun.handle).unwrap();
        let mut external = fixture.actor();
        if owner_rotation {
            let change = OwnerRotation {
                header: header(3),
                new_epoch: 2,
                new_key: public(&key(44)),
                issued_at: time(1000),
                expires_at: time(4600),
            };
            external
                .rotate_owner(
                    sign(change.clone(), 11),
                    sign(NewOwnerProof(change), 44),
                    now(),
                )
                .unwrap();
            assert_eq!(
                finalize(&mut custody, &begun.handle),
                Err(Error::WrongEpoch)
            );
        } else {
            let mut cert = fixture.cert.clone();
            cert.header = header(3);
            cert.key_epoch = 2;
            cert.agent_key = public(&key(44));
            cert.origin = CertificateOrigin::Rotation {
                previous_certificate: fixture.cert.digest().unwrap(),
                previous_key_epoch: 1,
            };
            let proof = RotationProof {
                header: header(3),
                agent_id: agent(),
                certificate_hash: cert.digest().unwrap(),
                nonce: Nonce::from_bytes([99; 16]),
                issued_at: time(1000),
            };
            external
                .rotate(
                    sign(cert, 11),
                    sign(proof.clone(), 22),
                    sign(NewKeyProof(proof), 44),
                    now(),
                )
                .unwrap();
            assert_eq!(
                finalize(&mut custody, &begun.handle),
                Err(Error::WrongCheckpoint)
            );
        }
        assert!(custody.entries.is_empty());
    }
}

#[test]
fn eight_slots_reject_ninth_before_consumption_then_retire_releases_capacity() {
    let fixture = Fixture::new();
    let mut custody = fixture.custody();
    let mut handles = Vec::new();
    for n in 2..10 {
        handles.push(fixture.begin(&mut custody, n).unwrap().handle);
    }
    let revision = fixture.actor().snapshot().unwrap().revision();
    assert!(matches!(fixture.begin(&mut custody, 10), Err(Error::Quota)));
    assert_eq!(fixture.actor().snapshot().unwrap().revision(), revision);
    script(&mut custody, &[(1000.0, 0.0)]);
    custody.retire(&handles[0], agent(), &operation()).unwrap();
    script(&mut custody, &[(1000.0, 0.0)]);
    custody.retire(&handles[0], agent(), &operation()).unwrap();
    assert!(fixture.begin(&mut custody, 10).is_ok());
}

#[test]
fn zero_collision_and_entropy_error_do_not_consume_lease() {
    for entropy in [Ok([0; 32]), Err(Error::Entropy), Ok([2; 32])] {
        let fixture = Fixture::new();
        let mut custody = fixture.custody();
        let _existing = fixture.begin(&mut custody, 2).unwrap();
        let revision = fixture.actor().snapshot().unwrap().revision();
        script(&mut custody, &[(1000.0, 0.0)]);
        custody.entropy = Some(entropy);
        let (lease, proof, fresh) = fixture.request(3);
        assert!(matches!(
            custody.begin(agent(), operation(), lease, proof, fresh),
            Err(Error::Entropy)
        ));
        assert_eq!(fixture.actor().snapshot().unwrap().revision(), revision);
        assert!(fixture.begin(&mut custody, 3).is_ok());
    }
}

#[test]
fn bad_context_and_wrong_phase_retire_without_new_commit() {
    for wrong in [0, 1, 2] {
        let fixture = Fixture::new();
        let mut custody = fixture.custody();
        let begun = fixture.begin(&mut custody, 2).unwrap();
        let revision = fixture.actor().snapshot().unwrap().revision();
        script(&mut custody, &[(1000.0, 0.0)]);
        let result = match wrong {
            0 => custody.check(
                &begun.handle,
                AgentId::new("77777777-7777-4777-8777-777777777777").unwrap(),
                &operation(),
            ),
            1 => custody.check(
                &begun.handle,
                agent(),
                &Operation::TelemetryRead {
                    source: ResourceId::new("88888888-8888-4888-8888-888888888888").unwrap(),
                },
            ),
            _ => custody.finalize(&begun.handle, agent(), &operation()),
        };
        assert!(matches!(result, Err(Error::WrongClaim | Error::WrongState)));
        assert!(custody.entries.is_empty());
        assert_eq!(fixture.actor().snapshot().unwrap().revision(), revision);
    }
    let fixture = Fixture::new();
    let mut custody = fixture.custody();
    let begun = fixture.begin(&mut custody, 2).unwrap();
    check(&mut custody, &begun.handle).unwrap();
    script(&mut custody, &[(1000.0, 1.0)]);
    assert_eq!(
        custody.check(&begun.handle, agent(), &operation()),
        Err(Error::WrongState)
    );
    assert!(custody.entries.is_empty());
}

#[test]
fn wall_and_monotonic_post_io_failures_are_terminal() {
    for observations in [
        [(1000.0, 1.0), (1120.0, 1.0)],
        [(1000.5, 1.0), (1000.25, 1.0)],
        [(1000.0, 1.0), (1000.0, 5.0)],
        [(1000.0, 1.0), (1000.0, 0.5)],
    ] {
        let fixture = Fixture::new();
        let mut custody = fixture.custody();
        let begun = fixture.begin(&mut custody, 2).unwrap();
        script(&mut custody, &observations);
        assert!(matches!(
            custody.check(&begun.handle, agent(), &operation()),
            Err(Error::Expired | Error::InvalidTime)
        ));
        assert!(custody.entries.is_empty());
    }
}

#[test]
fn slow_admission_deadline_starts_before_commit_and_keeps_proof_spent() {
    let fixture = Fixture::new();
    let mut custody = fixture.custody();
    script(&mut custody, &[(1000.0, 0.0), (1000.0, 5.0), (1000.0, 5.0)]);
    custody.entropy = Some(Ok([2; 32]));
    let (lease, proof, fresh) = fixture.request(2);
    assert!(matches!(
        custody.begin(agent(), operation(), lease, proof, fresh),
        Err(Error::Expired)
    ));
    assert!(custody.entries.is_empty());
    let (lease, proof, fresh) = fixture.request(2);
    assert!(matches!(
        fixture
            .actor()
            .admit(lease, proof, Some(&fresh), &operation(), now()),
        Err(Error::Replay)
    ));
}

#[test]
fn known_post_admission_expiry_never_starts_another_actor_load() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    struct CountingStore {
        store: Memory,
        loads: Arc<AtomicUsize>,
    }
    impl CompareAndSwap for CountingStore {
        fn load(&self) -> Result<State> {
            self.loads.fetch_add(1, Ordering::SeqCst);
            self.store.load()
        }
        fn compare_and_swap(&mut self, expected: u64, next: State) -> Result<()> {
            self.store.compare_and_swap(expected, next)
        }
    }
    for post_admission in [(1000.0, 5.0), (1120.0, 1.0)] {
        let fixture = Fixture::new();
        let loads = Arc::new(AtomicUsize::new(0));
        let actor = Actor::new(CountingStore {
            store: fixture.store.clone(),
            loads: Arc::clone(&loads),
        })
        .unwrap();
        let initial = loads.load(Ordering::SeqCst);
        let mut custody = Custody::new(actor);
        script(&mut custody, &[(1000.0, 0.0), post_admission]);
        custody.entropy = Some(Ok([2; 32]));
        let (lease, proof, fresh) = fixture.request(2);
        assert!(matches!(
            custody.begin(agent(), operation(), lease, proof, fresh),
            Err(Error::Expired)
        ));
        assert_eq!(
            loads.load(Ordering::SeqCst),
            initial + 1,
            "expired post-admission observation must not start check_permit store I/O"
        );
        assert!(custody.entries.is_empty());
        // The successful admission commit still burns its one-use proof.
        let (lease, proof, fresh) = fixture.request(2);
        assert!(matches!(
            fixture
                .actor()
                .admit(lease, proof, Some(&fresh), &operation(), now()),
            Err(Error::Replay)
        ));
    }
}

#[test]
fn invalid_context_before_begin_does_not_consume_and_handle_parser_is_closed() {
    let fixture = Fixture::new();
    let mut custody = fixture.custody();
    let revision = fixture.actor().snapshot().unwrap().revision();
    let (lease, proof, fresh) = fixture.request(2);
    assert!(matches!(
        custody.begin(
            AgentId::new("77777777-7777-4777-8777-777777777777").unwrap(),
            operation(),
            lease,
            proof,
            fresh
        ),
        Err(Error::WrongClaim)
    ));
    assert_eq!(fixture.actor().snapshot().unwrap().revision(), revision);
    for text in [
        "".to_owned(),
        "00".repeat(32),
        "FF".repeat(32),
        "a".repeat(63),
        "g".repeat(64),
    ] {
        assert!(Handle::parse(&text).is_err());
    }
    assert!(Handle::parse(&"01".repeat(32)).is_ok());
}

#[test]
fn malformed_signed_data_and_purpose_cross_signature_do_not_consume_or_poison_custody() {
    let fixture = Fixture::new();
    let mut custody = fixture.custody();
    let revision = fixture.actor().snapshot().unwrap().revision();
    let malformed = serde_json::json!({"action":"begin","agentId":agent(),"operation":operation(),
        "lease":{},"proof":{},"freshness":{}});
    let mut raw = serde_json::to_vec(&malformed).unwrap();
    raw.push(b'\n');
    let command = super::super::decode(&raw).ok().unwrap();
    assert!(matches!(
        super::super::execute(&mut custody, command),
        Err(Error::InvalidWire)
    ));
    let (mut lease, proof, fresh) = fixture.request(2);
    lease.signature = proof.signature;
    script(&mut custody, &[(1000.0, 0.0)]);
    custody.entropy = Some(Ok([2; 32]));
    assert!(matches!(
        custody.begin(agent(), operation(), lease, proof, fresh),
        Err(Error::InvalidSignature)
    ));
    assert_eq!(fixture.actor().snapshot().unwrap().revision(), revision);
    assert!(custody.entries.is_empty());
    assert!(fixture.begin(&mut custody, 2).is_ok());
}

#[test]
fn signed_input_invalid_time_is_nonfatal_and_preserves_another_pending_permit() {
    for issued in [939, 1001, 253_402_300_800_u64] {
        let fixture = Fixture::new();
        let mut custody = fixture.custody();
        let pending_a = fixture.begin(&mut custody, 2).unwrap();
        let before = fixture.actor().snapshot().unwrap().revision();
        let (lease, mut proof, fresh) = fixture.request(3);
        // Modify untrusted body without re-signing. Actor's existing recent()
        // check rejects these before proof.verify, but this is not an OS fault.
        proof.body.issued_at = serde_json::from_value(serde_json::json!(issued)).unwrap();
        script(&mut custody, &[(1000.0, 1.0)]);
        custody.entropy = Some(Ok([3; 32]));
        let result = custody.begin(agent(), operation(), lease, proof, fresh);
        assert!(matches!(result, Err(Error::InvalidTime)));
        assert!(!custody.clock_failed());
        assert!(!super::super::closes_worker(&custody, Error::InvalidTime));
        assert_eq!(fixture.actor().snapshot().unwrap().revision(), before);
        assert_eq!(custody.entries.len(), 1);
        check(&mut custody, &pending_a.handle).unwrap();
        finalize(&mut custody, &pending_a.handle).unwrap();
        // Independent explicit valid control proves bad B did not consume its
        // lease/nonce; the worker never retries/re-signs the rejected input.
        script(&mut custody, &[(1000.0, 3.0), (1000.0, 3.0), (1000.0, 3.0)]);
        custody.entropy = Some(Ok([3; 32]));
        let (lease, proof, fresh) = fixture.request(3);
        assert!(custody
            .begin(agent(), operation(), lease, proof, fresh)
            .is_ok());
    }
}

#[test]
fn actual_clock_failure_is_fatal_clears_pending_and_cannot_recover_in_process() {
    for failure in [
        Ok(sample(999.75, 1.0)),
        Ok(Sample {
            wall: Duration::from_secs(253_402_300_800),
            monotonic: Duration::from_secs(1),
        }),
        Err(Error::InvalidTime),
    ] {
        let fixture = Fixture::new();
        let mut custody = fixture.custody();
        let pending = fixture.begin(&mut custody, 2).unwrap();
        custody.clock.scripted = std::collections::VecDeque::from([failure]);
        assert_eq!(
            custody.check(&pending.handle, agent(), &operation()),
            Err(Error::InvalidTime)
        );
        assert!(custody.clock_failed());
        assert!(super::super::closes_worker(&custody, Error::InvalidTime));
        assert!(custody.entries.is_empty());
        script(&mut custody, &[(1000.0, 1.0)]);
        assert_eq!(
            custody.retire(&pending.handle, agent(), &operation()),
            Err(Error::InvalidTime)
        );
    }
}

struct SqliteTemp(std::path::PathBuf);
impl SqliteTemp {
    fn new() -> Self {
        use std::os::unix::fs::DirBuilderExt;
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "cc-custody-sqlite-{}-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, std::sync::atomic::Ordering::SeqCst),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::DirBuilder::new()
            .mode(0o700)
            .create(&path)
            .unwrap();
        std::fs::write(path.join("owned.marker"), b"cc-custody-sqlite-unit-v1").unwrap();
        Self(path)
    }
    fn pin() -> OwnerPin {
        OwnerPin {
            context: context(),
            key: public(&key(11)),
            epoch: 1,
            checkpoint: 0,
            expires_at: time(4600),
        }
    }
    fn create(&self) -> Actor<SqliteJournalStore> {
        Actor::new(
            SqliteJournalStore::create(
                self.0.join("data"),
                self.0.join("anchor"),
                Self::pin(),
                now(),
                SqliteStorageBudget::default(),
            )
            .unwrap(),
        )
        .unwrap()
    }
    fn open(&self) -> Actor<SqliteJournalStore> {
        Actor::new(
            SqliteJournalStore::open(
                self.0.join("data"),
                self.0.join("anchor"),
                Self::pin(),
                SqliteStorageBudget::default(),
            )
            .unwrap(),
        )
        .unwrap()
    }
}
impl Drop for SqliteTemp {
    fn drop(&mut self) {
        let base = std::env::temp_dir();
        assert_eq!(self.0.parent(), Some(base.as_path()));
        assert!(self
            .0
            .file_name()
            .unwrap()
            .to_string_lossy()
            .starts_with("cc-custody-sqlite-"));
        assert!(!std::fs::symlink_metadata(&self.0)
            .unwrap()
            .file_type()
            .is_symlink());
        assert!(
            std::fs::read(self.0.join("owned.marker")).unwrap() == b"cc-custody-sqlite-unit-v1",
            "owned public test marker must match"
        );
        std::fs::remove_dir_all(&self.0).unwrap();
    }
}

#[test]
fn real_sqlite_suffix_revoke_and_restart_use_retained_permit_not_revision_inference() {
    let temp = SqliteTemp::new();
    let mut actor = temp.create();
    let cert = enroll(&mut actor);
    let fixture = Fixture::new();
    assert!(cert == fixture.cert, "synthetic authority setup must match");
    let mut custody = Custody::new(actor);
    script(&mut custody, &[(1000.0, 0.0), (1000.0, 0.0), (1000.0, 0.0)]);
    custody.entropy = Some(Ok([2; 32]));
    let (lease, proof, fresh) = fixture.request(2);
    let begun = custody
        .begin(agent(), operation(), lease, proof, fresh)
        .unwrap();
    script(&mut custody, &[(1000.0, 1.0), (1000.0, 1.0)]);
    custody.check(&begun.handle, agent(), &operation()).unwrap();
    script(&mut custody, &[(1000.0, 2.0), (1000.0, 2.0)]);
    custody
        .finalize(&begun.handle, agent(), &operation())
        .unwrap();
    drop(custody);
    // Full verified SQLite replay restores consumption under unchanged authority.
    let mut actor = temp.open();
    let (lease, proof, fresh) = fixture.request(2);
    assert!(matches!(
        actor.admit(lease, proof, Some(&fresh), &operation(), now()),
        Err(Error::Replay)
    ));
    let mut custody = Custody::new(actor);
    script(&mut custody, &[(1000.0, 0.0), (1000.0, 0.0), (1000.0, 0.0)]);
    custody.entropy = Some(Ok([3; 32]));
    let (lease, proof, fresh) = fixture.request(3);
    let begun = custody
        .begin(agent(), operation(), lease, proof, fresh)
        .unwrap();
    script(&mut custody, &[(1000.0, 1.0), (1000.0, 1.0)]);
    custody.check(&begun.handle, agent(), &operation()).unwrap();
    let mut external = temp.open();
    let before = external.snapshot().unwrap().capacity();
    assert_eq!(before.consumed_leases, 2);
    assert_eq!(before.consumed_nonces, 3); // enrollment claim plus two requests
    external
        .revoke(
            sign(
                Revocation {
                    header: header(3),
                    agent_id: agent(),
                    key_epoch: 1,
                    certificate_hash: cert.digest().unwrap(),
                    issued_at: time(1000),
                },
                11,
            ),
            now(),
        )
        .unwrap();
    script(&mut custody, &[(1000.0, 2.0)]);
    assert_eq!(
        custody.finalize(&begun.handle, agent(), &operation()),
        Err(Error::WrongCheckpoint)
    );
    assert!(custody.entries.is_empty());
    drop(external);
    drop(custody);
    assert!(temp.open().snapshot().unwrap().revoked(agent()));
    // Revoked denial is not used as evidence for Replay; unchanged-authority
    // replay and durable nonce/lease counts above supply that evidence.
}
