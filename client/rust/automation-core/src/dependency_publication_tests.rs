//! Genuine framed archive/preview fixture; no constructed approval witness.
//! Synthetic catalog ACK DATA here do not claim an actual Native SQL commit.
use super::*;
use crate::dependency_publication::*;
use std::path::Path;
use uuid::Uuid;

fn source(trees: &Trees, operation: Uuid) -> DependencyPublicationSource {
    DependencyPublicationSource::new(
        &trees.profile_path,
        "managed",
        trees.project,
        operation,
        7,
        &"a".repeat(64),
    )
    .unwrap()
}
fn family(trees: &Trees) -> PathBuf {
    trees
        .profile_path
        .join("automation")
        .join("project-revisions")
        .join(trees.project.to_string())
}
fn ack(trees: &Trees, transaction: Uuid) -> CatalogPublicationAck {
    CatalogPublicationAck::new(trees.project, transaction, 8, &"b".repeat(64)).unwrap()
}
fn publish(
    trees: &Trees,
    stage: &ProjectRoot,
    preview: &VerifiedDependencyPreview,
    value: &Value,
    operation: Uuid,
) -> PublishedDependencyGeneration {
    prepare_publish(
        source(trees, operation),
        &trees.base,
        &trees.base.revision().unwrap(),
        &selected(value),
        stage,
        preview,
        &CancellationToken::new(),
    )
    .unwrap()
}
fn reconcile(
    trees: &Trees,
    operation: Uuid,
    transaction: Uuid,
    digest: &str,
    fingerprint: &str,
) -> crate::Result<PublicationAuditObservation> {
    reconcile_selected_catalog(
        source(trees, operation),
        PublicationRecoveryReference::new(transaction, operation).unwrap(),
        &ack(trees, transaction),
        digest,
        fingerprint,
    )
}

#[test]
fn facade_metadata_is_bounded_and_never_deserializable_approval() {
    let own = tempfile::tempdir().unwrap();
    let id = Uuid::new_v4();
    let valid =
        || DependencyPublicationSource::new(own.path(), "managed", id, id, 7, &"a".repeat(64));
    assert!(valid().is_ok());
    assert!(!format!("{:?}", valid().unwrap()).contains(own.path().to_str().unwrap()));
    for kind in ["directory", "git", "Managed", ""] {
        assert!(
            DependencyPublicationSource::new(own.path(), kind, id, id, 7, &"a".repeat(64)).is_err()
        );
    }
    for revision in [0, u64::MAX] {
        assert!(DependencyPublicationSource::new(
            own.path(),
            "managed",
            id,
            id,
            revision,
            &"a".repeat(64)
        )
        .is_err());
    }
    for digest in ["a".repeat(63), "A".repeat(64), "g".repeat(64)] {
        assert!(
            DependencyPublicationSource::new(own.path(), "managed", id, id, 7, &digest).is_err()
        );
        assert!(CatalogPublicationAck::new(id, id, 8, &digest).is_err());
    }
    assert!(DependencyPublicationSource::new(
        Path::new("relative"),
        "managed",
        id,
        id,
        7,
        &"a".repeat(64)
    )
    .is_err());
    assert!(DependencyPublicationSource::new(
        own.path(),
        "managed",
        Uuid::nil(),
        id,
        7,
        &"a".repeat(64)
    )
    .is_err());
    assert!(CatalogPublicationAck::new(id, id, 1, &"b".repeat(64)).is_err());
    assert!(PublicationRecoveryReference::new(Uuid::nil(), id).is_err());
}

