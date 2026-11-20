//! Own EXT-backed directory only; no host state/keychain or production root.
use super::*;
use cap_std::{ambient_authority, fs::Dir};
use std::{
    fs,
    os::unix::fs::{symlink, MetadataExt, PermissionsExt},
    path::{Path, PathBuf},
};
fn fixture() -> tempfile::TempDir {
    let root = PathBuf::from(
        std::env::var_os("CC_SVC_IDENTITY_TEST_ROOT")
            .expect("provide the owned EXT-backed identity test fixture"),
    );
    assert!(
        root.is_absolute()
            && !fs::symlink_metadata(&root)
                .unwrap()
                .file_type()
                .is_symlink()
    );
    assert_eq!(
        fs::metadata(&root).unwrap().uid(),
        rustix::process::geteuid().as_raw()
    );
    let own = tempfile::tempdir_in(root).unwrap();
    fs::set_permissions(own.path(), fs::Permissions::from_mode(0o700)).unwrap();
    own
}
fn parent(path: &Path) -> Dir {
    Dir::open_ambient_dir(path, ambient_authority()).unwrap()
}
fn location(root: &Path, id: Uuid) -> PathBuf {
    root.join(format!("service-identity-{id}"))
}
#[test]
fn actual_filesystem_reload_same_public_identity_and_exclusive_create_cas() {
    let own = fixture();
    let id = Uuid::new_v4();
    let held_parent = parent(own.path());
    assert!(rustix::fs::fcntl_getfl(&held_parent)
        .unwrap()
        .contains(rustix::fs::OFlags::PATH));
    let original = ServiceIdentity::create(held_parent, id).unwrap();
    let facts = original.facts().clone();
    let key = location(own.path(), id).join("identity.v1");
    let bytes = Zeroizing::new(fs::read(&key).unwrap());
    assert!(ServiceIdentity::open_existing(parent(own.path()), id).is_err());
    assert!(ServiceIdentity::create(parent(own.path()), id).is_err());
    original.recheck().unwrap();
    drop(original);
    let restarted = ServiceIdentity::open_existing(parent(own.path()), id).unwrap();
    assert_eq!(restarted.facts(), &facts);
    restarted.recheck().unwrap();
    assert!(Zeroizing::new(fs::read(&key).unwrap()).as_slice() == bytes.as_slice());
}

