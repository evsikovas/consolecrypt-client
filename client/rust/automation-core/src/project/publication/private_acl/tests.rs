//! Only fresh owned temporary ACLs are changed. No real profile or global path.
use super::*;
use cap_std::fs::Dir;
use std::{
    fs,
    os::windows::fs::{MetadataExt, OpenOptionsExt},
    path::{Path, PathBuf},
};

const EVERYONE: &[u8] = &[1, 1, 0, 0, 0, 0, 0, 1, 0, 0, 0, 0];
const SYNTHETIC_USER: &[u8] = &[1, 2, 0, 0, 0, 0, 0, 5, 21, 0, 0, 0, 42, 0, 0, 0];

fn broad(mut raw: Vec<u8>) -> Vec<u8> {
    let start = u32_at(&raw, 16).unwrap() as usize;
    raw.extend_from_slice(&[0, 3]);
    raw.extend_from_slice(&((8 + EVERYONE.len()) as u16).to_le_bytes());
    raw.extend_from_slice(&FILE_ALL.to_le_bytes());
    raw.extend_from_slice(EVERYONE);
    let size = raw.len() - start;
    raw[start + 2..start + 4].copy_from_slice(&(size as u16).to_le_bytes());
    raw[start + 4..start + 6].copy_from_slice(&4u16.to_le_bytes());
    raw
}
pub(in crate::project::publication) fn protect_owned_fixture(path: &Path) {
    kernel::fixture_set(path, &new_descriptor(&kernel::fixture_user()).unwrap());
}
struct Fixture {
    path: PathBuf,
    _owned: tempfile::TempDir,
}
impl Fixture {
    fn new() -> Self {
        let owned = tempfile::Builder::new()
            .prefix("owned-private-acl-")
            .tempdir()
            .unwrap();
        let path = owned.path().to_owned();
        protect_owned_fixture(&path);
        Self {
            path,
            _owned: owned,
        }
    }
    fn dir(&self) -> Dir {
        Dir::open_ambient_dir(&self.path, cap_std::ambient_authority()).unwrap()
    }
}

#[test]
fn publication_acl_strict_descriptor_bounds_null_future_and_principal_matrix() {
    let raw = new_descriptor(SYNTHETIC_USER).unwrap();
    descriptor(&raw, SYNTHETIC_USER, true).unwrap();
    let acl = u32_at(&raw, 16).unwrap() as usize;
    let mut bad = Vec::new();
    for end in [0, 19, 21, raw.len() - 1] {
        bad.push(raw[..end].to_vec());
    }
    bad.push(vec![0; MAX_DESCRIPTOR + 1]);
    for (offset, value) in [
        (0, 2),
        (2, 0),
        (3, 0),
        (acl, 4),
        (acl + 4, 65),
        (acl + 8, 1),
        (acl + 8, 5),
        (acl + 8, 9),
        (acl + 8, 255),
    ] {
        let mut changed = raw.clone();
        changed[offset] = value;
        bad.push(changed);
    }
    let mut null = raw.clone();
    null[16..20].fill(0);
    bad.push(null);
    let mut owner = raw.clone();
    owner[20 + 8] ^= 1;
    bad.push(owner);
    let mut unaligned = raw.clone();
    unaligned[16] += 1;
    bad.push(unaligned);
    let mut oversized_ace = raw.clone();
    oversized_ace[acl + 10..acl + 12].copy_from_slice(&u16::MAX.to_le_bytes());
    bad.push(oversized_ace);
    bad.push(broad(raw.clone()));
    for changed in bad {
        assert!(descriptor(&changed, SYNTHETIC_USER, true).is_err());
    }
    let mut unprotected = raw.clone();
    unprotected[3] &= !0x10;
    descriptor(&unprotected, SYNTHETIC_USER, false).unwrap();
    assert!(descriptor(&unprotected, SYNTHETIC_USER, true).is_err());
    let mut no_child_inheritance = raw.clone();
    no_child_inheritance[acl + 9] = 0;
    descriptor(&no_child_inheritance, SYNTHETIC_USER, false).unwrap();
    assert!(descriptor(&no_child_inheritance, SYNTHETIC_USER, true).is_err());
    assert!(new_descriptor(SYSTEM).is_err());
    assert!(new_descriptor(ADMINISTRATORS).is_err());
}

#[test]
fn publication_acl_actual_create_is_protected_inherited_and_never_adopts_existing_name() {
    let f = Fixture::new();
    let parent = f.dir();
    verify(&parent, true).unwrap();
    let created = create(&parent, "private-child").unwrap();
    verify(&created, true).unwrap();
    let before = super::super::identity(&created).unwrap();
    assert!(create(&parent, "private-child").is_err());
    assert_eq!(super::super::identity(&created).unwrap(), before);
    // Check real child FILE inheritance before the first synthetic byte.
    let mut output = fs::OpenOptions::new()
        .write(true)
        .read(true)
        .create_new(true)
        .open(f.path.join("private-child/synthetic.yml"))
        .unwrap();
    descriptor(
        &kernel::fixture_security(&output),
        &kernel::fixture_user(),
        false,
    )
    .unwrap();
    use std::io::Write;
    output
        .write_all(b"synthetic owned plaintext only\n")
        .unwrap();
    assert_eq!(
        fs::read(f.path.join("private-child/synthetic.yml")).unwrap(),
        b"synthetic owned plaintext only\n"
    );
}

