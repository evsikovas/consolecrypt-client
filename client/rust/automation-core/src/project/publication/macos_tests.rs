//! AUTHORED, NOT RUN here: real Darwin/APFS syscalls and owned temporary roots.
//! No ACL setter touches a profile, user directory or any ambient application data.
use super::*;
use std::{
    fs,
    io::Write,
    os::unix::fs::{MetadataExt, PermissionsExt},
};

// This module is cfg(test). Probe only the newly owned fixture after its
// mandatory guard failed; never print paths, pointers, ACEs or exported bytes.
// Each probe is a fresh observation, not a replacement for the failed guard.
fn owned_acl_export(label: &str, acquire: impl FnOnce() -> *mut c_void) -> Option<Vec<u8>> {
    // SAFETY: callers acquire a libc-owned ACL from the live owned directory
    // or acl_init. Acl frees it on every branch. errno is thread-local; the
    // buffer is bounded exactly as in the production guard and never printed.
    unsafe {
        *libc::__error() = 0;
        let pointer = acquire();
        let error = *libc::__error();
        eprintln!(
            "owned-acl-probe {label} acquire_null={} errno={error}",
            pointer.is_null()
        );
        let acl = Acl::new(pointer).ok()?;
        *libc::__error() = 0;
        let valid = acl_valid(acl.0.as_ptr());
        let error = *libc::__error();
        eprintln!("owned-acl-probe {label} valid={valid} errno={error}");
        if valid != 0 {
            return None;
        }
        *libc::__error() = 0;
        let size = acl_size(acl.0.as_ptr());
        let error = *libc::__error();
        eprintln!("owned-acl-probe {label} size={size} errno={error}");
        if !(1..=8192).contains(&size) {
            return None;
        }
        let mut bytes = vec![0u8; size as usize];
        *libc::__error() = 0;
        let copied = acl_copy_ext(bytes.as_mut_ptr().cast(), acl.0.as_ptr(), size);
        let error = *libc::__error();
        eprintln!("owned-acl-probe {label} copied={copied} errno={error}");
        (copied == size).then_some(bytes)
    }
}
fn diagnose_owned_acl(directory: &Dir) {
    // SAFETY: the fixture retains this descriptor throughout the read-only probe.
    let actual = owned_acl_export("actual", || unsafe {
        acl_get_fd_np(directory.as_raw_fd(), 0x100)
    });
    // SAFETY: acl_init returns a new libc allocation owned by the probe.
    let empty = owned_acl_export("sdk-empty", || unsafe { acl_init(0) });
    match (actual, empty) {
        (Some(actual), Some(empty)) => eprintln!(
            "owned-acl-probe compared=true actual_size={} empty_size={} equal={}",
            actual.len(),
            empty.len(),
            actual == empty
        ),
        _ => eprintln!("owned-acl-probe compared=false"),
    }
}

struct Owned {
    directory: Dir,
    path: std::path::PathBuf,
    _temp: tempfile::TempDir,
}
impl Owned {
    fn new() -> Self {
        let temp = tempfile::Builder::new()
            .prefix("cc-owned-darwin-publication-")
            .tempdir()
            .unwrap();
        // Test-only resolution of the newly owned temp root, not a production
        // fallback for a selected profile with a symlink ancestor.
        let path = temp.path().canonicalize().unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o700)).unwrap();
        let directory = Dir::open_ambient_dir(&path, cap_std::ambient_authority()).unwrap();
        supported(&directory)
            .expect("actual writable local APFS required; unsupported is NOT a skip");
        let acl_result = verify_empty_acl(&directory);
        if acl_result.is_err() {
            diagnose_owned_acl(&directory);
        }
        acl_result.expect("owned fixture must begin with empty extended ACL");
        Self {
            directory,
            path,
            _temp: temp,
        }
    }
}

#[test]
fn created_child_and_file_are_private_before_synthetic_plaintext() {
    let own = Owned::new();
    let child = super::super::mkdir(&own.directory, "created").unwrap();
    verify_empty_acl(&child).unwrap();
    assert_eq!(
        fs::metadata(own.path.join("created")).unwrap().mode() & 0o777,
        0o700
    );
    let mut file = create_file(&child, Path::new("plain.txt"), 0o600).unwrap();
    verify_empty_acl(&file).unwrap();
    assert_eq!(file.metadata().unwrap().len(), 0);
    file.write_all(b"owned synthetic plaintext").unwrap();
    full_sync_file(&file).unwrap();
    assert_eq!(
        fs::metadata(own.path.join("created/plain.txt"))
            .unwrap()
            .mode()
            & 0o777,
        0o600
    );
    assert_eq!(
        fs::read(own.path.join("created/plain.txt")).unwrap(),
        b"owned synthetic plaintext"
    );
}

#[test]
fn symlink_leaf_and_nested_directory_are_never_followed() {
    let own = Owned::new();
    let sibling = own.path.join("sibling");
    fs::create_dir(&sibling).unwrap();
    fs::write(sibling.join("sentinel"), b"unchanged").unwrap();
    std::os::unix::fs::symlink("sibling", own.path.join("linked-dir")).unwrap();
    std::os::unix::fs::symlink("sibling/sentinel", own.path.join("linked-file")).unwrap();
    assert!(open_child(&own.directory, Path::new("linked-dir")).is_err());
    assert!(metadata(&own.directory, Path::new("linked-file")).is_err());
    assert!(read_file(&own.directory, Path::new("linked-dir/sentinel")).is_err());
    assert!(create_file(&own.directory, Path::new("linked-dir/new"), 0o600).is_err());
    assert!(create_file(&own.directory, Path::new("linked-file"), 0o600).is_err());
    assert_eq!(fs::read(sibling.join("sentinel")).unwrap(), b"unchanged");
    assert!(!sibling.join("new").exists());
}

