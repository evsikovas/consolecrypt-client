use cc_automation_core::ProjectRoot;
use std::sync::{Arc, Barrier};

#[test]
fn role_scaffold_is_complete_and_existing_user_paths_are_preserved() {
    let directory = tempfile::tempdir().unwrap();
    let project = ProjectRoot::open(directory.path()).unwrap();
    let task = project.create_role("example_role").unwrap();
    assert_eq!(task.path, "roles/example_role/tasks/main.yml");
    assert!(task.text.contains("ansible.builtin.debug"));
    for name in [
        "defaults",
        "files",
        "handlers",
        "meta",
        "tasks",
        "templates",
        "tests",
        "vars",
    ] {
        assert!(directory
            .path()
            .join("roles/example_role")
            .join(name)
            .is_dir());
    }
    std::fs::write(
        directory.path().join("roles/example_role/tasks/main.yml"),
        "user changes",
    )
    .unwrap();
    assert!(project.create_role("example_role").is_err());
    assert_eq!(
        std::fs::read_to_string(directory.path().join("roles/example_role/tasks/main.yml"))
            .unwrap(),
        "user changes"
    );
    std::fs::create_dir(directory.path().join("roles/empty_user_role")).unwrap();
    assert!(project.create_role("empty_user_role").is_err());
    assert_eq!(
        std::fs::read_dir(directory.path().join("roles/empty_user_role"))
            .unwrap()
            .count(),
        0
    );
    std::fs::write(directory.path().join("roles/user_file"), "keep").unwrap();
    assert!(project.create_role("user_file").is_err());
    assert_eq!(
        std::fs::read_to_string(directory.path().join("roles/user_file")).unwrap(),
        "keep"
    );
    for name in ["../bad", "bad/name", "Upper", "bad-name", "", "_private"] {
        assert!(project.create_role(name).is_err());
    }
}

#[test]
fn racing_creators_never_merge_or_replace_a_published_role() {
    let directory = tempfile::tempdir().unwrap();
    let barrier = Arc::new(Barrier::new(8));
    let threads: Vec<_> = (0..8)
        .map(|_| {
            let root = directory.path().to_owned();
            let barrier = barrier.clone();
            std::thread::spawn(move || {
                let project = ProjectRoot::open(&root).unwrap();
                barrier.wait();
                project.create_role("shared_role").is_ok()
            })
        })
        .collect();
    assert_eq!(
        threads
            .into_iter()
            .map(|thread| thread.join().unwrap())
            .filter(|success| *success)
            .count(),
        1
    );
    let project = ProjectRoot::open(directory.path()).unwrap();
    assert!(project
        .read_file("roles/shared_role/tasks/main.yml")
        .unwrap()
        .text
        .contains("ansible.builtin.debug"));
    assert!(!std::fs::read_dir(directory.path().join("roles"))
        .unwrap()
        .any(|entry| entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with(".cc-role-")));
}

#[test]
fn first_role_budgets_its_new_parent_before_writing_anything() {
    let directory = tempfile::tempdir().unwrap();
    // The snapshot entry limit is 20,000. A first role needs 17 entries,
    // including its `roles` parent, not just the 16 entries below that parent.
    for index in 0..19_984 {
        std::fs::create_dir(directory.path().join(format!("fixture_{index:05}"))).unwrap();
    }
    let project = ProjectRoot::open(directory.path()).unwrap();
    assert!(project.create_role("would_overflow").is_err());
    assert!(!directory.path().join("roles").exists());
    assert_eq!(std::fs::read_dir(directory.path()).unwrap().count(), 19_984);
}
