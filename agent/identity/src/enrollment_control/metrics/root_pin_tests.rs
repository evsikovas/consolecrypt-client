//! Actual local-FD custody components, not a root-ownership/Actor admission.
use super::*;
use std::io::{Seek, SeekFrom, Write};

struct Directory(PathBuf);
impl Directory {
    fn new() -> Self {
        let mut nonce=[0u8;16];getrandom::fill(&mut nonce).unwrap();
        let path=std::env::temp_dir().join(format!("cc-root-pin-{}",hex::encode(nonce)));
        fs::DirBuilder::new().mode(0o700).create(&path).unwrap();Self(path)
    }
    fn file(&self) -> PathBuf {
        let path=self.0.join("owned");
        let mut f=OpenOptions::new().write(true).create_new(true).mode(0o600).open(&path).unwrap();
        f.write_all(b"original").unwrap();f.sync_all().unwrap();path
    }
}
impl Drop for Directory { fn drop(&mut self) { fs::remove_dir_all(&self.0).unwrap(); } }
fn component_pin(path:&Path) -> RootPin {
    // Exercise the actual scanner/held-FD check on our own temporary file.
    // Production capture still requires its fixed allowlist + root ancestors.
    let before=fs::symlink_metadata(path).unwrap();
    RootPin::capture_file(path,&before,64,Some(digest(b"original"))).unwrap()
}
#[test]
fn fixed_root_pin_original_fd_readonly_cloexec_clone_and_no_hot_read() {
    let d=Directory::new();let path=d.file();let pin=component_pin(&path);
    let cloned=pin.clone();assert!(std::sync::Arc::ptr_eq(&pin.file,&cloned.file));
    assert_eq!(pin.file.as_raw_fd(),cloned.file.as_raw_fd());
    let flags=rustix::fs::fcntl_getfl(pin.file.as_ref()).unwrap();
    assert_eq!(flags&rustix::fs::OFlags::RWMODE,rustix::fs::OFlags::RDONLY);
    assert!(rustix::io::fcntl_getfd(pin.file.as_ref()).unwrap().contains(rustix::io::FdFlags::CLOEXEC));
    assert_eq!(pin.identity,root_identity(&pin.file.metadata().unwrap()));
    assert_eq!(pin.identity.3,pin.file.metadata().unwrap().gid());
    let mut original=pin.file.as_ref();original.seek(SeekFrom::Start(0)).unwrap();
    for _ in 0..128 { pin.check_identity().unwrap();cloned.check_identity().unwrap(); }
    assert_eq!(original.stream_position().unwrap(),0,"hot check must not read/rehash body");
    assert!(matches!(RootPin::capture(&path,digest(b"original")),Err(Error::ScopeDenied)));
    fs::set_permissions(&d.0,fs::Permissions::from_mode(0o722)).unwrap();
    assert!(RootPin::parents(&path).is_err(),"explicit writable ancestor must fail root policy");
    fs::set_permissions(&d.0,fs::Permissions::from_mode(0o700)).unwrap();
}
#[test]
fn fixed_root_pin_mutation_swap_hardlink_mode_and_symlink_are_not_adopted() {
    for action in 0..5 {
        let d=Directory::new();let path=d.file();let pin=component_pin(&path);
        match action {
            0=>{fs::write(&path,b"mutation").unwrap();let f=OpenOptions::new().write(true).open(&path).unwrap();
                f.set_modified(std::time::UNIX_EPOCH+Duration::from_secs(1)).unwrap();},
            1=>{fs::rename(&path,d.0.join("original-inode")).unwrap();fs::write(&path,b"original").unwrap();},
            2=>{fs::hard_link(&path,d.0.join("second-link")).unwrap();},
            3=>{fs::set_permissions(&path,fs::Permissions::from_mode(0o400)).unwrap();},
            _=>{fs::rename(&path,d.0.join("original-inode")).unwrap();std::os::unix::fs::symlink(d.0.join("original-inode"),&path).unwrap();}
        }
        assert!(pin.check_identity().is_err(),"changed original must be denied");
        assert!(pin.clone().check_identity().is_err(),"clone must not renew a changed pin");
    }
}
#[test]
fn fixed_root_pin_capture_hash_cap_and_post_read_change_are_checked() {
    let d=Directory::new();let path=d.file();let before=fs::symlink_metadata(&path).unwrap();
    assert!(matches!(RootPin::capture_file(&path,&before,3,Some(digest(b"original"))),Err(Error::Quota)));
    assert!(RootPin::capture_file(&path,&before,64,Some(digest(b"different"))).is_err());
    let file=OpenOptions::new().read(true).custom_flags(NOFOLLOW|NONBLOCK|CLOEXEC).open(&path).unwrap();
    // A byte change after the completed digest cannot borrow its old result.
    fs::write(&path,b"mutation").unwrap();
    OpenOptions::new().write(true).open(&path).unwrap().set_modified(std::time::UNIX_EPOCH+Duration::from_secs(1)).unwrap();
    assert!(RootPin::finish(&path,&before,file,8,digest(b"original"),Some(digest(b"original"))).is_err());
}
