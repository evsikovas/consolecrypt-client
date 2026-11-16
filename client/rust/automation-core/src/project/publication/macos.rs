//! Darwin publication subset: descriptor-relative, no-symlink local APFS only.
//! No ACL mutation, ambient destination, rename fallback or new authority.
use cap_std::fs::{Dir, File as CapFile};
use rustix::fs::{Mode, OFlags};
use std::{
    ffi::c_void,
    fs::File,
    io,
    os::{
        fd::{AsFd, AsRawFd},
        unix::ffi::OsStrExt,
    },
    path::{Component, Path},
    ptr::NonNull,
};

fn unsupported() -> io::Error {
    io::Error::new(
        io::ErrorKind::Unsupported,
        "unsupported Darwin publication filesystem, ACL or operation",
    )
}
fn resolution_error(error: rustix::io::Errno) -> io::Error {
    if matches!(
        error,
        rustix::io::Errno::NOSYS | rustix::io::Errno::INVAL | rustix::io::Errno::NOTSUP
    ) {
        unsupported()
    } else {
        error.into()
    }
}

struct Mount {
    fsid: libc::fsid_t,
    device: u64,
    subtype: u32,
    mounted_on: Vec<u8>,
    mounted_from: Vec<u8>,
}
// Apple's public fsid_t is exactly two initialized i32 values. Fail the build
// rather than silently changing the opaque representation comparison.
const _: [(); 8] = [(); std::mem::size_of::<libc::fsid_t>()];
fn c_field(value: &[libc::c_char]) -> io::Result<Vec<u8>> {
    let end = value.iter().position(|v| *v == 0).ok_or_else(unsupported)?;
    if end == 0 {
        return Err(unsupported());
    }
    Ok(value[..end].iter().map(|v| *v as u8).collect())
}
fn mount(fd: impl AsFd) -> io::Result<Mount> {
    let stat = rustix::fs::fstatfs(fd.as_fd())?;
    // Do not bless network/FUSE/nullfs/unknown filesystems as local APFS.
    if c_field(&stat.f_fstypename)? != b"apfs"
        || stat.f_flags & libc::MNT_LOCAL as u32 == 0
        || stat.f_flags & (libc::MNT_RDONLY | libc::MNT_SNAPSHOT) as u32 != 0
    {
        return Err(unsupported());
    }
    let metadata = rustix::fs::fstat(fd)?;
    // Managed publication does not materialize dataless/FileProvider content
    // or interpret unknown system flags. Observe only this narrow local subset;
    // never clear flags to make a source pass.
    if metadata.st_ino == 0 || metadata.st_flags & !(libc::UF_NODUMP | libc::UF_HIDDEN) != 0 {
        return Err(unsupported());
    }
    Ok(Mount {
        fsid: stat.f_fsid,
        device: metadata.st_dev as u64,
        subtype: stat.f_fssubtype,
        mounted_on: c_field(&stat.f_mntonname)?,
        mounted_from: c_field(&stat.f_mntfromname)?,
    })
}
fn same_mount(a: &Mount, b: &Mount) -> bool {
    // SAFETY: libc's Darwin fsid_t is the SDK's two initialized i32 values,
    // without padding. Compare its opaque representation, never invent a
    // filesystem ID from a path or truncate it to st_dev alone.
    let same_fsid = unsafe {
        libc::memcmp(
            std::ptr::addr_of!(a.fsid).cast(),
            std::ptr::addr_of!(b.fsid).cast(),
            std::mem::size_of::<libc::fsid_t>(),
        ) == 0
    };
    same_fsid
        && a.device == b.device
        && a.subtype == b.subtype
        && a.mounted_on == b.mounted_on
        && a.mounted_from == b.mounted_from
}
fn check_mount(parent: &Dir, expected: &Mount, opened: impl AsFd) -> io::Result<()> {
    if !same_mount(expected, &mount(opened)?) || !same_mount(expected, &mount(parent)?) {
        return Err(unsupported());
    }
    Ok(())
}
fn physical_path(fd: impl AsFd) -> io::Result<Vec<u8>> {
    let mut buffer = [0u8; libc::MAXPATHLEN as usize];
    // SAFETY: F_GETPATH_NOFIRMLINK takes a writable MAXPATHLEN buffer. The fd
    // remains borrowed/live and no kernel-returned path is used to open data.
    if unsafe {
        libc::fcntl(
            fd.as_fd().as_raw_fd(),
            libc::F_GETPATH_NOFIRMLINK,
            buffer.as_mut_ptr(),
        )
    } < 0
    {
        return Err(unsupported());
    }
    let end = buffer
        .iter()
        .position(|b| *b == 0)
        .ok_or_else(unsupported)?;
    if end == 0 || buffer[0] != b'/' {
        return Err(unsupported());
    }
    Ok(buffer[..end].to_vec())
}
fn check_edge(parent: &Dir, name: &Path, opened: impl AsFd) -> io::Result<()> {
    // st_dev/fsid alone cannot prove the absence of same-volume firmlink
    // redirection. Require the actual opened physical child to be this ONE
    // physical parent/name edge, then repeat the parent observation. Paths are
    // comparison data only, never recaptured ambient capabilities.
    let before = physical_path(parent)?;
    let mut expected = before.clone();
    if expected.last() != Some(&b'/') {
        expected.push(b'/');
    }
    expected.extend_from_slice(name.as_os_str().as_bytes());
    if physical_path(opened)? != expected || physical_path(parent)? != before {
        return Err(unsupported());
    }
    Ok(())
}
pub(super) fn supported(dir: &Dir) -> io::Result<()> {
    mount(dir)?;
    Ok(())
}
fn leaf(name: &Path) -> io::Result<()> {
    let mut parts = name.components();
    if !matches!(parts.next(), Some(Component::Normal(_))) || parts.next().is_some() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "one relative leaf required",
        ));
    }
    Ok(())
}
fn flags() -> OFlags {
    // Darwin rejects NOFOLLOW combined with NOFOLLOW_ANY as EINVAL. The latter
    // already rejects symlinks in every component, including the final leaf.
    OFlags::CLOEXEC | OFlags::NOFOLLOW_ANY
}

