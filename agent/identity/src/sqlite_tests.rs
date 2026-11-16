use super::*;
use ed25519_dalek::{Signer, SigningKey};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

#[test]
fn owner_role_history_sqlite_restores_and_refreshes_retained_roles_after_revoke() {
    let temp = Temp::new();
    let (mut actor, cert) = enroll(temp.create());
    let change = OwnerRotation {
        header: header(3),
        new_epoch: 2,
        new_key: public(&key(55)),
        issued_at: time(1000),
        expires_at: time(4600),
    };
    let mut external = Actor::new(temp.open().unwrap()).unwrap();
    external
        .rotate_owner(
            sign(change.clone(), 11),
            sign(NewOwnerProof(change), 55),
            now(),
        )
        .unwrap();
    assert_eq!(
        actor
            .snapshot()
            .unwrap()
            .capacity()
            .retained_owner_key_tombstones,
        2
    );
    let offered = Offer {
        header: Header {
            owner_epoch: 2,
            checkpoint: 4,
            ..header(4)
        },
        invitation_id: InvitationId::new("99999999-9999-4999-8999-999999999999").unwrap(),
        agent_id: AgentId::new("99999999-9999-4999-8999-999999999999").unwrap(),
        agent_key: public(&key(11)),
        token_hash: InvitationToken::from_bytes([77; 32]).unwrap().digest(),
        scopes: Scopes(vec![Scope::TelemetryRead]),
        issued_at: time(1000),
        expires_at: time(1600),
    };
    let revision = actor.snapshot().unwrap().revision();
    assert_eq!(
        actor.offer(sign(offered.clone(), 55), now()),
        Err(Error::KeyReuse)
    );
    assert_eq!(actor.snapshot().unwrap().revision(), revision);
    external
        .revoke(
            sign(
                Revocation {
                    header: offered.header.clone(),
                    agent_id: id(),
                    key_epoch: 1,
                    certificate_hash: cert.digest().unwrap(),
                    issued_at: time(1000),
                },
                55,
            ),
            now(),
        )
        .unwrap();
    assert!(actor.snapshot().unwrap().revoked(id()));
    assert_eq!(
        actor
            .snapshot()
            .unwrap()
            .capacity()
            .retained_owner_key_tombstones,
        2
    );
    drop(external);
    drop(actor);
    let mut actor = Actor::new(temp.open().unwrap()).unwrap();
    let blocked = OwnerRotation {
        header: Header {
            owner_epoch: 2,
            checkpoint: 5,
            ..header(5)
        },
        new_epoch: 3,
        new_key: public(&key(22)),
        issued_at: time(1000),
        expires_at: time(4600),
    };
    let before = actor.snapshot().unwrap().revision();
    assert_eq!(
        actor.rotate_owner(
            sign(blocked.clone(), 55),
            sign(NewOwnerProof(blocked), 22),
            now()
        ),
        Err(Error::KeyReuse)
    );
    assert_eq!(actor.snapshot().unwrap().revision(), before);
    let fresh = OwnerRotation {
        header: Header {
            owner_epoch: 2,
            checkpoint: 5,
            ..header(5)
        },
        new_epoch: 3,
        new_key: public(&key(66)),
        issued_at: time(1000),
        expires_at: time(4600),
    };
    actor
        .rotate_owner(
            sign(fresh.clone(), 55),
            sign(NewOwnerProof(fresh), 66),
            now(),
        )
        .unwrap();
    assert_eq!(
        actor
            .snapshot()
            .unwrap()
            .capacity()
            .retained_owner_key_tombstones,
        3
    );
}

