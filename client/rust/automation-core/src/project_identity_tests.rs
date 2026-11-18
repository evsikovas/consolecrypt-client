use crate::ProjectRoot;
#[test]
fn current_name_reopen_uses_real_directory_identity_and_not_just_identical_bytes() {
    let fixture = tempfile::Builder::new()
        .prefix("owned-directory-identity-")
        .tempdir()
        .unwrap();
    let parent = fixture.path().join("current-parent");
    let path = parent.join("project");
    let moved = fixture.path().join("original-parent");
    std::fs::create_dir(&parent).unwrap();
    std::fs::create_dir(&path).unwrap();
    std::fs::write(path.join("file.yml"), b"synthetic original bytes\n").unwrap();
    // Prove this own fixture permits rename before any captured Dir exists.
    std::fs::rename(&parent, &moved).unwrap();
    std::fs::rename(&moved, &parent).unwrap();
    let original = ProjectRoot::open(&path).unwrap();
    let revision = original.revision().unwrap();
    let again = ProjectRoot::open(&path).unwrap();
    assert!(original.same_directory(&again).unwrap());
    assert!(original.reopen_same_directory(&path).is_ok());
    assert_eq!(parent.parent(), Some(fixture.path()));
    assert_eq!(moved.parent(), Some(fixture.path()));
    match std::fs::rename(&parent, &moved) {
        Ok(()) => {
            std::fs::create_dir(&parent).unwrap();
            std::fs::create_dir(&path).unwrap();
            let replacement = ProjectRoot::open(&path).unwrap();
            original
                .freeze_to(
                    &revision,
                    &replacement,
                    &tokio_util::sync::CancellationToken::new(),
                )
                .unwrap();
            assert_eq!(
                replacement.revision().unwrap(),
                revision,
                "replacement must be byte-identical"
            );
            assert_eq!(
                original.revision().unwrap(),
                revision,
                "retained directory remains readable"
            );
            assert!(!original.same_directory(&replacement).unwrap());
            assert!(original.reopen_same_directory(&path).is_err());
        }
        Err(error) => {
            #[cfg(windows)]
            {
                // Real production cap_std Dir handles block both leaf and
                // this ancestor rename (observed32/5 respectively). No skip
                // or weaker share-mode: assert exact deny +preserved namespace.
                assert!(matches!(error.raw_os_error(), Some(5 | 32)));
                assert!(path.is_dir() && !moved.exists());
                assert_eq!(original.revision().unwrap(), revision);
                assert!(original.reopen_same_directory(&path).is_ok());
                // Independent real identical-directory identity check remains
                // required even when this namespace replacement was prevented.
                let sibling = fixture.path().join("identical-sibling");
                std::fs::create_dir(&sibling).unwrap();
                let replacement = ProjectRoot::open(&sibling).unwrap();
                original
                    .freeze_to(
                        &revision,
                        &replacement,
                        &tokio_util::sync::CancellationToken::new(),
                    )
                    .unwrap();
                assert_eq!(replacement.revision().unwrap(), revision);
                assert!(!original.same_directory(&replacement).unwrap());
                assert!(original.reopen_same_directory(&sibling).is_err());
                // Directory identity is not content identity: real mutation
                // must still be detected by the existing complete walker.
                std::fs::write(path.join("file.yml"), b"actual changed bytes\n").unwrap();
                assert_ne!(original.revision().unwrap(), revision);
            }
            #[cfg(not(windows))]
            panic!("owned Unix ancestor rename unexpectedly denied: {error}");
        }
    }
}
#[cfg(unix)]
#[test]
fn current_name_symlink_is_denied_even_when_target_is_original_directory() {
    let fixture = tempfile::Builder::new()
        .prefix("owned-directory-symlink-")
        .tempdir()
        .unwrap();
    let path = fixture.path().join("current");
    let moved = fixture.path().join("original-moved");
    std::fs::create_dir(&path).unwrap();
    let original = ProjectRoot::open(&path).unwrap();
    std::fs::rename(&path, &moved).unwrap();
    std::os::unix::fs::symlink(&moved, &path).unwrap();
    assert!(original.reopen_same_directory(&path).is_err());
}
#[test]
fn project_move_preserves_file_bytes_and_refuses_clobber_or_stale_source() {
    let own = tempfile::tempdir().unwrap();
    let root = crate::ProjectRoot::open(own.path()).unwrap();
    let source = root.write_file("old/run.yml", None, "---\n").unwrap();
    let existing = root.write_file("new/run.yml", None, "unchanged\n").unwrap();
    assert!(root.move_file("old/run.yml", "new", &source.sha256).is_err());
    assert_eq!(root.read_file("new/run.yml").unwrap().sha256, existing.sha256);
    assert!(root.move_file("old/run.yml", "", &existing.sha256).is_err());
    assert!(root.move_file("old/run.yml", "../outside", &source.sha256).is_err());
    root.move_file("old/run.yml", "", &source.sha256).unwrap();
    assert!(root.read_file("old/run.yml").is_err());
    assert_eq!(root.read_file("run.yml").unwrap().sha256, source.sha256);
}
