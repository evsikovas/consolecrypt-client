use ed25519_dalek::{Signer, SigningKey};
use std::cell::RefCell;
use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
thread_local! {
    static FAIL:RefCell<Option<(&'static str,&'static str)>>=const{RefCell::new(None)};
    static FILL:RefCell<Option<std::path::PathBuf>>=const{RefCell::new(None)};
    static IO_ERROR:std::cell::Cell<Option<i32>>=const{std::cell::Cell::new(None)};
}
pub(super) fn io_error(code: Option<i32>) {
    IO_ERROR.with(|v| v.set(code));
}
pub(super) fn fault(stage: &str, name: &str) -> Result<()> {
    if stage == "after-create" && name == ACTIVE {
        if let Some(path) = FILL.with(|f| f.borrow_mut().take()) {
            let mut file = OpenOptions::new()
                .create_new(true)
                .write(true)
                .mode(0o600)
                .open(path)
                .unwrap();
            let block = [0u8; 65536];
            let mut written = 0;
            loop {
                match file.write_all(&block) {
                    Ok(()) => {
                        written += block.len();
                        assert!(written <= 64 * 1024 * 1024);
                    }
                    Err(error) => {
                        assert_eq!(error.raw_os_error(), Some(28));
                        break;
                    }
                }
            }
        }
    }
    if FAIL.with(|f| {
        f.borrow()
            .as_ref()
            .is_some_and(|v| v.0 == stage && v.1 == name)
    }) {
        return Err(Error::UncertainCommit);
    }
    if std::env::var("CC_ACTIVATION_FAULT").ok().as_deref() == Some(&format!("{stage}:{name}")) {
        let root = owned_child_root();
        fs::write(root.join("fault.ready"), b"activation-owned-ready").unwrap();
        std::thread::sleep(Duration::from_secs(20));
        return Err(Error::UncertainCommit);
    }
    Ok(())
}
static NEXT: AtomicU64 = AtomicU64::new(0);
struct Temp(std::path::PathBuf);
impl Temp {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "cc-node-activation-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::SeqCst)
        ));
        fs::DirBuilder::new().mode(0o700).create(&path).unwrap();
        fs::write(path.join("owned.marker"), b"cc-activation-unit-v1").unwrap();
        Self(path)
    }
    fn node(&self) -> std::path::PathBuf {
        self.0.join("node")
    }
}
fn validate_root(path: &Path) {
    assert_eq!(path.parent(), Some(std::env::temp_dir().as_path()));
    assert!(path
        .file_name()
        .unwrap()
        .to_str()
        .unwrap()
        .starts_with("cc-node-activation-"));
    assert_eq!(
        fs::read(path.join("owned.marker")).unwrap(),
        b"cc-activation-unit-v1"
    );
    assert!(fs::symlink_metadata(path).unwrap().is_dir());
}
impl Drop for Temp {
    fn drop(&mut self) {
        validate_root(&self.0);
        fs::remove_dir_all(&self.0).unwrap();
    }
}
fn owned_child_root() -> std::path::PathBuf {
    let path = std::path::PathBuf::from(std::env::var_os("CC_ACTIVATION_ROOT").unwrap());
    validate_root(&path);
    path
}
fn id() -> AgentId {
    AgentId::new("44444444-4444-4444-8444-444444444444").unwrap()
}
fn time(t: u64) -> Timestamp {
    Timestamp::new(t).unwrap()
}
fn now() -> Result<Now> {
    Ok(Now(time(1000)))
}
fn sign<T: Statement>(body: T, seed: u8) -> Signed<T> {
    Signed {
        signature: SignatureBytes::from_bytes(
            SigningKey::from_bytes(&[seed; 32])
                .sign(&body.signing_bytes().unwrap())
                .to_bytes(),
        ),
        body,
    }
}
fn original() -> OwnerPin {
    OwnerPin {
        context: Context {
            tenant: Tenant::LocalProfile {
                id: LocalProfileId::new("11111111-1111-4111-8111-111111111111").unwrap(),
            },
            deployment_id: DeploymentId::new("22222222-2222-4222-8222-222222222222").unwrap(),
        },
        key: PublicKey::from_bytes(SigningKey::from_bytes(&[11; 32]).verifying_key().to_bytes()),
        epoch: 1,
        checkpoint: 0,
        expires_at: time(10000),
    }
}
fn open(temp: &Temp) -> Actor<SqliteJournalStore> {
    Actor::new(
        SqliteJournalStore::open(
            temp.0.join("db"),
            temp.0.join("anchor"),
            original(),
            SqliteStorageBudget::default(),
        )
        .unwrap(),
    )
    .unwrap()
}
fn fixture() -> (
    Temp,
    Actor<SqliteJournalStore>,
    PreparedNodeKeyPublic,
    LocalActivationInput,
) {
    fixture_with_budget(SqliteStorageBudget::default())
}
fn fixture_with_budget(
    budget: SqliteStorageBudget,
) -> (
    Temp,
    Actor<SqliteJournalStore>,
    PreparedNodeKeyPublic,
    LocalActivationInput,
) {
    let temp = Temp::new();
    let old = provision_node(&temp.node(), id()).unwrap();
    let pin = original();
    let mut actor = Actor::new(
        SqliteJournalStore::create(
            temp.0.join("db"),
            temp.0.join("anchor"),
            pin.clone(),
            now().unwrap(),
            budget,
        )
        .unwrap(),
    )
    .unwrap();
    let offer = sign(
        Offer {
            header: Header {
                version: 1,
                context: pin.context.clone(),
                owner_epoch: 1,
                checkpoint: 1,
            },
            invitation_id: InvitationId::new("33333333-3333-4333-8333-333333333333").unwrap(),
            agent_id: id(),
            agent_key: old.public_key,
            token_hash: InvitationToken::from_bytes([33; 32]).unwrap().digest(),
            scopes: Scopes(vec![Scope::TelemetryRead]),
            issued_at: time(1000),
            expires_at: time(1600),
        },
        11,
    );
    let claim = create_node_claim(&temp.node(), id(), &pin, offer.clone(), now().unwrap()).unwrap();
    actor.offer(offer.clone(), now().unwrap()).unwrap();
    actor
        .consume(
            &InvitationToken::from_bytes([33; 32]).unwrap(),
            claim.clone(),
            now().unwrap(),
        )
        .unwrap();
    let previous = sign(
        Certificate {
            header: Header {
                checkpoint: 2,
                ..offer.body.header
            },
            agent_id: id(),
            key_epoch: 1,
            agent_key: old.public_key,
            scopes: offer.body.scopes,
            capabilities: claim.body.capabilities.clone(),
            origin: CertificateOrigin::Enrollment {
                invitation_id: claim.body.invitation_id,
                claim_hash: claim.body.digest().unwrap(),
                key_fingerprint: old.key_fingerprint,
            },
            not_before: time(1000),
            expires_at: time(1600),
        },
        11,
    );
    actor.approve(previous.clone(), now().unwrap()).unwrap();
    let prepared = prepare_node_rotation(&temp.node(), id(), old.key_fingerprint).unwrap();
    let next = sign(
        Certificate {
            header: Header {
                checkpoint: 3,
                ..previous.body.header.clone()
            },
            agent_id: id(),
            key_epoch: 2,
            agent_key: prepared.new_public_key,
            scopes: previous.body.scopes.clone(),
            capabilities: previous.body.capabilities.clone(),
            origin: CertificateOrigin::Rotation {
                previous_certificate: previous.body.digest().unwrap(),
                previous_key_epoch: 1,
            },
            not_before: time(1000),
            expires_at: time(1500),
        },
        11,
    );
    let proofs = sign_prepared_node_rotation(
        &temp.node(),
        id(),
        &prepared.rotation_id,
        actor.snapshot().unwrap().owner(),
        PreparedRotationInput {
            previous_certificate: previous.clone(),
            certificate: next.clone(),
        },
        now().unwrap(),
    )
    .unwrap();
    actor
        .rotate(proofs.certificate, proofs.old, proofs.new, now().unwrap())
        .unwrap();
    (
        temp,
        actor,
        prepared,
        LocalActivationInput {
            previous_certificate: previous.body,
            committed_certificate: next.body,
        },
    )
}
fn immutable(temp: &Temp) -> Vec<(String, Hash)> {
    [
        "node-identity.key",
        "node-rotation.intent.v2",
        "node-rotation.pending.v2",
        "node-rotation.state.v2",
    ]
    .iter()
    .map(|n| {
        (
            n.to_string(),
            file_hash(&zeroize::Zeroizing::new(
                fs::read(temp.node().join(n)).unwrap(),
            )),
        )
    })
    .collect()
}
#[test]
fn activation_clock_reobserves_around_io_and_never_hides_expiry_or_rollback() {
    for (at, t, expected) in [
        (1, 999, Error::InvalidTime),
        (2, 1500, Error::Expired),
        (5, 1500, Error::Expired),
        (7, 999, Error::InvalidTime),
    ] {
        let (temp, mut actor, p, input) = fixture();
        let before = immutable(&temp);
        let mut calls = 0;
        let result = activate(
            &temp.node(),
            &mut actor,
            id(),
            &p.rotation_id,
            input,
            &mut || {
                calls += 1;
                Ok(Now(time(if calls >= at { t } else { 1000 })))
            },
        );
        assert_eq!(result, Err(expected), "clock sample {at}");
        assert!(calls >= at);
        assert_eq!(immutable(&temp), before);
    }
}
#[test]
fn activation_retained_commit_is_not_new_issuance_and_owner_narrowing_does_not_rewrite_intent() {
    let (temp, mut actor, p, input) = fixture();
    let before = immutable(&temp);
    let owner = actor.snapshot().unwrap().owner().clone();
    let change = OwnerRotation {
        header: Header {
            checkpoint: 4,
            ..Header {
                version: 1,
                context: owner.context,
                owner_epoch: 1,
                checkpoint: 3,
            }
        },
        new_epoch: 2,
        new_key: PublicKey::from_bytes(
            SigningKey::from_bytes(&[55; 32]).verifying_key().to_bytes(),
        ),
        issued_at: time(1100),
        expires_at: time(1400),
    };
    actor
        .rotate_owner(
            sign(change.clone(), 11),
            sign(NewOwnerProof(change), 55),
            Now(time(1100)),
        )
        .unwrap();
    let output = activate(
        &temp.node(),
        &mut actor,
        id(),
        &p.rotation_id,
        input.clone(),
        &mut || Ok(Now(time(1200))),
    )
    .unwrap();
    assert_eq!(output.selected_key_epoch, 2);
    assert_eq!(immutable(&temp), before);
    let records = (
        fs::read(temp.node().join(INTENT)).unwrap(),
        fs::read(temp.node().join(ACTIVE)).unwrap(),
    );
    assert_eq!(
        activate(
            &temp.node(),
            &mut actor,
            id(),
            &p.rotation_id,
            input.clone(),
            &mut || Ok(Now(time(1201)))
        )
        .unwrap(),
        output
    );
    assert_eq!(
        records,
        (
            fs::read(temp.node().join(INTENT)).unwrap(),
            fs::read(temp.node().join(ACTIVE)).unwrap()
        )
    );
    assert_eq!(
        activate(
            &temp.node(),
            &mut actor,
            id(),
            &p.rotation_id,
            input,
            &mut || Ok(Now(time(1400)))
        ),
        Err(Error::Expired)
    );
}
#[test]
fn activation_denies_foreign_assertions_widening_and_revoked_head_without_publication() {
    for case in [
        "previous",
        "certificate",
        "agent",
        "capability",
        "scope",
        "expiry",
        "backdate",
        "revoked",
    ] {
        let (temp, mut actor, p, mut input) = fixture();
        let before = immutable(&temp);
        let revision = actor.snapshot().unwrap().revision();
        match case {
            "previous" => input.previous_certificate.not_before = time(999),
            "certificate" => input.committed_certificate.header.checkpoint = 99,
            "agent" => {
                input.committed_certificate.agent_id =
                    AgentId::new("99999999-9999-4999-8999-999999999999").unwrap()
            }
            "capability" => input
                .committed_certificate
                .capabilities
                .scopes
                .0
                .push(Scope::MetricsCollect),
            "scope" => input
                .committed_certificate
                .scopes
                .0
                .push(Scope::MetricsCollect),
            "expiry" => input.committed_certificate.expires_at = time(1601),
            "backdate" => input.committed_certificate.not_before = time(999),
            "revoked" => actor
                .revoke(
                    sign(
                        Revocation {
                            header: Header {
                                version: 1,
                                context: original().context,
                                owner_epoch: 1,
                                checkpoint: 4,
                            },
                            agent_id: id(),
                            key_epoch: 2,
                            certificate_hash: input.committed_certificate.digest().unwrap(),
                            issued_at: time(1000),
                        },
                        11,
                    ),
                    now().unwrap(),
                )
                .unwrap(),
            _ => unreachable!(),
        }
        let before_revision = actor.snapshot().unwrap().revision();
        assert!(
            activate(
                &temp.node(),
                &mut actor,
                id(),
                &p.rotation_id,
                input,
                &mut now
            )
            .is_err(),
            "{case}"
        );
        assert_eq!(actor.snapshot().unwrap().revision(), before_revision);
        assert!(before_revision >= revision);
        assert!(!temp.node().join(INTENT).exists());
        assert!(!temp.node().join(ACTIVE).exists());
        assert_eq!(immutable(&temp), before);
    }
}
#[test]
fn activation_rechecks_external_revoke_after_intent_before_selection() {
    for revoke_at in [5, 7] {
        let (temp, mut actor, p, input) = fixture();
        let mut other = open(&temp);
        let before = immutable(&temp);
        let cert = input.committed_certificate.clone();
        let mut calls = 0;
        let result = activate(
            &temp.node(),
            &mut actor,
            id(),
            &p.rotation_id,
            input,
            &mut || {
                calls += 1;
                if calls == revoke_at {
                    other
                        .revoke(
                            sign(
                                Revocation {
                                    header: Header {
                                        version: 1,
                                        context: original().context,
                                        owner_epoch: 1,
                                        checkpoint: 4,
                                    },
                                    agent_id: id(),
                                    key_epoch: 2,
                                    certificate_hash: cert.digest().unwrap(),
                                    issued_at: time(1000),
                                },
                                11,
                            ),
                            now().unwrap(),
                        )
                        .unwrap();
                }
                now()
            },
        );
        assert_eq!(result, Err(Error::Revoked));
        assert!(temp.node().join(INTENT).exists());
        assert_eq!(temp.node().join(ACTIVE).exists(), revoke_at == 7);
        assert_eq!(immutable(&temp), before);
        if revoke_at == 7 {
            assert_eq!(provision_node(&temp.node(), id()).unwrap().key_epoch, 2);
        } else {
            assert!(provision_node(&temp.node(), id()).is_err());
        }
    }
}
#[test]
fn activation_rechecks_external_owner_change_and_requires_explicit_fresh_resume() {
    for rotate_at in [5, 7] {
        let (temp, mut actor, p, input) = fixture();
        let mut other = open(&temp);
        let before = immutable(&temp);
        let revision = actor.snapshot().unwrap().revision();
        let change = OwnerRotation {
            header: Header {
                version: 1,
                context: original().context,
                owner_epoch: 1,
                checkpoint: 4,
            },
            new_epoch: 2,
            new_key: PublicKey::from_bytes(
                SigningKey::from_bytes(&[55; 32]).verifying_key().to_bytes(),
            ),
            issued_at: time(1000),
            expires_at: time(1400),
        };
        let mut calls = 0;
        let result = activate(
            &temp.node(),
            &mut actor,
            id(),
            &p.rotation_id,
            input.clone(),
            &mut || {
                calls += 1;
                if calls == rotate_at {
                    other
                        .rotate_owner(
                            sign(change.clone(), 11),
                            sign(NewOwnerProof(change.clone()), 55),
                            now().unwrap(),
                        )
                        .unwrap();
                }
                now()
            },
        );
        assert!(
            matches!(result, Err(Error::WrongCheckpoint)),
            "changed owner must invalidate this activation acknowledgement"
        );
        assert_eq!(calls, rotate_at + 1);
        assert!(
            immutable(&temp) == before,
            "activation must preserve original and prepared private records"
        );
        assert!(temp.node().join(INTENT).exists());
        assert_eq!(temp.node().join(ACTIVE).exists(), rotate_at == 7);
        if rotate_at == 7 {
            assert_eq!(provision_node(&temp.node(), id()).unwrap().key_epoch, 2);
        } else {
            assert!(provision_node(&temp.node(), id()).is_err());
        }
        let intent_before_resume = fs::read(temp.node().join(INTENT)).unwrap();
        let active_before_resume = fs::read(temp.node().join(ACTIVE)).ok();
        let current_owner = other.snapshot().unwrap().owner().clone();
        assert_eq!(current_owner.epoch, 2);
        assert_eq!(current_owner.checkpoint, 4);
        let mut replayed = open(&temp);
        assert_eq!(replayed.snapshot().unwrap().revision(), revision + 1);
        assert!(
            replayed
                .observe_local_rotation(&input.committed_certificate, &mut || Ok(Now(time(1001))))
                .unwrap()
                == current_owner,
            "original-pin replay must retain the exact committed certificate"
        );
        let resumed = activate(
            &temp.node(),
            &mut actor,
            id(),
            &p.rotation_id,
            input,
            &mut || Ok(Now(time(1001))),
        )
        .unwrap();
        assert_eq!(resumed.selected_key_epoch, 2);
        assert!(resumed.local_only);
        assert!(
            fs::read(temp.node().join(INTENT)).unwrap() == intent_before_resume,
            "explicit resume must preserve the exact existing intent"
        );
        if let Some(active) = active_before_resume {
            assert!(
                fs::read(temp.node().join(ACTIVE)).unwrap() == active,
                "explicit resume must preserve the exact existing selector"
            );
        }
        assert!(
            immutable(&temp) == before,
            "explicit resume must preserve original and prepared private records"
        );
        assert_eq!(actor.snapshot().unwrap().revision(), revision + 1);
        assert_eq!(open(&temp).snapshot().unwrap().revision(), revision + 1);
    }
}
#[test]
fn activation_partial_unknown_linked_noncanonical_records_quarantine_without_repair() {
    for case in [
        "partial-intent",
        "partial-active",
        "unknown",
        "noncanonical",
        "linked",
        "permissive",
    ] {
        let (temp, mut actor, p, input) = fixture();
        let before = immutable(&temp);
        if case == "partial-intent" || case == "unknown" {
            let path = temp.node().join(if case == "unknown" {
                "node-activation.unknown.v3"
            } else {
                INTENT
            });
            let file = OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o600)
                .open(path)
                .unwrap();
            drop(file);
        } else {
            FAIL.with(|f| *f.borrow_mut() = Some(("after-readback", INTENT)));
            assert!(activate(
                &temp.node(),
                &mut actor,
                id(),
                &p.rotation_id,
                input.clone(),
                &mut now
            )
            .is_err());
            FAIL.with(|f| *f.borrow_mut() = None);
            match case {
                "partial-active" => {
                    OpenOptions::new()
                        .create_new(true)
                        .write(true)
                        .mode(0o600)
                        .open(temp.node().join(ACTIVE))
                        .unwrap();
                }
                "noncanonical" => {
                    let path = temp.node().join(INTENT);
                    let bytes = fs::read(&path).unwrap();
                    let mut modified = vec![b' '];
                    modified.extend(bytes);
                    fs::write(path, modified).unwrap();
                }
                "linked" => {
                    fs::hard_link(temp.node().join(INTENT), temp.0.join("hardlink")).unwrap();
                }
                "permissive" => {
                    fs::set_permissions(temp.node().join(INTENT), fs::Permissions::from_mode(0o644))
                        .unwrap()
                }
                _ => unreachable!(),
            }
        }
        let records = fs::read_dir(temp.node())
            .unwrap()
            .map(|e| {
                let p = e.unwrap().path();
                (
                    p.clone(),
                    file_hash(&zeroize::Zeroizing::new(fs::read(p).unwrap())),
                )
            })
            .collect::<Vec<_>>();
        assert!(
            activate(
                &temp.node(),
                &mut actor,
                id(),
                &p.rotation_id,
                input,
                &mut now
            )
            .is_err(),
            "{case}"
        );
        for (path, hash) in records {
            assert_eq!(
                file_hash(&zeroize::Zeroizing::new(fs::read(path).unwrap())),
                hash
            );
        }
        assert!(provision_node(&temp.node(), id()).is_err());
        assert_eq!(immutable(&temp), before);
    }
}

