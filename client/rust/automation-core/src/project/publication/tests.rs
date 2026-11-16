//! Real filesystem kernel tests. No fabricated VerifiedDependencyPreview or
//! approval; actual source/archive/AppCore integration remains a separate gate.
use super::*;
use std::{fs, path::PathBuf};

struct Fixture {
    base: ProjectRoot,
    candidate: ProjectRoot,
    before: ProjectRevision,
    expected: ProjectRevision,
    profile: PathBuf,
    root: PathBuf,
    project: Uuid,
    operation: Uuid,
    _owner: Option<tempfile::TempDir>, // Last: handles drop before owned cleanup.
}
fn private_directory(path: &Path) {
    fs::create_dir_all(path).unwrap();
    #[cfg(windows)]
    private_acl::tests::protect_owned_fixture(path);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o700)).unwrap();
    }
}
fn held() -> OwnedSemaphorePermit {
    Arc::new(Semaphore::new(1)).try_acquire_owned().unwrap()
}
impl Fixture {
    fn new() -> Self {
        let owner = tempfile::Builder::new()
            .prefix("owned-publication-")
            .tempdir()
            .unwrap();
        let root = owner.path().to_owned();
        let project = Uuid::new_v4();
        let operation = Uuid::new_v4();
        let profile = root.join("profile");
        private_directory(&profile);
        private_directory(&profile.join("automation"));
        private_directory(&profile.join("automation/projects"));
        private_directory(
            &profile
                .join("automation/projects")
                .join(project.to_string()),
        );
        private_directory(&root.join("candidate"));
        fs::write(
            profile
                .join("automation/projects")
                .join(project.to_string())
                .join("playbook.yml"),
            b"---\n# unchanged original\n",
        )
        .unwrap();
        fs::write(
            root.join("candidate/playbook.yml"),
            b"---\n# reviewed new generation\n",
        )
        .unwrap();
        private_directory(&root.join("candidate/roles/fixture/empty"));
        fs::write(
            root.join("candidate/roles/fixture/data.bin"),
            [0, 255, 13, 10, 0],
        )
        .unwrap();
        // Opaque bytes here test exact copying only. Production requires the
        // existing verified preview to derive/validate the actual schemas.
        fs::write(
            root.join("candidate/.consolecrypt-dependencies.json"),
            b"synthetic exact lock bytes\n",
        )
        .unwrap();
        fs::write(
            root.join("candidate/.consolecrypt-dependency-receipts.json"),
            b"synthetic exact receipt bytes\n",
        )
        .unwrap();
        let mut fixture = Self::reopen(root, project, operation);
        fixture._owner = Some(owner);
        fixture
    }
    fn reopen(root: PathBuf, project: Uuid, operation: Uuid) -> Self {
        let profile = root.join("profile");
        let base = ProjectRoot::open(
            &profile
                .join("automation/projects")
                .join(project.to_string()),
        )
        .unwrap();
        let candidate = ProjectRoot::open(&root.join("candidate")).unwrap();
        let before = base.revision_for_publication().unwrap();
        let expected = candidate.revision_for_publication().unwrap();
        Self {
            base,
            candidate,
            before,
            expected,
            profile,
            root,
            project,
            operation,
            _owner: None,
        }
    }
    fn family(&self) -> PathBuf {
        self.profile
            .join("automation/project-revisions")
            .join(self.project.to_string())
    }
    fn facts(&self) -> Facts {
        Facts {
            project: self.project,
            operation: self.operation,
            catalog_revision: 7,
            catalog_row_sha256: "a".repeat(64),
            base: identity(&self.base.root).unwrap(),
            base_digest: self.before.digest.clone(),
            candidate_digest: self.expected.digest.clone(),
            review_digest: "b".repeat(64),
            selection_sha256: "c".repeat(64),
            archive_sha256: "d".repeat(64),
            tree_digest: "e".repeat(64),
            old_lock: None,
            old_receipt: None,
            candidate_lock: Some("f".repeat(64)),
            candidate_receipt: Some("1".repeat(64)),
        }
    }
    fn namespace(&self) -> namespace::ManagedNamespace {
        namespace::ManagedNamespace::open(&self.profile, self.project, &self.base, &self.candidate)
            .unwrap()
    }
    fn existing(&self) -> namespace::ManagedNamespace {
        namespace::ManagedNamespace::existing(
            &self.profile,
            self.project,
            &self.base,
            &self.candidate,
        )
        .unwrap()
    }
    fn check(&self, namespace: &namespace::ManagedNamespace) -> Result<()> {
        namespace.check_named_source(&self.base)?;
        if self.base.revision_for_publication()? != self.before
            || self.candidate.revision_for_publication()? != self.expected
        {
            return Err(AutomationError::FileChanged);
        }
        Ok(())
    }
    fn prepare(&self) -> Prepared {
        Prepared::prepare(
            self.namespace(),
            &self.candidate,
            &self.expected,
            self.facts(),
            held(),
            &CancellationToken::new(),
            |n| self.check(n),
            |_| Ok(()),
        )
        .unwrap()
    }
    fn unchanged(&self) {
        assert_eq!(self.base.revision_for_publication().unwrap(), self.before);
        assert_eq!(
            fs::read(
                self.profile
                    .join("automation/projects")
                    .join(self.project.to_string())
                    .join("playbook.yml")
            )
            .unwrap(),
            b"---\n# unchanged original\n"
        );
        assert!(
            !self
                .profile
                .join("automation/projects")
                .join(self.project.to_string())
                .join(".consolecrypt-dependencies.json")
                .exists()
        );
    }
}

