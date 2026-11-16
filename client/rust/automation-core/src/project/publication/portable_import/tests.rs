//! Real AEAD package/actual filesystem kernel, no manufactured package/approval.
//! Catalog ACKs below are DATA fixtures, not claims of real Native SQL commits.
use super::*;
use crate::dependency_publication::CatalogPublicationAck;
use crate::portable_package::{decrypt_package, encrypt_selection};
use cc_crypto_core::SecretString;
use std::fs;

struct Fixture {
    profile: PathBuf,
    encrypted: Vec<u8>,
    _owner: tempfile::TempDir,
}
impl Fixture {
    fn new() -> Self {
        let owner = tempfile::Builder::new()
            .prefix("owned-publication-portable-")
            .tempdir()
            .unwrap();
        let profile = owner.path().join("profile");
        fs::create_dir(&profile).unwrap();
        #[cfg(windows)]
        super::super::private_acl::tests::protect_owned_fixture(&profile);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&profile, fs::Permissions::from_mode(0o700)).unwrap();
        }
        let source = owner.path().join("source");
        fs::create_dir(&source).unwrap();
        let root = ProjectRoot::open(&source).unwrap();
        root.write_file(
            "playbook.yml",
            None,
            "---\n# synthetic reviewed bytes only\n",
        )
        .unwrap();
        let revision = root.revision().unwrap();
        let encrypted = encrypt_selection(
            &root,
            &revision,
            &["playbook.yml".into()],
            Some("playbook.yml".into()),
            &SecretString::from("synthetic portable fixture password"),
            &CancellationToken::new(),
        )
        .unwrap();
        drop(root);
        Self {
            profile,
            encrypted,
            _owner: owner,
        }
    }
    fn package(&self) -> PortableImport {
        decrypt_package(
            &self.encrypted,
            &SecretString::from("synthetic portable fixture password"),
            &CancellationToken::new(),
        )
        .unwrap()
    }
    fn prepare(&self, project: Uuid, operation: Uuid, transaction: Uuid) -> PreparedPortableImport {
        PreparedPortableImport::prepare(
            &self.profile,
            project,
            operation,
            transaction,
            &"a".repeat(64),
            self.package(),
            &CancellationToken::new(),
        )
        .unwrap()
    }
    fn family(&self, project: Uuid) -> PathBuf {
        self.profile
            .join("automation")
            .join("project-revisions")
            .join(project.to_string())
    }
}

#[test]
fn genuine_recovery_uses_exact_sql_identity_and_never_recopies_or_reselects_ready() {
    let _publication = crate::dependency_transfer::DEPENDENCY_TEST_SERIAL.blocking_lock();
    let _package = crate::portable_package::test_serial();
    let f = Fixture::new();
    let project = Uuid::new_v4();
    let operation = Uuid::new_v4();
    let transaction = Uuid::new_v4();
    let prepared = f.prepare(project, operation, transaction);
    let fingerprint = prepared.fingerprint().unwrap();
    drop(prepared); // process-boundary analogue: no live prepared owner reused
    let ack2 = CatalogPublicationAck::new(project, transaction, 2, &"b".repeat(64)).unwrap();
    let family = f.family(project);
    let before_stage = fs::read(
        family
            .join(format!("stage-{transaction}"))
            .join("playbook.yml"),
    )
    .unwrap();
    assert!(recover(
        &f.profile,
        project,
        operation,
        transaction,
        &"a".repeat(64),
        &"0".repeat(64),
        f.package(),
        &ack2,
        &CancellationToken::new()
    )
    .is_err());
    assert_eq!(
        fs::read(
            family
                .join(format!("stage-{transaction}"))
                .join("playbook.yml")
        )
        .unwrap(),
        before_stage
    );
    assert!(!family.join(format!("generation-{transaction}")).exists());
    let published = recover(
        &f.profile,
        project,
        operation,
        transaction,
        &"a".repeat(64),
        &fingerprint,
        f.package(),
        &ack2,
        &CancellationToken::new(),
    )
    .unwrap();
    let target_id = identity(&published.target.root).unwrap();
    let ack3 = CatalogPublicationAck::new(project, transaction, 3, &"c".repeat(64)).unwrap();
    published.record_catalog_ack(&ack3).unwrap();
    published.record_complete(&ack3).unwrap();
    drop(published);
    let entries_before: std::collections::BTreeSet<_> = fs::read_dir(&family)
        .unwrap()
        .map(|e| e.unwrap().file_name())
        .collect();
    let selected = recover(
        &f.profile,
        project,
        operation,
        transaction,
        &"a".repeat(64),
        &fingerprint,
        f.package(),
        &ack3,
        &CancellationToken::new(),
    )
    .unwrap();
    assert_eq!(identity(&selected.target.root).unwrap(), target_id);
    selected.record_catalog_ack(&ack3).unwrap();
    selected.record_complete(&ack3).unwrap();
    let entries_after: std::collections::BTreeSet<_> = fs::read_dir(&family)
        .unwrap()
        .map(|e| e.unwrap().file_name())
        .collect();
    assert_eq!(entries_before, entries_after);
    assert!(!family.join(format!("stage-{transaction}")).exists());
}

