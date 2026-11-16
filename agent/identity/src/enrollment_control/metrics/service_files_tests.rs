//! Real Linux filesystem/Files cleanup causals. No Actor/permit is fabricated.
use super::*;
fn owned_files() -> Files {
    let mut random = [0u8; 16];
    getrandom::fill(&mut random).unwrap();
    let directory =
        std::env::temp_dir().join(format!("cc-owned-service-files-{}", hex::encode(random)));
    fs::DirBuilder::new()
        .mode(0o700)
        .create(&directory)
        .unwrap();
    let m = directory.symlink_metadata().unwrap();
    let lock = OpenOptions::new()
        .read(true)
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(directory.with_extension("lock"))
        .unwrap();
    Files {
        directory_fd: File::open(&directory).unwrap(),
        directory,
        identity: (m.dev(), m.ino()),
        paths: Vec::new(),
        session: hex::encode(random),
        unit: String::new(),
        _lock: lock,
        cleanup_allowed: false,
        disk: true,
    }
}
#[test]
fn original_files_cleanup_refuses_replaced_leaf_and_preserves_foreign_bytes() {
    let mut files = owned_files();
    let root = files.directory.clone();
    let lock = root.with_extension("lock");
    files
        .write("scrape.json", b"original private fixture")
        .unwrap();
    let leaf = root.join("scrape.json");
    fs::rename(&leaf, root.join("original-held-away")).unwrap();
    let mut foreign = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o400)
        .open(&leaf)
        .unwrap();
    foreign.write_all(b"foreign bytes").unwrap();
    foreign.sync_all().unwrap();
    drop(foreign);
    assert_eq!(files.cleanup(), Err(Error::UncertainCommit));
    assert_eq!(fs::read(&leaf).unwrap(), b"foreign bytes");
    assert_eq!(
        fs::read(root.join("original-held-away")).unwrap(),
        b"original private fixture"
    );
    // Test-owned explicit teardown, not the product's refused unlink path.
    drop(files);
    fs::remove_file(&leaf).unwrap();
    fs::remove_file(root.join("original-held-away")).unwrap();
    fs::remove_dir(&root).unwrap();
    fs::remove_file(&lock).unwrap();
}
#[test]
fn failed_exclusive_create_never_registers_existing_leaf_for_cleanup() {
    let mut files = owned_files();
    let root = files.directory.clone();
    let lock = root.with_extension("lock");
    let leaf = root.join("scrape.json");
    fs::write(&leaf, b"preexisting").unwrap();
    assert!(files.write("scrape.json", b"replacement").is_err());
    assert!(files.paths.is_empty());
    assert!(files.cleanup().is_err());
    assert_eq!(fs::read(&leaf).unwrap(), b"preexisting");
    drop(files);
    fs::remove_file(&leaf).unwrap();
    fs::remove_dir(&root).unwrap();
    fs::remove_file(&lock).unwrap();
}
#[test]
fn original_regular_leaf_can_be_removed_after_original_owner_allows_cleanup() {
    let mut files = owned_files();
    let root = files.directory.clone();
    let lock = root.with_extension("lock");
    files.write("scrape.json", b"owned fixture").unwrap();
    files.cleanup().unwrap();
    assert!(!root.exists());
    drop(files);
    fs::remove_file(lock).unwrap();
}

#[test]
fn renamed_namespace_with_held_original_directory_does_not_report_cleanup() {
    let mut files = owned_files();
    let root = files.directory.clone();
    let lock = root.with_extension("lock");
    files.write("scrape.json", b"retained owned fixture").unwrap();
    let moved = root.with_extension("moved");
    fs::rename(&root, &moved).unwrap();
    assert_eq!(files.cleanup(), Err(Error::UncertainCommit));
    assert_eq!(files.paths.len(), 1);
    assert!(files.directory_fd.metadata().unwrap().nlink() > 0);
    assert_eq!(files.paths[0].1.as_ref().unwrap().file.metadata().unwrap().nlink(), 1);
    assert_eq!(fs::read(moved.join("scrape.json")).unwrap(), b"retained owned fixture");
    // The SAME original namespace is restored by this test's owned teardown.
    fs::rename(&moved, &root).unwrap();
    files.cleanup().unwrap();
    assert!(!root.exists());
    assert_eq!(files.directory_fd.metadata().unwrap().nlink(), 0);
    files.cleanup().unwrap(); // Actually-unlinked original FD is idempotent.
    drop(files);
    fs::remove_file(lock).unwrap();
}