#[test]
fn exact_binary_empty_lock_receipt_freeze_and_new_generation() {
    let fixture = Fixture::new();
    let prepared = fixture.prepare();
    let target = prepared.intent.target();
    assert!(fixture.family().join(READY).is_file());
    assert!(!fixture.family().join(&target).exists());
    let published = prepared
        .publish(&CancellationToken::new(), |n| fixture.check(n), |_| Ok(()))
        .unwrap();
    let copied = ProjectRoot {
        root: namespace::open_child(&published.namespace.root, &target).unwrap(),
        #[cfg(any(target_os = "linux", target_os = "macos"))]
        publication_boundary: true,
    };
    assert_eq!(copied.revision_for_publication().unwrap(), fixture.expected);
    for path in [
        "roles/fixture/data.bin",
        ".consolecrypt-dependencies.json",
        ".consolecrypt-dependency-receipts.json",
    ] {
        assert_eq!(
            fs::read(fixture.family().join(&target).join(path)).unwrap(),
            fs::read(fixture.root.join("candidate").join(path)).unwrap()
        );
    }
    assert!(
        fixture
            .family()
            .join(&target)
            .join("roles/fixture/empty")
            .is_dir()
    );
    fixture.unchanged();
    assert!(
        Prepared::prepare(
            fixture.namespace(),
            &fixture.candidate,
            &fixture.expected,
            fixture.facts(),
            held(),
            &CancellationToken::new(),
            |n| fixture.check(n),
            |_| Ok(())
        )
        .is_err()
    );
}

#[test]
fn existing_destination_is_preserved_and_recovery_is_not_a_retry() {
    let fixture = Fixture::new();
    let prepared = fixture.prepare();
    let target = prepared.intent.target();
    private_directory(&fixture.family().join(&target));
    fs::write(
        fixture.family().join(&target).join("keep"),
        b"concurrent owner",
    )
    .unwrap();
    assert!(
        prepared
            .publish(&CancellationToken::new(), |n| fixture.check(n), |_| Ok(()))
            .is_err()
    );
    assert_eq!(
        fs::read(fixture.family().join(&target).join("keep")).unwrap(),
        b"concurrent owner"
    );
    assert!(fixture.family().join(READY).is_file());
    assert!(!fixture.family().join(PUBLISHED).exists());
    fixture.unchanged();
}