#[test]
fn renamed_target_missing_observation_recovers_only_matching_actual_fingerprint() {
    let _publication = crate::dependency_transfer::DEPENDENCY_TEST_SERIAL.blocking_lock();
    let _package = crate::portable_package::test_serial();
    let f = Fixture::new();
    let project = Uuid::new_v4();
    let operation = Uuid::new_v4();
    let transaction = Uuid::new_v4();
    let prepared = f.prepare(project, operation, transaction);
    let fingerprint = prepared.fingerprint().unwrap();
    let ack2 = CatalogPublicationAck::new(project, transaction, 2, &"b".repeat(64)).unwrap();
    let published = prepared.publish(&ack2, &CancellationToken::new()).unwrap();
    let target_id = identity(&published.target.root).unwrap();
    drop(published);
    // An owned fixture removes ONLY the observation to model rename→journal
    // interruption. This is not a claim of an OS process crash or power cut.
    fs::remove_file(f.family(project).join(PUBLISHED)).unwrap();
    let recovered = recover(
        &f.profile,
        project,
        operation,
        transaction,
        &"a".repeat(64),
        &fingerprint,
        f.package(),
        &ack2,
        &CancellationToken::new(),
    )
    .unwrap();
    assert_eq!(identity(&recovered.target.root).unwrap(), target_id);
    assert_eq!(recovered.fingerprint().unwrap(), fingerprint);
    assert!(!f
        .family(project)
        .join(format!("stage-{transaction}"))
        .exists());
}
#[test]
fn actual_authenticated_package_stages_then_requires_prepared_ack_before_no_replace_and_exact_final_audit(
) {
    let _publication = crate::dependency_transfer::DEPENDENCY_TEST_SERIAL.blocking_lock();
    let _package = crate::portable_package::test_serial();
    let f = Fixture::new();
    let project = Uuid::new_v4();
    let operation = Uuid::new_v4();
    let transaction = Uuid::new_v4();
    let package = f.package();
    assert_eq!(
        package.encrypted_sha256(),
        Some(hash(&f.encrypted).as_str())
    );
    let expected = package.summary().portable_digest;
    drop(package);
    admit(&f.profile, project).unwrap();
    assert!(
        !f.profile.join("automation").exists(),
        "read-only admission must never create namespace"
    );
    let prepared = f.prepare(project, operation, transaction);
    let fingerprint = prepared.fingerprint().unwrap();
    let family = f.family(project);
    assert!(!family.join(format!("generation-{transaction}")).exists());
    assert_eq!(
        fs::read(family.join(format!("stage-{transaction}/playbook.yml"))).unwrap(),
        b"---\n# synthetic reviewed bytes only\n"
    );
    let published = prepared
        .publish(
            &CatalogPublicationAck::new(project, transaction, 2, &"b".repeat(64)).unwrap(),
            &CancellationToken::new(),
        )
        .unwrap();
    assert_eq!(published.fingerprint().unwrap(), fingerprint);
    assert_eq!(published.digest(), expected);
    published.recheck().unwrap();
    assert!(family.join(format!("generation-{transaction}")).exists());
    assert!(!family.join(format!("stage-{transaction}")).exists());
    let selected = CatalogPublicationAck::new(project, transaction, 3, &"c".repeat(64)).unwrap();
    published.record_catalog_ack(&selected).unwrap();
    published.record_complete(&selected).unwrap();
    let bytes = fs::read(family.join(COMPLETE)).unwrap();
    published.record_complete(&selected).unwrap();
    assert_eq!(fs::read(family.join(COMPLETE)).unwrap(), bytes);
    let changed = CatalogPublicationAck::new(project, transaction, 3, &"d".repeat(64)).unwrap();
    assert!(published.record_complete(&changed).is_err());
    assert_eq!(fs::read(family.join(COMPLETE)).unwrap(), bytes);
    drop(published);
}
#[test]
fn wrong_password_or_tamper_never_constructs_witness_or_creates_import_namespace() {
    let _package = crate::portable_package::test_serial();
    let f = Fixture::new();
    assert!(decrypt_package(
        &f.encrypted,
        &SecretString::from("synthetic wrong password"),
        &CancellationToken::new()
    )
    .is_err());
    let mut tampered = f.encrypted.clone();
    let last = tampered.len() - 1;
    tampered[last] ^= 1;
    assert!(decrypt_package(
        &tampered,
        &SecretString::from("synthetic portable fixture password"),
        &CancellationToken::new()
    )
    .is_err());
    assert!(!f.profile.join("automation").exists());
}
#[test]
fn existing_fresh_family_is_never_adopted_or_overwritten_and_cancelled_preparation_retains_no_claim_of_installation(
) {
    let _publication = crate::dependency_transfer::DEPENDENCY_TEST_SERIAL.blocking_lock();
    let _package = crate::portable_package::test_serial();
    let f = Fixture::new();
    let project = Uuid::new_v4();
    let operation = Uuid::new_v4();
    let transaction = Uuid::new_v4();
    let prepared = f.prepare(project, operation, transaction);
    let family = f.family(project);
    let original = fs::read(family.join(INTENT)).unwrap();
    let attempt = PreparedPortableImport::prepare(
        &f.profile,
        project,
        Uuid::new_v4(),
        Uuid::new_v4(),
        &"a".repeat(64),
        f.package(),
        &CancellationToken::new(),
    );
    assert!(attempt.is_err());
    assert_eq!(fs::read(family.join(INTENT)).unwrap(), original);
    let cancelled = CancellationToken::new();
    cancelled.cancel();
    assert!(prepared
        .publish(
            &CatalogPublicationAck::new(project, transaction, 2, &"b".repeat(64)).unwrap(),
            &cancelled
        )
        .is_err());
    assert!(family.join(format!("stage-{transaction}")).exists());
    assert!(!family.join(format!("generation-{transaction}")).exists());
    assert!(!family.join(COMPLETE).exists());
}
#[test]
fn incorrect_prepared_sql_ack_denies_rename_and_never_becomes_approval_from_journal() {
    let _publication = crate::dependency_transfer::DEPENDENCY_TEST_SERIAL.blocking_lock();
    let _package = crate::portable_package::test_serial();
    let f = Fixture::new();
    let project = Uuid::new_v4();
    let operation = Uuid::new_v4();
    let transaction = Uuid::new_v4();
    let prepared = f.prepare(project, operation, transaction);
    assert!(CatalogPublicationAck::new(project, transaction, 1, &"a".repeat(64)).is_err());
    assert!(prepared
        .publish(
            &CatalogPublicationAck::new(project, transaction, 3, &"a".repeat(64)).unwrap(),
            &CancellationToken::new()
        )
        .is_err());
    assert!(!f
        .family(project)
        .join(format!("generation-{transaction}"))
        .exists());
}