#[tokio::test]
async fn genuine_preview_publishes_retained_generation_and_exact_audit_without_replacing_old_facts()
{
    run_fixed_prepare_fixture(
        "valid",
        Some(|trees, stage, preview, value, operation| {
            let before = trees.base.revision().unwrap();
            let selected = selected(value);
            let mut published = publish(trees, stage, preview, value, operation);
            // Native managed loader intentionally compares exact OsStr, not
            // normalized Path equality. Component joins must match on Windows.
            let native_target = trees
                .profile_path
                .join("automation")
                .join("project-revisions")
                .join(trees.project.to_string())
                .join(format!("generation-{}", published.transaction()));
            assert_eq!(
                published.target_path().as_os_str(),
                native_target.as_os_str()
            );
            assert_eq!(
                ProjectRoot::open(published.target_path())
                    .unwrap()
                    .revision_for_publication()
                    .unwrap(),
                *preview.candidate()
            );
            assert_eq!(published.old_catalog_revision(), 7);
            assert_eq!(published.old_row_sha256(), "a".repeat(64));
            assert_eq!(published.candidate_digest(), preview.candidate().digest);
            let fingerprint = published.audit_fingerprint().unwrap();
            assert!(cc_models::automation::is_digest(&fingerprint));
            let ack = ack(trees, published.transaction());
            assert_eq!(
                published.audit_observation().unwrap(),
                PublicationAuditObservation::FilesystemPublished
            );
            assert!(published
                .record_complete(&ack, &trees.base, &before, &selected, stage, preview)
                .is_err());
            assert!(!family(trees).join("complete.json").exists());
            for bad in [
                CatalogPublicationAck::new(
                    Uuid::new_v4(),
                    published.transaction(),
                    8,
                    &"b".repeat(64),
                )
                .unwrap(),
                CatalogPublicationAck::new(trees.project, Uuid::new_v4(), 8, &"b".repeat(64))
                    .unwrap(),
                CatalogPublicationAck::new(
                    trees.project,
                    published.transaction(),
                    9,
                    &"b".repeat(64),
                )
                .unwrap(),
            ] {
                assert!(published
                    .record_catalog_ack(&bad, &trees.base, &before, &selected, stage, preview)
                    .is_err());
                assert!(!family(trees).join("catalog-committed.json").exists());
            }
            published
                .record_catalog_ack(&ack, &trees.base, &before, &selected, stage, preview)
                .unwrap();
            let exact = fs::read(family(trees).join("catalog-committed.json")).unwrap();
            published
                .record_catalog_ack(&ack, &trees.base, &before, &selected, stage, preview)
                .unwrap();
            assert_eq!(
                published.audit_observation().unwrap(),
                PublicationAuditObservation::CatalogCommitted
            );
            let changed = CatalogPublicationAck::new(
                trees.project,
                published.transaction(),
                8,
                &"c".repeat(64),
            )
            .unwrap();
            assert!(published
                .record_catalog_ack(&changed, &trees.base, &before, &selected, stage, preview)
                .is_err());
            assert_eq!(
                fs::read(family(trees).join("catalog-committed.json")).unwrap(),
                exact
            );
            published
                .record_complete(&ack, &trees.base, &before, &selected, stage, preview)
                .unwrap();
            assert_eq!(
                fs::read(family(trees).join("complete.json")).unwrap(),
                exact
            );
            assert_eq!(
                published.audit_observation().unwrap(),
                PublicationAuditObservation::Complete
            );
            assert_eq!(published.audit_fingerprint().unwrap(), fingerprint);
            let mut changed_before = before.clone();
            changed_before.digest = "f".repeat(64);
            assert!(published
                .recheck(&trees.base, &changed_before, &selected, stage, preview)
                .is_err());
            published
                .recheck(&trees.base, &before, &selected, stage, preview)
                .unwrap();
            assert_eq!(trees.base.revision().unwrap(), before);
        }),
    )
    .await;
}

#[tokio::test]
async fn selected_restart_records_only_missing_audit_and_rejects_wrong_current_metadata() {
    run_fixed_prepare_fixture(
        "valid",
        Some(|trees, stage, preview, value, operation| {
            let mut published = publish(trees, stage, preview, value, operation);
            let tx = published.transaction();
            let fingerprint = published.audit_fingerprint().unwrap();
            let digest = published.candidate_digest().to_owned();
            let selected = selected(value);
            let before = trees.base.revision().unwrap();
            // Simulated SQL succeeded, process stopped before completion audit.
            published
                .record_catalog_ack(
                    &ack(trees, tx),
                    &trees.base,
                    &before,
                    &selected,
                    stage,
                    preview,
                )
                .unwrap();
            let committed = fs::read(family(trees).join("catalog-committed.json")).unwrap();
            drop(published);
            assert!(reconcile(trees, operation, tx, &"f".repeat(64), &fingerprint).is_err());
            assert!(reconcile(trees, operation, tx, &digest, &"f".repeat(64)).is_err());
            assert!(reconcile(trees, Uuid::new_v4(), tx, &digest, &fingerprint).is_err());
            assert!(!family(trees).join("complete.json").exists());
            assert_eq!(
                reconcile(trees, operation, tx, &digest, &fingerprint).unwrap(),
                PublicationAuditObservation::Complete
            );
            assert_eq!(
                reconcile(trees, operation, tx, &digest, &fingerprint).unwrap(),
                PublicationAuditObservation::Complete
            );
            assert_eq!(
                fs::read(family(trees).join("complete.json")).unwrap(),
                committed
            );
            assert_eq!(
                fs::read(family(trees).join("catalog-committed.json")).unwrap(),
                committed
            );
        }),
    )
    .await;
}

