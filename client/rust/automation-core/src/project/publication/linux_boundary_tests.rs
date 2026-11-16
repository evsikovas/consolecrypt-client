//! Actual ProjectRoot walker/copy and fixed marker I/O, not a mirror engine.
use super::*;
use std::{fs, path::PathBuf};

#[test]
fn strict_clone_preserves_ordinary_snapshot_and_existing_git_exclusion_policy() {
    let own = tempfile::tempdir().unwrap();
    fs::create_dir(own.path().join(".git")).unwrap();
    fs::write(
        own.path().join(".git/owned-test-data"),
        b"not a project snapshot file",
    )
    .unwrap();
    fs::write(own.path().join("play.yml"), b"---\n").unwrap();
    let plain = ProjectRoot::open(own.path()).unwrap();
    assert!(!plain.publication_boundary);
    let strict = plain.publication_root().unwrap();
    assert!(strict.publication_boundary);
    assert_eq!(plain.revision().unwrap(), strict.revision().unwrap());
    assert!(matches!(
        plain.revision_for_publication(),
        Err(AutomationError::PublicationUnsupported(
            "excluded .git entry"
        ))
    ));
    assert!(
        !plain.publication_boundary,
        "strict derivation must not mutate the ordinary root"
    );
}

#[test]
fn strict_scope_propagates_parent_create_role_reopen_and_real_copy_without_new_parser() {
    let own = tempfile::tempdir().unwrap();
    let strict = ProjectRoot::open(own.path())
        .unwrap()
        .publication_root()
        .unwrap();
    strict
        .write_file("owned/nested/play.yml", None, "---\n")
        .unwrap();
    strict.create_role("owned_role").unwrap();
    let named = strict.reopen_same_directory(own.path()).unwrap();
    assert!(named.publication_boundary);
    let before = strict.revision_for_publication().unwrap();
    let output = tempfile::tempdir().unwrap();
    let destination = ProjectRoot::open(output.path())
        .unwrap()
        .publication_root()
        .unwrap();
    strict
        .freeze_to(&before, &destination, &CancellationToken::new())
        .unwrap();
    assert_eq!(destination.revision_for_publication().unwrap(), before);
    assert_eq!(
        named.read_file("owned/nested/play.yml").unwrap().text,
        "---\n"
    );
}

#[test]
fn actual_fixed_marker_io_remains_bounded_and_never_overwrites_existing_bytes() {
    let own = tempfile::tempdir().unwrap();
    let dir = Dir::open_ambient_dir(own.path(), cap_std::ambient_authority()).unwrap();
    let marker = serde_json::json!({"version":1,"owned":true});
    write_new(&dir, "owned-marker.json", &marker).unwrap();
    let original = fs::read(own.path().join("owned-marker.json")).unwrap();
    assert_eq!(read_small(&dir, "owned-marker.json").unwrap(), original);
    assert!(write_new(
        &dir,
        "owned-marker.json",
        &serde_json::json!({"version":99})
    )
    .is_err());
    assert_eq!(
        fs::read(own.path().join("owned-marker.json")).unwrap(),
        original
    );
    fs::write(own.path().join("oversize.json"), vec![b'x'; MAX_MARKER + 1]).unwrap();
    assert!(read_small(&dir, "oversize.json").is_err());
}

fn fixture() -> PathBuf {
    let path = std::env::var("CC_PUBLICATION_WHOLE_BOUNDARY_ROOT")
        .expect("explicit owned bind fixture required");
    assert!(path.starts_with("/tmp/owned-publication-whole-"));
    PathBuf::from(path)
}
fn xdev<T>(result: Result<T>) {
    assert!(
        matches!(result, Err(AutomationError::Io(ref error)) if error.raw_os_error()==Some(18)),
        "actual constrained I/O must reject the mount before parsing/reading/copying"
    );
}

#[test]
#[ignore = "requires owned Docker nested directory+file bind fixture; no in-process mounts"]
fn nested_bind_rejected_at_real_walk_read_metadata_copy_and_reopen_checkpoints() {
    let fixture = fixture();
    for (case, file) in [
        ("directory-case", "mounted/public-test-marker"),
        ("file-case", "mounted-file"),
    ] {
        let path = fixture.join(case);
        let plain = ProjectRoot::open(&path).unwrap();
        // Synthetic public bytes only. Existing snapshots retain their policy;
        // this demonstrates a real mounted object that the strict clone denies.
        assert_eq!(plain.read_text(file).unwrap(), "owned nested bind marker");
        let expected = plain.revision().unwrap();
        let strict = plain.publication_root().unwrap();
        xdev(strict.read_file(file));
        xdev(strict.revision_for_publication());
        let named = strict.reopen_same_directory(&path).unwrap();
        assert!(named.publication_boundary);
        xdev(named.read_file(file));
        let output = tempfile::tempdir().unwrap();
        let destination = ProjectRoot::open(output.path())
            .unwrap()
            .publication_root()
            .unwrap();
        xdev(strict.freeze_to(&expected, &destination, &CancellationToken::new()));
        assert!(fs::read_dir(output.path()).unwrap().next().is_none());
        assert_eq!(plain.read_text(file).unwrap(), "owned nested bind marker");
    }
}

#[test]
#[ignore = "requires owned Docker file bind at fixed journal name; no in-process mounts"]
fn file_bind_at_fixed_journal_name_is_denied_before_metadata_or_marker_bytes() {
    let path = fixture().join("marker-case");
    let parent = Dir::open_ambient_dir(&path, cap_std::ambient_authority()).unwrap();
    xdev(read_small(&parent, INTENT));
    xdev(exists(&parent, INTENT));
    assert_eq!(
        fs::read(path.join(INTENT)).unwrap(),
        b"owned nested bind marker"
    );
}