#[test]
fn selected_root_pins_actual_identity_and_journal_while_allowing_legitimate_content_edits() {
    let _publication = crate::dependency_transfer::DEPENDENCY_TEST_SERIAL.blocking_lock();
    let _package = crate::portable_package::test_serial();
    let f = Fixture::new();
    let project = Uuid::new_v4();
    let operation = Uuid::new_v4();
    let transaction = Uuid::new_v4();
    let prepared = f.prepare(project, operation, transaction);
    let fingerprint = prepared.fingerprint().unwrap();
    let published = prepared
        .publish(
            &CatalogPublicationAck::new(project, transaction, 2, &"b".repeat(64)).unwrap(),
            &CancellationToken::new(),
        )
        .unwrap();
    let pinned =
        PinnedPortableImport::open(&f.profile, project, transaction, &fingerprint).unwrap();
    let current=pinned.root().read_file("playbook.yml").unwrap();
    pinned
        .root()
        .write_file("playbook.yml", Some(&current.sha256), "---\n# legitimate ordinary edit\n")
        .unwrap();
    pinned.recheck().unwrap();
    assert!(
        published.recheck().is_err(),
        "pre-SQL/historical publication recheck remains stronger than ordinary loader identity"
    );
    let path = published.target_path();
    drop(pinned);
    drop(published);
    let again = PinnedPortableImport::open(&f.profile, project, transaction, &fingerprint).unwrap();
    assert!(again
        .root()
        .read_file("playbook.yml")
        .unwrap()
        .text
        .contains("legitimate"));
    drop(again);
    let backup = f.family(project).join("owned-original-backup");
    fs::rename(&path, &backup).unwrap();
    fs::create_dir(&path).unwrap();
    #[cfg(windows)]
    super::super::private_acl::tests::protect_owned_fixture(&path);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&path, fs::Permissions::from_mode(0o700)).unwrap();
    }
    fs::write(
        path.join("playbook.yml"),
        fs::read(backup.join("playbook.yml")).unwrap(),
    )
    .unwrap();
    let replacement = ProjectRoot::open(&path).unwrap();
    let id = identity(&replacement.root).unwrap();
    drop(replacement);
    let family = f.family(project);
    let intent: ImportIntent = parsed(&fs::read(family.join(INTENT)).unwrap()).unwrap();
    let rewritten = intent.marker(id).unwrap();
    for name in [CREATED, READY, PUBLISHED] {
        fs::write(family.join(name), encode(&rewritten).unwrap()).unwrap();
    }
    // Even a self-consistent rewrite cannot replace the Native SQL fingerprint.
    // Remove backup from the active family before exercising that exact fence.
    fs::rename(
        &backup,
        f._owner
            .path()
            .join("owned-unchanged-backup-outside-family"),
    )
    .unwrap();
    assert!(PinnedPortableImport::open(&f.profile, project, transaction, &fingerprint).is_err());
}