#[test]
fn activation_allows_authenticated_ordinary_admission_suffix_without_literal_revision_equality() {
    let (temp, mut actor, p, input) = fixture();
    let other_id = AgentId::new("99999999-9999-4999-8999-999999999999").unwrap();
    let other_key =
        PublicKey::from_bytes(SigningKey::from_bytes(&[22; 32]).verifying_key().to_bytes());
    let offered = sign(
        Offer {
            header: Header {
                version: 1,
                context: original().context,
                owner_epoch: 1,
                checkpoint: 4,
            },
            invitation_id: InvitationId::new("99999999-9999-4999-8999-999999999999").unwrap(),
            agent_id: other_id,
            agent_key: other_key,
            token_hash: InvitationToken::from_bytes([44; 32]).unwrap().digest(),
            scopes: Scopes(vec![Scope::TelemetryRead]),
            issued_at: time(1000),
            expires_at: time(1600),
        },
        11,
    );
    actor.offer(offered.clone(), now().unwrap()).unwrap();
    let claim = Claim {
        header: offered.body.header.clone(),
        invitation_id: offered.body.invitation_id,
        offer_hash: offered.body.digest().unwrap(),
        agent_id: other_id,
        agent_key: other_key,
        capabilities: Capabilities {
            version: 1,
            scopes: Scopes(vec![Scope::TelemetryRead]),
        },
        nonce: Nonce::from_bytes([198; 16]),
        issued_at: time(1000),
    };
    actor
        .consume(
            &InvitationToken::from_bytes([44; 32]).unwrap(),
            sign(claim.clone(), 22),
            now().unwrap(),
        )
        .unwrap();
    let cert = Certificate {
        header: Header {
            checkpoint: 5,
            ..offered.body.header
        },
        agent_id: other_id,
        key_epoch: 1,
        agent_key: other_key,
        scopes: offered.body.scopes,
        capabilities: claim.capabilities.clone(),
        origin: CertificateOrigin::Enrollment {
            invitation_id: claim.invitation_id,
            claim_hash: claim.digest().unwrap(),
            key_fingerprint: other_key.fingerprint(),
        },
        not_before: time(1000),
        expires_at: time(1600),
    };
    actor
        .approve(sign(cert.clone(), 11), now().unwrap())
        .unwrap();
    let mut external = open(&temp);
    let revision = actor.snapshot().unwrap().revision();
    let mut calls = 0;
    let operation = Operation::TelemetryRead {
        source: ResourceId::new("66666666-6666-4666-8666-666666666666").unwrap(),
    };
    let lease = sign(
        Lease {
            header: cert.header.clone(),
            id: LeaseId::new("88888888-8888-4888-8888-888888888888").unwrap(),
            agent_id: other_id,
            key_epoch: 1,
            certificate_hash: cert.digest().unwrap(),
            operation: operation.clone(),
            issued_at: time(1000),
            expires_at: time(1300),
        },
        11,
    );
    let freshness = sign(
        Freshness {
            header: cert.header.clone(),
            issued_at: time(1000),
            expires_at: time(1300),
        },
        11,
    );
    let proof = sign(
        AgentRequest {
            header: cert.header,
            agent_id: other_id,
            key_epoch: 1,
            lease_hash: lease.body.digest().unwrap(),
            nonce: Nonce::from_bytes([199; 16]),
            issued_at: time(1000),
        },
        22,
    );
    let mut bundle = Some((lease, proof));
    let result = activate(
        &temp.node(),
        &mut actor,
        id(),
        &p.rotation_id,
        input,
        &mut || {
            calls += 1;
            if calls == 5 {
                let (lease, proof) = bundle.take().unwrap();
                external
                    .admit(lease, proof, Some(&freshness), &operation, now().unwrap())
                    .unwrap();
            }
            now()
        },
    )
    .unwrap();
    assert_eq!(result.selected_key_epoch, 2);
    assert_eq!(actor.snapshot().unwrap().revision(), revision + 1);
}
#[test]
fn activation_child() {
    if std::env::var_os("CC_ACTIVATION_ROOT").is_none() {
        return;
    }
    let root = owned_child_root();
    let input: LocalActivationInput =
        serde_json::from_slice(&fs::read(root.join("input.json")).unwrap()).unwrap();
    let p: PreparedNodeKeyPublic =
        serde_json::from_slice(&fs::read(root.join("prepared.json")).unwrap()).unwrap();
    let mut actor = Actor::new(
        SqliteJournalStore::open(
            root.join("db"),
            root.join("anchor"),
            original(),
            SqliteStorageBudget::default(),
        )
        .unwrap(),
    )
    .unwrap();
    let _ = activate(
        &root.join("node"),
        &mut actor,
        id(),
        &p.rotation_id,
        input,
        &mut now,
    );
}