#[test]
fn destination_created_after_last_tree_check_is_not_replaced_by_os_rename() {
    let fixture = Fixture::new();
    let prepared = fixture.prepare();
    let target = prepared.intent.target();
    let stage = prepared.intent.stage();
    let result = prepared.publish(
        &CancellationToken::new(),
        |n| fixture.check(n),
        |phase| {
            if phase == Phase::BeforeRename {
                private_directory(&fixture.family().join(&target));
                fs::write(
                    fixture.family().join(&target).join("keep"),
                    b"racing destination",
                )
                .unwrap();
            }
            Ok(())
        },
    );
    assert!(result.is_err());
    assert_eq!(
        fs::read(fixture.family().join(&target).join("keep")).unwrap(),
        b"racing destination"
    );
    assert!(fixture.family().join(stage).is_dir());
    assert!(!fixture.family().join(PUBLISHED).exists());
    fixture.unchanged();
}

#[test]
fn unknown_entry_denies_without_creating_intent() {
    let fixture = Fixture::new();
    let namespace = fixture.namespace();
    fs::write(fixture.family().join("unknown"), b"preserve").unwrap();
    assert!(
        Prepared::prepare(
            namespace,
            &fixture.candidate,
            &fixture.expected,
            fixture.facts(),
            held(),
            &CancellationToken::new(),
            |n| fixture.check(n),
            |_| Ok(())
        )
        .is_err()
    );
    assert!(!fixture.family().join(INTENT).exists());
    fixture.unchanged();
}

#[test]
fn exclusive_fixed_intent_serializes_competing_first_installs() {
    let fixture = Fixture::new();
    let namespace = fixture.namespace();
    let parent = namespace.root.try_clone().unwrap();
    let intent = Intent {
        version: 1,
        transaction: Uuid::new_v4(),
        parent: identity(&parent).unwrap(),
        facts: fixture.facts(),
    };
    let first = encode(&intent).unwrap();
    write_new(&parent, INTENT, &intent).unwrap();
    let mut other = intent;
    other.transaction = Uuid::new_v4();
    assert!(write_new(&parent, INTENT, &other).is_err());
    assert_eq!(read_small(&parent, INTENT).unwrap(), first);
    fixture.unchanged();
}

#[test]
fn excluded_entry_inserted_after_freeze_never_reaches_ready() {
    let fixture = Fixture::new();
    let result = Prepared::prepare(
        fixture.namespace(),
        &fixture.candidate,
        &fixture.expected,
        fixture.facts(),
        held(),
        &CancellationToken::new(),
        |n| fixture.check(n),
        |phase| {
            if phase == Phase::Frozen {
                fs::write(
                    fixture.root.join("candidate/.git"),
                    b"omitted data must not vanish",
                )
                .unwrap();
            }
            Ok(())
        },
    );
    assert!(result.is_err());
    assert!(!fixture.family().join(READY).exists());
    assert_eq!(
        fs::read(fixture.root.join("candidate/.git")).unwrap(),
        b"omitted data must not vanish"
    );
    fixture.unchanged();
}

#[test]
fn changed_base_and_cancel_before_rename_preserve_both_trees() {
    let fixture = Fixture::new();
    let prepared = fixture.prepare();
    let stage = prepared.intent.stage();
    let target = prepared.intent.target();
    let cancel = CancellationToken::new();
    cancel.cancel();
    assert!(
        prepared
            .publish(&cancel, |n| fixture.check(n), |_| Ok(()))
            .is_err()
    );
    assert!(fixture.family().join(stage).is_dir());
    assert!(!fixture.family().join(target).exists());
    fixture.unchanged();
    let fixture = Fixture::new();
    let prepared = fixture.prepare();
    let target = prepared.intent.target();
    fs::write(
        fixture
            .profile
            .join("automation/projects")
            .join(fixture.project.to_string())
            .join("playbook.yml"),
        b"new user edit\n",
    )
    .unwrap();
    assert!(
        prepared
            .publish(&CancellationToken::new(), |n| fixture.check(n), |_| Ok(()))
            .is_err()
    );
    assert!(!fixture.family().join(target).exists());
    assert_eq!(
        fs::read(
            fixture
                .profile
                .join("automation/projects")
                .join(fixture.project.to_string())
                .join("playbook.yml")
        )
        .unwrap(),
        b"new user edit\n"
    );
}