#[tokio::test]
async fn future_duplicate_and_orphan_audits_are_preserved_without_completion_repair() {
    run_fixed_prepare_fixture(
        "valid",
        Some(|trees, stage, preview, value, operation| {
            let published = publish(trees, stage, preview, value, operation);
            let tx = published.transaction();
            let fingerprint = published.audit_fingerprint().unwrap();
            let digest = published.candidate_digest().to_owned();
            drop(published);
            let committed = family(trees).join("catalog-committed.json");
            let complete = family(trees).join("complete.json");
            for bytes in [
                b"{\"version\":99,\"future\":true}".as_slice(),
                b"{\"version\":1,\"version\":1}".as_slice(),
                b"not JSON".as_slice(),
            ] {
                fs::write(&committed, bytes).unwrap();
                assert!(reconcile(trees, operation, tx, &digest, &fingerprint).is_err());
                assert_eq!(fs::read(&committed).unwrap(), bytes);
                assert!(!complete.exists());
                fs::remove_file(&committed).unwrap();
            }
            let orphan = b"{\"version\":99,\"future\":\"orphan\"}";
            fs::write(&complete, orphan).unwrap();
            assert!(reconcile(trees, operation, tx, &digest, &fingerprint).is_err());
            assert_eq!(fs::read(&complete).unwrap(), orphan);
            assert!(!committed.exists());
        }),
    )
    .await;
}

#[tokio::test]
async fn same_byte_replacement_and_self_consistent_journal_rewrite_fail_native_fingerprint() {
    run_fixed_prepare_fixture(
        "valid",
        Some(|trees, stage, preview, value, operation| {
            let mut published = publish(trees, stage, preview, value, operation);
            let target = published.target_path().to_owned();
            let held = trees._own.path().join("held-generation");
            let tx = published.transaction();
            let fingerprint = published.audit_fingerprint().unwrap();
            let digest = published.candidate_digest().to_owned();
            let before = trees.base.revision().unwrap();
            #[cfg(windows)]
            {
                // Actual retained no-delete-sharing directory cap spans SQL.
                let error = fs::rename(&target, &held).unwrap_err();
                assert!(matches!(error.raw_os_error(), Some(5 | 32)), "{error}");
            }
            #[cfg(unix)]
            {
                fs::rename(&target, &held).unwrap();
                assert!(published
                    .recheck(&trees.base, &before, &selected(value), stage, preview)
                    .is_err());
                fs::rename(&held, &target).unwrap();
            }
            published
                .recheck(&trees.base, &before, &selected(value), stage, preview)
                .unwrap();
            drop(published);
            fs::rename(&target, &held).unwrap();
            private_fixture_directory(&target);
            let original_target = ProjectRoot::open(&held).unwrap();
            let replacement = ProjectRoot::open(&target).unwrap();
            original_target
                .freeze_to(preview.candidate(), &replacement, &CancellationToken::new())
                .unwrap();
            assert_eq!(
                replacement.revision_for_publication().unwrap(),
                *preview.candidate()
            );
            assert!(!original_target.same_directory(&replacement).unwrap());
            assert!(reconcile(trees, operation, tx, &digest, &fingerprint).is_err());
            // Rewrite ALL generation markers to the replacement ID. Ordinary
            // self-consistency is now valid, but Native persisted the OLD fingerprint.
            let new_identity = directory_identity(&target);
            let mut preserved = Vec::new();
            for name in ["created.json", "ready.json", "published.json"] {
                let path = family(trees).join(name);
                let mut marker: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
                marker["stage"] = new_identity.clone();
                let bytes = serde_json::to_vec(&marker).unwrap();
                fs::write(&path, &bytes).unwrap();
                preserved.push((path, bytes));
            }
            assert!(reconcile(trees, operation, tx, &digest, &fingerprint).is_err());
            assert!(
                open_selected_generation(&trees.profile_path, trees.project, tx, &fingerprint)
                    .is_err()
            );
            assert!(!family(trees).join("catalog-committed.json").exists());
            assert!(!family(trees).join("complete.json").exists());
            for (path, bytes) in preserved {
                assert_eq!(fs::read(path).unwrap(), bytes);
            }
            assert_eq!(
                replacement.revision_for_publication().unwrap(),
                *preview.candidate()
            );
        }),
    )
    .await;
}

fn directory_identity(path: &Path) -> Value {
    let dir = cap_std::fs::Dir::open_ambient_dir(path, cap_std::ambient_authority()).unwrap();
    #[cfg(windows)]
    {
        let file = dir.into_std_file();
        let (volume, file_id) = crate::project_identity_windows::stable(&file).unwrap();
        json!({"platform":"windows", "volume":volume, "file_id":file_id})
    }
    #[cfg(unix)]
    {
        use cap_fs_ext::MetadataExt;
        let metadata = dir.dir_metadata().unwrap();
        json!({"platform":"unix", "device":metadata.dev(), "inode":metadata.ino()})
    }
}