#[test]
fn any_v3_state_with_missing_persistent_lock_denies_all_entrypoints_without_inode_recreation() {
    for state in ["complete", "partial", "unknown", "linked"] {
        let (temp, mut actor, p, input) = fixture();
        let pin = actor.snapshot().unwrap().owner().clone();
        if state == "complete" {
            activate(
                &temp.node(),
                &mut actor,
                id(),
                &p.rotation_id,
                input.clone(),
                &mut now,
            )
            .unwrap();
        } else {
            let path = temp.node().join(if state == "unknown" {
                "node-activation.unknown.v3"
            } else {
                INTENT
            });
            OpenOptions::new()
                .create_new(true)
                .write(true)
                .mode(0o600)
                .open(&path)
                .unwrap();
            if state == "linked" {
                fs::hard_link(path, temp.0.join("linked-intent")).unwrap();
            }
        }
        fs::remove_file(temp.node().join(".node-keys.lock")).unwrap();
        let before = immutable(&temp);
        let absent = || {
            assert!(
                !temp.node().join(".node-keys.lock").exists(),
                "{state} lock recreated"
            )
        };
        assert!(matches!(
            provision_node(&temp.node(), id()),
            Err(Error::Storage)
        ));
        absent();
        assert!(matches!(
            prepare_node_rotation(
                &temp.node(),
                id(),
                input.previous_certificate.agent_key.fingerprint()
            ),
            Err(Error::Storage)
        ));
        absent();
        let offer = sign(
            Offer {
                header: Header {
                    version: 1,
                    context: pin.context.clone(),
                    owner_epoch: 1,
                    checkpoint: 4,
                },
                invitation_id: InvitationId::new("99999999-9999-4999-8999-999999999999").unwrap(),
                agent_id: id(),
                agent_key: input.previous_certificate.agent_key,
                token_hash: InvitationToken::from_bytes([44; 32]).unwrap().digest(),
                scopes: Scopes(vec![Scope::TelemetryRead]),
                issued_at: time(1000),
                expires_at: time(1300),
            },
            11,
        );
        assert!(matches!(
            create_node_claim(&temp.node(), id(), &pin, offer, now().unwrap()),
            Err(Error::Storage)
        ));
        absent();
        let header = Header {
            version: 1,
            context: pin.context.clone(),
            owner_epoch: 1,
            checkpoint: 3,
        };
        let operation = Operation::TelemetryRead {
            source: ResourceId::new("66666666-6666-4666-8666-666666666666").unwrap(),
        };
        let binding = NodeRequestBinding {
            certificate: sign(input.committed_certificate.clone(), 11),
            lease: sign(
                Lease {
                    header: header.clone(),
                    id: LeaseId::new("88888888-8888-4888-8888-888888888888").unwrap(),
                    agent_id: id(),
                    key_epoch: 2,
                    certificate_hash: input.committed_certificate.digest().unwrap(),
                    operation: operation.clone(),
                    issued_at: time(1000),
                    expires_at: time(1300),
                },
                11,
            ),
            freshness: sign(
                Freshness {
                    header,
                    issued_at: time(1000),
                    expires_at: time(1300),
                },
                11,
            ),
            operation,
        };
        assert!(matches!(
            create_node_request(&temp.node(), id(), &pin, binding, now().unwrap()),
            Err(Error::Storage)
        ));
        absent();
        assert!(matches!(
            sign_prepared_node_rotation(
                &temp.node(),
                id(),
                &p.rotation_id,
                &pin,
                PreparedRotationInput {
                    previous_certificate: sign(input.previous_certificate.clone(), 11),
                    certificate: sign(input.committed_certificate.clone(), 11)
                },
                now().unwrap()
            ),
            Err(Error::Storage)
        ));
        absent();
        assert!(matches!(
            activate(
                &temp.node(),
                &mut actor,
                id(),
                &p.rotation_id,
                input,
                &mut now
            ),
            Err(Error::Storage)
        ));
        absent();
        assert_eq!(immutable(&temp), before);
    }
    // Legacy v1 lock bootstrap intentionally remains compatible.
    let temp = Temp::new();
    let public = provision_node(&temp.node(), id()).unwrap();
    fs::remove_file(temp.node().join(".node-keys.lock")).unwrap();
    assert_eq!(provision_node(&temp.node(), id()).unwrap(), public);
}
#[test]
fn activation_native_child_kill_releases_guard_and_only_complete_states_resume() {
    for (stage, name, resumable) in [
        ("after-create", INTENT, false),
        ("after-write", INTENT, true),
        ("after-dir-sync", INTENT, true),
        ("after-create", ACTIVE, false),
        ("after-write", ACTIVE, true),
        ("after-dir-sync", ACTIVE, true),
    ] {
        let (temp, actor, p, input) = fixture();
        drop(actor);
        let before = immutable(&temp);
        fs::write(
            temp.0.join("input.json"),
            serde_json::to_vec(&input).unwrap(),
        )
        .unwrap();
        fs::write(
            temp.0.join("prepared.json"),
            serde_json::to_vec(&p).unwrap(),
        )
        .unwrap();
        let mut child = Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "node_activation::linux::tests::activation_child",
                "--nocapture",
            ])
            .env("CC_ACTIVATION_ROOT", &temp.0)
            .env("CC_ACTIVATION_FAULT", format!("{stage}:{name}"))
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let started = Instant::now();
        while !temp.0.join("fault.ready").exists() {
            assert!(started.elapsed() < Duration::from_secs(5));
            assert!(child.try_wait().unwrap().is_none());
            std::thread::sleep(Duration::from_millis(5));
        }
        // The child owns exclusive guard while paused. No side effect from a
        // competing existing-key reader; native descriptor is released on kill.
        assert_eq!(provision_node(&temp.node(), id()), Err(Error::Conflict));
        child.kill().unwrap();
        child.wait().unwrap();
        let mut actor = open(&temp);
        let result = activate(
            &temp.node(),
            &mut actor,
            id(),
            &p.rotation_id,
            input,
            &mut now,
        );
        assert_eq!(result.is_ok(), resumable, "{stage}:{name}");
        assert_eq!(immutable(&temp), before);
        if resumable {
            assert_eq!(provision_node(&temp.node(), id()).unwrap().key_epoch, 2);
        } else {
            assert!(provision_node(&temp.node(), id()).is_err());
        }
    }
}