#[tokio::test]
async fn reloaded_node_keeps_original_lock_until_close_and_identity_is_not_enrollment() {
    let own = fixture();
    let id = Uuid::new_v4();
    let initial = ServiceIdentity::create(parent(own.path()), id).unwrap();
    let facts = initial.facts().clone();
    drop(initial);
    let reopened = ServiceIdentity::open_existing(parent(own.path()), id).unwrap();
    let mut node = crate::ServiceNode::with_identity(reopened).unwrap();
    assert_eq!(node.public_keys(), &facts.public_keys);
    assert!(ServiceIdentity::open_existing(parent(own.path()), id).is_err());
    assert!(node
        .activate_due(cc_models::automation::JobMode::Apply)
        .await
        .is_err());
    assert!(node.shutdown().await);
    let loaded = ServiceIdentity::open_existing(parent(own.path()), id).unwrap();
    assert_eq!(loaded.facts(), &facts);
}
#[test]
fn corrupt_future_and_partial_state_never_repaired_or_regenerated() {
    let own = fixture();
    let id = Uuid::new_v4();
    drop(ServiceIdentity::create(parent(own.path()), id).unwrap());
    let file = location(own.path(), id).join("identity.v1");
    let original = Zeroizing::new(fs::read(&file).unwrap());
    let mut future = original.clone();
    future[7] = 2;
    let digest = checksum(&future[..PREFIX]);
    future[PREFIX..].copy_from_slice(&digest);
    fs::write(&file, &future).unwrap();
    assert!(ServiceIdentity::open_existing(parent(own.path()), id).is_err());
    assert!(ServiceIdentity::create(parent(own.path()), id).is_err());
    assert!(Zeroizing::new(fs::read(&file).unwrap()).as_slice() == future.as_slice());
    fs::write(&file, b"partial").unwrap();
    assert!(ServiceIdentity::open_existing(parent(own.path()), id).is_err());
    assert!(ServiceIdentity::create(parent(own.path()), id).is_err());
    assert_eq!(fs::read(&file).unwrap(), b"partial");
    let empty = Uuid::new_v4();
    fs::create_dir(location(own.path(), empty)).unwrap();
    fs::set_permissions(
        location(own.path(), empty),
        fs::Permissions::from_mode(0o700),
    )
    .unwrap();
    assert!(ServiceIdentity::open_existing(parent(own.path()), empty).is_err());
    assert!(ServiceIdentity::create(parent(own.path()), empty).is_err());
    assert!(fs::read_dir(location(own.path(), empty))
        .unwrap()
        .next()
        .is_none());
}
#[test]
fn symlink_hardlink_permissions_and_unknown_entries_deny_without_touching_destination() {
    let own = fixture();
    let id = Uuid::new_v4();
    drop(ServiceIdentity::create(parent(own.path()), id).unwrap());
    let root = location(own.path(), id);
    let file = root.join("identity.v1");
    let original = Zeroizing::new(fs::read(&file).unwrap());
    let external = own.path().join("outside");
    fs::write(&external, b"outside unchanged").unwrap();
    fs::remove_file(&file).unwrap();
    symlink(&external, &file).unwrap();
    assert!(ServiceIdentity::open_existing(parent(own.path()), id).is_err());
    assert_eq!(fs::read(&external).unwrap(), b"outside unchanged");
    fs::remove_file(&file).unwrap();
    fs::write(&file, &original).unwrap();
    fs::set_permissions(&file, fs::Permissions::from_mode(0o600)).unwrap();
    let linked = own.path().join("extra-link");
    fs::hard_link(&file, &linked).unwrap();
    assert!(ServiceIdentity::open_existing(parent(own.path()), id).is_err());
    fs::remove_file(linked).unwrap();
    fs::set_permissions(&file, fs::Permissions::from_mode(0o640)).unwrap();
    assert!(ServiceIdentity::open_existing(parent(own.path()), id).is_err());
    fs::set_permissions(&file, fs::Permissions::from_mode(0o600)).unwrap();
    fs::create_dir(root.join("future-state")).unwrap();
    assert!(ServiceIdentity::open_existing(parent(own.path()), id).is_err());
    assert!(Zeroizing::new(fs::read(&file).unwrap()).as_slice() == original.as_slice());
}
#[test]
fn held_named_root_replacement_and_key_replacement_deny_even_identical_bytes() {
    let own = fixture();
    let id = Uuid::new_v4();
    let held = ServiceIdentity::create(parent(own.path()), id).unwrap();
    let root = location(own.path(), id);
    let file = root.join("identity.v1");
    let bytes = Zeroizing::new(fs::read(&file).unwrap());
    fs::rename(&root, own.path().join("original")).unwrap();
    fs::create_dir(&root).unwrap();
    fs::set_permissions(&root, fs::Permissions::from_mode(0o700)).unwrap();
    fs::write(&file, &bytes).unwrap();
    fs::set_permissions(&file, fs::Permissions::from_mode(0o600)).unwrap();
    assert!(held.recheck().is_err());
    drop(held);
    let held = ServiceIdentity::open_existing(parent(own.path()), id).unwrap();
    fs::rename(&file, own.path().join("original-key")).unwrap();
    fs::write(&file, &bytes).unwrap();
    fs::set_permissions(&file, fs::Permissions::from_mode(0o600)).unwrap();
    assert!(held.recheck().is_err());
}

#[test]
fn actual_held_identity_namespace_survives_existing_only_restart_and_excludes_ephemeral() {
    assert!(crate::ServiceNode::new().unwrap().local_namespace().unwrap().is_none());
    let own=fixture();let id=Uuid::new_v4();
    let original=ServiceIdentity::create(parent(own.path()),id).unwrap();
    let namespace=original.local_namespace().unwrap();drop(original);
    let restarted=ServiceIdentity::open_existing(parent(own.path()),id).unwrap();
    assert_eq!(namespace,restarted.local_namespace().unwrap());
    let node=crate::ServiceNode::with_identity(restarted).unwrap();
    assert_eq!(node.local_namespace().unwrap(),Some(namespace));
}
