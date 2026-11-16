// Included in the Linux-only reader test module; test keys never enter production.
struct ReaderTemp(std::path::PathBuf);
impl ReaderTemp {
    fn new() -> Self {
        use std::os::unix::fs::DirBuilderExt;
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "cc-reader-sqlite-unit-{}-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, std::sync::atomic::Ordering::SeqCst),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::DirBuilder::new()
            .mode(0o700)
            .create(&path)
            .unwrap();
        std::fs::write(path.join("owned.marker"), b"cc-reader-sqlite-unit-v1").unwrap();
        Self(path)
    }
    fn store(&self, f: &Fixture, create: bool) -> ReaderSqliteLedger {
        if create {
            ReaderSqliteLedger::create(
                self.0.join("data"),
                self.0.join("anchor"),
                f.pin.clone(),
                Now(time(1000)),
                ReaderStorageBudget::default(),
            )
            .unwrap()
        } else {
            ReaderSqliteLedger::open(
                self.0.join("data"),
                self.0.join("anchor"),
                f.pin.clone(),
                ReaderStorageBudget::default(),
            )
            .unwrap()
        }
    }
    fn actor(&self, f: &Fixture, create: bool) -> ReaderActor<ReaderSqliteLedger, Nodes> {
        let mut actor = ReaderActor::new(
            self.store(f, create),
            Actor::new(f.nodes.clone()).unwrap(),
            f.targets.clone(),
            f.pin.node_trust_origin_hash,
        )
        .unwrap();
        if create {
            actor
                .administer(
                    ReaderAdministration::InstallAssociation {
                        signed: f.association.clone(),
                    },
                    &mut clock,
                )
                .unwrap();
        }
        actor
    }
}
impl Drop for ReaderTemp {
    fn drop(&mut self) {
        let base = std::env::temp_dir();
        assert_eq!(self.0.parent(), Some(base.as_path()));
        assert!(self
            .0
            .file_name()
            .unwrap()
            .to_string_lossy()
            .starts_with("cc-reader-sqlite-unit-"));
        assert!(!std::fs::symlink_metadata(&self.0)
            .unwrap()
            .file_type()
            .is_symlink());
        assert!(
            std::fs::read(self.0.join("owned.marker")).unwrap() == b"cc-reader-sqlite-unit-v1",
            "owned test marker must match"
        );
        std::fs::remove_dir_all(&self.0).unwrap();
    }
}
#[test]
fn reader_sqlite_record_cap_includes_genesis_and_last_allowed_event() {
    let f = Fixture::new();
    let temp = ReaderTemp::new();
    let budget = ReaderStorageBudget {
        max_records: 2,
        ..ReaderStorageBudget::default()
    };
    let store = ReaderSqliteLedger::create(
        temp.0.join("data"),
        temp.0.join("anchor"),
        f.pin.clone(),
        Now(time(1000)),
        budget,
    )
    .unwrap();
    let mut actor = ReaderActor::new(
        store,
        Actor::new(f.nodes.clone()).unwrap(),
        f.targets.clone(),
        f.pin.node_trust_origin_hash,
    )
    .unwrap();
    actor
        .administer(
            ReaderAdministration::InstallAssociation {
                signed: f.association.clone(),
            },
            &mut clock,
        )
        .unwrap();
    let state_before = actor.snapshot().unwrap();
    assert_eq!(state_before.revision(), 1);
    let capacity_before = serde_json::to_value(state_before.capacity()).unwrap();
    let anchor_before = std::fs::read(temp.0.join("anchor/reader-head.json")).unwrap();
    let store = actor.into_store();
    let logical_before = store.status().unwrap().logical_bytes;
    let mut actor = ReaderActor::new(
        store,
        Actor::new(f.nodes.clone()).unwrap(),
        f.targets.clone(),
        f.pin.node_trust_origin_hash,
    )
    .unwrap();
    let rejected = actor.challenge(
        f.association.body.digest().unwrap(),
        ReaderOperationId::new("99999999-9999-4999-8999-999999999999").unwrap(),
        f.cert.agent_id,
        &f.association.body.operation,
        f.association.body.target_descriptor_hash,
        f.fresh(),
        &mut clock,
    );
    assert_eq!(rejected.err(), Some(ReaderError::Quota));
    let state_after = actor.snapshot().unwrap();
    assert_eq!(state_after.revision(), state_before.revision());
    assert_eq!(state_after.authority(), state_before.authority());
    assert_eq!(
        serde_json::to_value(state_after.capacity()).unwrap(),
        capacity_before
    );
    assert!(
        std::fs::read(temp.0.join("anchor/reader-head.json")).unwrap() == anchor_before,
        "reader anchor unchanged after quota denial"
    );
    assert_eq!(
        actor.into_store().status().unwrap().logical_bytes,
        logical_before
    );
}
#[test]
fn reader_sqlite_consumption_restart_and_external_signed_revoke_refresh() {
    let f = Fixture::new();
    let temp = ReaderTemp::new();
    let mut actor = temp.actor(&f, true);
    let c = f.challenge(&mut actor, 1);
    let bundle = f.bundle(&c, 1);
    let p = f.begin(&mut actor, bundle.clone()).unwrap();
    actor.check_permit(&p, &mut clock).unwrap();
    drop(actor);
    let mut actor = temp.actor(&f, false);
    assert!(matches!(
        f.begin(&mut actor, bundle),
        Err(ReaderError::Replay)
    ));
    let c = f.challenge(&mut actor, 2);
    let p = f.begin(&mut actor, f.bundle(&c, 2)).unwrap();
    let mut external = temp.actor(&f, false);
    let head = external.snapshot().unwrap().authority().clone();
    external
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
                            association_id: f.association.body.association_id,
                            body_hash: f.association.body.digest().unwrap(),
                        },
                        issued_at: time(1000),
                    },
                    33,
                ),
            },
            &mut clock,
        )
        .unwrap();
    assert_eq!(
        actor.check_permit(&p, &mut clock),
        Err(ReaderError::WrongHead)
    );
    drop(external);
    drop(actor);
    let state = temp.store(&f, false).load().unwrap();
    assert_eq!(state.capacity().consumed_grants, 2);
    assert_eq!(state.authority().authority_revision, 2);
    // Registered historical events remain restorable beyond their lifetime.
    let store = temp.store(&f, false);
    assert_eq!(store.load().unwrap().capacity().consumed_nonces, 2);
}
#[test]
fn reader_sqlite_two_connections_cannot_consume_same_grant() {
    let f = Fixture::new();
    let temp = ReaderTemp::new();
    let mut actor = temp.actor(&f, true);
    let c = f.challenge(&mut actor, 1);
    let bundle = f.bundle(&c, 1);
    drop(actor);
    let barrier = Arc::new(std::sync::Barrier::new(2));
    let mut children = Vec::new();
    for _ in 0..2 {
        let base = temp.0.clone();
        let pin = f.pin.clone();
        let nodes = f.nodes.clone();
        let targets = f.targets.clone();
        let origin = f.pin.node_trust_origin_hash;
        let bundle = bundle.clone();
        let barrier = Arc::clone(&barrier);
        let agent = f.cert.agent_id;
        let operation = f.association.body.operation.clone();
        let target = f.association.body.target_descriptor_hash;
        children.push(std::thread::spawn(move || {
            let store = ReaderSqliteLedger::open(
                base.join("data"),
                base.join("anchor"),
                pin,
                ReaderStorageBudget::default(),
            )
            .unwrap();
            let mut actor =
                ReaderActor::new(store, Actor::new(nodes).unwrap(), targets, origin).unwrap();
            barrier.wait();
            actor.begin(bundle, agent, &operation, target, &mut clock)
        }));
    }
    let outcomes: Vec<_> = children.into_iter().map(|c| c.join().unwrap()).collect();
    assert_eq!(outcomes.iter().filter(|v| v.is_ok()).count(), 1);
    assert!(outcomes
        .iter()
        .filter_map(|v| v.as_ref().err())
        .all(|e| matches!(
            e,
            ReaderError::Conflict | ReaderError::Replay | ReaderError::UncertainCommit
        )));
    assert_eq!(
        temp.store(&f, false)
            .load()
            .unwrap()
            .capacity()
            .consumed_grants,
        1
    );
}
#[test]
fn reader_sqlite_anchor_rollback_partial_files_and_private_modes_fail_closed() {
    use std::os::unix::fs::PermissionsExt;
    let f = Fixture::new();
    let temp = ReaderTemp::new();
    let mut actor = temp.actor(&f, true);
    let old = std::fs::read(temp.0.join("anchor/reader-head.json")).unwrap();
    f.challenge(&mut actor, 1);
    drop(actor);
    let latest = std::fs::read(temp.0.join("anchor/reader-head.json")).unwrap();
    std::fs::write(temp.0.join("anchor/reader-head.json"), &old).unwrap();
    assert!(ReaderSqliteLedger::open(
        temp.0.join("data"),
        temp.0.join("anchor"),
        f.pin.clone(),
        ReaderStorageBudget::default()
    )
    .is_err());
    // Test-owned restoration of the exact newest anchor is setup, no product repair.
    std::fs::write(temp.0.join("anchor/reader-head.json"), &latest).unwrap();
    std::fs::write(temp.0.join("anchor/reader-head-unknown.tmp"), b"partial").unwrap();
    assert!(ReaderSqliteLedger::open(
        temp.0.join("data"),
        temp.0.join("anchor"),
        f.pin.clone(),
        ReaderStorageBudget::default()
    )
    .is_err());
    std::fs::remove_file(temp.0.join("anchor/reader-head-unknown.tmp")).unwrap();
    std::fs::set_permissions(temp.0.join("data"), std::fs::Permissions::from_mode(0o500)).unwrap();
    assert!(ReaderSqliteLedger::open(
        temp.0.join("data"),
        temp.0.join("anchor"),
        f.pin.clone(),
        ReaderStorageBudget::default()
    )
    .is_err());
    std::fs::set_permissions(temp.0.join("data"), std::fs::Permissions::from_mode(0o700)).unwrap();
    assert!(temp.store(&f, false).load().is_ok());
}
#[test]
fn invalid_reader_time_body_does_not_poison_pending_but_node_clock_floor_does() {
    let f = Fixture::new();
    let mut actor = f.actor();
    let c = f.challenge(&mut actor, 1);
    let p = f.begin(&mut actor, f.bundle(&c, 1)).unwrap();
    let c2 = f.challenge(&mut actor, 2);
    let mut bad = f.bundle(&c2, 2);
    bad.proof.body.issued_at = time(1001);
    let revision = actor.snapshot().unwrap().revision();
    assert!(matches!(
        f.begin(&mut actor, bad),
        Err(ReaderError::InvalidTime)
    ));
    assert_eq!(actor.snapshot().unwrap().revision(), revision);
    actor.check_permit(&p, &mut clock).unwrap();
    let hdr = Header {
        version: 1,
        context: f.pin.context.clone(),
        owner_epoch: 1,
        checkpoint: 2,
    };
    let lease = Lease {
        header: hdr.clone(),
        id: LeaseId::new("55555555-5555-4555-8555-000000000088").unwrap(),
        agent_id: f.cert.agent_id,
        key_epoch: 1,
        certificate_hash: f.cert.digest().unwrap(),
        operation: f.association.body.operation.clone(),
        issued_at: time(1001),
        expires_at: time(1120),
    };
    let proof = AgentRequest {
        header: hdr.clone(),
        agent_id: f.cert.agent_id,
        key_epoch: 1,
        lease_hash: lease.digest().unwrap(),
        nonce: Nonce::from_bytes([88; 16]),
        issued_at: time(1001),
    };
    let fresh = node_signed(
        Freshness {
            header: hdr,
            issued_at: time(1001),
            expires_at: time(1130),
        },
        11,
    );
    Actor::new(f.nodes.clone())
        .unwrap()
        .admit(
            node_signed(lease, 11),
            node_signed(proof, 22),
            Some(&fresh),
            &f.association.body.operation,
            Now(time(1001)),
        )
        .unwrap();
    assert_eq!(
        actor.check_permit(&p, &mut clock),
        Err(ReaderError::ClockUnavailable)
    );
}
#[test]
fn reader_sqlite_process_death_preserves_pending_anchor_and_consumption_outcomes() {
    if let Ok(path) = std::env::var("CC_READER_SQLITE_TEST_ROOT") {
        let path = std::path::PathBuf::from(path);
        let f = Fixture::new();
        let store = ReaderSqliteLedger::open(
            path.join("data"),
            path.join("anchor"),
            f.pin.clone(),
            ReaderStorageBudget::default(),
        )
        .unwrap();
        let mut actor = ReaderActor::new(
            store,
            Actor::new(f.nodes.clone()).unwrap(),
            f.targets.clone(),
            f.pin.node_trust_origin_hash,
        )
        .unwrap();
        let c: ReaderChallenge =
            strict_json(&std::fs::read(path.join("challenge.json")).unwrap(), 16384).unwrap();
        let _permit = f.begin(&mut actor, f.bundle(&c, 1)).unwrap();
        return;
    }
    for point in [
        "before-sql-commit",
        "after-sql-commit",
        "after-anchor-ack",
        "after-pending-clear",
    ] {
        let f = Fixture::new();
        let temp = ReaderTemp::new();
        let mut actor = temp.actor(&f, true);
        let c = f.challenge(&mut actor, 1);
        drop(actor);
        std::fs::write(
            temp.0.join("challenge.json"),
            serde_json::to_vec(&c).unwrap(),
        )
        .unwrap();
        let mut child=std::process::Command::new(std::env::current_exe().unwrap()).args(["--exact","reader::tests::linux::reader_sqlite_process_death_preserves_pending_anchor_and_consumption_outcomes","--test-threads=1"])
            .env("CC_READER_SQLITE_TEST_ROOT",&temp.0).env("CC_READER_SQLITE_TEST_FAULT",point)
            .stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null()).spawn().unwrap();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while !temp.0.join("fault.ready").exists() && std::time::Instant::now() < deadline {
            if child.try_wait().unwrap().is_some() {
                panic!("owned reader crash child exited before fault marker");
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        if !temp.0.join("fault.ready").exists() {
            let _ = child.kill();
            let _ = child.wait();
            panic!("owned reader fault deadline expired");
        }
        child.kill().unwrap();
        child.wait().unwrap();
        let opened = ReaderSqliteLedger::open(
            temp.0.join("data"),
            temp.0.join("anchor"),
            f.pin.clone(),
            ReaderStorageBudget::default(),
        );
        match point {
            "before-sql-commit" => {
                let store = opened.unwrap();
                let mut actor = ReaderActor::new(
                    store,
                    Actor::new(f.nodes.clone()).unwrap(),
                    f.targets.clone(),
                    f.pin.node_trust_origin_hash,
                )
                .unwrap();
                f.begin(&mut actor, f.bundle(&c, 1)).unwrap();
            }
            "after-pending-clear" => {
                let store = opened.unwrap();
                let mut actor = ReaderActor::new(
                    store,
                    Actor::new(f.nodes.clone()).unwrap(),
                    f.targets.clone(),
                    f.pin.node_trust_origin_hash,
                )
                .unwrap();
                assert!(matches!(
                    f.begin(&mut actor, f.bundle(&c, 1)),
                    Err(ReaderError::Replay)
                ));
            }
            _ => assert!(opened.is_err()),
        }
    }
}
