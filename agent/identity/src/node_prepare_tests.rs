use std::cell::{Cell, RefCell};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

#[test]
fn inspector_shared_guard_is_actually_readonly_existing_and_preserves_initial_denial() {
    let root=Temp::new();root.provision();
    let before=root.active();let inode=fs::metadata(root.node().join(LOCK)).unwrap().ino();
    let guard=inspection_read_lock(&root.node()).unwrap();
    let mut descriptor=&guard.file;
    assert_eq!(descriptor.write_all(b"must-not-write").unwrap_err().raw_os_error(),Some(9));
    assert_eq!(fs::metadata(root.node().join(LOCK)).unwrap().len(),0);
    drop(guard);assert_eq!(*root.active(),*before);
    assert_eq!(fs::metadata(root.node().join(LOCK)).unwrap().ino(),inode);
    fs::remove_file(root.node().join(LOCK)).unwrap();
    assert!(matches!(inspection_read_lock(&root.node()),Err(Error::Storage)));
    assert!(!root.node().join(LOCK).exists());
    fs::remove_file(root.node().join("node-identity.key")).unwrap();
    assert!(matches!(inspection_read_lock(&root.node()),Err(Error::Storage)));
    assert!(!root.node().join(LOCK).exists());
}

thread_local! {
    static ROTATION_NONCES:Cell<usize>=const {Cell::new(0)};
    static ENTROPY_CALLS:Cell<usize>=const {Cell::new(0)};
    static FAIL:RefCell<Option<(&'static str,&'static str,Error)>>=const {RefCell::new(None)};
    static FILL:RefCell<Option<PathBuf>>=const {RefCell::new(None)};
    static IO_ERROR:Cell<Option<i32>>=const {Cell::new(None)};
}
pub(super) fn rotation_nonce_point()->Result<()> {
    ROTATION_NONCES.with(|n|n.set(n.get()+1));fault("rotation-nonce",MATERIAL)
}
pub(super) fn io_failure(code: Option<i32>) {
    IO_ERROR.with(|v| v.set(code));
}
pub(super) fn fault(stage: &str, name: &str) -> Result<()> {
    if stage == "before-entropy" {
        ENTROPY_CALLS.with(|n| n.set(n.get() + 1));
    }
    if let Some(error) = FAIL.with(|f| {
        f.borrow()
            .filter(|f| f.0 == stage && f.1 == name)
            .map(|f| f.2)
    }) {
        return Err(error);
    }
    if stage == "after-dir-sync" && name == INTENT {
        if let Some(path) = FILL.with(|v| v.borrow_mut().take()) {
            let mut filler = OpenOptions::new()
                .create_new(true)
                .write(true)
                .mode(0o600)
                .open(path)
                .unwrap();
            let block = [0u8; 65536];
            let mut written = 0usize;
            let error = loop {
                match filler.write_all(&block) {
                    Ok(()) => {
                        written += block.len();
                        assert!(written <= 64 * 1024 * 1024);
                    }
                    Err(error) => break error,
                }
            };
            assert_eq!(error.raw_os_error(), Some(28));
        }
    }
    if std::env::var("CC_NODE_PREPARE_FAULT").ok().as_deref() == Some(&format!("{stage}:{name}")) {
        let root = owned_root();
        fs::write(root.join("fault.ready"), b"owned-node-prepare-fault").unwrap();
        let began = Instant::now();
        while began.elapsed() < Duration::from_secs(30) {
            std::thread::sleep(Duration::from_millis(10));
        }
        return Err(Error::Storage);
    }
    Ok(())
}
static NEXT: AtomicU64 = AtomicU64::new(0);
struct Temp(PathBuf);
impl Temp {
    fn new() -> Self {
        use std::os::unix::fs::DirBuilderExt;
        let root = std::env::temp_dir().join(format!(
            "cc-node-prepare-unit-{}-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::SeqCst),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::DirBuilder::new().mode(0o700).create(&root).unwrap();
        fs::write(root.join("owned.marker"), b"cc-node-prepare-owned-v1").unwrap();
        Self(root)
    }
    fn node(&self) -> PathBuf {
        self.0.join("node")
    }
    fn provision(&self) -> PublicNodeIdentity {
        provision_node(&self.node(), id()).unwrap()
    }
    fn active(&self) -> Zeroizing<Vec<u8>> {
        Zeroizing::new(fs::read(self.node().join("node-identity.key")).unwrap())
    }
}
impl Drop for Temp {
    fn drop(&mut self) {
        validate_root(&self.0);
        fs::remove_dir_all(&self.0).unwrap();
    }
}
fn validate_root(root: &Path) {
    assert_eq!(root.parent(), Some(std::env::temp_dir().as_path()));
    assert!(root
        .file_name()
        .unwrap()
        .to_str()
        .unwrap()
        .starts_with("cc-node-prepare-unit-"));
    assert!(fs::symlink_metadata(root).unwrap().is_dir());
    assert_eq!(
        fs::read(root.join("owned.marker")).unwrap(),
        b"cc-node-prepare-owned-v1"
    );
}
fn owned_root() -> PathBuf {
    let root = PathBuf::from(std::env::var_os("CC_NODE_PREPARE_ROOT").unwrap());
    validate_root(&root);
    root
}
pub(super) fn initial_directory_created() -> Result<()> {
    if std::env::var("CC_NODE_PREPARE_FAULT").ok().as_deref() != Some("init-race") {
        return Ok(());
    }
    let root = owned_root();
    fs::write(
        root.join("fault.ready"),
        b"creator paused before native lock",
    )
    .unwrap();
    let began = Instant::now();
    while !root.join("release-init").exists() {
        if began.elapsed() > Duration::from_secs(5) {
            return Err(Error::Storage);
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    Ok(())
}
fn id() -> AgentId {
    AgentId::new("44444444-4444-4444-8444-444444444444").unwrap()
}
fn entropy() -> usize {
    ENTROPY_CALLS.with(Cell::get)
}
fn fingerprint(root: &Temp) -> Hash {
    root.provision().key_fingerprint
}

#[test]
fn prepare_private_binding_idempotence_and_old_active_bytes_are_preserved() {
    let root = Temp::new();
    let active = root.provision();
    let bytes = root.active();
    let before = entropy();
    let prepared = prepare_node_rotation(&root.node(), id(), active.key_fingerprint).unwrap();
    assert_eq!(entropy(), before + 1);
    assert_ne!(prepared.new_public_key, active.public_key);
    assert_eq!(prepared.proposed_key_epoch, 2);
    assert_eq!(prepared.from_key_epoch, 1);
    assert_eq!(
        prepare_node_rotation(&root.node(), id(), active.key_fingerprint).unwrap(),
        prepared
    );
    assert_eq!(entropy(), before + 1);
    assert_eq!(*root.active(), *bytes);
    assert_eq!(root.provision(), active);
    for name in [LOCK, INTENT, MATERIAL, SELECTOR, "node-identity.key"] {
        let m = fs::symlink_metadata(root.node().join(name)).unwrap();
        assert_eq!(m.mode() & 0o777, 0o600);
        assert_eq!(m.nlink(), 1);
    }
    assert_eq!(fs::metadata(root.node()).unwrap().mode() & 0o777, 0o700);
    let text = serde_json::to_string(&prepared).unwrap();
    assert!(!text.contains("seed") && !text.contains("scope") && !text.contains("active"));
}
#[test]
fn prepare_checks_agent_fingerprint_and_bad_records_before_entropy() {
    let root = Temp::new();
    let active = root.provision();
    let bytes = root.active();
    let before = entropy();
    assert!(matches!(
        prepare_node_rotation(
            &root.node(),
            AgentId::new("99999999-9999-4999-8999-999999999999").unwrap(),
            active.key_fingerprint
        ),
        Err(Error::WrongContext)
    ));
    assert_eq!(
        prepare_node_rotation(&root.node(), id(), Hash::from_bytes([99; 32])),
        Err(Error::WrongClaim)
    );
    assert_eq!(entropy(), before);
    assert!(!root.node().join(INTENT).exists());
    assert_eq!(*root.active(), *bytes);
    for (stage, name) in [
        ("after-create", INTENT),
        ("after-file-sync", INTENT),
        ("after-dir-sync", INTENT),
        ("before-entropy", MATERIAL),
        ("after-create", MATERIAL),
        ("after-file-sync", MATERIAL),
        ("after-create", SELECTOR),
    ] {
        let root = Temp::new();
        let fp = fingerprint(&root);
        let bytes = root.active();
        FAIL.with(|f| *f.borrow_mut() = Some((stage, name, Error::Entropy)));
        assert_eq!(
            prepare_node_rotation(&root.node(), id(), fp),
            Err(Error::Entropy)
        );
        FAIL.with(|f| *f.borrow_mut() = None);
        let after = entropy();
        assert!(prepare_node_rotation(&root.node(), id(), fp).is_err());
        assert_eq!(entropy(), after);
        assert_eq!(*root.active(), *bytes);
        assert!(provision_node(&root.node(), id()).is_err());
    }
}
#[test]
fn prepare_rejects_unknown_partial_duplicate_linked_and_mismatched_sidecars() {
    use std::os::unix::fs::symlink;
    for case in [
        "missing",
        "phase",
        "duplicate",
        "intent-binding",
        "material-bit",
        "hardlink",
        "symlink",
        "permissions",
        "unknown",
    ] {
        let root = Temp::new();
        let fp = fingerprint(&root);
        let bytes = root.active();
        prepare_node_rotation(&root.node(), id(), fp).unwrap();
        let selector = root.node().join(SELECTOR);
        match case {
            "missing" => fs::remove_file(root.node().join(MATERIAL)).unwrap(),
            "phase" => {
                let mut v: serde_json::Value =
                    serde_json::from_slice(&fs::read(&selector).unwrap()).unwrap();
                v["phase"] = serde_json::json!("Active2");
                fs::write(&selector, serde_json::to_vec(&v).unwrap()).unwrap();
            }
            "duplicate" => {
                let text = fs::read_to_string(&selector).unwrap();
                fs::write(&selector, format!("{{\"format\":2,{}", &text[1..])).unwrap();
            }
            "intent-binding" => {
                let p = root.node().join(INTENT);
                let mut v: serde_json::Value =
                    serde_json::from_slice(&fs::read(&p).unwrap()).unwrap();
                v["proposedKeyEpoch"] = serde_json::json!(3);
                fs::write(p, serde_json::to_vec(&v).unwrap()).unwrap();
            }
            "material-bit" => {
                let p = root.node().join(MATERIAL);
                let mut b = Zeroizing::new(fs::read(&p).unwrap());
                b[116] ^= 1;
                fs::write(p, b.as_slice()).unwrap();
            }
            "hardlink" => fs::hard_link(&selector, root.0.join("linked")).unwrap(),
            "symlink" => {
                fs::rename(&selector, root.0.join("elsewhere")).unwrap();
                symlink(root.0.join("elsewhere"), &selector).unwrap();
            }
            "permissions" => {
                fs::set_permissions(&selector, fs::Permissions::from_mode(0o644)).unwrap()
            }
            "unknown" => {
                let p = root.node().join("node-rotation.future.v3");
                fs::write(&p, b"unknown").unwrap();
                fs::set_permissions(p, fs::Permissions::from_mode(0o600)).unwrap();
            }
            _ => unreachable!(),
        }
        let before = entropy();
        assert!(
            prepare_node_rotation(&root.node(), id(), fp).is_err(),
            "{case}"
        );
        assert_eq!(entropy(), before);
        assert!(provision_node(&root.node(), id()).is_err(), "{case}");
        assert_eq!(*root.active(), *bytes);
    }
}
#[test]
fn prepare_concurrent_cooperating_writers_publish_one_candidate() {
    let root = Temp::new();
    let fp = fingerprint(&root);
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));
    let mut jobs = Vec::new();
    for _ in 0..2 {
        let path = root.node();
        let barrier = barrier.clone();
        jobs.push(std::thread::spawn(move || {
            barrier.wait();
            let result = prepare_node_rotation(&path, id(), fp);
            (result, entropy())
        }));
    }
    let results: Vec<_> = jobs.into_iter().map(|j| j.join().unwrap()).collect();
    let prepared = prepare_node_rotation(&root.node(), id(), fp).unwrap();
    assert_eq!(results.iter().map(|v| v.1).sum::<usize>(), 1);
    for result in results {
        match result.0 {
            Ok(public) => assert_eq!(public, prepared),
            Err(error) => assert_eq!(error, Error::Conflict),
        }
    }
}
#[test]
fn fault_child() {
    if std::env::var_os("CC_NODE_PREPARE_ROOT").is_none() {
        return;
    }
    let root = owned_root();
    let node = root.join("node");
    let public = provision_node(&node, id()).unwrap();
    if std::env::var("CC_NODE_PREPARE_FAULT").ok().as_deref() == Some("init-race") {
        fs::write(
            root.join("initial-public.json"),
            serde_json::to_vec(&public).unwrap(),
        )
        .unwrap();
        return;
    }
    if std::env::var("CC_NODE_PREPARE_FAULT").ok().as_deref() == Some("hold-lock") {
        let _guard = lock(&node).unwrap();
        fs::write(root.join("fault.ready"), b"held").unwrap();
        std::thread::sleep(Duration::from_secs(30));
        return;
    }
    let _ = prepare_node_rotation(&node, id(), public.key_fingerprint);
}
fn run_child(root: &Temp, point: &str) -> std::process::Child {
    Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "node_prepare::linux::tests::fault_child",
            "--nocapture",
        ])
        .env("CC_NODE_PREPARE_ROOT", &root.0)
        .env("CC_NODE_PREPARE_FAULT", point)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap()
}
fn wait_ready(child: &mut std::process::Child, root: &Temp) {
    let began = Instant::now();
    while !root.0.join("fault.ready").exists() {
        if child.try_wait().unwrap().is_some() {
            panic!("owned fault child exited early");
        }
        if began.elapsed() > Duration::from_secs(5) {
            child.kill().unwrap();
            child.wait().unwrap();
            panic!("owned fault deadline");
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}
#[test]
fn prepare_owned_child_kills_keep_candidate_or_quarantine_without_regeneration() {
    for point in [
        format!("after-dir-sync:{INTENT}"),
        format!("after-dir-sync:{MATERIAL}"),
        format!("after-create:{SELECTOR}"),
        format!("after-file-sync:{SELECTOR}"),
        format!("after-readback:{SELECTOR}"),
    ] {
        let root = Temp::new();
        let fp = fingerprint(&root);
        let active = root.active();
        let mut child = run_child(&root, &point);
        wait_ready(&mut child, &root);
        child.kill().unwrap();
        assert!(!child.wait().unwrap().success());
        let before = entropy();
        let result = prepare_node_rotation(&root.node(), id(), fp);
        assert_eq!(entropy(), before);
        assert_eq!(*root.active(), *active);
        if point.starts_with("after-file-sync") || point.starts_with("after-readback") {
            let public = result.unwrap();
            assert_eq!(
                prepare_node_rotation(&root.node(), id(), fp).unwrap(),
                public
            );
        } else {
            assert!(result.is_err());
        }
    }
}
#[test]
fn prepare_native_lock_is_released_on_owned_process_exit_without_inode_recreation() {
    let root = Temp::new();
    let fp = fingerprint(&root);
    let inode = fs::metadata(root.node().join(LOCK)).unwrap().ino();
    let mut child = run_child(&root, "hold-lock");
    wait_ready(&mut child, &root);
    assert!(matches!(
        prepare_node_rotation(&root.node(), id(), fp),
        Err(Error::Conflict)
    ));
    child.kill().unwrap();
    child.wait().unwrap();
    prepare_node_rotation(&root.node(), id(), fp).unwrap();
    assert_eq!(fs::metadata(root.node().join(LOCK)).unwrap().ino(), inode);
}

struct OwnedChild(std::process::Child);
impl Drop for OwnedChild {
    fn drop(&mut self) {
        if self.0.try_wait().unwrap().is_none() {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
}
fn missing_leaf_inputs() -> (OwnerPin, Signed<Offer>, NodeRequestBinding) {
    let time = |n| Timestamp::new(n).unwrap();
    let context = Context {
        tenant: Tenant::LocalProfile {
            id: LocalProfileId::new("11111111-1111-4111-8111-111111111111").unwrap(),
        },
        deployment_id: DeploymentId::new("22222222-2222-4222-8222-222222222222").unwrap(),
    };
    let header = Header {
        version: 1,
        context: context.clone(),
        owner_epoch: 1,
        checkpoint: 1,
    };
    let key = PublicKey::from_bytes(SigningKey::from_bytes(&[22; 32]).verifying_key().to_bytes());
    let pin = OwnerPin {
        context,
        key: PublicKey::from_bytes(SigningKey::from_bytes(&[11; 32]).verifying_key().to_bytes()),
        epoch: 1,
        checkpoint: 0,
        expires_at: time(10000),
    };
    let offer = Signed {
        signature: SignatureBytes::from_bytes([0; 64]),
        body: Offer {
            header: header.clone(),
            invitation_id: InvitationId::new("33333333-3333-4333-8333-333333333333").unwrap(),
            agent_id: id(),
            agent_key: key,
            token_hash: Hash::from_bytes([33; 32]),
            scopes: Scopes(vec![Scope::TelemetryRead]),
            issued_at: time(1000),
            expires_at: time(1600),
        },
    };
    let certificate = Signed {
        signature: SignatureBytes::from_bytes([0; 64]),
        body: Certificate {
            header: header.clone(),
            agent_id: id(),
            key_epoch: 1,
            agent_key: key,
            scopes: Scopes(vec![Scope::TelemetryRead]),
            capabilities: Capabilities {
                version: 1,
                scopes: Scopes(vec![Scope::TelemetryRead]),
            },
            origin: CertificateOrigin::Enrollment {
                invitation_id: offer.body.invitation_id,
                claim_hash: Hash::from_bytes([44; 32]),
                key_fingerprint: key.fingerprint(),
            },
            not_before: time(1000),
            expires_at: time(1600),
        },
    };
    let operation = Operation::TelemetryRead {
        source: ResourceId::new("66666666-6666-4666-8666-666666666666").unwrap(),
    };
    let lease = Signed {
        signature: SignatureBytes::from_bytes([0; 64]),
        body: Lease {
            header: header.clone(),
            id: LeaseId::new("88888888-8888-4888-8888-888888888888").unwrap(),
            agent_id: id(),
            key_epoch: 1,
            certificate_hash: certificate.body.digest().unwrap(),
            operation: operation.clone(),
            issued_at: time(1000),
            expires_at: time(1300),
        },
    };
    let freshness = Signed {
        signature: SignatureBytes::from_bytes([0; 64]),
        body: Freshness {
            header,
            issued_at: time(1000),
            expires_at: time(1300),
        },
    };
    (
        pin,
        offer,
        NodeRequestBinding {
            certificate,
            lease,
            freshness,
            operation,
        },
    )
}
#[test]
fn prepare_initial_creator_is_not_blocked_by_existing_readers_of_missing_active_leaf() {
    let root = Temp::new();
    let mut child = OwnedChild(run_child(&root, "init-race"));
    wait_ready(&mut child.0, &root);
    assert!(!root.node().join("node-identity.key").exists());
    assert!(!root.node().join(LOCK).exists());
    assert!(matches!(
        provision_node(&root.node(), id()),
        Err(Error::Storage)
    ));
    assert!(matches!(
        prepare_node_rotation(&root.node(), id(), Hash::from_bytes([99; 32])),
        Err(Error::Storage)
    ));
    let (pin, offer, binding) = missing_leaf_inputs();
    let observed = Now(Timestamp::new(1000).unwrap());
    assert!(matches!(
        create_node_claim(&root.node(), id(), &pin, offer, observed),
        Err(Error::Storage)
    ));
    assert!(matches!(
        create_node_request(&root.node(), id(), &pin, binding, observed),
        Err(Error::Storage)
    ));
    // A paused creator has not acquired its lock yet. The missing-leaf readers
    // must not publish/take that lock at all, or they can make the creator lose.
    assert!(
        !root.node().join(LOCK).exists(),
        "missing-key reader acquired the initial creator's native lock"
    );
    fs::write(root.0.join("release-init"), b"resume owned creator").unwrap();
    let began = Instant::now();
    while child.0.try_wait().unwrap().is_none() {
        if began.elapsed() > Duration::from_secs(5) {
            panic!("owned initial creator deadline");
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(child.0.wait().unwrap().success());
    assert_eq!(root.active().len(), 148);
    root.provision();
}

// Run only in the explicitly owned 64MiB native tmpfs acceptance fixture.
#[test]
fn enospc_child() {
    if std::env::var("CC_NODE_PREPARE_ENOSPC").ok().as_deref() != Some("owned64m") {
        return;
    }
    let root = Temp::new();
    let fp = fingerprint(&root);
    let active = root.active();
    FILL.with(|v| *v.borrow_mut() = Some(root.0.join("filler")));
    let before = entropy();
    assert!(prepare_node_rotation(&root.node(), id(), fp).is_err());
    assert_eq!(IO_ERROR.with(Cell::get), Some(28));
    assert_eq!(entropy(), before + 1);
    assert_eq!(*root.active(), *active);
    fs::remove_file(root.0.join("filler")).unwrap();
    assert!(prepare_node_rotation(&root.node(), id(), fp).is_err());
    assert_eq!(entropy(), before + 1);
    assert_eq!(*root.active(), *active);
    println!("owned64m native ENOSPC: active bytes unchanged, durable intent quarantined, no replacement entropy after space restored");
}