#[test]
fn owner_role_history_sqlite_legacy_valid_signatures_fail_closed_on_original_pin_replay() {
    use crate::durable::{Entry, Transition};
    for kind in ["agent-promotion", "owner-back", "former-owner-agent"] {
        let temp = Temp::new();
        let (mut actor, _) = enroll(temp.create());
        if kind != "agent-promotion" {
            let change = OwnerRotation {
                header: header(3),
                new_epoch: 2,
                new_key: public(&key(55)),
                issued_at: time(1000),
                expires_at: time(4600),
            };
            actor
                .rotate_owner(
                    sign(change.clone(), 11),
                    sign(NewOwnerProof(change), 55),
                    now(),
                )
                .unwrap();
        }
        let pin = actor.snapshot().unwrap().owner().clone();
        let h = Header {
            version: 1,
            context: pin.context.clone(),
            owner_epoch: pin.epoch,
            checkpoint: pin.checkpoint + 1,
        };
        let transition = if kind == "former-owner-agent" {
            let offer = sign(
                Offer {
                    header: h,
                    invitation_id: InvitationId::new("99999999-9999-4999-8999-999999999999")
                        .unwrap(),
                    agent_id: AgentId::new("99999999-9999-4999-8999-999999999999").unwrap(),
                    agent_key: public(&key(11)),
                    token_hash: InvitationToken::from_bytes([77; 32]).unwrap().digest(),
                    scopes: Scopes(vec![Scope::TelemetryRead]),
                    issued_at: time(1000),
                    expires_at: time(1600),
                },
                55,
            );
            offer.verify(pin.key).unwrap();
            Transition::Offer(offer)
        } else {
            let target = if kind == "agent-promotion" { 22 } else { 11 };
            let change = OwnerRotation {
                header: h,
                new_epoch: pin.epoch + 1,
                new_key: public(&key(target)),
                issued_at: time(1000),
                expires_at: time(4600),
            };
            let old = sign(change.clone(), if pin.epoch == 1 { 11 } else { 55 });
            let new = sign(NewOwnerProof(change), target);
            old.verify(pin.key).unwrap();
            new.verify(public(&key(target))).unwrap();
            Transition::RotateOwner { old, new }
        };
        drop(actor);
        // Authentic old-policy statements + a consistent journal/anchor, not an
        // unsigned cached State. Stricter replay must reject semantics, not just
        // a broken checksum, without migrating/resetting the on-disk fixture.
        let bytes = serde_json::to_vec(&Entry {
            at: time(1000),
            transition,
        })
        .unwrap();
        let conn = temp.raw();
        let head = read_head(&conn).unwrap();
        let next = Head {
            revision: head.revision + 1,
            digest: chained(head.digest, &bytes),
            logical_bytes: head.logical_bytes + bytes.len() as u64,
            pending: false,
        };
        conn.execute(
            "INSERT INTO identity_events VALUES (?1,?2,?3,?4)",
            params![
                next.revision as i64,
                head.digest.as_bytes().as_slice(),
                bytes,
                next.digest.as_bytes().as_slice()
            ],
        )
        .unwrap();
        conn.execute(
            "UPDATE identity_meta SET revision=?1,digest=?2,logical_bytes=?3 WHERE singleton=1",
            params![
                next.revision as i64,
                next.digest.as_bytes().as_slice(),
                next.logical_bytes as i64
            ],
        )
        .unwrap();
        write_anchor(&temp.0.join("anchor"), &next).unwrap();
        drop(conn);
        assert!(matches!(temp.open(), Err(Error::KeyReuse)), "{kind}");
    }
}

