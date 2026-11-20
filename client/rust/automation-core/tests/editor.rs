use cc_automation_core::ProjectRoot;

#[test]
fn stale_editor_and_ai_proposals_cannot_overwrite_manual_changes() {
    let directory = tempfile::tempdir().unwrap();
    std::fs::write(directory.path().join("playbook.yml"), "original\n").unwrap();
    let root = ProjectRoot::open(directory.path()).unwrap();
    let original = root.read_file("playbook.yml").unwrap();
    std::fs::write(directory.path().join("playbook.yml"), "manual edit\n").unwrap();
    assert!(root
        .write_file("playbook.yml", Some(&original.sha256), "stale proposal\n")
        .is_err());
    assert_eq!(root.read_text("playbook.yml").unwrap(), "manual edit\n");
    assert_eq!(std::fs::read_dir(directory.path()).unwrap().count(), 1);
}

#[test]
fn saves_preserve_crlf_and_create_new_files_without_clobbering() {
    let directory = tempfile::tempdir().unwrap();
    std::fs::write(directory.path().join("playbook.yml"), "---\r\noriginal\r\n").unwrap();
    let root = ProjectRoot::open(directory.path()).unwrap();
    let original = root.read_file("playbook.yml").unwrap();
    assert_eq!(original.line_endings, "crlf");
    let saved = root
        .write_file("playbook.yml", Some(&original.sha256), "---\nupdated\n")
        .unwrap();
    assert_eq!(saved.text, "---\r\nupdated\r\n");
    assert_ne!(saved.sha256, original.sha256);
    let created = root.write_file("новый файл.yml", None, "---\n").unwrap();
    assert_eq!(created.text, "---\n");
    assert!(root
        .write_file("новый файл.yml", None, "overwrite")
        .is_err());
    assert!(root.write_file("../outside", None, "outside").is_err());
    assert!(root.write_file("C:/outside", None, "outside").is_err());
    let nested = root
        .write_file("roles/demo/tasks/main.yml", None, "---\n")
        .unwrap();
    assert_eq!(root.read_file(&nested.path).unwrap().sha256, nested.sha256);
}

#[cfg(unix)]
#[test]
fn intermediate_directory_symlinks_are_rejected_even_if_they_stay_inside_root() {
    let directory = tempfile::tempdir().unwrap();
    std::fs::create_dir(directory.path().join("real")).unwrap();
    std::fs::write(directory.path().join("real/task.yml"), "original").unwrap();
    std::os::unix::fs::symlink("real", directory.path().join("alias")).unwrap();
    let root = ProjectRoot::open(directory.path()).unwrap();
    assert!(root.read_file("alias/task.yml").is_err());
    assert!(root.write_file("alias/new.yml", None, "changed").is_err());
    assert!(!directory.path().join("real/new.yml").exists());
    assert!(root.revision().is_err());
}

#[test]
fn editing_never_writes_through_a_hardlink_outside_the_project() {
    let directory = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    std::fs::write(outside.path().join("private"), "untouched").unwrap();
    std::fs::hard_link(
        outside.path().join("private"),
        directory.path().join("linked.yml"),
    )
    .unwrap();
    let root = ProjectRoot::open(directory.path()).unwrap();
    assert!(root.write_file("linked.yml", None, "changed").is_err());
    assert_eq!(
        std::fs::read_to_string(outside.path().join("private")).unwrap(),
        "untouched"
    );
}