#[test]
fn activation_native_enospc_on_selected_publication_quarantines_without_entropy_or_repair() {
    if std::env::var("CC_NODE_ACTIVATION_ENOSPC").ok().as_deref() != Some("owned64m") {
        return;
    }
    // Explicit supported test budget fits this owned64MiB filesystem before
    // deliberate exhaustion. Production/default fixtures remain unchanged.
    let (temp, mut actor, p, input) = fixture_with_budget(SqliteStorageBudget {
        max_database_bytes: 1024 * 1024,
        max_wal_bytes: 4 * 1024 * 1024,
        free_reserve_bytes: 1024 * 1024,
        ..SqliteStorageBudget::default()
    });
    let before = immutable(&temp);
    FILL.with(|f| *f.borrow_mut() = Some(temp.0.join("filler")));
    assert!(activate(
        &temp.node(),
        &mut actor,
        id(),
        &p.rotation_id,
        input.clone(),
        &mut now
    )
    .is_err());
    assert_eq!(IO_ERROR.with(std::cell::Cell::get), Some(28));
    assert_eq!(immutable(&temp), before);
    fs::remove_file(temp.0.join("filler")).unwrap();
    assert!(activate(
        &temp.node(),
        &mut actor,
        id(),
        &p.rotation_id,
        input,
        &mut now
    )
    .is_err());
    assert!(provision_node(&temp.node(), id()).is_err());
    assert_eq!(immutable(&temp), before);
    println!("owned64m native activation ENOSPC: original148/pending212/v2 unchanged, partial active quarantined after space restored, no entropy/repair");
}

