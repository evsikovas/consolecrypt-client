//! Linux-only descriptor-relative namespace restrictions. Safe pinned rustix
//! APIs only; no mount operation, fallback flags, ambient child paths or ACL edit.
use cap_std::fs::Dir;
use rustix::fs::{Mode, OFlags, ResolveFlags};
use std::{
    fs::File,
    io,
    path::{Component, Path},
};

fn unsupported() -> io::Error {
    io::Error::new(
        io::ErrorKind::Unsupported,
        "unsupported publication filesystem or path resolution",
    )
}

fn resolution_error(error: rustix::io::Errno) -> io::Error {
    // Unsupported strong semantics must never fall back to weaker opens.
    if matches!(
        error,
        rustix::io::Errno::NOSYS | rustix::io::Errno::INVAL | rustix::io::Errno::PERM
    ) {
        unsupported()
    } else {
        error.into()
    }
}

/// Mount transitions are allowed ONLY while locating the originally selected
/// profile, so a legitimate separately mounted home is supported. This still
/// rejects every symbolic/magic link and path escape.
pub(super) fn open_ancestor(parent: &Dir, name: &Path) -> io::Result<Dir> {
    open_directory(parent, name, false)
}
/// Inside the captured profile's managed namespace, NO_XDEV explicitly rejects
/// bind mounts too; equal st_dev is not a substitute for this kernel check.
pub(super) fn open_child(parent: &Dir, name: &Path) -> io::Result<Dir> {
    open_directory(parent, name, true)
}
fn open_directory(parent: &Dir, name: &Path, no_mounts: bool) -> io::Result<Dir> {
    let mut parts = name.components();
    if !matches!(parts.next(), Some(Component::Normal(_))) || parts.next().is_some() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "one relative directory name required",
        ));
    }
    let mut resolve =
        ResolveFlags::BENEATH | ResolveFlags::NO_SYMLINKS | ResolveFlags::NO_MAGICLINKS;
    if no_mounts {
        resolve |= ResolveFlags::NO_XDEV;
    }
    let fd = rustix::fs::openat2(
        parent,
        name,
        OFlags::RDONLY | OFlags::DIRECTORY | OFlags::CLOEXEC | OFlags::NOFOLLOW,
        Mode::empty(),
        resolve,
    )
    .map_err(resolution_error)?;
    Ok(Dir::from_std_file(File::from(fd)))
}

/// Deliberately narrow local subset, queried from the HELD descriptor. The
/// EXT magic identifies the ext2/3/4 family; required no-replace/fsync operations
/// still fail closed if a particular filesystem/kernel cannot honor them.
/// TMPFS is useful for ephemeral owned tests; it is not durable storage proof.
/// Overlay/NFS/CIFS/FUSE/9p/unknown do not prove local backing and remain denied.
pub(super) fn supported(dir: &Dir) -> io::Result<()> {
    let stat = rustix::fs::fstatfs(dir)?;
    let kind = stat.f_type as i64;
    if kind == libc::EXT4_SUPER_MAGIC as i64 || kind == libc::TMPFS_MAGIC as i64 {
        Ok(())
    } else {
        Err(unsupported())
    }
}

fn resolve() -> ResolveFlags {
    ResolveFlags::BENEATH
        | ResolveFlags::NO_SYMLINKS
        | ResolveFlags::NO_MAGICLINKS
        | ResolveFlags::NO_XDEV
}
/// Actual bounded read, not an earlier check followed by an unconstrained open.
pub(super) fn read_file(parent: &Dir, relative: &Path) -> io::Result<cap_std::fs::File> {
    let fd = rustix::fs::openat2(
        parent,
        relative,
        OFlags::RDONLY | OFlags::CLOEXEC | OFlags::NOFOLLOW | OFlags::NONBLOCK,
        Mode::empty(),
        resolve(),
    )
    .map_err(resolution_error)?;
    Ok(cap_std::fs::File::from_std(File::from(fd)))
}
/// New regular-file creation under the retained directory; never adopt a raced
/// file/mount or retry create without the resolve restrictions.
pub(super) fn create_file(
    parent: &Dir,
    relative: &Path,
    mode: u32,
) -> io::Result<cap_std::fs::File> {
    if mode & !0o777 != 0 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "invalid file mode",
        ));
    }
    let fd = rustix::fs::openat2(
        parent,
        relative,
        OFlags::WRONLY | OFlags::CREATE | OFlags::EXCL | OFlags::CLOEXEC | OFlags::NOFOLLOW,
        Mode::from_bits_truncate(mode),
        resolve(),
    )
    .map_err(resolution_error)?;
    Ok(cap_std::fs::File::from_std(File::from(fd)))
}
/// O_PATH observes only the constrained object, so even classification does not
/// first stat through a nested mount. Without O_NOFOLLOW a final symlink denies.
pub(super) fn metadata(parent: &Dir, relative: &Path) -> io::Result<cap_std::fs::Metadata> {
    let fd = rustix::fs::openat2(
        parent,
        relative,
        OFlags::PATH | OFlags::CLOEXEC,
        Mode::empty(),
        resolve(),
    )
    .map_err(resolution_error)?;
    cap_std::fs::File::from_std(File::from(fd)).metadata()
}

#[cfg(test)]
#[path = "linux_locality_tests.rs"]
mod tests;