/// Profile ancestors may cross volumes. Managed descendants may not.
pub(super) fn open_ancestor(parent: &Dir, name: &Path) -> io::Result<Dir> {
    leaf(name)?;
    let fd = rustix::fs::openat(
        parent,
        name,
        OFlags::RDONLY | OFlags::DIRECTORY | flags(),
        Mode::empty(),
    )
    .map_err(resolution_error)?;
    Ok(Dir::from_std_file(File::from(fd)))
}
pub(super) fn open_child(parent: &Dir, name: &Path) -> io::Result<Dir> {
    let original_mount = mount(parent)?;
    let child = open_ancestor(parent, name)?;
    check_mount(parent, &original_mount, &child)?;
    check_edge(parent, name, &child)?;
    verify_empty_acl(&child)?;
    Ok(child)
}
fn parent_for(parent: &Dir, relative: &Path) -> io::Result<(Dir, std::ffi::OsString)> {
    if relative.as_os_str().as_encoded_bytes().len() > 4096 {
        return Err(unsupported());
    }
    let parts: Vec<_> = relative.components().take(65).collect();
    if parts.is_empty()
        || parts.len() > 64
        || parts.iter().any(|p| !matches!(p, Component::Normal(_)))
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "bounded relative normal path required",
        ));
    }
    let mut current = parent.try_clone()?;
    supported(&current)?;
    for component in &parts[..parts.len() - 1] {
        current = open_child(&current, Path::new(component.as_os_str()))?;
    }
    Ok((current, parts.last().unwrap().as_os_str().into()))
}
fn checked_file(parent: &Dir, name: &Path, extra: OFlags, mode: Mode) -> io::Result<CapFile> {
    leaf(name)?;
    let original_mount = mount(parent)?;
    let fd = rustix::fs::openat(parent, name, extra | flags(), mode).map_err(resolution_error)?;
    check_mount(parent, &original_mount, &fd)?;
    check_edge(parent, name, &fd)?;
    Ok(CapFile::from_std(File::from(fd)))
}
pub(super) fn read_file(parent: &Dir, relative: &Path) -> io::Result<CapFile> {
    let (parent, name) = parent_for(parent, relative)?;
    let file = checked_file(
        &parent,
        Path::new(&name),
        OFlags::RDONLY | OFlags::NONBLOCK,
        Mode::empty(),
    )?;
    let stat = rustix::fs::fstat(&file)?;
    if stat.st_mode & libc::S_IFMT != libc::S_IFREG || stat.st_nlink != 1 {
        return Err(unsupported());
    }
    Ok(file)
}
pub(super) fn create_file(parent: &Dir, relative: &Path, mode: u32) -> io::Result<CapFile> {
    if ![0o600, 0o700].contains(&mode) {
        return Err(unsupported());
    }
    let (parent, name) = parent_for(parent, relative)?;
    // Empty inherited security BEFORE creating plaintext, never repair afterward.
    verify_empty_acl(&parent)?;
    let file = checked_file(
        &parent,
        Path::new(&name),
        OFlags::WRONLY | OFlags::CREATE | OFlags::EXCL,
        Mode::from_bits_truncate(mode as _),
    )?;
    verify_empty_acl(&file)?;
    let stat = rustix::fs::fstat(&file)?;
    if stat.st_mode & libc::S_IFMT != libc::S_IFREG
        || stat.st_nlink != 1
        || stat.st_uid != rustix::process::geteuid().as_raw()
        || stat.st_mode & 0o077 != 0
    {
        return Err(unsupported());
    }
    Ok(file)
}
pub(super) fn metadata(parent: &Dir, relative: &Path) -> io::Result<cap_std::fs::Metadata> {
    let (parent, name) = parent_for(parent, relative)?;
    // O_EVTONLY observes metadata without obtaining a data-read descriptor.
    // It is short-lived; the actual directory/data handles remain retained.
    let file = checked_file(
        &parent,
        Path::new(&name),
        OFlags::from_bits_retain(libc::O_EVTONLY as _) | OFlags::NONBLOCK,
        Mode::empty(),
    )?;
    file.metadata()
}
pub(super) fn full_sync_file(file: &CapFile) -> io::Result<()> {
    mount(file)?;
    rustix::fs::fsync(file)?;
    // No fallback to ordinary fsync when the required device flush is unavailable.
    rustix::fs::fcntl_fullfsync(file).map_err(resolution_error)
}

