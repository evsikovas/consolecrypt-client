//! Exercises the ACTUAL safe syscall adapter. No mocked resolver or mount call.
use super::*;
use cap_std::fs::MetadataExt;
use std::{
    fs,
    os::unix::{
        fs::{symlink, PermissionsExt},
        io::AsRawFd,
    },
    sync::atomic::{AtomicU64, Ordering},
};
static NEXT: AtomicU64 = AtomicU64::new(0);
struct Owned(std::path::PathBuf);
impl Owned {
    fn new() -> Self {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "owned-publication-linux-{}-{nonce}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o700)).unwrap();
        Self(path)
    }
    fn dir(&self) -> Dir {
        Dir::open_ambient_dir(&self.0, cap_std::ambient_authority()).unwrap()
    }
}
impl Drop for Owned {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).unwrap();
    }
}

#[test]
fn supported_filesystem_comes_from_held_descriptor_and_proc_is_unsupported() {
    let own = Owned::new();
    let held = own.dir();
    supported(&held).unwrap();
    println!(
        "owned temporary filesystem magic: {:#x}",
        rustix::fs::fstatfs(&held).unwrap().f_type
    );
    let proc = Dir::open_ambient_dir("/proc", cap_std::ambient_authority()).unwrap();
    assert_eq!(
        supported(&proc).unwrap_err().kind(),
        io::ErrorKind::Unsupported
    );
    let moved = own.0.with_extension("moved");
    fs::rename(&own.0, &moved).unwrap();
    // Descriptor observation still examines the original object, not a guessed
    // current path. The separate namespace named-identity fence detects rename.
    supported(&held).unwrap();
    fs::rename(&moved, &own.0).unwrap();
}

#[test]
fn explicit_single_component_and_no_symbolic_link_resolution() {
    let own = Owned::new();
    fs::create_dir(own.0.join("actual")).unwrap();
    symlink("actual", own.0.join("alias")).unwrap();
    let dir = own.dir();
    open_child(&dir, Path::new("actual")).unwrap();
    for name in ["", ".", "..", "actual/child", "/proc", "alias"] {
        assert!(open_child(&dir, Path::new(name)).is_err(), "{name}");
    }
    assert!(open_ancestor(&dir, Path::new("alias")).is_err());
}

#[test]
fn pre_profile_mount_transition_allowed_but_managed_boundary_refuses_it() {
    let root = Dir::open_ambient_dir("/", cap_std::ambient_authority()).unwrap();
    let proc = open_ancestor(&root, Path::new("proc")).unwrap();
    assert_eq!(
        rustix::fs::fstatfs(&proc).unwrap().f_type,
        rustix::fs::PROC_SUPER_MAGIC
    );
    assert_eq!(
        open_child(&root, Path::new("proc"))
            .unwrap_err()
            .raw_os_error(),
        Some(18)
    );
}

#[test]
fn magic_proc_fd_links_are_denied_even_during_pre_profile_walk() {
    let own = Owned::new();
    let held = own.dir();
    let fds = Dir::open_ambient_dir("/proc/self/fd", cap_std::ambient_authority()).unwrap();
    let name = held.as_raw_fd().to_string();
    assert!(open_ancestor(&fds, Path::new(&name)).is_err());
    assert!(open_child(&fds, Path::new(&name)).is_err());
}

#[test]
fn rename_and_same_bytes_replacement_remain_distinct_held_directory_objects() {
    let own = Owned::new();
    fs::create_dir(own.0.join("named")).unwrap();
    fs::write(
        own.0.join("named/public-test-marker"),
        b"synthetic exact bytes",
    )
    .unwrap();
    let parent = own.dir();
    let original = open_child(&parent, Path::new("named")).unwrap();
    let identity = original.dir_metadata().unwrap();
    fs::rename(own.0.join("named"), own.0.join("held")).unwrap();
    fs::create_dir(own.0.join("named")).unwrap();
    fs::write(
        own.0.join("named/public-test-marker"),
        b"synthetic exact bytes",
    )
    .unwrap();
    let replacement = open_child(&parent, Path::new("named")).unwrap();
    let next = replacement.dir_metadata().unwrap();
    assert_eq!(identity.dev(), next.dev());
    assert_ne!(identity.ino(), next.ino());
    assert_eq!(
        original.read("public-test-marker").unwrap(),
        replacement.read("public-test-marker").unwrap()
    );
    // Production namespace retains/checks its captured full identity, not bytes.
}