#[test]
fn create_new_never_truncates_an_existing_regular_file() {
    let own = Owned::new();
    fs::write(own.path.join("existing"), b"retained sentinel").unwrap();
    let before = fs::metadata(own.path.join("existing")).unwrap().ino();
    assert_eq!(
        create_file(&own.directory, Path::new("existing"), 0o600)
            .unwrap_err()
            .kind(),
        io::ErrorKind::AlreadyExists
    );
    assert_eq!(
        fs::metadata(own.path.join("existing")).unwrap().ino(),
        before
    );
    assert_eq!(
        fs::read(own.path.join("existing")).unwrap(),
        b"retained sentinel"
    );
}

#[test]
fn kernel_physical_edge_rejects_wrong_name_and_renamed_open_file() {
    let own = Owned::new();
    let file = create_file(&own.directory, Path::new("captured"), 0o600).unwrap();
    check_edge(&own.directory, Path::new("captured"), &file).unwrap();
    assert!(check_edge(&own.directory, Path::new("other"), &file).is_err());
    fs::rename(own.path.join("captured"), own.path.join("moved")).unwrap();
    fs::write(own.path.join("captured"), b"replacement").unwrap();
    assert!(check_edge(&own.directory, Path::new("captured"), &file).is_err());
    assert_eq!(fs::read(own.path.join("captured")).unwrap(), b"replacement");
    assert_eq!(fs::metadata(own.path.join("moved")).unwrap().len(), 0);
}

#[test]
fn actual_rename_excl_preserves_existing_directory_file_and_symlink() {
    let own = Owned::new();
    let stage = super::super::mkdir(&own.directory, "stage").unwrap();
    fs::write(own.path.join("stage/value"), b"reviewed source").unwrap();
    let expected = super::super::identity(&stage).unwrap();
    for kind in 0..3 {
        match kind {
            0 => fs::create_dir(own.path.join("target")).unwrap(),
            1 => fs::write(own.path.join("target"), b"existing target").unwrap(),
            _ => std::os::unix::fs::symlink("stage", own.path.join("target")).unwrap(),
        }
        assert!(
            super::super::super::rename_new(&own.directory, "stage", "target", &expected).is_err()
        );
        assert_eq!(
            fs::read(own.path.join("stage/value")).unwrap(),
            b"reviewed source"
        );
        if kind == 0 {
            fs::remove_dir(own.path.join("target")).unwrap();
        } else {
            if kind == 1 {
                assert_eq!(
                    fs::read(own.path.join("target")).unwrap(),
                    b"existing target"
                );
            } else {
                assert!(
                    fs::symlink_metadata(own.path.join("target"))
                        .unwrap()
                        .file_type()
                        .is_symlink()
                );
            }
            fs::remove_file(own.path.join("target")).unwrap();
        }
    }
    super::super::super::rename_new(&own.directory, "stage", "target", &expected).unwrap();
    assert_eq!(
        super::super::identity(&open_child(&own.directory, Path::new("target")).unwrap()).unwrap(),
        expected
    );
}

#[test]
fn same_inode_extended_acl_mutation_is_denied_without_repair() {
    let own = Owned::new();
    let before = fs::metadata(&own.path).unwrap().ino();
    let status = std::process::Command::new("/bin/chmod")
        .args(["+a", "everyone allow read,search"])
        .arg(&own.path)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .unwrap();
    assert!(
        status.success(),
        "owned ACL mutation prerequisite must actually succeed"
    );
    assert_eq!(fs::metadata(&own.path).unwrap().ino(), before);
    assert!(verify_empty_acl(&own.directory).is_err());
    assert!(create_file(&own.directory, Path::new("must-not-write"), 0o600).is_err());
    assert!(!own.path.join("must-not-write").exists());
    assert!(
        verify_empty_acl(&own.directory).is_err(),
        "deny must not repair the ACL"
    );
}

#[test]
fn retained_namespace_denies_same_byte_family_replacement() {
    let own = Owned::new();
    let project = uuid::Uuid::new_v4();
    let namespace =
        super::super::ManagedNamespace::fresh_project(&own.path, project, true).unwrap();
    let family = own
        .path
        .join("automation/project-revisions")
        .join(project.to_string());
    fs::write(family.join("synthetic"), b"same bytes").unwrap();
    let original_id = super::super::identity(&namespace.root).unwrap();
    let moved = own.path.join("retained-old-family");
    fs::rename(&family, &moved).unwrap();
    fs::create_dir(&family).unwrap();
    fs::set_permissions(&family, fs::Permissions::from_mode(0o700)).unwrap();
    fs::write(family.join("synthetic"), b"same bytes").unwrap();
    assert_eq!(
        super::super::identity(&namespace.root).unwrap(),
        original_id
    );
    assert!(namespace.check().is_err());
    assert_eq!(fs::read(moved.join("synthetic")).unwrap(), b"same bytes");
    assert_eq!(fs::read(family.join("synthetic")).unwrap(), b"same bytes");
}