static NEXT: AtomicU64 = AtomicU64::new(0);
struct Temp(PathBuf);
impl Temp {
    fn new() -> Self {
        use std::os::unix::fs::DirBuilderExt;
        let path = std::env::temp_dir().join(format!(
            "cc-agent-sqlite-unit-{}-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::SeqCst),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::DirBuilder::new().mode(0o700).create(&path).unwrap();
        fs::write(
            path.join("owned-test.marker"),
            b"cc-agent-sqlite-owned-unit-v1",
        )
        .unwrap();
        Self(path)
    }
    fn create(&self) -> SqliteJournalStore {
        SqliteJournalStore::create(
            self.0.join("data"),
            self.0.join("anchor"),
            pin(),
            now(),
            SqliteStorageBudget::default(),
        )
        .unwrap()
    }
    fn open(&self) -> Result<SqliteJournalStore> {
        SqliteJournalStore::open(
            self.0.join("data"),
            self.0.join("anchor"),
            pin(),
            SqliteStorageBudget::default(),
        )
    }
    fn raw(&self) -> Connection {
        Connection::open(self.0.join("data").join(DB_NAME)).unwrap()
    }
}
impl Drop for Temp {
    fn drop(&mut self) {
        let base = std::env::temp_dir();
        assert_eq!(self.0.parent(), Some(base.as_path()));
        assert!(self
            .0
            .file_name()
            .unwrap()
            .to_string_lossy()
            .starts_with("cc-agent-sqlite-unit-"));
        assert!(!fs::symlink_metadata(&self.0)
            .unwrap()
            .file_type()
            .is_symlink());
        assert_eq!(
            fs::read(self.0.join("owned-test.marker")).unwrap(),
            b"cc-agent-sqlite-owned-unit-v1"
        );
        fs::remove_dir_all(&self.0).unwrap();
    }
}
fn time(v: u64) -> Timestamp {
    Timestamp::new(v).unwrap()
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
fn sign<T: Statement>(body: T, n: u8) -> Signed<T> {
    let signature =
        SignatureBytes::from_bytes(key(n).sign(&body.signing_bytes().unwrap()).to_bytes());
    Signed { body, signature }
}
fn id() -> AgentId {
    AgentId::new("44444444-4444-4444-8444-444444444444").unwrap()
}
fn pin() -> OwnerPin {
    OwnerPin {
        context: Context {
            tenant: Tenant::LocalProfile {
                id: LocalProfileId::new("11111111-1111-4111-8111-111111111111").unwrap(),
            },
            deployment_id: DeploymentId::new("22222222-2222-4222-8222-222222222222").unwrap(),
        },
        key: public(&key(11)),
        epoch: 1,
        checkpoint: 0,
        expires_at: time(10000),
    }
}
fn header(cp: u64) -> Header {
    Header {
        version: 1,
        context: pin().context,
        owner_epoch: 1,
        checkpoint: cp,
    }
}
fn offer() -> Signed<Offer> {
    sign(
        Offer {
            header: header(1),
            invitation_id: InvitationId::new("33333333-3333-4333-8333-333333333333").unwrap(),
            agent_id: id(),
            agent_key: public(&key(22)),
            token_hash: InvitationToken::from_bytes([33; 32]).unwrap().digest(),
            scopes: Scopes(vec![Scope::TelemetryRead]),
            issued_at: time(1000),
            expires_at: time(1600),
        },
        11,
    )
}
fn enroll(store: SqliteJournalStore) -> (Actor<SqliteJournalStore>, Certificate) {
    let mut actor = Actor::new(store).unwrap();
    let offered = offer();
    actor.offer(offered.clone(), now()).unwrap();
    let claim = Claim {
        header: header(1),
        invitation_id: offered.body.invitation_id,
        offer_hash: offered.body.digest().unwrap(),
        agent_id: id(),
        agent_key: offered.body.agent_key,
        capabilities: Capabilities {
            version: 1,
            scopes: Scopes(vec![Scope::TelemetryRead]),
        },
        nonce: Nonce::from_bytes([1; 16]),
        issued_at: time(1000),
    };
    actor
        .consume(
            &InvitationToken::from_bytes([33; 32]).unwrap(),
            sign(claim.clone(), 22),
            now(),
        )
        .unwrap();
    let cert = Certificate {
        header: header(2),
        agent_id: id(),
        key_epoch: 1,
        agent_key: offered.body.agent_key,
        scopes: Scopes(vec![Scope::TelemetryRead]),
        capabilities: claim.capabilities.clone(),
        origin: CertificateOrigin::Enrollment {
            invitation_id: offered.body.invitation_id,
            claim_hash: claim.digest().unwrap(),
            key_fingerprint: claim.agent_key.fingerprint(),
        },
        not_before: time(1000),
        expires_at: time(4600),
    };
    actor.approve(sign(cert.clone(), 11), now()).unwrap();
    (actor, cert)
}
fn request(
    cert: &Certificate,
    n: u8,
    cp: u64,
) -> (
    Signed<Lease>,
    Signed<AgentRequest>,
    Signed<Freshness>,
    Operation,
) {
    let operation = Operation::TelemetryRead {
        source: ResourceId::new("66666666-6666-4666-8666-666666666666").unwrap(),
    };
    let lease = Lease {
        header: header(cp),
        id: LeaseId::new(&format!("88888888-8888-4888-8888-{n:012x}")).unwrap(),
        agent_id: id(),
        key_epoch: 1,
        certificate_hash: cert.digest().unwrap(),
        operation: operation.clone(),
        issued_at: time(1000),
        expires_at: time(1120),
    };
    let proof = sign(
        AgentRequest {
            header: lease.header.clone(),
            agent_id: id(),
            key_epoch: 1,
            lease_hash: lease.digest().unwrap(),
            nonce: Nonce::from_bytes([n; 16]),
            issued_at: time(1000),
        },
        22,
    );
    (
        sign(lease, 11),
        proof,
        sign(
            Freshness {
                header: header(cp),
                issued_at: time(1000),
                expires_at: time(1300),
            },
            11,
        ),
        operation,
    )
}
#[test]
fn sqlite_signed_lifecycle_replay_revoke_and_clock_floor_survive_restart() {
    let temp = Temp::new();
    let (mut actor, cert) = enroll(temp.create());
    let (lease, proof, fresh, op) = request(&cert, 2, 2);
    let permit = actor
        .admit(
            lease.clone(),
            proof.clone(),
            Some(&fresh),
            &op,
            Now(time(1001)),
        )
        .unwrap();
    drop(actor);
    let mut actor = Actor::new(temp.open().unwrap()).unwrap();
    assert!(matches!(
        actor.check_permit(&permit, now()),
        Err(Error::InvalidTime)
    ));
    assert!(matches!(
        actor.admit(lease, proof, Some(&fresh), &op, Now(time(1001))),
        Err(Error::Replay)
    ));
    actor
        .revoke(
            sign(
                Revocation {
                    header: header(3),
                    agent_id: id(),
                    key_epoch: 1,
                    certificate_hash: cert.digest().unwrap(),
                    issued_at: time(1001),
                },
                11,
            ),
            Now(time(1001)),
        )
        .unwrap();
    drop(actor);
    let mut actor = Actor::new(temp.open().unwrap()).unwrap();
    let (lease, proof, fresh, op) = request(&cert, 3, 3);
    assert!(matches!(
        actor.admit(lease, proof, Some(&fresh), &op, Now(time(1001))),
        Err(Error::Revoked)
    ));
}
#[test]
fn sqlite_verified_suffix_refreshes_other_connection_and_old_tamper_waits_for_restart() {
    let temp = Temp::new();
    let store = temp.create();
    let other = temp.open().unwrap();
    let mut actor = Actor::new(store).unwrap();
    actor.offer(offer(), now()).unwrap();
    let state = other.load().unwrap();
    assert_eq!(
        state.invitation_status(offer().body.invitation_id),
        Some(InvitationStatus::Offered)
    );
    let stats = other.status().unwrap();
    assert_eq!(stats.startup_records_verified, 1);
    assert_eq!(stats.suffix_records_verified, 1);
    let raw = temp.raw();
    raw.execute(
        "UPDATE identity_events SET entry=?1 WHERE revision=0",
        params![b"{}".as_slice()],
    )
    .unwrap();
    drop(raw);
    // The in-process state remains the original verified state, not the changed
    // database row. No unsigned SQL row becomes cached permission authority.
    assert_eq!(other.load().unwrap().owner(), &state.owner().clone());
    drop(actor);
    drop(other);
    assert!(matches!(temp.open(), Err(Error::WrongCheckpoint)));
}
#[test]
fn sqlite_invalid_authenticated_suffix_and_schema_mutation_never_publish_state() {
    let temp = Temp::new();
    let store = temp.create();
    let waiting = temp.open().unwrap();
    let mut actor = Actor::new(store).unwrap();
    actor.offer(offer(), now()).unwrap();
    let raw = temp.raw();
    raw.execute(
        "UPDATE identity_events SET entry=?1 WHERE revision=1",
        params![b"{}".as_slice()],
    )
    .unwrap();
    assert!(matches!(waiting.load(), Err(Error::WrongCheckpoint)));
    raw.execute("CREATE TABLE unexpected_permissions (allow INTEGER)", [])
        .unwrap();
    assert!(matches!(actor.snapshot(), Err(Error::Storage)));
}
#[test]
fn sqlite_database_only_rollback_and_missing_anchor_fail_without_reset() {
    let temp = Temp::new();
    drop(temp.create());
    let backup = temp.0.join("owned-backup.sqlite3");
    fs::copy(temp.0.join("data").join(DB_NAME), &backup).unwrap();
    let mut actor = Actor::new(temp.open().unwrap()).unwrap();
    actor.offer(offer(), now()).unwrap();
    drop(actor);
    fs::copy(&backup, temp.0.join("data").join(DB_NAME)).unwrap();
    assert!(matches!(temp.open(), Err(Error::WrongCheckpoint)));
    fs::remove_file(temp.0.join("anchor").join(ANCHOR_NAME)).unwrap();
    assert!(matches!(temp.open(), Err(Error::Storage)));
}
#[test]
fn sqlite_two_writers_never_both_admit_same_signed_lease() {
    let temp = Temp::new();
    let (first, cert) = enroll(temp.create());
    let second = Actor::new(temp.open().unwrap()).unwrap();
    let bundle = request(&cert, 2, 2);
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));
    std::thread::scope(|scope| {
        let a = scope.spawn(|| {
            let mut actor = first;
            barrier.wait();
            actor.admit(
                bundle.0.clone(),
                bundle.1.clone(),
                Some(&bundle.2),
                &bundle.3,
                now(),
            )
        });
        let b = scope.spawn(|| {
            let mut actor = second;
            barrier.wait();
            actor.admit(
                bundle.0.clone(),
                bundle.1.clone(),
                Some(&bundle.2),
                &bundle.3,
                now(),
            )
        });
        let a = a.join().unwrap();
        let b = b.join().unwrap();
        assert_eq!(usize::from(a.is_ok()) + usize::from(b.is_ok()), 1);
    });
    assert_eq!(temp.open().unwrap().load().unwrap().revision(), 4);
}
#[test]
fn sqlite_private_files_links_free_space_and_wal_reader_pressure_fail_closed() {
    use std::os::unix::fs::symlink;
    let temp = Temp::new();
    let (mut actor, cert) = enroll(temp.create());
    let raw = temp.raw();
    raw.execute_batch("BEGIN;SELECT * FROM identity_meta;")
        .unwrap();
    let (lease, proof, fresh, op) = request(&cert, 2, 2);
    assert!(matches!(
        actor.admit(lease.clone(), proof.clone(), Some(&fresh), &op, now()),
        Err(Error::Conflict)
    ));
    raw.execute_batch("ROLLBACK").unwrap();
    drop(raw);
    actor.admit(lease, proof, Some(&fresh), &op, now()).unwrap();
    let store = actor.into_store();
    let db = temp.0.join("data").join(DB_NAME);
    let extra = temp.0.join("own-extra-link");
    fs::hard_link(&db, &extra).unwrap();
    assert!(matches!(store.load(), Err(Error::Storage)));
    fs::remove_file(extra).unwrap();
    fs::set_permissions(&db, fs::Permissions::from_mode(0o644)).unwrap();
    assert!(matches!(store.load(), Err(Error::Storage)));
    fs::set_permissions(&db, fs::Permissions::from_mode(0o600)).unwrap();
    drop(store);
    // Preflight must reject even a dangling WAL link before SQLite opens it.
    let wal = temp.0.join("data").join(format!("{DB_NAME}-wal"));
    assert!(!wal.exists());
    let outside = temp.0.join("must-not-create");
    symlink(&outside, &wal).unwrap();
    assert!(matches!(temp.open(), Err(Error::Storage)));
    assert!(!outside.exists());
}
#[test]
fn sqlite_logical_budget_denies_without_erasing_replay_or_changing_revision() {
    let temp = Temp::new();
    let budget = SqliteStorageBudget {
        max_logical_bytes: 512,
        ..SqliteStorageBudget::default()
    };
    let store = SqliteJournalStore::create(
        temp.0.join("data"),
        temp.0.join("anchor"),
        pin(),
        now(),
        budget,
    )
    .unwrap();
    let mut actor = Actor::new(store).unwrap();
    assert!(matches!(actor.offer(offer(), now()), Err(Error::Quota)));
    assert_eq!(actor.snapshot().unwrap().revision(), 0);
}
#[test]
fn sqlite_one_bounded_warm_sequence_retains_verified_state_and_never_claims_compaction() {
    let temp = Temp::new();
    let (mut actor, cert) = enroll(temp.create());
    let started = Instant::now();
    for n in 2..12u8 {
        let (lease, proof, fresh, op) = request(&cert, n, 2);
        actor.admit(lease, proof, Some(&fresh), &op, now()).unwrap();
    }
    let status = actor.into_store().status().unwrap();
    assert_eq!(status.startup_records_verified, 1);
    assert_eq!(status.suffix_records_verified, 0);
    assert_eq!(status.policy.consumed_leases, 10);
    assert_eq!(status.policy.consumed_nonces, 11);
    assert_eq!(status.policy.max_nonces, 4096);
    eprintln!("sqlite-warm-owned-sequence: admissions=10 elapsed_ms={} startup_verified={} suffix_verified={} logical_bytes={} wal_bytes={}",started.elapsed().as_millis(),status.startup_records_verified,status.suffix_records_verified,status.logical_bytes,status.wal_bytes);
}
struct OwnChild(Child);
impl Drop for OwnChild {
    fn drop(&mut self) {
        if self.0.try_wait().ok().flatten().is_none() {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
}
#[test]
fn fault_child() {
    let Some(root) = std::env::var_os("CC_AGENT_SQLITE_UNIT_ROOT") else {
        return;
    };
    let root = PathBuf::from(root);
    assert_eq!(
        fs::read(root.join("owned-test.marker")).unwrap(),
        b"cc-agent-sqlite-owned-unit-v1"
    );
    let store = SqliteJournalStore::open(
        root.join("data"),
        root.join("anchor"),
        pin(),
        SqliteStorageBudget::default(),
    )
    .unwrap();
    let mut actor = Actor::new(store).unwrap();
    let cert = actor.snapshot().unwrap().certificate(id()).unwrap().clone();
    let (lease, proof, fresh, op) = request(&cert, 2, 2);
    let _ = actor.admit(lease, proof, Some(&fresh), &op, now());
    panic!("fault hook not reached");
}
#[test]
fn sqlite_own_process_death_before_commit_anchor_and_response_has_safe_outcomes() {
    for point in [
        "before-sql-commit",
        "after-sql-commit",
        "after-anchor-ack",
        "after-pending-clear",
    ] {
        let temp = Temp::new();
        let (actor, cert) = enroll(temp.create());
        drop(actor);
        let child = Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "sqlite::tests::fault_child", "--nocapture"])
            .env("CC_AGENT_SQLITE_UNIT_ROOT", &temp.0)
            .env("CC_AGENT_SQLITE_UNIT_FAULT", point)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let mut child = OwnChild(child);
        let started = Instant::now();
        while !temp.0.join("fault.ready").exists() {
            assert!(
                started.elapsed() < Duration::from_secs(10),
                "own child fault deadline {point}"
            );
            assert!(
                child.0.try_wait().unwrap().is_none(),
                "own child exited before {point}"
            );
            std::thread::sleep(Duration::from_millis(10));
        }
        child.0.kill().unwrap();
        child.0.wait().unwrap();
        let opened = temp.open();
        if point == "after-sql-commit" || point == "after-anchor-ack" {
            assert!(
                matches!(opened, Err(Error::UncertainCommit)),
                "uncertain {point} must not repair"
            );
        } else {
            let mut actor = Actor::new(opened.unwrap()).unwrap();
            let (lease, proof, fresh, op) = request(&cert, 2, 2);
            let admitted = actor.admit(lease, proof, Some(&fresh), &op, now());
            if point == "before-sql-commit" {
                assert!(admitted.is_ok());
            } else {
                assert!(matches!(admitted, Err(Error::Replay)));
            }
        }
    }
}

#[test]
fn sqlite_live_database_replacement_is_rejected_until_fresh_verified_open() {
    let temp = Temp::new();
    let store = temp.create();
    let path = temp.0.join("data").join(DB_NAME);
    let old = store.database_identity;
    // Same contents in a different inode must not leave a live connection writing
    // an orphaned database while its configured path points somewhere else.
    store
        .inner
        .borrow()
        .conn
        .execute_batch("PRAGMA wal_checkpoint(TRUNCATE)")
        .unwrap();
    let replacement = temp.0.join("owned-replacement.sqlite3");
    fs::copy(&path, &replacement).unwrap();
    fs::rename(&replacement, &path).unwrap();
    assert_ne!(db_identity(&temp.0.join("data")).unwrap(), old);
    assert!(matches!(store.load(), Err(Error::Storage)));
}
#[test]
fn sqlite_native_page_limit_full_rolls_back_and_never_consumes_rejected_lease() {
    let temp = Temp::new();
    let (actor, cert) = enroll(temp.create());
    let mut store = actor.into_store();
    let pages: i64 = store
        .inner
        .get_mut()
        .conn
        .pragma_query_value(None, "page_count", |r| r.get(0))
        .unwrap();
    store
        .inner
        .get_mut()
        .conn
        .pragma_update(None, "max_page_count", pages + 1)
        .unwrap();
    let mut actor = Actor::new(store).unwrap();
    let mut rejected = None;
    for n in 2..32u8 {
        let bundle = request(&cert, n, 2);
        let before = actor.snapshot().unwrap().revision();
        match actor.admit(
            bundle.0.clone(),
            bundle.1.clone(),
            Some(&bundle.2),
            &bundle.3,
            now(),
        ) {
            Ok(_) => (),
            Err(Error::Quota) => {
                assert_eq!(actor.snapshot().unwrap().revision(), before);
                rejected = Some(bundle);
                break;
            }
            Err(error) => panic!("unexpected native quota result {error}"),
        }
    }
    let (lease, proof, fresh, op) =
        rejected.expect("bounded own database must hit native SQLite page limit");
    let mut store = actor.into_store();
    store
        .inner
        .get_mut()
        .conn
        .pragma_update(None, "max_page_count", 16384i64)
        .unwrap();
    let mut actor = Actor::new(store).unwrap();
    assert!(actor.admit(lease, proof, Some(&fresh), &op, now()).is_ok());
}
