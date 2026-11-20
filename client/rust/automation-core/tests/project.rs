use cc_automation_core::{validate_project_path, ProjectRoot};
use std::fs;

#[test]
fn project_preview_and_revision_work_without_python_ansible_or_wsl() {
    let temporary = tempfile::tempdir().unwrap();
    fs::write(
        temporary.path().join("playbook.yml"),
        "---\r\n- hosts: all\r\n",
    )
    .unwrap();
    fs::create_dir(temporary.path().join("empty")).unwrap();
    fs::create_dir(temporary.path().join(".git")).unwrap();
    fs::write(temporary.path().join(".git/config"), "must not be copied").unwrap();
    let root = ProjectRoot::open(temporary.path()).unwrap();
    let revision = root.revision().unwrap();
    assert_eq!(revision.entries.len(), 2);
    assert!(root.read_text("playbook.yml").unwrap().contains("\r\n"));
    assert_eq!(root.revision().unwrap().digest, revision.digest);
    fs::write(temporary.path().join("playbook.yml"), "- hosts: changed\n").unwrap();
    assert_ne!(root.revision().unwrap().digest, revision.digest);
}

#[test]
fn hardlinks_cannot_smuggle_files_from_outside_the_project() {
    let temporary = tempfile::tempdir().unwrap();
    let project = temporary.path().join("project");
    fs::create_dir(&project).unwrap();
    let outside = temporary.path().join("private");
    fs::write(&outside, "synthetic-secret").unwrap();
    fs::hard_link(&outside, project.join("linked")).unwrap();
    let root = ProjectRoot::open(&project).unwrap();
    assert!(root.revision().is_err());
    assert!(root.read_text("linked").is_err());
    for path in [
        "../private",
        "/absolute",
        "C:private",
        "a\\b",
        "a//b",
        ".git/config",
    ] {
        assert!(validate_project_path(path).is_err(), "{path}");
    }
}