#[test]
fn cancel_after_rename_retains_observed_published_tree_without_install_claim() {
    let fixture = Fixture::new();
    let prepared = fixture.prepare();
    let target = prepared.intent.target();
    let cancel = CancellationToken::new();
    assert!(
        prepared
            .publish(
                &cancel,
                |n| fixture.check(n),
                |phase| {
                    if phase == Phase::Renamed {
                        cancel.cancel();
                    }
                    Ok(())
                }
            )
            .is_err()
    );
    assert!(fixture.family().join(target).is_dir());
    assert!(fixture.family().join(PUBLISHED).is_file());
    fixture.unchanged();
}

#[test]
fn journal_schema_duplicate_budget_and_foreign_version_fail_closed() {
    assert!(parsed::<Marker>(br#"{"version":1,"version":1,"intent_sha256":"x","stage":{"platform":"unix","device":1,"inode":1}}"#).is_err());
    assert!(parsed::<Intent>(&vec![b' '; MAX_MARKER + 1]).is_err());
    let fixture = Fixture::new();
    let prepared = fixture.prepare();
    let transaction = prepared.intent.transaction;
    drop(prepared);
    let intent_path = fixture.family().join(INTENT);
    let mut intent: Intent = parsed(&fs::read(&intent_path).unwrap()).unwrap();
    intent.version = 2;
    fs::write(&intent_path, encode(&intent).unwrap()).unwrap();
    assert!(
        Prepared::resume(
            fixture.existing(),
            &fixture.expected,
            fixture.facts(),
            transaction,
            held(),
            &CancellationToken::new(),
            |n| fixture.check(n)
        )
        .is_err()
    );
    fixture.unchanged();
}

#[test]
fn recovery_requires_exact_original_context_and_stage_identity() {
    let fixture = Fixture::new();
    let prepared = fixture.prepare();
    let transaction = prepared.intent.transaction;
    let stage = prepared.intent.stage();
    drop(prepared);
    let mut changed = fixture.facts();
    changed.catalog_revision += 1;
    assert!(
        Prepared::resume(
            fixture.existing(),
            &fixture.expected,
            changed,
            transaction,
            held(),
            &CancellationToken::new(),
            |n| fixture.check(n)
        )
        .is_err()
    );
    let other = Fixture::new();
    // Same bytes are insufficient: a different directory must not inherit the
    // Ready marker's identity. Rename only the NEW stage, never original roots.
    fs::rename(
        fixture.family().join(&stage),
        fixture.family().join("preserved-old-stage"),
    )
    .unwrap();
    private_directory(&fixture.family().join(&stage));
    let replacement = ProjectRoot::open(&fixture.family().join(&stage)).unwrap();
    other
        .candidate
        .freeze_to(&other.expected, &replacement, &CancellationToken::new())
        .unwrap();
    drop(replacement);
    // Unknown preserved sibling would independently deny; move it to our OWN
    // root before checking the specific identity fence (never delete a tree).
    fs::rename(
        fixture.family().join("preserved-old-stage"),
        fixture.root.join("preserved-old-stage"),
    )
    .unwrap();
    assert!(
        Prepared::resume(
            fixture.existing(),
            &fixture.expected,
            fixture.facts(),
            transaction,
            held(),
            &CancellationToken::new(),
            |n| fixture.check(n)
        )
        .is_err()
    );
    fixture.unchanged();
}

#[test]
fn namespace_inside_candidate_and_managed_path_mismatch_are_denied_before_write() {
    let fixture = Fixture::new();
    assert!(
        namespace::ManagedNamespace::open(
            &fixture.root.join("candidate"),
            fixture.project,
            &fixture.base,
            &fixture.candidate
        )
        .is_err()
    );
    assert!(!fixture.root.join("candidate/automation").exists());
    let foreign = Fixture::new();
    assert!(
        namespace::ManagedNamespace::open(
            &fixture.profile,
            fixture.project,
            &foreign.base,
            &fixture.candidate
        )
        .is_err()
    );
    assert!(
        !fixture
            .profile
            .join("automation/project-revisions")
            .exists()
    );
    fixture.unchanged();
}

#[test]
fn directory_git_future_kinds_and_nonportable_base_names_are_explicitly_unsupported() {
    for kind in ["directory", "git", "Managed", "future", ""] {
        assert!(matches!(
            managed_only(kind),
            Err(AutomationError::PublicationUnsupported(_))
        ));
    }
    managed_only("managed").unwrap();
    let fixture = Fixture::new();
    fs::write(
        fixture.root.join("candidate/данные.yml"),
        b"existing data preserved\n",
    )
    .unwrap();
    let actual = fixture.candidate.revision_for_publication().unwrap();
    assert!(matches!(
        supported_tree(&actual),
        Err(AutomationError::PublicationUnsupported(_))
    ));
    assert!(
        !fixture
            .profile
            .join("automation/project-revisions")
            .exists()
    );
    assert_eq!(
        fs::read(fixture.root.join("candidate/данные.yml")).unwrap(),
        b"existing data preserved\n"
    );
    fixture.unchanged();
}

#[cfg(unix)]
#[test]
fn symlink_namespace_ancestor_and_retargeted_parent_are_denied() {
    use std::os::unix::fs::symlink;
    let fixture = Fixture::new();
    let external = fixture.root.join("external");
    private_directory(&external);
    symlink(
        &external,
        fixture.profile.join("automation/project-revisions"),
    )
    .unwrap();
    assert!(
        namespace::ManagedNamespace::open(
            &fixture.profile,
            fixture.project,
            &fixture.base,
            &fixture.candidate
        )
        .is_err()
    );
    assert!(fs::read_dir(&external).unwrap().next().is_none());
    fixture.unchanged();
    let fixture = Fixture::new();
    let prepared = fixture.prepare();
    fs::rename(fixture.family(), fixture.root.join("preserved-family")).unwrap();
    symlink(fixture.root.join("preserved-family"), fixture.family()).unwrap();
    assert!(
        prepared
            .publish(&CancellationToken::new(), |n| fixture.check(n), |_| Ok(()))
            .is_err()
    );
    fixture.unchanged();
}

#[cfg(windows)]
#[test]
fn held_namespace_prevents_parent_rename_and_destination_junction_is_preserved() {
    let fixture = Fixture::new();
    let prepared = fixture.prepare();
    let target = prepared.intent.target();
    let error = fs::rename(fixture.family(), fixture.root.join("must-not-move")).unwrap_err();
    assert!(matches!(error.raw_os_error(), Some(5 | 32)));
    // cmd mklink parses retained forward slashes as switches. Normalize
    // only this OWN fixture argument to native Windows separators.
    let junction: PathBuf = fixture.family().join(&target).components().collect();
    let owned = fixture.root.join("owned-junction-target");
    private_directory(&owned);
    fs::write(owned.join("keep"), b"owned concurrent directory").unwrap();
    let result = std::process::Command::new("cmd")
        .args(["/d", "/c", "mklink", "/J"])
        .arg(&junction)
        .arg(&owned)
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "owned junction fixture requires no admin: status={:?}; stdout={}; stderr={}",
        result.status,
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
    let outcome = prepared.publish(&CancellationToken::new(), |n| fixture.check(n), |_| Ok(()));
    assert!(outcome.is_err());
    assert_eq!(
        fs::read(owned.join("keep")).unwrap(),
        b"owned concurrent directory"
    );
    fs::remove_dir(&junction).unwrap(); // Junction itself ONLY, no recursion.
    assert!(owned.join("keep").is_file());
    fixture.unchanged();
}

#[test]
fn process_wide_two_permits_are_held_by_detached_workers() {
    // Genuine framed-publication fixtures share this real process-wide budget.
    // Keep the capacity assertion isolated from unrelated parallel test work.
    let _serial = crate::dependency_transfer::DEPENDENCY_TEST_SERIAL.blocking_lock();
    let first = permit().unwrap();
    let second = permit().unwrap();
    assert!(matches!(permit(), Err(AutomationError::QueueFull)));
    let (release_tx, release_rx) = std::sync::mpsc::channel();
    let worker = std::thread::spawn(move || {
        let _held = first;
        release_rx
            .recv_timeout(std::time::Duration::from_secs(5))
            .unwrap();
    });
    // A dropped caller has not stopped its blocking worker or freed admission.
    assert!(matches!(permit(), Err(AutomationError::QueueFull)));
    release_tx.send(()).unwrap();
    worker.join().unwrap();
    let next = permit().unwrap();
    drop((next, second));
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ChildControl {
    version: u32,
    owner: Uuid,
    project: Uuid,
    operation: Uuid,
    phase: u8,
}
const CHILD_ENV: &str = "CC_OWNED_PUBLICATION_CHILD";
#[test]
#[ignore = "child fixture ONLY; parent supplies its exact owned root/UUID and exits at a journal phase"]
fn crash_child() {
    let root = PathBuf::from(std::env::var_os(CHILD_ENV).expect("owned coordinator only"));
    assert!(
        root.is_absolute()
            && root
                .file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with("owned-publication-")
            && root.as_os_str().len() < 4096
    );
    let raw = fs::read(root.join("owned-child.json")).unwrap();
    assert!(raw.len() <= 1024);
    let control: ChildControl = parsed(&raw).unwrap();
    assert_eq!(control.version, 1);
    assert!(!control.owner.is_nil());
    assert!(control.phase <= 4);
    assert_eq!(
        std::env::var("CC_OWNED_PUBLICATION_NONCE").unwrap(),
        control.owner.to_string()
    );
    let fixture = Fixture::reopen(root, control.project, control.operation);
    let stop = |phase: Phase| -> Result<()> {
        if phase as u8 == control.phase {
            std::process::exit(37)
        }
        Ok(())
    };
    let prepared = Prepared::prepare(
        fixture.namespace(),
        &fixture.candidate,
        &fixture.expected,
        fixture.facts(),
        held(),
        &CancellationToken::new(),
        |n| fixture.check(n),
        stop,
    )
    .unwrap();
    let _published = prepared
        .publish(&CancellationToken::new(), |n| fixture.check(n), stop)
        .unwrap();
    panic!("requested crash phase was not reached");
}

#[test]
fn real_process_crash_five_phases_preserve_source_and_require_explicit_recovery() {
    for phase in 0..=4 {
        let fixture = Fixture::new();
        let control = ChildControl {
            version: 1,
            owner: Uuid::new_v4(),
            project: fixture.project,
            operation: fixture.operation,
            phase,
        };
        fs::write(
            fixture.root.join("owned-child.json"),
            serde_json::to_vec(&control).unwrap(),
        )
        .unwrap();
        let mut child = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "project::publication::tests::crash_child",
                "--ignored",
                "--nocapture",
            ])
            .env(CHILD_ENV, &fixture.root)
            .env("CC_OWNED_PUBLICATION_NONCE", control.owner.to_string())
            .current_dir(&fixture.root)
            .spawn()
            .unwrap();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
        let status = loop {
            if let Some(status) = child.try_wait().unwrap() {
                break status;
            }
            if std::time::Instant::now() >= deadline {
                let _ = child.kill();
                let _ = child.wait();
                panic!("owned publication child exceeded20s at phase {phase}");
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        };
        assert_eq!(
            status.code(),
            Some(37),
            "actual process exit at phase {phase}"
        );
        fixture.unchanged();
        let namespace = fixture.existing();
        let intent: Intent = parsed(&read_small(&namespace.root, INTENT).unwrap()).unwrap();
        let observation = observe(&namespace, &intent, &fixture.expected).unwrap();
        assert_eq!(
            observation,
            match phase {
                0 => RecoveryObservation::IntentOnly,
                1 => RecoveryObservation::Partial,
                2 => RecoveryObservation::Ready,
                _ => RecoveryObservation::Published,
            }
        );
        assert!(
            !namespace
                .root
                .entries()
                .unwrap()
                .collect::<std::io::Result<Vec<_>>>()
                .unwrap()
                .is_empty()
        );
        let result = Prepared::resume(
            namespace,
            &fixture.expected,
            fixture.facts(),
            intent.transaction,
            held(),
            &CancellationToken::new(),
            |n| fixture.check(n),
        );
        match (phase, result) {
            (0 | 1, Err(_)) => {
                assert!(!fixture.family().join(PUBLISHED).exists());
            }
            (2, Ok(Resumed::Ready(prepared))) => {
                assert!(!fixture.family().join(prepared.intent.target()).exists());
                let _published = prepared
                    .publish(&CancellationToken::new(), |n| fixture.check(n), |_| Ok(()))
                    .unwrap();
            }
            (3 | 4, Ok(Resumed::Published(published))) => {
                assert!(fixture.family().join(published.intent.target()).is_dir());
                assert!(fixture.family().join(PUBLISHED).is_file());
            }
            _ => panic!("wrong crash/recovery disposition at {phase}"),
        }
        fixture.unchanged();
    }
}

#[test]
fn publication_base_admission_is_readonly_with_missing_or_existing_private_family() {
    let f = Fixture::new();
    let family = f.profile.join("automation").join("project-revisions");
    let before = f.base.revision().unwrap();
    assert!(!family.exists());
    crate::check_dependency_publication_base(&f.profile, f.project, &f.base).unwrap();
    assert!(!family.exists(), "observation cannot create the namespace");
    private_directory(&family);
    private_directory(&family.join(f.project.to_string()));
    crate::check_dependency_publication_base(&f.profile, f.project, &f.base).unwrap();
    assert_eq!(f.base.revision().unwrap(), before);
    assert_eq!(fs::read_dir(family.join(f.project.to_string())).unwrap().count(), 0);
}
#[test]
fn publication_base_admission_rejects_foreign_root_and_excluded_entries_before_any_family_write() {
    let f = Fixture::new();
    let family = f.profile.join("automation").join("project-revisions");
    assert!(crate::check_dependency_publication_base(&f.profile, f.project, &f.candidate).is_err());
    private_directory(&f.root.join("profile/automation/projects").join(f.project.to_string()).join("nested/.git"));
    assert!(matches!(crate::check_dependency_publication_base(&f.profile, f.project, &f.base), Err(AutomationError::PublicationUnsupported(_))));
    assert!(!family.exists());
}
#[test]
fn publication_base_admission_never_adopts_existing_regular_file_as_family() {
    let f = Fixture::new();
    let family = f.profile.join("automation").join("project-revisions");
    fs::write(&family, b"owned preexisting marker").unwrap();
    assert!(crate::check_dependency_publication_base(&f.profile, f.project, &f.base).is_err());
    assert_eq!(fs::read(&family).unwrap(), b"owned preexisting marker");
}
