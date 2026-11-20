//! Real filesystem policy tests. No fabricated archive/review witness.
use crate::{AutomationError, ProjectRoot};
fn excluded(root: &ProjectRoot) {
    assert!(matches!(
        root.revision_for_publication(),
        Err(AutomationError::PublicationUnsupported(
            "excluded .git entry"
        ))
    ));
}
#[test]
fn publication_policy_matches_existing_revision_for_ordinary_binary_and_empty_directories() {
    let source = tempfile::tempdir().unwrap();
    std::fs::create_dir(source.path().join("empty")).unwrap();
    std::fs::write(source.path().join("ordinary.bin"), [0, 255, 13, 10]).unwrap();
    let root = ProjectRoot::open(source.path()).unwrap();
    assert_eq!(
        root.revision().unwrap(),
        root.revision_for_publication().unwrap()
    );
    let copy = tempfile::tempdir().unwrap();
    let destination = ProjectRoot::open(copy.path()).unwrap();
    root.freeze_to(
        &root.revision_for_publication().unwrap(),
        &destination,
        &tokio_util::sync::CancellationToken::new(),
    )
    .unwrap();
    assert_eq!(
        root.revision().unwrap(),
        destination.revision_for_publication().unwrap()
    );
}
#[test]
fn top_and_nested_excluded_files_or_directories_never_become_lossy_publication() {
    for nested in [false, true] {
        for directory in [false, true] {
            let source = tempfile::tempdir().unwrap();
            std::fs::write(source.path().join("original.yml"), b"---\n").unwrap();
            let parent = if nested {
                let p = source.path().join("nested");
                std::fs::create_dir(&p).unwrap();
                p
            } else {
                source.path().to_path_buf()
            };
            let root = ProjectRoot::open(source.path()).unwrap();
            let before = root.revision().unwrap();
            let ignored = parent.join(".git");
            if directory {
                std::fs::create_dir(&ignored).unwrap();
                std::fs::write(ignored.join("preserve"), b"owned excluded metadata").unwrap();
            } else {
                std::fs::write(&ignored, b"owned gitdir marker").unwrap();
            }
            assert_eq!(root.revision().unwrap(), before);
            excluded(&root);
            if directory {
                assert_eq!(
                    std::fs::read(ignored.join("preserve")).unwrap(),
                    b"owned excluded metadata"
                );
            } else {
                assert_eq!(std::fs::read(&ignored).unwrap(), b"owned gitdir marker");
            }
        }
    }
}
#[cfg(unix)]
#[test]
fn excluded_dangling_and_directory_links_reject_before_following_without_changing_snapshot_policy()
{
    use std::os::unix::fs::symlink;
    for nested in [false, true] {
        for dangling in [false, true] {
            let source = tempfile::tempdir().unwrap();
            let target = tempfile::tempdir().unwrap();
            std::fs::write(
                target.path().join("preserve"),
                b"must not traverse or change",
            )
            .unwrap();
            let parent = if nested {
                let p = source.path().join("nested");
                std::fs::create_dir(&p).unwrap();
                p
            } else {
                source.path().to_path_buf()
            };
            let root = ProjectRoot::open(source.path()).unwrap();
            let before = root.revision().unwrap();
            symlink(
                if dangling {
                    target.path().join("missing")
                } else {
                    target.path().to_path_buf()
                },
                parent.join(".git"),
            )
            .unwrap();
            excluded(&root);
            assert_eq!(root.revision().unwrap(), before);
            assert_eq!(
                std::fs::read(target.path().join("preserve")).unwrap(),
                b"must not traverse or change"
            );
        }
    }
}
#[cfg(windows)]
#[test]
fn excluded_owned_junction_rejects_without_traversing_or_weakening_held_root() {
    // /J requires no symlink privilege. Both names and target are this TempDir.
    let source = tempfile::tempdir().unwrap();
    let target = source.path().join("target");
    std::fs::create_dir(&target).unwrap();
    std::fs::write(target.join("preserve"), b"owned target unchanged").unwrap();
    let project = source.path().join("project");
    std::fs::create_dir(&project).unwrap();
    let link = project.join(".git");
    let status = std::process::Command::new("cmd.exe")
        .args(["/d", "/c", "mklink", "/J"])
        .arg(&link)
        .arg(&target)
        .stdout(std::process::Stdio::null())
        .status()
        .unwrap();
    assert!(status.success());
    let root = ProjectRoot::open(&project).unwrap();
    let before = root.revision().unwrap();
    excluded(&root);
    assert_eq!(root.revision().unwrap(), before);
    assert_eq!(
        std::fs::read(target.join("preserve")).unwrap(),
        b"owned target unchanged"
    );
    // Explicitly unlink only the junction; never recurse through it.
    std::fs::remove_dir(&link).unwrap();
    assert!(target.join("preserve").is_file());
}
#[cfg(unix)]
#[test]
fn excluded_unreadable_directory_is_not_opened() {
    use std::os::unix::fs::PermissionsExt;
    let source = tempfile::tempdir().unwrap();
    let git = source.path().join(".git");
    std::fs::create_dir(&git).unwrap();
    std::fs::set_permissions(&git, std::fs::Permissions::from_mode(0)).unwrap();
    let root = ProjectRoot::open(source.path()).unwrap();
    excluded(&root);
    assert!(root.revision().is_ok());
    std::fs::set_permissions(&git, std::fs::Permissions::from_mode(0o700)).unwrap();
}