// These declarations use Apple's public sys/acl.h, sys/fcntl.h and sys/stat.h.
// The opaque ACL allocation is
// owned by Acl and released with acl_free on every branch. No ACL struct layout
// or ACE evaluator is copied into Rust, and no production ACL setter exists.
unsafe extern "C" {
    #[cfg(test)]
    fn acl_get_fd_np(fd: libc::c_int, kind: libc::c_int) -> *mut c_void;
    fn filesec_init() -> *mut c_void;
    fn filesec_free(security: *mut c_void);
    fn filesec_get_property(
        security: *mut c_void,
        property: libc::c_int,
        value: *mut c_void,
    ) -> libc::c_int;
    fn filesec_query_property(
        security: *mut c_void,
        property: libc::c_int,
        present: *mut libc::c_int,
    ) -> libc::c_int;
    #[cfg_attr(
        any(target_arch = "x86", target_arch = "x86_64"),
        link_name = "fstatx_np$INODE64"
    )]
    fn fstatx_np(fd: libc::c_int, stat: *mut libc::stat, security: *mut c_void) -> libc::c_int;
    fn acl_init(count: libc::c_int) -> *mut c_void;
    fn acl_valid(acl: *mut c_void) -> libc::c_int;
    fn acl_size(acl: *mut c_void) -> libc::ssize_t;
    fn acl_copy_ext(buffer: *mut c_void, acl: *mut c_void, size: libc::ssize_t) -> libc::ssize_t;
    fn acl_free(value: *mut c_void) -> libc::c_int;
}
struct Acl(NonNull<c_void>);
impl Acl {
    fn new(pointer: *mut c_void) -> io::Result<Self> {
        NonNull::new(pointer).map(Self).ok_or_else(unsupported)
    }
    fn export(&self) -> io::Result<Vec<u8>> {
        // SAFETY: self owns a live ACL returned by libc; outputs are bounded
        // before allocating/copying and the allocation stays live throughout.
        unsafe {
            if acl_valid(self.0.as_ptr()) != 0 {
                return Err(unsupported());
            }
            let size = acl_size(self.0.as_ptr());
            if !(1..=8192).contains(&size) {
                return Err(unsupported());
            }
            let mut bytes = vec![0u8; size as usize];
            if acl_copy_ext(bytes.as_mut_ptr().cast(), self.0.as_ptr(), size) != size {
                return Err(unsupported());
            }
            Ok(bytes)
        }
    }
}
impl Drop for Acl {
    fn drop(&mut self) {
        // SAFETY: unique ownership of a libc ACL allocation, never cloned.
        unsafe {
            acl_free(self.0.as_ptr());
        }
    }
}
// Public SDK filesec_property_t values from sys/fcntl.h. The filesec itself
// remains opaque and is released on every branch; no security setter is used.
const FILESEC_OWNER: libc::c_int = 1;
const FILESEC_GROUP: libc::c_int = 2;
const FILESEC_MODE: libc::c_int = 4;
const FILESEC_ACL: libc::c_int = 5;
struct FileSecurity(NonNull<c_void>);
impl Drop for FileSecurity {
    fn drop(&mut self) {
        // SAFETY: uniquely owned filesec_init allocation, never cloned.
        unsafe { filesec_free(self.0.as_ptr()) };
    }
}
pub(super) fn verify_empty_acl(fd: impl AsFd) -> io::Result<()> {
    // acl_get_fd_np collapses an absent ACL property and a failed stat into
    // NULL. Read the retained descriptor into a fresh SDK filesec instead;
    // never interpret an errno value alone as proof of empty security.
    // SAFETY: libc creates an opaque allocation owned by FileSecurity.
    let security = FileSecurity(NonNull::new(unsafe { filesec_init() }).ok_or_else(unsupported)?);
    let mut stat = std::mem::MaybeUninit::<libc::stat>::zeroed();
    let mut owner: libc::uid_t = 0;
    let mut group: libc::gid_t = 0;
    let mut mode: libc::mode_t = 0;
    // SAFETY: this live borrowed descriptor and all output pointers have their
    // public SDK types. The filesec stays live until both ACL observations end.
    // Require populated metadata too: an incomplete SDK stat must not look
    // like an intentionally absent ACL in a freshly initialized filesec.
    unsafe {
        if fstatx_np(
            fd.as_fd().as_raw_fd(),
            stat.as_mut_ptr(),
            security.0.as_ptr(),
        ) != 0
            || filesec_get_property(
                security.0.as_ptr(),
                FILESEC_OWNER,
                std::ptr::addr_of_mut!(owner).cast(),
            ) != 0
            || filesec_get_property(
                security.0.as_ptr(),
                FILESEC_GROUP,
                std::ptr::addr_of_mut!(group).cast(),
            ) != 0
            || filesec_get_property(
                security.0.as_ptr(),
                FILESEC_MODE,
                std::ptr::addr_of_mut!(mode).cast(),
            ) != 0
        {
            return Err(unsupported());
        }
    }
    // SAFETY: fstatx_np succeeded and all three SDK metadata properties were
    // populated from its stat output, not left unset after a partial failure.
    let stat = unsafe { stat.assume_init() };
    if stat.st_ino == 0 || (owner, group, mode) != (stat.st_uid, stat.st_gid, stat.st_mode) {
        return Err(unsupported());
    }
    let mut present: libc::c_int = -1;
    // SAFETY: opaque SDK allocation remains live; present is a writable int.
    if unsafe { filesec_query_property(security.0.as_ptr(), FILESEC_ACL, &mut present) } != 0
        || present < 0
    {
        return Err(unsupported());
    }
    if present == 0 {
        // Explicit SDK absence after a complete successful descriptor stat.
        return Ok(());
    }
    let mut pointer = std::ptr::null_mut::<c_void>();
    // SAFETY: FILESEC_ACL returns a separately owned acl_t through this pointer.
    let result = unsafe {
        filesec_get_property(
            security.0.as_ptr(),
            FILESEC_ACL,
            std::ptr::addr_of_mut!(pointer).cast(),
        )
    };
    let actual = NonNull::new(pointer).map(Acl);
    if result != 0 {
        return Err(unsupported());
    }
    let actual = actual.ok_or_else(unsupported)?;
    // Present ACLs must still match SDK-created empty security byte-for-byte,
    // including flags. Malformed, nonempty or unknown security remains denied.
    let empty = Acl::new(unsafe { acl_init(0) })?;
    if actual.export()? != empty.export()? {
        return Err(unsupported());
    }
    Ok(())
}

#[cfg(test)]
#[path = "macos_tests.rs"]
mod tests;
