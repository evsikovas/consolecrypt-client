use cc_automation_core::{ProjectEntry, ProjectRoot, GIT_SOURCE_FILE};
use serde_json::json;
fn metadata() -> serde_json::Value {
    json!({"version":1,"kind":"git","url":"https://example.test/repo.git","reference":"main",
        "commit":"a".repeat(40),"ca_sha256":null,"executable_paths":["script.sh"]})
}
#[test]
fn provenance_and_portable_executable_modes_are_bound_by_the_existing_revision_digest() {
    let folder = tempfile::tempdir().unwrap();
    std::fs::write(folder.path().join("script.sh"), "#!/bin/sh\nexit 0\n").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(
            folder.path().join("script.sh"),
            std::fs::Permissions::from_mode(0o700),
        )
        .unwrap();
    }
    std::fs::write(folder.path().join(GIT_SOURCE_FILE), metadata().to_string()).unwrap();
    let root = ProjectRoot::open(folder.path()).unwrap();
    let before = root.revision().unwrap();
    assert!(before
        .entries
        .iter()
        .any(|entry| matches!(entry,ProjectEntry::File {path,mode:0o700,..} if path=="script.sh")));
    let (_, stamp) = root.git_source().unwrap().unwrap();
    assert!(root
        .write_file(GIT_SOURCE_FILE, Some(&stamp), "replacement")
        .is_err());
    let mut changed = metadata();
    changed["commit"] = "b".repeat(40).into();
    std::fs::write(folder.path().join(GIT_SOURCE_FILE), changed.to_string()).unwrap();
    assert_ne!(before.digest, root.revision().unwrap().digest);
    assert_ne!(stamp, root.git_source().unwrap().unwrap().1);
}
#[test]
fn malformed_metadata_cannot_add_credentials_or_escape_the_workspace() {
    let folder = tempfile::tempdir().unwrap();
    let root = ProjectRoot::open(folder.path()).unwrap();
    for (key, value) in [
        ("url", json!("https://user:password@example.test/repo")),
        ("executable_paths", json!(["../outside"])),
        ("version", json!(2)),
        ("approve", json!(true)),
    ] {
        let mut source = metadata();
        source[key] = value;
        std::fs::write(folder.path().join(GIT_SOURCE_FILE), source.to_string()).unwrap();
        assert!(root.revision().is_err());
    }
}