#[tokio::test]
async fn genuine_recovery_keeps_old_row_facts_and_changed_original_blocks_metadata_reconcile() {
    run_fixed_prepare_fixture(
        "valid",
        Some(|trees, stage, preview, value, operation| {
            let published = publish(trees, stage, preview, value, operation);
            let tx = published.transaction();
            let fingerprint = published.audit_fingerprint().unwrap();
            let digest = published.candidate_digest().to_owned();
            drop(published);
            let before = trees.base.revision().unwrap();
            // Genuine preview is still owned by this dedicated fixed-protocol
            // fixture. This test does not claim renewed Native user acceptance.
            let mut recovered = recover_publish(
                source(trees, operation),
                PublicationRecoveryReference::new(tx, operation).unwrap(),
                &trees.base,
                &before,
                &selected(value),
                stage,
                preview,
                &CancellationToken::new(),
            )
            .unwrap();
            assert_eq!(recovered.transaction(), tx);
            assert_eq!(recovered.audit_fingerprint().unwrap(), fingerprint);
            assert_eq!(recovered.old_catalog_revision(), 7);
            recovered
                .recheck(&trees.base, &before, &selected(value), stage, preview)
                .unwrap();
            drop(recovered);
            let path = trees.base_path.join("play.yml");
            let original_bytes = fs::read(&path).unwrap();
            fs::write(&path, b"changed old source after actual publish").unwrap();
            assert!(reconcile(trees, operation, tx, &digest, &fingerprint).is_err());
            assert!(!family(trees).join("catalog-committed.json").exists());
            assert!(!family(trees).join("complete.json").exists());
            fs::write(&path, original_bytes).unwrap();
            assert_eq!(
                reconcile(trees, operation, tx, &digest, &fingerprint).unwrap(),
                PublicationAuditObservation::Complete
            );
        }),
    )
    .await;
}

#[tokio::test]
async fn ordinary_selected_root_is_physically_pinned_and_allows_legitimate_file_edits() {
    run_fixed_prepare_fixture(
        "valid",
        Some(|trees, stage, preview, value, operation| {
            let mut published = publish(trees, stage, preview, value, operation);
            let tx = published.transaction();
            let fingerprint = published.audit_fingerprint().unwrap();
            let target = published.target_path().to_owned();
            let digest = published.candidate_digest().to_owned();
            let before = trees.base.revision().unwrap();
            let selected = selected(value);
            let ack = ack(trees, tx);
            published
                .record_catalog_ack(&ack, &trees.base, &before, &selected, stage, preview)
                .unwrap();
            published
                .record_complete(&ack, &trees.base, &before, &selected, stage, preview)
                .unwrap();
            drop(published);
            assert!(open_selected_generation(
                &trees.profile_path,
                trees.project,
                tx,
                &"f".repeat(64)
            )
            .is_err());
            let pinned =
                open_selected_generation(&trees.profile_path, trees.project, tx, &fingerprint)
                    .unwrap();
            assert!(!format!("{pinned:?}").contains(trees.profile_path.to_str().unwrap()));
            let moved = trees._own.path().join("owned-selected-moved");
            #[cfg(windows)]
            {
                let error = fs::rename(&target, &moved).unwrap_err();
                assert!(matches!(error.raw_os_error(), Some(5 | 32)), "{error}");
            }
            #[cfg(unix)]
            {
                fs::rename(&target, &moved).unwrap();
                assert!(pinned.recheck().is_err());
                fs::rename(&moved, &target).unwrap();
            }
            pinned.recheck().unwrap();
            let file = pinned.root().read_file("play.yml").unwrap();
            pinned
                .root()
                .write_file(
                    "play.yml",
                    Some(&file.sha256),
                    "---\n# legitimate user edit after install\n",
                )
                .unwrap();
            pinned.recheck().unwrap();
            assert_ne!(pinned.root().revision().unwrap().digest, digest);
            // Reopening the SAME installed generation does not assert its historical
            // tree digest. Ordinary edits remain usable; no new receipt is minted.
            let reopened =
                open_selected_generation(&trees.profile_path, trees.project, tx, &fingerprint)
                    .unwrap();
            reopened.recheck().unwrap();
            assert_eq!(
                reopened.root().read_file("play.yml").unwrap().text,
                "---\n# legitimate user edit after install\n"
            );
            // In contrast audit-repair of the old installation requires unchanged
            // bytes and must not silently certify a later edited tree.
            let audit_before = fs::read(family(trees).join("complete.json")).unwrap();
            assert!(reconcile(trees, operation, tx, &digest, &fingerprint).is_err());
            assert_eq!(
                fs::read(family(trees).join("complete.json")).unwrap(),
                audit_before
            );
        }),
    )
    .await;
}