// Test-only reuse of the existing TelemetryRead activation fixture. This proves
// selection observation and initial-enrollment denial, NOT TunnelOpen rotation.
#[test]
fn enrollment_probe_reports_actual_activated_epoch_two_and_bootstrap_stays_denied() {
    let (temp,mut actor,prepared,input)=fixture();
    let old=crate::inspect_existing_node_identity(&temp.node(),id()).unwrap();
    assert_eq!(old.key_epoch,1);
    let immutable_before=immutable(&temp);
    let activated=activate(&temp.node(),&mut actor,id(),&prepared.rotation_id,input,&mut now).unwrap();
    assert_eq!(activated.selected_key_epoch,2);
    let actual=crate::inspect_existing_node_identity(&temp.node(),id()).unwrap();
    assert_eq!(actual.format,3);assert_eq!(actual.key_epoch,2);
    assert_eq!(actual.public_key,prepared.new_public_key);
    assert_eq!(actual.key_fingerprint,prepared.new_key_fingerprint);
    assert_ne!(actual.key_fingerprint,old.key_fingerprint);
    assert_eq!(immutable(&temp),immutable_before);
    assert_eq!(crate::inspect_existing_node_identity(&temp.node(),id()).unwrap(),actual);
    let snapshot=actor.snapshot().unwrap();let pin=snapshot.owner().clone();
    assert_eq!(snapshot.certificate(id()).unwrap().key_epoch,2);
    let offer=sign(Offer {header:Header {version:1,context:pin.context.clone(),owner_epoch:pin.epoch,
        checkpoint:pin.checkpoint.checked_add(1).unwrap()},
        invitation_id:InvitationId::new("99999999-9999-4999-8999-999999999999").unwrap(),
        agent_id:id(),agent_key:actual.public_key,
        token_hash:InvitationToken::from_bytes([33;32]).unwrap().digest(),
        scopes:Scopes(vec![Scope::TunnelOpen]),issued_at:time(1000),expires_at:time(1300)},11);
    // Exact error is activation-presence denial, not expired synthetic fixture time.
    assert!(matches!(crate::create_tunnel_node_claim(&temp.node(),id(),&pin,offer,now().unwrap()),
        Err(Error::WrongState)));
    assert_eq!(actor.snapshot().unwrap().revision(),snapshot.revision());
    assert_eq!(immutable(&temp),immutable_before);
}