#[test]
fn actual_read_create_and_metadata_use_the_same_strict_adapter_without_overwrite() {
    use std::io::{Read, Write};
    let own = Owned::new();
    let dir = own.dir();
    let mut file = create_file(&dir, Path::new("owned.txt"), 0o600).unwrap();
    file.write_all(b"owned actual read bytes").unwrap();
    file.sync_all().unwrap();
    drop(file);
    assert!(create_file(&dir, Path::new("owned.txt"), 0o600).is_err());
    assert!(create_file(&dir, Path::new("invalid.txt"), 0o4600).is_err());
    let metadata = metadata(&dir, Path::new("owned.txt")).unwrap();
    assert!(metadata.is_file());
    assert_eq!(metadata.mode() & 0o777, 0o600);
    let mut bytes = Vec::new();
    read_file(&dir, Path::new("owned.txt"))
        .unwrap()
        .read_to_end(&mut bytes)
        .unwrap();
    assert_eq!(bytes, b"owned actual read bytes");
    symlink("owned.txt", own.0.join("alias.txt")).unwrap();
    assert!(read_file(&dir, Path::new("alias.txt")).is_err());
    assert!(super::metadata(&dir, Path::new("alias.txt")).is_err());
}

#[test]
#[ignore = "requires explicit owned Docker same-filesystem nested bind fixture"]
fn same_filesystem_bind_is_rejected_despite_equal_device_and_preserves_owned_bytes() {
    let raw = std::env::var("CC_PUBLICATION_SAME_FS_BIND_PARENT")
        .expect("explicit owned fixture required");
    assert!(raw.starts_with("/tmp/owned-publication-bind-"));
    let parent = Dir::open_ambient_dir(&raw, cap_std::ambient_authority()).unwrap();
    supported(&parent).unwrap();
    println!(
        "owned bind filesystem magic: {:#x}",
        rustix::fs::fstatfs(&parent).unwrap().f_type
    );
    let source = parent.open_dir("payload").unwrap();
    let mounted = parent.open_dir("mounted").unwrap();
    let source_meta = source.dir_metadata().unwrap();
    let mount_meta = mounted.dir_metadata().unwrap();
    assert_eq!(parent.dir_metadata().unwrap().dev(), mount_meta.dev());
    assert_eq!(
        (source_meta.dev(), source_meta.ino()),
        (mount_meta.dev(), mount_meta.ino())
    );
    assert_eq!(
        mounted.read("public-test-marker").unwrap(),
        b"owned nested bind marker"
    );
    // A mere dev/inode comparison ACCEPTS this. The actual resolver MUST not.
    assert_eq!(
        open_child(&parent, Path::new("mounted"))
            .unwrap_err()
            .raw_os_error(),
        Some(18)
    );
    for error in [
        read_file(&parent, Path::new("mounted/public-test-marker")).unwrap_err(),
        metadata(&parent, Path::new("mounted/public-test-marker")).unwrap_err(),
        create_file(&parent, Path::new("mounted/never-created.txt"), 0o600).unwrap_err(),
    ] {
        assert_eq!(error.raw_os_error(), Some(18));
    }
    assert!(!source.exists("never-created.txt"));
    assert_eq!(
        mounted.read("public-test-marker").unwrap(),
        b"owned nested bind marker"
    );
    assert_eq!(source.dir_metadata().unwrap().ino(), source_meta.ino());
}