#[test]
fn publication_acl_actual_broad_parent_is_not_repaired_or_used_to_create_family() {
    let f = Fixture::new();
    let user = kernel::fixture_user();
    kernel::fixture_set(&f.path, &broad(new_descriptor(&user).unwrap()));
    let parent = f.dir();
    let file = parent.try_clone().unwrap().into_std_file();
    let before = kernel::fixture_security(&file);
    assert!(verify(&parent, false).is_err());
    assert!(create(&parent, "must-not-create").is_err());
    assert!(!f.path.join("must-not-create").exists());
    assert_eq!(
        kernel::fixture_security(&file),
        before,
        "production verification never repairs an existing ACL"
    );
}

#[test]
fn publication_acl_actual_same_handle_acl_change_is_rechecked_not_cached_by_fileid() {
    let f = Fixture::new();
    let parent = f.dir();
    let before = super::super::identity(&parent).unwrap();
    verify(&parent, true).unwrap();
    kernel::fixture_set(
        &f.path,
        &broad(new_descriptor(&kernel::fixture_user()).unwrap()),
    );
    assert_eq!(super::super::identity(&parent).unwrap(), before);
    assert!(verify(&parent, true).is_err());
    assert!(create(&parent, "late-child").is_err());
    assert!(!f.path.join("late-child").exists());
}

#[test]
fn publication_acl_actual_directory_handle_without_read_control_is_explicit_error() {
    let f = Fixture::new();
    let file = fs::OpenOptions::new()
        .access_mode(0x80 | 0x0010_0000)
        .custom_flags(0x0200_0000 | 0x0020_0000)
        .share_mode(3)
        .open(&f.path)
        .unwrap();
    let parent = Dir::from_std_file(file);
    assert!(verify(&parent, false).is_err());
    assert!(create(&parent, "no-acl-fallback").is_err());
    assert!(!f.path.join("no-acl-fallback").exists());
}

#[test]
fn publication_acl_actual_created_handle_pins_name_and_distinguishes_other_directory() {
    let f = Fixture::new();
    let parent = f.dir();
    let created = create(&parent, "held-child").unwrap();
    let other = create(&parent, "other-child").unwrap();
    assert_ne!(
        super::super::identity(&created).unwrap(),
        super::super::identity(&other).unwrap()
    );
    let error = fs::rename(f.path.join("held-child"), f.path.join("renamed")).unwrap_err();
    assert!(matches!(error.raw_os_error(), Some(5 | 32)));
    assert!(f.path.join("held-child").is_dir() && !f.path.join("renamed").exists());
    verify(&created, true).unwrap();
    drop(created);
    fs::rename(f.path.join("held-child"), f.path.join("renamed")).unwrap();
    fs::rename(f.path.join("renamed"), f.path.join("held-child")).unwrap();
}

#[test]
fn publication_acl_actual_junction_handle_is_rejected_without_following_target() {
    let f = Fixture::new();
    let parent = f.dir();
    let target = create(&parent, "junction-target").unwrap();
    drop(target);
    let alias: PathBuf = f.path.join("junction-alias").components().collect();
    let target: PathBuf = f.path.join("junction-target").components().collect();
    let result = std::process::Command::new("cmd")
        .args(["/d", "/c", "mklink", "/J"])
        .arg(&alias)
        .arg(&target)
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "owned local junction creation must succeed"
    );
    let file = fs::OpenOptions::new()
        .access_mode(0x0002_0000 | 0x80 | 0x0010_0000)
        .custom_flags(0x0200_0000 | 0x0020_0000)
        .share_mode(3)
        .open(&alias)
        .unwrap();
    assert_ne!(file.metadata().unwrap().file_attributes() & 0x400, 0);
    let linked = Dir::from_std_file(file);
    assert!(verify(&linked, false).is_err());
    assert!(create(&linked, "no-follow").is_err());
    assert!(!target.join("no-follow").exists());
    drop(linked);
    fs::remove_dir(&alias).unwrap(); // Exact owned junction inode only, no recursion.
    assert!(target.is_dir());
}

#[test]
fn publication_acl_actual_namespace_family_and_stage_are_protected_before_plaintext() {
    let f = Fixture::new();
    let parent = f.dir();
    let profile = create(&parent, "profile").unwrap();
    drop(profile);
    let project = uuid::Uuid::new_v4();
    let path = f
        .path
        .join("profile/automation/projects")
        .join(project.to_string());
    fs::create_dir_all(&path).unwrap();
    fs::create_dir(f.path.join("candidate")).unwrap();
    let base = crate::ProjectRoot::open(&path).unwrap();
    let candidate = crate::ProjectRoot::open(&f.path.join("candidate")).unwrap();
    let namespace = super::super::namespace::ManagedNamespace::open(
        &f.path.join("profile"),
        project,
        &base,
        &candidate,
    )
    .unwrap();
    verify(&namespace.root, true).unwrap();
    let stage = super::super::namespace::mkdir(&namespace.root, "stage-owned-test").unwrap();
    verify(&stage, true).unwrap();
    assert!(
        stage.entries().unwrap().next().is_none(),
        "ACL is proved before any plaintext appears"
    );
    namespace.check().unwrap();
}
